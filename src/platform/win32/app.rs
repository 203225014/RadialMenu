//! Сборка всего вместе: главное окно, поток хуков, кадровый таймер, трей, настройки и тема.
//!
//! Главный поток спит в MsgWaitForMultipleObjectsEx: пока меню не на экране, процесс
//! не потребляет процессор (нет ни кадров, ни опросов звука).

use std::time::SystemTime;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, POINT, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Graphics::Gdi::{EnumDisplaySettingsW, DEVMODEW, ENUM_CURRENT_SETTINGS};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    CancelWaitableTimer, CreateMutexW, CreateWaitableTimerExW, SetWaitableTimer, INFINITE,
};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, KillTimer, MessageBoxW,
    MsgWaitForMultipleObjectsEx, PeekMessageW, MB_ICONERROR, MB_OK, MB_TOPMOST, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SetTimer,
    TranslateMessage, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_DESTROY, WM_LBUTTONDBLCLK, WM_QUIT, WM_RBUTTONUP, WM_SETTINGCHANGE, WM_TIMER,
    WNDCLASSW,
};

use super::input::hook::{self, WM_RM_DOWN, WM_RM_UP, WM_RM_WHEEL};
use super::input::send;
use super::slot::Slot;
use super::system::audio::AudioMeter;
use super::system::{autostart, desktops, files, media, programs, util};
use super::updater::{self, Mode, Notice, WM_UPDATE, WM_UPDATE_EXIT};
use crate::platform::diag;
use super::ui::layer::Layer;
use super::ui::tray::{self, Cmd, WM_TRAY};
use super::ui::{note, settings};
use crate::engine::Engine;
use crate::model::action::vk;
use crate::model::menu_yaml::MenuError;
use crate::model::{Action, Context, Flag, MenuSet, Settings, Step, Theme, ThemeMode, MAX_PROFILES};
use crate::platform::theme::Follower;
use crate::platform::update;
use crate::render::compose::Canvas;
use crate::render::raster;
use crate::ui::settings_form::Status;

const NIN_BALLOONUSERCLICK: u32 = 0x0405;

/// Радиус размытия «стекла» под кнопками, px (минимальный: контуры фона угадываются).
const BLUR_RADIUS: usize = 1;

const TIMER_ACTION: usize = 1;
const TIMER_STEPS: usize = 2;
/// Раз в несколько секунд смотреть, не правили ли config.txt / menu.yaml: профили вызываются
/// не основной кнопкой, и перечитывать файлы «по нажатию основной» им не помогло бы.
const TIMER_RELOAD: usize = 3;
const RELOAD_MS: u32 = 3000;
const CAPTURE_DELAY_MS: u32 = 120; // перед снимком экрана: дать системе перерисовать экран без меню
const ACTION_DELAY_MS: u32 = 30; // дать меню погаснуть и фокусу вернуться к окну под ним
const TIP: &str = "Радиальное меню";

struct App {
    /// Ядро на каждое меню файла: 0 — общее, дальше — для отдельных программ.
    engines: Vec<Engine>,
    /// Какое из них сейчас на экране.
    active: usize,
    layer: Layer,
    audio: AudioMeter,
    settings: Settings,
    menus: MenuSet,
    /// Ошибка в menu.yaml (показываем в уведомлении и в настройках).
    menu_error: Option<MenuError>,
    theme: Follower,
    /// Время изменения config.txt и menu.yaml на момент последнего чтения.
    stamps: (Option<SystemTime>, Option<SystemTime>),
    hwnd: isize,
    timer: isize,
    frame_ms: u32,
    pending: Option<Action>,
    /// Оставшиеся шаги цепочки `steps`: паузы между ними отсчитывает таймер, окно не блокируется.
    steps: std::collections::VecDeque<Step>,
    paused: bool,
    layer_moved: bool,
    /// Меню убрано с экрана до действия-снимка: пока оно не закроется совсем, не показывать его снова.
    capture_hide: bool,
    /// Есть ли рабочий стол слева / справа от текущего (узнаётся при открытии меню).
    desktops: (bool, bool),
    /// Размытый снимок экрана под меню, пока оно открыто.
    backdrop: Option<Vec<u8>>,
}

impl App {
    fn engine(&mut self) -> &mut Engine {
        &mut self.engines[self.active]
    }
}

thread_local! {
    static APP: Slot<App> = const { Slot::new() };
    static TASKBAR_CREATED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.with(f))
}

fn stamps() -> (Option<SystemTime>, Option<SystemTime>) {
    (
        files::mtime(&files::config_path()),
        files::mtime(&files::menu_path()),
    )
}

/// Ядра всех меню под текущие настройки; шрифт нужен только на время сборки (подписи).
fn build_engines(settings: &Settings, menus: &MenuSet, theme: Theme) -> Vec<Engine> {
    let font = raster::load_font();
    let screen = util::monitor_at(POINT::default());
    menus
        .menus()
        .map(|m| {
            Engine::new(
                settings.clone(),
                util::ui_scale(),
                (screen.2, screen.3),
                m.clone(),
                font.as_ref(),
                theme,
                &programs::icon,
            )
        })
        .collect()
}

/// Окно меню — под самое большое из меню.
fn window_size(engines: &[Engine]) -> i32 {
    engines.iter().map(Engine::window_size).max().unwrap_or(1)
}

/// Пересобрать ядра (и окно меню, если сменился его размер).
fn rebuild(app: &mut App) {
    app.engines = build_engines(&app.settings, &app.menus, app.theme.theme());
    app.active = 0;
    let size = window_size(&app.engines);
    if size != app.layer.size() {
        match Layer::new(size) {
            Ok(l) => {
                app.layer = l;
                app.layer_moved = true;
            }
            Err(e) => util::log_error(&format!("окно меню {size}px: {e}")),
        }
    }
}

fn tip(paused: bool) -> String {
    if paused {
        format!("{TIP} (пауза)")
    } else {
        TIP.to_string()
    }
}

fn warn_menu(hwnd: isize, paused: bool, e: &MenuError) {
    tray::notify(
        util::h(hwnd),
        &tip(paused),
        "Ошибка в menu.yaml",
        &format!("{e}\nПоказано меню по умолчанию."),
    );
}

/// Тема могла смениться (системная или выбранная): перекрасить всё, что открыто.
fn apply_theme(app: &mut App) {
    if let Some(t) = app.theme.refresh(app.settings.theme) {
        diag::log(&format!("тема меню: {t:?} (режим {})", app.settings.theme.key()));
        for e in &mut app.engines {
            e.set_theme(t);
        }
        note::apply_theme(t);
    }
}

/// Тему выбрали в окне настроек (ещё не сохранив) — сразу показать её в меню и заметке;
/// `None` — окно закрыли, вернуть ту, что сохранена.
fn preview_theme(mode: Option<ThemeMode>) {
    with_app(|app| {
        let mode = mode.unwrap_or(app.settings.theme);
        if let Some(t) = app.theme.refresh(mode) {
            diag::log(&format!("тема меню: {t:?} (режим {})", mode.key()));
            for e in &mut app.engines {
                e.set_theme(t);
            }
            note::apply_theme(t);
        }
    });
}

/// Новые настройки: кнопка, сочетание, тема, размеры. Пересобирает только то, что нужно.
fn apply_settings(app: &mut App, new: Settings) {
    let relayout = app.settings.affects_layout(&new);
    updater::set_enabled(new.check_updates);
    app.settings = new;
    sync_triggers(app);
    apply_theme(app);
    if relayout {
        rebuild(app);
    }
}

/// Передать хуку, чем вызывается основное меню и каждый профиль. Профиль, который просит ту же
/// кнопку или то же сочетание, что основное, не включается: основное в настройках главнее.
fn sync_triggers(app: &App) {
    hook::set_trigger(0, app.settings.button, app.settings.hotkey);
    for n in 0..MAX_PROFILES {
        let (mut button, mut hotkey) = (0, None);
        if let Some(p) = app.menus.profiles.get(n) {
            (button, hotkey) = (p.trigger.button, p.trigger.hotkey);
            if button != 0 && button == app.settings.button {
                diag::log(&format!("профиль «{}»: кнопка занята основным меню, кнопка профиля не работает", p.name));
                button = 0;
            }
            if hotkey.is_some() && hotkey == app.settings.hotkey {
                diag::log(&format!("профиль «{}»: сочетание занято основным меню, сочетание профиля не работает", p.name));
                hotkey = None;
            }
        }
        hook::set_trigger(n + 1, button, hotkey);
    }
}

/// Файлы поправили руками — перечитать (зовётся перед показом меню, это один stat на файл).
fn reload_if_changed(app: &mut App) {
    let now = stamps();
    if now == app.stamps {
        return;
    }
    let (config_changed, menu_changed) = (now.0 != app.stamps.0, now.1 != app.stamps.1);
    app.stamps = now;
    if config_changed {
        apply_settings(app, files::load_settings());
    }
    if menu_changed {
        let (menus, err) = files::load_menu();
        if let Some(e) = &err {
            warn_menu(app.hwnd, app.paused, e);
        }
        app.menu_error = err;
        if menus != app.menus {
            app.menus = menus;
            rebuild(app);
            sync_triggers(app);
        }
    }
}

/// «Сохранить» в окне настроек.
fn save_settings(new: Settings, autostart_on: bool) -> Result<(), String> {
    files::save_settings(&new)?;
    if autostart_on != autostart::enabled() {
        autostart::set(autostart_on);
    }
    with_app(|app| {
        apply_settings(app, new);
        app.stamps.0 = files::mtime(&files::config_path()); // свою запись не перечитываем
    });
    Ok(())
}

/// Папку хранилища выбрали прямо в окне заметки — запомнить её в настройках.
fn set_vault(path: &str) {
    with_app(|app| {
        let mut s = app.settings.clone();
        s.vault = path.to_string();
        match files::save_settings(&s) {
            Ok(()) => {
                app.settings = s;
                app.stamps.0 = files::mtime(&files::config_path()); // свою запись не перечитываем
            }
            Err(e) => util::log_error(&format!("сохранение папки хранилища: {e}")),
        }
    });
}

fn open_settings() {
    let Some((s, status)) = with_app(|app| {
        let status = app.menu_error.as_ref().map(|e| Status {
            text: format!("menu.yaml, {e}"),
            bad: true,
        });
        (app.settings.clone(), status)
    }) else {
        return;
    };
    settings::open(&s, autostart::enabled(), status, save_settings, preview_theme);
}

/// Период кадра по частоте обновления экрана (60 Гц → 16 мс, 144 Гц → 7 мс).
fn frame_period_ms() -> u32 {
    unsafe {
        let mut dm = DEVMODEW {
            dmSize: std::mem::size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        if EnumDisplaySettingsW(None, ENUM_CURRENT_SETTINGS, &mut dm).as_bool()
            && dm.dmDisplayFrequency > 1
        {
            return (1000 / dm.dmDisplayFrequency).clamp(6, 17);
        }
    }
    16
}

fn start_timer(app: &App) {
    unsafe {
        let due = -(app.frame_ms as i64) * 10_000; // отрицательное — относительное время, 100 нс
        let _ = SetWaitableTimer(
            HANDLE(app.timer as *mut _),
            &due,
            app.frame_ms as i32,
            None,
            None,
            false,
        );
    }
}

fn stop_timer(app: &App) {
    unsafe {
        let _ = CancelWaitableTimer(HANDLE(app.timer as *mut _));
    }
}

fn schedule_action(app: &mut App, action: Option<Action>) {
    if let Some(a) = action {
        let shot = a.captures_screen();
        if shot {
            app.layer.hide(); // меню не должно попасть на снимок
            app.capture_hide = true;
        }
        app.pending = Some(a);
        unsafe {
            SetTimer(Some(util::h(app.hwnd)), TIMER_ACTION, if shot { CAPTURE_DELAY_MS } else { ACTION_DELAY_MS }, None);
        }
    }
}

/// Один кадр. Зовётся по кадровому таймеру, пока меню на экране.
fn tick() {
    let now = util::now();
    with_app(|app| {
        if app.engine().timed_out(now) {
            // событие «отпустил» могло потеряться — закрываем сами
            hook::cancel();
            let a = app.engine().release(now);
            schedule_action(app, a);
        }
        let (dx, dy) = hook::take_motion();
        // опрос звука — только если в этом меню есть кнопки, которым он нужен
        let playing = app.engines[app.active].menu().uses(Flag::Media) && app.audio.playing(now, false);
        let ctx = Context::default()
            .with(Flag::Media, playing)
            .with(Flag::DesktopPrev, app.desktops.0)
            .with(Flag::DesktopNext, app.desktops.1);
        let track = if app.engines[app.active].menu().uses(Flag::Media) { media::current() } else { None };
        app.engines[app.active].set_track(track.as_ref());
        let n = app.layer.size() as usize;
        let frame = {
            let engine = &mut app.engines[app.active];
            let mut cv = Canvas::new(n, n, app.layer.pixels()).with_backdrop(app.backdrop.as_deref());
            engine.frame(now, (dx as f32, dy as f32), &ctx, &mut cv)
        };
        if frame.visible && app.capture_hide {
            app.layer.hide(); // дорисовывается угасание, но на экран оно не выходит
        } else if frame.visible {
            app.layer.present(
                frame.dirty,
                frame.alpha,
                std::mem::take(&mut app.layer_moved),
            );
            app.layer.show();
        } else {
            app.capture_hide = false;
            app.layer.hide();
            app.backdrop = None;
            stop_timer(app);
            app.audio.release();
            media::set_wanted(false);
        }
    });
}

fn on_down(profile: usize) {
    let now = util::now();
    // какая программа активна — до показа меню (наше окно фокус не берёт)
    let exe = programs::foreground_exe();
    let started = with_app(|app| {
        if app.engines[app.active].is_active() {
            return false;
        }
        reload_if_changed(app);
        // профиль — своё меню; основное — по активной программе
        let slot = profile.checked_sub(1).and_then(|n| app.menus.profile_slot(n));
        app.active = slot.unwrap_or_else(|| app.menus.pick(exe.as_deref())).min(app.engines.len() - 1);
        let (fx, fy) = hook::frozen();
        let (mx, my, mw, mh) = util::monitor_at(POINT { x: fx, y: fy });
        app.engine().set_screen((mw, mh));
        let size = app.layer.size();
        app.layer.move_to(mx + (mw - size) / 2, my + (mh - size) / 2);
        app.layer_moved = true;
        app.layer.clear();
        app.backdrop = app.layer.backdrop(BLUR_RADIUS); // до показа окна: иначе снимется само меню
        hook::discard_motion(); // сдвиги до показа не в счёт
        app.engine().press(now);
        let menu = app.engines[app.active].menu();
        if menu.uses(Flag::DesktopPrev) || menu.uses(Flag::DesktopNext) {
            app.desktops = desktops::neighbours();
        }
        if app.engines[app.active].menu().uses(Flag::Media) {
            app.audio.playing(now, true); // сразу узнать, играет ли звук
            media::set_wanted(true); // и что именно играет
        }
        start_timer(app);
        true
    })
    .unwrap_or(false);
    if started {
        tick(); // первый кадр (прозрачный) готов до показа окна
    }
}

fn on_up() {
    let now = util::now();
    with_app(|app| {
        let a = app.engine().release(now);
        schedule_action(app, a);
    });
}

fn on_wheel(up: bool) {
    let now = util::now();
    if let Some(a) = with_app(|app| app.engine().wheel(up, now)).flatten() {
        execute(a);
    }
}

/// Исполнить действие. Зовётся вне заимствования состояния: окна заводят свои сообщения.
fn execute(action: Action) {
    match action {
        Action::Keys(keys) => send::chord(&keys),
        Action::Text(t) => send::text(&t),
        Action::Run(r) => send::run(&r),
        Action::Open(target) => send::open(&target),
        Action::Media(m) => send::tap(m.vk()),
        Action::Volume(up) => send::volume(up),
        Action::Explorer => send::explorer(),
        Action::Navigate(forward) => {
            let key = if forward { vk::RIGHT } else { vk::LEFT };
            super::system::pointer::navigate(hook::frozen(), forward, || send::chord(&[vk::MENU, key]));
        }
        Action::ScreenshotWindow => {
            if let Err(e) = super::system::screenshot::window_to_clipboard(hook::frozen()) {
                util::log_error(&format!("скриншот окна: {e}"));
            }
        }
        Action::Note => {
            if let Some((vault, theme)) = with_app(|app| (app.settings.vault.clone(), app.theme.theme())) {
                note::open(&vault, theme, set_vault);
            }
        }
        Action::Settings => open_settings(),
        Action::Steps(steps) => {
            // новая цепочка заменяет недоигранную прежнюю
            if with_app(|app| app.steps = steps.into()).is_some() {
                next_step();
            }
        }
    }
}

/// Выполнить очередной шаг цепочки и завести таймер на паузу после него.
fn next_step() {
    let Some((step, hwnd)) = with_app(|app| (app.steps.pop_front(), app.hwnd)) else {
        return;
    };
    let Some(step) = step else {
        return;
    };
    if let Some(a) = step.action {
        execute(a);
    }
    // execute мог запустить вложенную цепочку: она сама ведёт таймер
    let more = with_app(|app| !app.steps.is_empty()).unwrap_or(false);
    unsafe {
        if more {
            SetTimer(Some(util::h(hwnd)), TIMER_STEPS, step.wait_ms.max(1), None);
        } else {
            let _ = KillTimer(Some(util::h(hwnd)), TIMER_STEPS);
        }
    }
}

fn run_pending(hwnd: HWND) {
    unsafe {
        let _ = KillTimer(Some(hwnd), TIMER_ACTION);
    }
    if let Some(a) = with_app(|app| app.pending.take()).flatten() {
        execute(a);
    }
}

fn toggle_pause(hwnd: HWND) {
    let paused = with_app(|app| {
        app.paused = !app.paused;
        hook::set_enabled(!app.paused);
        if app.paused {
            app.engine().release(util::now()); // отмена без действия
        }
        app.paused
    })
    .unwrap_or(false);
    tray::set_tip(hwnd, &tip(paused));
}

/// `WM_SETTINGCHANGE` с «ImmersiveColorSet» — пользователь сменил тему Windows.
unsafe fn is_color_change(lparam: LPARAM) -> bool {
    if lparam.0 == 0 {
        return false;
    }
    PCWSTR(lparam.0 as *const u16)
        .to_string()
        .is_ok_and(|s| s == "ImmersiveColorSet")
}

unsafe extern "system" fn main_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_RM_DOWN => on_down(wparam.0),
        WM_RM_UP => on_up(),
        WM_RM_WHEEL => on_wheel(wparam.0 != 0),
        WM_TIMER if wparam.0 == TIMER_ACTION => run_pending(hwnd),
        WM_TIMER if wparam.0 == TIMER_STEPS => next_step(),
        WM_TIMER if wparam.0 == TIMER_RELOAD => {
            with_app(|app| {
                // пока меню открыто, ничего не пересобираем
                if !app.engines[app.active].is_active() {
                    reload_if_changed(app);
                }
            });
        }
        WM_SETTINGCHANGE if is_color_change(lparam) => {
            with_app(apply_theme);
            settings::refresh_theme();
            // панель задач сменила цвет — значку нужен другой рисунок
            let paused = with_app(|a| a.paused).unwrap_or(false);
            tray::set_tip(hwnd, &tip(paused));
        }
        WM_TRAY => match (lparam.0 & 0xFFFF) as u32 {
            WM_LBUTTONDBLCLK => open_settings(),
            // щёлкнули по уведомлению «вышла новая версия» — открыть настройки, где кнопка «Обновить»
            NIN_BALLOONUSERCLICK if updater::available().is_some() => open_settings(),
            WM_RBUTTONUP => {
                let (paused, on) = with_app(|a| (a.paused, a.settings.check_updates)).unwrap_or((false, true));
                match tray::menu(hwnd, paused, updater::available().as_deref(), on) {
                    Some(Cmd::Settings) => open_settings(),
                    Some(Cmd::TogglePause) => toggle_pause(hwnd),
                    Some(Cmd::ToggleAutostart) => autostart::set(!autostart::enabled()),
                    Some(Cmd::Update) => open_settings(),
                    Some(Cmd::ToggleUpdates) => toggle_updates(),
                    Some(Cmd::CheckNow) => updater::check(Mode::Tray),
                    Some(Cmd::OpenLog) => {
                        diag::log("журнал открыт из трея"); // чтобы файл точно существовал
                        send::open(&diag::log_path().to_string_lossy());
                    }
                    Some(Cmd::Exit) => {
                        tray::remove(hwnd);
                        PostQuitMessage(0);
                    }
                    None => {}
                }
            }
            _ => {}
        },
        WM_UPDATE => on_update_answer(hwnd),
        WM_UPDATE_EXIT => {
            // установщик запущен: освободить exe и значок
            tray::remove(hwnd);
            PostQuitMessage(0);
        }
        WM_DESTROY => PostQuitMessage(0),
        m if m != 0 && m == TASKBAR_CREATED.with(|t| t.get()) => {
            // Проводник перезапустился — вернуть значок
            let paused = with_app(|a| a.paused).unwrap_or(false);
            tray::add(hwnd, &tip(paused));
        }
        _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
    }
    LRESULT(0)
}

unsafe fn create_main_window() -> windows::core::Result<HWND> {
    let hinst = GetModuleHandleW(None)?;
    let class = w!("RadialMenuMain");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(main_proc),
        hInstance: hinst.into(),
        lpszClassName: class,
        ..Default::default()
    };
    RegisterClassW(&wc);
    // обычное (невидимое) окно верхнего уровня, а не message-only: ему приходят
    // широковещательные WM_SETTINGCHANGE о смене темы
    CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        w!("RadialMenu"),
        WINDOW_STYLE(0),
        0,
        0,
        0,
        0,
        None,
        None,
        Some(hinst.into()),
        None,
    )
}

fn setup() -> Result<HWND, String> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // COM нужен до сборки меню: иконки программ берутся через оболочку Windows
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let hwnd = create_main_window().map_err(|e| format!("главное окно: {e}"))?;
        TASKBAR_CREATED.with(|t| t.set(RegisterWindowMessageW(w!("TaskbarCreated"))));

        let settings = files::load_settings();
        let (menus, menu_error) = files::load_menu();
        let theme = Follower::new(settings.theme);
        let engines = build_engines(&settings, &menus, theme.theme());
        let layer = Layer::new(window_size(&engines)).map_err(|e| format!("окно меню: {e}"))?;
        // высокоточный таймер (Windows 10 1803+), иначе обычный
        let timer = CreateWaitableTimerExW(None, None, 0x2, 0x1F0003)
            .or_else(|_| CreateWaitableTimerExW(None, None, 0, 0x1F0003))
            .map_err(|e| format!("таймер: {e}"))?;
        let (button, hotkey) = (settings.button, settings.hotkey);
        APP.with(|a| {
            a.set(App {
                engines,
                active: 0,
                layer,
                audio: AudioMeter::new(),
                settings,
                menus,
                menu_error: menu_error.clone(),
                theme,
                stamps: stamps(),
                hwnd: hwnd.0 as isize,
                timer: timer.0 as isize,
                frame_ms: frame_period_ms(),
                pending: None,
                steps: Default::default(),
                paused: false,
                layer_moved: true,
                capture_hide: false,
                desktops: (true, true),
                backdrop: None,
            })
        });
        tray::add(hwnd, TIP);
        if let Some(e) = &menu_error {
            warn_menu(hwnd.0 as isize, false, e);
        }
        hook::spawn(button, hotkey, hwnd);
        with_app(|app| sync_triggers(app));
        SetTimer(Some(hwnd), TIMER_RELOAD, RELOAD_MS, None);
        updater::start(hwnd.0 as isize, with_app(|a| a.settings.check_updates).unwrap_or(true));
        if std::env::args().any(|a| a == "--updated") {
            // установщик перезапустил нас после обновления
            tray::inform(
                hwnd,
                TIP,
                "Приложение обновлено",
                &format!("Установлена версия {}.", update::current()),
            );
            diag::log(&format!("обновлено до {}", update::current()));
        }
        Ok(hwnd)
    }
}

// ---------------------------------------------------------------- обновления

fn on_update_answer(hwnd: HWND) {
    let paused = with_app(|a| a.paused).unwrap_or(false);
    let tip = tip(paused);
    match updater::take_answer() {
        Notice::None => {}
        Notice::Found(v) => tray::inform(
            hwnd,
            &tip,
            &format!("Доступна версия {v}"),
            "Щёлкните, чтобы открыть настройки и обновить программу.",
        ),
        Notice::UpToDate => {
            tray::inform(hwnd, &tip, "Обновлений нет", &format!("Установлена последняя версия {}.", update::current()))
        }
        Notice::Failed(e) => {
            tray::notify(hwnd, &tip, "Не удалось проверить обновления", &format!("{e}\nПроверьте подключение к интернету."))
        }
    }
}

fn toggle_updates() {
    let on = with_app(|a| {
        let mut s = a.settings.clone();
        s.check_updates = !s.check_updates;
        if files::save_settings(&s).is_ok() {
            a.settings = s;
            a.stamps.0 = files::mtime(&files::config_path()); // свою запись не перечитываем
        }
        a.settings.check_updates
    });
    if let Some(on) = on {
        updater::set_enabled(on);
    }
}

/// Окно после аварии: без него exe без консоли просто исчез бы.
fn show_fatal(log_path: &str) {
    let text = util::wide(&format!(
        "Радиальное меню аварийно завершилось.

Подробности записаны в журнал:
{log_path}

Пришлите этот файл разработчику."
    ));
    let title = util::wide("Радиальное меню");
    unsafe {
        MessageBoxW(None, util::pcwstr(&text), util::pcwstr(&title), MB_OK | MB_ICONERROR | MB_TOPMOST);
    }
}

/// Точка входа Windows-версии.
pub fn run() {
    // установщик при удалении снимает автозапуск этим ключом и сразу выходит
    if std::env::args().any(|a| a == "--remove-autostart") {
        autostart::set(false);
        return;
    }
    diag::install_panic_hook(show_fatal);
    diag::log_start();
    autostart::refresh_path(); // программу перенесли или обновили — запись автозапуска должна вести к ней
    unsafe {
        // один экземпляр: два хука на одну кнопку мешают друг другу
        let _mutex = CreateMutexW(None, false, w!("Local\\RadialMenuSingleton"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        let hwnd = match setup() {
            Ok(h) => h,
            Err(e) => {
                util::log_error(&e);
                return;
            }
        };
        let timer = HANDLE(with_app(|a| a.timer).unwrap_or(0) as *mut _);
        let mut msg = MSG::default();
        loop {
            let r = MsgWaitForMultipleObjectsEx(
                Some(&[timer]),
                INFINITE,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            );
            if r == WAIT_OBJECT_0 {
                tick();
            }
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    tray::remove(hwnd);
                    return;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

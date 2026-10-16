//! Сборка всего вместе: главное окно, поток хука, кадровый таймер, значок в трее.
//!
//! Главный поток спит в MsgWaitForMultipleObjectsEx: пока меню не на экране, процесс
//! не потребляет процессор (нет ни кадров, ни опросов звука).

use std::cell::RefCell;

use windows::core::w;
use windows::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Graphics::Gdi::{EnumDisplaySettingsW, DEVMODEW, ENUM_CURRENT_SETTINGS};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    CancelWaitableTimer, CreateMutexW, CreateWaitableTimerExW, SetWaitableTimer, INFINITE,
};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, KillTimer, MsgWaitForMultipleObjectsEx,
    PeekMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SetCursorPos, SetTimer,
    TranslateMessage, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_DESTROY, WM_LBUTTONDBLCLK, WM_QUIT, WM_RBUTTONUP, WM_TIMER, WNDCLASSW,
};

use super::audio::AudioMeter;
use super::hook::{self, SHARED, WM_RM_DOWN, WM_RM_UP, WM_RM_WHEEL};
use super::layer::Layer;
use super::tray::{self, Cmd, WM_TRAY};
use super::{input, util};
use crate::action::Action;
use crate::compose::Canvas;
use crate::config::Settings;
use crate::engine::Engine;
use crate::menu::{default_menu, Context};
use crate::raster;

const TIMER_ACTION: usize = 1;
const ACTION_DELAY_MS: u32 = 30; // дать меню погаснуть и фокусу вернуться к окну под ним
const TIP: &str = "Радиальное меню";

struct App {
    engine: Engine,
    layer: Layer,
    audio: AudioMeter,
    settings: Settings,
    hwnd: isize,
    timer: isize,
    frame_ms: u32,
    pending: Option<Action>,
    paused: bool,
    layer_moved: bool,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static TASKBAR_CREATED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn config_path() -> Option<std::path::PathBuf> {
    std::env::var_os("APPDATA").map(|d| {
        std::path::PathBuf::from(d)
            .join("RadialMenu")
            .join("config.txt")
    })
}

fn load_settings() -> Settings {
    let mut s = match config_path().and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(text) => Settings::parse(&text),
        None => {
            let d = Settings::default();
            if let Some(p) = config_path() {
                // при первом запуске кладём файл с настройками по умолчанию — его удобно править
                if let Some(dir) = p.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(p, d.to_text());
            }
            d
        }
    };
    s.apply_args(std::env::args().skip(1)); // параметры командной строки — только на этот запуск
    s
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
        app.pending = Some(a);
        unsafe {
            SetTimer(
                Some(HWND(app.hwnd as *mut _)),
                TIMER_ACTION,
                ACTION_DELAY_MS,
                None,
            );
        }
    }
}

/// Один кадр. Зовётся по кадровому таймеру, пока меню на экране.
fn tick() {
    let now = util::now();
    with_app(|app| {
        if app.engine.timed_out(now) {
            // событие «отпустил» могло потеряться — закрываем сами
            SHARED
                .capture
                .store(false, std::sync::atomic::Ordering::Relaxed);
            let a = app.engine.release(now);
            schedule_action(app, a);
        }
        let (dx, dy) = SHARED.take();
        let ctx = Context {
            media_playing: app.audio.playing(now, false),
        };
        let local_coords = !SHARED.use_raw.load(std::sync::atomic::Ordering::Relaxed)
            && unsafe {
                windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
                    windows::Win32::UI::WindowsAndMessaging::SM_REMOTESESSION,
                )
            } == 0;
        if app.engine.is_active() && local_coords {
            // по координатам: возвращаем курсор на место нажатия
            let f = SHARED.frozen();
            unsafe {
                let _ = SetCursorPos(f.0, f.1);
            }
            SHARED.rebase(f);
        }
        let n = app.layer.size() as usize;
        let frame = {
            let mut cv = Canvas::new(n, n, app.layer.pixels());
            app.engine.frame(now, (dx as f32, dy as f32), &ctx, &mut cv)
        };
        if frame.visible {
            app.layer.present(
                frame.dirty,
                frame.alpha,
                std::mem::take(&mut app.layer_moved),
            );
            app.layer.show();
        } else {
            app.layer.hide();
            stop_timer(app);
            app.audio.release();
        }
    });
}

fn on_down() {
    let now = util::now();
    let started = with_app(|app| {
        if app.engine.is_active() {
            return false;
        }
        let (fx, fy) = SHARED.frozen();
        let (mx, my, mw, mh) = util::monitor_at(windows::Win32::Foundation::POINT { x: fx, y: fy });
        app.engine.set_screen((mw, mh));
        let size = app.layer.size();
        app.layer
            .move_to(mx + (mw - size) / 2, my + (mh - size) / 2);
        app.layer_moved = true;
        app.layer.clear();
        SHARED.take(); // сдвиги до показа не в счёт
        app.engine.press(now);
        app.audio.playing(now, true); // сразу узнать, играет ли звук
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
        let a = app.engine.release(now);
        schedule_action(app, a);
    });
}

fn on_wheel(up: bool) {
    let now = util::now();
    let (action, vault) = with_app(|app| (app.engine.wheel(up, now), app.settings.vault.clone()))
        .unwrap_or((None, String::new()));
    if let Some(a) = action {
        input::exec(a, &vault);
    }
}

fn run_pending(hwnd: HWND) {
    unsafe {
        let _ = KillTimer(Some(hwnd), TIMER_ACTION);
    }
    // действие исполняем уже без заимствования: окно заметки заводит свой цикл сообщений
    if let Some((a, vault)) =
        with_app(|app| app.pending.take().map(|a| (a, app.settings.vault.clone()))).flatten()
    {
        input::exec(a, &vault);
    }
}

fn toggle_pause(hwnd: HWND) {
    use std::sync::atomic::Ordering::Relaxed;
    let paused = with_app(|app| {
        app.paused = !app.paused;
        SHARED.enabled.store(!app.paused, Relaxed);
        if app.paused {
            SHARED.capture.store(false, Relaxed);
            app.engine.release(util::now()); // отмена без действия
        }
        app.paused
    })
    .unwrap_or(false);
    tray::set_tip(
        hwnd,
        &if paused {
            format!("{TIP} (пауза)")
        } else {
            TIP.to_string()
        },
    );
}

unsafe extern "system" fn main_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_RM_DOWN => on_down(),
        WM_RM_UP => on_up(),
        WM_RM_WHEEL => on_wheel(wparam.0 != 0),
        WM_TIMER if wparam.0 == TIMER_ACTION => run_pending(hwnd),
        WM_TRAY => {
            let ev = (lparam.0 & 0xFFFF) as u32;
            if ev == WM_RBUTTONUP || ev == WM_LBUTTONDBLCLK {
                let paused = with_app(|a| a.paused).unwrap_or(false);
                match tray::menu(hwnd, paused) {
                    Some(Cmd::TogglePause) => toggle_pause(hwnd),
                    Some(Cmd::ToggleAutostart) => tray::set_autostart(!tray::autostart()),
                    Some(Cmd::Exit) => {
                        tray::remove(hwnd);
                        PostQuitMessage(0);
                    }
                    None => {}
                }
            }
        }
        WM_DESTROY => PostQuitMessage(0),
        m if m != 0 && m == TASKBAR_CREATED.with(|t| t.get()) => {
            // Проводник перезапустился — вернуть значок
            let paused = with_app(|a| a.paused).unwrap_or(false);
            tray::add(
                hwnd,
                &if paused {
                    format!("{TIP} (пауза)")
                } else {
                    TIP.to_string()
                },
            );
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
        let hwnd = create_main_window().map_err(|e| format!("главное окно: {e}"))?;
        TASKBAR_CREATED.with(|t| t.set(RegisterWindowMessageW(w!("TaskbarCreated"))));

        let settings = load_settings();
        let scale = util::ui_scale();
        let screen = util::monitor_at(Default::default());
        let font = raster::load_font(); // шрифт нужен только для предварительной отрисовки подписей
        let engine = Engine::new(
            settings.clone(),
            scale,
            (screen.2, screen.3),
            default_menu(),
            font.as_ref(),
        );
        drop(font);
        let layer = Layer::new(engine.window_size()).map_err(|e| format!("окно меню: {e}"))?;
        // высокоточный таймер (Windows 10 1803+), иначе обычный
        let timer = CreateWaitableTimerExW(None, None, 0x2, 0x1F0003)
            .or_else(|_| CreateWaitableTimerExW(None, None, 0, 0x1F0003))
            .map_err(|e| format!("таймер: {e}"))?;
        let button = settings.button;
        APP.with(|a| {
            *a.borrow_mut() = Some(App {
                engine,
                layer,
                audio: AudioMeter::new(),
                settings,
                hwnd: hwnd.0 as isize,
                timer: timer.0 as isize,
                frame_ms: frame_period_ms(),
                pending: None,
                paused: false,
                layer_moved: true,
            });
        });
        tray::add(hwnd, TIP);
        hook::spawn(button, hwnd);
        Ok(hwnd)
    }
}

/// Точка входа Windows-версии.
pub fn run() {
    std::panic::set_hook(Box::new(|info| util::log_error(&format!("паника: {info}"))));
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

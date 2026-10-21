//! Окно настроек. Внешний вид и поведение — в `ui::settings_form` (без ОС);
//! здесь окно без рамки, вывод готовой картинки, мышь/клавиши и родное поле ввода пути.

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, EndPaint, InvalidateRect,
    ScreenToClient, SetBkColor, SetDIBitsToDevice, SetTextColor, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DIB_RGB_COLORS, FF_DONTCARE, FW_NORMAL, HDC,
    HGDIOBJ, OUT_DEFAULT_PRECIS, PAINTSTRUCT, CLEARTYPE_QUALITY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_ESCAPE,
    VK_RETURN,
};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetWindowTextLengthW,
    GetWindowTextW, IsWindow, LoadCursorW, PostMessageW, RegisterClassW, SendMessageW,
    MoveWindow, SetTimer, SetWindowTextW, ShowWindow, ES_AUTOHSCROLL, HTCAPTION, IDC_ARROW, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WM_COMMAND, WM_CTLCOLOREDIT, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCLBUTTONDOWN,
    WM_PAINT, WM_SETFONT, WM_TIMER, WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_EX_APPWINDOW, WS_POPUP,
};

use super::focus::take_focus;
use crate::model::{Settings, Theme, ThemeMode};
use crate::platform::theme::system_prefers_dark;
use crate::platform::win32::input::hook::{self, MOD_ALT, MOD_CTRL, MOD_SHIFT, MOD_WIN, WM_RM_RECORD};
use crate::platform::win32::input::send;
use crate::platform::win32::slot::Slot;
use crate::platform::win32::system::util::{
    h, log_error, monitor_at, pcwstr, rgb, round_corners, ui_scale, wide,
};
use crate::platform::win32::system::{dialogs, files};
use crate::platform::win32::updater::{self, Mode};
use crate::render::painter::Painter;
use crate::render::palette;
use crate::model::NAME_MAX;
use crate::ui::settings_form::{Box2, Cmd, Fonts, Key, SettingsForm, Status};

/// Что делать по «Сохранить»: применить и записать (ошибка — показать в окне).
pub type OnSave = fn(Settings, bool) -> Result<(), String>;

/// Выбранную в окне тему сразу показать в меню (`Some`) или вернуть сохранённую (`None` — окно закрыто).
pub type OnPreview = fn(Option<ThemeMode>);

const WM_SET_KEY: u32 = WM_APP + 30;
const WM_MOUSELEAVE: u32 = 0x02A3;
const EN_CHANGE: u32 = 0x0300;
const EM_LIMITTEXT: u32 = 0x00C5;
const TIMER_UPDATE: usize = 7;

type Rect4 = (i32, i32, i32, i32);

#[derive(Default)]
struct Shown {
    vault: Option<Rect4>,
    name: Option<Rect4>,
    name_for: Option<usize>,
}

struct Win {
    hwnd: isize,
    edit: isize,
    /// Поле названия раскрытого меню (видно только на странице «Меню»).
    name_edit: isize,
    /// Где родные поля сейчас показаны (`None` — скрыты) и для какого меню лежит название.
    shown: Shown,
    form: SettingsForm,
    fonts: Fonts,
    on_save: OnSave,
    on_preview: OnPreview,
    /// Тема, которая сейчас показана в меню по выбору в окне.
    previewed: ThemeMode,
    /// Кисть фона поля ввода и тема, под которую она сделана.
    field_brush: (Theme, isize),
    edit_font: isize,
    tracking: bool,
}

thread_local! {
    static WIN: Slot<Win> = const { Slot::new() };
}

fn with<R>(f: impl FnOnce(&mut Win) -> R) -> Option<R> {
    WIN.with(|s| s.with(f))
}

/// Тема окна — по черновику: выбор в окне сразу видно.
fn theme_of(form: &SettingsForm) -> Theme {
    form.draft.theme.resolve(system_prefers_dark())
}

/// Открыть настройки (или вывести вперёд уже открытые). `status` — что показать внизу.
pub fn open(settings: &Settings, autostart: bool, status: Option<Status>, on_save: OnSave, on_preview: OnPreview) {
    if let Some((hw, ed)) = with(|w| (w.hwnd, w.edit)) {
        if unsafe { IsWindow(Some(h(hw))).as_bool() } {
            take_focus(h(hw), h(ed));
            return;
        }
    }
    let Some(fonts) = Fonts::load() else {
        log_error("настройки: не найдены шрифты Segoe UI/Arial");
        return;
    };
    let mut form = SettingsForm::new(settings.clone(), autostart, ui_scale());
    form.status = status;
    form.version = crate::platform::update::current().to_string();
    form.update = updater::view();
    unsafe {
        if let Err(e) = create(form, fonts, on_save, on_preview) {
            log_error(&format!("окно настроек: {e}"));
        }
    }
}

/// Если в окне выбрали другую тему — показать её и в самом меню.
fn preview_if_changed() {
    let call = with(|w| {
        let t = w.form.draft.theme;
        (t != w.previewed).then(|| {
            w.previewed = t;
            (w.on_preview, t)
        })
    })
    .flatten();
    if let Some((f, t)) = call {
        f(Some(t));
    }
}

/// Тема системы сменилась — перерисовать (если в окне выбрано «Авто»).
pub fn refresh_theme() {
    if let Some(hw) = with(|w| w.hwnd) {
        unsafe {
            let _ = InvalidateRect(Some(h(hw)), None, false);
            let _ = InvalidateRect(Some(h(with(|w| w.edit).unwrap_or(0))), None, true);
            let _ = InvalidateRect(Some(h(with(|w| w.name_edit).unwrap_or(0))), None, true);
        }
    }
}

unsafe fn create(form: SettingsForm, fonts: Fonts, on_save: OnSave, on_preview: OnPreview) -> windows::core::Result<()> {
    let hinst = GetModuleHandleW(None)?;
    let class = w!("RadialMenuSettings");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(win_proc),
        hInstance: hinst.into(),
        lpszClassName: class,
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        ..Default::default()
    };
    RegisterClassW(&wc);
    let (ww, wh) = form.size();
    let mut cur = POINT::default();
    let _ = GetCursorPos(&mut cur);
    let (mx, my, mw, mh) = monitor_at(cur);
    let hwnd = CreateWindowExW(
        WS_EX_APPWINDOW,
        class,
        w!("Радиальное меню — настройки"),
        WS_POPUP | WS_CLIPCHILDREN,
        mx + (mw - ww) / 2,
        my + (mh - wh) / 2,
        ww,
        wh,
        None,
        None,
        Some(hinst.into()),
        None,
    )?;
    round_corners(hwnd);

    let text = wide(&form.draft.vault);
    let edit = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        w!("EDIT"),
        pcwstr(&text),
        WS_CHILD | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
        0,
        0,
        10,
        10,
        Some(hwnd),
        None,
        Some(hinst.into()),
        None,
    )?;
    let height = -((13.0 * ui_scale()).round() as i32);
    let font = CreateFontW(
        height,
        0,
        0,
        0,
        FW_NORMAL.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        FF_DONTCARE.0 as u32,
        w!("Segoe UI"),
    );
    SendMessageW(edit, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
    let _ = SetWindowSubclass(edit, Some(edit_proc), 1, 0);

    // поле названия меню: то же оформление; положение и видимость задаёт sync_edits
    let name_edit = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        w!("EDIT"),
        w!(""),
        WS_CHILD | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
        0,
        0,
        10,
        10,
        Some(hwnd),
        None,
        Some(hinst.into()),
        None,
    )?;
    SendMessageW(name_edit, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
    SendMessageW(name_edit, EM_LIMITTEXT, Some(WPARAM(NAME_MAX)), Some(LPARAM(0)));
    let _ = SetWindowSubclass(name_edit, Some(edit_proc), 2, 0);

    let theme = theme_of(&form);
    let theme_mode = form.draft.theme;
    WIN.with(|s| {
        s.set(Win {
            hwnd: hwnd.0 as isize,
            edit: edit.0 as isize,
            name_edit: name_edit.0 as isize,
            shown: Shown::default(),
            form,
            fonts,
            on_save,
            on_preview,
            previewed: theme_mode,
            field_brush: (theme, CreateSolidBrush(rgb(palette::ui(theme).bg2)).0 as isize),
            edit_font: font.0 as isize,
            tracking: false,
        })
    });
    sync_edits();
    let _ = ShowWindow(hwnd, SW_SHOW);
    take_focus(hwnd, hwnd);
    // ход проверки и скачивания приходит из другого потока — сверяемся с ним по таймеру
    SetTimer(Some(hwnd), TIMER_UPDATE, 250, None);
    Ok(())
}

unsafe fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc: HDC = BeginPaint(hwnd, &mut ps);
    let frame = with(|w| {
        let (cw, ch) = w.form.size();
        let mut p = Painter::new(cw as u32, ch as u32)?;
        w.form.draw(&mut p, &w.fonts, theme_of(&w.form), false);
        let mut bgra = vec![0u8; (cw * ch * 4) as usize];
        p.to_bgra(&mut bgra);
        Some((cw, ch, bgra))
    })
    .flatten();
    if let Some((cw, ch, bgra)) = frame {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: cw,
                biHeight: -ch, // сверху вниз
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        SetDIBitsToDevice(
            hdc,
            0,
            0,
            cw as u32,
            ch as u32,
            0,
            0,
            0,
            ch as u32,
            bgra.as_ptr() as *const _,
            &bmi,
            DIB_RGB_COLORS,
        );
    }
    let _ = EndPaint(hwnd, &ps);
}

fn xy(lparam: LPARAM) -> (f32, f32) {
    let x = (lparam.0 & 0xFFFF) as i16 as f32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
    (x, y)
}

unsafe fn edit_text(edit: HWND) -> String {
    let len = GetWindowTextLengthW(edit).max(0) as usize;
    let mut buf = vec![0u16; len + 1];
    let n = GetWindowTextW(edit, &mut buf).max(0) as usize;
    String::from_utf16_lossy(&buf[..n])
}

/// Показать в окне актуальное состояние обновления (перерисовать, если оно изменилось).
fn sync_update(hwnd: HWND) {
    let view = updater::view();
    let changed = with(|w| w.form.update != view && {
        w.form.update = view;
        true
    });
    if changed == Some(true) {
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }
}

/// Родные поля ввода: поставить на место, показать/спрятать по странице, подставить название меню.
unsafe fn sync_edits() {
    let rect = |r: Box2| (r.x.round() as i32, r.y.round() as i32, r.w.round() as i32, r.h.round() as i32);
    let Some((vault, name, vr, nr, sel, name_text)) = with(|w| {
        (
            w.edit,
            w.name_edit,
            w.form.vault_edit_rect().map(rect),
            w.form.name_edit_rect().map(rect),
            w.form.selected,
            w.form.selected_name(),
        )
    }) else {
        return;
    };
    let (old_v, old_n, old_for) = with(|w| (w.shown.vault, w.shown.name, w.shown.name_for)).unwrap_or_default();
    let place = |edit: isize, new: Option<Rect4>, old: Option<Rect4>| {
        if new == old {
            return;
        }
        match new {
            Some((x, y, w, h)) => {
                let _ = MoveWindow(h_of(edit), x, y, w, h, true);
                if old.is_none() {
                    let _ = ShowWindow(h_of(edit), SW_SHOW);
                }
            }
            None => {
                let _ = ShowWindow(h_of(edit), SW_HIDE);
            }
        }
    };
    place(vault, vr, old_v);
    place(name, nr, old_n);
    if nr.is_some() && (old_n.is_none() || old_for != Some(sel)) {
        let t = wide(&name_text);
        let _ = SetWindowTextW(h_of(name), pcwstr(&t));
    }
    with(|w| {
        w.shown = Shown { vault: vr, name: nr, name_for: nr.map(|_| sel) };
    });
}

fn h_of(v: isize) -> HWND {
    h(v)
}

/// Исполнить команду формы и привести в порядок поле названия (вкладка могла смениться).
unsafe fn run(hwnd: HWND, cmd: Cmd) {
    run_cmd(hwnd, cmd);
    sync_edits();
}

/// Исполнить команду формы — уже вне `with`: тут бывают диалоги и синхронные сообщения.
unsafe fn run_cmd(hwnd: HWND, cmd: Cmd) {
    match cmd {
        Cmd::None => {}
        Cmd::Redraw => {
            let _ = InvalidateRect(Some(hwnd), None, false);
            // поле ввода тоже перекрасить: могли сменить тему
            if let Some(e) = with(|w| (w.edit, w.name_edit)) {
                let _ = InvalidateRect(Some(h(e.0)), None, true);
                let _ = InvalidateRect(Some(h(e.1)), None, true);
            }
        }
        Cmd::Close => {
            let _ = DestroyWindow(hwnd);
        }
        Cmd::Save => {
            let Some((draft, auto, on_save)) = with(|w| (w.form.draft.clone(), w.form.autostart, w.on_save)) else {
                return;
            };
            match on_save(draft, auto) {
                Ok(()) => {
                    let _ = DestroyWindow(hwnd);
                }
                Err(e) => {
                    with(|w| w.form.status = Some(Status { text: e, bad: true }));
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
        }
        Cmd::Browse => {
            let start = with(|w| w.form.draft.vault.clone()).unwrap_or_default();
            if let Some(path) = dialogs::pick_folder(hwnd, "Папка хранилища Obsidian", &start) {
                if let Some(e) = with(|w| w.edit) {
                    let t = wide(&path);
                    let _ = SetWindowTextW(h(e), pcwstr(&t)); // EN_CHANGE обновит черновик
                }
            }
        }
        Cmd::StartRecording => {
            // нажатия пойдут этому окну через хук — так записываются и Win+…, и Alt+…
            hook::set_recorder(Some(hwnd));
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        Cmd::StopRecording => {
            hook::set_recorder(None);
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        Cmd::OpenLog => {
            crate::platform::diag::log("журнал открыт из настроек"); // чтобы файл точно существовал
            send::open(&crate::platform::diag::log_path().to_string_lossy());
        }
        Cmd::OpenFolder => {
            if let Some(d) = files::config_dir() {
                send::open(&d.to_string_lossy());
            }
        }
        Cmd::OpenMenuFile(id) => {
            let path = match id {
                0 => files::ensure_menu_file(),
                id => with(|w| w.form.draft.profiles.iter().find(|p| p.id == id).cloned())
                    .flatten()
                    .and_then(|p| files::ensure_profile_file(&p)),
            };
            let status = match path {
                Some(p) => {
                    send::open(&p.to_string_lossy());
                    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    Status {
                        text: format!("{name} открыт. Правки применятся в течение нескольких секунд"),
                        bad: false,
                    }
                }
                None => Status {
                    text: "Не найдена папка APPDATA".into(),
                    bad: true,
                },
            };
            with(|w| w.form.status = Some(status));
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        Cmd::ResetMenuFile(id) => {
            let status = match files::reset_menu_file(id) {
                Ok(name) => Status {
                    text: format!("{name} сброшен к стандартному. Прежний файл сохранён как {name}.bak"),
                    bad: false,
                },
                Err(e) => Status { text: e, bad: true },
            };
            with(|w| w.form.status = Some(status));
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        Cmd::ResetGeneral => {
            let status = match files::reset_general() {
                Ok(_) => Status { text: "Общие настройки сброшены к стандартным".into(), bad: false },
                Err(e) => Status { text: e, bad: true },
            };
            preview_if_changed(); // тема в черновике могла измениться — сразу показать её в меню
            with(|w| w.form.status = Some(status));
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        Cmd::CheckUpdates => {
            updater::check(Mode::Settings);
            sync_update(hwnd);
        }
        Cmd::Update => {
            updater::install();
            sync_update(hwnd);
        }
        Cmd::DragWindow => {
            let _ = ReleaseCapture();
            SendMessageW(hwnd, WM_NCLBUTTONDOWN, Some(WPARAM(HTCAPTION as usize)), Some(LPARAM(0)));
        }
        Cmd::FocusVault => {
            if let Some(e) = with(|w| w.edit) {
                let _ = SetFocus(Some(h(e)));
            }
        }
    }
}

/// Esc и Enter из поля ввода — окну (иначе их съест поле).
unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if msg == WM_KEYDOWN && (wparam.0 == VK_ESCAPE.0 as usize || wparam.0 == VK_RETURN.0 as usize) {
        if let Some(p) = with(|w| w.hwnd) {
            let _ = PostMessageW(Some(h(p)), WM_SET_KEY, wparam, LPARAM(0));
        }
        return LRESULT(0);
    }
    // символ Enter/Esc поле не должно вставлять (звук «бип»)
    if msg == windows::Win32::UI::WindowsAndMessaging::WM_CHAR && (wparam.0 == 0x0D || wparam.0 == 0x1B) {
        return LRESULT(0);
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

unsafe extern "system" fn win_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_TIMER if wparam.0 == TIMER_UPDATE => {
            sync_update(hwnd);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = xy(lparam);
            let (cmd, track) = with(|w| {
                let track = !std::mem::replace(&mut w.tracking, true);
                (w.form.mouse_move(x, y), track)
            })
            .unwrap_or((Cmd::None, false));
            if track {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);
            }
            run(hwnd, cmd);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            let cmd = with(|w| {
                w.tracking = false;
                w.form.mouse_leave()
            })
            .unwrap_or(Cmd::None);
            run(hwnd, cmd);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = xy(lparam);
            let cmd = with(|w| w.form.mouse_down(x, y)).unwrap_or(Cmd::None);
            if cmd != Cmd::DragWindow {
                SetCapture(hwnd); // ползунок тянется и за пределами окна
                if cmd != Cmd::FocusVault {
                    let _ = SetFocus(Some(hwnd)); // увести фокус из поля, чтобы Esc/Enter работали
                }
            }
            run(hwnd, cmd);
            preview_if_changed();
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = xy(lparam);
            let _ = ReleaseCapture();
            let cmd = with(|w| w.form.mouse_up(x, y)).unwrap_or(Cmd::None);
            run(hwnd, cmd);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            // у колеса координаты экранные
            let mut p = POINT {
                x: (lparam.0 & 0xFFFF) as i16 as i32,
                y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
            };
            let _ = ScreenToClient(hwnd, &mut p);
            let up = ((wparam.0 >> 16) & 0xFFFF) as i16 > 0;
            let cmd = with(|w| w.form.wheel(p.x as f32, p.y as f32, up)).unwrap_or(Cmd::None);
            run(hwnd, cmd);
            LRESULT(0)
        }
        WM_RM_RECORD => {
            let m = lparam.0 as u32;
            let cmd = with(|w| {
                // боковая кнопка мыши: wParam = RECORD_MOUSE | номер кнопки
                if wparam.0 & hook::RECORD_MOUSE != 0 {
                    return w.form.record_button((wparam.0 & 0xFF) as u8);
                }
                w.form.record(
                    wparam.0 as u16,
                    m & MOD_CTRL != 0,
                    m & MOD_ALT != 0,
                    m & MOD_SHIFT != 0,
                    m & MOD_WIN != 0,
                )
            })
            .unwrap_or(Cmd::None);
            run(hwnd, cmd);
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SET_KEY => {
            let key = match wparam.0 {
                k if k == VK_ESCAPE.0 as usize => Some(Key::Escape),
                k if k == VK_RETURN.0 as usize => Some(Key::Enter),
                _ => None,
            };
            if let Some(k) = key {
                let cmd = with(|w| w.form.key(k)).unwrap_or(Cmd::None);
                run(hwnd, cmd);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            if ((wparam.0 >> 16) & 0xFFFF) as u32 == EN_CHANGE {
                let from = lparam.0;
                let (vault, name) = with(|w| (w.edit, w.name_edit)).unwrap_or((0, 0));
                if from == name {
                    let text = edit_text(h(name));
                    with(|w| w.form.set_name(&text));
                    let _ = InvalidateRect(Some(hwnd), None, false); // подпись вкладки
                } else if from == vault {
                    let text = edit_text(h(vault));
                    with(|w| w.form.set_vault(&text));
                }
            }
            LRESULT(0)
        }
        WM_CTLCOLOREDIT => {
            let hdc = HDC(wparam.0 as *mut _);
            let brush = with(|w| {
                let theme = theme_of(&w.form);
                if w.field_brush.0 != theme {
                    let _ = DeleteObject(HGDIOBJ(w.field_brush.1 as *mut _));
                    w.field_brush = (theme, CreateSolidBrush(rgb(palette::ui(theme).bg2)).0 as isize);
                }
                let c = palette::ui(theme);
                (c, w.field_brush.1)
            });
            match brush {
                Some((c, br)) => {
                    SetTextColor(hdc, rgb(c.fg));
                    SetBkColor(hdc, rgb(c.bg2));
                    LRESULT(br)
                }
                None => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
        WM_DESTROY => {
            hook::set_recorder(None); // закрыли посреди записи — вернуть перехват
            if let Some(w) = WIN.with(|s| s.take()) {
                let _ = DeleteObject(HGDIOBJ(w.field_brush.1 as *mut _));
                let _ = DeleteObject(HGDIOBJ(w.edit_font as *mut _));
                (w.on_preview)(None); // не сохранили — вернуть прежнюю тему меню
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

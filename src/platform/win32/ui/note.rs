//! Окно быстрой заметки для Obsidian.
//!
//! Пишешь текст → Ctrl+Enter (или «Сохранить») → заметка в корне хранилища: Ring_NN.md или
//! под своим названием (его можно поменять в шапке). Esc или крестик — закрыть. Нет папки
//! хранилища — её выбирают прямо в окне. Набрал `[[` — подсказки заметок (отдельное окно
//! `suggest`), стрелки/Enter/Tab подставляют ссылку, `[[` сам закрывается `]]`. Окно
//! немодальное и живёт, только пока открыто. Что править в тексте, решает `notes::editor`;
//! вид оболочки (шапка, карточка, кнопки) рисует tiny-skia, поля ввода — родные Edit.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, ClientToScreen, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DeleteObject,
    EndPaint, GetDC, GetTextMetricsW, InvalidateRect, ReleaseDC, SelectObject, SetBkColor,
    SetDIBitsToDevice, SetTextColor, SetWindowRgn, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_QUALITY, DIB_RGB_COLORS, FF_DONTCARE, FW_BOLD,
    FW_NORMAL, HDC, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS, PAINTSTRUCT, TEXTMETRICW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_CONTROL, VK_DOWN,
    VK_ESCAPE, VK_RETURN, VK_TAB, VK_UP,
};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, IsWindow, KillTimer, LoadCursorW, PostMessageW,
    RegisterClassW, SendMessageW, SetCursor, SetTimer, SetWindowTextW, ShowWindow, ES_AUTOHSCROLL,
    ES_AUTOVSCROLL, ES_MULTILINE, ES_WANTRETURN, HTCAPTION, HTCLIENT, IDC_ARROW, IDC_HAND,
    SW_HIDE, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CHAR, WM_CTLCOLOREDIT,
    WM_CTLCOLORSTATIC, WM_DESTROY, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_NCHITTEST, WM_PAINT, WM_SETCURSOR, WM_SETFONT, WM_TIMER, WNDCLASSW,
    WS_CHILD, WS_CLIPCHILDREN, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

use super::focus::take_focus;
use super::suggest::{self, WM_SUGGEST_PICK};
use crate::model::{IconRef, Theme};
use crate::notes::{self, editor, NoteRef, VaultIndex};
use crate::platform::win32::slot::Slot;
use crate::platform::win32::system::dialogs;
use crate::platform::win32::system::util::{h, log_error, monitor_at, rgb, ui_scale, wide};
use crate::render::painter::{Align, Painter};
use crate::render::palette::{self, Ui};
use crate::render::raster::{self, Icon, Mask};
use crate::ui::settings_form::Fonts;

const WM_NOTE_INDEX: u32 = WM_APP + 20;
const WM_NOTE_SUGGEST: u32 = WM_APP + 21;
const TIMER_CLOSE: usize = 1;
const WM_MOUSELEAVE: u32 = 0x02A3;

// Сообщения Edit (в windows-rs они разбросаны по модулям — проще держать числа).
const EM_GETSEL: u32 = 0x00B0;
const EM_SETSEL: u32 = 0x00B1;
const EM_REPLACESEL: u32 = 0x00C2;
const EM_LIMITTEXT: u32 = 0x00C5;
const EM_SETREADONLY: u32 = 0x00CF;
const EM_POSFROMCHAR: u32 = 0x00D6;

// Геометрия окна в логических px (умножается на масштаб).
const WIN_W: f32 = 520.0;
const WIN_H: f32 = 300.0;
const HEAD: f32 = 50.0; // шапка
const FOOT: f32 = 48.0; // подвал
const PAD: f32 = 14.0;
const CARD_PAD: f32 = 12.0; // поле ввода внутри карточки
const TITLE_W: f32 = 200.0; // поле названия

/// Кисти GDI под текущую тему (пересоздаются при её смене) — для полей ввода.
struct Brushes {
    bg: isize,
    bg2: isize,
}

impl Brushes {
    unsafe fn new(p: &Ui) -> Brushes {
        Brushes {
            bg: CreateSolidBrush(rgb(p.bg)).0 as isize,
            bg2: CreateSolidBrush(rgb(p.bg2)).0 as isize,
        }
    }

    unsafe fn free(&self) {
        for b in [self.bg, self.bg2] {
            let _ = DeleteObject(HGDIOBJ(b as *mut _));
        }
    }
}

/// Чуть светлее цвет — для наведения.
fn lighten(c: u32) -> u32 {
    let ch = |v: u32| v + (255 - v) * 18 / 100;
    ch(c >> 16 & 0xFF) << 16 | ch(c >> 8 & 0xFF) << 8 | ch(c & 0xFF)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TitleTone {
    Accent,
    Ok,
    Bad,
}

/// Кнопки, нарисованные самим окном.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Btn {
    Close,
    Save,
    Pick,
}

struct Note {
    /// Вызывается с папкой, которую выбрали в окне, чтобы запомнить её в настройках.
    on_pick: fn(&str),
    hover: Option<Btn>,
    hwnd: isize,
    edit: isize,
    title_edit: isize,
    vault: PathBuf,
    vault_ok: bool,
    index: Option<VaultIndex>,
    index_rx: Option<Receiver<VaultIndex>>,
    items: Vec<NoteRef>,
    /// Надпись в шапке, когда поля названия нет (сохранено, нет папки).
    title: String,
    /// Имя по умолчанию (Ring_NN), которое подставлено в поле названия.
    auto_title: String,
    tone: TitleTone,
    saved: bool,
    swallow_char: bool,
    pal: Ui,
    br: Brushes,
    fonts: Option<Fonts>,
    xmark: Option<Mask>,
    f_title: isize,
    f_text: isize,
    // геометрия, px
    scale: f32,
    line_h: i32,
    title_h: i32,
    w: i32,
    h: i32,
}

thread_local! {
    static NOTE: Slot<Note> = const { Slot::new() };
}

fn with<R>(f: impl FnOnce(&mut Note) -> R) -> Option<R> {
    NOTE.with(|s| s.with(f))
}

fn px(v: f32, s: f32) -> i32 {
    (v * s).round() as i32
}

unsafe fn font(points: f32, bold: bool, s: f32) -> HFONT {
    let height = -((points * 96.0 / 72.0 * s).round() as i32);
    CreateFontW(
        height,
        0,
        0,
        0,
        if bold { FW_BOLD.0 as i32 } else { FW_NORMAL.0 as i32 },
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        DEFAULT_QUALITY,
        FF_DONTCARE.0 as u32,
        w!("Segoe UI"),
    )
}

/// Открыть окно заметки (или вернуть фокус уже открытому).
pub fn open(vault: &str, theme: Theme, on_pick: fn(&str)) {
    let existing = with(|n| (n.hwnd, n.edit)).filter(|&(_, e)| unsafe { IsWindow(Some(h(e))).as_bool() });
    if let Some((hwnd, edit)) = existing {
        take_focus(h(hwnd), h(edit));
        return;
    }
    unsafe {
        if let Err(e) = create(vault, theme, on_pick) {
            log_error(&format!("окно заметки: {e}"));
        }
    }
}

/// Тема сменилась: перекрасить открытое окно.
pub fn apply_theme(theme: Theme) {
    let pal = palette::ui(theme);
    let hwnd = with(|n| unsafe {
        n.br.free();
        n.pal = pal;
        n.br = Brushes::new(&n.pal);
        n.hwnd
    });
    suggest::set_theme(pal);
    if let Some(hw) = hwnd {
        unsafe {
            let _ = InvalidateRect(Some(h(hw)), None, true);
        }
    }
}

/// Карточка под поле ввода: (left, top, right, bottom).
fn card_rect(w: i32, hgt: i32, s: f32) -> (i32, i32, i32, i32) {
    (px(PAD, s), px(HEAD - 2.0, s), w - px(PAD, s), hgt - px(FOOT, s))
}

/// Где стоит поле названия: (x, y, ширина, высота).
fn title_rect(title_h: i32, s: f32) -> (i32, i32, i32, i32) {
    let head = px(HEAD - 4.0, s);
    (px(PAD + 4.0, s), (head - title_h) / 2 + px(1.0, s), px(TITLE_W, s), title_h)
}

unsafe fn create(vault: &str, theme: Theme, on_pick: fn(&str)) -> windows::core::Result<()> {
    let s = ui_scale();
    let hinst = GetModuleHandleW(None)?;
    let class = w!("RadialMenuNote");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(note_proc),
        hInstance: hinst.into(),
        lpszClassName: class,
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        ..Default::default()
    };
    RegisterClassW(&wc);

    let (f_title, f_text) = (font(11.5, true, s), font(12.0, false, s));
    // высота строк в полях ввода
    let dc = GetDC(None);
    let metrics = |f: HFONT| {
        let old = SelectObject(dc, f.into());
        let mut tm = TEXTMETRICW::default();
        let _ = GetTextMetricsW(dc, &mut tm);
        SelectObject(dc, old);
        tm.tmHeight.max(12)
    };
    let (line_h, title_h) = (metrics(f_text), metrics(f_title));
    ReleaseDC(None, dc);

    let vault_path = PathBuf::from(vault);
    let vault_ok = !vault.is_empty() && vault_path.is_dir();
    let (ww, wh) = (px(WIN_W, s), px(WIN_H, s));
    let mut cur = Default::default();
    let _ = GetCursorPos(&mut cur);
    let (mx, my, mw, mh) = monitor_at(cur);
    let (x, y) = (mx + (mw - ww) / 2, my + (mh - wh) / 2 - px(40.0, s));

    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
        class,
        w!("Заметка"),
        WS_POPUP | WS_CLIPCHILDREN,
        x,
        y,
        ww,
        wh,
        None,
        None,
        Some(hinst.into()),
        None,
    )?;
    let r = px(16.0, s);
    let _ = SetWindowRgn(hwnd, Some(CreateRoundRectRgn(0, 0, ww + 1, wh + 1, r, r)), true); // скруглённые углы

    let (card_l, card_t, card_r, card_b) = card_rect(ww, wh, s);
    let inset = px(CARD_PAD, s);
    let shown = WINDOW_STYLE(if vault_ok { WS_VISIBLE.0 } else { 0 });
    let edit = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        w!("EDIT"),
        w!(""),
        WS_CHILD | shown | WINDOW_STYLE(ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | ES_WANTRETURN as u32),
        card_l + inset,
        card_t + inset - px(2.0, s),
        card_r - card_l - 2 * inset,
        card_b - card_t - 2 * inset + px(2.0, s),
        Some(hwnd),
        None,
        Some(hinst.into()),
        None,
    )?;
    let (tx, ty, tw, th) = title_rect(title_h, s);
    let title_edit = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        w!("EDIT"),
        w!(""),
        WS_CHILD | shown | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
        tx,
        ty,
        tw,
        th,
        Some(hwnd),
        None,
        Some(hinst.into()),
        None,
    )?;
    SendMessageW(edit, WM_SETFONT, Some(WPARAM(f_text.0 as usize)), Some(LPARAM(1)));
    SendMessageW(title_edit, WM_SETFONT, Some(WPARAM(f_title.0 as usize)), Some(LPARAM(1)));
    SendMessageW(title_edit, EM_LIMITTEXT, Some(WPARAM(100)), None);
    let _ = SetWindowSubclass(edit, Some(edit_proc), 1, 0);
    let _ = SetWindowSubclass(title_edit, Some(title_proc), 3, 0);

    let index_rx = vault_ok.then(|| start_scan(hwnd.0 as isize, vault_path.clone()));

    let pal = palette::ui(theme);
    let xmark = match raster::icon(&IconRef::builtin("xmark"), px(16.0, s) as usize, None, &raster::no_program_icons) {
        Some(Icon::Mask(m)) => Some(m),
        _ => None,
    };
    NOTE.with(|slot| {
        slot.set(Note {
            on_pick,
            hover: None,
            hwnd: hwnd.0 as isize,
            edit: edit.0 as isize,
            title_edit: title_edit.0 as isize,
            vault: vault_path,
            vault_ok,
            index: None,
            index_rx,
            items: Vec::new(),
            title: "Заметка".into(),
            auto_title: String::new(),
            tone: TitleTone::Accent,
            saved: false,
            swallow_char: false,
            br: Brushes::new(&pal),
            pal,
            fonts: Fonts::load(),
            xmark,
            f_title: f_title.0 as isize,
            f_text: f_text.0 as isize,
            scale: s,
            line_h,
            title_h,
            w: ww,
            h: wh,
        })
    });

    let _ = ShowWindow(hwnd, SW_SHOW);
    take_focus(hwnd, if vault_ok { edit } else { hwnd });
    Ok(())
}

fn close() {
    suggest::hide();
    if let Some(hw) = with(|n| n.hwnd) {
        unsafe {
            let _ = DestroyWindow(h(hw));
        }
    }
}

/// Обход хранилища — не на потоке интерфейса; по готовности окну придёт `WM_NOTE_INDEX`.
fn start_scan(hwnd: isize, path: PathBuf) -> Receiver<VaultIndex> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        if tx.send(notes::scan(&path)).is_ok() {
            unsafe {
                let _ = PostMessageW(Some(h(hwnd)), WM_NOTE_INDEX, WPARAM(0), LPARAM(0));
            }
        }
    });
    rx
}

/// Выбрать папку хранилища в окне: запомнить в настройках и сразу начать писать.
unsafe fn pick_vault() {
    let Some((hwnd, start, on_pick)) = with(|n| (n.hwnd, n.vault.to_string_lossy().into_owned(), n.on_pick)) else {
        return;
    };
    let Some(path) = dialogs::pick_folder(h(hwnd), "Папка хранилища Obsidian", &start) else {
        return;
    };
    on_pick(&path);
    let fields = with(|n| {
        n.vault = PathBuf::from(&path);
        n.vault_ok = true;
        n.hover = None;
        n.tone = TitleTone::Accent;
        n.index = None;
        n.index_rx = Some(start_scan(n.hwnd, n.vault.clone()));
        (n.edit, n.title_edit)
    });
    if let Some((e, t)) = fields {
        let _ = SetWindowTextW(h(e), w!(""));
        let _ = ShowWindow(h(e), SW_SHOW);
        let _ = ShowWindow(h(t), SW_SHOW);
        let _ = InvalidateRect(Some(h(hwnd)), None, true);
        take_focus(h(hwnd), h(e));
    }
}

// ---------------------------------------------------------------- кнопки

fn btn_rect(n: &Note, b: Btn) -> RECT {
    let s = n.scale;
    match b {
        Btn::Close => RECT {
            left: n.w - px(44.0, s),
            top: px(9.0, s),
            right: n.w - px(14.0, s),
            bottom: px(39.0, s),
        },
        Btn::Save => RECT {
            left: n.w - px(PAD + 124.0, s),
            top: n.h - px(FOOT - 8.0, s),
            right: n.w - px(PAD, s),
            bottom: n.h - px(10.0, s),
        },
        Btn::Pick => {
            let (l, t, r, bt) = card_rect(n.w, n.h, s);
            let (bw, bh) = (px(184.0, s), px(36.0, s));
            let top = (t + bt) / 2 + px(14.0, s);
            RECT { left: (l + r - bw) / 2, top, right: (l + r + bw) / 2, bottom: top + bh }
        }
    }
}

/// Кнопки, которые сейчас показаны.
fn active(n: &Note) -> Vec<Btn> {
    let mut v = vec![Btn::Close];
    if !n.vault_ok {
        v.push(Btn::Pick);
    } else if !n.saved {
        v.push(Btn::Save);
    }
    v
}

fn hit(n: &Note, x: i32, y: i32) -> Option<Btn> {
    active(n).into_iter().find(|&b| {
        let r = btn_rect(n, b);
        x >= r.left && x < r.right && y >= r.top && y < r.bottom
    })
}

unsafe fn press(b: Btn) {
    match b {
        Btn::Close => close(),
        Btn::Save => save(),
        Btn::Pick => pick_vault(),
    }
}

// ---------------------------------------------------------------- поле ввода

unsafe fn edit_text(edit: HWND) -> Vec<u16> {
    let len = GetWindowTextLengthW(edit).max(0) as usize;
    let mut buf = vec![0u16; len + 1];
    let n = GetWindowTextW(edit, &mut buf).max(0) as usize;
    buf.truncate(n);
    buf
}

unsafe fn selection(edit: HWND) -> (usize, usize) {
    let (mut a, mut b) = (0u32, 0u32);
    SendMessageW(
        edit,
        EM_GETSEL,
        Some(WPARAM(&mut a as *mut u32 as usize)),
        Some(LPARAM(&mut b as *mut u32 as isize)),
    );
    (a as usize, b as usize)
}

unsafe fn set_selection(edit: HWND, a: usize, b: usize) {
    SendMessageW(edit, EM_SETSEL, Some(WPARAM(a)), Some(LPARAM(b as isize)));
}

/// Применить правку из `notes::editor` (одним шагом — Ctrl+Z отменит её целиком).
unsafe fn apply(edit: HWND, e: &editor::TextEdit) {
    set_selection(edit, e.start, e.end);
    let t = wide(&e.insert);
    SendMessageW(edit, EM_REPLACESEL, Some(WPARAM(1)), Some(LPARAM(t.as_ptr() as isize)));
    set_selection(edit, e.caret, e.caret);
}

// ---------------------------------------------------------------- подсказки [[

unsafe fn suggest_update() {
    let Some((edit, ok, saved)) = with(|n| (n.edit, n.vault_ok, n.saved)) else {
        return;
    };
    if !ok || saved {
        return;
    }
    let ed = h(edit);
    let (caret, _) = selection(ed);
    let Some(query) = editor::query_at(&edit_text(ed), caret) else {
        return suggest::hide();
    };
    let items: Vec<NoteRef> = with(|n| match &n.index {
        Some(idx) => notes::suggest(idx, &query, 80).into_iter().cloned().collect(),
        None => Vec::new(),
    })
    .unwrap_or_default();
    if items.is_empty() {
        return suggest::hide();
    }
    let Some((line_h, scale, parent, pal)) = with(|n| (n.line_h, n.scale, n.hwnd, n.pal)) else {
        return;
    };
    let shown: Vec<String> = items.iter().map(|i| i.display.clone()).collect();
    with(|n| n.items = items);

    // строка с курсором — в координатах экрана
    let pos = SendMessageW(ed, EM_POSFROMCHAR, Some(WPARAM(caret)), None).0 as u32;
    let mut pt = POINT { x: (pos & 0xFFFF) as i16 as i32, y: (pos >> 16) as i16 as i32 };
    let _ = ClientToScreen(ed, &mut pt);
    suggest::show(h(parent), shown, (pt.x, pt.y, pt.y + line_h), pal, scale);
}

/// Подставить выбранную ссылку вместо набранной части после `[[`.
unsafe fn accept() {
    let Some(edit) = with(|n| n.edit) else {
        return;
    };
    let sel = suggest::selected();
    let link = sel.and_then(|i| with(|n| n.items.get(i).map(|x| x.link.clone())).flatten());
    let ed = h(edit);
    if let Some(link) = link {
        let (caret, _) = selection(ed);
        if let Some(e) = editor::accept_link(&edit_text(ed), caret, &link) {
            apply(ed, &e);
        }
    }
    suggest::hide();
}

// ---------------------------------------------------------------- сохранение

unsafe fn save() {
    let Some((edit, title_edit, ok, vault, ring, auto)) = with(|n| {
        (
            n.edit,
            n.title_edit,
            n.vault_ok,
            n.vault.clone(),
            n.index.as_ref().map(|i| i.max_ring),
            n.auto_title.clone(),
        )
    }) else {
        return;
    };
    if !ok {
        return close();
    }
    let text = String::from_utf16_lossy(&edit_text(h(edit)));
    if text.trim().is_empty() {
        return close();
    }
    let name = notes::clean_name(&String::from_utf16_lossy(&edit_text(h(title_edit))));
    let result = if name.is_empty() || name == auto {
        // имя по умолчанию; индекс ещё не готов — номер узнаём прямо сейчас
        let ring = ring.unwrap_or_else(|| notes::scan(&vault).max_ring);
        notes::save(&vault, &text, ring + 1)
    } else {
        notes::save_named(&vault, &text, &name)
    };
    suggest::hide();
    SendMessageW(h(edit), EM_SETREADONLY, Some(WPARAM(1)), None);
    let _ = ShowWindow(h(title_edit), SW_HIDE);
    if let Err(e) = &result {
        log_error(&format!("сохранение заметки: {e}"));
    }
    let hwnd = with(|n| {
        n.saved = true;
        (n.title, n.tone) = match &result {
            Ok(name) => (format!("Сохранено: {name}.md"), TitleTone::Ok),
            Err(_) => ("Не удалось сохранить".into(), TitleTone::Bad),
        };
        n.hwnd
    });
    if let Some(hw) = hwnd {
        let _ = InvalidateRect(Some(h(hw)), None, false);
        SetTimer(Some(h(hw)), TIMER_CLOSE, 650, None);
    }
}

// ---------------------------------------------------------------- процедуры контролов

/// Сочетания, общие для обоих полей: Ctrl+Enter — сохранить, Esc — закрыть.
unsafe fn common_keys(vk: u16) -> bool {
    let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
    if ctrl && vk == VK_RETURN.0 {
        with(|n| n.swallow_char = true);
        save();
        return true;
    }
    if vk == VK_ESCAPE.0 {
        if suggest::visible() {
            suggest::hide();
        } else {
            close();
        }
        return true;
    }
    false
}

unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    let list_visible = suggest::visible();
    let parent_msg = |m: u32| {
        if let Some(p) = with(|n| n.hwnd) {
            let _ = PostMessageW(Some(h(p)), m, WPARAM(0), LPARAM(0));
        }
    };
    match msg {
        WM_KEYDOWN => {
            let vk = wparam.0 as u16;
            let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
            if common_keys(vk) {
                return LRESULT(0);
            }
            if list_visible {
                if vk == VK_UP.0 {
                    suggest::move_sel(-1);
                    return LRESULT(0);
                }
                if vk == VK_DOWN.0 {
                    suggest::move_sel(1);
                    return LRESULT(0);
                }
                if vk == VK_RETURN.0 || vk == VK_TAB.0 {
                    with(|n| n.swallow_char = true);
                    accept();
                    return LRESULT(0);
                }
            }
            if ctrl && vk == b'A' as u16 {
                set_selection(hwnd, 0, usize::MAX); // выделить всё
                return LRESULT(0);
            }
        }
        WM_CHAR => {
            if with(|n| std::mem::take(&mut n.swallow_char)).unwrap_or(false) {
                return LRESULT(0); // символ от Enter/Tab, который мы уже обработали
            }
            let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
            if ctrl && wparam.0 == 0x0A {
                return LRESULT(0); // Ctrl+Enter приходит как перевод строки
            }
            if wparam.0 == '[' as usize {
                if let Some(e) = editor::auto_close(&edit_text(hwnd), selection(hwnd)) {
                    apply(hwnd, &e);
                    parent_msg(WM_NOTE_SUGGEST);
                    return LRESULT(0);
                }
            }
        }
        WM_KEYUP => {
            let vk = wparam.0 as u16;
            let quiet = [VK_UP.0, VK_DOWN.0, VK_RETURN.0, VK_TAB.0, VK_ESCAPE.0, 0x10, 0x11];
            if !quiet.contains(&vk) {
                parent_msg(WM_NOTE_SUGGEST);
            }
        }
        WM_LBUTTONUP => parent_msg(WM_NOTE_SUGGEST),
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/// Поле названия: Enter/Tab/↓ переводят в текст заметки, Esc закрывает, Ctrl+Enter сохраняет.
unsafe extern "system" fn title_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        WM_KEYDOWN => {
            let vk = wparam.0 as u16;
            if common_keys(vk) {
                return LRESULT(0);
            }
            if [VK_RETURN.0, VK_TAB.0, VK_DOWN.0].contains(&vk) {
                if let Some(e) = with(|n| n.edit) {
                    let _ = SetFocus(Some(h(e)));
                }
                return LRESULT(0);
            }
        }
        WM_CHAR => {
            let c = wparam.0;
            if with(|n| std::mem::take(&mut n.swallow_char)).unwrap_or(false) || c == 0x0D || c == 0x09 || c == 0x0A {
                return LRESULT(0); // Enter/Tab уже обработаны, звонка не нужно
            }
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

// ---------------------------------------------------------------- вид

/// Оболочка окна: рамка, шапка, карточка под текст и кнопки. Поля ввода лежат сверху.
fn chrome(n: &Note) -> Option<Painter> {
    let fonts = n.fonts.as_ref()?;
    let (s, p) = (n.scale, &n.pal);
    let mut c = Painter::new(n.w as u32, n.h as u32)?;
    c.clear(p.bg);
    c.stroke_rrect(0.0, 0.0, n.w as f32, n.h as f32, 16.0 * s, p.edge, 0.6, 1.0_f32.max(s * 0.5));

    // шапка
    let head_cy = (HEAD - 4.0) * s / 2.0 + 1.0 * s;
    let tx = (PAD + 4.0) * s;
    if n.vault_ok && !n.saved {
        let (fx, fy, fw, fh) = title_rect(n.title_h, s);
        // подчёркивание: «название можно менять»
        c.fill_rrect(fx as f32, (fy + fh) as f32 + 1.0 * s, fw as f32, 1.5 * s, 0.75 * s, p.acc, 0.45);
        c.text(&fonts.regular, ".md", (fx + fw) as f32 + 5.0 * s, head_cy, 14.0 * s, p.dim, Align::Left);
    } else {
        let tone = match n.tone {
            TitleTone::Accent => p.acc,
            TitleTone::Ok => p.ok,
            TitleTone::Bad => p.bad,
        };
        c.text(&fonts.bold, &n.title, tx, head_cy, 15.5 * s, tone, Align::Left);
    }

    // карточка под текст
    let (cl, ct, cr, cb) = card_rect(n.w, n.h, s);
    let (cl, ct, cw, ch) = (cl as f32, ct as f32, (cr - cl) as f32, (cb - ct) as f32);
    c.fill_rrect(cl, ct, cw, ch, 14.0 * s, p.bg2, 1.0);

    if n.vault_ok {
        if !n.saved {
            let right = btn_rect(n, Btn::Save).left as f32 - 12.0 * s;
            let hint = "Ctrl+Enter — сохранить   ·   Esc — закрыть   ·   [[ — ссылка";
            let cy = n.h as f32 - (FOOT - 8.0) * s / 2.0 - 8.0 * s;
            c.text_clipped(&fonts.regular, hint, tx, cy, 12.0 * s, p.dim, right - tx);
        }
    } else {
        // нет папки хранилища: объяснение (кнопка — среди кнопок ниже)
        let mid = cl + cw / 2.0;
        c.text(&fonts.bold, "Куда сохранять заметки?", mid, ct + 38.0 * s, 16.0 * s, p.fg, Align::Center);
        let msg = if n.vault.as_os_str().is_empty() {
            "Выберите папку хранилища Obsidian — заметки\nбудут появляться в ней как Ring_NN.md.".to_string()
        } else {
            format!("Папка не найдена:\n{}", n.vault.display())
        };
        for (i, line) in msg.lines().enumerate() {
            let cy = ct + (70.0 + 20.0 * i as f32) * s;
            c.text_clipped(&fonts.regular, line, cl + 20.0 * s, cy, 13.0 * s, p.dim, cw - 40.0 * s);
        }
    }

    // кнопки
    for b in active(n) {
        let r = btn_rect(n, b);
        let hot = n.hover == Some(b);
        let (x, y, w, hh) = (r.left as f32, r.top as f32, (r.right - r.left) as f32, (r.bottom - r.top) as f32);
        match b {
            Btn::Close => {
                if hot {
                    c.fill_rrect(x, y, w, hh, 9.0 * s, p.bad, 1.0);
                }
                if let Some(m) = &n.xmark {
                    let (ix, iy) = (x + (w - m.n as f32) / 2.0, y + (hh - m.n as f32) / 2.0);
                    c.mask(m, ix, iy, if hot { p.on_acc } else { p.dim }, 1.0);
                }
            }
            Btn::Save | Btn::Pick => {
                c.fill_rrect(x, y, w, hh, 12.0 * s, if hot { lighten(p.acc) } else { p.acc }, 1.0);
                let label = if b == Btn::Save { "Сохранить" } else { "Выбрать папку…" };
                c.text(&fonts.bold, label, x + w / 2.0, y + hh / 2.0 + 0.5 * s, 13.5 * s, p.on_acc, Align::Center);
            }
        }
    }
    Some(c)
}

unsafe fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc: HDC = BeginPaint(hwnd, &mut ps);
    if let Some((img, w, hgt)) = with(|n| chrome(n).map(|c| (c, n.w, n.h))).flatten() {
        let mut bgra = vec![0u8; (w * hgt * 4) as usize];
        img.to_bgra(&mut bgra);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -hgt, // сверху вниз
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        SetDIBitsToDevice(hdc, 0, 0, w as u32, hgt as u32, 0, 0, 0, hgt as u32, bgra.as_ptr() as *const _, &bmi, DIB_RGB_COLORS);
    }
    let _ = EndPaint(hwnd, &ps);
}

unsafe extern "system" fn note_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_NCHITTEST => {
            // шапку можно тянуть, как заголовок окна (кроме крестика)
            let r = DefWindowProcW(hwnd, msg, wparam, lparam);
            if r.0 as u32 == HTCLIENT {
                let (x, y) = (lparam.0 as i16 as i32, (lparam.0 >> 16) as i16 as i32);
                let mut wr = RECT::default();
                let _ = GetWindowRect(hwnd, &mut wr);
                let on_button = with(|n| hit(n, x - wr.left, y - wr.top).is_some()).unwrap_or(false);
                let s = with(|n| n.scale).unwrap_or(1.0);
                if !on_button && y - wr.top < px(HEAD - 4.0, s) {
                    return LRESULT(HTCAPTION as isize);
                }
            }
            r
        }
        WM_MOUSEMOVE => {
            let (x, y) = (lparam.0 as i16 as i32, (lparam.0 >> 16) as i16 as i32);
            let rects = with(|n| {
                let now = hit(n, x, y);
                (now != n.hover).then(|| {
                    let old = std::mem::replace(&mut n.hover, now);
                    [old, now].map(|b| b.map(|b| btn_rect(n, b)))
                })
            })
            .flatten();
            if let Some(rects) = rects {
                for r in rects.into_iter().flatten() {
                    let _ = InvalidateRect(Some(hwnd), Some(&r), false);
                }
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            let old = with(|n| n.hover.take().map(|b| btn_rect(n, b))).flatten();
            if let Some(r) = old {
                let _ = InvalidateRect(Some(hwnd), Some(&r), false);
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            if with(|n| n.hover.is_some()).unwrap_or(false) && (lparam.0 & 0xFFFF) as u32 == HTCLIENT {
                SetCursor(LoadCursorW(None, IDC_HAND).ok());
                return LRESULT(1);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_LBUTTONDOWN => LRESULT(0),
        WM_LBUTTONUP => {
            let (x, y) = (lparam.0 as i16 as i32, (lparam.0 >> 16) as i16 as i32);
            if let Some(b) = with(|n| hit(n, x, y)).flatten() {
                press(b);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            // фокус на самом окне (поля скрыты, пока не выбрана папка)
            if wparam.0 as u16 == VK_ESCAPE.0 {
                close();
            } else if wparam.0 as u16 == VK_RETURN.0 && with(|n| !n.vault_ok).unwrap_or(false) {
                pick_vault();
            }
            LRESULT(0)
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC => {
            let hdc = HDC(wparam.0 as *mut _);
            let child = lparam.0;
            let Some((fg, bg, br)) = with(|n| {
                if child == n.title_edit {
                    (n.pal.acc, n.pal.bg, n.br.bg) // название — на фоне окна, акцентным цветом
                } else {
                    (n.pal.fg, n.pal.bg2, n.br.bg2) // текст заметки — на фоне карточки
                }
            }) else {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            };
            SetTextColor(hdc, rgb(fg));
            SetBkColor(hdc, rgb(bg));
            LRESULT(br)
        }
        WM_SUGGEST_PICK => {
            accept();
            if let Some((hw, e)) = with(|n| (n.hwnd, n.edit)) {
                take_focus(h(hw), h(e));
            }
            LRESULT(0)
        }
        WM_NOTE_INDEX => {
            let got = with(|n| {
                let idx = n.index_rx.as_ref()?.try_recv().ok()?;
                let name = notes::ring_name(idx.max_ring + 1);
                n.index = Some(idx);
                n.index_rx = None;
                let old = std::mem::replace(&mut n.auto_title, name.clone());
                Some((name, old, n.title_edit))
            })
            .flatten();
            if let Some((name, old, title_edit)) = got {
                // подставляем имя по умолчанию, если поле пустое или в нём прежнее авто-имя
                let cur = String::from_utf16_lossy(&edit_text(h(title_edit)));
                if cur.is_empty() || cur == old {
                    let t = wide(&name);
                    let _ = SetWindowTextW(h(title_edit), PCWSTR(t.as_ptr()));
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
                suggest_update(); // пока ждали, могли уже набрать [[
            }
            LRESULT(0)
        }
        WM_NOTE_SUGGEST => {
            suggest_update();
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_CLOSE => {
            let _ = KillTimer(Some(hwnd), TIMER_CLOSE);
            close();
            LRESULT(0)
        }
        WM_DESTROY => {
            suggest::destroy();
            if let Some(n) = NOTE.with(|s| s.take()) {
                n.br.free();
                for f in [n.f_title, n.f_text] {
                    let _ = DeleteObject(HGDIOBJ(f as *mut _));
                }
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Снимок окна для проверки внешнего вида: `NOTE_SHOT=файл.png [NOTE_VAULT=папка] [NOTE_THEME=light]
/// cargo test note_screenshot -- --ignored`. Нужен рабочий стол Windows.
#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Gdi::{CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, GetDIBits};
    use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
    use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE};

    #[test]
    #[ignore = "открывает окно на рабочем столе"]
    fn note_screenshot() {
        let Ok(out) = std::env::var("NOTE_SHOT") else { return };
        let vault = std::env::var("NOTE_VAULT").unwrap_or_default();
        let theme = if std::env::var("NOTE_THEME").is_ok_and(|t| t == "light") { Theme::Light } else { Theme::Dark };
        unsafe {
            open(&vault, theme, |_| {});
            let t0 = std::time::Instant::now();
            while t0.elapsed().as_millis() < 700 {
                let mut m = MSG::default();
                while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&m);
                    DispatchMessageW(&m);
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let hw = with(|n| n.hwnd).expect("окно открылось");
            let (w, hgt) = with(|n| (n.w, n.h)).unwrap();
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let bmp = CreateCompatibleBitmap(screen, w, hgt);
            let old = SelectObject(dc, bmp.into());
            let _ = PrintWindow(h(hw), dc, PRINT_WINDOW_FLAGS(2));
            let mut bi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -hgt,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut px = vec![0u8; (w * hgt * 4) as usize];
            GetDIBits(dc, bmp, 0, hgt as u32, Some(px.as_mut_ptr() as *mut _), &mut bi, DIB_RGB_COLORS);
            SelectObject(dc, old);
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);
            let rgb: Vec<u8> = px.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0]]).collect();
            let f = std::fs::File::create(&out).unwrap();
            let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w as u32, hgt as u32);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&rgb).unwrap();
            close();
        }
    }
}

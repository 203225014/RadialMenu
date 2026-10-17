//! Окно быстрой заметки для Obsidian на «родных» контролах Win32.
//!
//! Пишешь текст → Ctrl+Enter (или кнопка «Сохранить») → Ring_NN.md в корне хранилища. Esc или
//! крестик — отмена. Нет папки хранилища — её можно выбрать прямо в окне. Набрал `[[` —
//! список заметок (фильтруется по набору), стрелки/Enter/Tab подставляют ссылку,
//! `[[` автоматически закрывается `]]`. Окно немодальное и живёт, только пока открыто.
//! Что именно править в тексте, решает `notes::editor`; здесь — только окно.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreatePen, CreateRoundRectRgn, CreateSolidBrush, DeleteObject,
    DrawTextW, EndPaint, FillRect, GetDC, GetStockObject, GetTextExtentPoint32W, GetTextMetricsW,
    InvalidateRect, LineTo, SetWindowRgn, MoveToEx, ReleaseDC, RoundRect, SelectObject, SetBkColor, SetBkMode,
    SetTextColor, TextOutW, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_QUALITY, DT_CENTER,
    DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK, FF_DONTCARE, FW_BOLD, FW_NORMAL, HBRUSH,
    HDC, HFONT, HGDIOBJ, NULL_BRUSH, NULL_PEN, OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID,
    TEXTMETRICW, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_CONTROL, VK_DOWN, VK_ESCAPE,
    VK_RETURN, VK_TAB, VK_UP,
};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetCursorPos,
    GetWindowRect, GetWindowTextLengthW, GetWindowTextW, IsWindow, IsWindowVisible, KillTimer,
    LoadCursorW, PostMessageW, RegisterClassW, SendMessageW, SetLayeredWindowAttributes, SetTimer,
    SetWindowPos, ShowWindow, ES_AUTOVSCROLL, ES_MULTILINE, ES_WANTRETURN, HTCAPTION, HTCLIENT,
    HWND_TOP, IDC_ARROW, IDC_HAND, SetCursor, SetWindowTextW, WM_LBUTTONDOWN,
    WM_MOUSEMOVE, WM_SETCURSOR, LBS_NOINTEGRALHEIGHT, LBS_NOTIFY, LBS_OWNERDRAWFIXED, LB_ADDSTRING,
    LB_GETCOUNT, LB_GETCURSEL, LB_RESETCONTENT, LB_SETCURSEL, LB_SETTOPINDEX, LWA_ALPHA,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
    WM_CHAR, WM_CTLCOLOREDIT, WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DRAWITEM,
    WM_KEYDOWN, WM_KEYUP, WM_LBUTTONUP, WM_MEASUREITEM, WM_NCHITTEST, WM_PAINT, WM_SETFONT,
    WM_TIMER, WNDCLASSW, WS_BORDER, WS_CHILD, WS_CLIPCHILDREN, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

use super::focus::take_focus;
use crate::model::Theme;
use crate::notes::{self, editor, NoteRef, VaultIndex};
use crate::platform::win32::slot::Slot;
use crate::platform::win32::system::dialogs;
use crate::platform::win32::system::util::{h, log_error, monitor_at, rgb, ui_scale, wide};
use crate::render::palette::{self, Ui};

const WM_NOTE_INDEX: u32 = WM_APP + 20;
const WM_NOTE_SUGGEST: u32 = WM_APP + 21;
const WM_NOTE_ACCEPT: u32 = WM_APP + 22;
const TIMER_CLOSE: usize = 1;
const WM_MOUSELEAVE: u32 = 0x02A3;

// Геометрия окна в логических px (умножается на масштаб).
const WIN_W: f32 = 520.0;
const WIN_H: f32 = 300.0;
const HEAD: f32 = 46.0; // шапка
const FOOT: f32 = 48.0; // подвал
const PAD: f32 = 14.0;
const CARD_PAD: f32 = 12.0; // поле ввода внутри карточки

// Сообщения Edit (в windows-rs они разбросаны по модулям — проще держать числа).
const EM_GETSEL: u32 = 0x00B0;
const EM_SETSEL: u32 = 0x00B1;
const EM_REPLACESEL: u32 = 0x00C2;
const EM_SETREADONLY: u32 = 0x00CF;
const EM_POSFROMCHAR: u32 = 0x00D6;

/// Ресурсы GDI под текущую тему (пересоздаются при её смене).
struct Brushes {
    bg: isize,
    bg2: isize,
    edge: isize,
    acc: isize,
    acc_hot: isize,
    bad: isize,
}

impl Brushes {
    unsafe fn new(p: &Ui) -> Brushes {
        Brushes {
            bg: CreateSolidBrush(rgb(p.bg)).0 as isize,
            bg2: CreateSolidBrush(rgb(p.bg2)).0 as isize,
            edge: CreateSolidBrush(rgb(p.edge)).0 as isize,
            acc: CreateSolidBrush(rgb(p.acc)).0 as isize,
            acc_hot: CreateSolidBrush(rgb(lighten(p.acc))).0 as isize,
            bad: CreateSolidBrush(rgb(p.bad)).0 as isize,
        }
    }

    unsafe fn free(&self) {
        for b in [self.bg, self.bg2, self.edge, self.acc, self.acc_hot, self.bad] {
            let _ = DeleteObject(HGDIOBJ(b as *mut _));
        }
    }
}

/// Чуть светлее/ярче цвет для наведения.
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
    list: isize,
    vault: PathBuf,
    vault_ok: bool,
    index: Option<VaultIndex>,
    index_rx: Option<Receiver<VaultIndex>>,
    items: Vec<NoteRef>,
    title: String,
    tone: TitleTone,
    saved: bool,
    swallow_char: bool,
    pal: Ui,
    br: Brushes,
    f_title: isize,
    f_text: isize,
    f_small: isize,
    f_btn: isize,
    // геометрия, px
    w: i32,
    h: i32,
    scale: f32,
    item_h: i32,
    list_w: i32,
    line_h: i32,
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
        if bold {
            FW_BOLD.0 as i32
        } else {
            FW_NORMAL.0 as i32
        },
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
    let hwnd = with(|n| unsafe {
        n.br.free();
        n.pal = palette::ui(theme);
        n.br = Brushes::new(&n.pal);
        n.hwnd
    });
    if let Some(hw) = hwnd {
        unsafe {
            let _ = InvalidateRect(Some(h(hw)), None, true);
        }
    }
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

    let (f_title, f_text, f_small) = (font(11.5, true, s), font(12.0, false, s), font(9.0, false, s));
    let f_btn = font(10.0, true, s);
    // высота строки текста и ширина списка
    let dc = GetDC(None);
    let old = SelectObject(dc, f_text.into());
    let mut tm = TEXTMETRICW::default();
    let _ = GetTextMetricsW(dc, &mut tm);
    SelectObject(dc, old);
    ReleaseDC(None, dc);
    let line_h = tm.tmHeight.max(12);
    let item_h = line_h + px(6.0, s);
    let list_w = tm.tmAveCharWidth.max(6) * 34;

    let vault_path = PathBuf::from(vault);
    let vault_ok = !vault.is_empty() && vault_path.is_dir();
    let (ww, wh) = (px(WIN_W, s), px(WIN_H, s));
    let mut cur = Default::default();
    let _ = GetCursorPos(&mut cur);
    let (mx, my, mw, mh) = monitor_at(cur);
    let (x, y) = (mx + (mw - ww) / 2, my + (mh - wh) / 2 - px(40.0, s));

    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
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
    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 244, LWA_ALPHA);
    let r = px(16.0, s);
    let _ = SetWindowRgn(hwnd, Some(CreateRoundRectRgn(0, 0, ww + 1, wh + 1, r, r)), true); // скруглённые углы

    let (card_l, card_t, card_r, card_b) = card_rect(ww, wh, s);
    let inset = px(CARD_PAD, s);
    let edit = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        w!("EDIT"),
        w!(""),
        WS_CHILD
            | WINDOW_STYLE(if vault_ok { WS_VISIBLE.0 } else { 0 })
            | WINDOW_STYLE(ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | ES_WANTRETURN as u32),
        card_l + inset,
        card_t + inset - px(2.0, s),
        card_r - card_l - 2 * inset,
        card_b - card_t - 2 * inset + px(2.0, s),
        Some(hwnd),
        None,
        Some(hinst.into()),
        None,
    )?;
    let list = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        w!("LISTBOX"),
        w!(""),
        WS_CHILD
            | WS_BORDER
            | WINDOW_STYLE((LBS_NOTIFY | LBS_OWNERDRAWFIXED | LBS_NOINTEGRALHEIGHT) as u32),
        0,
        0,
        list_w,
        item_h * 6 + 2,
        Some(hwnd),
        None,
        Some(hinst.into()),
        None,
    )?;
    for c in [edit, list] {
        SendMessageW(c, WM_SETFONT, Some(WPARAM(f_text.0 as usize)), Some(LPARAM(1)));
    }
    let _ = SetWindowSubclass(edit, Some(edit_proc), 1, 0);
    let _ = SetWindowSubclass(list, Some(list_proc), 2, 0);

    let index_rx = vault_ok.then(|| start_scan(hwnd.0 as isize, vault_path.clone()));

    let pal = palette::ui(theme);
    NOTE.with(|slot| {
        slot.set(Note {
            on_pick,
            hover: None,
            hwnd: hwnd.0 as isize,
            edit: edit.0 as isize,
            list: list.0 as isize,
            vault: vault_path,
            vault_ok,
            index: None,
            index_rx,
            items: Vec::new(),
            title: if vault_ok { "Ring_…".into() } else { "Заметка".into() },
            tone: TitleTone::Accent,
            saved: false,
            swallow_char: false,
            br: Brushes::new(&pal),
            pal,
            f_title: f_title.0 as isize,
            f_text: f_text.0 as isize,
            f_small: f_small.0 as isize,
            f_btn: f_btn.0 as isize,
            w: ww,
            h: wh,
            scale: s,
            item_h,
            list_w,
            line_h,
        })
    });

    let _ = ShowWindow(hwnd, SW_SHOW);
    take_focus(hwnd, if vault_ok { edit } else { hwnd });
    Ok(())
}

fn close() {
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
    let edit = with(|n| {
        n.vault = PathBuf::from(&path);
        n.vault_ok = true;
        n.hover = None;
        n.title = "Ring_…".into();
        n.tone = TitleTone::Accent;
        n.index = None;
        n.index_rx = Some(start_scan(n.hwnd, n.vault.clone()));
        n.edit
    });
    if let Some(e) = edit {
        let _ = SetWindowTextW(h(e), w!(""));
        let _ = ShowWindow(h(e), SW_SHOW);
        let _ = InvalidateRect(Some(h(hwnd)), None, true);
        take_focus(h(hwnd), h(e));
    }
}

// ---------------------------------------------------------------- кнопки

/// Карточка под поле ввода: (left, top, right, bottom).
fn card_rect(w: i32, hgt: i32, s: f32) -> (i32, i32, i32, i32) {
    (px(PAD, s), px(HEAD, s), w - px(PAD, s), hgt - px(FOOT, s))
}

fn btn_rect(n: &Note, b: Btn) -> RECT {
    let s = n.scale;
    match b {
        Btn::Close => RECT {
            left: n.w - px(42.0, s),
            top: px(9.0, s),
            right: n.w - px(14.0, s),
            bottom: px(37.0, s),
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
            RECT {
                left: (l + r - bw) / 2,
                top,
                right: (l + r + bw) / 2,
                bottom: top + bh,
            }
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

unsafe fn hide_list() {
    if let Some(l) = with(|n| n.list) {
        let _ = ShowWindow(h(l), SW_HIDE);
    }
}

unsafe fn suggest_update() {
    let Some((edit, list, ok, saved)) = with(|n| (n.edit, n.list, n.vault_ok, n.saved)) else {
        return;
    };
    if !ok || saved {
        return;
    }
    let ed = h(edit);
    let (caret, _) = selection(ed);
    let Some(query) = editor::query_at(&edit_text(ed), caret) else {
        return hide_list();
    };
    let items: Vec<NoteRef> = with(|n| match &n.index {
        Some(idx) => notes::suggest(idx, &query, 80).into_iter().cloned().collect(),
        None => Vec::new(),
    })
    .unwrap_or_default();
    if items.is_empty() {
        return hide_list();
    }
    let Some((item_h, list_w, line_h, scale, parent)) =
        with(|n| (n.item_h, n.list_w, n.line_h, n.scale, n.hwnd))
    else {
        return;
    };
    let lb = h(list);
    SendMessageW(lb, LB_RESETCONTENT, None, None);
    for it in &items {
        let t = wide(&it.display);
        SendMessageW(lb, LB_ADDSTRING, None, Some(LPARAM(t.as_ptr() as isize)));
    }
    SendMessageW(lb, LB_SETCURSEL, Some(WPARAM(0)), None);
    let rows = items.len().min(6) as i32;
    let list_h = rows * item_h + 2;
    with(|n| n.items = items);

    // под строкой с курсором; не помещается — над ней
    let pos = SendMessageW(ed, EM_POSFROMCHAR, Some(WPARAM(caret)), None).0 as u32;
    let (cx, cy) = ((pos & 0xFFFF) as i16 as i32, (pos >> 16) as i16 as i32);
    let mut er = RECT::default();
    let _ = GetWindowRect(ed, &mut er);
    let mut pr = RECT::default();
    let _ = GetWindowRect(h(parent), &mut pr);
    let (ex, ey) = (er.left - pr.left, er.top - pr.top);
    let (bw, bh) = (pr.right - pr.left - 2, pr.bottom - pr.top - 2);
    let pad = px(4.0, scale);
    let x = (ex + cx).clamp(pad, (bw - list_w - pad).max(pad));
    let mut y = ey + cy + line_h + 2;
    if y + list_h > bh - pad {
        y = (ey + cy - list_h - 2).max(pad);
    }
    let _ = SetWindowPos(lb, Some(HWND_TOP), x, y, list_w, list_h, SWP_SHOWWINDOW | SWP_NOACTIVATE);
}

unsafe fn move_selection(d: i32) {
    let Some(list) = with(|n| n.list) else { return };
    let lb = h(list);
    let n = SendMessageW(lb, LB_GETCOUNT, None, None).0 as i32;
    if n <= 0 {
        return;
    }
    let cur = SendMessageW(lb, LB_GETCURSEL, None, None).0 as i32;
    let i = (cur.max(0) + d).clamp(0, n - 1);
    SendMessageW(lb, LB_SETCURSEL, Some(WPARAM(i as usize)), None);
    SendMessageW(lb, LB_SETTOPINDEX, Some(WPARAM(i as usize)), None); // держим выбранный в виду
}

/// Подставить выбранную ссылку вместо набранной части после `[[`.
unsafe fn accept() {
    let Some((edit, list)) = with(|n| (n.edit, n.list)) else {
        return;
    };
    let sel = SendMessageW(h(list), LB_GETCURSEL, None, None).0 as i32;
    let link = with(|n| n.items.get(sel.max(0) as usize).map(|i| i.link.clone())).flatten();
    let ed = h(edit);
    if let Some(link) = link.filter(|_| sel >= 0) {
        let (caret, _) = selection(ed);
        if let Some(e) = editor::accept_link(&edit_text(ed), caret, &link) {
            apply(ed, &e);
        }
    }
    hide_list();
}

// ---------------------------------------------------------------- сохранение

unsafe fn save() {
    let Some((edit, ok, vault, ring)) = with(|n| {
        (n.edit, n.vault_ok, n.vault.clone(), n.index.as_ref().map(|i| i.max_ring))
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
    // индекс ещё не готов — номер узнаём прямо сейчас
    let ring = ring.unwrap_or_else(|| notes::scan(&vault).max_ring);
    let result = notes::save(&vault, &text, ring + 1);
    hide_list();
    SendMessageW(h(edit), EM_SETREADONLY, Some(WPARAM(1)), None);
    if let Err(e) = &result {
        log_error(&format!("сохранение заметки: {e}"));
    }
    let hwnd = with(|n| {
        n.saved = true;
        (n.title, n.tone) = match &result {
            Ok(name) => (format!("Сохранено: {name}"), TitleTone::Ok),
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

unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    let list_visible = with(|n| n.list)
        .map(|l| IsWindowVisible(h(l)).as_bool())
        .unwrap_or(false);
    let parent_msg = |m: u32| {
        if let Some(p) = with(|n| n.hwnd) {
            let _ = PostMessageW(Some(h(p)), m, WPARAM(0), LPARAM(0));
        }
    };
    match msg {
        WM_KEYDOWN => {
            let vk = wparam.0 as u16;
            let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
            if ctrl && vk == VK_RETURN.0 {
                with(|n| n.swallow_char = true);
                save();
                return LRESULT(0);
            }
            if vk == VK_ESCAPE.0 {
                if list_visible {
                    hide_list();
                } else {
                    close();
                }
                return LRESULT(0);
            }
            if list_visible {
                if vk == VK_UP.0 {
                    move_selection(-1);
                    return LRESULT(0);
                }
                if vk == VK_DOWN.0 {
                    move_selection(1);
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

/// Клик по подсказке подставляет её (LBN_SELCHANGE не приходит, если кликнули по уже выбранной).
unsafe extern "system" fn list_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    let r = DefSubclassProc(hwnd, msg, wparam, lparam);
    if msg == WM_LBUTTONUP {
        if let Some(p) = with(|n| n.hwnd) {
            let _ = PostMessageW(Some(h(p)), WM_NOTE_ACCEPT, WPARAM(0), LPARAM(0));
        }
    }
    r
}

// ---------------------------------------------------------------- процедура окна

unsafe fn text_at(hdc: HDC, font: isize, color: u32, text: &str, mut rc: RECT, flags: windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT) {
    let old = SelectObject(hdc, HGDIOBJ(font as *mut _));
    SetTextColor(hdc, rgb(color));
    let mut t: Vec<u16> = text.encode_utf16().collect();
    DrawTextW(hdc, &mut t, &mut rc, flags | DT_NOPREFIX);
    SelectObject(hdc, old);
}

/// Скруглённый прямоугольник заливкой `fill`, без контура.
unsafe fn round_fill(hdc: HDC, r: &RECT, fill: isize, radius: i32) {
    let (ob, op) = (
        SelectObject(hdc, HGDIOBJ(fill as *mut _)),
        SelectObject(hdc, GetStockObject(NULL_PEN)),
    );
    let _ = RoundRect(hdc, r.left, r.top, r.right + 1, r.bottom + 1, radius, radius);
    SelectObject(hdc, ob);
    SelectObject(hdc, op);
}

unsafe fn line(hdc: HDC, color: u32, width: i32, (x0, y0): (i32, i32), (x1, y1): (i32, i32)) {
    let pen = CreatePen(PS_SOLID, width, rgb(color));
    let old = SelectObject(hdc, pen.into());
    let _ = MoveToEx(hdc, x0, y0, None);
    let _ = LineTo(hdc, x1, y1);
    SelectObject(hdc, old);
    let _ = DeleteObject(pen.into());
}

unsafe fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc: HDC = BeginPaint(hwnd, &mut ps);
    with(|n| {
        let (s, p) = (n.scale, n.pal);
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        FillRect(hdc, &rc, HBRUSH(n.br.bg as *mut _));
        // рамка по скруглённому краю
        let pen = CreatePen(PS_SOLID, 1, rgb(p.edge));
        let (op, ob) = (SelectObject(hdc, pen.into()), SelectObject(hdc, GetStockObject(NULL_BRUSH)));
        let r = px(16.0, s);
        let _ = RoundRect(hdc, 0, 0, rc.right, rc.bottom, r, r);
        SelectObject(hdc, op);
        SelectObject(hdc, ob);
        let _ = DeleteObject(pen.into());
        SetBkMode(hdc, TRANSPARENT);

        // шапка: название, пояснение, тонкая линия
        let tone = match n.tone {
            TitleTone::Accent => p.acc,
            TitleTone::Ok => p.ok,
            TitleTone::Bad => p.bad,
        };
        let (x, y) = (px(PAD + 4.0, s), px(11.0, s));
        let old = SelectObject(hdc, HGDIOBJ(n.f_title as *mut _));
        SetTextColor(hdc, rgb(tone));
        let t: Vec<u16> = n.title.encode_utf16().collect();
        let _ = TextOutW(hdc, x, y, &t);
        let mut sz = SIZE::default();
        let _ = GetTextExtentPoint32W(hdc, &t, &mut sz);
        SelectObject(hdc, old);
        let sub = RECT { left: x + sz.cx + px(10.0, s), top: y + px(3.0, s), right: n.w - px(60.0, s), bottom: y + px(24.0, s) };
        text_at(hdc, n.f_small, p.dim, "новая заметка", sub, DT_SINGLELINE | DT_VCENTER);
        let sep = RECT { left: px(PAD, s), top: px(HEAD - 4.0, s), right: n.w - px(PAD, s), bottom: px(HEAD - 3.0, s) };
        FillRect(hdc, &sep, HBRUSH(n.br.bg2 as *mut _));

        // карточка под текст
        let (cl, ct, cr, cb) = card_rect(n.w, n.h, s);
        let card = RECT { left: cl, top: ct, right: cr, bottom: cb };
        round_fill(hdc, &card, n.br.bg2, px(14.0, s));

        if n.vault_ok {
            let hint = RECT { left: px(PAD + 4.0, s), top: n.h - px(FOOT - 4.0, s), right: n.w - px(PAD + 134.0, s), bottom: n.h - px(8.0, s) };
            let text = if n.saved { "" } else { "Ctrl+Enter — сохранить   ·   Esc — закрыть   ·   [[ — ссылка" };
            text_at(hdc, n.f_small, p.dim, text, hint, DT_SINGLELINE | DT_VCENTER);
        } else {
            // нет папки хранилища: объяснение и кнопка выбора
            let head = RECT { left: cl + px(20.0, s), top: ct + px(26.0, s), right: cr - px(20.0, s), bottom: ct + px(54.0, s) };
            text_at(hdc, n.f_title, p.fg, "Куда сохранять заметки?", head, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
            let msg = if n.vault.as_os_str().is_empty() {
                "Выберите папку хранилища Obsidian — заметки\nбудут появляться в ней как Ring_NN.md.".to_string()
            } else {
                format!("Папка не найдена:\n{}", n.vault.display())
            };
            let body = RECT { left: cl + px(20.0, s), top: ct + px(58.0, s), right: cr - px(20.0, s), bottom: ct + px(112.0, s) };
            text_at(hdc, n.f_small, p.dim, &msg, body, DT_CENTER | DT_WORDBREAK);
        }

        // кнопки
        for b in active(n) {
            let r = btn_rect(n, b);
            let hot = n.hover == Some(b);
            match b {
                Btn::Close => {
                    if hot {
                        round_fill(hdc, &r, n.br.bad, px(10.0, s));
                    }
                    let k = px(5.0, s);
                    let (cx, cy) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
                    let col = if hot { p.on_acc } else { p.dim };
                    let w = px(1.6, s).max(1);
                    line(hdc, col, w, (cx - k, cy - k), (cx + k + 1, cy + k + 1));
                    line(hdc, col, w, (cx - k, cy + k), (cx + k + 1, cy - k - 1));
                }
                Btn::Save | Btn::Pick => {
                    round_fill(hdc, &r, if hot { n.br.acc_hot } else { n.br.acc }, px(12.0, s));
                    let label = if b == Btn::Save { "Сохранить" } else { "Выбрать папку…" };
                    text_at(hdc, n.f_btn, p.on_acc, label, r, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
                }
            }
        }
    });
    let _ = EndPaint(hwnd, &ps);
}

unsafe fn draw_item(di: &DRAWITEMSTRUCT) {
    let Some((text, f_text, br_acc, br_bg2, fg, on_acc)) = with(|n| {
        let text = n.items.get(di.itemID as usize).map(|i| i.display.clone());
        (text, n.f_text, n.br.acc, n.br.bg2, n.pal.fg, n.pal.on_acc)
    }) else {
        return;
    };
    let Some(text) = text else { return };
    let selected = di.itemState.0 & 1 != 0; // ODS_SELECTED
    let brush = if selected { br_acc } else { br_bg2 };
    FillRect(di.hDC, &di.rcItem, HBRUSH(brush as *mut _));
    SetBkMode(di.hDC, TRANSPARENT);
    SetTextColor(di.hDC, rgb(if selected { on_acc } else { fg }));
    let old = SelectObject(di.hDC, HGDIOBJ(f_text as *mut _));
    let mut t: Vec<u16> = text.encode_utf16().collect();
    let mut rc = di.rcItem;
    rc.left += 8;
    DrawTextW(di.hDC, &mut t, &mut rc, DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX);
    SelectObject(di.hDC, old);
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
            let changed = with(|n| {
                let now = hit(n, x, y);
                (now != n.hover).then(|| {
                    let old = std::mem::replace(&mut n.hover, now);
                    (old, now, [old, now].map(|b| b.map(|b| btn_rect(n, b))))
                })
            })
            .flatten();
            if let Some((_, _, rects)) = changed {
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
            // фокус на самом окне (поле скрыто, пока не выбрана папка)
            if wparam.0 as u16 == VK_ESCAPE.0 {
                close();
            } else if wparam.0 as u16 == VK_RETURN.0 && with(|n| !n.vault_ok).unwrap_or(false) {
                pick_vault();
            }
            LRESULT(0)
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC | WM_CTLCOLORLISTBOX => {
            let hdc = HDC(wparam.0 as *mut _);
            let Some((fg, bg, br)) = with(|n| {
                (n.pal.fg, n.pal.bg2, n.br.bg2) // и список, и поле ввода — на фоне карточки
            }) else {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            };
            SetTextColor(hdc, rgb(fg));
            SetBkColor(hdc, rgb(bg));
            LRESULT(br)
        }
        WM_MEASUREITEM => {
            let mi = &mut *(lparam.0 as *mut MEASUREITEMSTRUCT);
            mi.itemHeight = with(|n| n.item_h).unwrap_or(20) as u32;
            LRESULT(1)
        }
        WM_DRAWITEM => {
            draw_item(&*(lparam.0 as *const DRAWITEMSTRUCT));
            LRESULT(1)
        }
        WM_NOTE_ACCEPT => {
            accept();
            if let Some((hw, e)) = with(|n| (n.hwnd, n.edit)) {
                take_focus(h(hw), h(e));
            }
            LRESULT(0)
        }
        WM_NOTE_INDEX => {
            let got = with(|n| {
                let idx = n.index_rx.as_ref()?.try_recv().ok()?;
                if !n.saved {
                    n.title = notes::ring_name(idx.max_ring + 1);
                }
                n.index = Some(idx);
                n.index_rx = None;
                Some(())
            })
            .flatten();
            if got.is_some() {
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
            if let Some(n) = NOTE.with(|s| s.take()) {
                n.br.free();
                for f in [n.f_title, n.f_text, n.f_small, n.f_btn] {
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
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, GetDIBits, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, DIB_RGB_COLORS,
    };
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

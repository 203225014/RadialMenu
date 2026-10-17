//! Окно быстрой заметки для Obsidian на «родных» контролах Win32.
//!
//! Пишешь текст → Ctrl+Enter → Ring_NN.md в корне хранилища. Esc — отмена. Набрал `[[` —
//! список заметок (фильтруется по набору), стрелки/Enter/Tab подставляют ссылку,
//! `[[` автоматически закрывается `]]`. Окно немодальное и живёт, только пока открыто.
//! Что именно править в тексте, решает `notes::editor`; здесь — только окно.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, GetDC,
    GetTextExtentPoint32W, GetTextMetricsW, InvalidateRect, ReleaseDC, SelectObject, SetBkColor,
    SetBkMode, SetTextColor, TextOutW, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_QUALITY,
    DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_BOLD, FW_NORMAL, HBRUSH, HDC, HFONT,
    HGDIOBJ, OUT_DEFAULT_PRECIS, PAINTSTRUCT, TEXTMETRICW, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_TAB, VK_UP,
};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetCursorPos,
    GetWindowRect, GetWindowTextLengthW, GetWindowTextW, IsWindow, IsWindowVisible, KillTimer,
    LoadCursorW, PostMessageW, RegisterClassW, SendMessageW, SetLayeredWindowAttributes, SetTimer,
    SetWindowPos, ShowWindow, ES_AUTOVSCROLL, ES_MULTILINE, ES_WANTRETURN, HTCAPTION, HTCLIENT,
    HWND_TOP, IDC_ARROW, LBS_NOINTEGRALHEIGHT, LBS_NOTIFY, LBS_OWNERDRAWFIXED, LB_ADDSTRING,
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
use crate::platform::win32::system::util::{h, log_error, monitor_at, pcwstr, rgb, ui_scale, wide};
use crate::render::palette::{self, Ui};

const WM_NOTE_INDEX: u32 = WM_APP + 20;
const WM_NOTE_SUGGEST: u32 = WM_APP + 21;
const WM_NOTE_ACCEPT: u32 = WM_APP + 22;
const TIMER_CLOSE: usize = 1;

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
}

impl Brushes {
    unsafe fn new(p: &Ui) -> Brushes {
        Brushes {
            bg: CreateSolidBrush(rgb(p.bg)).0 as isize,
            bg2: CreateSolidBrush(rgb(p.bg2)).0 as isize,
            edge: CreateSolidBrush(rgb(p.edge)).0 as isize,
            acc: CreateSolidBrush(rgb(p.acc)).0 as isize,
        }
    }

    unsafe fn free(&self) {
        for b in [self.bg, self.bg2, self.edge, self.acc] {
            let _ = DeleteObject(HGDIOBJ(b as *mut _));
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TitleTone {
    Accent,
    Ok,
    Bad,
}

struct Note {
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
    // геометрия, px
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
pub fn open(vault: &str, theme: Theme) {
    let existing = with(|n| (n.hwnd, n.edit)).filter(|&(_, e)| unsafe { IsWindow(Some(h(e))).as_bool() });
    if let Some((hwnd, edit)) = existing {
        take_focus(h(hwnd), h(edit));
        return;
    }
    unsafe {
        if let Err(e) = create(vault, theme) {
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

unsafe fn create(vault: &str, theme: Theme) -> windows::core::Result<()> {
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

    let (f_title, f_text, f_small) = (font(11.0, true, s), font(12.0, false, s), font(9.0, false, s));
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
    let (ww, wh) = (px(500.0, s), px(270.0, s));
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
    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 240, LWA_ALPHA); // как «прозрачность 0.94»

    let (pad, head, foot) = (px(15.0, s), px(38.0, s), px(26.0, s));
    let edit_text = if vault_ok {
        String::new()
    } else if vault.is_empty() {
        "Папка хранилища не задана.\r\nУкажите её в «Настройках» (значок в трее).".to_string()
    } else {
        format!("Папка хранилища не найдена:\r\n{vault}\r\nПроверьте путь в «Настройках».")
    };
    let et = wide(&edit_text);
    let edit = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        w!("EDIT"),
        pcwstr(&et),
        WS_CHILD
            | WS_VISIBLE
            | WINDOW_STYLE(ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | ES_WANTRETURN as u32),
        pad,
        1 + head,
        ww - 2 * pad,
        wh - 2 - head - foot,
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
    if !vault_ok {
        SendMessageW(edit, EM_SETREADONLY, Some(WPARAM(1)), None);
    }
    let _ = SetWindowSubclass(edit, Some(edit_proc), 1, 0);
    let _ = SetWindowSubclass(list, Some(list_proc), 2, 0);

    let index_rx = vault_ok.then(|| {
        // обход большого хранилища — не на потоке интерфейса
        let (tx, rx) = channel();
        let (hw, path) = (hwnd.0 as isize, vault_path.clone());
        std::thread::spawn(move || {
            if tx.send(notes::scan(&path)).is_ok() {
                unsafe {
                    let _ = PostMessageW(Some(h(hw)), WM_NOTE_INDEX, WPARAM(0), LPARAM(0));
                }
            }
        });
        rx
    });

    let pal = palette::ui(theme);
    NOTE.with(|slot| {
        slot.set(Note {
            hwnd: hwnd.0 as isize,
            edit: edit.0 as isize,
            list: list.0 as isize,
            vault: vault_path,
            vault_ok,
            index: None,
            index_rx,
            items: Vec::new(),
            title: if vault_ok { "Ring_…".into() } else { "Нет хранилища".into() },
            tone: if vault_ok { TitleTone::Accent } else { TitleTone::Bad },
            saved: false,
            swallow_char: false,
            br: Brushes::new(&pal),
            pal,
            f_title: f_title.0 as isize,
            f_text: f_text.0 as isize,
            f_small: f_small.0 as isize,
            scale: s,
            item_h,
            list_w,
            line_h,
        })
    });

    let _ = ShowWindow(hwnd, SW_SHOW);
    take_focus(hwnd, edit);
    Ok(())
}

fn close() {
    if let Some(hw) = with(|n| n.hwnd) {
        unsafe {
            let _ = DestroyWindow(h(hw));
        }
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

unsafe fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc: HDC = BeginPaint(hwnd, &mut ps);
    let st = with(|n| {
        let tone = match n.tone {
            TitleTone::Accent => n.pal.acc,
            TitleTone::Ok => n.pal.ok,
            TitleTone::Bad => n.pal.bad,
        };
        (n.br.edge, n.br.bg, n.f_title, n.f_small, n.title.clone(), tone, n.pal.dim, n.scale)
    });
    if let Some((br_edge, br_bg, f_title, f_small, title, tcolor, dim, s)) = st {
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        FillRect(hdc, &rc, HBRUSH(br_edge as *mut _));
        let inner = RECT {
            left: rc.left + 1,
            top: rc.top + 1,
            right: rc.right - 1,
            bottom: rc.bottom - 1,
        };
        FillRect(hdc, &inner, HBRUSH(br_bg as *mut _));
        SetBkMode(hdc, TRANSPARENT);

        let old = SelectObject(hdc, HGDIOBJ(f_title as *mut _));
        SetTextColor(hdc, rgb(tcolor));
        let (x, y) = (px(15.0, s), px(10.0, s));
        let t = title.encode_utf16().collect::<Vec<u16>>();
        let _ = TextOutW(hdc, x, y, &t);
        let mut sz = SIZE::default();
        let _ = GetTextExtentPoint32W(hdc, &t, &mut sz);

        SelectObject(hdc, HGDIOBJ(f_small as *mut _));
        SetTextColor(hdc, rgb(dim));
        let sub = "новая заметка".encode_utf16().collect::<Vec<u16>>();
        let _ = TextOutW(hdc, x + sz.cx + px(8.0, s), y + px(3.0, s), &sub);
        let mut foot = "Ctrl+Enter — сохранить    Esc — отмена    [[ — ссылка на заметку"
            .encode_utf16()
            .collect::<Vec<u16>>();
        let mut fr = RECT {
            left: px(15.0, s),
            top: rc.bottom - px(26.0, s),
            right: rc.right - px(10.0, s),
            bottom: rc.bottom - 2,
        };
        DrawTextW(hdc, &mut foot, &mut fr, DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX);
        SelectObject(hdc, old);
    }
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
            // шапку можно тянуть, как заголовок окна
            let r = DefWindowProcW(hwnd, msg, wparam, lparam);
            if r.0 as u32 == HTCLIENT {
                let y = (lparam.0 >> 16) as i16 as i32;
                let mut wr = RECT::default();
                let _ = GetWindowRect(hwnd, &mut wr);
                let s = with(|n| n.scale).unwrap_or(1.0);
                if y - wr.top < px(36.0, s) {
                    return LRESULT(HTCAPTION as isize);
                }
            }
            r
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC | WM_CTLCOLORLISTBOX => {
            let hdc = HDC(wparam.0 as *mut _);
            let list = msg == WM_CTLCOLORLISTBOX;
            let Some((fg, bg, br)) = with(|n| {
                if list {
                    (n.pal.fg, n.pal.bg2, n.br.bg2)
                } else {
                    (n.pal.fg, n.pal.bg, n.br.bg)
                }
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
                for f in [n.f_title, n.f_text, n.f_small] {
                    let _ = DeleteObject(HGDIOBJ(f as *mut _));
                }
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

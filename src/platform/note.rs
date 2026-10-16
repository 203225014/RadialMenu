//! Окно быстрой заметки для Obsidian на «родных» контролах Win32.
//!
//! Пишешь текст → Ctrl+Enter → Ring_NN.md в корне хранилища. Esc — отмена. Набрал `[[` —
//! список заметок (фильтруется по набору), стрелки/Enter/Tab подставляют ссылку,
//! `[[` автоматически закрывается `]]`. Окно живёт только пока открыто.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Mutex;

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
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CallWindowProcW, CreateWindowExW, DefWindowProcW, DestroyWindow,
    GetCursorPos, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindow, IsWindowVisible, KillTimer, LoadCursorW, PostMessageW,
    RegisterClassW, SendMessageW, SetForegroundWindow, SetLayeredWindowAttributes, SetTimer,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, ES_AUTOVSCROLL, ES_MULTILINE, ES_WANTRETURN,
    GWLP_WNDPROC, HTCAPTION, HTCLIENT, HWND_TOP, IDC_ARROW, LBS_NOINTEGRALHEIGHT, LBS_NOTIFY,
    LBS_OWNERDRAWFIXED, LB_ADDSTRING, LB_GETCOUNT, LB_GETCURSEL, LB_RESETCONTENT, LB_SETCURSEL,
    LB_SETTOPINDEX, LWA_ALPHA, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WM_CHAR, WM_CTLCOLOREDIT, WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC,
    WM_DESTROY, WM_DRAWITEM, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONUP, WM_MEASUREITEM, WM_NCHITTEST,
    WM_PAINT, WM_SETFONT, WM_TIMER, WNDCLASSW, WNDPROC, WS_BORDER, WS_CHILD, WS_CLIPCHILDREN,
    WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

use super::util::{monitor_at, pcwstr, ui_scale, wide};
use crate::notes::{self, NoteRef, VaultIndex};

// Палитра (как в прежней версии).
const BG: u32 = 0x1b1f2b;
const BG2: u32 = 0x262c3d;
const EDGE: u32 = 0x8fa6d6;
const FG: u32 = 0xffffff;
const DIM: u32 = 0x8791a8;
const ACC: u32 = 0x699bff;
const OK: u32 = 0x7fe0a3;
const BAD: u32 = 0xff8a8a;

const WM_NOTE_INDEX: u32 = WM_APP + 20;
const WM_NOTE_SUGGEST: u32 = WM_APP + 21;
const WM_NOTE_ACCEPT: u32 = WM_APP + 22;
const TIMER_CLOSE: usize = 1;

// Сообщения Edit/ListBox (в windows-rs они разбросаны по модулям — проще держать числа).
const EM_GETSEL: u32 = 0x00B0;
const EM_SETSEL: u32 = 0x00B1;
const EM_REPLACESEL: u32 = 0x00C2;
const EM_SETREADONLY: u32 = 0x00CF;
const EM_POSFROMCHAR: u32 = 0x00D6;

fn rgb(hex: u32) -> COLORREF {
    COLORREF(((hex & 0xFF) << 16) | (hex & 0xFF00) | ((hex >> 16) & 0xFF))
}

fn h(hwnd: isize) -> HWND {
    HWND(hwnd as *mut _)
}

struct Note {
    hwnd: isize,
    edit: isize,
    list: isize,
    vault: PathBuf,
    vault_ok: bool,
    index: Option<VaultIndex>,
    items: Vec<NoteRef>,
    title: String,
    title_color: u32,
    saved: bool,
    swallow_char: bool,
    // ресурсы GDI
    f_title: isize,
    f_text: isize,
    f_small: isize,
    br_bg: isize,
    br_bg2: isize,
    br_edge: isize,
    // геометрия, px
    scale: f32,
    item_h: i32,
    list_w: i32,
    line_h: i32,
}

thread_local! {
    static NOTE: RefCell<Option<Note>> = const { RefCell::new(None) };
}

/// Короткий доступ к состоянию. Внутри — никаких вызовов Win32, которые шлют сообщения окну.
fn with<R>(f: impl FnOnce(&mut Note) -> R) -> Option<R> {
    NOTE.with(|n| n.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

static INDEX_SLOT: Mutex<Option<(isize, VaultIndex)>> = Mutex::new(None);
static EDIT_PROC: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
static LIST_PROC: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

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
pub fn open(vault: &str) {
    let existing = with(|n| n.edit).filter(|&e| unsafe { IsWindow(Some(h(e))).as_bool() });
    if let Some(edit) = existing {
        focus(with(|n| n.hwnd).unwrap_or(0), edit);
        return;
    }
    unsafe {
        if let Err(e) = create(vault) {
            super::util::log_error(&format!("окно заметки: {e}"));
        }
    }
}

unsafe fn create(vault: &str) -> windows::core::Result<()> {
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

    let (f_title, f_text, f_small) = (
        font(11.0, true, s),
        font(12.0, false, s),
        font(9.0, false, s),
    );
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
    let vault_ok = vault_path.is_dir();
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
    } else {
        format!("Папка хранилища не найдена:\r\n{vault}")
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
    SendMessageW(
        edit,
        WM_SETFONT,
        Some(WPARAM(f_text.0 as usize)),
        Some(LPARAM(1)),
    );
    SendMessageW(
        list,
        WM_SETFONT,
        Some(WPARAM(f_text.0 as usize)),
        Some(LPARAM(1)),
    );
    if !vault_ok {
        SendMessageW(edit, EM_SETREADONLY, Some(WPARAM(1)), None);
    }
    let old = SetWindowLongPtrW(edit, GWLP_WNDPROC, edit_proc as *const () as isize);
    EDIT_PROC.store(old, std::sync::atomic::Ordering::Relaxed);
    let old = SetWindowLongPtrW(list, GWLP_WNDPROC, list_proc as *const () as isize);
    LIST_PROC.store(old, std::sync::atomic::Ordering::Relaxed);

    NOTE.with(|n| {
        *n.borrow_mut() = Some(Note {
            hwnd: hwnd.0 as isize,
            edit: edit.0 as isize,
            list: list.0 as isize,
            vault: vault_path.clone(),
            vault_ok,
            index: None,
            items: Vec::new(),
            title: if vault_ok {
                "Ring_…".into()
            } else {
                "Нет хранилища".into()
            },
            title_color: ACC,
            saved: false,
            swallow_char: false,
            f_title: f_title.0 as isize,
            f_text: f_text.0 as isize,
            f_small: f_small.0 as isize,
            br_bg: CreateSolidBrush(rgb(BG)).0 as isize,
            br_bg2: CreateSolidBrush(rgb(BG2)).0 as isize,
            br_edge: CreateSolidBrush(rgb(EDGE)).0 as isize,
            scale: s,
            item_h,
            list_w,
            line_h,
        });
    });

    let _ = ShowWindow(hwnd, SW_SHOW);
    focus(hwnd.0 as isize, edit.0 as isize);

    if vault_ok {
        // обход большого хранилища — не на потоке интерфейса
        let hw = hwnd.0 as isize;
        std::thread::spawn(move || {
            let idx = notes::scan(&vault_path);
            if let Ok(mut slot) = INDEX_SLOT.lock() {
                *slot = Some((hw, idx));
            }
            unsafe {
                let _ = PostMessageW(Some(h(hw)), WM_NOTE_INDEX, WPARAM(0), LPARAM(0));
            }
        });
    }
    Ok(())
}

/// Отнять клавиатурный фокус у текущего окна (Windows не любит, когда фокус «крадут»).
fn focus(hwnd: isize, edit: isize) {
    unsafe {
        let (hw, ed) = (h(hwnd), h(edit));
        let fg = GetForegroundWindow();
        let cur = GetCurrentThreadId();
        let fg_thread = GetWindowThreadProcessId(fg, None);
        let attached =
            fg_thread != 0 && fg_thread != cur && AttachThreadInput(fg_thread, cur, true).as_bool();
        let _ = SetForegroundWindow(hw);
        let _ = BringWindowToTop(hw);
        let _ = SetFocus(Some(ed));
        if attached {
            let _ = AttachThreadInput(fg_thread, cur, false);
        }
    }
}

fn close() {
    if let Some(hw) = with(|n| n.hwnd) {
        unsafe {
            let _ = DestroyWindow(h(hw));
        }
    }
}

// ---------------------------------------------------------------- текст

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

unsafe fn replace_selection(edit: HWND, text: &str) {
    let t = wide(text);
    SendMessageW(
        edit,
        EM_REPLACESEL,
        Some(WPARAM(1)),
        Some(LPARAM(t.as_ptr() as isize)),
    );
}

/// Текст текущей строки до курсора и позиция начала этой строки (в UTF-16).
unsafe fn line_before_caret(edit: HWND) -> (String, usize) {
    let buf = edit_text(edit);
    let (caret, _) = selection(edit);
    let caret = caret.min(buf.len());
    let start = buf[..caret]
        .iter()
        .rposition(|&c| c == 0x0A || c == 0x0D)
        .map_or(0, |p| p + 1);
    (String::from_utf16_lossy(&buf[start..caret]), start)
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
    let (before, _) = line_before_caret(h(edit));
    let Some(qpos) = notes::link_query(&before) else {
        return hide_list();
    };
    let query = before[qpos..].to_string();
    let items: Vec<NoteRef> = with(|n| match &n.index {
        Some(idx) => notes::suggest(idx, &query, 80)
            .into_iter()
            .cloned()
            .collect(),
        None => Vec::new(),
    })
    .unwrap_or_default();
    if items.is_empty() {
        return hide_list();
    }
    let (item_h, list_w, line_h, scale, parent) =
        with(|n| (n.item_h, n.list_w, n.line_h, n.scale, n.hwnd)).unwrap();
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
    let (caret, _) = selection(h(edit));
    let pos = SendMessageW(h(edit), EM_POSFROMCHAR, Some(WPARAM(caret)), None).0 as u32;
    let (cx, cy) = ((pos & 0xFFFF) as i16 as i32, (pos >> 16) as i16 as i32);
    let mut er = RECT::default();
    let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(h(edit), &mut er);
    let mut pr = RECT::default();
    let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(h(parent), &mut pr);
    let (ex, ey) = (er.left - pr.left, er.top - pr.top);
    let (bw, bh) = (pr.right - pr.left - 2, pr.bottom - pr.top - 2);
    let pad = px(4.0, scale);
    let x = (ex + cx).clamp(pad, (bw - list_w - pad).max(pad));
    let mut y = ey + cy + line_h + 2;
    if y + list_h > bh - pad {
        y = (ey + cy - list_h - 2).max(pad);
    }
    let _ = SetWindowPos(
        lb,
        Some(HWND_TOP),
        x,
        y,
        list_w,
        list_h,
        SWP_SHOWWINDOW | SWP_NOACTIVATE,
    );
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
    let Some(link) = link.filter(|_| sel >= 0) else {
        return hide_list();
    };
    let ed = h(edit);
    let (before, line_start) = line_before_caret(ed);
    let Some(qpos) = notes::link_query(&before) else {
        return hide_list();
    };
    let qstart = line_start + before[..qpos].encode_utf16().count();
    let (caret, _) = selection(ed);
    set_selection(ed, qstart, caret);
    replace_selection(ed, &link);
    let buf = edit_text(ed);
    let (pos, _) = selection(ed);
    if buf.get(pos) == Some(&(']' as u16)) && buf.get(pos + 1) == Some(&(']' as u16)) {
        set_selection(ed, pos + 2, pos + 2);
    } else {
        replace_selection(ed, "]]");
    }
    hide_list();
}

// ---------------------------------------------------------------- сохранение

unsafe fn save() {
    let Some((edit, ok, vault, ring)) = with(|n| {
        (
            n.edit,
            n.vault_ok,
            n.vault.clone(),
            n.index.as_ref().map(|i| i.max_ring),
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
    // индекс ещё не готов — номер узнаём прямо сейчас
    let ring = ring.unwrap_or_else(|| notes::scan(&vault).max_ring);
    let result = notes::save(&vault, &text, ring + 1);
    hide_list();
    SendMessageW(h(edit), EM_SETREADONLY, Some(WPARAM(1)), None);
    let hwnd = with(|n| {
        n.saved = true;
        match &result {
            Ok(name) => {
                n.title = format!("Сохранено: {name}");
                n.title_color = OK;
            }
            Err(e) => {
                super::util::log_error(&format!("сохранение заметки: {e}"));
                n.title = "Не удалось сохранить".into();
                n.title_color = BAD;
            }
        }
        n.hwnd
    });
    if let Some(hw) = hwnd {
        let _ = InvalidateRect(Some(h(hw)), None, false);
        SetTimer(Some(h(hw)), TIMER_CLOSE, 650, None);
    }
}

// ---------------------------------------------------------------- процедура поля ввода

unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let prev: WNDPROC =
        std::mem::transmute::<isize, WNDPROC>(EDIT_PROC.load(std::sync::atomic::Ordering::Relaxed));
    let list_visible = with(|n| IsWindowVisible(h(n.list)).as_bool()).unwrap_or(false);
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
                // [[ → [[]] с курсором внутри
                let buf = edit_text(hwnd);
                let (caret, end) = selection(hwnd);
                if caret == end && caret > 0 && buf.get(caret - 1) == Some(&('[' as u16)) {
                    replace_selection(hwnd, "[]]");
                    let (pos, _) = selection(hwnd);
                    set_selection(hwnd, pos - 2, pos - 2);
                    parent_msg(WM_NOTE_SUGGEST);
                    return LRESULT(0);
                }
            }
        }
        WM_KEYUP => {
            let vk = wparam.0 as u16;
            if ![
                VK_UP.0,
                VK_DOWN.0,
                VK_RETURN.0,
                VK_TAB.0,
                VK_ESCAPE.0,
                0x10,
                0x11,
            ]
            .contains(&vk)
            {
                parent_msg(WM_NOTE_SUGGEST);
            }
        }
        WM_LBUTTONUP => parent_msg(WM_NOTE_SUGGEST),
        _ => {}
    }
    CallWindowProcW(prev, hwnd, msg, wparam, lparam)
}

/// Клик по подсказке подставляет её (LBN_SELCHANGE не приходит, если кликнули по уже выбранной).
unsafe extern "system" fn list_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let prev: WNDPROC =
        std::mem::transmute::<isize, WNDPROC>(LIST_PROC.load(std::sync::atomic::Ordering::Relaxed));
    let r = CallWindowProcW(prev, hwnd, msg, wparam, lparam);
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
        (
            n.br_edge,
            n.br_bg,
            n.f_title,
            n.f_small,
            n.title.clone(),
            n.title_color,
            n.scale,
        )
    });
    if let Some((br_edge, br_bg, f_title, f_small, title, tcolor, s)) = st {
        let mut rc = RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rc);
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
        SetTextColor(hdc, rgb(DIM));
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
        DrawTextW(
            hdc,
            &mut foot,
            &mut fr,
            DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
        );
        SelectObject(hdc, old);
    }
    let _ = EndPaint(hwnd, &ps);
}

unsafe fn draw_item(di: &DRAWITEMSTRUCT) {
    let Some((text, f_text, br_acc, br_bg2)) = with(|n| {
        let text = n.items.get(di.itemID as usize).map(|i| i.display.clone());
        (text, n.f_text, 0isize, n.br_bg2)
    })
    .and_then(|(t, f, a, b)| t.map(|t| (t, f, a, b))) else {
        return;
    };
    let _ = br_acc;
    let selected = di.itemState.0 & 1 != 0; // ODS_SELECTED
    let brush = if selected {
        CreateSolidBrush(rgb(ACC))
    } else {
        HBRUSH(br_bg2 as *mut _)
    };
    FillRect(di.hDC, &di.rcItem, brush);
    if selected {
        let _ = DeleteObject(brush.into());
    }
    SetBkMode(di.hDC, TRANSPARENT);
    SetTextColor(di.hDC, rgb(FG));
    let old = SelectObject(di.hDC, HGDIOBJ(f_text as *mut _));
    let mut t: Vec<u16> = text.encode_utf16().collect();
    let mut rc = di.rcItem;
    rc.left += 8;
    DrawTextW(
        di.hDC,
        &mut t,
        &mut rc,
        DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
    );
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
                let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut wr);
                let s = with(|n| n.scale).unwrap_or(1.0);
                if y - wr.top < px(36.0, s) {
                    return LRESULT(HTCAPTION as isize);
                }
            }
            r
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC => {
            let hdc = HDC(wparam.0 as *mut _);
            SetTextColor(hdc, rgb(FG));
            SetBkColor(hdc, rgb(BG));
            LRESULT(with(|n| n.br_bg).unwrap_or(0))
        }
        WM_CTLCOLORLISTBOX => {
            let hdc = HDC(wparam.0 as *mut _);
            SetTextColor(hdc, rgb(FG));
            SetBkColor(hdc, rgb(BG2));
            LRESULT(with(|n| n.br_bg2).unwrap_or(0))
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
            if let Some(e) = with(|n| n.edit) {
                let _ = SetFocus(Some(h(e)));
            }
            LRESULT(0)
        }
        WM_NOTE_INDEX => {
            let idx = INDEX_SLOT.lock().ok().and_then(|mut s| s.take());
            if let Some((owner, index)) = idx {
                if owner == hwnd.0 as isize {
                    with(|n| {
                        if !n.saved {
                            n.title = notes::ring_name(index.max_ring + 1);
                        }
                        n.index = Some(index);
                    });
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    suggest_update(); // пока ждали, могли уже набрать [[
                }
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
            if let Some(n) = NOTE.with(|n| n.borrow_mut().take()) {
                for f in [n.f_title, n.f_text, n.f_small, n.br_bg, n.br_bg2, n.br_edge] {
                    let _ = DeleteObject(HGDIOBJ(f as *mut _));
                }
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

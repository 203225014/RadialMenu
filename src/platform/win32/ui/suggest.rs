//! Окно подсказок `[[`: отдельное окно без рамки поверх заметки, не берёт фокус (набор текста
//! продолжается в поле). Картинку со скруглением и тенью рисует `ui::suggest_view`, сюда она
//! выводится попиксельно (UpdateLayeredWindow). Выбор мышью — сообщение `WM_SUGGEST_PICK` владельцу.

use std::ffi::c_void;

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, PostMessageW, RegisterClassW, ShowWindow,
    UpdateLayeredWindow, SW_HIDE, SW_SHOWNA, ULW_ALPHA, WM_APP, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

use crate::platform::win32::slot::Slot;
use crate::platform::win32::system::util::{h, monitor_at};
use crate::render::palette::Ui;
use crate::ui::settings_form::Fonts;
use crate::ui::suggest_view::{self as view, Geometry};

/// Владельцу: кликнули по подсказке (в `wparam` — номер).
pub const WM_SUGGEST_PICK: u32 = WM_APP + 30;
const WM_MOUSELEAVE: u32 = 0x02A3;

struct Popup {
    hwnd: isize,
    owner: isize,
    items: Vec<String>,
    sel: usize,
    top: usize,
    hover: Option<usize>,
    scale: f32,
    pal: Ui,
    fonts: Option<Fonts>,
    geo: Geometry,
    pos: (i32, i32),
    shown: bool,
}

thread_local! {
    static POPUP: Slot<Popup> = const { Slot::new() };
}

fn with<R>(f: impl FnOnce(&mut Popup) -> R) -> Option<R> {
    POPUP.with(|s| s.with(f))
}

pub fn visible() -> bool {
    with(|p| p.shown).unwrap_or(false)
}

pub fn selected() -> Option<usize> {
    with(|p| p.shown.then_some(p.sel)).flatten()
}

/// Показать (или обновить) подсказки. `line` — (x, верх, низ) строки с курсором в координатах
/// экрана: плашка встаёт под ней, а если места нет — над ней.
pub fn show(owner: HWND, items: Vec<String>, line: (i32, i32, i32), pal: Ui, scale: f32) {
    if items.is_empty() {
        return hide();
    }
    unsafe {
        if with(|_| ()).is_none() && create(owner, pal, scale).is_err() {
            return;
        }
    }
    with(|p| {
        let same = p.items == items;
        p.owner = owner.0 as isize;
        p.pal = pal;
        p.scale = scale;
        if !same {
            p.items = items;
            p.sel = 0;
            p.top = 0;
            p.hover = None;
        }
        p.geo = view::geometry(p.items.len(), scale);
        let (x, top, bottom) = line;
        let g = p.geo;
        let mon = monitor_at(POINT { x, y: top });
        let (mx, my, mw, mh) = mon;
        let (px, py, pw, ph) = (g.panel.0 as i32, g.panel.1 as i32, g.panel.2 as i32, g.panel.3 as i32);
        let mut wx = (x - px).clamp(mx - px, (mx + mw - px - pw - 8).max(mx - px));
        let below = bottom + 4;
        let mut wy = below - py;
        if below + ph > my + mh - 8 {
            wy = top - 4 - ph - py; // над строкой
        }
        wy = wy.max(my - py);
        wx = wx.max(mx - px);
        p.pos = (wx, wy);
        unsafe { present(p) };
    });
}

pub fn hide() {
    let hw = with(|p| {
        p.shown = false;
        p.hwnd
    });
    if let Some(hw) = hw {
        unsafe {
            let _ = ShowWindow(h(hw), SW_HIDE);
        }
    }
}

/// Сдвинуть выбор (стрелки в поле ввода) и прокрутить так, чтобы он был виден.
pub fn move_sel(d: i32) {
    with(|p| {
        if p.items.is_empty() {
            return;
        }
        let last = p.items.len() as i32 - 1;
        p.sel = (p.sel as i32 + d).clamp(0, last) as usize;
        scroll_to_sel(p);
        unsafe { present(p) };
    });
}

pub fn set_theme(pal: Ui) {
    with(|p| {
        p.pal = pal;
        if p.shown {
            unsafe { present(p) };
        }
    });
}

pub fn destroy() {
    if let Some(pp) = POPUP.with(|s| s.take()) {
        unsafe {
            let _ = DestroyWindow(h(pp.hwnd));
        }
    }
}

fn scroll_to_sel(p: &mut Popup) {
    if p.sel < p.top {
        p.top = p.sel;
    } else if p.sel >= p.top + view::MAX_ROWS {
        p.top = p.sel + 1 - view::MAX_ROWS;
    }
}

unsafe fn create(owner: HWND, pal: Ui, scale: f32) -> windows::core::Result<()> {
    let hinst = GetModuleHandleW(None)?;
    let class = w!("RadialMenuSuggest");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(proc),
        hInstance: hinst.into(),
        lpszClassName: class,
        ..Default::default()
    };
    RegisterClassW(&wc);
    let hwnd = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
        class,
        w!(""),
        WS_POPUP,
        0,
        0,
        10,
        10,
        Some(owner),
        None,
        Some(hinst.into()),
        None,
    )?;
    POPUP.with(|s| {
        s.set(Popup {
            hwnd: hwnd.0 as isize,
            owner: owner.0 as isize,
            items: Vec::new(),
            sel: 0,
            top: 0,
            hover: None,
            scale,
            pal,
            fonts: Fonts::load(),
            geo: view::geometry(0, scale),
            pos: (0, 0),
            shown: false,
        })
    });
    Ok(())
}

/// Нарисовать плашку и выложить её на экран.
unsafe fn present(p: &mut Popup) {
    let Some(fonts) = &p.fonts else { return };
    let Some(img) = view::render(&p.items, p.sel, p.hover, p.top, p.scale, &p.pal, fonts) else {
        return;
    };
    let (w, hgt) = (img.width() as i32, img.height() as i32);
    let screen = GetDC(None);
    let dc = CreateCompatibleDC(Some(screen));
    let bmi = BITMAPINFO {
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
    let mut bits: *mut c_void = std::ptr::null_mut();
    if let Ok(bmp) = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
        let old = SelectObject(dc, bmp.into());
        let out = std::slice::from_raw_parts_mut(bits as *mut u8, (w * hgt * 4) as usize);
        img.to_bgra(out); // предумноженная альфа, как требует ULW_ALPHA
        let dst = POINT { x: p.pos.0, y: p.pos.1 };
        let size = SIZE { cx: w, cy: hgt };
        let src = POINT::default();
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = UpdateLayeredWindow(
            h(p.hwnd),
            Some(screen),
            Some(&dst),
            Some(&size),
            Some(dc),
            Some(&src),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        SelectObject(dc, old);
        let _ = DeleteObject(bmp.into());
    }
    let _ = DeleteDC(dc);
    ReleaseDC(None, screen);
    if !p.shown {
        p.shown = true;
        let _ = ShowWindow(h(p.hwnd), SW_SHOWNA);
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let y = (lparam.0 >> 16) as i16 as f32;
    match msg {
        WM_MOUSEACTIVATE => LRESULT(3), // MA_NOACTIVATE: фокус остаётся в поле ввода
        WM_MOUSEMOVE => {
            let tracking = with(|p| {
                let row = view::row_at(&p.geo, p.scale, y, p.top, p.items.len());
                if row != p.hover {
                    p.hover = row;
                    present(p);
                    true
                } else {
                    false
                }
            })
            .unwrap_or(false);
            if tracking {
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
            with(|p| {
                if p.hover.take().is_some() {
                    present(p);
                }
            });
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = (wparam.0 >> 16) as i16 as i32;
            with(|p| {
                let max_top = p.items.len().saturating_sub(view::MAX_ROWS);
                let step = if delta > 0 { -1 } else { 1 };
                p.top = (p.top as i32 + step).clamp(0, max_top as i32) as usize;
                present(p);
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let pick = with(|p| {
                let row = view::row_at(&p.geo, p.scale, y, p.top, p.items.len())?;
                p.sel = row;
                Some((p.owner, row))
            })
            .flatten();
            if let Some((owner, row)) = pick {
                let _ = PostMessageW(Some(h(owner)), WM_SUGGEST_PICK, WPARAM(row), LPARAM(0));
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

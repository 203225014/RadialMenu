//! Скриншот окна под указателем: окно, над которым вызвали меню (а не активное, как у Alt+PrtScn),
//! снимается с экрана и кладётся в буфер обмена картинкой.

use std::ffi::c_void;

use windows::Win32::Foundation::{GlobalFree, HANDLE, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

const CF_DIB: u32 = 8;

/// Рамка окна под точкой `at` (экранные координаты): то, что видно на экране, без невидимых полей.
fn window_rect(at: (i32, i32)) -> Option<RECT> {
    unsafe {
        let (_, win) = super::pointer::windows_at(at)?;
        let mut r = RECT::default();
        let ok = DwmGetWindowAttribute(
            win,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut r as *mut RECT as *mut c_void,
            std::mem::size_of::<RECT>() as u32,
        )
        .is_ok();
        (ok || GetWindowRect(win, &mut r).is_ok()).then_some(r)
    }
}

/// Снять окно под указателем и положить в буфер обмена.
pub fn window_to_clipboard(at: (i32, i32)) -> Result<(), String> {
    let r = window_rect(at).ok_or("под указателем нет окна")?;
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    if w <= 0 || h <= 0 || w > 16_384 || h > 16_384 {
        return Err(format!("странный размер окна {w}×{h}"));
    }
    let pixels = grab(r, w, h)?;
    to_clipboard(&pixels, w, h)
}

/// Пиксели экрана в прямоугольнике `r`: BGRA, строки сверху вниз.
fn grab(r: RECT, w: i32, h: i32) -> Result<Vec<u8>, String> {
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let out = match CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bmp) if !bits.is_null() => {
                let old = SelectObject(dc, bmp.into());
                let res = BitBlt(dc, 0, 0, w, h, Some(screen), r.left, r.top, SRCCOPY)
                    .map(|_| std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize).to_vec())
                    .map_err(|e| format!("снимок экрана: {e}"));
                SelectObject(dc, old);
                let _ = DeleteObject(bmp.into());
                res
            }
            _ => Err("не создался буфер снимка".to_string()),
        };
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
        out
    }
}

/// Картинка (BGRA сверху вниз) в буфер обмена как DIB: строки снизу вверх, непрозрачная.
fn to_clipboard(bgra: &[u8], w: i32, h: i32) -> Result<(), String> {
    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w,
        biHeight: h, // снизу вверх
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        biSizeImage: (w * h * 4) as u32,
        ..Default::default()
    };
    let head_len = std::mem::size_of::<BITMAPINFOHEADER>();
    let stride = (w * 4) as usize;
    unsafe {
        let mem = GlobalAlloc(GMEM_MOVEABLE, head_len + bgra.len()).map_err(|e| format!("память: {e}"))?;
        let p = GlobalLock(mem) as *mut u8;
        if p.is_null() {
            let _ = GlobalFree(Some(mem));
            return Err("не заблокировалась память".into());
        }
        std::ptr::copy_nonoverlapping(&header as *const _ as *const u8, p, head_len);
        let body = std::slice::from_raw_parts_mut(p.add(head_len), bgra.len());
        for (row, src) in bgra.chunks_exact(stride).enumerate() {
            let dst = &mut body[(h as usize - 1 - row) * stride..][..stride];
            dst.copy_from_slice(src);
            dst.chunks_exact_mut(4).for_each(|px| px[3] = 255);
        }
        let _ = GlobalUnlock(mem);
        if OpenClipboard(None).is_err() {
            let _ = GlobalFree(Some(mem));
            return Err("буфер обмена занят".into());
        }
        let _ = EmptyClipboard();
        let set = SetClipboardData(CF_DIB, Some(HANDLE(mem.0)));
        let _ = CloseClipboard();
        if let Err(e) = set {
            let _ = GlobalFree(Some(mem)); // при успехе память принадлежит системе
            return Err(format!("буфер обмена: {e}"));
        }
    }
    Ok(())
}

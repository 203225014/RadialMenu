//! Полупрозрачное окно поверх всех: не берёт фокус, пропускает клики, рисуется
//! по пикселям (UpdateLayeredWindow, предумноженный BGRA).

use std::ffi::c_void;

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, AC_SRC_ALPHA,
    AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, SetWindowPos, ShowWindow,
    UpdateLayeredWindowIndirect, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE,
    SW_SHOWNOACTIVATE, ULW_ALPHA, UPDATELAYEREDWINDOWINFO, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::render::compose::Rect;

unsafe extern "system" fn layer_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

pub struct Layer {
    hwnd: HWND,
    dc: HDC,
    bmp: HBITMAP,
    bits: *mut u8,
    size: i32,
    pos: (i32, i32),
    visible: bool,
    last_alpha: Option<u8>,
}

impl Layer {
    pub fn new(size: i32) -> windows::core::Result<Layer> {
        unsafe {
            let hinst = GetModuleHandleW(None)?;
            let class = w!("RadialMenuLayer");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(layer_proc),
                hInstance: hinst.into(),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST
                    | WS_EX_NOACTIVATE
                    | WS_EX_TRANSPARENT,
                class,
                w!("RadialMenuLayer"),
                WS_POPUP,
                0,
                0,
                size,
                size,
                None,
                None,
                Some(hinst.into()),
                None,
            )?;
            let dc = CreateCompatibleDC(None);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: size,
                    biHeight: -size, // сверху вниз
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bmp = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
            SelectObject(dc, bmp.into());
            Ok(Layer {
                hwnd,
                dc,
                bmp,
                bits: bits as *mut u8,
                size,
                pos: (i32::MIN, i32::MIN),
                visible: false,
                last_alpha: None,
            })
        }
    }

    pub fn size(&self) -> i32 {
        self.size
    }

    pub fn pixels(&mut self) -> &mut [u8] {
        // DIB живёт, пока жив Layer; размер задан при создании
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.size * self.size * 4) as usize) }
    }

    pub fn clear(&mut self) {
        self.pixels().fill(0);
    }

    /// Куда поставить окно (левый верхний угол) при следующем показе/обновлении.
    pub fn move_to(&mut self, x: i32, y: i32) {
        self.pos = (x, y);
    }

    /// Показать содержимое буфера; `dirty` — что изменилось, `alpha` — общая прозрачность.
    pub fn present(&mut self, dirty: Rect, alpha: u8, moved: bool) {
        let full = Rect {
            x0: 0,
            y0: 0,
            x1: self.size,
            y1: self.size,
        };
        // при смене общей прозрачности обновляем окно целиком: она касается всех пикселей
        let whole = moved || self.last_alpha != Some(alpha);
        self.last_alpha = Some(alpha);
        let dirty = if whole {
            full
        } else {
            dirty.clip(self.size as usize, self.size as usize)
        };
        if dirty.is_empty() && self.visible {
            return;
        }
        let dirty = if dirty.is_empty() { full } else { dirty };
        unsafe {
            let pt_dst = POINT {
                x: self.pos.0,
                y: self.pos.1,
            };
            let size = SIZE {
                cx: self.size,
                cy: self.size,
            };
            let pt_src = POINT::default();
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: alpha,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let rc = RECT {
                left: dirty.x0,
                top: dirty.y0,
                right: dirty.x1,
                bottom: dirty.y1,
            };
            let info = UPDATELAYEREDWINDOWINFO {
                cbSize: std::mem::size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
                hdcDst: HDC::default(),
                pptDst: &pt_dst,
                psize: &size,
                hdcSrc: self.dc,
                pptSrc: &pt_src,
                crKey: COLORREF(0),
                pblend: &blend,
                dwFlags: ULW_ALPHA,
                prcDirty: &rc,
            };
            let _ = UpdateLayeredWindowIndirect(self.hwnd, &info);
        }
    }

    pub fn show(&mut self) {
        if !self.visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                let _ = SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
            self.visible = true;
        }
    }

    pub fn hide(&mut self) {
        self.last_alpha = None;
        if self.visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.visible = false;
        }
    }
}

impl Drop for Layer {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            let _ = DeleteDC(self.dc);
            let _ = DeleteObject(self.bmp.into());
        }
    }
}

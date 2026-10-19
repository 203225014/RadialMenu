//! Окно под указателем: действия меню относятся к окну, над которым меню вызвали, а не к активному.

use std::ffi::c_void;

use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetClassNameW, GetForegroundWindow, GetWindow, GetWindowLongPtrW, GetWindowRect, IsWindowVisible,
    PostMessageW, WindowFromPoint, GA_ROOT, GWL_EXSTYLE, GW_HWNDNEXT, WM_APPCOMMAND, WS_EX_TRANSPARENT,
};

const APPCOMMAND_BROWSER_BACKWARD: isize = 1;
const APPCOMMAND_BROWSER_FORWARD: isize = 2;
const FAPPCOMMAND_MOUSE: isize = 0x8000;

/// Невидимое, прозрачное для кликов или служебное окно — «окном под указателем» его считать нельзя
/// (например, `Shell_LightDismissOverlay`: Windows кладёт его на экран, пока открыта панель Win+V).
fn is_overlay(w: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(w).as_bool() {
            return true;
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(w, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut c_void, 4);
        if cloaked != 0 {
            return true;
        }
        if GetWindowLongPtrW(w, GWL_EXSTYLE) as u32 & WS_EX_TRANSPARENT.0 != 0 {
            return true;
        }
        let mut name = [0u16; 64];
        let n = GetClassNameW(w, &mut name).max(0) as usize;
        String::from_utf16_lossy(&name[..n]) == "Shell_LightDismissOverlay"
    }
}

/// Окно под точкой `at` и его окно верхнего уровня; служебные накладки пропускаются — берётся первое
/// настоящее окно под ними в порядке наложения.
pub fn windows_at(at: (i32, i32)) -> Option<(HWND, HWND)> {
    unsafe {
        let hit = WindowFromPoint(POINT { x: at.0, y: at.1 });
        if hit.is_invalid() {
            return None;
        }
        let root = GetAncestor(hit, GA_ROOT);
        let root = if root.is_invalid() { hit } else { root };
        if !is_overlay(root) {
            return Some((hit, root));
        }
        let mut cur = root;
        for _ in 0..200 {
            cur = GetWindow(cur, GW_HWNDNEXT).ok()?;
            let mut r = RECT::default();
            if !is_overlay(cur) && GetWindowRect(cur, &mut r).is_ok() && contains(&r, at) {
                return Some((cur, cur));
            }
        }
        None
    }
}

fn contains(r: &RECT, at: (i32, i32)) -> bool {
    at.0 >= r.left && at.0 < r.right && at.1 >= r.top && at.1 < r.bottom
}

/// «Назад» / «вперёд» для окна под указателем. Если это активное окно — `fallback` (Alt+←/→: так
/// работает везде); иначе окну шлётся команда «назад/вперёд» (её понимают браузеры, Проводник и др.),
/// фокус при этом не меняется.
pub fn navigate(at: (i32, i32), forward: bool, fallback: impl FnOnce()) {
    let Some((hit, root)) = windows_at(at) else {
        return fallback();
    };
    if unsafe { GetForegroundWindow() } == root {
        return fallback();
    }
    let cmd = if forward { APPCOMMAND_BROWSER_FORWARD } else { APPCOMMAND_BROWSER_BACKWARD };
    unsafe {
        let _ = PostMessageW(Some(hit), WM_APPCOMMAND, WPARAM(hit.0 as usize), LPARAM((cmd | FAPPCOMMAND_MOUSE) << 16));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Живая проверка: `cargo test live_windows_at -- --ignored --nocapture`, пока открыта панель Win+V
    /// (точка — центр второго монитора по первому аргументу окружения `RM_X`/`RM_Y`).
    #[test]
    #[ignore = "нужна открытая панель Win+V"]
    fn live_windows_at() {
        let at = (
            std::env::var("RM_X").ok().and_then(|v| v.parse().ok()).unwrap_or(0),
            std::env::var("RM_Y").ok().and_then(|v| v.parse().ok()).unwrap_or(0),
        );
        std::thread::sleep(std::time::Duration::from_secs(3));
        let (_, root) = windows_at(at).expect("окно найдено");
        let mut name = [0u16; 64];
        let n = unsafe { GetClassNameW(root, &mut name) } as usize;
        let mut r = RECT::default();
        unsafe {
            let _ = GetWindowRect(root, &mut r);
        }
        eprintln!("окно под точкой: {} {:?}", String::from_utf16_lossy(&name[..n]), (r.left, r.top, r.right, r.bottom));
        assert!(!is_overlay(root));
    }
}

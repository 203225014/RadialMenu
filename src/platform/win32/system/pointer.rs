//! Окно под указателем: действия меню относятся к окну, над которым меню вызвали, а не к активному.

use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetForegroundWindow, PostMessageW, WindowFromPoint, GA_ROOT, WM_APPCOMMAND,
};

const APPCOMMAND_BROWSER_BACKWARD: isize = 1;
const APPCOMMAND_BROWSER_FORWARD: isize = 2;
const FAPPCOMMAND_MOUSE: isize = 0x8000;

/// Окно под точкой `at` и его окно верхнего уровня.
pub fn windows_at(at: (i32, i32)) -> Option<(HWND, HWND)> {
    unsafe {
        let hit = WindowFromPoint(POINT { x: at.0, y: at.1 });
        if hit.is_invalid() {
            return None;
        }
        let root = GetAncestor(hit, GA_ROOT);
        Some((hit, if root.is_invalid() { hit } else { root }))
    }
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

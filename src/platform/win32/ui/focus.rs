//! Вывести своё окно на передний план с клавиатурным фокусом.

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
};

/// Отнять фокус у текущего окна (Windows не любит, когда фокус «крадут», поэтому
/// на время подключаемся к очереди ввода окна, у которого он сейчас).
pub fn take_focus(hwnd: HWND, focus: HWND) {
    unsafe {
        let fg = GetForegroundWindow();
        let cur = GetCurrentThreadId();
        let fg_thread = GetWindowThreadProcessId(fg, None);
        let attached =
            fg_thread != 0 && fg_thread != cur && AttachThreadInput(fg_thread, cur, true).as_bool();
        let _ = SetForegroundWindow(hwnd);
        let _ = BringWindowToTop(hwnd);
        let _ = SetFocus(Some(focus));
        if attached {
            let _ = AttachThreadInput(fg_thread, cur, false);
        }
    }
}

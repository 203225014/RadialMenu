//! «Не мешать в играх»: описание активного окна для правила из `model::games`.
//!
//! Вызывается из потока хуков — только в момент нажатия «своей» кнопки/клавиши и раз в тик
//! сторожа, поэтому лишней работы на обычных нажатиях нет. Имя exe кэшируется по pid.

use std::sync::{Mutex, RwLock};

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_RUNNING_D3D_FULL_SCREEN};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetDesktopWindow, GetForegroundWindow, GetShellWindow, GetWindowLongPtrW,
    GetWindowRect, GetWindowThreadProcessId, GWL_STYLE, WS_CAPTION,
};

use crate::model::{Foreground, GamePolicy};

static POLICY: RwLock<Option<GamePolicy>> = RwLock::new(None);
/// (pid, имя exe, когда узнали, мс): процесс активного окна меняется редко.
static EXE_CACHE: Mutex<Option<(u32, Option<String>, u64)>> = Mutex::new(None);
const EXE_TTL_MS: u64 = 5000;

pub fn set_policy(p: GamePolicy) {
    if let Ok(mut g) = POLICY.write() {
        *g = Some(p);
    }
}

/// Сейчас меню вызывать не нужно: активна игра или программа из списка.
pub fn bypass_now() -> bool {
    let Ok(guard) = POLICY.try_read() else {
        return false;
    };
    let Some(policy) = guard.as_ref().filter(|p| p.is_active()) else {
        return false;
    };
    policy.bypass(&foreground(policy.ignore_fullscreen))
}

fn exe_of(pid: u32) -> Option<String> {
    let now = unsafe { windows::Win32::System::SystemInformation::GetTickCount64() };
    if let Ok(mut cache) = EXE_CACHE.lock() {
        if let Some((p, exe, at)) = cache.as_ref() {
            if *p == pid && now.saturating_sub(*at) < EXE_TTL_MS {
                return exe.clone();
            }
        }
        let exe = query_exe(pid);
        *cache = Some((pid, exe.clone(), now));
        return exe;
    }
    query_exe(pid)
}

fn query_exe(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let ok =
            QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
                .is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return None;
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        Some(crate::model::games::normalize_name(&full))
    }
}

/// Рабочий стол и оболочка закрывают весь монитор, но игрой не являются.
unsafe fn is_shell(hwnd: HWND) -> bool {
    if hwnd == GetShellWindow() || hwnd == GetDesktopWindow() {
        return true;
    }
    let mut name = [0u16; 32];
    let n = GetClassNameW(hwnd, &mut name).max(0) as usize;
    matches!(
        String::from_utf16_lossy(&name[..n]).as_str(),
        "Progman" | "WorkerW"
    )
}

/// Окно без заголовка закрывает весь монитор. Развёрнутое обычное окно (с заголовком) — нет.
unsafe fn covers_monitor(hwnd: HWND) -> bool {
    let mut r = RECT::default();
    if GetWindowRect(hwnd, &mut r).is_err() {
        return false;
    }
    let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !GetMonitorInfoW(mon, &mut mi).as_bool() {
        return false;
    }
    let m = mi.rcMonitor;
    let covers = r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom;
    let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
    covers && style & WS_CAPTION.0 != WS_CAPTION.0
}

fn foreground(want_fullscreen: bool) -> Foreground {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Foreground::default();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let exe = if pid != 0 { exe_of(pid) } else { None };
        let (mut fullscreen, mut d3d_exclusive) = (false, false);
        if want_fullscreen && !is_shell(hwnd) {
            fullscreen = covers_monitor(hwnd);
            d3d_exclusive =
                SHQueryUserNotificationState().is_ok_and(|s| s == QUNS_RUNNING_D3D_FULL_SCREEN);
        }
        Foreground {
            exe,
            fullscreen,
            d3d_exclusive,
        }
    }
}

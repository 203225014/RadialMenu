//! Значок в трее: «Пауза», «Запускать с Windows», «Выход».

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, DestroyMenu, GetCursorPos,
    GetSystemMetrics, PostMessageW, SetForegroundWindow, TrackPopupMenu, HICON, LR_DEFAULTCOLOR,
    MF_CHECKED, MF_SEPARATOR, MF_STRING, SM_CXSMICON, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    WM_APP, WM_NULL,
};

use super::util::{pcwstr, wide};

pub const WM_TRAY: u32 = WM_APP + 10;
const ICON_ID: u32 = 1;
const ICO: &[u8] = include_bytes!("../../assets/RadialMenu.ico");
const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("RadialMenu");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    TogglePause,
    ToggleAutostart,
    Exit,
}

/// Кадр нужного размера из ICO (в нём PNG- или BMP-кадры).
fn icon() -> HICON {
    let want = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
    let count = u16::from_le_bytes([ICO[4], ICO[5]]) as usize;
    let mut best: Option<(i32, usize, usize)> = None; // (разница размера, смещение, длина)
    for i in 0..count {
        let e = &ICO[6 + i * 16..6 + (i + 1) * 16];
        let w = if e[0] == 0 { 256 } else { e[0] as i32 };
        let size = u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as usize;
        let off = u32::from_le_bytes([e[12], e[13], e[14], e[15]]) as usize;
        let score = (w - want).abs();
        if off + size <= ICO.len() && best.is_none_or(|b| score < b.0) {
            best = Some((score, off, size));
        }
    }
    let Some((_, off, size)) = best else {
        return HICON::default();
    };
    unsafe {
        CreateIconFromResourceEx(
            &ICO[off..off + size],
            true,
            0x0003_0000,
            want,
            want,
            LR_DEFAULTCOLOR,
        )
        .unwrap_or_default()
    }
}

fn data(hwnd: HWND, tip: &str) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: ICON_ID,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: icon(),
        ..Default::default()
    };
    for (d, s) in nid.szTip.iter_mut().zip(tip.encode_utf16().take(127)) {
        *d = s;
    }
    nid
}

pub fn add(hwnd: HWND, tip: &str) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &data(hwnd, tip));
    }
}

pub fn set_tip(hwnd: HWND, tip: &str) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &data(hwnd, tip));
    }
}

pub fn remove(hwnd: HWND) {
    unsafe {
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: ICON_ID,
            ..Default::default()
        };
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

/// Контекстное меню значка; возвращает выбранную команду.
pub fn menu(hwnd: HWND, paused: bool) -> Option<Cmd> {
    unsafe {
        let m = CreatePopupMenu().ok()?;
        let pause = wide(if paused {
            "Продолжить"
        } else {
            "Пауза"
        });
        let auto = wide("Запускать с Windows");
        let exit = wide("Выход");
        let _ = AppendMenuW(m, MF_STRING, 1, pcwstr(&pause));
        let _ = AppendMenuW(
            m,
            if autostart() {
                MF_STRING | MF_CHECKED
            } else {
                MF_STRING
            },
            2,
            pcwstr(&auto),
        );
        let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(m, MF_STRING, 3, pcwstr(&exit));
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        let _ = SetForegroundWindow(hwnd); // иначе меню не закроется кликом снаружи
        let id = TrackPopupMenu(
            m,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
            p.x,
            p.y,
            Some(0),
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, Default::default(), Default::default());
        let _ = DestroyMenu(m);
        match id.0 {
            1 => Some(Cmd::TogglePause),
            2 => Some(Cmd::ToggleAutostart),
            3 => Some(Cmd::Exit),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------- автозапуск (HKCU\...\Run)

pub fn autostart() -> bool {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_READ, &mut key).is_err() {
            return false;
        }
        let found = RegQueryValueExW(key, RUN_VALUE, None, None, None, None).is_ok();
        let _ = RegCloseKey(key);
        found
    }
}

pub fn set_autostart(on: bool) {
    unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
        .is_err()
        {
            return;
        }
        if on {
            if let Ok(exe) = std::env::current_exe() {
                let value = wide(&format!("\"{}\"", exe.display()));
                let bytes =
                    std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2);
                let _ = RegSetValueExW(key, RUN_VALUE, None, REG_SZ, Some(bytes));
            }
        } else {
            let _ = RegDeleteValueW(key, RUN_VALUE);
        }
        let _ = RegCloseKey(key);
    }
}

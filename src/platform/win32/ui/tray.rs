//! Значок в трее: «Настройки», «Пауза», «Запускать с Windows», «Выход»; всплывающие уведомления.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW, NOTIFY_ICON_INFOTIP_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, DestroyMenu, GetCursorPos,
    GetSystemMetrics, PostMessageW, SetForegroundWindow, SetMenuDefaultItem, TrackPopupMenu,
    HICON, LR_DEFAULTCOLOR, MF_CHECKED, MF_SEPARATOR, MF_STRING, SM_CXSMICON, TPM_NONOTIFY,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_NULL,
};

use crate::platform::win32::system::autostart;
use crate::platform::win32::system::util::{pcwstr, wide};

pub const WM_TRAY: u32 = WM_APP + 10;
const ICON_ID: u32 = 1;
/// Значки без плитки: светлый рисунок — для тёмной панели задач, тёмный — для светлой.
const ICO_ON_DARK: &[u8] = include_bytes!("../../../../assets/tray-dark.ico");
const ICO_ON_LIGHT: &[u8] = include_bytes!("../../../../assets/tray-light.ico");

/// Светлая ли панель задач (`SystemUsesLightTheme`; она может отличаться от темы приложений).
fn taskbar_is_light() -> bool {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    r.is_ok() && value != 0
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    Settings,
    TogglePause,
    ToggleAutostart,
    OpenLog,
    /// Открыть настройки, где можно обновить программу.
    Update,
    ToggleUpdates,
    CheckNow,
    Exit,
}

/// Кадр нужного размера из ICO (в нём PNG- или BMP-кадры).
fn icon() -> HICON {
    let ico: &[u8] = if taskbar_is_light() { ICO_ON_LIGHT } else { ICO_ON_DARK };
    let want = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    let mut best: Option<(i32, usize, usize)> = None; // (разница размера, смещение, длина)
    for i in 0..count {
        let e = &ico[6 + i * 16..6 + (i + 1) * 16];
        let w = if e[0] == 0 { 256 } else { e[0] as i32 };
        let size = u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as usize;
        let off = u32::from_le_bytes([e[12], e[13], e[14], e[15]]) as usize;
        let score = (w - want).abs();
        if off + size <= ico.len() && best.is_none_or(|b| score < b.0) {
            best = Some((score, off, size));
        }
    }
    let Some((_, off, size)) = best else {
        return HICON::default();
    };
    unsafe {
        CreateIconFromResourceEx(
            &ico[off..off + size],
            true,
            0x0003_0000,
            want,
            want,
            LR_DEFAULTCOLOR,
        )
        .unwrap_or_default()
    }
}

fn copy(dst: &mut [u16], s: &str) {
    let room = dst.len() - 1; // последний — завершающий ноль
    for (d, c) in dst.iter_mut().zip(s.encode_utf16().take(room)) {
        *d = c;
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
    copy(&mut nid.szTip, tip);
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

/// Всплывающее уведомление у значка (например, об ошибке в menu.yaml).
pub fn notify(hwnd: HWND, tip: &str, title: &str, text: &str) {
    balloon(hwnd, tip, title, text, NIIF_WARNING);
}

/// Обычное (не тревожное) уведомление: «вышла новая версия», «обновлений нет».
pub fn inform(hwnd: HWND, tip: &str, title: &str, text: &str) {
    balloon(hwnd, tip, title, text, NIIF_INFO);
}

fn balloon(hwnd: HWND, tip: &str, title: &str, text: &str, kind: NOTIFY_ICON_INFOTIP_FLAGS) {
    let mut nid = data(hwnd, tip);
    nid.uFlags |= NIF_INFO;
    nid.dwInfoFlags = kind;
    copy(&mut nid.szInfoTitle, title);
    copy(&mut nid.szInfo, text);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
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
/// `update` — версия, которую можно скачать (пункт появляется первым); `check_updates` — галка проверки.
pub fn menu(hwnd: HWND, paused: bool, update: Option<&str>, check_updates: bool) -> Option<Cmd> {
    unsafe {
        let m = CreatePopupMenu().ok()?;
        let settings = wide("Настройки…");
        let pause = wide(if paused { "Продолжить" } else { "Пауза" });
        let auto = wide("Запускать с Windows");
        let log = wide("Открыть журнал");
        let exit = wide("Выход");
        let upd = update.map(|v| wide(&format!("Доступна версия {v} — обновить…")));
        if let Some(u) = &upd {
            let _ = AppendMenuW(m, MF_STRING, 8, pcwstr(u));
            let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
        }
        let _ = AppendMenuW(m, MF_STRING, 1, pcwstr(&settings));
        let _ = SetMenuDefaultItem(m, if upd.is_some() { 8 } else { 1 }, 0); // жирным: то же, что двойной щелчок по значку
        let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(m, MF_STRING, 2, pcwstr(&pause));
        let checked = if autostart::enabled() {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        let _ = AppendMenuW(m, checked, 3, pcwstr(&auto));
        let upd_on = wide("Проверять обновления");
        let upd_now = wide("Проверить обновления сейчас");
        let _ = AppendMenuW(m, if check_updates { MF_STRING | MF_CHECKED } else { MF_STRING }, 6, pcwstr(&upd_on));
        let _ = AppendMenuW(m, MF_STRING, 7, pcwstr(&upd_now));
        let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(m, MF_STRING, 5, pcwstr(&log));
        let _ = AppendMenuW(m, MF_STRING, 4, pcwstr(&exit));
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
            1 => Some(Cmd::Settings),
            2 => Some(Cmd::TogglePause),
            3 => Some(Cmd::ToggleAutostart),
            4 => Some(Cmd::Exit),
            5 => Some(Cmd::OpenLog),
            6 => Some(Cmd::ToggleUpdates),
            7 => Some(Cmd::CheckNow),
            8 => Some(Cmd::Update),
            _ => None,
        }
    }
}

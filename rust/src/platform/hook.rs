//! Низкоуровневый хук мыши на отдельном потоке: глотает боковую кнопку и, пока она
//! зажата, копит сдвиги указателя (Raw Input локально, координаты — по RDP).
//! Колбэк трогает только атомики и шлёт сообщения главному окну — он обязан отвечать быстро
//! (Windows снимает хук, если ответа нет ~300 мс).

use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicIsize, AtomicU32, AtomicU64, Ordering::Relaxed,
};

use windows::core::w;
use windows::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_XUP, MOUSEINPUT,
    VK_XBUTTON1, VK_XBUTTON2,
};
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
    RIDEV_INPUTSINK, RID_INPUT, RIM_TYPEMOUSE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
    GetSystemMetrics, PostMessageW, RegisterClassW, SetTimer, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HHOOK, HWND_MESSAGE, LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT,
    SM_REMOTESESSION, WH_MOUSE_LL, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_INPUT, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_TIMER, WM_XBUTTONDOWN, WM_XBUTTONUP, WNDCLASSW,
};

/// Сообщения главному окну.
pub const WM_RM_DOWN: u32 = WM_APP + 1;
pub const WM_RM_UP: u32 = WM_APP + 2;
/// wParam: 1 — колесо вверх, 0 — вниз.
pub const WM_RM_WHEEL: u32 = WM_APP + 3;

pub struct Shared {
    /// 1 или 2 — какая боковая кнопка.
    pub button: AtomicU32,
    /// Выключатель («Пауза» в трее): выключен — все события идут дальше как обычно.
    pub enabled: AtomicBool,
    /// Кнопка зажата и проглочена.
    pub capture: AtomicBool,
    /// Сдвиги берутся из Raw Input (курсор стоит на месте), иначе — по координатам.
    pub use_raw: AtomicBool,
    raw_ok: AtomicBool,
    dx: AtomicI32,
    dy: AtomicI32,
    last_x: AtomicI32,
    last_y: AtomicI32,
    frozen_x: AtomicI32,
    frozen_y: AtomicI32,
    /// До этого момента (GetTickCount64, мс) синтетические события нашей кнопки не глотаем.
    pass_until: AtomicU64,
    main_hwnd: AtomicIsize,
}

pub static SHARED: Shared = Shared {
    button: AtomicU32::new(1),
    enabled: AtomicBool::new(true),
    capture: AtomicBool::new(false),
    use_raw: AtomicBool::new(true),
    raw_ok: AtomicBool::new(true),
    dx: AtomicI32::new(0),
    dy: AtomicI32::new(0),
    last_x: AtomicI32::new(0),
    last_y: AtomicI32::new(0),
    frozen_x: AtomicI32::new(0),
    frozen_y: AtomicI32::new(0),
    pass_until: AtomicU64::new(0),
    main_hwnd: AtomicIsize::new(0),
};

impl Shared {
    /// Забрать накопленный сдвиг.
    pub fn take(&self) -> (i32, i32) {
        (self.dx.swap(0, Relaxed), self.dy.swap(0, Relaxed))
    }

    /// Где нажали кнопку.
    pub fn frozen(&self) -> (i32, i32) {
        (self.frozen_x.load(Relaxed), self.frozen_y.load(Relaxed))
    }

    pub fn rebase(&self, p: (i32, i32)) {
        self.last_x.store(p.0, Relaxed);
        self.last_y.store(p.1, Relaxed);
    }

    fn post(&self, msg: u32, wparam: usize) {
        let h = self.main_hwnd.load(Relaxed);
        if h != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(h as *mut _)), msg, WPARAM(wparam), LPARAM(0));
            }
        }
    }
}

fn remote_session() -> bool {
    unsafe { GetSystemMetrics(SM_REMOTESESSION) != 0 }
}

fn tick_ms() -> u64 {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let s = &SHARED;
    if code == 0 && s.enabled.load(Relaxed) {
        let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        match wparam.0 as u32 {
            m @ (WM_XBUTTONDOWN | WM_XBUTTONUP) => {
                let which = (info.mouseData >> 16) & 0xFFFF;
                if which == s.button.load(Relaxed) {
                    if info.flags & LLMHF_INJECTED != 0 && tick_ms() < s.pass_until.load(Relaxed) {
                        return CallNextHookEx(None, code, wparam, lparam);
                    }
                    if m == WM_XBUTTONDOWN {
                        s.dx.store(0, Relaxed);
                        s.dy.store(0, Relaxed);
                        s.frozen_x.store(info.pt.x, Relaxed);
                        s.frozen_y.store(info.pt.y, Relaxed);
                        s.last_x.store(info.pt.x, Relaxed);
                        s.last_y.store(info.pt.y, Relaxed);
                        // режим выбираем на каждое нажатие: RDP мог подключиться позже запуска
                        s.use_raw
                            .store(s.raw_ok.load(Relaxed) && !remote_session(), Relaxed);
                        s.capture.store(true, Relaxed);
                        s.post(WM_RM_DOWN, 0);
                        return LRESULT(1);
                    }
                    // отпускание глотаем, только если глотали нажатие
                    if s.capture.swap(false, Relaxed) {
                        s.post(WM_RM_UP, 0);
                        return LRESULT(1);
                    }
                }
            }
            WM_MOUSEWHEEL if s.capture.load(Relaxed) => {
                let delta = (info.mouseData >> 16) as u16 as i16;
                if delta != 0 {
                    s.post(WM_RM_WHEEL, (delta > 0) as usize);
                }
                return LRESULT(1); // колесо в это время не крутит страницы
            }
            WM_MOUSEMOVE if s.capture.load(Relaxed) => {
                if s.use_raw.load(Relaxed) {
                    return LRESULT(1); // курсор остаётся на месте
                }
                let (x, y) = (info.pt.x, info.pt.y);
                // свой SetCursorPos (в точку нажатия) не считаем движением
                let own = info.flags & LLMHF_INJECTED != 0 && (x, y) == s.frozen();
                if !own {
                    s.dx.fetch_add(x - s.last_x.load(Relaxed), Relaxed);
                    s.dy.fetch_add(y - s.last_y.load(Relaxed), Relaxed);
                    s.last_x.store(x, Relaxed);
                    s.last_y.store(y, Relaxed);
                }
            }
            _ => {}
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn raw_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_INPUT {
        if SHARED.capture.load(Relaxed) {
            let mut raw: RAWINPUT = std::mem::zeroed();
            let mut size = std::mem::size_of::<RAWINPUT>() as u32;
            let got = GetRawInputData(
                HRAWINPUT(lparam.0 as *mut _),
                RID_INPUT,
                Some(&mut raw as *mut _ as *mut _),
                &mut size,
                std::mem::size_of::<RAWINPUTHEADER>() as u32,
            );
            if got != u32::MAX && raw.header.dwType == RIM_TYPEMOUSE.0 {
                let m = raw.data.mouse;
                if m.usFlags.0 & 1 == 0 {
                    // относительное движение
                    SHARED.dx.fetch_add(m.lLastX, Relaxed);
                    SHARED.dy.fetch_add(m.lLastY, Relaxed);
                }
            }
        }
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Окно-приёмник Raw Input на потоке хука.
unsafe fn setup_raw() -> bool {
    let Ok(hinst) = GetModuleHandleW(None) else {
        return false;
    };
    let class = w!("RadialMenuRawInput");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(raw_proc),
        hInstance: hinst.into(),
        lpszClassName: class,
        ..Default::default()
    };
    RegisterClassW(&wc);
    let Ok(hwnd) = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        w!(""),
        WINDOW_STYLE(0),
        0,
        0,
        0,
        0,
        Some(HWND_MESSAGE),
        None,
        Some(hinst.into()),
        None,
    ) else {
        return false;
    };
    let rid = RAWINPUTDEVICE {
        usUsagePage: 1,
        usUsage: 2,
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    };
    RegisterRawInputDevices(&[rid], std::mem::size_of::<RAWINPUTDEVICE>() as u32).is_ok()
}

unsafe fn install() -> Option<HHOOK> {
    let hinst = GetModuleHandleW(None).ok()?;
    SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), Some(hinst.into()), 0).ok()
}

/// Отпустить «зависшую» боковую кнопку синтетическим событием (мимо нашего хука).
unsafe fn release_stuck(button: u32) {
    SHARED.pass_until.store(tick_ms() + 500, Relaxed);
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                mouseData: button,
                dwFlags: MOUSEEVENTF_XUP,
                ..Default::default()
            },
        },
    };
    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
}

const WATCHDOG_MS: u32 = 250;

fn thread_main() {
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
        let mut hook = install();
        if hook.is_none() {
            super::util::log_error(&format!("SetWindowsHookEx не удался: {:?}", GetLastError()));
            return;
        }
        // приёмник Raw Input заводим всегда: сеанс RDP может смениться локальным уже после запуска
        if !setup_raw() {
            SHARED.raw_ok.store(false, Relaxed);
            SHARED.use_raw.store(false, Relaxed); // не вышло — работаем по координатам
        }
        SetTimer(None, 0, WATCHDOG_MS, None);
        let (mut ticks, mut stuck_ticks) = (0u32, 0u32);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if msg.message == WM_TIMER && msg.hwnd.is_invalid() {
                ticks += 1;
                let capture = SHARED.capture.load(Relaxed);
                // сторож: хук глотает кнопку целиком, так что «зажата» без меню — это утечка
                // (хук не успел ответить); отпускаем сами, иначе клики перестают работать
                let button = SHARED.button.load(Relaxed);
                let vk = if button == 1 {
                    VK_XBUTTON1
                } else {
                    VK_XBUTTON2
                };
                if !capture
                    && SHARED.enabled.load(Relaxed)
                    && GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0
                {
                    stuck_ticks += 1;
                    if stuck_ticks >= 2 {
                        release_stuck(button);
                        stuck_ticks = 0;
                    }
                } else {
                    stuck_ticks = 0;
                }
                // Windows мог снять хук после долгого ответа — раз в 2 с ставим заново
                if ticks % 8 == 0 && !capture {
                    if let Some(old) = hook.take() {
                        let _ = UnhookWindowsHookEx(old);
                    }
                    hook = install();
                }
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Запустить поток хука. `main` — окно, которому он шлёт события.
pub fn spawn(button: u8, main: HWND) {
    SHARED.button.store(button as u32, Relaxed);
    SHARED.main_hwnd.store(main.0 as isize, Relaxed);
    std::thread::Builder::new()
        .name("mouse-hook".into())
        .spawn(thread_main)
        .expect("не удалось запустить поток хука");
}

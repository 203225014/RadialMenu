//! Низкоуровневые хуки мыши и клавиатуры на отдельном потоке.
//!
//! Меню вызывается боковой кнопкой мыши и/или сочетанием клавиш: пока кнопка (клавиша)
//! зажата, хук её глотает и копит сдвиги указателя (Raw Input локально, координаты — по RDP).
//! Колбэки трогают только атомики и шлют сообщения окнам — они обязаны отвечать быстро
//! (Windows снимает хук, если ответа нет ~300 мс).
//!
//! Снаружи модуль виден только через функции ниже: состояние хука — его внутреннее дело.

use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicIsize, AtomicU32, AtomicU64, AtomicU8, Ordering::Relaxed,
};

use windows::core::w;
use windows::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYEVENTF_KEYUP, MOUSEEVENTF_XUP, MOUSEINPUT, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU,
    VK_RWIN, VK_SHIFT, VK_XBUTTON1, VK_XBUTTON2,
};
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
    RIDEV_INPUTSINK, RID_INPUT, RIM_TYPEMOUSE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, GetMessageW,
    GetSystemMetrics, PostMessageW, RegisterClassW, SetCursorPos, SetTimer, SetWindowsHookExW,
    SystemParametersInfoW, SPI_GETKEYBOARDDELAY, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    TranslateMessage, UnhookWindowsHookEx, HHOOK, HWND_MESSAGE, KBDLLHOOKSTRUCT, LLKHF_INJECTED,
    LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, SM_REMOTESESSION, WH_KEYBOARD_LL, WH_MOUSE_LL,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_INPUT, WM_KEYDOWN, WM_KEYUP, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WM_XBUTTONDOWN, WM_XBUTTONUP, WNDCLASSW,
};

use crate::model::action::vk;
use crate::model::{Chord, MAX_PROFILES};
use crate::platform::win32::system::util;

/// Сообщения главному окну. `WM_RM_DOWN`: wParam — номер профиля (0 — основное меню).
pub const WM_RM_DOWN: u32 = WM_APP + 1;
pub const WM_RM_UP: u32 = WM_APP + 2;
/// wParam: 1 — колесо вверх, 0 — вниз.
pub const WM_RM_WHEEL: u32 = WM_APP + 3;
/// Окну, записывающему сочетание: wParam — клавиша, lParam — модификаторы (`MOD_*`).
pub const WM_RM_RECORD: u32 = WM_APP + 4;

pub const MOD_CTRL: u32 = 1;
pub const MOD_ALT: u32 = 2;
pub const MOD_SHIFT: u32 = 4;
pub const MOD_WIN: u32 = 8;

/// Чем сейчас удерживается меню.
const SRC_NONE: u8 = 0;
const SRC_MOUSE: u8 = 1;
const SRC_KEYS: u8 = 2;

/// Свободный код клавиши: нажимаем его, чтобы одиночное отпускание Alt/Win
/// не открыло меню окна или «Пуск» (сочетание-то мы проглотили).
const MASK_KEY: u16 = 0xE8;

/// Способ вызова одного профиля.
struct Slot {
    /// 0 — мышь не используется, 1 или 2 — какая боковая кнопка.
    button: AtomicU32,
    /// Сочетание: клавиша | модификаторы << 16; 0 — не задано.
    hotkey: AtomicU32,
}

/// Профилей всего: основное меню (0) и дополнительные.
const SLOTS: usize = 1 + MAX_PROFILES;

#[allow(clippy::declare_interior_mutable_const)]
const NO_SLOT: Slot = Slot { button: AtomicU32::new(0), hotkey: AtomicU32::new(0) };

struct Shared {
    /// Способы вызова по номерам профилей.
    slots: [Slot; SLOTS],
    /// Профиль, меню которого сейчас открыто, и чем оно удерживается (кнопка мыши).
    active: AtomicU8,
    cap_button: AtomicU32,
    /// Выключатель («Пауза» в трее): выключен — все события идут дальше как обычно.
    enabled: AtomicBool,
    /// Кто держит меню открытым (`SRC_*`).
    capture: AtomicU8,
    /// Окно, которое сейчас записывает сочетание (0 — никто).
    recorder: AtomicIsize,
    /// Клавиша, проглоченная при записи: её отпускание тоже глотаем.
    rec_key: AtomicU32,
    /// Сдвиги берутся из Raw Input (курсор стоит на месте), иначе — по координатам.
    use_raw: AtomicBool,
    raw_ok: AtomicBool,
    dx: AtomicI32,
    dy: AtomicI32,
    last_x: AtomicI32,
    last_y: AtomicI32,
    frozen_x: AtomicI32,
    frozen_y: AtomicI32,
    /// До этого момента (GetTickCount64, мс) синтетические события нашей кнопки не глотаем.
    pass_until: AtomicU64,
    /// Счётчик вызовов колбэков — по нему сторож видит, что хуки живы.
    calls: AtomicU32,
    /// Когда в последний раз пришло нажатие (или автоповтор) клавиши сочетания, мс.
    /// Саму клавишу мы глотаем, поэтому GetAsyncKeyState её зажатой не видит.
    key_tick: AtomicU64,
    main_hwnd: AtomicIsize,
}

static SHARED: Shared = Shared {
    slots: [Slot { button: AtomicU32::new(1), hotkey: AtomicU32::new(0) }, NO_SLOT, NO_SLOT, NO_SLOT],
    active: AtomicU8::new(0),
    cap_button: AtomicU32::new(0),
    enabled: AtomicBool::new(true),
    capture: AtomicU8::new(SRC_NONE),
    recorder: AtomicIsize::new(0),
    rec_key: AtomicU32::new(0),
    use_raw: AtomicBool::new(true),
    raw_ok: AtomicBool::new(true),
    dx: AtomicI32::new(0),
    dy: AtomicI32::new(0),
    last_x: AtomicI32::new(0),
    last_y: AtomicI32::new(0),
    frozen_x: AtomicI32::new(0),
    frozen_y: AtomicI32::new(0),
    pass_until: AtomicU64::new(0),
    calls: AtomicU32::new(0),
    key_tick: AtomicU64::new(0),
    main_hwnd: AtomicIsize::new(0),
};

// ---------------------------------------------------------------- интерфейс для главного потока

/// Чем вызывается профиль `profile` (0 — основное меню): боковая кнопка (0 — никакая, 1 — «назад»,
/// 2 — «вперёд») и/или сочетание клавиш.
pub fn set_trigger(profile: usize, button: u8, hotkey: Option<Chord>) {
    if let Some(slot) = SHARED.slots.get(profile) {
        slot.button.store(button.min(2) as u32, Relaxed);
        slot.hotkey.store(hotkey.map_or(0, pack), Relaxed);
    }
}

/// Пауза: выключенный хук пропускает все события дальше.
pub fn set_enabled(on: bool) {
    SHARED.enabled.store(on, Relaxed);
    if !on {
        cancel();
    }
}

/// Запись сочетания: все нажатия (кроме модификаторов) уходят окну `hwnd` как
/// `WM_RM_RECORD` и больше никуда — так записываются и системные сочетания вроде Win+E.
pub fn set_recorder(hwnd: Option<HWND>) {
    SHARED.recorder.store(hwnd.map_or(0, |h| h.0 as isize), Relaxed);
}

/// Забыть о зажатой кнопке (меню закрыто без события «отпустил»).
pub fn cancel() {
    SHARED.capture.store(SRC_NONE, Relaxed);
}

/// Где был курсор, когда меню вызвали.
pub fn frozen() -> (i32, i32) {
    (SHARED.frozen_x.load(Relaxed), SHARED.frozen_y.load(Relaxed))
}

/// Сбросить накопленный сдвиг (то, что было до показа меню, не в счёт).
pub fn discard_motion() {
    take();
}

/// Сдвиг указателя с прошлого вызова. В режиме координат (без Raw Input) заодно
/// возвращает курсор в точку вызова, кроме удалённого сеанса: по RDP курсор не держим.
pub fn take_motion() -> (i32, i32) {
    let d = take();
    let by_coords = !SHARED.use_raw.load(Relaxed);
    if SHARED.capture.load(Relaxed) != SRC_NONE && by_coords && !remote_session() {
        let f = frozen();
        unsafe {
            let _ = SetCursorPos(f.0, f.1);
        }
        SHARED.last_x.store(f.0, Relaxed);
        SHARED.last_y.store(f.1, Relaxed);
    }
    d
}

fn pack(c: Chord) -> u32 {
    let mods = (c.ctrl as u32 * MOD_CTRL)
        | (c.alt as u32 * MOD_ALT)
        | (c.shift as u32 * MOD_SHIFT)
        | (c.win as u32 * MOD_WIN);
    c.key as u32 | mods << 16
}

fn take() -> (i32, i32) {
    (SHARED.dx.swap(0, Relaxed), SHARED.dy.swap(0, Relaxed))
}

fn post_to(hwnd: isize, msg: u32, wparam: usize, lparam: isize) {
    if hwnd != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as *mut _)), msg, WPARAM(wparam), LPARAM(lparam));
        }
    }
}

fn post(msg: u32, wparam: usize) {
    post_to(SHARED.main_hwnd.load(Relaxed), msg, wparam, 0);
}

fn remote_session() -> bool {
    unsafe { GetSystemMetrics(SM_REMOTESESSION) != 0 }
}

fn tick_ms() -> u64 {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }
}

fn key_down(k: u16) -> bool {
    unsafe { GetAsyncKeyState(k as i32) as u16 & 0x8000 != 0 }
}

/// Какие модификаторы зажаты сейчас.
fn mods_now() -> u32 {
    let mut m = 0;
    if key_down(VK_CONTROL.0) {
        m |= MOD_CTRL;
    }
    if key_down(VK_MENU.0) {
        m |= MOD_ALT;
    }
    if key_down(VK_SHIFT.0) {
        m |= MOD_SHIFT;
    }
    if key_down(VK_LWIN.0) || key_down(VK_RWIN.0) {
        m |= MOD_WIN;
    }
    m
}

/// Нажать и отпустить «пустую» клавишу (см. `MASK_KEY`).
fn send_mask_key() {
    let k = |up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(MASK_KEY),
                dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() },
                ..Default::default()
            },
        },
    };
    unsafe {
        SendInput(&[k(false), k(true)], std::mem::size_of::<INPUT>() as i32);
    }
}

/// Меню вызвано: запомнить, где курсор, и выбрать способ считать сдвиги.
fn begin(src: u8, profile: usize, at: (i32, i32)) {
    let s = &SHARED;
    s.dx.store(0, Relaxed);
    s.dy.store(0, Relaxed);
    s.frozen_x.store(at.0, Relaxed);
    s.frozen_y.store(at.1, Relaxed);
    s.last_x.store(at.0, Relaxed);
    s.last_y.store(at.1, Relaxed);
    // режим выбираем на каждый вызов: RDP мог подключиться позже запуска
    s.use_raw.store(s.raw_ok.load(Relaxed) && !remote_session(), Relaxed);
    s.active.store(profile as u8, Relaxed);
    s.capture.store(src, Relaxed);
    post(WM_RM_DOWN, profile);
}

/// Отпустили то, чем держали меню.
fn end(src: u8) -> bool {
    if SHARED
        .capture
        .compare_exchange(src, SRC_NONE, Relaxed, Relaxed)
        .is_ok()
    {
        post(WM_RM_UP, 0);
        true
    } else {
        false
    }
}

// ---------------------------------------------------------------- поток хуков

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let s = &SHARED;
    s.calls.fetch_add(1, Relaxed);
    if code == 0 && s.enabled.load(Relaxed) {
        let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        let capturing = s.capture.load(Relaxed) != SRC_NONE;
        match wparam.0 as u32 {
            m @ (WM_XBUTTONDOWN | WM_XBUTTONUP) => {
                let which = (info.mouseData >> 16) & 0xFFFF;
                let profile = (0..SLOTS).find(|&p| {
                    let b = s.slots[p].button.load(Relaxed);
                    b != 0 && b == which
                });
                if let Some(profile) = profile {
                    if info.flags & LLMHF_INJECTED != 0 && tick_ms() < s.pass_until.load(Relaxed) {
                        return CallNextHookEx(None, code, wparam, lparam);
                    }
                    if m == WM_XBUTTONDOWN {
                        if !capturing {
                            s.cap_button.store(which, Relaxed);
                            begin(SRC_MOUSE, profile, (info.pt.x, info.pt.y));
                            return LRESULT(1);
                        }
                    } else if s.cap_button.load(Relaxed) == which && end(SRC_MOUSE) {
                        // отпускание глотаем, только если глотали нажатие
                        return LRESULT(1);
                    }
                }
            }
            WM_MOUSEWHEEL if capturing => {
                let delta = (info.mouseData >> 16) as u16 as i16;
                if delta != 0 {
                    post(WM_RM_WHEEL, (delta > 0) as usize);
                }
                return LRESULT(1); // колесо в это время не крутит страницы
            }
            WM_MOUSEMOVE if capturing => {
                if s.use_raw.load(Relaxed) {
                    return LRESULT(1); // курсор остаётся на месте
                }
                let (x, y) = (info.pt.x, info.pt.y);
                // свой SetCursorPos (в точку вызова) не считаем движением
                let own = info.flags & LLMHF_INJECTED != 0 && (x, y) == frozen();
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

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let s = &SHARED;
    s.calls.fetch_add(1, Relaxed);
    if code != 0 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
    let key = info.vkCode as u16;
    // свои нажатия (SendInput) и модификаторы сами по себе не трогаем
    if info.flags.0 & LLKHF_INJECTED.0 != 0 || vk::is_modifier(key) {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let msg = wparam.0 as u32;
    let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;

    // запись сочетания в настройках
    let recorder = s.recorder.load(Relaxed);
    if recorder != 0 {
        if down {
            s.rec_key.store(key as u32, Relaxed);
            post_to(recorder, WM_RM_RECORD, key as usize, mods_now() as isize);
            return LRESULT(1);
        }
        if up && s.rec_key.swap(0, Relaxed) == key as u32 {
            if mods_now() & (MOD_ALT | MOD_WIN) != 0 {
                send_mask_key();
            }
            return LRESULT(1);
        }
        return CallNextHookEx(None, code, wparam, lparam);
    }

    if !s.enabled.load(Relaxed) {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    match s.capture.load(Relaxed) {
        SRC_KEYS => {
            // держат сочетание того профиля, что открыт
            let hk = s.slots[s.active.load(Relaxed) as usize % SLOTS].hotkey.load(Relaxed);
            if hk != 0 && key as u32 == hk & 0xFFFF {
                if down {
                    // автоповтор, пока держат: заодно знак сторожу, что клавиша ещё зажата
                    s.key_tick.store(tick_ms(), Relaxed);
                    return LRESULT(1);
                }
                if up && end(SRC_KEYS) {
                    if (hk >> 16) & (MOD_ALT | MOD_WIN) != 0 {
                        send_mask_key();
                    }
                    return LRESULT(1);
                }
            }
        }
        SRC_NONE if down => {
            let now = mods_now();
            let profile = (0..SLOTS).find(|&p| {
                let hk = s.slots[p].hotkey.load(Relaxed);
                hk != 0 && key as u32 == hk & 0xFFFF && now == hk >> 16
            });
            if let Some(profile) = profile {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                s.key_tick.store(tick_ms(), Relaxed);
                begin(SRC_KEYS, profile, (pt.x, pt.y));
                return LRESULT(1);
            }
        }
        _ => {}
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
        if SHARED.capture.load(Relaxed) != SRC_NONE {
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

/// Оба хука; `None` — не удалось поставить хук мыши (без него меню не работает вовсе).
unsafe fn install() -> Option<(HHOOK, Option<HHOOK>)> {
    let hinst = GetModuleHandleW(None).ok()?;
    let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), Some(hinst.into()), 0).ok()?;
    let keys = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), Some(hinst.into()), 0).ok();
    Some((mouse, keys))
}

unsafe fn uninstall(h: (HHOOK, Option<HHOOK>)) {
    let _ = UnhookWindowsHookEx(h.0);
    if let Some(k) = h.1 {
        let _ = UnhookWindowsHookEx(k);
    }
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
/// Не переставлять хук чаще, чем раз в столько тиков сторожа (2 с).
const REINSTALL_COOLDOWN: u32 = 8;

/// Проверка живости хука: курсор двигался, а колбэк ни разу не вызвался — значит,
/// Windows хук сняла (так бывает после долгого ответа). Чистая логика — для тестов.
#[derive(Default)]
pub struct Liveness {
    last_pos: Option<(i32, i32)>,
    last_calls: u32,
    silent_ticks: u32,
    since_install: u32,
}

impl Liveness {
    /// Один тик сторожа; `true` — хук пора поставить заново.
    pub fn tick(&mut self, cursor: (i32, i32), calls: u32, capturing: bool) -> bool {
        self.since_install = self.since_install.saturating_add(1);
        let moved = self.last_pos.is_some_and(|p| p != cursor);
        let silent = calls == self.last_calls;
        self.last_pos = Some(cursor);
        self.last_calls = calls;
        // пока меню открыто, курсор двигаем мы сами — это не признак
        if capturing || !(moved && silent) {
            self.silent_ticks = 0;
            return false;
        }
        self.silent_ticks += 1;
        if self.silent_ticks >= 2 && self.since_install >= REINSTALL_COOLDOWN {
            self.silent_ticks = 0;
            self.since_install = 0;
            return true;
        }
        false
    }
}

/// Сторож: то, что держит меню, на самом деле уже отпущено (событие потерялось).
unsafe fn watchdog(stuck_ticks: &mut u32) {
    let s = &SHARED;
    let capture = s.capture.load(Relaxed);
    // боковая «зажата» без меню — утечка (хук не успел ответить); отпускаем сами,
    // иначе клики перестают работать
    if capture == SRC_NONE && s.enabled.load(Relaxed) {
        let stuck = s.slots.iter().map(|sl| sl.button.load(Relaxed)).find(|&b| {
            b != 0 && key_down(if b == 1 { VK_XBUTTON1 } else { VK_XBUTTON2 }.0)
        });
        if let Some(button) = stuck {
            *stuck_ticks += 1;
            if *stuck_ticks >= 2 {
                release_stuck(button);
                *stuck_ticks = 0;
            }
            return;
        }
    }
    *stuck_ticks = 0;
    // клавишу сочетания отпустили, а мы не заметили — закрыть меню
    if capture == SRC_KEYS {
        let mods = s.slots[s.active.load(Relaxed) as usize % SLOTS].hotkey.load(Relaxed) >> 16;
        let quiet = tick_ms().saturating_sub(s.key_tick.load(Relaxed));
        if keys_released(quiet, repeat_delay_ms(), mods, mods_now()) {
            end(SRC_KEYS);
        }
    }
}

/// Задержка автоповтора клавиатуры из настроек Windows, мс (250–1000).
fn repeat_delay_ms() -> u64 {
    let mut v: u32 = 1;
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETKEYBOARDDELAY,
            0,
            Some(&mut v as *mut u32 as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    (v.min(3) as u64 + 1) * 250
}

/// Отпустили ли сочетание, если «отпускание» потерялось. Саму клавишу хук глотает,
/// поэтому судим по косвенным признакам: отпущены все модификаторы сочетания, или
/// автоповтор молчит дольше, чем бывает при удержании (`quiet_ms` — с последнего нажатия).
fn keys_released(quiet_ms: u64, repeat_delay: u64, mods: u32, held: u32) -> bool {
    let mods_gone = mods != 0 && held & mods == 0;
    mods_gone || quiet_ms > repeat_delay + 400
}

fn thread_main() {
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
        let mut hooks = install();
        if hooks.is_none() {
            util::log_error(&format!("SetWindowsHookEx не удался: {:?}", GetLastError()));
            return;
        }
        // приёмник Raw Input заводим всегда: сеанс RDP может смениться локальным уже после запуска
        if !setup_raw() {
            SHARED.raw_ok.store(false, Relaxed);
            SHARED.use_raw.store(false, Relaxed); // не вышло — работаем по координатам
        }
        SetTimer(None, 0, WATCHDOG_MS, None);
        let mut stuck_ticks = 0u32;
        let mut alive = Liveness::default();
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if msg.message == WM_TIMER && msg.hwnd.is_invalid() {
                watchdog(&mut stuck_ticks);
                let mut p = POINT::default();
                let _ = GetCursorPos(&mut p);
                let capturing = SHARED.capture.load(Relaxed) != SRC_NONE;
                if alive.tick((p.x, p.y), SHARED.calls.load(Relaxed), capturing) {
                    if let Some(old) = hooks.take() {
                        uninstall(old);
                    }
                    hooks = install();
                    util::log_error("хуки ввода не отвечали — поставлены заново");
                }
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Запустить поток хуков. `main` — окно, которому они шлют события.
pub fn spawn(button: u8, hotkey: Option<Chord>, main: HWND) {
    set_trigger(0, button, hotkey);
    SHARED.main_hwnd.store(main.0 as isize, Relaxed);
    std::thread::Builder::new()
        .name("input-hooks".into())
        .spawn(thread_main)
        .expect("не удалось запустить поток хуков");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(l: &mut Liveness, steps: &[((i32, i32), u32, bool)]) -> Vec<bool> {
        steps.iter().map(|&(p, c, cap)| l.tick(p, c, cap)).collect()
    }

    #[test]
    fn live_hook_is_left_alone() {
        let mut l = Liveness::default();
        let steps: Vec<_> = (0..20).map(|i| ((i, i), i as u32, false)).collect();
        assert!(run(&mut l, &steps).iter().all(|&r| !r));
    }

    #[test]
    fn still_cursor_is_not_a_symptom() {
        let mut l = Liveness::default();
        let steps: Vec<_> = (0..20).map(|_| ((5, 5), 7, false)).collect();
        assert!(run(&mut l, &steps).iter().all(|&r| !r));
    }

    #[test]
    fn silent_hook_with_moving_cursor_is_reinstalled_with_cooldown() {
        let mut l = Liveness::default();
        let steps: Vec<_> = (0..30).map(|i| ((i, 0), 3, false)).collect();
        let r = run(&mut l, &steps);
        let first = r.iter().position(|&x| x).expect("должен переставить");
        assert!(first + 1 >= REINSTALL_COOLDOWN as usize);
        let next = r[first + 1..].iter().position(|&x| x).unwrap() + 1;
        assert!(next >= REINSTALL_COOLDOWN as usize, "не чаще раза в 2 с");
    }

    #[test]
    fn own_cursor_moves_during_menu_do_not_count() {
        let mut l = Liveness::default();
        let steps: Vec<_> = (0..30).map(|i| ((i, 0), 3, true)).collect();
        assert!(run(&mut l, &steps).iter().all(|&r| !r));
    }

    #[test]
    fn held_shortcut_is_not_closed_by_watchdog() {
        // держат Ctrl+Alt+Space: автоповтор ещё не начался (задержка 500 мс), модификаторы зажаты
        assert!(!keys_released(300, 500, MOD_CTRL | MOD_ALT, MOD_CTRL | MOD_ALT));
        // идёт автоповтор
        assert!(!keys_released(40, 500, MOD_CTRL | MOD_ALT, MOD_CTRL | MOD_ALT));
        // отпустили модификаторы, а «отпускание» клавиши потерялось
        assert!(keys_released(40, 500, MOD_CTRL | MOD_ALT, 0));
        // одна клавиша без модификаторов (F13): судим только по автоповтору
        assert!(!keys_released(600, 500, 0, 0));
        assert!(keys_released(1000, 500, 0, 0));
    }

    #[test]
    fn chords_pack_modifiers() {
        let c = Chord::parse("ctrl+alt+space").unwrap();
        let p = pack(c);
        assert_eq!(p & 0xFFFF, vk::SPACE as u32);
        assert_eq!(p >> 16, MOD_CTRL | MOD_ALT);
        let w = pack(Chord::parse("win+shift+f13").unwrap());
        assert_eq!(w >> 16, MOD_WIN | MOD_SHIFT);
    }
}

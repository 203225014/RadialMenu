//! Исполнение действий «наружу»: нажатия клавиш, печать текста, медиаклавиши, запуск
//! программ и ссылок. Действия, открывающие свои окна (заметка, настройки), исполняет `app`.

use windows::core::{w, PCWSTR};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY,
};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::model::action::vk;
use crate::model::Run;
use crate::platform::win32::system::util::{log_error, pcwstr, wide};

fn key(k: u16, up: bool) -> INPUT {
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    if vk::is_extended(k) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(k),
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}

fn unicode(unit: u16, up: bool) -> INPUT {
    let mut flags = KEYEVENTF_UNICODE;
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wScan: unit,
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}

fn send(inputs: &[INPUT]) {
    unsafe {
        SendInput(inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Нажать клавиши по порядку и отпустить в обратном — одним пакетом, чтобы никто не вклинился.
pub fn chord(keys: &[u16]) {
    let mut v: Vec<INPUT> = keys.iter().map(|&k| key(k, false)).collect();
    v.extend(keys.iter().rev().map(|&k| key(k, true)));
    send(&v);
}

/// Одиночная клавиша (медиа, громкость).
pub fn tap(k: u16) {
    send(&[key(k, false), key(k, true)]);
}

pub fn volume(up: bool) {
    tap(if up { vk::VOLUME_UP } else { vk::VOLUME_DOWN });
}

/// Напечатать текст как набор символов (не зависит от раскладки). Перевод строки — Enter.
pub fn text(s: &str) {
    let mut v = Vec::with_capacity(s.len() * 2);
    for line in s.split('\n').enumerate() {
        let (i, line) = line;
        if i > 0 {
            v.push(key(vk::RETURN, false));
            v.push(key(vk::RETURN, true));
        }
        for unit in line.trim_end_matches('\r').encode_utf16() {
            v.push(unicode(unit, false));
            v.push(unicode(unit, true));
        }
    }
    send(&v);
}

pub fn explorer() {
    if let Err(e) = std::process::Command::new("explorer.exe").spawn() {
        log_error(&format!("explorer: {e}"));
    }
}

fn shell(verb: PCWSTR, file: &str, args: &str, cwd: &str) -> bool {
    let (f, a, d) = (wide(file), wide(args), wide(cwd));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC,
        lpVerb: verb,
        lpFile: pcwstr(&f),
        lpParameters: if args.is_empty() { PCWSTR::null() } else { pcwstr(&a) },
        lpDirectory: if cwd.is_empty() { PCWSTR::null() } else { pcwstr(&d) },
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info).is_ok() }
}

/// Открыть ссылку, протокол, файл или папку так, как это сделал бы Проводник.
pub fn open(target: &str) {
    if !shell(w!("open"), target, "", "") {
        log_error(&format!("не удалось открыть «{target}»"));
    }
}

/// Запустить программу: имя ищется как в «Выполнить» (PATH, App Paths), `admin` — через UAC.
pub fn run(r: &Run) {
    let verb = if r.admin { w!("runas") } else { PCWSTR::null() };
    if !shell(verb, &r.path, &r.args, &r.cwd) {
        log_error(&format!("не удалось запустить «{}» {}", r.path, r.args));
    }
}

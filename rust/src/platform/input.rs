//! Исполнение действий: нажатия клавиш, запуск программ, шаг громкости.

use windows::core::w;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VIRTUAL_KEY,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use super::util::{pcwstr, wide};
use crate::action::Action;

fn key(vk: u16, up: bool, extended: bool) -> INPUT {
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
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
    let mut v: Vec<INPUT> = keys.iter().map(|&k| key(k, false, false)).collect();
    v.extend(keys.iter().rev().map(|&k| key(k, true, false)));
    send(&v);
}

/// Медиаклавиша и громкость — расширенные клавиши.
pub fn media_key(vk: u16) {
    send(&[key(vk, false, true), key(vk, true, true)]);
}

pub fn exec(action: Action, vault: &str) {
    match action {
        Action::Keys(keys) => chord(keys),
        Action::MediaKey(vk) => media_key(vk),
        Action::Volume(up) => media_key(if up {
            crate::action::vk::VOLUME_UP
        } else {
            crate::action::vk::VOLUME_DOWN
        }),
        Action::Explorer => {
            if let Err(e) = std::process::Command::new("explorer.exe").spawn() {
                super::util::log_error(&format!("explorer: {e}"));
            }
        }
        Action::OpenUri(uri) => unsafe {
            let u = wide(uri);
            ShellExecuteW(None, w!("open"), pcwstr(&u), None, None, SW_SHOWNORMAL);
        },
        Action::Note => super::note::open(vault),
    }
}

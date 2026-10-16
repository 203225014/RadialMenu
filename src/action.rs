//! Действия, которые меню умеет запускать. Данные — без привязки к платформе:
//! исполняет их `platform` (на Windows — SendInput, ShellExecute, окно заметки).

/// Коды виртуальных клавиш Windows, нужные меню.
pub mod vk {
    pub const CONTROL: u16 = 0x11;
    pub const LWIN: u16 = 0x5B;
    pub const A: u16 = 0x41;
    pub const C: u16 = 0x43;
    pub const V: u16 = 0x56;
    pub const X: u16 = 0x58;
    pub const MEDIA_NEXT: u16 = 0xB0;
    pub const MEDIA_PREV: u16 = 0xB1;
    pub const MEDIA_PLAY_PAUSE: u16 = 0xB3;
    pub const VOLUME_DOWN: u16 = 0xAE;
    pub const VOLUME_UP: u16 = 0xAF;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Аккорд: зажать клавиши по порядку, отпустить в обратном (Ctrl+C, Win+V…).
    Keys(&'static [u16]),
    /// Медиаклавиша (расширенная).
    MediaKey(u16),
    /// Шаг громкости (`true` — громче).
    Volume(bool),
    /// Открыть Проводник.
    Explorer,
    /// Открыть ссылку/протокол (`ms-settings:` и т. п.).
    OpenUri(&'static str),
    /// Окно быстрой заметки.
    Note,
}

pub const COPY: Action = Action::Keys(&[vk::CONTROL, vk::C]);
pub const PASTE: Action = Action::Keys(&[vk::CONTROL, vk::V]);
pub const CUT: Action = Action::Keys(&[vk::CONTROL, vk::X]);
pub const SELECT_ALL: Action = Action::Keys(&[vk::CONTROL, vk::A]);
pub const CLIPBOARD_HISTORY: Action = Action::Keys(&[vk::LWIN, vk::V]);

//! Действия, которые меню умеет запускать, и сочетания клавиш. Данные — без привязки
//! к платформе: исполняет их `platform` (на Windows — SendInput, ShellExecute, свои окна).

/// Коды виртуальных клавиш Windows (ими же описываются сочетания в файлах).
pub mod vk {
    pub const BACK: u16 = 0x08;
    pub const TAB: u16 = 0x09;
    pub const RETURN: u16 = 0x0D;
    pub const SHIFT: u16 = 0x10;
    pub const CONTROL: u16 = 0x11;
    pub const MENU: u16 = 0x12; // Alt
    pub const PAUSE: u16 = 0x13;
    pub const CAPITAL: u16 = 0x14;
    pub const ESCAPE: u16 = 0x1B;
    pub const SPACE: u16 = 0x20;
    pub const PRIOR: u16 = 0x21;
    pub const NEXT: u16 = 0x22;
    pub const END: u16 = 0x23;
    pub const HOME: u16 = 0x24;
    pub const LEFT: u16 = 0x25;
    pub const UP: u16 = 0x26;
    pub const RIGHT: u16 = 0x27;
    pub const DOWN: u16 = 0x28;
    pub const SNAPSHOT: u16 = 0x2C;
    pub const INSERT: u16 = 0x2D;
    pub const DELETE: u16 = 0x2E;
    pub const LWIN: u16 = 0x5B;
    pub const RWIN: u16 = 0x5C;
    pub const APPS: u16 = 0x5D;
    pub const NUMPAD0: u16 = 0x60;
    pub const F1: u16 = 0x70;
    pub const SCROLL: u16 = 0x91;
    pub const LSHIFT: u16 = 0xA0;
    pub const RSHIFT: u16 = 0xA1;
    pub const LCONTROL: u16 = 0xA2;
    pub const RCONTROL: u16 = 0xA3;
    pub const LMENU: u16 = 0xA4;
    pub const RMENU: u16 = 0xA5;
    pub const A: u16 = 0x41;
    pub const C: u16 = 0x43;
    pub const V: u16 = 0x56;
    pub const X: u16 = 0x58;
    pub const MEDIA_NEXT: u16 = 0xB0;
    pub const MEDIA_PREV: u16 = 0xB1;
    pub const MEDIA_PLAY_PAUSE: u16 = 0xB3;
    pub const VOLUME_MUTE: u16 = 0xAD;
    pub const VOLUME_DOWN: u16 = 0xAE;
    pub const VOLUME_UP: u16 = 0xAF;

    /// Знаки пунктуации (раскладка США — так Windows называет эти клавиши).
    const OEM: [(&str, u16); 11] = [
        (";", 0xBA),
        ("=", 0xBB),
        (",", 0xBC),
        ("-", 0xBD),
        (".", 0xBE),
        ("/", 0xBF),
        ("`", 0xC0),
        ("[", 0xDB),
        ("\\", 0xDC),
        ("]", 0xDD),
        ("'", 0xDE),
    ];

    /// Ctrl, Alt, Shift, Win — с любой стороны.
    pub fn is_modifier(k: u16) -> bool {
        matches!(
            k,
            CONTROL | SHIFT | MENU | LWIN | RWIN | LSHIFT | RSHIFT | LCONTROL | RCONTROL | LMENU | RMENU
        )
    }

    /// Клавиши, которым при эмуляции нужен флаг «расширенная» (стрелки, Home, Win…).
    pub fn is_extended(k: u16) -> bool {
        matches!(
            k,
            PRIOR
                | NEXT
                | END
                | HOME
                | LEFT
                | UP
                | RIGHT
                | DOWN
                | INSERT
                | DELETE
                | LWIN
                | RWIN
                | APPS
                | SNAPSHOT
                | RCONTROL
                | RMENU
        ) || (0xA6..=0xB7).contains(&k)
    }

    /// Имя клавиши → код (`ctrl`, `win`, `a`, `f5`, `enter`, `capslock`, `num5`, `;`…).
    pub fn from_name(name: &str) -> Option<u16> {
        let n = name.trim().to_lowercase();
        let k = match n.as_str() {
            "ctrl" | "control" => CONTROL,
            "shift" => SHIFT,
            "alt" => MENU,
            "win" => LWIN,
            "enter" | "return" => RETURN,
            "tab" => TAB,
            "esc" | "escape" => ESCAPE,
            "space" => SPACE,
            "backspace" => BACK,
            "delete" | "del" => DELETE,
            "insert" | "ins" => INSERT,
            "home" => HOME,
            "end" => END,
            "pgup" | "pageup" => PRIOR,
            "pgdn" | "pagedown" => NEXT,
            "left" => LEFT,
            "right" => RIGHT,
            "up" => UP,
            "down" => DOWN,
            "printscreen" | "prtsc" => SNAPSHOT,
            "capslock" | "caps" => CAPITAL,
            "pause" => PAUSE,
            "scrolllock" => SCROLL,
            "menu" | "apps" => APPS,
            "mute" => VOLUME_MUTE,
            _ => {
                if let Some((_, k)) = OEM.iter().find(|(s, _)| *s == n) {
                    return Some(*k);
                }
                if let Some(d) = n.strip_prefix("num").and_then(|d| d.parse::<u16>().ok()) {
                    return (d <= 9).then_some(NUMPAD0 + d);
                }
                let b = n.as_bytes();
                if b.len() == 1 && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) {
                    b[0].to_ascii_uppercase() as u16
                } else if let Some(num) = n.strip_prefix('f').and_then(|d| d.parse::<u16>().ok()) {
                    if (1..=24).contains(&num) {
                        F1 + num - 1
                    } else {
                        return None;
                    }
                } else {
                    return None;
                }
            }
        };
        Some(k)
    }

    /// Обратное к `from_name`; `None` — у клавиши нет имени (её нельзя записать в файл).
    pub fn name(k: u16) -> Option<String> {
        let s = match k {
            CONTROL | LCONTROL | RCONTROL => "ctrl",
            SHIFT | LSHIFT | RSHIFT => "shift",
            MENU | LMENU | RMENU => "alt",
            LWIN | RWIN => "win",
            RETURN => "enter",
            TAB => "tab",
            ESCAPE => "esc",
            SPACE => "space",
            BACK => "backspace",
            DELETE => "delete",
            INSERT => "insert",
            HOME => "home",
            END => "end",
            PRIOR => "pgup",
            NEXT => "pgdn",
            LEFT => "left",
            RIGHT => "right",
            UP => "up",
            DOWN => "down",
            SNAPSHOT => "printscreen",
            CAPITAL => "capslock",
            PAUSE => "pause",
            SCROLL => "scrolllock",
            APPS => "menu",
            VOLUME_MUTE => "mute",
            0x30..=0x39 | 0x41..=0x5A => return Some(((k as u8) as char).to_ascii_lowercase().to_string()),
            k if (NUMPAD0..NUMPAD0 + 10).contains(&k) => return Some(format!("num{}", k - NUMPAD0)),
            k if (F1..F1 + 24).contains(&k) => return Some(format!("f{}", k - F1 + 1)),
            k => return OEM.iter().find(|(_, c)| *c == k).map(|(s, _)| s.to_string()),
        };
        Some(s.to_string())
    }

    /// Разобрать `ctrl+shift+esc` в коды по порядку.
    pub fn parse_list(s: &str) -> Result<Vec<u16>, String> {
        if s.trim().is_empty() {
            return Err("пустое сочетание клавиш".into());
        }
        // «+» как разделитель; сама клавиша «=» тоже пишется знаком
        s.split('+')
            .map(|k| from_name(k).ok_or_else(|| format!("неизвестная клавиша «{}»", k.trim())))
            .collect()
    }
}

/// Сочетание для вызова меню: модификаторы + одна обычная клавиша (её держат, пока меню открыто).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub key: u16,
}

impl Chord {
    pub fn parse(s: &str) -> Result<Chord, String> {
        let keys = vk::parse_list(s)?;
        let mut c = Chord {
            ctrl: false,
            alt: false,
            shift: false,
            win: false,
            key: 0,
        };
        for k in keys {
            match k {
                vk::CONTROL => c.ctrl = true,
                vk::MENU => c.alt = true,
                vk::SHIFT => c.shift = true,
                vk::LWIN => c.win = true,
                k if c.key == 0 => c.key = k,
                _ => return Err("в сочетании должна быть одна обычная клавиша".into()),
            }
        }
        if c.key == 0 {
            return Err("нужна обычная клавиша, не только Ctrl/Alt/Shift/Win".into());
        }
        Ok(c)
    }

    /// Из нажатия: модификаторы в момент нажатия + сама клавиша. `None` — клавиша-модификатор
    /// или без имени.
    pub fn from_press(key: u16, ctrl: bool, alt: bool, shift: bool, win: bool) -> Option<Chord> {
        if vk::is_modifier(key) || vk::name(key).is_none() {
            return None;
        }
        Some(Chord {
            ctrl,
            alt,
            shift,
            win,
            key,
        })
    }

    /// Для файла настроек: `ctrl+alt+space`.
    pub fn to_text(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for (on, n) in [(self.ctrl, "ctrl"), (self.alt, "alt"), (self.shift, "shift"), (self.win, "win")] {
            if on {
                parts.push(n.into());
            }
        }
        parts.push(vk::name(self.key).unwrap_or_else(|| format!("0x{:02x}", self.key)));
        parts.join("+")
    }

    /// Для людей: `Ctrl+Alt+Space`.
    pub fn display(&self) -> String {
        self.to_text()
            .split('+')
            .map(|p| {
                let mut c = p.chars();
                match c.next() {
                    Some(f) => f.to_uppercase().chain(c).collect(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join("+")
    }
}

/// Медиаклавиши, доступные из меню.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Media {
    Prev,
    Next,
    PlayPause,
    Mute,
}

impl Media {
    pub fn vk(self) -> u16 {
        match self {
            Media::Prev => vk::MEDIA_PREV,
            Media::Next => vk::MEDIA_NEXT,
            Media::PlayPause => vk::MEDIA_PLAY_PAUSE,
            Media::Mute => vk::VOLUME_MUTE,
        }
    }

    pub fn from_key(s: &str) -> Option<Media> {
        match s.trim() {
            "prev" | "previous" => Some(Media::Prev),
            "next" => Some(Media::Next),
            "play-pause" | "play" | "pause" => Some(Media::PlayPause),
            "mute" => Some(Media::Mute),
            _ => None,
        }
    }
}

/// Запуск программы.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Run {
    /// Путь к программе, файлу или просто имя (`notepad`, `code`) — ищется как в «Выполнить».
    pub path: String,
    /// Аргументы одной строкой (как в командной строке).
    pub args: String,
    /// Рабочая папка (пусто — папка программы).
    pub cwd: String,
    /// Запросить права администратора (UAC).
    pub admin: bool,
}

/// Шаг цепочки: действие и пауза после него.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub action: Option<Action>,
    pub wait_ms: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Аккорд: зажать клавиши по порядку, отпустить в обратном (Ctrl+C, Win+V…).
    Keys(Vec<u16>),
    /// Напечатать текст (как будто его набрали).
    Text(String),
    /// Запустить программу.
    Run(Run),
    /// Открыть ссылку, протокол, файл или папку (`https://…`, `ms-settings:`, `D:\Отчёты`).
    Open(String),
    /// Медиаклавиша.
    Media(Media),
    /// Шаг громкости (`true` — громче).
    Volume(bool),
    /// Открыть Проводник.
    Explorer,
    /// Окно быстрой заметки.
    Note,
    /// Окно настроек.
    Settings,
    /// Несколько действий по очереди.
    Steps(Vec<Step>),
}

impl Action {
    pub fn keys(codes: &[u16]) -> Action {
        Action::Keys(codes.to_vec())
    }

    pub fn copy() -> Action {
        Action::keys(&[vk::CONTROL, vk::C])
    }
    pub fn paste() -> Action {
        Action::keys(&[vk::CONTROL, vk::V])
    }
    pub fn cut() -> Action {
        Action::keys(&[vk::CONTROL, vk::X])
    }
    pub fn select_all() -> Action {
        Action::keys(&[vk::CONTROL, vk::A])
    }
    pub fn clipboard_history() -> Action {
        Action::keys(&[vk::LWIN, vk::V])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_names_roundtrip() {
        for n in [
            "ctrl", "alt", "shift", "win", "a", "z", "0", "f1", "f24", "enter", "space", "capslock",
            "num5", ";", "/", "[", "pgdn", "menu", "mute",
        ] {
            let k = vk::from_name(n).unwrap_or_else(|| panic!("{n}"));
            assert_eq!(vk::name(k).as_deref(), Some(n), "{n}");
        }
        assert_eq!(vk::from_name("F12"), Some(vk::F1 + 11));
        assert_eq!(vk::from_name("f25"), None);
        assert_eq!(vk::from_name("num10"), None);
        assert!(vk::is_extended(vk::LEFT) && !vk::is_extended(vk::A));
    }

    #[test]
    fn key_lists() {
        assert_eq!(vk::parse_list("ctrl+v").unwrap(), [vk::CONTROL, vk::V]);
        assert_eq!(vk::parse_list("Win + V").unwrap(), [vk::LWIN, vk::V]);
        assert!(vk::parse_list("ctrl+щ").unwrap_err().contains("щ"));
        assert!(vk::parse_list(" ").is_err());
    }

    #[test]
    fn chords() {
        let c = Chord::parse("ctrl+alt+space").unwrap();
        assert!(c.ctrl && c.alt && !c.shift && !c.win && c.key == vk::SPACE);
        assert_eq!(c.to_text(), "ctrl+alt+space");
        assert_eq!(c.display(), "Ctrl+Alt+Space");
        assert_eq!(Chord::parse(&c.to_text()).unwrap(), c);
        assert_eq!(Chord::parse("f13").unwrap().key, vk::F1 + 12);
        assert!(Chord::parse("ctrl+alt").is_err());
        assert!(Chord::parse("a+b").is_err());
        assert_eq!(Chord::from_press(vk::LCONTROL, true, false, false, false), None);
        let p = Chord::from_press(vk::from_name("q").unwrap(), false, true, false, false).unwrap();
        assert_eq!(p.to_text(), "alt+q");
    }

    #[test]
    fn media_names() {
        assert_eq!(Media::from_key("next"), Some(Media::Next));
        assert_eq!(Media::from_key("play-pause"), Some(Media::PlayPause));
        assert_eq!(Media::from_key("louder"), None);
    }
}

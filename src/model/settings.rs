//! Пользовательские настройки: простой файл `ключ = значение` (комментарии — `#`).

use super::action::Chord;
use super::theme::ThemeMode;

pub const SENS_RANGE: (u32, u32) = (20, 300);
pub const SIZE_RANGE: (u32, u32) = (50, 150);
pub const DWELL_RANGE: (u32, u32) = (100, 2000);

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Боковая кнопка мыши: 0 — не использовать, 1 — «назад» (XButton1), 2 — «вперёд» (XButton2).
    pub button: u8,
    /// Сочетание клавиш для меню (держать, пока меню открыто); `None` — не задано.
    pub hotkey: Option<Chord>,
    /// Папка хранилища Obsidian, куда пишутся заметки Ring_NN.md (пусто — не задана).
    pub vault: String,
    /// Чувствительность движения мыши внутри меню, %.
    pub sens_percent: u32,
    /// Размер меню, %.
    pub size_percent: u32,
    /// Задержка раскрытия «Копировать»/«Вставить», мс.
    pub dwell_ms: u32,
    /// Тема: как в системе, светлая или тёмная.
    pub theme: ThemeMode,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            button: 1,
            hotkey: None,
            vault: String::new(),
            sens_percent: 100,
            size_percent: 100,
            dwell_ms: 450,
            theme: ThemeMode::Auto,
        }
    }
}

impl Settings {
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim(), v.trim().trim_matches('"'));
            match k {
                "button" => s.button = v.parse().unwrap_or(s.button),
                "hotkey" => s.hotkey = Chord::parse(v).ok(),
                "vault" => s.vault = v.to_string(),
                "sens_percent" => s.sens_percent = v.parse().unwrap_or(s.sens_percent),
                "size_percent" => s.size_percent = v.parse().unwrap_or(s.size_percent),
                "dwell_ms" => s.dwell_ms = v.parse().unwrap_or(s.dwell_ms),
                "theme" => s.theme = ThemeMode::from_key(v).unwrap_or(s.theme),
                _ => {}
            }
        }
        s.normalize();
        s
    }

    pub fn normalize(&mut self) {
        if self.button > 2 {
            self.button = 1;
        }
        // меню должно открываться хоть чем-то
        if self.button == 0 && self.hotkey.is_none() {
            self.button = 1;
        }
        self.sens_percent = self.sens_percent.clamp(SENS_RANGE.0, SENS_RANGE.1);
        self.size_percent = self.size_percent.clamp(SIZE_RANGE.0, SIZE_RANGE.1);
        self.dwell_ms = self.dwell_ms.clamp(DWELL_RANGE.0, DWELL_RANGE.1);
    }

    /// Применить параметры командной строки (`--button N`, `--hotkey СОЧЕТАНИЕ`, `--vault ПУТЬ`).
    pub fn apply_args<I: IntoIterator<Item = String>>(&mut self, args: I) {
        let args: Vec<String> = args.into_iter().collect();
        for w in args.windows(2) {
            match w[0].as_str() {
                "--button" => {
                    if let Ok(b) = w[1].parse() {
                        self.button = b;
                    }
                }
                "--hotkey" => self.hotkey = Chord::parse(&w[1]).ok(),
                "--vault" => self.vault = w[1].clone(),
                _ => {}
            }
        }
        self.normalize();
    }

    /// Нужно ли пересобрать меню (размеры, подписи), если настройки сменились на `other`.
    pub fn affects_layout(&self, other: &Settings) -> bool {
        self.size_percent != other.size_percent
            || self.sens_percent != other.sens_percent
            || self.dwell_ms != other.dwell_ms
    }

    pub fn to_text(&self) -> String {
        format!(
            "# Радиальное меню (удобнее менять через «Настройки» в трее)\n\
             # button: 0 = не использовать, 1 = боковая «назад», 2 = боковая «вперёд»\nbutton = {}\n\
             # сочетание клавиш для меню (держать, пока меню открыто), например ctrl+alt+space; пусто — нет\nhotkey = {}\n\
             # папка хранилища Obsidian для заметок\nvault = \"{}\"\n\
             # чувствительность, % ({}..{})\nsens_percent = {}\n\
             # размер меню, % ({}..{})\nsize_percent = {}\n\
             # задержка раскрытия подменю, мс ({}..{})\ndwell_ms = {}\n\
             # тема: auto (как в системе), light, dark\ntheme = {}\n",
            self.button,
            self.hotkey.map(|c| c.to_text()).unwrap_or_default(),
            self.vault,
            SENS_RANGE.0,
            SENS_RANGE.1,
            self.sens_percent,
            SIZE_RANGE.0,
            SIZE_RANGE.1,
            self.size_percent,
            DWELL_RANGE.0,
            DWELL_RANGE.1,
            self.dwell_ms,
            self.theme.key(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let s = Settings {
            button: 2,
            vault: "D:/Notes".into(),
            sens_percent: 150,
            theme: ThemeMode::Light,
            hotkey: Chord::parse("ctrl+alt+space").ok(),
            ..Settings::default()
        };
        assert_eq!(Settings::parse(&s.to_text()), s);
        assert_eq!(Settings::parse(&Settings::default().to_text()), Settings::default());
    }

    #[test]
    fn clamps_and_ignores_garbage() {
        let s = Settings::parse("button = 9\nsens_percent = 5000\nmusor\ndwell_ms = abc\ntheme = neon\n");
        assert_eq!(s.button, 1);
        assert_eq!(s.sens_percent, 300);
        assert_eq!(s.dwell_ms, 450);
        assert_eq!(s.theme, ThemeMode::Auto);
    }

    #[test]
    fn old_files_keep_working() {
        // файл прежней версии: без темы, с заданным хранилищем
        let s = Settings::parse("button = 2\nvault = \"C:/Users/HP/Work\"\n");
        assert_eq!((s.button, s.vault.as_str(), s.theme), (2, "C:/Users/HP/Work", ThemeMode::Auto));
    }

    #[test]
    fn default_vault_is_not_set() {
        assert!(Settings::default().vault.is_empty());
    }

    #[test]
    fn args_override() {
        let mut s = Settings::default();
        s.apply_args(["--button", "2", "--vault", "E:/V"].map(String::from));
        assert_eq!((s.button, s.vault.as_str()), (2, "E:/V"));
        s.apply_args(["--hotkey", "f13"].map(String::from));
        assert_eq!(s.hotkey.unwrap().to_text(), "f13");
    }

    #[test]
    fn some_trigger_always_stays_on() {
        let s = Settings::parse("button = 0\n");
        assert_eq!(s.button, 1, "без сочетания мышь не отключается");
        let s = Settings::parse("button = 0\nhotkey = alt+q\n");
        assert_eq!((s.button, s.hotkey.unwrap().to_text().as_str()), (0, "alt+q"));
        let s = Settings::parse("hotkey = ctrl+alt\n");
        assert_eq!(s.hotkey, None, "одни модификаторы — не сочетание");
    }
}

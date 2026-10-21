//! Когда меню не должно вмешиваться: игры и другие программы, где боковые кнопки и сочетания
//! заняты своими назначениями. Только решение «пропустить ли нажатие» — без обращений к ОС:
//! платформа описывает активное окно (`Foreground`), а правило здесь, поэтому оно проверяется тестами.

/// Программы, которые бывают на весь экран, но играми не являются: в них меню нужно
/// (полноэкранное видео — те же кнопки «Пауза» и громкость). Имена в нижнем регистре.
pub const NOT_GAMES: &[&str] = &[
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "vivaldi.exe",
    "yandex.exe",
    "vlc.exe",
    "mpv.exe",
    "mpc-hc.exe",
    "mpc-hc64.exe",
    "mpc-be.exe",
    "mpc-be64.exe",
    "potplayermini64.exe",
    "potplayermini.exe",
    "wmplayer.exe",
    "explorer.exe",
];

/// Что известно об активном окне.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Foreground {
    /// Имя exe в нижнем регистре (`game.exe`); `None` — не удалось узнать.
    pub exe: Option<String>,
    /// Окно без рамки и заголовка закрывает весь монитор (полноэкранный или «безрамочный» режим).
    pub fullscreen: bool,
    /// Windows считает, что идёт полноэкранное приложение Direct3D (эксклюзивный режим игр).
    pub d3d_exclusive: bool,
}

/// Правила обхода. `Default` — как в новой установке: полноэкранные игры пропускаются.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GamePolicy {
    /// Не перехватывать вызов меню, когда активна полноэкранная программа.
    pub ignore_fullscreen: bool,
    /// Программы, в которых меню не вызывается никогда (имена exe, нижний регистр).
    pub ignore_apps: Vec<String>,
}

impl Default for GamePolicy {
    fn default() -> Self {
        GamePolicy {
            ignore_fullscreen: true,
            ignore_apps: Vec::new(),
        }
    }
}

/// Привести имя к виду списка: без пути, в нижнем регистре.
pub fn normalize_name(s: &str) -> String {
    let base = s
        .trim()
        .trim_matches('"')
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("");
    base.trim().to_lowercase()
}

/// Разобрать список из файла настроек: `a.exe, b.exe; C:\Games\c.exe`.
pub fn parse_list(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in s.split([',', ';']) {
        let name = normalize_name(part);
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

impl GamePolicy {
    /// Нужно ли вообще смотреть на активное окно (иначе правило заведомо «нет»).
    pub fn is_active(&self) -> bool {
        self.ignore_fullscreen || !self.ignore_apps.is_empty()
    }

    /// `true` — меню не вызывать, нажатие отдать программе как есть.
    pub fn bypass(&self, fg: &Foreground) -> bool {
        let exe = fg.exe.as_deref();
        if exe.is_some_and(|e| self.ignore_apps.iter().any(|a| a == e)) {
            return true; // явный список главнее всего
        }
        if !self.ignore_fullscreen || exe.is_some_and(|e| NOT_GAMES.contains(&e)) {
            return false;
        }
        fg.fullscreen || fg.d3d_exclusive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fg(exe: &str, fullscreen: bool, d3d: bool) -> Foreground {
        Foreground {
            exe: Some(exe.into()),
            fullscreen,
            d3d_exclusive: d3d,
        }
    }

    #[test]
    fn ordinary_windows_are_left_alone() {
        let p = GamePolicy::default();
        assert!(!p.bypass(&fg("code.exe", false, false)));
        assert!(
            !p.bypass(&Foreground::default()),
            "окно неизвестно — меню работает"
        );
    }

    #[test]
    fn fullscreen_games_are_skipped() {
        let p = GamePolicy::default();
        assert!(
            p.bypass(&fg("game.exe", true, false)),
            "безрамочный полный экран"
        );
        assert!(
            p.bypass(&fg("game.exe", false, true)),
            "эксклюзивный режим Direct3D"
        );
    }

    #[test]
    fn fullscreen_video_and_browsers_keep_the_menu() {
        let p = GamePolicy::default();
        assert!(!p.bypass(&fg("chrome.exe", true, false)));
        assert!(!p.bypass(&fg("vlc.exe", true, true)));
    }

    #[test]
    fn explicit_list_beats_everything() {
        let p = GamePolicy {
            ignore_fullscreen: false,
            ignore_apps: vec!["chrome.exe".into(), "game.exe".into()],
        };
        assert!(
            p.bypass(&fg("game.exe", false, false)),
            "в списке — всегда, даже в окне"
        );
        assert!(
            p.bypass(&fg("chrome.exe", true, false)),
            "список главнее исключений для браузеров"
        );
        assert!(
            !p.bypass(&fg("other.exe", true, true)),
            "полный экран выключен — не смотрим"
        );
    }

    #[test]
    fn switched_off_policy_is_inactive() {
        let p = GamePolicy {
            ignore_fullscreen: false,
            ignore_apps: vec![],
        };
        assert!(!p.is_active());
        assert!(!p.bypass(&fg("game.exe", true, true)));
        assert!(GamePolicy::default().is_active());
    }

    #[test]
    fn list_parsing_normalises_names() {
        assert_eq!(
            parse_list(" Game.EXE, C:\\Games\\Dota2\\dota2.exe ;\"D:/x/Y.exe\",,game.exe"),
            ["game.exe", "dota2.exe", "y.exe"]
        );
        assert!(parse_list("  ,; ").is_empty());
    }
}

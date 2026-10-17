//! Тема системы: светлая или тёмная. Для каждой ОС — свой источник:
//! * Windows — `AppsUseLightTheme` в реестре (событие смены — `WM_SETTINGCHANGE`);
//! * macOS — `defaults read -g AppleInterfaceStyle`;
//! * Linux и прочие Unix — `gsettings` (GNOME и всё на GTK), затем `kdeglobals` (KDE),
//!   затем переменная `GTK_THEME`.
//!
//! Где нет события о смене темы, `Follower::refresh` просто зовут периодически.

use crate::model::{Theme, ThemeMode};

/// Тёмная ли тема у ОС; `None` — узнать не удалось.
pub fn system_prefers_dark() -> Option<bool> {
    imp::detect()
}

/// Следит за итоговой темой: пересчитывает её при смене выбора или темы системы.
pub struct Follower {
    mode: ThemeMode,
    current: Theme,
}

impl Follower {
    pub fn new(mode: ThemeMode) -> Follower {
        Follower::with_detector(mode, system_prefers_dark)
    }

    pub fn with_detector(mode: ThemeMode, detect: impl Fn() -> Option<bool>) -> Follower {
        Follower {
            mode,
            current: mode.resolve(detect()),
        }
    }

    pub fn theme(&self) -> Theme {
        self.current
    }

    /// Пересчитать (выбор пользователя мог смениться); `Some` — тема изменилась.
    pub fn refresh(&mut self, mode: ThemeMode) -> Option<Theme> {
        self.refresh_with(mode, system_prefers_dark)
    }

    pub fn refresh_with(&mut self, mode: ThemeMode, detect: impl Fn() -> Option<bool>) -> Option<Theme> {
        self.mode = mode;
        // систему спрашиваем, только если тема от неё зависит
        let system = if mode == ThemeMode::Auto { detect() } else { None };
        let t = self.mode.resolve(system);
        (t != std::mem::replace(&mut self.current, t)).then_some(t)
    }
}

/// Ответ `gsettings get org.gnome.desktop.interface color-scheme` (или `gtk-theme`).
pub fn parse_gsettings(out: &str) -> Option<bool> {
    let v = out.trim().trim_matches('\'').to_ascii_lowercase();
    match v.as_str() {
        "prefer-dark" => Some(true),
        "prefer-light" => Some(false),
        "default" | "" => None, // «по умолчанию» ничего не говорит — смотрим дальше
        theme => Some(theme.contains("dark")), // имя GTK-темы: Adwaita-dark, Yaru-dark…
    }
}

/// `~/.config/kdeglobals`: схема цветов из секции `[General]`.
pub fn parse_kdeglobals(text: &str) -> Option<bool> {
    let mut general = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            general = line == "[General]";
        } else if general {
            if let Some(v) = line.strip_prefix("ColorScheme=") {
                return Some(v.to_ascii_lowercase().contains("dark"));
            }
        }
    }
    None
}

/// Ответ `defaults read -g AppleInterfaceStyle`: ключ есть только у тёмной темы.
pub fn parse_macos(out: &str, success: bool) -> Option<bool> {
    Some(success && out.trim().eq_ignore_ascii_case("dark"))
}

#[cfg(windows)]
mod imp {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

    pub fn detect() -> Option<bool> {
        let mut value: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
                w!("AppsUseLightTheme"),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut value as *mut u32 as *mut _),
                Some(&mut size),
            )
        };
        r.is_ok().then_some(value == 0)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    pub fn detect() -> Option<bool> {
        let out = std::process::Command::new("defaults")
            .args(["read", "-g", "AppleInterfaceStyle"])
            .output()
            .ok()?;
        super::parse_macos(&String::from_utf8_lossy(&out.stdout), out.status.success())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use super::{parse_gsettings, parse_kdeglobals};

    fn gsettings(key: &str) -> Option<bool> {
        let out = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", key])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        parse_gsettings(&String::from_utf8_lossy(&out.stdout))
    }

    pub fn detect() -> Option<bool> {
        gsettings("color-scheme")
            .or_else(|| gsettings("gtk-theme"))
            .or_else(|| {
                let home = std::env::var_os("XDG_CONFIG_HOME")
                    .map(std::path::PathBuf::from)
                    .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
                parse_kdeglobals(&std::fs::read_to_string(home.join("kdeglobals")).ok()?)
            })
            .or_else(|| std::env::var("GTK_THEME").ok().map(|t| t.to_ascii_lowercase().contains("dark")))
    }
}

#[cfg(not(any(windows, unix)))]
mod imp {
    pub fn detect() -> Option<bool> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gsettings_answers() {
        assert_eq!(parse_gsettings("'prefer-dark'\n"), Some(true));
        assert_eq!(parse_gsettings("'prefer-light'"), Some(false));
        assert_eq!(parse_gsettings("'default'"), None);
        assert_eq!(parse_gsettings("'Adwaita-dark'"), Some(true));
        assert_eq!(parse_gsettings("'Yaru'"), Some(false));
    }

    #[test]
    fn kde_and_macos_answers() {
        let kde = "[Colors:View]\nColorScheme=Nope\n[General]\nColorScheme=BreezeDark\n";
        assert_eq!(parse_kdeglobals(kde), Some(true));
        assert_eq!(parse_kdeglobals("[General]\nColorScheme=BreezeLight\n"), Some(false));
        assert_eq!(parse_kdeglobals("[General]\nName=x\n"), None);
        assert_eq!(parse_macos("Dark\n", true), Some(true));
        assert_eq!(parse_macos("", false), Some(false)); // ключа нет — светлая
    }

    #[test]
    fn follower_reports_only_changes() {
        let mut f = Follower::with_detector(ThemeMode::Auto, || Some(true));
        assert_eq!(f.theme(), Theme::Dark);
        assert_eq!(f.refresh_with(ThemeMode::Auto, || Some(true)), None);
        assert_eq!(f.refresh_with(ThemeMode::Auto, || Some(false)), Some(Theme::Light));
        assert_eq!(f.refresh_with(ThemeMode::Light, || Some(true)), None); // явный выбор важнее системы
        assert_eq!(f.refresh_with(ThemeMode::Dark, || None), Some(Theme::Dark));
    }

    #[test]
    fn detection_does_not_panic_here() {
        let _ = system_prefers_dark();
    }
}

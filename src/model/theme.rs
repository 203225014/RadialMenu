//! Тема оформления: что выбрал пользователь и что в итоге показываем.

/// Готовая тема для отрисовки.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Theme {
    Light,
    #[default]
    Dark,
}

impl Theme {
    pub const ALL: [Theme; 2] = [Theme::Light, Theme::Dark];

    pub fn is_dark(self) -> bool {
        self == Theme::Dark
    }
}

/// Выбор в настройках: «как в системе» или явно.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    Auto,
    Light,
    Dark,
}

impl ThemeMode {
    pub const ALL: [ThemeMode; 3] = [ThemeMode::Auto, ThemeMode::Light, ThemeMode::Dark];

    /// `system_dark` — тёмная ли тема у ОС (`None` — неизвестно, тогда тёмная).
    pub fn resolve(self, system_dark: Option<bool>) -> Theme {
        match self {
            ThemeMode::Light => Theme::Light,
            ThemeMode::Dark => Theme::Dark,
            ThemeMode::Auto => {
                if system_dark.unwrap_or(true) {
                    Theme::Dark
                } else {
                    Theme::Light
                }
            }
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            ThemeMode::Auto => "auto",
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        }
    }

    pub fn from_key(s: &str) -> Option<ThemeMode> {
        ThemeMode::ALL.into_iter().find(|m| m.key() == s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_follows_system() {
        assert_eq!(ThemeMode::Auto.resolve(Some(false)), Theme::Light);
        assert_eq!(ThemeMode::Auto.resolve(Some(true)), Theme::Dark);
        assert_eq!(ThemeMode::Auto.resolve(None), Theme::Dark);
        assert_eq!(ThemeMode::Light.resolve(Some(true)), Theme::Light);
        assert_eq!(ThemeMode::Dark.resolve(Some(false)), Theme::Dark);
    }

    #[test]
    fn keys_roundtrip() {
        for m in ThemeMode::ALL {
            assert_eq!(ThemeMode::from_key(m.key()), Some(m));
        }
        assert_eq!(ThemeMode::from_key("purple"), None);
    }
}

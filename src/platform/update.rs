//! Проверка обновлений: разбор ответа GitHub Releases и сравнение версий. Без сети и ОС —
//! запрос делает платформа (`win32::system::http`), поэтому логика проверяется тестами везде.

/// Репозиторий, в котором выходят релизы.
pub const REPO: &str = "203225014/RadialMenu";

/// Адрес последнего релиза (GitHub API; репозиторий должен быть публичным).
pub fn latest_url() -> String {
    format!("https://api.github.com/repos/{REPO}/releases/latest")
}

/// Версия вида `1.2.3` (лишние части после третьей и суффикс `-beta` отбрасываются).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// `v0.7.0`, `0.7`, `0.7.0-beta.1` → версия; не число — `None`.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim().trim_start_matches(['v', 'V']);
        let core = s.split(['-', '+']).next()?;
        let mut parts = core.split('.');
        let mut next = || -> Option<u32> {
            match parts.next() {
                Some(p) => p.parse().ok(),
                None => Some(0),
            }
        };
        let v = Version(next()?, next()?, next()?);
        // пустая строка или «v» без цифр
        (!core.is_empty()).then_some(v)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// Что нужно знать о выпущенной версии.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// Страница релиза.
    pub page: String,
    /// Прямая ссылка на установщик (`RadialMenu-Setup-*.exe`), если он приложен.
    pub installer: Option<String>,
}

impl Release {
    /// Куда вести пользователя: сразу установщик, а нет его — страница релиза.
    pub fn download_url(&self) -> &str {
        self.installer.as_deref().unwrap_or(&self.page)
    }
}

/// Разбор ответа `releases/latest`. Черновики и пререлизы GitHub в этот ответ не отдаёт.
pub fn parse_release(json: &str) -> Option<Release> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let version = Version::parse(v.get("tag_name")?.as_str()?)?;
    let page = v.get("html_url")?.as_str()?.to_string();
    let installer = v.get("assets").and_then(|a| a.as_array()).and_then(|assets| {
        assets.iter().find_map(|a| {
            let name = a.get("name")?.as_str()?;
            (name.starts_with("RadialMenu-Setup-") && name.ends_with(".exe"))
                .then(|| a.get("browser_download_url")?.as_str().map(str::to_string))
                .flatten()
        })
    });
    Some(Release { version, page, installer })
}

/// Версия этой сборки.
pub fn current() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version(0, 0, 0))
}

/// Вышла ли версия новее текущей.
pub fn is_newer(latest: Version, current: Version) -> bool {
    latest > current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_and_compare() {
        assert_eq!(Version::parse("v0.7.0"), Some(Version(0, 7, 0)));
        assert_eq!(Version::parse("1.2"), Some(Version(1, 2, 0)));
        assert_eq!(Version::parse("0.8.0-beta.1"), Some(Version(0, 8, 0)));
        assert_eq!(Version::parse(" V2.10.3 "), Some(Version(2, 10, 3)));
        assert_eq!(Version::parse("abc"), None);
        assert_eq!(Version::parse(""), None);
        assert_eq!(Version::parse("v"), None);
        // числа, а не строки: 0.10 новее 0.9
        assert!(is_newer(Version(0, 10, 0), Version(0, 9, 5)));
        assert!(!is_newer(Version(0, 7, 0), Version(0, 7, 0)));
        assert!(!is_newer(Version(0, 6, 9), Version(0, 7, 0)));
        assert_eq!(Version(0, 7, 0).to_string(), "0.7.0");
        assert!(current() >= Version(0, 6, 0));
    }

    const SAMPLE: &str = r#"{
        "tag_name": "v0.8.0",
        "html_url": "https://github.com/203225014/RadialMenu/releases/tag/v0.8.0",
        "assets": [
            { "name": "RadialMenu-0.8.0-portable.zip", "browser_download_url": "https://x/zip" },
            { "name": "RadialMenu-Setup-0.8.0.exe", "browser_download_url": "https://x/setup.exe" },
            { "name": "SHA256SUMS.txt", "browser_download_url": "https://x/sums" }
        ]
    }"#;

    #[test]
    fn release_json_is_parsed() {
        let r = parse_release(SAMPLE).unwrap();
        assert_eq!(r.version, Version(0, 8, 0));
        assert_eq!(r.installer.as_deref(), Some("https://x/setup.exe"));
        assert_eq!(r.download_url(), "https://x/setup.exe");
        assert!(r.page.ends_with("/tag/v0.8.0"));
    }

    #[test]
    fn release_without_installer_points_to_its_page() {
        let r = parse_release(r#"{"tag_name":"v1.0.0","html_url":"https://p","assets":[]}"#).unwrap();
        assert_eq!(r.installer, None);
        assert_eq!(r.download_url(), "https://p");
    }

    #[test]
    fn garbage_is_rejected() {
        assert_eq!(parse_release("not json"), None);
        assert_eq!(parse_release(r#"{"message":"Not Found"}"#), None); // приватный репозиторий
        assert_eq!(parse_release(r#"{"tag_name":"nightly","html_url":"u"}"#), None);
    }

    #[test]
    fn latest_url_points_to_the_repo() {
        assert_eq!(latest_url(), "https://api.github.com/repos/203225014/RadialMenu/releases/latest");
    }
}

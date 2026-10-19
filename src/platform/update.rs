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
    /// Имя файла установщика — по нему ищется контрольная сумма.
    pub installer_name: Option<String>,
    /// Прямая ссылка на `SHA256SUMS.txt` из того же релиза.
    pub sums: Option<String>,
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
    let assets = v.get("assets").and_then(|a| a.as_array()).cloned().unwrap_or_default();
    let find = |pick: &dyn Fn(&str) -> bool| -> Option<(String, String)> {
        assets.iter().find_map(|a| {
            let name = a.get("name")?.as_str()?;
            pick(name)
                .then(|| Some((name.to_string(), a.get("browser_download_url")?.as_str()?.to_string())))
                .flatten()
        })
    };
    let setup = find(&|n| n.starts_with("RadialMenu-Setup-") && n.ends_with(".exe"));
    let sums = find(&|n| n == "SHA256SUMS.txt").map(|(_, url)| url);
    let (installer_name, installer) = match setup {
        Some((n, u)) => (Some(n), Some(u)),
        None => (None, None),
    };
    Some(Release { version, page, installer, installer_name, sums })
}

/// Контрольная сумма файла `file` из `SHA256SUMS.txt` (строки `хэш  имя` или `хэш *имя`), строчными буквами.
pub fn sum_for(sums: &str, file: &str) -> Option<String> {
    sums.trim_start_matches('\u{feff}').lines().find_map(|line| {
        let (hash, name) = line.trim().split_once(char::is_whitespace)?;
        let name = name.trim().trim_start_matches('*');
        (name == file && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

/// Файлы для установки берём только из релизов нашего репозитория на github.com.
pub fn is_trusted_asset(url: &str) -> bool {
    url.starts_with(&format!("https://github.com/{REPO}/releases/download/"))
}

/// Хэш в виде строки из 64 шестнадцатеричных цифр.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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
    fn release_remembers_installer_name_and_sums() {
        let r = parse_release(SAMPLE).unwrap();
        assert_eq!(r.installer_name.as_deref(), Some("RadialMenu-Setup-0.8.0.exe"));
        assert_eq!(r.sums.as_deref(), Some("https://x/sums"));
    }

    #[test]
    fn sums_file_is_read() {
        let text = format!(
            "\u{feff}{}  RadialMenu-0.8.0-portable.zip\r\n{}  RadialMenu-Setup-0.8.0.exe\r\n",
            "a".repeat(64),
            "B".repeat(64)
        );
        assert_eq!(sum_for(&text, "RadialMenu-Setup-0.8.0.exe"), Some("b".repeat(64)));
        assert_eq!(sum_for(&text, "other.exe"), None);
        assert_eq!(sum_for("short  RadialMenu-Setup-0.8.0.exe", "RadialMenu-Setup-0.8.0.exe"), None);
        assert_eq!(hex(&[0, 255, 16]), "00ff10");
    }

    #[test]
    fn only_our_release_files_are_trusted() {
        assert!(is_trusted_asset("https://github.com/203225014/RadialMenu/releases/download/v0.9.0/RadialMenu-Setup-0.9.0.exe"));
        assert!(!is_trusted_asset("https://github.com/evil/RadialMenu/releases/download/v1/x.exe"));
        assert!(!is_trusted_asset("http://github.com/203225014/RadialMenu/releases/download/v1/x.exe"));
        assert!(!is_trusted_asset("https://github.com.evil.com/203225014/RadialMenu/releases/download/x"));
    }

    #[test]
    fn latest_url_points_to_the_repo() {
        assert_eq!(latest_url(), "https://api.github.com/repos/203225014/RadialMenu/releases/latest");
    }
}

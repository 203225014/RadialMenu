//! Стандартные файлы меню: поставляются с программой, меняются нами, а правки пользователя не трогаются.
//!
//! Идея. Программа знает, какое стандартное меню она записала в файл (хеш лежит в `defaults.txt`). Пока файл не
//! менялся — при выходе новой версии со стандартным меню получше он тихо заменяется на новый стандарт. Стоит
//! пользователю хоть что-то поправить — файл больше никогда не перезаписывается обновлением. «Сбросить» в
//! настройках возвращает стандарт (прежний файл остаётся рядом как `.bak`).
//!
//! Здесь только решение «что делать с файлом» — без обращений к диску, поэтому оно проверяется тестами.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use super::menu_yaml::{NAV_TEMPLATE, TEMPLATE};

/// Какой стандарт соответствует файлу.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Основное меню (и новые меню «Мое меню N»).
    Main,
    /// Навигация жестами.
    Nav,
}

impl Kind {
    /// Текущий стандарт.
    pub fn text(self) -> &'static str {
        match self {
            Kind::Main => TEMPLATE,
            Kind::Nav => NAV_TEMPLATE,
        }
    }

    /// Хеши прежних стандартов, выпущенных раньше: файл, совпавший с одним из них, — тоже не менялся
    /// пользователем (его записали старые версии, когда о `defaults.txt` ещё не знали).
    /// Новый стандарт заменяет текущий — прежний при этом добавляется сюда (хеш печатает тест `legacy_is_complete`).
    fn legacy(self) -> &'static [&'static str] {
        match self {
            Kind::Main => &[
                "ca112a91d43dd3be", // 0.10.0
                "c3a668b3eed380ca", // 0.9.0 (иконки столов)
                "1d35c805863fa383", // 0.8.0
                "c5b1c0fb885d7f1d", // профили
                "5672b576d162f9ed", // меню секторами
                "bac9520dce92479c", // 0.6.0
            ],
            Kind::Nav => &[
                "265f0ceec24189ee", // 0.11.0: назад/вперёд по бокам, столы снизу
                "6756672d34a0e0d2", // 0.10.1
                "4632c2aee680ebc1", // 0.10.0
                "44a0fa0750d31620", // до слияния с 0.11.0 (не выпускалась)
            ],
        }
    }
}

/// Хеш текста с поправкой на то, что меняют редакторы: BOM, переводы строк, пробелы в конце строк и пустые строки в конце.
pub fn hash(text: &str) -> String {
    let t = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let t = t.lines().map(str::trim_end).collect::<Vec<_>>().join("\n");
    let digest = Sha256::digest(t.trim_end_matches('\n').as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Что делать с файлом меню при запуске.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Файла нет — записать стандарт.
    Create,
    /// Файл не менялся, а стандарт новее — заменить.
    Upgrade,
    /// Файл совпадает с текущим стандартом, но программа об этом не знала — запомнить.
    Adopt,
    /// Файл изменён пользователем (или уже актуален) — не трогать.
    Keep,
}

/// `on_disk` — текст файла (`None` — файла нет), `recorded` — хеш стандарта, который программа записала раньше.
pub fn plan(kind: Kind, on_disk: Option<&str>, recorded: Option<&str>) -> Plan {
    let Some(text) = on_disk else {
        return Plan::Create;
    };
    let h = hash(text);
    let current = hash(kind.text());
    if h == current {
        return if recorded == Some(current.as_str()) { Plan::Keep } else { Plan::Adopt };
    }
    let untouched = recorded == Some(h.as_str()) || kind.legacy().contains(&h.as_str());
    if untouched {
        Plan::Upgrade
    } else {
        Plan::Keep
    }
}

/// Файл совпадает с текущим стандартом (для подписи «стандартное» в настройках).
pub fn is_default(kind: Kind, text: &str) -> bool {
    hash(text) == hash(kind.text())
}

/// Журнал `defaults.txt`: имя файла → хеш стандарта, который программа в него записала.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ledger(BTreeMap<String, String>);

impl Ledger {
    pub fn parse(text: &str) -> Ledger {
        let mut m = BTreeMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let (k, v) = (k.trim(), v.trim());
                if !k.is_empty() && !v.is_empty() {
                    m.insert(k.to_string(), v.to_string());
                }
            }
        }
        Ledger(m)
    }

    pub fn to_text(&self) -> String {
        let mut s = String::from(
            "# Служебный файл программы: какие стандартные меню она записала. Править не нужно.\n\
             # Файл меню, который вы не меняли, обновляется вместе с программой; изменённый — никогда.\n",
        );
        for (k, v) in &self.0 {
            s.push_str(&format!("{k} = {v}\n"));
        }
        s
    }

    pub fn get(&self, file: &str) -> Option<&str> {
        self.0.get(file).map(String::as_str)
    }

    pub fn set(&mut self, file: &str, kind: Kind) {
        self.0.insert(file.to_string(), hash(kind.text()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_ignores_what_editors_change() {
        let a = "menu:\n  - { label: A }\n";
        assert_eq!(hash(a), hash("\u{feff}menu:\r\n  - { label: A }   \r\n\r\n\r\n"));
        assert_ne!(hash(a), hash("menu:\n  - { label: B }\n"));
    }

    #[test]
    fn missing_file_is_created() {
        assert_eq!(plan(Kind::Main, None, None), Plan::Create);
    }

    #[test]
    fn current_default_is_adopted_once_then_kept() {
        let cur = Kind::Main.text();
        assert_eq!(plan(Kind::Main, Some(cur), None), Plan::Adopt);
        let h = hash(cur);
        assert_eq!(plan(Kind::Main, Some(cur), Some(&h)), Plan::Keep);
    }

    #[test]
    fn untouched_old_default_is_upgraded() {
        // файл записан прежней версией (в журнале этот стандарт есть) — меняем на новый
        let old = "menu:\n  - { label: Старый стандарт }\n";
        assert_eq!(plan(Kind::Main, Some(old), Some(&hash(old))), Plan::Upgrade);
        // файл записан версией до появления журнала — его узнаём по списку прежних стандартов
        let v011 = include_str!("legacy/nav-0.11.0.yaml");
        assert_eq!(plan(Kind::Nav, Some(v011), None), Plan::Upgrade);
        assert_eq!(plan(Kind::Nav, Some(&v011.replace('\n', "\r\n")), None), Plan::Upgrade, "редактор мог сменить переводы строк");
    }

    #[test]
    fn user_edits_are_never_overwritten() {
        let old = "menu:\n  - { label: Старый стандарт }\n";
        let edited = format!("{old}  - {{ label: Моя кнопка, run: notepad }}\n");
        assert_eq!(plan(Kind::Main, Some(&edited), Some(&hash(old))), Plan::Keep, "правка поверх записанного стандарта");
        let v011 = include_str!("legacy/nav-0.11.0.yaml");
        let tweaked = v011.replace("Скриншот", "Снимок");
        assert_eq!(plan(Kind::Nav, Some(&tweaked), None), Plan::Keep, "правка поверх прежнего стандарта");
        assert_eq!(plan(Kind::Main, Some("menu: []\n"), None), Plan::Keep, "совсем чужой файл");
    }

    #[test]
    fn standards_differ_and_current_is_not_legacy() {
        assert_ne!(hash(Kind::Main.text()), hash(Kind::Nav.text()));
        for k in [Kind::Main, Kind::Nav] {
            assert!(!k.legacy().contains(&hash(k.text()).as_str()), "текущий стандарт {k:?} не может быть «прежним»");
        }
    }

    /// Выпуская новый стандарт, старый нужно добавить в `legacy` — иначе файлы, записанные версиями без
    /// журнала, останутся навсегда. Тест напоминает, какой хеш добавить.
    #[test]
    fn legacy_is_complete() {
        let v011 = include_str!("legacy/nav-0.11.0.yaml");
        assert!(Kind::Nav.legacy().contains(&hash(v011).as_str()), "хеш {} надо добавить в Kind::Nav.legacy()", hash(v011));
    }

    #[test]
    fn ledger_round_trips() {
        let mut l = Ledger::default();
        l.set("menu.yaml", Kind::Main);
        l.set("menu-2.yaml", Kind::Nav);
        let again = Ledger::parse(&l.to_text());
        assert_eq!(again, l);
        assert_eq!(again.get("menu-2.yaml"), Some(hash(Kind::Nav.text()).as_str()));
        assert_eq!(again.get("нет.yaml"), None);
    }

    #[test]
    fn default_detection() {
        assert!(is_default(Kind::Main, Kind::Main.text()));
        assert!(!is_default(Kind::Main, Kind::Nav.text()));
    }
}

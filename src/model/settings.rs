//! Пользовательские настройки: простой файл `ключ = значение` (комментарии — `#`).

use super::action::Chord;
use super::theme::ThemeMode;

/// Основное 2 меню (профиль) со своей кнопкой мыши и/или сочетанием; состав — в файле
/// `menu-{id}.yaml` рядом с `menu.yaml`. Основное меню — это `button`/`hotkey` настроек.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileCfg {
    /// 2..=4: по нему называется файл меню и ключи в config.txt.
    pub id: u8,
    /// 0 — мышь не используется, 1 — «назад», 2 — «вперёд».
    pub button: u8,
    pub hotkey: Option<Chord>,
    /// Название вкладки («Основное 2», «Мое меню 1»…); переименовывается в «Настройках».
    pub name: String,
}

/// Название основного меню по умолчанию.
pub const MAIN_NAME: &str = "Основное 1";

/// Самое длинное название меню, символов.
pub const NAME_MAX: usize = 24;

impl ProfileCfg {
    /// Меню с названием по умолчанию: «Основное 2» (навигация), дальше «Мое меню 1», «Мое меню 2».
    pub fn new(id: u8, button: u8) -> ProfileCfg {
        ProfileCfg { id, button, hotkey: None, name: Self::default_name(id) }
    }

    pub fn default_name(id: u8) -> String {
        if id == 2 { "Основное 2".to_string() } else { format!("Мое меню {}", id.saturating_sub(2)) }
    }

    pub fn file_name(&self) -> String {
        format!("menu-{}.yaml", self.id)
    }
}

/// Название меню без лишнего: пробелы по краям, переводы строк, длина; пусто — `None`.
pub fn clean_name(s: &str) -> Option<String> {
    let t: String = s.chars().filter(|c| !c.is_control()).take(NAME_MAX).collect();
    let t = t.trim().to_string();
    (!t.is_empty()).then_some(t)
}

/// Сколько профилей, кроме основного меню, можно завести.
pub const MAX_PROFILES: usize = 3;
const PROFILE_IDS: std::ops::RangeInclusive<u8> = 2..=4;

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
    /// Раз в сутки спрашивать у GitHub, не вышла ли новая версия.
    pub check_updates: bool,
    /// Название основного меню (его файл — menu.yaml).
    pub main_name: String,
    /// Остальные меню на другие кнопки/сочетания.
    pub profiles: Vec<ProfileCfg>,
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
            check_updates: true,
            main_name: MAIN_NAME.to_string(),
            // навигация жестами на боковую «вперёд» (меню — в menu-2.yaml)
            profiles: vec![ProfileCfg::new(2, 2)],
        }
    }
}

impl Settings {
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        // меню перечисляются в файле явно; в файле старой версии их нет вовсе — тогда остаётся «Основное 2»
        let mut explicit = false;
        s.profiles.clear();
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
                "check_updates" => {
                    s.check_updates = match v.to_ascii_lowercase().as_str() {
                        "0" | "false" | "no" | "off" => false,
                        "1" | "true" | "yes" | "on" => true,
                        _ => s.check_updates,
                    }
                }
                "profiles" => explicit = true,
                "main_name" => s.main_name = clean_name(v).unwrap_or_else(|| MAIN_NAME.to_string()),
                _ => {
                    if let Some((id, field)) = k.strip_prefix("profile").and_then(|r| {
                        let (num, field) = r.split_once('_').unwrap_or((r, ""));
                        Some((num.parse::<u8>().ok().filter(|n| PROFILE_IDS.contains(n))?, field))
                    }) {
                        explicit = true;
                        let at = match s.profiles.iter().position(|p| p.id == id) {
                            Some(i) => i,
                            None => {
                                s.profiles.push(ProfileCfg { id, button: 0, hotkey: None, name: ProfileCfg::default_name(id) });
                                s.profiles.len() - 1
                            }
                        };
                        let p = &mut s.profiles[at];
                        match field {
                            "button" => p.button = v.parse().unwrap_or(0),
                            "hotkey" => p.hotkey = Chord::parse(v).ok(),
                            "name" => p.name = clean_name(v).unwrap_or_else(|| ProfileCfg::default_name(id)),
                            _ => {}
                        }
                    }
                }
            }
        }
        if !explicit {
            s.profiles = Settings::default().profiles;
        }
        s.normalize();
        s
    }

    pub fn normalize(&mut self) {
        self.main_name = clean_name(&self.main_name).unwrap_or_else(|| MAIN_NAME.to_string());
        for p in &mut self.profiles {
            p.name = clean_name(&p.name).unwrap_or_else(|| ProfileCfg::default_name(p.id));
        }
        if self.button > 2 {
            self.button = 1;
        }
        // меню должно открываться хоть чем-то
        if self.button == 0 && self.hotkey.is_none() {
            self.button = 1;
        }
        // профиль без способа вызова бесполезен; лишние — отбрасываем
        for p in &mut self.profiles {
            if p.button > 2 {
                p.button = 0;
            }
        }
        self.profiles.retain(|p| p.button != 0 || p.hotkey.is_some());
        self.profiles.sort_by_key(|p| p.id);
        self.profiles.truncate(MAX_PROFILES);
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

    /// Строки профилей для config.txt.
    fn profiles_text(&self) -> String {
        self.profiles
            .iter()
            .map(|p| {
                format!(
                    "# меню «{name}» (файл menu-{id}.yaml) на другой кнопке и/или сочетании\nprofile{id} = 1\nprofile{id}_name = {name}\nprofile{id}_button = {}\nprofile{id}_hotkey = {}\n",
                    p.button,
                    p.hotkey.map(|c| c.to_text()).unwrap_or_default(),
                    id = p.id,
                    name = p.name
                )
            })
            .collect::<String>()
            // сколько дополнительных меню (0 — все убраны намеренно; нет строки — старый файл)
            + &format!("profiles = {}\n", self.profiles.len())
    }

    pub fn to_text(&self) -> String {
        format!(
            "# Радиальное меню (удобнее менять через «Настройки» в трее)\n\
             # название основного меню (его состав — menu.yaml)
main_name = {}
\n             # button: 0 = не использовать, 1 = боковая «назад», 2 = боковая «вперёд»\nbutton = {}\n\
             # сочетание клавиш для меню (держать, пока меню открыто), например ctrl+alt+space; пусто — нет\nhotkey = {}\n\
             # папка хранилища Obsidian для заметок\nvault = \"{}\"\n\
             # чувствительность, % ({}..{})\nsens_percent = {}\n\
             # размер меню, % ({}..{})\nsize_percent = {}\n\
             # задержка раскрытия подменю, мс ({}..{})\ndwell_ms = {}\n\
             # тема: auto (как в системе), light, dark\ntheme = {}\n\
             # проверять обновления раз в сутки (1/0); запрос идёт только на github.com\ncheck_updates = {}\n{}",
            self.main_name,
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
            u8::from(self.check_updates),
            self.profiles_text(),
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

    #[test]
    fn profiles_round_trip_and_default_to_navigation() {
        let d = Settings::default();
        assert_eq!(d.profiles, vec![ProfileCfg::new(2, 2)]);
        assert_eq!(d.profiles[0].name, "Основное 2");
        assert_eq!(Settings::parse("").profiles, d.profiles, "в старом файле меню нет — берётся «Основное 2»");
        assert!(Settings::parse("profiles = 0\n").profiles.is_empty(), "убрали намеренно — не возвращаем");
        let s = Settings {
            profiles: vec![
                ProfileCfg { name: "Работа".into(), ..ProfileCfg::new(2, 2) },
                ProfileCfg { hotkey: Chord::parse("ctrl+alt+n").ok(), ..ProfileCfg::new(4, 0) },
            ],
            ..Settings::default()
        };
        assert_eq!(Settings::parse(&s.to_text()), s);
        assert_eq!(s.profiles[1].file_name(), "menu-4.yaml");
    }

    fn s_names() -> Vec<String> {
        (2..=4).map(ProfileCfg::default_name).collect()
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  Работа \t"), Some("Работа".to_string()));
        assert_eq!(clean_name("   "), None);
        assert_eq!(clean_name(&"я".repeat(50)).unwrap().chars().count(), NAME_MAX);
        let s = Settings::parse("profile3_button = 1\nprofile3_name =   \n");
        assert_eq!(s.profiles[0].name, "Мое меню 1", "пустое название — по умолчанию");
    }

    #[test]
    fn broken_profiles_are_dropped() {
        // без кнопки и сочетания, с чужим номером и сверх лимита
        assert_eq!(s_names(), vec!["Основное 2", "Мое меню 1", "Мое меню 2"]);
        let s = Settings::parse("profile2 = 1\nprofile3_button = 1\nprofile9_button = 1\nprofile4_button = 7\n");
        assert_eq!(s.profiles.iter().map(|p| p.id).collect::<Vec<_>>(), vec![3]);
    }

    #[test]
    fn update_check_flag_round_trips() {
        assert!(Settings::parse("").check_updates); // по умолчанию включена
        assert!(!Settings::parse("check_updates = 0
").check_updates);
        assert!(!Settings::parse("check_updates = off
").check_updates);
        assert!(Settings::parse("check_updates = maybe
").check_updates);
        let off = Settings { check_updates: false, ..Settings::default() };
        assert!(!Settings::parse(&off.to_text()).check_updates);
    }
}

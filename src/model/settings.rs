//! Пользовательские настройки: простой файл `ключ = значение` (комментарии — `#`).

use super::action::Chord;
use super::games::{self, GamePolicy};
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
    /// Не вызывать меню, пока активна полноэкранная программа (игра): кнопки и сочетания остаются ей.
    pub ignore_fullscreen: bool,
    /// Программы, в которых меню не вызывается никогда (имена exe в нижнем регистре).
    pub ignore_apps: Vec<String>,
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
            ignore_fullscreen: true,
            ignore_apps: Vec::new(),
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
                "ignore_fullscreen" => {
                    s.ignore_fullscreen = match v.to_ascii_lowercase().as_str() {
                        "0" | "false" | "no" | "off" => false,
                        "1" | "true" | "yes" | "on" => true,
                        _ => s.ignore_fullscreen,
                    }
                }
                "ignore_apps" => s.ignore_apps = games::parse_list(v),
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

    /// Правила «не мешать в играх» для хука.
    pub fn game_policy(&self) -> GamePolicy {
        GamePolicy { ignore_fullscreen: self.ignore_fullscreen, ignore_apps: self.ignore_apps.clone() }
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
             # проверять обновления раз в сутки (1/0); запрос идёт только на github.com\ncheck_updates = {}\n\
             # не вызывать меню, пока активна полноэкранная программа (игра), чтобы кнопки и сочетания остались ей (1/0)\nignore_fullscreen = {}\n\
             # программы, в которых меню не вызывается никогда: имена exe через запятую, например game.exe, dota2.exe\nignore_apps = {}\n{}",
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
            u8::from(self.ignore_fullscreen),
            self.ignore_apps.join(", "),
            self.profiles_text(),
        )
    }
}

/// Ключи «Общих» настроек: их возвращает к стандарту кнопка «Сбросить» на странице «Общие».
pub const GENERAL_KEYS: [&str; 6] = ["theme", "size_percent", "sens_percent", "dwell_ms", "ignore_fullscreen", "ignore_apps"];

/// Значения ключей в тексте настроек (только действующие строки `ключ = значение`, не комментарии).
fn key_values(text: &str) -> std::collections::BTreeMap<String, String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

/// Ключи, записанные в файле явно (раскомментированные).
pub fn explicit_keys(text: &str) -> std::collections::BTreeSet<String> {
    key_values(text).into_keys().collect()
}

/// Ключ описывает меню, их вызовы и папку заметок — это выбор человека, а не «стандартное поведение»: пишем всегда.
fn is_structural(key: &str) -> bool {
    matches!(key, "button" | "hotkey" | "vault" | "main_name" | "profiles") || key.starts_with("profile")
}

impl Settings {
    /// Вернуть «Общие» настройки к стандартным (папка заметок, меню и их вызовы не трогаются).
    pub fn reset_general(&mut self) {
        let d = Settings::default();
        self.theme = d.theme;
        self.size_percent = d.size_percent;
        self.sens_percent = d.sens_percent;
        self.dwell_ms = d.dwell_ms;
        self.ignore_fullscreen = d.ignore_fullscreen;
        self.ignore_apps = d.ignore_apps;
    }

    /// Ключи, значения которых у `self` и `other` различаются.
    pub fn changed_keys(&self, other: &Settings) -> std::collections::BTreeSet<String> {
        let (a, b) = (key_values(&self.to_text()), key_values(&other.to_text()));
        a.keys().chain(b.keys()).filter(|k| a.get(*k) != b.get(*k)).cloned().collect()
    }

    /// Текст файла настроек, в котором **явно записано только то, что отличается от стандарта** или что
    /// человек менял сам (`explicit`). Остальное — закомментированные строки со стандартным значением:
    /// когда мы изменим стандарт, у тех, кто эти настройки не трогал, поменяется и поведение, а выбор
    /// тех, кто настраивал, останется как был. Раскомментировав строку, можно зафиксировать значение.
    pub fn to_text_minimal(&self, explicit: &std::collections::BTreeSet<String>) -> String {
        let defaults = key_values(&Settings::default().to_text());
        let mut out = String::new();
        let mut comments: Vec<&str> = Vec::new();
        let full = self.to_text();
        for line in full.lines() {
            let t = line.trim();
            if t.starts_with('#') {
                comments.push(line);
                continue;
            }
            let Some((k, v)) = t.split_once('=') else {
                for c in comments.drain(..) {
                    out.push_str(c);
                    out.push('\n');
                }
                out.push_str(line);
                out.push('\n');
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            let keep = is_structural(k) || explicit.contains(k) || defaults.get(k).map(String::as_str) != Some(v);
            for c in comments.drain(..) {
                out.push_str(c);
                out.push('\n');
            }
            if keep {
                out.push_str(line);
            } else {
                out.push_str(&format!("# {line}    (стандартное значение: следует за обновлениями программы)"));
            }
            out.push('\n');
        }
        out
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
    fn game_rules_round_trip() {
        let d = Settings::default();
        assert!(d.ignore_fullscreen && d.ignore_apps.is_empty(), "новая установка не мешает в играх");
        let s = Settings::parse("ignore_fullscreen = 0\nignore_apps = Game.exe, C:\\G\\Dota2.exe\n");
        assert!(!s.ignore_fullscreen);
        assert_eq!(s.ignore_apps, ["game.exe", "dota2.exe"]);
        assert_eq!(Settings::parse(&s.to_text()), s);
        assert!(Settings::parse("").ignore_fullscreen, "старый файл без ключа — правило включено");
        assert_eq!(s.game_policy().ignore_apps.len(), 2);
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

    #[test]
    fn untouched_settings_follow_the_defaults() {
        let s = Settings::default();
        let text = s.to_text_minimal(&Default::default());
        let keys = explicit_keys(&text);
        assert!(!keys.contains("size_percent") && !keys.contains("theme") && !keys.contains("ignore_fullscreen"), "{keys:?}");
        assert!(text.contains("# size_percent = 100"), "строка остаётся в файле закомментированной — как подсказка");
        assert_eq!(Settings::parse(&text), s, "закомментированное читается как стандарт");
    }

    #[test]
    fn customised_and_explicit_values_are_written() {
        let s = Settings { size_percent: 120, theme: ThemeMode::Light, ..Settings::default() };
        let keys = explicit_keys(&s.to_text_minimal(&Default::default()));
        assert!(keys.contains("size_percent") && keys.contains("theme"), "значение не стандартное — пишем");
        assert!(!keys.contains("dwell_ms"));
        // человек сам выбрал значение, равное стандарту, — оно фиксируется
        let explicit: std::collections::BTreeSet<String> = ["dwell_ms".to_string()].into();
        assert!(explicit_keys(&Settings::default().to_text_minimal(&explicit)).contains("dwell_ms"));
        assert_eq!(Settings::parse(&s.to_text_minimal(&Default::default())), s);
    }

    #[test]
    fn menus_and_triggers_are_always_written() {
        let s = Settings { button: 2, ..Settings::default() };
        let text = s.to_text_minimal(&Default::default());
        assert!(explicit_keys(&text).contains("profiles"), "состав меню — не «стандартное значение»");
        assert_eq!(Settings::parse(&text), s);
    }

    #[test]
    fn changed_keys_see_only_real_changes() {
        let a = Settings::default();
        let b = Settings { sens_percent: 150, check_updates: false, ..Settings::default() };
        let keys: Vec<String> = a.changed_keys(&b).into_iter().collect();
        assert_eq!(keys, ["check_updates", "sens_percent"]);
        assert!(a.changed_keys(&a).is_empty());
    }

    #[test]
    fn reset_general_keeps_vault_and_menus() {
        let mut s = Settings {
            size_percent: 130,
            theme: ThemeMode::Dark,
            ignore_fullscreen: false,
            ignore_apps: vec!["game.exe".into()],
            vault: "D:/V".into(),
            button: 2,
            ..Settings::default()
        };
        s.reset_general();
        let d = Settings::default();
        assert_eq!((s.size_percent, s.theme, s.ignore_fullscreen, s.ignore_apps.len()), (d.size_percent, d.theme, true, 0));
        assert_eq!((s.vault.as_str(), s.button), ("D:/V", 2));
    }
}

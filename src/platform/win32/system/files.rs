//! Файлы пользователя в `%APPDATA%\RadialMenu`: настройки (`config.txt`), меню (`menu.yaml`)
//! и папка `icons` для своих значков. Создаются при первом запуске; правки замечаются по
//! времени изменения.

use std::path::PathBuf;
use std::time::SystemTime;

use super::util::log_error;
use crate::model::defaults::{self, Kind, Ledger, Plan};
use crate::model::menu_yaml::{self, MenuError};
use crate::model::settings::{explicit_keys, GENERAL_KEYS};
use crate::model::{default_menu, MenuSet, Profile, ProfileCfg, Settings, Trigger};
use crate::render::assets;

fn dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("RadialMenu"))
}

/// Папка с настройками, меню и журналом (создаётся, если её нет).
pub fn config_dir() -> Option<PathBuf> {
    let d = dir()?;
    let _ = std::fs::create_dir_all(&d);
    Some(d)
}

pub fn config_path() -> Option<PathBuf> {
    dir().map(|d| d.join("config.txt"))
}

pub fn menu_path() -> Option<PathBuf> {
    dir().map(|d| d.join("menu.yaml"))
}

/// Время изменения файла (`None` — файла нет).
pub fn mtime(p: &Option<PathBuf>) -> Option<SystemTime> {
    p.as_ref()?.metadata().ok()?.modified().ok()
}

fn write_new(p: &PathBuf, text: &str) {
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Err(e) = std::fs::write(p, text) {
        log_error(&format!("запись {}: {e}", p.display()));
    }
}

/// Настройки из файла; при первом запуске кладём файл со значениями по умолчанию.
/// Параметры командной строки действуют только на этот запуск и в файл не попадают.
pub fn load_settings() -> Settings {
    let path = config_path();
    let mut s = match path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(text) => Settings::parse(&text),
        None => {
            let d = Settings::default();
            if let Some(p) = &path {
                write_new(p, &d.to_text_minimal(&Default::default()));
            }
            d
        }
    };
    s.apply_args(std::env::args().skip(1));
    s
}

/// Сохранить настройки. В файле явно остаётся только то, что отличается от стандарта или что человек менял
/// сам: остальное следует за стандартными значениями, которые мы можем обновлять.
pub fn save_settings(s: &Settings) -> Result<(), String> {
    write_settings(s, &[])
}

/// То же, но перечисленные ключи снова становятся «стандартными» (кнопка «Сбросить»).
fn write_settings(s: &Settings, forget: &[&str]) -> Result<(), String> {
    let p = config_path().ok_or("не найдена папка APPDATA")?;
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let prev = std::fs::read_to_string(&p).unwrap_or_default();
    let mut explicit = explicit_keys(&prev);
    explicit.extend(Settings::parse(&prev).changed_keys(s)); // то, что поменяли сейчас, — выбор человека
    for k in forget {
        explicit.remove(*k);
    }
    std::fs::write(&p, s.to_text_minimal(&explicit)).map_err(|e| format!("не удалось записать настройки: {e}"))
}

/// «Общие» настройки (тема, размеры, чувствительность, задержка, игры) — к стандартным значениям.
pub fn reset_general() -> Result<Settings, String> {
    let mut s = Settings::parse(&config_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default());
    s.reset_general();
    write_settings(&s, &GENERAL_KEYS)?;
    Ok(s)
}

// ---------------------------------------------------------------- стандартные меню

fn ledger_path() -> Option<PathBuf> {
    dir().map(|d| d.join("defaults.txt"))
}

fn load_ledger() -> Ledger {
    ledger_path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| Ledger::parse(&t)).unwrap_or_default()
}

fn save_ledger(l: &Ledger) {
    if let Some(p) = ledger_path() {
        write_new(&p, &l.to_text());
    }
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Файл стандартного меню: нет — создать; не менялся, а стандарт новее — обновить; менялся — не трогать.
fn ensure_managed(path: &std::path::Path, kind: Kind) {
    let name = file_name(path);
    let mut ledger = load_ledger();
    let text = std::fs::read_to_string(path).ok();
    let plan = defaults::plan(kind, text.as_deref(), ledger.get(&name));
    match plan {
        Plan::Keep => return,
        Plan::Create | Plan::Upgrade => {
            write_new(&path.to_path_buf(), kind.text());
            if plan == Plan::Upgrade {
                crate::platform::diag::log(&format!("{name}: не менялся — обновлён до нового стандартного меню"));
            }
        }
        Plan::Adopt => {}
    }
    ledger.set(&name, kind);
    save_ledger(&ledger);
}

fn kind_of(id: u8) -> Kind {
    if id == 2 {
        Kind::Nav
    } else {
        Kind::Main
    }
}

/// Вернуть меню к стандартному: прежний файл сохраняется рядом как `имя.bak`.
/// `id` — 0 для основного, иначе номер меню. Возвращает имя файла.
pub fn reset_menu_file(id: u8) -> Result<String, String> {
    let path = if id == 0 { menu_path() } else { dir().map(|d| d.join(format!("menu-{id}.yaml"))) }.ok_or("не найдена папка APPDATA")?;
    let name = file_name(&path);
    if path.exists() {
        let bak = path.with_file_name(format!("{name}.bak"));
        std::fs::copy(&path, &bak).map_err(|e| format!("не удалось сохранить копию {name}.bak: {e}"))?;
    }
    let kind = kind_of(id);
    std::fs::write(&path, kind.text()).map_err(|e| format!("не удалось записать {name}: {e}"))?;
    let mut ledger = load_ledger();
    ledger.set(&name, kind);
    save_ledger(&ledger);
    Ok(name)
}

/// Файл меню профиля (`menu-2.yaml`…).
pub fn profile_path(p: &ProfileCfg) -> Option<PathBuf> {
    dir().map(|d| d.join(p.file_name()))
}

/// Время изменения файлов меню профилей — чтобы заметить правки.
pub fn profile_stamps(settings: &Settings) -> Vec<Option<SystemTime>> {
    settings.profiles.iter().map(|p| mtime(&profile_path(p))).collect()
}

/// Прочитать и разобрать файл меню. Ошибка — с именем файла, чтобы было понятно, где искать.
fn read_menu(path: &std::path::Path) -> Result<MenuSet, MenuError> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let wrap = |e: MenuError| {
        log_error(&format!("{name}, {e}"));
        MenuError { line: 0, msg: format!("{name}, {e}") }
    };
    let text = std::fs::read_to_string(path)
        .map_err(|e| wrap(MenuError { line: 0, msg: format!("не удалось прочитать файл: {e}") }))?;
    menu_yaml::parse(&text, path.parent(), &assets::exists).map_err(wrap)
}

/// Меню из файлов: основное (`menu.yaml`) и профили. Ошибка в файле не мешает работать: вместо него
/// берём меню по умолчанию, а первую ошибку возвращаем, чтобы её показать.
pub fn load_menu(settings: &Settings) -> (MenuSet, Option<MenuError>) {
    let mut first_error = None;
    let mut set = match ensure_menu_file() {
        Some(path) => read_menu(&path).unwrap_or_else(|e| {
            first_error = Some(e);
            MenuSet::single(default_menu())
        }),
        None => MenuSet::single(default_menu()),
    };
    for cfg in &settings.profiles {
        let menu = match ensure_profile_file(cfg) {
            Some(path) => match read_menu(&path) {
                Ok(m) => m.main,
                Err(e) => {
                    first_error.get_or_insert(e);
                    default_menu()
                }
            },
            None => default_menu(),
        };
        set.profiles.push(Profile {
            name: cfg.name.clone(),
            trigger: Trigger { button: cfg.button, hotkey: cfg.hotkey },
            menu,
        });
    }
    (set, first_error)
}

/// Файл меню профиля; если его нет — кладём шаблон («Навигация» для меню 2, иначе — как у основного).
pub fn ensure_profile_file(cfg: &ProfileCfg) -> Option<PathBuf> {
    let p = profile_path(cfg)?;
    ensure_managed(&p, kind_of(cfg.id));
    Some(p)
}

/// Убедиться, что menu.yaml (с описанием формата) и папка icons существуют.
pub fn ensure_menu_file() -> Option<PathBuf> {
    let p = menu_path()?;
    ensure_managed(&p, Kind::Main);
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d.join("icons"));
    }
    Some(p)
}

//! Файлы пользователя в `%APPDATA%\RadialMenu`: настройки (`config.txt`), меню (`menu.yaml`)
//! и папка `icons` для своих значков. Создаются при первом запуске; правки замечаются по
//! времени изменения.

use std::path::PathBuf;
use std::time::SystemTime;

use super::util::log_error;
use crate::model::menu_yaml::{self, MenuError};
use crate::model::{default_menu, MenuSet, Profile, ProfileCfg, Settings, Trigger};
use crate::render::assets;

fn dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("RadialMenu"))
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
                write_new(p, &d.to_text());
            }
            d
        }
    };
    s.apply_args(std::env::args().skip(1));
    s
}

pub fn save_settings(s: &Settings) -> Result<(), String> {
    let p = config_path().ok_or("не найдена папка APPDATA")?;
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    std::fs::write(&p, s.to_text()).map_err(|e| format!("не удалось записать настройки: {e}"))
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

/// Файл меню профиля; если его нет — кладём шаблон («Навигация» для профиля 2, иначе — как у основного).
pub fn ensure_profile_file(cfg: &ProfileCfg) -> Option<PathBuf> {
    let p = profile_path(cfg)?;
    if !p.exists() {
        write_new(&p, if cfg.id == 2 { menu_yaml::NAV_TEMPLATE } else { menu_yaml::TEMPLATE });
    }
    Some(p)
}

/// Убедиться, что menu.yaml (с описанием формата) и папка icons существуют.
pub fn ensure_menu_file() -> Option<PathBuf> {
    let p = menu_path()?;
    if !p.exists() {
        write_new(&p, menu_yaml::TEMPLATE);
    }
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d.join("icons"));
    }
    Some(p)
}

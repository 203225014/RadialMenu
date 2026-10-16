//! Радиальное меню на боковой кнопке мыши.
//!
//! Слои:
//! * переносимое ядро (`geom`, `metrics`, `menu`, `layout`, `nav`, `anim`, `style`,
//!   `engine`, `raster`, `compose`, `notes`, `config`) — чистая логика и программная графика,
//!   полностью покрыта тестами и работает на любой ОС;
//! * `platform` — тонкая обвязка Win32 (хук мыши, окно-слой, ввод клавиш, звук, заметка, трей).

pub mod action;
pub mod anim;
pub mod assets;
pub mod compose;
pub mod config;
pub mod engine;
pub mod geom;
pub mod layout;
pub mod menu;
pub mod metrics;
pub mod nav;
pub mod notes;
pub mod raster;
pub mod style;

#[cfg(windows)]
pub mod platform;

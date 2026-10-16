//! Windows-обвязка: всё, что говорит с системой. Логика меню — в переносимом ядре.

mod app;
mod audio;
mod hook;
mod input;
mod layer;
mod note;
mod tray;
mod util;

pub use app::run;

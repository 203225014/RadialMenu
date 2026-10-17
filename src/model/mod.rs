//! Слой данных: из чего состоит меню и его файл, что оно умеет запускать, настройки и тема.
//! Ничего не знает ни о графике, ни об ОС.

pub mod action;
pub mod icon;
pub mod menu;
pub mod menu_yaml;
pub mod settings;
pub mod theme;

pub use action::{Action, Chord, Media, Run, Step};
pub use icon::IconRef;
pub use menu::{default_menu, AppMenu, Context, Flag, Item, Menu, MenuSet, NodeId, Shape, ROOT};
pub use settings::Settings;
pub use theme::{Theme, ThemeMode};

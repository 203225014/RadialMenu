//! Слой графики: иконки, предрисовка масок, композитор кадра меню, палитры тем
//! и рисование окон. Чистый код без ОС — кадры и окна можно проверить в PNG.

pub mod assets;
pub mod blur;
pub mod compose;
pub mod painter;
pub mod palette;
pub mod raster;

//! Слой платформы: всё, что говорит с ОС. Определение темы системы есть для всех ОС,
//! само приложение (хук мыши, окна, трей) — пока только для Windows (`win32`).

pub mod diag;
pub mod theme;

#[cfg(windows)]
pub mod win32;

#[cfg(windows)]
pub use win32::run;

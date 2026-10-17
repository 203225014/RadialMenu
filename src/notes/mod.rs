//! Слой заметок для Obsidian: хранилище (индекс, сохранение Ring_NN.md) и правка текста
//! в окне заметки (`[[` → `[[]]`, подстановка ссылки). Без окон — окно только применяет правки.

pub mod editor;
pub mod vault;

pub use vault::{clean_name, link_query, ring_name, save, save_named, scan, suggest, NoteRef, VaultIndex};

//! Откуда брать иконку кнопки.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum IconRef {
    /// Без иконки.
    None,
    /// Встроенная (см. `render::assets::NAMES`).
    Builtin(String),
    /// Свой файл SVG или PNG. `color` — показывать в своих цветах, иначе перекрасить под тему.
    File { path: PathBuf, color: bool },
    /// Иконка программы, файла или ярлыка (её достаёт платформа).
    Program(PathBuf),
    /// Первая буква подписи в кружке — когда иконки нет.
    Letter(char),
}

impl IconRef {
    pub fn builtin(name: &str) -> IconRef {
        if name.is_empty() || name == "blank" {
            IconRef::None
        } else {
            IconRef::Builtin(name.into())
        }
    }

    /// Буква для кнопки без иконки: первая буква или цифра подписи.
    pub fn letter_for(label: &str) -> IconRef {
        label
            .chars()
            .find(|c| c.is_alphanumeric())
            .map(|c| IconRef::Letter(c.to_uppercase().next().unwrap_or(c)))
            .unwrap_or(IconRef::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters() {
        assert_eq!(IconRef::letter_for("«obsidian»"), IconRef::Letter('O'));
        assert_eq!(IconRef::letter_for("ёлка"), IconRef::Letter('Ё'));
        assert_eq!(IconRef::letter_for("—"), IconRef::None);
        assert_eq!(IconRef::builtin("blank"), IconRef::None);
    }
}

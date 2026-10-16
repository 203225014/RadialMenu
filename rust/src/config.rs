//! Пользовательские настройки: простой файл `ключ = значение` (комментарии — `#`).

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// 1 = боковая «назад» (XButton1), 2 = «вперёд» (XButton2).
    pub button: u8,
    /// Папка хранилища Obsidian, куда пишутся заметки Ring_NN.md.
    pub vault: String,
    /// Чувствительность движения мыши внутри меню, %.
    pub sens_percent: u32,
    /// Размер меню, %.
    pub size_percent: u32,
    /// Задержка раскрытия «Копировать»/«Вставить», мс.
    pub dwell_ms: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            button: 1,
            vault: "C:/Users/HP/Work".into(),
            sens_percent: 100,
            size_percent: 100,
            dwell_ms: 450,
        }
    }
}

impl Settings {
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
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
                "vault" => s.vault = v.to_string(),
                "sens_percent" => s.sens_percent = v.parse().unwrap_or(s.sens_percent),
                "size_percent" => s.size_percent = v.parse().unwrap_or(s.size_percent),
                "dwell_ms" => s.dwell_ms = v.parse().unwrap_or(s.dwell_ms),
                _ => {}
            }
        }
        s.normalize();
        s
    }

    pub fn normalize(&mut self) {
        if self.button != 1 && self.button != 2 {
            self.button = 1;
        }
        self.sens_percent = self.sens_percent.clamp(20, 300);
        self.size_percent = self.size_percent.clamp(50, 150);
        self.dwell_ms = self.dwell_ms.clamp(100, 2000);
    }

    /// Применить параметры командной строки (`--button N`, `--vault ПУТЬ`).
    pub fn apply_args<I: IntoIterator<Item = String>>(&mut self, args: I) {
        let args: Vec<String> = args.into_iter().collect();
        for w in args.windows(2) {
            match w[0].as_str() {
                "--button" => {
                    if let Ok(b) = w[1].parse() {
                        self.button = b;
                    }
                }
                "--vault" => self.vault = w[1].clone(),
                _ => {}
            }
        }
        self.normalize();
    }

    pub fn to_text(&self) -> String {
        format!(
            "# Радиальное меню\n# button: 1 = боковая «назад», 2 = боковая «вперёд»\nbutton = {}\n\
             # папка хранилища Obsidian\nvault = \"{}\"\n# чувствительность, % (20..300)\nsens_percent = {}\n\
             # размер меню, % (50..150)\nsize_percent = {}\n# задержка раскрытия подменю, мс\ndwell_ms = {}\n",
            self.button, self.vault, self.sens_percent, self.size_percent, self.dwell_ms
        )
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
            ..Settings::default()
        };
        assert_eq!(Settings::parse(&s.to_text()), s);
    }

    #[test]
    fn clamps_and_ignores_garbage() {
        let s = Settings::parse("button = 9\nsens_percent = 5000\nmusor\ndwell_ms = abc\n");
        assert_eq!(s.button, 1);
        assert_eq!(s.sens_percent, 300);
        assert_eq!(s.dwell_ms, 450);
    }

    #[test]
    fn args_override() {
        let mut s = Settings::default();
        s.apply_args(["--button", "2", "--vault", "E:/V"].map(String::from));
        assert_eq!((s.button, s.vault.as_str()), (2, "E:/V"));
    }
}

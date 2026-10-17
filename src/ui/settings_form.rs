//! Окно настроек без ОС: раскладка, попадание по элементам, правка черновика настроек и
//! отрисовка в стиле меню (стеклянные «таблетки», акцентный синий). Платформа подаёт
//! события мыши и клавиш, показывает готовую картинку и исполняет команды (`Cmd`).

use crate::model::settings::{DWELL_RANGE, SENS_RANGE, SIZE_RANGE};
use crate::model::action::vk;
use crate::model::{Chord, Settings, Theme, ThemeMode};
use crate::render::painter::{blend, Align, Painter};
use crate::render::palette::{self, Ui};

/// Базовые размеры (при 96 dpi), px.
const W: f32 = 480.0;
const HEAD: f32 = 56.0;
const ROW: f32 = 50.0;
const PAD: f32 = 24.0;
const CTRL_X: f32 = 208.0;
const CTRL_H: f32 = 32.0;
const BTN_W: f32 = 112.0;
const BROWSE_W: f32 = 84.0;
const VALUE_W: f32 = 64.0;

// строки окна
const R_THEME: usize = 0;
const R_BUTTON: usize = 1;
const R_HOTKEY: usize = 2;
const R_SIZE: usize = 3;
const R_SENS: usize = 4;
const R_DWELL: usize = 5;
const R_VAULT: usize = 6;
const R_AUTO: usize = 7;
const R_MENU: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slider {
    Size,
    Sens,
    Dwell,
}

impl Slider {
    fn range(self) -> (u32, u32, u32) {
        let ((lo, hi), step) = match self {
            Slider::Size => (SIZE_RANGE, 5),
            Slider::Sens => (SENS_RANGE, 10),
            Slider::Dwell => (DWELL_RANGE, 50),
        };
        (lo, hi, step)
    }

    fn get(self, s: &Settings) -> u32 {
        match self {
            Slider::Size => s.size_percent,
            Slider::Sens => s.sens_percent,
            Slider::Dwell => s.dwell_ms,
        }
    }

    fn set(self, s: &mut Settings, v: u32) {
        let (lo, hi, step) = self.range();
        let v = (((v.clamp(lo, hi) - lo) as f32 / step as f32).round() as u32 * step + lo).min(hi);
        match self {
            Slider::Size => s.size_percent = v,
            Slider::Sens => s.sens_percent = v,
            Slider::Dwell => s.dwell_ms = v,
        }
    }

    fn text(self, v: u32) -> String {
        match self {
            Slider::Dwell => format!("{v} мс"),
            _ => format!("{v} %"),
        }
    }
}

/// Элемент окна, по которому можно попасть мышью.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Close,
    Header,
    Theme(ThemeMode),
    /// Боковая кнопка мыши: 0 — нет, 1, 2.
    Button(u8),
    /// Поле сочетания клавиш (щелчок — записать новое).
    Hotkey,
    ClearHotkey,
    Slider(Slider),
    Vault,
    Browse,
    Autostart,
    MenuFile,
    Cancel,
    Save,
}

/// Что нужно сделать платформе после события.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    /// Ничего не изменилось.
    None,
    /// Перерисовать окно.
    Redraw,
    /// Закрыть без сохранения.
    Close,
    /// Сохранить черновик и закрыть.
    Save,
    /// Выбрать папку хранилища.
    Browse,
    /// Открыть menu.yaml в редакторе.
    OpenMenuFile,
    /// Начать запись сочетания: платформа отключает свой перехват клавиш, пока идёт запись.
    StartRecording,
    /// Запись закончена (или отменена): вернуть перехват и перерисовать.
    StopRecording,
    /// Начать перетаскивание окна за шапку.
    DragWindow,
    /// Перевести фокус в поле пути.
    FocusVault,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Escape,
    Enter,
}

/// Прямоугольник в px окна.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box2 {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Box2 {
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
}

/// Строка под элементами: подсказка или итог действия.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub text: String,
    pub bad: bool,
}

pub struct Fonts {
    pub regular: fontdue::Font,
    pub bold: fontdue::Font,
}

impl Fonts {
    pub fn load() -> Option<Fonts> {
        Some(Fonts {
            regular: crate::render::raster::load_font_regular()?,
            bold: crate::render::raster::load_font()?,
        })
    }
}

pub struct SettingsForm {
    /// Черновик: что будет сохранено по «Сохранить».
    pub draft: Settings,
    /// Запускать с ОС (хранится не в файле настроек, а в системе).
    pub autostart: bool,
    pub status: Option<Status>,
    scale: f32,
    hover: Option<Target>,
    pressed: Option<Target>,
    drag: Option<Slider>,
    recording: bool,
}

const ROWS: [&str; 9] = [
    "Тема",
    "Кнопка мыши",
    "Сочетание клавиш",
    "Размер меню",
    "Чувствительность",
    "Задержка веера",
    "Хранилище заметок",
    "Запуск с системой",
    "Состав меню",
];

impl SettingsForm {
    pub fn new(settings: Settings, autostart: bool, scale: f32) -> SettingsForm {
        SettingsForm {
            draft: settings,
            autostart,
            status: None,
            scale: scale.max(0.5),
            hover: None,
            pressed: None,
            drag: None,
            recording: false,
        }
    }

    /// Идёт запись сочетания: клавиши — в `record`, а не в `key`.
    pub fn is_recording(&self) -> bool {
        self.recording
    }

    fn s(&self, v: f32) -> f32 {
        v * self.scale
    }

    fn b(&self, x: f32, y: f32, w: f32, h: f32) -> Box2 {
        Box2 {
            x: self.s(x),
            y: self.s(y),
            w: self.s(w),
            h: self.s(h),
        }
    }

    fn row_y(i: usize) -> f32 {
        HEAD + 8.0 + i as f32 * ROW
    }

    fn footer_y() -> f32 {
        Self::row_y(ROWS.len()) + 12.0
    }

    /// Размер окна в px.
    pub fn size(&self) -> (i32, i32) {
        let h = Self::footer_y() + 16.0 + CTRL_H + 2.0 + PAD;
        (self.s(W).round() as i32, self.s(h).round() as i32)
    }

    fn control(&self, row: usize, w: f32) -> Box2 {
        let y = Self::row_y(row) + (ROW - CTRL_H) / 2.0;
        self.b(CTRL_X, y, w, CTRL_H)
    }

    fn ctrl_w() -> f32 {
        W - PAD - CTRL_X
    }

    fn seg_boxes(&self, row: usize, n: usize) -> Vec<Box2> {
        let full = self.control(row, Self::ctrl_w());
        let w = full.w / n as f32;
        (0..n)
            .map(|i| Box2 {
                x: full.x + w * i as f32,
                w,
                ..full
            })
            .collect()
    }

    fn slider_row(s: Slider) -> usize {
        match s {
            Slider::Size => R_SIZE,
            Slider::Sens => R_SENS,
            Slider::Dwell => R_DWELL,
        }
    }

    /// Дорожка ползунка: (x0, x1, cy).
    fn track(&self, s: Slider) -> (f32, f32, f32) {
        let c = self.control(Self::slider_row(s), Self::ctrl_w() - VALUE_W);
        let r = self.s(9.0);
        (c.x + r, c.x + c.w - r, c.cy())
    }

    fn close_box(&self) -> Box2 {
        self.b(W - PAD - 28.0 + 6.0, 14.0, 28.0, 28.0)
    }

    fn vault_box(&self) -> Box2 {
        self.control(R_VAULT, Self::ctrl_w() - BROWSE_W - 8.0)
    }

    /// Где платформе разместить родное поле ввода пути (внутри рамки поля).
    pub fn vault_edit_rect(&self) -> Box2 {
        let b = self.vault_box();
        let (px, py) = (self.s(10.0), self.s(8.0));
        Box2 {
            x: b.x + px,
            y: b.y + py,
            w: b.w - 2.0 * px,
            h: b.h - 2.0 * py,
        }
    }

    fn browse_box(&self) -> Box2 {
        let v = self.vault_box();
        Box2 {
            x: v.x + v.w + self.s(8.0),
            w: self.s(BROWSE_W),
            ..v
        }
    }

    fn hotkey_box(&self) -> Box2 {
        self.control(R_HOTKEY, Self::ctrl_w() - BROWSE_W - 8.0)
    }

    fn clear_box(&self) -> Box2 {
        let v = self.hotkey_box();
        Box2 {
            x: v.x + v.w + self.s(8.0),
            w: self.s(BROWSE_W),
            ..v
        }
    }

    fn toggle_box(&self) -> Box2 {
        let c = self.control(R_AUTO, 46.0);
        Box2 {
            y: c.cy() - self.s(13.0),
            h: self.s(26.0),
            ..c
        }
    }

    fn menu_file_box(&self) -> Box2 {
        self.control(R_MENU, 190.0)
    }

    fn footer_boxes(&self) -> (Box2, Box2) {
        let y = Self::footer_y() + 16.0;
        let save = self.b(W - PAD - BTN_W, y, BTN_W, CTRL_H + 2.0);
        let cancel = self.b(W - PAD - 2.0 * BTN_W - 10.0, y, BTN_W, CTRL_H + 2.0);
        (cancel, save)
    }

    pub fn hit(&self, x: f32, y: f32) -> Option<Target> {
        if self.close_box().contains(x, y) {
            return Some(Target::Close);
        }
        if y < self.s(HEAD) {
            return Some(Target::Header);
        }
        for (b, m) in self.seg_boxes(R_THEME, 3).iter().zip(ThemeMode::ALL) {
            if b.contains(x, y) {
                return Some(Target::Theme(m));
            }
        }
        for (i, b) in self.seg_boxes(R_BUTTON, 3).iter().enumerate() {
            if b.contains(x, y) {
                return Some(Target::Button(i as u8));
            }
        }
        for s in [Slider::Size, Slider::Sens, Slider::Dwell] {
            let (x0, x1, cy) = self.track(s);
            let grip = self.s(14.0);
            if x >= x0 - grip && x <= x1 + grip && (y - cy).abs() <= grip {
                return Some(Target::Slider(s));
            }
        }
        let (cancel, save) = self.footer_boxes();
        [
            (self.hotkey_box(), Target::Hotkey),
            (self.clear_box(), Target::ClearHotkey),
            (self.vault_box(), Target::Vault),
            (self.browse_box(), Target::Browse),
            (self.toggle_box(), Target::Autostart),
            (self.menu_file_box(), Target::MenuFile),
            (cancel, Target::Cancel),
            (save, Target::Save),
        ]
        .into_iter()
        .find(|(b, _)| b.contains(x, y))
        .map(|(_, t)| t)
    }

    fn slide_to(&mut self, s: Slider, x: f32) {
        let (x0, x1, _) = self.track(s);
        let (lo, hi, _) = s.range();
        let t = ((x - x0) / (x1 - x0)).clamp(0.0, 1.0);
        s.set(&mut self.draft, lo + (t * (hi - lo) as f32).round() as u32);
    }

    pub fn mouse_move(&mut self, x: f32, y: f32) -> Cmd {
        if let Some(s) = self.drag {
            let before = s.get(&self.draft);
            self.slide_to(s, x);
            return if s.get(&self.draft) != before {
                Cmd::Redraw
            } else {
                Cmd::None
            };
        }
        let h = self.hit(x, y);
        if h != self.hover {
            self.hover = h;
            Cmd::Redraw
        } else {
            Cmd::None
        }
    }

    pub fn mouse_leave(&mut self) -> Cmd {
        if self.hover.take().is_some() {
            Cmd::Redraw
        } else {
            Cmd::None
        }
    }

    pub fn mouse_down(&mut self, x: f32, y: f32) -> Cmd {
        let Some(t) = self.hit(x, y) else {
            return Cmd::None;
        };
        self.status = None;
        // щелчок мимо поля сочетания отменяет запись
        let stop = self.recording && !matches!(t, Target::Hotkey | Target::ClearHotkey);
        if stop {
            self.recording = false;
        }
        let cmd = self.down_on(t, x);
        if stop && matches!(cmd, Cmd::Redraw | Cmd::None) {
            Cmd::StopRecording
        } else {
            cmd
        }
    }

    fn down_on(&mut self, t: Target, x: f32) -> Cmd {
        match t {
            Target::Header => Cmd::DragWindow,
            Target::Hotkey => {
                self.recording = !self.recording;
                if self.recording {
                    Cmd::StartRecording
                } else {
                    Cmd::StopRecording
                }
            }
            Target::Theme(m) => {
                self.draft.theme = m;
                Cmd::Redraw
            }
            Target::Button(b) => {
                self.draft.button = b;
                Cmd::Redraw
            }
            Target::Slider(s) => {
                self.drag = Some(s);
                self.slide_to(s, x);
                Cmd::Redraw
            }
            Target::Autostart => {
                self.autostart = !self.autostart;
                Cmd::Redraw
            }
            Target::Vault => Cmd::FocusVault,
            // кнопки срабатывают по отпусканию над ними, как обычно
            _ => {
                self.pressed = Some(t);
                Cmd::Redraw
            }
        }
    }

    pub fn mouse_up(&mut self, x: f32, y: f32) -> Cmd {
        if self.drag.take().is_some() {
            return Cmd::Redraw;
        }
        let Some(p) = self.pressed.take() else {
            return Cmd::None;
        };
        if self.hit(x, y) != Some(p) {
            return Cmd::Redraw;
        }
        match p {
            Target::Close | Target::Cancel => Cmd::Close,
            Target::Save => self.try_save(),
            Target::ClearHotkey => {
                self.draft.hotkey = None;
                if std::mem::take(&mut self.recording) {
                    Cmd::StopRecording
                } else {
                    Cmd::Redraw
                }
            }
            Target::Browse => Cmd::Browse,
            Target::MenuFile => Cmd::OpenMenuFile,
            _ => Cmd::Redraw,
        }
    }

    /// Колесо над ползунком двигает его на шаг.
    pub fn wheel(&mut self, x: f32, y: f32, up: bool) -> Cmd {
        let Some(Target::Slider(s)) = self.hit(x, y) else {
            return Cmd::None;
        };
        let (_, _, step) = s.range();
        let v = s.get(&self.draft);
        s.set(
            &mut self.draft,
            if up { v + step } else { v.saturating_sub(step) },
        );
        Cmd::Redraw
    }

    pub fn key(&mut self, k: Key) -> Cmd {
        match k {
            Key::Escape if self.recording => {
                self.recording = false;
                Cmd::StopRecording
            }
            Key::Escape => Cmd::Close,
            Key::Enter => self.try_save(),
        }
    }

    /// Нажатие во время записи сочетания: `key` — код клавиши, остальное — зажатые модификаторы.
    /// Esc — отмена, Backspace/Delete — убрать сочетание, Ctrl/Alt/Shift/Win сами по себе — ждём дальше.
    pub fn record(&mut self, key: u16, ctrl: bool, alt: bool, shift: bool, win: bool) -> Cmd {
        if !self.recording {
            return Cmd::None;
        }
        let plain = !(ctrl || alt || shift || win);
        if plain && key == vk::ESCAPE {
            self.recording = false;
            return Cmd::StopRecording;
        }
        if plain && (key == vk::BACK || key == vk::DELETE) {
            self.draft.hotkey = None;
            self.recording = false;
            return Cmd::StopRecording;
        }
        match Chord::from_press(key, ctrl, alt, shift, win) {
            Some(c) => {
                self.draft.hotkey = Some(c);
                self.recording = false;
                Cmd::StopRecording
            }
            None => Cmd::None,
        }
    }

    /// Сохранить можно, только если меню хоть чем-то открывается.
    fn try_save(&mut self) -> Cmd {
        if self.draft.button == 0 && self.draft.hotkey.is_none() {
            self.status = Some(Status {
                text: "Меню нечем открыть: выберите кнопку мыши или задайте сочетание".into(),
                bad: true,
            });
            return Cmd::Redraw;
        }
        Cmd::Save
    }

    /// Текст поля пути сменили в родном поле ввода.
    pub fn set_vault(&mut self, path: &str) {
        self.draft.vault = path.to_string();
    }

    // ---------------------------------------------------------------- отрисовка

    fn hot(&self, t: Target) -> bool {
        self.hover == Some(t) || self.pressed == Some(t)
    }

    /// Нарисовать окно. `vault_text` — рисовать ли текст пути (если поверх нет родного поля).
    pub fn draw(&self, p: &mut Painter, f: &Fonts, theme: Theme, vault_text: bool) {
        let c = palette::ui(theme);
        let (w, h) = (p.width() as f32, p.height() as f32);
        p.clear(c.bg);
        p.stroke_rrect(0.0, 0.0, w, h, 0.0, c.edge, 0.85, 1.0);

        // шапка: кольцо-логотип, заголовок, крестик
        let (lx, ly) = (self.s(PAD + 10.0), self.s(HEAD / 2.0 + 2.0));
        for (i, col) in [c.acc, c.edge, c.acc, c.edge].into_iter().enumerate() {
            let a0 = -135.0_f32 + i as f32 * 90.0 + 12.0;
            let a1 = a0 + 66.0;
            let r = self.s(8.0);
            let pt = |a: f32| (lx + r * a.to_radians().cos(), ly + r * a.to_radians().sin());
            let steps = 6;
            for k in 0..steps {
                let (x0, y0) = pt(a0 + (a1 - a0) * k as f32 / steps as f32);
                let (x1, y1) = pt(a0 + (a1 - a0) * (k + 1) as f32 / steps as f32);
                p.line(x0, y0, x1, y1, col, 1.0, self.s(3.0));
            }
        }
        p.text(&f.bold, "Настройки", self.s(PAD + 28.0), ly, self.s(17.0), c.fg, Align::Left);
        let cb = self.close_box();
        if self.hot(Target::Close) {
            p.fill_rrect(cb.x, cb.y, cb.w, cb.h, self.s(8.0), c.bg2, 1.0);
        }
        let (cx, cy, k) = (cb.x + cb.w / 2.0, cb.cy(), self.s(5.0));
        p.line(cx - k, cy - k, cx + k, cy + k, c.dim, 1.0, self.s(1.6));
        p.line(cx - k, cy + k, cx + k, cy - k, c.dim, 1.0, self.s(1.6));
        p.line(self.s(PAD), self.s(HEAD), w - self.s(PAD), self.s(HEAD), c.edge, 0.25, 1.0);

        for (i, label) in ROWS.iter().enumerate() {
            let cy = self.s(Self::row_y(i) + ROW / 2.0);
            p.text(&f.regular, label, self.s(PAD), cy, self.s(14.0), c.fg, Align::Left);
        }

        // тема и кнопка мыши — сегменты
        let themes = ["Авто", "Светлая", "Тёмная"];
        let boxes = self.seg_boxes(R_THEME, 3);
        let sel = ThemeMode::ALL.iter().position(|&m| m == self.draft.theme).unwrap_or(0);
        self.segments(p, f, &c, &boxes, &themes, sel, |i| Target::Theme(ThemeMode::ALL[i]));
        let boxes = self.seg_boxes(R_BUTTON, 3);
        let sel = self.draft.button.min(2) as usize;
        self.segments(p, f, &c, &boxes, &["Нет", "«Назад»", "«Вперёд»"], sel, |i| {
            Target::Button(i as u8)
        });

        // сочетание клавиш: поле (щелчок — записать) и «Сбросить»
        let hb = self.hotkey_box();
        p.fill_rrect(hb.x, hb.y, hb.w, hb.h, self.s(8.0), c.bg2, 1.0);
        let (edge, edge_a, width) = if self.recording {
            (c.acc, 1.0, 1.5)
        } else if self.hot(Target::Hotkey) {
            (c.edge, 0.9, 1.0)
        } else {
            (c.edge, 0.45, 1.0)
        };
        p.stroke_rrect(hb.x, hb.y, hb.w, hb.h, self.s(8.0), edge, edge_a, width);
        let (text, col) = match (self.recording, self.draft.hotkey) {
            (true, _) => ("Нажмите сочетание…".to_string(), c.acc),
            (false, Some(k)) => (k.display(), c.fg),
            (false, None) => ("не задано".to_string(), c.dim),
        };
        p.text_clipped(&f.regular, &text, hb.x + self.s(10.0), hb.cy(), self.s(13.0), col, hb.w - self.s(20.0));
        self.button(p, f, &c, self.clear_box(), "Сбросить", Target::ClearHotkey, false);

        for sl in [Slider::Size, Slider::Sens, Slider::Dwell] {
            self.slider(p, f, &c, sl);
        }

        // путь к хранилищу и «Обзор»
        let vb = self.vault_box();
        p.fill_rrect(vb.x, vb.y, vb.w, vb.h, self.s(8.0), c.bg2, 1.0);
        let edge_a = if self.hot(Target::Vault) { 0.9 } else { 0.45 };
        p.stroke_rrect(vb.x, vb.y, vb.w, vb.h, self.s(8.0), c.edge, edge_a, 1.0);
        if vault_text {
            let r = self.vault_edit_rect();
            let (text, col) = if self.draft.vault.is_empty() {
                ("не задано", c.dim)
            } else {
                (self.draft.vault.as_str(), c.fg)
            };
            p.text_clipped(&f.regular, text, r.x, r.y + r.h / 2.0, self.s(13.0), col, r.w);
        }
        self.button(p, f, &c, self.browse_box(), "Обзор…", Target::Browse, false);

        // автозапуск — переключатель
        let tb = self.toggle_box();
        let on = self.autostart;
        let track = if on { c.acc } else { blend(c.bg2, c.dim, 0.35) };
        let track = if self.hot(Target::Autostart) {
            blend(track, c.fg, 0.08)
        } else {
            track
        };
        p.fill_rrect(tb.x, tb.y, tb.w, tb.h, tb.h / 2.0, track, 1.0);
        let r = tb.h / 2.0 - self.s(3.0);
        let kx = if on { tb.x + tb.w - tb.h / 2.0 } else { tb.x + tb.h / 2.0 };
        p.fill_circle(kx, tb.cy(), r, 0xFFFFFF, 1.0);
        p.text(
            &f.regular,
            if on { "включён" } else { "выключен" },
            tb.x + tb.w + self.s(12.0),
            tb.cy(),
            self.s(13.0),
            c.dim,
            Align::Left,
        );

        self.button(p, f, &c, self.menu_file_box(), "Открыть menu.yaml", Target::MenuFile, false);

        // низ: строка состояния и кнопки
        let fy = self.s(Self::footer_y());
        p.line(self.s(PAD), fy, w - self.s(PAD), fy, c.edge, 0.25, 1.0);
        let (cancel, save) = self.footer_boxes();
        if let Some(st) = &self.status {
            let col = if st.bad { c.bad } else { c.ok };
            p.text_clipped(
                &f.regular,
                &st.text,
                self.s(PAD),
                save.cy(),
                self.s(12.5),
                col,
                cancel.x - self.s(PAD + 8.0),
            );
        }
        self.button(p, f, &c, cancel, "Отмена", Target::Cancel, false);
        self.button(p, f, &c, save, "Сохранить", Target::Save, true);
    }

    #[allow(clippy::too_many_arguments)]
    fn segments(
        &self,
        p: &mut Painter,
        f: &Fonts,
        c: &Ui,
        boxes: &[Box2],
        labels: &[&str],
        selected: usize,
        target: impl Fn(usize) -> Target,
    ) {
        let (first, last) = (boxes[0], boxes[boxes.len() - 1]);
        let r = self.s(9.0);
        p.fill_rrect(first.x, first.y, last.x + last.w - first.x, first.h, r, c.bg2, 1.0);
        for (i, (b, label)) in boxes.iter().zip(labels).enumerate() {
            let inset = self.s(3.0);
            if i == selected {
                p.fill_rrect(b.x + inset, b.y + inset, b.w - 2.0 * inset, b.h - 2.0 * inset, r - inset, c.acc, 1.0);
            } else if self.hot(target(i)) {
                p.fill_rrect(b.x + inset, b.y + inset, b.w - 2.0 * inset, b.h - 2.0 * inset, r - inset, blend(c.bg2, c.fg, 0.08), 1.0);
            }
            let col = if i == selected { c.on_acc } else { c.fg };
            p.text(&f.regular, label, b.x + b.w / 2.0, b.cy(), self.s(13.0), col, Align::Center);
        }
    }

    fn slider(&self, p: &mut Painter, f: &Fonts, c: &Ui, sl: Slider) {
        let (x0, x1, cy) = self.track(sl);
        let (lo, hi, _) = sl.range();
        let v = sl.get(&self.draft);
        let t = (v - lo) as f32 / (hi - lo) as f32;
        let kx = x0 + (x1 - x0) * t;
        let th = self.s(4.0);
        p.fill_rrect(x0, cy - th / 2.0, x1 - x0, th, th / 2.0, blend(c.bg2, c.dim, 0.35), 1.0);
        p.fill_rrect(x0, cy - th / 2.0, kx - x0, th, th / 2.0, c.acc, 1.0);
        let hot = self.hot(Target::Slider(sl)) || self.drag == Some(sl);
        if hot {
            p.fill_circle(kx, cy, self.s(13.0), c.acc, 0.18);
        }
        p.fill_circle(kx, cy, self.s(9.0), c.acc, 1.0);
        p.fill_circle(kx, cy, self.s(4.0), 0xFFFFFF, 1.0);
        let vx = self.s(W - PAD);
        p.text(&f.regular, &sl.text(v), vx, cy, self.s(13.0), c.dim, Align::Right);
    }

    #[allow(clippy::too_many_arguments)]
    fn button(&self, p: &mut Painter, f: &Fonts, c: &Ui, b: Box2, label: &str, t: Target, primary: bool) {
        let base = if primary { c.acc } else { c.bg2 };
        let fill = if self.pressed == Some(t) {
            blend(base, c.fg, 0.16)
        } else if self.hover == Some(t) {
            blend(base, c.fg, 0.08)
        } else {
            base
        };
        p.fill_rrect(b.x, b.y, b.w, b.h, self.s(9.0), fill, 1.0);
        if !primary {
            p.stroke_rrect(b.x, b.y, b.w, b.h, self.s(9.0), c.edge, 0.35, 1.0);
        }
        let col = if primary { c.on_acc } else { c.fg };
        let font = if primary { &f.bold } else { &f.regular };
        p.text(font, label, b.x + b.w / 2.0, b.cy(), self.s(13.0), col, Align::Center);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> SettingsForm {
        SettingsForm::new(Settings::default(), false, 1.0)
    }

    fn center(b: Box2) -> (f32, f32) {
        (b.x + b.w / 2.0, b.cy())
    }

    fn click(f: &mut SettingsForm, b: Box2) -> Cmd {
        let (x, y) = center(b);
        let down = f.mouse_down(x, y);
        let up = f.mouse_up(x, y);
        if up == Cmd::Redraw || up == Cmd::None {
            down
        } else {
            up
        }
    }

    #[test]
    fn every_control_is_hittable_and_distinct() {
        let f = form();
        let mut boxes = vec![
            (f.close_box(), Target::Close),
            (f.vault_box(), Target::Vault),
            (f.browse_box(), Target::Browse),
            (f.toggle_box(), Target::Autostart),
            (f.menu_file_box(), Target::MenuFile),
            (f.hotkey_box(), Target::Hotkey),
            (f.clear_box(), Target::ClearHotkey),
        ];
        let (cancel, save) = f.footer_boxes();
        boxes.push((cancel, Target::Cancel));
        boxes.push((save, Target::Save));
        for (b, m) in f.seg_boxes(0, 3).into_iter().zip(ThemeMode::ALL) {
            boxes.push((b, Target::Theme(m)));
        }
        let (w, h) = f.size();
        for (b, t) in boxes {
            let (x, y) = center(b);
            assert_eq!(f.hit(x, y), Some(t), "{t:?}");
            assert!(b.x >= 0.0 && b.y >= 0.0 && b.x + b.w <= w as f32 && b.y + b.h <= h as f32, "{t:?} вне окна");
        }
        assert_eq!(f.hit(10.0, 10.0), Some(Target::Header));
    }

    #[test]
    fn clicks_edit_the_draft() {
        let mut f = form();
        let b = f.seg_boxes(0, 3)[1];
        assert_eq!(click(&mut f, b), Cmd::Redraw);
        assert_eq!(f.draft.theme, ThemeMode::Light);
        let b = f.seg_boxes(R_BUTTON, 3)[2];
        click(&mut f, b);
        assert_eq!(f.draft.button, 2);
        let b = f.toggle_box();
        click(&mut f, b);
        assert!(f.autostart);
        let (cancel, save) = f.footer_boxes();
        assert_eq!(click(&mut f, save), Cmd::Save);
        assert_eq!(click(&mut f, cancel), Cmd::Close);
        let b = f.browse_box();
        assert_eq!(click(&mut f, b), Cmd::Browse);
        let b = f.menu_file_box();
        assert_eq!(click(&mut f, b), Cmd::OpenMenuFile);
        let b = f.vault_box();
        assert_eq!(click(&mut f, b), Cmd::FocusVault);
    }

    #[test]
    fn records_a_shortcut() {
        let mut f = form();
        let b = f.hotkey_box();
        assert_eq!(click(&mut f, b), Cmd::StartRecording);
        assert!(f.is_recording());
        // одни модификаторы — ждём дальше
        assert_eq!(f.record(vk::LCONTROL, true, false, false, false), Cmd::None);
        assert_eq!(f.record(vk::SPACE, true, true, false, false), Cmd::StopRecording);
        assert_eq!(f.draft.hotkey.unwrap().display(), "Ctrl+Alt+Space");
        assert!(!f.is_recording());
        // Esc во время записи — отмена записи, а не закрытие окна
        let b = f.hotkey_box();
        click(&mut f, b);
        assert_eq!(f.key(Key::Escape), Cmd::StopRecording);
        assert!(f.draft.hotkey.is_some());
        assert_eq!(f.key(Key::Escape), Cmd::Close);
        // Backspace — убрать сочетание
        let b = f.hotkey_box();
        click(&mut f, b);
        f.record(vk::BACK, false, false, false, false);
        assert_eq!(f.draft.hotkey, None);
        // щелчок мимо во время записи — запись отменяется, перехват возвращается
        let b = f.hotkey_box();
        click(&mut f, b);
        let t = f.seg_boxes(R_THEME, 3)[2];
        assert_eq!(click(&mut f, t), Cmd::StopRecording);
        assert!(!f.is_recording());
    }

    #[test]
    fn cannot_save_without_any_trigger() {
        let mut f = form();
        let none = f.seg_boxes(R_BUTTON, 3)[0];
        click(&mut f, none);
        assert_eq!(f.draft.button, 0);
        assert_eq!(f.key(Key::Enter), Cmd::Redraw);
        assert!(f.status.as_ref().is_some_and(|s| s.bad));
        let b = f.hotkey_box();
        click(&mut f, b);
        f.record(vk::F1 + 12, false, false, false, false);
        assert_eq!(f.key(Key::Enter), Cmd::Save);
    }

    #[test]
    fn button_fires_only_if_released_over_it() {
        let mut f = form();
        let (x, y) = center(f.footer_boxes().1);
        f.mouse_down(x, y);
        assert_eq!(f.mouse_up(5.0, 300.0), Cmd::Redraw);
    }

    #[test]
    fn slider_drags_snaps_and_clamps() {
        let mut f = form();
        let (x0, x1, cy) = f.track(Slider::Size);
        f.mouse_down(x0, cy);
        assert_eq!(f.draft.size_percent, SIZE_RANGE.0);
        f.mouse_move(x1 + 500.0, cy); // за край — максимум
        assert_eq!(f.draft.size_percent, SIZE_RANGE.1);
        f.mouse_move((x0 + x1) / 2.0 + 1.0, cy);
        assert_eq!(f.draft.size_percent % 5, 0);
        assert_eq!(f.draft.size_percent, 100);
        f.mouse_up(0.0, 0.0);
        f.mouse_move(x0, cy + 200.0); // после отпускания ползунок не тянется
        assert_eq!(f.draft.size_percent, 100);
        let (_, _, dy) = f.track(Slider::Dwell);
        f.wheel(x0, dy, true);
        assert_eq!(f.draft.dwell_ms, 500);
        for _ in 0..100 {
            f.wheel(x0, dy, false);
        }
        assert_eq!(f.draft.dwell_ms, DWELL_RANGE.0);
    }

    #[test]
    fn scales_with_dpi() {
        let a = SettingsForm::new(Settings::default(), false, 1.0).size();
        let b = SettingsForm::new(Settings::default(), false, 1.5).size();
        assert!((b.0 as f32 / a.0 as f32 - 1.5).abs() < 0.01);
        let f = SettingsForm::new(Settings::default(), false, 1.5);
        let (x, y) = center(f.toggle_box());
        assert_eq!(f.hit(x, y), Some(Target::Autostart));
    }

    #[test]
    fn draws_in_both_themes() {
        let Some(fonts) = Fonts::load() else { return };
        for theme in Theme::ALL {
            let f = form();
            let (w, h) = f.size();
            let mut p = Painter::new(w as u32, h as u32).unwrap();
            f.draw(&mut p, &fonts, theme, true);
            let bg = palette::ui(theme).bg;
            let px = &p.rgba()[(20 * w as usize + w as usize / 2) * 4..][..3];
            assert_eq!(px, [(bg >> 16) as u8, (bg >> 8) as u8, bg as u8], "{theme:?}");
        }
    }
}

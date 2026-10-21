//! Окно настроек без ОС: раскладка, попадание по элементам, правка черновика настроек и
//! отрисовка в стиле меню (стеклянные «таблетки», акцентный синий). Платформа подаёт
//! события мыши и клавиш, показывает готовую картинку и исполняет команды (`Cmd`).
//!
//! Три страницы: «Меню» (карточки меню: название, вызов, файл), «Общие» (тема, размеры, заметки,
//! автозапуск) и «О программе» (версия, обновление, журнал).

use crate::model::action::vk;
use crate::model::settings::{DWELL_RANGE, SENS_RANGE, SIZE_RANGE};
use crate::model::{clean_name, Chord, ProfileCfg, Settings, Theme, ThemeMode, MAIN_NAME, MAX_PROFILES, NAME_MAX};
use crate::render::painter::{blend, Align, Painter};
use crate::render::palette::{self, Ui};

/// Базовые размеры (при 96 dpi), px.
const W: f32 = 480.0;
const HEAD: f32 = 56.0;
const PAD: f32 = 24.0;
const TABS_Y: f32 = HEAD + 16.0;
const TABS_H: f32 = 36.0;
const BODY_Y: f32 = TABS_Y + TABS_H + 16.0;
const BODY_H: f32 = 470.0;
const FOOT_H: f32 = 72.0;
const H: f32 = BODY_Y + BODY_H + FOOT_H;
const CTRL_H: f32 = 32.0;
const BTN_W: f32 = 112.0;
const ROW: f32 = 44.0;
const GROUP_H: f32 = 30.0;
/// Цвет предупреждения («не назначен вызов»).
const WARN: u32 = 0xE8A76A;

// карточка меню
const CARD_W: f32 = W - 2.0 * PAD;
const CARD_HEAD: f32 = 52.0;
const CARD_GAP: f32 = 10.0;
const CARD_ROW: f32 = 40.0;
const CARD_PAD: f32 = 14.0;
const LABEL_W: f32 = 64.0;
const FIELD_X: f32 = PAD + CARD_PAD + LABEL_W + 12.0;
const FIELD_W: f32 = CARD_W - CARD_PAD - LABEL_W - 12.0 - CARD_PAD;
const BTN_CHIP_W: f32 = 126.0;
const KEY_CHIP_W: f32 = 104.0;
const ASSIGN_W: f32 = 82.0;
const ADD_H: f32 = 44.0;

// страница «Общие»
const LABEL_X: f32 = PAD;
const CTRL_X: f32 = PAD + 160.0;
const VALUE_W: f32 = 64.0;
const BROWSE_W: f32 = 84.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Menus,
    General,
    About,
}

impl Page {
    const ALL: [Page; 3] = [Page::Menus, Page::General, Page::About];

    fn title(self) -> &'static str {
        match self {
            Page::Menus => "Меню",
            Page::General => "Общие",
            Page::About => "О программе",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slider {
    Size,
    Sens,
    Dwell,
}

impl Slider {
    const ALL: [Slider; 3] = [Slider::Size, Slider::Sens, Slider::Dwell];

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

    fn label(self) -> &'static str {
        match self {
            Slider::Size => "Размер меню",
            Slider::Sens => "Чувствительность",
            Slider::Dwell => "Задержка веера",
        }
    }
}

/// Элемент окна, по которому можно попасть мышью.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Close,
    Header,
    Page(Page),
    /// Заголовок карточки меню: раскрыть её.
    Card(usize),
    /// «× » на чипе боковой кнопки / сочетания выбранного меню.
    ClearButton,
    ClearHotkey,
    /// «+ Назначить» (и «Нажмите…» во время записи).
    Assign,
    MenuFile,
    /// «Сбросить» состав меню к стандартному (с подтверждением вторым нажатием).
    ResetMenu,
    RemoveMenu,
    AddMenu,
    Theme(ThemeMode),
    Slider(Slider),
    Vault,
    Browse,
    Autostart,
    /// «Сбросить» общие настройки к стандартным (с подтверждением вторым нажатием).
    ResetGeneral,
    AutoCheck,
    /// Кнопка в строке обновления (что она делает — зависит от состояния).
    Update,
    OpenLog,
    OpenFolder,
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
    /// Открыть файл меню в редакторе: 0 — menu.yaml, иначе номер меню (menu-N.yaml).
    OpenMenuFile(u8),
    /// Вернуть файл меню к стандартному (прежний останется как .bak): 0 — menu.yaml, иначе номер меню.
    ResetMenuFile(u8),
    /// Общие настройки сброшены в черновике — платформа записывает их в файл как «стандартные».
    ResetGeneral,
    /// Начать запись вызова: платформа отключает свой перехват клавиш и кнопок, пока идёт запись.
    StartRecording,
    /// Запись закончена (или отменена): вернуть перехват и перерисовать.
    StopRecording,
    /// Начать перетаскивание окна за шапку.
    DragWindow,
    /// Перевести фокус в поле пути.
    FocusVault,
    /// Спросить, не вышла ли новая версия.
    CheckUpdates,
    /// Скачать и установить найденную версию.
    Update,
    /// Открыть журнал работы.
    OpenLog,
    /// Открыть папку с настройками.
    OpenFolder,
}

/// Что известно об обновлении (платформа обновляет это, пока окно открыто).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum UpdateView {
    /// Ещё не проверяли.
    #[default]
    Idle,
    Checking,
    UpToDate,
    /// Доступна версия (текст версии).
    Available(String),
    /// Скачивание, %.
    Downloading(u8),
    /// Скачано, запускается установщик.
    Installing,
    /// Не получилось; `retry` — можно повторить скачивание (а не только проверку).
    Failed { msg: String, retry: bool },
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

/// Куда попали внутри раскрытой карточки меню.
struct CardBody {
    name: Box2,
    call: Box2,
    file: Box2,
    reset: Box2,
    remove: Option<Box2>,
}

struct Card {
    /// Вся карточка (рамка).
    full: Box2,
    header: Box2,
    body: Option<CardBody>,
}

/// Из чего состоит строка «Вызов» выбранного меню.
struct Call {
    button_chip: Option<Box2>,
    clear_button: Option<Box2>,
    key_chip: Option<Box2>,
    clear_key: Option<Box2>,
    assign: Box2,
}

pub struct SettingsForm {
    /// Черновик: что будет сохранено по «Сохранить».
    pub draft: Settings,
    /// Запускать с ОС (хранится не в файле настроек, а в системе).
    pub autostart: bool,
    pub status: Option<Status>,
    /// Версия этой сборки, «0.8.0».
    pub version: String,
    pub update: UpdateView,
    pub page: Page,
    /// Раскрытая карточка меню: 0 — основное, k — `draft.profiles[k - 1]`.
    pub selected: usize,
    scale: f32,
    hover: Option<Target>,
    pressed: Option<Target>,
    drag: Option<Slider>,
    recording: bool,
    /// Кнопка «Сбросить», по которой уже нажали один раз (ждёт подтверждения).
    armed: Option<Target>,
}

impl SettingsForm {
    pub fn new(settings: Settings, autostart: bool, scale: f32) -> SettingsForm {
        SettingsForm {
            draft: settings,
            autostart,
            status: None,
            version: String::new(),
            update: UpdateView::Idle,
            page: Page::Menus,
            selected: 0,
            scale: scale.max(0.5),
            hover: None,
            pressed: None,
            drag: None,
            recording: false,
            armed: None,
        }
    }

    /// Идёт запись вызова: клавиши и боковые кнопки — в `record*`, а не в `key`.
    pub fn is_recording(&self) -> bool {
        self.recording
    }

    fn s(&self, v: f32) -> f32 {
        v * self.scale
    }

    fn b(&self, x: f32, y: f32, w: f32, h: f32) -> Box2 {
        Box2 { x: self.s(x), y: self.s(y), w: self.s(w), h: self.s(h) }
    }

    /// Размер окна в px.
    pub fn size(&self) -> (i32, i32) {
        (self.s(W).round() as i32, self.s(H).round() as i32)
    }

    // ---------------------------------------------------------------- меню как данные

    fn n_menus(&self) -> usize {
        1 + self.draft.profiles.len()
    }

    fn profile(&self, k: usize) -> Option<&ProfileCfg> {
        k.checked_sub(1).and_then(|k| self.draft.profiles.get(k))
    }

    fn profile_mut(&mut self, k: usize) -> Option<&mut ProfileCfg> {
        k.checked_sub(1).and_then(|k| self.draft.profiles.get_mut(k))
    }

    /// Название меню `k` (с запасным по умолчанию, если пусто).
    pub fn menu_name(&self, k: usize) -> String {
        match self.profile(k) {
            Some(p) => clean_name(&p.name).unwrap_or_else(|| ProfileCfg::default_name(p.id)),
            None => clean_name(&self.draft.main_name).unwrap_or_else(|| MAIN_NAME.to_string()),
        }
    }

    fn trigger_of(&self, k: usize) -> (u8, Option<Chord>) {
        match self.profile(k) {
            Some(p) => (p.button, p.hotkey),
            None => (self.draft.button, self.draft.hotkey),
        }
    }

    /// Кнопка мыши и сочетание выбранного меню.
    fn current(&self) -> (u8, Option<Chord>) {
        self.trigger_of(self.selected)
    }

    fn set_button(&mut self, b: u8) {
        let k = self.selected;
        match self.profile_mut(k) {
            Some(p) => p.button = b,
            None => self.draft.button = b,
        }
    }

    fn set_hotkey(&mut self, c: Option<Chord>) {
        let k = self.selected;
        match self.profile_mut(k) {
            Some(p) => p.hotkey = c,
            None => self.draft.hotkey = c,
        }
    }

    /// Номер файла меню выбранной карточки: 0 — menu.yaml, иначе `menu-N.yaml`.
    fn menu_id(&self) -> u8 {
        self.profile(self.selected).map_or(0, |p| p.id)
    }

    fn can_add_menu(&self) -> bool {
        self.draft.profiles.len() < MAX_PROFILES
    }

    /// Название выбранного меню для поля ввода.
    pub fn selected_name(&self) -> String {
        match self.profile(self.selected) {
            Some(p) => p.name.clone(),
            None => self.draft.main_name.clone(),
        }
    }

    /// Название меню изменили в поле ввода.
    pub fn set_name(&mut self, name: &str) {
        let limited: String = name.chars().take(NAME_MAX).collect();
        let k = self.selected;
        match self.profile_mut(k) {
            Some(p) => p.name = limited,
            None => self.draft.main_name = limited,
        }
    }

    /// Текст поля пути изменили в родном поле ввода.
    pub fn set_vault(&mut self, path: &str) {
        self.draft.vault = path.to_string();
    }

    // ---------------------------------------------------------------- раскладка

    fn page_boxes(&self) -> Vec<Box2> {
        let w = CARD_W / Page::ALL.len() as f32;
        (0..Page::ALL.len()).map(|i| self.b(PAD + w * i as f32, TABS_Y, w, TABS_H)).collect()
    }

    fn close_box(&self) -> Box2 {
        self.b(W - PAD - 28.0 + 6.0, 14.0, 28.0, 28.0)
    }

    fn footer_y() -> f32 {
        H - FOOT_H
    }

    fn footer_boxes(&self) -> (Box2, Box2) {
        let y = Self::footer_y() + 20.0;
        let save = self.b(W - PAD - BTN_W, y, BTN_W, CTRL_H + 2.0);
        let cancel = self.b(W - PAD - 2.0 * BTN_W - 10.0, y, BTN_W, CTRL_H + 2.0);
        (cancel, save)
    }

    /// Карточки меню и кнопка «Добавить меню».
    fn cards(&self) -> (Vec<Card>, Option<Box2>) {
        let mut y = BODY_Y;
        let mut cards = Vec::new();
        for k in 0..self.n_menus() {
            let open = k == self.selected;
            let rows = if k == 0 { 3.0 } else { 4.0 };
            let h = if open { CARD_HEAD + 2.0 + rows * CARD_ROW + 8.0 } else { CARD_HEAD };
            let header = self.b(PAD, y, CARD_W, CARD_HEAD);
            let body = open.then(|| {
                let r0 = y + CARD_HEAD + 2.0;
                let row = |i: f32| r0 + i * CARD_ROW + 4.0;
                CardBody {
                    name: self.b(FIELD_X, row(0.0), FIELD_W, CTRL_H),
                    call: self.b(FIELD_X, row(1.0), FIELD_W, CTRL_H),
                    file: self.b(FIELD_X, row(2.0), 170.0, CTRL_H),
                    reset: self.b(FIELD_X + 170.0 + 8.0, row(2.0), FIELD_W - 170.0 - 8.0, CTRL_H),
                    remove: (k != 0).then(|| self.b(FIELD_X, row(3.0), 130.0, CTRL_H)),
                }
            });
            cards.push(Card { full: self.b(PAD, y, CARD_W, h), header, body });
            y += h + CARD_GAP;
        }
        let add = self.can_add_menu().then(|| self.b(PAD, y, CARD_W, ADD_H));
        (cards, add)
    }

    /// Чипы и «+ Назначить» в строке «Вызов» (внутри `area`).
    fn call_boxes(&self, area: Box2) -> Call {
        let (button, hotkey) = self.current();
        if self.recording {
            return Call { button_chip: None, clear_button: None, key_chip: None, clear_key: None, assign: area };
        }
        let mut x = area.x;
        let chip = |x: f32, w: f32| Box2 { x, y: area.y, w: self.s(w), h: area.h };
        let cross = |c: Box2| Box2 { x: c.x + c.w - self.s(26.0), w: self.s(26.0), ..c };
        let (mut bc, mut cb, mut kc, mut ck) = (None, None, None, None);
        if button != 0 {
            let c = chip(x, BTN_CHIP_W);
            (bc, cb) = (Some(c), Some(cross(c)));
            x += self.s(BTN_CHIP_W + 8.0);
        }
        if hotkey.is_some() {
            let c = chip(x, KEY_CHIP_W);
            (kc, ck) = (Some(c), Some(cross(c)));
            x += self.s(KEY_CHIP_W + 8.0);
        }
        Call { button_chip: bc, clear_button: cb, key_chip: kc, clear_key: ck, assign: Box2 { x, w: self.s(ASSIGN_W), ..area } }
    }

    /// Короткая подпись чипа в строке «Вызов».
    fn button_short(b: u8) -> &'static str {
        match b {
            1 => "Кнопка «назад»",
            2 => "Кнопка «вперёд»",
            _ => "",
        }
    }

    fn button_name(b: u8) -> &'static str {
        match b {
            1 => "Боковая «назад»",
            2 => "Боковая «вперёд»",
            _ => "",
        }
    }

    // страница «Общие»: строки и группы подряд, y — верх строки
    fn general_rows(&self) -> GeneralRows {
        let mut y = BODY_Y;
        let mut g = GeneralRows::default();
        let group = |y: &mut f32, name: &'static str, g: &mut GeneralRows| {
            g.groups.push((name, *y));
            *y += GROUP_H;
        };
        group(&mut y, "Внешний вид", &mut g);
        g.theme = y;
        y += ROW;
        g.sliders[0] = y;
        y += ROW;
        group(&mut y, "Управление", &mut g);
        g.sliders[1] = y;
        y += ROW;
        g.sliders[2] = y;
        y += ROW;
        group(&mut y, "Заметки", &mut g);
        g.vault = y;
        y += ROW;
        group(&mut y, "Система", &mut g);
        g.auto = y;
        y += ROW;
        group(&mut y, "Стандартные значения", &mut g);
        g.reset = y;
        g
    }

    fn ctrl(&self, row_y: f32, x: f32, w: f32) -> Box2 {
        self.b(x, row_y + (ROW - CTRL_H) / 2.0, w, CTRL_H)
    }

    fn theme_boxes(&self) -> Vec<Box2> {
        let full = self.ctrl(self.general_rows().theme, CTRL_X, W - PAD - CTRL_X);
        let w = full.w / 3.0;
        (0..3).map(|i| Box2 { x: full.x + w * i as f32, w, ..full }).collect()
    }

    /// Дорожка ползунка: (x0, x1, cy).
    fn track(&self, s: Slider) -> (f32, f32, f32) {
        let rows = self.general_rows();
        let i = Slider::ALL.iter().position(|&x| x == s).unwrap_or(0);
        let c = self.ctrl(rows.sliders[i], CTRL_X, W - PAD - CTRL_X - VALUE_W);
        let r = self.s(9.0);
        (c.x + r, c.x + c.w - r, c.cy())
    }

    fn vault_box(&self) -> Box2 {
        self.ctrl(self.general_rows().vault, CTRL_X, W - PAD - CTRL_X - BROWSE_W - 8.0)
    }

    fn browse_box(&self) -> Box2 {
        let v = self.vault_box();
        Box2 { x: v.x + v.w + self.s(8.0), w: self.s(BROWSE_W), ..v }
    }

    fn toggle_box_at(&self, row_y: f32) -> Box2 {
        let c = self.ctrl(row_y, W - PAD - 46.0, 46.0);
        Box2 { y: c.cy() - self.s(13.0), h: self.s(26.0), ..c }
    }

    /// Кнопка «Сбросить» на странице «Общие».
    fn reset_general_box(&self) -> Box2 {
        self.b(CTRL_X, self.general_rows().reset + (ROW - CTRL_H) / 2.0, W - PAD - CTRL_X, CTRL_H)
    }

    fn autostart_box(&self) -> Box2 {
        self.toggle_box_at(self.general_rows().auto)
    }

    /// Где платформе разместить родное поле пути (внутри рамки поля): только на странице «Общие».
    pub fn vault_edit_rect(&self) -> Option<Box2> {
        (self.page == Page::General).then(|| {
            let b = self.vault_box();
            let (px, py) = (self.s(10.0), self.s(8.0));
            Box2 { x: b.x + px, y: b.y + py, w: b.w - 2.0 * px, h: b.h - 2.0 * py }
        })
    }

    /// Где разместить родное поле названия раскрытого меню: только на странице «Меню».
    pub fn name_edit_rect(&self) -> Option<Box2> {
        if self.page != Page::Menus {
            return None;
        }
        let (cards, _) = self.cards();
        let n = cards.get(self.selected)?.body.as_ref()?.name;
        let (px, py) = (self.s(10.0), self.s(8.0));
        Some(Box2 { x: n.x + px, y: n.y + py, w: n.w - 2.0 * px, h: n.h - 2.0 * py })
    }

    // страница «О программе»
    fn about_rows(&self) -> AboutRows {
        let hero = BODY_Y + 6.0;
        let g1 = hero + 84.0;
        let update = g1 + GROUP_H;
        let auto = update + 56.0;
        let g2 = auto + ROW;
        let log = g2 + GROUP_H;
        AboutRows { hero, g1, update, auto, g2, log, folder: log + ROW }
    }

    /// Кнопка строки обновления и её подпись; `None` — пока идёт работа, кнопки нет.
    fn update_button(&self) -> Option<(Box2, &'static str, bool)> {
        let (label, primary) = match &self.update {
            UpdateView::Idle | UpdateView::UpToDate => ("Проверить", false),
            UpdateView::Available(_) => ("Обновить", true),
            UpdateView::Failed { retry: true, .. } => ("Повторить", true),
            UpdateView::Failed { .. } => ("Проверить", false),
            UpdateView::Checking | UpdateView::Downloading(_) | UpdateView::Installing => return None,
        };
        let r = self.about_rows();
        Some((self.b(W - PAD - 100.0, r.update + 8.0, 100.0, CTRL_H), label, primary))
    }

    fn update_text_box(&self) -> Box2 {
        let r = self.about_rows();
        let w = if self.update_button().is_some() { CARD_W - 100.0 - 12.0 } else { CARD_W };
        self.b(PAD, r.update, w, 52.0)
    }

    fn small_button(&self, row_y: f32) -> Box2 {
        self.b(W - PAD - 100.0, row_y + (ROW - CTRL_H) / 2.0, 100.0, CTRL_H)
    }

    // ---------------------------------------------------------------- попадание

    /// Все элементы текущей страницы и окна, по которым можно попасть, в порядке проверки.
    fn items(&self) -> Vec<(Box2, Target)> {
        let mut v = vec![(self.close_box(), Target::Close)];
        let (cancel, save) = self.footer_boxes();
        v.push((cancel, Target::Cancel));
        v.push((save, Target::Save));
        for (b, p) in self.page_boxes().into_iter().zip(Page::ALL) {
            v.push((b, Target::Page(p)));
        }
        match self.page {
            Page::Menus => {
                let (cards, add) = self.cards();
                for (k, c) in cards.iter().enumerate() {
                    if let Some(body) = &c.body {
                        let call = self.call_boxes(body.call);
                        v.push((call.assign, Target::Assign));
                        for (b, t) in [(call.clear_button, Target::ClearButton), (call.clear_key, Target::ClearHotkey)] {
                            if let Some(b) = b {
                                v.push((b, t));
                            }
                        }
                        v.push((body.file, Target::MenuFile));
                        v.push((body.reset, Target::ResetMenu));
                        if let Some(r) = body.remove {
                            v.push((r, Target::RemoveMenu));
                        }
                    }
                    v.push((c.header, Target::Card(k)));
                }
                if let Some(a) = add {
                    v.push((a, Target::AddMenu));
                }
            }
            Page::General => {
                for (b, m) in self.theme_boxes().into_iter().zip(ThemeMode::ALL) {
                    v.push((b, Target::Theme(m)));
                }
                for s in Slider::ALL {
                    let (x0, x1, cy) = self.track(s);
                    let grip = self.s(14.0);
                    v.push((Box2 { x: x0 - grip, y: cy - grip, w: x1 - x0 + 2.0 * grip, h: 2.0 * grip }, Target::Slider(s)));
                }
                v.push((self.vault_box(), Target::Vault));
                v.push((self.browse_box(), Target::Browse));
                v.push((self.autostart_box(), Target::Autostart));
                v.push((self.reset_general_box(), Target::ResetGeneral));
            }
            Page::About => {
                let r = self.about_rows();
                if let Some((b, _, _)) = self.update_button() {
                    v.push((b, Target::Update));
                }
                v.push((self.toggle_box_at(r.auto), Target::AutoCheck));
                v.push((self.small_button(r.log), Target::OpenLog));
                v.push((self.small_button(r.folder), Target::OpenFolder));
            }
        }
        v
    }

    pub fn hit(&self, x: f32, y: f32) -> Option<Target> {
        if let Some((_, t)) = self.items().into_iter().find(|(b, _)| b.contains(x, y)) {
            return Some(t);
        }
        (y < self.s(HEAD)).then_some(Target::Header)
    }

    fn slide_to(&mut self, s: Slider, x: f32) {
        let (x0, x1, _) = self.track(s);
        let (lo, hi, _) = s.range();
        let t = ((x - x0) / (x1 - x0)).clamp(0.0, 1.0);
        s.set(&mut self.draft, lo + (t * (hi - lo) as f32).round() as u32);
    }

    // ---------------------------------------------------------------- события

    pub fn mouse_move(&mut self, x: f32, y: f32) -> Cmd {
        if let Some(s) = self.drag {
            let before = s.get(&self.draft);
            self.slide_to(s, x);
            return if s.get(&self.draft) != before { Cmd::Redraw } else { Cmd::None };
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
        if !matches!(t, Target::ResetMenu | Target::ResetGeneral) {
            self.armed = None; // нажали что-то другое — подтверждение сброса снимается
        }
        // щелчок мимо «Назначить» отменяет запись
        let stop = self.recording && t != Target::Assign;
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
            Target::Page(p) => {
                self.page = p;
                Cmd::Redraw
            }
            Target::Card(k) => {
                self.selected = k.min(self.n_menus() - 1);
                Cmd::Redraw
            }
            Target::Assign => {
                self.recording = !self.recording;
                if self.recording {
                    self.status = Some(Status {
                        text: "Нажмите боковую кнопку мыши или сочетание клавиш. Esc — отмена".into(),
                        bad: false,
                    });
                    Cmd::StartRecording
                } else {
                    Cmd::StopRecording
                }
            }
            Target::Theme(m) => {
                self.draft.theme = m;
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
            Target::AutoCheck => {
                self.draft.check_updates = !self.draft.check_updates;
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
            Target::ClearButton => {
                self.set_button(0);
                Cmd::Redraw
            }
            Target::ClearHotkey => {
                self.set_hotkey(None);
                Cmd::Redraw
            }
            Target::Browse => Cmd::Browse,
            Target::MenuFile => Cmd::OpenMenuFile(self.menu_id()),
            Target::ResetMenu => {
                if self.armed.take() == Some(Target::ResetMenu) {
                    Cmd::ResetMenuFile(self.menu_id())
                } else {
                    self.armed = Some(Target::ResetMenu);
                    Cmd::Redraw
                }
            }
            Target::ResetGeneral => {
                if self.armed.take() == Some(Target::ResetGeneral) {
                    self.draft.reset_general();
                    Cmd::ResetGeneral
                } else {
                    self.armed = Some(Target::ResetGeneral);
                    Cmd::Redraw
                }
            }
            Target::AddMenu => {
                self.add_menu();
                Cmd::Redraw
            }
            Target::RemoveMenu => {
                if let Some(k) = self.selected.checked_sub(1).filter(|&k| k < self.draft.profiles.len()) {
                    self.draft.profiles.remove(k);
                }
                self.selected = 0;
                Cmd::Redraw
            }
            Target::Update => match self.update {
                UpdateView::Available(_) | UpdateView::Failed { retry: true, .. } => Cmd::Update,
                UpdateView::Idle | UpdateView::UpToDate | UpdateView::Failed { .. } => Cmd::CheckUpdates,
                _ => Cmd::Redraw,
            },
            Target::OpenLog => Cmd::OpenLog,
            Target::OpenFolder => Cmd::OpenFolder,
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
        s.set(&mut self.draft, if up { v + step } else { v.saturating_sub(step) });
        Cmd::Redraw
    }

    pub fn key(&mut self, k: Key) -> Cmd {
        match k {
            Key::Escape if self.recording => {
                self.recording = false;
                self.status = None;
                Cmd::StopRecording
            }
            Key::Escape => Cmd::Close,
            Key::Enter => self.try_save(),
        }
    }

    /// Какое другое меню уже вызывается этой кнопкой / сочетанием (номер карточки).
    fn taken_by(&self, button: u8, hotkey: Option<Chord>) -> Option<usize> {
        (0..self.n_menus()).filter(|&k| k != self.selected).find(|&k| {
            let (b, h) = self.trigger_of(k);
            (button != 0 && b == button) || (hotkey.is_some() && h == hotkey)
        })
    }

    fn assigned(&mut self) -> Cmd {
        self.recording = false;
        self.status = None;
        Cmd::StopRecording
    }

    fn refused(&mut self, other: usize, what: &str) -> Cmd {
        self.recording = false;
        self.status = Some(Status { text: format!("{what} уже вызывает меню «{}»", self.menu_name(other)), bad: true });
        Cmd::StopRecording
    }

    /// Боковую кнопку нажали во время записи (`b`: 1 — «назад», 2 — «вперёд»).
    pub fn record_button(&mut self, b: u8) -> Cmd {
        if !self.recording || !(1..=2).contains(&b) {
            return Cmd::None;
        }
        if let Some(o) = self.taken_by(b, None) {
            return self.refused(o, "Эта кнопка");
        }
        self.set_button(b);
        self.assigned()
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
            self.status = None;
            return Cmd::StopRecording;
        }
        if plain && (key == vk::BACK || key == vk::DELETE) {
            self.set_hotkey(None);
            return self.assigned();
        }
        match Chord::from_press(key, ctrl, alt, shift, win) {
            Some(c) => {
                if let Some(o) = self.taken_by(0, Some(c)) {
                    return self.refused(o, "Это сочетание");
                }
                self.set_hotkey(Some(c));
                self.assigned()
            }
            None => Cmd::None,
        }
    }

    /// Новое меню: ближайший свободный номер и боковая кнопка, которую ещё никто не занял.
    fn add_menu(&mut self) {
        if !self.can_add_menu() {
            return;
        }
        let id = (2u8..=4).find(|id| !self.draft.profiles.iter().any(|p| p.id == *id)).unwrap_or(2);
        let used = |b: u8| self.draft.button == b || self.draft.profiles.iter().any(|p| p.button == b);
        let button = [1u8, 2].into_iter().find(|&b| !used(b)).unwrap_or(0);
        self.draft.profiles.push(ProfileCfg::new(id, button));
        self.draft.profiles.sort_by_key(|p| p.id);
        self.selected = self.draft.profiles.iter().position(|p| p.id == id).map_or(0, |k| k + 1);
    }

    /// Не сохраняем, если какое-то меню нечем открыть или два меню открываются одним и тем же.
    fn check_triggers(&self) -> Result<(), (usize, String)> {
        for k in 0..self.n_menus() {
            let (button, hotkey) = self.trigger_of(k);
            if button == 0 && hotkey.is_none() {
                return Err((k, format!("«{}»: назначьте кнопку мыши или сочетание клавиш", self.menu_name(k))));
            }
        }
        for a in 0..self.n_menus() {
            for b in a + 1..self.n_menus() {
                let ((ba, ha), (bb, hb)) = (self.trigger_of(a), self.trigger_of(b));
                let same_button = ba != 0 && ba == bb;
                let same_key = ha.is_some() && ha == hb;
                if same_button || same_key {
                    let what = if same_button { "кнопка мыши" } else { "сочетание" };
                    return Err((b, format!("«{}» и «{}»: одна и та же {what}", self.menu_name(a), self.menu_name(b))));
                }
            }
        }
        Ok(())
    }

    fn try_save(&mut self) -> Cmd {
        self.draft.main_name = self.menu_name(0);
        for k in 1..self.n_menus() {
            let name = self.menu_name(k);
            if let Some(p) = self.profile_mut(k) {
                p.name = name;
            }
        }
        if let Err((k, text)) = self.check_triggers() {
            self.page = Page::Menus;
            self.selected = k; // показать, где поправить
            self.status = Some(Status { text, bad: true });
            return Cmd::Redraw;
        }
        Cmd::Save
    }

    // ---------------------------------------------------------------- отрисовка

    fn hot(&self, t: Target) -> bool {
        self.hover == Some(t) || self.pressed == Some(t)
    }

    /// Нарисовать окно. `edit_text` — рисовать ли текст полей ввода (если поверх нет родных полей).
    pub fn draw(&self, p: &mut Painter, f: &Fonts, theme: Theme, edit_text: bool) {
        let c = palette::ui(theme);
        let (w, h) = (p.width() as f32, p.height() as f32);
        p.clear(c.bg);
        p.stroke_rrect(0.0, 0.0, w, h, 0.0, c.edge, 0.85, 1.0);
        self.draw_head(p, f, &c);

        // страницы
        let boxes = self.page_boxes();
        let names: Vec<&str> = Page::ALL.iter().map(|p| p.title()).collect();
        let sel = Page::ALL.iter().position(|&x| x == self.page).unwrap_or(0);
        self.segments(p, f, &c, &boxes, &names, sel, |i| Target::Page(Page::ALL[i]));

        match self.page {
            Page::Menus => self.draw_menus(p, f, &c, edit_text),
            Page::General => self.draw_general(p, f, &c, edit_text),
            Page::About => self.draw_about(p, f, &c),
        }

        // низ: строка состояния и кнопки
        let fy = self.s(Self::footer_y());
        p.line(self.s(PAD), fy, w - self.s(PAD), fy, c.edge, 0.25, 1.0);
        let (cancel, save) = self.footer_boxes();
        if let Some(st) = &self.status {
            let col = if st.bad { c.bad } else { c.ok };
            p.text_clipped(&f.regular, &st.text, self.s(PAD), save.cy(), self.s(12.5), col, cancel.x - self.s(PAD + 8.0));
        }
        self.button(p, f, &c, cancel, "Отмена", Target::Cancel, false);
        self.button(p, f, &c, save, "Сохранить", Target::Save, true);
    }

    fn draw_head(&self, p: &mut Painter, f: &Fonts, c: &Ui) {
        let w = p.width() as f32;
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
    }

    /// Чип-«таблетка» с текстом внутри `b`.
    #[allow(clippy::too_many_arguments)]
    fn chip(&self, p: &mut Painter, f: &Fonts, c: &Ui, b: Box2, text: &str, accent: u32, cross: bool) {
        let r = b.h / 2.0;
        p.fill_rrect(b.x, b.y, b.w, b.h, r, blend(c.bg, accent, 0.18), 1.0);
        p.stroke_rrect(b.x, b.y, b.w, b.h, r, accent, 0.55, 1.0);
        let pad = self.s(10.0);
        let room = b.w - 2.0 * pad - if cross { self.s(14.0) } else { 0.0 };
        p.text_clipped(&f.regular, text, b.x + pad, b.cy(), self.s(12.0), c.fg, room);
        if cross {
            let (cx, cy, k) = (b.x + b.w - self.s(14.0), b.cy(), self.s(3.2));
            p.line(cx - k, cy - k, cx + k, cy + k, c.dim, 1.0, self.s(1.3));
            p.line(cx - k, cy + k, cx + k, cy - k, c.dim, 1.0, self.s(1.3));
        }
    }

    fn field(&self, p: &mut Painter, c: &Ui, b: Box2, hot: bool) {
        p.fill_rrect(b.x, b.y, b.w, b.h, self.s(8.0), c.bg2, 1.0);
        p.stroke_rrect(b.x, b.y, b.w, b.h, self.s(8.0), c.edge, if hot { 0.9 } else { 0.45 }, 1.0);
    }

    fn draw_menus(&self, p: &mut Painter, f: &Fonts, c: &Ui, edit_text: bool) {
        let (cards, add) = self.cards();
        for (k, card) in cards.iter().enumerate() {
            let open = k == self.selected;
            let fb = card.full;
            p.fill_rrect(fb.x, fb.y, fb.w, fb.h, self.s(12.0), blend(c.bg, c.bg2, 0.45), 1.0);
            if open {
                p.stroke_rrect(fb.x, fb.y, fb.w, fb.h, self.s(12.0), c.acc, 0.75, 1.2);
            } else {
                let a = if self.hot(Target::Card(k)) { 0.7 } else { 0.35 };
                p.stroke_rrect(fb.x, fb.y, fb.w, fb.h, self.s(12.0), c.edge, a, 1.0);
            }
            // заголовок: название, чипы вызова справа, стрелка
            let hb = card.header;
            let cy = hb.cy();
            p.text_clipped(&f.bold, &self.menu_name(k), hb.x + self.s(CARD_PAD), cy, self.s(14.0), c.fg, hb.w * 0.45);
            let (button, hotkey) = self.trigger_of(k);
            let mut chips: Vec<(String, u32)> = Vec::new();
            if button != 0 {
                chips.push((Self::button_name(button).to_string(), c.acc));
            }
            if let Some(h) = hotkey {
                chips.push((h.display(), c.acc));
            }
            if chips.is_empty() {
                chips.push(("Не назначен вызов".to_string(), WARN));
            }
            let mut right = hb.x + hb.w - self.s(36.0);
            for (text, col) in chips.iter().rev() {
                let tw = Painter::measure(&f.regular, text, self.s(12.0)) + self.s(20.0);
                let cb = Box2 { x: right - tw, y: cy - self.s(12.0), w: tw, h: self.s(24.0) };
                self.chip(p, f, c, cb, text, *col, false);
                right -= tw + self.s(6.0);
            }
            let (ax, ay, d) = (hb.x + hb.w - self.s(20.0), cy, self.s(4.0));
            if open {
                p.line(ax - d, ay - d / 2.0, ax, ay + d / 2.0, c.dim, 1.0, self.s(1.6));
                p.line(ax, ay + d / 2.0, ax + d, ay - d / 2.0, c.dim, 1.0, self.s(1.6));
            } else {
                p.line(ax - d / 2.0, ay - d, ax + d / 2.0, ay, c.dim, 1.0, self.s(1.6));
                p.line(ax + d / 2.0, ay, ax - d / 2.0, ay + d, c.dim, 1.0, self.s(1.6));
            }
            let Some(body) = &card.body else { continue };
            let lx = hb.x + self.s(CARD_PAD);
            for (label, b) in [("Название", body.name), ("Вызов", body.call), ("Состав", body.file)] {
                p.text(&f.regular, label, lx, b.cy(), self.s(13.0), c.dim, Align::Left);
            }
            // название
            self.field(p, c, body.name, false);
            if edit_text {
                let r = self.name_edit_rect().unwrap_or(body.name);
                p.text_clipped(&f.regular, &self.selected_name(), r.x, r.cy(), self.s(13.0), c.fg, r.w);
            }
            // вызов
            let call = self.call_boxes(body.call);
            if self.recording {
                let a = call.assign;
                p.fill_rrect(a.x, a.y, a.w, a.h, self.s(8.0), c.bg2, 1.0);
                p.stroke_rrect(a.x, a.y, a.w, a.h, self.s(8.0), c.acc, 1.0, 1.5);
                p.fill_circle(a.x + self.s(14.0), a.cy(), self.s(4.0), c.acc, 1.0);
                p.text_clipped(&f.regular, "Нажмите кнопку или сочетание…", a.x + self.s(26.0), a.cy(), self.s(13.0), c.acc, a.w - self.s(34.0));
            } else {
                if let Some(b) = call.button_chip {
                    self.chip(p, f, c, b, Self::button_short(button_of(self, k)), c.acc, true);
                }
                if let Some(b) = call.key_chip {
                    let text = self.current().1.map(|h| h.display()).unwrap_or_default();
                    self.chip(p, f, c, b, &text, c.acc, true);
                }
                self.button(p, f, c, call.assign, "+ Назначить", Target::Assign, false);
            }
            // состав
            self.button(p, f, c, body.file, &format!("Открыть {}", self.file_name()), Target::MenuFile, false);
            self.reset_button(p, f, c, body.reset, Target::ResetMenu, "Сбросить", "Точно сбросить?");
            if let Some(rb) = body.remove {
                let hot = self.hot(Target::RemoveMenu);
                let col = c.bad;
                p.fill_rrect(rb.x, rb.y, rb.w, rb.h, self.s(9.0), if hot { blend(c.bg, col, 0.2) } else { c.bg }, 1.0);
                p.stroke_rrect(rb.x, rb.y, rb.w, rb.h, self.s(9.0), col, 0.5, 1.0);
                p.text(&f.regular, "Удалить меню", rb.x + rb.w / 2.0, rb.cy(), self.s(13.0), col, Align::Center);
            }
        }
        if let Some(a) = add {
            let hot = self.hot(Target::AddMenu);
            p.stroke_rrect(a.x, a.y, a.w, a.h, self.s(12.0), if hot { c.acc } else { c.edge }, 0.7, 1.2);
            p.text(&f.regular, "+  Добавить меню", a.x + a.w / 2.0, a.cy(), self.s(13.0), if hot { c.acc } else { c.fg }, Align::Center);
        }
    }

    fn file_name(&self) -> String {
        match self.menu_id() {
            0 => "menu.yaml".to_string(),
            id => format!("menu-{id}.yaml"),
        }
    }

    fn draw_general(&self, p: &mut Painter, f: &Fonts, c: &Ui, edit_text: bool) {
        let rows = self.general_rows();
        for (name, y) in &rows.groups {
            p.text(&f.regular, &name.to_uppercase(), self.s(LABEL_X), self.s(*y + GROUP_H - 8.0), self.s(11.0), c.dim, Align::Left);
        }
        let label = |p: &mut Painter, text: &str, y: f32| {
            p.text(&f.regular, text, self.s(LABEL_X), self.s(y + ROW / 2.0), self.s(13.5), c.fg, Align::Left);
        };
        // тема
        label(p, "Тема", rows.theme);
        let themes = ["Авто", "Светлая", "Тёмная"];
        let sel = ThemeMode::ALL.iter().position(|&m| m == self.draft.theme).unwrap_or(0);
        self.segments(p, f, c, &self.theme_boxes(), &themes, sel, |i| Target::Theme(ThemeMode::ALL[i]));
        // ползунки
        for (i, s) in Slider::ALL.into_iter().enumerate() {
            label(p, s.label(), rows.sliders[i]);
            self.slider(p, f, c, s);
        }
        // хранилище
        label(p, "Папка хранилища", rows.vault);
        let vb = self.vault_box();
        self.field(p, c, vb, self.hot(Target::Vault));
        if edit_text {
            let r = self.vault_edit_rect().unwrap_or(vb);
            let (text, col) = if self.draft.vault.is_empty() { ("не задана", c.dim) } else { (self.draft.vault.as_str(), c.fg) };
            p.text_clipped(&f.regular, text, r.x, r.cy(), self.s(13.0), col, r.w);
        }
        self.button(p, f, c, self.browse_box(), "Обзор…", Target::Browse, false);
        // автозапуск
        label(p, "Запускать с Windows", rows.auto);
        self.toggle(p, c, self.autostart_box(), self.autostart, Target::Autostart);
        // стандартные значения
        label(p, "Общие настройки", rows.reset);
        self.reset_button(p, f, c, self.reset_general_box(), Target::ResetGeneral, "Сбросить к стандартным", "Точно сбросить?");
    }

    /// Кнопка сброса: первое нажатие просит подтверждения, второе — выполняет.
    #[allow(clippy::too_many_arguments)]
    fn reset_button(&self, p: &mut Painter, f: &Fonts, c: &Ui, b: Box2, t: Target, label: &str, confirm: &str) {
        if self.armed != Some(t) {
            return self.button(p, f, c, b, label, t, false);
        }
        let hot = self.hot(t);
        let col = c.bad;
        p.fill_rrect(b.x, b.y, b.w, b.h, self.s(9.0), if hot { blend(c.bg, col, 0.3) } else { blend(c.bg, col, 0.15) }, 1.0);
        p.stroke_rrect(b.x, b.y, b.w, b.h, self.s(9.0), col, 0.8, 1.2);
        p.text(&f.regular, confirm, b.x + b.w / 2.0, b.cy(), self.s(13.0), col, Align::Center);
    }

    fn toggle(&self, p: &mut Painter, c: &Ui, tb: Box2, on: bool, t: Target) {
        let track = if on { c.acc } else { blend(c.bg2, c.dim, 0.35) };
        let track = if self.hot(t) { blend(track, c.fg, 0.08) } else { track };
        p.fill_rrect(tb.x, tb.y, tb.w, tb.h, tb.h / 2.0, track, 1.0);
        let r = tb.h / 2.0 - self.s(3.0);
        let kx = if on { tb.x + tb.w - tb.h / 2.0 } else { tb.x + tb.h / 2.0 };
        p.fill_circle(kx, tb.cy(), r, 0xFFFFFF, 1.0);
    }

    fn draw_about(&self, p: &mut Painter, f: &Fonts, c: &Ui) {
        let r = self.about_rows();
        // логотип-кольцо, название, версия
        let (lx, ly) = (self.s(PAD + 28.0), self.s(r.hero + 36.0));
        for (i, col) in [c.acc, c.edge, c.acc, c.edge].into_iter().enumerate() {
            let a0 = -135.0_f32 + i as f32 * 90.0 + 12.0;
            let a1 = a0 + 66.0;
            let rad = self.s(22.0);
            let pt = |a: f32| (lx + rad * a.to_radians().cos(), ly + rad * a.to_radians().sin());
            for k in 0..8 {
                let (x0, y0) = pt(a0 + (a1 - a0) * k as f32 / 8.0);
                let (x1, y1) = pt(a0 + (a1 - a0) * (k + 1) as f32 / 8.0);
                p.line(x0, y0, x1, y1, col, 1.0, self.s(6.0));
            }
        }
        p.text(&f.bold, "Радиальное меню", self.s(PAD + 72.0), self.s(r.hero + 26.0), self.s(18.0), c.fg, Align::Left);
        p.text(&f.regular, &format!("Версия {}", self.version), self.s(PAD + 72.0), self.s(r.hero + 50.0), self.s(13.0), c.dim, Align::Left);

        let group = |p: &mut Painter, text: &str, y: f32| {
            p.text(&f.regular, &text.to_uppercase(), self.s(LABEL_X), self.s(y + GROUP_H - 8.0), self.s(11.0), c.dim, Align::Left);
        };
        group(p, "Обновления", r.g1);
        self.draw_update(p, f, c);
        p.text(&f.regular, "Проверять автоматически раз в сутки", self.s(PAD), self.s(r.auto + ROW / 2.0), self.s(13.5), c.fg, Align::Left);
        self.toggle(p, c, self.toggle_box_at(r.auto), self.draft.check_updates, Target::AutoCheck);

        group(p, "Если что-то не работает", r.g2);
        p.text(&f.regular, "Журнал работы", self.s(PAD), self.s(r.log + ROW / 2.0), self.s(13.5), c.fg, Align::Left);
        self.button(p, f, c, self.small_button(r.log), "Открыть", Target::OpenLog, false);
        p.text(&f.regular, "Папка с настройками", self.s(PAD), self.s(r.folder + ROW / 2.0), self.s(13.5), c.fg, Align::Left);
        self.button(p, f, c, self.small_button(r.folder), "Открыть", Target::OpenFolder, false);
    }

    /// Строка обновления: состояние, кнопка или полоса прогресса.
    fn draw_update(&self, p: &mut Painter, f: &Fonts, c: &Ui) {
        let t = self.update_text_box();
        let (top, bottom) = (t.y + t.h * 0.34, t.y + t.h * 0.74);
        let line1 = |p: &mut Painter, text: &str, col: u32| p.text_clipped(&f.regular, text, t.x, top, self.s(13.5), col, t.w);
        let line2 = |p: &mut Painter, text: &str, col: u32| p.text_clipped(&f.regular, text, t.x, bottom, self.s(12.0), col, t.w);
        let bar = |p: &mut Painter, part: f32| {
            let (h, y) = (self.s(6.0), bottom - self.s(3.0));
            p.fill_rrect(t.x, y, t.w, h, h / 2.0, blend(c.bg2, c.dim, 0.35), 1.0);
            if part > 0.0 {
                p.fill_rrect(t.x, y, (t.w * part.min(1.0)).max(h), h, h / 2.0, c.acc, 1.0);
            }
        };
        match &self.update {
            UpdateView::Idle => {
                line1(p, "Обновления", c.fg);
                line2(p, "Ещё не проверялось", c.dim);
            }
            UpdateView::Checking => {
                line1(p, "Обновления", c.fg);
                line2(p, "Проверяю…", c.dim);
            }
            UpdateView::UpToDate => {
                line1(p, "Версия актуальна", c.ok);
                line2(p, "Установлена последняя версия", c.dim);
            }
            UpdateView::Available(v) => {
                line1(p, &format!("Доступна версия {v}"), c.acc);
                line2(p, "Нажмите «Обновить»", c.dim);
            }
            UpdateView::Downloading(pct) => {
                line1(p, &format!("Скачиваю обновление — {pct} %"), c.fg);
                bar(p, *pct as f32 / 100.0);
            }
            UpdateView::Installing => {
                line1(p, "Устанавливаю и перезапускаю…", c.fg);
                bar(p, 1.0);
            }
            UpdateView::Failed { msg, .. } => {
                line1(p, "Не получилось", c.bad);
                line2(p, msg, c.dim);
            }
        }
        if let Some((b, label, primary)) = self.update_button() {
            self.button(p, f, c, b, label, Target::Update, primary);
        }
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
            let px = self.s(13.0);
            let max_w = b.w - self.s(10.0);
            let tw = Painter::measure(&f.regular, label, px).min(max_w);
            p.text_clipped(&f.regular, label, b.x + b.w / 2.0 - tw / 2.0, b.cy(), px, col, max_w);
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
        p.text(&f.regular, &sl.text(v), self.s(W - PAD), cy, self.s(13.0), c.dim, Align::Right);
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

/// Кнопка мыши меню `k` (чип показывается в раскрытой карточке — она всегда текущая).
fn button_of(f: &SettingsForm, k: usize) -> u8 {
    f.trigger_of(k).0
}

#[derive(Default)]
struct GeneralRows {
    groups: Vec<(&'static str, f32)>,
    theme: f32,
    sliders: [f32; 3],
    vault: f32,
    auto: f32,
    reset: f32,
}

struct AboutRows {
    hero: f32,
    g1: f32,
    update: f32,
    auto: f32,
    g2: f32,
    log: f32,
    folder: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> SettingsForm {
        SettingsForm::new(Settings { profiles: vec![], ..Settings::default() }, false, 1.0)
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

    fn go(f: &mut SettingsForm, p: Page) {
        let i = Page::ALL.iter().position(|&x| x == p).unwrap();
        let b = f.page_boxes()[i];
        click(f, b);
        assert_eq!(f.page, p);
    }

    fn header(f: &SettingsForm, k: usize) -> Box2 {
        f.cards().0[k].header
    }

    #[test]
    fn every_control_is_hittable_inside_the_window() {
        let mut f = form();
        f.add_menu();
        f.version = "0.9.0".into();
        let (w, h) = f.size();
        for page in Page::ALL {
            f.page = page;
            for (b, t) in f.items() {
                let (x, y) = center(b);
                assert_eq!(f.hit(x, y), Some(t), "{page:?} {t:?}");
                assert!(b.x >= 0.0 && b.y >= 0.0 && b.x + b.w <= w as f32 && b.y + b.h <= h as f32, "{page:?} {t:?} вне окна");
            }
        }
        assert_eq!(f.hit(10.0, 10.0), Some(Target::Header));
    }

    #[test]
    fn pages_switch_and_show_their_own_controls() {
        let mut f = form();
        assert_eq!(f.page, Page::Menus);
        assert!(f.vault_edit_rect().is_none() && f.name_edit_rect().is_some());
        go(&mut f, Page::General);
        assert!(f.vault_edit_rect().is_some() && f.name_edit_rect().is_none());
        go(&mut f, Page::About);
        assert!(f.vault_edit_rect().is_none() && f.name_edit_rect().is_none());
    }

    #[test]
    fn general_controls_edit_the_draft() {
        let mut f = form();
        go(&mut f, Page::General);
        let b = f.theme_boxes()[1];
        assert_eq!(click(&mut f, b), Cmd::Redraw);
        assert_eq!(f.draft.theme, ThemeMode::Light);
        let b = f.autostart_box();
        click(&mut f, b);
        assert!(f.autostart);
        let b = f.browse_box();
        assert_eq!(click(&mut f, b), Cmd::Browse);
        let b = f.vault_box();
        assert_eq!(click(&mut f, b), Cmd::FocusVault);
    }

    #[test]
    fn slider_drags_snaps_and_clamps() {
        let mut f = form();
        go(&mut f, Page::General);
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
    fn menus_are_added_selected_edited_and_removed() {
        let mut f = form();
        assert_eq!(f.menu_name(0), "Основное 1");
        let add = f.cards().1.expect("«Добавить меню»");
        assert_eq!(click(&mut f, add), Cmd::Redraw);
        assert_eq!(f.draft.profiles, vec![ProfileCfg::new(2, 2)], "берётся свободная боковая кнопка");
        assert_eq!((f.selected, f.menu_name(1).as_str()), (1, "Основное 2"));
        // переименование и выбор карточек
        f.set_name("Работа");
        assert_eq!(f.menu_name(1), "Работа");
        let h0 = header(&f, 0);
        click(&mut f, h0);
        assert_eq!(f.selected, 0);
        f.set_name("Главное");
        assert_eq!(f.draft.main_name, "Главное");
        assert_eq!(f.draft.profiles[0].name, "Работа", "название основного не задело другое");
        // файл меню — у каждой карточки свой
        let file = f.cards().0[0].body.as_ref().unwrap().file;
        assert_eq!(click(&mut f, file), Cmd::OpenMenuFile(0));
        let h1 = header(&f, 1);
        click(&mut f, h1);
        let file = f.cards().0[1].body.as_ref().unwrap().file;
        assert_eq!(click(&mut f, file), Cmd::OpenMenuFile(2));
        // у основного «Удалить» нет, у остальных есть; удаление возвращает к основному
        assert!(f.cards().0[1].body.as_ref().unwrap().remove.is_some());
        let rm = f.cards().0[1].body.as_ref().unwrap().remove.unwrap();
        click(&mut f, rm);
        assert!(f.draft.profiles.is_empty() && f.selected == 0);
        assert!(f.cards().0[0].body.as_ref().unwrap().remove.is_none());
    }

    #[test]
    fn at_most_three_extra_menus_and_default_names() {
        let mut f = form();
        for _ in 0..5 {
            f.add_menu();
        }
        assert_eq!(f.draft.profiles.iter().map(|p| p.id).collect::<Vec<_>>(), vec![2, 3, 4]);
        assert!(f.cards().1.is_none(), "«Добавить меню» больше не нужна");
        assert_eq!(
            (0..4).map(|k| f.menu_name(k)).collect::<Vec<_>>(),
            ["Основное 1", "Основное 2", "Мое меню 1", "Мое меню 2"]
        );
        // всё помещается в тело страницы
        let last = f.cards().0.last().unwrap().full;
        assert!(last.y + last.h <= f.s(BODY_Y + BODY_H));
    }

    #[test]
    fn empty_names_fall_back_to_defaults_on_save() {
        let mut f = form();
        f.add_menu();
        f.draft.profiles[0].button = 2;
        f.set_name("   ");
        f.selected = 0;
        f.set_name("");
        assert_eq!(f.key(Key::Enter), Cmd::Save);
        assert_eq!((f.draft.main_name.as_str(), f.draft.profiles[0].name.as_str()), ("Основное 1", "Основное 2"));
        f.set_name(&"я".repeat(40));
        assert_eq!(f.selected_name().chars().count(), NAME_MAX);
    }

    #[test]
    fn assigning_a_trigger_by_pressing_it() {
        let mut f = form();
        let call = |f: &SettingsForm| f.call_boxes(f.cards().0[f.selected].body.as_ref().unwrap().call);
        // сочетание
        let a = call(&f).assign;
        assert_eq!(click(&mut f, a), Cmd::StartRecording);
        assert!(f.is_recording());
        assert_eq!(f.record(vk::LCONTROL, true, false, false, false), Cmd::None, "одни модификаторы — ждём");
        assert_eq!(f.record(vk::SPACE, true, true, false, false), Cmd::StopRecording);
        assert_eq!(f.draft.hotkey.unwrap().display(), "Ctrl+Alt+Space");
        assert_eq!(f.draft.button, 1, "кнопка осталась");
        // боковая кнопка заменяет прежнюю
        let a = call(&f).assign;
        click(&mut f, a);
        assert_eq!(f.record_button(2), Cmd::StopRecording);
        assert_eq!(f.draft.button, 2);
        assert_eq!(f.record_button(2), Cmd::None, "запись уже закончена");
        // Esc отменяет запись, ничего не меняя
        let a = call(&f).assign;
        click(&mut f, a);
        assert_eq!(f.key(Key::Escape), Cmd::StopRecording);
        assert_eq!((f.draft.button, f.draft.hotkey.is_some()), (2, true));
        assert_eq!(f.key(Key::Escape), Cmd::Close);
        // щелчок мимо во время записи отменяет её и возвращает перехват
        let a = call(&f).assign;
        click(&mut f, a);
        let tab = f.page_boxes()[1];
        assert_eq!(click(&mut f, tab), Cmd::StopRecording);
        assert!(!f.is_recording());
    }

    #[test]
    fn chips_remove_triggers() {
        let mut f = form();
        f.draft.hotkey = Chord::parse("win+down").ok();
        let call = f.call_boxes(f.cards().0[0].body.as_ref().unwrap().call);
        assert!(call.button_chip.is_some() && call.key_chip.is_some());
        click(&mut f, call.clear_key.unwrap());
        assert!(f.draft.hotkey.is_none());
        let call = f.call_boxes(f.cards().0[0].body.as_ref().unwrap().call);
        assert!(call.key_chip.is_none());
        click(&mut f, call.clear_button.unwrap());
        assert_eq!(f.draft.button, 0);
        // без вызова сохранить нельзя
        assert_eq!(f.key(Key::Enter), Cmd::Redraw);
        let st = f.status.clone().unwrap();
        assert!(st.bad && st.text.contains("«Основное 1»") && st.text.contains("назначьте"), "{}", st.text);
    }

    #[test]
    fn a_taken_trigger_is_refused_at_once() {
        let mut f = form();
        f.add_menu(); // «вперёд»
        f.selected = 0;
        f.draft.button = 1;
        let a = f.call_boxes(f.cards().0[0].body.as_ref().unwrap().call).assign;
        click(&mut f, a);
        assert_eq!(f.record_button(2), Cmd::StopRecording);
        assert_eq!(f.draft.button, 1, "кнопку не отдали");
        let st = f.status.clone().unwrap();
        assert!(st.bad && st.text.contains("«Основное 2»"), "{}", st.text);
    }

    #[test]
    fn saving_checks_every_menu_has_its_own_trigger() {
        let mut f = form();
        f.add_menu();
        assert_eq!(f.key(Key::Enter), Cmd::Save);
        f.draft.profiles[0].button = f.draft.button; // обе на одну кнопку
        assert_eq!(f.key(Key::Enter), Cmd::Redraw);
        let st = f.status.clone().unwrap();
        assert!(st.bad && st.text.contains("одна и та же кнопка мыши"), "{}", st.text);
        assert_eq!(f.selected, 1, "показали карточку, где править");
        f.draft.profiles[0].button = 0;
        assert_eq!(f.key(Key::Enter), Cmd::Redraw);
        assert!(f.status.unwrap().text.contains("«Основное 2»: назначьте"));
    }

    #[test]
    fn button_fires_only_if_released_over_it() {
        let mut f = form();
        let (x, y) = center(f.footer_boxes().1);
        f.mouse_down(x, y);
        assert_eq!(f.mouse_up(5.0, 300.0), Cmd::Redraw);
        let (cancel, save) = f.footer_boxes();
        assert_eq!(click(&mut f, save), Cmd::Save);
        assert_eq!(click(&mut f, cancel), Cmd::Close);
    }

    #[test]
    fn update_button_depends_on_state() {
        let mut f = form();
        go(&mut f, Page::About);
        let press = |f: &mut SettingsForm| match f.update_button() {
            Some((b, _, _)) => click(f, b),
            None => Cmd::None,
        };
        assert_eq!(press(&mut f), Cmd::CheckUpdates);
        f.update = UpdateView::UpToDate;
        assert_eq!(press(&mut f), Cmd::CheckUpdates);
        f.update = UpdateView::Available("0.9.0".into());
        assert_eq!(f.update_button().map(|b| (b.1, b.2)), Some(("Обновить", true)));
        assert_eq!(press(&mut f), Cmd::Update);
        f.update = UpdateView::Failed { msg: "нет сети".into(), retry: true };
        assert_eq!(press(&mut f), Cmd::Update);
        f.update = UpdateView::Failed { msg: "нет сети".into(), retry: false };
        assert_eq!(press(&mut f), Cmd::CheckUpdates);
        for busy in [UpdateView::Checking, UpdateView::Downloading(40), UpdateView::Installing] {
            f.update = busy;
            assert!(f.update_button().is_none());
        }
    }

    #[test]
    fn about_page_toggles_and_opens() {
        let mut f = form();
        go(&mut f, Page::About);
        let r = f.about_rows();
        let on = f.draft.check_updates;
        let t = f.toggle_box_at(r.auto);
        click(&mut f, t);
        assert_eq!(f.draft.check_updates, !on);
        let b = f.small_button(r.log);
        assert_eq!(click(&mut f, b), Cmd::OpenLog);
        let b = f.small_button(r.folder);
        assert_eq!(click(&mut f, b), Cmd::OpenFolder);
    }

    #[test]
    fn scales_with_dpi() {
        let a = SettingsForm::new(Settings::default(), false, 1.0).size();
        let b = SettingsForm::new(Settings::default(), false, 1.5).size();
        assert!((b.0 as f32 / a.0 as f32 - 1.5).abs() < 0.01);
        let mut f = SettingsForm::new(Settings::default(), false, 1.5);
        f.page = Page::General;
        let (x, y) = center(f.autostart_box());
        assert_eq!(f.hit(x, y), Some(Target::Autostart));
    }

    #[test]
    fn draws_every_page_in_both_themes() {
        let Some(fonts) = Fonts::load() else { return };
        for theme in Theme::ALL {
            for page in Page::ALL {
                let mut f = SettingsForm::new(Settings::default(), true, 1.0);
                f.page = page;
                f.version = "0.9.0".into();
                f.add_menu();
                f.add_menu();
                f.selected = 1;
                let (w, h) = f.size();
                let mut p = Painter::new(w as u32, h as u32).unwrap();
                f.draw(&mut p, &fonts, theme, true);
                let bg = palette::ui(theme).bg;
                let px = &p.rgba()[(20 * w as usize + w as usize / 2) * 4..][..3];
                assert_eq!(px, [(bg >> 16) as u8, (bg >> 8) as u8, bg as u8], "{theme:?} {page:?}");
                if page == Page::Menus {
                    f.recording = true; // запись вызова тоже рисуется
                    f.draw(&mut p, &fonts, theme, true);
                }
                if page == Page::About {
                    for st in [
                        UpdateView::Checking,
                        UpdateView::Available("0.9.0".into()),
                        UpdateView::Downloading(50),
                        UpdateView::Installing,
                        UpdateView::Failed { msg: "x".into(), retry: true },
                    ] {
                        f.update = st;
                        f.draw(&mut p, &fonts, theme, true);
                    }
                }
            }
        }
    }

    #[test]
    fn menu_reset_needs_a_second_click() {
        let mut f = form();
        let reset = f.cards().0[0].body.as_ref().unwrap().reset;
        assert_eq!(click(&mut f, reset), Cmd::Redraw, "первое нажатие только просит подтверждения");
        assert_eq!(f.armed, Some(Target::ResetMenu));
        assert_eq!(click(&mut f, reset), Cmd::ResetMenuFile(0));
        assert_eq!(f.armed, None, "после сброса подтверждение снято");
    }

    #[test]
    fn any_other_click_cancels_the_reset() {
        let mut f = form();
        let reset = f.cards().0[0].body.as_ref().unwrap().reset;
        click(&mut f, reset);
        assert_eq!(f.armed, Some(Target::ResetMenu));
        let tab = f.page_boxes()[1];
        click(&mut f, tab); // ушли на другую страницу
        assert_eq!(f.armed, None);
        f.page = Page::Menus;
        let reset = f.cards().0[0].body.as_ref().unwrap().reset;
        assert_eq!(click(&mut f, reset), Cmd::Redraw, "после отмены снова нужно два нажатия");
    }

    #[test]
    fn general_reset_restores_the_draft_to_defaults() {
        let mut f = form();
        f.page = Page::General;
        f.draft.size_percent = 130;
        f.draft.theme = ThemeMode::Dark;
        f.draft.ignore_fullscreen = false;
        f.draft.vault = "D:/V".into();
        let b = f.reset_general_box();
        assert_eq!(click(&mut f, b), Cmd::Redraw);
        assert_eq!(f.draft.size_percent, 130, "до подтверждения ничего не меняется");
        assert_eq!(click(&mut f, b), Cmd::ResetGeneral);
        let d = Settings::default();
        assert_eq!((f.draft.size_percent, f.draft.theme, f.draft.ignore_fullscreen), (d.size_percent, d.theme, true));
        assert_eq!(f.draft.vault, "D:/V", "папка заметок — выбор человека, сброс её не трогает");
    }

    #[test]
    fn reset_button_is_in_every_open_card_and_inside_the_window() {
        let mut f = SettingsForm::new(Settings::default(), false, 1.0);
        for k in 0..f.n_menus() {
            f.selected = k;
            let (cards, _) = f.cards();
            let body = cards[k].body.as_ref().unwrap();
            assert!(body.reset.w > 60.0, "кнопка читается: {}", body.reset.w);
            assert!(body.reset.x >= body.file.x + body.file.w, "не налезает на «Открыть»");
            let (_, h) = f.size();
            assert!(cards[k].full.y + cards[k].full.h < h as f32, "карточка помещается в окно");
        }
    }
}

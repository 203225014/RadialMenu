//! Слой логики меню: состояние, навигация, анимация и кадр. Не зависит от платформы —
//! платформа подаёт нажатия/сдвиги мыши/время и отдаёт буфер для рисования.

pub mod anim;
pub mod geom;
pub mod layout;
pub mod metrics;
pub mod nav;

use crate::model::{Action, Context, IconRef, Menu, NodeId, Settings, Theme, ROOT};
use crate::render::compose::{Canvas, Rect, SectorShape};
use crate::render::palette as style;
use crate::render::painter::Painter;
use crate::render::raster::{self, Bake, PillKind, ProgramIcons, Rgba, Track, BOX, DIAM, DOT_BOX};
use anim::{Anim, Scene};
use geom::ease_back;
use layout::Layout;
use metrics::{Metrics, FADE_IN, FADE_OUT, HOVER_GROW, STUCK_T, TOAST_T};
use nav::Nav;

/// Сторона иконки относительно половины толщины кнопки.
const ICON_K: f32 = 1.0;

pub const TOAST_UP: &str = "Громкость +";
pub const TOAST_DOWN: &str = "Громкость −";

/// Итог кадра для платформы.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// Что изменилось в буфере (для UpdateLayeredWindow).
    pub dirty: Rect,
    /// Общая прозрачность окна 0..255.
    pub alpha: u8,
    /// `false` — меню погасло, окно можно прятать.
    pub visible: bool,
}

pub struct Engine {
    settings: Settings,
    theme: Theme,
    ui_scale: f32,
    screen: (i32, i32),
    menu: Menu,
    metrics: Metrics,
    layout: Layout,
    bake: Bake,
    nav: Nav,
    anim: Anim,
    active: bool,
    fading: bool,
    show_t: f64,
    fade_t: f64,
    t_prev: f64,
    toast: Option<(&'static str, f64)>,
    prev_dirty: Rect,
    /// Шрифт для подписи, которая известна только во время работы (что сейчас играет).
    font: Option<fontdue::Font>,
    pill_scale: f32,
    track: Option<Track>,
    track_cards: Vec<(Theme, Rgba)>,
    /// Прозрачность подписи трека 0..1 (плавно появляется, пока открыта кнопка плеера).
    track_a: f32,
}

impl Engine {
    pub fn new(
        settings: Settings,
        ui_scale: f32,
        screen: (i32, i32),
        mut menu: Menu,
        font: Option<&fontdue::Font>,
        theme: Theme,
        programs: ProgramIcons,
    ) -> Engine {
        let settings_scale = settings.size_percent as f32 / 100.0;
        let metrics = Metrics::new(&settings, ui_scale, screen, &menu);
        let layout = Layout::new(&menu, &metrics);
        let mut texts: Vec<(String, PillKind)> = menu
            .label_variants()
            .into_iter()
            .map(|l| (l, PillKind::Label))
            .collect();
        texts.push((TOAST_UP.into(), PillKind::Toast));
        texts.push((TOAST_DOWN.into(), PillKind::Toast));
        let mut bake = Bake::build(
            &menu.icons(),
            &texts,
            ui_scale * settings.size_percent as f32 / 100.0,
            font,
            programs,
        );
        // иконку программы или файла не достали (нет файла, нет иконки) — первая буква подписи
        let mut letters = Vec::new();
        for node in menu.nodes.iter_mut().skip(1) {
            if bake.icon(&node.icon).is_none() && matches!(node.icon, IconRef::Program(_) | IconRef::File { .. }) {
                node.icon = IconRef::letter_for(&node.label);
                letters.push(node.icon.clone());
            }
        }
        bake.add_icons(&letters, font);
        let anim = Anim::new(&menu);
        Engine {
            settings,
            theme,
            ui_scale,
            screen,
            menu,
            metrics,
            layout,
            bake,
            nav: Nav::default(),
            anim,
            active: false,
            fading: false,
            show_t: 0.0,
            fade_t: 0.0,
            t_prev: 0.0,
            toast: None,
            prev_dirty: Rect::EMPTY,
            font: font.cloned(),
            pill_scale: ui_scale * settings_scale,
            track: None,
            track_cards: Vec::new(),
            track_a: 0.0,
        }
    }

    /// Что сейчас играет; `None` — ничего. Карточка показывается, пока открыта кнопка с управлением медиа.
    pub fn set_track(&mut self, track: Option<&Track>) {
        if track == self.track.as_ref() {
            return;
        }
        self.track = track.cloned();
        self.track_cards.clear();
        let (Some(font), Some(t)) = (&self.font, &self.track) else {
            return;
        };
        let s = self.pill_scale;
        let max_w = (raster::CARD_W - raster::CARD_PAD_L - raster::CARD_PAD_R) * s;
        let fit = Track {
            title: fit_text(font, &t.title, 13.5 * s, max_w),
            artist: fit_text(font, &t.artist, 12.0 * s, max_w),
            paused: t.paused,
        };
        self.track_cards = Theme::ALL.iter().map(|&th| (th, raster::track_card(font, &fit, th, s))).collect();
    }

    /// Угол кнопки плеера на основном кольце (градусы, ось y вниз).
    fn media_theta(&self) -> Option<f32> {
        let id = self.menu.top().iter().copied().find(|&i| self.menu.is_media_group(i))?;
        Some(self.layout.slot(id).arc.theta)
    }

    pub fn menu(&self) -> &Menu {
        &self.menu
    }

    pub fn theme(&self) -> Theme {
        self.theme
    }

    /// Сменить тему: подписи уже нарисованы для обеих, поэтому это мгновенно.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    /// Сторона квадратного окна под меню, px.
    pub fn window_size(&self) -> i32 {
        2 * Metrics::window_half(&self.settings, self.ui_scale, &self.menu)
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Нужны ли кадры (меню на экране или гаснет).
    pub fn is_busy(&self) -> bool {
        self.active || self.fading
    }

    /// Предохранитель: «отпускание» могло потеряться — меню не должно висеть вечно.
    pub fn timed_out(&self, now: f64) -> bool {
        self.active && now - self.show_t > STUCK_T
    }

    /// Экран, на котором показывается меню, сменился: пересчитать размеры (только вне показа).
    pub fn set_screen(&mut self, screen: (i32, i32)) {
        if screen != self.screen && !self.is_busy() {
            self.screen = screen;
            self.metrics = Metrics::new(&self.settings, self.ui_scale, screen, &self.menu);
            self.layout = Layout::new(&self.menu, &self.metrics);
        }
    }

    pub fn press(&mut self, now: f64) {
        self.nav.reset();
        self.anim.reset(&self.menu);
        self.toast = None;
        self.active = true;
        self.fading = false;
        self.show_t = now;
        self.t_prev = now;
        self.prev_dirty = Rect::EMPTY;
    }

    /// Кнопку отпустили: меню гаснет, возвращается действие выбранного пункта.
    pub fn release(&mut self, now: f64) -> Option<Action> {
        if !self.active {
            return None;
        }
        let action = self.nav.hot.and_then(|id| self.menu.node(id).action.clone());
        self.active = false;
        self.fading = true;
        self.fade_t = now;
        action
    }

    /// Колесо мыши при открытом меню — громкость.
    pub fn wheel(&mut self, up: bool, now: f64) -> Option<Action> {
        if !self.active {
            return None;
        }
        self.toast = Some((if up { TOAST_UP } else { TOAST_DOWN }, now + TOAST_T));
        Some(Action::Volume(up))
    }

    /// Один кадр: сдвиг указателя → выбор → анимация → отрисовка в `cv`.
    pub fn frame(&mut self, now: f64, delta: (f32, f32), ctx: &Context, cv: &mut Canvas) -> Frame {
        if !self.is_busy() {
            return Frame {
                dirty: Rect::EMPTY,
                alpha: 0,
                visible: false,
            };
        }
        if self.active {
            self.nav
                .update(&self.menu, &self.layout, &self.metrics, delta, now);
        }
        let dt = (now - self.t_prev).clamp(0.0, 0.05);
        self.t_prev = now;
        self.anim.step(
            dt,
            &Scene {
                menu: &self.menu,
                ctx,
                active: self.active,
                open: self.nav.open,
                hot: self.nav.hot,
            },
        );
        let media_open = self
            .nav
            .hot
            .or(self.nav.open)
            .is_some_and(|id| self.menu.is_media_group(id) || self.menu.node(id).parent.is_some_and(|p| p != ROOT && self.menu.is_media_group(p)));
        let want = if self.active && media_open && !self.track_cards.is_empty() { 1.0 } else { 0.0 };
        self.track_a += (want - self.track_a) * (dt as f32 / 0.12).min(1.0);
        let g = if self.active {
            ((now - self.show_t) / FADE_IN).min(1.0)
        } else {
            (1.0 - (now - self.fade_t) / FADE_OUT).max(0.0)
        };
        cv.clear(self.prev_dirty);
        let drawn = self.draw(cv, ctx, now);
        let dirty = self.prev_dirty.union(drawn);
        self.prev_dirty = drawn;
        let mut visible = true;
        if !self.active && g <= 0.0 {
            self.fading = false;
            cv.clear(drawn);
            self.prev_dirty = Rect::EMPTY;
            visible = false;
        }
        Frame {
            dirty,
            alpha: (255.0 * g).round() as u8,
            visible,
        }
    }

    fn draw(&self, cv: &mut Canvas, ctx: &Context, now: f64) -> Rect {
        let c = (cv.w as f32 / 2.0, cv.h as f32 / 2.0);
        let (m, bake, s) = (&self.metrics, &self.bake, self.ui_scale);
        let to_box = BOX / (DIAM / 2.0); // радиус круга → сторона спрайта
        let mut dirty = Rect::EMPTY;

        // центр: маленькая серая кнопка и синяя точка — куда сейчас смотрит мышь
        let hp = ease_back(self.anim.hub_p);
        let hub_size = to_box * m.hub_r * hp;
        if hub_size >= 6.0 {
            let a = (self.anim.hub_p * 2.5).min(1.0);
            let look = style::mix(
                &self
                    .anim
                    .hub_w
                    .unwrap_or_else(|| style::one_hot(style::DEFAULT)),
                self.theme,
                None,
            );
            dirty = dirty.union(cv.button(&bake.button, None, c.0, c.1, hub_size, &look, a));
            let (mut dx, mut dy) = (self.nav.px, self.nav.py);
            let len = dx.hypot(dy);
            let reach = m.hub_r * 0.45;
            if len > reach {
                dx *= reach / len;
                dy *= reach / len;
            }
            let ds = DOT_BOX * (m.hub_r / 17.0) * hp;
            dirty = dirty.union(cv.image_scaled(&bake.dot, c.0 + dx, c.1 + dy, ds, a));
        }

        // кнопки
        let shown: Vec<NodeId> = self
            .menu
            .ids()
            .filter(|&i| self.anim.nodes[i].p > 0.01)
            .collect();
        for &id in &shown {
            let a = &self.anim.nodes[id];
            let slot = self.layout.slot(id);
            // появление: сектор «вырастает» из кольца родителя (основные — из центра) и толстеет
            let e = ease_back(a.p).max(0.0);
            let from = match self.menu.node(id).parent {
                Some(ROOT) | None => slot.arc.r * 0.55,
                Some(p) => self.layout.slot(p).arc.r,
            };
            let r = from + (slot.arc.r - from) * e;
            let h = (m.h * (1.0 + HOVER_GROW * a.hv) * e).max(0.0);
            let look = style::mix(
                &a.w.unwrap_or_else(|| style::one_hot(style::DEFAULT)),
                self.theme,
                self.menu.node(id).color,
            );
            let shape = SectorShape {
                cx: c.0,
                cy: c.1,
                r,
                a0: (slot.arc.theta - slot.arc.span).to_radians(),
                a1: (slot.arc.theta + slot.arc.span).to_radians(),
                h,
            };
            let t = slot.arc.theta.to_radians();
            let icon_c = (c.0 + r * t.cos(), c.1 + r * t.sin());
            let icon = bake.icon(self.menu.icon(id, ctx));
            dirty = dirty.union(cv.sector(&shape, icon, icon_c, h * ICON_K, &look, (a.p * 2.5).clamp(0.0, 1.0)));
        }

        // подпись выбранной кнопки — со стороны центра, в промежутке между кнопками
        for &id in &shown {
            let a = &self.anim.nodes[id];
            if a.hv <= 0.15 {
                continue;
            }
            let Some(pill) = bake.pill(&self.menu.label(id, ctx), PillKind::Label, self.theme) else {
                continue;
            };
            // подпись — в пустом поле между центром и первым кольцом, в сторону выбранной кнопки
            let slot = self.layout.slot(id);
            let t = slot.arc.theta.to_radians();
            let rho = (m.hub_r + m.ring(0) - m.h) / 2.0;
            let (x, y) = (c.0 + rho * t.cos(), c.1 + rho * t.sin());
            let py = y - pill.h as f32 / 2.0;
            dirty = dirty.union(put_pill(
                cv,
                pill,
                x - pill.w as f32 / 2.0,
                py,
                (a.hv * 1.5).min(1.0),
            ));
        }

        // подпись при смене громкости — под центром, пока не погаснет
        if let Some((text, until)) = self.toast {
            let left = (until - now) as f32;
            if left > 0.0 {
                if let Some(pill) = bake.pill(text, PillKind::Toast, self.theme) {
                    let y = c.1 + m.hub_r + 14.0 * s;
                    dirty = dirty.union(put_pill(
                        cv,
                        pill,
                        c.0 - pill.w as f32 / 2.0,
                        y,
                        (left / 0.3).min(1.0),
                    ));
                }
            }
        }
        // что сейчас играет — карточка снаружи основного кольца, на стороне, противоположной кнопке плеера:
        // веер раскрыт у самой кнопки, поэтому здесь он карточке не мешает и она стоит на месте
        if self.track_a > 0.02 {
            if let Some((_, card)) = self.track_cards.iter().find(|(t, _)| *t == self.theme) {
                let up = self.media_theta().is_some_and(|t| t.to_radians().sin() > 0.0);
                let edge = m.ring(0) + m.h + 14.0 * s;
                let y = if up { c.1 - edge - card.h as f32 } else { c.1 + edge };
                dirty = dirty.union(put_pill(cv, card, c.0 - card.w as f32 / 2.0, y, self.track_a.min(1.0)));
            }
        }
        dirty
    }
}

/// Обрезать строку многоточием, чтобы она влезла в `max_w` px.
fn fit_text(font: &fontdue::Font, text: &str, px: f32, max_w: f32) -> String {
    if Painter::measure(font, text, px) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    for keep in (1..chars.len()).rev() {
        let s: String = chars[..keep].iter().collect::<String>().trim_end().to_string() + "…";
        if Painter::measure(font, &s, px) <= max_w {
            return s;
        }
    }
    "…".to_string()
}

/// Положить плашку, не выпуская её за край холста.
fn put_pill(cv: &mut Canvas, pill: &raster::Rgba, x: f32, y: f32, alpha: f32) -> Rect {
    let px = x.clamp(2.0, (cv.w as f32 - pill.w as f32 - 2.0).max(2.0));
    let py = y.clamp(2.0, (cv.h as f32 - pill.h as f32 - 2.0).max(2.0));
    cv.image(pill, px as i32, py as i32, alpha)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::default_menu;

    fn engine() -> Engine {
        let font = raster::load_font();
        Engine::new(
            Settings::default(),
            1.0,
            (1920, 1080),
            default_menu(),
            font.as_ref(),
            Theme::Dark,
            &raster::no_program_icons,
        )
    }

    fn run(e: &mut Engine, cv: &mut Canvas, t0: f64, secs: f64, delta: (f32, f32)) -> (f64, Frame) {
        let ctx = Context::default();
        let mut t = t0;
        let mut f = Frame {
            dirty: Rect::EMPTY,
            alpha: 0,
            visible: true,
        };
        while t < t0 + secs {
            t += 0.008;
            f = e.frame(t, delta, &ctx, cv);
        }
        (t, f)
    }

    #[test]
    fn idle_engine_does_nothing() {
        let mut e = engine();
        let mut buf = vec![0u8; 16];
        let mut cv = Canvas {
            w: 2,
            h: 2,
            px: &mut buf,
        };
        let f = e.frame(1.0, (0.0, 0.0), &Context::default(), &mut cv);
        assert!(!f.visible && f.dirty.is_empty() && !e.is_busy());
    }

    #[test]
    fn full_cycle_selects_and_returns_action() {
        let mut e = engine();
        let n = e.window_size() as usize;
        let mut buf = vec![0u8; n * n * 4];
        let mut cv = Canvas::new(n, n, &mut buf);
        e.press(0.0);
        let (t, f) = run(&mut e, &mut cv, 0.0, 0.5, (0.0, 0.0));
        assert!(f.visible && f.alpha == 255 && !f.dirty.is_empty());
        // двигаем вправо — «Вставить» (Ctrl+V)
        let (t, _) = run(&mut e, &mut cv, t, 0.4, (10.0, 0.0));
        let act = e.release(t);
        assert_eq!(act, Some(Action::paste()));
        // гаснет и перестаёт быть занятым
        let (_, f) = run(&mut e, &mut cv, t, 0.5, (0.0, 0.0));
        assert!(!f.visible && !e.is_busy());
        assert!(
            buf.iter().all(|&b| b == 0),
            "после гашения холст должен быть пуст"
        );
    }

    #[test]
    fn release_in_center_cancels() {
        let mut e = engine();
        let n = e.window_size() as usize;
        let mut buf = vec![0u8; n * n * 4];
        let mut cv = Canvas::new(n, n, &mut buf);
        e.press(0.0);
        let (t, _) = run(&mut e, &mut cv, 0.0, 0.3, (0.0, 0.0));
        assert_eq!(e.release(t), None);
    }

    #[test]
    fn wheel_only_while_active_and_shows_toast() {
        let mut e = engine();
        assert_eq!(e.wheel(true, 0.0), None);
        e.press(0.0);
        assert_eq!(e.wheel(false, 0.1), Some(Action::Volume(false)));
        assert!(e.toast.is_some());
    }

    #[test]
    fn theme_switch_recolours_without_rebuild() {
        let mut e = engine();
        let n = e.window_size() as usize;
        let shot = |e: &mut Engine| {
            let mut buf = vec![0u8; n * n * 4];
            let mut cv = Canvas::new(n, n, &mut buf);
            e.press(0.0);
            run(e, &mut cv, 0.0, 0.5, (0.0, 0.0));
            e.release(0.5);
            run(e, &mut cv, 0.5, 0.5, (0.0, 0.0));
            e.press(1.0);
            run(e, &mut cv, 1.0, 0.5, (10.0, 0.0));
            buf
        };
        let dark = shot(&mut e);
        e.set_theme(Theme::Light);
        assert_eq!(e.theme(), Theme::Light);
        let light = shot(&mut e);
        // светлое стекло ярче тёмного в среднем по кадру
        let avg = |b: &[u8]| b.chunks_exact(4).map(|p| p[1] as u64).sum::<u64>();
        assert!(avg(&light) > avg(&dark));
    }

    #[test]
    fn missing_program_icon_becomes_a_letter() {
        use crate::model::{Item, Run};
        let menu = Menu::from_items(vec![
            Item::new("", "Блокнот")
                .icon(IconRef::Program("notepad".into()))
                .action(Action::Run(Run { path: "notepad".into(), ..Run::default() })),
            Item::new("gear", "Б").action(Action::Note),
        ]);
        let font = raster::load_font();
        let e = Engine::new(Settings::default(), 1.0, (1920, 1080), menu, font.as_ref(), Theme::Dark, &raster::no_program_icons);
        let first = e.menu().top()[0];
        assert_eq!(e.menu().node(first).icon, IconRef::Letter('Б'));
        if font.is_some() {
            assert!(e.bake.icon(&IconRef::Letter('Б')).is_some());
        }
    }

    #[test]
    fn big_menu_selects_every_direction() {
        use crate::model::Item;
        let menu = Menu::from_items(
            (0..12).map(|i| Item::new("gear", &format!("К{i}")).action(Action::Note)).collect(),
        );
        let mut e = Engine::new(Settings::default(), 1.0, (1920, 1080), menu, None, Theme::Dark, &raster::no_program_icons);
        let n = e.window_size() as usize;
        let mut buf = vec![0u8; n * n * 4];
        let mut cv = Canvas::new(n, n, &mut buf);
        for i in 0..12 {
            let a = (-90.0f32 + 30.0 * i as f32).to_radians();
            e.press(0.0);
            run(&mut e, &mut cv, 0.0, 0.3, (a.cos() * 8.0, a.sin() * 8.0));
            assert_eq!(e.nav.hot, Some(e.menu().top()[i]), "направление {i}");
            e.release(0.3);
            run(&mut e, &mut cv, 0.3, 0.4, (0.0, 0.0));
        }
    }

    #[test]
    fn media_group_is_found_in_the_default_menu() {
        let menu = default_menu();
        let media: Vec<_> = menu.ids().filter(|&i| menu.is_media_group(i)).collect();
        assert!(!media.is_empty());
        assert!(media.iter().all(|&i| menu.node(i).parent == Some(ROOT) || menu.is_media_group(menu.node(i).parent.unwrap())));
        // «Открыть» к медиа не относится
        assert!(!menu.is_media_group(menu.top()[0]));
    }

    #[test]
    fn long_title_is_cut_with_an_ellipsis() {
        let Some(font) = raster::load_font() else { return };
        let long = "Очень длинное название трека — Исполнитель с длинным именем и ещё немного текста";
        let cut = fit_text(&font, long, 13.0, 200.0);
        assert!(cut.ends_with('…') && Painter::measure(&font, &cut, 13.0) <= 200.0, "{cut}");
        assert_eq!(fit_text(&font, "Короткий", 13.0, 200.0), "Короткий");
    }

    #[test]
    fn track_card_stays_put_and_is_shown_only_while_the_media_button_is_open() {
        let Some(font) = raster::load_font() else { return };
        let menu = default_menu();
        let media = menu.ids().find(|&i| menu.node(i).state == Some(crate::model::Flag::Media)).expect("кнопка «Звук»");
        let dir = {
            let e = Engine::new(Settings::default(), 1.0, (1920, 1080), menu.clone(), Some(&font), Theme::Dark, &raster::no_program_icons);
            e.layout.slot(media).off
        };
        let track = Track { title: "Песня".into(), artist: "Группа".into(), paused: false };
        let shot = |track: Option<&Track>, toward: (f32, f32)| {
            let mut e = Engine::new(Settings::default(), 1.0, (1920, 1080), menu.clone(), Some(&font), Theme::Dark, &raster::no_program_icons);
            e.set_track(track);
            let n = e.window_size() as usize;
            let mut buf = vec![0u8; n * n * 4];
            let mut cv = Canvas::new(n, n, &mut buf);
            e.press(0.0);
            let len = toward.0.hypot(toward.1).max(1.0);
            run(&mut e, &mut cv, 0.0, 0.8, (toward.0 / len * 6.0, toward.1 / len * 6.0));
            // карточка — снаружи основного кольца, со стороны, противоположной кнопке плеера
            let m = e.metrics;
            let (cx, cy) = (n as f32 / 2.0, n as f32 / 2.0);
            let up = e.media_theta().is_some_and(|t| t.to_radians().sin() > 0.0);
            let edge = m.ring(0) + m.h + 14.0;
            let (y0, y1) = if up { (cy - edge - 54.0, cy - edge) } else { (cy + edge, cy + edge + 54.0) };
            (y0 as usize..y1 as usize)
                .flat_map(|y| (cx as usize - 100..cx as usize + 100).map(move |x| (x, y)))
                .filter(|&(x, y)| buf[(y * n + x) * 4 + 3] > 120)
                .count()
        };
        let with = shot(Some(&track), dir);
        let without = shot(None, dir);
        let elsewhere = shot(Some(&track), (0.0, -1.0)); // тянемся к «Открыть»
        assert!(with > 200, "подпись трека нарисована: {with}");
        assert_eq!(without, 0, "нечего играть — подписи нет");
        assert_eq!(elsewhere, shot(None, (0.0, -1.0)), "у другой кнопки карточки трека нет");
    }

    #[test]
    fn stuck_guard() {
        let mut e = engine();
        e.press(0.0);
        assert!(!e.timed_out(5.0) && e.timed_out(STUCK_T + 1.0));
    }

    #[test]
    fn dirty_tracks_motion_and_stays_in_bounds() {
        let mut e = engine();
        let n = e.window_size() as usize;
        let mut buf = vec![0u8; n * n * 4];
        let mut cv = Canvas::new(n, n, &mut buf);
        e.press(0.0);
        let (_, f) = run(&mut e, &mut cv, 0.0, 0.6, (3.0, -3.0));
        let b = cv.bounds();
        assert!(
            f.dirty.x0 >= b.x0 && f.dirty.y0 >= b.y0 && f.dirty.x1 <= b.x1 && f.dirty.y1 <= b.y1
        );
    }
}

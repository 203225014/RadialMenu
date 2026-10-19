//! Дерево меню. Данные плоские (арена), поведение — данные, а не замыкания:
//! меню описывается файлом (см. `menu_yaml`), проверяется и строится при запуске.

use super::action::{Action, Chord, Media};
use super::icon::IconRef;

pub type NodeId = usize;

/// Корень дерева (невидимый, держит основные кнопки).
pub const ROOT: NodeId = 0;

/// Сколько основных кнопок и пунктов веера помещается в раскладку.
pub const MAX_TOP: usize = 12;
pub const MAX_KIDS: usize = 6;
/// Пунктов во вложенном веере (веер пункта веера) — их положение ограничено соседями.
pub const MAX_SUB: usize = 4;
/// Уровней: основные кнопки, веер, вложенный веер.
pub const MAX_DEPTH: usize = 3;

/// Источник состояния «включено» из внешнего мира. Новый источник — вариант здесь
/// и его опрос в платформе; сами кнопки описываются в файле меню.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Flag {
    /// Звучит аудио.
    Media,
    /// Есть рабочий стол левее текущего.
    DesktopPrev,
    /// Есть рабочий стол правее текущего.
    DesktopNext,
}

impl Flag {
    pub const ALL: [Flag; 3] = [Flag::Media, Flag::DesktopPrev, Flag::DesktopNext];

    fn bit(self) -> u32 {
        1 << self as u32
    }

    pub fn key(self) -> &'static str {
        match self {
            Flag::Media => "media",
            Flag::DesktopPrev => "desktop-prev",
            Flag::DesktopNext => "desktop-next",
        }
    }

    pub fn from_key(s: &str) -> Option<Flag> {
        Flag::ALL.into_iter().find(|f| f.key() == s)
    }

    /// Приписка к подписи кнопки-индикатора.
    pub fn suffix(self, on: bool) -> &'static str {
        match (self, on) {
            (Flag::Media, true) => "играет",
            (Flag::Media, false) => "выкл",
            (Flag::DesktopPrev | Flag::DesktopNext, _) => "",
        }
    }
}

/// Что известно меню о внешнем мире на момент кадра.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Context {
    flags: u32,
}

impl Context {
    pub fn with(mut self, flag: Flag, on: bool) -> Context {
        self.set(flag, on);
        self
    }

    pub fn set(&mut self, flag: Flag, on: bool) {
        if on {
            self.flags |= flag.bit();
        } else {
            self.flags &= !flag.bit();
        }
    }

    pub fn get(&self, flag: Flag) -> bool {
        self.flags & flag.bit() != 0
    }

    /// Все сочетания флагов — чтобы заранее нарисовать все подписи и иконки.
    pub fn all() -> impl Iterator<Item = Context> {
        (0..1u32 << Flag::ALL.len()).map(|flags| Context { flags })
    }
}

/// Другая иконка и подпись, пока флаг выключен («Пауза» ↔ «Плей»).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variant {
    pub flag: Flag,
    pub icon: IconRef,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub icon: IconRef,
    pub label: String,
    pub action: Option<Action>,
    /// Подменю раскрывается, только если задержаться на кнопке.
    pub dwell: bool,
    /// Кнопка-индикатор: зелёная и с припиской, пока флаг включён.
    pub state: Option<Flag>,
    /// Иконка/подпись на время, пока флаг выключен.
    pub off: Option<Variant>,
    /// Свой цвет подсветки (0xRRGGBB) вместо синего.
    pub color: Option<u32>,
    /// Кнопка доступна, только пока флаг включён; иначе она приглушена и не выбирается.
    pub enabled_if: Option<Flag>,
    pub children: Vec<NodeId>,
    pub parent: Option<NodeId>,
}

/// Расположение кнопок по кругу.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    /// Угол первой основной кнопки, ° (ось Y вниз: −90 — сверху, 0 — справа).
    pub start_angle: f32,
    /// Остальные — по часовой стрелке.
    pub clockwise: bool,
    /// Устарело: веер раскладывается кольцом сам. Поле читается из старых файлов и не используется.
    pub fan_spread: f32,
}

impl Default for Shape {
    fn default() -> Self {
        Shape {
            start_angle: -90.0,
            clockwise: true,
            fan_spread: 60.0,
        }
    }
}

/// Описание пункта для сборки дерева.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub node: Node,
    pub kids: Vec<Item>,
}

impl Item {
    pub fn new(icon: &str, label: &str) -> Item {
        Item {
            node: Node {
                icon: IconRef::builtin(icon),
                label: label.into(),
                action: None,
                dwell: false,
                state: None,
                off: None,
                color: None,
                enabled_if: None,
                children: Vec::new(),
                parent: None,
            },
            kids: Vec::new(),
        }
    }
    pub fn icon(mut self, icon: IconRef) -> Item {
        self.node.icon = icon;
        self
    }
    pub fn action(mut self, a: Action) -> Item {
        self.node.action = Some(a);
        self
    }
    pub fn dwell(mut self) -> Item {
        self.node.dwell = true;
        self
    }
    pub fn state(mut self, f: Flag) -> Item {
        self.node.state = Some(f);
        self
    }
    pub fn enabled_if(mut self, f: Flag) -> Item {
        self.node.enabled_if = Some(f);
        self
    }
    pub fn color(mut self, rgb: u32) -> Item {
        self.node.color = Some(rgb);
        self
    }
    pub fn off(mut self, flag: Flag, icon: &str, label: &str) -> Item {
        self.node.off = Some(Variant {
            flag,
            icon: IconRef::builtin(icon),
            label: label.into(),
        });
        self
    }
    pub fn children(mut self, kids: Vec<Item>) -> Item {
        self.kids = kids;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub nodes: Vec<Node>,
    pub shape: Shape,
}

impl Menu {
    pub fn from_items(items: Vec<Item>) -> Menu {
        let mut menu = Menu {
            nodes: Vec::new(),
            shape: Shape::default(),
        };
        let root = Item::new("", "").children(items);
        menu.add(root, None);
        menu
    }

    pub fn with_shape(mut self, shape: Shape) -> Menu {
        self.shape = shape;
        self
    }

    fn add(&mut self, item: Item, parent: Option<NodeId>) -> NodeId {
        let id = self.nodes.len();
        let mut node = item.node;
        node.parent = parent;
        node.children.clear();
        self.nodes.push(node);
        for kid in item.kids {
            let k = self.add(kid, Some(id));
            self.nodes[id].children.push(k);
        }
        id
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    pub fn children(&self, id: NodeId) -> &[NodeId] {
        &self.nodes[id].children
    }

    /// Основные кнопки (дети корня).
    pub fn top(&self) -> &[NodeId] {
        self.children(ROOT)
    }

    /// Самый большой веер основной кнопки.
    pub fn max_fan(&self) -> usize {
        self.top().iter().map(|&t| self.children(t).len()).max().unwrap_or(0)
    }

    /// Самый большой вложенный веер (у пункта веера).
    pub fn max_sub(&self) -> usize {
        self.top()
            .iter()
            .flat_map(|&t| self.children(t))
            .map(|&k| self.children(k).len())
            .max()
            .unwrap_or(0)
    }

    /// Число уровней: 1 — только основные кнопки, 2 — есть веера, 3 — есть вложенные.
    pub fn depth(&self) -> usize {
        fn go(m: &Menu, id: NodeId) -> usize {
            1 + m.children(id).iter().map(|&k| go(m, k)).max().unwrap_or(0)
        }
        go(self, ROOT) - 1
    }

    /// Кнопка управляет медиа: показывает «играет» или в её веере есть клавиши плеера.
    pub fn is_media_group(&self, id: NodeId) -> bool {
        let n = &self.nodes[id];
        n.state == Some(Flag::Media)
            || n.children.iter().any(|&k| matches!(self.nodes[k].action, Some(Action::Media(_))))
    }

    /// Номер кольца кнопки: 0 — основные, 1 — веер, 2 — вложенный веер.
    pub fn level(&self, id: NodeId) -> usize {
        let mut n = 0;
        let mut cur = self.nodes[id].parent;
        while let Some(p) = cur.filter(|&p| p != ROOT) {
            n += 1;
            cur = self.nodes[p].parent;
        }
        n
    }

    /// `id` — раскрытая кнопка или её предок (на пути к самой глубокой раскрытой).
    pub fn on_path(&self, id: NodeId, deep: Option<NodeId>) -> bool {
        let mut cur = deep;
        while let Some(c) = cur {
            if c == id {
                return true;
            }
            cur = self.nodes[c].parent;
        }
        false
    }

    /// Все узлы, кроме корня.
    pub fn ids(&self) -> impl Iterator<Item = NodeId> {
        1..self.nodes.len()
    }

    /// Кнопка сейчас недоступна (её условие `enabled_if` не выполнено).
    pub fn is_disabled(&self, id: NodeId, ctx: &Context) -> bool {
        self.nodes[id].enabled_if.is_some_and(|f| !ctx.get(f))
    }

    pub fn is_state_on(&self, id: NodeId, ctx: &Context) -> bool {
        self.nodes[id].state.is_some_and(|f| ctx.get(f))
    }

    /// Нужен ли меню этот источник состояния (чтобы не опрашивать его зря).
    pub fn uses(&self, flag: Flag) -> bool {
        self.ids().any(|id| {
            let n = &self.nodes[id];
            n.state == Some(flag) || n.enabled_if == Some(flag) || n.off.as_ref().is_some_and(|v| v.flag == flag)
        })
    }

    fn variant(&self, id: NodeId, ctx: &Context) -> Option<&Variant> {
        self.nodes[id].off.as_ref().filter(|v| !ctx.get(v.flag))
    }

    /// Иконка узла с учётом ситуации.
    pub fn icon(&self, id: NodeId, ctx: &Context) -> &IconRef {
        match self.variant(id, ctx) {
            Some(v) => &v.icon,
            None => &self.nodes[id].icon,
        }
    }

    /// Подпись узла с учётом ситуации (как на плашке при наведении).
    pub fn label(&self, id: NodeId, ctx: &Context) -> String {
        let n = &self.nodes[id];
        let mut s = match self.variant(id, ctx) {
            Some(v) => v.label.clone(),
            None => n.label.clone(),
        };
        if let Some(f) = n.state {
            s.push_str(" · ");
            s.push_str(f.suffix(ctx.get(f)));
        }
        s
    }

    /// Все иконки, которые могут понадобиться.
    pub fn icons(&self) -> Vec<IconRef> {
        let mut out: Vec<IconRef> = Vec::new();
        for ctx in Context::all() {
            for id in self.ids() {
                let k = self.icon(id, &ctx);
                if *k != IconRef::None && !out.contains(k) {
                    out.push(k.clone());
                }
            }
        }
        out
    }

    /// Все подписи, которые могут понадобиться (для предварительной отрисовки плашек).
    pub fn label_variants(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for ctx in Context::all() {
            for id in self.ids() {
                let l = self.label(id, &ctx);
                if !out.contains(&l) {
                    out.push(l);
                }
            }
        }
        out
    }
}

/// Меню для программы: какое меню показывать, если активно окно одной из `exes`.
#[derive(Clone, Debug, PartialEq)]
pub struct AppMenu {
    /// Имена exe в нижнем регистре (`code.exe`).
    pub exes: Vec<String>,
    pub menu: Menu,
}

/// Чем вызывается профиль: боковой кнопкой мыши (1 — «назад», 2 — «вперёд») и/или сочетанием клавиш.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Trigger {
    pub button: u8,
    pub hotkey: Option<Chord>,
}

/// Отдельное меню со своим способом вызова (у основного вызов — в настройках).
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub name: String,
    pub trigger: Trigger,
    pub menu: Menu,
}

/// Сколько профилей, кроме основного, можно завести.
pub const MAX_PROFILES: usize = 3;

/// Всё содержимое файла меню: общее меню, меню для отдельных программ и профили.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuSet {
    pub main: Menu,
    pub apps: Vec<AppMenu>,
    pub profiles: Vec<Profile>,
}

impl MenuSet {
    pub fn single(main: Menu) -> MenuSet {
        MenuSet {
            main,
            apps: Vec::new(),
            profiles: Vec::new(),
        }
    }

    /// Все меню по порядку: 0 — общее, дальше — для программ, потом профили.
    pub fn menus(&self) -> impl Iterator<Item = &Menu> {
        std::iter::once(&self.main)
            .chain(self.apps.iter().map(|a| &a.menu))
            .chain(self.profiles.iter().map(|p| &p.menu))
    }

    /// Номер меню (как в `menus`) для профиля `n` (с 0, в порядке файла).
    pub fn profile_slot(&self, n: usize) -> Option<usize> {
        (n < self.profiles.len()).then_some(1 + self.apps.len() + n)
    }

    /// Номер меню (как в `menus`) для активной программы (`code.exe`, регистр не важен).
    pub fn pick(&self, exe: Option<&str>) -> usize {
        let Some(exe) = exe else { return 0 };
        let exe = exe.to_lowercase();
        self.apps
            .iter()
            .position(|a| a.exes.contains(&exe))
            .map_or(0, |i| i + 1)
    }
}

/// Меню по умолчанию (то же, что в шаблоне menu.yaml).
pub fn default_menu() -> Menu {
    Menu::from_items(vec![
        Item::new("square-plus", "Открыть").children(vec![
            Item::new("folder", "Проводник").action(Action::Explorer),
            Item::new("gear", "Параметры").action(Action::Open("ms-settings:".into())),
            Item::new("pencil-to-square", "Заметка").action(Action::Note),
        ]),
        Item::new("paste", "Вставить")
            .action(Action::paste())
            .dwell()
            .children(vec![
                Item::new("clock-arrow-rotate-left", "Буфер обмена").action(Action::clipboard_history())
            ]),
        Item::new("headphones", "Медиа")
            .state(Flag::Media)
            .children(vec![
                Item::new("backward-step", "Назад").action(Action::Media(Media::Prev)),
                Item::new("pause", "Пауза")
                    .action(Action::Media(Media::PlayPause))
                    .off(Flag::Media, "play", "Плей"),
                Item::new("forward-step", "Вперёд").action(Action::Media(Media::Next)),
            ]),
        Item::new("copy", "Копировать")
            .action(Action::copy())
            .dwell()
            .children(vec![
                Item::new("square-dashed", "Выбрать все").action(Action::select_all()),
                Item::new("scissors", "Вырезать").action(Action::cut()),
            ]),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_menu_shape() {
        let m = default_menu();
        assert_eq!(m.top().len(), 4);
        assert_eq!(m.nodes.len(), 1 + 4 + 3 + 1 + 3 + 2);
        assert_eq!(m.max_fan(), 3);
        for &t in m.top() {
            assert_eq!(m.node(t).parent, Some(ROOT));
            for &c in m.children(t) {
                assert_eq!(m.node(c).parent, Some(t));
            }
        }
    }

    #[test]
    fn variant_label_and_icon() {
        let m = default_menu();
        let pause = m.ids().find(|&i| m.node(i).off.is_some()).unwrap();
        let on = Context::default().with(Flag::Media, true);
        let off = Context::default();
        assert_eq!(*m.icon(pause, &on), IconRef::builtin("pause"));
        assert_eq!(m.label(pause, &on), "Пауза");
        assert_eq!(*m.icon(pause, &off), IconRef::builtin("play"));
        assert_eq!(m.label(pause, &off), "Плей");
        let media = m.top()[2];
        assert_eq!(m.label(media, &on), "Медиа · играет");
        assert_eq!(m.label(media, &off), "Медиа · выкл");
        assert!(m.is_state_on(media, &on) && !m.is_state_on(media, &off));
        assert!(m.uses(Flag::Media));
    }

    #[test]
    fn variants_cover_all_states() {
        let m = default_menu();
        let l = m.label_variants();
        for s in ["Плей", "Пауза", "Медиа · играет", "Медиа · выкл", "Копировать"] {
            assert!(l.iter().any(|x| x == s), "{s}");
        }
        let icons = m.icons();
        assert!(icons.contains(&IconRef::builtin("play")) && icons.contains(&IconRef::builtin("pause")));
    }

    #[test]
    fn app_menus_are_picked_by_exe() {
        let other = Menu::from_items(vec![Item::new("gear", "X").action(Action::Note)]);
        let set = MenuSet {
            main: default_menu(),
            apps: vec![AppMenu {
                exes: vec!["code.exe".into(), "cursor.exe".into()],
                menu: other.clone(),
            }],
            profiles: vec![Profile { name: "P".into(), trigger: Trigger { button: 2, hotkey: None }, menu: other }],
        };
        assert_eq!(set.pick(Some("Code.EXE")), 1);
        assert_eq!(set.pick(Some("explorer.exe")), 0);
        assert_eq!(set.pick(None), 0);
        assert_eq!(set.menus().count(), 3);
        assert_eq!(set.profile_slot(0), Some(2));
        assert_eq!(set.profile_slot(1), None);
    }
}

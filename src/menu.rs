//! Дерево меню. Данные плоские (арена), поведение — перечисления, а не замыкания:
//! так меню легко описывать, проверять и (позже) грузить из файла.

use crate::action::{self, vk, Action};

pub type NodeId = usize;

/// Корень дерева (невидимый, держит основные кнопки).
pub const ROOT: NodeId = 0;

/// Что известно меню о внешнем мире на момент кадра.
#[derive(Clone, Copy, Debug, Default)]
pub struct Context {
    pub media_playing: bool,
}

/// Иконка/подпись, зависящие от ситуации.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dynamic {
    None,
    /// «Пауза», пока играет звук, иначе «Плей».
    PlayPause,
}

/// Источник состояния «включено» (зелёный стиль).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateSrc {
    None,
    MediaPlaying,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub icon: &'static str,
    pub label: &'static str,
    pub action: Option<Action>,
    /// Подменю раскрывается, только если задержаться на кнопке.
    pub dwell: bool,
    pub dynamic: Dynamic,
    pub state: StateSrc,
    pub children: Vec<NodeId>,
    pub parent: Option<NodeId>,
}

/// Описание пункта для сборки дерева.
pub struct Item {
    node: Node,
    kids: Vec<Item>,
}

impl Item {
    pub fn new(icon: &'static str, label: &'static str) -> Item {
        Item {
            node: Node {
                icon,
                label,
                action: None,
                dwell: false,
                dynamic: Dynamic::None,
                state: StateSrc::None,
                children: Vec::new(),
                parent: None,
            },
            kids: Vec::new(),
        }
    }
    pub fn action(mut self, a: Action) -> Item {
        self.node.action = Some(a);
        self
    }
    pub fn dwell(mut self) -> Item {
        self.node.dwell = true;
        self
    }
    pub fn dynamic(mut self, d: Dynamic) -> Item {
        self.node.dynamic = d;
        self
    }
    pub fn state(mut self, s: StateSrc) -> Item {
        self.node.state = s;
        self
    }
    pub fn children(mut self, kids: Vec<Item>) -> Item {
        self.kids = kids;
        self
    }
}

#[derive(Clone, Debug)]
pub struct Menu {
    pub nodes: Vec<Node>,
}

impl Menu {
    pub fn from_items(items: Vec<Item>) -> Menu {
        let mut menu = Menu { nodes: Vec::new() };
        let root = Item::new("", "").children(items);
        menu.add(root, None);
        menu
    }

    fn add(&mut self, item: Item, parent: Option<NodeId>) -> NodeId {
        let id = self.nodes.len();
        let mut node = item.node;
        node.parent = parent;
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

    /// Все узлы, кроме корня.
    pub fn ids(&self) -> impl Iterator<Item = NodeId> {
        1..self.nodes.len()
    }

    pub fn is_state_on(&self, id: NodeId, ctx: &Context) -> bool {
        match self.nodes[id].state {
            StateSrc::None => false,
            StateSrc::MediaPlaying => ctx.media_playing,
        }
    }

    /// Иконка узла с учётом ситуации.
    pub fn icon(&self, id: NodeId, ctx: &Context) -> &'static str {
        let n = &self.nodes[id];
        match n.dynamic {
            Dynamic::None => n.icon,
            Dynamic::PlayPause => {
                if ctx.media_playing {
                    "pause"
                } else {
                    "play"
                }
            }
        }
    }

    /// Подпись узла с учётом ситуации (как на плашке при наведении).
    pub fn label(&self, id: NodeId, ctx: &Context) -> String {
        let n = &self.nodes[id];
        let mut s = match n.dynamic {
            Dynamic::None => n.label,
            Dynamic::PlayPause => {
                if ctx.media_playing {
                    "Пауза"
                } else {
                    "Плей"
                }
            }
        }
        .to_string();
        if n.state != StateSrc::None {
            s.push_str(if self.is_state_on(id, ctx) {
                " · играет"
            } else {
                " · выкл"
            });
        }
        s
    }

    /// Все иконки, которые могут понадобиться.
    pub fn icon_keys(&self) -> Vec<&'static str> {
        let mut keys: Vec<&'static str> = Vec::new();
        for ctx in [
            Context {
                media_playing: false,
            },
            Context {
                media_playing: true,
            },
        ] {
            for id in self.ids() {
                let k = self.icon(id, &ctx);
                if !keys.contains(&k) {
                    keys.push(k);
                }
            }
        }
        keys
    }

    /// Все подписи, которые могут понадобиться (для предварительной отрисовки плашек).
    pub fn label_variants(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for ctx in [
            Context {
                media_playing: false,
            },
            Context {
                media_playing: true,
            },
        ] {
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

/// Меню по умолчанию. ТУТ МЕНЯЕТСЯ СОСТАВ МЕНЮ.
pub fn default_menu() -> Menu {
    Menu::from_items(vec![
        Item::new("square-plus", "Открыть").children(vec![
            Item::new("folder", "Проводник").action(Action::Explorer),
            Item::new("gear", "Параметры").action(Action::OpenUri("ms-settings:")),
            Item::new("pencil-to-square", "Заметка").action(Action::Note),
        ]),
        Item::new("paste", "Вставить")
            .action(action::PASTE)
            .dwell()
            .children(vec![Item::new("clock-arrow-rotate-left", "Буфер обмена")
                .action(action::CLIPBOARD_HISTORY)]),
        Item::new("headphones", "Медиа")
            .state(StateSrc::MediaPlaying)
            .children(vec![
                Item::new("backward-step", "Назад").action(Action::MediaKey(vk::MEDIA_PREV)),
                Item::new("pause", "Пауза")
                    .action(Action::MediaKey(vk::MEDIA_PLAY_PAUSE))
                    .dynamic(Dynamic::PlayPause),
                Item::new("forward-step", "Вперёд").action(Action::MediaKey(vk::MEDIA_NEXT)),
            ]),
        Item::new("copy", "Копировать")
            .action(action::COPY)
            .dwell()
            .children(vec![
                Item::new("square-dashed", "Выбрать все").action(action::SELECT_ALL),
                Item::new("scissors", "Вырезать").action(action::CUT),
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
        for &t in m.top() {
            assert_eq!(m.node(t).parent, Some(ROOT));
            for &c in m.children(t) {
                assert_eq!(m.node(c).parent, Some(t));
            }
        }
    }

    #[test]
    fn dynamic_label_and_icon() {
        let m = default_menu();
        let pause = m
            .ids()
            .find(|&i| m.node(i).dynamic == Dynamic::PlayPause)
            .unwrap();
        let on = Context {
            media_playing: true,
        };
        let off = Context {
            media_playing: false,
        };
        assert_eq!(
            (m.icon(pause, &on), m.label(pause, &on).as_str()),
            ("pause", "Пауза")
        );
        assert_eq!(
            (m.icon(pause, &off), m.label(pause, &off).as_str()),
            ("play", "Плей")
        );
        let media = m.top()[2];
        assert_eq!(m.label(media, &on), "Медиа · играет");
        assert_eq!(m.label(media, &off), "Медиа · выкл");
    }

    #[test]
    fn variants_cover_dynamic_states() {
        let m = default_menu();
        let l = m.label_variants();
        for s in [
            "Плей",
            "Пауза",
            "Медиа · играет",
            "Медиа · выкл",
            "Копировать",
        ] {
            assert!(l.iter().any(|x| x == s), "{s}");
        }
        assert!(m.icon_keys().contains(&"play") && m.icon_keys().contains(&"pause"));
    }
}

//! Файл меню `menu.yaml`: состав меню без пересборки программы.
//! Формат описан в шаблоне (`TEMPLATE`), который кладётся при первом запуске.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::action::{vk, Action, Media, Run, Step};
use super::icon::IconRef;
use super::menu::{AppMenu, Flag, Item, Menu, MenuSet, Shape, Variant, MAX_DEPTH, MAX_KIDS, MAX_SUB, MAX_TOP};

/// Шаблон menu.yaml: меню по умолчанию и описание формата с примерами.
pub const TEMPLATE: &str = include_str!("menu_template.yaml");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuError {
    /// Номер строки с 1 (0 — ошибка в смысле, а не в синтаксисе; место — в тексте).
    pub line: usize,
    pub msg: String,
}

impl fmt::Display for MenuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}", self.msg)
        } else {
            write!(f, "строка {}: {}", self.line, self.msg)
        }
    }
}

// ---------------------------------------------------------------- схема файла

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Doc {
    #[serde(default)]
    layout: Option<LayoutDoc>,
    menu: Vec<ItemDoc>,
    #[serde(default)]
    apps: Vec<AppDoc>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct LayoutDoc {
    #[serde(default)]
    start: Option<StartDoc>,
    #[serde(default)]
    clockwise: Option<bool>,
    #[serde(default)]
    fan_spread: Option<f32>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StartDoc {
    Deg(f32),
    Name(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AppDoc {
    #[serde(rename = "match")]
    matches: OneOrMany,
    menu: Vec<ItemDoc>,
    #[serde(default)]
    layout: Option<LayoutDoc>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

/// Поля действия — одинаковые у кнопки и у шага цепочки.
macro_rules! action_fields {
    ($(#[$m:meta])* struct $name:ident { $($extra:tt)* }) => {
        $(#[$m])*
        struct $name {
            #[serde(default)]
            keys: Option<String>,
            #[serde(default)]
            text: Option<String>,
            #[serde(default)]
            run: Option<RunDoc>,
            #[serde(default)]
            open: Option<String>,
            #[serde(default)]
            media: Option<String>,
            #[serde(default)]
            volume: Option<String>,
            #[serde(default)]
            action: Option<String>,
            #[serde(default)]
            steps: Option<Vec<StepDoc>>,
            $($extra)*
        }
    };
}

action_fields! {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ItemDoc {
        label: String,
        #[serde(default)]
        icon: Option<IconDoc>,
        #[serde(default)]
        color: Option<String>,
        #[serde(default)]
        dwell: bool,
        #[serde(default)]
        state: Option<String>,
        #[serde(default)]
        when_off: Option<VariantDoc>,
        #[serde(default)]
        items: Vec<ItemDoc>,
    }
}

action_fields! {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct StepDoc {
        #[serde(default)]
        wait: Option<u32>,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VariantDoc {
    flag: String,
    #[serde(default)]
    icon: Option<IconDoc>,
    label: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum IconDoc {
    Name(String),
    File {
        file: String,
        #[serde(default)]
        color: Option<bool>,
    },
    From {
        from: String,
    },
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RunDoc {
    Path(String),
    Full(RunFull),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunFull {
    path: String,
    #[serde(default)]
    args: Option<ArgsDoc>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    admin: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ArgsDoc {
    Line(String),
    List(Vec<String>),
}

// ---------------------------------------------------------------- разбор

/// Разобрать и проверить файл меню.
/// `base` — папка файла (от неё считаются относительные пути иконок);
/// `is_icon` — есть ли встроенная иконка с таким именем (её знает слой графики).
pub fn parse(text: &str, base: Option<&Path>, is_icon: &dyn Fn(&str) -> bool) -> Result<MenuSet, MenuError> {
    let doc: Doc = serde_yaml_ng::from_str(text).map_err(|e| MenuError {
        line: e.location().map_or(0, |l| l.line()),
        msg: friendly(&e.to_string()),
    })?;
    let cx = Cx { base, is_icon };
    let sem = |msg: String| MenuError { line: 0, msg };
    let shape = cx.shape(doc.layout.as_ref(), Shape::default()).map_err(sem)?;
    let main = cx.menu(&doc.menu, shape, "").map_err(sem)?;
    let mut apps = Vec::new();
    for (i, a) in doc.apps.iter().enumerate() {
        let exes: Vec<String> = match &a.matches {
            OneOrMany::One(s) => vec![s.clone()],
            OneOrMany::Many(v) => v.clone(),
        }
        .into_iter()
        .map(|e| {
            let e = e.trim().to_lowercase();
            if e.ends_with(".exe") {
                e
            } else {
                format!("{e}.exe")
            }
        })
        .collect();
        let at = format!("apps {} ({}) → ", i + 1, exes.join(", "));
        if exes.is_empty() {
            return Err(sem(format!("{at}match: укажите хотя бы одну программу")));
        }
        let shape = cx.shape(a.layout.as_ref(), main.shape).map_err(sem)?;
        let menu = cx.menu(&a.menu, shape, &at).map_err(sem)?;
        apps.push(AppMenu { exes, menu });
    }
    Ok(MenuSet { main, apps })
}

/// Сообщения serde_yaml_ng — на понятный язык (самые частые случаи).
fn friendly(msg: &str) -> String {
    let mut m = msg.to_string();
    if let Some(cut) = m.find(" at line ") {
        m.truncate(cut); // номер строки покажем сами
    }
    if let Some(rest) = m.split("unknown field `").nth(1) {
        let field = rest.split('`').next().unwrap_or("");
        let at = m.split(": unknown field").next().filter(|p| !p.starts_with("unknown")).unwrap_or("");
        let at = if at.is_empty() { String::new() } else { format!("{at}: ") };
        return format!("{at}неизвестное поле «{field}» (опечатка? список полей — в начале файла)");
    }
    if let Some(rest) = m.split("missing field `").nth(1) {
        let field = rest.split('`').next().unwrap_or("");
        return format!("не хватает поля «{field}»");
    }
    for (needle, hint) in [
        ("untagged enum IconDoc", "icon: ожидается имя, путь к файлу, { file: …, color: … } или { from: … }"),
        ("untagged enum RunDoc", "run: ожидается путь или { path: …, args: …, cwd: …, admin: … }"),
        ("untagged enum ArgsDoc", "args: ожидается строка или список строк"),
        ("untagged enum StartDoc", "start: ожидается top, right, bottom, left или угол"),
        ("untagged enum OneOrMany", "match: ожидается имя программы или список имён"),
    ] {
        if m.contains(needle) {
            return hint.into();
        }
    }
    m
}

/// `%USERPROFILE%\…` → полный путь (неизвестные переменные остаются как есть).
pub fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('%') {
            Some(j) if j > 0 && !after[..j].contains(char::is_whitespace) => {
                let name = &after[..j];
                match std::env::var(name) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[j + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `#RGB` / `#RRGGBB` → 0xRRGGBB.
fn color(s: &str) -> Option<u32> {
    let h = s.trim().strip_prefix('#')?;
    let v = u32::from_str_radix(h, 16).ok()?;
    match h.len() {
        6 => Some(v),
        3 => {
            let (r, g, b) = ((v >> 8) & 0xF, (v >> 4) & 0xF, v & 0xF);
            Some((r * 17) << 16 | (g * 17) << 8 | (b * 17))
        }
        _ => None,
    }
}

struct Cx<'a> {
    base: Option<&'a Path>,
    is_icon: &'a dyn Fn(&str) -> bool,
}

impl Cx<'_> {
    fn path(&self, s: &str) -> PathBuf {
        let p = PathBuf::from(expand_env(s.trim()));
        match self.base {
            Some(b) if p.is_relative() => b.join(p),
            _ => p,
        }
    }

    fn shape(&self, doc: Option<&LayoutDoc>, mut s: Shape) -> Result<Shape, String> {
        let Some(d) = doc else { return Ok(s) };
        if let Some(start) = &d.start {
            s.start_angle = match start {
                StartDoc::Deg(a) => *a,
                StartDoc::Name(n) => match n.as_str() {
                    "top" => -90.0,
                    "right" => 0.0,
                    "bottom" => 90.0,
                    "left" => 180.0,
                    _ => return Err(format!("layout → start: «{n}» — ожидается top, right, bottom, left или угол")),
                },
            };
        }
        if let Some(c) = d.clockwise {
            s.clockwise = c;
        }
        if let Some(f) = d.fan_spread {
            if !(20.0..=90.0).contains(&f) {
                return Err(format!("layout → fan_spread: {f} — ожидается от 20 до 90 градусов"));
            }
            s.fan_spread = f;
        }
        Ok(s)
    }

    fn menu(&self, items: &[ItemDoc], shape: Shape, at: &str) -> Result<Menu, String> {
        if items.is_empty() {
            return Err(format!("{at}menu: нужна хотя бы одна кнопка"));
        }
        if items.len() > MAX_TOP {
            return Err(format!("{at}menu: основных кнопок {} — помещается не больше {MAX_TOP}", items.len()));
        }
        let mut tops = Vec::new();
        for (i, d) in items.iter().enumerate() {
            let here = format!("{at}кнопка {} «{}»", i + 1, d.label);
            let item = self.node(d, &here, 1)?;
            tops.push(item);
        }
        Ok(Menu::from_items(tops).with_shape(shape))
    }

    /// Кнопка вместе с веером; `depth` — её уровень (1 — основная).
    fn node(&self, d: &ItemDoc, here: &str, depth: usize) -> Result<Item, String> {
        let mut item = self.item(d, here)?;
        let max = if depth == 1 { MAX_KIDS } else { MAX_SUB };
        if !d.items.is_empty() && depth >= MAX_DEPTH {
            return Err(format!("{here}: веера вкладываются не глубже {} уровней", MAX_DEPTH - 1));
        }
        if d.items.len() > max {
            return Err(format!("{here}: в веере не больше {max} пунктов"));
        }
        for (j, k) in d.items.iter().enumerate() {
            let kh = format!("{here} → пункт {} «{}»", j + 1, k.label);
            item.kids.push(self.node(k, &kh, depth + 1)?);
        }
        let n = &item.node;
        if item.kids.is_empty() && n.action.is_none() {
            let hint = if depth == 1 { "нет ни действия, ни пунктов веера (items)" } else { "пункту веера нужно действие (keys, run, open…)" };
            return Err(format!("{here}: {hint}"));
        }
        if n.dwell && (item.kids.is_empty() || n.action.is_none()) {
            return Err(format!("{here}: dwell имеет смысл, только когда у кнопки есть и действие, и веер"));
        }
        Ok(item)
    }

    fn item(&self, d: &ItemDoc, here: &str) -> Result<Item, String> {
        let label = d.label.trim();
        if label.is_empty() {
            return Err(format!("{here}: пустая подпись"));
        }
        let fields = Fields {
            keys: &d.keys,
            text: &d.text,
            run: &d.run,
            open: &d.open,
            media: &d.media,
            volume: &d.volume,
            action: &d.action,
            steps: &d.steps,
        };
        let action = self.action(&fields).map_err(|e| format!("{here}: {e}"))?;
        let icon = self.icon(d.icon.as_ref(), label, action.as_ref()).map_err(|e| format!("{here}: {e}"))?;
        let mut item = Item::new("", label).icon(icon);
        if let Some(a) = action {
            item = item.action(a);
        }
        if d.dwell {
            item = item.dwell();
        }
        if let Some(c) = &d.color {
            let rgb = color(c).ok_or_else(|| format!("{here}: color «{c}» — ожидается вида \"#3D7BFF\""))?;
            item = item.color(rgb);
        }
        if let Some(s) = &d.state {
            item = item.state(flag(s).map_err(|e| format!("{here}: state: {e}"))?);
        }
        if let Some(v) = &d.when_off {
            let f = flag(&v.flag).map_err(|e| format!("{here}: when_off: {e}"))?;
            let icon = self.icon(v.icon.as_ref(), &v.label, None).map_err(|e| format!("{here}: when_off: {e}"))?;
            item.node.off = Some(Variant {
                flag: f,
                icon,
                label: v.label.trim().to_string(),
            });
        }
        Ok(item)
    }

    fn icon(&self, d: Option<&IconDoc>, label: &str, action: Option<&Action>) -> Result<IconRef, String> {
        let file = |s: &str, color: Option<bool>| -> Result<IconRef, String> {
            let path = self.path(s);
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if ext != "svg" && ext != "png" {
                return Err(format!("icon «{s}»: поддерживаются файлы .svg и .png"));
            }
            // PNG обычно цветные, SVG из наборов иконок — одноцветные (их красим под тему)
            Ok(IconRef::File {
                color: color.unwrap_or(ext == "png"),
                path,
            })
        };
        match d {
            None => Ok(match action {
                Some(Action::Run(r)) => IconRef::Program(PathBuf::from(expand_env(&r.path))),
                _ => IconRef::letter_for(label),
            }),
            Some(IconDoc::Name(s)) => {
                let s = s.trim();
                let low = s.to_lowercase();
                if low == "none" {
                    Ok(IconRef::None)
                } else if low == "letter" {
                    Ok(IconRef::letter_for(label))
                } else if s.contains(['/', '\\']) || low.ends_with(".svg") || low.ends_with(".png") {
                    file(s, None)
                } else if (self.is_icon)(s) {
                    Ok(IconRef::builtin(s))
                } else {
                    Err(format!(
                        "нет встроенной иконки «{s}» (список — в начале файла; свой значок — путь к .svg или .png)"
                    ))
                }
            }
            Some(IconDoc::File { file: f, color }) => file(f, *color),
            Some(IconDoc::From { from }) => Ok(IconRef::Program(self.path(from))),
        }
    }

    fn action(&self, f: &Fields) -> Result<Option<Action>, String> {
        let mut found: Vec<(&str, Action)> = Vec::new();
        if let Some(k) = f.keys {
            found.push(("keys", Action::Keys(vk::parse_list(k)?)));
        }
        if let Some(t) = f.text {
            if t.is_empty() {
                return Err("text: пустой текст".into());
            }
            found.push(("text", Action::Text(t.clone())));
        }
        if let Some(r) = f.run {
            found.push(("run", Action::Run(run(r)?)));
        }
        if let Some(o) = f.open {
            if o.trim().is_empty() {
                return Err("open: пустой адрес".into());
            }
            found.push(("open", Action::Open(expand_env(o.trim()))));
        }
        if let Some(m) = f.media {
            let m = Media::from_key(m).ok_or_else(|| format!("media: «{m}» — ожидается prev, next, play-pause или mute"))?;
            found.push(("media", Action::Media(m)));
        }
        if let Some(v) = f.volume {
            let up = match v.trim() {
                "up" => true,
                "down" => false,
                _ => return Err(format!("volume: «{v}» — ожидается up или down")),
            };
            found.push(("volume", Action::Volume(up)));
        }
        if let Some(a) = f.action {
            let act = match a.trim() {
                "explorer" => Action::Explorer,
                "note" => Action::Note,
                "settings" => Action::Settings,
                _ => return Err(format!("action: «{a}» — ожидается explorer, note или settings")),
            };
            found.push(("action", act));
        }
        if let Some(steps) = f.steps {
            let mut out = Vec::new();
            for (i, s) in steps.iter().enumerate() {
                let fields = Fields {
                    keys: &s.keys,
                    text: &s.text,
                    run: &s.run,
                    open: &s.open,
                    media: &s.media,
                    volume: &s.volume,
                    action: &s.action,
                    steps: &s.steps,
                };
                let action = self.action(&fields).map_err(|e| format!("steps → шаг {}: {e}", i + 1))?;
                if matches!(action, Some(Action::Steps(_))) {
                    return Err(format!("steps → шаг {}: цепочка внутри цепочки не нужна", i + 1));
                }
                if action.is_none() && s.wait.is_none() {
                    return Err(format!("steps → шаг {}: нужно действие или wait", i + 1));
                }
                out.push(Step {
                    action,
                    wait_ms: s.wait.unwrap_or(0).min(10_000),
                });
            }
            if out.is_empty() {
                return Err("steps: пустая цепочка".into());
            }
            found.push(("steps", Action::Steps(out)));
        }
        match found.len() {
            0 => Ok(None),
            1 => Ok(found.pop().map(|(_, a)| a)),
            _ => Err(format!(
                "у кнопки несколько действий ({}) — оставьте одно или соберите их в steps",
                found.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
            )),
        }
    }
}

struct Fields<'a> {
    keys: &'a Option<String>,
    text: &'a Option<String>,
    run: &'a Option<RunDoc>,
    open: &'a Option<String>,
    media: &'a Option<String>,
    volume: &'a Option<String>,
    action: &'a Option<String>,
    steps: &'a Option<Vec<StepDoc>>,
}

/// Аргумент со пробелами — в кавычках (как в командной строке).
fn quote(a: &str) -> String {
    if a.is_empty() || a.contains([' ', '\t', '"']) {
        format!("\"{}\"", a.replace('"', "\\\""))
    } else {
        a.to_string()
    }
}

fn run(r: &RunDoc) -> Result<Run, String> {
    let out = match r {
        RunDoc::Path(p) => Run {
            path: expand_env(p.trim()),
            ..Run::default()
        },
        RunDoc::Full(f) => Run {
            path: expand_env(f.path.trim()),
            args: match &f.args {
                None => String::new(),
                Some(ArgsDoc::Line(s)) => expand_env(s),
                Some(ArgsDoc::List(v)) => v.iter().map(|a| quote(&expand_env(a))).collect::<Vec<_>>().join(" "),
            },
            cwd: f.cwd.as_deref().map(|c| expand_env(c.trim())).unwrap_or_default(),
            admin: f.admin,
        },
    };
    if out.path.is_empty() {
        return Err("run: пустой путь".into());
    }
    Ok(out)
}

fn flag(s: &str) -> Result<Flag, String> {
    Flag::from_key(s.trim()).ok_or_else(|| {
        let known: Vec<&str> = Flag::ALL.iter().map(|f| f.key()).collect();
        format!("неизвестный флаг «{s}» (есть: {})", known.join(", "))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::menu::default_menu;

    fn any(_: &str) -> bool {
        true
    }

    fn one(text: &str) -> Result<MenuSet, MenuError> {
        parse(text, Some(Path::new("C:/cfg")), &any)
    }

    fn err(text: &str) -> String {
        one(text).unwrap_err().to_string()
    }

    #[test]
    fn template_is_the_default_menu() {
        let set = parse(TEMPLATE, None, &crate::render::assets::exists).unwrap();
        assert_eq!(set.main, default_menu());
        assert!(set.apps.is_empty(), "примеры в шаблоне закомментированы");
    }

    #[test]
    fn template_examples_are_valid_when_uncommented() {
        // раскомментированные примеры из шаблона должны разбираться без правок
        let mut text = String::new();
        let mut in_examples = false;
        for line in TEMPLATE.lines() {
            if line.starts_with("# >>> пример") {
                in_examples = true;
                continue;
            }
            if in_examples {
                if let Some(rest) = line.strip_prefix("# ") {
                    text.push_str(rest);
                } else if line == "#" {
                } else {
                    text.push_str(line);
                }
            } else {
                text.push_str(line);
            }
            text.push('\n');
        }
        let set = parse(&text, Some(Path::new("C:/cfg")), &crate::render::assets::exists)
            .unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert!(set.main.top().len() > 4);
        assert!(!set.apps.is_empty());
    }

    #[test]
    fn actions_and_shortcuts() {
        let set = one(
            r#"
menu:
  - { label: Блокнот, run: notepad }
  - label: VS Code
    run: { path: "%TEMP_TEST_DIR%/code.exe", args: [".", "--new window"], cwd: D:/p, admin: true }
  - { label: Сайт, open: "https://ya.ru" }
  - { label: Подпись, text: "С уважением" }
  - { label: Тише, volume: down }
  - label: Скопировать всё
    steps: [ { keys: ctrl+a }, { wait: 50 }, { keys: ctrl+c } ]
"#,
        )
        .unwrap();
        let m = &set.main;
        let act = |i: usize| m.node(m.top()[i]).action.clone().unwrap();
        assert_eq!(act(0), Action::Run(Run { path: "notepad".into(), ..Run::default() }));
        // у run без icon — иконка программы
        assert_eq!(m.node(m.top()[0]).icon, IconRef::Program("notepad".into()));
        let Action::Run(r) = act(1) else { panic!() };
        assert_eq!(r.args, ". \"--new window\"");
        assert!(r.admin && r.cwd == "D:/p");
        assert_eq!(act(2), Action::Open("https://ya.ru".into()));
        assert_eq!(m.node(m.top()[2]).icon, IconRef::Letter('С'));
        assert_eq!(act(3), Action::Text("С уважением".into()));
        assert_eq!(act(4), Action::Volume(false));
        let Action::Steps(s) = act(5) else { panic!() };
        assert_eq!(s.len(), 3);
        assert_eq!((s[1].action.clone(), s[1].wait_ms), (None, 50));
    }

    #[test]
    fn icons_colors_layout_and_apps() {
        let set = one(
            r##"
layout: { start: left, clockwise: false, fan_spread: 45 }
menu:
  - { label: A, icon: icons/a.svg, keys: a }
  - { label: B, icon: { file: "D:/b.svg", color: true }, keys: b }
  - { label: C, icon: { from: "C:/Program Files/X/x.exe" }, keys: c, color: "#e5484d" }
  - { label: D, icon: none, keys: d, color: "#0f0" }
apps:
  - match: Code
    layout: { start: 30 }
    menu: [ { label: Палитра, keys: ctrl+shift+p } ]
"##,
        )
        .unwrap();
        let m = &set.main;
        assert_eq!(m.shape.start_angle, 180.0);
        assert!(!m.shape.clockwise && m.shape.fan_spread == 45.0);
        let n = |i: usize| m.node(m.top()[i]);
        assert_eq!(n(0).icon, IconRef::File { path: Path::new("C:/cfg").join("icons/a.svg"), color: false });
        assert_eq!(n(1).icon, IconRef::File { path: "D:/b.svg".into(), color: true });
        assert_eq!(n(2).icon, IconRef::Program("C:/Program Files/X/x.exe".into()));
        assert_eq!((n(2).color, n(3).color), (Some(0xE5484D), Some(0x00FF00)));
        assert_eq!(n(3).icon, IconRef::None);
        assert_eq!(set.apps[0].exes, ["code.exe"]);
        assert_eq!(set.apps[0].menu.shape.start_angle, 30.0);
        assert!(!set.apps[0].menu.shape.clockwise, "остальное наследуется от общего layout");
        assert_eq!(set.pick(Some("CODE.exe")), 1);
    }

    #[test]
    fn readable_errors() {
        let e = err("menu:\n  - label: A\n    keyz: a\n");
        assert!(e.starts_with("строка 3") && e.contains("«keyz»"), "{e}");
        let e = err("menu:\n  - { keys: a }\n");
        assert!(e.contains("label"), "{e}");
        let e = err("menu:\n  - { label: A, keys: ctrl+щ }\n");
        assert!(e.contains("кнопка 1 «A»") && e.contains("щ"), "{e}");
        let e = err("menu:\n  - { label: A, keys: a, run: b }\n");
        assert!(e.contains("несколько действий"), "{e}");
        let e = err("menu:\n  - label: A\n    items: [ { label: B } ]\n");
        assert!(e.contains("пункт 1 «B»") && e.contains("нужно действие"), "{e}");
        let e = err("menu:\n  - { label: A }\n");
        assert!(e.contains("нет ни действия"), "{e}");
        let e = err("menu:\n  - { label: A, keys: a, icon: icons/a.gif }\n");
        assert!(e.contains(".svg и .png"), "{e}");
        let e = parse("menu:\n  - { label: A, keys: a, icon: nope }\n", None, &|_| false).unwrap_err();
        assert!(e.to_string().contains("нет встроенной иконки «nope»"));
        let e = err("menu:\n  - { label: A, keys: a, color: red }\n");
        assert!(e.contains("color"), "{e}");
        let e = err("menu: []\n");
        assert!(e.contains("хотя бы одна"), "{e}");
        let e = err("menu:\n  - { label: A, keys: a, icon: [1] }\n");
        assert!(e.contains("icon: ожидается"), "{e}");
        let e = err("menu:\n  - { label: A, keys: a, dwell: true }\n");
        assert!(e.contains("dwell"), "{e}");
        let e = err("layout: { start: up }\nmenu:\n  - { label: A, keys: a }\n");
        assert!(e.contains("start"), "{e}");
        let e = err("menu:\n  - { label: A, steps: [ { steps: [ { keys: b } ] } ] }\n");
        assert!(e.contains("цепочка внутри цепочки"), "{e}");
    }

    #[test]
    fn limits() {
        let many = format!("menu:\n{}", "  - { label: A, keys: a }\n".repeat(MAX_TOP + 1));
        assert!(err(&many).contains(&MAX_TOP.to_string()));
        let ok = format!("menu:\n{}", "  - { label: A, keys: a }\n".repeat(MAX_TOP));
        assert_eq!(one(&ok).unwrap().main.top().len(), MAX_TOP);
        let fan = format!("menu:\n  - label: A\n    items:\n{}", "      - { label: B, keys: b }\n".repeat(MAX_KIDS + 1));
        assert!(err(&fan).contains(&MAX_KIDS.to_string()));
    }

    #[test]
    fn env_and_colors() {
        std::env::set_var("RM_TEST_VAR", "X:/home");
        assert_eq!(expand_env("%RM_TEST_VAR%/a"), "X:/home/a");
        assert_eq!(expand_env("100% and %NOPE_RM%"), "100% and %NOPE_RM%");
        assert_eq!(color("#123"), Some(0x112233));
        assert_eq!(color("#3D7BFF"), Some(0x3D7BFF));
        assert_eq!(color("3D7BFF"), None);
    }
}

//! The HUD and every menu. Menus are lists of items worked out from the
//! game's state each frame and rebuilt only when they change, so a pad
//! (up, down, left, right, A, B) and a mouse both work everywhere.

use std::collections::BTreeMap;

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::blocks;
use crate::creations::{Library, Origin};
use crate::creatures::Herd;
use crate::input::{glyph, Controls};
use crate::player::{MainCamera, Me, MAX_HEALTH};
use crate::script::{KnobValue, Scripts};
use crate::settings::Settings;
use crate::textures::BlockAtlas;

pub const INK: Color = Color::srgb(0.98, 0.96, 0.9);
pub const DIM: Color = Color::srgba(0.98, 0.96, 0.9, 0.62);
pub const MINT: Color = Color::srgb(0.37, 0.88, 0.78);
pub const GOLD: Color = Color::srgb(1.0, 0.77, 0.36);
pub const RED: Color = Color::srgb(1.0, 0.45, 0.42);
const PANEL: Color = Color::srgba(0.07, 0.09, 0.12, 0.86);
const CARD: Color = Color::srgba(1.0, 1.0, 1.0, 0.06);
const PICK: Color = Color::srgba(0.37, 0.88, 0.78, 0.22);

pub const SWATCHES: &[&str] = &[
    "#4f7a3a", "#6fa64a", "#2f5d50", "#3a6fb0", "#5fe0c8", "#b18cff", "#ff7fa0", "#e84a4a", "#ffc35c", "#f0e0b0", "#8a7a52", "#3b2a1c", "#f5f5f5", "#2a2a2e",
];

#[derive(Clone, Debug, PartialEq)]
pub enum Screen {
    Title,
    Playing,
    Pause,
    Inside(String),
    Tweak(String),
    Share(String),
    Join,
    Code,
    Library,
    Settings,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Resume,
    PlayAlone,
    Host,
    JoinAddress(String),
    OpenJoin,
    OpenCode,
    LoadCode(String),
    OpenLibrary,
    OpenSettings,
    Inside(String),
    Tweak(String),
    Remix(String),
    Share(String),
    EditFolder(String),
    Run(String),
    Stop(String),
    ResetKnobs(String),
    Back,
    Leave,
    Quit,
    Survival,
    InvertY,
    Lighting,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Button(Act),
    Slider { key: String, value: f64, min: f64, max: f64 },
    Toggle { key: String, value: bool },
    Swatch { key: String, value: String },
    /// A text field; typed letters go in.
    Field { key: String, value: String },
    Note,
    Big,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub label: String,
    pub kind: Kind,
}

fn button(label: impl Into<String>, act: Act) -> Item {
    Item { label: label.into(), kind: Kind::Button(act) }
}
fn note(label: impl Into<String>) -> Item {
    Item { label: label.into(), kind: Kind::Note }
}

impl Item {
    fn selectable(&self) -> bool {
        !matches!(self.kind, Kind::Note | Kind::Big)
    }
}

#[derive(Resource)]
pub struct Focus {
    pub screen: Screen,
    pub sel: usize,
    pub scroll: i32,
    /// Text fields' contents, by key.
    pub fields: BTreeMap<String, String>,
    /// Worlds found on the local network: (name, address).
    pub lan: Vec<(String, String)>,
    /// What the last share or code load said.
    pub status: String,
    pub reload_at: Option<(String, f32)>,
    sig: String,
    pub items: Vec<Item>,
}

impl Default for Focus {
    fn default() -> Self {
        Self {
            screen: Screen::Title,
            sel: 0,
            scroll: 0,
            fields: BTreeMap::new(),
            lan: vec![],
            status: String::new(),
            reload_at: None,
            sig: String::new(),
            items: vec![],
        }
    }
}

impl Focus {
    pub fn playing(&self) -> bool {
        self.screen == Screen::Playing
    }
    pub fn go(&mut self, s: Screen) {
        self.screen = s;
        self.sel = 0;
        self.scroll = 0;
    }
}

#[derive(Resource, Default)]
pub struct Toasts(pub Vec<(String, f32)>);

impl Toasts {
    pub fn push(&mut self, s: impl Into<String>) {
        let s = s.into();
        if self.0.iter().any(|(t, _)| *t == s) {
            return;
        }
        self.0.push((s, 6.0));
        if self.0.len() > 4 {
            self.0.remove(0);
        }
    }
}

#[derive(Resource, Default)]
pub struct Acts(pub Vec<Act>);

#[derive(Resource, Clone)]
pub struct Fonts {
    pub ui: Handle<Font>,
    pub title: Handle<Font>,
    pub code: Handle<Font>,
}

pub fn fonts(mut fonts: ResMut<Assets<Font>>, mut commands: Commands) {
    let ui = fonts.add(Font::from_bytes(include_bytes!("../../../assets/fonts/Montserrat-SemiBold.ttf").to_vec()));
    let title = fonts.add(Font::from_bytes(include_bytes!("../../../assets/fonts/Fredoka.ttf").to_vec()));
    commands.insert_resource(Fonts { ui, title, code: Handle::default() });
}

/// What the world shows in the top corner and the title.
#[derive(Resource, Default, Clone)]
pub struct WorldInfo {
    pub line: String,
    pub in_world: bool,
}

// ---------- the menus' contents ----------

pub fn menu_items(focus: &Focus, lib: &Library, scripts: &Scripts, settings: &Settings, info: &WorldInfo) -> (String, Vec<Item>) {
    match &focus.screen {
        Screen::Title => (
            "Potluck".into(),
            vec![
                note("A block world where everything is made by someone you know."),
                button("Play alone", Act::PlayAlone),
                button("Host a world for friends", Act::Host),
                button("Join a friend's world", Act::OpenJoin),
                button("Load a creation from a code", Act::OpenCode),
                button("Creations", Act::OpenLibrary),
                button("Settings", Act::OpenSettings),
                button("Quit", Act::Quit),
            ],
        ),
        Screen::Playing => (String::new(), vec![]),
        Screen::Pause => {
            let mut v = vec![note(info.line.clone()), button("Back to the world", Act::Resume), button("Creations in this world", Act::OpenLibrary), button("Load a creation from a code", Act::OpenCode)];
            v.push(button("Settings", Act::OpenSettings));
            v.push(button("Leave this world", Act::Leave));
            ("Paused".into(), v)
        }
        Screen::Inside(id) => {
            let Some(c) = lib.list.get(id) else { return ("Gone".into(), vec![button("Back", Act::Back)]) };
            let m = &c.manifest;
            let mut v = vec![note(format!("{} by {}", cap(&m.kind), m.by))];
            if !m.about.is_empty() {
                v.push(note(m.about.clone()));
            }
            if let Some(r) = &m.remix_of {
                v.push(note(format!("A remix of {r}")));
            }
            let vm = scripts.vms.get(id);
            if vm.map(|v| !v.knobs.is_empty()).unwrap_or(false) {
                v.push(button("Tweak", Act::Tweak(id.clone())));
            }
            if c.editable() {
                v.push(button("Edit in my editor", Act::EditFolder(id.clone())));
            } else {
                v.push(button("Remix (make my own copy)", Act::Remix(id.clone())));
            }
            v.push(button("Share with a code", Act::Share(id.clone())));
            if info.in_world {
                if lib.running.contains(id) {
                    v.push(button("Take out of this world", Act::Stop(id.clone())));
                } else {
                    v.push(button("Bring into this world", Act::Run(id.clone())));
                }
            }
            v.push(button("Back", Act::Back));
            let lic = format!("Licence {} · {} files · {:.1} KB", m.licence, c.files.len(), c.size() as f32 / 1024.0);
            v.push(note(lic));
            if let Some(code) = &c.code {
                v.push(note(format!("Code {}", pretty_code(code))));
            }
            (m.name.clone(), v)
        }
        Screen::Tweak(id) => {
            let name = lib.list.get(id).map(|c| c.manifest.name.clone()).unwrap_or_default();
            let mut v = vec![note("Changes show at once, for everyone in this world.")];
            if let Some(vm) = scripts.vms.get(id) {
                for k in &vm.knobs {
                    let kind = match &k.value {
                        KnobValue::Number(n) => Kind::Slider { key: k.key.clone(), value: *n, min: k.min, max: k.max },
                        KnobValue::Toggle(b) => Kind::Toggle { key: k.key.clone(), value: *b },
                        KnobValue::Color(c) => Kind::Swatch { key: k.key.clone(), value: c.clone() },
                    };
                    v.push(Item { label: k.label.clone(), kind });
                }
            }
            v.push(button("Back to the code", Act::Inside(id.clone())));
            v.push(button("Put the knobs back", Act::ResetKnobs(id.clone())));
            v.push(button("Done", Act::Resume));
            (format!("Tweak {name}"), v)
        }
        Screen::Share(id) => {
            let name = lib.list.get(id).map(|c| c.manifest.name.clone()).unwrap_or_default();
            let mut v = vec![];
            if let Some(code) = lib.list.get(id).and_then(|c| c.code.clone()) {
                v.push(Item { label: pretty_code(&code), kind: Kind::Big });
                v.push(note(format!("potluck.linuxgroove.com/c/{code}")));
                v.push(note("Say it or type it: anyone can load it from Load a creation from a code."));
            }
            v.push(note(focus.status.clone()));
            v.push(button("Back", Act::Inside(id.clone())));
            (format!("Share {name}"), v)
        }
        Screen::Join => {
            let mut v = vec![];
            if focus.lan.is_empty() {
                v.push(note("Looking for worlds on this network..."));
            }
            for (name, addr) in &focus.lan {
                v.push(button(format!("{name}  ({addr})"), Act::JoinAddress(addr.clone())));
            }
            v.push(Item { label: "Or type an address".into(), kind: Kind::Field { key: "address".into(), value: focus.fields.get("address").cloned().unwrap_or_default() } });
            v.push(button("Join that address", Act::JoinAddress(focus.fields.get("address").cloned().unwrap_or_default())));
            if !focus.status.is_empty() {
                v.push(note(focus.status.clone()));
            }
            v.push(button("Back", Act::Back));
            ("Join a friend".into(), v)
        }
        Screen::Code => {
            let code = focus.fields.get("code").cloned().unwrap_or_default();
            let mut v = vec![note("Codes are 8 letters and numbers, like N8GT-3WFA.")];
            v.push(Item { label: "Code".into(), kind: Kind::Field { key: "code".into(), value: code.clone() } });
            v.push(button("Load it", Act::LoadCode(code)));
            if !focus.status.is_empty() {
                v.push(note(focus.status.clone()));
            }
            v.push(button("Back", Act::Back));
            ("Load a code".into(), v)
        }
        Screen::Library => {
            let mut v = vec![];
            let add = |title: &str, f: &dyn Fn(&crate::creations::Creation) -> bool, v: &mut Vec<Item>| {
                let list: Vec<_> = lib.list.values().filter(|c| f(c)).collect();
                if list.is_empty() {
                    return;
                }
                v.push(note(title.to_string()));
                for c in list {
                    let running = if lib.running.contains(&c.id) { "  ·  in this world" } else { "" };
                    v.push(button(format!("{} by {}{running}", c.manifest.name, c.manifest.by), Act::Inside(c.id.clone())));
                }
            };
            add("Made by me", &|c| c.origin == Origin::Mine, &mut v);
            add("From friends and codes", &|c| matches!(c.origin, Origin::Friend(_) | Origin::Code(_)), &mut v);
            add("Made by Potluck", &|c| c.origin == Origin::Base, &mut v);
            v.push(button("Back", Act::Back));
            ("Creations".into(), v)
        }
        Screen::Settings => {
            let v = vec![
                Item { label: "Your name".into(), kind: Kind::Field { key: "name".into(), value: focus.fields.get("name").cloned().unwrap_or_else(|| settings.name.clone()) } },
                Item { label: "Mouse speed".into(), kind: Kind::Slider { key: "mouse".into(), value: settings.mouse_sensitivity as f64, min: 0.2, max: 3.0 } },
                Item { label: "Stick speed".into(), kind: Kind::Slider { key: "stick".into(), value: settings.stick_sensitivity as f64, min: 0.2, max: 3.0 } },
                Item { label: "Field of view".into(), kind: Kind::Slider { key: "fov".into(), value: settings.fov as f64, min: 70.0, max: 110.0 } },
                Item { label: "Look up and down the other way".into(), kind: Kind::Toggle { key: "invert".into(), value: settings.invert_y } },
                Item { label: "Survival (health; blocks run out)".into(), kind: Kind::Toggle { key: "survival".into(), value: settings.survival } },
                Item { label: format!("Lighting: {} (after a restart)", cap(&settings.lighting)), kind: Kind::Button(Act::Lighting) },
                Item { label: "Game server".into(), kind: Kind::Field { key: "server".into(), value: focus.fields.get("server").cloned().unwrap_or_else(|| settings.server.clone()) } },
                button("Back", Act::Back),
            ];
            ("Settings".into(), v)
        }
    }
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub fn pretty_code(code: &str) -> String {
    if code.len() == 8 { format!("{}-{}", &code[..4], &code[4..]) } else { code.to_string() }
}

// ---------- menu input ----------

pub fn menu_input(
    c: Res<Controls>,
    mut focus: ResMut<Focus>,
    lib: Res<Library>,
    scripts: Res<Scripts>,
    mut settings: ResMut<Settings>,
    info: Res<WorldInfo>,
    mut acts: ResMut<Acts>,
    mut lib_mut: ResMut<crate::KnobEdits>,
    buttons: Query<(&Interaction, &MenuButton), Changed<Interaction>>,
    time: Res<Time>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut last_cursor: Local<Option<Vec2>>,
) {
    // a button drawn again under a resting pointer counts as hovered; only a
    // pointer that moved should take the selection from the keys or the pad
    let cursor = windows.single().ok().and_then(|w| w.cursor_position());
    let moved = cursor.is_some() && cursor != *last_cursor;
    *last_cursor = cursor;
    if focus.playing() {
        return;
    }
    let (_, items) = menu_items(&focus, &lib, &scripts, &settings, &info);
    let selectable: Vec<usize> = items.iter().enumerate().filter(|(_, i)| i.selectable()).map(|(n, _)| n).collect();
    if selectable.is_empty() {
        return;
    }
    let mut pos = selectable.iter().position(|&i| i == focus.sel).unwrap_or(0);
    // mouse
    let mut clicked = false;
    for (int, b) in &buttons {
        match int {
            Interaction::Pressed => {
                if let Some(p) = selectable.iter().position(|&i| i == b.0) {
                    pos = p;
                    clicked = true;
                }
            }
            Interaction::Hovered if moved => {
                if let Some(p) = selectable.iter().position(|&i| i == b.0) {
                    pos = p;
                }
            }
            Interaction::Hovered => {}
            Interaction::None => {}
        }
    }
    if c.down {
        pos = (pos + 1) % selectable.len();
    }
    if c.up {
        pos = (pos + selectable.len() - 1) % selectable.len();
    }
    focus.sel = selectable[pos];
    // scrolling the code in Look inside
    if matches!(focus.screen, Screen::Inside(_)) {
        focus.scroll = (focus.scroll + c.hotbar_step * 3).max(0);
    }
    let item = items[focus.sel].clone();
    let step = (c.right as i32 - c.left as i32) as f64;
    let accept = c.accept || clicked;
    match &item.kind {
        Kind::Button(act) => {
            if accept {
                acts.0.push(act.clone());
            }
        }
        Kind::Toggle { key, value } => {
            if accept || step != 0.0 {
                set_value(&focus, key, KnobValue::Toggle(!value), &mut settings, &mut lib_mut);
            }
        }
        Kind::Slider { key, value, min, max } => {
            let range = max - min;
            let mut v = *value + step * range / 20.0 + c.slide as f64 * range * time.delta_secs() as f64 * 0.6;
            if clicked {
                v = *value + range / 10.0;
                if v > *max + 1e-6 {
                    v = *min;
                }
            }
            let v = v.clamp(*min, *max);
            if (v - value).abs() > 1e-9 {
                let v = if range >= 5.0 { (v * 10.0).round() / 10.0 } else { (v * 100.0).round() / 100.0 };
                set_value(&focus, key, KnobValue::Number(v), &mut settings, &mut lib_mut);
            }
        }
        Kind::Swatch { key, value } => {
            if step != 0.0 || accept {
                let i = SWATCHES.iter().position(|s| s.eq_ignore_ascii_case(value)).map(|i| i as i32).unwrap_or(-1);
                let d = if step == 0.0 { 1 } else { step as i32 };
                let n = (i + d).rem_euclid(SWATCHES.len() as i32) as usize;
                set_value(&focus, key, KnobValue::Color(SWATCHES[n].into()), &mut settings, &mut lib_mut);
            }
        }
        Kind::Field { key, value } => {
            let mut v = value.clone();
            let mut changed = false;
            if !c.typed.is_empty() {
                v.push_str(&c.typed);
                changed = true;
            }
            if c.backspace {
                v.pop();
                changed = true;
            }
            if changed {
                let v: String = v.chars().take(64).collect();
                focus.fields.insert(key.clone(), v.clone());
                match key.as_str() {
                    "name" => settings.name = v.chars().filter(|c| c.is_alphanumeric() || *c == '_' || *c == ' ').take(20).collect(),
                    "server" => settings.server = v,
                    _ => {}
                }
            }
            if c.accept {
                // Enter in a field does the button after it
                if let Some(Item { kind: Kind::Button(a), .. }) = items.get(focus.sel + 1) {
                    let mut a = a.clone();
                    let v = focus.fields.get(key).cloned().unwrap_or_default();
                    match &mut a {
                        Act::JoinAddress(s) | Act::LoadCode(s) => *s = v,
                        _ => {}
                    }
                    acts.0.push(a);
                }
            }
        }
        _ => {}
    }
    if c.back && !matches!(item.kind, Kind::Field { .. } if c.backspace) {
        let back = match &focus.screen {
            Screen::Title => None,
            Screen::Pause => Some(Act::Resume),
            Screen::Tweak(id) | Screen::Share(id) => Some(Act::Inside(id.clone())),
            _ => Some(Act::Back),
        };
        if let Some(a) = back {
            acts.0.push(a);
        }
    }
}

fn set_value(focus: &Focus, key: &str, v: KnobValue, settings: &mut Settings, edits: &mut crate::KnobEdits) {
    if let Screen::Tweak(id) = &focus.screen {
        edits.0.push((id.clone(), key.to_string(), v));
        return;
    }
    match (key, v) {
        ("mouse", KnobValue::Number(n)) => settings.mouse_sensitivity = n as f32,
        ("stick", KnobValue::Number(n)) => settings.stick_sensitivity = n as f32,
        ("fov", KnobValue::Number(n)) => settings.fov = n.round() as f32,
        ("invert", KnobValue::Toggle(b)) => settings.invert_y = b,
        ("survival", KnobValue::Toggle(b)) => settings.survival = b,
        _ => {}
    }
    settings.save();
}

// ---------- drawing the menus ----------

#[derive(Component)]
pub struct MenuRoot;

#[derive(Component)]
pub struct MenuButton(pub usize);

pub fn draw_menu(
    mut commands: Commands,
    mut focus: ResMut<Focus>,
    lib: Res<Library>,
    scripts: Res<Scripts>,
    settings: Res<Settings>,
    info: Res<WorldInfo>,
    fonts: Res<Fonts>,
    roots: Query<Entity, With<MenuRoot>>,
) {
    let (title, items) = menu_items(&focus, &lib, &scripts, &settings, &info);
    let side = match &focus.screen {
        Screen::Inside(id) => lib.list.get(id).map(|c| {
            let src = c.files.get(&c.manifest.main).cloned().unwrap_or_default();
            let vm = scripts.vms.get(id);
            let log: Vec<String> = vm.map(|v| v.log.iter().rev().take(6).rev().cloned().collect()).unwrap_or_default();
            let err = vm.and_then(|v| v.error.clone());
            let ms = vm.map(|v| v.load_ms).unwrap_or(0.0);
            (c.manifest.main.clone(), src, log, err, ms)
        }),
        _ => None,
    };
    let sig = format!("{:?}|{title}|{items:?}|{}|{}|{:?}", focus.screen, focus.sel, focus.scroll, side);
    if sig == focus.sig {
        return;
    }
    focus.sig = sig;
    focus.items = items.clone();
    for e in &roots {
        commands.entity(e).despawn();
    }
    if focus.playing() {
        return;
    }
    let title_screen = focus.screen == Screen::Title;
    let ui = fonts.ui.clone();
    let tf = fonts.title.clone();
    let sel = focus.sel;
    let scroll = focus.scroll;
    commands
        .spawn((
            MenuRoot,
            Node {
                width: percent(100),
                height: percent(100),
                position_type: PositionType::Absolute,
                justify_content: if title_screen { JustifyContent::FlexStart } else { JustifyContent::Center },
                align_items: AlignItems::Center,
                padding: UiRect::all(px(40)),
                column_gap: px(24),
                ..default()
            },
            BackgroundColor(if title_screen { Color::NONE } else { Color::srgba(0.0, 0.0, 0.0, 0.35) }),
            GlobalZIndex(10),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    width: px(if side.is_some() { 400 } else { 520 }),
                    padding: UiRect::all(px(26)),
                    row_gap: px(10),
                    margin: if title_screen { UiRect::left(px(60)) } else { UiRect::ZERO },
                    border_radius: BorderRadius::all(px(18)),
                    ..default()
                },
                BackgroundColor(PANEL),
            ))
            .with_children(|p| {
                if !title.is_empty() {
                    p.spawn((Text::new(title.clone()), TextFont { font: tf.clone().into(), font_size: FontSize::Px(if title_screen { 64.0 } else { 34.0 }), weight: FontWeight(600), ..default() }, TextColor(if title_screen { GOLD } else { INK })));
                }
                for (i, item) in items.iter().enumerate() {
                    let picked = i == sel;
                    match &item.kind {
                        Kind::Note => {
                            if !item.label.is_empty() {
                                p.spawn((Text::new(item.label.clone()), TextFont { font: ui.clone().into(), font_size: FontSize::Px(16.0), ..default() }, TextColor(DIM)));
                            }
                        }
                        Kind::Big => {
                            p.spawn((Text::new(item.label.clone()), TextFont { font: tf.clone().into(), font_size: FontSize::Px(60.0), weight: FontWeight(600), ..default() }, TextColor(MINT)));
                        }
                        _ => {
                            // how full a slider's bar is (drawn, since the fonts have no block characters)
                            let bar = match &item.kind {
                                Kind::Slider { value, min, max, .. } => Some(((value - min) / (max - min)).clamp(0.0, 1.0) as f32),
                                _ => None,
                            };
                            let (label, right) = match &item.kind {
                                Kind::Slider { value, .. } => (item.label.clone(), trim_num(*value)),
                                Kind::Toggle { value, .. } => (item.label.clone(), if *value { "On".into() } else { "Off".into() }),
                                Kind::Swatch { value, .. } => (item.label.clone(), format!("< {value} >")),
                                Kind::Field { value, .. } => (item.label.clone(), format!("{value}{}", if picked { "|" } else { "" })),
                                _ => (item.label.clone(), String::new()),
                            };
                            let swatch = if let Kind::Swatch { value, .. } = &item.kind { crate::script::parse_color(value) } else { None };
                            p.spawn((
                                Interaction::default(),
                                MenuButton(i),
                                Node {
                                    padding: UiRect::axes(px(16), px(11)),
                                    justify_content: JustifyContent::SpaceBetween,
                                    align_items: AlignItems::Center,
                                    column_gap: px(12),
                                    border_radius: BorderRadius::all(px(12)),
                                    border: UiRect::all(px(2)),
                                    ..default()
                                },
                                BorderColor::all(if picked { MINT } else { Color::NONE }),
                                BackgroundColor(if picked { PICK } else { CARD }),
                            ))
                            .with_children(|b| {
                                b.spawn((Text::new(label), TextFont { font: ui.clone().into(), font_size: FontSize::Px(19.0), ..default() }, TextColor(INK)));
                                if let Some(col) = swatch {
                                    b.spawn((Node { width: px(56), height: px(22), border_radius: BorderRadius::all(px(6)), ..default() }, BackgroundColor(col)));
                                }
                                if let Some(f) = bar {
                                    b.spawn((
                                        Node { width: px(170), height: px(8), margin: UiRect::left(Val::Auto), border_radius: BorderRadius::all(px(4)), ..default() },
                                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.15)),
                                    ))
                                    .with_child((Node { width: percent(f * 100.0), height: percent(100), border_radius: BorderRadius::all(px(4)), ..default() }, BackgroundColor(MINT)));
                                }
                                if !right.is_empty() {
                                    let mut t = b.spawn((Text::new(right), TextFont { font: Handle::<Font>::default().into(), font_size: FontSize::Px(15.0), ..default() }, TextColor(MINT)));
                                    if bar.is_some() {
                                        // numbers take the same room, so the bars line up
                                        t.insert((Node { min_width: px(40), ..default() }, TextLayout::justify(Justify::Right)));
                                    }
                                }
                            });
                        }
                    }
                }
                if !title_screen {
                    p.spawn((Text::new("Up and down to choose · A or Enter to pick · B or Esc to go back"), TextFont { font: ui.clone().into(), font_size: FontSize::Px(12.0), ..default() }, TextColor(DIM)));
                }
            });
            // Look inside: the code, the log and any error
            if let Some((file, src, log, err, ms)) = side {
                root.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        width: px(720),
                        max_height: percent(92),
                        padding: UiRect::all(px(20)),
                        row_gap: px(6),
                        border_radius: BorderRadius::all(px(18)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.04, 0.05, 0.07, 0.92)),
                ))
                .with_children(|p| {
                    p.spawn((Text::new(format!("{file}  ·  loaded in {ms:.0} ms  ·  wheel or right stick to scroll")), TextFont { font: ui.clone().into(), font_size: FontSize::Px(13.0), ..default() }, TextColor(DIM)));
                    let lines: Vec<&str> = src.lines().collect();
                    let start = (scroll as usize).min(lines.len().saturating_sub(1));
                    let shown: String = lines.iter().enumerate().skip(start).take(30).map(|(n, l)| format!("{:>3}  {}\n", n + 1, l.replace('\t', "    "))).collect();
                    p.spawn((Text::new(shown), TextFont { font: Handle::<Font>::default().into(), font_size: FontSize::Px(14.0), ..default() }, TextColor(Color::srgb(0.85, 0.9, 0.86))));
                    if let Some(e) = err {
                        p.spawn((Text::new(format!("Problem: {e}")), TextFont { font: ui.clone().into(), font_size: FontSize::Px(15.0), ..default() }, TextColor(RED)));
                    }
                    if !log.is_empty() {
                        p.spawn((Text::new(format!("Log\n{}", log.join("\n"))), TextFont { font: Handle::<Font>::default().into(), font_size: FontSize::Px(13.0), ..default() }, TextColor(DIM)));
                    }
                });
            }
        });
}

fn trim_num(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

// ---------- the HUD ----------

#[derive(Component)]
pub struct Hud;
#[derive(Component)]
pub struct HotbarSlot(usize);
#[derive(Component)]
pub struct HotbarCount(usize);
#[derive(Component)]
pub struct Prompt;
#[derive(Component)]
pub struct ToastText;
#[derive(Component)]
pub struct ToastBox;
#[derive(Component)]
pub struct InfoText;
#[derive(Component)]
pub struct HealthBar;
#[derive(Component)]
pub struct Crosshair;
#[derive(Component)]
pub struct LabelText(usize);

pub fn spawn_hud(mut commands: Commands, fonts: Res<Fonts>) {
    let ui = fonts.ui.clone();
    commands
        .spawn((Hud, Node { width: percent(100), height: percent(100), position_type: PositionType::Absolute, ..default() }, Pickable::IGNORE))
        .with_children(|h| {
            h.spawn((
                Crosshair,
                Text::new("+"),
                TextFont { font: ui.clone().into(), font_size: FontSize::Px(28.0), ..default() },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
                Node { position_type: PositionType::Absolute, left: percent(50), top: percent(50), margin: UiRect { left: px(-8), top: px(-18), ..default() }, ..default() },
            ));
            h.spawn((
                InfoText,
                Text::new(""),
                TextFont { font: ui.clone().into(), font_size: FontSize::Px(15.0), ..default() },
                TextColor(DIM),
                Node { position_type: PositionType::Absolute, left: px(18), top: px(14), ..default() },
            ));
            // over menus too, so a share code shows while Look inside is open
            h.spawn((
                ToastBox,
                Node { position_type: PositionType::Absolute, top: px(40), width: percent(100), padding: UiRect::horizontal(percent(15)), justify_content: JustifyContent::Center, ..default() },
                GlobalZIndex(30),
                Pickable::IGNORE,
            ))
            .with_child((
                ToastText,
                Text::new(""),
                TextFont { font: ui.clone().into(), font_size: FontSize::Px(18.0), ..default() },
                TextColor(INK),
                TextLayout::justify(Justify::Center),
                Node { padding: UiRect::axes(px(16), px(8)), border_radius: BorderRadius::all(px(12)), ..default() },
                BackgroundColor(Color::srgba(0.07, 0.08, 0.1, 0.8)),
                Visibility::Hidden,
            ));
            h.spawn((
                Prompt,
                Text::new(""),
                TextFont { font: ui.clone().into(), font_size: FontSize::Px(17.0), ..default() },
                TextColor(INK),
                TextLayout::justify(Justify::Center),
                Node { position_type: PositionType::Absolute, bottom: px(96), width: percent(100), justify_content: JustifyContent::Center, ..default() },
            ));
            h.spawn((
                Node { position_type: PositionType::Absolute, bottom: px(84), left: percent(50), margin: UiRect::left(px(-240)), width: px(480), height: px(6), border_radius: BorderRadius::all(px(3)), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
            ))
            .with_children(|b| {
                b.spawn((HealthBar, Node { width: percent(100), height: percent(100), border_radius: BorderRadius::all(px(3)), ..default() }, BackgroundColor(RED)));
            });
            h.spawn(Node { position_type: PositionType::Absolute, bottom: px(16), width: percent(100), justify_content: JustifyContent::Center, column_gap: px(6), ..default() }).with_children(|bar| {
                for i in 0..9 {
                    bar.spawn((
                        HotbarSlot(i),
                        ImageNode::default(),
                        Node { width: px(56), height: px(56), border: UiRect::all(px(3)), border_radius: BorderRadius::all(px(10)), padding: UiRect::all(px(6)), justify_content: JustifyContent::FlexEnd, align_items: AlignItems::FlexEnd, ..default() },
                        BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.5)),
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
                    ))
                    .with_children(|s| {
                        s.spawn((HotbarCount(i), Text::new(""), TextFont { font: ui.clone().into(), font_size: FontSize::Px(14.0), ..default() }, TextColor(INK)));
                    });
                }
            });
            for i in 0..24 {
                h.spawn((
                    LabelText(i),
                    Text::new(""),
                    TextFont { font: ui.clone().into(), font_size: FontSize::Px(15.0), ..default() },
                    TextColor(INK),
                    TextLayout::justify(Justify::Center),
                    Node { position_type: PositionType::Absolute, padding: UiRect::axes(px(8), px(3)), border_radius: BorderRadius::all(px(8)), ..default() },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
                    Visibility::Hidden,
                ));
            }
        });
}

/// Things in the world with a label above them: (where, text).
#[derive(Resource, Default)]
pub struct Labels(pub Vec<(Vec3, String)>);

#[allow(clippy::too_many_arguments)]
pub fn update_hud(
    me: Res<Me>,
    focus: Res<Focus>,
    c: Res<Controls>,
    settings: Res<Settings>,
    herd: Res<Herd>,
    lib: Res<Library>,
    atlas: Res<BlockAtlas>,
    info: Res<WorldInfo>,
    labels: Res<Labels>,
    time: Res<Time>,
    mut toasts: ResMut<Toasts>,
    mut hud: Query<&mut Visibility, (With<Hud>, Without<LabelText>)>,
    mut slots: Query<(&HotbarSlot, &mut ImageNode, &mut BorderColor)>,
    mut texts: ParamSet<(
        Query<(&HotbarCount, &mut Text)>,
        Query<&mut Text, With<Prompt>>,
        Query<(&mut Text, &mut Visibility), (With<ToastText>, Without<Hud>)>,
        Query<&mut Text, With<InfoText>>,
        Query<(&LabelText, &mut Text, &mut Node, &mut Visibility)>,
    )>,
    mut health: Query<(&mut Node, &mut BackgroundColor), (With<HealthBar>, Without<LabelText>)>,
    cam: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
) {
    let show = focus.screen != Screen::Title && info.in_world;
    for mut v in &mut hud {
        *v = if show { Visibility::Inherited } else { Visibility::Hidden };
    }
    for (s, mut img, mut border) in &mut slots {
        let id = me.hotbar[s.0];
        let empty = settings.survival && me.counts[s.0] == 0;
        img.image = if empty { Handle::default() } else { atlas.icons.get(blocks::block(id).faces[1]).cloned().unwrap_or_default() };
        img.color = if empty { Color::NONE } else { Color::WHITE };
        *border = BorderColor::all(if s.0 == me.slot { Color::srgb(1.0, 1.0, 1.0) } else { Color::srgba(0.0, 0.0, 0.0, 0.5) });
    }
    for (s, mut t) in &mut texts.p0() {
        let n = me.counts[s.0];
        t.0 = if settings.survival && n > 0 { n.to_string() } else { String::new() };
    }
    // what pressing would do
    let pad = c.pad;
    let mut prompt = String::new();
    if let Some(id) = me.riding {
        let kind = herd.bodies.get(&id).map(|b| b.kind.clone()).unwrap_or_default();
        prompt = format!("Riding {kind}  ·  {} or {} to get off", glyph(pad, "interact"), if pad { "B" } else { "Shift" });
    } else if let Some(b) = me.aim_body.and_then(|id| herd.bodies.get(&id)) {
        let name = lib.list.get(&b.creation).map(|c| format!("{} by {}", c.manifest.name, c.manifest.by)).unwrap_or(b.kind.clone());
        let verb = if b.rideable && b.rider.is_none() { "Ride" } else { "Use" };
        prompt = format!("{name}\n{} {verb}   {} Look inside", glyph(pad, "interact"), glyph(pad, "inside"));
    } else if let Some(hit) = me.target {
        let b = blocks::block(hit.id);
        prompt = b.name.to_string();
        if let Some((_, p)) = me.breaking {
            prompt = format!("{}  {:.0}%", b.name, p * 100.0);
        }
    }
    if focus.screen != Screen::Playing {
        // a menu is open: what pressing would do in the world doesn't apply
        prompt.clear();
    }
    if let Ok(mut t) = texts.p1().single_mut() {
        t.0 = prompt;
    }
    for t in &mut toasts.0 {
        t.1 -= time.delta_secs();
    }
    toasts.0.retain(|t| t.1 > 0.0);
    if let Ok((mut t, mut v)) = texts.p2().single_mut() {
        t.0 = toasts.0.iter().map(|t| t.0.clone()).collect::<Vec<_>>().join("\n");
        *v = if t.0.is_empty() { Visibility::Hidden } else { Visibility::Inherited };
    }
    if let Ok(mut t) = texts.p3().single_mut() {
        t.0 = info.line.clone();
    }
    if let Ok((mut n, mut bg)) = health.single_mut() {
        n.width = percent((me.health / MAX_HEALTH * 100.0).max(0.0));
        let flash = (me.hurt < 0.4) as i32 as f32;
        bg.0 = if settings.survival { RED.mix(&INK, flash) } else { Color::NONE };
    }
    // labels over creatures and players
    let Ok((camera, ct)) = cam.single() else { return };
    let mut q = texts.p4();
    let mut list: Vec<(Vec2, &String)> = vec![];
    for (p, text) in &labels.0 {
        if (ct.translation() - *p).length() > 40.0 {
            continue;
        }
        if let Ok(v) = camera.world_to_viewport(ct, *p) {
            list.push((v, text));
        }
    }
    for (l, mut t, mut node, mut vis) in &mut q {
        if let Some((v, text)) = list.get(l.0).filter(|_| show) {
            t.0 = (*text).clone();
            node.left = px(v.x - 4.0 * text.len().min(30) as f32);
            node.top = px(v.y - 14.0);
            *vis = Visibility::Inherited;
        } else {
            *vis = Visibility::Hidden;
        }
    }
}

/// Grabs the mouse while playing, and lets it go in menus. It asks again
/// only when play starts, the window gets focus or a click lands in it: some
/// desktops refuse a grab until then, and asking every frame fills the log.
pub fn cursor(
    focus: Res<Focus>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut windows: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut was: Local<(bool, bool)>,
) {
    let Ok((window, mut opts)) = windows.single_mut() else { return };
    let grab = focus.playing();
    let ask = grab != was.0 || (window.focused && !was.1) || mouse.just_pressed(MouseButton::Left);
    *was = (grab, window.focused);
    let mode = if grab { CursorGrabMode::Locked } else { CursorGrabMode::None };
    if opts.grab_mode != mode && ask {
        opts.grab_mode = mode;
        opts.visible = !grab;
    }
}

/// A white outline round the block in the crosshair.
pub fn outline(me: Res<Me>, focus: Res<Focus>, mut gizmos: Gizmos) {
    if !focus.playing() || me.aim_body.is_some() {
        return;
    }
    if let Some(hit) = me.target {
        let c = hit.block.as_vec3() + Vec3::splat(0.5);
        gizmos.cube(Transform::from_translation(c).with_scale(Vec3::splat(1.004)), Color::srgba(1.0, 1.0, 1.0, 0.6));
    }
}

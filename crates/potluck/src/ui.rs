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

pub mod draw;
pub use draw::draw_menu;

pub const INK: Color = Color::srgb(0.98, 0.96, 0.9);
pub const DIM: Color = Color::srgba(0.98, 0.96, 0.9, 0.62);
pub const MINT: Color = Color::srgb(0.37, 0.88, 0.78);
pub const GOLD: Color = Color::srgb(1.0, 0.77, 0.36);
pub const RED: Color = Color::srgb(1.0, 0.45, 0.42);

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
    Servers,
    Code,
    Library,
    Settings,
    /// Suggesting a remix's change to the creation it came from.
    Suggest(String),
    /// The changes others suggested to one of mine.
    Suggestions(String),
    Report(String),
    /// A sign-in link to the market, as a QR code.
    Link,
    /// Playing as the same account on another device.
    Devices,
    /// Who's playing, asked once before the first thing that goes online;
    /// then the act that asked.
    Age(Box<Act>),
    /// What a parent decides, behind their PIN.
    Parents,
    /// Creations' actions, and the buttons they're on.
    Controls,
    /// A creation's question (`ui.ask`), with its choices.
    Panel,
    /// The whole map, with its list on the right (`map`).
    Map,
    /// One waypoint, on the map.
    Waypoint(u32),
    /// A place someone shared in chat, by the line's id, on the map.
    Shared(u64),
    /// Saying something, and the last lines (`chat`).
    Chat,
    /// Every block, to put one in your hand (creative).
    Blocks,
    /// The worlds the player keeps, to play (or host, when true).
    Worlds(bool),
    /// One kept world: rename it or delete it.
    Forget(String),
    /// What the player carries: the backpack and the hotbar (survival).
    Backpack,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Resume,
    /// Into the world as it is (the title's own, which isn't kept): `--play`.
    PlayAlone,
    /// The kept worlds, to play alone or (true) to host for friends.
    OpenWorlds(bool),
    /// A new kept world, played (or hosted) straight away.
    NewWorld(bool),
    PlayWorld(String, bool),
    /// A kept world's own page: rename it, delete it.
    OpenForget(String),
    ForgetWorld(String),
    RenameWorld(String),
    Host,
    JoinAddress(String),
    OpenJoin,
    OpenServers,
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
    /// Share to the market: to Everyone (true) or by code only.
    ShareTo(String, bool),
    Suggest(String),
    SendSuggestion(String),
    OpenSuggestions(String),
    /// Load a suggestion's remix (code, version) to try it.
    TrySuggestion(String, u32),
    Merge(String, i64),
    CloseSuggestion(String, i64),
    Report(String),
    SendReport(String, String),
    /// A sign-in link to a market page (path, what it's for).
    Market(String, String),
    OpenUrl(String),
    OpenDevices,
    /// A code for another device to type.
    MakeLinkCode,
    /// Play as the account a link or recovery code belongs to.
    LinkDevice(String),
    UnlinkDevice,
    /// Under 13 (true) or not, from the Age screen.
    SetAge(bool),
    OpenParents,
    UnlockParents(String),
    SetPin(String),
    /// Interact at one of the Green's arches (0 to 3, west to east).
    Arch(usize),
    /// Put a creation shared to Everyone in the jam running now.
    EnterJam(String),
    OpenControls,
    /// Wait for the next button or key for a creation's action.
    Rebind(String, String),
    /// Every creation's actions back on the buttons it chose.
    ResetControls,
    /// An answer to the creation's question on the screen: a choice from 0,
    /// or None for Close.
    Answer(Option<u32>),
    OpenMap,
    /// Go to the spot picked on the map.
    MapGo,
    /// The map back on where you are.
    MapHome,
    /// The map's list of waypoints, a page further.
    MapPage,
    /// A waypoint where you stand, or at the spot picked on the map.
    MarkHere,
    MarkCursor,
    /// The minimap nearer, further or off.
    MinimapZoom,
    OpenWaypoint(u32),
    GoWaypoint(u32),
    RenameWaypoint(u32, String),
    RecolourWaypoint(u32),
    /// Show a waypoint on the minimap and over the world, or not.
    ShowWaypoint(u32),
    /// Share a waypoint in chat, for others' maps.
    ShareWaypoint(u32),
    DeleteWaypoint(u32),
    /// A place shared in chat, by the line's id: look at it, keep it as a
    /// waypoint, go there.
    OpenShared(u64),
    KeepShared(u64),
    GoShared(u64),
    /// Go to a friend in this world.
    GoPlayer(u32),
    OpenChat,
    /// Say something in chat.
    Say(String),
    /// Share where you are in chat.
    SharePlace,
    /// Every block to choose from (creative); in survival, the backpack.
    OpenBlocks,
    /// Put a block in your hand from Blocks.
    Hold(blocks::BlockId),
    /// On the backpack: choose a slot to move, or where the chosen one
    /// goes (true for half of it).
    Slot(usize, bool),
    /// Put the backpack in order.
    Tidy,
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
    /// A QR code of the label, for a phone's camera.
    Qr,
    /// A small button that sits in a row with the ones beside it.
    Chip(Act),
    /// A line of chat: the label is who said it, in their colour.
    Line { color: [u8; 3], words: String },
    /// A creation on the Creations sheet, as a card with its picture; the
    /// act is what A does (Look inside).
    Card(Act),
    /// One of a few choices, turned with left and right (the act moves it on).
    Choice { act: Act, value: String },
    /// A block on the Blocks sheet, as its picture; A puts it in your hand.
    Tile(blocks::BlockId),
    /// A slot of the pockets on the backpack (`items::POCKETS`): A moves
    /// what's in it, X half of it.
    Slot(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub label: String,
    pub kind: Kind,
}

pub fn button(label: impl Into<String>, act: Act) -> Item {
    Item { label: label.into(), kind: Kind::Button(act) }
}
pub fn note(label: impl Into<String>) -> Item {
    Item { label: label.into(), kind: Kind::Note }
}
pub fn toggle(label: impl Into<String>, key: &str, value: bool) -> Item {
    Item { label: label.into(), kind: Kind::Toggle { key: key.into(), value } }
}
pub fn field(label: impl Into<String>, key: &str, value: String) -> Item {
    Item { label: label.into(), kind: Kind::Field { key: key.into(), value } }
}

impl Item {
    fn selectable(&self) -> bool {
        !matches!(self.kind, Kind::Note | Kind::Big | Kind::Qr | Kind::Line { .. })
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
    /// Suggested changes to the creation on the Suggestions screen.
    pub suggestions: Vec<crate::share::Suggestion>,
    /// The market link on the Link screen: (address, what it's for).
    pub link: Option<(String, String)>,
    /// Servers listed on the market.
    pub servers: Vec<crate::share::Listed>,
    /// The kept worlds, as the Worlds screen lists them.
    pub worlds: Vec<crate::worlds::Kept>,
    /// What the player carries, as the backpack shows it (`game::pockets`
    /// copies it in while it's open).
    pub pockets: crate::items::Slots,
    /// The slot being moved on the backpack, and whether only half of it.
    pub moving: Option<(usize, bool)>,
    /// Who this device plays as on the market, once asked.
    pub account: Option<crate::share::Account>,
    /// The jam running now, once the market's said.
    pub jam: Option<crate::share::JamNow>,
    /// A creation's action waiting for its new button: (creation, action).
    pub rebinding: Option<(String, String)>,
    /// A code for another device to type, on the Devices screen.
    pub link_code: Option<String>,
    /// Parent settings are unlocked until the screen is left.
    pub parents_open: bool,
    /// Wrong PINs lately: a few in a row wait a while.
    pub pin_misses: Vec<std::time::Instant>,
    /// The creation's question on the Panel screen.
    pub panel: Option<crate::script::Panel>,
    /// What Settings says under the dev bridge: its address, or why it
    /// couldn't start.
    pub bridge: Option<String>,
    /// The keyboard on the screen, when a pad is typing into a field.
    pub keyboard: Option<crate::keyboard::Keys>,
    /// The title and items of the screens other modules make: the map's
    /// (`map::fill`) and chat's (`chat::fill`).
    pub made: (String, Vec<Item>),
    /// The page of Settings open (`SETTINGS_PAGES`).
    pub settings_page: usize,
    /// The shelf of Creations open (`shelves`).
    pub shelf: usize,
    /// The page of Blocks open (`BLOCK_PAGES`).
    pub blocks_page: usize,
    /// How many cards a row of the Creations sheet holds, as last drawn.
    pub cols: usize,
    /// The first item drawn, when a menu is taller than the window.
    first: usize,
    qr: Option<(String, Handle<Image>)>,
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
            suggestions: vec![],
            link: None,
            servers: vec![],
            worlds: vec![],
            pockets: Default::default(),
            moving: None,
            account: None,
            jam: None,
            rebinding: None,
            link_code: None,
            parents_open: false,
            pin_misses: vec![],
            panel: None,
            bridge: None,
            keyboard: None,
            made: (String::new(), vec![]),
            settings_page: 0,
            shelf: 0,
            blocks_page: 0,
            cols: 4,
            first: 0,
            qr: None,
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
    /// The page open on a screen with pages, to change.
    fn page_mut(&mut self) -> Option<&mut usize> {
        match self.screen {
            Screen::Settings => Some(&mut self.settings_page),
            Screen::Library => Some(&mut self.shelf),
            Screen::Blocks => Some(&mut self.blocks_page),
            _ => None,
        }
    }
}

/// Settings' pages, by what they're about.
pub const SETTINGS_PAGES: &[&str] = &["You", "Controls", "Picture and sound", "Online", "Family"];

/// The shelves of the Creations sheet: what's in this world only in one.
pub fn shelves(in_world: bool) -> &'static [&'static str] {
    if in_world {
        &["Everything", "In this world", "Made by me", "From friends", "Made by Potluck"]
    } else {
        &["Everything", "Made by me", "From friends", "Made by Potluck"]
    }
}

/// The pages of Blocks.
pub const BLOCK_PAGES: &[&str] = &["Building", "Nature", "Things", "From creations"];

/// The blocks on a page of Blocks: the game's own by what they're for, or
/// those creations here made. Never air or bedrock.
pub fn block_shelf(page: &str) -> Vec<blocks::BlockId> {
    use blocks::Shape;
    if page == "From creations" {
        return crate::made::table().iter().map(|e| e.id).collect();
    }
    let ground = [blocks::GRASS, blocks::DIRT, blocks::SAND, blocks::GRAVEL, blocks::SNOW, blocks::FARMLAND];
    (1..blocks::BLOCKS.len() as blocks::BlockId)
        .filter(|&id| {
            let b = blocks::block(id);
            b.shape != Shape::Air && id != blocks::BEDROCK
        })
        .filter(|&id| {
            let b = blocks::block(id);
            let on = if b.shape == Shape::Model || b.light {
                "Things"
            } else if matches!(b.shape, Shape::Cross | Shape::Water) || b.shape == Shape::Cutout && id != blocks::GLASS || ground.contains(&id) || (blocks::COAL_ORE..=blocks::MOONSILVER_ORE).contains(&id) {
                "Nature"
            } else {
                "Building"
            };
            on == page
        })
        .collect()
}

/// A screen's pages (LB and RB, or Tab), and the one open.
pub fn pages(focus: &Focus, info: &WorldInfo) -> Option<(&'static [&'static str], usize)> {
    match focus.screen {
        Screen::Settings => Some((SETTINGS_PAGES, focus.settings_page.min(SETTINGS_PAGES.len() - 1))),
        Screen::Library => {
            let list = shelves(info.in_world);
            Some((list, focus.shelf.min(list.len() - 1)))
        }
        Screen::Blocks => Some((BLOCK_PAGES, focus.blocks_page.min(BLOCK_PAGES.len() - 1))),
        _ => None,
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
    /// Small text: hints, descriptions, the HUD (Barlow Medium).
    pub ui: Handle<Font>,
    /// Menu lines, tabs and names (Barlow SemiBold).
    pub lines: Handle<Font>,
    /// Screen titles and the little capitals over them (Bungee, which is all capitals).
    pub heading: Handle<Font>,
    pub code: Handle<Font>,
    /// The game's name as a picture for the title screen: Bungee in the game's dirt and grass (tools/title.py).
    pub logo: Handle<Image>,
}

pub fn fonts(mut fonts: ResMut<Assets<Font>>, mut images: ResMut<Assets<Image>>, mut commands: Commands) {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
    let ui = fonts.add(Font::from_bytes(include_bytes!("../../../assets/fonts/Barlow-Medium.ttf").to_vec()));
    let lines = fonts.add(Font::from_bytes(include_bytes!("../../../assets/fonts/Barlow-SemiBold.ttf").to_vec()));
    let heading = fonts.add(Font::from_bytes(include_bytes!("../../../assets/fonts/Bungee-Regular.ttf").to_vec()));
    let png = include_bytes!("../../../assets/title/potluck.png");
    let logo = Image::from_buffer(png, ImageType::Extension("png"), CompressedImageFormats::NONE, true, ImageSampler::linear(), RenderAssetUsages::RENDER_WORLD)
        .map(|img| images.add(img))
        .unwrap_or_default();
    commands.insert_resource(Fonts { ui, lines, heading, code: Handle::default(), logo });
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
                button("Play alone", Act::OpenWorlds(false)),
                button("Host a world for friends", Act::OpenWorlds(true)),
                button("Join a friend's world", Act::OpenJoin),
                button("Load a creation from a code", Act::OpenCode),
                button("Creations", Act::OpenLibrary),
                button("The market", Act::Market("/".into(), "The market".into())),
                button("Settings", Act::OpenSettings),
                button("Quit", Act::Quit),
            ],
        ),
        Screen::Playing => (String::new(), vec![]),
        Screen::Map | Screen::Waypoint(_) | Screen::Shared(_) | Screen::Chat => focus.made.clone(),
        Screen::Pause => {
            let mut v = vec![note(info.line.clone()), button("Back to the world", Act::Resume), button("Map", Act::OpenMap)];
            if settings.chat {
                v.push(button("Chat", Act::OpenChat));
            }
            v.extend([button("Creations in this world", Act::OpenLibrary), button("Load a creation from a code", Act::OpenCode)]);
            v.push(button("The market", Act::Market("/".into(), "The market".into())));
            if let Some(j) = &focus.jam {
                v.push(button(format!("This week's jam: {}", j.theme), Act::Market(format!("/jams/{}", j.slug), j.theme.clone())));
            }
            v.push(button("Settings", Act::OpenSettings));
            v.push(button("Leave this world", Act::Leave));
            ("Paused".into(), v)
        }
        Screen::Inside(id) => {
            let Some(c) = lib.list.get(id) else { return ("Gone".into(), vec![button("Back", Act::Back)]) };
            let m = &c.manifest;
            let mut v = vec![note(format!("{} by {}", kind_name(&m.kind), m.by))];
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
            let mark = lib.marks.get(id).filter(|m| m.version > 0);
            if c.editable() && settings.sharing {
                v.push(button(if mark.is_some() { "Share a new version" } else { "Share" }, Act::Share(id.clone())));
                if !settings.young && m.from.as_ref().is_some_and(|f| f.code.is_some() || f.base.is_some()) {
                    v.push(button("Suggest my change to the original", Act::Suggest(id.clone())));
                }
                if mark.is_some() {
                    v.push(button("Changes others suggested", Act::OpenSuggestions(id.clone())));
                }
            }
            if let Some(mk) = mark {
                v.push(button("See it on the market", Act::Market(format!("/c/{}", mk.code), m.name.clone())));
            }
            if !c.editable() && matches!(c.origin, Origin::Code(_)) {
                v.push(button("Report it", Act::Report(id.clone())));
            }
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
            let c = lib.list.get(id);
            let name = c.map(|c| c.manifest.name.clone()).unwrap_or_default();
            let mut v = vec![];
            if let Some(code) = c.and_then(|c| c.code.clone()) {
                v.push(Item { label: pretty_code(&code), kind: Kind::Big });
                if lib.marks.get(id).is_some_and(|m| m.version > 0) {
                    v.push(note(format!("{}/c/{code}", host(&settings.market))));
                }
                v.push(note("Say it or type it: anyone can load it from Load a creation from a code."));
            }
            if !settings.sharing {
                v.push(note("A parent turned sharing off. It's under Settings, Parent settings."));
            } else if c.is_some_and(|c| c.editable()) {
                if !settings.young {
                    let everyone = lib.marks.get(id).is_some_and(|m| m.everyone);
                    v.push(button(if everyone { "Share this version to Everyone" } else { "Share to Everyone" }, Act::ShareTo(id.clone(), true)));
                    v.push(note("On the market for anyone to find, once a moderator has looked at it."));
                }
                if let Some(j) = focus.jam.as_ref().filter(|_| !settings.young) {
                    if lib.marks.get(id).is_some_and(|m| m.everyone && m.version > 0) {
                        v.push(button(format!("Enter it in this week's jam: {}", j.theme), Act::EnterJam(id.clone())));
                        v.push(note("Stars it gets until the jam ends count. The four with the most go on the Green of every new world for a month."));
                    } else {
                        v.push(note(format!("This week's jam is \u{201c}{}\u{201d}: share it to Everyone to enter it.", j.theme)));
                    }
                }
                v.push(button("Share by code only", Act::ShareTo(id.clone(), false)));
                v.push(note(if settings.young { "Give the code to your friends: only people who have it can load it." } else { "Only people you give the code to can load it." }));
            } else {
                v.push(note("Remix it to share a version of your own."));
            }
            v.push(note(focus.status.clone()));
            v.push(button("Back", Act::Inside(id.clone())));
            (format!("Share {name}"), v)
        }
        Screen::Suggest(id) => {
            let orig = lib.list.get(id).and_then(|c| c.manifest.remix_of.clone()).unwrap_or_else(|| "the original".into());
            let v = vec![
                note(format!("Your remix goes to the maker of {orig} as a suggestion. If they like it, they merge it into theirs, with your name on the change.")),
                Item { label: "What does it change?".into(), kind: Kind::Field { key: "suggest".into(), value: focus.fields.get("suggest").cloned().unwrap_or_default() } },
                button("Send it", Act::SendSuggestion(id.clone())),
                note(focus.status.clone()),
                button("Back", Act::Inside(id.clone())),
            ];
            ("Suggest a change".into(), v)
        }
        Screen::Suggestions(id) => {
            let mut v = vec![];
            if focus.suggestions.is_empty() && focus.status.is_empty() {
                v.push(note("Nothing yet. When someone remixes it and suggests their change, it shows up here."));
            }
            for s in &focus.suggestions {
                v.push(note(format!("\u{201c}{}\u{201d} from {}, in their {}", s.title, s.by, s.from_name)));
                v.push(button("Try it in this world", Act::TrySuggestion(s.from_code.clone(), s.from_version)));
                v.push(button("Merge it into mine", Act::Merge(id.clone(), s.id)));
                v.push(button("No thanks", Act::CloseSuggestion(id.clone(), s.id)));
            }
            v.push(note(focus.status.clone()));
            v.push(button("Back", Act::Inside(id.clone())));
            ("Suggested changes".into(), v)
        }
        Screen::Report(id) => {
            let mut v = vec![note("What's wrong with it? A moderator looks at every report, and enough of them hide it until then.")];
            for (reason, label) in [("unkind", "It's unkind or rude"), ("unsafe", "It isn't safe"), ("broken", "It's broken"), ("copied", "It's copied from someone"), ("other", "Something else")] {
                v.push(button(label, Act::SendReport(id.clone(), reason.into())));
            }
            v.push(button("Back", Act::Inside(id.clone())));
            ("Report it".into(), v)
        }
        Screen::Link => {
            let mut v = vec![];
            let title = match &focus.link {
                Some((url, title)) => {
                    v.push(Item { label: url.clone(), kind: Kind::Qr });
                    v.push(note("Point a phone's camera at it to carry on there, signed in as you. It works once, in the next five minutes."));
                    v.push(button("Open it on this screen", Act::OpenUrl(url.clone())));
                    title.clone()
                }
                None => {
                    v.push(note(if focus.status.is_empty() { "Asking the market for a link...".to_string() } else { focus.status.clone() }));
                    "The market".into()
                }
            };
            v.push(button("Back", Act::Back));
            (title, v)
        }
        Screen::Devices => {
            let mut v = vec![];
            match &focus.account {
                Some(a) if a.linked => v.push(note(format!("This device plays as {} (@{}), linked from another device.", a.name, a.handle))),
                Some(a) => v.push(note(format!("This device plays as {} (@{}).", a.name, a.handle))),
                None => {}
            }
            v.push(note("Play as you on another device too: your name, your block head and what you've shared come along."));
            if let Some(code) = &focus.link_code {
                v.push(Item { label: code.clone(), kind: Kind::Big });
                v.push(note("On the other device, choose Settings, then Link a device, and type this in the next ten minutes. It works once."));
            }
            v.push(button(if focus.link_code.is_some() { "Make another code" } else { "Make a code for another device" }, Act::MakeLinkCode));
            let typed = focus.fields.get("link").cloned().unwrap_or_default();
            v.push(Item { label: "A code from another device, or a recovery code".into(), kind: Kind::Field { key: "link".into(), value: typed.clone() } });
            v.push(button("Play as that account here", Act::LinkDevice(typed)));
            if focus.account.as_ref().is_some_and(|a| a.linked) {
                v.push(button("Unlink this device", Act::UnlinkDevice));
            }
            v.push(button("Your devices and recovery code on the market", Act::Market("/me/devices".into(), "Your devices".into())));
            if !focus.status.is_empty() {
                v.push(note(focus.status.clone()));
            }
            v.push(button("Back", Act::OpenSettings));
            ("Link a device".into(), v)
        }
        Screen::Age(_) => (
            "Who's playing?".into(),
            vec![
                note("Younger players share with friends, by code, and play on servers open to all ages. A parent can change it later in Settings, Parent settings."),
                button("Under 13", Act::SetAge(true)),
                button("13 or older", Act::SetAge(false)),
                button("Back", Act::Back),
            ],
        ),
        Screen::Parents => {
            let mut v = vec![];
            if !focus.parents_open {
                v.push(note("These are for a parent or carer. Type the PIN to change them."));
                let pin = focus.fields.get("pin").cloned().unwrap_or_default();
                v.push(Item { label: "PIN".into(), kind: Kind::Field { key: "pin".into(), value: pin.clone() } });
                v.push(button("Unlock", Act::UnlockParents(pin)));
            } else {
                v.push(Item { label: "Under 13: shares by code, with friends, and plays on all-ages servers".into(), kind: Kind::Toggle { key: "young".into(), value: settings.young } });
                v.push(Item { label: "Can share creations".into(), kind: Kind::Toggle { key: "sharing".into(), value: settings.sharing } });
                v.push(Item { label: "Can join servers from the market's list".into(), kind: Kind::Toggle { key: "servers".into(), value: settings.servers } });
                v.push(toggle("Can chat in a world", "chat", settings.chat));
                let pin = focus.fields.get("newpin").cloned().unwrap_or_default();
                let label = if settings.parent_pin.is_empty() { "A PIN, so only you can change these" } else { "A new PIN" };
                v.push(Item { label: label.into(), kind: Kind::Field { key: "newpin".into(), value: pin.clone() } });
                v.push(button(if settings.parent_pin.is_empty() { "Set the PIN" } else { "Change the PIN" }, Act::SetPin(pin)));
            }
            if !focus.status.is_empty() {
                v.push(note(focus.status.clone()));
            }
            v.push(button("Back", Act::OpenSettings));
            ("Parent settings".into(), v)
        }
        Screen::Join => {
            let mut v = vec![];
            if focus.lan.is_empty() {
                v.push(note("Looking for worlds on this network..."));
            }
            for (name, addr) in &focus.lan {
                v.push(button(format!("{name}  ({addr})"), Act::JoinAddress(addr.clone())));
            }
            v.push(button("Servers on the market", Act::OpenServers));
            v.push(Item { label: "Or type an address".into(), kind: Kind::Field { key: "address".into(), value: focus.fields.get("address").cloned().unwrap_or_default() } });
            v.push(button("Join that address", Act::JoinAddress(focus.fields.get("address").cloned().unwrap_or_default())));
            if !focus.status.is_empty() {
                v.push(note(focus.status.clone()));
            }
            v.push(button("Back", Act::Back));
            ("Join a friend".into(), v)
        }
        Screen::Servers => {
            let mut v = vec![];
            if !settings.servers {
                v.push(note("A parent turned off servers from the market's list. Friends' worlds still work, from Join a friend."));
                v.push(button("Back", Act::Back));
                return ("Servers".into(), v);
            }
            if settings.young {
                v.push(note("Servers open to players of all ages."));
            }
            for s in focus.servers.iter().filter(|s| s.all_ages || !settings.young) {
                let full = s.max_players > 0 && s.players >= s.max_players;
                let label = format!("{}  ·  {} of {} here{}", s.name, s.players, s.max_players, if full { ", full" } else { "" });
                v.push(button(label, Act::JoinAddress(s.address.clone())));
                let mut about = s.about.clone();
                if !s.creations.is_empty() {
                    if !about.is_empty() {
                        about.push_str("  ·  ");
                    }
                    about.push_str(&format!("Runs {}", s.creations.join(", ")));
                }
                if s.join == "signed-in" || s.join == "list" {
                    if !about.is_empty() {
                        about.push_str("  ·  ");
                    }
                    about.push_str(if s.join == "list" { "Invited players only" } else { "Signed-in players only" });
                }
                if !about.is_empty() {
                    v.push(note(about));
                }
            }
            if focus.servers.is_empty() && focus.status.is_empty() {
                v.push(note("Asking the market..."));
            }
            if !focus.status.is_empty() {
                v.push(note(focus.status.clone()));
            }
            v.push(button("Back", Act::Back));
            ("Servers".into(), v)
        }
        Screen::Worlds(host) => {
            let host = *host;
            let mut v = vec![];
            if host {
                v.push(note("Friends on this network can join, or anyone you give the address to."));
            }
            v.push(button("A new world", Act::NewWorld(host)));
            for k in &focus.worlds {
                v.push(button(format!("{}  \u{b7}  {}", k.name, crate::worlds::ago(k.played)), Act::PlayWorld(k.id.clone(), host)));
            }
            if !focus.worlds.is_empty() {
                v.push(note("Each world saves as you play."));
                v.push(button("Rename or delete a world", Act::OpenForget(String::new())));
            }
            if !focus.status.is_empty() {
                v.push(note(focus.status.clone()));
            }
            v.push(button("Back", Act::Back));
            ((if host { "Host a world" } else { "Your worlds" }).into(), v)
        }
        Screen::Forget(id) if id.is_empty() => {
            let mut v: Vec<Item> = focus.worlds.iter().map(|k| button(k.name.clone(), Act::OpenForget(k.id.clone()))).collect();
            v.push(button("Back", Act::OpenWorlds(false)));
            ("Which world?".into(), v)
        }
        Screen::Forget(id) => {
            let Some(k) = focus.worlds.iter().find(|k| k.id == *id) else { return ("Gone".into(), vec![button("Back", Act::OpenWorlds(false))]) };
            let name = focus.fields.get("world_name").cloned().unwrap_or_else(|| k.name.clone());
            let mut v = vec![field("Name", "world_name", name), button("Rename it", Act::RenameWorld(id.clone()))];
            v.push(note("Deleting a world is for good: everything built in it goes too."));
            v.push(button(format!("Delete {} for good", k.name), Act::ForgetWorld(id.clone())));
            if !focus.status.is_empty() {
                v.push(note(focus.status.clone()));
            }
            v.push(button("Back", Act::OpenWorlds(false)));
            (k.name.clone(), v)
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
            if let Some(j) = focus.jam.as_ref().filter(|_| !settings.young) {
                v.push(note(format!("This week's jam is \u{201c}{}\u{201d}. Make something for it, share it to Everyone and enter it from Share.", j.theme)));
            }
            let shelf = pages(focus, info).map(|(list, n)| list[n]).unwrap_or("Everything");
            let mut list: Vec<&crate::creations::Creation> = lib
                .list
                .values()
                .filter(|c| match shelf {
                    "In this world" => lib.running.contains(&c.id),
                    "Made by me" => c.origin == Origin::Mine,
                    "From friends" => matches!(c.origin, Origin::Friend(_) | Origin::Code(_)),
                    "Made by Potluck" => c.origin == Origin::Base,
                    _ => true,
                })
                .collect();
            // mine first, then friends', then Potluck's, what's here before the rest
            let rank = |c: &crate::creations::Creation| match c.origin {
                Origin::Mine => 0,
                Origin::Base => 2,
                _ => 1,
            };
            list.sort_by_key(|c| (rank(c), !lib.running.contains(&c.id), c.manifest.name.to_lowercase()));
            for c in &list {
                v.push(Item { label: c.manifest.name.clone(), kind: Kind::Card(Act::Inside(c.id.clone())) });
            }
            if list.is_empty() {
                v.push(note(match shelf {
                    "In this world" => "Nothing from here runs in this world yet. Choose one on another shelf and bring it in.",
                    "Made by me" => "Nothing yet. Look inside any creation and Remix it to make one of your own.",
                    "From friends" => "Nothing yet. When a friend gives you a code, load it from Load a creation from a code.",
                    _ => "Nothing here yet.",
                }));
            }
            ("Creations".into(), v)
        }
        Screen::Backpack => {
            use crate::items::{HOTBAR, POCKETS};
            // the backpack's rows, then the hotbar, as they're drawn
            let order = (HOTBAR..POCKETS).chain(0..HOTBAR);
            let mut v: Vec<Item> = order
                .map(|i| {
                    let s = focus.pockets.0[i];
                    let label = if s.empty() { String::new() } else { format!("{} \u{00d7}{}", crate::items::name(s.thing), s.count) };
                    Item { label, kind: Kind::Slot(i) }
                })
                .collect();
            v.push(button("Tidy the backpack", Act::Tidy));
            ("Backpack".into(), v)
        }
        Screen::Blocks => {
            let page = pages(focus, info).map(|(list, n)| list[n]).unwrap_or("Building");
            let mut v: Vec<Item> = block_shelf(page).into_iter().map(|id| Item { label: blocks::block(id).name.to_string(), kind: Kind::Tile(id) }).collect();
            if v.is_empty() {
                v.push(note("No creation here makes blocks of its own yet. Brickworks does: bring it in from Creations, or remix it to draw your own."));
            }
            ("Blocks".into(), v)
        }
        Screen::Controls => {
            let mut v = vec![];
            if let Some((cid, aid)) = &focus.rebinding {
                let name = lib.list.get(cid).and_then(|c| c.manifest.actions.get(aid)).map(|a| a.name.clone()).unwrap_or_default();
                v.push(Item { label: format!("Press a button or a key for {name}"), kind: Kind::Big });
                v.push(note("A pad button moves it on the pad, a key on the keyboard. B or Esc leaves it as it was."));
                return ("Creations' buttons".into(), v);
            }
            v.push(note("What creations' buttons do while you ride their creatures, or while a mini game has your controls. Choose one to move it."));
            // what's in this world first, then the rest
            let mut list: Vec<&crate::creations::Creation> = lib.list.values().filter(|c| !c.manifest.actions.is_empty()).collect();
            list.sort_by_key(|c| (!lib.running.contains(&c.id), c.manifest.name.clone()));
            for c in &list {
                for (aid, a) in &c.manifest.actions {
                    let (pad, key) = settings.binding(&c.id, aid, a);
                    let on: Vec<String> = [crate::input::button_name(&pad), key].into_iter().filter(|b| !b.is_empty()).collect();
                    v.push(button(format!("{}: {}   {}", c.manifest.name, a.name, on.join(" / ")), Act::Rebind(c.id.clone(), aid.clone())));
                }
            }
            if list.is_empty() {
                v.push(note("None of your creations have buttons of their own yet."));
            }
            if !settings.bindings.is_empty() {
                v.push(button("Put them all back", Act::ResetControls));
            }
            v.push(button("Back", Act::OpenSettings));
            ("Creations' buttons".into(), v)
        }
        Screen::Panel => {
            let Some(p) = &focus.panel else { return (String::new(), vec![button("Close", Act::Answer(None))]) };
            // always says whose question it is
            let from = lib.list.get(&p.creation).map(|c| format!("From {} by {}", c.manifest.name, c.manifest.by)).unwrap_or_else(|| "From a creation in this world".into());
            let mut v = vec![note(from)];
            if !p.text.is_empty() {
                v.push(note(p.text.clone()));
            }
            for (i, c) in p.choices.iter().enumerate() {
                v.push(button(c.clone(), Act::Answer(Some(i as u32))));
            }
            v.push(button("Close", Act::Answer(None)));
            (p.title.clone(), v)
        }
        Screen::Settings => {
            let field_of = |key: &str, now: &str| focus.fields.get(key).cloned().unwrap_or_else(|| now.to_string());
            let mut v = match SETTINGS_PAGES[focus.settings_page.min(SETTINGS_PAGES.len() - 1)] {
                "You" => vec![
                    Item { label: "Your name".into(), kind: Kind::Field { key: "name".into(), value: field_of("name", &settings.name) } },
                    Item { label: "Survival".into(), kind: Kind::Toggle { key: "survival".into(), value: settings.survival } },
                    note("Health, and blocks that run out. Off, you build with everything."),
                    button("Your profile on the market", Act::Market("/me/profile".into(), "Your profile".into())),
                    button("Link a device", Act::OpenDevices),
                ],
                "Controls" => vec![
                    Item { label: "Mouse speed".into(), kind: Kind::Slider { key: "mouse".into(), value: settings.mouse_sensitivity as f64, min: 0.2, max: 3.0 } },
                    Item { label: "Stick speed".into(), kind: Kind::Slider { key: "stick".into(), value: settings.stick_sensitivity as f64, min: 0.2, max: 3.0 } },
                    Item { label: "Look up and down the other way".into(), kind: Kind::Toggle { key: "invert".into(), value: settings.invert_y } },
                    button("Creations' buttons", Act::OpenControls),
                ],
                "Picture and sound" => vec![
                    Item { label: "Lighting".into(), kind: Kind::Choice { act: Act::Lighting, value: cap(&settings.lighting) } },
                    note("Low for handhelds and laptops, High and Ultra for graphics cards. Takes effect after a restart."),
                    Item { label: "Field of view".into(), kind: Kind::Slider { key: "fov".into(), value: settings.fov as f64, min: 70.0, max: 110.0 } },
                    Item { label: "Full screen (F11)".into(), kind: Kind::Toggle { key: "fullscreen".into(), value: settings.fullscreen } },
                    Item { label: "Sound volume".into(), kind: Kind::Slider { key: "volume".into(), value: settings.volume as f64, min: 0.0, max: 1.0 } },
                    Item { label: "Subtitles for sounds".into(), kind: Kind::Toggle { key: "subtitles".into(), value: settings.subtitles } },
                ],
                "Online" => vec![
                    Item { label: "Game server".into(), kind: Kind::Field { key: "server".into(), value: field_of("server", &settings.server) } },
                    Item { label: "Market".into(), kind: Kind::Field { key: "market".into(), value: field_of("market", &settings.market) } },
                    Item { label: "Dev bridge for coding helpers on this computer".into(), kind: Kind::Toggle { key: "bridge".into(), value: settings.dev_bridge } },
                ],
                _ => {
                    let on = |b: bool| if b { "on" } else { "off" };
                    vec![
                        note(format!(
                            "{}. Sharing creations is {}, servers from the market's list are {} and chat is {}.",
                            if settings.young { "Set for a player under 13" } else { "Set for a player 13 or older" },
                            on(settings.sharing),
                            on(settings.servers),
                            on(settings.chat)
                        )),
                        button("Parent settings", Act::OpenParents),
                        note(if settings.parent_pin.is_empty() { "A parent can set a PIN there, so only they can change these." } else { "Locked with a parent's PIN." }),
                    ]
                }
            };
            // where a coding helper finds the bridge, under its switch
            if let Some(n) = focus.bridge.clone().filter(|_| settings.dev_bridge) {
                if let Some(at) = v.iter().position(|i| matches!(&i.kind, Kind::Toggle { key, .. } if key == "bridge")) {
                    v.insert(at + 1, note(n));
                }
            }
            v.push(button("Back", Act::Back));
            ("Settings".into(), v)
        }
    }
}

/// What a creation's kind is called on the screen.
pub fn kind_name(kind: &str) -> String {
    match kind {
        "game" => "Mini game".into(),
        k => cap(k),
    }
}

pub(crate) fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// An address without its https://, as people say it.
pub(crate) fn host(url: &str) -> &str {
    url.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/')
}

/// A QR code as a picture: black on white with the quiet edge readers need.
pub fn qr_image(text: &str) -> Option<Image> {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::ImageSampler;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    let w = code.width();
    let colors = code.to_colors();
    let (quiet, scale) = (4, 6);
    let size = (w + quiet * 2) * scale;
    let mut data = vec![255u8; size * size * 4];
    for y in 0..w {
        for x in 0..w {
            if colors[y * w + x] != qrcode::Color::Dark {
                continue;
            }
            for dy in 0..scale {
                let row = ((y + quiet) * scale + dy) * size;
                for dx in 0..scale {
                    let i = (row + (x + quiet) * scale + dx) * 4;
                    data[i..i + 3].copy_from_slice(&[20, 24, 30]);
                }
            }
        }
    }
    let mut image = Image::new(Extent3d { width: size as u32, height: size as u32, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
    image.sampler = ImageSampler::nearest();
    Some(image)
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
    clicks: Query<(&Interaction, Option<&MenuTab>, Option<&MenuPrompt>), (Changed<Interaction>, Or<(With<MenuTab>, With<MenuPrompt>)>)>,
    time: Res<Time>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut last_cursor: Local<Option<Vec2>>,
    mut heard: ResMut<crate::sound::Heard>,
) {
    // a button drawn again under a resting pointer counts as hovered; only a
    // pointer that moved should take the selection from the keys or the pad
    let cursor = windows.single().ok().and_then(|w| w.cursor_position());
    let moved = cursor.is_some() && cursor != *last_cursor;
    *last_cursor = cursor;
    if focus.playing() || focus.keyboard.is_some() {
        return;
    }
    // the next button or key for a creation's action
    if let Some((creation, action)) = focus.rebinding.clone() {
        if c.back {
            focus.rebinding = None;
        } else if let Some(b) = c.just.first() {
            let def = lib.list.get(&creation).and_then(|c| c.manifest.actions.get(&action)).cloned().unwrap_or_default();
            let (pad, keys) = settings.binding(&creation, &action, &def);
            let mut chosen = crate::settings::Binding { pad, keys };
            match b.split_once(':') {
                Some(("pad", p)) => chosen.pad = p.to_string(),
                Some((_, k)) => chosen.keys = k.to_string(),
                None => {}
            }
            let key = format!("{creation}/{action}");
            if chosen.pad == def.pad && chosen.keys == def.keys {
                settings.bindings.remove(&key);
            } else {
                settings.bindings.insert(key, chosen);
            }
            settings.save();
            focus.rebinding = None;
        }
        return;
    }
    // a page's tab or a prompt at the bottom, clicked
    let mut tab = None;
    let mut prompt = None;
    for (int, t, p) in &clicks {
        if *int == Interaction::Pressed {
            tab = t.map(|t| t.0).or(tab);
            prompt = p.copied().or(prompt);
        }
    }
    // pages: RB and LB, Tab, or a click on one
    if let Some((list, now)) = pages(&focus, &info) {
        let step = c.page + (prompt == Some(MenuPrompt::Page)) as i32;
        let mut to = (now as i32 + step).rem_euclid(list.len() as i32) as usize;
        if let Some(t) = tab {
            to = t.min(list.len() - 1);
        }
        if to != now {
            heard.click();
            if let Some(p) = focus.page_mut() {
                *p = to;
            }
            focus.sel = 0;
            focus.first = 0;
            // the first thing you can choose on it, not a note
            let (_, items) = menu_items(&focus, &lib, &scripts, &settings, &info);
            focus.sel = items.iter().position(|i| i.selectable()).unwrap_or(0);
            return;
        }
    }
    let (_, items) = menu_items(&focus, &lib, &scripts, &settings, &info);
    let selectable: Vec<usize> = items.iter().enumerate().filter(|(_, i)| i.selectable()).map(|(n, _)| n).collect();
    if selectable.is_empty() {
        // an empty shelf still goes back
        if focus.screen == Screen::Blocks && (c.back || c.blocks || prompt == Some(MenuPrompt::Back)) {
            acts.0.push(Act::Resume);
        } else if c.back || prompt == Some(MenuPrompt::Back) {
            acts.0.push(Act::Back);
        }
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
    // small buttons side by side: left and right go along them too
    let chip = matches!(items.get(focus.sel).map(|i| &i.kind), Some(Kind::Chip(_)));
    if matches!(items.get(selectable[pos]).map(|i| &i.kind), Some(Kind::Card(_) | Kind::Tile(_) | Kind::Slot(_))) {
        // cards in rows: left and right along a row, up and down a row at a time
        let cols = focus.cols.max(1) as i32;
        let last = selectable.len() as i32 - 1;
        let mut p = pos as i32;
        p += c.right as i32 - c.left as i32 + (c.down as i32 - c.up as i32) * cols;
        pos = if c.down && p > last && (pos as i32) / cols < last / cols { last } else { p.clamp(0, last) } as usize;
    } else {
        if c.down || chip && c.right {
            pos = (pos + 1) % selectable.len();
        }
        if c.up || chip && c.left {
            pos = (pos + selectable.len() - 1) % selectable.len();
        }
    }
    focus.sel = selectable[pos];
    // scrolling the code in Look inside
    if matches!(focus.screen, Screen::Inside(_)) {
        focus.scroll = (focus.scroll + c.hotbar_step * 3).max(0);
    }
    let item = items[focus.sel].clone();
    let step = (c.right as i32 - c.left as i32) as f64;
    let accept = c.accept || clicked || prompt == Some(MenuPrompt::Accept);
    let interact = c.interact || prompt == Some(MenuPrompt::Interact);
    match &item.kind {
        Kind::Button(act) | Kind::Chip(act) => {
            if accept {
                heard.click();
                acts.0.push(act.clone());
            }
        }
        Kind::Card(act) => {
            if accept {
                heard.click();
                acts.0.push(act.clone());
            }
            // X or F brings it into this world, or takes it out
            if let (Act::Inside(id), true, true) = (act, interact, info.in_world) {
                heard.click();
                acts.0.push(if lib.running.contains(id) { Act::Stop(id.clone()) } else { Act::Run(id.clone()) });
            }
        }
        Kind::Tile(id) => {
            if accept {
                heard.click();
                acts.0.push(Act::Hold(*id));
            }
        }
        Kind::Slot(i) => {
            if accept || interact {
                heard.click();
                acts.0.push(Act::Slot(*i, interact && !accept));
            }
        }
        Kind::Choice { act, .. } => {
            if accept || step != 0.0 {
                heard.click();
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
                let v: String = v.chars().take(crate::keyboard::max_len(key)).collect();
                set_field(&mut focus, &mut settings, key, &v);
            }
            if c.accept && c.pad && !clicked {
                // a pad types on the keyboard on the screen
                crate::keyboard::open(&mut focus, key, &item.label);
                heard.click();
            } else if c.accept {
                // Enter in a field does the button after it
                let v = focus.fields.get(key).cloned().unwrap_or_else(|| value.clone());
                if let Some(a) = crate::keyboard::after(&items, key, &v) {
                    acts.0.push(a);
                }
            }
        }
        _ => {}
    }
    // the key that opened Blocks closes it
    let shut = matches!(focus.screen, Screen::Blocks | Screen::Backpack) && c.blocks;
    if (c.back || shut || prompt == Some(MenuPrompt::Back)) && !matches!(item.kind, Kind::Field { .. } if c.backspace) {
        let back = match &focus.screen {
            Screen::Title => None,
            Screen::Pause => Some(Act::Resume),
            Screen::Tweak(id) | Screen::Share(id) | Screen::Suggest(id) | Screen::Suggestions(id) | Screen::Report(id) => Some(Act::Inside(id.clone())),
            Screen::Devices | Screen::Parents | Screen::Controls => Some(Act::OpenSettings),
            Screen::Panel => Some(Act::Answer(None)),
            Screen::Map | Screen::Chat | Screen::Blocks => Some(Act::Resume),
            // putting back what was being moved, then away
            Screen::Backpack if focus.moving.is_some() && !shut => {
                focus.moving = None;
                None
            }
            Screen::Backpack => Some(Act::Resume),
            Screen::Waypoint(_) | Screen::Shared(_) => Some(Act::OpenMap),
            _ => Some(Act::Back),
        };
        if let Some(a) = back {
            acts.0.push(a);
        }
    }
}

/// What a text field says now, and the setting it is, if it's one.
pub fn set_field(focus: &mut Focus, settings: &mut Settings, key: &str, v: &str) {
    focus.fields.insert(key.to_string(), v.to_string());
    match key {
        "name" => settings.name = v.chars().filter(|c| c.is_alphanumeric() || *c == '_' || *c == ' ').take(20).collect(),
        "server" => settings.server = v.to_string(),
        "market" => settings.market = v.to_string(),
        _ => {}
    }
}

/// A field's button with what the field says in it (Enter in a field).
pub fn with_value(mut a: Act, v: &str) -> Act {
    match &mut a {
        Act::JoinAddress(s) | Act::LoadCode(s) | Act::LinkDevice(s) | Act::UnlockParents(s) | Act::SetPin(s) | Act::Say(s) | Act::RenameWaypoint(_, s) => *s = v.to_string(),
        _ => {}
    }
    a
}

/// The play keys a menu saw this frame don't act in the world too, when
/// the menu closes (Esc that leaves the pause menu doesn't open it again).
pub fn menu_owns(focus: Res<Focus>, mut c: ResMut<Controls>) {
    if focus.playing() {
        return;
    }
    c.pause = false;
    c.jump_pressed = false;
    c.interact = false;
    c.look_inside = false;
    c.blocks = false;
    c.share = false;
    c.place_pressed = false;
    c.break_pressed = false;
    c.map = false;
    c.mark = false;
    c.minimap = false;
    c.chat = false;
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
        ("volume", KnobValue::Number(n)) => settings.volume = n.clamp(0.0, 1.0) as f32,
        ("subtitles", KnobValue::Toggle(b)) => settings.subtitles = b,
        ("fullscreen", KnobValue::Toggle(b)) => settings.fullscreen = b,
        ("bridge", KnobValue::Toggle(b)) => settings.dev_bridge = b,
        ("minimap_turns", KnobValue::Toggle(b)) => settings.minimap_turns = b,
        ("waypoint_labels", KnobValue::Toggle(b)) => settings.waypoint_labels = b,
        ("chat", KnobValue::Toggle(b)) if focus.parents_open => settings.chat = b,
        // a parent's, once unlocked
        ("young", KnobValue::Toggle(b)) if focus.parents_open => settings.young = b,
        ("sharing", KnobValue::Toggle(b)) if focus.parents_open => settings.sharing = b,
        ("servers", KnobValue::Toggle(b)) if focus.parents_open => settings.servers = b,
        _ => {}
    }
    settings.save();
}

// ---------- drawing the menus ----------

#[derive(Component)]
pub struct MenuRoot;

#[derive(Component)]
pub struct MenuButton(pub usize);

/// One of a screen's pages, to click.
#[derive(Component)]
pub struct MenuTab(pub usize);

/// A prompt at the bottom of a menu, which does what it says when clicked.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub enum MenuPrompt {
    Accept,
    Back,
    Page,
    Interact,
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
/// The health bar's share of the row above the hotbar.
#[derive(Component)]
pub struct HealthTrack;
/// A creation's meter beside the health bar, and its fill.
#[derive(Component)]
pub struct MeterTrack(usize);
#[derive(Component)]
pub struct MeterFill(usize);
#[derive(Component)]
pub struct Crosshair;
#[derive(Component)]
pub struct LabelText(usize);
/// Where creations' lines go (`ui.show`): an index into `script::PLACES`.
#[derive(Component)]
pub struct PieceText(pub usize);
/// Subtitles for sounds, bottom right.
#[derive(Component)]
pub struct CaptionText;

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
            // over menus too (shown when the rest of the HUD hides), so a
            // share code shows while Look inside is open
            h.spawn((
                ToastBox,
                Node { position_type: PositionType::Absolute, top: px(40), width: percent(100), padding: UiRect::horizontal(percent(15)), justify_content: JustifyContent::Center, ..default() },
                GlobalZIndex(30),
                Pickable::IGNORE,
                Visibility::Visible,
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
            // health, and creations' meters beside it (`ui.meter`), sharing the row above the hotbar
            let track = || (Node { flex_grow: 1.0, flex_basis: px(0), height: percent(100), border_radius: BorderRadius::all(px(3)), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.3)));
            h.spawn(Node { position_type: PositionType::Absolute, bottom: px(84), left: percent(50), margin: UiRect::left(px(-240)), width: px(480), height: px(6), column_gap: px(10), ..default() }).with_children(|row| {
                row.spawn((HealthTrack, track())).with_children(|b| {
                    b.spawn((HealthBar, Node { width: percent(100), height: percent(100), border_radius: BorderRadius::all(px(3)), ..default() }, BackgroundColor(RED)));
                });
                for i in 0..crate::script::MAX_METERS {
                    let (mut node, bg) = track();
                    node.display = Display::None;
                    row.spawn((MeterTrack(i), node, bg)).with_children(|b| {
                        b.spawn((MeterFill(i), Node { width: percent(0), height: percent(100), border_radius: BorderRadius::all(px(3)), ..default() }, BackgroundColor(Color::WHITE)));
                    });
                }
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
            // creations' lines: top right, top left (under the world's line), above the hotbar
            let places = [
                Node { position_type: PositionType::Absolute, right: px(18), top: px(14), max_width: percent(40), ..default() },
                Node { position_type: PositionType::Absolute, left: px(18), top: px(40), max_width: percent(40), ..default() },
                Node { position_type: PositionType::Absolute, bottom: px(130), width: percent(100), justify_content: JustifyContent::Center, ..default() },
            ];
            for (i, node) in places.into_iter().enumerate() {
                h.spawn((
                    PieceText(i),
                    Text::new(""),
                    TextFont { font: ui.clone().into(), font_size: FontSize::Px(17.0), ..default() },
                    TextColor(INK),
                    TextLayout::justify(if i == 0 { Justify::Right } else if i == 2 { Justify::Center } else { Justify::Left }),
                    TextShadow::default(),
                    node,
                    Visibility::Hidden,
                ));
            }
            h.spawn((
                CaptionText,
                Text::new(""),
                TextFont { font: ui.clone().into(), font_size: FontSize::Px(16.0), ..default() },
                TextColor(INK),
                TextLayout::justify(Justify::Right),
                Node { position_type: PositionType::Absolute, right: px(18), bottom: px(96), padding: UiRect::axes(px(10), px(6)), border_radius: BorderRadius::all(px(8)), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
                Visibility::Hidden,
            ));
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
        Query<(&PieceText, &mut Text, &mut Visibility), Without<Hud>>,
        Query<(&mut Text, &mut Visibility), (With<CaptionText>, Without<Hud>)>,
    )>,
    mut bars: ParamSet<(
        Query<(&mut Node, &mut BackgroundColor), (With<HealthBar>, Without<LabelText>)>,
        Query<&mut Node, (With<HealthTrack>, Without<LabelText>)>,
        Query<(&MeterTrack, &mut Node), Without<LabelText>>,
        Query<(&MeterFill, &mut Node, &mut BackgroundColor), Without<LabelText>>,
    )>,
    cam: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
) {
    // menus that cover the world hide the hotbar and the crosshair too
    let show = focus.screen != Screen::Title && info.in_world && !draw::blurs(&focus.screen);
    for mut v in &mut hud {
        *v = if show { Visibility::Inherited } else { Visibility::Hidden };
    }
    for (s, mut img, mut border) in &mut slots {
        let slot = me.pockets.0[s.0];
        let empty = settings.survival && slot.empty() || slot.thing == 0;
        img.image = if empty { Handle::default() } else { atlas.icons.get(crate::items::icon(slot.thing)).cloned().unwrap_or_default() };
        img.color = if empty { Color::NONE } else { Color::WHITE };
        *border = BorderColor::all(if s.0 == me.slot { Color::srgb(1.0, 1.0, 1.0) } else { Color::srgba(0.0, 0.0, 0.0, 0.5) });
    }
    for (s, mut t) in &mut texts.p0() {
        let n = me.pockets.0[s.0].count;
        // one of a thing that only comes one to a slot (a tool) needs no number
        let one = n == 1 && crate::items::stack(me.pockets.0[s.0].thing) == 1;
        t.0 = if settings.survival && n > 0 && !one { n.to_string() } else { String::new() };
    }
    // what pressing would do
    let pad = c.pad;
    let mut prompt = String::new();
    if let Some(id) = me.riding {
        let kind = herd.bodies.get(&id).map(|b| b.kind.clone()).unwrap_or_default();
        prompt = match &me.live_prompt {
            // its own buttons, and B or Shift still gets you off
            Some(live) => format!("Riding {kind}  ·  {live}  ·  {} to get off", if pad { "B" } else { "Shift" }),
            None => format!("Riding {kind}  ·  {} or {} to get off", glyph(pad, "interact"), if pad { "B" } else { "Shift" }),
        };
    } else if let Some(b) = me.aim_body.and_then(|id| herd.bodies.get(&id)) {
        let name = lib.list.get(&b.creation).map(|c| format!("{} by {}", c.manifest.name, c.manifest.by)).unwrap_or(b.kind.clone());
        let verb = if b.rideable && b.rider.is_none() { "Ride" } else { "Use" };
        prompt = format!("{name}\n{} {verb}   {} Look inside", glyph(pad, "interact"), glyph(pad, "inside"));
    } else if let Some((_, text)) = &me.aim_arch {
        prompt = text.clone();
    } else if let Some(live) = &me.live_prompt {
        let name = me.focus.as_ref().and_then(|id| lib.list.get(id)).map(|c| c.manifest.name.clone()).unwrap_or_default();
        prompt = format!("{name}\n{live}");
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
    // subtitles, newest at the bottom
    if let Ok((mut t, mut v)) = texts.p6().single_mut() {
        t.0 = me.captions.iter().map(|c| c.0.as_str()).collect::<Vec<_>>().join("\n");
        *v = if t.0.is_empty() || !show { Visibility::Hidden } else { Visibility::Inherited };
    }
    // creations' lines, each place's in the order of their creation and id
    for (place, mut t, mut v) in &mut texts.p5() {
        let at = crate::script::PLACES[place.0];
        t.0 = me.pieces.values().filter(|p| p.at == at).map(|p| p.text.as_str()).collect::<Vec<_>>().join("\n");
        *v = if t.0.is_empty() { Visibility::Hidden } else { Visibility::Inherited };
    }
    if let Ok((mut n, mut bg)) = bars.p0().single_mut() {
        n.width = percent((me.health / MAX_HEALTH * 100.0).max(0.0));
        let flash = (me.hurt < 0.4) as i32 as f32;
        bg.0 = if settings.survival { RED.mix(&INK, flash) } else { Color::NONE };
    }
    // creations' meters share the row with it (health only in survival)
    let meters: Vec<(f32, [u8; 3])> = me.pieces.values().filter(|p| p.at == crate::script::METER).filter_map(|p| p.meter).take(crate::script::MAX_METERS).collect();
    if let Ok(mut n) = bars.p1().single_mut() {
        n.display = if settings.survival { Display::Flex } else { Display::None };
    }
    for (t, mut n) in &mut bars.p2() {
        n.display = if t.0 < meters.len() { Display::Flex } else { Display::None };
    }
    for (f, mut n, mut bg) in &mut bars.p3() {
        if let Some((v, c)) = meters.get(f.0) {
            n.width = percent(v * 100.0);
            bg.0 = Color::srgb_u8(c[0], c[1], c[2]);
        }
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
            // centred on its longest line, standing on the point
            let wide = text.lines().map(|l| l.chars().count()).max().unwrap_or(0).min(30);
            node.left = px(v.x - 4.0 * wide as f32);
            node.top = px(v.y - 14.0 * text.lines().count().max(1) as f32);
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

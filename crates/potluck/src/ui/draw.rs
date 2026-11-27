//! How the menus look. Short menus are words on a shade over the world
//! ("open air": no box, the chosen line marked in mint); screens you browse
//! or read (Creations, the market, Look inside, servers, suggestions) get a
//! near-solid sheet so pictures and code stay readable. Sizes are for a
//! 720-line window and scale with the window's height.

use bevy::text::LetterSpacing;

use super::*;

const SLATE: (f32, f32, f32) = (0.039, 0.055, 0.078);
const FAINT: Color = Color::srgba(0.98, 0.96, 0.9, 0.42);
const SOFT: Color = Color::srgba(0.98, 0.96, 0.9, 0.7);
const DARK: Color = Color::srgb(0.11, 0.14, 0.19);
const WHITE: Color = Color::WHITE;

fn slate(a: f32) -> Color {
    Color::srgba(SLATE.0, SLATE.1, SLATE.2, a)
}
fn mint(a: f32) -> Color {
    Color::srgba(0.37, 0.88, 0.78, a)
}
fn white(a: f32) -> Color {
    Color::srgba(1.0, 1.0, 1.0, a)
}

/// How a screen is laid out.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Look {
    /// Words on a shade down the left, over the world.
    Air,
    /// A near-solid sheet in the middle, for browsing and reading.
    Sheet,
    /// The list beside the full map.
    Map,
    /// Chat, low on the left.
    Chat,
}

pub fn look(s: &Screen) -> Look {
    match s {
        Screen::Map | Screen::Waypoint(_) | Screen::Shared(_) => Look::Map,
        Screen::Chat => Look::Chat,
        Screen::Library | Screen::Link | Screen::Inside(_) | Screen::Suggestions(_) | Screen::Servers => Look::Sheet,
        _ => Look::Air,
    }
}

/// Whether the world behind a screen is softened (`render::MenuBlur`):
/// not on the title, where the island is the picture, nor where you watch
/// the world change (Tweak) or talk over it.
pub fn blurs(s: &Screen) -> bool {
    !matches!(s, Screen::Playing | Screen::Title | Screen::Tweak(_) | Screen::Chat | Screen::Panel | Screen::Map | Screen::Waypoint(_) | Screen::Shared(_))
}

/// Rows with something to change on the right (settings) draw smaller and
/// brighter than a plain list of places to go.
#[derive(Clone, Copy, PartialEq)]
enum Rows {
    Menu,
    Values,
    Small,
}

struct Ctx {
    s: f32,
    fonts: Fonts,
    qr: Option<Handle<Image>>,
    pad: bool,
}

impl Ctx {
    fn u(&self, v: f32) -> Val {
        px(v * self.s)
    }
    fn round(&self, size: f32, weight: u16) -> TextFont {
        TextFont { font: self.fonts.title.clone().into(), font_size: FontSize::Px(size * self.s), weight: FontWeight(weight), ..default() }
    }
    fn plain(&self, size: f32) -> TextFont {
        TextFont { font: self.fonts.ui.clone().into(), font_size: FontSize::Px(size * self.s), ..default() }
    }
    fn mono(&self, size: f32) -> TextFont {
        TextFont { font: Handle::<Font>::default().into(), font_size: FontSize::Px(size * self.s), ..default() }
    }
    fn shadow(&self, d: f32, a: f32) -> TextShadow {
        TextShadow { offset: Vec2::splat(d * self.s), color: Color::srgba(0.0, 0.0, 0.0, a) }
    }
}

/// Roughly how tall an item draws (before scaling), for fitting a long menu.
fn item_height(item: &Item, wide: f32, rows: Rows) -> f32 {
    let lines = |s: &str, size: f32| ((s.chars().count() as f32 * size * 0.55 / wide).ceil()).max(1.0);
    match &item.kind {
        Kind::Note if item.label.is_empty() => 0.0,
        Kind::Note => lines(&item.label, 15.0) * 21.0 + 8.0,
        Kind::Line { words, .. } => lines(&format!("{}  {words}", item.label), 16.0) * 22.0 + 6.0,
        Kind::Big => 86.0,
        Kind::Qr => 250.0,
        Kind::Chip(_) => 24.0,
        _ => match rows {
            Rows::Menu => 52.0,
            Rows::Values => 51.0,
            Rows::Small => 44.0,
        },
    }
}

/// The items that fit in `room`: from `first`, moved so the selection shows.
fn fit(items: &[Item], h: &[f32], sel: usize, first: usize, room: f32) -> (usize, usize) {
    let total: f32 = h.iter().sum();
    if total <= room || items.is_empty() {
        return (0, items.len());
    }
    let sel = sel.min(items.len() - 1);
    let mut first = first.min(sel);
    // a heading just above the selection comes along
    if first == sel && sel > 0 && matches!(items[sel - 1].kind, Kind::Note) {
        first = sel - 1;
    }
    while first < sel && h[first..=sel].iter().sum::<f32>() > room {
        first += 1;
    }
    let mut end = first;
    let mut used = 0.0;
    while end < items.len() && used + h[end] <= room {
        used += h[end];
        end += 1;
    }
    // room left at the bottom: show more above instead
    while first > 0 && used + h[first - 1] <= room {
        first -= 1;
        used += h[first];
    }
    (first, end.max(sel + 1))
}

/// What a screen's head shows from its first notes: a kicker over the title
/// and a line under it. Returns how many leading items the head took.
fn head_notes(screen: &Screen, items: &[Item]) -> (Option<String>, Option<String>, usize) {
    let note_at = |i: usize| items.get(i).filter(|it| it.kind == Kind::Note && !it.label.is_empty()).map(|it| it.label.clone());
    match screen {
        // whose question it is, then the question
        Screen::Panel => match note_at(0) {
            Some(k) => match note_at(1) {
                Some(sub) => (Some(k), Some(sub), 2),
                None => (Some(k), None, 1),
            },
            None => (None, None, 0),
        },
        // "Creature by Potluck", then what it is
        Screen::Inside(_) => match note_at(0) {
            Some(k) => (Some(k), None, 1),
            None => (None, None, 0),
        },
        Screen::Settings | Screen::Library | Screen::Controls | Screen::Map | Screen::Waypoint(_) | Screen::Shared(_) | Screen::Chat => (None, None, 0),
        _ => match note_at(0) {
            Some(sub) => (None, Some(sub), 1),
            None => (None, None, 0),
        },
    }
}

/// The prompts along the bottom: what each button does here.
fn prompts(screen: &Screen, item: Option<&Item>, info: &WorldInfo) -> Vec<(MenuPrompt, &'static str, &'static str, &'static str)> {
    let choose = match item.map(|i| &i.kind) {
        Some(Kind::Slider { .. } | Kind::Toggle { .. } | Kind::Choice { .. } | Kind::Swatch { .. }) => "Change",
        Some(Kind::Field { .. }) => "Type",
        Some(Kind::Card(_)) => "Look inside",
        _ => "Choose",
    };
    let mut v = vec![(MenuPrompt::Accept, "A", "Enter", choose)];
    match screen {
        Screen::Title => return v,
        Screen::Settings | Screen::Library => v.push((MenuPrompt::Page, "LB RB", "Tab", "Pages")),
        Screen::Tweak(_) => v.push((MenuPrompt::Accept, "\u{2039} \u{203a}", "\u{2039} \u{203a}", "Turn")),
        _ => {}
    }
    if *screen == Screen::Library && info.in_world {
        v.insert(1, (MenuPrompt::Interact, "X", "F", "Bring in or take out"));
    }
    let back = match screen {
        Screen::Pause => "Back to the world",
        Screen::Panel => "Close",
        _ => "Back",
    };
    v.push((MenuPrompt::Back, "B", "Esc", back));
    v
}

#[allow(clippy::too_many_arguments)]
pub fn draw_menu(
    mut commands: Commands,
    mut focus: ResMut<Focus>,
    lib: Res<Library>,
    scripts: Res<Scripts>,
    settings: Res<Settings>,
    info: Res<WorldInfo>,
    fonts: Res<Fonts>,
    roots: Query<Entity, With<MenuRoot>>,
    mut images: ResMut<Assets<Image>>,
    halves: Res<crate::halves::Halves>,
    windows: Query<&Window, With<PrimaryWindow>>,
    controls: Res<Controls>,
    photos: Res<crate::photos::Photos>,
) {
    let (title, items) = menu_items(&focus, &lib, &scripts, &settings, &info);
    if let Some(url) = items.iter().find(|i| i.kind == Kind::Qr).map(|i| i.label.clone()) {
        if focus.qr.as_ref().map(|q| &q.0) != Some(&url) {
            focus.qr = qr_image(&url).map(|img| (url, images.add(img)));
        }
    }
    let side = match &focus.screen {
        Screen::Inside(id) => lib.list.get(id).map(|c| {
            let src = c.files.get(&c.manifest.main).cloned().unwrap_or_default();
            let vm = scripts.vms.get(id);
            let mut log: Vec<String> = vm.map(|v| v.log.clone()).unwrap_or_default();
            let half = halves.list.get(id);
            // what its client half printed on this game, too
            log.extend(half.iter().flat_map(|h| h.log.iter().map(|l| format!("client: {l}"))));
            let log: Vec<String> = log.iter().rev().take(6).rev().cloned().collect();
            let err = vm.and_then(|v| v.error.clone()).or_else(|| half.and_then(|h| h.error.as_ref()).map(|e| format!("Its client half stopped: {e}")));
            let ms = vm.map(|v| v.load_ms).unwrap_or(0.0);
            let budget = vm.map(|v| v.budget());
            Side { file: c.manifest.main.clone(), src, log, err, ms, budget }
        }),
        _ => None,
    };
    let look = look(&focus.screen);
    let win = windows.single().map(|w| Vec2::new(w.width(), w.height())).unwrap_or(Vec2::new(1280.0, 720.0));
    let s = match look {
        Look::Map | Look::Chat => 1.0,
        _ => (win.y / 720.0).clamp(0.75, 2.5),
    };
    let ctx = Ctx { s, fonts: fonts.clone(), qr: focus.qr.as_ref().map(|q| q.1.clone()), pad: controls.pad };
    let pages = pages(&focus, &info);
    let (kicker, sub, skip) = head_notes(&focus.screen, &items);
    let rows = if look == Look::Map || look == Look::Chat {
        Rows::Small
    } else if look == Look::Sheet || focus.screen == Screen::Controls || items.iter().any(|i| matches!(i.kind, Kind::Slider { .. } | Kind::Toggle { .. } | Kind::Swatch { .. } | Kind::Field { .. } | Kind::Choice { .. })) {
        Rows::Values
    } else {
        Rows::Menu
    };
    let width = match (look, &focus.screen) {
        (Look::Map, _) => crate::map::PANEL - 20.0,
        (Look::Chat, _) => crate::chat::WIDTH,
        (_, Screen::Settings) => 1020.0 * s,
        (Look::Sheet, Screen::Inside(_)) => 470.0 * s,
        (Look::Sheet, _) => 700.0 * s,
        _ if rows == Rows::Values => 680.0 * s,
        _ => 620.0 * s,
    };
    // the list's own width, for wrapping
    let list_w = match focus.screen {
        Screen::Settings => width - 340.0 * s,
        _ => width,
    } / s;
    // what fits: cards by rows, anything else by height
    let cards: Vec<usize> = items.iter().enumerate().filter(|(_, i)| matches!(i.kind, Kind::Card(_))).map(|(n, _)| n).collect();
    let grid = Grid::new(win, s, !cards.is_empty() && look == Look::Sheet);
    let (first, end) = if grid.on {
        let k = cards.iter().position(|&i| i == focus.sel).unwrap_or(0);
        let row = k / grid.cols;
        let mut top = (focus.first / grid.cols).min(row);
        if row >= top + grid.rows {
            top = row + 1 - grid.rows;
        }
        (top * grid.cols, (top * grid.cols + grid.cols * grid.rows).min(cards.len()))
    } else {
        let list = &items[skip.min(items.len())..];
        let h: Vec<f32> = list.iter().map(|i| item_height(i, list_w - 52.0, rows) * if look == Look::Map || look == Look::Chat { 1.0 } else { s }).collect();
        let head = if title.is_empty() { 0.0 } else { 70.0 } + if sub.is_some() { 34.0 } else { 0.0 } + if kicker.is_some() { 22.0 } else { 0.0 };
        let room = match look {
            Look::Map => win.y - 40.0 - 52.0 - 52.0 - 26.0 - 60.0,
            Look::Chat => win.y - 80.0 - 52.0 - 26.0 - 60.0,
            Look::Sheet => win.y - (60.0 + 48.0 + 70.0 + head + 30.0) * s,
            Look::Air => win.y - (40.0 + 110.0 + head + 26.0) * s,
        }
        .max(200.0);
        let (a, b) = fit(list, &h, focus.sel.saturating_sub(skip), focus.first.saturating_sub(skip), room);
        (a + skip, b + skip)
    };
    let running: Vec<&String> = if focus.screen == Screen::Library { lib.running.iter().collect() } else { vec![] };
    let pictures = photos.version;
    let sig = format!("{:?}|{title}|{items:?}|{}|{}|{:?}|{first}|{end}|{s}|{}|{}|{pages:?}|{running:?}|{pictures}|{}", focus.screen, focus.sel, focus.scroll, side, ctx.pad, info.in_world, grid.cols);
    if sig == focus.sig {
        return;
    }
    focus.sig = sig;
    focus.first = first;
    if grid.on {
        focus.cols = grid.cols;
    }
    focus.items = items.clone();
    for e in &roots {
        commands.entity(e).despawn();
    }
    if focus.playing() {
        return;
    }
    let sel = focus.sel;
    let item_sel = items.get(sel);
    let bottom = prompts(&focus.screen, item_sel, &info);
    let m = Menu { title, items: &items, skip, first, end, sel, kicker, sub, rows, width, pages, prompts: bottom };
    match look {
        Look::Air => air(&mut commands, &ctx, &m, &focus, &info),
        Look::Sheet => sheet(&mut commands, &ctx, &m, &focus, &lib, &photos, side, &grid, &cards),
        Look::Map | Look::Chat => panel(&mut commands, &ctx, &m, look),
    }
}

struct Side {
    file: String,
    src: String,
    log: Vec<String>,
    err: Option<String>,
    ms: f32,
    budget: Option<String>,
}

impl std::fmt::Debug for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}|{}|{:?}|{:?}|{:.0}|{:?}", self.file, self.src.len(), self.log, self.err, self.ms, self.budget)
    }
}

/// A menu worked out, ready to draw.
struct Menu<'a> {
    title: String,
    items: &'a [Item],
    /// Leading items the head shows.
    skip: usize,
    first: usize,
    end: usize,
    sel: usize,
    kicker: Option<String>,
    sub: Option<String>,
    rows: Rows,
    width: f32,
    pages: Option<(&'static [&'static str], usize)>,
    prompts: Vec<(MenuPrompt, &'static str, &'static str, &'static str)>,
}

/// How the Creations sheet's cards fit the window.
struct Grid {
    on: bool,
    cols: usize,
    rows: usize,
}

impl Grid {
    /// The detail beside the cards is this wide, before scaling.
    const DETAIL: f32 = 330.0;
    fn new(win: Vec2, s: f32, on: bool) -> Grid {
        let inner = win.x - (2.0 * 40.0 + 2.0 * 30.0 + Self::DETAIL + 24.0) * s;
        let cols = ((inner + 14.0 * s) / (194.0 * s)).floor().clamp(2.0, 6.0) as usize;
        let card_w = (inner - 14.0 * s * (cols as f32 - 1.0)) / cols as f32;
        let card_h = card_w / PICTURE + 54.0 * s;
        // the sheet's edges and padding, the title and tabs, the jam, the prompts
        let room = win.y - (2.0 * 24.0 + 22.0 + 18.0 + 48.0 + 16.0 + 52.0 + 42.0) * s;
        let rows = ((room + 14.0 * s) / (card_h + 14.0 * s)).floor().max(1.0) as usize;
        Grid { on, cols, rows }
    }
}

/// How wide a creation's picture is for its height.
const PICTURE: f32 = crate::photos::SIZE.x as f32 / crate::photos::SIZE.y as f32;

fn trim_num(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn root<'a>(commands: &'a mut Commands) -> EntityCommands<'a> {
    commands.spawn((
        MenuRoot,
        Node { width: percent(100), height: percent(100), position_type: PositionType::Absolute, ..default() },
        GlobalZIndex(10),
    ))
}

/// The shade from the left that the words sit on.
fn shade(wide: bool) -> BackgroundGradient {
    let stops = if wide {
        vec![ColorStop::percent(slate(0.86), 0.0), ColorStop::percent(slate(0.74), 45.0), ColorStop::percent(slate(0.3), 74.0), ColorStop::percent(slate(0.06), 100.0)]
    } else {
        vec![ColorStop::percent(slate(0.84), 0.0), ColorStop::percent(slate(0.66), 26.0), ColorStop::percent(slate(0.22), 48.0), ColorStop::percent(slate(0.0), 62.0)]
    };
    BackgroundGradient(vec![LinearGradient::to_right(stops).into()])
}

/// The head: a kicker, the title and a line under it.
fn head(p: &mut ChildSpawnerCommands, ctx: &Ctx, m: &Menu, big: bool, size: f32) {
    if let Some(k) = &m.kicker {
        p.spawn((Text::new(k.to_uppercase()), ctx.plain(12.5), TextColor(MINT), LetterSpacing::Px(1.4 * ctx.s), Node { margin: UiRect::bottom(ctx.u(6.0)), ..default() }));
    }
    if !m.title.is_empty() {
        if big {
            p.spawn((Text::new(m.title.clone()), ctx.round(96.0, 700), TextColor(GOLD), TextShadow { offset: Vec2::splat(5.0 * ctx.s), color: Color::srgba(0.24, 0.12, 0.0, 0.35) }));
        } else {
            p.spawn((Text::new(m.title.clone()), ctx.round(size, 700), TextColor(INK), ctx.shadow(3.0, 0.28)));
        }
    }
    if let Some(sub) = &m.sub {
        p.spawn((Text::new(sub.clone()), ctx.plain(17.0), TextColor(SOFT), Node { margin: UiRect::top(ctx.u(8.0)), ..default() }));
    }
}

/// The open air menus: the shade, the words, the prompts.
fn air(commands: &mut Commands, ctx: &Ctx, m: &Menu, focus: &Focus, info: &WorldInfo) {
    let title_screen = focus.screen == Screen::Title;
    let settings = focus.screen == Screen::Settings;
    root(commands).with_children(|root| {
        root.spawn((Node { position_type: PositionType::Absolute, left: px(0), top: px(0), width: percent(100), height: percent(100), ..default() }, shade(settings || m.rows == Rows::Values)));
        root.spawn(Node {
            position_type: PositionType::Absolute,
            left: ctx.u(96.0),
            top: ctx.u(40.0),
            bottom: ctx.u(96.0),
            width: px(m.width),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|col| {
            col.spawn(Node { flex_direction: FlexDirection::Column, margin: UiRect::bottom(ctx.u(if title_screen { 34.0 } else { 26.0 })), flex_shrink: 0.0, ..default() }).with_children(|h| head(h, ctx, m, title_screen, 46.0));
            if let Some((pages, on)) = m.pages {
                col.spawn(Node { column_gap: ctx.u(44.0), ..default() }).with_children(|body| {
                    tabs_down(body, ctx, pages, on);
                    body.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.0,
                        row_gap: ctx.u(6.0),
                        border: UiRect::left(ctx.u(2.0)),
                        padding: UiRect::left(ctx.u(34.0)),
                        ..default()
                    })
                    .insert(BorderColor::all(white(0.1)))
                    .with_children(|l| list(l, ctx, m));
                });
            } else {
                col.spawn(Node { flex_direction: FlexDirection::Column, row_gap: ctx.u(if m.rows == Rows::Menu { 2.0 } else { 6.0 }), ..default() }).with_children(|l| list(l, ctx, m));
            }
        });
        prompt_bar(root, ctx, &m.prompts, Some(ctx.u(96.0)));
        if title_screen {
            // this week's jam and the version, in the corner
            root.spawn(Node { position_type: PositionType::Absolute, right: ctx.u(36.0), bottom: ctx.u(30.0), flex_direction: FlexDirection::Column, align_items: AlignItems::FlexEnd, row_gap: ctx.u(4.0), ..default() }).with_children(|c| {
                if let Some(j) = &focus.jam {
                    c.spawn((Text::new("This week\u{2019}s jam: "), ctx.plain(14.0), TextColor(SOFT), ctx.shadow(1.0, 0.5))).with_child((TextSpan::new(j.theme.clone()), ctx.plain(14.0), TextColor(GOLD)));
                }
                c.spawn((Text::new(crate::share::version()), ctx.plain(12.0), TextColor(FAINT)));
            });
        }
        let _ = info;
    });
}

/// Pages down the left of Settings: the open one in gold.
fn tabs_down(p: &mut ChildSpawnerCommands, ctx: &Ctx, pages: &[&str], on: usize) {
    p.spawn(Node { flex_direction: FlexDirection::Column, width: ctx.u(230.0), flex_shrink: 0.0, row_gap: ctx.u(2.0), ..default() }).with_children(|t| {
        for (n, name) in pages.iter().enumerate() {
            t.spawn((Interaction::default(), MenuTab(n), Node { padding: UiRect::vertical(ctx.u(8.0)), align_items: AlignItems::Center, column_gap: ctx.u(12.0), ..default() })).with_children(|b| {
                b.spawn((Text::new(*name), ctx.round(22.0, 600), TextColor(if n == on { GOLD } else { FAINT })));
                if n == on {
                    b.spawn((Node { width: ctx.u(8.0), height: ctx.u(8.0), ..default() }, BackgroundColor(GOLD)));
                }
            });
        }
    });
}

/// Pages across the top of a sheet: the open one a cream tab.
fn tabs_across(p: &mut ChildSpawnerCommands, ctx: &Ctx, pages: &[&str], on: usize) {
    p.spawn(Node { align_items: AlignItems::Center, column_gap: ctx.u(6.0), ..default() }).with_children(|t| {
        glyph(t, ctx, if ctx.pad { "LB" } else { "" });
        for (n, name) in pages.iter().enumerate() {
            t.spawn((
                Interaction::default(),
                MenuTab(n),
                Node { padding: UiRect::axes(ctx.u(14.0), ctx.u(7.0)), border_radius: BorderRadius::all(ctx.u(8.0)), ..default() },
                BackgroundColor(if n == on { INK } else { Color::NONE }),
            ))
            .with_child((Text::new(*name), ctx.round(18.0, 600), TextColor(if n == on { DARK } else { FAINT })));
        }
        glyph(t, ctx, if ctx.pad { "RB" } else { "Tab" });
    });
}

/// The list's rows, from the first that fits to the last, with "more" marks.
fn list(p: &mut ChildSpawnerCommands, ctx: &Ctx, m: &Menu) {
    let from = m.first.max(m.skip);
    if from > m.skip {
        p.spawn((Text::new("\u{25b2} More above"), ctx.plain(12.0), TextColor(FAINT)));
    }
    let mut i = from;
    while i < m.end {
        if matches!(m.items[i].kind, Kind::Chip(_)) {
            // a run of chips wraps in rows
            let start = i;
            while i < m.end && matches!(m.items[i].kind, Kind::Chip(_)) {
                i += 1;
            }
            p.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: ctx.u(8.0), row_gap: ctx.u(8.0), margin: UiRect::vertical(ctx.u(4.0)), ..default() }).with_children(|r| {
                for n in start..i {
                    item(r, ctx, n, &m.items[n], n == m.sel, m.rows, n > 0 && m.items[n - 1].selectable());
                }
            });
            continue;
        }
        item(p, ctx, i, &m.items[i], i == m.sel, m.rows, i > 0 && is_row(&m.items[i - 1]));
        i += 1;
    }
    if m.end < m.items.len() {
        p.spawn((Text::new("\u{25bc} More below"), ctx.plain(12.0), TextColor(FAINT)));
    }
}

fn is_row(i: &Item) -> bool {
    matches!(i.kind, Kind::Slider { .. } | Kind::Toggle { .. } | Kind::Swatch { .. } | Kind::Field { .. } | Kind::Choice { .. })
}

/// One item of a list. A note right under a setting is its hint.
fn item(p: &mut ChildSpawnerCommands, ctx: &Ctx, i: usize, item: &Item, picked: bool, rows: Rows, after_row: bool) {
    match &item.kind {
        Kind::Note => {
            if item.label.is_empty() {
                return;
            }
            if after_row && rows == Rows::Values {
                p.spawn((Text::new(item.label.clone()), ctx.plain(14.0), TextColor(FAINT), Node { margin: UiRect::new(px(0), px(0), ctx.u(-4.0), ctx.u(4.0)), ..default() }));
            } else {
                p.spawn((Text::new(item.label.clone()), ctx.plain(15.5), TextColor(SOFT), Node { margin: UiRect::vertical(ctx.u(4.0)), ..default() }));
            }
        }
        Kind::Line { color, words } => {
            let font = ctx.plain(16.0);
            p.spawn((Text::new(format!("{}  ", item.label)), font.clone(), TextColor(Color::srgb_u8(color[0], color[1], color[2])))).with_child((TextSpan::new(words.clone()), font, TextColor(INK)));
        }
        Kind::Qr => {
            if let Some(h) = &ctx.qr {
                p.spawn((ImageNode::new(h.clone()), Node { width: ctx.u(230.0), height: ctx.u(230.0), flex_shrink: 0.0, border_radius: BorderRadius::all(ctx.u(12.0)), ..default() }));
            }
        }
        Kind::Big => {
            p.spawn((Text::new(item.label.clone()), ctx.round(60.0, 700), TextColor(MINT), ctx.shadow(3.0, 0.3), Node { margin: UiRect::vertical(ctx.u(4.0)), ..default() }));
        }
        Kind::Chip(_) => {
            p.spawn((
                Interaction::default(),
                MenuButton(i),
                Node { padding: UiRect::axes(ctx.u(14.0), ctx.u(7.0)), border_radius: BorderRadius::all(ctx.u(16.0)), border: UiRect::all(ctx.u(2.0)), ..default() },
                BorderColor::all(if picked { MINT } else { Color::NONE }),
                BackgroundColor(if picked { mint(0.25) } else { white(0.08) }),
            ))
            .with_child((Text::new(item.label.clone()), ctx.round(16.0, 600), TextColor(if picked { WHITE } else { INK })));
        }
        _ => row(p, ctx, i, item, picked, rows),
    }
}

/// A row you can choose: the label, and on the right what it's set to.
fn row(p: &mut ChildSpawnerCommands, ctx: &Ctx, i: usize, item: &Item, picked: bool, rows: Rows) {
    let (size, pad_y) = match rows {
        Rows::Menu => (25.0, 9.0),
        Rows::Values => (21.0, 10.0),
        Rows::Small => (19.0, 8.0),
    };
    let colour = match (picked, rows) {
        (true, _) => WHITE,
        (false, Rows::Menu) => SOFT,
        (false, _) => INK,
    };
    let mut b = p.spawn((
        Interaction::default(),
        MenuButton(i),
        Node {
            padding: UiRect::new(ctx.u(26.0), ctx.u(18.0), ctx.u(pad_y), ctx.u(pad_y)),
            margin: UiRect::left(ctx.u(-26.0)),
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            column_gap: ctx.u(16.0),
            flex_shrink: 0.0,
            ..default()
        },
    ));
    if picked {
        b.insert(BackgroundGradient(vec![LinearGradient::to_right(vec![ColorStop::percent(mint(0.34), 0.0), ColorStop::percent(mint(0.12), 55.0), ColorStop::percent(mint(0.0), 100.0)]).into()]));
    }
    b.with_children(|b| {
        if picked {
            // the bar that says which line is chosen
            b.spawn((
                Node { position_type: PositionType::Absolute, left: px(0), top: ctx.u(6.0), bottom: ctx.u(6.0), width: ctx.u(6.0), ..default() },
                BackgroundColor(MINT),
                BoxShadow(vec![ShadowStyle { color: mint(0.7), x_offset: px(0), y_offset: px(0), spread_radius: px(0), blur_radius: ctx.u(14.0) }]),
            ));
        }
        b.spawn((Text::new(item.label.clone()), ctx.round(size, 600), TextColor(colour), Node { flex_shrink: 1.0, ..default() }));
        if rows != Rows::Menu {
            value(b, ctx, item, picked);
        }
    });
}

/// What a row is set to: block cells, a switch, a choice, a colour or a field.
fn value(b: &mut ChildSpawnerCommands, ctx: &Ctx, item: &Item, picked: bool) {
    let arrow = |b: &mut ChildSpawnerCommands, s: &str| {
        b.spawn((Text::new(s), ctx.round(22.0, 600), TextColor(if picked { SOFT } else { FAINT })));
    };
    match &item.kind {
        Kind::Slider { value, min, max, .. } => {
            let f = ((value - min) / (max - min)).clamp(0.0, 1.0) as f32;
            let on = (f * 12.0).round() as usize;
            b.spawn(Node { align_items: AlignItems::Center, column_gap: ctx.u(14.0), flex_shrink: 0.0, ..default() }).with_children(|v| {
                v.spawn(Node { column_gap: ctx.u(3.0), ..default() }).with_children(|cells| {
                    for n in 0..12 {
                        cells.spawn((Node { width: ctx.u(11.0), height: ctx.u(14.0), ..default() }, BackgroundColor(if n < on { MINT } else { white(0.18) })));
                    }
                });
                let num = if *min >= 0.0 && *max <= 1.0 { format!("{}%", (value * 100.0).round()) } else { trim_num(*value) };
                v.spawn((Text::new(num), ctx.round(16.0, 600), TextColor(MINT), TextLayout::justify(Justify::Right), Node { min_width: ctx.u(44.0), ..default() }));
            });
        }
        Kind::Toggle { value, .. } => {
            b.spawn((Node { width: ctx.u(46.0), height: ctx.u(24.0), border_radius: BorderRadius::all(ctx.u(4.0)), flex_shrink: 0.0, ..default() }, BackgroundColor(if *value { MINT } else { white(0.18) }))).with_child((
                Node { position_type: PositionType::Absolute, top: ctx.u(4.0), left: ctx.u(if *value { 26.0 } else { 4.0 }), width: ctx.u(16.0), height: ctx.u(16.0), border_radius: BorderRadius::all(ctx.u(2.0)), ..default() },
                BackgroundColor(if *value { WHITE } else { Color::srgb(0.84, 0.86, 0.89) }),
            ));
        }
        Kind::Choice { value, .. } => {
            b.spawn(Node { align_items: AlignItems::Center, column_gap: ctx.u(12.0), flex_shrink: 0.0, ..default() }).with_children(|v| {
                arrow(v, "\u{2039}");
                v.spawn((Text::new(value.clone()), ctx.round(19.0, 600), TextColor(INK)));
                arrow(v, "\u{203a}");
            });
        }
        Kind::Swatch { value, .. } => {
            let col = crate::script::parse_color(value).unwrap_or(Color::WHITE);
            b.spawn(Node { align_items: AlignItems::Center, column_gap: ctx.u(12.0), flex_shrink: 0.0, ..default() }).with_children(|v| {
                arrow(v, "\u{2039}");
                v.spawn((Node { width: ctx.u(52.0), height: ctx.u(20.0), border_radius: BorderRadius::all(ctx.u(4.0)), ..default() }, BackgroundColor(col)));
                arrow(v, "\u{203a}");
            });
        }
        Kind::Field { key, value } => {
            // a PIN shows as dots
            let shown = if key.ends_with("pin") { "\u{2022}".repeat(value.chars().count()) } else { value.clone() };
            b.spawn((
                Node { padding: UiRect::axes(ctx.u(12.0), ctx.u(6.0)), min_width: ctx.u(200.0), max_width: ctx.u(380.0), border_radius: BorderRadius::all(ctx.u(6.0)), border: UiRect::bottom(ctx.u(2.0)), flex_shrink: 0.0, ..default() },
                BackgroundColor(white(if picked { 0.12 } else { 0.07 })),
                BorderColor::all(if picked { MINT } else { white(0.14) }),
            ))
            .with_children(|f| {
                f.spawn((Text::new(shown), ctx.plain(16.0), TextColor(INK))).with_child((TextSpan::new(if picked { "|" } else { "" }), ctx.plain(16.0), TextColor(MINT)));
            });
        }
        Kind::Button(Act::OpenControls | Act::OpenDevices | Act::OpenParents | Act::Market(..)) => arrow(b, "\u{203a}"),
        _ => {}
    }
}

/// A button's glyph: a coloured face button, a bumper, or a key.
fn glyph(p: &mut ChildSpawnerCommands, ctx: &Ctx, name: &str) {
    if name.is_empty() {
        return;
    }
    let face = |c: Color, ink: Color| (c, ink);
    match name {
        "A" | "B" | "X" | "Y" => {
            let (bg, ink) = match name {
                "A" => face(Color::srgb(0.36, 0.68, 0.22), WHITE),
                "B" => face(Color::srgb(0.88, 0.27, 0.2), WHITE),
                "X" => face(Color::srgb(0.2, 0.47, 0.86), WHITE),
                _ => face(Color::srgb(0.96, 0.73, 0.14), DARK),
            };
            p.spawn((
                Node { width: ctx.u(24.0), height: ctx.u(24.0), border_radius: BorderRadius::all(ctx.u(12.0)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, flex_shrink: 0.0, ..default() },
                BackgroundColor(bg),
            ))
            .with_child((Text::new(name), ctx.round(14.0, 700), TextColor(ink)));
        }
        _ => {
            p.spawn((
                Node { padding: UiRect::axes(ctx.u(7.0), ctx.u(2.0)), min_width: ctx.u(24.0), height: ctx.u(22.0), border_radius: BorderRadius::all(ctx.u(5.0)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, flex_shrink: 0.0, ..default() },
                BackgroundColor(Color::srgba(0.98, 0.96, 0.9, 0.9)),
            ))
            .with_child((Text::new(name), ctx.round(13.0, 600), TextColor(DARK)));
        }
    }
}

/// The prompts along the bottom; each does what it says when clicked.
fn prompt_bar(p: &mut ChildSpawnerCommands, ctx: &Ctx, prompts: &[(MenuPrompt, &str, &str, &str)], left: Option<Val>) {
    let mut node = Node { align_items: AlignItems::Center, column_gap: ctx.u(22.0), flex_shrink: 0.0, ..default() };
    if let Some(l) = left {
        node.position_type = PositionType::Absolute;
        node.left = l;
        node.bottom = ctx.u(34.0);
    } else {
        node.margin = UiRect::top(ctx.u(16.0));
    }
    p.spawn(node).with_children(|bar| {
        for (does, pad, key, label) in prompts {
            bar.spawn((Interaction::default(), *does, Node { align_items: AlignItems::Center, column_gap: ctx.u(7.0), ..default() })).with_children(|b| {
                for g in (if ctx.pad { *pad } else { *key }).split(' ') {
                    glyph(b, ctx, g);
                }
                b.spawn((Text::new(*label), ctx.round(16.0, 600), TextColor(SOFT), ctx.shadow(1.0, 0.4)));
            });
        }
    });
}

/// The sheet: a near-solid card over the softened world.
#[allow(clippy::too_many_arguments)]
fn sheet(commands: &mut Commands, ctx: &Ctx, m: &Menu, focus: &Focus, lib: &Library, photos: &crate::photos::Photos, side: Option<Side>, grid: &Grid, cards: &[usize]) {
    let full = grid.on || side.is_some() || focus.screen == Screen::Library;
    let mut r = root(commands);
    r.insert((BackgroundColor(slate(0.32)), Node { width: percent(100), height: percent(100), position_type: PositionType::Absolute, justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }));
    r.with_children(|root| {
        let mut node = Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::new(ctx.u(30.0), ctx.u(30.0), ctx.u(26.0), ctx.u(22.0)),
            border_radius: BorderRadius::all(ctx.u(22.0)),
            overflow: Overflow::clip(),
            ..default()
        };
        if full {
            node.position_type = PositionType::Absolute;
            node.left = ctx.u(40.0);
            node.right = ctx.u(40.0);
            node.top = ctx.u(24.0);
            node.bottom = ctx.u(24.0);
            node.padding = UiRect::new(ctx.u(30.0), ctx.u(30.0), ctx.u(22.0), ctx.u(18.0));
        } else {
            node.width = px(m.width + 60.0 * ctx.s);
            node.max_height = percent(94);
        }
        root.spawn((node, BackgroundColor(slate(0.965)), BoxShadow(vec![ShadowStyle { color: Color::srgba(0.0, 0.0, 0.0, 0.45), x_offset: px(0), y_offset: ctx.u(20.0), spread_radius: px(0), blur_radius: ctx.u(60.0) }]))).with_children(|sh| {
            if focus.screen == Screen::Library {
                creations(sh, ctx, m, focus, lib, photos, grid, cards);
            } else if let Some(side) = side {
                inside(sh, ctx, m, side, focus.scroll);
            } else {
                sh.spawn(Node { flex_direction: FlexDirection::Column, margin: UiRect::bottom(ctx.u(18.0)), flex_shrink: 0.0, ..default() }).with_children(|h| head(h, ctx, m, false, 40.0));
                sh.spawn(Node { flex_direction: FlexDirection::Column, row_gap: ctx.u(4.0), padding: UiRect::left(ctx.u(4.0)), flex_shrink: 1.0, overflow: Overflow::clip(), ..default() }).with_children(|l| {
                    // the market's QR code beside what it's for
                    if let Some(q) = m.items.iter().position(|i| i.kind == Kind::Qr) {
                        l.spawn(Node { column_gap: ctx.u(26.0), align_items: AlignItems::Center, margin: UiRect::bottom(ctx.u(14.0)), ..default() }).with_children(|w| {
                            item(w, ctx, q, &m.items[q], false, m.rows, false);
                            w.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, flex_shrink: 1.0, row_gap: ctx.u(8.0), ..default() }).with_children(|t| {
                                t.spawn((Text::new("ON A PHONE"), ctx.plain(12.5), TextColor(MINT), LetterSpacing::Px(1.4 * ctx.s)));
                                if let Some(n) = m.items.get(q + 1).filter(|i| i.kind == Kind::Note) {
                                    t.spawn((Text::new(n.label.clone()), ctx.plain(16.0), TextColor(SOFT)));
                                }
                            });
                        });
                        let rest = Menu { first: (q + 2).max(m.first), skip: (q + 2).max(m.skip), ..clone_menu(m) };
                        list(l, ctx, &rest);
                    } else {
                        list(l, ctx, m);
                    }
                });
            }
            prompt_bar(sh, ctx, &m.prompts, None);
        });
    });
}

fn clone_menu<'a>(m: &Menu<'a>) -> Menu<'a> {
    Menu { title: m.title.clone(), items: m.items, skip: m.skip, first: m.first, end: m.end, sel: m.sel, kicker: m.kicker.clone(), sub: m.sub.clone(), rows: m.rows, width: m.width, pages: m.pages, prompts: m.prompts.clone() }
}

/// Look inside: what it is and what you can do on the left, its code on the right.
fn inside(sh: &mut ChildSpawnerCommands, ctx: &Ctx, m: &Menu, side: Side, scroll: i32) {
    sh.spawn(Node { column_gap: ctx.u(28.0), flex_grow: 1.0, min_height: px(0), ..default() }).with_children(|body| {
        body.spawn(Node { flex_direction: FlexDirection::Column, width: px(m.width), flex_shrink: 0.0, ..default() }).with_children(|left| {
            left.spawn(Node { flex_direction: FlexDirection::Column, margin: UiRect::bottom(ctx.u(14.0)), ..default() }).with_children(|h| head(h, ctx, m, false, 40.0));
            left.spawn(Node { flex_direction: FlexDirection::Column, row_gap: ctx.u(2.0), padding: UiRect::left(ctx.u(4.0)), ..default() }).with_children(|l| list(l, ctx, m));
        });
        body.spawn((
            Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, min_width: px(0), padding: UiRect::all(ctx.u(20.0)), row_gap: ctx.u(6.0), border_radius: BorderRadius::all(ctx.u(14.0)), overflow: Overflow::clip(), ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.28)),
        ))
        .with_children(|p| {
            p.spawn((Text::new(format!("{}  \u{b7}  loaded in {:.0} ms  \u{b7}  wheel or LB and RB to scroll", side.file, side.ms)), ctx.plain(13.0), TextColor(FAINT)));
            if let Some(b) = side.budget {
                p.spawn((Text::new(cap(&b)), ctx.plain(13.0), TextColor(FAINT)));
            }
            let lines: Vec<&str> = side.src.lines().collect();
            let start = (scroll as usize).min(lines.len().saturating_sub(1));
            let shown: String = lines.iter().enumerate().skip(start).take(34).map(|(n, l)| format!("{:>3}  {}\n", n + 1, l.replace('\t', "    "))).collect();
            p.spawn((Text::new(shown), ctx.mono(15.0), TextColor(Color::srgb(0.85, 0.9, 0.86)), TextLayout::no_wrap(), Node { flex_shrink: 1.0, overflow: Overflow::clip(), ..default() }));
            if let Some(e) = side.err {
                p.spawn((Text::new(format!("Problem: {e}")), ctx.plain(15.0), TextColor(RED)));
            }
            if !side.log.is_empty() {
                p.spawn((Text::new(format!("Log\n{}", side.log.join("\n"))), ctx.mono(13.0), TextColor(SOFT)));
            }
        });
    });
}

/// The Creations sheet: shelves across the top, cards with pictures, and
/// the chosen one's details beside them.
#[allow(clippy::too_many_arguments)]
fn creations(sh: &mut ChildSpawnerCommands, ctx: &Ctx, m: &Menu, focus: &Focus, lib: &Library, photos: &crate::photos::Photos, grid: &Grid, cards: &[usize]) {
    sh.spawn(Node { align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, column_gap: ctx.u(20.0), flex_shrink: 0.0, ..default() }).with_children(|top| {
        top.spawn((Text::new(m.title.clone()), ctx.round(36.0, 700), TextColor(INK), ctx.shadow(3.0, 0.28)));
        if let Some((pages, on)) = m.pages {
            tabs_across(top, ctx, pages, on);
        }
    });
    let chosen = match m.items.get(m.sel).map(|i| &i.kind) {
        Some(Kind::Card(Act::Inside(id))) => lib.list.get(id),
        _ => None,
    };
    sh.spawn(Node { column_gap: ctx.u(24.0), flex_grow: 1.0, min_height: px(0), margin: UiRect::top(ctx.u(16.0)), ..default() }).with_children(|body| {
        body.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, min_width: px(0), row_gap: ctx.u(12.0), ..default() }).with_children(|left| {
            for it in m.items.iter().filter(|i| i.kind == Kind::Note) {
                // this week's jam, or why a shelf is empty
                left.spawn((Node { padding: UiRect::axes(ctx.u(14.0), ctx.u(10.0)), border_radius: BorderRadius::all(ctx.u(10.0)), flex_shrink: 0.0, ..default() }, BackgroundColor(Color::srgba(1.0, 0.77, 0.36, 0.12))))
                    .with_child((Text::new(it.label.clone()), ctx.plain(14.5), TextColor(SOFT)));
            }
            left.spawn(Node { display: Display::Grid, grid_template_columns: RepeatedGridTrack::flex(grid.cols as u16, 1.0), column_gap: ctx.u(14.0), row_gap: ctx.u(14.0), padding: UiRect::all(ctx.u(4.0)), margin: UiRect::all(ctx.u(-4.0)), ..default() }).with_children(|g| {
                for k in m.first..m.end {
                    let Some(&n) = cards.get(k) else { continue };
                    let Kind::Card(Act::Inside(id)) = &m.items[n].kind else { continue };
                    let Some(c) = lib.list.get(id) else { continue };
                    card(g, ctx, n, c, n == m.sel, lib.running.contains(id), photos.get(id));
                }
            });
            let more = (m.first > 0, m.end < cards.len());
            if more.0 || more.1 {
                let rows = cards.len().div_ceil(grid.cols);
                left.spawn((Text::new(format!("Row {} of {}", m.first / grid.cols + 1, rows.max(1))), ctx.plain(12.0), TextColor(FAINT)));
            }
        });
        if let Some(c) = chosen {
            detail(body, ctx, c, lib.running.contains(&c.id), photos.get(&c.id));
        }
    });
    let _ = focus;
}

/// The colours behind a creation that has no picture yet, by kind.
fn kind_colours(kind: &str) -> (Color, Color) {
    match kind {
        "creature" => (Color::srgb(0.31, 0.48, 0.23), Color::srgb(0.18, 0.36, 0.31)),
        "game" => (Color::srgb(0.85, 0.55, 0.2), Color::srgb(0.55, 0.27, 0.16)),
        "mod" => (Color::srgb(0.23, 0.44, 0.69), Color::srgb(0.17, 0.25, 0.45)),
        _ => (Color::srgb(0.48, 0.38, 0.7), Color::srgb(0.25, 0.2, 0.42)),
    }
}

/// A creation's picture, or its kind's colours and its first letter.
fn picture(p: &mut ChildSpawnerCommands, ctx: &Ctx, c: &crate::creations::Creation, photo: Option<Handle<Image>>, here: bool, letter: f32) {
    let (a, b) = kind_colours(&c.manifest.kind);
    let mut pic = p.spawn((
        Node { width: percent(100), aspect_ratio: Some(PICTURE), justify_content: JustifyContent::Center, align_items: AlignItems::Center, flex_shrink: 0.0, overflow: Overflow::clip(), ..default() },
        BackgroundGradient(vec![LinearGradient::to_bottom_right(vec![ColorStop::percent(a, 0.0), ColorStop::percent(b, 100.0)]).into()]),
    ));
    pic.with_children(|pic| {
        match photo {
            Some(h) => {
                pic.spawn((ImageNode::new(h), Node { position_type: PositionType::Absolute, left: px(0), top: px(0), width: percent(100), height: percent(100), ..default() }));
            }
            None => {
                let first: String = c.manifest.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
                pic.spawn((Text::new(first), ctx.round(letter, 700), TextColor(Color::srgba(0.98, 0.96, 0.9, 0.85)), TextShadow { offset: Vec2::splat(4.0 * ctx.s), color: Color::srgba(0.0, 0.0, 0.0, 0.25) }));
            }
        }
        if here {
            pic.spawn((
                Node { position_type: PositionType::Absolute, left: ctx.u(8.0), top: ctx.u(8.0), padding: UiRect::axes(ctx.u(7.0), ctx.u(3.0)), border_radius: BorderRadius::all(ctx.u(5.0)), ..default() },
                BackgroundColor(MINT),
            ))
            .with_child((Text::new("IN THIS WORLD"), ctx.plain(10.5), TextColor(Color::srgb(0.05, 0.16, 0.14)), LetterSpacing::Px(0.6 * ctx.s)));
        }
    });
}

fn card(g: &mut ChildSpawnerCommands, ctx: &Ctx, n: usize, c: &crate::creations::Creation, picked: bool, here: bool, photo: Option<Handle<Image>>) {
    let mut e = g.spawn((
        Interaction::default(),
        MenuButton(n),
        Node { flex_direction: FlexDirection::Column, border_radius: BorderRadius::all(ctx.u(14.0)), overflow: Overflow::clip(), min_width: px(0), ..default() },
        BackgroundColor(white(0.05)),
    ));
    if picked {
        e.insert((Outline::new(ctx.u(3.0), px(0), MINT), BoxShadow(vec![ShadowStyle { color: mint(0.35), x_offset: px(0), y_offset: px(0), spread_radius: px(0), blur_radius: ctx.u(24.0) }])));
    }
    e.with_children(|card| {
        picture(card, ctx, c, photo, here, 64.0);
        card.spawn(Node { flex_direction: FlexDirection::Column, padding: UiRect::new(ctx.u(12.0), ctx.u(12.0), ctx.u(8.0), ctx.u(9.0)), row_gap: ctx.u(1.0), ..default() }).with_children(|meta| {
            meta.spawn((Text::new(c.manifest.name.clone()), ctx.round(17.0, 600), TextColor(if picked { WHITE } else { INK }), TextLayout::no_wrap()));
            meta.spawn((Text::new(format!("{} \u{b7} by {}", kind_name(&c.manifest.kind), c.manifest.by)), ctx.plain(12.5), TextColor(FAINT), TextLayout::no_wrap()));
        });
    });
}

/// The chosen creation, bigger, with what it is and where it's from.
fn detail(body: &mut ChildSpawnerCommands, ctx: &Ctx, c: &crate::creations::Creation, here: bool, photo: Option<Handle<Image>>) {
    let m = &c.manifest;
    body.spawn((
        Node { flex_direction: FlexDirection::Column, width: ctx.u(Grid::DETAIL), flex_shrink: 0.0, border_radius: BorderRadius::all(ctx.u(16.0)), overflow: Overflow::clip(), ..default() },
        BackgroundColor(white(0.05)),
    ))
    .with_children(|d| {
        picture(d, ctx, c, photo, false, 96.0);
        d.spawn(Node { flex_direction: FlexDirection::Column, padding: UiRect::all(ctx.u(18.0)), row_gap: ctx.u(10.0), ..default() }).with_children(|t| {
            t.spawn((Text::new(format!("{} by {}", kind_name(&m.kind), m.by).to_uppercase()), ctx.plain(12.5), TextColor(MINT), LetterSpacing::Px(1.4 * ctx.s)));
            t.spawn((Text::new(m.name.clone()), ctx.round(30.0, 700), TextColor(INK)));
            if !m.about.is_empty() {
                t.spawn((Text::new(m.about.clone()), ctx.plain(15.0), TextColor(SOFT)));
            }
            let mut facts = vec![];
            if here {
                facts.push("In this world".to_string());
            }
            if let Some(r) = &m.remix_of {
                facts.push(format!("A remix of {r}"));
            }
            facts.push(format!("Licence {}", m.licence));
            facts.push(format!("{} files \u{b7} {:.1} KB", c.files.len(), c.size() as f32 / 1024.0));
            t.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: ctx.u(6.0), row_gap: ctx.u(6.0), ..default() }).with_children(|f| {
                for fact in facts {
                    f.spawn((Node { padding: UiRect::axes(ctx.u(8.0), ctx.u(4.0)), border_radius: BorderRadius::all(ctx.u(6.0)), ..default() }, BackgroundColor(white(0.08)))).with_child((Text::new(fact), ctx.plain(12.0), TextColor(SOFT)));
                }
            });
        });
    });
}

/// The map's list and chat: the same rows, in the panel they've always had.
fn panel(commands: &mut Commands, ctx: &Ctx, m: &Menu, look: Look) {
    let on_map = look == Look::Map;
    let (justify, align, dim, pad) = if on_map {
        (JustifyContent::FlexEnd, AlignItems::Center, Color::NONE, UiRect::all(px(20)))
    } else {
        (JustifyContent::FlexStart, AlignItems::FlexEnd, Color::srgba(0.0, 0.0, 0.0, 0.15), UiRect::all(px(18)))
    };
    let mut r = root(commands);
    r.insert((Node { width: percent(100), height: percent(100), position_type: PositionType::Absolute, justify_content: justify, align_items: align, padding: pad, ..default() }, BackgroundColor(dim)));
    r.with_children(|root| {
        root.spawn((
            Node { flex_direction: FlexDirection::Column, width: px(m.width), max_height: percent(100), padding: UiRect::new(px(26), px(22), px(22), px(20)), row_gap: px(4), border_radius: BorderRadius::all(px(18)), overflow: Overflow::clip(), ..default() },
            BackgroundColor(slate(0.88)),
        ))
        .with_children(|p| {
            if !m.title.is_empty() {
                p.spawn((Text::new(m.title.clone()), ctx.round(34.0, 700), TextColor(INK), ctx.shadow(2.0, 0.28), Node { margin: UiRect::bottom(px(8)), ..default() }));
            }
            list(p, ctx, m);
            if !on_map {
                prompt_bar(p, ctx, &m.prompts, None);
            }
        });
    });
}

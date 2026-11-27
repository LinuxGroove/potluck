//! Chat in a world. Every line goes through the world's host, so chat works
//! in a friend's world on a LAN with no internet as well as on a server.
//! The host tidies and filters each line, keeps anyone from flooding it,
//! holds younger players to quick lines, and sends a short history to
//! whoever joins. Places go in chat too: share where you are, and it shows
//! on everyone's map to go to or keep.
//!
//! Lines are shaped for a bridge to another chat later (the LinuxGroove
//! Fluxer server): each says where it came from (`Source`), `Line::may_leave`
//! says whether it may go out of the world, and `Chat::inbox` takes lines
//! from outside. Nothing crosses to or from a younger player: their lines
//! stay in the world, and bridged lines never reach their games.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::net::{Net, Role};
use crate::settings::Settings;
use crate::ui::{Fonts, Focus, Screen, WorldInfo, DIM, GOLD, INK, MINT};

/// The longest line, in letters.
pub const MAX_LEN: usize = 160;
/// How wide the chat screen is, at the bottom left over the world.
pub const WIDTH: f32 = 600.0;
/// Lines each game keeps.
const KEEP: usize = 100;
/// Lines a player gets when they join.
const HISTORY: usize = 30;
/// At most this many lines in this many seconds from one player.
const RATE: (usize, f64) = (5, 8.0);
/// Lines shown over the world while playing, and for how long.
const FEED: usize = 6;
const FEED_SECS: f64 = 12.0;

/// What younger players say (and anyone can, with a pad and no typing).
pub const QUICK: &[&str] = &[
    "Hi!",
    "Over here!",
    "Follow me",
    "Wait for me",
    "Look at this!",
    "Nice build!",
    "Want to play?",
    "Thanks!",
    "Help!",
    "Yes",
    "No",
    "Be right back",
    "Bye!",
];

/// A place shared in chat.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    pub at: [i32; 3],
}

/// Who a line is from.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Source {
    /// A player in this world, by their id here.
    Player(u32),
    /// A creation, by its id: always shown as from the creation.
    Creation(String),
    /// Another chat linked to this world, by its name ("Fluxer").
    Bridge(String),
    /// The world itself.
    World,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Line {
    /// The host's number for it, in order.
    pub id: u64,
    pub name: String,
    pub from: Source,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<Place>,
    /// Said by a younger player: it stays in the world.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub young: bool,
    /// Unix seconds.
    pub at: i64,
}

impl Line {
    /// Whether it may go out of the world, to a bridge: players' own lines,
    /// never a younger player's.
    pub fn may_leave(&self) -> bool {
        matches!(self.from, Source::Player(_)) && !self.young
    }

    /// Whether a younger player's game may show it: anything but a bridge's.
    pub fn for_young(&self) -> bool {
        !matches!(self.from, Source::Bridge(_))
    }

    /// The name as chat shows it.
    pub fn who(&self) -> String {
        match &self.from {
            Source::Creation(_) => format!("{} (a creation)", self.name),
            Source::Bridge(on) => format!("{} on {on}", self.name),
            _ => self.name.clone(),
        }
    }

    /// The words as chat shows them.
    pub fn words(&self) -> String {
        match &self.place {
            Some(p) if self.text.is_empty() => format!("shared {} ({}, {}, {}), on your map", p.name, p.at[0], p.at[1], p.at[2]),
            Some(p) => format!("{} · {} ({}, {}, {}), on your map", self.text, p.name, p.at[0], p.at[1], p.at[2]),
            None => self.text.clone(),
        }
    }
}

/// What a player asks to say.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Say {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<Place>,
}

/// Words chat hides, from `assets/chat_words.txt`: whole words, and words
/// starting with a listed one (`word*`), after folding look-alikes.
pub struct Words {
    whole: HashSet<String>,
    starts: Vec<String>,
}

impl Words {
    pub fn parse(text: &str) -> Words {
        let mut w = Words { whole: HashSet::new(), starts: vec![] };
        for line in text.lines() {
            let l = line.split('#').next().unwrap_or("").trim().to_lowercase();
            if l.is_empty() {
                continue;
            }
            match l.strip_suffix('*') {
                Some(p) => w.starts.push(fold(p)),
                None => {
                    w.whole.insert(fold(&l));
                }
            }
        }
        w
    }

    fn listed(&self, word: &str) -> bool {
        let f = fold(word);
        self.whole.contains(&f) || self.starts.iter().any(|s| f.starts_with(s.as_str()))
    }

    /// The text with every listed word starred out.
    pub fn mask(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut word = String::new();
        let flush = |word: &mut String, out: &mut String| {
            if !word.is_empty() {
                if self.listed(word) {
                    out.extend(word.chars().map(|_| '*'));
                } else {
                    out.push_str(word);
                }
                word.clear();
            }
        };
        for c in text.chars() {
            if c.is_alphanumeric() || "@$".contains(c) {
                word.push(c);
            } else {
                flush(&mut word, &mut out);
                out.push(c);
            }
        }
        flush(&mut word, &mut out);
        out
    }
}

impl Default for Words {
    fn default() -> Self {
        Words::parse(include_str!("../../../assets/chat_words.txt"))
    }
}

/// Lower case with number and symbol stand-ins turned back into letters and
/// runs of a letter squashed, as the market's checks fold words.
fn fold(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        let c = match c {
            '0' => 'o',
            '1' | '!' => 'i',
            '3' => 'e',
            '4' | '@' => 'a',
            '5' | '$' => 's',
            '7' => 't',
            c => c,
        };
        if out.chars().last() != Some(c) {
            out.push(c);
        }
    }
    out
}

/// One line's words tidied: no control characters, single spaces, cut to
/// `MAX_LEN` letters.
pub fn tidy(text: &str) -> String {
    let clean: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    clean.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(MAX_LEN).collect()
}

/// Chat on this game: the lines seen here, what this player said, and on
/// the host, what it needs to keep order.
#[derive(Resource)]
pub struct Chat {
    /// Lines seen here, oldest first, with when they arrived (seconds since
    /// the game started), for fading them out.
    pub lines: VecDeque<(Line, f64)>,
    /// What this game's own player said, for the host.
    pub out: Vec<Say>,
    /// Lines from outside the world (a bridge), for the host to pass on.
    pub inbox: Vec<Line>,
    /// Lines that may leave the world, for a bridge to take, oldest first.
    pub outbox: VecDeque<Line>,
    /// A server writes every line here as JSON, one a line, for its operator.
    pub log: Option<PathBuf>,
    next: u64,
    /// When each player last spoke (the host's), for keeping order.
    said: HashMap<u32, VecDeque<f64>>,
    words: Words,
    /// The world the lines are from (`Library::world`).
    world: u32,
    /// This game's player is younger: lines from outside the world stay hidden.
    pub young: bool,
    /// A parent turned chat off on this game: nothing shows.
    pub muted: bool,
}

impl Default for Chat {
    fn default() -> Self {
        Chat { lines: VecDeque::new(), out: vec![], inbox: vec![], outbox: VecDeque::new(), log: None, next: 1, said: HashMap::new(), words: Words::default(), world: 0, young: false, muted: false }
    }
}

/// Who's speaking, as the host knows them.
pub struct Speaker<'a> {
    pub id: u32,
    pub name: &'a str,
    pub young: bool,
}

impl Chat {
    /// The host's word on a line a player asked to say: the line, or why not.
    pub fn hear(&mut self, who: Speaker, say: Say, allowed: bool, now: f64) -> Result<Line, String> {
        if !allowed {
            return Err("Chat is off in this world.".into());
        }
        let mut text = tidy(&say.text);
        let place = match say.place {
            Some(p) => {
                let inside = crate::world::Voxels::inside(p.at[0], p.at[1].clamp(0, crate::world::WY - 1), p.at[2]);
                if !inside {
                    return Err("That place isn't in this world.".into());
                }
                let name: String = tidy(&p.name).chars().take(32).collect();
                Some(Place { name: self.words.mask(if name.is_empty() { "A place" } else { &name }), at: p.at })
            }
            None => None,
        };
        if text.is_empty() && place.is_none() {
            return Err(String::new());
        }
        // younger players say quick lines, which need no filter
        if who.young && !text.is_empty() && !QUICK.contains(&text.as_str()) {
            return Err("Younger players chat with quick lines.".into());
        }
        let times = self.said.entry(who.id).or_default();
        while times.front().is_some_and(|t| now - t > RATE.1) {
            times.pop_front();
        }
        if times.len() >= RATE.0 {
            return Err("Slow down a little: a few lines at a time.".into());
        }
        times.push_back(now);
        text = self.words.mask(&text);
        let line = Line { id: self.next, name: who.name.chars().take(20).collect(), from: Source::Player(who.id), text, place, young: who.young, at: crate::hosting::now() };
        self.next += 1;
        Ok(line)
    }

    /// A line from outside the world, numbered by this host.
    pub fn bridged(&mut self, mut line: Line) -> Line {
        line.id = self.next;
        self.next += 1;
        line.text = self.words.mask(&tidy(&line.text));
        line.young = false;
        line.place = None;
        line
    }

    /// Keeps a line seen here.
    pub fn keep(&mut self, line: Line, now: f64) {
        if self.lines.iter().any(|(l, _)| l.id == line.id && l.at == line.at && l.text == line.text) {
            return;
        }
        if let Some(path) = &self.log {
            write_log(path, &line);
        }
        if line.may_leave() {
            self.outbox.push_back(line.clone());
            if self.outbox.len() > KEEP {
                self.outbox.pop_front();
            }
        }
        self.lines.push_back((line, now));
        if self.lines.len() > KEEP {
            self.lines.pop_front();
        }
    }

    /// The last lines, for someone joining: younger players don't get a
    /// bridge's.
    pub fn history(&self, young: bool) -> Vec<Line> {
        let shown: Vec<&Line> = self.lines.iter().map(|(l, _)| l).filter(|l| !young || l.for_young()).collect();
        shown[shown.len().saturating_sub(HISTORY)..].iter().map(|l| (*l).clone()).collect()
    }

    /// Hides these words (one a line, as `assets/chat_words.txt`) instead.
    pub fn use_words(&mut self, list: &str) {
        self.words = Words::parse(list);
    }

    /// The lines this game's player sees, oldest first.
    pub fn seen(&self) -> impl DoubleEndedIterator<Item = &(Line, f64)> {
        self.lines.iter().filter(|(l, _)| !self.muted && (!self.young || l.for_young()))
    }

    /// Places others shared lately, newest first: (line id, who, place).
    pub fn places(&self, me: u32) -> Vec<(u64, String, Place)> {
        self.seen().rev().filter_map(|(l, _)| l.place.clone().filter(|_| l.from != Source::Player(me)).map(|p| (l.id, l.name.clone(), p))).take(8).collect()
    }
}

fn write_log(path: &std::path::Path, line: &Line) {
    // keep it to a few megabytes: the older half moves aside
    if std::fs::metadata(path).is_ok_and(|m| m.len() > 4 << 20) {
        let _ = std::fs::rename(path, path.with_extension("old.jsonl"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{}", serde_json::to_string(line).unwrap_or_default());
    }
}

/// Chat each frame: on the host (or alone), what players said becomes lines
/// for everyone; on a guest, what this player said goes to the host and
/// the host's lines arrive.
#[allow(clippy::too_many_arguments)]
pub fn run(
    time: Res<Time>,
    mut chat: ResMut<Chat>,
    mut net: ResMut<Net>,
    settings: Option<Res<Settings>>,
    lib: Res<crate::creations::Library>,
    mut toasts: ResMut<crate::ui::Toasts>,
    mut heard: ResMut<crate::sound::Heard>,
    me: Option<Res<crate::player::Me>>,
) {
    let now = time.elapsed_secs_f64();
    // a new world starts its chat afresh
    if chat.world != lib.world {
        chat.world = lib.world;
        chat.lines.clear();
        chat.said.clear();
    }
    let young = settings.as_ref().is_some_and(|s| s.young);
    let muted = settings.as_ref().is_some_and(|s| !s.chat);
    (chat.young, chat.muted) = (young, muted);
    let name = settings.as_ref().map(|s| s.name.clone()).unwrap_or_default();
    let before = chat.lines.back().map(|(l, _)| l.id);
    match &net.role {
        Role::Client(_) => {
            for say in std::mem::take(&mut chat.out) {
                if !muted {
                    net.say(&say);
                }
            }
            for line in net.take_chat() {
                if muted || young && !line.for_young() {
                    continue;
                }
                chat.keep(line, now);
            }
        }
        _ => {
            let allowed = net.rules().is_none_or(|r| r.chat);
            let mine = std::mem::take(&mut chat.out);
            let mut says: Vec<(u32, Say)> = if me.is_some() && !muted { mine.into_iter().map(|s| (net.my_id, s)).collect() } else { vec![] };
            says.extend(net.take_says());
            let mut lines = vec![];
            for (pid, say) in says {
                let (who, is_young) = if pid == net.my_id && me.is_some() { (name.clone(), young) } else { (net.name_of(pid).unwrap_or_default(), net.young_of(pid)) };
                match chat.hear(Speaker { id: pid, name: &who, young: is_young }, say, allowed, now) {
                    Ok(line) => lines.push(line),
                    Err(why) if why.is_empty() => {}
                    Err(why) if pid == net.my_id && me.is_some() => toasts.push(why),
                    Err(why) => net.tell(pid, &why),
                }
            }
            for line in std::mem::take(&mut chat.inbox) {
                let line = chat.bridged(line);
                lines.push(line);
            }
            for line in lines {
                net.send_chat(&line);
                // the host keeps every line (for joiners and a bridge); its own
                // player sees only theirs to see
                chat.keep(line, now);
            }
            // whoever just joined gets the last few lines
            for pid in net.take_welcomed() {
                let history = chat.history(net.young_of(pid));
                net.send_chat_to(pid, history);
            }
        }
    }
    // a soft sound for a line from someone else
    let fresh = chat.lines.back().is_some_and(|(l, _)| Some(l.id) != before && l.from != Source::Player(net.my_id) && (!young || l.for_young()));
    if fresh && me.is_some() && !muted {
        heard.0.push(crate::sound::Play { name: "pop".into(), at: None, volume: 0.12, pitch: 1.4, caption: Some(String::new()) });
    }
}

// ---------- on the screen ----------

/// A name's colour in chat: the same hue as the player's shirt.
pub fn name_color(line: &Line) -> Color {
    match &line.from {
        Source::Player(id) => Color::hsl((*id as f32 * 67.0) % 360.0, 0.7, 0.72),
        Source::Creation(_) => GOLD,
        Source::Bridge(_) => Color::srgb(0.72, 0.62, 1.0),
        Source::World => DIM,
    }
}

#[derive(Component)]
pub struct Feed;
/// A line over the world: its id and when it came.
#[derive(Component)]
pub struct FeedLine(u64, f64);
/// One coloured part of a line, at full strength.
#[derive(Component)]
pub struct FeedSpan(Color);

pub fn spawn_feed(mut commands: Commands) {
    commands.spawn((
        Feed,
        Node { position_type: PositionType::Absolute, left: px(18), bottom: px(18), max_width: percent(27), flex_direction: FlexDirection::Column, align_items: AlignItems::FlexStart, row_gap: px(4), ..default() },
        Pickable::IGNORE,
        Visibility::Hidden,
    ));
}

/// Lines over the world while playing, fading out after a while; a menu
/// (and the chat screen, which shows them itself) hides them.
#[allow(clippy::too_many_arguments)]
pub fn draw_feed(
    mut commands: Commands,
    time: Res<Time>,
    chat: Res<Chat>,
    focus: Res<Focus>,
    info: Res<WorldInfo>,
    fonts: Res<Fonts>,
    mut feed: Query<(Entity, &mut Visibility), With<Feed>>,
    rows: Query<(Entity, &FeedLine, &Children)>,
    kids: Query<&Children, Without<FeedLine>>,
    mut colors: Query<(&FeedSpan, Option<&mut TextColor>, Option<&mut BackgroundColor>)>,
) {
    let Ok((root, mut vis)) = feed.single_mut() else { return };
    let show = info.in_world && matches!(focus.screen, Screen::Playing | Screen::Panel);
    *vis = if show { Visibility::Inherited } else { Visibility::Hidden };
    let now = time.elapsed_secs_f64();
    let fresh: Vec<&(Line, f64)> = chat.seen().filter(|(_, t)| now - t < FEED_SECS + 2.0).collect();
    let fresh = &fresh[fresh.len().saturating_sub(FEED)..];
    let shown: Vec<u64> = rows.iter().map(|(_, l, _)| l.0).collect();
    let want: Vec<u64> = fresh.iter().map(|(l, _)| l.id).collect();
    if shown != want {
        for (e, _, _) in &rows {
            commands.entity(e).despawn();
        }
        for (line, t) in fresh {
            let row = commands
                .spawn((
                    FeedLine(line.id, *t),
                    Node { padding: UiRect::axes(px(8), px(3)), border_radius: BorderRadius::all(px(6)), ..default() },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
                    FeedSpan(Color::srgba(0.0, 0.0, 0.0, 0.45)),
                ))
                .with_children(|r| {
                    let name = name_color(line);
                    let words = if line.place.is_some() { MINT } else { INK };
                    r.spawn((Text::new(format!("{}  ", line.who())), TextFont { font: fonts.ui.clone().into(), font_size: FontSize::Px(15.0), ..default() }, TextColor(name), FeedSpan(name)))
                        .with_child((TextSpan::new(line.words()), TextFont { font: fonts.ui.clone().into(), font_size: FontSize::Px(15.0), ..default() }, TextColor(words), FeedSpan(words)));
                })
                .id();
            commands.entity(root).add_child(row);
        }
        return;
    }
    // older lines fade
    for (e, l, names) in &rows {
        let a = (1.0 - ((now - l.1 - FEED_SECS) / 2.0).clamp(0.0, 1.0)) as f32;
        // the row, its name and the words under the name
        let mut all = vec![e];
        for n in names {
            all.push(*n);
            all.extend(kids.get(*n).into_iter().flatten().copied());
        }
        for e in all {
            if let Ok((s, text, bg)) = colors.get_mut(e) {
                let c = s.0.with_alpha(s.0.alpha() * a);
                // a Text is a Node too, so it has a background of its own:
                // colour the words, or the row's background, never both
                if let Some(mut t) = text {
                    t.0 = c;
                } else if let Some(mut b) = bg {
                    b.0 = c;
                }
            }
        }
    }
}

/// What the chat screen shows: the last lines, as (name, its colour, words).
pub fn screen_lines(chat: &Chat, n: usize) -> Vec<(String, [u8; 3], String)> {
    let all: Vec<&(Line, f64)> = chat.seen().collect();
    all[all.len().saturating_sub(n)..]
        .iter()
        .map(|(l, _)| {
            let c = name_color(l).to_srgba();
            (l.who(), [(c.red * 255.0) as u8, (c.green * 255.0) as u8, (c.blue * 255.0) as u8], l.words())
        })
        .collect()
}

/// Lines the chat screen shows at most.
const SCREEN_LINES: usize = 12;

/// Opens chat, if this player may chat.
pub fn open(focus: &mut Focus, settings: &Settings, toasts: &mut crate::ui::Toasts) {
    if !settings.chat {
        toasts.push("A parent turned chat off.");
        return;
    }
    focus.status.clear();
    focus.go(Screen::Chat);
}

/// T or Enter (D-pad up on a pad) opens chat while playing.
pub fn keys(c: Res<crate::input::Controls>, mut focus: ResMut<Focus>, info: Res<WorldInfo>, settings: Res<Settings>, mut toasts: ResMut<crate::ui::Toasts>) {
    if c.chat && focus.playing() && info.in_world {
        open(&mut focus, &settings, &mut toasts);
    }
}

/// The chat screen's items: the last lines, a field to type in (with a pad,
/// the keyboard on the screen), quick lines and sharing where you are.
pub fn fill(mut focus: ResMut<Focus>, chat: Res<Chat>, net: Res<Net>, settings: Res<Settings>) {
    use crate::ui::{button, field, note, Act, Item, Kind};
    if focus.screen != Screen::Chat {
        return;
    }
    let mut v = vec![];
    if !settings.chat {
        v.push(note("A parent turned chat off."));
    } else if !net.chat_allowed() {
        v.push(note("Chat is off in this world."));
    } else {
        let lines = screen_lines(&chat, SCREEN_LINES);
        if lines.is_empty() {
            v.push(note("Nobody's said anything yet. Lines go to everyone in this world."));
        }
        for (who, color, words) in lines {
            v.push(Item { label: who, kind: Kind::Line { color, words } });
        }
        if !settings.young {
            let typed = focus.fields.get("say").cloned().unwrap_or_default();
            v.push(field("Say something", "say", typed.clone()));
            v.push(button("Send", Act::Say(typed)));
            v.push(note("Or a quick line"));
        } else {
            v.push(note("Quick lines"));
        }
        for q in QUICK {
            v.push(Item { label: q.to_string(), kind: Kind::Chip(Act::Say(q.to_string())) });
        }
        v.push(button("Share where I am", Act::SharePlace));
        if !focus.status.is_empty() {
            v.push(note(focus.status.clone()));
        }
    }
    v.push(button("Close", Act::Resume));
    focus.made = ("Chat".into(), v);
}

/// What chat's buttons do: saying something, and sharing where you are.
#[allow(clippy::too_many_arguments)]
pub fn acts(
    mut acts: ResMut<crate::ui::Acts>,
    mut focus: ResMut<Focus>,
    mut chat: ResMut<Chat>,
    me: Res<crate::player::Me>,
    settings: Res<Settings>,
    info: Res<WorldInfo>,
    mut toasts: ResMut<crate::ui::Toasts>,
) {
    use crate::ui::Act;
    let mut rest = vec![];
    for act in std::mem::take(&mut acts.0) {
        match act {
            Act::OpenChat if info.in_world => open(&mut focus, &settings, &mut toasts),
            Act::Say(text) => {
                let text = tidy(&text);
                if !text.is_empty() {
                    chat.out.push(Say { text, place: None });
                }
                focus.fields.remove("say");
                focus.go(Screen::Playing);
            }
            Act::SharePlace => {
                let at = me.pos.floor().as_ivec3();
                chat.out.push(Say { text: String::new(), place: Some(Place { name: format!("{}'s spot", settings.name), at: at.to_array() }) });
                focus.go(Screen::Playing);
            }
            other => rest.push(other),
        }
    }
    acts.0 = rest;
}

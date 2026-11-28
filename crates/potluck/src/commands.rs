//! Commands in chat, as Minecraft has them: a line that starts with a
//! slash does something instead of going to everyone. A few, kept simple:
//!
//! - `/msg NAME words` (or `/tell`, `/w`): only NAME sees them, and you.
//! - `/r words`: answers whoever last sent you a private message.
//! - `/tp X Y Z`, `/tp X Z` (on the ground there) or `/tp NAME`: go there.
//!   `~` is where you are along that way, `~5` five blocks on from it.
//! - `/gamemode survival` or `/gamemode creative`.
//! - `/help`: these.
//!
//! A command never lets a player do what the menus don't: `/tp` follows
//! the map's rules (`waypoints::may_go`: anywhere at once while building,
//! not in survival, and nowhere where the world turned teleporting off),
//! and `/gamemode` is the Survival switch in Settings. Private messages
//! follow chat's rules: the host filters them, slows floods and keeps them
//! in a server's log, they never leave the world, and younger players
//! neither send nor get them. Only the world's host may `/tp` to a younger
//! player, as only the host's map shows where everyone is.
//!
//! The player's own game runs `/help`, `/gamemode` and `/tp` to a place
//! (`local`); the host runs `/msg`, `/r` and `/tp NAME` (`host`), since
//! only it knows everyone and where they are.

use bevy::prelude::*;

use crate::chat::{tidy, Chat, Line, Say, Source, Speaker, To};
use crate::waypoints::{may_go, Goal, Travel};

/// What a command asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Help,
    /// What follows `/msg`: a name, then the words.
    Msg(String),
    /// What follows `/r`: the words.
    Reply(String),
    /// To a place: x, then y if given, then z.
    Tp(Coord, Option<Coord>, Coord),
    /// To a player, by name.
    TpTo(String),
    Mode { survival: bool },
}

/// One coordinate of `/tp`: a block, or blocks on from where you are (`~`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Coord {
    At(f32),
    On(f32),
}

impl Coord {
    fn parse(s: &str) -> Option<Coord> {
        let (on, n) = match s.strip_prefix('~') {
            Some("") => return Some(Coord::On(0.0)),
            Some(rest) => (true, rest),
            None => (false, s),
        };
        // "inf" and "nan" parse as numbers, and go nowhere
        let v = n.parse::<f32>().ok().filter(|v| v.is_finite() && v.abs() < 1e7)?;
        Some(if on { Coord::On(v) } else { Coord::At(v) })
    }

    /// Where it is, from where you are along the same way.
    pub fn of(self, here: f32) -> f32 {
        match self {
            Coord::At(v) => v,
            Coord::On(d) => here + d,
        }
    }
}

const TP: &str = "/tp X Y Z, /tp X Z or /tp NAME";
const MODES: &str = "/gamemode survival, or /gamemode creative";

/// What these words ask for, if they're a command: None when they don't
/// start with a slash, so they're for chat.
pub fn parse(text: &str) -> Option<Result<Command, String>> {
    let rest = text.trim().strip_prefix('/')?;
    let (word, args) = match rest.split_once(char::is_whitespace) {
        Some((w, a)) => (w, a.trim()),
        None => (rest, ""),
    };
    Some(match word.to_lowercase().as_str() {
        "help" | "?" => Ok(Command::Help),
        "msg" | "tell" | "w" | "whisper" if args.is_empty() => Err("/msg NAME words: only they see them.".into()),
        "msg" | "tell" | "w" | "whisper" => Ok(Command::Msg(args.into())),
        "r" | "reply" if args.is_empty() => Err("/r words: an answer to whoever last sent you a private message.".into()),
        "r" | "reply" => Ok(Command::Reply(args.into())),
        "tp" | "teleport" => {
            let parts: Vec<&str> = args.split_whitespace().collect();
            let coords: Option<Vec<Coord>> = parts.iter().map(|p| Coord::parse(p)).collect();
            match (parts.len(), coords) {
                (0, _) => Err(TP.into()),
                (2, Some(c)) => Ok(Command::Tp(c[0], None, c[1])),
                (3, Some(c)) => Ok(Command::Tp(c[0], Some(c[1]), c[2])),
                (n, Some(_)) if n > 3 => Err(TP.into()),
                // one word or more, not all numbers: a name, which may have spaces
                _ => Ok(Command::TpTo(args.into())),
            }
        }
        "gamemode" => match args.to_lowercase().as_str() {
            "survival" | "s" | "0" => Ok(Command::Mode { survival: true }),
            "creative" | "c" | "1" => Ok(Command::Mode { survival: false }),
            "" => Err(format!("{MODES}.")),
            _ => Err(format!("Potluck has survival and creative: {MODES}.")),
        },
        "" => Err("Type a command after the slash. /help lists them.".into()),
        other => Err(format!("There's no /{}. /help lists the commands.", other.chars().take(20).collect::<String>())),
    })
}

/// The commands, for `/help`: (how to type it, what it does).
pub const HELP: &[(&str, &str)] = &[
    ("/msg NAME words", "only they see them"),
    ("/r words", "answers the last private message"),
    ("/tp X Y Z  or  /tp NAME", "go there, while building"),
    ("/gamemode survival  or  creative", "switch, as in Settings"),
];

/// What the player's own game needs to run a command.
pub struct Here<'a> {
    pub chat: &'a mut Chat,
    pub settings: &'a mut crate::settings::Settings,
    pub me: &'a crate::player::Me,
    pub travel: &'a mut Travel,
    pub toasts: &'a mut crate::ui::Toasts,
    /// The world lets players teleport.
    pub teleport: bool,
    /// Seconds since the game started.
    pub now: f64,
}

/// Runs a command typed on this game: what it can do itself, and what it
/// asks the host for (through chat, as a line would go).
pub fn local(cmd: Result<Command, String>, text: &str, h: Here) {
    let cmd = match cmd {
        Ok(c) => c,
        Err(why) => return h.toasts.push(why),
    };
    match cmd {
        Command::Help => {
            for (how, what) in HELP {
                h.chat.note(how, what, h.now);
            }
        }
        Command::Mode { survival } if survival == h.settings.survival => h.toasts.push(if survival { "You're in survival already." } else { "You're in creative already." }),
        Command::Mode { survival } => {
            h.settings.survival = survival;
            h.settings.save();
            h.toasts.push(if survival { "Survival: health, and blocks that run out." } else { "Creative: build with everything." });
        }
        Command::Tp(x, y, z) => {
            let p = h.me.pos;
            let to = Vec3::new(x.of(p.x), y.map_or(p.y, |y| y.of(p.y)), z.of(p.z));
            let edge = (crate::world::BORDER - 2) as f32;
            if to.x.abs() > edge || to.z.abs() > edge {
                return h.toasts.push("That's past the edge of the world.");
            }
            let name = match y {
                Some(_) => format!("{}, {}, {}", to.x.floor(), to.y.floor(), to.z.floor()),
                None => format!("{}, {}", to.x.floor(), to.z.floor()),
            };
            match may_go(&Goal::Spot, h.settings.survival, h.teleport, h.me.riding.is_some(), h.travel.rest) {
                // with a height, there if it's clear; without, on the ground
                Ok(wait) => h.travel.start(h.me, to, y.is_some(), &name, wait),
                Err(why) => h.toasts.push(why),
            }
        }
        Command::TpTo(_) => match may_go(&Goal::Player, h.settings.survival, h.teleport, h.me.riding.is_some(), h.travel.rest) {
            // only the host knows where everyone is: ask it
            Ok(_) => {
                h.chat.asked = true;
                h.chat.out.push(Say { text: text.into(), place: None });
            }
            Err(why) => h.toasts.push(why),
        },
        Command::Msg(_) | Command::Reply(_) => h.chat.out.push(Say { text: text.into(), place: None }),
    }
}

/// Goes to a player the host found for `/tp NAME`, if this game asked.
pub fn found(chat: &mut Chat, settings: &crate::settings::Settings, me: &crate::player::Me, travel: &mut Travel, teleport: bool, toasts: &mut crate::ui::Toasts) {
    for (name, at) in std::mem::take(&mut chat.found) {
        if !std::mem::take(&mut chat.asked) || !at.is_finite() {
            continue;
        }
        match may_go(&Goal::Player, settings.survival, teleport, me.riding.is_some(), travel.rest) {
            Ok(wait) => travel.start(me, at, true, &name, wait),
            Err(why) => toasts.push(why),
        }
    }
}

/// Someone in the world, as the host knows them.
#[derive(Clone, Debug)]
pub struct Player {
    pub id: u32,
    pub name: String,
    pub young: bool,
    pub pos: Vec3,
}

/// What the host does with a command a player sent.
#[derive(Debug)]
pub enum Answer {
    /// A private line, for the one it's to and the one who sent it.
    Private(Line),
    /// Where the player someone asked for is, for them to go (`/tp NAME`).
    Found { name: String, at: Vec3 },
}

/// Who a command names at the start of `text`, among those here: the
/// longest name the text starts with, as whole words and in any case, so
/// names with spaces work. Returns them and the words after.
pub fn named<'a>(here: &'a [Player], text: &'a str) -> Result<(&'a Player, &'a str), String> {
    let text = text.trim();
    let mut cuts: Vec<usize> = text.char_indices().filter(|(_, c)| c.is_whitespace()).map(|(i, _)| i).collect();
    cuts.push(text.len());
    for &cut in cuts.iter().rev() {
        let name = text[..cut].trim().to_lowercase();
        let found: Vec<&Player> = here.iter().filter(|p| !p.name.trim().is_empty() && p.name.trim().to_lowercase() == name).collect();
        match found.len() {
            0 => continue,
            1 => return Ok((found[0], text[cut..].trim())),
            n => return Err(format!("{n} players here are called {}.", found[0].name)),
        }
    }
    let first: String = text.split_whitespace().next().unwrap_or("").chars().take(20).collect();
    Err(format!("Nobody called {first} is here."))
}

/// The host's word on a command a player sent: what to do, or why not
/// (empty when there's nothing to say). `owner` is the host's own player,
/// if it has one (a server hasn't).
pub fn host(chat: &mut Chat, who: Speaker, text: &str, here: &[Player], owner: Option<u32>, allowed: bool, now: f64) -> Result<Answer, String> {
    let cmd = match parse(text) {
        Some(Ok(c)) => c,
        Some(Err(why)) => return Err(why),
        None => return Err(String::new()),
    };
    match cmd {
        Command::Msg(rest) => {
            let (to, words) = named(here, &rest)?;
            chat.private(who, to, words, allowed, now).map(Answer::Private)
        }
        Command::Reply(words) => {
            let Some(last) = chat.replies.get(&who.id).copied() else { return Err("Nobody's sent you a private message yet.".into()) };
            let Some(to) = here.iter().find(|p| p.id == last) else { return Err("Whoever sent you that has left.".into()) };
            chat.private(who, to, &words, allowed, now).map(Answer::Private)
        }
        Command::TpTo(name) => {
            chat.slow(who.id, now)?;
            let (them, _) = named(here, &name)?;
            if them.id == who.id {
                return Err("That's you.".into());
            }
            if them.young && owner != Some(who.id) {
                return Err(format!("You can't teleport to {}.", them.name));
            }
            Ok(Answer::Found { name: them.name.clone(), at: them.pos })
        }
        // the player's own game runs the rest
        _ => Err(String::new()),
    }
}

impl Chat {
    /// The host's word on a private line from one player to another.
    pub fn private(&mut self, who: Speaker, to: &Player, text: &str, allowed: bool, now: f64) -> Result<Line, String> {
        if !allowed {
            return Err("Chat is off in this world.".into());
        }
        if who.young {
            return Err("Younger players chat with quick lines, for everyone to see.".into());
        }
        if to.id == who.id {
            return Err("That's you.".into());
        }
        if to.young {
            return Err(format!("{} can't get private messages.", to.name));
        }
        let text = tidy(text);
        if text.is_empty() {
            return Err(format!("Write something for {} after their name.", to.name));
        }
        self.slow(who.id, now)?;
        let line = Line {
            id: self.take_id(),
            name: who.name.chars().take(20).collect(),
            from: Source::Player(who.id),
            text: self.mask(&text),
            place: None,
            young: false,
            to: Some(To { id: to.id, name: to.name.chars().take(20).collect() }),
            at: crate::hosting::now(),
        };
        self.replies.insert(to.id, who.id);
        Ok(line)
    }
}

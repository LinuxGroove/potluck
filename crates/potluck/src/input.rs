//! The player's controls as actions, from a pad or a keyboard and mouse,
//! read once a frame so the rest of the game never asks which device.

use bevy::input::gamepad::Gamepad;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::input::ButtonState;
use bevy::prelude::*;

use crate::creations::{ActionDef, Library};
use crate::creatures::Herd;
use crate::player::Me;
use crate::settings::Settings;

/// Pad buttons a creation's action can ask for. B, Menu, View and the
/// sticks stay the game's, so B always gets you off a ride.
pub const PAD_BUTTONS: &[(&str, GamepadButton)] = &[
    ("A", GamepadButton::South),
    ("X", GamepadButton::West),
    ("Y", GamepadButton::North),
    ("LB", GamepadButton::LeftTrigger),
    ("RB", GamepadButton::RightTrigger),
    ("LT", GamepadButton::LeftTrigger2),
    ("RT", GamepadButton::RightTrigger2),
    ("Up", GamepadButton::DPadUp),
    ("Down", GamepadButton::DPadDown),
    ("Left", GamepadButton::DPadLeft),
    ("Right", GamepadButton::DPadRight),
];

/// Keys a creation's action can ask for: Space and the letters but W, A, S
/// and D. Shift, Esc, Tab and the number row stay the game's.
const KEYS: &[(&str, KeyCode)] = &[
    ("Space", KeyCode::Space),
    ("B", KeyCode::KeyB), ("C", KeyCode::KeyC), ("E", KeyCode::KeyE), ("F", KeyCode::KeyF), ("G", KeyCode::KeyG),
    ("H", KeyCode::KeyH), ("I", KeyCode::KeyI), ("J", KeyCode::KeyJ), ("K", KeyCode::KeyK), ("L", KeyCode::KeyL),
    ("M", KeyCode::KeyM), ("N", KeyCode::KeyN), ("O", KeyCode::KeyO), ("P", KeyCode::KeyP), ("Q", KeyCode::KeyQ),
    ("R", KeyCode::KeyR), ("T", KeyCode::KeyT), ("U", KeyCode::KeyU), ("V", KeyCode::KeyV), ("X", KeyCode::KeyX),
    ("Y", KeyCode::KeyY), ("Z", KeyCode::KeyZ),
];

/// Actions a creation can have.
pub const MAX_ACTIONS: usize = 8;

pub fn pad_ok(b: &str) -> bool {
    PAD_BUTTONS.iter().any(|(n, _)| *n == b)
}

pub fn key_ok(k: &str) -> bool {
    KEYS.iter().any(|(n, _)| *n == k)
}

/// What's wrong with one of a creation's actions, if anything.
pub fn action_problem(id: &str, a: &ActionDef) -> Option<String> {
    if id.is_empty() || id.len() > 24 || !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
        return Some(format!("an action's id is 1 to 24 small letters, numbers or _ (not \"{id}\")"));
    }
    if a.name.trim().is_empty() || a.name.chars().count() > 24 {
        return Some(format!("action {id} needs a name of 1 to 24 letters"));
    }
    if a.pad.is_empty() && a.keys.is_empty() {
        return Some(format!("action {id} needs a pad button or a key"));
    }
    if !a.pad.is_empty() && !pad_ok(&a.pad) {
        let all: Vec<&str> = PAD_BUTTONS.iter().map(|b| b.0).collect();
        return Some(format!("action {id} can't have the pad's {} (it can have: {})", a.pad, all.join(", ")));
    }
    if !a.keys.is_empty() && !key_ok(&a.keys) {
        return Some(format!("action {id} can't have the {} key (it can have Space or a letter but W, A, S and D)", a.keys));
    }
    None
}

/// How a button shows in prompts.
pub fn button_name(b: &str) -> String {
    match b {
        "Up" | "Down" | "Left" | "Right" => format!("D-pad {}", b.to_lowercase()),
        _ => b.to_string(),
    }
}

/// The creation whose actions are live for this player: the one whose
/// creature they ride, or the one that has their controls.
pub fn live_creation(me: &Me, herd: &Herd) -> Option<String> {
    me.riding.and_then(|id| herd.bodies.get(&id)).map(|b| b.creation.clone()).or_else(|| me.focus.clone())
}

#[derive(Resource, Default)]
pub struct Controls {
    /// x is right, y is forward, each -1..1.
    pub mv: Vec2,
    /// How far to turn this frame, in radians (x yaw, y pitch).
    pub look: Vec2,
    pub jump: bool,
    pub jump_pressed: bool,
    pub crouch: bool,
    pub sprint_pressed: bool,
    pub break_held: bool,
    pub break_pressed: bool,
    pub place_pressed: bool,
    pub hotbar_step: i32,
    pub hotbar_slot: Option<usize>,
    pub interact: bool,
    pub look_inside: bool,
    pub pause: bool,
    pub share: bool,
    pub camera: bool,
    // menus
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub accept: bool,
    pub back: bool,
    /// Typed this frame (for code and address fields).
    pub typed: String,
    pub backspace: bool,
    /// The last thing pressed was on a pad (for prompts).
    pub pad: bool,
    /// Right stick held left or right, for sliders.
    pub slide: f32,
    stick_latch: bool,
    y_held: f32,
    /// Buttons a creation could ask for, held now and pressed this frame:
    /// `pad:X`, `key:F`.
    pub held: Vec<String>,
    pub just: Vec<String>,
    /// The creation whose actions are live, and its actions held now.
    pub live: Option<String>,
    pub actions: Vec<String>,
}

pub fn read(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    pads: Query<&Gamepad>,
    mut typed: MessageReader<KeyboardInput>,
    settings: Res<Settings>,
    time: Res<Time>,
    mut c: ResMut<Controls>,
    me: Res<Me>,
    herd: Res<Herd>,
    lib: Res<Library>,
    focus: Res<crate::ui::Focus>,
) {
    let dt = time.delta_secs();
    let y_held = c.y_held;
    let latch = c.stick_latch;
    let pad_before = c.pad;
    *c = Controls::default();
    c.pad = pad_before;
    c.y_held = y_held;
    c.stick_latch = latch;

    // keyboard and mouse
    let k = |code| keys.pressed(code);
    let kp = |code| keys.just_pressed(code);
    let mut mv = Vec2::ZERO;
    if k(KeyCode::KeyW) { mv.y += 1.0 }
    if k(KeyCode::KeyS) { mv.y -= 1.0 }
    if k(KeyCode::KeyD) { mv.x += 1.0 }
    if k(KeyCode::KeyA) { mv.x -= 1.0 }
    let sens = 0.0022 * settings.mouse_sensitivity;
    let mut look = motion.delta * sens;
    if settings.invert_y {
        look.y = -look.y;
    }
    c.jump = k(KeyCode::Space);
    c.jump_pressed = kp(KeyCode::Space);
    c.crouch = k(KeyCode::ShiftLeft) || k(KeyCode::ShiftRight);
    c.sprint_pressed = kp(KeyCode::ControlLeft);
    c.break_held = mouse.pressed(MouseButton::Left);
    c.break_pressed = mouse.just_pressed(MouseButton::Left);
    c.place_pressed = mouse.just_pressed(MouseButton::Right);
    if scroll.delta.y > 0.0 { c.hotbar_step -= 1 }
    if scroll.delta.y < 0.0 { c.hotbar_step += 1 }
    let digits = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9];
    for (i, d) in digits.iter().enumerate() {
        if kp(*d) {
            c.hotbar_slot = Some(i);
        }
    }
    c.interact = kp(KeyCode::KeyF) || kp(KeyCode::KeyE);
    c.look_inside = kp(KeyCode::KeyL) || kp(KeyCode::Tab);
    c.pause = kp(KeyCode::Escape);
    c.share = kp(KeyCode::KeyP);
    c.camera = kp(KeyCode::F5);
    c.up = kp(KeyCode::ArrowUp);
    c.down = kp(KeyCode::ArrowDown);
    c.left = kp(KeyCode::ArrowLeft);
    c.right = kp(KeyCode::ArrowRight);
    c.accept = kp(KeyCode::Enter) || kp(KeyCode::NumpadEnter);
    c.back = kp(KeyCode::Escape);
    c.backspace = kp(KeyCode::Backspace);
    if keys.get_just_pressed().next().is_some() || mouse.get_just_pressed().next().is_some() || motion.delta.length() > 2.0 {
        c.pad = false;
    }
    for (name, code) in KEYS {
        if keys.pressed(*code) {
            c.held.push(format!("key:{name}"));
        }
        if keys.just_pressed(*code) {
            c.just.push(format!("key:{name}"));
        }
    }
    for ev in typed.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        if let Key::Character(s) = &ev.logical_key {
            c.typed.push_str(s);
        } else if let Key::Space = &ev.logical_key {
            c.typed.push(' ');
        }
    }

    // pads
    for pad in &pads {
        let dead = |v: Vec2| if v.length() < 0.18 { Vec2::ZERO } else { v * ((v.length() - 0.18) / 0.82 / v.length()) };
        let ls = dead(pad.left_stick());
        let rs = dead(pad.right_stick());
        if ls != Vec2::ZERO {
            mv = ls;
        }
        if rs != Vec2::ZERO {
            // a curve so small moves aim finely and full tilt turns fast
            let speed = 3.2 * settings.stick_sensitivity;
            let mut d = Vec2::new(rs.x, -rs.y) * rs.length() * speed * dt;
            if settings.invert_y {
                d.y = -d.y;
            }
            look += d;
            c.slide = rs.x;
        }
        let p = |b| pad.pressed(b);
        let jp = |b| pad.just_pressed(b);
        c.jump |= p(GamepadButton::South);
        c.jump_pressed |= jp(GamepadButton::South);
        c.crouch |= p(GamepadButton::East);
        c.sprint_pressed |= jp(GamepadButton::LeftThumb);
        c.break_held |= p(GamepadButton::RightTrigger2);
        c.break_pressed |= jp(GamepadButton::RightTrigger2);
        c.place_pressed |= jp(GamepadButton::LeftTrigger2);
        if jp(GamepadButton::LeftTrigger) { c.hotbar_step -= 1 }
        if jp(GamepadButton::RightTrigger) { c.hotbar_step += 1 }
        c.interact |= jp(GamepadButton::West);
        c.pause |= jp(GamepadButton::Start);
        c.camera |= jp(GamepadButton::RightThumb);
        c.share |= jp(GamepadButton::DPadLeft);
        // Y: hold to look inside what you aim at
        if p(GamepadButton::North) {
            c.y_held += dt;
            if c.y_held >= 0.35 && c.y_held - dt < 0.35 {
                c.look_inside = true;
            }
        } else {
            if c.y_held > 0.0 && c.y_held < 0.35 {
                // a tap opens it too, until there's an inventory
                c.look_inside = true;
            }
            c.y_held = 0.0;
        }
        // menus: the d-pad, or the left stick flicked
        c.up |= jp(GamepadButton::DPadUp);
        c.down |= jp(GamepadButton::DPadDown);
        c.left |= jp(GamepadButton::DPadLeft);
        c.right |= jp(GamepadButton::DPadRight);
        let raw = pad.left_stick();
        if raw.length() > 0.6 {
            if !c.stick_latch {
                c.stick_latch = true;
                if raw.y.abs() > raw.x.abs() {
                    if raw.y > 0.0 { c.up = true } else { c.down = true }
                } else if raw.x > 0.0 {
                    c.right = true
                } else {
                    c.left = true
                }
            }
        } else if raw.length() < 0.3 {
            c.stick_latch = false;
        }
        for (name, b) in PAD_BUTTONS {
            if p(*b) {
                c.held.push(format!("pad:{name}"));
            }
            if jp(*b) {
                c.just.push(format!("pad:{name}"));
            }
        }
        c.accept |= jp(GamepadButton::South);
        c.back |= jp(GamepadButton::East);
        if pad.get_just_pressed().next().is_some() || ls != Vec2::ZERO || rs != Vec2::ZERO {
            c.pad = true;
        }
    }
    c.mv = if mv.length() > 1.0 { mv.normalize() } else { mv };
    c.look = look;
    if focus.playing() {
        claim(&mut c, live_creation(&me, &herd), &lib, &settings);
    }
}

/// A creation's actions while they're live: which are held, and the game's
/// own use of their buttons set aside (so X bucks rather than gets you off).
pub fn claim(c: &mut Controls, live: Option<String>, lib: &Library, settings: &Settings) {
    let Some(creation) = live.and_then(|id| lib.list.get(&id)) else { return };
    let mut claimed: Vec<String> = vec![];
    for (id, a) in &creation.manifest.actions {
        let (pad, key) = settings.binding(&creation.id, id, a);
        let mine = [(!pad.is_empty()).then(|| format!("pad:{pad}")), (!key.is_empty()).then(|| format!("key:{key}"))];
        if mine.iter().flatten().any(|b| c.held.contains(b)) {
            c.actions.push(id.clone());
        }
        claimed.extend(mine.into_iter().flatten());
    }
    for b in &claimed {
        match b.as_str() {
            "pad:A" | "key:Space" => (c.jump, c.jump_pressed) = (false, false),
            "pad:X" | "key:F" | "key:E" => c.interact = false,
            "pad:Y" | "key:L" => c.look_inside = false,
            "pad:LB" | "pad:RB" => c.hotbar_step = 0,
            "pad:LT" => c.place_pressed = false,
            "pad:RT" => (c.break_held, c.break_pressed) = (false, false),
            "pad:Left" | "key:P" => c.share = false,
            _ => {}
        }
    }
    c.live = Some(creation.id.clone());
}

/// The name of a button for prompts, on the device last used.
pub fn glyph(pad: bool, action: &str) -> &'static str {
    match (pad, action) {
        (true, "break") => "RT",
        (true, "place") => "LT",
        (true, "interact") => "X",
        (true, "inside") => "Y",
        (true, "jump") => "A",
        (true, "back") => "B",
        (true, "accept") => "A",
        (true, "share") => "D-pad left",
        (true, "pause") => "Menu",
        (false, "break") => "Left click",
        (false, "place") => "Right click",
        (false, "interact") => "F",
        (false, "inside") => "L",
        (false, "jump") => "Space",
        (false, "back") => "Esc",
        (false, "accept") => "Enter",
        (false, "share") => "P",
        (false, "pause") => "Esc",
        _ => "?",
    }
}

//! The player's controls as actions, from a pad or a keyboard and mouse,
//! read once a frame so the rest of the game never asks which device.

use bevy::input::gamepad::Gamepad;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::input::ButtonState;
use bevy::prelude::*;

use crate::settings::Settings;

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
        c.accept |= jp(GamepadButton::South);
        c.back |= jp(GamepadButton::East);
        if pad.get_just_pressed().next().is_some() || ls != Vec2::ZERO || rs != Vec2::ZERO {
            c.pad = true;
        }
    }
    c.mv = if mv.length() > 1.0 { mv.normalize() } else { mv };
    c.look = look;
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

//! A keyboard on the screen, for typing with a pad: in chat, and in any
//! text field (codes, addresses, names, waypoints). A on a field opens it.
//! The d-pad or left stick moves, A types the key, X deletes, Y is a space,
//! LT switches capitals, Menu is done and B leaves the field as it was. A
//! real keyboard and the mouse work on it too.

use bevy::prelude::*;

use crate::input::Controls;
use crate::settings::Settings;
use crate::ui::{Act, Acts, Fonts, Focus, Item, Kind, Screen, DIM, INK, MINT};

const ROWS: [&str; 4] = ["1234567890", "qwertyuiop", "asdfghjkl'", "zxcvbnm,.?"];
/// The last row's wide keys.
const WIDE: [&str; 4] = ["Capitals", "Space", "Delete", "Done"];

/// A field being typed into with the keyboard on the screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Keys {
    /// The field's key in `Focus::fields`.
    pub field: String,
    /// What it says so far.
    pub text: String,
    /// What the field is for, shown above the keys.
    pub label: String,
    pub row: usize,
    pub col: usize,
    pub capitals: bool,
    pub max: usize,
}

impl Keys {
    pub fn new(field: &str, label: &str, text: &str, max: usize) -> Keys {
        Keys { field: field.into(), text: text.into(), label: label.into(), row: 1, col: 0, capitals: false, max }
    }

    fn width(row: usize) -> usize {
        if row < ROWS.len() { ROWS[row].chars().count() } else { WIDE.len() }
    }

    /// The key at a row and column, as it types.
    fn key(&self, row: usize, col: usize) -> String {
        match ROWS.get(row) {
            Some(r) => {
                let c = r.chars().nth(col).unwrap_or(' ');
                if self.capitals { c.to_uppercase().collect() } else { c.to_string() }
            }
            None => WIDE[col.min(WIDE.len() - 1)].to_string(),
        }
    }

    /// Moves the highlight, wrapping round the edges.
    pub fn step(&mut self, dx: i32, dy: i32) {
        if dy != 0 {
            let rows = ROWS.len() as i32 + 1;
            let from = Keys::width(self.row) as f32;
            self.row = (self.row as i32 + dy).rem_euclid(rows) as usize;
            // keep about the same place across rows of different widths
            let to = Keys::width(self.row) as f32;
            self.col = (((self.col as f32 + 0.5) / from * to) as usize).min(Keys::width(self.row) - 1);
        }
        if dx != 0 {
            self.col = (self.col as i32 + dx).rem_euclid(Keys::width(self.row) as i32) as usize;
        }
    }

    fn put(&mut self, s: &str) {
        if self.text.chars().count() + s.chars().count() <= self.max {
            self.text.push_str(s);
        }
    }

    /// Presses the highlighted key. True when it's Done.
    pub fn press(&mut self) -> bool {
        if self.row < ROWS.len() {
            let k = self.key(self.row, self.col);
            self.put(&k);
            // capitals for one letter, as phones do
            self.capitals = false;
            return false;
        }
        match self.col {
            0 => self.capitals = !self.capitals,
            1 => self.put(" "),
            2 => {
                self.text.pop();
            }
            _ => return true,
        }
        false
    }
}

/// How long a field may be.
pub fn max_len(field: &str) -> usize {
    match field {
        "say" => crate::chat::MAX_LEN,
        "name" => 20,
        "waypoint" => 32,
        _ => 64,
    }
}

/// Opens the keyboard for a field.
pub fn open(focus: &mut Focus, field: &str, label: &str) {
    let text = focus.fields.get(field).cloned().unwrap_or_default();
    focus.keyboard = Some(Keys::new(field, label, &text, max_len(field)));
}

/// A key on the screen, for the mouse: (row, column).
#[derive(Component)]
pub struct KeyButton(usize, usize);
#[derive(Component)]
pub struct KeyboardRoot;

/// The keyboard's controls, taken before the menus see them.
#[allow(clippy::too_many_arguments)]
pub fn input(
    mut c: ResMut<Controls>,
    mut focus: ResMut<Focus>,
    mut settings: ResMut<Settings>,
    mut acts: ResMut<Acts>,
    pads: Query<&bevy::input::gamepad::Gamepad>,
    buttons: Query<(&Interaction, &KeyButton), Changed<Interaction>>,
    mut heard: ResMut<crate::sound::Heard>,
) {
    let Some(mut k) = focus.keyboard.clone() else { return };
    let jp = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    let (mut done, mut cancel) = (false, false);
    for (i, b) in &buttons {
        if *i == Interaction::Pressed {
            (k.row, k.col) = (b.0, b.1);
            done |= k.press();
            heard.click();
        }
    }
    let dx = c.right as i32 - c.left as i32;
    let dy = c.down as i32 - c.up as i32;
    k.step(dx, dy);
    if c.accept && c.pad {
        done |= k.press();
        heard.click();
    }
    if jp(GamepadButton::West) {
        k.text.pop();
    }
    if jp(GamepadButton::North) {
        k.put(" ");
    }
    if jp(GamepadButton::LeftTrigger2) {
        k.capitals = !k.capitals;
    }
    // a real keyboard types too
    if !c.typed.is_empty() {
        let typed = c.typed.clone();
        k.put(&typed);
    }
    if c.backspace {
        k.text.pop();
    }
    done |= c.pause && c.pad || (c.accept && !c.pad);
    cancel |= c.back && !c.backspace;
    // nothing else sees these this frame
    c.accept = false;
    c.back = false;
    c.pause = false;
    (c.up, c.down, c.left, c.right) = (false, false, false, false);
    c.typed.clear();
    c.backspace = false;
    if cancel {
        focus.keyboard = None;
        return;
    }
    if done {
        let text = k.text.trim().to_string();
        focus.keyboard = None;
        crate::ui::set_field(&mut focus, &mut settings, &k.field, &text);
        // as Enter in the field: the button after it
        if let Some(act) = after(&focus.items, &k.field, &text) {
            acts.0.push(act);
        }
        return;
    }
    focus.keyboard = Some(k);
}

/// The button after a field, with what the field says in it.
pub fn after(items: &[Item], field: &str, text: &str) -> Option<Act> {
    let i = items.iter().position(|i| matches!(&i.kind, Kind::Field { key, .. } if key == field))?;
    match &items.get(i + 1)?.kind {
        Kind::Button(a) => Some(crate::ui::with_value(a.clone(), text)),
        _ => None,
    }
}

/// Draws the keyboard when it's open, again whenever it changes.
pub fn draw(mut commands: Commands, focus: Res<Focus>, fonts: Res<Fonts>, roots: Query<Entity, With<KeyboardRoot>>, mut was: Local<Option<Keys>>) {
    if *was == focus.keyboard {
        return;
    }
    *was = focus.keyboard.clone();
    for e in &roots {
        commands.entity(e).despawn();
    }
    let Some(k) = &focus.keyboard else { return };
    let ui = fonts.ui.clone();
    let font = |size: f32| TextFont { font: ui.clone().into(), font_size: FontSize::Px(size), ..default() };
    commands
        .spawn((
            KeyboardRoot,
            // beside the chat screen when it's open, so the lines stay in view
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::FlexEnd,
                padding: UiRect { bottom: px(40), left: px(if focus.screen == Screen::Chat { crate::chat::WIDTH + 30.0 } else { 0.0 }), ..default() },
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.3)),
            GlobalZIndex(20),
        ))
        .with_children(|root| {
            root.spawn((
                Node { flex_direction: FlexDirection::Column, padding: UiRect::all(px(18)), row_gap: px(8), border_radius: BorderRadius::all(px(18)), ..default() },
                BackgroundColor(Color::srgba(0.07, 0.09, 0.12, 0.94)),
            ))
            .with_children(|p| {
                p.spawn((Text::new(k.label.clone()), font(15.0), TextColor(DIM)));
                p.spawn((
                    Text::new(format!("{}|", k.text)),
                    font(22.0),
                    TextColor(INK),
                    Node { padding: UiRect::axes(px(12), px(8)), border_radius: BorderRadius::all(px(10)), min_width: px(560), ..default() },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.06)),
                ));
                for row in 0..=ROWS.len() {
                    p.spawn(Node { column_gap: px(6), justify_content: JustifyContent::Center, ..default() }).with_children(|r| {
                        for col in 0..Keys::width(row) {
                            let picked = (row, col) == (k.row, k.col);
                            let label = k.key(row, col);
                            let wide = row == ROWS.len();
                            let on = wide && col == 0 && k.capitals;
                            r.spawn((
                                Interaction::default(),
                                KeyButton(row, col),
                                Node {
                                    width: px(if wide { 128.0 } else { 50.0 }),
                                    height: px(46),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    border: UiRect::all(px(2)),
                                    border_radius: BorderRadius::all(px(10)),
                                    ..default()
                                },
                                BorderColor::all(if picked { MINT } else { Color::NONE }),
                                BackgroundColor(if picked { Color::srgba(0.37, 0.88, 0.78, 0.25) } else if on { Color::srgba(0.37, 0.88, 0.78, 0.12) } else { Color::srgba(1.0, 1.0, 1.0, 0.07) }),
                            ))
                            .with_child((Text::new(label), font(if wide { 16.0 } else { 20.0 }), TextColor(INK)));
                        }
                    });
                }
                p.spawn((Text::new("A types · X deletes · Y space · LT capitals · Menu done · B leaves it"), font(12.0), TextColor(DIM)));
            });
        });
}

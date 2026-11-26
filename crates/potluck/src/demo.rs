//! A scripted play-through for testing and for showing the game: it plays
//! with the same controls a player has, at a fixed 30 frames a second
//! however slowly the machine draws, and records every frame to a video
//! (through ffmpeg) with stills along the way.
//!
//!     potluck --demo=DIR [--size=1280x720]
//!
//! DIR gets `potluck-test-run.mp4` and numbered PNG stills.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};

use crate::creations::Library;
use crate::creatures::Herd;
use crate::input::Controls;
use crate::player::Me;
use crate::render::Clock;
use crate::script::Scripts;
use crate::settings::Settings;
use crate::ui::{self, Acts, Focus, Kind, Screen, WorldInfo};
use crate::world::{self, Voxels};

pub const FPS: f64 = 30.0;

/// What the steps share: the creature being ridden, the remix's id.
#[derive(Default)]
pub struct Mem {
    pub mount: Option<u64>,
    pub remix: Option<String>,
    pub stuck: f32,
    pub placed: u32,
    pub from: f32,
}

pub struct Ctx<'a> {
    pub c: &'a mut Controls,
    pub me: &'a mut Me,
    pub focus: &'a mut Focus,
    pub acts: &'a mut Acts,
    pub clock: &'a mut Clock,
    pub herd: &'a Herd,
    pub lib: &'a Library,
    pub scripts: &'a Scripts,
    pub settings: &'a Settings,
    pub info: &'a WorldInfo,
    pub voxels: &'a Voxels,
    pub mem: &'a mut Mem,
    pub dt: f32,
    /// Seconds and frames since the step began.
    pub t: f32,
    pub frame: u32,
    pub still: Option<String>,
}

type Run = Box<dyn FnMut(&mut Ctx) -> bool + Send + Sync>;

pub struct Step {
    caption: &'static str,
    secs: f32,
    run: Run,
}

fn step(caption: &'static str, secs: f32, run: impl FnMut(&mut Ctx) -> bool + Send + Sync + 'static) -> Step {
    Step { caption, secs, run: Box::new(run) }
}

struct Recorder {
    ffmpeg: Option<Child>,
    size: UVec2,
    out: PathBuf,
    /// Frames that came back early, until the ones before them arrive.
    waiting: BTreeMap<u32, Vec<u8>>,
    next: u32,
    failed: bool,
}

impl Recorder {
    fn frame(&mut self, n: u32, img: &Image, still: Option<PathBuf>) {
        let Ok(dynamic) = img.clone().try_into_dynamic() else {
            self.failed = true;
            return;
        };
        let rgba = dynamic.to_rgba8();
        if let Some(p) = still {
            let _ = rgba.save(&p);
        }
        if rgba.width() != self.size.x || rgba.height() != self.size.y {
            return;
        }
        self.waiting.insert(n, rgba.into_raw());
        if self.ffmpeg.is_none() && !self.failed {
            let video = self.out.join("potluck-test-run.mp4");
            let child = Command::new("ffmpeg")
                .args(["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgba", "-s"])
                .arg(format!("{}x{}", self.size.x, self.size.y))
                .args(["-r", "30", "-i", "-", "-c:v", "libx264", "-preset", "medium", "-crf", "20", "-pix_fmt", "yuv420p", "-movflags", "+faststart"])
                .arg(&video)
                .stdin(Stdio::piped())
                .spawn();
            match child {
                Ok(c) => self.ffmpeg = Some(c),
                Err(e) => {
                    error!("Couldn't start ffmpeg: {e}");
                    self.failed = true;
                }
            }
        }
        while let Some(bytes) = self.waiting.remove(&self.next) {
            if let Some(stdin) = self.ffmpeg.as_mut().and_then(|c| c.stdin.as_mut()) {
                if stdin.write_all(&bytes).is_err() {
                    self.failed = true;
                }
            }
            self.next += 1;
        }
    }

    fn finish(&mut self) {
        if let Some(mut c) = self.ffmpeg.take() {
            drop(c.stdin.take());
            let _ = c.wait();
        }
    }
}

#[derive(Resource)]
pub struct Demo {
    steps: Vec<Step>,
    at: usize,
    since: f32,
    frames: u32,
    /// Frames asked for and frames written.
    shot: u32,
    rec: Arc<Mutex<Recorder>>,
    dir: PathBuf,
    stills: u32,
    started: bool,
    ended: f32,
    wall: std::time::Instant,
}

#[derive(Component)]
pub struct Caption;

pub fn add(app: &mut App, dir: PathBuf, size: UVec2) {
    let _ = std::fs::create_dir_all(&dir);
    let rec = Recorder { ffmpeg: None, size, out: dir.clone(), waiting: BTreeMap::new(), next: 0, failed: false };
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / FPS)))
        .insert_resource(Demo { steps: script(), at: 0, since: 0.0, frames: 0, shot: 0, rec: Arc::new(Mutex::new(rec)), dir, stills: 0, started: false, ended: 0.0, wall: std::time::Instant::now() })
        .add_systems(PostStartup, spawn_caption)
        .add_systems(Update, drive.after(crate::input::read).before(ui::menu_input));
}

fn spawn_caption(mut commands: Commands, fonts: Res<ui::Fonts>) {
    commands
        .spawn((Node { position_type: PositionType::Absolute, top: px(18), width: percent(100), justify_content: JustifyContent::Center, ..default() }, GlobalZIndex(50)))
        .with_children(|p| {
            p.spawn((
                Caption,
                Text::new(""),
                TextFont { font: fonts.title.clone().into(), font_size: FontSize::Px(30.0), ..default() },
                TextColor(Color::srgb(1.0, 0.95, 0.85)),
                Node { padding: UiRect::axes(px(22), px(8)), border_radius: BorderRadius::all(px(14)), ..default() },
                BackgroundColor(Color::srgba(0.07, 0.08, 0.1, 0.72)),
                Visibility::Hidden,
            ));
        });
}

#[allow(clippy::too_many_arguments)]
fn drive(
    mut commands: Commands,
    mut demo: ResMut<Demo>,
    time: Res<Time>,
    mut c: ResMut<Controls>,
    mut me: ResMut<Me>,
    mut focus: ResMut<Focus>,
    mut acts: ResMut<Acts>,
    mut clock: ResMut<Clock>,
    world: (Res<Herd>, Res<Library>, Res<Scripts>, Res<Settings>, Res<WorldInfo>, Res<Voxels>),
    mut caption: Query<(&mut Text, &mut Visibility), With<Caption>>,
    mut mem: Local<Mem>,
    mut exit: MessageWriter<AppExit>,
) {
    let (herd, lib, scripts, settings, info, voxels) = world;
    let dt = time.delta_secs();
    // wait for the world to be drawn before the first frame
    if !demo.started {
        if time.elapsed_secs() > 2.0 && voxels.dirty.is_empty() {
            demo.started = true;
            demo.wall = std::time::Instant::now();
            info!("Demo: the world is ready; recording");
        }
        return;
    }
    if demo.at >= demo.steps.len() {
        // let the last frames come back, then stop
        demo.ended += dt;
        let written = demo.rec.lock().unwrap().next;
        if written >= demo.shot || demo.ended > 20.0 {
            demo.rec.lock().unwrap().finish();
            info!("Demo: {} frames written to {}", written, demo.dir.display());
            exit.write(AppExit::Success);
        }
        return;
    }
    let at = demo.at;
    let since = demo.since;
    let frame = demo.frames;
    let mut ctx = Ctx {
        c: &mut c,
        me: &mut me,
        focus: &mut focus,
        acts: &mut acts,
        clock: &mut clock,
        herd: &herd,
        lib: &lib,
        scripts: &scripts,
        settings: &settings,
        info: &info,
        voxels: &voxels,
        mem: &mut mem,
        dt,
        t: since,
        frame,
        still: None,
    };
    let s = &mut demo.steps[at];
    let done = (s.run)(&mut ctx) || since >= s.secs;
    let cap = s.caption;
    let still = ctx.still.take();
    if let Ok((mut text, mut vis)) = caption.single_mut() {
        if text.0 != cap {
            text.0 = cap.to_string();
        }
        *vis = if cap.is_empty() { Visibility::Hidden } else { Visibility::Inherited };
    }
    // this frame goes in the video
    let n = demo.shot;
    demo.shot += 1;
    let still_path = still.map(|name| {
        demo.stills += 1;
        demo.dir.join(format!("{:02}-{name}.png", demo.stills))
    });
    let rec = demo.rec.clone();
    commands.spawn(Screenshot::primary_window()).observe(move |shot: On<ScreenshotCaptured>| {
        rec.lock().unwrap().frame(n, &shot.image, still_path.clone());
    });
    if n % 60 == 0 {
        let secs = demo.wall.elapsed().as_secs_f32();
        info!("Demo: frame {n}, step {} ({cap}), {:.2} s a frame", at + 1, if n > 0 { secs / n as f32 } else { 0.0 });
    }
    if done {
        demo.at += 1;
        demo.since = 0.0;
        demo.frames = 0;
    } else {
        demo.since += dt;
        demo.frames += 1;
    }
}

// ---------- helpers the steps use ----------

fn angle_to(from: f32, to: f32) -> f32 {
    (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// Turns the view smoothly toward a point, through the look control.
fn aim(x: &mut Ctx, at: Vec3, rate: f32) -> f32 {
    let d = at - x.me.eye();
    let yaw = (-d.x).atan2(-d.z);
    let pitch = (d.y / d.length().max(0.01)).asin();
    let dy = angle_to(x.me.yaw, yaw);
    let dp = pitch - x.me.pitch;
    let k = (x.dt * rate).min(1.0);
    x.c.look.x = -dy * k;
    x.c.look.y = -dp * k;
    dy.abs() + dp.abs()
}

/// Turns toward a spot on the ground and walks there, hopping up steps.
fn walk_to(x: &mut Ctx, to: Vec2, pitch: f32, sprint: bool) -> bool {
    let d = to - Vec2::new(x.me.pos.x, x.me.pos.z);
    if d.length() < 1.2 {
        return true;
    }
    let yaw = (-d.x).atan2(-d.y);
    let dy = angle_to(x.me.yaw, yaw);
    let k = (x.dt * 4.0).min(1.0);
    x.c.look.x = -dy * k;
    x.c.look.y = -(pitch - x.me.pitch) * k;
    if dy.abs() < 0.7 {
        x.c.mv.y = 1.0;
        if sprint && !x.me.sprint && x.frame > 3 {
            x.c.sprint_pressed = true;
        }
    }
    let flat = Vec2::new(x.me.vel.x, x.me.vel.z).length();
    if x.c.mv.y > 0.0 && flat < 1.5 && x.frame > 8 {
        x.mem.stuck += x.dt;
    } else {
        x.mem.stuck = 0.0;
    }
    x.c.jump = x.mem.stuck > 0.12;
    false
}

/// The nearest free mossback.
fn nearest_free(x: &Ctx) -> Option<(u64, Vec3)> {
    x.herd.bodies.values().filter(|b| b.rideable && b.rider.is_none()).map(|b| (b.id, b.pos)).min_by(|a, b| a.1.distance(x.me.pos).total_cmp(&b.1.distance(x.me.pos)))
}

/// Gets on a mossback: walks over, looks at it and presses Interact.
fn mount(x: &mut Ctx) -> bool {
    if x.me.riding.is_some() {
        return true;
    }
    let Some((id, pos)) = nearest_free(x) else { return false };
    x.mem.mount = Some(id);
    let far = Vec2::new(pos.x - x.me.pos.x, pos.z - x.me.pos.z).length();
    if far > 2.6 {
        walk_to(x, Vec2::new(pos.x, pos.z), -0.25, false);
        return false;
    }
    let off = aim(x, pos + Vec3::Y * 0.9, 8.0);
    if off < 0.25 && x.me.aim_body == Some(id) {
        x.c.interact = true;
    }
    false
}

/// Rides along, steering gently; gets back on if thrown.
fn ride(x: &mut Ctx, turn: f32) {
    if x.me.riding.is_none() {
        mount(x);
        return;
    }
    x.c.mv.y = 1.0;
    x.c.look.x = turn * x.dt;
    x.c.look.y = -(-0.08 - x.me.pitch) * (x.dt * 3.0).min(1.0);
}

/// Moves the menu's selection down to an item, one press at a time.
fn menu_to(x: &mut Ctx, label: &str) -> bool {
    let (_, items) = ui::menu_items(x.focus, x.lib, x.scripts, x.settings, x.info);
    let Some(cur) = items.get(x.focus.sel) else { return false };
    if cur.label.contains(label) {
        return true;
    }
    if x.frame % 7 == 3 {
        x.c.down = true;
    }
    false
}

fn slider(x: &Ctx, label: &str) -> Option<(f64, f64)> {
    let (_, items) = ui::menu_items(x.focus, x.lib, x.scripts, x.settings, x.info);
    items.iter().find(|i| i.label.contains(label)).and_then(|i| match i.kind {
        Kind::Slider { value, max, .. } => Some((value, max)),
        _ => None,
    })
}

/// What an editor would save: the remix's main.luau, changed.
fn edit_remix(x: &mut Ctx) -> bool {
    let Some(id) = x.mem.remix.clone() else { return false };
    let Some(dir) = x.lib.list.get(&id).and_then(|c| c.dir.clone()) else { return false };
    let path = dir.join("main.luau");
    let Ok(code) = std::fs::read_to_string(&path) else { return false };
    let code = code
        .replace("color = \"#6fa64a\"", "color = \"#ff7ab8\"")
        .replace("{ \"Hrrmf!\", \"Mmmf.\", \"Off you get!\", \"Blrrp.\" }", "{ \"Wheee!\", \"Up you go!\", \"Fly, friend!\" }")
        .replace("world.random() < 0.5", "world.random() < 0.9")
        .replace("self.patience = world.random(2, 6)", "self.patience = world.random(1, 3)");
    std::fs::write(&path, code).is_ok()
}

// ---------- the play-through ----------

fn script() -> Vec<Step> {
    let green = Vec3::new(world::GREEN.x + 0.5, world::GREEN_Y as f32 + 1.0, world::GREEN.z + 0.5);
    let paddock = world::paddock();
    vec![
        step("Potluck: a test build of M0 and M1", 3.5, |x| {
            if x.frame == 45 {
                x.still = Some("title".into());
            }
            if x.t > 3.2 {
                x.c.accept = true;
            }
            false
        }),
        step("Walk down the hill to the Green", 16.0, move |x| {
            if x.frame == 70 {
                x.still = Some("the-green".into());
            }
            walk_to(x, Vec2::new(paddock.x - 3.5, paddock.z + 1.5), -0.06, true)
        }),
        step("Mossbacks live in the paddock", 3.0, move |x| {
            if x.frame == 60 {
                x.still = Some("paddock".into());
            }
            let at = nearest_free(x).map(|(_, p)| p + Vec3::Y * 0.7).unwrap_or(paddock);
            aim(x, at, 2.5);
            false
        }),
        step("Climb on with Interact", 10.0, |x| {
            x.me.third_person = true;
            mount(x)
        }),
        step("Ride it: a Mossback is a creation written in Luau", 7.0, |x| {
            if x.frame == 120 {
                x.still = Some("riding".into());
            }
            ride(x, if x.t < 3.5 { 0.25 } else { -0.3 });
            false
        }),
        step("Look inside: anyone can read the code that runs it", 6.0, |x| {
            if x.frame == 0 {
                if x.me.riding.is_some() {
                    x.c.look_inside = true;
                } else if let Some(b) = x.herd.bodies.values().next() {
                    x.focus.go(Screen::Inside(b.creation.clone()));
                }
            }
            if x.frame == 75 {
                x.still = Some("look-inside".into());
            }
            if x.frame > 100 && x.frame % 12 == 0 {
                x.c.hotbar_step = 1;
            }
            false
        }),
        step("Remix it: make your own copy to change", 4.0, |x| {
            if x.frame < 40 {
                if menu_to(x, "Remix") && x.frame > 20 {
                    x.c.accept = true;
                }
            }
            if let Screen::Inside(id) = &x.focus.screen {
                if id != "mossback" {
                    x.mem.remix = Some(id.clone());
                }
            }
            if x.frame == 100 {
                x.still = Some("remix".into());
            }
            false
        }),
        step("Change it in any editor: the world reloads it as you save", 6.0, |x| {
            if x.frame == 20 {
                edit_remix(x);
            }
            if x.frame == 30 {
                x.focus.scroll = 0;
            }
            if x.frame == 150 {
                x.still = Some("hot-reload".into());
            }
            false
        }),
        step("Tweak: turn Buck force all the way up", 7.0, |x| {
            if x.frame == 0 {
                x.focus.sel = 0;
            }
            if !matches!(x.focus.screen, Screen::Tweak(_)) {
                if menu_to(x, "Tweak") && x.frame % 7 == 0 {
                    x.c.accept = true;
                }
                return false;
            }
            if let Some((v, max)) = slider(x, "Buck") {
                if menu_to(x, "Buck") {
                    if v < max {
                        if x.frame % 3 == 0 {
                            x.c.right = true;
                        }
                    } else if x.t > 5.0 {
                        x.still = if x.mem.placed == 0 { Some("tweak".into()) } else { None };
                        x.mem.placed = 1;
                    }
                }
            }
            false
        }),
        step("", 1.2, |x| {
            if menu_to(x, "Done") && x.frame % 7 == 0 {
                x.c.accept = true;
            }
            x.mem.placed = 0;
            x.focus.screen == Screen::Playing
        }),
        step("Pink moss, and it bucks hard now. Hold on...", 16.0, |x| {
            let thrown = x.mem.placed == 1;
            if x.me.riding.is_some() && !thrown {
                ride(x, 0.2);
            } else if !thrown && x.mem.mount.is_some() && x.t > 1.0 && x.me.riding.is_none() && x.me.vel.y.abs() > 3.0 {
                // off we go
                x.mem.placed = 1;
                x.mem.from = x.t;
            } else if !thrown {
                mount(x);
            }
            if thrown && x.t - x.mem.from > 0.5 && x.t - x.mem.from < 0.55 {
                x.still = Some("thrown".into());
            }
            thrown && x.me.on_ground && x.t - x.mem.from > 2.0
        }),
        step("Build: every block is yours to place", 9.0, |x| {
            x.me.third_person = false;
            if x.frame == 0 {
                x.mem.placed = 0;
            }
            let k = (x.dt * 5.0).min(1.0);
            x.c.look.y = -((-0.75 + x.mem.placed as f32 * 0.09) - x.me.pitch) * k;
            x.c.hotbar_slot = Some(match x.mem.placed {
                0..=3 => 1,
                4 => 4,
                _ => 3,
            });
            if x.frame > 20 && x.frame % 14 == 0 && x.mem.placed < 7 {
                x.c.place_pressed = true;
                x.mem.placed += 1;
            }
            if x.frame > 20 {
                x.c.look.x = 0.35 * x.dt * if x.mem.placed > 4 { -1.0 } else { 1.0 };
            }
            if x.frame == 240 {
                x.still = Some("building".into());
            }
            false
        }),
        step("Evening on the Green: the lanterns come on", 11.0, move |x| {
            if x.frame == 0 {
                x.mem.from = x.clock.time;
                x.clock.running = false;
                x.me.flying = true;
            }
            x.c.jump = x.t < 1.6;
            aim(x, green + Vec3::Y * 2.0, 2.0);
            let f = (x.t / 9.0).min(1.0);
            let f = f * f * (3.0 - 2.0 * f);
            x.clock.time = x.mem.from + (0.83 - x.mem.from) * f;
            if x.frame == 300 {
                x.still = Some("night".into());
            }
            false
        }),
        step("Potluck: everything here is made by someone you know", 3.0, |x| {
            x.c.look.x = 0.05 * x.dt;
            false
        }),
    ]
}

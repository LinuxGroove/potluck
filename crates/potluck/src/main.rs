//! The Potluck game.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use bevy::light::atmosphere::ScatteringMedium;
use bevy::prelude::*;
use bevy::window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode, WindowResolution};

use potluck::*;

/// How frames reach the screen in play: waiting for the screen (vsync),
/// except where the game draws on another graphics card than the desktop's
/// (laptops that hand each frame to another chip for the screen, like the
/// p16g3). There plain vsync missed many refreshes, 35 to 46 fps where
/// mailbox holds 60, so `pick_present` switches to mailbox once the card is
/// known: the screen shows the newest finished frame at each refresh and the
/// game never waits. Mailbox draws as many frames as it can, so on the
/// desktop's own card it takes all of it and the desktop can't keep up (on
/// amd-halo the window stopped updating until it lost focus).
fn smooth_present() -> PresentMode {
    if cfg!(target_os = "linux") { PresentMode::Fifo } else { PresentMode::AutoVsync }
}

/// Play's window, which `pick_present` and `full_screen` look after (the
/// tools draw at their own size and pace).
#[derive(Resource)]
struct PlayWindow {
    /// The present mode is still to be chosen.
    pick: bool,
}

/// Switches play to mailbox once the graphics card is known, if no screen is
/// plugged into it (`smooth_present`).
fn pick_present(
    mut play: ResMut<PlayWindow>,
    info: Option<Res<bevy::render::renderer::RenderAdapterInfo>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    let (Some(info), Ok(mut window)) = (info, windows.single_mut()) else { return };
    play.pick = false;
    if drawn_off_screen(&info) {
        info!("{} has no screen of its own, so frames go to the screen by mailbox", info.name);
        window.present_mode = PresentMode::Mailbox;
    }
}

/// Whether the game draws on a graphics card with no screen plugged in while
/// another card has one, from the connectors in /sys/class/drm.
#[cfg(target_os = "linux")]
fn drawn_off_screen(info: &bevy::render::renderer::RenderAdapterInfo) -> bool {
    use std::path::Path;
    // software renderers (lavapipe) have no PCI vendor id
    if info.vendor > 0xffff {
        return false;
    }
    let drm = Path::new("/sys/class/drm");
    let id = |p: &Path| std::fs::read_to_string(p).ok().and_then(|s| u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok());
    let mut showing = Vec::new();
    for e in std::fs::read_dir(drm).into_iter().flatten().flatten() {
        // connectors are card1-HDMI-A-1 and the like
        let name = e.file_name().to_string_lossy().into_owned();
        let Some((card, _)) = name.split_once('-') else { continue };
        if std::fs::read_to_string(e.path().join("status")).is_ok_and(|s| s.trim() == "connected") {
            let dev = drm.join(card).join("device");
            if let (Some(v), Some(d)) = (id(&dev.join("vendor")), id(&dev.join("device"))) {
                showing.push((v, d));
            }
        }
    }
    !showing.is_empty() && !showing.contains(&(info.vendor, info.device))
}

#[cfg(not(target_os = "linux"))]
fn drawn_off_screen(_: &bevy::render::renderer::RenderAdapterInfo) -> bool {
    false
}

/// Fills the screen or plays in a window, as the setting says; F11 switches.
fn full_screen(
    keys: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<settings::Settings>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut was: Local<Option<bool>>,
) {
    if keys.just_pressed(KeyCode::F11) {
        settings.fullscreen = !settings.fullscreen;
        settings.save();
    }
    let now = settings.fullscreen;
    // the window starts as main() opened it (--windowed keeps it a window
    // until the player switches)
    if was.replace(now).is_some_and(|w| w != now)
        && let Ok(mut window) = windows.single_mut()
    {
        window.mode = if now { WindowMode::BorderlessFullscreen(MonitorSelection::Current) } else { WindowMode::Windowed };
    }
}

fn main() {
    // NVIDIA's driver on Wayland can lose the window a few seconds in with
    // explicit sync ("Timeline acquire sync point set, but no buffer
    // committed"), so it's off unless set otherwise
    #[cfg(target_os = "linux")]
    if std::env::var_os("__NV_DISABLE_EXPLICIT_SYNC").is_none() {
        // SAFETY: nothing else is running yet to read the environment
        unsafe { std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1") };
    }
    let mut settings = settings::Settings::load();
    share::launch_ping(&settings.server);
    let args: Vec<String> = std::env::args().collect();
    let opt = |k: &str| args.iter().find_map(|a| a.strip_prefix(&format!("--{k}=")).map(|s| s.to_string()));
    // try a lighting tier without changing the setting: --lighting=high
    if let Some(l) = opt("lighting") {
        settings.lighting = render::Tier::NAMES[render::Tier::of(&l) as usize].into();
    }
    let size = opt("size").and_then(|s| s.split_once('x').and_then(|(w, h)| Some(UVec2::new(w.parse().ok()?, h.parse().ok()?)))).unwrap_or(UVec2::new(1600, 900));
    // screenshots and recordings draw offscreen at exactly --size, in a window
    // that fits the screen; --onscreen draws a shot straight to a window of
    // that size, as play does, to measure frame rates the way players get them
    let onscreen = args.iter().any(|a| a == "--onscreen");
    let offscreen = (opt("shot").is_some() && !onscreen) || opt("demo").is_some();
    let window = if offscreen { (size.as_vec2() * (1600.0 / size.x as f32).min(900.0 / size.y as f32).min(1.0)).as_uvec2() } else { size };
    let mut resolution = WindowResolution::new(window.x, window.y);
    if onscreen {
        resolution = resolution.with_scale_factor_override(1.0);
    }
    // play fills the screen unless the setting says otherwise; --windowed or
    // --size opens a window this once
    let play = !offscreen && !onscreen;
    let windowed = args.iter().any(|a| a == "--windowed") || opt("size").is_some();
    let mode = if play && settings.fullscreen && !windowed { WindowMode::BorderlessFullscreen(MonitorSelection::Primary) } else { WindowMode::Windowed };
    let present_asked = opt("present").is_some() || args.iter().any(|a| a == "--no-vsync");
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Potluck".into(),
            resolution,
            mode,
            // --latency=N lets N frames queue for the screen (2 unless asked)
            desired_maximum_frame_latency: opt("latency").and_then(|n| n.parse().ok()).and_then(std::num::NonZero::new),
            // --no-vsync to see how fast a card really draws; the tools draw
            // offscreen at their own pace, so they never wait for the screen;
            // --present=immediate|mailbox|fifo|relaxed asks for one mode
            present_mode: match opt("present").as_deref() {
                Some("immediate") => PresentMode::Immediate,
                Some("mailbox") => PresentMode::Mailbox,
                Some("fifo") => PresentMode::Fifo,
                Some("relaxed") => PresentMode::FifoRelaxed,
                _ if offscreen || args.iter().any(|a| a == "--no-vsync") => PresentMode::AutoNoVsync,
                _ => smooth_present(),
            },
            ..default()
        }),
        ..default()
    }));
    sim(&mut app, SEED);
    app.add_plugins(MaterialPlugin::<render::WaterMaterial>::default());
    render::add_menu_blur(&mut app);
    if offscreen {
        app.insert_resource(OffscreenSize(size));
    }
    if play {
        app.insert_resource(PlayWindow { pick: !present_asked })
            .add_systems(Update, (pick_present.run_if(|p: Res<PlayWindow>| p.pick), full_screen));
    }
    #[cfg(feature = "raytracing")]
    if render::Tier::of(&settings.lighting) == render::Tier::Ultra {
        // checks the graphics card itself, and stays idle on one that can't ray trace
        app.add_plugins(bevy::solari::SolariPlugins).add_systems(PostUpdate, render::raytrace_meshes);
        // the ray tracer binds each vertex slab whole, and some cards bind at most 128 MiB
        use bevy::render::{mesh::allocator::MeshAllocatorSettings, slab_allocator::SlabAllocatorSettings};
        let slabs = SlabAllocatorSettings { max_slab_size: 64 << 20, large_threshold: 32 << 20, ..default() };
        app.sub_app_mut(bevy::render::RenderApp).insert_resource(MeshAllocatorSettings { slab_allocator_settings: slabs, ..default() });
    }
    app.insert_resource(settings)
        .init_resource::<input::Controls>()
        .init_resource::<player::Me>()
        .init_resource::<mesh::ChunkEntities>()
        .init_resource::<mesh::MeshFocus>()
        .init_resource::<mesh::Cover>()
        .init_resource::<creatures::PartMaterials>()
        .init_resource::<ui::Labels>()
        .insert_resource(waypoints::Waypoints::load())
        .init_resource::<waypoints::Travel>()
        .init_resource::<map::MapView>()
        .init_resource::<map::MapImage>()
        .add_systems(Startup, ((ui::fonts, setup, ui::spawn_hud, map::spawn_hud, map::spawn_map, chat::spawn_feed, waypoints::spawn_fade).chain(), render::motes::setup))
        .add_systems(PostStartup, map::palette)
        .add_systems(
            Update,
            (
                (
                    input::read,
                    keyboard::input,
                    ui::menu_input,
                    map::map_input,
                    ui::menu_owns,
                    map::map_acts,
                    chat::acts,
                    game::do_acts,
                    map::fill,
                    chat::fill,
                    ui::draw_menu,
                    photos::take,
                    game::title_view,
                )
                    .chain(),
                (player::look, player::walk, game::interact, player::build, player::settle_plants, map::keys, chat::keys, waypoints::follow_world, waypoints::travel).chain(),
            )
                .chain()
                .before(Sim),
        )
        .add_systems(
            Update,
            (
                (net::client_net, net::lan_list, game::riding, creatures::ride, player::place_camera).chain(),
                (focus_mesh, mesh::remesh, mesh::wind).chain(),
                (render::sky, render::clouds::drift, render::dome::follow, render::lanterns, render::flicker, render::motes::motes, creatures::draw, (net::draw_players, net::draw_me, creatures::people).chain(), game::labels, ui::update_hud, ui::cursor, ui::outline),
                (map::refresh, map::draw_hud, map::make_room, map::draw_map, chat::draw_feed, keyboard::draw),
            )
                .chain()
                .after(Sim),
        );
    // where a frame's time goes, logged with a shot: --timings
    if args.iter().any(|a| a == "--timings") {
        app.add_plugins((bevy::diagnostic::FrameTimeDiagnosticsPlugin::default(), bevy::render::diagnostic::RenderDiagnosticsPlugin));
        render::timings::add(&mut app);
    }
    if args.iter().any(|a| a == "--play") {
        app.add_systems(PostStartup, |mut acts: ResMut<ui::Acts>| acts.0.push(ui::Act::PlayAlone));
    }
    // a scripted play-through, recorded: --demo=DIR [--size=1280x720]
    if let Some(dir) = opt("demo") {
        demo::add(&mut app, dir.into(), size);
    }
    // a screenshot: --shot=out.png [--after=8] [--at=x,y,z,yaw,pitch] [--time=0.5] [--screen=inside]
    // (a y of 0 or less is that far above the ground: --at=186,-1.5,286,0,-5;
    // --at=creature looks a creature in the face, --at=creature:2 the third;
    // --person[=hue] stands a player in view)
    if let Some(path) = opt("shot") {
        let after: f32 = opt("after").and_then(|v| v.parse().ok()).unwrap_or(8.0);
        let at: Option<Vec<f32>> = opt("at").map(|v| v.split(',').filter_map(|n| n.parse().ok()).collect());
        let tod: Option<f32> = opt("time").and_then(|v| v.parse().ok());
        let screen = opt("screen");
        let creature = opt("at").and_then(|a| a.strip_prefix("creature").map(|n| n.trim_start_matches(':').to_string()));
        // --person[=hue] stands a player a few blocks ahead, facing you
        let person = args.iter().find(|a| a.starts_with("--person")).map(|a| a.strip_prefix("--person=").and_then(|h| h.parse().ok()).unwrap_or(30.0));
        // --lines puts a few lines of chat on the screen, as friends might say
        let lines = args.iter().any(|a| a == "--lines");
        app.insert_resource(Shot { path, after, at, tod, screen, creature, person, lines, done: false, frames: 0 });
        app.add_systems(Update, shot.after(Sim));
        // no outline round the block in the crosshair
        app.add_systems(Startup, |mut gizmos: ResMut<GizmoConfigStore>| gizmos.config_mut::<DefaultGizmoConfigGroup>().0.enabled = false);
    }
    app.run();
}

/// The size the tools draw at, offscreen.
#[derive(Resource)]
struct OffscreenSize(UVec2);

#[derive(Resource)]
struct Shot {
    path: String,
    after: f32,
    at: Option<Vec<f32>>,
    tod: Option<f32>,
    screen: Option<String>,
    /// Which creature to look in the face, counting by id, or the first of a creation's.
    creature: Option<String>,
    /// A player to stand in view, by their shirt's hue, until it's placed.
    person: Option<f32>,
    /// Lines of chat to show.
    lines: bool,
    done: bool,
    /// Frames drawn in the last three seconds before the picture, for the frame rate.
    frames: u32,
}

fn shot(
    mut commands: Commands,
    time: Res<Time>,
    mut s: ResMut<Shot>,
    mut me: ResMut<player::Me>,
    mut clock: ResMut<render::Clock>,
    mut focus: ResMut<ui::Focus>,
    mut info: ResMut<ui::WorldInfo>,
    herd: Res<creatures::Herd>,
    voxels: Res<world::Voxels>,
    off: Option<Res<render::Offscreen>>,
    diagnostics: Option<Res<bevy::diagnostic::DiagnosticsStore>>,
    frames: Option<Res<render::timings::Shared>>,
    mut exit: MessageWriter<AppExit>,
    mut chat: ResMut<chat::Chat>,
) {
    use bevy::render::view::screenshot::save_to_disk;
    if let Some(t) = s.tod {
        clock.time = t;
        clock.running = false;
    }
    if let Some(at) = &s.at {
        if at.len() >= 5 {
            // a height of 0 or less is above the ground there: -1.5 stands 1.5 up,
            // moved to the nearest spot clear of trunks and leaves (forests)
            let (mut x, mut z) = (at[0], at[2]);
            let y = if at[1] <= 0.0 {
                let up = (-at[1]).ceil() as i32 + 1;
                let clear = |x: i32, z: i32| {
                    let t = voxels.top(x, z);
                    voxels.get(x, t, z) != blocks::LOG
                        && (1..=up).all(|d| {
                            let b = voxels.get(x, t + d, z);
                            b == blocks::AIR || blocks::shape(b) == blocks::Shape::Cross
                        })
                };
                let (cx, cz) = (x.floor() as i32, z.floor() as i32);
                if let Some((dx, dz)) = (0..=6i32)
                    .flat_map(|r| (-r..=r).flat_map(move |dx| (-r..=r).map(move |dz| (dx, dz))).filter(move |&(dx, dz)| dx.abs().max(dz.abs()) == r))
                    .find(|&(dx, dz)| clear(cx + dx, cz + dz))
                {
                    x += dx as f32;
                    z += dz as f32;
                }
                voxels.top(x.floor() as i32, z.floor() as i32) as f32 + 1.0 - at[1]
            } else {
                at[1]
            };
            me.pos = Vec3::new(x, y, z);
            me.vel = Vec3::ZERO;
            me.yaw = at[3].to_radians();
            me.pitch = at[4].to_radians();
            me.flying = true;
        }
    }
    if let Some(n) = s.creature.clone() {
        let mut ids: Vec<_> = herd.bodies.iter().filter(|(_, b)| n.parse::<usize>().is_ok() || n.is_empty() || b.creation == n).map(|(id, _)| *id).collect();
        ids.sort();
        let n = n.parse::<usize>().unwrap_or(0);
        if let Some(b) = ids.get(n).and_then(|id| herd.bodies.get(id)) {
            // creatures face -z, turned by their yaw; stand a few blocks in
            // front, a little above, looking back at it
            let fwd = Quat::from_rotation_y(b.yaw) * Vec3::NEG_Z;
            let reach = 1.5 + b.half.x.max(b.half.z) * 1.8;
            me.pos = b.pos + fwd * reach + Vec3::Y * (b.half.y * 1.1 - player::EYE);
            me.vel = Vec3::ZERO;
            me.yaw = fwd.x.atan2(fwd.z);
            me.pitch = -0.15;
            me.flying = true;
        }
    }
    if let Some(hue) = s.person.take() {
        let fwd = Vec3::new(-me.yaw.sin(), 0.0, -me.yaw.cos());
        let mut at = me.pos + fwd * 2.4;
        at.y = voxels.top(at.x.floor() as i32, at.z.floor() as i32) as f32 + 1.0;
        commands.spawn((Transform::from_translation(at).with_rotation(Quat::from_rotation_y(me.yaw + std::f32::consts::PI)), Visibility::default(), creatures::Person::new(hue)));
    }
    let t = time.elapsed_secs();
    if t > 1.0 && focus.screen == ui::Screen::Title {
        if let Some(sc) = s.screen.clone() {
            info.in_world = true;
            let id = s.creature.clone().filter(|c| !c.is_empty() && c.parse::<usize>().is_err()).or_else(|| herd.bodies.values().next().map(|b| b.creation.clone())).unwrap_or("mossback".into());
            // a page of Settings or a shelf of Creations: settings:2
            let (sc, page) = sc.split_once(':').map(|(a, b)| (a.to_string(), b.parse().unwrap_or(0))).unwrap_or((sc, 0));
            focus.go(match sc.as_str() {
                "inside" => ui::Screen::Inside(id.clone()),
                "tweak" => ui::Screen::Tweak(id.clone()),
                "title" => ui::Screen::Title,
                "pause" => ui::Screen::Pause,
                "map" => ui::Screen::Map,
                "chat" | "keyboard" => ui::Screen::Chat,
                "settings" => ui::Screen::Settings,
                "creations" => ui::Screen::Library,
                "market" => ui::Screen::Link,
                "question" => ui::Screen::Panel,
                _ => ui::Screen::Playing,
            });
            focus.settings_page = page;
            focus.shelf = page;
            if sc == "market" {
                // what the market would send: a sign-in link for the phone
                focus.link = Some(("https://potluck.linuxgroove.com/in/7QK2-M9XD-PL4F-W2HZ".into(), "The market".into()));
            }
            if sc == "question" {
                // as a creation asks: through Me, which keeps it open
                me.panel = Some(potluck::script::Panel { creation: id.clone(), ask: 1, title: "Ride me?".into(), text: "I'll take you up the hill, but you'll have to hold on.".into(), choices: vec!["Climb on".into(), "Not now".into()] });
                focus.panel = me.panel.clone();
            }
            if sc == "keyboard" {
                // a pad typing a line
                focus.fields.insert("say".into(), "see you at the c".into());
                keyboard::open(&mut focus, "say", "Say something");
                if let Some(k) = focus.keyboard.as_mut() {
                    (k.row, k.col) = (3, 6);
                }
            }
            if s.lines {
                let now = time.elapsed_secs_f64();
                let p = me.pos.floor().as_ivec3();
                let say = [
                    (2, "Kaden", "found a cave under the hill!", None),
                    (2, "Kaden", "", Some(chat::Place { name: "Kaden's cave".into(), at: [p.x + 40, p.y - 6, p.z - 30] })),
                    (3, "Drew", "on my way", None),
                    (4, "Cy", "Nice build!", None),
                ];
                for (i, (id, name, text, place)) in say.into_iter().enumerate() {
                    let line = chat::Line { id: i as u64 + 1, name: name.into(), from: chat::Source::Player(id), text: text.into(), place, young: id == 4, at: hosting::now() };
                    chat.keep(line, now);
                }
            }
        }
    }
    if t > s.after - 3.0 && !s.done {
        s.frames += 1;
    }
    if t > s.after && !s.done {
        s.done = true;
        info!("Shot: {:.0} frames a second over the last 3 s", s.frames as f32 / 3.0);
        if let Some(f) = frames {
            f.report(3.0);
        }
        if let Some(d) = diagnostics {
            // each pass's CPU and GPU time, the dearest first
            let mut all: Vec<(String, f64)> = d
                .iter()
                .filter(|x| x.path().as_str().ends_with("elapsed_gpu") || x.path().as_str().ends_with("elapsed_cpu"))
                .filter_map(|x| Some((x.path().to_string(), x.smoothed()?)))
                .collect();
            all.sort_by(|a, b| b.1.total_cmp(&a.1));
            for (path, v) in all {
                info!("Timing: {path} {v:.3}");
            }
        }
        let path = s.path.clone();
        commands.spawn(render::screenshot(off.as_deref())).observe(save_to_disk(path));
    }
    if t > s.after + 2.0 {
        exit.write(AppExit::Success);
    }
}

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut waters: ResMut<Assets<render::WaterMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut media: ResMut<Assets<ScatteringMedium>>,
    settings: Res<settings::Settings>,
    voxels: Res<world::Voxels>,
    mut me: ResMut<player::Me>,
    device: Option<Res<bevy::render::renderer::RenderDevice>>,
    offscreen: Option<Res<OffscreenSize>>,
) {
    let atlas = textures::build(&mut images);
    let rt = render::Raytraced::new(&settings, device.as_deref());
    commands.insert_resource(render::make_materials(&atlas, &mut mats, &mut waters, rt, render::Tier::of(&settings.lighting)));
    commands.insert_resource(atlas);
    commands.insert_resource(rt);
    commands.insert_resource(mesh::Cover::of(render::Tier::of(&settings.lighting)));
    mesh::spawn_wind(&mut commands);
    let off = offscreen.map(|o| render::Offscreen::new(&mut images, o.0));
    render::spawn_sky(&mut commands, &settings, &mut media, rt, off.as_ref());
    render::spawn_sea(&mut commands, &mut meshes, &mut mats);
    render::clouds::spawn(&mut commands, &mut meshes, &mut mats, &mut images);
    if render::Tier::of(&settings.lighting) == render::Tier::Low {
        render::dome::spawn(&mut commands, &mut meshes, &mut mats);
    }
    if let Some(off) = off {
        commands.insert_resource(off);
    }
    me.respawn(&voxels, None);
}

fn focus_mesh(me: Res<player::Me>, mut focus: ResMut<mesh::MeshFocus>) {
    focus.0 = me.eye();
}

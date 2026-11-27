//! The Potluck game.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use bevy::light::atmosphere::ScatteringMedium;
use bevy::prelude::*;
use bevy::window::{PresentMode, WindowResolution};

use potluck::*;

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
    // that fits the screen
    let offscreen = opt("shot").is_some() || opt("demo").is_some();
    let window = if offscreen { (size.as_vec2() * (1600.0 / size.x as f32).min(900.0 / size.y as f32).min(1.0)).as_uvec2() } else { size };
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Potluck".into(),
            resolution: WindowResolution::new(window.x, window.y),
            // --no-vsync to see how fast a card really draws; the tools draw
            // offscreen at their own pace, so they never wait for the screen
            present_mode: if offscreen || args.iter().any(|a| a == "--no-vsync") { PresentMode::AutoNoVsync } else { PresentMode::AutoVsync },
            ..default()
        }),
        ..default()
    }));
    sim(&mut app, SEED);
    if offscreen {
        app.insert_resource(OffscreenSize(size));
    }
    #[cfg(feature = "raytracing")]
    if render::Tier::of(&settings.lighting) == render::Tier::Ultra {
        // checks the graphics card itself, and stays idle on one that can't ray trace
        app.add_plugins((bevy::solari::SolariPlugins, bevy::post_process::auto_exposure::AutoExposurePlugin))
            .add_systems(PostUpdate, render::raytrace_meshes)
            .add_systems(PostStartup, render::adapt::setup)
            .add_systems(Update, render::adapt::follow_day);
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
        .add_systems(Startup, ((ui::fonts, setup, ui::spawn_hud).chain(), render::motes::setup))
        .add_systems(
            Update,
            (
                (input::read, ui::menu_input, game::do_acts, ui::draw_menu, game::title_view).chain(),
                (player::look, player::walk, game::interact, player::build, player::settle_plants).chain(),
            )
                .chain()
                .before(Sim),
        )
        .add_systems(
            Update,
            (
                (net::client_net, net::lan_list, game::riding, creatures::ride, player::place_camera).chain(),
                (focus_mesh, mesh::remesh, mesh::wind).chain(),
                (render::sky, render::lanterns, render::flicker, render::motes::motes, creatures::draw, net::draw_players, net::draw_me, game::labels, ui::update_hud, ui::cursor, ui::outline),
            )
                .chain()
                .after(Sim),
        );
    if args.iter().any(|a| a == "--play") {
        app.add_systems(PostStartup, |mut acts: ResMut<ui::Acts>| acts.0.push(ui::Act::PlayAlone));
    }
    // a scripted play-through, recorded: --demo=DIR [--size=1280x720]
    if let Some(dir) = opt("demo") {
        demo::add(&mut app, dir.into(), size);
    }
    // a screenshot: --shot=out.png [--after=8] [--at=x,y,z,yaw,pitch] [--time=0.5] [--screen=inside]
    // (a y of 0 or less is that far above the ground: --at=186,-1.5,286,0,-5)
    if let Some(path) = opt("shot") {
        let after: f32 = opt("after").and_then(|v| v.parse().ok()).unwrap_or(8.0);
        let at: Option<Vec<f32>> = opt("at").map(|v| v.split(',').filter_map(|n| n.parse().ok()).collect());
        let tod: Option<f32> = opt("time").and_then(|v| v.parse().ok());
        let screen = opt("screen");
        app.insert_resource(Shot { path, after, at, tod, screen, done: false, frames: 0 });
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
    mut exit: MessageWriter<AppExit>,
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
    let t = time.elapsed_secs();
    if t > 1.0 && focus.screen == ui::Screen::Title {
        if let Some(sc) = s.screen.clone() {
            info.in_world = true;
            let id = herd.bodies.values().next().map(|b| b.creation.clone()).unwrap_or("mossback".into());
            focus.go(match sc.as_str() {
                "inside" => ui::Screen::Inside(id),
                "tweak" => ui::Screen::Tweak(id),
                "title" => ui::Screen::Title,
                "pause" => ui::Screen::Pause,
                _ => ui::Screen::Playing,
            });
        }
    }
    if t > s.after - 3.0 && !s.done {
        s.frames += 1;
    }
    if t > s.after && !s.done {
        s.done = true;
        info!("Shot: {:.0} frames a second over the last 3 s", s.frames as f32 / 3.0);
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
    commands.insert_resource(render::make_materials(&atlas, &mut mats, rt));
    commands.insert_resource(atlas);
    commands.insert_resource(rt);
    commands.insert_resource(mesh::Cover::of(render::Tier::of(&settings.lighting)));
    mesh::spawn_wind(&mut commands);
    let off = offscreen.map(|o| render::Offscreen::new(&mut images, o.0));
    render::spawn_sky(&mut commands, &settings, &mut media, rt, off.as_ref());
    render::spawn_sea(&mut commands, &mut meshes, &mut mats);
    if let Some(off) = off {
        commands.insert_resource(off);
    }
    me.respawn(&voxels, None);
}

fn focus_mesh(me: Res<player::Me>, mut focus: ResMut<mesh::MeshFocus>) {
    focus.0 = me.eye();
}

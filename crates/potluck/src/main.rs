//! The Potluck game.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use bevy::light::atmosphere::ScatteringMedium;
use bevy::prelude::*;
use bevy::window::{PresentMode, WindowResolution};

use potluck::*;

fn main() {
    let mut settings = settings::Settings::load();
    share::launch_ping(&settings.server);
    let args: Vec<String> = std::env::args().collect();
    let opt = |k: &str| args.iter().find_map(|a| a.strip_prefix(&format!("--{k}=")).map(|s| s.to_string()));
    // try a lighting tier without changing the setting: --lighting=high
    if let Some(l) = opt("lighting") {
        settings.lighting = render::Tier::NAMES[render::Tier::of(&l) as usize].into();
    }
    let size = opt("size").and_then(|s| s.split_once('x').and_then(|(w, h)| Some(UVec2::new(w.parse().ok()?, h.parse().ok()?)))).unwrap_or(UVec2::new(1600, 900));
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Potluck".into(),
            resolution: WindowResolution::new(size.x, size.y),
            present_mode: PresentMode::AutoVsync,
            ..default()
        }),
        ..default()
    }));
    sim(&mut app, SEED);
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
        .init_resource::<creatures::PartMaterials>()
        .init_resource::<ui::Labels>()
        .add_systems(Startup, (ui::fonts, setup, ui::spawn_hud).chain())
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
                (focus_mesh, mesh::remesh).chain(),
                (render::sky, render::lanterns, creatures::draw, net::draw_players, net::draw_me, game::labels, ui::update_hud, ui::cursor, ui::outline),
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
    if let Some(path) = opt("shot") {
        let after: f32 = opt("after").and_then(|v| v.parse().ok()).unwrap_or(8.0);
        let at: Option<Vec<f32>> = opt("at").map(|v| v.split(',').filter_map(|n| n.parse().ok()).collect());
        let tod: Option<f32> = opt("time").and_then(|v| v.parse().ok());
        let screen = opt("screen");
        app.insert_resource(Shot { path, after, at, tod, screen, done: false });
        app.add_systems(Update, shot.after(Sim));
    }
    app.run();
}

#[derive(Resource)]
struct Shot {
    path: String,
    after: f32,
    at: Option<Vec<f32>>,
    tod: Option<f32>,
    screen: Option<String>,
    done: bool,
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
    mut exit: MessageWriter<AppExit>,
) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    if let Some(t) = s.tod {
        clock.time = t;
        clock.running = false;
    }
    if let Some(at) = &s.at {
        if at.len() >= 5 {
            me.pos = Vec3::new(at[0], at[1], at[2]);
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
    if t > s.after && !s.done {
        s.done = true;
        let path = s.path.clone();
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
    if t > s.after + 2.0 {
        exit.write(AppExit::Success);
    }
}

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut media: ResMut<Assets<ScatteringMedium>>,
    settings: Res<settings::Settings>,
    voxels: Res<world::Voxels>,
    mut me: ResMut<player::Me>,
    device: Option<Res<bevy::render::renderer::RenderDevice>>,
) {
    let atlas = textures::build(&mut images);
    commands.insert_resource(render::make_materials(&atlas, &mut mats));
    commands.insert_resource(atlas);
    let rt = render::Raytraced::new(&settings, device.as_deref());
    commands.insert_resource(rt);
    render::spawn_sky(&mut commands, &settings, &mut media, rt);
    me.respawn(&voxels, None);
}

fn focus_mesh(me: Res<player::Me>, mut focus: ResMut<mesh::MeshFocus>) {
    focus.0 = me.eye();
}

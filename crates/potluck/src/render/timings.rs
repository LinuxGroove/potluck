//! Where a frame's time goes, for `--timings`: how long each frame took,
//! the main world's systems, the render world's in its stages (preparing
//! assets, views, queueing, preparing buffers, recording and submitting,
//! cleaning up), how long it waited for the window's next image (where
//! vsync shows up), and how many meshes and materials changed. Logged with
//! a shot, next to each render pass's own CPU and GPU time from Bevy's
//! render diagnostics.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use bevy::prelude::*;
use bevy::render::view::prepare_windows;
use bevy::render::{Render, RenderApp, RenderSystems};

/// The render world's stages, each ending where the next begins.
const STAGES: [&str; 6] = ["assets", "views", "queue", "prepare", "render", "cleanup"];

/// One frame: when it ended (seconds since start), its length, the main
/// and render worlds' busy times and the render world's stages, in
/// milliseconds, and what changed.
#[derive(Clone, Copy, Default)]
struct Frame {
    at: f32,
    length: f32,
    main: f32,
    render: f32,
    acquire: f32,
    stages: [f32; 6],
    meshes: f32,
    materials: f32,
    drawn: f32,
}

/// The render world's last times, and the frames.
#[derive(Default)]
struct Log {
    render: f32,
    acquire: f32,
    stages: [f32; 6],
    frames: Vec<Frame>,
}

/// What both worlds share; the shot reads it with `report`.
#[derive(Resource, Clone, Default)]
pub struct Shared(Arc<Mutex<Log>>);

#[derive(Resource, Default)]
struct Started(Option<Instant>);

#[derive(Resource, Default)]
struct RenderStarted {
    frame: Option<Instant>,
    acquire: Option<Instant>,
    /// When each stage ended.
    marks: [Option<Instant>; 5],
}

/// Points between the render world's sets, where a stage ends.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Mark(usize);

pub fn add(app: &mut App) {
    let shared = Shared::default();
    app.insert_resource(shared.clone())
        .init_resource::<Started>()
        .add_systems(First, |mut s: ResMut<Started>| s.0 = Some(Instant::now()))
        .add_systems(Last, end_main);
    let Some(render) = app.get_sub_app_mut(RenderApp) else { return };
    use RenderSystems as R;
    render.configure_sets(
        Render,
        (
            Mark(0).after(R::PrepareMeshes).before(R::CreateViews),
            Mark(1).after(R::PrepareViews).before(R::Queue),
            Mark(2).after(R::PhaseSort).before(R::Prepare),
            Mark(3).after(R::Prepare).before(R::Render),
            Mark(4).after(R::Render).before(R::Cleanup),
        ),
    );
    for i in 0..5 {
        render.add_systems(Render, (move |mut s: ResMut<RenderStarted>| s.marks[i] = Some(Instant::now())).in_set(Mark(i)));
    }
    render
        .insert_resource(shared)
        .init_resource::<RenderStarted>()
        .add_systems(Render, (|mut s: ResMut<RenderStarted>| s.frame = Some(Instant::now())).in_set(R::ExtractCommands))
        .add_systems(
            Render,
            (
                (|mut s: ResMut<RenderStarted>| s.acquire = Some(Instant::now())).before(prepare_windows),
                (|s: Res<RenderStarted>, shared: Res<Shared>| {
                    if let (Some(at), Ok(mut log)) = (s.acquire, shared.0.lock()) {
                        log.acquire = at.elapsed().as_secs_f32() * 1000.0;
                    }
                })
                .after(prepare_windows),
            )
                .in_set(R::PrepareViews),
        )
        .add_systems(
            Render,
            (|s: Res<RenderStarted>, shared: Res<Shared>| {
                let Some(start) = s.frame else { return };
                let Ok(mut log) = shared.0.lock() else { return };
                let now = Instant::now();
                log.render = (now - start).as_secs_f32() * 1000.0;
                let mut from = start;
                for i in 0..6 {
                    let end = if i < 5 { s.marks[i].unwrap_or(from) } else { now };
                    log.stages[i] = end.saturating_duration_since(from).as_secs_f32() * 1000.0;
                    from = end;
                }
            })
            .in_set(R::PostCleanup),
        );
}

fn end_main(
    started: Res<Started>,
    shared: Res<Shared>,
    time: Res<Time<Real>>,
    mut meshes: MessageReader<AssetEvent<Mesh>>,
    mut materials: MessageReader<AssetEvent<StandardMaterial>>,
    drawn: Query<&ViewVisibility, With<Mesh3d>>,
) {
    let meshes = meshes.read().filter(|e| matches!(e, AssetEvent::Added { .. } | AssetEvent::Modified { .. })).count() as f32;
    let materials = materials.read().filter(|e| matches!(e, AssetEvent::Added { .. } | AssetEvent::Modified { .. })).count() as f32;
    let drawn = drawn.iter().filter(|v| v.get()).count() as f32;
    let (Some(at), Ok(mut log)) = (started.0, shared.0.lock()) else { return };
    let frame = Frame {
        at: time.elapsed_secs(),
        length: time.delta_secs() * 1000.0,
        main: at.elapsed().as_secs_f32() * 1000.0,
        render: log.render,
        acquire: log.acquire,
        stages: log.stages,
        meshes,
        materials,
        drawn,
    };
    log.frames.push(frame);
    // a minute at 240 frames a second is plenty
    if log.frames.len() > 15_000 {
        log.frames.drain(..5_000);
    }
}

impl Shared {
    /// Logs the last `seconds` of frames: percentiles of each time, and how
    /// many frames fell in each vsync step at 60 and 120 Hz.
    pub fn report(&self, seconds: f32) {
        let Ok(log) = self.0.lock() else { return };
        let Some(last) = log.frames.last() else { return };
        let since = last.at - seconds;
        let frames: Vec<Frame> = log.frames.iter().copied().filter(|f| f.at > since).collect();
        let pick = |f: &dyn Fn(&Frame) -> f32| {
            let mut v: Vec<f32> = frames.iter().map(f).collect();
            v.sort_by(|a, b| a.total_cmp(b));
            let at = |p: f32| v[((v.len() - 1) as f32 * p).round() as usize];
            format!("median {:.2} 90% {:.2} 99% {:.2} most {:.2}", at(0.5), at(0.9), at(0.99), v[v.len() - 1])
        };
        info!("Frames: {} over {seconds:.1} s", frames.len());
        info!("Frame length ms: {}", pick(&|f| f.length));
        info!("Main world ms: {}", pick(&|f| f.main));
        info!("Render world ms: {}", pick(&|f| f.render));
        for (i, name) in STAGES.iter().enumerate() {
            info!("Render world {name} ms: {}", pick(&|f| f.stages[i]));
        }
        info!("Waiting for the window ms: {}", pick(&|f| f.acquire));
        info!("Meshes changed a frame: {}", pick(&|f| f.meshes));
        info!("Materials changed a frame: {}", pick(&|f| f.materials));
        info!("Meshes in view: {}", pick(&|f| f.drawn));
        let steps = [8.7, 17.0, 25.3, 33.7, f32::MAX];
        let mut counts = [0usize; 5];
        for f in &frames {
            counts[steps.iter().position(|s| f.length <= *s).unwrap()] += 1;
        }
        info!("Frame lengths: to 8.7 ms {}, to 17 ms {}, to 25.3 ms {}, to 33.7 ms {}, longer {}", counts[0], counts[1], counts[2], counts[3], counts[4]);
    }
}

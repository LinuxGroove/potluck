//! Where a frame's time goes, for `--timings`: how long each frame took,
//! the main world's systems, the render world's, and how much of that the
//! render world spent waiting for the window's next image (which is where
//! vsync and a busy card show up). Logged with a shot, next to each render
//! pass's own CPU and GPU time from Bevy's render diagnostics.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use bevy::prelude::*;
use bevy::render::view::prepare_windows;
use bevy::render::{Render, RenderApp, RenderSystems};

/// One frame: when it ended (seconds since start), its length, and the
/// main and render worlds' busy times, in milliseconds.
#[derive(Clone, Copy, Default)]
struct Frame {
    at: f32,
    length: f32,
    main: f32,
    render: f32,
    acquire: f32,
}

/// What both worlds share: the render world's last times, and the frames.
#[derive(Default)]
struct Log {
    render: f32,
    acquire: f32,
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
}

pub fn add(app: &mut App) {
    let shared = Shared::default();
    app.insert_resource(shared.clone())
        .init_resource::<Started>()
        .add_systems(First, |mut s: ResMut<Started>| s.0 = Some(Instant::now()))
        .add_systems(Last, end_main);
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render
            .insert_resource(shared)
            .init_resource::<RenderStarted>()
            .add_systems(Render, (|mut s: ResMut<RenderStarted>| s.frame = Some(Instant::now())).in_set(RenderSystems::ExtractCommands))
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
                    .in_set(RenderSystems::PrepareViews),
            )
            .add_systems(
                Render,
                (|s: Res<RenderStarted>, shared: Res<Shared>| {
                    if let (Some(at), Ok(mut log)) = (s.frame, shared.0.lock()) {
                        log.render = at.elapsed().as_secs_f32() * 1000.0;
                    }
                })
                .in_set(RenderSystems::PostCleanup),
            );
    }
}

fn end_main(started: Res<Started>, shared: Res<Shared>, time: Res<Time<Real>>) {
    let (Some(at), Ok(mut log)) = (started.0, shared.0.lock()) else { return };
    let frame = Frame {
        at: time.elapsed_secs(),
        length: time.delta_secs() * 1000.0,
        main: at.elapsed().as_secs_f32() * 1000.0,
        render: log.render,
        acquire: log.acquire,
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
        let pick = |f: fn(&Frame) -> f32| {
            let mut v: Vec<f32> = frames.iter().map(f).collect();
            v.sort_by(|a, b| a.total_cmp(b));
            let at = |p: f32| v[((v.len() - 1) as f32 * p).round() as usize];
            format!("median {:.2} 90% {:.2} 99% {:.2} most {:.2}", at(0.5), at(0.9), at(0.99), v[v.len() - 1])
        };
        info!("Frames: {} over {seconds:.1} s", frames.len());
        info!("Frame length ms: {}", pick(|f| f.length));
        info!("Main world ms: {}", pick(|f| f.main));
        info!("Render world ms: {}", pick(|f| f.render));
        info!("Waiting for the window ms: {}", pick(|f| f.acquire));
        let steps = [8.7, 17.0, 25.3, 33.7, f32::MAX];
        let mut counts = [0usize; 5];
        for f in &frames {
            counts[steps.iter().position(|s| f.length <= *s).unwrap()] += 1;
        }
        info!("Frame lengths: to 8.7 ms {}, to 17 ms {}, to 25.3 ms {}, to 33.7 ms {}, longer {}", counts[0], counts[1], counts[2], counts[3], counts[4]);
    }
}

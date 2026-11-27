//! The eye adjusting to the dark, for ray traced Ultra.
//!
//! The other tiers add a little made-up light from everywhere; the ray tracer
//! only has what really gets in, and it counts leaves as solid, so a forest
//! floor or a deep room comes out near black. By day the eye opens up for
//! dark places, a few stops at most and slowly, as eyes do; at night it
//! barely does, so night stays night.

use bevy::curve::cubic_splines::LinearSpline;
use bevy::post_process::auto_exposure::{AutoExposure, AutoExposureCompensationCurve};
use bevy::prelude::*;

use super::{Clock, Raytraced};
use crate::player::MainCamera;
use crate::noise::smooth;

/// The curve the eye follows, and the daylight it was made for.
#[derive(Resource)]
pub struct Adapt {
    curve: Handle<AutoExposureCompensationCurve>,
    day: f32,
}

/// How far the eye opens for a scene whose average brightness, as drawn,
/// is `x` (log2): not at all for a scene bright enough to read, up to `most`
/// stops for a very dark one.
pub fn lift(x: f32, day: f32) -> f32 {
    let most = 0.8 + 3.7 * day;
    most * smooth(-4.5, -9.5, x)
}

fn curve(day: f32) -> AutoExposureCompensationCurve {
    // the eye's target is the curve's value less the scene's brightness, so
    // a curve of x itself leaves the scene alone
    let points: Vec<Vec2> = (0..=18).map(|i| {
        let x = -16.0 + i as f32;
        Vec2::new(x, x + lift(x, day))
    }).collect();
    AutoExposureCompensationCurve::from_curve(LinearSpline::new(points)).expect("the curve rises")
}

/// Gives a ray traced camera an eye that adjusts.
pub fn setup(rt: Res<Raytraced>, cam: Query<Entity, With<MainCamera>>, clock: Res<Clock>, mut curves: ResMut<Assets<AutoExposureCompensationCurve>>, mut commands: Commands) {
    if !rt.0 {
        return;
    }
    let day = clock.daylight();
    let handle = curves.add(curve(day));
    for e in &cam {
        commands.entity(e).insert(AutoExposure {
            // what the eye measures, in log2 of the drawn brightness
            range: -16.0..=2.0,
            // skip the sky and the darkest corners
            filter: 0.1..=0.8,
            speed_brighten: 0.8,
            speed_darken: 1.6,
            compensation_curve: handle.clone(),
            ..default()
        });
    }
    commands.insert_resource(Adapt { curve: handle, day });
}

/// Remakes the curve as the day turns.
pub fn follow_day(clock: Res<Clock>, adapt: Option<ResMut<Adapt>>, mut curves: ResMut<Assets<AutoExposureCompensationCurve>>) {
    let Some(mut adapt) = adapt else { return };
    let day = clock.daylight();
    if (day - adapt.day).abs() > 0.04 {
        adapt.day = day;
        if let Some(mut c) = curves.get_mut(&adapt.curve) {
            *c = curve(day);
        }
    }
}

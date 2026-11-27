//! Pictures of creations for their cards on the Creations sheet. While the
//! sheet is open, a small camera looks one of a creation's creatures in this
//! world in the face for a few frames; the picture is read back, kept only if
//! there's something in it (not a blank sky while the game's still warming
//! up), and saved as a PNG in the data folder (`pictures/`), so it's there
//! next time even when the creation isn't brought in. One at a time, the
//! chosen card's first, then the nearest.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::{AtmosphereSettings, DistanceFog};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};

use crate::blocks;
use crate::creatures::Herd;
use crate::creations::Library;
use crate::player::{MainCamera, Me};
use crate::ui::{Focus, Screen};
use crate::world::Voxels;

/// How big a picture is: about the size the detail beside the cards shows
/// it on a 1080p screen, twice as wide as tall like the cards.
pub const SIZE: UVec2 = UVec2::new(500, 250);
/// Frames a picture's camera draws before it's read back: TAA and the sky settle.
const FRAMES: u32 = 6;
/// Frames to wait for a picture to come back before trying again.
const PATIENCE: u32 = 240;
/// Tries at one creation's picture in a run of the game.
const TRIES: u32 = 4;

#[derive(Resource, Default)]
pub struct Photos {
    list: HashMap<String, Handle<Image>>,
    /// Goes up when a picture arrives, so the menu draws again.
    pub version: u64,
    /// Taken this time the game's run (the rest are from the data folder).
    fresh: HashSet<String>,
    /// The one being taken: the creation, its camera and frames so far.
    taking: Option<(String, Entity, u32)>,
    /// Pictures read back, waiting to be looked at: (creation, RGBA, width, height).
    back: Vec<(String, Vec<u8>, u32, u32)>,
    /// Tries so far, and the time to try again.
    tries: HashMap<String, (u32, f32)>,
    /// What the camera taking a picture draws into.
    target: Option<Handle<Image>>,
    loaded: bool,
}

impl Photos {
    pub fn get(&self, id: &str) -> Option<Handle<Image>> {
        self.list.get(id).cloned()
    }
}

fn folder() -> PathBuf {
    crate::settings::data_dir().join("pictures")
}

/// A file name safe for any creation's id.
fn file(id: &str) -> PathBuf {
    let safe: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    folder().join(format!("{safe}.png"))
}

fn load(path: &std::path::Path) -> Option<Image> {
    let bytes = std::fs::read(path).ok()?;
    let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    Some(Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, img.into_raw(), TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD))
}

/// Where to stand to look a creature in the face: in front, a little above,
/// with nothing solid at the eye or on the way to it.
fn viewpoint(voxels: &Voxels, pos: Vec3, half: Vec3, yaw: f32) -> Option<(Vec3, Vec3)> {
    let target = pos + Vec3::Y * half.y * 0.9;
    let reach = 2.4 + half.x.max(half.z) * 2.4;
    let solid = |p: Vec3| blocks::is_solid(voxels.get(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32));
    // its face first, then round it
    for turn in [0.0f32, 0.5, -0.5, 1.0, -1.0, 1.6, -1.6, 3.14] {
        let fwd = Quat::from_rotation_y(yaw + turn) * Vec3::NEG_Z;
        let eye = target + fwd * reach + Vec3::Y * (0.6 + half.y * 0.5);
        let clear = (1..=10).all(|n| !solid(target.lerp(eye, n as f32 / 10.0)));
        if clear {
            return Some((eye, target));
        }
    }
    None
}

/// Whether a picture has something in it: enough difference between its
/// pixels that it isn't a flat sky or an empty frame.
pub fn worth_keeping(rgba: &[u8]) -> bool {
    let lum: Vec<f32> = rgba.chunks_exact(4).step_by(7).map(|p| 0.3 * p[0] as f32 + 0.59 * p[1] as f32 + 0.11 * p[2] as f32).collect();
    if lum.is_empty() {
        return false;
    }
    let mean = lum.iter().sum::<f32>() / lum.len() as f32;
    let var = lum.iter().map(|l| (l - mean) * (l - mean)).sum::<f32>() / lum.len() as f32;
    var.sqrt() > 14.0
}

/// Loads the kept pictures once, then takes new ones while the sheet is open.
#[allow(clippy::too_many_arguments)]
pub fn take(
    mut commands: Commands,
    mut photos: ResMut<Photos>,
    mut images: ResMut<Assets<Image>>,
    focus: Res<Focus>,
    lib: Res<Library>,
    herd: Res<Herd>,
    me: Res<Me>,
    voxels: Res<Voxels>,
    time: Res<Time>,
    main: Query<(&Exposure, &Tonemapping, &DistanceFog, &bevy::render::view::ColorGrading, Option<&AtmosphereSettings>), With<MainCamera>>,
) {
    if !photos.loaded {
        photos.loaded = true;
        for id in lib.list.keys() {
            if let Some(img) = load(&file(id)) {
                let h = images.add(img);
                photos.list.insert(id.clone(), h);
            }
        }
        photos.version += 1;
    }
    // pictures that came back: keep the good ones, try the rest again later
    for (id, rgba, w, h) in std::mem::take(&mut photos.back) {
        if photos.taking.as_ref().is_some_and(|t| t.0 == id) {
            if let Some((_, cam, _)) = photos.taking.take() {
                commands.entity(cam).despawn();
            }
        }
        if !worth_keeping(&rgba) {
            let tries = photos.tries.get(&id).map_or(0, |t| t.0) + 1;
            photos.tries.insert(id.clone(), (tries, time.elapsed_secs() + 3.0));
            continue;
        }
        if let Some(img) = image::RgbaImage::from_raw(w, h, rgba.clone()) {
            let _ = std::fs::create_dir_all(folder());
            if let Err(e) = img.save_with_format(file(&id), image::ImageFormat::Png) {
                warn!("couldn't keep the picture of {id}: {e}");
            }
        }
        let picture = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, rgba, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
        let handle = images.add(picture);
        photos.list.insert(id.clone(), handle);
        photos.fresh.insert(id);
        photos.version += 1;
    }
    if let Some((id, cam, frames)) = photos.taking.clone() {
        if frames == FRAMES {
            // read it back, to look at before it's kept
            let from = id.clone();
            commands.spawn(Screenshot::image(photos.target.clone().unwrap_or_default())).observe(move |shot: On<ScreenshotCaptured>, mut photos: ResMut<Photos>| {
                let Ok(img) = shot.image.clone().try_into_dynamic() else { return };
                let mut rgba = img.to_rgba8();
                // the picture is opaque wherever it's drawn
                for p in rgba.pixels_mut() {
                    p.0[3] = 255;
                }
                let (w, h) = rgba.dimensions();
                photos.back.push((from.clone(), rgba.into_raw(), w, h));
            });
        }
        if frames > PATIENCE {
            // it never came back: try again later
            commands.entity(cam).despawn();
            photos.taking = None;
            let tries = photos.tries.get(&id).map_or(0, |t| t.0) + 1;
            photos.tries.insert(id, (tries, time.elapsed_secs() + 3.0));
        } else {
            photos.taking = Some((id, cam, frames + 1));
        }
        return;
    }
    if !matches!(focus.screen, Screen::Library | Screen::Inside(_)) {
        return;
    }
    // the chosen card's creation first, then the nearest creature of any other
    let chosen = match &focus.screen {
        Screen::Inside(id) => Some(id.clone()),
        _ => focus.items.get(focus.sel).and_then(|i| match &i.kind {
            crate::ui::Kind::Card(crate::ui::Act::Inside(id)) => Some(id.clone()),
            _ => None,
        }),
    };
    let now = time.elapsed_secs();
    let ready = |id: &str| photos.tries.get(id).is_none_or(|(n, at)| *n < TRIES && now >= *at);
    let mut bodies: Vec<_> = herd.bodies.values().filter(|b| lib.running.contains(&b.creation) && !photos.fresh.contains(&b.creation) && ready(&b.creation) && b.rider.is_none()).collect();
    bodies.sort_by(|a, b| {
        let first = |c: &str| chosen.as_deref() != Some(c);
        (first(&a.creation), a.pos.distance_squared(me.pos)).partial_cmp(&(first(&b.creation), b.pos.distance_squared(me.pos))).unwrap_or(std::cmp::Ordering::Equal)
    });
    let Some((b, (eye, target))) = bodies.iter().find_map(|b| viewpoint(&voxels, b.pos, Vec3::new(b.reach(), b.half.y, b.reach()), b.yaw).map(|v| (*b, v))) else { return };
    let Ok((exposure, tonemapping, fog, grading, sky)) = main.single() else { return };
    let mut image = Image::new_target_texture(SIZE.x, SIZE.y, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let h = images.add(image);
    let cam = commands
        .spawn((
            Camera3d::default(),
            Camera { order: -1, ..default() },
            RenderTarget::Image(h.clone().into()),
            Projection::Perspective(PerspectiveProjection { fov: 55f32.to_radians(), near: 0.05, far: 400.0, ..default() }),
            Transform::from_translation(eye).looking_at(target, Vec3::Y),
            *exposure,
            *tonemapping,
            fog.clone(),
            grading.clone(),
        ))
        .id();
    if sky.is_some() {
        commands.entity(cam).insert(AtmosphereSettings::default());
    }
    photos.target = Some(h);
    photos.taking = Some((b.creation.clone(), cam, 0));
}

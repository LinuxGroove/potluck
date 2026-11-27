//! Blockbench models as creatures' bodies (world 0.11).
//!
//! A `.bbmodel` is Blockbench's own project file: JSON with the cubes
//! (`elements`), the groups they sit in (`outliner`, and `groups` in newer
//! files) and the textures as PNGs inside it. A body is boxes too, so each
//! cube becomes a part: its size and centre in blocks (16 pixels to one), its
//! colour the texture's most common colour on its faces, and `head`, `leg` or
//! `glow` when its name or a group it's in says so. Cubes are drawn upright,
//! so a rotation is left out (and said so in the log).

use bevy::math::Vec3;
use std::collections::HashMap;

/// The most parts a body has (`script::parse_kind` keeps to it too).
pub const MAX_PARTS: usize = 64;

/// One cube of a model, as a part of a body.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelPart {
    pub name: String,
    pub size: Vec3,
    pub at: Vec3,
    pub color: [u8; 3],
    pub head: bool,
    pub leg: bool,
    pub glow: bool,
}

/// Blockbench's marker colours, for cubes with no texture on them.
const MARKERS: [[u8; 3]; 8] = [
    [0x89, 0xc6, 0xe8], // light blue
    [0xf2, 0xd1, 0x5b], // yellow
    [0xf0, 0x9a, 0x4a], // orange
    [0xe0, 0x5a, 0x4e], // red
    [0xa8, 0x72, 0xd0], // purple
    [0x4f, 0x78, 0xd8], // blue
    [0x5c, 0xb0, 0x52], // green
    [0xa8, 0xd8, 0x4a], // lime
];

/// A texture, decoded, with how many of its pixels make one of the model's
/// UV units.
struct Texture {
    image: image::RgbaImage,
    per_u: f32,
    per_v: f32,
}

/// Reads a Blockbench model into body parts, scaled by `scale` (1 is 16
/// pixels to a block). Gives the parts and notes for the creation's log
/// (what was left out), or what's wrong, in plain words.
pub fn read(file: &str, text: &str, scale: f32) -> Result<(Vec<ModelPart>, Vec<String>), String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("{file} isn't a Blockbench model: {e}"))?;
    let elements = v.get("elements").and_then(|e| e.as_array()).ok_or_else(|| format!("{file} has no cubes in it (is it a Blockbench .bbmodel?)"))?;
    let scale = scale.clamp(0.1, 4.0) / 16.0;
    let mut notes = vec![];

    // what each cube sits in: group names from the outliner, outermost first
    let mut group_names: HashMap<String, String> = HashMap::new();
    for g in v.get("groups").and_then(|g| g.as_array()).into_iter().flatten() {
        if let (Some(id), Some(name)) = (g.get("uuid").and_then(|u| u.as_str()), g.get("name").and_then(|n| n.as_str())) {
            group_names.insert(id.into(), name.into());
        }
    }
    let mut groups_of: HashMap<String, Vec<String>> = HashMap::new();
    let mut hidden: Vec<String> = vec![];
    fn walk(node: &serde_json::Value, path: &mut Vec<String>, names: &HashMap<String, String>, out: &mut HashMap<String, Vec<String>>, hidden: &mut Vec<String>, seen_hidden: bool) {
        match node {
            serde_json::Value::String(id) => {
                out.insert(id.clone(), path.clone());
                if seen_hidden {
                    hidden.push(id.clone());
                }
            }
            serde_json::Value::Object(o) => {
                let name = o.get("name").and_then(|n| n.as_str()).map(String::from).or_else(|| o.get("uuid").and_then(|u| u.as_str()).and_then(|u| names.get(u).cloned())).unwrap_or_default();
                let off = o.get("visibility").and_then(|v| v.as_bool()) == Some(false);
                path.push(name);
                for c in o.get("children").and_then(|c| c.as_array()).into_iter().flatten() {
                    walk(c, path, names, out, hidden, seen_hidden || off);
                }
                path.pop();
            }
            _ => {}
        }
    }
    for node in v.get("outliner").and_then(|o| o.as_array()).into_iter().flatten() {
        walk(node, &mut vec![], &group_names, &mut groups_of, &mut hidden, false);
    }

    // the textures, decoded once
    let res_w = v.pointer("/resolution/width").and_then(|w| w.as_f64()).unwrap_or(16.0) as f32;
    let res_h = v.pointer("/resolution/height").and_then(|h| h.as_f64()).unwrap_or(16.0) as f32;
    let mut textures: Vec<Option<Texture>> = vec![];
    for t in v.get("textures").and_then(|t| t.as_array()).into_iter().flatten() {
        let image = t.get("source").and_then(|s| s.as_str()).and_then(|s| s.strip_prefix("data:image/png;base64,")).and_then(|b| {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.decode(b.trim()).ok()
        });
        let image = image.and_then(|bytes| image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok()).map(|i| i.to_rgba8());
        if image.is_none() {
            let name = t.get("name").and_then(|n| n.as_str()).unwrap_or("a texture");
            notes.push(format!("{file}: {name} isn't a PNG kept in the model, so its cubes take their marker colours"));
        }
        textures.push(image.map(|image| {
            let uw = t.get("uv_width").and_then(|w| w.as_f64()).map(|w| w as f32).unwrap_or(res_w).max(1.0);
            let uh = t.get("uv_height").and_then(|h| h.as_f64()).map(|h| h as f32).unwrap_or(res_h).max(1.0);
            Texture { per_u: image.width() as f32 / uw, per_v: image.height() as f32 / uh, image }
        }));
    }

    let mut parts = vec![];
    let (mut rotated, mut skipped, mut over) = (0, 0, 0);
    for e in elements {
        let kind = e.get("type").and_then(|t| t.as_str()).unwrap_or("cube");
        if kind != "cube" {
            if kind == "mesh" {
                skipped += 1;
            }
            continue;
        }
        let uuid = e.get("uuid").and_then(|u| u.as_str()).unwrap_or("");
        if e.get("visibility").and_then(|v| v.as_bool()) == Some(false) || hidden.iter().any(|h| h == uuid) {
            continue;
        }
        let (Some(from), Some(to)) = (vec3(e.get("from")), vec3(e.get("to"))) else {
            skipped += 1;
            continue;
        };
        if parts.len() >= MAX_PARTS {
            over += 1;
            continue;
        }
        if vec3(e.get("rotation")).is_some_and(|r| r.abs().max_element() > 0.01) {
            rotated += 1;
        }
        let inflate = e.get("inflate").and_then(|i| i.as_f64()).unwrap_or(0.0) as f32;
        let lo = from.min(to) - Vec3::splat(inflate);
        let hi = from.max(to) + Vec3::splat(inflate);
        let name = e.get("name").and_then(|n| n.as_str()).unwrap_or("cube").to_string();
        let mut said: Vec<String> = groups_of.get(uuid).cloned().unwrap_or_default();
        said.push(name.clone());
        let says = |word: &str| said.iter().any(|s| s.to_lowercase().contains(word));
        let marker = e.get("color").and_then(|c| c.as_u64()).unwrap_or(0) as usize % MARKERS.len();
        parts.push(ModelPart {
            size: ((hi - lo) * scale).max(Vec3::splat(0.02)),
            at: (lo + hi) / 2.0 * scale,
            color: colour_of(e, &textures).unwrap_or(MARKERS[marker]),
            head: says("head"),
            leg: says("leg"),
            glow: says("glow"),
            name,
        });
    }
    if parts.is_empty() {
        return Err(format!("{file} has no cubes in it to make a body from"));
    }
    if rotated > 0 {
        notes.push(format!("{file}: {rotated} turned cubes are drawn upright (bodies are upright boxes)"));
    }
    if skipped > 0 {
        notes.push(format!("{file}: {skipped} meshes are left out (only cubes make a body)"));
    }
    if over > 0 {
        notes.push(format!("{file}: only the first {MAX_PARTS} cubes are used, so {over} are left out"));
    }
    Ok((parts, notes))
}

fn vec3(v: Option<&serde_json::Value>) -> Option<Vec3> {
    let a = v?.as_array()?;
    let n = |i: usize| a.get(i).and_then(|x| x.as_f64()).map(|x| x as f32);
    Some(Vec3::new(n(0)?, n(1)?, n(2)?))
}

/// The most common colour on a cube's faces (to a sixteenth of each channel),
/// as the average of the pixels that have it; none when nothing's painted.
fn colour_of(e: &serde_json::Value, textures: &[Option<Texture>]) -> Option<[u8; 3]> {
    let mut buckets: HashMap<u16, (u32, [u32; 3])> = HashMap::new();
    for (_, face) in e.get("faces")?.as_object()? {
        let Some(t) = face.get("texture").and_then(|t| t.as_u64()).and_then(|t| textures.get(t as usize)).and_then(|t| t.as_ref()) else { continue };
        let Some(uv) = face.get("uv").and_then(|u| u.as_array()) else { continue };
        let n = |i: usize| uv.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
        let (u0, u1) = (n(0).min(n(2)) * t.per_u, n(0).max(n(2)) * t.per_u);
        let (v0, v1) = (n(1).min(n(3)) * t.per_v, n(1).max(n(3)) * t.per_v);
        let (w, h) = (t.image.width() as i64, t.image.height() as i64);
        let (x0, x1) = ((u0.floor() as i64).clamp(0, w), (u1.ceil() as i64).clamp(0, w));
        let (y0, y1) = ((v0.floor() as i64).clamp(0, h), (v1.ceil() as i64).clamp(0, h));
        for y in y0..y1.max(y0 + 1).min(h) {
            for x in x0..x1.max(x0 + 1).min(w) {
                let p = t.image.get_pixel(x as u32, y as u32).0;
                if p[3] < 128 {
                    continue;
                }
                let key = ((p[0] as u16 >> 4) << 8) | ((p[1] as u16 >> 4) << 4) | (p[2] as u16 >> 4);
                let b = buckets.entry(key).or_default();
                b.0 += 1;
                for i in 0..3 {
                    b.1[i] += p[i] as u32;
                }
            }
        }
    }
    let (_, (count, sum)) = buckets.into_iter().max_by_key(|(k, (c, _))| (*c, *k))?;
    Some([(sum[0] / count) as u8, (sum[1] / count) as u8, (sum[2] / count) as u8])
}

//! What the game holds in memory, logged when it quits (so every shot, demo
//! and play-through records it) and, with `--memory[=seconds]` or
//! `POTLUCK_MEMORY=seconds`, as a line every so often while it runs:
//!
//! - the process's own memory, resident now and at its peak (/proc on Linux);
//! - the graphics memory the game asked for, from wgpu's allocator: every
//!   texture and buffer by name, summed, the largest first;
//! - what the graphics driver says it holds for the game, where the kernel
//!   tells (amdgpu, i915 and xe in /proc/self/fdinfo). NVIDIA's driver
//!   doesn't, so `tools/memory-watch.sh` asks nvidia-smi from outside.
//!
//! On integrated graphics (the Steam Deck, amd-halo) the graphics memory is
//! the same RAM, so what a low-end machine needs is roughly the two added.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::render::renderer::RenderDevice;

const MB: f64 = 1024.0 * 1024.0;

/// How many of the largest kinds of graphics allocation the full report lists.
const LISTED: usize = 24;

/// The process's resident memory now and at its peak, in bytes.
#[derive(Clone, Copy, Default, Debug)]
pub struct Process {
    pub resident: u64,
    pub peak: u64,
}

/// Reads /proc/self/status (Linux only).
pub fn process() -> Option<Process> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb = |key: &str| s.lines().find_map(|l| l.strip_prefix(key)?.trim().strip_suffix("kB")?.trim().parse::<u64>().ok()).map(|k| k * 1024);
    Some(Process { resident: kb("VmRSS:")?, peak: kb("VmHWM:").unwrap_or(0) })
}

/// What the graphics driver holds for this process, by memory region
/// (`vram`, `gtt`, `system0` ...), in bytes, from the DRM fdinfo the kernel
/// keeps for each open graphics device. Empty where the driver doesn't say.
pub fn driver() -> Vec<(String, u64)> {
    let mut regions: HashMap<String, u64> = HashMap::new();
    let mut seen = std::collections::HashSet::new();
    let Ok(fds) = std::fs::read_dir("/proc/self/fdinfo") else { return Vec::new() };
    for fd in fds.flatten() {
        let Ok(info) = std::fs::read_to_string(fd.path()) else { continue };
        if !info.contains("drm-driver:") {
            continue;
        }
        // several fds can share one client
        let client = info.lines().find_map(|l| l.strip_prefix("drm-client-id:")).map(|c| c.trim().to_string()).unwrap_or_else(|| fd.file_name().to_string_lossy().into());
        if !seen.insert(client) {
            continue;
        }
        // newer kernels say resident; amdgpu's older keys are drm-memory-*
        let resident = info.contains("drm-resident-");
        for l in info.lines() {
            let Some((key, value)) = l.split_once(':') else { continue };
            let region = if resident { key.strip_prefix("drm-resident-") } else { key.strip_prefix("drm-memory-") };
            let Some(region) = region else { continue };
            let mut parts = value.split_whitespace();
            let Some(n) = parts.next().and_then(|n| n.parse::<u64>().ok()) else { continue };
            let unit = match parts.next() {
                Some("KiB") => 1024,
                Some("MiB") => 1024 * 1024,
                Some("GiB") => 1024 * 1024 * 1024,
                _ => 1,
            };
            *regions.entry(region.to_string()).or_default() += n * unit;
        }
    }
    let mut out: Vec<_> = regions.into_iter().filter(|(_, b)| *b > 0).collect();
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out
}

/// The graphics memory the game asked for, from wgpu's allocator.
#[derive(Default, Debug)]
pub struct Gpu {
    /// Bytes in live textures and buffers.
    pub allocated: u64,
    /// Bytes in the blocks they're cut from, unused parts included.
    pub reserved: u64,
    pub allocations: usize,
    /// The blocks of graphics memory wgpu took from the driver: each one's
    /// size and how much of it is in use, the largest first.
    pub blocks: Vec<(u64, u64)>,
    /// Allocations grouped by name (numbers in names count as one), with
    /// how many and their bytes, the largest first.
    pub by_name: Vec<(String, usize, u64)>,
}

/// Asks wgpu's allocator (Vulkan, DirectX 12 and Metal; `None` elsewhere).
pub fn gpu(device: &RenderDevice) -> Option<Gpu> {
    let r = device.wgpu_device().generate_allocator_report()?;
    let mut groups: HashMap<String, (usize, u64)> = HashMap::new();
    for a in &r.allocations {
        let g = groups.entry(group(&a.name)).or_default();
        g.0 += 1;
        g.1 += a.size;
    }
    let mut by_name: Vec<_> = groups.into_iter().map(|(n, (c, b))| (n, c, b)).collect();
    by_name.sort_by(|a, b| b.2.cmp(&a.2));
    let mut blocks: Vec<(u64, u64)> = r.blocks.iter().map(|b| (b.size, r.allocations[b.allocations.clone()].iter().map(|a| a.size).sum())).collect();
    blocks.sort_by(|a, b| b.0.cmp(&a.0));
    Some(Gpu { allocated: r.total_allocated_bytes, reserved: r.total_reserved_bytes, allocations: r.allocations.len(), blocks, by_name })
}

/// A name with its numbers taken out, so "general vertex slab 12" and
/// "general vertex slab 3" count together.
fn group(name: &str) -> String {
    let mut out = String::new();
    let mut digits = false;
    for c in name.chars() {
        if c.is_ascii_digit() {
            if !digits {
                out.push('#');
            }
            digits = true;
        } else {
            digits = false;
            out.push(c);
        }
    }
    out
}

fn mb(b: u64) -> String {
    format!("{:.0} MB", b as f64 / MB)
}

/// One line: the process, the graphics memory it asked for and what the
/// driver holds.
pub fn summary(device: Option<&RenderDevice>) -> String {
    let mut s = String::from("Memory:");
    if let Some(p) = process() {
        s += &format!(" RAM {} (peak {})", mb(p.resident), mb(p.peak));
    }
    if let Some(g) = device.and_then(gpu) {
        s += &format!(", GPU {} in {} allocations ({} reserved in {} blocks)", mb(g.allocated), g.allocations, mb(g.reserved), g.blocks.len());
    }
    let d = driver();
    if !d.is_empty() {
        s += ", driver";
        for (i, (region, b)) in d.iter().enumerate() {
            s += &format!("{} {region} {}", if i == 0 { "" } else { "," }, mb(*b));
        }
    }
    s
}

/// The summary, then the largest kinds of graphics allocation and what the
/// world and creations hold in RAM.
pub fn report(device: Option<&RenderDevice>, extra: &[(&str, u64)]) {
    info!("{}", summary(device));
    if let Some(g) = device.and_then(gpu) {
        for (name, count, bytes) in g.by_name.iter().take(LISTED) {
            info!("GPU memory: {:>7.1} MB  {count:>5} x {name}", *bytes as f64 / MB);
        }
        let rest: u64 = g.by_name.iter().skip(LISTED).map(|x| x.2).sum();
        if rest > 0 {
            info!("GPU memory: {:>7.1} MB  the other {} kinds", rest as f64 / MB, g.by_name.len() - LISTED);
        }
        // blocks part used are memory the card gives the game and nobody uses
        let blocks: Vec<String> = g.blocks.iter().map(|(size, used)| format!("{} ({:.0}% used)", mb(*size), 100.0 * *used as f64 / (*size).max(1) as f64)).collect();
        info!("GPU memory blocks: {}", blocks.join(", "));
    }
    for (what, bytes) in extra {
        info!("RAM: {:>7.1} MB  {what}", *bytes as f64 / MB);
    }
}

/// How often to log the summary while running, from `--memory[=seconds]`
/// or `POTLUCK_MEMORY=seconds` (30 s if no number is given).
#[derive(Resource)]
struct Every(f32);

pub fn add(app: &mut App, args: &[String]) {
    let asked = args
        .iter()
        .find_map(|a| a.strip_prefix("--memory").map(|v| v.trim_start_matches('=').to_string()))
        .or_else(|| std::env::var("POTLUCK_MEMORY").ok());
    if let Some(v) = asked {
        app.insert_resource(Every(v.parse().unwrap_or(30.0f32).max(1.0))).add_systems(Last, every);
    }
    app.add_systems(Last, at_exit);
}

fn every(time: Res<Time<Real>>, e: Res<Every>, device: Option<Res<RenderDevice>>, mut next: Local<f32>) {
    let t = time.elapsed_secs();
    if t >= *next {
        *next = t + e.0;
        info!("{} at {t:.0} s", summary(device.as_deref()));
    }
}

/// The full report when the game quits.
fn at_exit(
    mut exits: MessageReader<AppExit>,
    device: Option<Res<RenderDevice>>,
    voxels: Option<Res<crate::world::Voxels>>,
    scripts: Option<Res<crate::script::Scripts>>,
    mut done: Local<bool>,
) {
    if exits.read().count() == 0 || *done {
        return;
    }
    *done = true;
    let mut extra = Vec::new();
    if let Some(v) = voxels {
        extra.push(("the world's blocks", v.bytes() as u64));
    }
    if let Some(s) = scripts {
        extra.push(("creations' Luau", s.memory() as u64));
    }
    report(device.as_deref(), &extra);
}

#[cfg(test)]
mod tests {
    #[test]
    fn groups_names_without_numbers() {
        assert_eq!(super::group("general vertex slab 12 (3 bytes)"), "general vertex slab # (# bytes)");
        assert_eq!(super::group("shadow_map"), "shadow_map");
    }

    #[test]
    fn reads_its_own_memory() {
        if cfg!(target_os = "linux") {
            let p = super::process().expect("reads /proc/self/status");
            assert!(p.resident > 0 && p.peak >= p.resident);
        }
    }
}

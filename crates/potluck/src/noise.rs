//! Seeded randomness and value noise, the same on every machine for a seed.

/// A small, fast, seeded random number generator (mulberry32).
#[derive(Clone)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed)
    }

    /// The next number in `0.0..1.0`.
    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f64 / 4_294_967_296.0) as f32
    }

    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + ((self.next() * (hi - lo + 1) as f32) as i32).min(hi - lo)
    }
}

/// A seed from a name, so each texture or feature has its own stream.
pub fn seed_of(name: &str) -> u32 {
    name.bytes().fold(7u32, |a, c| a.wrapping_mul(31).wrapping_add(c as u32))
}

pub fn hash2(x: i32, z: i32, s: i32) -> f32 {
    let mut h = x
        .wrapping_mul(374_761_393)
        .wrapping_add(z.wrapping_mul(668_265_263))
        .wrapping_add(s.wrapping_mul(2_147_483_647));
    h = (h ^ ((h as u32) >> 13) as i32).wrapping_mul(1_274_126_177);
    ((h ^ ((h as u32) >> 16) as i32) as u32) as f32 / 4_294_967_296.0
}

fn fade(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

pub fn vnoise(x: f32, z: f32, s: i32) -> f32 {
    let (xi, zi) = (x.floor(), z.floor());
    let (xf, zf) = (fade(x - xi), fade(z - zi));
    let (xi, zi) = (xi as i32, zi as i32);
    let a = hash2(xi, zi, s);
    let b = hash2(xi + 1, zi, s);
    let c = hash2(xi, zi + 1, s);
    let d = hash2(xi + 1, zi + 1, s);
    a + (b - a) * xf + (c - a) * zf + (a - b - c + d) * xf * zf
}

/// Fractal noise in `0.0..1.0`.
pub fn fbm(x: f32, z: f32, s: i32, octaves: u32) -> f32 {
    let (mut v, mut amp, mut f, mut sum) = (0.0, 0.5, 1.0, 0.0);
    for i in 0..octaves {
        v += amp * vnoise(x * f, z * f, s + i as i32 * 17);
        sum += amp;
        amp *= 0.5;
        f *= 2.03;
    }
    v / sum
}

/// Noise that tiles across a 32 px texture, on a lattice of `cells`.
pub fn tnoise(x: f32, y: f32, cells: i32, s: i32) -> f32 {
    let fx = x / 32.0 * cells as f32;
    let fy = y / 32.0 * cells as f32;
    let (xi, yi) = (fx.floor(), fy.floor());
    let (xf, yf) = (fade(fx - xi), fade(fy - yi));
    let (xi, yi) = (xi as i32, yi as i32);
    let g = |i: i32, j: i32| hash2(i.rem_euclid(cells), j.rem_euclid(cells), s);
    let a = g(xi, yi);
    let b = g(xi + 1, yi);
    let c = g(xi, yi + 1);
    let d = g(xi + 1, yi + 1);
    a + (b - a) * xf + (c - a) * yf + (a - b - c + d) * xf * yf
}

pub fn tfbm(x: f32, y: f32, s: i32) -> f32 {
    0.55 * tnoise(x, y, 4, s) + 0.3 * tnoise(x, y, 8, s + 5) + 0.15 * tnoise(x, y, 16, s + 9)
}

pub fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

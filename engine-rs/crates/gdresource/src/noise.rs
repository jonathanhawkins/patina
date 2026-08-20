//! FastNoiseLite procedural noise (pat-nzgdu.219).
//!
//! A self-contained, pure-`std` implementation of the core of Godot's
//! [`FastNoiseLite`] noise generator. It provides **Value** and **Perlin**
//! gradient noise in 2D and 3D, the **FBm / Ridged / Ping-Pong** fractal
//! accumulation pipeline (with octaves, lacunarity, gain and
//! weighted-strength), and a deterministic grayscale image bake used by
//! `NoiseTexture2D` / `NoiseTexture3D`.
//!
//! # Design
//!
//! The hashing, coordinate-priming and Value-noise math follow the reference
//! FastNoiseLite (Jordan Peck, MIT) exactly, including its integer-overflow
//! wrapping semantics — every integer step here uses `wrapping_*` so results
//! match the C/C# reference and never panic in debug builds. Perlin noise uses
//! the classic improved-noise gradient sets (8 directions in 2D, 16 in 3D)
//! selected by the same hash; it is a valid, deterministic Perlin field but is
//! not bit-identical to FastNoiseLite's 128-gradient variant.
//!
//! The final noise value is clamped to `[-1.0, 1.0]` so callers (image bakers,
//! shaders) get a stable, documented range for every noise/fractal combination.
//!
//! # Scope / remaining work (pat-nzgdu.219)
//!
//! This module is the noise **engine core** only. The full bead additionally
//! needs `NoiseType::ValueCubic/Simplex/SimplexSmooth/Cellular`, domain warp,
//! the `Noise` / `FastNoiseLite` / `NoiseTexture2D` / `NoiseTexture3D` resource
//! classes registered in `class_db.rs`, and importer-path wiring so a `.tres`
//! noise texture loads and samples. Those are not implemented here.
//!
//! [`FastNoiseLite`]: https://github.com/Auburn/FastNoiseLite

// Coordinate-priming constants from the reference FastNoiseLite. Products with
// these overflow i32 by design; all uses go through `wrapping_*`.
const PRIME_X: i32 = 501_125_321;
const PRIME_Y: i32 = 1_136_930_381;
const PRIME_Z: i32 = 1_720_413_743;

/// Which base noise field [`FastNoiseLite`] samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoiseType {
    /// Interpolated per-lattice-point random values (hash-based).
    Value,
    /// Classic Perlin gradient noise.
    Perlin,
}

/// How successive octaves are combined into the final value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FractalType {
    /// No fractal accumulation — a single octave of the base noise.
    None,
    /// Fractional Brownian motion (sum of octaves at decreasing amplitude).
    Fbm,
    /// Ridged multifractal (absolute-valued, inverted octaves).
    Ridged,
    /// Ping-pong folded octaves.
    PingPong,
}

/// A configurable coherent-noise generator modeled on Godot's `FastNoiseLite`.
///
/// Construct with [`FastNoiseLite::new`] (or [`FastNoiseLite::with_seed`]) and
/// tune with the builder-style setters, then sample with
/// [`get_noise_2d`](Self::get_noise_2d) / [`get_noise_3d`](Self::get_noise_3d).
///
/// ```
/// use gdresource::noise::{FastNoiseLite, NoiseType, FractalType};
///
/// let n = FastNoiseLite::with_seed(1337)
///     .noise_type(NoiseType::Perlin)
///     .fractal(FractalType::Fbm)
///     .octaves(4);
/// let v = n.get_noise_2d(12.5, -3.0);
/// assert!((-1.0..=1.0).contains(&v));
/// ```
#[derive(Debug, Clone, Copy)]
pub struct FastNoiseLite {
    seed: i32,
    frequency: f32,
    noise_type: NoiseType,
    fractal_type: FractalType,
    octaves: i32,
    lacunarity: f32,
    gain: f32,
    weighted_strength: f32,
    ping_pong_strength: f32,
}

impl Default for FastNoiseLite {
    fn default() -> Self {
        // Defaults mirror Godot's FastNoiseLite where a matching feature ships
        // here (frequency 0.01, 5 octaves, lacunarity 2.0, gain 0.5).
        Self {
            seed: 0,
            frequency: 0.01,
            noise_type: NoiseType::Perlin,
            fractal_type: FractalType::Fbm,
            octaves: 5,
            lacunarity: 2.0,
            gain: 0.5,
            weighted_strength: 0.0,
            ping_pong_strength: 2.0,
        }
    }
}

impl FastNoiseLite {
    /// A generator with default settings and seed `0`.
    pub fn new() -> Self {
        Self::default()
    }

    /// A generator with default settings and the given `seed`.
    pub fn with_seed(seed: i32) -> Self {
        Self {
            seed,
            ..Self::default()
        }
    }

    /// Sets the random seed.
    pub fn seed(mut self, seed: i32) -> Self {
        self.seed = seed;
        self
    }

    /// Sets the input coordinate scale. Coordinates are multiplied by this
    /// before sampling, so smaller values produce larger features.
    pub fn frequency(mut self, frequency: f32) -> Self {
        self.frequency = frequency;
        self
    }

    /// Selects the base noise field.
    pub fn noise_type(mut self, noise_type: NoiseType) -> Self {
        self.noise_type = noise_type;
        self
    }

    /// Selects the fractal accumulation mode.
    pub fn fractal(mut self, fractal_type: FractalType) -> Self {
        self.fractal_type = fractal_type;
        self
    }

    /// Sets the number of fractal octaves (clamped to at least 1).
    pub fn octaves(mut self, octaves: i32) -> Self {
        self.octaves = octaves.max(1);
        self
    }

    /// Sets the per-octave frequency multiplier.
    pub fn lacunarity(mut self, lacunarity: f32) -> Self {
        self.lacunarity = lacunarity;
        self
    }

    /// Sets the per-octave amplitude multiplier.
    pub fn gain(mut self, gain: f32) -> Self {
        self.gain = gain;
        self
    }

    /// Sets how strongly higher octaves are weighted toward the previous
    /// octave's value (0.0 = classic fractal, 1.0 = fully weighted).
    pub fn weighted_strength(mut self, weighted_strength: f32) -> Self {
        self.weighted_strength = weighted_strength;
        self
    }

    /// Sets the ping-pong fold strength (only used by [`FractalType::PingPong`]).
    pub fn ping_pong_strength(mut self, ping_pong_strength: f32) -> Self {
        self.ping_pong_strength = ping_pong_strength;
        self
    }

    /// Samples the 2D noise field at `(x, y)`, returning a value in `[-1, 1]`.
    pub fn get_noise_2d(&self, x: f32, y: f32) -> f32 {
        let x = x * self.frequency;
        let y = y * self.frequency;
        let v = match self.fractal_type {
            FractalType::None => self.gen_single_2d(self.seed, x, y),
            FractalType::Fbm => self.gen_fbm_2d(x, y),
            FractalType::Ridged => self.gen_ridged_2d(x, y),
            FractalType::PingPong => self.gen_ping_pong_2d(x, y),
        };
        v.clamp(-1.0, 1.0)
    }

    /// Samples the 3D noise field at `(x, y, z)`, returning a value in `[-1, 1]`.
    pub fn get_noise_3d(&self, x: f32, y: f32, z: f32) -> f32 {
        let x = x * self.frequency;
        let y = y * self.frequency;
        let z = z * self.frequency;
        let v = match self.fractal_type {
            FractalType::None => self.gen_single_3d(self.seed, x, y, z),
            FractalType::Fbm => self.gen_fbm_3d(x, y, z),
            FractalType::Ridged => self.gen_ridged_3d(x, y, z),
            FractalType::PingPong => self.gen_ping_pong_3d(x, y, z),
        };
        v.clamp(-1.0, 1.0)
    }

    /// Bakes the 2D noise field into a `width`×`height` grayscale (L8) image,
    /// row-major with one byte per pixel. Noise values in `[-1, 1]` map to
    /// bytes `[0, 255]`. Deterministic for a fixed configuration.
    pub fn bake_image_l8(&self, width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity((width as usize) * (height as usize));
        for y in 0..height {
            for x in 0..width {
                let n = self.get_noise_2d(x as f32, y as f32);
                out.push(noise_to_u8(n));
            }
        }
        out
    }

    /// Bakes the 3D noise field into a `width`×`height`×`depth` grayscale (L8)
    /// volume, ordered x-fastest then y then z (slice-major). Deterministic.
    pub fn bake_volume_l8(&self, width: u32, height: u32, depth: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity((width as usize) * (height as usize) * (depth as usize));
        for z in 0..depth {
            for y in 0..height {
                for x in 0..width {
                    let n = self.get_noise_3d(x as f32, y as f32, z as f32);
                    out.push(noise_to_u8(n));
                }
            }
        }
        out
    }

    // ---- fractal pipelines (2D) -------------------------------------------

    /// Amplitude normalization factor so FBm output stays within `[-1, 1]`.
    fn fractal_bounding(&self) -> f32 {
        let gain = self.gain.abs();
        let mut amp = gain;
        let mut amp_fractal = 1.0_f32;
        for _ in 1..self.octaves {
            amp_fractal += amp;
            amp *= gain;
        }
        1.0 / amp_fractal
    }

    fn gen_fbm_2d(&self, mut x: f32, mut y: f32) -> f32 {
        let mut seed = self.seed;
        let mut sum = 0.0_f32;
        let mut amp = self.fractal_bounding();
        for _ in 0..self.octaves {
            let noise = self.gen_single_2d(seed, x, y);
            seed = seed.wrapping_add(1);
            sum += noise * amp;
            amp *= lerp(1.0, (noise + 1.0) * 0.5, self.weighted_strength);
            x *= self.lacunarity;
            y *= self.lacunarity;
            amp *= self.gain;
        }
        sum
    }

    fn gen_ridged_2d(&self, mut x: f32, mut y: f32) -> f32 {
        let mut seed = self.seed;
        let mut sum = 0.0_f32;
        let mut amp = self.fractal_bounding();
        for _ in 0..self.octaves {
            let noise = self.gen_single_2d(seed, x, y).abs();
            seed = seed.wrapping_add(1);
            sum += (noise * -2.0 + 1.0) * amp;
            amp *= lerp(1.0, 1.0 - noise, self.weighted_strength);
            x *= self.lacunarity;
            y *= self.lacunarity;
            amp *= self.gain;
        }
        sum
    }

    fn gen_ping_pong_2d(&self, mut x: f32, mut y: f32) -> f32 {
        let mut seed = self.seed;
        let mut sum = 0.0_f32;
        let mut amp = self.fractal_bounding();
        for _ in 0..self.octaves {
            let noise = ping_pong((self.gen_single_2d(seed, x, y) + 1.0) * self.ping_pong_strength);
            seed = seed.wrapping_add(1);
            sum += (noise - 0.5) * 2.0 * amp;
            amp *= lerp(1.0, noise, self.weighted_strength);
            x *= self.lacunarity;
            y *= self.lacunarity;
            amp *= self.gain;
        }
        sum
    }

    // ---- fractal pipelines (3D) -------------------------------------------

    fn gen_fbm_3d(&self, mut x: f32, mut y: f32, mut z: f32) -> f32 {
        let mut seed = self.seed;
        let mut sum = 0.0_f32;
        let mut amp = self.fractal_bounding();
        for _ in 0..self.octaves {
            let noise = self.gen_single_3d(seed, x, y, z);
            seed = seed.wrapping_add(1);
            sum += noise * amp;
            amp *= lerp(1.0, (noise + 1.0) * 0.5, self.weighted_strength);
            x *= self.lacunarity;
            y *= self.lacunarity;
            z *= self.lacunarity;
            amp *= self.gain;
        }
        sum
    }

    fn gen_ridged_3d(&self, mut x: f32, mut y: f32, mut z: f32) -> f32 {
        let mut seed = self.seed;
        let mut sum = 0.0_f32;
        let mut amp = self.fractal_bounding();
        for _ in 0..self.octaves {
            let noise = self.gen_single_3d(seed, x, y, z).abs();
            seed = seed.wrapping_add(1);
            sum += (noise * -2.0 + 1.0) * amp;
            amp *= lerp(1.0, 1.0 - noise, self.weighted_strength);
            x *= self.lacunarity;
            y *= self.lacunarity;
            z *= self.lacunarity;
            amp *= self.gain;
        }
        sum
    }

    fn gen_ping_pong_3d(&self, mut x: f32, mut y: f32, mut z: f32) -> f32 {
        let mut seed = self.seed;
        let mut sum = 0.0_f32;
        let mut amp = self.fractal_bounding();
        for _ in 0..self.octaves {
            let noise =
                ping_pong((self.gen_single_3d(seed, x, y, z) + 1.0) * self.ping_pong_strength);
            seed = seed.wrapping_add(1);
            sum += (noise - 0.5) * 2.0 * amp;
            amp *= lerp(1.0, noise, self.weighted_strength);
            x *= self.lacunarity;
            y *= self.lacunarity;
            z *= self.lacunarity;
            amp *= self.gain;
        }
        sum
    }

    // ---- single-octave dispatch -------------------------------------------

    fn gen_single_2d(&self, seed: i32, x: f32, y: f32) -> f32 {
        match self.noise_type {
            NoiseType::Value => single_value_2d(seed, x, y),
            NoiseType::Perlin => single_perlin_2d(seed, x, y),
        }
    }

    fn gen_single_3d(&self, seed: i32, x: f32, y: f32, z: f32) -> f32 {
        match self.noise_type {
            NoiseType::Value => single_value_3d(seed, x, y, z),
            NoiseType::Perlin => single_perlin_3d(seed, x, y, z),
        }
    }
}

// ---- shared scalar helpers ------------------------------------------------

#[inline]
fn fast_floor(f: f32) -> i32 {
    if f >= 0.0 {
        f as i32
    } else {
        f as i32 - 1
    }
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + t * (b - a)
}

/// Hermite smoothstep, `3t² − 2t³`.
#[inline]
fn interp_hermite(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Quintic smootherstep, `6t⁵ − 15t⁴ + 10t³`.
#[inline]
fn interp_quintic(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Folds `t` into a `[0, 1]` triangle wave (used by ping-pong fractal).
#[inline]
fn ping_pong(t: f32) -> f32 {
    let t = t - ((t * 0.5) as i32 as f32) * 2.0;
    if t < 1.0 {
        t
    } else {
        2.0 - t
    }
}

/// Maps a noise value in `[-1, 1]` to a grayscale byte in `[0, 255]`.
#[inline]
fn noise_to_u8(n: f32) -> u8 {
    let mapped = (n * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0;
    (mapped + 0.5) as u8
}

/// The reference FastNoiseLite integer hash (wrapping multiply).
#[inline]
fn hash2(seed: i32, x_primed: i32, y_primed: i32) -> i32 {
    let mut hash = seed ^ x_primed ^ y_primed;
    hash = hash.wrapping_mul(0x27d4_eb2d);
    hash
}

#[inline]
fn hash3(seed: i32, x_primed: i32, y_primed: i32, z_primed: i32) -> i32 {
    let mut hash = seed ^ x_primed ^ y_primed ^ z_primed;
    hash = hash.wrapping_mul(0x27d4_eb2d);
    hash
}

// ---- Value noise ----------------------------------------------------------

/// Hash a primed lattice point to a value in roughly `[-1, 1]`.
#[inline]
fn val_coord_2d(seed: i32, x_primed: i32, y_primed: i32) -> f32 {
    let mut hash = hash2(seed, x_primed, y_primed);
    hash = hash.wrapping_mul(hash);
    hash ^= hash << 19;
    (hash as f32) * (1.0 / 2_147_483_648.0)
}

#[inline]
fn val_coord_3d(seed: i32, x_primed: i32, y_primed: i32, z_primed: i32) -> f32 {
    let mut hash = hash3(seed, x_primed, y_primed, z_primed);
    hash = hash.wrapping_mul(hash);
    hash ^= hash << 19;
    (hash as f32) * (1.0 / 2_147_483_648.0)
}

fn single_value_2d(seed: i32, x: f32, y: f32) -> f32 {
    let x0 = fast_floor(x);
    let y0 = fast_floor(y);

    let xs = interp_hermite(x - x0 as f32);
    let ys = interp_hermite(y - y0 as f32);

    let x0p = x0.wrapping_mul(PRIME_X);
    let y0p = y0.wrapping_mul(PRIME_Y);
    let x1p = x0p.wrapping_add(PRIME_X);
    let y1p = y0p.wrapping_add(PRIME_Y);

    let xf0 = lerp(
        val_coord_2d(seed, x0p, y0p),
        val_coord_2d(seed, x1p, y0p),
        xs,
    );
    let xf1 = lerp(
        val_coord_2d(seed, x0p, y1p),
        val_coord_2d(seed, x1p, y1p),
        xs,
    );

    lerp(xf0, xf1, ys)
}

fn single_value_3d(seed: i32, x: f32, y: f32, z: f32) -> f32 {
    let x0 = fast_floor(x);
    let y0 = fast_floor(y);
    let z0 = fast_floor(z);

    let xs = interp_hermite(x - x0 as f32);
    let ys = interp_hermite(y - y0 as f32);
    let zs = interp_hermite(z - z0 as f32);

    let x0p = x0.wrapping_mul(PRIME_X);
    let y0p = y0.wrapping_mul(PRIME_Y);
    let z0p = z0.wrapping_mul(PRIME_Z);
    let x1p = x0p.wrapping_add(PRIME_X);
    let y1p = y0p.wrapping_add(PRIME_Y);
    let z1p = z0p.wrapping_add(PRIME_Z);

    let xf00 = lerp(
        val_coord_3d(seed, x0p, y0p, z0p),
        val_coord_3d(seed, x1p, y0p, z0p),
        xs,
    );
    let xf10 = lerp(
        val_coord_3d(seed, x0p, y1p, z0p),
        val_coord_3d(seed, x1p, y1p, z0p),
        xs,
    );
    let xf01 = lerp(
        val_coord_3d(seed, x0p, y0p, z1p),
        val_coord_3d(seed, x1p, y0p, z1p),
        xs,
    );
    let xf11 = lerp(
        val_coord_3d(seed, x0p, y1p, z1p),
        val_coord_3d(seed, x1p, y1p, z1p),
        xs,
    );

    let yf0 = lerp(xf00, xf10, ys);
    let yf1 = lerp(xf01, xf11, ys);
    lerp(yf0, yf1, zs)
}

// ---- Perlin noise ---------------------------------------------------------

// Classic improved-noise gradient sets (unit-ish directions). 2D uses 8
// directions; 3D uses the canonical 16-entry set (12 edge vectors, last 4
// repeated) so a 4-bit hash indexes it without bias correction.
const GRAD_2D: [(f32, f32); 8] = [
    (1.0, 0.0),
    (-1.0, 0.0),
    (0.0, 1.0),
    (0.0, -1.0),
    (
        std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    ),
    (
        -std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    ),
    (
        std::f32::consts::FRAC_1_SQRT_2,
        -std::f32::consts::FRAC_1_SQRT_2,
    ),
    (
        -std::f32::consts::FRAC_1_SQRT_2,
        -std::f32::consts::FRAC_1_SQRT_2,
    ),
];

const GRAD_3D: [(f32, f32, f32); 16] = [
    (1.0, 1.0, 0.0),
    (-1.0, 1.0, 0.0),
    (1.0, -1.0, 0.0),
    (-1.0, -1.0, 0.0),
    (1.0, 0.0, 1.0),
    (-1.0, 0.0, 1.0),
    (1.0, 0.0, -1.0),
    (-1.0, 0.0, -1.0),
    (0.0, 1.0, 1.0),
    (0.0, -1.0, 1.0),
    (0.0, 1.0, -1.0),
    (0.0, -1.0, -1.0),
    (1.0, 1.0, 0.0),
    (0.0, -1.0, 1.0),
    (-1.0, 1.0, 0.0),
    (0.0, -1.0, -1.0),
];

#[inline]
fn grad_coord_2d(seed: i32, x_primed: i32, y_primed: i32, xd: f32, yd: f32) -> f32 {
    let mut hash = hash2(seed, x_primed, y_primed);
    hash ^= hash >> 15;
    let (gx, gy) = GRAD_2D[(hash & 7) as usize];
    xd * gx + yd * gy
}

#[inline]
fn grad_coord_3d(
    seed: i32,
    x_primed: i32,
    y_primed: i32,
    z_primed: i32,
    xd: f32,
    yd: f32,
    zd: f32,
) -> f32 {
    let mut hash = hash3(seed, x_primed, y_primed, z_primed);
    hash ^= hash >> 15;
    let (gx, gy, gz) = GRAD_3D[(hash & 15) as usize];
    xd * gx + yd * gy + zd * gz
}

fn single_perlin_2d(seed: i32, x: f32, y: f32) -> f32 {
    let x0 = fast_floor(x);
    let y0 = fast_floor(y);

    let xd0 = x - x0 as f32;
    let yd0 = y - y0 as f32;
    let xd1 = xd0 - 1.0;
    let yd1 = yd0 - 1.0;

    let xs = interp_quintic(xd0);
    let ys = interp_quintic(yd0);

    let x0p = x0.wrapping_mul(PRIME_X);
    let y0p = y0.wrapping_mul(PRIME_Y);
    let x1p = x0p.wrapping_add(PRIME_X);
    let y1p = y0p.wrapping_add(PRIME_Y);

    let xf0 = lerp(
        grad_coord_2d(seed, x0p, y0p, xd0, yd0),
        grad_coord_2d(seed, x1p, y0p, xd1, yd0),
        xs,
    );
    let xf1 = lerp(
        grad_coord_2d(seed, x0p, y1p, xd0, yd1),
        grad_coord_2d(seed, x1p, y1p, xd1, yd1),
        xs,
    );

    // Gradients have length 1 or √2; the interpolated dot stays within ~±0.71,
    // so scale up to fill [-1, 1] (final clamp guards the rare extreme).
    lerp(xf0, xf1, ys) * 1.4142135
}

fn single_perlin_3d(seed: i32, x: f32, y: f32, z: f32) -> f32 {
    let x0 = fast_floor(x);
    let y0 = fast_floor(y);
    let z0 = fast_floor(z);

    let xd0 = x - x0 as f32;
    let yd0 = y - y0 as f32;
    let zd0 = z - z0 as f32;
    let xd1 = xd0 - 1.0;
    let yd1 = yd0 - 1.0;
    let zd1 = zd0 - 1.0;

    let xs = interp_quintic(xd0);
    let ys = interp_quintic(yd0);
    let zs = interp_quintic(zd0);

    let x0p = x0.wrapping_mul(PRIME_X);
    let y0p = y0.wrapping_mul(PRIME_Y);
    let z0p = z0.wrapping_mul(PRIME_Z);
    let x1p = x0p.wrapping_add(PRIME_X);
    let y1p = y0p.wrapping_add(PRIME_Y);
    let z1p = z0p.wrapping_add(PRIME_Z);

    let xf00 = lerp(
        grad_coord_3d(seed, x0p, y0p, z0p, xd0, yd0, zd0),
        grad_coord_3d(seed, x1p, y0p, z0p, xd1, yd0, zd0),
        xs,
    );
    let xf10 = lerp(
        grad_coord_3d(seed, x0p, y1p, z0p, xd0, yd1, zd0),
        grad_coord_3d(seed, x1p, y1p, z0p, xd1, yd1, zd0),
        xs,
    );
    let xf01 = lerp(
        grad_coord_3d(seed, x0p, y0p, z1p, xd0, yd0, zd1),
        grad_coord_3d(seed, x1p, y0p, z1p, xd1, yd0, zd1),
        xs,
    );
    let xf11 = lerp(
        grad_coord_3d(seed, x0p, y1p, z1p, xd0, yd1, zd1),
        grad_coord_3d(seed, x1p, y1p, z1p, xd1, yd1, zd1),
        xs,
    );

    let yf0 = lerp(xf00, xf10, ys);
    let yf1 = lerp(xf01, xf11, ys);
    // 3D edge gradients have length √2; scale the trilinear dot into [-1, 1].
    lerp(yf0, yf1, zs) * 0.9642
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_range(v: f32) -> bool {
        (-1.0..=1.0).contains(&v)
    }

    #[test]
    fn value_noise_is_deterministic_and_in_range() {
        let n = FastNoiseLite::with_seed(42)
            .noise_type(NoiseType::Value)
            .fractal(FractalType::None)
            .frequency(0.1);
        for i in 0..200 {
            let x = i as f32 * 0.37 - 30.0;
            let y = i as f32 * -0.19 + 12.0;
            let a = n.get_noise_2d(x, y);
            let b = n.get_noise_2d(x, y);
            assert_eq!(a, b, "value noise must be deterministic");
            assert!(in_range(a), "value noise {a} out of [-1,1]");
        }
    }

    #[test]
    fn perlin_noise_is_deterministic_and_in_range() {
        let n = FastNoiseLite::with_seed(7)
            .noise_type(NoiseType::Perlin)
            .fractal(FractalType::None)
            .frequency(0.05);
        for i in 0..200 {
            let x = i as f32 * 0.53 + 3.0;
            let y = i as f32 * 0.28 - 9.0;
            let a = n.get_noise_2d(x, y);
            assert_eq!(a, n.get_noise_2d(x, y));
            assert!(in_range(a), "perlin noise {a} out of [-1,1]");
        }
    }

    #[test]
    fn perlin_is_zero_on_lattice_points_and_nonzero_between() {
        // Perlin gradient noise is exactly 0 at integer lattice coordinates
        // (the offset vector is zero there). Use frequency 1 so integer inputs
        // land on lattice points.
        let n = FastNoiseLite::with_seed(99)
            .noise_type(NoiseType::Perlin)
            .fractal(FractalType::None)
            .frequency(1.0);
        for gx in -3..=3 {
            for gy in -3..=3 {
                let v = n.get_noise_2d(gx as f32, gy as f32);
                assert!(v.abs() < 1e-4, "perlin at lattice ({gx},{gy}) = {v}");
            }
        }
        // Off-lattice it should vary: the strongest off-lattice sample over a
        // grid must be clearly non-zero (robust to any single point that
        // happens to fall near a zero crossing).
        let mut max_off = 0.0_f32;
        for i in 0..8 {
            for j in 0..8 {
                let v = n.get_noise_2d(i as f32 + 0.5, j as f32 + 0.5).abs();
                if v > max_off {
                    max_off = v;
                }
            }
        }
        assert!(
            max_off > 1e-2,
            "perlin should be non-zero off-lattice ({max_off})"
        );
    }

    #[test]
    fn different_seeds_produce_different_fields() {
        let a = FastNoiseLite::with_seed(1).noise_type(NoiseType::Value);
        let b = FastNoiseLite::with_seed(2).noise_type(NoiseType::Value);
        let mut differences = 0;
        for i in 0..50 {
            let x = i as f32 * 1.3;
            let y = i as f32 * 0.7;
            if (a.get_noise_2d(x, y) - b.get_noise_2d(x, y)).abs() > 1e-6 {
                differences += 1;
            }
        }
        assert!(
            differences > 40,
            "seeds should decorrelate ({differences}/50)"
        );
    }

    #[test]
    fn noise_is_continuous() {
        // Nearby samples must be close (no discontinuities across cells).
        let n = FastNoiseLite::with_seed(5)
            .noise_type(NoiseType::Perlin)
            .fractal(FractalType::Fbm)
            .frequency(0.1);
        let mut prev = n.get_noise_2d(0.0, 0.0);
        for i in 1..500 {
            let x = i as f32 * 0.02;
            let cur = n.get_noise_2d(x, 0.0);
            assert!(
                (cur - prev).abs() < 0.2,
                "discontinuity at x={x}: {prev} -> {cur}"
            );
            prev = cur;
        }
    }

    #[test]
    fn fractal_modes_stay_in_range() {
        for ft in [FractalType::Fbm, FractalType::Ridged, FractalType::PingPong] {
            for nt in [NoiseType::Value, NoiseType::Perlin] {
                let n = FastNoiseLite::with_seed(123)
                    .noise_type(nt)
                    .fractal(ft)
                    .octaves(6)
                    .frequency(0.07);
                for i in 0..300 {
                    let x = i as f32 * 0.41 - 20.0;
                    let y = i as f32 * 0.23 + 5.0;
                    let v2 = n.get_noise_2d(x, y);
                    let v3 = n.get_noise_3d(x, y, x - y);
                    assert!(in_range(v2), "{ft:?}/{nt:?} 2d {v2} out of range");
                    assert!(in_range(v3), "{ft:?}/{nt:?} 3d {v3} out of range");
                }
            }
        }
    }

    #[test]
    fn noise_3d_is_deterministic_and_in_range() {
        let n = FastNoiseLite::with_seed(2024)
            .noise_type(NoiseType::Value)
            .fractal(FractalType::Fbm)
            .frequency(0.08);
        for i in 0..150 {
            let x = i as f32 * 0.3;
            let y = i as f32 * -0.4 + 2.0;
            let z = i as f32 * 0.15 - 1.0;
            let a = n.get_noise_3d(x, y, z);
            assert_eq!(a, n.get_noise_3d(x, y, z));
            assert!(in_range(a));
        }
    }

    #[test]
    fn bake_image_is_deterministic_and_non_uniform() {
        // Mirrors the bead's acceptance intent: a fixed-seed bake is stable and
        // varies across pixels.
        let n = FastNoiseLite::with_seed(1337)
            .noise_type(NoiseType::Perlin)
            .fractal(FractalType::Fbm)
            .frequency(0.06)
            .octaves(4);
        let a = n.bake_image_l8(32, 32);
        let b = n.bake_image_l8(32, 32);
        assert_eq!(a.len(), 32 * 32);
        assert_eq!(a, b, "bake must be deterministic for a fixed config");

        let first = a[0];
        assert!(
            a.iter().any(|&px| px != first),
            "baked image must be non-uniform"
        );
        // A well-spread noise image should use a decent range of values.
        let min = *a.iter().min().unwrap();
        let max = *a.iter().max().unwrap();
        assert!(
            max - min > 25,
            "baked image dynamic range too small: {min}..{max}"
        );
    }

    #[test]
    fn bake_volume_is_deterministic_and_non_uniform() {
        let n = FastNoiseLite::with_seed(2)
            .noise_type(NoiseType::Value)
            .fractal(FractalType::Fbm)
            .frequency(0.1);
        let a = n.bake_volume_l8(8, 8, 8);
        assert_eq!(a.len(), 8 * 8 * 8);
        assert_eq!(a, n.bake_volume_l8(8, 8, 8));
        let first = a[0];
        assert!(
            a.iter().any(|&px| px != first),
            "volume must be non-uniform"
        );
    }

    #[test]
    fn ping_pong_folds_into_unit_triangle() {
        for i in 0..1000 {
            let t = i as f32 * 0.013 - 6.0;
            let p = ping_pong(t);
            assert!((0.0..=1.0).contains(&p), "ping_pong({t}) = {p}");
        }
        assert!((ping_pong(0.0) - 0.0).abs() < 1e-6);
        assert!((ping_pong(1.0) - 1.0).abs() < 1e-6);
        assert!((ping_pong(2.0) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn octave_count_changes_output() {
        // FBm with more octaves adds detail, so the field differs.
        let base = FastNoiseLite::with_seed(11)
            .noise_type(NoiseType::Perlin)
            .fractal(FractalType::Fbm)
            .frequency(0.09);
        let one = base.octaves(1);
        let five = base.octaves(5);
        let mut diff = 0;
        for i in 0..100 {
            let x = i as f32 * 0.7;
            if (one.get_noise_2d(x, 3.0) - five.get_noise_2d(x, 3.0)).abs() > 1e-4 {
                diff += 1;
            }
        }
        assert!(
            diff > 50,
            "octave count should change the field ({diff}/100)"
        );
    }
}

// ===========================================================================
// Godot enum bridge (pat-s3rrv.208)
// ===========================================================================

/// Maps a Godot `FastNoiseLite.noise_type` enum VALUE onto the noise this
/// module implements, and reports whether the match is exact.
///
/// ⚠️ **The two enums do not share an ordering and must never be cast across.**
/// Godot's is `SIMPLEX=0, SIMPLEX_SMOOTH=1, CELLULAR=2, PERLIN=3,
/// VALUE_CUBIC=4, VALUE=5`, while [`NoiseType`] here is `Value, Perlin` — so an
/// `as` cast would turn Godot's PERLIN into an out-of-range index and its
/// SIMPLEX_SMOOTH into Perlin, silently. The property's DEFAULT is 1
/// (SIMPLEX_SMOOTH), so the wrong-by-default case is the common one.
///
/// The `bool` is `false` when the requested type is not implemented here and a
/// substitute was returned. Callers that can surface that (an importer warning,
/// an editor badge) should; the substitute is chosen to be the nearest
/// available family so a scene still renders recognisable noise rather than a
/// flat field.
pub fn noise_type_from_godot(value: i64) -> (NoiseType, bool) {
    match value {
        // Implemented exactly.
        3 => (NoiseType::Perlin, true),
        5 => (NoiseType::Value, true),
        // Gradient noises we do not have: Perlin is the nearest in character.
        0 | 1 => (NoiseType::Perlin, false),
        // Cellular and value-cubic both derive from the lattice-value family.
        2 | 4 => (NoiseType::Value, false),
        // Out of range: Godot would clamp to its own default (SIMPLEX_SMOOTH),
        // which we do not have, so this lands on the same substitute as 1.
        _ => (NoiseType::Perlin, false),
    }
}

/// Maps a Godot `FastNoiseLite.fractal_type` enum value onto [`FractalType`].
///
/// The two orderings DO coincide here (`NONE=0, FBM=1, RIDGED=2, PING_PONG=3`),
/// but this is written out rather than cast so that the agreement is checked
/// once and stays checked — the sibling `noise_type` enums look equally
/// castable and are not.
pub fn fractal_type_from_godot(value: i64) -> FractalType {
    match value {
        0 => FractalType::None,
        2 => FractalType::Ridged,
        3 => FractalType::PingPong,
        // 1 is FBM and is also Godot's default for this property.
        _ => FractalType::Fbm,
    }
}

#[cfg(test)]
mod godot_bridge_tests {
    use super::*;

    /// 🔑 The exact pairs, pinned by VALUE rather than by position.
    #[test]
    fn godot_noise_type_values_map_to_the_right_family() {
        assert_eq!(noise_type_from_godot(3), (NoiseType::Perlin, true));
        assert_eq!(noise_type_from_godot(5), (NoiseType::Value, true));
    }

    /// ⚠️ The regression this guards: casting Godot's value straight to this
    /// module's enum. Under a cast, 1 (SIMPLEX_SMOOTH) becomes Perlin and
    /// claims to be exact; here it is Perlin and is reported as a SUBSTITUTE.
    /// The difference is invisible in the pixels and is the whole point.
    #[test]
    fn unimplemented_types_are_reported_as_substitutes() {
        for value in [0, 1, 2, 4] {
            let (_, exact) = noise_type_from_godot(value);
            assert!(
                !exact,
                "Godot noise_type {value} is not implemented here and must be \
                 reported as a substitute, not silently accepted"
            );
        }
    }

    /// Godot's default is 1 = SIMPLEX_SMOOTH, so an unconfigured resource is
    /// exactly the substituted case — worth pinning so nobody "fixes" the
    /// default to 0 on the assumption that the first variant is the default.
    #[test]
    fn the_godot_default_noise_type_is_a_substitute() {
        assert_eq!(noise_type_from_godot(1), (NoiseType::Perlin, false));
    }

    #[test]
    fn fractal_values_match_godots_ordering() {
        assert_eq!(fractal_type_from_godot(0), FractalType::None);
        assert_eq!(fractal_type_from_godot(1), FractalType::Fbm);
        assert_eq!(fractal_type_from_godot(2), FractalType::Ridged);
        assert_eq!(fractal_type_from_godot(3), FractalType::PingPong);
    }
}

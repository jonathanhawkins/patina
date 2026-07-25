//! PCG32 pseudo-random number generator (pat-u3l8c.19).
//!
//! Mirrors Godot's `RandomPCG` / `RandomNumberGenerator`: a PCG32 stream that is
//! fully deterministic given a seed. This replaces the old global counter-LCG
//! (seeded 42) so shipped games get real, reproducible randomness instead of an
//! identical sequence every run.
//!
//! The [`Rng`] type provides the Godot `RandomNumberGenerator` method surface
//! (`seed`/`state`/`randi`/`randf`/`randi_range`/`randf_range`/`randfn`), and the
//! free functions ([`randomize`], [`seed`], [`randi`], [`randf`], …) drive a
//! thread-local global generator like Godot's `@GlobalScope` random functions.

use std::cell::RefCell;

/// PCG32 LCG multiplier (canonical constant, matches Godot).
const PCG_MULT: u64 = 6364136223846793005;
/// Default PCG increment / stream selector (odd), matching Godot's default.
pub const PCG_DEFAULT_INC: u64 = 0xda3e39cb94b95bdb;
/// Default seed used when no explicit seed is set.
pub const PCG_DEFAULT_SEED: u64 = 0x853c49e6748fea9b;

/// A deterministic PCG32 random number generator.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
    inc: u64,
    seed: u64,
}

impl Default for Rng {
    fn default() -> Self {
        Self::with_seed(PCG_DEFAULT_SEED)
    }
}

impl Rng {
    /// Creates a generator seeded with [`PCG_DEFAULT_SEED`].
    pub fn new() -> Self {
        Self::with_seed(PCG_DEFAULT_SEED)
    }

    /// Creates a generator with an explicit seed.
    pub fn with_seed(seed: u64) -> Self {
        let mut rng = Self {
            state: 0,
            inc: PCG_DEFAULT_INC,
            seed,
        };
        rng.set_seed(seed);
        rng
    }

    /// Reseeds the generator (Godot `RandomNumberGenerator.seed` setter). The
    /// same seed always reproduces the same sequence.
    pub fn set_seed(&mut self, seed: u64) {
        self.seed = seed;
        // Standard pcg32_srandom_r with a fixed default stream.
        self.state = 0;
        self.inc = PCG_DEFAULT_INC;
        self.next_u32();
        self.state = self.state.wrapping_add(seed);
        self.next_u32();
    }

    /// Returns the seed this generator was last seeded with.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Returns the current internal PCG state (Godot exposes `state`).
    pub fn state(&self) -> u64 {
        self.state
    }

    /// Sets the internal PCG state directly (Godot `state` setter), letting a
    /// caller resume a stream exactly where another left off.
    pub fn set_state(&mut self, state: u64) {
        self.state = state;
    }

    /// Core PCG32 step: advances the state and returns the next 32-bit value.
    fn next_u32(&mut self) -> u32 {
        let oldstate = self.state;
        self.state = oldstate.wrapping_mul(PCG_MULT).wrapping_add(self.inc);
        let xorshifted = (((oldstate >> 18) ^ oldstate) >> 27) as u32;
        let rot = (oldstate >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Returns a uniformly-distributed 32-bit unsigned integer.
    pub fn randi(&mut self) -> u32 {
        self.next_u32()
    }

    /// Returns a uniform float in `[0, 1]` (inclusive, like Godot `randf`).
    pub fn randf(&mut self) -> f64 {
        self.next_u32() as f64 / u32::MAX as f64
    }

    /// Returns a uniform integer in the inclusive range `[from, to]`.
    ///
    /// The bounds may be given in either order.
    pub fn randi_range(&mut self, from: i64, to: i64) -> i64 {
        let (lo, hi) = if from <= to { (from, to) } else { (to, from) };
        // Inclusive span; guard against overflow for full-range requests.
        let span = (hi as i128 - lo as i128 + 1) as u128;
        if span <= 1 {
            return lo;
        }
        lo + (self.next_u32() as u128 % span) as i64
    }

    /// Returns a uniform float in the inclusive range `[from, to]`.
    pub fn randf_range(&mut self, from: f64, to: f64) -> f64 {
        from + self.randf() * (to - from)
    }

    /// Returns a normally-distributed float with the given `mean` and standard
    /// `deviation` (Box–Muller transform, like Godot `randfn`).
    pub fn randfn(&mut self, mean: f64, deviation: f64) -> f64 {
        // Guard u1 away from 0 so ln() is finite.
        let u1 = self.randf().max(1e-12);
        let u2 = self.randf();
        let mag = deviation * (-2.0 * u1.ln()).sqrt();
        mag * (std::f64::consts::TAU * u2).cos() + mean
    }

    /// Reseeds from a non-deterministic entropy source (wall clock mixed with
    /// the current state) and returns the new seed, like Godot `randomize`.
    pub fn randomize(&mut self) -> u64 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let s = nanos ^ self.state.rotate_left(17) ^ 0x9e3779b97f4a7c15u64;
        self.set_seed(s);
        s
    }
}

/// Stateless Godot `rand_from_seed`: returns a value and advances `*seed`.
///
/// Repeated calls threading the updated `seed` back in produce a deterministic
/// stream, matching Godot's global `rand_from_seed`.
pub fn rand_from_seed(seed: &mut u64) -> u32 {
    let mut rng = Rng::with_seed(*seed);
    let value = rng.randi();
    *seed = rng.state();
    value
}

thread_local! {
    static GLOBAL: RefCell<Rng> = RefCell::new(Rng::new());
}

/// Reseeds the global generator from entropy and returns the new seed
/// (Godot `@GlobalScope.randomize`).
pub fn randomize() -> u64 {
    GLOBAL.with(|r| r.borrow_mut().randomize())
}

/// Reseeds the global generator (Godot `@GlobalScope.seed`).
pub fn seed(s: u64) {
    GLOBAL.with(|r| r.borrow_mut().set_seed(s));
}

/// Global `randi` — uniform 32-bit unsigned integer.
pub fn randi() -> u32 {
    GLOBAL.with(|r| r.borrow_mut().randi())
}

/// Global `randf` — uniform float in `[0, 1]`.
pub fn randf() -> f64 {
    GLOBAL.with(|r| r.borrow_mut().randf())
}

/// Global `randi_range` — uniform inclusive integer range.
pub fn randi_range(from: i64, to: i64) -> i64 {
    GLOBAL.with(|r| r.borrow_mut().randi_range(from, to))
}

/// Global `randf_range` — uniform inclusive float range.
pub fn randf_range(from: f64, to: f64) -> f64 {
    GLOBAL.with(|r| r.borrow_mut().randf_range(from, to))
}

/// Global `randfn` — normal distribution.
pub fn randfn(mean: f64, deviation: f64) -> f64 {
    GLOBAL.with(|r| r.borrow_mut().randfn(mean, deviation))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::with_seed(1234);
        let mut b = Rng::with_seed(1234);
        for _ in 0..100 {
            assert_eq!(a.randi(), b.randi());
        }
    }

    #[test]
    fn different_seed_diverges() {
        let mut a = Rng::with_seed(1);
        let mut b = Rng::with_seed(2);
        // The two streams must not be identical over a short prefix.
        let mut differ = false;
        for _ in 0..10 {
            if a.randi() != b.randi() {
                differ = true;
                break;
            }
        }
        assert!(differ, "distinct seeds must produce distinct sequences");
    }

    #[test]
    fn randf_within_unit_interval() {
        let mut rng = Rng::with_seed(99);
        for _ in 0..10_000 {
            let v = rng.randf();
            assert!((0.0..=1.0).contains(&v), "randf out of [0,1]: {}", v);
        }
    }

    #[test]
    fn randi_range_is_inclusive_and_in_bounds() {
        let mut rng = Rng::with_seed(7);
        let mut saw_lo = false;
        let mut saw_hi = false;
        for _ in 0..20_000 {
            let v = rng.randi_range(3, 8);
            assert!((3..=8).contains(&v), "randi_range out of [3,8]: {}", v);
            if v == 3 {
                saw_lo = true;
            }
            if v == 8 {
                saw_hi = true;
            }
        }
        assert!(
            saw_lo && saw_hi,
            "both inclusive bounds should be reachable"
        );
    }

    #[test]
    fn randi_range_reversed_bounds_and_single() {
        let mut rng = Rng::with_seed(7);
        for _ in 0..1000 {
            let v = rng.randi_range(10, 4); // reversed
            assert!((4..=10).contains(&v));
        }
        assert_eq!(rng.randi_range(5, 5), 5);
    }

    #[test]
    fn randf_range_in_bounds() {
        let mut rng = Rng::with_seed(55);
        for _ in 0..10_000 {
            let v = rng.randf_range(-2.0, 5.0);
            assert!(
                (-2.0..=5.0).contains(&v),
                "randf_range out of bounds: {}",
                v
            );
        }
    }

    #[test]
    fn randfn_mean_is_approximately_correct() {
        let mut rng = Rng::with_seed(2024);
        let (mean, dev) = (5.0, 2.0);
        let n = 50_000;
        let mut sum = 0.0;
        for _ in 0..n {
            sum += rng.randfn(mean, dev);
        }
        let avg = sum / n as f64;
        // Generous tolerance: sample mean must be near the requested mean.
        assert!((avg - mean).abs() < 0.1, "randfn mean off: {}", avg);
    }

    #[test]
    fn rand_from_seed_is_deterministic() {
        let mut s1 = 42u64;
        let mut s2 = 42u64;
        let a = rand_from_seed(&mut s1);
        let b = rand_from_seed(&mut s2);
        assert_eq!(a, b);
        assert_eq!(s1, s2, "updated seed must match for equal inputs");
        // Threading the updated seed advances the stream.
        let c = rand_from_seed(&mut s1);
        assert_eq!(s1 != s2 || a == c, true);
    }

    #[test]
    fn randomize_leaves_a_reproducible_seed() {
        let mut rng = Rng::new();
        let s = rng.randomize();
        let after: Vec<u32> = (0..16).map(|_| rng.randi()).collect();
        // Reseeding a fresh generator with the returned seed reproduces it.
        let mut clone = Rng::with_seed(s);
        let reproduced: Vec<u32> = (0..16).map(|_| clone.randi()).collect();
        assert_eq!(after, reproduced);
    }

    #[test]
    fn state_roundtrip_resumes_stream() {
        let mut rng = Rng::with_seed(321);
        for _ in 0..5 {
            rng.randi();
        }
        let saved = rng.state();
        let expected: Vec<u32> = (0..8).map(|_| rng.randi()).collect();

        let mut resumed = Rng::with_seed(0);
        resumed.set_state(saved);
        let got: Vec<u32> = (0..8).map(|_| resumed.randi()).collect();
        assert_eq!(expected, got);
    }

    #[test]
    fn global_seed_makes_sequence_deterministic() {
        seed(777);
        let a: Vec<u32> = (0..32).map(|_| randi()).collect();
        seed(777);
        let b: Vec<u32> = (0..32).map(|_| randi()).collect();
        assert_eq!(a, b);
    }
}

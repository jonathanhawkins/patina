//! 3D CPU particle system: emission, simulation, gravity, velocity, and color curves.
//!
//! Provides a Godot-compatible CPUParticles3D emitter with deterministic
//! randomness (xorshift), configurable 3D emission shapes, per-particle
//! physics (gravity, angular velocity, damping), and color/scale curves
//! interpolated over particle lifetime.

use gdcore::math::{Color, Vector3};

// ---------------------------------------------------------------------------
// Deterministic random (xorshift32)
// ---------------------------------------------------------------------------

/// Simple xorshift32 PRNG for deterministic particle randomness.
fn xorshift32(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

/// Returns a float in [0, 1) from the xorshift state.
fn rand_f32(state: &mut u32) -> f32 {
    (xorshift32(state) & 0x00FF_FFFF) as f32 / 16_777_216.0
}

/// Returns a float in [min, max] from the xorshift state.
fn rand_range(state: &mut u32, min: f32, max: f32) -> f32 {
    min + rand_f32(state) * (max - min)
}

// ---------------------------------------------------------------------------
// ColorCurve
// ---------------------------------------------------------------------------

/// A piecewise-linear color curve sampled by normalized time [0, 1].
///
/// Mirrors Godot's `Gradient` resource. When empty, returns white.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorCurve {
    /// Sorted (time, color) keyframes. Times must be in [0, 1].
    pub keys: Vec<(f32, Color)>,
}

impl Default for ColorCurve {
    fn default() -> Self {
        Self {
            keys: vec![(0.0, Color::WHITE), (1.0, Color::WHITE)],
        }
    }
}

impl ColorCurve {
    /// Creates a simple two-stop gradient from `start` to `end`.
    pub fn linear(start: Color, end: Color) -> Self {
        Self {
            keys: vec![(0.0, start), (1.0, end)],
        }
    }

    /// Creates a three-stop gradient: start → mid → end.
    pub fn three_stop(start: Color, mid: Color, end: Color) -> Self {
        Self {
            keys: vec![(0.0, start), (0.5, mid), (1.0, end)],
        }
    }

    /// Samples the curve at normalized time `t` (clamped to [0, 1]).
    pub fn sample(&self, t: f32) -> Color {
        if self.keys.is_empty() {
            return Color::WHITE;
        }
        let t = t.clamp(0.0, 1.0);
        if self.keys.len() == 1 {
            return self.keys[0].1;
        }
        // Before first key
        if t <= self.keys[0].0 {
            return self.keys[0].1;
        }
        // After last key
        if t >= self.keys[self.keys.len() - 1].0 {
            return self.keys[self.keys.len() - 1].1;
        }
        // Find the two surrounding keys
        for i in 0..self.keys.len() - 1 {
            let (t0, c0) = self.keys[i];
            let (t1, c1) = self.keys[i + 1];
            if t >= t0 && t <= t1 {
                let span = t1 - t0;
                if span < 1e-6 {
                    return c0;
                }
                let frac = (t - t0) / span;
                return c0.lerp(c1, frac);
            }
        }
        self.keys[self.keys.len() - 1].1
    }
}

// ---------------------------------------------------------------------------
// ScaleCurve
// ---------------------------------------------------------------------------

/// A piecewise-linear float curve sampled by normalized time [0, 1].
///
/// Used for scale-over-lifetime and other scalar properties.
#[derive(Debug, Clone, PartialEq)]
pub struct ScaleCurve {
    /// Sorted (time, value) keyframes. Times must be in [0, 1].
    pub keys: Vec<(f32, f32)>,
}

impl Default for ScaleCurve {
    fn default() -> Self {
        Self {
            keys: vec![(0.0, 1.0), (1.0, 1.0)],
        }
    }
}

impl ScaleCurve {
    /// Creates a linear ramp from `start` to `end`.
    pub fn linear(start: f32, end: f32) -> Self {
        Self {
            keys: vec![(0.0, start), (1.0, end)],
        }
    }

    /// Samples the curve at normalized time `t` (clamped to [0, 1]).
    pub fn sample(&self, t: f32) -> f32 {
        if self.keys.is_empty() {
            return 1.0;
        }
        let t = t.clamp(0.0, 1.0);
        if self.keys.len() == 1 {
            return self.keys[0].1;
        }
        if t <= self.keys[0].0 {
            return self.keys[0].1;
        }
        if t >= self.keys[self.keys.len() - 1].0 {
            return self.keys[self.keys.len() - 1].1;
        }
        for i in 0..self.keys.len() - 1 {
            let (t0, v0) = self.keys[i];
            let (t1, v1) = self.keys[i + 1];
            if t >= t0 && t <= t1 {
                let span = t1 - t0;
                if span < 1e-6 {
                    return v0;
                }
                let frac = (t - t0) / span;
                return v0 + (v1 - v0) * frac;
            }
        }
        self.keys[self.keys.len() - 1].1
    }
}

// ---------------------------------------------------------------------------
// EmissionShape3D
// ---------------------------------------------------------------------------

/// Shape from which 3D particles are spawned.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum EmissionShape3D {
    /// Emit from a single point at the origin.
    #[default]
    Point,
    /// Emit from a random position within a sphere.
    Sphere { radius: f32 },
    /// Emit from a random position within an axis-aligned box.
    Box { extents: Vector3 },
    /// Emit from the surface of a sphere.
    SphereSurface { radius: f32 },
    /// Emit from a cone pointing along +Y with the given angle and height.
    Cone { angle: f32, height: f32 },
}

impl EmissionShape3D {
    /// Sample a random position within this shape.
    fn sample(&self, rng: &mut u32) -> Vector3 {
        match *self {
            EmissionShape3D::Point => Vector3::ZERO,
            EmissionShape3D::Sphere { radius } => {
                // Uniform sphere sampling via rejection
                loop {
                    let x = rand_range(rng, -1.0, 1.0);
                    let y = rand_range(rng, -1.0, 1.0);
                    let z = rand_range(rng, -1.0, 1.0);
                    let len_sq = x * x + y * y + z * z;
                    if len_sq <= 1.0 && len_sq > 1e-6 {
                        let r = radius * rand_f32(rng).cbrt();
                        let inv_len = 1.0 / len_sq.sqrt();
                        return Vector3::new(x * inv_len * r, y * inv_len * r, z * inv_len * r);
                    }
                }
            }
            EmissionShape3D::Box { extents } => {
                let x = rand_range(rng, -extents.x, extents.x);
                let y = rand_range(rng, -extents.y, extents.y);
                let z = rand_range(rng, -extents.z, extents.z);
                Vector3::new(x, y, z)
            }
            EmissionShape3D::SphereSurface { radius } => {
                // Random direction on unit sphere via rejection
                loop {
                    let x = rand_range(rng, -1.0, 1.0);
                    let y = rand_range(rng, -1.0, 1.0);
                    let z = rand_range(rng, -1.0, 1.0);
                    let len_sq = x * x + y * y + z * z;
                    if len_sq > 1e-6 && len_sq <= 1.0 {
                        let inv_len = radius / len_sq.sqrt();
                        return Vector3::new(x * inv_len, y * inv_len, z * inv_len);
                    }
                }
            }
            EmissionShape3D::Cone { angle, height } => {
                let half_angle = angle.to_radians() * 0.5;
                let y = rand_range(rng, 0.0, height);
                let r = y * half_angle.tan() * rand_f32(rng).sqrt();
                let theta = rand_f32(rng) * std::f32::consts::TAU;
                Vector3::new(r * theta.cos(), y, r * theta.sin())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ParticleMaterial3D
// ---------------------------------------------------------------------------

/// Per-emitter configuration that controls how 3D particles look and move.
#[derive(Debug, Clone, PartialEq)]
pub struct ParticleMaterial3D {
    /// Base emission direction (normalized).
    pub direction: Vector3,
    /// Angular spread in degrees around `direction`.
    pub spread: f32,
    /// Minimum initial speed.
    pub initial_velocity_min: f32,
    /// Maximum initial speed.
    pub initial_velocity_max: f32,
    /// Gravity vector applied every frame (world space).
    pub gravity: Vector3,
    /// Linear damping factor (velocity *= 1 - damping * dt).
    pub damping: f32,
    /// Minimum initial scale.
    pub scale_min: f32,
    /// Maximum initial scale.
    pub scale_max: f32,
    /// Color curve over particle lifetime.
    pub color_curve: ColorCurve,
    /// Scale curve over particle lifetime (multiplied with initial scale).
    pub scale_curve: ScaleCurve,
    /// Lifetime randomness factor in [0, 1].
    pub lifetime_randomness: f32,
    /// Tangential acceleration (perpendicular to velocity).
    pub tangential_accel: f32,
    /// Radial acceleration (toward/away from emitter origin).
    pub radial_accel: f32,
}

impl Default for ParticleMaterial3D {
    fn default() -> Self {
        Self {
            direction: Vector3::UP,
            spread: 45.0,
            initial_velocity_min: 0.0,
            initial_velocity_max: 0.0,
            gravity: Vector3::new(0.0, -9.8, 0.0),
            damping: 0.0,
            scale_min: 1.0,
            scale_max: 1.0,
            color_curve: ColorCurve::default(),
            scale_curve: ScaleCurve::default(),
            lifetime_randomness: 0.0,
            tangential_accel: 0.0,
            radial_accel: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Particle3D
// ---------------------------------------------------------------------------

/// A single live 3D particle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Particle3D {
    /// Current position in world space.
    pub position: Vector3,
    /// Current velocity.
    pub velocity: Vector3,
    /// Total lifetime this particle was given.
    pub lifetime: f32,
    /// Remaining lifetime (counts down to zero).
    pub lifetime_remaining: f32,
    /// Current display color (updated from curve each frame).
    pub color: Color,
    /// Base scale assigned at emission.
    pub base_scale: f32,
    /// Current display scale (base_scale * scale_curve).
    pub scale: f32,
}

impl Particle3D {
    /// Returns the normalized age in [0, 1] where 0 = just born, 1 = dead.
    pub fn age_ratio(&self) -> f32 {
        if self.lifetime <= 0.0 {
            return 1.0;
        }
        1.0 - (self.lifetime_remaining / self.lifetime).clamp(0.0, 1.0)
    }

    /// Returns `true` if this particle has expired.
    pub fn is_dead(&self) -> bool {
        self.lifetime_remaining <= 0.0
    }
}

// ---------------------------------------------------------------------------
// ParticleEmitter3D
// ---------------------------------------------------------------------------

/// Configuration for a 3D CPU particle emitter.
#[derive(Debug, Clone)]
pub struct ParticleEmitter3D {
    /// Material controlling particle appearance and physics.
    pub material: ParticleMaterial3D,
    /// Shape from which particles are emitted.
    pub emission_shape: EmissionShape3D,
    /// Number of particles to emit per cycle.
    pub amount: u32,
    /// Base lifetime in seconds.
    pub lifetime: f32,
    /// If `true`, emit all particles once and stop.
    pub one_shot: bool,
    /// Explosiveness ratio in [0, 1]. 1 = all particles at once.
    pub explosiveness: f32,
    /// Speed multiplier for the simulation.
    pub speed_scale: f32,
    /// Whether the emitter is currently active.
    pub emitting: bool,
    /// If `true`, particles are in the emitter's local space.
    pub local_coords: bool,
}

impl Default for ParticleEmitter3D {
    fn default() -> Self {
        Self {
            material: ParticleMaterial3D::default(),
            emission_shape: EmissionShape3D::default(),
            amount: 8,
            lifetime: 1.0,
            one_shot: false,
            explosiveness: 0.0,
            speed_scale: 1.0,
            emitting: true,
            local_coords: false,
        }
    }
}

impl ParticleEmitter3D {
    /// Spawn a single particle using the emitter's material and shape.
    pub fn emit_particle(&self, seed: u32) -> Particle3D {
        let mut rng = seed;
        if rng == 0 {
            rng = 1;
        }

        let position = self.emission_shape.sample(&mut rng);

        // Direction with spread: perturb direction by a random rotation
        let dir = self.material.direction.normalized();
        let spread_rad = self.material.spread.to_radians();
        let angle_off = rand_range(&mut rng, 0.0, spread_rad);
        let azimuth = rand_range(&mut rng, 0.0, std::f32::consts::TAU);

        // Build a perturbed direction via spherical offset from the base direction
        let velocity_dir = perturb_direction(dir, angle_off, azimuth);

        let speed = rand_range(
            &mut rng,
            self.material.initial_velocity_min,
            self.material.initial_velocity_max,
        );
        let velocity = velocity_dir * speed;

        let lifetime_factor = 1.0 - self.material.lifetime_randomness * rand_f32(&mut rng);
        let lifetime = self.lifetime * lifetime_factor;

        let base_scale = rand_range(&mut rng, self.material.scale_min, self.material.scale_max);

        Particle3D {
            position,
            velocity,
            lifetime,
            lifetime_remaining: lifetime,
            color: self.material.color_curve.sample(0.0),
            base_scale,
            scale: base_scale,
        }
    }
}

/// Perturb a direction vector by `angle_off` radians around an arbitrary axis
/// rotated by `azimuth`.
fn perturb_direction(dir: Vector3, angle_off: f32, azimuth: f32) -> Vector3 {
    if angle_off.abs() < 1e-6 {
        return dir;
    }

    // Find an orthonormal basis around dir
    let up = if dir.y.abs() < 0.999 {
        Vector3::UP
    } else {
        Vector3::new(1.0, 0.0, 0.0)
    };
    let right = dir.cross(up).normalized();
    let ortho_up = right.cross(dir).normalized();

    // Rotate around dir by azimuth to get the perturbation axis
    let perturb_axis = right * azimuth.cos() + ortho_up * azimuth.sin();

    // Rotate dir toward perturb_axis by angle_off
    dir * angle_off.cos() + perturb_axis * angle_off.sin()
}

// ---------------------------------------------------------------------------
// ParticleSimulator3D
// ---------------------------------------------------------------------------

/// Drives 3D particle emission, physics, and culling each frame.
#[derive(Debug, Clone)]
pub struct ParticleSimulator3D {
    /// Emitter configuration.
    pub emitter: ParticleEmitter3D,
    /// Currently live particles.
    pub active_particles: Vec<Particle3D>,
    /// Sub-frame time accumulator for emission pacing.
    pub time_accumulator: f32,
    /// Total number of particles emitted (used for seeding).
    pub total_emitted: u64,
}

impl ParticleSimulator3D {
    /// Creates a new simulator from the given emitter configuration.
    pub fn new(emitter: ParticleEmitter3D) -> Self {
        Self {
            emitter,
            active_particles: Vec::new(),
            time_accumulator: 0.0,
            total_emitted: 0,
        }
    }

    /// Advance the simulation by `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        let dt_scaled = dt * self.emitter.speed_scale;

        // --- Age and integrate existing particles ---
        for p in &mut self.active_particles {
            p.lifetime_remaining -= dt_scaled;

            // Apply gravity
            p.velocity = p.velocity + self.emitter.material.gravity * dt_scaled;

            // Apply damping
            if self.emitter.material.damping > 0.0 {
                let damp = (1.0 - self.emitter.material.damping * dt_scaled).max(0.0);
                p.velocity = p.velocity * damp;
            }

            // Apply radial acceleration (away from emitter origin)
            if self.emitter.material.radial_accel != 0.0 {
                let dist = p.position.length();
                if dist > 1e-6 {
                    let radial_dir = p.position * (1.0 / dist);
                    p.velocity = p.velocity + radial_dir * self.emitter.material.radial_accel * dt_scaled;
                }
            }

            // Apply tangential acceleration
            if self.emitter.material.tangential_accel != 0.0 {
                let dist = p.position.length();
                if dist > 1e-6 {
                    let radial_dir = p.position * (1.0 / dist);
                    // Tangential = cross(up, radial) for a default tangent
                    let tangent = Vector3::UP.cross(radial_dir).normalized();
                    p.velocity = p.velocity + tangent * self.emitter.material.tangential_accel * dt_scaled;
                }
            }

            // Integrate position
            p.position = p.position + p.velocity * dt_scaled;

            // Update color and scale from curves
            let t = p.age_ratio();
            p.color = self.emitter.material.color_curve.sample(t);
            p.scale = p.base_scale * self.emitter.material.scale_curve.sample(t);
        }

        // --- Remove dead particles ---
        self.active_particles.retain(|p| !p.is_dead());

        // --- Emit new particles ---
        if self.emitter.emitting {
            let amount = self.emitter.amount.max(1) as f32;
            let lifetime = self.emitter.lifetime.max(0.001);

            if self.emitter.explosiveness >= 1.0 {
                self.time_accumulator += dt_scaled;
                if self.time_accumulator >= lifetime {
                    self.time_accumulator -= lifetime;
                    for _ in 0..self.emitter.amount {
                        self.total_emitted += 1;
                        let seed = (self.total_emitted & 0xFFFF_FFFF) as u32;
                        let particle = self.emitter.emit_particle(seed.max(1));
                        self.active_particles.push(particle);
                    }
                    if self.emitter.one_shot {
                        self.emitter.emitting = false;
                    }
                }
            } else {
                let emit_interval = lifetime / amount;
                self.time_accumulator += dt_scaled;

                while self.time_accumulator >= emit_interval {
                    self.time_accumulator -= emit_interval;
                    self.total_emitted += 1;
                    let seed = (self.total_emitted & 0xFFFF_FFFF) as u32;
                    let particle = self.emitter.emit_particle(seed.max(1));
                    self.active_particles.push(particle);
                }

                if self.emitter.one_shot && self.total_emitted >= self.emitter.amount as u64 {
                    self.emitter.emitting = false;
                }
            }
        }
    }

    /// Returns draw data for each live particle: (position, color, scale).
    pub fn get_draw_data(&self) -> Vec<(Vector3, Color, f32)> {
        self.active_particles
            .iter()
            .map(|p| (p.position, p.color, p.scale))
            .collect()
    }

    /// Returns the number of currently live particles.
    pub fn particle_count(&self) -> usize {
        self.active_particles.len()
    }

    /// Returns `true` if this is a one-shot emitter and all particles are dead.
    pub fn is_complete(&self) -> bool {
        self.emitter.one_shot && !self.emitter.emitting && self.active_particles.is_empty()
    }

    /// Restarts the emitter, clearing all particles and resetting state.
    pub fn restart(&mut self) {
        self.active_particles.clear();
        self.time_accumulator = 0.0;
        self.total_emitted = 0;
        self.emitter.emitting = true;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-4;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    // -- xorshift determinism -----------------------------------------------

    #[test]
    fn xorshift_is_deterministic() {
        let mut a = 42u32;
        let mut b = 42u32;
        let vals_a: Vec<u32> = (0..10).map(|_| xorshift32(&mut a)).collect();
        let vals_b: Vec<u32> = (0..10).map(|_| xorshift32(&mut b)).collect();
        assert_eq!(vals_a, vals_b);
    }

    #[test]
    fn rand_f32_in_unit_range() {
        let mut rng = 123u32;
        for _ in 0..100 {
            let v = rand_f32(&mut rng);
            assert!(v >= 0.0 && v < 1.0, "rand_f32 out of range: {v}");
        }
    }

    // -- EmissionShape3D ----------------------------------------------------

    #[test]
    fn point_shape_emits_at_origin() {
        let mut rng = 1u32;
        let pos = EmissionShape3D::Point.sample(&mut rng);
        assert_eq!(pos, Vector3::ZERO);
    }

    #[test]
    fn sphere_shape_within_radius() {
        let shape = EmissionShape3D::Sphere { radius: 10.0 };
        let mut rng = 77u32;
        for _ in 0..50 {
            let pos = shape.sample(&mut rng);
            assert!(
                pos.length() <= 10.0 + EPSILON,
                "Sphere sample outside radius: {pos:?}, len={}",
                pos.length()
            );
        }
    }

    #[test]
    fn box_shape_within_extents() {
        let shape = EmissionShape3D::Box {
            extents: Vector3::new(5.0, 3.0, 7.0),
        };
        let mut rng = 99u32;
        for _ in 0..50 {
            let pos = shape.sample(&mut rng);
            assert!(pos.x.abs() <= 5.0 + EPSILON);
            assert!(pos.y.abs() <= 3.0 + EPSILON);
            assert!(pos.z.abs() <= 7.0 + EPSILON);
        }
    }

    #[test]
    fn sphere_surface_on_radius() {
        let shape = EmissionShape3D::SphereSurface { radius: 5.0 };
        let mut rng = 33u32;
        for _ in 0..50 {
            let pos = shape.sample(&mut rng);
            let len = pos.length();
            assert!(
                (len - 5.0).abs() < 0.01,
                "SphereSurface not on radius: len={len}"
            );
        }
    }

    #[test]
    fn cone_shape_within_bounds() {
        let shape = EmissionShape3D::Cone {
            angle: 45.0,
            height: 10.0,
        };
        let mut rng = 55u32;
        for _ in 0..50 {
            let pos = shape.sample(&mut rng);
            assert!(pos.y >= -EPSILON && pos.y <= 10.0 + EPSILON);
        }
    }

    // -- ColorCurve ---------------------------------------------------------

    #[test]
    fn color_curve_default_is_white() {
        let curve = ColorCurve::default();
        assert_eq!(curve.sample(0.0), Color::WHITE);
        assert_eq!(curve.sample(0.5), Color::WHITE);
        assert_eq!(curve.sample(1.0), Color::WHITE);
    }

    #[test]
    fn color_curve_linear_interpolation() {
        let curve = ColorCurve::linear(
            Color::new(1.0, 0.0, 0.0, 1.0),
            Color::new(0.0, 0.0, 1.0, 1.0),
        );
        let mid = curve.sample(0.5);
        assert!(approx_eq(mid.r, 0.5));
        assert!(approx_eq(mid.b, 0.5));

        let start = curve.sample(0.0);
        assert!(approx_eq(start.r, 1.0));
        assert!(approx_eq(start.b, 0.0));
    }

    #[test]
    fn color_curve_three_stop() {
        let curve = ColorCurve::three_stop(
            Color::new(1.0, 0.0, 0.0, 1.0),
            Color::new(0.0, 1.0, 0.0, 1.0),
            Color::new(0.0, 0.0, 1.0, 1.0),
        );
        let at_quarter = curve.sample(0.25);
        // Between red (0.0) and green (0.5), at 50%
        assert!(approx_eq(at_quarter.r, 0.5));
        assert!(approx_eq(at_quarter.g, 0.5));
    }

    #[test]
    fn color_curve_clamps_at_edges() {
        let curve = ColorCurve::linear(
            Color::new(1.0, 0.0, 0.0, 1.0),
            Color::new(0.0, 0.0, 1.0, 1.0),
        );
        let before = curve.sample(-0.5);
        assert!(approx_eq(before.r, 1.0));
        let after = curve.sample(1.5);
        assert!(approx_eq(after.b, 1.0));
    }

    // -- ScaleCurve ---------------------------------------------------------

    #[test]
    fn scale_curve_default_is_one() {
        let curve = ScaleCurve::default();
        assert!(approx_eq(curve.sample(0.0), 1.0));
        assert!(approx_eq(curve.sample(0.5), 1.0));
        assert!(approx_eq(curve.sample(1.0), 1.0));
    }

    #[test]
    fn scale_curve_linear_ramp() {
        let curve = ScaleCurve::linear(1.0, 0.0);
        assert!(approx_eq(curve.sample(0.0), 1.0));
        assert!(approx_eq(curve.sample(0.5), 0.5));
        assert!(approx_eq(curve.sample(1.0), 0.0));
    }

    // -- ParticleMaterial3D defaults ----------------------------------------

    #[test]
    fn material_defaults() {
        let mat = ParticleMaterial3D::default();
        assert_eq!(mat.direction, Vector3::UP);
        assert!(approx_eq(mat.spread, 45.0));
        assert!(approx_eq(mat.gravity.y, -9.8));
        assert!(approx_eq(mat.damping, 0.0));
        assert!(approx_eq(mat.scale_min, 1.0));
        assert!(approx_eq(mat.scale_max, 1.0));
    }

    // -- Particle3D ---------------------------------------------------------

    #[test]
    fn particle_age_ratio() {
        let p = Particle3D {
            position: Vector3::ZERO,
            velocity: Vector3::ZERO,
            lifetime: 2.0,
            lifetime_remaining: 1.0,
            color: Color::WHITE,
            base_scale: 1.0,
            scale: 1.0,
        };
        assert!(approx_eq(p.age_ratio(), 0.5));
    }

    #[test]
    fn particle_is_dead() {
        let mut p = Particle3D {
            position: Vector3::ZERO,
            velocity: Vector3::ZERO,
            lifetime: 1.0,
            lifetime_remaining: 0.5,
            color: Color::WHITE,
            base_scale: 1.0,
            scale: 1.0,
        };
        assert!(!p.is_dead());
        p.lifetime_remaining = 0.0;
        assert!(p.is_dead());
    }

    // -- ParticleEmitter3D --------------------------------------------------

    #[test]
    fn emitter_defaults() {
        let e = ParticleEmitter3D::default();
        assert_eq!(e.amount, 8);
        assert!(approx_eq(e.lifetime, 1.0));
        assert!(!e.one_shot);
        assert!(e.emitting);
    }

    #[test]
    fn emit_particle_deterministic() {
        let e = ParticleEmitter3D {
            material: ParticleMaterial3D {
                initial_velocity_min: 5.0,
                initial_velocity_max: 10.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let p1 = e.emit_particle(42);
        let p2 = e.emit_particle(42);
        assert_eq!(p1.position, p2.position);
        assert_eq!(p1.velocity, p2.velocity);
        assert_eq!(p1.lifetime, p2.lifetime);
    }

    #[test]
    fn emit_particle_different_seeds_differ() {
        let e = ParticleEmitter3D {
            material: ParticleMaterial3D {
                initial_velocity_min: 5.0,
                initial_velocity_max: 10.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let p1 = e.emit_particle(42);
        let p2 = e.emit_particle(99);
        // Extremely unlikely to be exactly equal
        assert!(p1.velocity != p2.velocity || p1.position != p2.position);
    }

    // -- ParticleSimulator3D ------------------------------------------------

    #[test]
    fn simulator_starts_empty() {
        let sim = ParticleSimulator3D::new(ParticleEmitter3D::default());
        assert_eq!(sim.particle_count(), 0);
        assert_eq!(sim.total_emitted, 0);
    }

    #[test]
    fn simulator_emits_particles() {
        let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
            amount: 10,
            lifetime: 1.0,
            ..Default::default()
        });
        sim.step(1.0);
        assert!(sim.particle_count() > 0, "Should have emitted particles");
        assert!(sim.total_emitted > 0);
    }

    #[test]
    fn simulator_particles_die() {
        let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
            amount: 5,
            lifetime: 0.1,
            one_shot: true,
            explosiveness: 1.0,
            ..Default::default()
        });
        sim.step(0.1); // Emit burst
        assert!(sim.particle_count() > 0);
        sim.step(0.2); // Particles should die
        assert_eq!(sim.particle_count(), 0);
        assert!(sim.is_complete());
    }

    #[test]
    fn simulator_gravity_affects_velocity() {
        let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
            amount: 1,
            lifetime: 10.0,
            explosiveness: 1.0,
            material: ParticleMaterial3D {
                gravity: Vector3::new(0.0, -10.0, 0.0),
                initial_velocity_min: 0.0,
                initial_velocity_max: 0.0,
                ..Default::default()
            },
            ..Default::default()
        });
        sim.step(10.0); // emit
        sim.step(1.0); // integrate 1s
        let p = &sim.active_particles[0];
        // Velocity should be roughly (0, -10, 0) after 1s of gravity
        assert!(p.velocity.y < -5.0, "Gravity not applied: vy={}", p.velocity.y);
    }

    #[test]
    fn simulator_damping_reduces_velocity() {
        let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
            amount: 1,
            lifetime: 10.0,
            explosiveness: 1.0,
            material: ParticleMaterial3D {
                gravity: Vector3::ZERO,
                damping: 0.5,
                initial_velocity_min: 10.0,
                initial_velocity_max: 10.0,
                ..Default::default()
            },
            ..Default::default()
        });
        sim.step(10.0); // emit
        let initial_speed = sim.active_particles[0].velocity.length();
        sim.step(1.0);
        let final_speed = sim.active_particles[0].velocity.length();
        assert!(
            final_speed < initial_speed,
            "Damping should reduce speed: {initial_speed} -> {final_speed}"
        );
    }

    #[test]
    fn simulator_color_curve_applied() {
        let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
            amount: 1,
            lifetime: 1.0,
            explosiveness: 1.0,
            material: ParticleMaterial3D {
                gravity: Vector3::ZERO,
                color_curve: ColorCurve::linear(
                    Color::new(1.0, 0.0, 0.0, 1.0),
                    Color::new(0.0, 0.0, 1.0, 1.0),
                ),
                initial_velocity_min: 0.0,
                initial_velocity_max: 0.0,
                ..Default::default()
            },
            ..Default::default()
        });
        sim.step(1.0); // emit
        // At birth, color should be red
        assert!(sim.active_particles[0].color.r > 0.8);
        sim.step(0.5); // half lifetime
        // Color should be roughly purple (mix of red and blue)
        let c = sim.active_particles[0].color;
        assert!(c.r > 0.3 && c.b > 0.3, "Color should be mixed: {c:?}");
    }

    #[test]
    fn simulator_scale_curve_applied() {
        let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
            amount: 1,
            lifetime: 1.0,
            explosiveness: 1.0,
            material: ParticleMaterial3D {
                gravity: Vector3::ZERO,
                scale_curve: ScaleCurve::linear(1.0, 0.0),
                initial_velocity_min: 0.0,
                initial_velocity_max: 0.0,
                ..Default::default()
            },
            ..Default::default()
        });
        sim.step(1.0); // emit
        assert!(approx_eq(sim.active_particles[0].scale, 1.0));
        sim.step(0.5);
        // At 50% life, scale should be ~0.5
        assert!(
            sim.active_particles[0].scale < 0.7,
            "Scale curve not applied: {}",
            sim.active_particles[0].scale
        );
    }

    #[test]
    fn simulator_one_shot_stops() {
        let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
            amount: 3,
            lifetime: 0.1,
            one_shot: true,
            explosiveness: 1.0,
            ..Default::default()
        });
        sim.step(0.1); // emit burst
        assert!(!sim.emitter.emitting);
        let count = sim.particle_count();
        sim.step(0.1); // no new particles
        // Should not emit more (old ones die)
        assert!(sim.particle_count() <= count);
    }

    #[test]
    fn simulator_restart() {
        let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
            amount: 3,
            lifetime: 0.1,
            one_shot: true,
            explosiveness: 1.0,
            ..Default::default()
        });
        sim.step(0.1);
        sim.step(0.2);
        assert!(sim.is_complete());

        sim.restart();
        assert_eq!(sim.particle_count(), 0);
        assert!(sim.emitter.emitting);
        assert_eq!(sim.total_emitted, 0);
    }

    #[test]
    fn simulator_get_draw_data() {
        let mut sim = ParticleSimulator3D::new(ParticleEmitter3D {
            amount: 5,
            lifetime: 1.0,
            explosiveness: 1.0,
            ..Default::default()
        });
        sim.step(1.0); // emit burst
        let data = sim.get_draw_data();
        assert_eq!(data.len(), 5);
        for (pos, color, scale) in &data {
            assert!(pos.length().is_finite());
            assert!(color.a > 0.0);
            assert!(*scale > 0.0);
        }
    }

    #[test]
    fn perturb_direction_zero_angle_returns_original() {
        let dir = Vector3::new(0.0, 1.0, 0.0);
        let result = perturb_direction(dir, 0.0, 0.0);
        assert!(approx_eq(result.x, dir.x));
        assert!(approx_eq(result.y, dir.y));
        assert!(approx_eq(result.z, dir.z));
    }

    #[test]
    fn perturb_direction_produces_unit_vector() {
        let dir = Vector3::new(0.0, 1.0, 0.0);
        let result = perturb_direction(dir, 0.5, 1.0);
        let len = result.length();
        assert!(
            (len - 1.0).abs() < 0.01,
            "Perturbed direction not unit: len={len}"
        );
    }
}

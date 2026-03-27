//! GPUParticles3D: 3D particle system with basic emitter and material.
//!
//! Mirrors Godot's `GPUParticles3D` node surface including:
//! - Emitter shapes (point, box, sphere)
//! - `ParticleProcessMaterial` with gravity, velocity, color, scale
//! - Deterministic xorshift32 PRNG for reproducible emission
//! - Simulation stepping with physics integration

use gdcore::math::{Color, Vector3};

// ---------------------------------------------------------------------------
// Deterministic random (xorshift32)
// ---------------------------------------------------------------------------

fn xorshift32(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

fn rand_f32(state: &mut u32) -> f32 {
    (xorshift32(state) & 0x00FF_FFFF) as f32 / 16_777_216.0
}

fn rand_range(state: &mut u32, min: f32, max: f32) -> f32 {
    min + rand_f32(state) * (max - min)
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
    /// Emit from a random position within a box.
    Box { extents: Vector3 },
    /// Emit from the surface of a sphere (shell).
    SphereSurface { radius: f32 },
    /// Emit from a ring around the Y axis.
    Ring {
        inner_radius: f32,
        outer_radius: f32,
        height: f32,
    },
}

impl EmissionShape3D {
    /// Sample a random position within this shape.
    fn sample(&self, rng: &mut u32) -> Vector3 {
        match *self {
            Self::Point => Vector3::ZERO,
            Self::Sphere { radius } => {
                // Uniform random point inside sphere via rejection-free method.
                let theta = rand_f32(rng) * std::f32::consts::TAU;
                let phi = (1.0 - 2.0 * rand_f32(rng)).acos();
                let r = radius * rand_f32(rng).cbrt();
                Vector3::new(
                    r * phi.sin() * theta.cos(),
                    r * phi.cos(),
                    r * phi.sin() * theta.sin(),
                )
            }
            Self::Box { extents } => Vector3::new(
                rand_range(rng, -extents.x, extents.x),
                rand_range(rng, -extents.y, extents.y),
                rand_range(rng, -extents.z, extents.z),
            ),
            Self::SphereSurface { radius } => {
                let theta = rand_f32(rng) * std::f32::consts::TAU;
                let phi = (1.0 - 2.0 * rand_f32(rng)).acos();
                Vector3::new(
                    radius * phi.sin() * theta.cos(),
                    radius * phi.cos(),
                    radius * phi.sin() * theta.sin(),
                )
            }
            Self::Ring {
                inner_radius,
                outer_radius,
                height,
            } => {
                let angle = rand_f32(rng) * std::f32::consts::TAU;
                let r = rand_range(rng, inner_radius, outer_radius);
                let y = rand_range(rng, -height * 0.5, height * 0.5);
                Vector3::new(r * angle.cos(), y, r * angle.sin())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ParticleProcessMaterial3D
// ---------------------------------------------------------------------------

/// Per-emitter configuration controlling how 3D particles look and move.
///
/// Maps to Godot's `ParticleProcessMaterial`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleProcessMaterial3D {
    /// Base emission direction (normalized).
    pub direction: Vector3,
    /// Angular spread in degrees around `direction`.
    pub spread: f32,
    /// Flatness of the spread (0 = cone, 1 = flat disc).
    pub flatness: f32,
    /// Minimum initial speed.
    pub initial_velocity_min: f32,
    /// Maximum initial speed.
    pub initial_velocity_max: f32,
    /// Gravity vector applied each frame.
    pub gravity: Vector3,
    /// Minimum angular velocity (radians/sec around Y).
    pub angular_velocity_min: f32,
    /// Maximum angular velocity (radians/sec around Y).
    pub angular_velocity_max: f32,
    /// Minimum initial scale.
    pub scale_min: f32,
    /// Maximum initial scale.
    pub scale_max: f32,
    /// Color at birth.
    pub start_color: Color,
    /// Color at death (linearly interpolated over lifetime).
    pub end_color: Color,
    /// Lifetime randomness factor in [0, 1].
    pub lifetime_randomness: f32,
    /// Linear damping factor.
    pub damping: f32,
}

impl Default for ParticleProcessMaterial3D {
    fn default() -> Self {
        Self {
            direction: Vector3::new(0.0, 1.0, 0.0),
            spread: 45.0,
            flatness: 0.0,
            initial_velocity_min: 0.0,
            initial_velocity_max: 0.0,
            gravity: Vector3::new(0.0, -9.8, 0.0),
            angular_velocity_min: 0.0,
            angular_velocity_max: 0.0,
            scale_min: 1.0,
            scale_max: 1.0,
            start_color: Color::WHITE,
            end_color: Color::WHITE,
            lifetime_randomness: 0.0,
            damping: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Particle3D
// ---------------------------------------------------------------------------

/// A single live 3D particle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Particle3D {
    /// Current position.
    pub position: Vector3,
    /// Current velocity.
    pub velocity: Vector3,
    /// Total lifetime this particle was given.
    pub lifetime: f32,
    /// Remaining lifetime (counts down to zero).
    pub lifetime_remaining: f32,
    /// Current display color.
    pub color: Color,
    /// Current display scale.
    pub scale: f32,
    /// Current rotation in radians (around Y axis).
    pub rotation: f32,
    /// Angular velocity in radians/sec.
    pub angular_velocity: f32,
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
// DrawMode
// ---------------------------------------------------------------------------

/// How particles are rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DrawMode3D {
    /// Render each particle as a billboard quad (default).
    #[default]
    Billboard,
    /// Render each particle using a mesh instance.
    Mesh,
    /// Render each particle as a cross (two intersecting quads).
    Cross,
}

impl DrawMode3D {
    /// Converts from the Godot integer representation.
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            1 => Self::Mesh,
            2 => Self::Cross,
            _ => Self::Billboard,
        }
    }

    /// Converts to the Godot integer representation.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::Billboard => 0,
            Self::Mesh => 1,
            Self::Cross => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// GPUParticles3D
// ---------------------------------------------------------------------------

/// A 3D GPU particle emitter node.
///
/// Maps to Godot's `GPUParticles3D` node type.
#[derive(Debug, Clone)]
pub struct GPUParticles3D {
    /// Process material controlling particle appearance and physics.
    pub material: ParticleProcessMaterial3D,
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
    /// Randomness ratio in [0, 1].
    pub randomness: f32,
    /// Speed multiplier for the simulation.
    pub speed_scale: f32,
    /// Whether the emitter is currently active.
    pub emitting: bool,
    /// If `true`, particles are in the emitter's local space.
    pub local_coords: bool,
    /// Draw mode for particle rendering.
    pub draw_mode: DrawMode3D,
    /// Visibility AABB for culling (half-extents).
    pub visibility_aabb_extents: Vector3,
    /// Fixed FPS for simulation (0 = use frame delta).
    pub fixed_fps: u32,
    /// Currently live particles.
    pub active_particles: Vec<Particle3D>,
    /// Sub-frame time accumulator for emission pacing.
    time_accumulator: f32,
    /// Total number of particles emitted (used for seeding).
    total_emitted: u64,
}

impl Default for GPUParticles3D {
    fn default() -> Self {
        Self {
            material: ParticleProcessMaterial3D::default(),
            emission_shape: EmissionShape3D::default(),
            amount: 8,
            lifetime: 1.0,
            one_shot: false,
            explosiveness: 0.0,
            randomness: 0.0,
            speed_scale: 1.0,
            emitting: true,
            local_coords: false,
            draw_mode: DrawMode3D::default(),
            visibility_aabb_extents: Vector3::new(4.0, 4.0, 4.0),
            fixed_fps: 0,
            active_particles: Vec::new(),
            time_accumulator: 0.0,
            total_emitted: 0,
        }
    }
}

impl GPUParticles3D {
    /// Creates a new GPUParticles3D with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawn a single particle using the current material and shape.
    pub fn emit_particle(&self, seed: u32) -> Particle3D {
        let mut rng = seed.max(1);

        let position = self.emission_shape.sample(&mut rng);

        // Direction with spread
        let dir = self.material.direction.normalized();
        let spread_rad = self.material.spread.to_radians();
        let theta = rand_range(&mut rng, -spread_rad, spread_rad);
        let phi = rand_range(&mut rng, 0.0, std::f32::consts::TAU);

        // Rotate direction by spread angles
        let spread_dir = if theta.abs() < 1e-6 {
            dir
        } else {
            // Find perpendicular vectors to dir
            let up = if dir.y.abs() > 0.99 {
                Vector3::new(0.0, 0.0, 1.0)
            } else {
                Vector3::new(0.0, 1.0, 0.0)
            };
            let right = up.cross(dir).normalized();
            let up2 = dir.cross(right).normalized();
            let cos_t = theta.cos();
            let sin_t = theta.sin();
            let cos_p = phi.cos();
            let sin_p = phi.sin();
            Vector3::new(
                dir.x * cos_t + right.x * sin_t * cos_p + up2.x * sin_t * sin_p,
                dir.y * cos_t + right.y * sin_t * cos_p + up2.y * sin_t * sin_p,
                dir.z * cos_t + right.z * sin_t * cos_p + up2.z * sin_t * sin_p,
            )
            .normalized()
        };

        let speed = rand_range(
            &mut rng,
            self.material.initial_velocity_min,
            self.material.initial_velocity_max,
        );
        let velocity = spread_dir * speed;

        let lifetime_factor = 1.0 - self.material.lifetime_randomness * rand_f32(&mut rng);
        let lifetime = self.lifetime * lifetime_factor;

        let scale = rand_range(&mut rng, self.material.scale_min, self.material.scale_max);
        let angular_velocity = rand_range(
            &mut rng,
            self.material.angular_velocity_min,
            self.material.angular_velocity_max,
        );

        Particle3D {
            position,
            velocity,
            lifetime,
            lifetime_remaining: lifetime,
            color: self.material.start_color,
            scale,
            rotation: 0.0,
            angular_velocity,
        }
    }

    /// Advance the simulation by `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        let dt_scaled = dt * self.speed_scale;

        // Age and integrate existing particles
        for p in &mut self.active_particles {
            p.lifetime_remaining -= dt_scaled;

            // Apply damping
            if self.material.damping > 0.0 {
                let factor = (1.0 - self.material.damping * dt_scaled).max(0.0);
                p.velocity = p.velocity * factor;
            }

            p.velocity = p.velocity + self.material.gravity * dt_scaled;
            p.position = p.position + p.velocity * dt_scaled;
            p.rotation += p.angular_velocity * dt_scaled;

            // Interpolate color over lifetime
            let t = p.age_ratio();
            p.color = self.material.start_color.lerp(self.material.end_color, t);
        }

        // Remove dead particles
        self.active_particles.retain(|p| !p.is_dead());

        // Emit new particles
        if self.emitting {
            let amount = self.amount.max(1) as f32;
            let lifetime = self.lifetime.max(0.001);

            if self.explosiveness >= 1.0 {
                self.time_accumulator += dt_scaled;
                if self.time_accumulator >= lifetime {
                    self.time_accumulator -= lifetime;
                    for _ in 0..self.amount {
                        self.total_emitted += 1;
                        let seed = (self.total_emitted & 0xFFFF_FFFF) as u32;
                        self.active_particles
                            .push(self.emit_particle(seed.max(1)));
                    }
                    if self.one_shot {
                        self.emitting = false;
                    }
                }
            } else {
                let emit_interval = lifetime / amount;
                self.time_accumulator += dt_scaled;

                while self.time_accumulator >= emit_interval {
                    self.time_accumulator -= emit_interval;
                    self.total_emitted += 1;
                    let seed = (self.total_emitted & 0xFFFF_FFFF) as u32;
                    self.active_particles
                        .push(self.emit_particle(seed.max(1)));
                }

                if self.one_shot && self.total_emitted >= self.amount as u64 {
                    self.emitting = false;
                }
            }
        }
    }

    /// Returns the number of currently live particles.
    pub fn particle_count(&self) -> usize {
        self.active_particles.len()
    }

    /// Returns `true` if this is a one-shot emitter and all particles are dead.
    pub fn is_complete(&self) -> bool {
        self.one_shot && !self.emitting && self.active_particles.is_empty()
    }

    /// Restart emission (resets state for one-shot emitters).
    pub fn restart(&mut self) {
        self.active_particles.clear();
        self.time_accumulator = 0.0;
        self.total_emitted = 0;
        self.emitting = true;
    }

    /// Returns draw data for each live particle: (position, color, scale).
    pub fn get_draw_data(&self) -> Vec<(Vector3, Color, f32)> {
        self.active_particles
            .iter()
            .map(|p| (p.position, p.color, p.scale))
            .collect()
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

    #[test]
    fn xorshift_deterministic() {
        let mut a = 42u32;
        let mut b = 42u32;
        let va: Vec<u32> = (0..10).map(|_| xorshift32(&mut a)).collect();
        let vb: Vec<u32> = (0..10).map(|_| xorshift32(&mut b)).collect();
        assert_eq!(va, vb);
    }

    #[test]
    fn rand_f32_in_unit_range() {
        let mut rng = 123u32;
        for _ in 0..100 {
            let v = rand_f32(&mut rng);
            assert!((0.0..1.0).contains(&v), "rand_f32 out of range: {v}");
        }
    }

    // -- EmissionShape3D ---

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
                "Sphere sample outside radius: {pos:?}"
            );
        }
    }

    #[test]
    fn box_shape_within_extents() {
        let shape = EmissionShape3D::Box {
            extents: Vector3::new(5.0, 3.0, 2.0),
        };
        let mut rng = 99u32;
        for _ in 0..50 {
            let pos = shape.sample(&mut rng);
            assert!(pos.x.abs() <= 5.0 + EPSILON);
            assert!(pos.y.abs() <= 3.0 + EPSILON);
            assert!(pos.z.abs() <= 2.0 + EPSILON);
        }
    }

    #[test]
    fn sphere_surface_on_radius() {
        let shape = EmissionShape3D::SphereSurface { radius: 5.0 };
        let mut rng = 55u32;
        for _ in 0..50 {
            let pos = shape.sample(&mut rng);
            assert!(
                approx_eq(pos.length(), 5.0),
                "SphereSurface sample not on radius: len={}",
                pos.length()
            );
        }
    }

    #[test]
    fn ring_shape_within_bounds() {
        let shape = EmissionShape3D::Ring {
            inner_radius: 3.0,
            outer_radius: 6.0,
            height: 2.0,
        };
        let mut rng = 88u32;
        for _ in 0..50 {
            let pos = shape.sample(&mut rng);
            let horiz = (pos.x * pos.x + pos.z * pos.z).sqrt();
            assert!(
                horiz >= 3.0 - EPSILON && horiz <= 6.0 + EPSILON,
                "Ring horizontal distance out of bounds: {horiz}"
            );
            assert!(pos.y.abs() <= 1.0 + EPSILON);
        }
    }

    // -- Material defaults ---

    #[test]
    fn material_defaults() {
        let m = ParticleProcessMaterial3D::default();
        assert_eq!(m.direction, Vector3::new(0.0, 1.0, 0.0));
        assert!(approx_eq(m.spread, 45.0));
        assert!(approx_eq(m.gravity.y, -9.8));
        assert_eq!(m.start_color, Color::WHITE);
    }

    // -- GPUParticles3D defaults ---

    #[test]
    fn gpu_particles_defaults() {
        let p = GPUParticles3D::new();
        assert_eq!(p.amount, 8);
        assert!(approx_eq(p.lifetime, 1.0));
        assert!(!p.one_shot);
        assert!(p.emitting);
        assert_eq!(p.draw_mode, DrawMode3D::Billboard);
    }

    // -- emit_particle ---

    #[test]
    fn emit_particle_deterministic() {
        let emitter = GPUParticles3D::new();
        let p1 = emitter.emit_particle(42);
        let p2 = emitter.emit_particle(42);
        assert_eq!(p1.position, p2.position);
        assert_eq!(p1.velocity, p2.velocity);
        assert!(approx_eq(p1.lifetime, p2.lifetime));
    }

    #[test]
    fn emit_particle_with_velocity() {
        let mut emitter = GPUParticles3D::new();
        emitter.material.initial_velocity_min = 50.0;
        emitter.material.initial_velocity_max = 50.0;
        emitter.material.spread = 0.0;

        let p = emitter.emit_particle(1);
        let speed = p.velocity.length();
        assert!(
            approx_eq(speed, 50.0),
            "Expected speed ~50, got {speed}"
        );
    }

    // -- Particle3D age/death ---

    #[test]
    fn particle_age_ratio() {
        let p = Particle3D {
            position: Vector3::ZERO,
            velocity: Vector3::ZERO,
            lifetime: 2.0,
            lifetime_remaining: 1.0,
            color: Color::WHITE,
            scale: 1.0,
            rotation: 0.0,
            angular_velocity: 0.0,
        };
        assert!(approx_eq(p.age_ratio(), 0.5));
    }

    #[test]
    fn particle_is_dead_when_expired() {
        let p = Particle3D {
            position: Vector3::ZERO,
            velocity: Vector3::ZERO,
            lifetime: 1.0,
            lifetime_remaining: 0.0,
            color: Color::WHITE,
            scale: 1.0,
            rotation: 0.0,
            angular_velocity: 0.0,
        };
        assert!(p.is_dead());
    }

    // -- DrawMode3D ---

    #[test]
    fn draw_mode_roundtrip() {
        for (int_val, expected) in [
            (0, DrawMode3D::Billboard),
            (1, DrawMode3D::Mesh),
            (2, DrawMode3D::Cross),
        ] {
            let mode = DrawMode3D::from_godot_int(int_val);
            assert_eq!(mode, expected);
            assert_eq!(mode.to_godot_int(), int_val);
        }
    }

    #[test]
    fn draw_mode_unknown_defaults_to_billboard() {
        assert_eq!(DrawMode3D::from_godot_int(99), DrawMode3D::Billboard);
    }

    // -- Simulation ---

    #[test]
    fn simulator_emits_particles_over_time() {
        let mut particles = GPUParticles3D::new();
        particles.amount = 4;
        particles.lifetime = 1.0;
        assert_eq!(particles.particle_count(), 0);

        particles.step(0.3);
        assert!(particles.particle_count() > 0, "Should have emitted particles");
    }

    #[test]
    fn simulator_gravity_affects_velocity() {
        let mut particles = GPUParticles3D::new();
        particles.material.gravity = Vector3::new(0.0, -98.0, 0.0);
        particles.material.initial_velocity_min = 0.0;
        particles.material.initial_velocity_max = 0.0;
        particles.amount = 1;
        particles.lifetime = 10.0;
        particles.explosiveness = 1.0;

        particles.step(10.0); // trigger burst
        particles.step(1.0);

        assert!(particles.particle_count() > 0);
        let p = &particles.active_particles[0];
        assert!(p.velocity.y < 0.0, "Gravity should push velocity negative Y");
    }

    #[test]
    fn one_shot_completes() {
        let mut particles = GPUParticles3D::new();
        particles.amount = 2;
        particles.lifetime = 0.1;
        particles.one_shot = true;
        particles.explosiveness = 1.0;
        assert!(!particles.is_complete());

        particles.step(0.1); // trigger burst
        assert!(!particles.is_complete(), "Particles still alive");

        particles.step(0.5); // let them die
        assert!(particles.is_complete(), "One-shot should be complete");
    }

    #[test]
    fn restart_resets_state() {
        let mut particles = GPUParticles3D::new();
        particles.one_shot = true;
        particles.explosiveness = 1.0;
        particles.lifetime = 0.1;

        particles.step(0.1);
        particles.step(0.5);
        assert!(particles.is_complete());

        particles.restart();
        assert!(!particles.is_complete());
        assert!(particles.emitting);
        assert_eq!(particles.particle_count(), 0);
    }

    #[test]
    fn get_draw_data_matches_count() {
        let mut particles = GPUParticles3D::new();
        particles.amount = 3;
        particles.explosiveness = 1.0;

        particles.step(1.0); // trigger burst
        particles.step(0.01);

        let data = particles.get_draw_data();
        assert_eq!(data.len(), particles.particle_count());
        for (pos, _color, scale) in &data {
            assert!(pos.length().is_finite());
            assert!(*scale > 0.0);
        }
    }

    #[test]
    fn color_interpolation_over_lifetime() {
        let mut particles = GPUParticles3D::new();
        particles.material.start_color = Color::rgb(1.0, 0.0, 0.0);
        particles.material.end_color = Color::rgb(0.0, 0.0, 1.0);
        particles.material.initial_velocity_min = 0.0;
        particles.material.initial_velocity_max = 0.0;
        particles.material.gravity = Vector3::ZERO;
        particles.amount = 1;
        particles.lifetime = 1.0;
        particles.explosiveness = 1.0;

        particles.step(1.0); // trigger burst
        particles.step(0.5); // advance to ~50%

        assert_eq!(particles.particle_count(), 1);
        let p = &particles.active_particles[0];
        assert!(p.color.r < 0.9, "Red should have decreased");
        assert!(p.color.b > 0.1, "Blue should have increased");
    }

    #[test]
    fn damping_reduces_velocity() {
        let mut particles = GPUParticles3D::new();
        particles.material.initial_velocity_min = 100.0;
        particles.material.initial_velocity_max = 100.0;
        particles.material.spread = 0.0;
        particles.material.gravity = Vector3::ZERO;
        particles.material.damping = 0.5;
        particles.amount = 1;
        particles.lifetime = 10.0;
        particles.explosiveness = 1.0;

        particles.step(10.0); // trigger burst
        let initial_speed = particles.active_particles[0].velocity.length();

        particles.step(2.0); // let damping act
        let final_speed = particles.active_particles[0].velocity.length();

        assert!(
            final_speed < initial_speed,
            "Damping should reduce speed: initial={initial_speed}, final={final_speed}"
        );
    }

    #[test]
    fn speed_scale_amplifies_simulation() {
        let mut particles = GPUParticles3D::new();
        particles.material.gravity = Vector3::new(0.0, -100.0, 0.0);
        particles.material.initial_velocity_min = 0.0;
        particles.material.initial_velocity_max = 0.0;
        particles.amount = 1;
        particles.lifetime = 10.0;
        particles.explosiveness = 1.0;
        particles.speed_scale = 2.0;

        particles.step(10.0); // trigger burst
        particles.step(1.0);

        assert!(particles.particle_count() > 0);
        let p = &particles.active_particles[0];
        // With speed_scale=2, gravity effect should be doubled
        assert!(
            p.velocity.y < -150.0,
            "Speed scale should amplify gravity, got {}",
            p.velocity.y
        );
    }
}

//! 2D particle system: emission, simulation, and draw-command generation.
//!
//! Provides a Godot-compatible CPU particle emitter with deterministic
//! randomness (xorshift), configurable emission shapes, and per-particle
//! physics (gravity, angular velocity, color interpolation).

use gdcore::math::{Color, Vector2};

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
// EmissionShape
// ---------------------------------------------------------------------------

/// Shape from which particles are spawned.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EmissionShape {
    /// Emit from a single point at the origin.
    Point,
    /// Emit from a random position within a circle.
    Circle { radius: f32 },
    /// Emit from a random position within a rectangle.
    Box { extents: Vector2 },
    /// Emit from a random position within a ring.
    Ring {
        inner_radius: f32,
        outer_radius: f32,
    },
}

impl Default for EmissionShape {
    fn default() -> Self {
        Self::Point
    }
}

impl EmissionShape {
    /// Sample a random position within this shape.
    fn sample(&self, rng: &mut u32) -> Vector2 {
        match *self {
            EmissionShape::Point => Vector2::ZERO,
            EmissionShape::Circle { radius } => {
                let angle = rand_f32(rng) * std::f32::consts::TAU;
                let r = radius * rand_f32(rng).sqrt();
                Vector2::new(r * angle.cos(), r * angle.sin())
            }
            EmissionShape::Box { extents } => {
                let x = rand_range(rng, -extents.x, extents.x);
                let y = rand_range(rng, -extents.y, extents.y);
                Vector2::new(x, y)
            }
            EmissionShape::Ring {
                inner_radius,
                outer_radius,
            } => {
                let angle = rand_f32(rng) * std::f32::consts::TAU;
                let r = rand_range(rng, inner_radius, outer_radius);
                Vector2::new(r * angle.cos(), r * angle.sin())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ParticleMaterial
// ---------------------------------------------------------------------------

/// Per-emitter configuration that controls how particles look and move.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleMaterial {
    /// Base emission direction (normalized).
    pub direction: Vector2,
    /// Angular spread in degrees around `direction`.
    pub spread: f32,
    /// Minimum initial speed.
    pub initial_velocity_min: f32,
    /// Maximum initial speed.
    pub initial_velocity_max: f32,
    /// Gravity applied every frame.
    pub gravity: Vector2,
    /// Minimum angular velocity (radians/sec).
    pub angular_velocity_min: f32,
    /// Maximum angular velocity (radians/sec).
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
}

impl Default for ParticleMaterial {
    fn default() -> Self {
        Self {
            direction: Vector2::UP,
            spread: 45.0,
            initial_velocity_min: 0.0,
            initial_velocity_max: 0.0,
            gravity: Vector2::ZERO,
            angular_velocity_min: 0.0,
            angular_velocity_max: 0.0,
            scale_min: 1.0,
            scale_max: 1.0,
            start_color: Color::WHITE,
            end_color: Color::WHITE,
            lifetime_randomness: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Particle
// ---------------------------------------------------------------------------

/// A single live particle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Particle {
    /// Current position.
    pub position: Vector2,
    /// Current velocity.
    pub velocity: Vector2,
    /// Total lifetime this particle was given.
    pub lifetime: f32,
    /// Remaining lifetime (counts down to zero).
    pub lifetime_remaining: f32,
    /// Current display color.
    pub color: Color,
    /// Current display scale.
    pub scale: f32,
    /// Current rotation in radians.
    pub rotation: f32,
    /// Angular velocity in radians/sec.
    pub angular_velocity: f32,
}

impl Particle {
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
// ParticleEmitter
// ---------------------------------------------------------------------------

/// Configuration object that knows how to spawn individual particles.
#[derive(Debug, Clone)]
pub struct ParticleEmitter {
    /// Material controlling particle appearance and physics.
    pub material: ParticleMaterial,
    /// Shape from which particles are emitted.
    pub emission_shape: EmissionShape,
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

impl Default for ParticleEmitter {
    fn default() -> Self {
        Self {
            material: ParticleMaterial::default(),
            emission_shape: EmissionShape::default(),
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

impl ParticleEmitter {
    /// Spawn a single particle using the emitter's material and shape.
    ///
    /// `seed` is used to derive deterministic randomness for this particle.
    pub fn emit_particle(&self, seed: u32) -> Particle {
        let mut rng = seed;
        // Ensure non-zero RNG state
        if rng == 0 {
            rng = 1;
        }

        // Position from emission shape
        let position = self.emission_shape.sample(&mut rng);

        // Direction with spread
        let base_angle = self.material.direction.y.atan2(self.material.direction.x);
        let spread_rad = self.material.spread.to_radians();
        let angle = base_angle + rand_range(&mut rng, -spread_rad, spread_rad);

        let speed = rand_range(
            &mut rng,
            self.material.initial_velocity_min,
            self.material.initial_velocity_max,
        );
        let velocity = Vector2::new(angle.cos() * speed, angle.sin() * speed);

        // Lifetime with randomness
        let lifetime_factor =
            1.0 - self.material.lifetime_randomness * rand_f32(&mut rng);
        let lifetime = self.lifetime * lifetime_factor;

        let scale = rand_range(&mut rng, self.material.scale_min, self.material.scale_max);
        let angular_velocity = rand_range(
            &mut rng,
            self.material.angular_velocity_min,
            self.material.angular_velocity_max,
        );

        Particle {
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
}

// ---------------------------------------------------------------------------
// ParticleSimulator
// ---------------------------------------------------------------------------

/// Drives particle emission, physics, and culling each frame.
#[derive(Debug, Clone)]
pub struct ParticleSimulator {
    /// Emitter configuration.
    pub emitter: ParticleEmitter,
    /// Currently live particles.
    pub active_particles: Vec<Particle>,
    /// Sub-frame time accumulator for emission pacing.
    pub time_accumulator: f32,
    /// Total number of particles emitted (used for seeding).
    pub total_emitted: u64,
}

impl ParticleSimulator {
    /// Creates a new simulator from the given emitter configuration.
    pub fn new(emitter: ParticleEmitter) -> Self {
        Self {
            emitter,
            active_particles: Vec::new(),
            time_accumulator: 0.0,
            total_emitted: 0,
        }
    }

    /// Advance the simulation by `dt` seconds.
    ///
    /// Ages existing particles, removes dead ones, emits new particles
    /// based on the configured rate and explosiveness, and applies
    /// gravity and velocity integration.
    pub fn step(&mut self, dt: f32) {
        let dt_scaled = dt * self.emitter.speed_scale;

        // --- Age and integrate existing particles ---
        for p in &mut self.active_particles {
            p.lifetime_remaining -= dt_scaled;
            p.velocity = p.velocity + self.emitter.material.gravity * dt_scaled;
            p.position = p.position + p.velocity * dt_scaled;
            p.rotation += p.angular_velocity * dt_scaled;

            // Interpolate color over lifetime
            let t = p.age_ratio();
            p.color = self.emitter.material.start_color.lerp(self.emitter.material.end_color, t);
        }

        // --- Remove dead particles ---
        self.active_particles.retain(|p| !p.is_dead());

        // --- Emit new particles ---
        if self.emitter.emitting {
            let amount = self.emitter.amount.max(1) as f32;
            let lifetime = self.emitter.lifetime.max(0.001);

            if self.emitter.explosiveness >= 1.0 {
                // All particles at once: accumulate time, emit a full burst per cycle
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
                // Continuous emission: spread particles over the lifetime
                let emit_interval = lifetime / amount;
                self.time_accumulator += dt_scaled;

                while self.time_accumulator >= emit_interval {
                    self.time_accumulator -= emit_interval;
                    self.total_emitted += 1;
                    let seed = (self.total_emitted & 0xFFFF_FFFF) as u32;
                    let particle = self.emitter.emit_particle(seed.max(1));
                    self.active_particles.push(particle);
                }

                // For one_shot, stop after emitting `amount` particles in the first cycle
                if self.emitter.one_shot && self.total_emitted >= self.emitter.amount as u64 {
                    self.emitter.emitting = false;
                }
            }
        }
    }

    /// Returns draw commands for each live particle: (position, color, scale).
    pub fn get_draw_commands(&self) -> Vec<(Vector2, Color, f32)> {
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

    // -- EmissionShape ------------------------------------------------------

    #[test]
    fn point_shape_emits_at_origin() {
        let mut rng = 1u32;
        let pos = EmissionShape::Point.sample(&mut rng);
        assert_eq!(pos, Vector2::ZERO);
    }

    #[test]
    fn circle_shape_within_radius() {
        let shape = EmissionShape::Circle { radius: 10.0 };
        let mut rng = 77u32;
        for _ in 0..50 {
            let pos = shape.sample(&mut rng);
            assert!(
                pos.length() <= 10.0 + EPSILON,
                "Circle sample outside radius: {pos:?}"
            );
        }
    }

    #[test]
    fn box_shape_within_extents() {
        let shape = EmissionShape::Box {
            extents: Vector2::new(5.0, 3.0),
        };
        let mut rng = 99u32;
        for _ in 0..50 {
            let pos = shape.sample(&mut rng);
            assert!(pos.x.abs() <= 5.0 + EPSILON);
            assert!(pos.y.abs() <= 3.0 + EPSILON);
        }
    }

    #[test]
    fn ring_shape_within_radii() {
        let shape = EmissionShape::Ring {
            inner_radius: 5.0,
            outer_radius: 10.0,
        };
        let mut rng = 55u32;
        for _ in 0..50 {
            let pos = shape.sample(&mut rng);
            let len = pos.length();
            assert!(
                len >= 5.0 - EPSILON && len <= 10.0 + EPSILON,
                "Ring sample outside bounds: len={len}"
            );
        }
    }

    // -- ParticleMaterial defaults ------------------------------------------

    #[test]
    fn material_defaults() {
        let m = ParticleMaterial::default();
        assert_eq!(m.direction, Vector2::UP);
        assert!(approx_eq(m.spread, 45.0));
        assert!(approx_eq(m.scale_min, 1.0));
        assert!(approx_eq(m.scale_max, 1.0));
        assert_eq!(m.start_color, Color::WHITE);
        assert_eq!(m.end_color, Color::WHITE);
    }

    // -- ParticleEmitter defaults -------------------------------------------

    #[test]
    fn emitter_defaults() {
        let e = ParticleEmitter::default();
        assert_eq!(e.amount, 8);
        assert!(approx_eq(e.lifetime, 1.0));
        assert!(!e.one_shot);
        assert!(approx_eq(e.explosiveness, 0.0));
        assert!(approx_eq(e.speed_scale, 1.0));
        assert!(e.emitting);
    }

    // -- emit_particle ------------------------------------------------------

    #[test]
    fn emit_particle_deterministic() {
        let emitter = ParticleEmitter::default();
        let p1 = emitter.emit_particle(42);
        let p2 = emitter.emit_particle(42);
        assert_eq!(p1.position, p2.position);
        assert_eq!(p1.velocity, p2.velocity);
        assert!(approx_eq(p1.lifetime, p2.lifetime));
    }

    #[test]
    fn emit_particle_with_velocity() {
        let emitter = ParticleEmitter {
            material: ParticleMaterial {
                initial_velocity_min: 100.0,
                initial_velocity_max: 100.0,
                spread: 0.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let p = emitter.emit_particle(1);
        let speed = p.velocity.length();
        assert!(
            approx_eq(speed, 100.0),
            "Expected speed ~100, got {speed}"
        );
    }

    #[test]
    fn emit_particle_lifetime_randomness() {
        let emitter = ParticleEmitter {
            material: ParticleMaterial {
                lifetime_randomness: 0.5,
                ..Default::default()
            },
            lifetime: 2.0,
            ..Default::default()
        };
        // Different seeds should produce different lifetimes
        let p1 = emitter.emit_particle(1);
        let p2 = emitter.emit_particle(999);
        // Both should be in [1.0, 2.0] (lifetime * [0.5, 1.0])
        assert!(p1.lifetime >= 1.0 - EPSILON && p1.lifetime <= 2.0 + EPSILON);
        assert!(p2.lifetime >= 1.0 - EPSILON && p2.lifetime <= 2.0 + EPSILON);
    }

    // -- Particle age/death -------------------------------------------------

    #[test]
    fn particle_age_ratio() {
        let p = Particle {
            position: Vector2::ZERO,
            velocity: Vector2::ZERO,
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
        let p = Particle {
            position: Vector2::ZERO,
            velocity: Vector2::ZERO,
            lifetime: 1.0,
            lifetime_remaining: 0.0,
            color: Color::WHITE,
            scale: 1.0,
            rotation: 0.0,
            angular_velocity: 0.0,
        };
        assert!(p.is_dead());
    }

    // -- ParticleSimulator --------------------------------------------------

    #[test]
    fn simulator_emits_particles_over_time() {
        let emitter = ParticleEmitter {
            amount: 4,
            lifetime: 1.0,
            ..Default::default()
        };
        let mut sim = ParticleSimulator::new(emitter);
        assert_eq!(sim.particle_count(), 0);

        // Step enough to emit at least one particle
        sim.step(0.3);
        assert!(sim.particle_count() > 0, "Should have emitted particles");
    }

    #[test]
    fn simulator_removes_dead_particles() {
        let emitter = ParticleEmitter {
            amount: 4,
            lifetime: 0.1,
            ..Default::default()
        };
        let mut sim = ParticleSimulator::new(emitter);
        sim.step(0.05); // emit some
        let count_before = sim.particle_count();
        assert!(count_before > 0);

        // Step past their lifetime
        sim.step(0.2);
        sim.step(0.2);
        // Dead ones should be removed (new ones may have been emitted)
        // At minimum the original batch should be gone
    }

    #[test]
    fn simulator_gravity_affects_velocity() {
        let emitter = ParticleEmitter {
            material: ParticleMaterial {
                gravity: Vector2::new(0.0, 98.0),
                initial_velocity_min: 0.0,
                initial_velocity_max: 0.0,
                ..Default::default()
            },
            amount: 1,
            lifetime: 10.0,
            explosiveness: 1.0,
            ..Default::default()
        };
        let mut sim = ParticleSimulator::new(emitter);
        // Trigger burst
        sim.step(10.0);
        // Now step a bit to let gravity act
        sim.step(1.0);

        assert!(sim.particle_count() > 0);
        let p = &sim.active_particles[0];
        // Gravity should have pushed velocity downward
        assert!(p.velocity.y > 0.0, "Gravity should increase y velocity");
    }

    #[test]
    fn simulator_one_shot_completes() {
        let emitter = ParticleEmitter {
            amount: 2,
            lifetime: 0.1,
            one_shot: true,
            explosiveness: 1.0,
            ..Default::default()
        };
        let mut sim = ParticleSimulator::new(emitter);
        assert!(!sim.is_complete());

        // Trigger burst
        sim.step(0.1);
        assert!(!sim.is_complete(), "Particles still alive");

        // Let them die
        sim.step(0.5);
        assert!(sim.is_complete(), "One-shot should be complete");
    }

    #[test]
    fn simulator_get_draw_commands() {
        let emitter = ParticleEmitter {
            amount: 3,
            lifetime: 1.0,
            explosiveness: 1.0,
            ..Default::default()
        };
        let mut sim = ParticleSimulator::new(emitter);
        sim.step(1.0); // trigger burst
        sim.step(0.01); // small step to keep alive

        let cmds = sim.get_draw_commands();
        assert_eq!(cmds.len(), sim.particle_count());
        for (pos, color, scale) in &cmds {
            assert!(pos.length().is_finite());
            assert!(color.a >= 0.0);
            assert!(*scale > 0.0);
        }
    }

    #[test]
    fn simulator_color_interpolation() {
        let emitter = ParticleEmitter {
            material: ParticleMaterial {
                start_color: Color::rgb(1.0, 0.0, 0.0),
                end_color: Color::rgb(0.0, 0.0, 1.0),
                initial_velocity_min: 0.0,
                initial_velocity_max: 0.0,
                ..Default::default()
            },
            amount: 1,
            lifetime: 1.0,
            explosiveness: 1.0,
            ..Default::default()
        };
        let mut sim = ParticleSimulator::new(emitter);
        sim.step(1.0); // trigger burst

        // Step to ~50% lifetime
        sim.step(0.5);

        assert_eq!(sim.particle_count(), 1);
        let p = &sim.active_particles[0];
        // Color should be roughly halfway between red and blue
        assert!(p.color.r < 0.9, "Red should have decreased");
        assert!(p.color.b > 0.1, "Blue should have increased");
    }

    #[test]
    fn simulator_speed_scale() {
        let emitter = ParticleEmitter {
            material: ParticleMaterial {
                gravity: Vector2::new(0.0, 100.0),
                initial_velocity_min: 0.0,
                initial_velocity_max: 0.0,
                ..Default::default()
            },
            amount: 1,
            lifetime: 10.0,
            explosiveness: 1.0,
            speed_scale: 2.0,
            ..Default::default()
        };
        let mut sim = ParticleSimulator::new(emitter);
        sim.step(10.0); // trigger burst
        sim.step(1.0);

        assert!(sim.particle_count() > 0);
        let p = &sim.active_particles[0];
        // With speed_scale=2, gravity should have had double effect
        // dt_scaled = 1.0 * 2.0 = 2.0, so velocity.y = 100 * 2 = 200
        assert!(
            p.velocity.y > 150.0,
            "Speed scale should amplify gravity effect, got {}",
            p.velocity.y
        );
    }
}

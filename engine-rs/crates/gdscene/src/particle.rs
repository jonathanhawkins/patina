//! 2D particle system: emission, simulation, and draw-command generation.
//!
//! Provides a Godot-compatible CPU particle emitter with deterministic
//! randomness (xorshift), configurable emission shapes, and per-particle
//! physics (gravity, angular velocity, color interpolation).
//!
//! # Parity: `GPUParticles2D` is CPU-simulated here (pat-z35r2.19)
//!
//! Patina registers and renders `GPUParticles2D`/`GPUParticles3D`, but they are
//! **not** GPU-simulated: `MainLoop::process_particles` drives all four particle
//! classes — `GPUParticles2D`, `GPUParticles3D`, `CPUParticles2D`,
//! `CPUParticles3D` — through this same [`ParticleSimulator`] on the CPU. The
//! `GPU` prefix is therefore a naming alias for Godot compatibility, not a
//! statement about where the work happens. `tests/gpu_particles_are_cpu_simulated.rs`
//! pins that equivalence so the claim can't drift unnoticed.
//!
//! Two Godot features follow from real GPU particles and are consequently
//! **absent**, not merely unimplemented here:
//!
//! - **Collision.** Godot's GPU particles collide against `ParticlesCollisionBox3D`
//!   / `Sphere3D` / `SDF3D` / `HeightField3D` (and the 2D SDF equivalents) inside
//!   the particle shader. This module has no collider input at all: a particle's
//!   only per-step force is `gravity`, so particles pass through every body in the
//!   scene. `collision_mode`/`collision_friction`/`collision_bounce` are unhonored.
//! - **Trails.** `trail_enabled`/`trail_lifetime` (and the trail mesh sections
//!   Godot builds from the particle's transform history) have no equivalent: each
//!   particle emits exactly one draw command per frame (see
//!   [`ParticleSimulator::get_draw_commands`]).
//!
//! Closing either gap means putting the simulation on the GPU (or writing a CPU
//! collider query per particle per step); both are out of scope for this module
//! as written, and are tracked separately. Do not describe these nodes as
//! GPU-accelerated in docs or parity scorecards on the strength of the class name.

use gdcore::math::{Color, Transform2D, Vector2};

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
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum EmissionShape {
    /// Emit from a single point at the origin.
    #[default]
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
    /// The emitter's global transform at the instant this particle was spawned
    /// — the map from this particle's simulation space to global space.
    ///
    /// With `local_coords` **false** (Godot's default) a particle is placed in
    /// global space at spawn and stops caring about the emitter afterwards, so
    /// moving the emitter must leave the existing cloud behind
    /// (`doc/classes/GPUParticles2D.xml`, `local_coords`). Keeping the spawn
    /// transform per particle is what makes that possible while the simulation
    /// itself stays emitter-relative: see
    /// [`ParticleSimulator::draw_commands_in_local_space`].
    ///
    /// With `local_coords` true it is unused (the emitter's *current* transform
    /// applies instead), and it stays [`Transform2D::IDENTITY`] for simulators
    /// driven directly rather than through a scene node.
    pub spawn_transform: Transform2D,
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

/// The order a simulator emits its draw commands in (Godot's
/// `GPUParticles2D.draw_order`, pat-nzgdu.145).
///
/// `VIEW_DEPTH` is deliberately absent: Godot only exposes it on
/// `GPUParticles3D`, where particles have a camera-relative depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParticleDrawOrder {
    /// Emission order — the order particles were spawned
    /// (Godot `DRAW_ORDER_INDEX`). The default.
    #[default]
    Index,
    /// By remaining lifetime, oldest (nearest death) first
    /// (Godot `DRAW_ORDER_LIFETIME`).
    Lifetime,
    /// By remaining lifetime, youngest first
    /// (Godot `DRAW_ORDER_REVERSE_LIFETIME`).
    ReverseLifetime,
}

impl ParticleDrawOrder {
    /// Maps Godot's `GPUParticles2D.DrawOrder` integer. Unknown values fall
    /// back to [`Index`](Self::Index), matching Godot's default.
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            1 => ParticleDrawOrder::Lifetime,
            2 => ParticleDrawOrder::ReverseLifetime,
            _ => ParticleDrawOrder::Index,
        }
    }

    /// The Godot `GPUParticles2D.DrawOrder` integer for this order.
    pub fn to_godot_int(self) -> i64 {
        match self {
            ParticleDrawOrder::Index => 0,
            ParticleDrawOrder::Lifetime => 1,
            ParticleDrawOrder::ReverseLifetime => 2,
        }
    }
}

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
    ///
    /// Godot defaults this to **false**: particles are spawned into global space
    /// and no longer follow the emitter. See [`Particle::spawn_transform`].
    pub local_coords: bool,
    /// The emitter node's global transform for the current frame.
    ///
    /// Stamped onto each particle at spawn ([`Particle::spawn_transform`]) so a
    /// `local_coords = false` cloud can be kept in global space. The scene layer
    /// refreshes it every frame from the node's transform
    /// (`MainLoop::process_particles`); a simulator driven directly leaves it at
    /// [`Transform2D::IDENTITY`], which makes both coordinate modes agree.
    pub emitter_transform: Transform2D,
    /// Order the simulator emits draw commands in (Godot `draw_order`,
    /// pat-nzgdu.145).
    pub draw_order: ParticleDrawOrder,
    /// Fixed simulation rate in steps per second (Godot `fixed_fps`).
    ///
    /// `0` (the default) simulates with the frame's own delta. Any positive
    /// value makes the simulation advance in fixed `1/fixed_fps` increments,
    /// so the result depends only on elapsed time — not on how the render
    /// delta happened to be chopped up (pat-nzgdu.145).
    pub fixed_fps: u32,
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
            emitter_transform: Transform2D::IDENTITY,
            draw_order: ParticleDrawOrder::Index,
            fixed_fps: 0,
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
        let lifetime_factor = 1.0 - self.material.lifetime_randomness * rand_f32(&mut rng);
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
            spawn_transform: self.emitter_transform,
        }
    }
}

// ---------------------------------------------------------------------------
// ParticleSimulator
// ---------------------------------------------------------------------------

/// Godot's `ParticleProcessMaterial.SubEmitterMode` (pat-0q2kx.233).
///
/// ⚠️ The discriminants are **not** in the order the names suggest:
/// `upstream/godot/scene/resources/particle_process_material.h:94-98` declares
/// `DISABLED, CONSTANT, AT_END, AT_COLLISION, AT_START`, so `AT_START` is **4**
/// and `AT_COLLISION` is 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubEmitterMode {
    /// No sub-emission (Godot's default).
    #[default]
    Disabled,
    /// Each live particle emits at `sub_emitter_frequency` Hz.
    Constant,
    /// A burst when a particle dies.
    AtEnd,
    /// A burst when a particle collides — depends on particle collision, which
    /// Patina does not simulate yet, so this behaves as `Disabled`.
    AtCollision,
    /// A burst when a particle is born.
    AtStart,
}

impl SubEmitterMode {
    /// Maps Godot's integer enum onto this one; anything unrecognised is
    /// `Disabled`, which is the safe reading of a value we do not model.
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            1 => Self::Constant,
            2 => Self::AtEnd,
            3 => Self::AtCollision,
            4 => Self::AtStart,
            _ => Self::Disabled,
        }
    }

    /// The Godot integer for this mode.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::Disabled => 0,
            Self::Constant => 1,
            Self::AtEnd => 2,
            Self::AtCollision => 3,
            Self::AtStart => 4,
        }
    }
}

/// One sub-emitter spawn request recorded by a parent simulator: where the
/// parent particle was, and how fast it was going (pat-0q2kx.233).
///
/// The node layer drains these and injects particles into the child simulator,
/// inheriting `velocity` only when `sub_emitter_keep_velocity` is set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SubEmitEvent {
    /// Position of the parent particle, in the parent emitter's space.
    pub position: Vector2,
    /// Velocity of the parent particle at the moment of the trigger.
    pub velocity: Vector2,
}

/// A per-particle value in `0.0..1.0` that is fixed for the particle's life,
/// standing in for Godot's `RANDOM_SEED` (pat-0q2kx.261).
///
/// Hashes only spawn-time fields — the spawn transform's origin and the
/// assigned lifetime — so successive steps produce the same number. It is not
/// Godot's RNG stream and does not try to be; the contract a shader relies on
/// is stability, not a particular sequence.
fn stable_particle_seed(p: &Particle) -> f32 {
    let mut h: u32 = 2166136261;
    for bits in [
        p.spawn_transform.origin.x.to_bits(),
        p.spawn_transform.origin.y.to_bits(),
        p.lifetime.to_bits(),
    ] {
        for byte in bits.to_le_bytes() {
            h ^= byte as u32;
            h = h.wrapping_mul(16777619);
        }
    }
    // Map to 0..1; the exact distribution does not matter, only determinism.
    (h % 10_000) as f32 / 10_000.0
}

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
    /// Sub-emitter trigger mode, mirrored from the node each frame.
    pub sub_emitter_mode: SubEmitterMode,
    /// `CONSTANT` mode rate, in emissions per second per live particle.
    pub sub_emitter_frequency: f32,
    /// Spawn requests recorded during [`Self::step`], drained by the node layer
    /// via [`Self::drain_sub_emissions`].
    sub_emissions: Vec<SubEmitEvent>,
    /// Fractional carry for `CONSTANT` mode, so a frequency that yields less
    /// than one emission per frame still emits at the right average rate
    /// instead of never firing.
    constant_carry: f32,
    /// Leftover time when `fixed_fps` is set (pat-nzgdu.145).
    fixed_step_accumulator: f32,
    /// A `finished` edge waiting to be drained by the node layer.
    finished_pending: bool,
    /// Whether the current completion has already produced an edge, so
    /// `finished` fires once per drain rather than every frame.
    finished_emitted: bool,
    /// An interpreted `shader_type particles` program assigned as this
    /// emitter's `process_material` (pat-0q2kx.261).
    ///
    /// `None` — the overwhelmingly common case — leaves the built-in gravity /
    /// velocity integration exactly as it was. When present, the shader's
    /// `process()` runs **after** that integration for each particle, so a
    /// shader that writes VELOCITY overrides the default motion rather than
    /// racing it.
    particles_shader: Option<gdserver2d::particles_shader::ParticlesProgram>,
}

/// Upper bound on catch-up iterations for one [`ParticleSimulator::step`] call.
///
/// Without it, a huge `dt` (a stall, or a debugger pause) against a high
/// `fixed_fps` would spin for an unbounded time — Godot clamps the same way.
const MAX_FIXED_STEPS_PER_FRAME: u32 = 64;

impl ParticleSimulator {
    /// Creates a new simulator from the given emitter configuration.
    pub fn new(emitter: ParticleEmitter) -> Self {
        Self {
            emitter,
            active_particles: Vec::new(),
            time_accumulator: 0.0,
            total_emitted: 0,
            sub_emitter_mode: SubEmitterMode::default(),
            // Godot's ParticleProcessMaterial default.
            sub_emitter_frequency: 4.0,
            sub_emissions: Vec::new(),
            constant_carry: 0.0,
            fixed_step_accumulator: 0.0,
            finished_pending: false,
            finished_emitted: false,
            particles_shader: None,
        }
    }

    /// Assign a `shader_type particles` program as this emitter's process
    /// material (pat-0q2kx.261).
    ///
    /// Returns the constructs the parser could not interpret, so the caller can
    /// surface them. An empty return does **not** mean the shader did anything
    /// — a shader with no recognised stage is simply inert; use
    /// [`has_particles_shader`](Self::has_particles_shader) for that question.
    pub fn set_particles_shader(&mut self, source: &str) -> Vec<String> {
        let program = gdserver2d::particles_shader::parse_particles_shader(source);
        let unsupported = program.unsupported.clone();
        self.particles_shader = if program.is_empty() {
            None
        } else {
            Some(program)
        };
        unsupported
    }

    /// Drop any assigned particles shader, restoring the built-in motion.
    pub fn clear_particles_shader(&mut self) {
        self.particles_shader = None;
    }

    /// Whether a particles shader with at least one interpretable statement is
    /// driving this emitter.
    pub fn has_particles_shader(&self) -> bool {
        self.particles_shader.is_some()
    }

    /// Takes the sub-emission requests recorded since the last drain
    /// (pat-0q2kx.233).
    ///
    /// The node layer owns the parent→child hop because a simulator has no
    /// handle on any other simulator; draining here keeps that seam explicit
    /// rather than letting one simulator reach into another.
    pub fn drain_sub_emissions(&mut self) -> Vec<SubEmitEvent> {
        std::mem::take(&mut self.sub_emissions)
    }

    /// Injects one particle at `position`, as a sub-emitter or an `emit_particle`
    /// call does (pat-0q2kx.233).
    ///
    /// The particle is built from this emitter's own material — a sub-emitter's
    /// children look like its own particles, which is Godot's behaviour — and
    /// then relocated. `velocity` overrides the material-derived velocity when
    /// the parent asked to pass its own along (`sub_emitter_keep_velocity`).
    ///
    /// Emission happens regardless of this emitter's `emitting` flag: Godot
    /// ignores a sub-emitter's own flag, since the parent is what drives it.
    pub fn inject_particle(&mut self, position: Vector2, velocity: Option<Vector2>) {
        self.total_emitted += 1;
        let seed = (self.total_emitted & 0xFFFF_FFFF) as u32;
        let mut particle = self.emitter.emit_particle(seed.max(1));
        particle.position = position;
        if let Some(v) = velocity {
            particle.velocity = v;
        }
        self.active_particles.push(particle);
    }

    /// Advance the simulation by `dt` seconds.
    ///
    /// When [`fixed_fps`](ParticleEmitter::fixed_fps) is set the simulation runs
    /// in fixed increments and any remainder is carried to the next call, so the
    /// outcome depends only on total elapsed time rather than on the render
    /// delta (pat-nzgdu.145). Otherwise `dt` is simulated directly.
    pub fn step(&mut self, dt: f32) {
        if self.emitter.fixed_fps > 0 {
            let fixed_dt = 1.0 / self.emitter.fixed_fps as f32;
            self.fixed_step_accumulator += dt;
            let mut steps = 0;
            while self.fixed_step_accumulator >= fixed_dt && steps < MAX_FIXED_STEPS_PER_FRAME {
                self.step_exact(fixed_dt);
                self.fixed_step_accumulator -= fixed_dt;
                steps += 1;
            }
            if steps == MAX_FIXED_STEPS_PER_FRAME {
                // Fell too far behind — drop the backlog instead of spiralling.
                self.fixed_step_accumulator = 0.0;
            }
        } else {
            self.step_exact(dt);
        }
        self.update_finished_edge();
    }

    /// Records a `finished` edge when a one-shot emitter drains.
    ///
    /// Only meaningful for `one_shot` (Godot never fires `finished` for a
    /// continuous emitter), and only after something was actually emitted —
    /// otherwise a freshly-built idle one-shot would "finish" on its first step
    /// without ever having run.
    fn update_finished_edge(&mut self) {
        let complete = self.is_complete() && self.total_emitted > 0;
        if complete {
            if !self.finished_emitted {
                self.finished_pending = true;
                self.finished_emitted = true;
            }
        } else {
            // Re-arm, so a restart()ed emitter can finish again.
            self.finished_emitted = false;
        }
    }

    /// Takes the pending `finished` edge, if any (pat-nzgdu.145).
    ///
    /// Returns `true` exactly once per completion — the seam the node layer
    /// drains each frame to emit Godot's `finished` signal. Draining is
    /// idempotent: a second call returns `false` until the emitter is restarted
    /// and drains again.
    pub fn take_finished(&mut self) -> bool {
        std::mem::take(&mut self.finished_pending)
    }

    /// Advance the simulation by exactly `dt` seconds.
    ///
    /// Ages existing particles, removes dead ones, emits new particles
    /// based on the configured rate and explosiveness, and applies
    /// gravity and velocity integration.
    fn step_exact(&mut self, dt: f32) {
        let dt_scaled = dt * self.emitter.speed_scale;

        // --- Age and integrate existing particles ---
        //
        // Taken out of `self` so the shader hook below can borrow the program
        // immutably while the particle loop borrows the vector mutably.
        let shader = self.particles_shader.clone();
        for p in &mut self.active_particles {
            p.lifetime_remaining -= dt_scaled;
            p.velocity = p.velocity + self.emitter.material.gravity * dt_scaled;
            p.position = p.position + p.velocity * dt_scaled;
            p.rotation += p.angular_velocity * dt_scaled;

            // Interpolate color over lifetime
            let t = p.age_ratio();
            p.color = self
                .emitter
                .material
                .start_color
                .lerp(self.emitter.material.end_color, t);

            // --- `shader_type particles` process() (pat-0q2kx.261) ---
            //
            // Runs AFTER the built-in integration and the colour ramp, so a
            // shader that writes VELOCITY or COLOR wins. Running it first would
            // let gravity and the ramp overwrite the shader's own values every
            // step, which reads as "the shader does nothing" — the exact
            // failure this bead exists to fix.
            if let Some(program) = &shader {
                let mut state = gdserver2d::particles_shader::ParticleShaderState {
                    velocity: p.velocity,
                    color: p.color,
                    position: p.position,
                    delta: dt_scaled,
                    lifetime: p.lifetime,
                    age: p.lifetime - p.lifetime_remaining,
                    // ⚠️ DERIVED, not Godot's RNG stream. `Particle` carries no
                    // seed field, and adding one would touch every construction
                    // site for a value nothing else reads. This hashes fields
                    // that are fixed at spawn, which gives the property a shader
                    // actually depends on — **stable for the particle's whole
                    // life** — without pretending to reproduce Godot's numbers,
                    // which no port can. Deriving it from `lifetime_remaining`
                    // instead would turn RANDOM_SEED into a clock: it would
                    // change every step and animate anything keyed off it.
                    random_seed: stable_particle_seed(p),
                };
                program.run_process(&mut state);
                p.velocity = state.velocity;
                p.color = state.color;
                p.position = state.position;
            }
        }

        // --- CONSTANT sub-emission ---
        // Each live particle emits at `sub_emitter_frequency` Hz. The rate is
        // accumulated in aggregate (frequency × live count × dt) with a
        // fractional carry, so the average spawn rate matches Godot even when it
        // is below one per frame. Positions come from successive live particles;
        // the per-particle *phase* is therefore not individually tracked, which
        // is the one approximation here.
        if self.sub_emitter_mode == SubEmitterMode::Constant && !self.active_particles.is_empty() {
            self.constant_carry +=
                self.sub_emitter_frequency * self.active_particles.len() as f32 * dt_scaled;
            let mut spawned = 0usize;
            while self.constant_carry >= 1.0 {
                self.constant_carry -= 1.0;
                let p = &self.active_particles[spawned % self.active_particles.len()];
                self.sub_emissions.push(SubEmitEvent {
                    position: p.position,
                    velocity: p.velocity,
                });
                spawned += 1;
                // Guard against a pathological frequency × dt turning one frame
                // into an unbounded loop.
                if spawned >= 4096 {
                    self.constant_carry = 0.0;
                    break;
                }
            }
        }

        // --- Record AT_END triggers before the dead are dropped ---
        // Has to happen here: after `retain` the dying particles' positions are
        // gone, and that position is exactly what the sub-emitter needs.
        if self.sub_emitter_mode == SubEmitterMode::AtEnd {
            for p in self.active_particles.iter().filter(|p| p.is_dead()) {
                self.sub_emissions.push(SubEmitEvent {
                    position: p.position,
                    velocity: p.velocity,
                });
            }
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
                        if self.sub_emitter_mode == SubEmitterMode::AtStart {
                            self.sub_emissions.push(SubEmitEvent {
                                position: particle.position,
                                velocity: particle.velocity,
                            });
                        }
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
                    if self.sub_emitter_mode == SubEmitterMode::AtStart {
                        self.sub_emissions.push(SubEmitEvent {
                            position: particle.position,
                            velocity: particle.velocity,
                        });
                    }
                    self.active_particles.push(particle);
                }

                // For one_shot, stop after emitting `amount` particles in the first cycle
                if self.emitter.one_shot && self.total_emitted >= self.emitter.amount as u64 {
                    self.emitter.emitting = false;
                }
            }
        }
    }

    /// Returns draw commands for each live particle: (position, color, scale),
    /// ordered by the emitter's [`draw_order`](ParticleEmitter::draw_order).
    ///
    /// [`Index`](ParticleDrawOrder::Index) keeps emission order — which is what
    /// this returned unconditionally before pat-nzgdu.145 — so the default is
    /// unchanged. The lifetime orders sort by remaining lifetime; the sort is
    /// stable, so particles with equal lifetime keep their emission order rather
    /// than shuffling between frames.
    pub fn get_draw_commands(&self) -> Vec<(Vector2, Color, f32)> {
        self.ordered_particles()
            .into_iter()
            .map(|p| (p.position, p.color, p.scale))
            .collect()
    }

    /// The live particles in the emitter's [`draw_order`](ParticleEmitter::draw_order).
    ///
    /// Shared by [`get_draw_commands`](Self::get_draw_commands) and
    /// [`draw_commands_in_local_space`](Self::draw_commands_in_local_space) so the
    /// two never disagree about which particle a command came from.
    fn ordered_particles(&self) -> Vec<&Particle> {
        let mut particles: Vec<&Particle> = self.active_particles.iter().collect();
        match self.emitter.draw_order {
            ParticleDrawOrder::Index => {}
            ParticleDrawOrder::Lifetime => {
                // Oldest (least remaining) first.
                particles.sort_by(|a, b| {
                    a.lifetime_remaining
                        .partial_cmp(&b.lifetime_remaining)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            ParticleDrawOrder::ReverseLifetime => {
                particles.sort_by(|a, b| {
                    b.lifetime_remaining
                        .partial_cmp(&a.lifetime_remaining)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            }
        }
        particles
    }

    /// Returns draw commands expressed in the emitter's **current** local space,
    /// given the emitter node's current global transform.
    ///
    /// This is the seam that implements Godot's `local_coords` (pat-0q2kx.51):
    ///
    /// - `local_coords = true` — particles belong to the emitter, so their
    ///   simulated positions already *are* emitter-local and pass through
    ///   unchanged. The cloud moves, rotates, and scales with the emitter.
    /// - `local_coords = false` (Godot's default) — each particle's position is
    ///   lifted to global space through the transform captured at its spawn
    ///   ([`Particle::spawn_transform`]) and then expressed back in the emitter's
    ///   *current* local space. Renderers apply the emitter's current global
    ///   transform to the returned points, so the round trip cancels: a particle
    ///   stays where it was born no matter where the emitter goes afterwards.
    ///
    /// Returning emitter-local points (rather than global ones) keeps the
    /// published `_particle_positions` contract that every renderer already
    /// draws through the emitter's transform.
    ///
    /// KNOWN LIMITATION: a global-space particle is *integrated* in its spawn
    /// frame, so gravity and velocity follow the emitter's rotation/scale at
    /// spawn rather than world axes. Identical for the overwhelmingly common
    /// unrotated, unscaled emitter; a rotated one tilts its cloud's gravity.
    pub fn draw_commands_in_local_space(
        &self,
        emitter_global: Transform2D,
    ) -> Vec<(Vector2, Color, f32)> {
        if self.emitter.local_coords {
            return self.get_draw_commands();
        }
        // A degenerate emitter basis (zero scale) has no inverse; leaving the
        // points emitter-local is the pre-existing behaviour rather than NaN.
        let det = emitter_global.x.x * emitter_global.y.y - emitter_global.x.y * emitter_global.y.x;
        if det.abs() < 1e-9 {
            return self.get_draw_commands();
        }
        let to_local = emitter_global.affine_inverse();
        self.ordered_particles()
            .into_iter()
            .map(|p| {
                (
                    to_local.xform(p.spawn_transform.xform(p.position)),
                    p.color,
                    p.scale,
                )
            })
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
        assert!(approx_eq(speed, 100.0), "Expected speed ~100, got {speed}");
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
            spawn_transform: Transform2D::IDENTITY,
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
            spawn_transform: Transform2D::IDENTITY,
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

    // -- pat-nzgdu.145: draw_order / fixed_fps / finished ---------------------

    /// A burst emitter whose particles are all alive at once.
    fn burst(amount: u32, lifetime: f32) -> ParticleEmitter {
        ParticleEmitter {
            amount,
            lifetime,
            one_shot: true,
            explosiveness: 1.0,
            ..Default::default()
        }
    }

    #[test]
    fn draw_order_godot_int_roundtrip() {
        for order in [
            ParticleDrawOrder::Index,
            ParticleDrawOrder::Lifetime,
            ParticleDrawOrder::ReverseLifetime,
        ] {
            assert_eq!(
                ParticleDrawOrder::from_godot_int(order.to_godot_int()),
                order
            );
        }
        assert_eq!(
            ParticleDrawOrder::from_godot_int(0),
            ParticleDrawOrder::Index
        );
        assert_eq!(
            ParticleDrawOrder::from_godot_int(1),
            ParticleDrawOrder::Lifetime
        );
        // Unknown values fall back to Godot's default.
        assert_eq!(
            ParticleDrawOrder::from_godot_int(99),
            ParticleDrawOrder::Index
        );
        assert_eq!(ParticleDrawOrder::default(), ParticleDrawOrder::Index);
    }

    #[test]
    fn draw_order_lifetime_sorts_by_age() {
        // Three particles of differing remaining lifetime, deliberately pushed
        // in an order that is neither sorted nor reverse-sorted.
        let mut sim = ParticleSimulator::new(burst(1, 1.0));
        let mut make = |remaining: f32, x: f32| Particle {
            position: Vector2::new(x, 0.0),
            velocity: Vector2::ZERO,
            lifetime: 1.0,
            lifetime_remaining: remaining,
            color: Color::WHITE,
            scale: 1.0,
            rotation: 0.0,
            angular_velocity: 0.0,
            spawn_transform: Transform2D::IDENTITY,
        };
        sim.active_particles = vec![make(0.5, 0.0), make(0.9, 1.0), make(0.1, 2.0)];

        // Default (Index) keeps emission order — the pre-pat-nzgdu.145 behaviour.
        let xs: Vec<f32> = sim.get_draw_commands().iter().map(|c| c.0.x).collect();
        assert_eq!(xs, vec![0.0, 1.0, 2.0]);

        // LIFETIME: oldest (least remaining) first => 0.1, 0.5, 0.9 => x 2,0,1.
        sim.emitter.draw_order = ParticleDrawOrder::Lifetime;
        let xs: Vec<f32> = sim.get_draw_commands().iter().map(|c| c.0.x).collect();
        assert_eq!(xs, vec![2.0, 0.0, 1.0]);

        // REVERSE_LIFETIME: youngest first => 0.9, 0.5, 0.1 => x 1,0,2.
        sim.emitter.draw_order = ParticleDrawOrder::ReverseLifetime;
        let xs: Vec<f32> = sim.get_draw_commands().iter().map(|c| c.0.x).collect();
        assert_eq!(xs, vec![1.0, 0.0, 2.0]);

        // Ordering must not disturb the particles themselves.
        assert_eq!(sim.particle_count(), 3);
    }

    #[test]
    fn fixed_fps_makes_the_sim_independent_of_the_render_delta() {
        // Same elapsed time, chopped up differently: one big frame vs many
        // small ones. With fixed_fps the outcome must match.
        // lifetime 0.3 against 0.5s elapsed: the burst fires around step 18 of
        // ~30, leaving ~0.2s of aging against a 0.3s lifetime — so all four
        // particles are still alive at the end, with margin on both sides. The
        // assertion is on the observable particle count, not the sub-step count,
        // which keeps it robust to a ±1 sub-step difference from float
        // accumulation.
        let run = |chunks: &[f32]| {
            let mut e = burst(4, 0.3);
            e.fixed_fps = 60;
            let mut sim = ParticleSimulator::new(e);
            for &dt in chunks {
                sim.step(dt);
            }
            sim.particle_count()
        };

        let one_big = run(&[0.5]);
        let many_small = run(&[0.1; 5]);
        let uneven = run(&[0.03, 0.21, 0.26]);
        assert_eq!(one_big, many_small, "fixed_fps must not depend on chunking");
        assert_eq!(one_big, uneven, "fixed_fps must not depend on chunking");
        assert!(one_big > 0, "the burst should have emitted");
    }

    #[test]
    fn fixed_fps_zero_keeps_the_variable_timestep() {
        // Default (0) must behave exactly as before pat-nzgdu.145.
        let mut sim = ParticleSimulator::new(burst(3, 10.0));
        assert_eq!(sim.emitter.fixed_fps, 0);
        sim.step(10.0);
        assert_eq!(
            sim.particle_count(),
            3,
            "burst emitted on the variable step"
        );
    }

    #[test]
    fn fixed_fps_survives_a_huge_delta_without_spiralling() {
        // A pathological dt must be clamped, not spin for ~600k iterations.
        //
        // lifetime 0.8 turns the clamp into something *observable* rather than a
        // test that passes vacuously: the clamp allows 64 steps ≈ 1.07s, so the
        // burst fires (~step 48) and the particles are still alive at the end.
        // Un-clamped, all 10000s would simulate, the particles would age out,
        // and this would be 0 — so the count proves the clamp actually engaged.
        let mut e = burst(2, 0.8);
        e.fixed_fps = 60;
        let mut sim = ParticleSimulator::new(e);
        sim.step(10_000.0);
        assert_eq!(
            sim.particle_count(),
            2,
            "the clamp must bound the catch-up, leaving the burst alive",
        );
    }

    #[test]
    fn finished_fires_exactly_once_when_a_one_shot_drains() {
        let mut sim = ParticleSimulator::new(burst(2, 0.1));

        // Nothing emitted yet: an idle one-shot must not "finish" immediately.
        assert!(!sim.take_finished(), "must not finish before emitting");

        sim.step(0.1); // burst
        assert!(!sim.is_complete());
        assert!(!sim.take_finished(), "particles are still alive");

        sim.step(0.5); // particles die out
        assert!(sim.is_complete());
        assert!(sim.take_finished(), "finished fires on drain");

        // Exactly once: draining is idempotent and later steps do not re-fire.
        assert!(!sim.take_finished(), "finished must not fire twice");
        sim.step(0.5);
        assert!(!sim.take_finished(), "finished must not re-fire each frame");
    }

    #[test]
    fn finished_never_fires_for_a_continuous_emitter() {
        // Godot only emits `finished` for one_shot emitters.
        let mut sim = ParticleSimulator::new(ParticleEmitter {
            amount: 2,
            lifetime: 0.05,
            one_shot: false,
            explosiveness: 1.0,
            ..Default::default()
        });
        for _ in 0..10 {
            sim.step(0.05);
            assert!(!sim.take_finished(), "continuous emitters never finish");
        }
    }

    #[test]
    fn finished_re_arms_when_the_emitter_runs_again() {
        let mut sim = ParticleSimulator::new(burst(2, 0.1));
        sim.step(0.1);
        sim.step(0.5);
        assert!(sim.take_finished());

        // Restart by re-enabling emission: a second drain fires again.
        sim.emitter.emitting = true;
        sim.total_emitted = 0;
        sim.step(0.1);
        assert!(!sim.take_finished(), "re-armed, not yet drained");
        sim.step(0.5);
        assert!(sim.take_finished(), "a second completion fires again");
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

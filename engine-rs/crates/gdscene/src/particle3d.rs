//! 3D CPU particle system: emission, simulation, gravity, velocity, and color curves.
//!
//! Provides a Godot-compatible CPUParticles3D emitter with deterministic
//! randomness (xorshift), configurable 3D emission shapes, per-particle
//! physics (gravity, angular velocity, damping), and color/scale curves
//! interpolated over particle lifetime.

use gdcore::math::{Color, Vector3};
use gdcore::math3d::Transform3D;

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
    /// Godot `ParticleProcessMaterial.attractor_interaction_enabled`. When
    /// false the emitter ignores every attractor in the scene.
    pub attractor_interaction_enabled: bool,
    /// Godot `ParticleProcessMaterial.collision_mode`.
    pub collision_mode: ParticleCollisionMode,
    /// Godot `collision_friction` — damps the velocity left tangential to the
    /// contact after the normal component is removed. Clamped to [0, 1].
    pub collision_friction: f32,
    /// Godot `collision_bounce` — restitution along the contact normal.
    pub collision_bounce: f32,
    /// Godot `collision_use_scale` — scales a particle's collision radius by
    /// its own display scale.
    pub collision_use_scale: bool,
    /// The particle's base collision radius (Godot's `FRAME.particle_size`).
    pub particle_size: f32,
    /// Layers this emitter occupies, matched against an attractor's or
    /// collider's `cull_mask`.
    pub cull_mask: u32,
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
            // Godot defaults attractor interaction ON, so an attractor placed
            // in a scene affects a default emitter without extra setup.
            attractor_interaction_enabled: true,
            collision_mode: ParticleCollisionMode::Disabled,
            collision_friction: 0.0,
            collision_bounce: 0.0,
            collision_use_scale: false,
            particle_size: 0.0,
            cull_mask: 0xFFFF_FFFF,
        }
    }
}

// ---------------------------------------------------------------------------
// Attractors and colliders (pat-s3rrv.216)
// ---------------------------------------------------------------------------

/// How a particle answers a collision, mirroring Godot's
/// `ParticleProcessMaterial.CollisionMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParticleCollisionMode {
    /// `COLLISION_DISABLED` (0) — particles pass through everything.
    #[default]
    Disabled,
    /// `COLLISION_RIGID` (1) — bounce using `collision_bounce`/`collision_friction`.
    Rigid,
    /// `COLLISION_HIDE_ON_CONTACT` (2) — the particle dies on contact.
    HideOnContact,
}

impl ParticleCollisionMode {
    /// Maps Godot's integer enum, treating anything unknown as disabled.
    pub fn from_godot(value: i64) -> Self {
        match value {
            1 => Self::Rigid,
            2 => Self::HideOnContact,
            _ => Self::Disabled,
        }
    }
}

/// The analytic volume shared by attractor and collider nodes.
///
/// Stage 1 of pat-s3rrv.216 covers the two analytic shapes; `VectorField3D`,
/// `HeightField3D` and `SDF3D` are follow-ups.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParticleVolume3D {
    /// `*Box3D` — `half_extents` is Godot's `size * 0.5`.
    Box {
        /// Half of the box's `size`, matching the shader's `extents`.
        half_extents: Vector3,
    },
    /// `*Sphere3D`.
    Sphere {
        /// The sphere's radius.
        radius: f32,
    },
}

/// One `GPUParticlesAttractor3D` for the frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleAttractor3D {
    /// The node's global transform.
    pub transform: Transform3D,
    /// The attractor's volume.
    pub volume: ParticleVolume3D,
    /// Godot `strength`; negative repels.
    pub strength: f32,
    /// Godot `attenuation` — the falloff exponent over normalized distance.
    pub attenuation: f32,
    /// Godot `directionality` — 0 pulls radially, 1 pushes along the
    /// attractor's Z axis.
    pub directionality: f32,
    /// Matched against the emitter's `cull_mask`.
    pub cull_mask: u32,
}

/// One `GPUParticlesCollision3D` for the frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleCollider3D {
    /// The node's global transform.
    pub transform: Transform3D,
    /// The collider's volume.
    pub volume: ParticleVolume3D,
    /// Matched against the emitter's `cull_mask`.
    pub cull_mask: u32,
}

/// Everything in the scene that a particle step must react to.
///
/// Built once per frame by `MainLoop::process_particles` and shared by every
/// simulator, so collecting the nodes is done once rather than per emitter.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParticleField3D {
    /// Attractors active this frame.
    pub attractors: Vec<ParticleAttractor3D>,
    /// Colliders active this frame.
    pub colliders: Vec<ParticleCollider3D>,
}

impl ParticleField3D {
    /// An empty field — no attractors, no colliders.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Whether there is nothing to react to, letting `step` skip the work.
    pub fn is_empty(&self) -> bool {
        self.attractors.is_empty() && self.colliders.is_empty()
    }
}

/// A contact between a particle and a collider.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleContact3D {
    /// Unit contact normal, pointing out of the collider.
    pub normal: Vector3,
    /// Penetration depth to push back out along `normal`.
    pub depth: f32,
}

/// Normalizes, or returns zero for a degenerate vector — the shader's
/// `safe_normalize`.
fn safe_normalize(v: Vector3) -> Vector3 {
    let len = v.length();
    if len > 1e-6 {
        v * (1.0 / len)
    } else {
        Vector3::new(0.0, 0.0, 0.0)
    }
}

/// GLSL `sign`, except zero maps to `1.0`.
///
/// The shader's `sign()` yields 0 on an exactly-centred axis, which would zero
/// that component of the contact normal; biasing to `+1` keeps the normal unit
/// length in that measure-zero case.
fn sign_nonzero(x: f32) -> f32 {
    if x < 0.0 {
        -1.0
    } else {
        1.0
    }
}

impl ParticleVolume3D {
    /// The shader's `extents` vector for this shape.
    fn extents(self) -> Vector3 {
        match self {
            ParticleVolume3D::Box { half_extents } => half_extents,
            ParticleVolume3D::Sphere { radius } => Vector3::new(radius, radius, radius),
        }
    }
}

impl ParticleAttractor3D {
    /// The acceleration applied to a particle at `position`, or `None` when the
    /// particle lies outside the attractor's volume.
    ///
    /// Ported from `servers/rendering/renderer_rd/shaders/particles.glsl:452-491`.
    /// ⚠️ The shader *subtracts* the term, and `dir` points from the attractor
    /// toward the particle, so a **positive `strength` pulls inward** and a
    /// negative one repels. Getting that sign backwards is the whole feature.
    pub fn force_at(&self, position: Vector3) -> Option<Vector3> {
        let rel = position - self.transform.origin;
        // `rel_vec * mat3(transform)` in GLSL is the transpose applied to the
        // vector, which is exactly `Basis::xform_inv`.
        let local = self.transform.basis.xform_inv(rel);
        let extents = self.volume.extents();
        let radial = safe_normalize(rel);

        let d = match self.volume {
            ParticleVolume3D::Sphere { radius } => {
                if radius.abs() < 1e-6 {
                    return None;
                }
                local.length() / radius
            }
            ParticleVolume3D::Box { .. } => {
                if extents.x.abs() < 1e-6 || extents.y.abs() < 1e-6 || extents.z.abs() < 1e-6 {
                    return None;
                }
                let ax = (local.x / extents.x).abs();
                let ay = (local.y / extents.y).abs();
                let az = (local.z / extents.z).abs();
                ax.max(ay).max(az)
            }
        };
        if d > 1.0 {
            return None;
        }

        let amount = (1.0 - d).max(0.0).powf(self.attenuation);
        // mix(dir, transform[2].xyz, directionality) — the basis Z column.
        let t = self.directionality.clamp(0.0, 1.0);
        let dir = safe_normalize(radial * (1.0 - t) + self.transform.basis.z * t);
        Some(dir * (-amount * self.strength))
    }
}

impl ParticleCollider3D {
    /// The contact for a particle at `position` with radius `particle_size`,
    /// or `None` when it is clear of the volume.
    ///
    /// Ported from `particles.glsl:546-592`.
    pub fn contact(&self, position: Vector3, particle_size: f32) -> Option<ParticleContact3D> {
        // The shader's epsilon, so a particle exactly touching counts as hit.
        const EPSILON: f32 = 0.001;
        let rel = position - self.transform.origin;
        let local = self.transform.basis.xform_inv(rel);

        match self.volume {
            ParticleVolume3D::Sphere { radius } => {
                let d = rel.length() - (particle_size + radius);
                if d <= EPSILON {
                    Some(ParticleContact3D {
                        normal: safe_normalize(rel),
                        depth: -d,
                    })
                } else {
                    None
                }
            }
            ParticleVolume3D::Box { half_extents } => {
                let e = half_extents;
                let abs_pos = Vector3::new(local.x.abs(), local.y.abs(), local.z.abs());
                let sgn = Vector3::new(
                    sign_nonzero(local.x),
                    sign_nonzero(local.y),
                    sign_nonzero(local.z),
                );

                if abs_pos.x > e.x || abs_pos.y > e.y || abs_pos.z > e.z {
                    // Outside the box: measure to the closest surface point.
                    let closest = Vector3::new(
                        abs_pos.x.min(e.x),
                        abs_pos.y.min(e.y),
                        abs_pos.z.min(e.z),
                    );
                    let rel_out = abs_pos - closest;
                    let depth = rel_out.length() - particle_size;
                    if depth > EPSILON {
                        return None;
                    }
                    let n = safe_normalize(rel_out);
                    let n = Vector3::new(n.x * sgn.x, n.y * sgn.y, n.z * sgn.z);
                    Some(ParticleContact3D {
                        normal: safe_normalize(self.transform.basis.xform(n)),
                        depth: -depth,
                    })
                } else {
                    // Inside the box: escape along the nearest face.
                    let axis_len = Vector3::new(e.x - abs_pos.x, e.y - abs_pos.y, e.z - abs_pos.z);
                    let n_local = if axis_len.x <= axis_len.y && axis_len.x <= axis_len.z {
                        Vector3::new(1.0, 0.0, 0.0)
                    } else if axis_len.y <= axis_len.x && axis_len.y <= axis_len.z {
                        Vector3::new(0.0, 1.0, 0.0)
                    } else {
                        Vector3::new(0.0, 0.0, 1.0)
                    };
                    let depth = n_local.x * axis_len.x
                        + n_local.y * axis_len.y
                        + n_local.z * axis_len.z
                        + particle_size;
                    let n = Vector3::new(
                        n_local.x * sgn.x,
                        n_local.y * sgn.y,
                        n_local.z * sgn.z,
                    );
                    Some(ParticleContact3D {
                        normal: safe_normalize(self.transform.basis.xform(n)),
                        depth,
                    })
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// GpuParticleInstance — GPU-instanced particle per-instance buffer row
// ---------------------------------------------------------------------------

/// Per-instance row a GPU particle pipeline binds for billboarded rendering.
///
/// Mirrors the structured-of-arrays layout the wgpu pipeline uploads each
/// frame: position, colour, scale, velocity (for motion-vector billboarding),
/// and `age_ratio` (for shader-side colour/scale curve sampling). The CPU
/// simulator emits the same struct so a CPU rasterizer can issue one batched
/// draw without expanding to per-particle draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpuParticleInstance {
    /// World-space (or local-space, depending on `local_coords`) position.
    pub position: Vector3,
    /// Current display colour (already evaluated through `color_curve`).
    pub color: Color,
    /// Current display scale (`base_scale * scale_curve`).
    pub scale: f32,
    /// Current world-space velocity, exposed so motion-vector billboards can
    /// orient quads along the trajectory.
    pub velocity: Vector3,
    /// Normalised age in `[0, 1]` (`0` = just born, `1` = dead) for shaders
    /// that prefer to evaluate curves on-GPU.
    pub age_ratio: f32,
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
    /// The emitter node's global transform at the moment of emission
    /// (pat-mgixk.70).
    ///
    /// ⚠️ Mirrors `ParticleEmitter2D::emitter_transform` (pat-0q2kx.51), which
    /// the 2D path has set from `node2d::get_global_transform` all along. The 3D
    /// emitter had no such field and `EmissionShape3D::sample` starts at
    /// `Vector3::ZERO`, so every 3D emitter spawned its particles at the WORLD
    /// ORIGIN however the node was placed — while colliders and attractors in
    /// the same simulation used global transforms.
    ///
    /// Defaults to identity, so a simulator built straight from
    /// `ParticleEmitter3D::default()` behaves exactly as before.
    pub emitter_transform: Transform3D,
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
            emitter_transform: Transform3D::IDENTITY,
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

        // Godot's default `local_coords = false` spawns in WORLD space at the
        // emitter's global transform and leaves each particle there as the
        // emitter moves on (`doc/classes/GPUParticles3D.xml`); the emission
        // shape is sampled in the emitter's own space first.
        //
        // ⚠️ With `local_coords = true` the renderer draws the particles
        // THROUGH the node's transform, so pre-transforming here would apply it
        // twice. That is why this is a branch and not an unconditional xform.
        let (position, velocity) = if self.local_coords {
            (position, velocity)
        } else {
            (
                self.emitter_transform.xform(position),
                // The BASIS only. A velocity is a direction: adding the
                // emitter's translation to it would hurl every particle away
                // from the origin at the emitter's distance per second.
                self.emitter_transform.basis.xform(velocity),
            )
        };

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
    /// A completed one-shot cycle waiting to be drained as Godot's `finished`
    /// signal (pat-s3rrv.111). The 3D twin of `ParticleSimulator::finished_pending`.
    finished_pending: bool,
    /// Whether the current completion has already produced an edge, so a
    /// one-shot that stays complete emits `finished` ONCE rather than on every
    /// frame for the rest of the scene's life.
    finished_emitted: bool,
}

impl ParticleSimulator3D {
    /// Creates a new simulator from the given emitter configuration.
    pub fn new(emitter: ParticleEmitter3D) -> Self {
        Self {
            emitter,
            active_particles: Vec::new(),
            time_accumulator: 0.0,
            total_emitted: 0,
            finished_pending: false,
            finished_emitted: false,
        }
    }

    /// Advance the simulation by `dt` seconds with nothing to react to.
    ///
    /// Kept alongside [`step_with_field`](Self::step_with_field) rather than
    /// replaced: every existing call site keeps compiling, and an emitter in a
    /// scene with no attractors or colliders behaves exactly as before.
    pub fn step(&mut self, dt: f32) {
        self.step_with_field(dt, &ParticleField3D::empty());
    }

    /// Advance the simulation by `dt` seconds, applying `field`'s attractor
    /// forces before the position update and resolving its colliders after it.
    pub fn step_with_field(&mut self, dt: f32, field: &ParticleField3D) {
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
                    p.velocity =
                        p.velocity + radial_dir * self.emitter.material.radial_accel * dt_scaled;
                }
            }

            // Apply tangential acceleration
            if self.emitter.material.tangential_accel != 0.0 {
                let dist = p.position.length();
                if dist > 1e-6 {
                    let radial_dir = p.position * (1.0 / dist);
                    // Tangential = cross(up, radial) for a default tangent
                    let tangent = Vector3::UP.cross(radial_dir).normalized();
                    p.velocity =
                        p.velocity + tangent * self.emitter.material.tangential_accel * dt_scaled;
                }
            }

            // Apply attractor forces (pat-s3rrv.216). Godot gates the whole
            // interaction on the material flag, so a disabled emitter ignores
            // every attractor in the scene.
            if self.emitter.material.attractor_interaction_enabled {
                let mut accel = Vector3::new(0.0, 0.0, 0.0);
                for attractor in &field.attractors {
                    if attractor.cull_mask & self.emitter.material.cull_mask == 0 {
                        continue;
                    }
                    if let Some(force) = attractor.force_at(p.position) {
                        accel = accel + force;
                    }
                }
                p.velocity = p.velocity + accel * dt_scaled;
            }

            // Integrate position
            p.position = p.position + p.velocity * dt_scaled;

            // Resolve collisions (pat-s3rrv.216).
            if self.emitter.material.collision_mode != ParticleCollisionMode::Disabled
                && !field.colliders.is_empty()
            {
                let mut size = self.emitter.material.particle_size;
                if self.emitter.material.collision_use_scale {
                    size *= p.scale;
                }

                // Merge overlapping contacts the way particles.glsl:654-664
                // does, so a particle wedged between two colliders is pushed
                // out of both rather than only the last one.
                let mut contact: Option<ParticleContact3D> = None;
                for collider in &field.colliders {
                    if collider.cull_mask & self.emitter.material.cull_mask == 0 {
                        continue;
                    }
                    let Some(hit) = collider.contact(p.position, size) else {
                        continue;
                    };
                    contact = Some(match contact {
                        None => hit,
                        Some(prev) => {
                            let acc = prev.normal * prev.depth;
                            let acc =
                                acc + hit.normal * (hit.depth - hit.normal.dot(acc)).max(0.0);
                            ParticleContact3D {
                                normal: safe_normalize(acc),
                                depth: acc.length(),
                            }
                        }
                    });
                }

                if let Some(hit) = contact {
                    if self.emitter.material.collision_mode
                        == ParticleCollisionMode::HideOnContact
                    {
                        // Godot sets `ACTIVE = false`; the retain below reaps it.
                        p.lifetime_remaining = 0.0;
                    } else {
                        // particle_process_material.cpp:1021-1031.
                        let response = hit.normal.dot(p.velocity);
                        let bounce = self.emitter.material.collision_bounce;
                        let trigger = if response.abs()
                            >= 2.0 / (bounce + 1.0).clamp(1.0, 2.0)
                        {
                            1.0
                        } else {
                            0.0
                        };
                        p.position = p.position + hit.normal * hit.depth;
                        // Remove the normal component, damp what is left
                        // tangentially, then add the bounce back along it.
                        p.velocity = p.velocity - hit.normal * response;
                        let friction =
                            self.emitter.material.collision_friction.clamp(0.0, 1.0);
                        p.velocity = p.velocity * (1.0 - friction);
                        p.velocity = p.velocity - hit.normal * response * (bounce * trigger);
                    }
                }
            }

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
        self.update_finished_edge();
    }

    /// Records a `finished` edge when a one-shot emitter drains (pat-s3rrv.111).
    ///
    /// ⚠️ EDGE, not level. [`is_complete`](Self::is_complete) stays true for
    /// every frame after the last particle dies, so emitting straight off it
    /// would fire `finished` on every frame forever. This latches once and
    /// re-arms only when the emitter leaves the complete state, which is what
    /// makes `restart()` able to finish again.
    ///
    /// The `total_emitted > 0` guard matters as much as the latch: a freshly
    /// built one-shot that has not emitted anything yet is *also* "one_shot,
    /// not emitting, no live particles", so without it every idle emitter
    /// would announce `finished` on its very first step.
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

    /// Takes the pending `finished` edge, if any (pat-s3rrv.111).
    ///
    /// Returns `true` exactly once per completion — the seam `MainLoop` drains
    /// each frame to emit Godot's `finished` signal. Draining is idempotent.
    pub fn take_finished(&mut self) -> bool {
        std::mem::take(&mut self.finished_pending)
    }

    /// Returns draw data for each live particle: (position, color, scale).
    pub fn get_draw_data(&self) -> Vec<(Vector3, Color, f32)> {
        self.active_particles
            .iter()
            .map(|p| (p.position, p.color, p.scale))
            .collect()
    }

    /// Flattens the live particle pool into the per-instance buffer that a
    /// GPU compute-driven particle pipeline would upload.
    ///
    /// Each row carries the structured-of-arrays surface a vertex shader
    /// reads: world-space position, current display colour, scale, velocity
    /// (for motion-vector billboarding), and `age_ratio` (for shader-side
    /// curve sampling). The buffer length matches `particle_count()` exactly,
    /// so the renderer issues one instanced draw of `len()` rather than
    /// expanding to `len()` per-particle draws.
    pub fn flatten_gpu_instance_buffer(&self) -> Vec<GpuParticleInstance> {
        self.active_particles
            .iter()
            .map(|p| GpuParticleInstance {
                position: p.position,
                color: p.color,
                scale: p.scale,
                velocity: p.velocity,
                age_ratio: p.age_ratio(),
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
        assert!(
            p.velocity.y < -5.0,
            "Gravity not applied: vy={}",
            p.velocity.y
        );
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

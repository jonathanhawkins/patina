//! CharacterBody3D — kinematic character controller for 3D.
//!
//! Provides [`CharacterBody3D`] with `move_and_slide` for 3D movement that
//! slides along surfaces and detects floor/wall/ceiling contacts.
//!
//! This mirrors Godot 4's `CharacterBody3D::move_and_slide` behaviour: motion
//! modes (grounded vs floating), moving-platform carry and on-leave velocity,
//! floor snapping onto slopes/stairs, and the slope knobs (`floor_stop_on_slope`,
//! `floor_constant_speed`, `floor_block_on_wall`, `wall_min_slide_angle`).

use gdcore::math::Vector3;

use crate::body::{BodyId3D, PhysicsBody3D};
use crate::collision;
use crate::shape::Shape3D;

/// How the character interprets surfaces during `move_and_slide`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MotionMode {
    /// Floor/wall/ceiling classification, snapping and platform carry are active.
    #[default]
    Grounded,
    /// Every surface is treated as a wall — pure sliding, no floor concept.
    Floating,
}

/// What velocity a character keeps when it leaves a moving platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlatformOnLeave {
    /// Add the full platform velocity to the outgoing velocity.
    #[default]
    AddVelocity,
    /// Add only the component of the platform velocity along `up_direction`.
    AddUpwardVelocity,
    /// Add nothing — the platform velocity is discarded.
    DoNotAddVelocity,
}

/// A kinematic character controller that slides along surfaces in 3D.
#[derive(Debug, Clone)]
pub struct CharacterBody3D {
    /// World-space position.
    pub position: Vector3,
    /// Collision shape in local space.
    pub shape: Shape3D,
    /// The up direction for floor/ceiling detection (default: positive Y).
    pub up_direction: Vector3,
    /// Maximum angle (in radians) from up_direction that still counts as floor.
    pub floor_max_angle: f32,
    /// Collision layer bitmask.
    pub collision_layer: u32,
    /// Collision mask bitmask.
    pub collision_mask: u32,

    /// Motion mode (grounded vs floating).
    pub motion_mode: MotionMode,
    /// Distance (world units) the shape is cast downward to stay glued to the
    /// floor when the character would otherwise leave it. `0` disables snapping.
    pub floor_snap_length: f32,
    /// When `true`, the character does not slide down a walkable slope while it
    /// has no horizontal input.
    pub floor_stop_on_slope: bool,
    /// When `true`, horizontal speed is preserved while moving along a slope
    /// (the slope projection does not slow the character down).
    pub floor_constant_speed: bool,
    /// When `true`, a grounded character cannot climb a wall it walks into.
    pub floor_block_on_wall: bool,
    /// Minimum wall angle (radians, measured from up) required to slide; below
    /// it the character stops instead of sliding, preventing jitter.
    pub wall_min_slide_angle: f32,
    /// Maximum number of slide iterations per motion step.
    pub max_slides: u32,
    /// Velocity kept when the character leaves a moving platform.
    pub platform_on_leave: PlatformOnLeave,

    on_floor: bool,
    on_wall: bool,
    on_ceiling: bool,
    floor_normal: Vector3,
    wall_normal: Vector3,
    /// Collision layers whose floor bodies act as moving platforms and
    /// propagate their velocity (Godot's `platform_floor_layers`).
    ///
    /// ⚠️ Defaults to ALL layers — Godot's default is `4294967295`, i.e. every
    /// floor body carries the character. Its sibling `platform_wall_layers`
    /// defaults to **0** instead, so the two cannot share a default; assuming
    /// they match gets one of them backwards.
    ///
    /// ⚠️ This gates the CARRY, not the floor. A body on an excluded layer is
    /// still a floor the character stands on — it simply does not drag it
    /// along. Skipping the body outright would drop the character through it.
    pub platform_floor_layers: u32,
    /// Collision layers whose WALL bodies act as moving platforms
    /// (Godot's `platform_wall_layers`).
    ///
    /// ⚠️ Defaults to **0**, the opposite of its floor sibling: Godot ignores
    /// every wall body unless a scene opts one in
    /// (`doc/classes/CharacterBody3D.xml`, "all wall bodies are ignored").
    /// Copying `platform_floor_layers`'s `u32::MAX` here would silently make
    /// every wall drag the character sideways.
    ///
    /// ⚠️ Consulted only when the character is on a wall and NOT on a floor.
    /// Godot's test is `if (floor) {..} else if (wall) {..}`
    /// (`character_body_3d.cpp:56-62`) — a character standing on a floor while
    /// brushing a moving wall is carried by the FLOOR.
    pub platform_wall_layers: u32,
    /// The body behind the wall contact of the last `move_and_slide`, kept so
    /// the next call can look up its velocity for the carry above.
    ///
    /// ⚠️ Mirrors Godot's `platform_rid`, which is likewise captured during one
    /// call and consulted at the top of the NEXT one — the carry has to be
    /// applied before the motion it affects, so it necessarily uses the
    /// previous frame's contact.
    wall_body: Option<BodyId3D>,
    /// Contacts recorded by the last `move_and_slide`, in the order they were
    /// resolved (Godot's `get_slide_collision(i)`).
    slide_collisions: Vec<KinematicCollision3D>,
    /// If true the body slides along a ceiling on contact; if false a jump into
    /// a ceiling is stopped and the body falls vertically (Godot's
    /// `slide_on_ceiling`, default true).
    pub slide_on_ceiling: bool,
    /// Extra separation kept between the body and a surface it resolves against
    /// (Godot's `CharacterBody3D.safe_margin`, default **0.001**).
    ///
    /// ⚠️ Applied on top of the measured penetration depth, so a body ends a
    /// resolve just clear of the surface rather than exactly touching it. Zero
    /// is a legal authored value and reproduces the old exact-contact
    /// behaviour, which is why the default lives here and not in the solver
    /// (pat-s3rrv.103).
    pub safe_margin: f32,
    /// Velocity of the floor body the character currently stands on.
    platform_velocity: Vector3,
    /// Angular velocity of that same floor body (Godot's
    /// `get_platform_angular_velocity`), captured at the probe that found it.
    platform_angular_velocity: Vector3,
    /// The LAST sub-motion actually achieved by the slide loop
    /// (Godot's `get_last_motion`).
    ///
    /// ⚠️ NOT the total travel — that is `get_position_delta`. Godot is explicit
    /// that "the movement can be split into multiple motions when sliding
    /// occurs, and this method returns the last one", which is what makes it
    /// useful for reading the CURRENT direction of travel. The two coincide
    /// only when nothing was hit, so a fixture that never collides cannot tell
    /// a correct implementation from one that returns the total.
    last_motion: Vector3,
}

/// One contact recorded by `move_and_slide`, behind Godot's
/// `get_slide_collision(i)` / `KinematicCollision3D` (pat-s3rrv.103).
///
/// ⚠️ Carries a `BodyId3D`, not a node: this crate is the solver and knows
/// nothing about the scene tree. `PhysicsServer3D` keeps the `body -> node` map,
/// so the script-facing `get_collider()` resolves it there. Recording the id is
/// what makes that resolution possible — the collision data was previously
/// computed and discarded with no record of what was struck.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KinematicCollision3D {
    /// The body that was hit. `None` only for a collision built without a known
    /// body (test fixtures).
    pub collider: Option<BodyId3D>,
    /// Surface normal at the contact, pointing away from the body.
    pub normal: Vector3,
    /// Penetration depth.
    pub depth: f32,
    /// Position of the character after collision resolution.
    pub position: Vector3,
    /// Motion left unapplied after this contact.
    pub remainder: Vector3,
}

/// Maximum sub-steps for large motions to prevent tunneling.
const MAX_SUBSTEPS: usize = 32;

/// Floor angle threshold (approximately 45 degrees).
const DEFAULT_FLOOR_MAX_ANGLE: f32 = std::f32::consts::FRAC_PI_4;

/// Default number of slide iterations per motion step (Godot 4 default).
const DEFAULT_MAX_SLIDES: u32 = 6;

/// Default floor snap length (Godot 4 default).
const DEFAULT_FLOOR_SNAP_LENGTH: f32 = 0.1;

/// Default minimum wall slide angle: 15 degrees in radians (Godot 4 default).
const DEFAULT_WALL_MIN_SLIDE_ANGLE: f32 = 0.2617994;

/// Downward probe margin used to detect the floor the character rests on.
const FLOOR_PROBE_MARGIN: f32 = 0.08;

/// Number of samples taken when casting the shape down for floor snapping.
const SNAP_SAMPLES: usize = 16;

/// Horizontal-input magnitude below which `floor_stop_on_slope` engages.
const STOP_ON_SLOPE_EPS: f32 = 1e-3;

impl CharacterBody3D {
    /// Creates a new character body at the given position.
    pub fn new(position: Vector3, shape: Shape3D) -> Self {
        Self {
            position,
            shape,
            up_direction: Vector3::new(0.0, 1.0, 0.0),
            floor_max_angle: DEFAULT_FLOOR_MAX_ANGLE,
            collision_layer: 1,
            collision_mask: 1,
            motion_mode: MotionMode::Grounded,
            floor_snap_length: DEFAULT_FLOOR_SNAP_LENGTH,
            floor_stop_on_slope: true,
            floor_constant_speed: false,
            floor_block_on_wall: true,
            wall_min_slide_angle: DEFAULT_WALL_MIN_SLIDE_ANGLE,
            max_slides: DEFAULT_MAX_SLIDES,
            platform_on_leave: PlatformOnLeave::AddVelocity,
            on_floor: false,
            on_wall: false,
            on_ceiling: false,
            floor_normal: Vector3::ZERO,
            wall_normal: Vector3::ZERO,
            platform_floor_layers: u32::MAX,
            platform_wall_layers: 0,
            wall_body: None,
            slide_collisions: Vec::new(),
            slide_on_ceiling: true,
            // Godot CharacterBody3D.safe_margin default.
            safe_margin: 0.001,
            platform_velocity: Vector3::ZERO,
            platform_angular_velocity: Vector3::ZERO,
            last_motion: Vector3::ZERO,
        }
    }

    /// Returns the minimum extent of the shape, used for sub-step sizing.
    fn safe_step_distance(&self) -> f32 {
        match &self.shape {
            Shape3D::Sphere { radius } => *radius,
            Shape3D::BoxShape { half_extents } => {
                half_extents.x.min(half_extents.y).min(half_extents.z)
            }
            Shape3D::CapsuleShape { radius, .. } => *radius,
            Shape3D::CylinderShape { radius, height } => radius.min(*height * 0.5),
            _ => 1.0,
        }
    }

    /// Moves the character by `velocity`, sliding along colliding surfaces.
    ///
    /// `velocity` is this frame's motion. Returns the resulting velocity after
    /// sliding (the character's own velocity, excluding any platform carry, but
    /// including on-leave platform velocity). Populates floor/wall/ceiling state
    /// for queries. Uses sub-stepping for large motions to prevent tunneling.
    ///
    /// In [`MotionMode::Grounded`] this also carries the character with a moving
    /// floor body, snaps the character down onto slopes/stairs, applies the
    /// slope knobs, and adds the platform velocity back on leave. In
    /// [`MotionMode::Floating`] every surface is a wall and none of that runs.
    pub fn move_and_slide(&mut self, velocity: Vector3, bodies: &[&PhysicsBody3D]) -> Vector3 {
        let was_on_floor = self.on_floor;
        let previous_platform_velocity = self.platform_velocity;
        // Read BEFORE the reset below, which clears both — the wall carry is
        // decided from the PREVIOUS call's contact, as it is in Godot.
        let was_on_wall = self.on_wall;
        let previous_wall_body = self.wall_body;

        // Reset contact state for this call.
        self.on_floor = false;
        self.on_wall = false;
        self.on_ceiling = false;
        self.floor_normal = Vector3::ZERO;
        self.wall_normal = Vector3::ZERO;
        // Reset with the rest of the per-call state: Godot reports these for
        // the LAST call, so a call that moves nothing must not leave the
        // previous call's motion visible.
        self.last_motion = Vector3::ZERO;
        self.slide_collisions.clear();
        self.wall_body = None;

        let grounded_mode = self.motion_mode == MotionMode::Grounded;

        // ⚠️ Godot's `vel_dir_facing_up` is taken from the body's OWN velocity
        // (`character_body_3d.cpp:159`), before the moving-platform carry below
        // is folded into `motion`. Deriving it from the carried motion instead
        // would let a descending platform flip the sign and cancel a jump the
        // player never made.
        let vel_dir_facing_up = velocity.dot(self.up_direction) > 0.0;

        // --- Platform detection & carry (Grounded only) ----------------------
        // Probe the floor beneath us; capture the floor body's velocity so the
        // character is carried, and remember the floor normal for the slope
        // knobs below.
        let mut motion = velocity;
        let mut probed_floor_normal: Option<Vector3> = None;
        if grounded_mode {
            if let Some((idx, normal)) = self.probe_floor(bodies) {
                // The floor is a floor either way; only whether it CARRIES the
                // character is masked (Godot's `platform_floor_layers`).
                if (self.platform_floor_layers & bodies[idx].collision_layer) != 0 {
                    self.platform_velocity = bodies[idx].linear_velocity;
                    self.platform_angular_velocity = bodies[idx].angular_velocity;
                    motion = motion + self.platform_velocity;
                } else {
                    self.platform_velocity = Vector3::ZERO;
                    self.platform_angular_velocity = Vector3::ZERO;
                }
                probed_floor_normal = Some(normal);
            } else if was_on_wall {
                // No floor, but we were against a wall: Godot's `else if
                // (collision_state.wall)` branch (`character_body_3d.cpp:60`).
                // A character pinned to a moving wall is carried by it, which is
                // how wall-riding elevators and conveyor walls work.
                //
                // ⚠️ The body is looked up by id rather than index: `bodies` is
                // rebuilt every tick by the caller and an index from the
                // previous call can point at a different body, or out of range.
                let carried = previous_wall_body
                    .and_then(|id| bodies.iter().position(|b| b.id == id))
                    .filter(|&idx| (self.platform_wall_layers & bodies[idx].collision_layer) != 0);
                if let Some(idx) = carried {
                    self.platform_velocity = bodies[idx].linear_velocity;
                    self.platform_angular_velocity = bodies[idx].angular_velocity;
                    motion = motion + self.platform_velocity;
                } else {
                    self.platform_velocity = Vector3::ZERO;
                    self.platform_angular_velocity = Vector3::ZERO;
                }
            } else {
                self.platform_velocity = Vector3::ZERO;
                self.platform_angular_velocity = Vector3::ZERO;
            }
        } else {
            self.platform_velocity = Vector3::ZERO;
            self.platform_angular_velocity = Vector3::ZERO;
        }

        // --- Slope knobs affecting the motion --------------------------------
        if let Some(floor_normal) = probed_floor_normal {
            let up = self.up_direction;
            let horizontal_input = velocity - up * velocity.dot(up);
            let horizontal_len = horizontal_input.length();
            let is_slope = floor_normal.dot(up) < 1.0 - 1e-4;
            if is_slope {
                if self.floor_stop_on_slope && horizontal_len < STOP_ON_SLOPE_EPS {
                    // No horizontal input on a walkable slope: cancel the motion
                    // so gravity does not slide the character downhill.
                    motion = Vector3::ZERO;
                } else if self.floor_constant_speed && horizontal_len > STOP_ON_SLOPE_EPS {
                    // Preserve the horizontal speed magnitude along the slope by
                    // rescaling the slope-tangent motion.
                    let tangential = motion - floor_normal * motion.dot(floor_normal);
                    let horiz_tan = tangential - up * tangential.dot(up);
                    let horiz_tan_len = horiz_tan.length();
                    if horiz_tan_len > 1e-6 {
                        motion = tangential * (horizontal_len / horiz_tan_len);
                    }
                }
            }
        }

        // --- Apply the sliding motion, sub-stepping large moves. -------------
        let mut motion_out = motion;
        let speed = motion.length();
        if speed >= 1e-8 {
            let safe_dist = self.safe_step_distance();
            let num_steps = ((speed / safe_dist).ceil() as usize).clamp(1, MAX_SUBSTEPS);
            let inv_steps = 1.0 / num_steps as f32;

            for _ in 0..num_steps {
                let sub = motion_out * inv_steps;
                if sub.length_squared() < 1e-8 {
                    break;
                }

                let sub_result = self.slide_step(sub, bodies, was_on_floor, vel_dir_facing_up);

                // If a component was absorbed by collision, zero it so later
                // sub-steps stop advancing along it.
                if sub.x.abs() > 1e-8 && sub_result.x.abs() < sub.x.abs() * 0.1 {
                    motion_out.x = 0.0;
                }
                if sub.y.abs() > 1e-8 && sub_result.y.abs() < sub.y.abs() * 0.1 {
                    motion_out.y = 0.0;
                }
                if sub.z.abs() > 1e-8 && sub_result.z.abs() < sub.z.abs() * 0.1 {
                    motion_out.z = 0.0;
                }
            }
        }

        // Map absorbed axes onto the character's OWN velocity for the return
        // value (platform carry is intentionally excluded from the result).
        let mut output_vel = velocity;
        if motion.x.abs() > 1e-8 && motion_out.x == 0.0 {
            output_vel.x = 0.0;
        }
        if motion.y.abs() > 1e-8 && motion_out.y == 0.0 {
            output_vel.y = 0.0;
        }
        if motion.z.abs() > 1e-8 && motion_out.z == 0.0 {
            output_vel.z = 0.0;
        }

        // --- Floor snapping (Grounded only) ----------------------------------
        if grounded_mode {
            self.apply_floor_snap(was_on_floor, velocity, bodies);
        }

        // --- On-leave velocity -----------------------------------------------
        if grounded_mode && was_on_floor && !self.on_floor {
            match self.platform_on_leave {
                PlatformOnLeave::AddVelocity => {
                    output_vel = output_vel + previous_platform_velocity;
                }
                PlatformOnLeave::AddUpwardVelocity => {
                    let up_comp = previous_platform_velocity.dot(self.up_direction);
                    output_vel = output_vel + self.up_direction * up_comp;
                }
                PlatformOnLeave::DoNotAddVelocity => {}
            }
            // We are airborne now — forget the platform we were standing on.
            self.platform_velocity = Vector3::ZERO;
        }

        output_vel
    }

    /// Performs a single slide step with the given step velocity.
    ///
    /// `was_on_floor` reports whether the character was grounded at the start of
    /// the `move_and_slide` call; it gates the grounded wall knobs.
    fn slide_step(
        &mut self,
        step_vel: Vector3,
        bodies: &[&PhysicsBody3D],
        was_on_floor: bool,
        vel_dir_facing_up: bool,
    ) -> Vector3 {
        let mut remaining = step_vel;

        for _ in 0..self.max_slides {
            if remaining.length_squared() < 1e-8 {
                break;
            }

            // ⚠️ `get_last_motion` is measured per SLIDE ITERATION, not per
            // anti-tunnelling sub-step: Godot splits the movement when sliding
            // occurs and reports the final segment. Recording it around the
            // outer distance loop instead would make it identical to
            // `get_position_delta` for every ordinary move, which is exactly
            // the bug the two methods exist to distinguish.
            let iter_start = self.position;
            let target = self.position + remaining;

            // Find the deepest collision against all bodies.
            //
            // ⚠️ The body INDEX is tracked alongside the result. Without it the
            // contact is recorded with no idea what was struck, and
            // `get_collider()` can only ever answer null — which is how the
            // collision data came to be computed and thrown away.
            let mut deepest: Option<collision::CollisionResult3D> = None;
            let mut deepest_idx: Option<usize> = None;

            for (body_idx, body) in bodies.iter().enumerate() {
                // Layer/mask filtering.
                if (self.collision_mask & body.collision_layer) == 0 {
                    continue;
                }

                let result =
                    collision::test_collision(target, &self.shape, body.position, &body.shape);

                if result.colliding
                    && result.depth > 0.0
                    && (deepest.is_none() || result.depth > deepest.as_ref().unwrap().depth)
                {
                    deepest = Some(result);
                    deepest_idx = Some(body_idx);
                }
            }

            let Some(result) = deepest else {
                // No collision — move freely.
                self.position = target;
                self.last_motion = self.position - iter_start;
                break;
            };

            // The collision normal points from A (character) toward B (body).
            // The surface normal points from the body toward the character.
            let surface_normal = -result.normal;
            self.classify_surface(surface_normal, deepest_idx.map(|i| bodies[i].id));

            // Separate from the surface.
            // ⚠️ `+ safe_margin`, not just the measured depth: Godot keeps a
            // small gap so the very next frame's overlap test does not start
            // from an exact-touch state, where a float rounding the wrong way
            // reads as a fresh penetration. Authoring `safe_margin = 0`
            // reproduces the old exact-contact behaviour (pat-s3rrv.103).
            self.position = target + surface_normal * (result.depth + self.safe_margin);
            self.last_motion = self.position - iter_start;

            // `slide_on_ceiling = false`: a jump into a ceiling stops dead and
            // the body falls vertically instead of skating along it
            // (`character_body_3d.cpp:314`).
            //
            // ⚠️ Placed AFTER the separation, not before it. Godot applies the
            // collision response and then cancels the REMAINDER; returning
            // early instead would leave the body still penetrating the ceiling
            // it just hit, and the next tick would start from inside geometry.
            //
            // ⚠️ Gated on `vel_dir_facing_up`. Godot cancels ONLY while the body
            // is moving upward — a body walking under a low ceiling, or falling
            // past one, still slides normally. Cancelling on every ceiling
            // contact would freeze a character the moment its head brushed
            // anything, which is what dropping that term looks like in play.
            if self.on_ceiling && !self.slide_on_ceiling && vel_dir_facing_up {
                // ⚠️ Recorded before returning. The contact HAPPENED — cancelling
                // the remaining motion is the response to it, not a reason to
                // hide it — and a script checking `get_slide_collision_count()`
                // after a blocked jump would otherwise see zero and conclude it
                // never touched anything. `remainder` is ZERO because that is
                // exactly what is left to apply.
                self.slide_collisions.push(KinematicCollision3D {
                    collider: deepest_idx.map(|i| bodies[i].id),
                    normal: surface_normal,
                    depth: result.depth,
                    position: self.position,
                    remainder: Vector3::ZERO,
                });
                return Vector3::ZERO;
            }

            // Slide: remove the velocity component along the surface normal.
            let vel_along_normal = remaining.dot(surface_normal);
            remaining = remaining - surface_normal * vel_along_normal;

            // Recorded AFTER the projection so `remainder` is the motion still
            // to be applied, which is what Godot reports — recording it before
            // would hand back the pre-slide motion and describe a body that
            // stopped dead.
            self.slide_collisions.push(KinematicCollision3D {
                collider: deepest_idx.map(|i| bodies[i].id),
                normal: surface_normal,
                depth: result.depth,
                position: self.position,
                remainder: remaining,
            });

            // Grounded wall knobs only apply to wall surfaces.
            let up_dot = surface_normal.dot(self.up_direction);
            let floor_cos = self.floor_max_angle.cos();
            let is_wall_surface = self.motion_mode == MotionMode::Grounded
                && up_dot <= floor_cos
                && up_dot >= -floor_cos;

            if is_wall_surface {
                // wall_min_slide_angle: near-floor "wall" edges shouldn't slide
                // (jitter guard) — stop instead.
                if self.wall_min_slide_angle > 0.0 {
                    let angle_from_up = up_dot.clamp(-1.0, 1.0).acos();
                    if angle_from_up < self.wall_min_slide_angle {
                        remaining = Vector3::ZERO;
                        break;
                    }
                }
                // floor_block_on_wall: a grounded character cannot climb the wall
                // it walks into — drop any upward slide component.
                if was_on_floor && self.floor_block_on_wall {
                    let up = self.up_direction;
                    let up_comp = remaining.dot(up);
                    if up_comp > 0.0 {
                        remaining = remaining - up * up_comp;
                    }
                }
            }
        }

        remaining
    }

    /// Moves the character by `motion`, stopping at the first collision.
    ///
    /// Returns `Some(CollisionResult3D)` if a collision occurred, or `None`
    /// if the full motion completed without hitting anything.
    pub fn move_and_collide(
        &mut self,
        motion: Vector3,
        bodies: &[&PhysicsBody3D],
    ) -> Option<collision::CollisionResult3D> {
        let target = self.position + motion;

        let mut deepest: Option<collision::CollisionResult3D> = None;
        for body in bodies {
            if (self.collision_mask & body.collision_layer) == 0 {
                continue;
            }
            let result = collision::test_collision(target, &self.shape, body.position, &body.shape);
            if result.colliding
                && result.depth > 0.0
                && (deepest.is_none() || result.depth > deepest.as_ref().unwrap().depth)
            {
                deepest = Some(result);
            }
        }

        match deepest {
            None => {
                self.position = target;
                None
            }
            Some(result) => {
                let surface_normal = -result.normal;
                // ⚠️ `+ safe_margin`, not just the measured depth: Godot keeps a
            // small gap so the very next frame's overlap test does not start
            // from an exact-touch state, where a float rounding the wrong way
            // reads as a fresh penetration. Authoring `safe_margin = 0`
            // reproduces the old exact-contact behaviour (pat-s3rrv.103).
            self.position = target + surface_normal * (result.depth + self.safe_margin);
                Some(collision::CollisionResult3D {
                    colliding: true,
                    normal: surface_normal,
                    depth: result.depth,
                })
            }
        }
    }

    /// Probes for the floor body directly beneath the character.
    ///
    /// Returns the index (into `bodies`) of the floor body and the surface
    /// normal, or `None` when the character is not resting on a floor. The shape
    /// is offset a small margin downward so a resting (just-touching) contact is
    /// detected.
    fn probe_floor(&self, bodies: &[&PhysicsBody3D]) -> Option<(usize, Vector3)> {
        let probe_pos = self.position - self.up_direction * FLOOR_PROBE_MARGIN;
        let floor_cos = self.floor_max_angle.cos();
        let mut best: Option<(usize, f32, Vector3)> = None;

        for (i, body) in bodies.iter().enumerate() {
            if (self.collision_mask & body.collision_layer) == 0 {
                continue;
            }
            let r = collision::test_collision(probe_pos, &self.shape, body.position, &body.shape);
            if r.colliding && r.depth > 0.0 {
                let normal = -r.normal;
                if normal.dot(self.up_direction) > floor_cos
                    && (best.is_none() || r.depth > best.as_ref().unwrap().1)
                {
                    best = Some((i, r.depth, normal));
                }
            }
        }

        best.map(|(i, _, n)| (i, n))
    }

    /// Finds the deepest floor-angled contact at `probe_pos`, returning the
    /// surface normal and penetration depth.
    fn deepest_floor_contact(
        &self,
        probe_pos: Vector3,
        bodies: &[&PhysicsBody3D],
    ) -> Option<(Vector3, f32)> {
        let floor_cos = self.floor_max_angle.cos();
        let mut best: Option<(Vector3, f32)> = None;

        for body in bodies {
            if (self.collision_mask & body.collision_layer) == 0 {
                continue;
            }
            let r = collision::test_collision(probe_pos, &self.shape, body.position, &body.shape);
            if r.colliding && r.depth > 0.0 {
                let normal = -r.normal;
                if normal.dot(self.up_direction) > floor_cos
                    && (best.is_none() || r.depth > best.as_ref().unwrap().1)
                {
                    best = Some((normal, r.depth));
                }
            }
        }

        best
    }

    /// Casts the shape downward up to `floor_snap_length` to keep the character
    /// glued to slopes/stairs when it would otherwise leave the floor.
    ///
    /// ⚠️ `pub` because Godot exposes `apply_floor_snap()` as a script method a
    /// body can call on demand, not only as an internal phase of
    /// `move_and_slide` (pat-s3rrv.103).
    pub fn apply_floor_snap(
        &mut self,
        was_on_floor: bool,
        velocity: Vector3,
        bodies: &[&PhysicsBody3D],
    ) {
        if self.floor_snap_length <= 0.0 || self.on_floor {
            return;
        }
        // Never snap while moving upward relative to the up direction.
        let along_up = velocity.dot(self.up_direction);
        if along_up > 1e-6 {
            return;
        }
        // Only snap when we were grounded or are actively descending.
        let descending = along_up < -1e-6;
        if !was_on_floor && !descending {
            return;
        }

        self.snap_probe(bodies);
    }

    /// Godot's **manual** `apply_floor_snap()` (pat-s3rrv.103).
    ///
    /// ⚠️ Deliberately does NOT share the velocity guards above.
    /// `CharacterBody3D.xml` is explicit: *"Allows to manually apply a snap to
    /// the floor **regardless of the body's velocity**. This function does
    /// nothing when `is_on_floor()` returns true."*
    ///
    /// Those guards are right for the caller inside [`Self::move_and_slide`] —
    /// yanking a rising body downward mid-jump would be wrong — but they make
    /// the manual call a no-op in the case it exists for: a body standing
    /// still, whose velocity is zero and which was not already grounded. Only
    /// the `on_floor` and `floor_snap_length` conditions carry over, because
    /// those are the two the documentation keeps.
    pub fn apply_floor_snap_manual(&mut self, bodies: &[&PhysicsBody3D]) {
        if self.floor_snap_length <= 0.0 || self.on_floor {
            return;
        }
        self.snap_probe(bodies);
    }

    /// The downward probe both snap paths share.
    ///
    /// Walks outward from the body in `SNAP_SAMPLES` steps up to
    /// `floor_snap_length` and stops at the first floor contact, placing the
    /// body exactly on it. Extracted so the manual and automatic snaps cannot
    /// drift into finding the floor at different distances.
    fn snap_probe(&mut self, bodies: &[&PhysicsBody3D]) {
        for i in 1..=SNAP_SAMPLES {
            let t = self.floor_snap_length * (i as f32 / SNAP_SAMPLES as f32);
            let probe_pos = self.position - self.up_direction * t;
            if let Some((normal, depth)) = self.deepest_floor_contact(probe_pos, bodies) {
                self.position = probe_pos + normal * depth;
                self.on_floor = true;
                self.floor_normal = normal;
                return;
            }
        }
    }

    /// Classifies a collision normal as floor, wall, or ceiling.
    ///
    /// In [`MotionMode::Floating`] every surface is a wall.
    /// Sorts one contact into floor / wall / ceiling.
    ///
    /// `body` is the body that was struck, recorded whenever the contact is a
    /// WALL so the next `move_and_slide` can carry the character with it
    /// (`platform_wall_layers`). It is threaded in rather than read from a
    /// field because the classification is what decides the contact *is* a
    /// wall — the caller does not know until this returns.
    fn classify_surface(&mut self, normal: Vector3, body: Option<BodyId3D>) {
        if self.motion_mode == MotionMode::Floating {
            self.on_wall = true;
            self.wall_normal = normal;
            self.wall_body = body;
            return;
        }

        let dot = normal.dot(self.up_direction);
        let floor_cos = self.floor_max_angle.cos();
        if dot > floor_cos {
            self.on_floor = true;
            self.floor_normal = normal;
        } else if dot < -floor_cos {
            self.on_ceiling = true;
        } else {
            self.on_wall = true;
            self.wall_normal = normal;
            // Last wall wins, matching Godot: `platform_rid` is overwritten by
            // each collision, so the platform is the most recent contact.
            self.wall_body = body;
        }
    }

    /// Returns `true` if the character is touching a floor surface.
    pub fn is_on_floor(&self) -> bool {
        self.on_floor
    }

    /// Returns `true` if the character is touching a wall surface.
    pub fn is_on_wall(&self) -> bool {
        self.on_wall
    }

    /// Returns `true` if the character is touching a ceiling surface.
    pub fn is_on_ceiling(&self) -> bool {
        self.on_ceiling
    }

    /// Returns the floor normal from the last `move_and_slide` call.
    pub fn get_floor_normal(&self) -> Vector3 {
        self.floor_normal
    }

    /// Returns the wall normal from the last `move_and_slide` call.
    pub fn get_wall_normal(&self) -> Vector3 {
        self.wall_normal
    }

    /// Returns the velocity of the floor body the character stands on (the
    /// moving-platform velocity used for carry).
    pub fn get_platform_velocity(&self) -> Vector3 {
        self.platform_velocity
    }

    /// How many contacts the last `move_and_slide` recorded.
    pub fn get_slide_collision_count(&self) -> usize {
        self.slide_collisions.len()
    }

    /// The `i`th contact from the last `move_and_slide`.
    pub fn get_slide_collision(&self, i: usize) -> Option<&KinematicCollision3D> {
        self.slide_collisions.get(i)
    }

    /// Angular velocity of the platform at the last collision point (Godot's
    /// `get_platform_angular_velocity`). ZERO when not standing on anything.
    pub fn get_platform_angular_velocity(&self) -> Vector3 {
        self.platform_angular_velocity
    }

    /// The last sub-motion the slide loop actually achieved (Godot's
    /// `get_last_motion`).
    ///
    /// ⚠️ Distinct from the total travel: when the character slides along a
    /// wall the motion is split, and this is the FINAL segment — which is why
    /// Godot documents it as the way to read the current direction of travel.
    pub fn get_last_motion(&self) -> Vector3 {
        self.last_motion
    }

    /// Returns the angle (radians) between the floor normal and `up_direction`.
    ///
    /// `0` on flat ground; larger on steeper slopes.
    pub fn get_floor_angle(&self) -> f32 {
        self.floor_normal
            .dot(self.up_direction)
            .clamp(-1.0, 1.0)
            .acos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::{BodyId3D, BodyType3D};

    fn make_static_floor(y: f32) -> PhysicsBody3D {
        let mut body = PhysicsBody3D::new(
            BodyId3D(1),
            BodyType3D::Static,
            Vector3::new(0.0, y, 0.0),
            Shape3D::BoxShape {
                half_extents: Vector3::new(100.0, 1.0, 100.0),
            },
            1.0,
        );
        body.collision_layer = 1;
        body
    }

    fn make_static_wall(x: f32) -> PhysicsBody3D {
        let mut body = PhysicsBody3D::new(
            BodyId3D(2),
            BodyType3D::Static,
            Vector3::new(x, 0.0, 0.0),
            Shape3D::BoxShape {
                half_extents: Vector3::new(1.0, 100.0, 100.0),
            },
            1.0,
        );
        body.collision_layer = 1;
        body
    }

    fn make_static_ceiling(y: f32) -> PhysicsBody3D {
        let mut body = PhysicsBody3D::new(
            BodyId3D(3),
            BodyType3D::Static,
            Vector3::new(0.0, y, 0.0),
            Shape3D::BoxShape {
                half_extents: Vector3::new(100.0, 1.0, 100.0),
            },
            1.0,
        );
        body.collision_layer = 1;
        body
    }

    /// A 30-degree ramp: an infinite plane through the origin whose surface
    /// normal is tilted 30 degrees from vertical toward +X. The walkable
    /// (empty) side descends toward +X.
    fn make_slope_plane_30() -> PhysicsBody3D {
        let normal = Vector3::new(0.5, 0.866_025_4, 0.0);
        let mut body = PhysicsBody3D::new(
            BodyId3D(10),
            BodyType3D::Static,
            Vector3::ZERO,
            Shape3D::WorldBoundaryShape {
                normal,
                distance: 0.0,
            },
            1.0,
        );
        body.collision_layer = 1;
        body
    }

    #[test]
    fn move_and_slide_no_collision() {
        let mut character = CharacterBody3D::new(Vector3::ZERO, Shape3D::Sphere { radius: 1.0 });
        let bodies: Vec<&PhysicsBody3D> = vec![];
        let result = character.move_and_slide(Vector3::new(10.0, 0.0, 0.0), &bodies);
        assert!((character.position.x - 10.0).abs() < 1e-4);
        assert!(!character.is_on_floor());
        assert!(!character.is_on_wall());
        assert!((result.x - 10.0).abs() < 1e-4);
    }

    #[test]
    fn move_and_slide_hits_floor() {
        let mut character =
            CharacterBody3D::new(Vector3::new(0.0, 2.0, 0.0), Shape3D::Sphere { radius: 1.0 });
        // Floor box centered at y=-1 with half_extent.y=1 => top edge at y=0
        // Character at y=2, moves down by 2.5 => center at y=-0.5
        // Sphere center inside box => collision detected
        let floor = make_static_floor(-1.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&floor];

        let result = character.move_and_slide(Vector3::new(0.0, -2.5, 0.0), &bodies);
        assert!(character.is_on_floor(), "Should detect floor");
        assert!(!character.is_on_wall());
        assert!(result.y.abs() < 1e-3, "Y velocity should be zeroed");
    }

    #[test]
    fn move_and_slide_slides_along_wall() {
        let mut character = CharacterBody3D::new(Vector3::ZERO, Shape3D::Sphere { radius: 1.0 });
        // Wall box centered at x=5, half_extents (1, 100, 100) => left edge at x=4
        // Character moves right by 5 => center at x=5 (inside wall box)
        let wall = make_static_wall(5.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&wall];

        let result = character.move_and_slide(Vector3::new(5.0, 0.0, 3.0), &bodies);
        assert!(character.is_on_wall(), "Should detect wall");
        assert!(result.x.abs() < 1e-3, "X velocity should be zeroed by wall");
        assert!((result.z - 3.0).abs() < 1e-3, "Z velocity should remain");
    }

    #[test]
    fn move_and_slide_detects_ceiling() {
        let mut character =
            CharacterBody3D::new(Vector3::new(0.0, -2.0, 0.0), Shape3D::Sphere { radius: 1.0 });
        // Ceiling box centered at y=1, half_extents (100, 1, 100) => bottom edge at y=0
        // Character at y=-2, moves up by 2.5 => center at y=0.5 (inside ceiling box)
        let ceiling = make_static_ceiling(1.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&ceiling];

        let _result = character.move_and_slide(Vector3::new(0.0, 2.5, 0.0), &bodies);
        assert!(character.is_on_ceiling(), "Should detect ceiling");
        assert!(!character.is_on_floor());
    }

    /// `slide_on_ceiling = false` cancels a jump into the ceiling
    /// (pat-s3rrv.103, `character_body_3d.cpp:314`).
    #[test]
    fn slide_on_ceiling_false_cancels_an_upward_jump() {
        // Ceiling bottom edge sits at y = 0; the character starts below it and
        // is driven UP and sideways, so it arrives while moving upward.
        let ceiling = make_static_ceiling(1.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&ceiling];

        let mut sliding = CharacterBody3D::new(
            Vector3::new(0.0, -2.0, 0.0),
            Shape3D::Sphere { radius: 1.0 },
        );
        sliding.move_and_slide(Vector3::new(3.0, 2.5, 0.0), &bodies);

        let mut stopped = CharacterBody3D::new(
            Vector3::new(0.0, -2.0, 0.0),
            Shape3D::Sphere { radius: 1.0 },
        );
        stopped.slide_on_ceiling = false;
        stopped.move_and_slide(Vector3::new(3.0, 2.5, 0.0), &bodies);

        assert!(
            sliding.is_on_ceiling() && stopped.is_on_ceiling(),
            "both fixtures must actually reach the ceiling, or the comparison \
             below is between two bodies that simply flew apart"
        );
        // The control: with sliding enabled the sideways component survives.
        assert!(
            sliding.position.x > 1.0,
            "slide_on_ceiling = true must let the body skate sideways, got x = {}",
            sliding.position.x
        );
        assert!(
            stopped.position.x < sliding.position.x,
            "slide_on_ceiling = false must stop the body instead of sliding: \
             x = {} vs {} with sliding on",
            stopped.position.x,
            sliding.position.x
        );
    }

    /// 🔑 The `vel_dir_facing_up` half of the rule.
    ///
    /// ⚠️ A body touching the ceiling while moving DOWNWARD is not "jumping
    /// against" it, and Godot does not cancel — the term is what stops a
    /// character freezing whenever its head brushes geometry. A naive
    /// `if on_ceiling && !slide_on_ceiling { stop }` passes the jump test above
    /// and fails here.
    #[test]
    fn slide_on_ceiling_false_still_slides_when_not_moving_up() {
        let ceiling = make_static_ceiling(1.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&ceiling];

        // Starts already penetrating the ceiling (top at y = 1.5 against a
        // bottom edge of y = 0), so contact is guaranteed on the first step,
        // then driven sideways and DOWN.
        let mut body =
            CharacterBody3D::new(Vector3::new(0.0, 0.5, 0.0), Shape3D::Sphere { radius: 1.0 });
        body.slide_on_ceiling = false;
        body.move_and_slide(Vector3::new(3.0, -0.5, 0.0), &bodies);

        assert!(
            body.position.x > 1.0,
            "moving downward, the cancel must NOT apply even with \
             slide_on_ceiling = false; the body only reached x = {} — the rule \
             is firing regardless of direction",
            body.position.x
        );
    }

    /// 🔑 `platform_floor_layers` gates the CARRY, not the floor
    /// (pat-s3rrv.103, `CharacterBody3D.xml:168`).
    ///
    /// ⚠️ The third assertion is the one that matters. Godot masks which floor
    /// bodies "act as moving platforms to be followed" — a body on an excluded
    /// layer is still a floor you stand on, it just does not drag you. An
    /// implementation that skipped the body during the floor probe would pass
    /// the first two assertions and drop the character through the platform.
    #[test]
    fn platform_floor_layers_gates_the_moving_platform_carry() {
        // The shared helper puts the floor on collision layer 1.
        let mut platform = make_static_floor(-1.0);
        platform.linear_velocity = Vector3::new(5.0, 0.0, 0.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&platform];

        // Default mask is every layer, so this one is carried.
        let mut carried =
            CharacterBody3D::new(Vector3::new(0.0, 2.0, 0.0), Shape3D::Sphere { radius: 1.0 });
        carried.move_and_slide(Vector3::new(0.0, -2.5, 0.0), &bodies);
        assert!(carried.is_on_floor(), "the carried fixture must land first");
        carried.move_and_slide(Vector3::ZERO, &bodies);

        // Layer 2 only — the layer-1 platform is excluded from the carry.
        let mut standing =
            CharacterBody3D::new(Vector3::new(0.0, 2.0, 0.0), Shape3D::Sphere { radius: 1.0 });
        standing.platform_floor_layers = 2;
        standing.move_and_slide(Vector3::new(0.0, -2.5, 0.0), &bodies);
        assert!(standing.is_on_floor(), "the masked fixture must land too");
        standing.move_and_slide(Vector3::ZERO, &bodies);

        // Control: with the layer included, a zero input motion still moves the
        // character, because the platform's velocity is what drives it.
        assert!(
            carried.position.x > 1.0,
            "an included platform must carry the character even with zero input \
             motion; it only reached x = {}",
            carried.position.x
        );
        assert!(
            standing.position.x.abs() < 1e-3,
            "an excluded platform must NOT carry it, but it moved to x = {}",
            standing.position.x
        );
        assert!(
            standing.is_on_floor(),
            "🔑 the mask gates the CARRY, not the floor — the character must \
             still be standing on the excluded platform, not falling through it"
        );
    }

    /// 🔑 `platform_wall_layers` lets a MOVING WALL carry the character, and
    /// defaults to ignoring every wall (pat-s3rrv.103,
    /// `character_body_3d.cpp:60`, `CharacterBody3D.xml`).
    ///
    /// ⚠️ **The default half is the load-bearing half.** Godot's default is
    /// `0` — "all wall bodies are ignored" — the exact opposite of
    /// `platform_floor_layers`'s `4294967295`. The `ignored` fixture below
    /// touches nothing but the default, so it fails the moment someone
    /// "harmonises" the two masks, which is the single most likely way to break
    /// this: every wall would start dragging the player sideways.
    ///
    /// ⚠️ No floor in `bodies`, deliberately. Godot tests the wall mask only in
    /// the `else if (collision_state.wall)` branch — floor wins — so a fixture
    /// standing on a floor would exercise the floor path no matter what this
    /// mask says.
    #[test]
    fn platform_wall_layers_gates_the_moving_wall_carry() {
        // The shared helper puts the wall on collision layer 1; it slides along
        // +Z, a direction the character cannot reach on its own here.
        let mut wall = make_static_wall(3.0);
        wall.linear_velocity = Vector3::new(0.0, 0.0, 4.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&wall];

        // Opted in: layer 1 is in the mask, so this wall is a moving platform.
        let mut carried = CharacterBody3D::new(Vector3::ZERO, Shape3D::Sphere { radius: 1.0 });
        carried.platform_wall_layers = 1;
        carried.move_and_slide(Vector3::new(2.5, 0.0, 0.0), &bodies);
        assert!(
            carried.is_on_wall(),
            "the carried fixture must reach the wall first, or the carry below \
             has no contact to key off and the test proves nothing"
        );
        carried.move_and_slide(Vector3::ZERO, &bodies);

        // Untouched default (0) — Godot ignores every wall body.
        let mut ignored = CharacterBody3D::new(Vector3::ZERO, Shape3D::Sphere { radius: 1.0 });
        ignored.move_and_slide(Vector3::new(2.5, 0.0, 0.0), &bodies);
        assert!(
            ignored.is_on_wall(),
            "the ignored fixture must reach the wall too"
        );
        ignored.move_and_slide(Vector3::ZERO, &bodies);

        assert!(
            carried.position.z > 0.5,
            "an included moving wall must carry the character along +Z even \
             with zero input motion; it only reached z = {}",
            carried.position.z
        );
        assert!(
            ignored.position.z.abs() < 1e-3,
            "🔑 with the DEFAULT mask of 0 no wall may carry anything, but the \
             character drifted to z = {}. A non-zero value here means the wall \
             mask was given the floor mask's all-layers default",
            ignored.position.z
        );
    }

    #[test]
    fn move_and_slide_respects_collision_mask() {
        let mut character = CharacterBody3D::new(Vector3::ZERO, Shape3D::Sphere { radius: 1.0 });
        character.collision_mask = 2; // Only scan layer 2

        let mut wall = make_static_wall(3.0);
        wall.collision_layer = 1; // Wall is on layer 1 — shouldn't collide
        let bodies: Vec<&PhysicsBody3D> = vec![&wall];

        let _result = character.move_and_slide(Vector3::new(10.0, 0.0, 0.0), &bodies);
        assert!(!character.is_on_wall());
        assert!((character.position.x - 10.0).abs() < 1e-4);
    }

    #[test]
    fn move_and_collide_no_collision() {
        let mut character = CharacterBody3D::new(Vector3::ZERO, Shape3D::Sphere { radius: 1.0 });
        let bodies: Vec<&PhysicsBody3D> = vec![];
        let result = character.move_and_collide(Vector3::new(5.0, 0.0, 0.0), &bodies);
        assert!(result.is_none());
        assert!((character.position.x - 5.0).abs() < 1e-4);
    }

    #[test]
    fn move_and_collide_stops_at_wall() {
        let mut character = CharacterBody3D::new(Vector3::ZERO, Shape3D::Sphere { radius: 1.0 });
        // Wall at x=5 with half_extent 1 => left edge at x=4
        // Move right by 5 => center at x=5, inside the wall box
        let wall = make_static_wall(5.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&wall];

        let result = character.move_and_collide(Vector3::new(5.0, 0.0, 0.0), &bodies);
        assert!(result.is_some());
    }

    #[test]
    fn get_floor_normal_after_landing() {
        let mut character =
            CharacterBody3D::new(Vector3::new(0.0, 2.0, 0.0), Shape3D::Sphere { radius: 1.0 });
        let floor = make_static_floor(-1.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&floor];

        character.move_and_slide(Vector3::new(0.0, -2.5, 0.0), &bodies);
        let normal = character.get_floor_normal();
        assert!(
            normal.y > 0.5,
            "Floor normal should point up (positive Y), got {:?}",
            normal
        );
    }

    #[test]
    fn zero_velocity_does_not_move() {
        let mut character =
            CharacterBody3D::new(Vector3::new(1.0, 2.0, 3.0), Shape3D::Sphere { radius: 1.0 });
        let bodies: Vec<&PhysicsBody3D> = vec![];
        let result = character.move_and_slide(Vector3::ZERO, &bodies);
        assert!((character.position.x - 1.0).abs() < 1e-6);
        assert!((character.position.y - 2.0).abs() < 1e-6);
        assert!((character.position.z - 3.0).abs() < 1e-6);
        assert!(result.length_squared() < 1e-8);
    }

    // -- move_and_slide parity (pat-u3l8c.58) --------------------------------

    #[test]
    fn character_snaps_to_floor_on_slope() {
        // A character resting on a 30-degree slope that moves horizontally
        // (toward the descending side) stays glued to the slope thanks to
        // floor snapping; with floor_snap_length = 0 it launches off.
        //
        // Resting sphere (radius 1) center at x=0 sits at y = 1 / cos(30) ~= 1.1547.
        let run = |snap_len: f32| -> (bool, f32) {
            let slope = make_slope_plane_30();
            let bodies: Vec<&PhysicsBody3D> = vec![&slope];
            let mut c = CharacterBody3D::new(
                Vector3::new(0.0, 1.154_700_5, 0.0),
                Shape3D::Sphere { radius: 1.0 },
            );
            c.floor_snap_length = snap_len;
            // Move horizontally (descending side) with a touch of gravity.
            c.move_and_slide(Vector3::new(0.1, -0.01, 0.0), &bodies);
            (c.is_on_floor(), c.position.y)
        };

        let (snapped_floor, snapped_y) = run(0.2);
        let (nosnap_floor, nosnap_y) = run(0.0);

        assert!(snapped_floor, "with snapping the character stays on the floor");
        assert!(!nosnap_floor, "without snapping the character leaves the floor");
        assert!(
            snapped_y < nosnap_y - 1e-3,
            "snapped y tracks the descending slope (lower) than the floating y: {snapped_y} vs {nosnap_y}"
        );
    }

    #[test]
    fn character_carried_by_moving_platform() {
        // A floor body with a nonzero velocity carries the resting character.
        let mut platform = PhysicsBody3D::new(
            BodyId3D(20),
            BodyType3D::Kinematic,
            Vector3::new(0.0, -1.0, 0.0),
            Shape3D::BoxShape {
                half_extents: Vector3::new(100.0, 1.0, 100.0),
            },
            1.0,
        );
        platform.collision_layer = 1;
        platform.linear_velocity = Vector3::new(2.0, 0.0, 0.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&platform];

        // Sphere (radius 1) resting on the platform top (y=0).
        let mut c = CharacterBody3D::new(Vector3::new(0.0, 1.0, 0.0), Shape3D::Sphere { radius: 1.0 });
        // No own velocity — the platform alone should move the character.
        c.move_and_slide(Vector3::ZERO, &bodies);

        assert!(
            (c.position.x - 2.0).abs() < 1e-3,
            "character carried by platform velocity, got x = {}",
            c.position.x
        );
        assert!(
            (c.get_platform_velocity().x - 2.0).abs() < 1e-6,
            "platform velocity is exposed, got {:?}",
            c.get_platform_velocity()
        );
    }

    #[test]
    fn character_platform_on_leave_add_velocity() {
        let bodies: Vec<&PhysicsBody3D> = vec![];

        // AddVelocity: the outgoing velocity retains the full platform velocity.
        let mut c = CharacterBody3D::new(Vector3::new(0.0, 1.0, 0.0), Shape3D::Sphere { radius: 1.0 });
        c.platform_on_leave = PlatformOnLeave::AddVelocity;
        // Simulate "was standing on a platform moving at +2 X last frame".
        c.on_floor = true;
        c.platform_velocity = Vector3::new(2.0, 0.0, 0.0);
        let out = c.move_and_slide(Vector3::new(0.0, 5.0, 0.0), &bodies); // jump straight up
        assert!(!c.is_on_floor(), "character left the floor");
        assert!(
            (out.x - 2.0).abs() < 1e-4,
            "AddVelocity keeps the platform velocity, got {:?}",
            out
        );
        assert!((out.y - 5.0).abs() < 1e-4, "own jump velocity preserved");

        // DoNotAddVelocity: the outgoing velocity keeps nothing from the platform.
        let mut c2 =
            CharacterBody3D::new(Vector3::new(0.0, 1.0, 0.0), Shape3D::Sphere { radius: 1.0 });
        c2.platform_on_leave = PlatformOnLeave::DoNotAddVelocity;
        c2.on_floor = true;
        c2.platform_velocity = Vector3::new(2.0, 0.0, 0.0);
        let out2 = c2.move_and_slide(Vector3::new(0.0, 5.0, 0.0), &bodies);
        assert!(
            out2.x.abs() < 1e-4,
            "DoNotAddVelocity keeps nothing from the platform, got {:?}",
            out2
        );
    }

    #[test]
    fn character_platform_on_leave_add_upward_velocity() {
        let bodies: Vec<&PhysicsBody3D> = vec![];
        // AddUpwardVelocity keeps only the up-axis component of the platform.
        let mut c = CharacterBody3D::new(Vector3::new(0.0, 1.0, 0.0), Shape3D::Sphere { radius: 1.0 });
        c.platform_on_leave = PlatformOnLeave::AddUpwardVelocity;
        c.on_floor = true;
        c.platform_velocity = Vector3::new(2.0, 3.0, 0.0); // moving up-and-sideways
        let out = c.move_and_slide(Vector3::new(0.0, 5.0, 0.0), &bodies);
        assert!(out.x.abs() < 1e-4, "sideways platform component dropped, got {:?}", out);
        assert!(
            (out.y - 8.0).abs() < 1e-4,
            "only the upward platform component (3) is added to the jump (5), got {:?}",
            out
        );
    }

    #[test]
    fn character_motion_mode_floating_ignores_floor() {
        let mut character =
            CharacterBody3D::new(Vector3::new(0.0, 2.0, 0.0), Shape3D::Sphere { radius: 1.0 });
        character.motion_mode = MotionMode::Floating;
        let floor = make_static_floor(-1.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&floor];

        character.move_and_slide(Vector3::new(0.0, -2.5, 0.0), &bodies);
        assert!(
            !character.is_on_floor(),
            "Floating mode must not register floor contacts"
        );
        assert!(
            character.is_on_wall(),
            "Floating mode treats the surface as a wall"
        );
    }

    #[test]
    fn character_max_slides_is_configurable() {
        let floor = make_static_floor(-1.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&floor];

        // max_slides = 0: the slide loop never runs, so the character neither
        // moves via sliding nor resolves the floor.
        let mut c0 =
            CharacterBody3D::new(Vector3::new(0.0, 2.0, 0.0), Shape3D::Sphere { radius: 1.0 });
        c0.max_slides = 0;
        c0.move_and_slide(Vector3::new(0.0, -2.5, 0.0), &bodies);
        assert!(
            (c0.position.y - 2.0).abs() < 1e-6,
            "with max_slides = 0 the slide loop is a no-op, got y = {}",
            c0.position.y
        );
        assert!(!c0.is_on_floor(), "no slide iterations means no floor contact");

        // max_slides = 6 (default): the floor is resolved.
        let mut c6 =
            CharacterBody3D::new(Vector3::new(0.0, 2.0, 0.0), Shape3D::Sphere { radius: 1.0 });
        c6.max_slides = 6;
        c6.move_and_slide(Vector3::new(0.0, -2.5, 0.0), &bodies);
        assert!(
            c6.is_on_floor(),
            "with slide iterations available the floor is resolved"
        );
        assert!(c6.position.y < 2.0, "the character moved toward the floor");
    }

    #[test]
    fn floor_stop_on_slope_keeps_character_still() {
        // A character resting on a 30-degree slope with only gravity (no
        // horizontal input) should not slide downhill when floor_stop_on_slope
        // is enabled; with it disabled it slides toward the descending side.
        let run = |stop: bool| -> f32 {
            let slope = make_slope_plane_30();
            let bodies: Vec<&PhysicsBody3D> = vec![&slope];
            let mut c = CharacterBody3D::new(
                Vector3::new(0.0, 1.154_700_5, 0.0),
                Shape3D::Sphere { radius: 1.0 },
            );
            c.floor_stop_on_slope = stop;
            c.floor_snap_length = 0.2;
            c.move_and_slide(Vector3::new(0.0, -0.1, 0.0), &bodies); // gravity only
            c.position.x
        };

        let stop_x = run(true);
        let slide_x = run(false);

        assert!(
            stop_x.abs() < 0.02,
            "floor_stop_on_slope keeps the character in place, got x = {stop_x}"
        );
        assert!(
            slide_x > 0.05,
            "without floor_stop_on_slope the character slides downhill, got x = {slide_x}"
        );
        assert!(stop_x < slide_x, "stopping must move less than sliding");
    }

    #[test]
    fn get_floor_angle_flat_floor_is_zero() {
        let mut c =
            CharacterBody3D::new(Vector3::new(0.0, 2.0, 0.0), Shape3D::Sphere { radius: 1.0 });
        let floor = make_static_floor(-1.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&floor];
        c.move_and_slide(Vector3::new(0.0, -2.5, 0.0), &bodies);
        assert!(c.is_on_floor());
        assert!(
            c.get_floor_angle() < 0.05,
            "flat floor angle should be ~0, got {}",
            c.get_floor_angle()
        );
    }
}

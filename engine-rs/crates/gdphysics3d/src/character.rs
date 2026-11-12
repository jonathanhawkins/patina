//! CharacterBody3D — kinematic character controller for 3D.
//!
//! Provides [`CharacterBody3D`] with `move_and_slide` for 3D movement that
//! slides along surfaces and detects floor/wall/ceiling contacts.
//!
//! A line-for-line port of Godot 4.6's `CharacterBody3D::move_and_slide`
//! (`scene/3d/physics/character_body_3d.cpp`: `_move_and_slide_grounded`,
//! `_move_and_slide_floating`, `apply_floor_snap`, `_set_collision_direction`)
//! and `PhysicsBody3D::move_and_collide`, over the Jolt `body_test_motion` port
//! in [`crate::jolt_motion`] (pat-kiuul).
//!
//! It replaced a sub-stepped push-out solver that matched Godot's outcomes but
//! not its numbers: a walking body rested at exactly `floor + safe_margin`
//! (-3.9990 on the 3D platformer) where Godot rests 1e-4 above the floor
//! (-3.9999), and a landing lost horizontal speed differently (dx 0.0947 vs
//! 0.0956 at the platformer's f195), which grew to 1e-2 within 40 frames.

use gdcore::math::Vector3;
use gdcore::math3d::Quaternion;

use crate::body::{BodyId3D, PhysicsBody3D};
use crate::collision;
use crate::jolt_motion::{
    body_test_motion, MotionCollision, MotionParameters, MotionResult, MotionShape, CMP_EPSILON,
};
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
    /// Where the shape sits relative to [`position`](Self::position): the
    /// `CollisionShape3D`'s transform under the body (pat-2ifdx).
    ///
    /// 🔑 A stock character raises its capsule by half its height so the body
    /// origin is at the FEET (the official 3D platformer authors `0, 1.05, 0`
    /// for a 2.1 m capsule). Testing the capsule at the origin instead put it
    /// half a body too low, so the character came to rest 1.05 m above where
    /// Godot stands it.
    pub shape_offset: Vector3,
    /// The shape's rotation relative to the body.
    pub shape_rotation: Quaternion,
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
    /// The body behind the floor (or wall) contact of the last
    /// `move_and_slide` — Godot's `platform_rid`, captured during one call and
    /// re-read at the top of the NEXT one for its current velocity.
    platform_body: Option<BodyId3D>,
    /// That body's collision layer (Godot's `platform_layer`), tested against
    /// `platform_floor_layers` / `platform_wall_layers`.
    platform_layer: u32,
    /// Velocity of the ceiling body hit during the last call (Godot's
    /// `platform_ceiling_velocity`).
    platform_ceiling_velocity: Vector3,
    /// Normal of the last ceiling contact.
    ceiling_normal: Vector3,
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
    /// Bodies Godot would hold as ONE body — a GridMap octant — by group key
    /// (pat-kiuul). Jolt removes internal edges per body, so cells of one
    /// octant smooth each other's seams and cells of two octants do not.
    /// `None`: every body is its own.
    pub body_groups: Option<std::sync::Arc<std::collections::HashMap<BodyId3D, u64>>>,
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
    /// The contact point (Godot's `KinematicCollision3D.get_position()`).
    pub position: Vector3,
    /// Motion left unapplied after this contact.
    pub remainder: Vector3,
}

/// Floor angle threshold (approximately 45 degrees).
const DEFAULT_FLOOR_MAX_ANGLE: f32 = std::f32::consts::FRAC_PI_4;

/// Default number of slide iterations per motion step (Godot 4 default).
const DEFAULT_MAX_SLIDES: u32 = 6;

/// Default floor snap length (Godot 4 default).
const DEFAULT_FLOOR_SNAP_LENGTH: f32 = 0.1;

/// Default minimum wall slide angle: 15 degrees in radians (Godot 4 default).
const DEFAULT_WALL_MIN_SLIDE_ANGLE: f32 = 0.2617994;

/// Godot's `FLOOR_ANGLE_THRESHOLD`: "so, if you pass 45 as limit, avoid
/// numerical precision errors when angle is 45".
const FLOOR_ANGLE_THRESHOLD: f32 = 0.01;

/// Godot's `Vector3::is_zero_approx`.
fn is_zero_approx(v: Vector3) -> bool {
    v.x.abs() < CMP_EPSILON && v.y.abs() < CMP_EPSILON && v.z.abs() < CMP_EPSILON
}

/// Godot's `Vector3::slide`.
fn slide(v: Vector3, n: Vector3) -> Vector3 {
    v - n * v.dot(n)
}

/// Godot's `Vector3::normalized` (zero stays zero).
fn normalized(v: Vector3) -> Vector3 {
    let l = v.length();
    if l == 0.0 {
        Vector3::ZERO
    } else {
        v / l
    }
}

/// Godot's `Vector3::is_equal_approx` (per component, relative tolerance).
fn is_equal_approx(a: Vector3, b: Vector3) -> bool {
    let eq = |x: f32, y: f32| {
        if x == y {
            return true;
        }
        let tol = (CMP_EPSILON * x.abs()).max(CMP_EPSILON);
        (x - y).abs() < tol
    };
    eq(a.x, b.x) && eq(a.y, b.y) && eq(a.z, b.z)
}

/// Godot's `CharacterBody3D::CollisionState`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct CollisionState {
    floor: bool,
    wall: bool,
    ceiling: bool,
}

impl CollisionState {
    const ALL: CollisionState = CollisionState {
        floor: true,
        wall: true,
        ceiling: true,
    };
}

impl CharacterBody3D {
    /// Creates a new character body at the given position.
    pub fn new(position: Vector3, shape: Shape3D) -> Self {
        Self {
            position,
            shape,
            shape_offset: Vector3::ZERO,
            shape_rotation: Quaternion::IDENTITY,
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
            ceiling_normal: Vector3::ZERO,
            platform_floor_layers: u32::MAX,
            platform_wall_layers: 0,
            platform_body: None,
            platform_layer: 0,
            platform_ceiling_velocity: Vector3::ZERO,
            slide_collisions: Vec::new(),
            slide_on_ceiling: true,
            // Godot CharacterBody3D.safe_margin default.
            safe_margin: 0.001,
            platform_velocity: Vector3::ZERO,
            platform_angular_velocity: Vector3::ZERO,
            last_motion: Vector3::ZERO,
            body_groups: None,
        }
    }

    /// The shape as the motion query sees it.
    fn motion_shape(&self) -> MotionShape<'_> {
        MotionShape {
            shape: &self.shape,
            offset: self.shape_offset,
            rotation: self.shape_rotation,
            collision_mask: self.collision_mask,
            groups: self.body_groups.as_deref(),
        }
    }

    /// Godot's `PhysicsBody3D::move_and_collide(parameters, result, test_only,
    /// cancel_sliding)` (`scene/3d/physics/physics_body_3d.cpp`).
    fn godot_move_and_collide(
        &mut self,
        params: &MotionParameters,
        bodies: &[&PhysicsBody3D],
        test_only: bool,
        cancel_sliding: bool,
    ) -> (bool, MotionResult) {
        let (colliding, mut r) = body_test_motion(&self.motion_shape(), bodies, params);

        // Restore direction of motion to be along original motion, in order to
        // avoid sliding due to recovery, but only if collision depth is low
        // enough to avoid tunneling.
        let mut cancel_sliding = cancel_sliding;
        if cancel_sliding {
            let motion_length = params.motion.length();
            let mut precision = 0.001f32;
            if colliding {
                // Can't just use margin as a threshold because collision depth
                // is calculated on unsafe motion, so even in normal resting
                // cases the depth can be a bit more than the margin.
                precision +=
                    motion_length * (r.collision_unsafe_fraction - r.collision_safe_fraction);
                if r.collisions[0].depth > params.margin + precision {
                    cancel_sliding = false;
                }
            }
            if cancel_sliding {
                // When motion is null, recovery is the resulting motion.
                let motion_normal = if motion_length > CMP_EPSILON {
                    params.motion / motion_length
                } else {
                    Vector3::ZERO
                };
                // Check depth of recovery.
                let projected_length = r.travel.dot(motion_normal);
                let recovery = r.travel - motion_normal * projected_length;
                let recovery_length = recovery.length();
                // Fixes cases where canceling slide causes the motion to go too
                // deep into the ground, because we're only taking rest
                // information into account and not general recovery.
                if recovery_length < params.margin + precision {
                    // Apply adjustment to motion.
                    r.travel = motion_normal * projected_length;
                    r.remainder = params.motion - r.travel;
                }
            }
        }

        if !test_only {
            self.position = params.from + r.travel;
        }
        (colliding, r)
    }

    /// Records one `MotionResult` as Godot's `motion_results.push_back`.
    fn push_motion_result(&mut self, r: &MotionResult) {
        let c = &r.collisions[0];
        self.slide_collisions.push(KinematicCollision3D {
            collider: Some(c.collider),
            normal: c.normal,
            depth: r.collision_depth,
            position: c.position,
            remainder: r.remainder,
        });
    }

    /// Moves the character by `velocity`, sliding along colliding surfaces.
    ///
    /// `velocity` is this frame's motion (a per-frame displacement): it is
    /// [`Self::move_and_slide_delta`] with `delta = 1`. Returns the resulting
    /// motion after sliding.
    pub fn move_and_slide(&mut self, velocity: Vector3, bodies: &[&PhysicsBody3D]) -> Vector3 {
        self.move_and_slide_delta(velocity, 1.0, bodies)
    }

    /// Godot's `CharacterBody3D::move_and_slide()`
    /// (`scene/3d/physics/character_body_3d.cpp`), line for line, over the
    /// Jolt motion query in [`crate::jolt_motion`] (pat-kiuul).
    ///
    /// `velocity` is per second and `delta` the physics step; returns the
    /// velocity the body leaves with (Godot writes it back to `velocity`).
    pub fn move_and_slide_delta(
        &mut self,
        velocity: Vector3,
        delta: f32,
        bodies: &[&PhysicsBody3D],
    ) -> Vector3 {
        let mut velocity = velocity;
        let mut current_platform_velocity = self.platform_velocity;

        if (self.on_floor || self.on_wall) && self.platform_body.is_some() {
            let excluded = if self.on_floor {
                (self.platform_floor_layers & self.platform_layer) == 0
            } else {
                (self.platform_wall_layers & self.platform_layer) == 0
            };
            if !excluded {
                // This approach makes sure there is less delay between the
                // actual body velocity and the one we saved.
                match bodies.iter().find(|b| Some(b.id) == self.platform_body) {
                    Some(b) => {
                        let local_position = self.position - b.position;
                        current_platform_velocity =
                            b.linear_velocity + b.angular_velocity.cross(local_position);
                    }
                    None => {
                        // Body is removed or destroyed, invalidate floor.
                        current_platform_velocity = Vector3::ZERO;
                        self.platform_body = None;
                    }
                }
            } else {
                current_platform_velocity = Vector3::ZERO;
            }
        }

        self.slide_collisions.clear();

        let was_on_floor = self.on_floor;
        self.on_floor = false;
        self.on_wall = false;
        self.on_ceiling = false;

        self.last_motion = Vector3::ZERO;

        if !is_zero_approx(current_platform_velocity) {
            let mut params = MotionParameters::new(
                self.position,
                current_platform_velocity * delta,
                self.safe_margin,
            );
            // Also report collisions generated only from recovery.
            params.recovery_as_collision = true;
            if let Some(id) = self.platform_body {
                params.exclude_bodies.push(id);
            }
            let (collided, floor_result) =
                self.godot_move_and_collide(&params, bodies, false, false);
            if collided {
                self.push_motion_result(&floor_result);
                self.set_collision_direction(&floor_result, CollisionState::ALL, bodies);
            }
        }

        if self.motion_mode == MotionMode::Grounded {
            self.move_and_slide_grounded(delta, was_on_floor, &mut velocity, bodies);
        } else {
            self.move_and_slide_floating(delta, &mut velocity, bodies);
        }

        match self.platform_on_leave {
            PlatformOnLeave::DoNotAddVelocity => {}
            mode => {
                // Add last platform velocity when just left a moving platform.
                if !self.on_floor && !self.on_wall {
                    if mode == PlatformOnLeave::AddUpwardVelocity
                        && current_platform_velocity.dot(self.up_direction) < 0.0
                    {
                        current_platform_velocity =
                            slide(current_platform_velocity, self.up_direction);
                    }
                    velocity = velocity + current_platform_velocity;
                }
            }
        }

        velocity
    }

    /// Godot's `_move_and_slide_grounded`.
    fn move_and_slide_grounded(
        &mut self,
        delta: f32,
        was_on_floor: bool,
        velocity: &mut Vector3,
        bodies: &[&PhysicsBody3D],
    ) {
        let up = self.up_direction;
        let margin = self.safe_margin;
        let mut motion = *velocity * delta;
        let motion_slide_up = slide(motion, up);
        let prev_floor_normal = self.floor_normal;

        self.platform_body = None;
        self.platform_velocity = Vector3::ZERO;
        self.platform_angular_velocity = Vector3::ZERO;
        self.platform_ceiling_velocity = Vector3::ZERO;
        self.floor_normal = Vector3::ZERO;
        self.wall_normal = Vector3::ZERO;
        self.ceiling_normal = Vector3::ZERO;

        // No sliding on first attempt to keep floor motion stable when
        // possible, when stop on slope is enabled or when there is no up
        // direction.
        let mut sliding_enabled = !self.floor_stop_on_slope;
        // Constant speed can be applied only the first time sliding is enabled.
        let mut can_apply_constant_speed = sliding_enabled;
        // If the platform's ceiling push down the body.
        let mut apply_ceiling_velocity = false;
        let mut first_slide = true;
        let vel_dir_facing_up = velocity.dot(up) > 0.0;
        let mut total_travel = Vector3::ZERO;

        for _iteration in 0..self.max_slides {
            let mut params = MotionParameters::new(self.position, motion, margin);
            // There can be 4 collisions between 2 walls + 2 more for the floor.
            params.max_collisions = 6;
            // Also report collisions generated only from recovery.
            params.recovery_as_collision = true;

            let (mut collided, mut result) =
                self.godot_move_and_collide(&params, bodies, false, !sliding_enabled);

            self.last_motion = result.travel;

            if collided {
                self.push_motion_result(&result);

                let previous_state = CollisionState {
                    floor: self.on_floor,
                    wall: self.on_wall,
                    ceiling: self.on_ceiling,
                };

                let result_state =
                    self.set_collision_direction(&result, CollisionState::ALL, bodies);

                // If we hit a ceiling platform, we set the vertical velocity to
                // at least the platform one.
                if self.on_ceiling
                    && self.platform_ceiling_velocity != Vector3::ZERO
                    && self.platform_ceiling_velocity.dot(up) < 0.0
                {
                    // If ceiling sliding is on, only apply when the ceiling is
                    // flat or when the motion is upward.
                    if !self.slide_on_ceiling
                        || motion.dot(up) < 0.0
                        || (self.ceiling_normal + up).length() < 0.01
                    {
                        apply_ceiling_velocity = true;
                        let ceiling_vertical_velocity = up * up.dot(self.platform_ceiling_velocity);
                        let motion_vertical_velocity = up * up.dot(*velocity);
                        if motion_vertical_velocity.dot(up) > 0.0
                            || ceiling_vertical_velocity.length_squared()
                                > motion_vertical_velocity.length_squared()
                        {
                            *velocity = ceiling_vertical_velocity + slide(*velocity, up);
                        }
                    }
                }

                if self.on_floor
                    && self.floor_stop_on_slope
                    && (normalized(*velocity) + up).length() < 0.01
                {
                    if result.travel.length() <= margin + CMP_EPSILON {
                        self.position = self.position - result.travel;
                    }
                    *velocity = Vector3::ZERO;
                    self.last_motion = Vector3::ZERO;
                    break;
                }

                // Godot also zeroes `motion` here; nothing reads it after the break.
                if is_zero_approx(result.remainder) {
                    break;
                }

                // Apply regular sliding by default.
                let mut apply_default_sliding = true;

                // Wall collision checks.
                if result_state.wall && motion_slide_up.dot(self.wall_normal) <= 0.0 {
                    // Move on floor only checks.
                    if self.floor_block_on_wall {
                        // Needs horizontal motion from current motion instead of
                        // motion_slide_up to properly test the angle and avoid
                        // standing on slopes.
                        let horizontal_motion = slide(motion, up);
                        let horizontal_normal = normalized(slide(self.wall_normal, up));
                        let motion_angle = (-horizontal_normal.dot(normalized(horizontal_motion)))
                            .clamp(-1.0, 1.0)
                            .acos()
                            .abs();

                        // Avoid to move forward on a wall if floor_block_on_wall
                        // is true. Applies only when the motion angle is under 90
                        // degrees, in order to avoid blocking lateral motion along
                        // a wall.
                        if motion_angle < 0.5 * std::f32::consts::PI {
                            apply_default_sliding = false;
                            if was_on_floor && !vel_dir_facing_up {
                                // Cancel the motion.
                                let travel_total = result.travel.length();
                                let cancel_dist_max = (margin * 20.0).min(0.1);
                                if travel_total <= margin + CMP_EPSILON {
                                    self.position = self.position - result.travel;
                                    // Cancel for constant speed computation.
                                    result.travel = Vector3::ZERO;
                                } else if travel_total < cancel_dist_max {
                                    // If the movement is large the body can be
                                    // prevented from reaching the walls.
                                    self.position = self.position - slide(result.travel, up);
                                    // Keep remaining motion in sync with amount
                                    // canceled.
                                    motion = slide(motion, up);
                                    result.travel = Vector3::ZERO;
                                } else {
                                    // Travel is too high to be safely canceled, we
                                    // take it into account.
                                    result.travel = slide(result.travel, up);
                                    motion = result.remainder;
                                }
                                // Determines if you are on the ground, and limits
                                // the possibility of climbing on the walls because
                                // of the approximations.
                                self.snap_on_floor(true, false, bodies);
                            } else {
                                // If the movement is not canceled we only keep the
                                // remaining.
                                motion = result.remainder;
                            }

                            // Apply slide on forward in order to allow only
                            // lateral motion on next step.
                            let forward = normalized(slide(self.wall_normal, up));
                            motion = slide(motion, forward);

                            // Scales the horizontal velocity according to the
                            // wall slope.
                            if vel_dir_facing_up {
                                let slide_motion = slide(*velocity, result.collisions[0].normal);
                                // Keeps the vertical motion from velocity and add
                                // the horizontal motion of the projection.
                                *velocity = up * up.dot(*velocity) + slide(slide_motion, up);
                            } else {
                                *velocity = slide(*velocity, forward);
                            }

                            // Allow only lateral motion along previous floor when
                            // already on floor. Fixes slowing down when moving in
                            // diagonal against an inclined wall.
                            if was_on_floor && !vel_dir_facing_up && motion.dot(up) > 0.0 {
                                // Slide along the corner between the wall and
                                // previous floor.
                                let floor_side = prev_floor_normal.cross(self.wall_normal);
                                if floor_side != Vector3::ZERO {
                                    motion = floor_side * motion.dot(floor_side);
                                }
                            }

                            // Stop all motion when a second wall is hit (unless
                            // sliding down or jumping), in order to avoid
                            // jittering in corner cases.
                            let mut stop_all_motion = previous_state.wall && !vel_dir_facing_up;

                            // Allow sliding when the body falls.
                            if !self.on_floor && motion.dot(up) < 0.0 {
                                let slide_motion = slide(motion, self.wall_normal);
                                // Test again to allow sliding only if the result
                                // goes downwards. Fixes jittering issues at the
                                // bottom of inclined walls.
                                if slide_motion.dot(up) < 0.0 {
                                    stop_all_motion = false;
                                    motion = slide_motion;
                                }
                            }

                            if stop_all_motion {
                                motion = Vector3::ZERO;
                                *velocity = Vector3::ZERO;
                            }
                        }
                    }

                    // Stop horizontal motion when under wall slide threshold.
                    if was_on_floor && self.wall_min_slide_angle > 0.0 && result_state.wall {
                        let horizontal_normal = normalized(slide(self.wall_normal, up));
                        let motion_angle = (-horizontal_normal.dot(normalized(motion_slide_up)))
                            .clamp(-1.0, 1.0)
                            .acos()
                            .abs();
                        if motion_angle < self.wall_min_slide_angle {
                            motion = up * motion.dot(up);
                            *velocity = up * velocity.dot(up);

                            apply_default_sliding = false;
                        }
                    }
                }

                if apply_default_sliding {
                    // Regular sliding, the last part of the test handle the case
                    // when you don't want to slide on the ceiling.
                    if (sliding_enabled || !self.on_floor)
                        && (!self.on_ceiling || self.slide_on_ceiling || !vel_dir_facing_up)
                        && !apply_ceiling_velocity
                    {
                        let collision_normal = result.collisions[0].normal;

                        let mut slide_motion = slide(result.remainder, collision_normal);
                        if self.on_floor && !self.on_wall && !is_zero_approx(motion_slide_up) {
                            // Slide using the intersection between the motion
                            // plane and the floor plane, in order to keep the
                            // direction intact.
                            let motion_length = slide_motion.length();
                            slide_motion = up.cross(result.remainder).cross(self.floor_normal);

                            // Keep the length from default slide to change speed
                            // in slopes by default, when constant speed is not
                            // enabled.
                            slide_motion = normalized(slide_motion);
                            slide_motion = slide_motion * motion_length;
                        }

                        if slide_motion.dot(*velocity) > 0.0 {
                            motion = slide_motion;
                        } else {
                            motion = Vector3::ZERO;
                        }

                        if self.slide_on_ceiling && result_state.ceiling {
                            // Apply slide only in the direction of the input
                            // motion, otherwise just stop to avoid jittering when
                            // moving against a wall.
                            if vel_dir_facing_up {
                                *velocity = slide(*velocity, collision_normal);
                            } else {
                                // Avoid acceleration in slope when falling.
                                *velocity = up * up.dot(*velocity);
                            }
                        }
                    }
                    // No sliding on first attempt to keep floor motion stable
                    // when possible.
                    else {
                        motion = result.remainder;
                        if result_state.ceiling && !self.slide_on_ceiling && vel_dir_facing_up {
                            *velocity = slide(*velocity, up);
                            motion = slide(motion, up);
                        }
                    }
                }

                total_travel = total_travel + result.travel;

                // Apply Constant Speed.
                if was_on_floor
                    && self.floor_constant_speed
                    && can_apply_constant_speed
                    && self.on_floor
                    && !is_zero_approx(motion)
                {
                    let travel_slide_up = slide(total_travel, up);
                    motion = normalized(motion)
                        * (motion_slide_up.length() - travel_slide_up.length()).max(0.0);
                }
            }
            // When you move forward in a downward slope you don't collide
            // because you will be in the air. This test ensures that constant
            // speed is applied, only if the player is still on the ground after
            // the snap is applied.
            else if self.floor_constant_speed
                && first_slide
                && self.on_floor_if_snapped(was_on_floor, vel_dir_facing_up, bodies)
            {
                can_apply_constant_speed = false;
                sliding_enabled = true;
                self.position = self.position - result.travel;

                // Slide using the intersection between the motion plane and the
                // floor plane, in order to keep the direction intact.
                let motion_slide_norm = normalized(up.cross(motion).cross(prev_floor_normal));

                motion = motion_slide_norm * motion_slide_up.length();
                collided = true;
            }

            if !collided || is_zero_approx(motion) {
                break;
            }

            can_apply_constant_speed = !can_apply_constant_speed && !sliding_enabled;
            sliding_enabled = true;
            first_slide = false;
        }

        self.snap_on_floor(was_on_floor, vel_dir_facing_up, bodies);

        // Reset the gravity accumulation when touching the ground.
        if self.on_floor && !vel_dir_facing_up {
            *velocity = slide(*velocity, up);
        }
    }

    /// Godot's `_move_and_slide_floating`.
    fn move_and_slide_floating(
        &mut self,
        delta: f32,
        velocity: &mut Vector3,
        bodies: &[&PhysicsBody3D],
    ) {
        let margin = self.safe_margin;
        let mut motion = *velocity * delta;

        self.platform_body = None;
        self.floor_normal = Vector3::ZERO;
        self.platform_velocity = Vector3::ZERO;
        self.platform_angular_velocity = Vector3::ZERO;

        let mut first_slide = true;
        for _iteration in 0..self.max_slides {
            let mut params = MotionParameters::new(self.position, motion, margin);
            // Also report collisions generated only from recovery.
            params.recovery_as_collision = true;

            let (collided, result) = self.godot_move_and_collide(&params, bodies, false, false);

            self.last_motion = result.travel;

            if collided {
                self.push_motion_result(&result);

                self.set_collision_direction(&result, CollisionState::ALL, bodies);

                // Godot also zeroes `motion` here; nothing reads it after the break.
                if is_zero_approx(result.remainder) {
                    break;
                }

                if self.wall_min_slide_angle != 0.0
                    && self
                        .wall_normal
                        .dot(-normalized(*velocity))
                        .clamp(-1.0, 1.0)
                        .acos()
                        < self.wall_min_slide_angle + FLOOR_ANGLE_THRESHOLD
                {
                    motion = Vector3::ZERO;
                    if result.travel.length() < margin + CMP_EPSILON {
                        self.position = self.position - result.travel;
                    }
                } else if first_slide {
                    let motion_slide_norm = normalized(slide(result.remainder, self.wall_normal));
                    motion = motion_slide_norm * (motion.length() - result.travel.length());
                } else {
                    motion = slide(result.remainder, self.wall_normal);
                }

                if motion.dot(*velocity) <= 0.0 {
                    motion = Vector3::ZERO;
                }
            }

            if !collided || is_zero_approx(motion) {
                break;
            }

            first_slide = false;
        }
    }

    /// Godot's `CharacterBody3D::apply_floor_snap()`.
    fn godot_apply_floor_snap(&mut self, bodies: &[&PhysicsBody3D]) {
        if self.on_floor {
            return;
        }

        // Snap by at least collision margin to keep floor state consistent.
        let length = self.floor_snap_length.max(self.safe_margin);

        let mut params =
            MotionParameters::new(self.position, -self.up_direction * length, self.safe_margin);
        params.max_collisions = 4;
        // Also report collisions generated only from recovery.
        params.recovery_as_collision = true;

        let from = self.position;
        let (collided, mut result) = self.godot_move_and_collide(&params, bodies, true, false);
        if collided {
            // Apply direction for floor only.
            let result_state = self.set_collision_direction(
                &result,
                CollisionState {
                    floor: true,
                    wall: false,
                    ceiling: false,
                },
                bodies,
            );

            if result_state.floor {
                // Ensure that we only move the body along the up axis, because
                // move_and_collide may stray the object a bit when getting it
                // unstuck. Canceling this motion should not affect
                // move_and_slide, as previous calls to move_and_collide already
                // took care of freeing the body.
                if result.travel.length() > self.safe_margin {
                    result.travel = self.up_direction * self.up_direction.dot(result.travel);
                } else {
                    result.travel = Vector3::ZERO;
                }

                self.position = from + result.travel;
            }
        }
    }

    /// Godot's `_snap_on_floor`.
    fn snap_on_floor(
        &mut self,
        was_on_floor: bool,
        vel_dir_facing_up: bool,
        bodies: &[&PhysicsBody3D],
    ) {
        if self.on_floor || !was_on_floor || vel_dir_facing_up {
            return;
        }
        self.godot_apply_floor_snap(bodies);
    }

    /// Godot's `_on_floor_if_snapped`.
    fn on_floor_if_snapped(
        &mut self,
        was_on_floor: bool,
        vel_dir_facing_up: bool,
        bodies: &[&PhysicsBody3D],
    ) -> bool {
        if self.up_direction == Vector3::ZERO || self.on_floor || !was_on_floor || vel_dir_facing_up
        {
            return false;
        }

        // Snap by at least collision margin to keep floor state consistent.
        let length = self.floor_snap_length.max(self.safe_margin);

        let mut params =
            MotionParameters::new(self.position, -self.up_direction * length, self.safe_margin);
        params.max_collisions = 4;
        // Also report collisions generated only from recovery.
        params.recovery_as_collision = true;

        let (collided, result) = self.godot_move_and_collide(&params, bodies, true, false);
        if collided {
            // Don't apply direction for any type.
            let result_state =
                self.set_collision_direction(&result, CollisionState::default(), bodies);
            return result_state.floor;
        }
        false
    }

    /// Godot's `_set_collision_direction`: classifies every collision of
    /// `result`, applying to the body's state only the kinds in `apply`.
    /// Returns the kinds found (`r_state`).
    fn set_collision_direction(
        &mut self,
        result: &MotionResult,
        apply: CollisionState,
        bodies: &[&PhysicsBody3D],
    ) -> CollisionState {
        let mut r_state = CollisionState::default();

        let mut wall_depth = -1.0f32;
        let mut floor_depth = -1.0f32;

        let was_on_wall = self.on_wall;
        let prev_wall_normal = self.wall_normal;
        let mut wall_collision_count = 0;
        let mut combined_wall_normal = Vector3::ZERO;
        // Avoid duplicate on average calculation.
        let mut tmp_wall_col = Vector3::ZERO;

        for collision in result.collisions.iter().rev() {
            if self.motion_mode == MotionMode::Grounded {
                // Check if any collision is floor.
                let floor_angle = collision.get_angle(self.up_direction);
                if floor_angle <= self.floor_max_angle + FLOOR_ANGLE_THRESHOLD {
                    r_state.floor = true;
                    if apply.floor && collision.depth > floor_depth {
                        self.on_floor = true;
                        self.floor_normal = collision.normal;
                        floor_depth = collision.depth;
                        self.set_platform_data(collision, bodies);
                    }
                    continue;
                }

                // Check if any collision is ceiling.
                let ceiling_angle = collision.get_angle(-self.up_direction);
                if ceiling_angle <= self.floor_max_angle + FLOOR_ANGLE_THRESHOLD {
                    r_state.ceiling = true;
                    if apply.ceiling {
                        self.platform_ceiling_velocity = collision.collider_velocity;
                        self.ceiling_normal = collision.normal;
                        self.on_ceiling = true;
                    }
                    continue;
                }
            }

            // Collision is wall by default.
            r_state.wall = true;

            if apply.wall && collision.depth > wall_depth {
                self.on_wall = true;
                wall_depth = collision.depth;
                self.wall_normal = collision.normal;

                // Don't apply wall velocity when the collider is a
                // CharacterBody3D (Patina: any kinematic body).
                let is_character = bodies
                    .get(collision.collider_index)
                    .is_some_and(|b| b.body_type == crate::body::BodyType3D::Kinematic);
                if !is_character {
                    self.set_platform_data(collision, bodies);
                }
            }

            // Collect normal for calculating average.
            if !is_equal_approx(collision.normal, tmp_wall_col) {
                tmp_wall_col = collision.normal;
                combined_wall_normal = combined_wall_normal + collision.normal;
                wall_collision_count += 1;
            }
        }

        if r_state.wall
            && wall_collision_count > 1
            && !r_state.floor
            && self.motion_mode == MotionMode::Grounded
        {
            // Check if wall normals cancel out to floor support.
            let combined_wall_normal = normalized(combined_wall_normal);
            let floor_angle = combined_wall_normal
                .dot(self.up_direction)
                .clamp(-1.0, 1.0)
                .acos();
            if floor_angle <= self.floor_max_angle + FLOOR_ANGLE_THRESHOLD {
                r_state.floor = true;
                r_state.wall = false;
                if apply.floor {
                    self.on_floor = true;
                    self.floor_normal = combined_wall_normal;
                }
                if apply.wall {
                    self.on_wall = was_on_wall;
                    self.wall_normal = prev_wall_normal;
                }
                return r_state;
            }
        }
        r_state
    }

    /// Godot's `_set_platform_data`.
    fn set_platform_data(&mut self, collision: &MotionCollision, bodies: &[&PhysicsBody3D]) {
        let Some(body) = bodies.get(collision.collider_index) else {
            return;
        };
        self.platform_body = Some(body.id);
        self.platform_velocity = collision.collider_velocity;
        self.platform_angular_velocity = collision.collider_angular_velocity;
        self.platform_layer = body.collision_layer;
    }

    /// Godot's `PhysicsBody3D::move_and_collide(motion)`: moves by `motion`,
    /// stopping at the first collision.
    ///
    /// Returns the collision (normal pointing away from the obstacle, depth)
    /// or `None` if the full motion completed.
    pub fn move_and_collide(
        &mut self,
        motion: Vector3,
        bodies: &[&PhysicsBody3D],
    ) -> Option<collision::CollisionResult3D> {
        let params = MotionParameters::new(self.position, motion, self.safe_margin);
        let (collided, r) = self.godot_move_and_collide(&params, bodies, false, false);
        if !collided {
            return None;
        }
        Some(collision::CollisionResult3D {
            colliding: true,
            normal: r.collisions[0].normal,
            depth: r.collision_depth,
        })
    }

    /// Godot's floor snap at the end of `move_and_slide` (`_snap_on_floor`):
    /// snaps only a body that was on the floor and is not moving up.
    pub fn apply_floor_snap(
        &mut self,
        was_on_floor: bool,
        velocity: Vector3,
        bodies: &[&PhysicsBody3D],
    ) {
        let vel_dir_facing_up = velocity.dot(self.up_direction) > 0.0;
        self.snap_on_floor(was_on_floor, vel_dir_facing_up, bodies);
    }

    /// Seeds the contact state the PREVIOUS `move_and_slide` left (pat-d4ghb).
    ///
    /// Godot's `CharacterBody3D` is one long-lived object, so `was_on_floor`,
    /// the floor normal and the platform velocity carry from call to call.
    /// Patina's scene bridge builds a fresh solver body per call from the
    /// node's published properties, and without this every call started
    /// airborne.
    pub fn restore_contact_state(
        &mut self,
        on_floor: bool,
        on_wall: bool,
        on_ceiling: bool,
        floor_normal: Vector3,
        wall_normal: Vector3,
        platform_velocity: Vector3,
    ) {
        self.on_floor = on_floor;
        self.on_wall = on_wall;
        self.on_ceiling = on_ceiling;
        self.floor_normal = floor_normal;
        self.wall_normal = wall_normal;
        self.platform_velocity = platform_velocity;
    }

    /// Seeds Godot's `platform_rid` / `platform_layer` from the previous call:
    /// the body the character stood on (or rode on a wall), whose velocity the
    /// next `move_and_slide` re-reads before moving (pat-kiuul).
    pub fn restore_platform(&mut self, body: Option<BodyId3D>, layer: u32) {
        self.platform_body = body;
        self.platform_layer = layer;
    }

    /// The body the last `move_and_slide` stood on or rode (Godot's
    /// `platform_rid`), and its collision layer.
    pub fn get_platform(&self) -> (Option<BodyId3D>, u32) {
        (self.platform_body, self.platform_layer)
    }

    /// Godot's **manual** `apply_floor_snap()` (pat-s3rrv.103): snaps to the
    /// floor regardless of the body's velocity; does nothing when already on
    /// the floor.
    pub fn apply_floor_snap_manual(&mut self, bodies: &[&PhysicsBody3D]) {
        self.godot_apply_floor_snap(bodies);
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
    pub fn get_last_motion(&self) -> Vector3 {
        self.last_motion
    }

    /// Returns the angle (radians) between the floor normal and `up_direction`.
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

    // Measured on Godot 4.6.1 + Jolt (pat-kiuul) by the probe project
    // `tools/oracle/character_body_3d_jolt/delta_one`, which runs with
    // `physics_ticks_per_second = 1`, so `move_and_slide()` moves by exactly
    // `velocity` -- the same units as this solver's `move_and_slide`.
    /// pat-d4ghb: Godot snaps only a body that WAS on the floor. A falling
    /// body 0.05 above a floor (inside the 0.1 snap length) stays airborne and
    /// moves by its own motion only.
    ///
    /// pat-kiuul: the snap is Jolt's shape cast, so it lands where the cast's
    /// binary search does -- NOT `safe_margin` short of the floor, which is
    /// what the old solver assumed (it put both bodies at 1.001). A 0.05 gap
    /// is touched exactly at the 0.5 the search tests first and lands on the
    /// floor (Godot 1.000000119); a 0.04 gap stops at the search's 0.390625
    /// (Godot 1.000937462).
    #[test]
    fn a_falling_body_is_not_snapped_onto_the_floor_below() {
        // Floor top at y = 0; sphere bottom at 0.05 + 0.02 after the motion.
        let floor = make_static_floor(-1.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&floor];
        let mut c = CharacterBody3D::new(
            Vector3::new(0.0, 1.07, 0.0),
            Shape3D::Sphere { radius: 1.0 },
        );
        c.move_and_slide(Vector3::new(0.0, -0.02, 0.0), &bodies);
        assert!(!c.is_on_floor(), "an airborne body is not snapped");
        assert!(
            (c.position.y - 1.050000072).abs() < 1e-6,
            "moved by its motion only: {}",
            c.position.y
        );

        // The same body grounded by the previous call IS snapped, along up only.
        let snapped = |y: f32| {
            let mut g =
                CharacterBody3D::new(Vector3::new(0.0, y, 0.0), Shape3D::Sphere { radius: 1.0 });
            g.restore_contact_state(
                true,
                false,
                false,
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::ZERO,
                Vector3::ZERO,
            );
            g.move_and_slide(Vector3::new(0.0, -0.02, 0.0), &bodies);
            assert!(g.is_on_floor(), "a grounded body is snapped");
            assert!(g.position.x.abs() < 1e-6 && g.position.z.abs() < 1e-6);
            g.position.y
        };
        let y = snapped(1.07);
        assert!(
            (y - 1.000000119).abs() < 1e-6,
            "0.05 gap: Godot 1.000000119, Patina {y}"
        );
        let y = snapped(1.06);
        assert!(
            (y - 1.000937462).abs() < 1e-6,
            "0.04 gap: Godot 1.000937462, Patina {y}"
        );
    }

    /// pat-d4ghb, measured on Godot 4.6.1 + Jolt: a GROUNDED body walking
    /// diagonally into a wall keeps only the velocity along the wall, and the
    /// floor reset zeroes its y: (6, -0.2, 3) reads (0, 0, 3).
    #[test]
    fn a_grounded_body_walking_into_a_wall_slides_its_velocity() {
        let floor = make_static_floor(-1.0);
        let wall = make_static_wall(5.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&floor, &wall];
        // Sphere radius 1 resting on y = 0 (centre 1.001), 0.01 from the wall
        // face at x = 4.
        let mut c = CharacterBody3D::new(
            Vector3::new(2.99, 1.001, 0.0),
            Shape3D::Sphere { radius: 1.0 },
        );
        c.restore_contact_state(
            true,
            false,
            false,
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::ZERO,
            Vector3::ZERO,
        );
        let dt = 1.0 / 60.0;
        let out = c.move_and_slide(Vector3::new(6.0, -0.2, 3.0) * dt, &bodies) / dt;
        assert!(c.is_on_wall(), "touching the wall");
        assert!(
            out.x.abs() < 1e-3 && out.y.abs() < 1e-3,
            "slid velocity {out:?}"
        );
        assert!(
            (out.z - 3.0).abs() < 1e-3,
            "kept the along-wall speed: {out:?}"
        );
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
        let mut character = CharacterBody3D::new(
            Vector3::new(0.0, -2.0, 0.0),
            Shape3D::Sphere { radius: 1.0 },
        );
        // Ceiling box centered at y=1, half_extents (100, 1, 100) => bottom edge at y=0
        // Character at y=-2, moves up by 2.5 => center at y=0.5 (inside ceiling box)
        let ceiling = make_static_ceiling(1.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&ceiling];

        let _result = character.move_and_slide(Vector3::new(0.0, 2.5, 0.0), &bodies);
        assert!(character.is_on_ceiling(), "Should detect ceiling");
        assert!(!character.is_on_floor());
    }

    // Measured on Godot 4.6.1 + Jolt (pat-kiuul) by the probe project
    // `tools/oracle/character_body_3d_jolt/delta_one`, which runs with
    // `physics_ticks_per_second = 1`, so `move_and_slide()` moves by exactly
    // `velocity` -- the same units as this solver's `move_and_slide`.
    /// `slide_on_ceiling = false` against a FLAT ceiling (pat-s3rrv.103,
    /// pat-kiuul). Godot's `_move_and_slide_grounded` then takes the
    /// "no sliding" branch: `motion = remainder.slide(up)`, `velocity =
    /// velocity.slide(up)`. That keeps the sideways part, so the body ends
    /// exactly where the sliding one does -- Godot: both at (3, -1.000902) with
    /// velocity (3, 0, 0). The old solver stopped the body dead at the contact,
    /// which no Godot branch does.
    #[test]
    fn slide_on_ceiling_false_cancels_an_upward_jump() {
        // Ceiling bottom edge sits at y = 0; the character starts below it and
        // is driven UP and sideways, so it arrives while moving upward.
        let ceiling = make_static_ceiling(1.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&ceiling];
        for slide_on_ceiling in [true, false] {
            let mut c = CharacterBody3D::new(
                Vector3::new(0.0, -2.0, 0.0),
                Shape3D::Sphere { radius: 1.0 },
            );
            c.slide_on_ceiling = slide_on_ceiling;
            let v = c.move_and_slide(Vector3::new(3.0, 2.5, 0.0), &bodies);
            assert!(
                c.is_on_ceiling(),
                "slide_on_ceiling {slide_on_ceiling}: reached the ceiling"
            );
            assert!(
                (c.position.x - 3.0).abs() < 1e-5 && (c.position.y + 1.000902).abs() < 2e-6,
                "slide_on_ceiling {slide_on_ceiling}: Godot (3, -1.000902), Patina {:?}",
                c.position
            );
            assert!(
                (v - Vector3::new(3.0, 0.0, 0.0)).length() < 1e-5,
                "slide_on_ceiling {slide_on_ceiling}: Godot velocity (3, 0, 0), Patina {v:?}"
            );
        }
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
            // RESTING on the slope, i.e. grounded by the previous call: Godot
            // snaps only a body that was on the floor (pat-d4ghb).
            c.restore_contact_state(
                true,
                false,
                false,
                Vector3::new(-0.5, 0.866_025_4, 0.0),
                Vector3::ZERO,
                Vector3::ZERO,
            );
            // Move horizontally (descending side) with a touch of gravity.
            c.move_and_slide(Vector3::new(0.1, -0.01, 0.0), &bodies);
            (c.is_on_floor(), c.position.y)
        };

        let (snapped_floor, snapped_y) = run(0.2);
        let (nosnap_floor, nosnap_y) = run(0.0);

        assert!(
            snapped_floor,
            "with snapping the character stays on the floor"
        );
        assert!(
            !nosnap_floor,
            "without snapping the character leaves the floor"
        );
        assert!(
            snapped_y < nosnap_y - 1e-3,
            "snapped y tracks the descending slope (lower) than the floating y: {snapped_y} vs {nosnap_y}"
        );
    }

    // Measured on Godot 4.6.1 + Jolt (pat-kiuul) by the probe project
    // `tools/oracle/character_body_3d_jolt/delta_one`, which runs with
    // `physics_ticks_per_second = 1`, so `move_and_slide()` moves by exactly
    // `velocity` -- the same units as this solver's `move_and_slide`.
    /// A floor body with a nonzero velocity carries the resting character --
    /// from the SECOND call: Godot re-reads the velocity of the `platform_rid`
    /// the PREVIOUS call stood on. Godot: first call x = 0 (on floor, platform
    /// velocity 2), second call x = 2.
    #[test]
    fn character_carried_by_moving_platform() {
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
        let mut c =
            CharacterBody3D::new(Vector3::new(0.0, 1.0, 0.0), Shape3D::Sphere { radius: 1.0 });
        // No own velocity -- the platform alone should move the character.
        c.move_and_slide(Vector3::ZERO, &bodies);
        assert!(c.is_on_floor());
        assert!(
            c.position.x.abs() < 1e-6,
            "not carried on the first call: x = {}",
            c.position.x
        );
        assert!(
            (c.get_platform_velocity().x - 2.0).abs() < 1e-6,
            "platform velocity is exposed, got {:?}",
            c.get_platform_velocity()
        );
        c.move_and_slide(Vector3::ZERO, &bodies);
        assert!(
            (c.position.x - 2.0).abs() < 1e-5 && (c.position.y - 1.0).abs() < 1e-5,
            "carried by the platform: Godot (2, 1), Patina {:?}",
            c.position
        );
    }

    #[test]
    fn character_platform_on_leave_add_velocity() {
        let bodies: Vec<&PhysicsBody3D> = vec![];

        // AddVelocity: the outgoing velocity retains the full platform velocity.
        let mut c =
            CharacterBody3D::new(Vector3::new(0.0, 1.0, 0.0), Shape3D::Sphere { radius: 1.0 });
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

    // Measured on Godot 4.6.1 + Jolt (pat-kiuul) by the probe project
    // `tools/oracle/character_body_3d_jolt/delta_one`, which runs with
    // `physics_ticks_per_second = 1`, so `move_and_slide()` moves by exactly
    // `velocity` -- the same units as this solver's `move_and_slide`.
    /// `PLATFORM_ON_LEAVE_ADD_UPWARD_VELOCITY` ignores only a DOWNWARD platform
    /// component (`current_platform_velocity.slide(up)` when it points down);
    /// the sideways part is kept. Godot: jumping 50 off a (2, 3, 0) platform
    /// leaves with (2, 53, 0). (The old solver dropped the sideways 2.)
    #[test]
    fn character_platform_on_leave_add_upward_velocity() {
        let bodies: Vec<&PhysicsBody3D> = vec![];
        let leave = |platform: Vector3| {
            let mut c =
                CharacterBody3D::new(Vector3::new(0.0, 1.0, 0.0), Shape3D::Sphere { radius: 1.0 });
            c.platform_on_leave = PlatformOnLeave::AddUpwardVelocity;
            c.on_floor = true;
            c.platform_velocity = platform;
            c.move_and_slide(Vector3::new(0.0, 5.0, 0.0), &bodies)
        };
        let out = leave(Vector3::new(2.0, 3.0, 0.0));
        assert!(
            (out - Vector3::new(2.0, 8.0, 0.0)).length() < 1e-5,
            "up-and-sideways: got {out:?}"
        );
        let out = leave(Vector3::new(2.0, -3.0, 0.0));
        assert!(
            (out - Vector3::new(2.0, 5.0, 0.0)).length() < 1e-5,
            "downward part ignored: got {out:?}"
        );
    }

    // Measured on Godot 4.6.1 + Jolt (pat-kiuul) by the probe project
    // `tools/oracle/character_body_3d_jolt/delta_one`, which runs with
    // `physics_ticks_per_second = 1`, so `move_and_slide()` moves by exactly
    // `velocity` -- the same units as this solver's `move_and_slide`.
    /// The same leave off a real moving body (Godot `StaticBody3D` with
    /// `constant_linear_velocity = (2, 3, 0)`): land, then jump at 50. Godot
    /// (2, 53, 0).
    #[test]
    fn leaving_a_rising_platform_keeps_its_velocity() {
        let mut platform = PhysicsBody3D::new(
            BodyId3D(21),
            BodyType3D::Kinematic,
            Vector3::new(0.0, -1.0, 0.0),
            Shape3D::BoxShape {
                half_extents: Vector3::new(10.0, 1.0, 10.0),
            },
            1.0,
        );
        platform.linear_velocity = Vector3::new(2.0, 3.0, 0.0);
        let bodies: Vec<&PhysicsBody3D> = vec![&platform];
        let mut c =
            CharacterBody3D::new(Vector3::new(0.0, 1.0, 0.0), Shape3D::Sphere { radius: 1.0 });
        c.platform_on_leave = PlatformOnLeave::AddUpwardVelocity;
        c.move_and_slide(Vector3::ZERO, &bodies);
        let out = c.move_and_slide(Vector3::new(0.0, 50.0, 0.0), &bodies);
        assert!(
            (out - Vector3::new(2.0, 53.0, 0.0)).length() < 1e-4,
            "Godot (2, 53, 0), Patina {out:?}"
        );
    }

    // Measured on Godot 4.6.1 + Jolt (pat-kiuul) by the probe project
    // `tools/oracle/character_body_3d_jolt/delta_one`, which runs with
    // `physics_ticks_per_second = 1`, so `move_and_slide()` moves by exactly
    // `velocity` -- the same units as this solver's `move_and_slide`.
    /// The 3D platformer's capsule (r 0.4, h 2.1, raised 1.05) walking on a
    /// 2 m box floor whose top is y = -4, with the game's per-tick motion at
    /// 120 Hz -- (6.0666667, -22/120, 0) / 120 -- for 40 calls. Godot rests it
    /// at y = -3.999896526, 1e-4 above the floor: Jolt's 5-step cast lands it
    /// below the safe margin and the next slide's recovery climbs back only 87 %
    /// of the way. The old push-out solver rested it at exactly floor + margin
    /// (-3.999). Position x: Godot 2.022220850.
    #[test]
    fn walking_capsule_rests_where_jolt_puts_it() {
        let mut floor = make_static_floor(-5.0);
        floor.shape = Shape3D::BoxShape {
            half_extents: Vector3::new(1000.0, 1.0, 1000.0),
        };
        let bodies: Vec<&PhysicsBody3D> = vec![&floor];
        let mut c = CharacterBody3D::new(
            Vector3::new(0.0, -3.998966, 0.0),
            Shape3D::CapsuleShape {
                radius: 0.4,
                height: 2.1,
            },
        );
        c.shape_offset = Vector3::new(0.0, 1.05, 0.0);
        let mut v = Vector3::ZERO;
        for _ in 0..40 {
            v = c.move_and_slide(
                Vector3::new(6.0666667 / 120.0, -22.0 / 120.0 / 120.0, 0.0),
                &bodies,
            );
        }
        assert!(c.is_on_floor());
        assert!(
            (c.position.y + 3.999896526).abs() < 2e-6,
            "Godot rests at -3.999896526, Patina {}",
            c.position.y
        );
        assert!(
            (c.position.x - 2.022220850).abs() < 1e-5,
            "Godot x 2.022220850, Patina {}",
            c.position.x
        );
        assert!(
            (v - Vector3::new(0.050555557, 0.0, 0.0)).length() < 1e-6,
            "Godot velocity, Patina {v:?}"
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
        assert!(
            !c0.is_on_floor(),
            "no slide iterations means no floor contact"
        );

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
        // The slid motion is the 0.1 m drop projected onto the 30-degree
        // plane — 0.05 m along it, 0.1 * sin30 * cos30 = 0.0433 m in X. This
        // threshold was 0.05, which only the old double-applied slide cleared
        // (pat-2ifdx).
        assert!(
            (slide_x - 0.1 * 0.5 * 0.866_025_4).abs() < 0.005,
            "without floor_stop_on_slope the character slides downhill by the \
             motion's projection onto the slope, got x = {slide_x}"
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

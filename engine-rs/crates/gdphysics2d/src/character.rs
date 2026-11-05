//! CharacterBody2D — kinematic character controller.
//!
//! Provides [`CharacterBody2D`] with `move_and_slide` for platformer-style
//! movement that slides along surfaces and detects floor/wall/ceiling contacts.

use gdcore::math::{Transform2D, Vector2};

use crate::body::PhysicsBody2D;
// ⚠️ `BodyId` is imported UNCONDITIONALLY: `Collision2D::collider` (below) is a
// non-test public field, so a `#[cfg(test)]` on this import compiles under
// `--lib` (which builds with `cfg(test)`) and fails every normal build — taking
// every crate below gdphysics2d with it. `BodyType` stays test-only because
// nothing outside the tests module names it.
use crate::body::BodyId;
#[cfg(test)]
use crate::body::BodyType;
use crate::collision;
use crate::godot_motion::{
    test_body_motion, MotionParameters, MotionResult, MotionSpace, MovingShapes,
};
use crate::godot_sat::{self, SatShape};
use crate::shape::Shape2D;

/// Godot's `FLOOR_ANGLE_THRESHOLD` (`character_body_2d.cpp`).
const FLOOR_ANGLE_THRESHOLD: f32 = 0.01;

/// A kinematic character controller that slides along surfaces.
///
/// `velocity` is expressed in **pixels per second**, matching Godot's
/// `CharacterBody2D.velocity`. Per-frame displacement is `velocity * delta`,
/// computed inside [`CharacterBody2D::move_and_slide_delta`]; the returned
/// velocity remains in pixels-per-second so it can be carried across frames
/// (this is the contract the physics server relies on).
#[derive(Debug, Clone)]
pub struct CharacterBody2D {
    /// World-space position.
    pub position: Vector2,
    /// Global rotation (radians) of the body.
    pub rotation: f32,
    /// Collision shape in local space.
    pub shape: Shape2D,
    /// The collider's LOCAL transform inside the body — the
    /// `CollisionShape2D`'s `position`/`rotation` (`PhysicsBody2D::shape_transform`).
    /// Every overlap test places the shape at `position` composed with this.
    /// It used to be dropped, so a character whose shape sits off its origin
    /// (the official 2D Platformer demo's player: `(0, -14)`) collided as if
    /// the shape were centred and stood 14 px above the floor.
    pub shape_offset: Transform2D,
    /// The up direction for floor/ceiling detection (default: negative Y).
    pub up_direction: Vector2,
    /// Maximum angle (in radians) from up_direction that still counts as floor.
    pub floor_max_angle: f32,
    /// Collision layer bitmask.
    pub collision_layer: u32,
    /// Collision mask bitmask.
    pub collision_mask: u32,

    /// Character velocity in pixels-per-second (Godot `velocity`).
    pub velocity: Vector2,

    /// Distance (px) below the feet the body snaps to the floor after moving.
    /// `0` disables floor snapping. Matches Godot `floor_snap_length`.
    pub floor_snap_length: f32,
    /// If true, the body does not slide down a slope when it has no lateral
    /// input velocity. Matches Godot `floor_stop_on_slope`.
    pub floor_stop_on_slope: bool,
    /// If true, floor slides preserve speed magnitude instead of losing speed
    /// to the slope. Matches Godot `floor_constant_speed`.
    pub floor_constant_speed: bool,
    /// If true, a wall touched while on the floor cannot push the body upward
    /// (prevents climbing walls). Matches Godot `floor_block_on_wall`.
    pub floor_block_on_wall: bool,
    /// If true, the body slides along a ceiling on contact; otherwise motion is
    /// cancelled on ceiling contact. Matches Godot `slide_on_ceiling`.
    pub slide_on_ceiling: bool,
    /// Maximum number of slide iterations per call. Matches Godot `max_slides`.
    pub max_slides: usize,

    /// Velocity (px/s) of the platform the body currently stands on. Updated at
    /// the end of every `move_and_slide` call and used to carry the body next
    /// frame (Godot `get_platform_velocity`).
    pub platform_velocity: Vector2,

    /// Grounded vs floating surface interpretation (pat-0q2kx.113).
    pub motion_mode: MotionMode,
    /// What to inherit when stepping off a moving platform (pat-0q2kx.113).
    pub platform_on_leave: PlatformOnLeave,

    /// `safe_margin` (Godot default 0.08 px).
    pub safe_margin: f32,
    /// `platform_floor_layers` (default: every layer).
    pub platform_floor_layers: u32,
    /// `platform_wall_layers` (default: none).
    pub platform_wall_layers: u32,
    /// `wall_min_slide_angle` (floating mode; default 15 degrees).
    pub wall_min_slide_angle: f32,
    /// The body last stood on / pushed against (`platform_rid`).
    pub platform_body: Option<BodyId>,
    /// That body's collision layer (`platform_layer`).
    pub platform_layer: u32,
    /// The space's last physics step, for moving one-way platforms. `0` = `dt`.
    pub last_step: f32,
    last_motion: Vector2,

    on_floor: bool,
    on_wall: bool,
    on_ceiling: bool,
    floor_normal: Vector2,
    wall_normal: Vector2,

    slide_collisions: Vec<KinematicCollision2D>,
}

/// How `move_and_slide` interprets surfaces (Godot `CharacterBody2D.motion_mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MotionMode {
    /// Godot `MOTION_MODE_GROUNDED` (default): surfaces are classified into
    /// floor / wall / ceiling against `up_direction`, enabling snapping,
    /// slope handling and `is_on_floor`.
    #[default]
    Grounded,
    /// Godot `MOTION_MODE_FLOATING`: no up direction is privileged, so the body
    /// slides along every surface alike and never reports floor/wall/ceiling.
    /// This is the top-down / space-game mode.
    Floating,
}

/// What happens to inherited platform velocity when the body leaves a moving
/// platform (Godot `CharacterBody2D.platform_on_leave`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlatformOnLeave {
    /// Godot `PLATFORM_ON_LEAVE_ADD_VELOCITY` (default): the platform's velocity
    /// is added to the body when it steps off, so walking off a moving train
    /// carries its momentum.
    #[default]
    AddVelocity,
    /// `PLATFORM_ON_LEAVE_ADD_UPWARD_VELOCITY`: only the component along
    /// `up_direction` is inherited.
    AddUpwardVelocity,
    /// `PLATFORM_ON_LEAVE_DO_NOTHING`: no velocity is inherited at all.
    DoNothing,
}

/// Default maximum number of slide iterations per `move_and_slide` call.
const DEFAULT_MAX_SLIDES: usize = 4;

/// Floor angle threshold (approximately 45 degrees).
const DEFAULT_FLOOR_MAX_ANGLE: f32 = std::f32::consts::FRAC_PI_4;

/// Deepest collision between `shape` at `tf` and **any** collider `other` owns
/// (pat-s3rrv.25).
///
/// Character sweeps used to test against `other.shape` alone. Once
/// body-vs-body learned to iterate every shape, that left the player as the one
/// thing in the scene that could walk through a platform's second collider
/// while a RigidBody2D bounced off it — the worst place for the engine to be
/// inconsistent.
///
/// Returns the DEEPEST hit rather than the first: the sweeps below pick the
/// deepest contact to resolve against, so surfacing a shallower one from an
/// earlier shape would depenetrate against the wrong surface.
///
/// With one collider — every scene until multi-shape bodies are authored — this
/// is exactly `test_collision` against that shape.
///
/// ⚠️ Inherited invariant from pat-0q2kx.113, kept here because the helper that
/// used to carry it (`body_transform`) is gone: the obstacle transform **must
/// include the body's rotation**. It once read
/// `Transform2D::translated(body.position)`, which silently dropped it, so every
/// obstacle collided as if axis-aligned and a 30° ramp came back with an
/// axis-aligned normal — classified as a wall instead of a floor. `shape_owners`
/// composes `from_rotation_origin(rotation, position)` with each collider's own
/// transform, so rotation is included; do not "simplify" it back to a
/// translation.
fn deepest_hit_against(
    shape: &Shape2D,
    tf: &Transform2D,
    other: &PhysicsBody2D,
    motion: Vector2,
) -> Option<collision::CollisionResult> {
    other
        .shape_owners()
        .into_iter()
        // One-way collision is a property of the SHAPE, not the body
        // (pat-s3rrv.25). Filtering here rather than at the call sites is what
        // lets a platform body carry a solid collider and a one-way collider at
        // once: the solid one still blocks in the same sweep that the one-way
        // one is passed through. `ShapeOwner2D::passthrough_for` carries the
        // pat-0q2kx.113 rule unchanged, including "zero motion never passes".
        .filter(|owner| !owner.passthrough_for(motion))
        .filter_map(|owner| collision::test_collision(shape, tf, &owner.shape, &owner.transform))
        .filter(|r| r.colliding && r.depth > 0.0)
        .max_by(|a, b| a.depth.total_cmp(&b.depth))
}

/// Splits `motion` into steps small enough that a moving body cannot pass
/// through a thin obstacle between two discrete overlap tests (pat-0q2kx.113).
///
/// The slide loop tests for overlap **at the target position**. That is exact
/// for slow motion and wrong for fast motion: at 600 px/s over a 1/60 s frame
/// the body advances 10 px per step, so a 4 px-thick platform can sit entirely
/// between the start and target positions and register no overlap at all — the
/// body tunnels straight through.
///
/// Substepping by a fraction of the *smallest* shape extent is the cheap,
/// correct-enough sweep: no step can exceed the thinnest thing it could hit.
/// The count is clamped so a pathological velocity cannot make the loop
/// unbounded.
fn sweep_steps(motion: Vector2, shape: &Shape2D) -> usize {
    const MAX_SUBSTEPS: usize = 16;
    let extent = shape_min_extent(shape);
    let distance = motion.length();
    if extent <= 0.0 || distance <= extent {
        return 1;
    }
    ((distance / extent).ceil() as usize).clamp(1, MAX_SUBSTEPS)
}

/// Godot's `PhysicsServer2D.body_test_motion` / `PhysicsBody2D.test_move`
/// (pat-0q2kx.200): would moving `shape` from `from` along `motion` hit any of
/// `bodies`, and how far does it get?
///
/// Returns `(collided, travel)`. `travel` is the motion actually achieved, so a
/// blocked sweep reports a vector strictly shorter than `motion` — which is the
/// property `test_move` and the server API are asked for.
///
/// Substeps with the same [`sweep_steps`] budget `move_and_slide` uses, so a
/// fast test cannot tunnel through a thin wall and report "clear". The narrow
/// phase does the shape work, so every shape pair the engine supports is
/// covered rather than just circles.
///
/// ⚠️ Bodies the sweep starts already overlapping are skipped: `test_move`
/// answers "can I move", and a body resting in contact with the floor must not
/// report every motion as blocked.
pub fn test_motion(
    from: Vector2,
    rotation: f32,
    shape: &Shape2D,
    motion: Vector2,
    bodies: &[&PhysicsBody2D],
) -> (bool, Vector2) {
    let steps = sweep_steps(motion, shape);
    let step = motion * (1.0 / steps as f32);

    let overlaps_at = |pos: Vector2, other: &PhysicsBody2D| -> bool {
        let tf = Transform2D::from_rotation_origin(rotation, pos);
        // `Vector2::ZERO` = no one-way filtering. `test_motion` never applied
        // it, and `passthrough_for` treats zero motion as "never passes", so
        // this path is byte-identical to before one-way moved onto the shape
        // owner (pat-s3rrv.25). Godot's `test_move` does honour one-way; wiring
        // it here is a behaviour change this bead does not own.
        deepest_hit_against(shape, &tf, other, Vector2::ZERO).is_some()
    };

    // Anything already touching at the start is not an obstacle to this motion.
    let ignored: Vec<bool> = bodies.iter().map(|b| overlaps_at(from, b)).collect();

    let mut reached = from;
    for i in 1..=steps {
        let candidate = from + step * i as f32;
        let blocked = bodies
            .iter()
            .enumerate()
            .any(|(bi, b)| !ignored[bi] && overlaps_at(candidate, b));
        if blocked {
            return (true, reached - from);
        }
        reached = candidate;
    }
    (false, motion)
}

/// The smallest half-extent of a shape — the dimension a sweep must not skip.
fn shape_min_extent(shape: &Shape2D) -> f32 {
    match shape {
        Shape2D::Circle { radius } => *radius,
        Shape2D::Rectangle { half_extents } => half_extents.x.min(half_extents.y),
        Shape2D::Capsule { radius, .. } => *radius,
        // Shapes with no meaningful "thinnest dimension" return 0.0, which
        // `sweep_steps` reads as "do not substep" — the conservative choice:
        // they keep the previous single-target behaviour rather than getting a
        // sweep derived from a made-up extent.
        //
        // ⚠️ Deliberately exhaustive rather than a `_ =>` catch-all: a new
        // Shape2D variant should fail to compile here so someone decides
        // whether it can be tunnelled through, instead of silently inheriting
        // "never sweep".
        Shape2D::Segment { .. }
        | Shape2D::ConvexPolygon { .. }
        | Shape2D::ConcavePolygon { .. }
        | Shape2D::WorldBoundary { .. }
        | Shape2D::SeparationRay { .. } => 0.0,
    }
    .max(0.0)
}

impl CharacterBody2D {
    /// The body's global transform (`get_global_transform`).
    fn global_transform(&self) -> Transform2D {
        Transform2D::from_rotation_origin(self.rotation, self.position)
    }

    fn up(&self) -> Vector2 {
        if self.up_direction.length_squared() > 1e-8 {
            godot_sat::normalized(self.up_direction)
        } else {
            Vector2::new(0.0, -1.0)
        }
    }

    pub fn new(position: Vector2, shape: Shape2D) -> Self {
        Self {
            position,
            rotation: 0.0,
            shape,
            shape_offset: Transform2D::IDENTITY,
            up_direction: Vector2::new(0.0, -1.0),
            floor_max_angle: DEFAULT_FLOOR_MAX_ANGLE,
            collision_layer: 1,
            collision_mask: 1,
            velocity: Vector2::ZERO,
            floor_snap_length: 0.0,
            floor_stop_on_slope: false,
            floor_constant_speed: false,
            floor_block_on_wall: true,
            slide_on_ceiling: true,
            max_slides: DEFAULT_MAX_SLIDES,
            platform_velocity: Vector2::ZERO,
            motion_mode: MotionMode::default(),
            platform_on_leave: PlatformOnLeave::default(),
            safe_margin: 0.08,
            platform_floor_layers: u32::MAX,
            platform_wall_layers: 0,
            wall_min_slide_angle: 15.0_f32.to_radians(),
            platform_body: None,
            platform_layer: 0,
            last_step: 0.0,
            last_motion: Vector2::ZERO,
            on_floor: false,
            on_wall: false,
            on_ceiling: false,
            floor_normal: Vector2::ZERO,
            wall_normal: Vector2::ZERO,
            slide_collisions: Vec::new(),
        }
    }

    /// Convenience wrapper that treats `velocity` as a one-shot per-tick motion
    /// vector (`delta = 1.0`).
    pub fn move_and_slide(&mut self, velocity: Vector2, bodies: &[&PhysicsBody2D]) -> Vector2 {
        self.move_and_slide_delta(velocity, 1.0, bodies)
    }

    fn own_shapes(&self) -> Vec<(SatShape, Transform2D)> {
        SatShape::from_shape(&self.shape)
            .map(|s| vec![(s, self.shape_offset)])
            .unwrap_or_default()
    }

    /// `PhysicsBody2D::move_and_collide(parameters, result, test_only,
    /// cancel_sliding)` (`physics_body_2d.cpp`).
    fn godot_move_and_collide(
        &mut self,
        space: &MotionSpace,
        shapes: &[(SatShape, Transform2D)],
        p: &MotionParameters,
        test_only: bool,
        mut cancel_sliding: bool,
    ) -> (bool, MotionResult) {
        let (colliding, mut r) = test_body_motion(space, &MovingShapes { shapes }, p);
        if cancel_sliding {
            let motion_length = p.motion.length();
            let mut precision = 0.001f32;
            if colliding {
                precision +=
                    motion_length * (r.collision_unsafe_fraction - r.collision_safe_fraction);
                if r.collision_depth > p.margin + precision {
                    cancel_sliding = false;
                }
            }
            if cancel_sliding {
                let motion_normal = if motion_length > godot_sat::CMP_EPSILON {
                    p.motion / motion_length
                } else {
                    Vector2::ZERO
                };
                let projected_length = r.travel.dot(motion_normal);
                let recovery = r.travel - motion_normal * projected_length;
                if recovery.length() < p.margin + precision {
                    r.travel = motion_normal * projected_length;
                    r.remainder = p.motion - r.travel;
                }
            }
        }
        if !test_only {
            self.position = p.from.origin + r.travel;
        }
        (colliding, r)
    }

    fn params(&self, motion: Vector2) -> MotionParameters {
        let mut p = MotionParameters::new(self.global_transform(), motion, self.safe_margin);
        p.recovery_as_collision = true;
        p
    }

    /// Moves the character according to `velocity` (pixels-per-second) over
    /// `dt` — Godot 4.6's `CharacterBody2D::move_and_slide`
    /// (`scene/2d/physics/character_body_2d.cpp`), ported line for line on top
    /// of [`crate::godot_motion::test_body_motion`]. Returns the new velocity.
    pub fn move_and_slide_delta(
        &mut self,
        velocity: Vector2,
        dt: f32,
        bodies: &[&PhysicsBody2D],
    ) -> Vector2 {
        self.velocity = velocity;
        self.slide_collisions.clear();
        let last_step = if self.last_step > 0.0 {
            self.last_step
        } else {
            dt
        };
        let space = MotionSpace::new(bodies, self.collision_mask, last_step);
        let shapes = self.own_shapes();
        let up = self.up();

        let mut current_platform_velocity = self.platform_velocity;
        if (self.on_floor || self.on_wall) && self.platform_body.is_some() {
            let excluded = if self.on_floor {
                (self.platform_floor_layers & self.platform_layer) == 0
            } else {
                (self.platform_wall_layers & self.platform_layer) == 0
            };
            if !excluded {
                match self.platform_body.and_then(|id| space.body(id)) {
                    Some(b) => {
                        let local = self.position - b.position;
                        let rel = local - b.center_of_mass;
                        let av = b.angular_velocity;
                        current_platform_velocity =
                            b.linear_velocity + Vector2::new(-av * rel.y, av * rel.x);
                    }
                    None => {
                        current_platform_velocity = Vector2::ZERO;
                        self.platform_body = None;
                    }
                }
            } else {
                current_platform_velocity = Vector2::ZERO;
            }
        }

        self.last_motion = Vector2::ZERO;
        let was_on_floor = self.on_floor;
        self.on_floor = false;
        self.on_ceiling = false;
        self.on_wall = false;

        if !godot_sat::vec_is_zero_approx(current_platform_velocity) {
            let mut p = self.params(current_platform_velocity * dt);
            if let Some(id) = self.platform_body {
                p.exclude_bodies.push(id);
            }
            let (hit, r) = self.godot_move_and_collide(&space, &shapes, &p, false, false);
            if hit {
                self.push_result(&r);
                self.set_collision_direction(&r, &space, up);
            }
        }

        match self.motion_mode {
            MotionMode::Grounded => self.slide_grounded(&space, &shapes, dt, was_on_floor, up),
            MotionMode::Floating => self.slide_floating(&space, &shapes, dt),
        }

        if self.platform_on_leave != PlatformOnLeave::DoNothing && !self.on_floor && !self.on_wall {
            if self.platform_on_leave == PlatformOnLeave::AddUpwardVelocity
                && current_platform_velocity.dot(up) < 0.0
            {
                current_platform_velocity = godot_sat::slide(current_platform_velocity, up);
            }
            self.velocity = self.velocity + current_platform_velocity;
        }
        self.velocity
    }

    fn push_result(&mut self, r: &MotionResult) {
        self.slide_collisions.push(KinematicCollision2D {
            collider: r.collider,
            normal: r.collision_normal,
            depth: r.collision_depth,
            position: r.collision_point,
            remainder: r.remainder,
            travel: r.travel,
            collider_velocity: r.collider_velocity,
        });
    }

    fn set_platform_data(&mut self, r: &MotionResult, space: &MotionSpace) {
        let Some(b) = r.collider.and_then(|id| space.body(id)) else {
            return;
        };
        self.platform_body = Some(b.id);
        self.platform_velocity = r.collider_velocity;
        self.platform_layer = b.collision_layer;
    }

    /// `_set_collision_direction`.
    fn set_collision_direction(&mut self, r: &MotionResult, space: &MotionSpace, up: Vector2) {
        let grounded = self.motion_mode == MotionMode::Grounded;
        if grounded && r.get_angle(up) <= self.floor_max_angle + FLOOR_ANGLE_THRESHOLD {
            self.on_floor = true;
            self.floor_normal = r.collision_normal;
            self.set_platform_data(r, space);
        } else if grounded && r.get_angle(-up) <= self.floor_max_angle + FLOOR_ANGLE_THRESHOLD {
            self.on_ceiling = true;
        } else {
            self.on_wall = true;
            self.wall_normal = r.collision_normal;
            // Don't apply wall velocity when the collider is a CharacterBody2D.
            let is_character = r
                .collider
                .and_then(|id| space.body(id))
                .is_some_and(|b| b.is_character_body);
            if !is_character {
                self.set_platform_data(r, space);
            }
        }
    }

    fn is_on_floor_only(&self) -> bool {
        self.on_floor && !self.on_wall && !self.on_ceiling
    }

    fn is_on_wall_only(&self) -> bool {
        self.on_wall && !self.on_floor && !self.on_ceiling
    }

    /// `_move_and_slide_grounded`.
    fn slide_grounded(
        &mut self,
        space: &MotionSpace,
        shapes: &[(SatShape, Transform2D)],
        dt: f32,
        was_on_floor: bool,
        up: Vector2,
    ) {
        use godot_sat::{normalized, slide, vec_is_zero_approx};
        let mut motion = self.velocity * dt;
        let motion_slide_up = slide(motion, up);
        let prev_floor_normal = self.floor_normal;

        self.platform_body = None;
        self.floor_normal = Vector2::ZERO;
        self.platform_velocity = Vector2::ZERO;

        let mut sliding_enabled = !self.floor_stop_on_slope;
        let mut can_apply_constant_speed = sliding_enabled;
        let mut apply_ceiling_velocity = false;
        let mut first_slide = true;
        let vel_dir_facing_up = self.velocity.dot(up) > 0.0;
        let mut last_travel = Vector2::ZERO;
        let margin = self.safe_margin;

        for _ in 0..self.max_slides.max(1) {
            let p = self.params(motion);
            let prev_position = p.from.origin;
            let (mut collided, r) =
                self.godot_move_and_collide(space, shapes, &p, false, !sliding_enabled);
            self.last_motion = r.travel;

            if collided {
                self.push_result(&r);
                self.set_collision_direction(&r, space, up);

                if self.on_ceiling
                    && r.collider_velocity != Vector2::ZERO
                    && r.collider_velocity.dot(up) < 0.0
                    && (!self.slide_on_ceiling
                        || motion.dot(up) < 0.0
                        || (r.collision_normal + up).length() < 0.01)
                {
                    apply_ceiling_velocity = true;
                    let ceiling_vertical_velocity = up * up.dot(r.collider_velocity);
                    let motion_vertical_velocity = up * up.dot(self.velocity);
                    if motion_vertical_velocity.dot(up) > 0.0
                        || ceiling_vertical_velocity.length_squared()
                            > motion_vertical_velocity.length_squared()
                    {
                        self.velocity = ceiling_vertical_velocity + slide(self.velocity, up);
                    }
                }

                if self.on_floor
                    && self.floor_stop_on_slope
                    && (normalized(self.velocity) + up).length() < 0.01
                {
                    if r.travel.length() <= margin + godot_sat::CMP_EPSILON {
                        self.position = self.position - r.travel;
                    }
                    self.velocity = Vector2::ZERO;
                    self.last_motion = Vector2::ZERO;
                    // (Godot also zeroes `motion`; the loop ends here.)
                    break;
                }

                if vec_is_zero_approx(r.remainder) {
                    // (Godot also zeroes `motion`; the loop ends here.)
                    break;
                }

                if self.floor_block_on_wall
                    && self.on_wall
                    && motion_slide_up.dot(r.collision_normal) <= 0.0
                {
                    if was_on_floor && !self.on_floor && !vel_dir_facing_up {
                        if r.travel.length() <= margin + godot_sat::CMP_EPSILON {
                            self.position = self.position - r.travel;
                        }
                        self.snap_on_floor(space, shapes, true, false, true, up);
                        self.velocity = Vector2::ZERO;
                        self.last_motion = Vector2::ZERO;
                        // (Godot also zeroes `motion`; the loop ends here.)
                        break;
                    } else if !self.on_floor {
                        motion = up * up.dot(r.remainder);
                        motion = slide(motion, r.collision_normal);
                    } else {
                        motion = r.remainder;
                    }
                } else if self.floor_constant_speed
                    && self.is_on_floor_only()
                    && can_apply_constant_speed
                    && was_on_floor
                    && motion.dot(r.collision_normal) < 0.0
                {
                    can_apply_constant_speed = false;
                    let motion_slide_norm = normalized(slide(r.remainder, r.collision_normal));
                    motion = motion_slide_norm
                        * (motion_slide_up.length()
                            - slide(r.travel, up).length()
                            - slide(last_travel, up).length());
                } else if (sliding_enabled || !self.on_floor)
                    && (!self.on_ceiling || self.slide_on_ceiling || !vel_dir_facing_up)
                    && !apply_ceiling_velocity
                {
                    let slide_motion = slide(r.remainder, r.collision_normal);
                    motion = if slide_motion.dot(self.velocity) > 0.0 {
                        slide_motion
                    } else {
                        Vector2::ZERO
                    };
                    if self.slide_on_ceiling && self.on_ceiling {
                        if vel_dir_facing_up {
                            self.velocity = slide(self.velocity, r.collision_normal);
                        } else {
                            self.velocity = up * up.dot(self.velocity);
                        }
                    }
                } else {
                    motion = r.remainder;
                    if self.on_ceiling && !self.slide_on_ceiling && vel_dir_facing_up {
                        self.velocity = slide(self.velocity, up);
                        motion = slide(motion, up);
                    }
                }
                last_travel = r.travel;
            } else if self.floor_constant_speed
                && first_slide
                && self.on_floor_if_snapped(space, shapes, was_on_floor, vel_dir_facing_up, up)
            {
                can_apply_constant_speed = false;
                sliding_enabled = true;
                self.position = prev_position;
                let motion_slide_norm = normalized(slide(motion, prev_floor_normal));
                motion = motion_slide_norm * motion_slide_up.length();
                collided = true;
            }

            can_apply_constant_speed = !can_apply_constant_speed && !sliding_enabled;
            sliding_enabled = true;
            first_slide = false;

            if !collided || vec_is_zero_approx(motion) {
                break;
            }
        }

        self.snap_on_floor(space, shapes, was_on_floor, vel_dir_facing_up, false, up);

        // Scales the horizontal velocity according to the wall slope.
        if self.is_on_wall_only() {
            if let Some(first) = self.slide_collisions.first().map(|c| c.normal) {
                if motion_slide_up.dot(first) < 0.0 {
                    let slide_motion = slide(self.velocity, first);
                    if motion_slide_up.dot(slide_motion) < 0.0 {
                        self.velocity = up * up.dot(self.velocity);
                    } else {
                        self.velocity = up * up.dot(self.velocity) + slide(slide_motion, up);
                    }
                }
            }
        }

        // Reset the gravity accumulation when touching the ground.
        if self.on_floor && !vel_dir_facing_up {
            self.velocity = slide(self.velocity, up);
        }
    }

    /// `_move_and_slide_floating`.
    fn slide_floating(&mut self, space: &MotionSpace, shapes: &[(SatShape, Transform2D)], dt: f32) {
        use godot_sat::{normalized, slide, vec_is_zero_approx};
        let mut motion = self.velocity * dt;
        self.platform_body = None;
        self.floor_normal = Vector2::ZERO;
        self.platform_velocity = Vector2::ZERO;
        let up = self.up();
        let mut first_slide = true;
        for _ in 0..self.max_slides.max(1) {
            let p = self.params(motion);
            let (collided, r) = self.godot_move_and_collide(space, shapes, &p, false, false);
            self.last_motion = r.travel;
            if collided {
                self.push_result(&r);
                self.set_collision_direction(&r, space, up);
                if vec_is_zero_approx(r.remainder) {
                    break;
                }
                if self.wall_min_slide_angle != 0.0
                    && r.get_angle(-normalized(self.velocity))
                        < self.wall_min_slide_angle + FLOOR_ANGLE_THRESHOLD
                {
                    motion = Vector2::ZERO;
                } else if first_slide {
                    let motion_slide_norm = normalized(slide(r.remainder, r.collision_normal));
                    motion = motion_slide_norm * (motion.length() - r.travel.length());
                } else {
                    motion = slide(r.remainder, r.collision_normal);
                }
                if motion.dot(self.velocity) <= 0.0 {
                    motion = Vector2::ZERO;
                }
            }
            if !collided || vec_is_zero_approx(motion) {
                break;
            }
            first_slide = false;
        }
    }

    /// `_apply_floor_snap(p_wall_as_floor)`.
    fn apply_floor_snap(
        &mut self,
        space: &MotionSpace,
        shapes: &[(SatShape, Transform2D)],
        wall_as_floor: bool,
        up: Vector2,
    ) {
        if self.on_floor {
            return;
        }
        let length = self.floor_snap_length.max(self.safe_margin);
        let mut p = self.params(-up * length);
        p.collide_separation_ray = true;
        let (hit, mut r) = self.godot_move_and_collide(space, shapes, &p, true, false);
        if hit
            && (r.get_angle(up) <= self.floor_max_angle + FLOOR_ANGLE_THRESHOLD
                || (wall_as_floor
                    && r.get_angle(-up) > self.floor_max_angle + FLOOR_ANGLE_THRESHOLD))
        {
            self.on_floor = true;
            self.floor_normal = r.collision_normal;
            self.set_platform_data(&r, space);
            if r.travel.length() > self.safe_margin {
                r.travel = up * up.dot(r.travel);
            } else {
                r.travel = Vector2::ZERO;
            }
            self.position = p.from.origin + r.travel;
        }
    }

    fn snap_on_floor(
        &mut self,
        space: &MotionSpace,
        shapes: &[(SatShape, Transform2D)],
        was_on_floor: bool,
        vel_dir_facing_up: bool,
        wall_as_floor: bool,
        up: Vector2,
    ) {
        if self.on_floor || !was_on_floor || vel_dir_facing_up {
            return;
        }
        self.apply_floor_snap(space, shapes, wall_as_floor, up);
    }

    fn on_floor_if_snapped(
        &mut self,
        space: &MotionSpace,
        shapes: &[(SatShape, Transform2D)],
        was_on_floor: bool,
        vel_dir_facing_up: bool,
        up: Vector2,
    ) -> bool {
        if self.up_direction == Vector2::ZERO || self.on_floor || !was_on_floor || vel_dir_facing_up
        {
            return false;
        }
        let length = self.floor_snap_length.max(self.safe_margin);
        let mut p = self.params(-up * length);
        p.collide_separation_ray = true;
        let (hit, r) = self.godot_move_and_collide(space, shapes, &p, true, false);
        hit && r.get_angle(up) <= self.floor_max_angle + FLOOR_ANGLE_THRESHOLD
    }

    /// Number of collisions recorded during the last `move_and_slide` call.
    pub fn get_slide_collision_count(&self) -> usize {
        self.slide_collisions.len()
    }

    /// Returns the `i`th slide collision from the last `move_and_slide` call.
    pub fn get_slide_collision(&self, i: usize) -> Option<&KinematicCollision2D> {
        self.slide_collisions.get(i)
    }

    /// Velocity (px/s) of the platform the body is standing on.
    pub fn get_platform_velocity(&self) -> Vector2 {
        self.platform_velocity
    }

    /// `get_last_motion`.
    pub fn get_last_motion(&self) -> Vector2 {
        self.last_motion
    }

    /// Seeds the "was on the floor last frame" state.
    pub fn set_on_floor(&mut self, on_floor: bool) {
        self.on_floor = on_floor;
    }

    /// Seeds the contact state Godot keeps as node members between
    /// `move_and_slide` calls.
    pub fn set_contact_state(
        &mut self,
        on_floor: bool,
        on_wall: bool,
        on_ceiling: bool,
        floor_normal: Vector2,
        wall_normal: Vector2,
    ) {
        self.on_floor = on_floor;
        self.on_wall = on_wall;
        self.on_ceiling = on_ceiling;
        self.floor_normal = floor_normal;
        self.wall_normal = wall_normal;
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
    pub fn get_floor_normal(&self) -> Vector2 {
        self.floor_normal
    }

    /// Returns the wall normal from the last `move_and_slide` call.
    pub fn get_wall_normal(&self) -> Vector2 {
        self.wall_normal
    }

    /// `PhysicsBody2D.move_and_collide(motion)`: Godot's `_move` with the
    /// body's safe margin, `recovery_as_collision = false` and slide
    /// cancelling on. Stops at the first collision.
    pub fn move_and_collide(
        &mut self,
        motion: Vector2,
        bodies: &[&PhysicsBody2D],
    ) -> Option<KinematicCollision2D> {
        let space = MotionSpace::new(bodies, self.collision_mask, 1.0 / 60.0);
        let shapes = self.own_shapes();
        let p = MotionParameters::new(self.global_transform(), motion, self.safe_margin);
        let (hit, r) = self.godot_move_and_collide(&space, &shapes, &p, false, true);
        if !hit {
            return None;
        }
        let up = self.up();
        let floor_cos = self.floor_max_angle.cos();
        let n = r.collision_normal;
        let dot = n.dot(up);
        if dot > floor_cos {
            self.on_floor = true;
            self.floor_normal = n;
        } else if dot < -floor_cos {
            self.on_ceiling = true;
        } else {
            self.on_wall = true;
            self.wall_normal = n;
        }
        Some(KinematicCollision2D {
            collider: r.collider,
            normal: n,
            depth: r.collision_depth,
            position: r.collision_point,
            remainder: r.remainder,
            travel: r.travel,
            collider_velocity: r.collider_velocity,
        })
    }
}

/// Collision information returned by [`CharacterBody2D::move_and_collide`].
#[derive(Debug, Clone)]
pub struct KinematicCollision2D {
    /// The body that was hit, for Godot's `get_collider()` (pat-s3rrv.95).
    ///
    /// ⚠️ A `BodyId`, not a node: this crate is the solver and knows nothing
    /// about the scene tree. `PhysicsServer` already keeps `body_to_node`, so
    /// the script-facing `get_collider()` resolves the node there. Recording the
    /// id is what makes that resolution possible at all — the collision data was
    /// previously computed and discarded with no record of what was struck.
    ///
    /// `None` only for a collision built without a known body (test fixtures).
    pub collider: Option<BodyId>,
    /// The surface normal at the collision point (pointing away from the body).
    pub normal: Vector2,
    /// Penetration depth.
    pub depth: f32,
    /// Position of the character after collision resolution.
    pub position: Vector2,
    /// Remaining motion not applied.
    pub remainder: Vector2,
    /// Motion actually travelled (`get_travel`).
    pub travel: Vector2,
    /// The collider's velocity at the contact (`get_collider_velocity`).
    pub collider_velocity: Vector2,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_static_floor(y: f32) -> PhysicsBody2D {
        PhysicsBody2D::new(
            BodyId(1),
            BodyType::Static,
            Vector2::new(0.0, y),
            Shape2D::Rectangle {
                half_extents: Vector2::new(100.0, 1.0),
            },
            1.0,
        )
    }

    fn make_static_wall(x: f32) -> PhysicsBody2D {
        PhysicsBody2D::new(
            BodyId(2),
            BodyType::Static,
            Vector2::new(x, 0.0),
            Shape2D::Rectangle {
                half_extents: Vector2::new(1.0, 100.0),
            },
            1.0,
        )
    }

    fn make_static_ceiling(y: f32) -> PhysicsBody2D {
        PhysicsBody2D::new(
            BodyId(3),
            BodyType::Static,
            Vector2::new(0.0, y),
            Shape2D::Rectangle {
                half_extents: Vector2::new(100.0, 1.0),
            },
            1.0,
        )
    }

    #[test]
    fn move_and_slide_no_collision() {
        let mut character = CharacterBody2D::new(Vector2::ZERO, Shape2D::Circle { radius: 1.0 });
        let bodies: Vec<&PhysicsBody2D> = vec![];
        let result = character.move_and_slide(Vector2::new(10.0, 0.0), &bodies);
        assert!((character.position.x - 10.0).abs() < 1e-4);
        assert!(!character.is_on_floor());
        assert!(!character.is_on_wall());
        assert!((result.x - 10.0).abs() < 1e-4);
    }

    #[test]
    fn move_and_slide_hits_floor() {
        let mut character =
            CharacterBody2D::new(Vector2::new(0.0, 0.0), Shape2D::Circle { radius: 1.0 });
        // Floor rect centered at y=10.5 with half_extent 1 => top edge at y=9.5
        // Character circle radius 1 moves to y=10 => bottom at y=9 => overlap 0.5
        let floor = make_static_floor(10.5);
        let bodies: Vec<&PhysicsBody2D> = vec![&floor];

        let result = character.move_and_slide(Vector2::new(0.0, 10.0), &bodies);
        assert!(character.is_on_floor(), "Should detect floor");
        assert!(!character.is_on_wall());
        assert!(result.y.abs() < 1e-3, "Y velocity should be zeroed");
    }

    #[test]
    fn move_and_slide_slides_along_wall() {
        let mut character =
            CharacterBody2D::new(Vector2::new(0.0, 0.0), Shape2D::Circle { radius: 1.0 });
        // Wall at x=10.5 (left edge at x=9.5). Character moves to x=10, overlap 0.5
        let wall = make_static_wall(10.5);
        let bodies: Vec<&PhysicsBody2D> = vec![&wall];

        let result = character.move_and_slide(Vector2::new(10.0, 5.0), &bodies);
        assert!(character.is_on_wall(), "Should detect wall");
        assert!(result.x.abs() < 1e-3, "X velocity should be zeroed by wall");
        assert!((result.y - 5.0).abs() < 1e-3, "Y velocity should remain");
    }

    #[test]
    fn move_and_slide_detects_ceiling() {
        let mut character =
            CharacterBody2D::new(Vector2::new(0.0, 0.0), Shape2D::Circle { radius: 1.0 });
        // Ceiling at y=-10.5 (bottom edge at y=-9.5). Character moves to y=-10, overlap 0.5
        let ceiling = make_static_ceiling(-10.5);
        let bodies: Vec<&PhysicsBody2D> = vec![&ceiling];

        let _result = character.move_and_slide(Vector2::new(0.0, -10.0), &bodies);
        assert!(character.is_on_ceiling(), "Should detect ceiling");
        assert!(!character.is_on_floor());
    }

    #[test]
    fn move_and_slide_respects_collision_mask() {
        let mut character = CharacterBody2D::new(Vector2::ZERO, Shape2D::Circle { radius: 1.0 });
        character.collision_mask = 2; // Only scan layer 2

        let mut wall = make_static_wall(3.0);
        wall.collision_layer = 1; // Wall is on layer 1 — shouldn't collide
        let bodies: Vec<&PhysicsBody2D> = vec![&wall];

        let _result = character.move_and_slide(Vector2::new(10.0, 0.0), &bodies);
        assert!(!character.is_on_wall());
        assert!((character.position.x - 10.0).abs() < 1e-4);
    }

    #[test]
    fn get_floor_normal_returns_correct_value() {
        let mut character =
            CharacterBody2D::new(Vector2::new(0.0, 0.0), Shape2D::Circle { radius: 1.0 });
        // Floor at y=10.5, character moves to y=10
        let floor = make_static_floor(10.5);
        let bodies: Vec<&PhysicsBody2D> = vec![&floor];

        character.move_and_slide(Vector2::new(0.0, 10.0), &bodies);
        let normal = character.get_floor_normal();
        assert!(
            normal.y < -0.5,
            "Floor normal should point up (negative Y), got {:?}",
            normal
        );
    }

    // -----------------------------------------------------------------------
    // pat-u3l8c.48 — px/s velocity semantics, floor snap, moving platforms,
    // slope knobs, and slide-collision list.
    // -----------------------------------------------------------------------

    /// A wide floor/platform rectangle whose top edge is at `top_y`.
    fn make_platform(id: u64, top_y: f32, vel: Vector2) -> PhysicsBody2D {
        let half = Vector2::new(200.0, 1.0);
        let mut body = PhysicsBody2D::new(
            BodyId(id),
            BodyType::Kinematic,
            Vector2::new(0.0, top_y + half.y),
            Shape2D::Rectangle { half_extents: half },
            1.0,
        );
        body.linear_velocity = vel;
        body
    }

    /// The velocity returned by `move_and_slide_delta` stays in px/s across a
    /// free (non-colliding) move, instead of collapsing to `velocity * dt`.
    #[test]
    fn move_and_slide_delta_keeps_velocity_in_pixels_per_second() {
        let mut character = CharacterBody2D::new(Vector2::ZERO, Shape2D::Circle { radius: 1.0 });
        let bodies: Vec<&PhysicsBody2D> = vec![];
        let dt = 1.0 / 60.0;

        let result = character.move_and_slide_delta(Vector2::new(120.0, 0.0), dt, &bodies);
        // Displacement is velocity * dt = 2px this tick...
        assert!((character.position.x - 120.0 * dt).abs() < 1e-3);
        // ...but the returned velocity is still 120 px/s, not 2 px/tick.
        assert!(
            (result.x - 120.0).abs() < 1e-3,
            "velocity must remain px/s, got {:?}",
            result
        );
    }

    /// Iterating `move_and_slide_delta` frame-over-frame does not decay speed.
    #[test]
    fn move_and_slide_delta_does_not_decay_speed_over_frames() {
        let mut character = CharacterBody2D::new(Vector2::ZERO, Shape2D::Circle { radius: 1.0 });
        let bodies: Vec<&PhysicsBody2D> = vec![];
        let dt = 1.0 / 60.0;
        let mut velocity = Vector2::new(200.0, 0.0);

        for _ in 0..30 {
            velocity = character.move_and_slide_delta(velocity, dt, &bodies);
        }
        // Constant horizontal velocity over 30 frames => ~100px of travel and
        // undiminished speed. The old bug collapsed this to ~v*dt each frame.
        assert!(
            (velocity.x - 200.0).abs() < 1e-2,
            "speed must not decay, got {:?}",
            velocity
        );
        assert!(
            character.position.x > 90.0,
            "should travel ~100px in 30 frames, got x={}",
            character.position.x
        );
    }

    /// Floor snapping keeps the body glued to a floor within snap range.
    #[test]
    fn move_and_slide_snaps_to_floor_within_range() {
        // Floor top at y=11. Character floats 0.5px above it.
        let floor = make_platform(1, 11.0, Vector2::ZERO);
        let bodies: Vec<&PhysicsBody2D> = vec![&floor];

        let mut character =
            CharacterBody2D::new(Vector2::new(0.0, 9.5), Shape2D::Circle { radius: 1.0 });
        character.on_floor = true; // was grounded last frame
        character.floor_snap_length = 2.0;

        // Horizontal move only; without snapping the body would stay airborne.
        character.move_and_slide_delta(Vector2::new(5.0, 0.0), 1.0, &bodies);
        assert!(character.is_on_floor(), "snap should re-ground the body");
        // Godot's snap is a cast: it stops at the 8-step binary search's SAFE
        // fraction of the 2 px snap, i.e. up to 2/256*1.5 px short of contact.
        assert!(
            (character.position.y - 10.0).abs() < 2e-2,
            "body should rest on floor top (center y=10), got {}",
            character.position.y
        );
    }

    /// With snapping disabled the body leaves the floor on a horizontal move.
    #[test]
    fn move_and_slide_without_snap_leaves_floor() {
        let floor = make_platform(1, 11.0, Vector2::ZERO);
        let bodies: Vec<&PhysicsBody2D> = vec![&floor];

        let mut character =
            CharacterBody2D::new(Vector2::new(0.0, 9.5), Shape2D::Circle { radius: 1.0 });
        character.on_floor = true;
        character.floor_snap_length = 0.0; // snapping off

        character.move_and_slide_delta(Vector2::new(5.0, 0.0), 1.0, &bodies);
        assert!(!character.is_on_floor(), "no snap => should be airborne");
        assert!((character.position.y - 9.5).abs() < 1e-3);
    }

    /// A body standing on a moving platform is carried by the platform.
    #[test]
    fn move_and_slide_carries_body_with_moving_platform() {
        let platform = make_platform(1, 11.0, Vector2::new(20.0, 0.0));
        let bodies: Vec<&PhysicsBody2D> = vec![&platform];

        let mut character =
            CharacterBody2D::new(Vector2::new(0.0, 9.5), Shape2D::Circle { radius: 1.0 });
        character.on_floor = true;
        character.platform_velocity = Vector2::new(20.0, 0.0);
        character.floor_snap_length = 2.0;

        // No self-velocity: the body should still ride the platform.
        character.move_and_slide_delta(Vector2::ZERO, 1.0, &bodies);
        assert!(
            (character.position.x - 20.0).abs() < 1e-2,
            "body should be carried +20px by the platform, got x={}",
            character.position.x
        );
        assert!(character.is_on_floor());
        assert!(
            (character.get_platform_velocity().x - 20.0).abs() < 1e-2,
            "platform velocity should persist, got {:?}",
            character.get_platform_velocity()
        );
    }

    /// Leaving a moving platform imparts the platform's velocity to the body.
    #[test]
    fn move_and_slide_adds_platform_velocity_on_leave() {
        // No bodies: the character jumps off into empty space.
        let bodies: Vec<&PhysicsBody2D> = vec![];
        let mut character =
            CharacterBody2D::new(Vector2::new(0.0, 9.5), Shape2D::Circle { radius: 1.0 });
        character.on_floor = true;
        character.platform_velocity = Vector2::new(20.0, 0.0);

        // Jump straight up; leaving the platform should add its +20 x velocity.
        let result = character.move_and_slide_delta(Vector2::new(0.0, -100.0), 1.0, &bodies);
        assert!(!character.is_on_floor());
        assert!(
            (result.x - 20.0).abs() < 1e-2,
            "leaving platform should add its velocity, got {:?}",
            result
        );
    }

    /// The slide-collision list is populated and exposed after a wall hit.
    #[test]
    fn slide_collision_list_is_exposed() {
        let wall = make_static_wall(10.5);
        let bodies: Vec<&PhysicsBody2D> = vec![&wall];
        let mut character = CharacterBody2D::new(Vector2::ZERO, Shape2D::Circle { radius: 1.0 });

        character.move_and_slide(Vector2::new(10.0, 5.0), &bodies);
        assert!(character.get_slide_collision_count() >= 1);
        let c = character.get_slide_collision(0).expect("one collision");
        assert!(
            c.normal.x < -0.5,
            "wall normal should point left, got {:?}",
            c.normal
        );
    }

    /// `max_slides` bounds the number of slide iterations (and thus recorded
    /// collisions) in a corner where multiple contacts would otherwise resolve.
    #[test]
    fn max_slides_bounds_iterations() {
        // Floor spans y in [0.5, 2.5]; wall spans x in [0.5, 2.5] — a corner.
        let floor = make_static_floor(1.5);
        let wall = make_static_wall(1.5);
        let bodies: Vec<&PhysicsBody2D> = vec![&floor, &wall];

        // Move into the corner so the target overlaps both bodies.
        let mut capped = CharacterBody2D::new(Vector2::ZERO, Shape2D::Circle { radius: 1.0 });
        capped.max_slides = 1;
        capped.move_and_slide(Vector2::new(1.5, 1.5), &bodies);
        assert!(
            capped.get_slide_collision_count() <= 1,
            "max_slides=1 must cap collisions to 1, got {}",
            capped.get_slide_collision_count()
        );
        assert!(
            capped.get_slide_collision_count() >= 1,
            "should still register the first collision"
        );

        // The same move with the default higher cap resolves the corner further.
        let mut uncapped = CharacterBody2D::new(Vector2::ZERO, Shape2D::Circle { radius: 1.0 });
        uncapped.move_and_slide(Vector2::new(1.5, 1.5), &bodies);
        assert!(
            uncapped.get_slide_collision_count() >= capped.get_slide_collision_count(),
            "raising max_slides should not reduce resolved collisions"
        );
    }

    /// A large ground circle used as a sloped floor. Contact against its side
    /// yields an angled floor normal (impossible with axis-aligned rects).
    fn make_ground_circle() -> PhysicsBody2D {
        PhysicsBody2D::new(
            BodyId(9),
            BodyType::Static,
            Vector2::new(0.0, 110.0),
            Shape2D::Circle { radius: 100.0 },
            1.0,
        )
    }

    /// floor_stop_on_slope stops a body with no lateral input from sliding down.
    #[test]
    fn floor_stop_on_slope_prevents_downhill_slide() {
        // ⚠️ Measured on DISPLACEMENT, not the returned velocity. In Godot the
        // knob shapes the motion; `velocity` is flattened by the floor reset
        // (`character_body_2d.cpp:281`, `velocity.slide(up_direction)`) either
        // way, so a purely-downward input returns ~0 in both cases. Both bodies
        // start overlapping the slope by the same amount, so the depenetration
        // push is common-mode and the DIFFERENCE is the downhill slide.
        let ground = make_ground_circle();
        let bodies: Vec<&PhysicsBody2D> = vec![&ground];
        let start = Vector2::new(30.0, 13.6);

        let mut stopping = CharacterBody2D::new(start, Shape2D::Circle { radius: 1.0 });
        stopping.floor_stop_on_slope = true;
        let v_stop = stopping.move_and_slide_delta(Vector2::new(0.0, 30.0), 1.0, &bodies);
        assert!(stopping.is_on_floor(), "should detect the slope as floor");
        assert!(
            v_stop.length() < 0.5,
            "stopped on the slope, got {v_stop:?}"
        );

        let mut sliding = CharacterBody2D::new(start, Shape2D::Circle { radius: 1.0 });
        sliding.floor_stop_on_slope = false;
        sliding.move_and_slide_delta(Vector2::new(0.0, 30.0), 1.0, &bodies);

        let downhill_stop = (stopping.position - start).x;
        let downhill_slide = (sliding.position - start).x;
        assert!(
            downhill_slide > downhill_stop + 0.3,
            "without stop_on_slope the body must slide further downhill: \
             slid {downhill_slide} vs stopped {downhill_stop}"
        );
    }

    /// floor_constant_speed keeps the TRAVEL across a floor slide.
    #[test]
    fn floor_constant_speed_preserves_speed() {
        // On displacement for the same reason as the test above: Godot's
        // constant speed is a property of the motion (`character_body_2d.cpp:
        // 210-213`); the returned velocity is floor-flattened in both cases.
        let ground = make_ground_circle();
        let bodies: Vec<&PhysicsBody2D> = vec![&ground];
        // Godot applies constant speed only to a body that WAS on the floor and
        // walks INTO the slope (`character_body_2d.cpp:210`: `p_was_on_floor`
        // and `motion.dot(collision_normal) < 0`), so: grounded, walking uphill.
        let input = Vector2::new(-20.0, 0.0);
        let start = Vector2::new(30.3, 13.65);

        let mut constant = CharacterBody2D::new(start, Shape2D::Circle { radius: 1.0 });
        constant.floor_constant_speed = true;
        constant.set_on_floor(true);
        constant.move_and_slide_delta(input, 0.5, &bodies);
        assert!(constant.is_on_floor());

        let mut varying = CharacterBody2D::new(start, Shape2D::Circle { radius: 1.0 });
        varying.floor_constant_speed = false;
        varying.set_on_floor(true);
        varying.move_and_slide_delta(input, 0.5, &bodies);

        let d_const = (constant.position - start).length();
        let d_var = (varying.position - start).length();
        assert!(
            d_const > d_var + 0.1,
            "constant_speed must keep more of the travel than a plain slide, \
             which bleeds it into the slope normal: {d_const} vs {d_var}"
        );
    }

    /// slide_on_ceiling=false cancels motion on ceiling contact (no sideways
    /// slide); slide_on_ceiling=true keeps horizontal velocity.
    #[test]
    fn slide_on_ceiling_knob_controls_sideways_motion() {
        let ceiling = make_static_ceiling(-10.5); // bottom edge at y=-9.5
        let bodies: Vec<&PhysicsBody2D> = vec![&ceiling];

        // Moving up-and-right into the ceiling.
        let mut sliding =
            CharacterBody2D::new(Vector2::new(0.0, 0.0), Shape2D::Circle { radius: 1.0 });
        sliding.slide_on_ceiling = true;
        let v_slide = sliding.move_and_slide_delta(Vector2::new(20.0, -10.0), 1.0, &bodies);
        assert!(sliding.is_on_ceiling());
        assert!(
            v_slide.x.abs() > 5.0,
            "slide_on_ceiling should keep horizontal velocity, got {:?}",
            v_slide
        );

        let mut stopping =
            CharacterBody2D::new(Vector2::new(0.0, 0.0), Shape2D::Circle { radius: 1.0 });
        stopping.slide_on_ceiling = false;
        let v_stop = stopping.move_and_slide_delta(Vector2::new(20.0, -10.0), 1.0, &bodies);
        assert!(stopping.is_on_ceiling());
        // Godot (`character_body_2d.cpp:236`): with slide_on_ceiling off, an
        // upward body loses its UPWARD velocity (`velocity.slide(up)`), not its
        // horizontal one — against a flat ceiling both settings agree.
        assert!(
            v_stop.y.abs() < 1e-3 && (v_stop.x - 20.0).abs() < 1e-3,
            "slide_on_ceiling=false drops only the upward velocity, got {:?}",
            v_stop
        );
    }

    /// floor_block_on_wall stops a grounded body from climbing a wall.
    #[test]
    fn floor_block_on_wall_prevents_wall_climb() {
        let wall = make_static_wall(10.5);
        let bodies: Vec<&PhysicsBody2D> = vec![&wall];

        // Grounded body moving right and up into a wall. The target center must
        // land inside the wall's collision band (x in [8.5, 12.5]).
        let mut blocked =
            CharacterBody2D::new(Vector2::new(0.0, 0.0), Shape2D::Circle { radius: 1.0 });
        blocked.on_floor = true; // grounded last frame
        blocked.floor_block_on_wall = true;
        let v_blocked = blocked.move_and_slide_delta(Vector2::new(10.0, -30.0), 1.0, &bodies);
        assert!(blocked.is_on_wall());
        // Godot keeps the upward velocity (a jump along a wall is legal) and
        // drops the part pushing into the wall (`is_on_wall_only` rescale,
        // `character_body_2d.cpp:270`).
        assert!(
            (v_blocked.y + 30.0).abs() < 1e-2 && v_blocked.x.abs() < 1e-2,
            "block_on_wall keeps the jump and drops the wall push, got {:?}",
            v_blocked
        );

        // Same move, blocking disabled: the body slides up the wall.
        let mut climbing =
            CharacterBody2D::new(Vector2::new(0.0, 0.0), Shape2D::Circle { radius: 1.0 });
        climbing.on_floor = true;
        climbing.floor_block_on_wall = false;
        let v_climb = climbing.move_and_slide_delta(Vector2::new(10.0, -30.0), 1.0, &bodies);
        assert!(
            v_climb.y < -5.0,
            "without block_on_wall the body slides up the wall, got {:?}",
            v_climb
        );
    }
}

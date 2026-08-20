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
use crate::shape::Shape2D;

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
    /// Collision shape in local space.
    pub shape: Shape2D,
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

/// Lateral-velocity threshold under which `floor_stop_on_slope` treats the body
/// as having no player input (px/s).
const STOP_ON_SLOPE_LATERAL_EPS: f32 = 1e-3;

/// How far down [`probe_resting_floor`] looks for the surface a body is already
/// standing on (pat-s3rrv.93).
///
/// ⚠️ Deliberately tiny. This probe must find the floor a body is TOUCHING, not
/// pull it down onto one it is merely near — that is `floor_snap_length`'s job,
/// and it is opt-in. A large value here would silently ground a body hovering
/// above a platform and start carrying it.
const RESTING_FLOOR_PROBE: f32 = 1e-2;

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
    /// Creates a new character body at the given position.
    pub fn new(position: Vector2, shape: Shape2D) -> Self {
        Self {
            position,
            shape,
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
            on_floor: false,
            on_wall: false,
            on_ceiling: false,
            floor_normal: Vector2::ZERO,
            wall_normal: Vector2::ZERO,
            slide_collisions: Vec::new(),
        }
    }

    /// Convenience wrapper that treats `velocity` as a one-shot per-tick motion
    /// vector (`delta = 1.0`), sliding along colliding surfaces.
    ///
    /// This preserves the historical "move by exactly this displacement" API.
    /// For frame-rate-correct movement where `velocity` is in pixels-per-second,
    /// call [`CharacterBody2D::move_and_slide_delta`] with the physics timestep.
    pub fn move_and_slide(&mut self, velocity: Vector2, bodies: &[&PhysicsBody2D]) -> Vector2 {
        self.move_and_slide_delta(velocity, 1.0, bodies)
    }

    /// Moves the character according to `velocity` (pixels-per-second) over the
    /// timestep `dt`, sliding along colliding surfaces.
    ///
    /// The per-tick displacement is `velocity * dt`; sliding removes velocity
    /// components along contact normals but keeps the result in
    /// pixels-per-second, so the returned value can be written straight back to
    /// `velocity` for the next frame. Also performs floor snapping, moving
    /// platform carry / on-leave velocity, and honors the slope knobs.
    pub fn move_and_slide_delta(
        &mut self,
        velocity: Vector2,
        dt: f32,
        bodies: &[&PhysicsBody2D],
    ) -> Vector2 {
        self.velocity = velocity;
        self.slide_collisions.clear();

        let up = if self.up_direction.length_squared() > 1e-8 {
            self.up_direction.normalized()
        } else {
            Vector2::new(0.0, -1.0)
        };
        let floor_cos = self.floor_max_angle.cos();

        // Contact state persists into this call as "was on floor last frame".
        let was_on_floor = self.on_floor;
        let incoming_platform_vel = self.platform_velocity;

        // Reset per-call contact state.
        self.on_floor = false;
        self.on_wall = false;
        self.on_ceiling = false;
        self.floor_normal = Vector2::ZERO;
        self.wall_normal = Vector2::ZERO;
        self.platform_velocity = Vector2::ZERO;

        // Velocity facing up (jumping) suppresses floor snapping.
        let vel_dir_facing_up = self.velocity.dot(up) > 0.0;

        // (1) Carry the body with the platform it stood on last frame.
        // Godot applies `platform_on_leave` to the velocity inherited from the
        // platform the body was standing on (pat-0q2kx.113). DO_NOTHING must
        // drop it entirely — carrying it regardless is what made a character
        // keep drifting after stepping off a moving platform.
        let carried = match self.platform_on_leave {
            PlatformOnLeave::AddVelocity => incoming_platform_vel,
            PlatformOnLeave::AddUpwardVelocity => {
                // Keep only the component along `up_direction`.
                let up = self.up_direction.normalized();
                up * incoming_platform_vel.dot(up)
            }
            PlatformOnLeave::DoNothing => Vector2::ZERO,
        };
        if carried.length_squared() > 1e-8 {
            self.position = self.position + carried * dt;
        }

        // Platform velocity captured from the floor body this frame.
        let mut floor_platform_vel = Vector2::ZERO;

        // (2) Grounded slide loop.
        let mut motion = self.velocity * dt;
        let max_slides = self.max_slides.max(1);

        for _ in 0..max_slides {
            if motion.length_squared() < 1e-8 {
                break;
            }

            // Advance in sub-steps no longer than the thinnest shape dimension,
            // so a fast body cannot tunnel between two discrete overlap tests
            // (pat-0q2kx.113). Each sub-step commits its own free movement; the
            // first one that collides falls through to the slide response below
            // with `motion` reduced to the remainder.
            let steps = sweep_steps(motion, &self.shape);
            if steps > 1 {
                let step = motion / steps as f32;
                let mut advanced = 0usize;
                let mut blocked = false;
                for _ in 0..steps {
                    let probe = self.position + step;
                    let tf_probe = Transform2D::translated(probe);
                    let hit = bodies.iter().any(|body| {
                        (self.collision_mask & body.collision_layer) != 0
                            && deepest_hit_against(&self.shape, &tf_probe, body, step).is_some()
                    });
                    if hit {
                        blocked = true;
                        break;
                    }
                    self.position = probe;
                    advanced += 1;
                }
                if !blocked {
                    // The whole motion cleared without touching anything.
                    break;
                }
                // Hand the resolve path exactly the ONE sub-step that collided,
                // not the whole unconsumed remainder.
                //
                // ⚠️ This is the subtle half of substepping. The resolve path
                // below overlap-tests at `position + motion`; if that were the
                // full remainder it would test far past the obstacle the sweep
                // just found, detect nothing, and move there — tunnelling in the
                // exact case the sweep exists to prevent. Narrowing to `step`
                // makes the target equal the probe that actually hit, so the
                // collision is found and slid against. Any distance beyond the
                // contact is dropped, as it is whenever `move_and_slide` hits
                // something.
                motion = step;
            }

            let target = self.position + motion;
            let tf_target = Transform2D::translated(target);
            let mut deepest: Option<(collision::CollisionResult, usize)> = None;

            for (idx, body) in bodies.iter().enumerate() {
                if (self.collision_mask & body.collision_layer) == 0 {
                    continue;
                }
                if let Some(result) = deepest_hit_against(&self.shape, &tf_target, body, motion) {
                    if result.colliding
                        && result.depth > 0.0
                        && (deepest.is_none() || result.depth > deepest.as_ref().unwrap().0.depth)
                    {
                        deepest = Some((result, idx));
                    }
                }
            }

            let Some((result, body_idx)) = deepest else {
                // No collision — move freely and finish.
                self.position = target;
                break;
            };

            // The collision normal points from A (character) toward B (body);
            // the surface normal points from the body toward the character.
            let surface_normal = -result.normal;
            self.classify_surface(surface_normal, up, floor_cos);

            // Depenetrate (push the character out of the body).
            self.position = target + surface_normal * result.depth;

            let is_floor = surface_normal.dot(up) > floor_cos;
            if is_floor {
                floor_platform_vel = bodies[body_idx].linear_velocity;
            }

            // Record the contact for get_slide_collision().
            let remainder = motion - surface_normal * motion.dot(surface_normal);
            self.slide_collisions.push(KinematicCollision2D {
                collider: Some(bodies[body_idx].id),
                normal: surface_normal,
                depth: result.depth,
                position: self.position,
                remainder,
            });

            // floor_stop_on_slope: a body with no lateral input does not slide
            // down a slope — it stops dead on contact.
            if is_floor && self.floor_stop_on_slope {
                let lateral = self.velocity - up * self.velocity.dot(up);
                if lateral.length() < STOP_ON_SLOPE_LATERAL_EPS {
                    self.velocity = Vector2::ZERO;
                    break;
                }
            }

            // Slide: project velocity (px/s) and motion onto the surface plane.
            let speed_before = self.velocity.length();
            self.velocity = self.velocity - surface_normal * self.velocity.dot(surface_normal);
            let mut new_motion = remainder;

            // floor_constant_speed: keep the speed magnitude across floor slides
            // instead of bleeding speed into the slope normal.
            if is_floor && self.floor_constant_speed && self.velocity.length_squared() > 1e-8 {
                self.velocity = self.velocity.normalized() * speed_before;
            }

            // slide_on_ceiling: when disabled, ceiling contact cancels motion so
            // the body does not slide sideways along the ceiling.
            if self.on_ceiling && !self.slide_on_ceiling {
                self.velocity = Vector2::ZERO;
                new_motion = Vector2::ZERO;
            }

            // floor_block_on_wall: while grounded, a wall may not push the body
            // up (no wall climbing).
            if self.floor_block_on_wall && self.on_wall && (self.on_floor || was_on_floor) {
                let up_vel = self.velocity.dot(up);
                if up_vel > 0.0 {
                    self.velocity = self.velocity - up * up_vel;
                }
                let up_motion = new_motion.dot(up);
                if up_motion > 0.0 {
                    new_motion = new_motion - up * up_motion;
                }
            }

            motion = new_motion;
        }

        // (3) Floor snapping: keep the body glued to a downward slope/step when
        // it was grounded, is not jumping, and a floor is within snap range.
        if !self.on_floor && was_on_floor && !vel_dir_facing_up && self.floor_snap_length > 0.0 {
            self.snap_down_to_floor(up, floor_cos, bodies, &mut floor_platform_vel);
        }

        // (3b) Still no floor contact, but the body was grounded and is not
        // jumping? It is resting on something it never collided with this
        // frame, because it barely moved. Re-acquire that contact so the
        // platform under it is not forgotten (pat-s3rrv.93).
        if !self.on_floor && was_on_floor && !vel_dir_facing_up {
            self.probe_resting_floor(up, floor_cos, bodies, &mut floor_platform_vel);
        }

        // (4) Remember the platform velocity for next frame's carry.
        self.platform_velocity = floor_platform_vel;

        // (5) On-leave: leaving a moving platform this frame adds its velocity
        // (Godot PLATFORM_ON_LEAVE_ADD_VELOCITY).
        if was_on_floor && !self.on_floor && incoming_platform_vel.length_squared() > 1e-8 {
            self.velocity = self.velocity + incoming_platform_vel;
        }

        self.velocity
    }

    /// Re-acquires the floor a body is already RESTING on (pat-s3rrv.93).
    ///
    /// ⚠️ `floor_platform_vel` is otherwise only ever set from inside the slide
    /// loop, i.e. as a side effect of *colliding* during this frame's motion. A
    /// body standing still on a moving platform has (near-)zero motion of its
    /// own, so it collides with nothing, and the platform under it is forgotten
    /// even though contact never broke.
    ///
    /// That is what carried a rider on alternate frames only: the frame after a
    /// successful carry reported `platform_velocity = 0`, which both cleared the
    /// next carry and tripped the idle-body skip in
    /// `PhysicsServer::process_character_movement`, giving 2,0,2,0 instead of a
    /// uniform 1 — exactly half the platform's rate.
    ///
    /// Unlike [`snap_down_to_floor`](Self::snap_down_to_floor) this does NOT
    /// move the body. It is a pure query at the body's current position, so it
    /// is safe to run regardless of `floor_snap_length`, which is 0 by default
    /// and would otherwise leave the common case unfixed.
    fn probe_resting_floor(
        &mut self,
        up: Vector2,
        floor_cos: f32,
        bodies: &[&PhysicsBody2D],
        floor_platform_vel: &mut Vector2,
    ) {
        // A hair of downward motion: resting contact is a touch, not an
        // overlap, and a strictly-zero probe misses a body sitting exactly on
        // the surface.
        let probe = -up * RESTING_FLOOR_PROBE;
        let tf = Transform2D::translated(self.position + probe);
        let mut best: Option<(collision::CollisionResult, usize)> = None;

        for (idx, body) in bodies.iter().enumerate() {
            if (self.collision_mask & body.collision_layer) == 0 {
                continue;
            }
            if let Some(result) = deepest_hit_against(&self.shape, &tf, body, probe) {
                if !result.colliding || result.depth <= 0.0 {
                    continue;
                }
                let surface_normal = -result.normal;
                if surface_normal.dot(up) > floor_cos
                    && (best.is_none() || result.depth > best.as_ref().unwrap().0.depth)
                {
                    best = Some((result, idx));
                }
            }
        }

        if let Some((result, idx)) = best {
            // Contact state only — the position is left exactly where the slide
            // loop put it.
            self.on_floor = true;
            self.floor_normal = -result.normal;
            *floor_platform_vel = bodies[idx].linear_velocity;
        }
    }

    /// Casts the body down by `floor_snap_length` and, if a floor is found,
    /// rests on it and marks the body as on the floor.
    fn snap_down_to_floor(
        &mut self,
        up: Vector2,
        floor_cos: f32,
        bodies: &[&PhysicsBody2D],
        floor_platform_vel: &mut Vector2,
    ) {
        let snap_target = self.position - up * self.floor_snap_length;
        let tf_target = Transform2D::translated(snap_target);
        let mut best: Option<(collision::CollisionResult, usize)> = None;

        for (idx, body) in bodies.iter().enumerate() {
            if (self.collision_mask & body.collision_layer) == 0 {
                continue;
            }
            // The snap probe travels DOWN toward the floor, against a one-way
            // platform's outward normal, so the platform is solid for it — which
            // is what `passthrough_for` already answers. Passing the real snap
            // motion keeps a one-way platform snappable (pat-s3rrv.25).
            if let Some(result) =
                deepest_hit_against(&self.shape, &tf_target, body, -up * self.floor_snap_length)
            {
                if !result.colliding || result.depth <= 0.0 {
                    continue;
                }
                // Only snap to surfaces that classify as floor.
                let surface_normal = -result.normal;
                if surface_normal.dot(up) > floor_cos
                    && (best.is_none() || result.depth > best.as_ref().unwrap().0.depth)
                {
                    best = Some((result, idx));
                }
            }
        }

        if let Some((result, idx)) = best {
            let surface_normal = -result.normal;
            self.position = snap_target + surface_normal * result.depth;
            self.on_floor = true;
            self.floor_normal = surface_normal;
            *floor_platform_vel = bodies[idx].linear_velocity;
        }
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

    /// Seeds the "was on the floor last frame" state.
    ///
    /// When the physics server reconstructs a fresh [`CharacterBody2D`] each
    /// frame, call this before [`CharacterBody2D::move_and_slide_delta`] so that
    /// floor snapping and platform on-leave velocity behave correctly.
    pub fn set_on_floor(&mut self, on_floor: bool) {
        self.on_floor = on_floor;
    }

    /// Classifies a collision normal as floor, wall, or ceiling.
    fn classify_surface(&mut self, normal: Vector2, up: Vector2, floor_cos: f32) {
        // MOTION_MODE_FLOATING privileges no direction, so nothing is floor,
        // wall or ceiling — the body just slides along whatever it touches
        // (pat-0q2kx.113). Returning early here rather than filtering at the
        // call sites keeps every caller (slide loop, snap, wall probe) honest.
        if self.motion_mode == MotionMode::Floating {
            return;
        }
        let dot = normal.dot(up);
        if dot > floor_cos {
            self.on_floor = true;
            self.floor_normal = normal;
        } else if dot < -floor_cos {
            self.on_ceiling = true;
        } else {
            self.on_wall = true;
            self.wall_normal = normal;
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
    pub fn get_floor_normal(&self) -> Vector2 {
        self.floor_normal
    }

    /// Returns the wall normal from the last `move_and_slide` call.
    pub fn get_wall_normal(&self) -> Vector2 {
        self.wall_normal
    }

    /// Moves the character by `motion` and stops at the first collision.
    ///
    /// Unlike `move_and_slide`, this does NOT slide along surfaces. Returns
    /// collision info if a collision occurred, or `None` if the full motion
    /// completed without hitting anything.
    pub fn move_and_collide(
        &mut self,
        motion: Vector2,
        bodies: &[&PhysicsBody2D],
    ) -> Option<KinematicCollision2D> {
        let target = self.position + motion;
        let tf_target = Transform2D::translated(target);

        let mut deepest: Option<(collision::CollisionResult, usize)> = None;

        for (idx, body) in bodies.iter().enumerate() {
            if (self.collision_mask & body.collision_layer) == 0 {
                continue;
            }

            // `move_and_collide` never applied one-way filtering, and zero
            // motion means `passthrough_for` always answers false, so this stays
            // exactly as it was (pat-s3rrv.25).
            if let Some(result) = deepest_hit_against(&self.shape, &tf_target, body, Vector2::ZERO)
            {
                if result.colliding
                    && result.depth > 0.0
                    && (deepest.is_none() || result.depth > deepest.as_ref().unwrap().0.depth)
                {
                    deepest = Some((result, idx));
                }
            }
        }

        match deepest {
            None => {
                // No collision — move full distance.
                self.position = target;
                None
            }
            Some((result, body_idx)) => {
                let surface_normal = -result.normal;
                // Push out of the collision.
                self.position = target + surface_normal * result.depth;
                let up = if self.up_direction.length_squared() > 1e-8 {
                    self.up_direction.normalized()
                } else {
                    Vector2::new(0.0, -1.0)
                };
                self.classify_surface(surface_normal, up, self.floor_max_angle.cos());
                Some(KinematicCollision2D {
                    collider: bodies.get(body_idx).map(|b| b.id),
                    normal: surface_normal,
                    depth: result.depth,
                    position: self.position,
                    remainder: Vector2::ZERO, // no sliding
                })
            }
        }
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
    /// Remaining motion not applied (always zero for move_and_collide).
    pub remainder: Vector2,
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
        assert!(
            (character.position.y - 10.0).abs() < 1e-2,
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
        let ground = make_ground_circle();
        let bodies: Vec<&PhysicsBody2D> = vec![&ground];

        // Sliding disabled: purely-downward velocity on a slope => stop.
        let mut stopping =
            CharacterBody2D::new(Vector2::new(30.0, 13.6), Shape2D::Circle { radius: 1.0 });
        stopping.floor_stop_on_slope = true;
        let v_stop = stopping.move_and_slide_delta(Vector2::new(0.0, 30.0), 1.0, &bodies);
        assert!(stopping.is_on_floor(), "should detect the slope as floor");
        assert!(
            v_stop.length() < 0.5,
            "stop_on_slope should zero velocity, got {:?}",
            v_stop
        );

        // Sliding enabled: the same body slides downhill (gains lateral speed).
        let mut sliding =
            CharacterBody2D::new(Vector2::new(30.0, 13.6), Shape2D::Circle { radius: 1.0 });
        sliding.floor_stop_on_slope = false;
        let v_slide = sliding.move_and_slide_delta(Vector2::new(0.0, 30.0), 1.0, &bodies);
        assert!(
            v_slide.x.abs() > 5.0,
            "without stop_on_slope the body should slide downhill, got {:?}",
            v_slide
        );
    }

    /// floor_constant_speed preserves speed magnitude across a floor slide.
    #[test]
    fn floor_constant_speed_preserves_speed() {
        let ground = make_ground_circle();
        let bodies: Vec<&PhysicsBody2D> = vec![&ground];
        let input = Vector2::new(10.0, 20.0);
        let input_speed = input.length();

        let mut constant =
            CharacterBody2D::new(Vector2::new(30.0, 13.6), Shape2D::Circle { radius: 1.0 });
        constant.floor_constant_speed = true;
        let v_const = constant.move_and_slide_delta(input, 0.5, &bodies);
        assert!(constant.is_on_floor());

        let mut varying =
            CharacterBody2D::new(Vector2::new(30.0, 13.6), Shape2D::Circle { radius: 1.0 });
        varying.floor_constant_speed = false;
        let v_var = varying.move_and_slide_delta(input, 0.5, &bodies);

        assert!(
            (v_const.length() - input_speed).abs() < 0.5,
            "constant_speed should preserve speed ~{}, got {}",
            input_speed,
            v_const.length()
        );
        assert!(
            v_var.length() < input_speed - 1.0,
            "without constant_speed the slope should bleed speed, got {}",
            v_var.length()
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
        assert!(
            v_stop.length() < 1.0,
            "slide_on_ceiling=false should cancel motion, got {:?}",
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
        // up is (0,-1); upward velocity is negative y. Blocked => y clamped to >= 0.
        assert!(
            v_blocked.y >= -1e-2,
            "block_on_wall should stop upward climb, got {:?}",
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

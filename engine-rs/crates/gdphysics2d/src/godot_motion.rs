//! A port of Godot 4.6's `GodotSpace2D::test_body_motion`
//! (`modules/godot_physics_2d/godot_space_2d.cpp`) — the kinematic query under
//! every `move_and_collide` / `move_and_slide`.
//!
//! Three passes, exactly as Godot runs them:
//!
//! 1. **Recovery.** Up to four times, collect every contact pair within the
//!    safe margin and push the body out by 40% of each pair's depth past
//!    `margin * 0.05`. This is why a resting body hovers a hair above the floor
//!    and why a walking one never snags.
//! 2. **Cast.** Per obstacle, an 8-step biased binary search along the motion
//!    for the last fraction that does not touch (`safe`) and the first that
//!    does (`unsafe`), with the margin OFF.
//! 3. **Rest.** At the unsafe fraction, the deepest margin contact is the
//!    collision: its normal, point, depth and the collider's velocity there.
//!
//! The narrow phase is [`crate::godot_sat`], the same SAT Godot uses, because
//! every number here (how many contact pairs, how deep each is) comes out of it.

use gdcore::math::{Rect2, Transform2D, Vector2};

use crate::body::{BodyId, BodyType, PhysicsBody2D};
use crate::godot_sat::{
    normalized, rect_intersects_inclusive, rect_merge, solve, xform_rect, SatShape, CMP_EPSILON,
};

/// `TEST_MOTION_MARGIN_MIN_VALUE`.
const TEST_MOTION_MARGIN_MIN_VALUE: f32 = 0.0001;
/// `TEST_MOTION_MIN_CONTACT_DEPTH_FACTOR`.
const TEST_MOTION_MIN_CONTACT_DEPTH_FACTOR: f32 = 0.05;
/// `physics/2d/solver/contact_max_allowed_penetration` default.
pub const DEFAULT_CONTACT_MAX_ALLOWED_PENETRATION: f32 = 0.3;

/// `PhysicsServer2D::MotionParameters`.
#[derive(Debug, Clone)]
pub struct MotionParameters {
    pub from: Transform2D,
    pub motion: Vector2,
    pub margin: f32,
    pub recovery_as_collision: bool,
    pub collide_separation_ray: bool,
    pub exclude_bodies: Vec<BodyId>,
}

impl MotionParameters {
    pub fn new(from: Transform2D, motion: Vector2, margin: f32) -> Self {
        Self {
            from,
            motion,
            margin,
            recovery_as_collision: false,
            collide_separation_ray: false,
            exclude_bodies: Vec::new(),
        }
    }
}

/// `PhysicsServer2D::MotionResult` (2D carries ONE collision).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MotionResult {
    pub travel: Vector2,
    pub remainder: Vector2,
    pub collision_point: Vector2,
    pub collision_normal: Vector2,
    pub collider_velocity: Vector2,
    pub collision_depth: f32,
    pub collision_safe_fraction: f32,
    pub collision_unsafe_fraction: f32,
    pub collision_local_shape: usize,
    pub collider: Option<BodyId>,
    pub collider_shape: usize,
}

impl MotionResult {
    /// `MotionResult::get_angle` — `Math::acos` clamps, so a dot a hair past 1
    /// is 0 rather than NaN.
    pub fn get_angle(&self, up: Vector2) -> f32 {
        godot_acos(self.collision_normal.dot(up))
    }
}

/// `Math::acos(float)`, which clamps.
pub fn godot_acos(x: f32) -> f32 {
    if x < -1.0 {
        std::f32::consts::PI
    } else if x > 1.0 {
        0.0
    } else {
        x.acos()
    }
}

/// One collider shape of one obstacle, in world space.
#[derive(Debug, Clone)]
struct Candidate {
    body_index: usize,
    shape_index: usize,
    shape: SatShape,
    xform: Transform2D,
    aabb: Rect2,
    one_way: bool,
    one_way_margin: f32,
}

/// The obstacles a moving body can hit, prepared once per `move_and_slide`
/// (Godot's broadphase): every shape of every body whose `collision_layer`
/// meets the mover's mask, as a [`SatShape`] with its world transform and AABB.
pub struct MotionSpace<'a> {
    bodies: &'a [&'a PhysicsBody2D],
    candidates: Vec<Candidate>,
    /// The space's last step (`GodotSpace2D::last_step`), which widens a moving
    /// one-way platform's valid depth by how far it moved.
    pub last_step: f32,
    pub contact_max_allowed_penetration: f32,
}

impl<'a> MotionSpace<'a> {
    pub fn new(bodies: &'a [&'a PhysicsBody2D], collision_mask: u32, last_step: f32) -> Self {
        let mut candidates = Vec::new();
        for (bi, body) in bodies.iter().enumerate() {
            if collision_mask & body.collision_layer == 0 {
                continue;
            }
            for (si, owner) in body.shape_owners().into_iter().enumerate() {
                let Some(shape) = SatShape::from_shape(&owner.shape) else {
                    continue;
                };
                let aabb = xform_rect(&owner.transform, shape.aabb());
                candidates.push(Candidate {
                    body_index: bi,
                    shape_index: si,
                    shape,
                    xform: owner.transform,
                    aabb,
                    one_way: owner.one_way_collision,
                    one_way_margin: owner.one_way_margin,
                });
            }
        }
        Self {
            bodies,
            candidates,
            last_step,
            contact_max_allowed_penetration: DEFAULT_CONTACT_MAX_ALLOWED_PENETRATION,
        }
    }

    /// The obstacle body with `id`, if it is in this space.
    pub fn body(&self, id: BodyId) -> Option<&'a PhysicsBody2D> {
        self.bodies.iter().copied().find(|b| b.id == id)
    }

    fn cull(&self, aabb: Rect2) -> Vec<usize> {
        (0..self.candidates.len())
            .filter(|&i| rect_intersects_inclusive(aabb, self.candidates[i].aabb))
            .collect()
    }

    /// `valid_depth` for a one-way candidate: the user margin (never below
    /// the motion margin), widened for a moving kinematic/rigid platform.
    fn one_way_valid(&self, c: &Candidate, margin: f32) -> (Vector2, f32) {
        let valid_dir = normalized(c.xform.y);
        let mut valid_depth = c.one_way_margin.max(margin);
        let b = self.bodies[c.body_index];
        if matches!(b.body_type, BodyType::Kinematic | BodyType::Rigid) {
            let lv = b.linear_velocity;
            let motion = lv * self.last_step;
            let motion_len = motion.length();
            let motion = normalized(motion);
            valid_depth += motion_len * motion.dot(-valid_dir).max(0.0);
        }
        (valid_dir, valid_depth)
    }
}

/// `GodotPhysicsServer2D::CollCbkData` + `_shape_col_cbk`.
struct CollCbk {
    max: usize,
    pairs: Vec<(Vector2, Vector2)>,
    passed: usize,
    valid_dir: Vector2,
    valid_depth: f32,
    invalid_by_dir: usize,
}

impl CollCbk {
    fn new(max: usize) -> Self {
        Self {
            max,
            pairs: Vec::with_capacity(max),
            passed: 0,
            valid_dir: Vector2::ZERO,
            valid_depth: 0.0,
            invalid_by_dir: 0,
        }
    }

    fn add(&mut self, a: Vector2, b: Vector2) {
        if self.max == 0 {
            return;
        }
        let rel_dir = a - b;
        let rel_length2 = rel_dir.length_squared();
        if self.valid_dir != Vector2::ZERO {
            if self.valid_depth < 10e20 {
                if rel_length2 > self.valid_depth * self.valid_depth
                    || (rel_length2 > CMP_EPSILON
                        && self.valid_dir.dot(normalized(rel_dir)) < CMP_EPSILON)
                {
                    self.invalid_by_dir += 1;
                    return;
                }
            } else if rel_length2 > 0.0 && self.valid_dir.dot(normalized(rel_dir)) < CMP_EPSILON {
                return;
            }
        }
        if self.pairs.len() == self.max {
            let mut min_depth = 1e20f32;
            let mut min_idx = 0;
            for (i, (pa, pb)) in self.pairs.iter().enumerate() {
                let d = (*pa - *pb).length_squared();
                if d < min_depth {
                    min_depth = d;
                    min_idx = i;
                }
            }
            if rel_length2 < min_depth {
                return;
            }
            self.pairs[min_idx] = (a, b);
            self.passed += 1;
        } else {
            self.pairs.push((a, b));
            self.passed += 1;
        }
    }
}

/// `_RestCallbackData2D` + `_rest_cbk_result`.
#[derive(Default)]
struct RestCbk {
    object: usize,
    shape: usize,
    local_shape: usize,
    best_object: Option<usize>,
    best_shape: usize,
    best_local_shape: usize,
    best_contact: Vector2,
    best_normal: Vector2,
    best_len: f32,
    valid_dir: Vector2,
    valid_depth: f32,
    min_allowed_depth: f32,
}

impl RestCbk {
    fn add(&mut self, a: Vector2, b: Vector2) {
        let contact_rel = b - a;
        let len = contact_rel.length();
        if len < self.min_allowed_depth {
            return;
        }
        if len <= self.best_len {
            return;
        }
        let normal = contact_rel / len;
        if self.valid_dir != Vector2::ZERO {
            if len > self.valid_depth {
                return;
            }
            if self.valid_dir.dot(normal) > -CMP_EPSILON {
                return;
            }
        }
        self.best_len = len;
        self.best_contact = b;
        self.best_normal = normal;
        self.best_object = Some(self.object);
        self.best_shape = self.shape;
        self.best_local_shape = self.local_shape;
    }
}

/// A moving body's own shapes, each with its transform LOCAL to the body.
pub struct MovingShapes<'a> {
    pub shapes: &'a [(SatShape, Transform2D)],
}

/// `GodotSpace2D::test_body_motion`. Returns whether the motion collided and
/// the motion result.
pub fn test_body_motion(
    space: &MotionSpace,
    body: &MovingShapes,
    p: &MotionParameters,
) -> (bool, MotionResult) {
    let mut result = MotionResult::default();

    let mut body_aabb: Option<Rect2> = None;
    for (shape, local) in body.shapes {
        let a = xform_rect(&(p.from * *local), shape.aabb());
        body_aabb = Some(match body_aabb {
            None => a,
            Some(b) => rect_merge(b, a),
        });
    }
    let Some(body_aabb) = body_aabb else {
        result.travel = p.motion;
        return (false, result);
    };

    let margin = p.margin.max(TEST_MOTION_MARGIN_MIN_VALUE);
    let mut body_aabb = body_aabb.grow(margin);

    // (local shape index, candidate index)
    let mut excluded: Vec<(usize, usize)> = Vec::new();
    let min_contact_depth = margin * TEST_MOTION_MIN_CONTACT_DEPTH_FACTOR;
    let motion_length = p.motion.length();
    let motion_normal = p.motion / motion_length;
    let mut body_transform = p.from;
    let mut recovered = false;

    let is_excluded_body =
        |c: &Candidate| p.exclude_bodies.contains(&space.bodies[c.body_index].id);

    // STEP 1: free the body if stuck.
    {
        let mut recover_attempts = 4;
        loop {
            let mut cbk = CollCbk::new(32);
            excluded.clear();
            let mut collided = false;
            let amount = space.cull(body_aabb);
            for (j, (body_shape, local)) in body.shapes.iter().enumerate() {
                let body_shape_xform = body_transform * *local;
                for &ci in &amount {
                    let c = &space.candidates[ci];
                    if is_excluded_body(c) {
                        continue;
                    }
                    if body_shape.allows_one_way_collision() && c.one_way {
                        let (dir, depth) = space.one_way_valid(c, margin);
                        cbk.valid_dir = dir;
                        cbk.valid_depth = depth;
                        cbk.invalid_by_dir = 0;
                    } else {
                        cbk.valid_dir = Vector2::ZERO;
                        cbk.valid_depth = 0.0;
                        cbk.invalid_by_dir = 0;
                    }
                    let current_passed = cbk.passed;
                    let mut did_collide = false;
                    let hit = {
                        let mut f = |a: Vector2, b: Vector2| cbk.add(a, b);
                        solve(
                            body_shape,
                            &body_shape_xform,
                            Vector2::ZERO,
                            &c.shape,
                            &c.xform,
                            Vector2::ZERO,
                            Some(&mut f),
                            None,
                            margin,
                            0.0,
                        )
                    };
                    if hit {
                        did_collide = cbk.passed > current_passed;
                    }
                    if !did_collide && cbk.invalid_by_dir > 0 && excluded.len() < 32 {
                        excluded.push((j, ci));
                    }
                    if did_collide {
                        collided = true;
                    }
                }
            }
            if !collided {
                break;
            }
            recovered = true;
            let mut recover_motion = Vector2::ZERO;
            for (a, b) in &cbk.pairs {
                let n = normalized(*a - *b);
                let d = n.dot(*b);
                let depth = n.dot(*a + recover_motion) - d;
                if depth > min_contact_depth + CMP_EPSILON {
                    // Every collision priority is 1, so the weight is 1.
                    recover_motion = recover_motion - n * ((depth - min_contact_depth) * 0.4);
                }
            }
            if recover_motion == Vector2::ZERO {
                break;
            }
            body_transform.origin = body_transform.origin + recover_motion;
            body_aabb.position = body_aabb.position + recover_motion;
            recover_attempts -= 1;
            if recover_attempts == 0 {
                break;
            }
        }
    }

    let mut safe = 1.0f32;
    let mut unsafe_ = 1.0f32;
    let mut best_shape: Option<usize> = None;

    // STEP 2: attempt the motion.
    {
        let mut motion_aabb = body_aabb;
        motion_aabb.position = motion_aabb.position + p.motion;
        let motion_aabb = rect_merge(motion_aabb, body_aabb);
        let amount = space.cull(motion_aabb);
        for (body_shape_idx, (body_shape, local)) in body.shapes.iter().enumerate() {
            if !p.collide_separation_ray {
                if let SatShape::SeparationRay { slide_on_slope, .. } = body_shape {
                    if !slide_on_slope {
                        continue;
                    }
                }
            }
            let body_shape_xform = body_transform * *local;
            let mut stuck = false;
            let mut best_safe = 1.0f32;
            let mut best_unsafe = 1.0f32;
            for &ci in &amount {
                let c = &space.candidates[ci];
                if is_excluded_body(c) {
                    continue;
                }
                if excluded.contains(&(body_shape_idx, ci)) {
                    continue;
                }
                if !solve(
                    body_shape,
                    &body_shape_xform,
                    p.motion,
                    &c.shape,
                    &c.xform,
                    Vector2::ZERO,
                    None,
                    None,
                    0.0,
                    0.0,
                ) {
                    continue;
                }
                if solve(
                    body_shape,
                    &body_shape_xform,
                    Vector2::ZERO,
                    &c.shape,
                    &c.xform,
                    Vector2::ZERO,
                    None,
                    None,
                    0.0,
                    0.0,
                ) {
                    if body_shape.allows_one_way_collision() && c.one_way {
                        let direction = normalized(c.xform.y);
                        if motion_normal.dot(direction) < 0.0 {
                            continue;
                        }
                    }
                    stuck = true;
                    break;
                }
                let mut low = 0.0f32;
                let mut hi = 1.0f32;
                let mut fraction_coeff = 0.5f32;
                for k in 0..8 {
                    let fraction = low + (hi - low) * fraction_coeff;
                    let mut sep = motion_normal;
                    let collided = solve(
                        body_shape,
                        &body_shape_xform,
                        p.motion * fraction,
                        &c.shape,
                        &c.xform,
                        Vector2::ZERO,
                        None,
                        Some(&mut sep),
                        0.0,
                        0.0,
                    );
                    if collided {
                        hi = fraction;
                        fraction_coeff = if k == 0 || low > 0.0 { 0.5 } else { 0.25 };
                    } else {
                        low = fraction;
                        fraction_coeff = if k == 0 || hi < 1.0 { 0.5 } else { 0.75 };
                    }
                }
                if body_shape.allows_one_way_collision() && c.one_way {
                    let mut cbk = CollCbk::new(1);
                    cbk.valid_dir = normalized(c.xform.y);
                    cbk.valid_depth = 10e20;
                    let mut sep = motion_normal;
                    let collided = {
                        let mut f = |a: Vector2, b: Vector2| cbk.add(a, b);
                        solve(
                            body_shape,
                            &body_shape_xform,
                            p.motion * (hi + space.contact_max_allowed_penetration),
                            &c.shape,
                            &c.xform,
                            Vector2::ZERO,
                            Some(&mut f),
                            Some(&mut sep),
                            0.0,
                            0.0,
                        )
                    };
                    if !collided || cbk.pairs.is_empty() {
                        continue;
                    }
                }
                if low < best_safe {
                    best_safe = low;
                    best_unsafe = hi;
                }
            }
            if stuck {
                safe = 0.0;
                unsafe_ = 0.0;
                best_shape = Some(body_shape_idx);
                break;
            }
            if best_safe == 1.0 {
                continue;
            }
            if best_safe < safe {
                safe = best_safe;
                unsafe_ = best_unsafe;
                best_shape = Some(body_shape_idx);
            }
        }
    }

    let mut collided = false;
    if (p.recovery_as_collision && recovered) || safe < 1.0 {
        if safe >= 1.0 {
            best_shape = None;
        }
        let mut ugt = body_transform;
        ugt.origin = ugt.origin + p.motion * unsafe_;
        let mut rcd = RestCbk {
            min_allowed_depth: motion_length.min(min_contact_depth),
            ..Default::default()
        };
        body_aabb.position = body_aabb.position + p.motion * unsafe_;
        let amount = space.cull(body_aabb);
        let (from_shape, to_shape) = match best_shape {
            Some(s) => (s, s + 1),
            None => (0, body.shapes.len()),
        };
        for j in from_shape..to_shape {
            let (body_shape, local) = &body.shapes[j];
            let body_shape_xform = ugt * *local;
            for &ci in &amount {
                let c = &space.candidates[ci];
                if is_excluded_body(c) {
                    continue;
                }
                if excluded.contains(&(j, ci)) {
                    continue;
                }
                if body_shape.allows_one_way_collision() && c.one_way {
                    let (dir, depth) = space.one_way_valid(c, margin);
                    rcd.valid_dir = dir;
                    rcd.valid_depth = depth;
                } else {
                    rcd.valid_dir = Vector2::ZERO;
                    rcd.valid_depth = 0.0;
                }
                rcd.object = ci;
                rcd.shape = c.shape_index;
                rcd.local_shape = j;
                let mut f = |a: Vector2, b: Vector2| rcd.add(a, b);
                solve(
                    body_shape,
                    &body_shape_xform,
                    Vector2::ZERO,
                    &c.shape,
                    &c.xform,
                    Vector2::ZERO,
                    Some(&mut f),
                    None,
                    margin,
                    0.0,
                );
            }
        }
        if rcd.best_len != 0.0 {
            if let Some(ci) = rcd.best_object {
                let c = &space.candidates[ci];
                let b = space.bodies[c.body_index];
                result.collider = Some(b.id);
                result.collider_shape = rcd.best_shape;
                result.collision_local_shape = rcd.best_local_shape;
                result.collision_normal = rcd.best_normal;
                result.collision_point = rcd.best_contact;
                result.collision_depth = rcd.best_len;
                result.collision_safe_fraction = safe;
                result.collision_unsafe_fraction = unsafe_;
                // `get_center_of_mass()` is the global-space offset of the
                // centre of mass from the body origin.
                let rel_vec = result.collision_point - (b.position + b.center_of_mass);
                let av = b.angular_velocity;
                result.collider_velocity =
                    Vector2::new(-av * rel_vec.y, av * rel_vec.x) + b.linear_velocity;
                result.travel = p.motion * safe;
                result.remainder = p.motion - p.motion * safe;
                result.travel = result.travel + (body_transform.origin - p.from.origin);
                collided = true;
            }
        }
    }

    if !collided {
        result.travel = p.motion;
        result.remainder = Vector2::ZERO;
        result.travel = result.travel + (body_transform.origin - p.from.origin);
    }
    (collided, result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::Shape2D;

    fn floor() -> PhysicsBody2D {
        PhysicsBody2D::new(
            BodyId(1),
            BodyType::Static,
            Vector2::new(0.0, 100.0),
            Shape2D::Rectangle {
                half_extents: Vector2::new(500.0, 50.0),
            },
            0.0,
        )
    }

    /// A box falling onto a floor stops above it, within Godot's binary-search
    /// resolution of the surface, and reports the floor's up normal.
    #[test]
    fn falling_box_stops_on_the_floor() {
        let f = floor();
        let bodies = [&f];
        let space = MotionSpace::new(&bodies, 1, 1.0 / 60.0);
        let shapes = [(
            SatShape::Rectangle {
                half_extents: Vector2::new(10.0, 10.0),
            },
            Transform2D::IDENTITY,
        )];
        let mover = MovingShapes { shapes: &shapes };
        let p = MotionParameters::new(
            Transform2D::translated(Vector2::new(0.0, 35.0)),
            Vector2::new(0.0, 10.0),
            0.08,
        );
        let (hit, r) = test_body_motion(&space, &mover, &p);
        assert!(hit);
        let end = 35.0 + r.travel.y;
        assert!(end <= 40.0 && end > 39.95, "rested at {end}");
        assert!((r.collision_normal.y + 1.0).abs() < 1e-5, "{r:?}");
        assert_eq!(r.collider, Some(BodyId(1)));
    }

    #[test]
    fn free_motion_travels_fully() {
        let f = floor();
        let bodies = [&f];
        let space = MotionSpace::new(&bodies, 1, 1.0 / 60.0);
        let shapes = [(SatShape::Circle { radius: 5.0 }, Transform2D::IDENTITY)];
        let mover = MovingShapes { shapes: &shapes };
        let p = MotionParameters::new(
            Transform2D::translated(Vector2::new(0.0, 0.0)),
            Vector2::new(3.0, 4.0),
            0.08,
        );
        let (hit, r) = test_body_motion(&space, &mover, &p);
        assert!(!hit);
        assert_eq!(r.travel, Vector2::new(3.0, 4.0));
    }
}

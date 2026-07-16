//! Physics world state and simulation stepping.
//!
//! The [`PhysicsWorld2D`] owns all physics bodies and runs the simulation loop:
//! integration, broad-phase culling, narrow-phase collision detection, and
//! overlap resolution.

use std::collections::{HashMap, HashSet};

use gdcore::math::{Transform2D, Vector2};

use crate::body::{BodyId, PhysicsBody2D};
use crate::collision;
use crate::joint::{self, Joint2D, JointId};
use crate::shape::Shape2D;

/// Result of a raycast query.
#[derive(Debug, Clone, Copy)]
pub struct RaycastHit {
    /// The body that was hit.
    pub body_id: BodyId,
    /// The hit point in world space.
    pub point: Vector2,
    /// The surface normal at the hit point.
    pub normal: Vector2,
    /// Distance from the ray origin to the hit point.
    pub distance: f32,
}

/// A single overlap reported by [`PhysicsWorld2D::shape_cast`] (Godot
/// `ShapeCast2D`).
#[derive(Debug, Clone, Copy)]
pub struct ShapeCastHit {
    /// The body the cast shape overlaps.
    pub body_id: BodyId,
    /// A contact point on the overlap, in world space.
    pub point: Vector2,
    /// The contact normal, pointing out of the hit body.
    pub normal: Vector2,
    /// Penetration depth of the overlap.
    pub depth: f32,
}

/// Whether a contact was just entered or just exited this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactState {
    /// The contact started this frame.
    Entered,
    /// The contact persisted from the previous frame.
    Persisting,
    /// The contact ended this frame.
    Exited,
}

/// A collision event generated during a physics step.
#[derive(Debug, Clone, Copy)]
pub struct CollisionEvent {
    /// First body in the collision pair.
    pub body_a: BodyId,
    /// Second body in the collision pair.
    pub body_b: BodyId,
    /// The contact point in world space.
    pub contact_point: Vector2,
    /// The collision normal (from A toward B).
    pub normal: Vector2,
    /// Whether this contact just entered, is persisting, or just exited.
    pub state: ContactState,
}

/// A 2D physics world that owns bodies and steps the simulation.
pub struct PhysicsWorld2D {
    bodies: HashMap<BodyId, PhysicsBody2D>,
    joints: HashMap<JointId, Joint2D>,
    next_id: u64,
    next_joint_id: u64,
    /// Number of constraint solver iterations per step.
    constraint_iterations: usize,
    /// Contacts from the previous frame, keyed by (min_id, max_id).
    previous_contacts: HashSet<(BodyId, BodyId)>,
    /// Global gravity vector (px/s²) applied to dynamic bodies each step.
    gravity: Vector2,
}

/// Godot's default 2D gravity vector: `physics/2d/default_gravity` (980.0)
/// multiplied by `physics/2d/default_gravity_vector` ((0, 1), i.e. down).
pub const DEFAULT_GRAVITY_2D: Vector2 = Vector2::new(0.0, 980.0);

impl PhysicsWorld2D {
    /// Creates an empty physics world.
    pub fn new() -> Self {
        Self {
            bodies: HashMap::new(),
            joints: HashMap::new(),
            next_id: 1,
            next_joint_id: 1,
            constraint_iterations: 10,
            previous_contacts: HashSet::new(),
            gravity: DEFAULT_GRAVITY_2D,
        }
    }

    /// Sets the global gravity vector (px/s²) applied to dynamic bodies.
    ///
    /// Defaults to [`DEFAULT_GRAVITY_2D`] (`(0, 980)`), matching Godot's
    /// `physics/2d/default_gravity` × `default_gravity_vector` project settings.
    pub fn set_gravity(&mut self, gravity: Vector2) {
        self.gravity = gravity;
    }

    /// Returns the global gravity vector (px/s²).
    pub fn gravity(&self) -> Vector2 {
        self.gravity
    }

    /// Sets the number of constraint solver iterations per step.
    ///
    /// More iterations produce more accurate joint constraint solving at the
    /// cost of performance. The default is 10.
    pub fn set_constraint_iterations(&mut self, count: usize) {
        self.constraint_iterations = count;
    }

    /// Adds a body to the world and returns its unique ID.
    pub fn add_body(&mut self, mut body: PhysicsBody2D) -> BodyId {
        let id = BodyId(self.next_id);
        self.next_id += 1;
        body.id = id;
        self.bodies.insert(id, body);
        id
    }

    /// Removes a body from the world by ID.
    pub fn remove_body(&mut self, id: BodyId) -> Option<PhysicsBody2D> {
        self.bodies.remove(&id)
    }

    /// Returns a reference to a body by ID.
    pub fn get_body(&self, id: BodyId) -> Option<&PhysicsBody2D> {
        self.bodies.get(&id)
    }

    /// Returns a mutable reference to a body by ID.
    pub fn get_body_mut(&mut self, id: BodyId) -> Option<&mut PhysicsBody2D> {
        self.bodies.get_mut(&id)
    }

    /// Returns the number of bodies in the world.
    pub fn body_count(&self) -> usize {
        self.bodies.len()
    }

    /// Adds a joint to the world and returns its unique ID.
    pub fn add_joint(&mut self, joint: Joint2D) -> JointId {
        let id = JointId(self.next_joint_id);
        self.next_joint_id += 1;
        self.joints.insert(id, joint);
        id
    }

    /// Removes a joint from the world by ID.
    pub fn remove_joint(&mut self, id: JointId) -> Option<Joint2D> {
        self.joints.remove(&id)
    }

    /// Returns a reference to a joint by ID.
    pub fn get_joint(&self, id: JointId) -> Option<&Joint2D> {
        self.joints.get(&id)
    }

    /// Returns a mutable reference to a joint by ID.
    pub fn get_joint_mut(&mut self, id: JointId) -> Option<&mut Joint2D> {
        self.joints.get_mut(&id)
    }

    /// Returns the number of joints in the world.
    pub fn joint_count(&self) -> usize {
        self.joints.len()
    }

    /// Steps the physics simulation by `dt` seconds.
    ///
    /// 1. Integrate all rigid/kinematic bodies.
    /// 2. Detect collisions between all body pairs (respecting layers/masks).
    /// 3. Resolve overlaps and apply impulses.
    ///
    /// Returns collision events for this frame (entered, persisting, exited).
    pub fn step(&mut self, dt: f32) -> Vec<CollisionEvent> {
        self.step_inner(dt, None, None)
    }

    /// Steps the simulation applying per-body area gravity/damp overrides.
    ///
    /// Behaves exactly like [`step`](Self::step) except that, during the
    /// integrate phase, a body whose [`BodyId`] appears in `gravity_overrides`
    /// is integrated with that gravity vector instead of the world's global
    /// gravity, and a body appearing in `damp_overrides` is integrated with that
    /// linear-damp coefficient instead of its own. Bodies absent from a map use
    /// the usual world gravity / own damping.
    ///
    /// This is how `Area2D` gravity/damp space overrides are applied: an owner
    /// (e.g. the scene `PhysicsServer`, which holds the `AreaStore` alongside
    /// the bodies) computes the effective per-body gravity/damp via
    /// [`AreaStore::compute_body_gravity`](crate::area2d::AreaStore::compute_body_gravity)
    /// and [`compute_body_damp`](crate::area2d::AreaStore::compute_body_damp),
    /// then drives the world through this method.
    pub fn step_with_area_overrides(
        &mut self,
        dt: f32,
        gravity_overrides: &HashMap<BodyId, Vector2>,
        damp_overrides: &HashMap<BodyId, f32>,
    ) -> Vec<CollisionEvent> {
        self.step_inner(dt, Some(gravity_overrides), Some(damp_overrides))
    }

    /// Shared implementation of [`step`](Self::step) and
    /// [`step_with_area_overrides`](Self::step_with_area_overrides).
    fn step_inner(
        &mut self,
        dt: f32,
        gravity_overrides: Option<&HashMap<BodyId, Vector2>>,
        damp_overrides: Option<&HashMap<BodyId, f32>>,
    ) -> Vec<CollisionEvent> {
        // Phase 1: Integrate (apply world gravity + per-body damping, replacing
        // either with a per-body area override when one is supplied).
        let gravity = self.gravity;
        for body in self.bodies.values_mut() {
            let g = gravity_overrides
                .and_then(|m| m.get(&body.id).copied())
                .unwrap_or(gravity);
            match damp_overrides.and_then(|m| m.get(&body.id).copied()) {
                Some(d) => {
                    // Temporarily swap in the effective damp so `integrate`
                    // (which reads `body.linear_damp`) uses it, then restore.
                    let saved = body.linear_damp;
                    body.linear_damp = d;
                    body.integrate(dt, g);
                    body.linear_damp = saved;
                }
                None => body.integrate(dt, g),
            }
        }

        // Phase 1.5: Apply joint constraints (iterated for convergence)
        let joint_ids: Vec<JointId> = self.joints.keys().copied().collect();
        for _iter in 0..self.constraint_iterations {
            for &jid in &joint_ids {
                let jt = &self.joints[&jid];
                let body_a_id = jt.base().body_a;
                let body_b_id = jt.base().body_b;
                if body_a_id == body_b_id {
                    continue;
                }
                if !self.bodies.contains_key(&body_a_id) || !self.bodies.contains_key(&body_b_id) {
                    continue;
                }
                // SAFETY: body_a_id != body_b_id, so these are disjoint map entries.
                let ptr = &mut self.bodies as *mut HashMap<BodyId, PhysicsBody2D>;
                let ba = unsafe { &mut *ptr }.get_mut(&body_a_id).unwrap();
                let bb = unsafe { &mut *ptr }.get_mut(&body_b_id).unwrap();
                joint::apply_joint_constraints(jt, ba, bb, dt);
            }
        }

        // Build set of jointed body pairs to skip collision between them
        let mut jointed_pairs: HashSet<(BodyId, BodyId)> = HashSet::new();
        for jt in self.joints.values() {
            if jt.is_enabled() {
                let a = jt.base().body_a;
                let b = jt.base().body_b;
                let pair = if a.0 < b.0 { (a, b) } else { (b, a) };
                jointed_pairs.insert(pair);
            }
        }

        // Phase 2 & 3: Collision detection and resolution
        let ids: Vec<BodyId> = self.bodies.keys().copied().collect();
        let mut current_contacts = HashSet::new();
        let mut events = Vec::new();

        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                let id_a = ids[i];
                let id_b = ids[j];

                // Skip collision between jointed body pairs
                let pair_key = if id_a.0 < id_b.0 {
                    (id_a, id_b)
                } else {
                    (id_b, id_a)
                };
                if jointed_pairs.contains(&pair_key) {
                    continue;
                }

                // Read shapes, positions, layers, and one-way info
                let (
                    shape_a,
                    pos_a,
                    rot_a,
                    shape_b,
                    pos_b,
                    rot_b,
                    layer_a,
                    mask_a,
                    layer_b,
                    mask_b,
                    ow_a,
                    ow_dir_a,
                    ow_b,
                    ow_dir_b,
                    vel_a,
                    vel_b,
                ) = {
                    let a = &self.bodies[&id_a];
                    let b = &self.bodies[&id_b];
                    (
                        a.shape,
                        a.position,
                        a.rotation,
                        b.shape,
                        b.position,
                        b.rotation,
                        a.collision_layer,
                        a.collision_mask,
                        b.collision_layer,
                        b.collision_mask,
                        a.one_way_collision,
                        a.one_way_direction,
                        b.one_way_collision,
                        b.one_way_direction,
                        a.linear_velocity,
                        b.linear_velocity,
                    )
                };

                // Collision layer/mask filtering
                if (layer_a & mask_b) == 0 && (layer_b & mask_a) == 0 {
                    continue;
                }

                // Build rotation-aware world transforms so the narrow-phase
                // accounts for each body's orientation (SAT in world space).
                let tf_a = body_transform(pos_a, rot_a);
                let tf_b = body_transform(pos_b, rot_b);

                if let Some(result) = collision::test_collision(&shape_a, &tf_a, &shape_b, &tf_b) {
                    if result.colliding && result.depth > 0.0 {
                        // One-way collision check
                        let approach_dir = vel_b - vel_a;
                        if ow_a && approach_dir.dot(ow_dir_a) >= 0.0 {
                            continue; // Body B is not approaching from the correct side of A
                        }
                        if ow_b && (-approach_dir).dot(ow_dir_b) >= 0.0 {
                            continue; // Body A is not approaching from the correct side of B
                        }

                        let pair = if id_a.0 < id_b.0 {
                            (id_a, id_b)
                        } else {
                            (id_b, id_a)
                        };
                        current_contacts.insert(pair);

                        let state = if self.previous_contacts.contains(&pair) {
                            ContactState::Persisting
                        } else {
                            ContactState::Entered
                        };

                        events.push(CollisionEvent {
                            body_a: id_a,
                            body_b: id_b,
                            contact_point: result.point,
                            normal: result.normal,
                            state,
                        });

                        // SAFETY: id_a != id_b because i != j and IDs are unique.
                        let ptr = &mut self.bodies as *mut HashMap<BodyId, PhysicsBody2D>;
                        // SAFETY: We guarantee id_a != id_b, so these are disjoint entries.
                        let body_a = unsafe { &mut *ptr }.get_mut(&id_a).unwrap();
                        let body_b = unsafe { &mut *ptr }.get_mut(&id_b).unwrap();
                        collision::separate_bodies(body_a, body_b, &result);
                    }
                }
            }
        }

        // Detect exited contacts
        for &pair in &self.previous_contacts {
            if !current_contacts.contains(&pair) {
                events.push(CollisionEvent {
                    body_a: pair.0,
                    body_b: pair.1,
                    contact_point: Vector2::ZERO,
                    normal: Vector2::ZERO,
                    state: ContactState::Exited,
                });
            }
        }

        self.previous_contacts = current_contacts;
        events
    }

    /// Casts a ray and returns the closest hit, if any.
    ///
    /// Tests against all bodies' shapes using a simple ray-shape intersection.
    pub fn raycast(
        &self,
        origin: Vector2,
        direction: Vector2,
        max_distance: f32,
    ) -> Option<RaycastHit> {
        let dir = direction.normalized();
        if dir.length_squared() < 1e-10 {
            return None;
        }

        let mut closest: Option<RaycastHit> = None;

        for body in self.bodies.values() {
            if let Some(hit) = raycast_shape(origin, dir, max_distance, body) {
                if closest.is_none() || hit.distance < closest.as_ref().unwrap().distance {
                    closest = Some(hit);
                }
            }
        }

        closest
    }

    /// Casts a ray like [`raycast`](Self::raycast) but honoring a Godot
    /// `RayCast2D`-style filter: bodies whose id is in `exclude` are skipped
    /// (e.g. `add_exception` / `exclude_parent`), and a body is only hit when
    /// its `collision_layer` intersects `collision_mask`. Passing
    /// `collision_mask == 0` disables the mask so every layer is eligible
    /// (matching the unfiltered [`raycast`](Self::raycast)). Returns the closest
    /// surviving hit.
    pub fn raycast_filtered(
        &self,
        origin: Vector2,
        direction: Vector2,
        max_distance: f32,
        exclude: &[BodyId],
        collision_mask: u32,
    ) -> Option<RaycastHit> {
        let dir = direction.normalized();
        if dir.length_squared() < 1e-10 {
            return None;
        }

        let mut closest: Option<RaycastHit> = None;
        for (&id, body) in &self.bodies {
            if exclude.contains(&id) {
                continue;
            }
            if collision_mask != 0 && (body.collision_layer & collision_mask) == 0 {
                continue;
            }
            if let Some(hit) = raycast_shape(origin, dir, max_distance, body) {
                if closest.is_none() || hit.distance < closest.as_ref().unwrap().distance {
                    closest = Some(hit);
                }
            }
        }
        closest
    }

    /// Returns the [`BodyId`]s of all bodies whose shape contains `world_point`,
    /// for input picking (Godot `CollisionObject2D` picking / `_intersect_point`).
    ///
    /// The world point is transformed into each body's LOCAL space — using the
    /// same rotation-aware [`body_transform`] convention the narrow-phase uses to
    /// place a body's shape in the world (see [`step_inner`](Self::step_inner)) —
    /// before the shape's [`Shape2D::contains_point`] test, which expects a
    /// shape-local point.
    ///
    /// Results are sorted deterministically by ascending [`BodyId`] so
    /// callers/tests are stable; a scene layer can re-sort by draw/z order.
    ///
    /// Every body is considered pickable: [`PhysicsBody2D`] has no
    /// pickable/monitorable flag, so none is respected here.
    pub fn point_query(&self, world_point: Vector2) -> Vec<BodyId> {
        let mut hits: Vec<BodyId> = self
            .bodies
            .iter()
            .filter_map(|(&id, body)| {
                // World -> local: invert the body's world transform, mirroring
                // the `body_transform(pos, rot)` the narrow-phase builds to place
                // this body's shape in the world.
                let local = body_transform(body.position, body.rotation)
                    .affine_inverse()
                    .xform(world_point);
                body.shape.contains_point(local).then_some(id)
            })
            .collect();
        // `BodyId` is not `Ord`; sort by its inner id for a deterministic order.
        hits.sort_by_key(|id| id.0);
        hits
    }

    /// Returns the smallest [`BodyId`] whose shape contains `world_point`, or
    /// `None` if the point hits no body. Convenience over [`point_query`].
    ///
    /// [`point_query`]: Self::point_query
    pub fn point_query_first(&self, world_point: Vector2) -> Option<BodyId> {
        self.point_query(world_point).into_iter().next()
    }

    /// Tests `shape` placed at `transform` against every body and returns all
    /// overlaps (Godot `ShapeCast2D`). Bodies in `exclude` are skipped and
    /// `collision_mask` filters by layer (`0` = every layer). Results are sorted
    /// by ascending penetration `depth`, so `hits[0]` is the shallowest contact
    /// and `hits.len()` is the collision count.
    ///
    /// This is a positional overlap query (the cast shape resolved at its target
    /// transform); a full swept sub-step search can layer on top later.
    pub fn shape_cast(
        &self,
        shape: &Shape2D,
        transform: Transform2D,
        exclude: &[BodyId],
        collision_mask: u32,
    ) -> Vec<ShapeCastHit> {
        let mut hits: Vec<ShapeCastHit> = Vec::new();
        for (&id, body) in &self.bodies {
            if exclude.contains(&id) {
                continue;
            }
            if collision_mask != 0 && (body.collision_layer & collision_mask) == 0 {
                continue;
            }
            let body_tf = body_transform(body.position, body.rotation);
            if let Some(result) = collision::test_collision(shape, &transform, &body.shape, &body_tf)
            {
                if result.colliding {
                    hits.push(ShapeCastHit {
                        body_id: id,
                        point: result.point,
                        normal: result.normal,
                        depth: result.depth,
                    });
                }
            }
        }
        hits.sort_by(|a, b| {
            a.depth
                .partial_cmp(&b.depth)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits
    }
}

impl Default for PhysicsWorld2D {
    fn default() -> Self {
        Self::new()
    }
}

/// Builds a body's world transform from its position and rotation (radians).
///
/// The basis carries the rotation so the narrow-phase can transform local
/// shape geometry into world space, making collisions orientation-aware.
fn body_transform(position: Vector2, rotation: f32) -> Transform2D {
    let mut tf = Transform2D::rotated(rotation);
    tf.origin = position;
    tf
}

/// Ray-shape intersection test for a single body.
fn raycast_shape(
    origin: Vector2,
    dir: Vector2,
    max_distance: f32,
    body: &PhysicsBody2D,
) -> Option<RaycastHit> {
    match body.shape {
        Shape2D::Circle { radius } => {
            ray_circle(origin, dir, max_distance, body.position, radius, body.id)
        }
        Shape2D::Rectangle { half_extents } => ray_aabb(
            origin,
            dir,
            max_distance,
            body.position,
            half_extents,
            body.id,
        ),
        Shape2D::Capsule {
            radius,
            height,
        } => ray_capsule(
            origin,
            dir,
            max_distance,
            body.position,
            radius,
            height,
            body.id,
        ),
        Shape2D::Segment { a, b } => {
            // Segment as thin capsule between endpoints (relative to body origin).
            ray_segment(origin, dir, max_distance, body.position + a, body.position + b, body.id)
        }
        _ => None,
    }
}

/// Ray vs vertical capsule (Godot CapsuleShape2D: height along Y, hemispheres).
fn ray_capsule(
    origin: Vector2,
    dir: Vector2,
    max_dist: f32,
    center: Vector2,
    radius: f32,
    height: f32,
    body_id: BodyId,
) -> Option<RaycastHit> {
    // Approximate capsule as two circles + AABB slab; take nearest hit.
    let half = (height * 0.5 - radius).max(0.0);
    let c1 = Vector2::new(center.x, center.y - half);
    let c2 = Vector2::new(center.x, center.y + half);
    let mut best: Option<RaycastHit> = None;
    for c in [c1, c2] {
        if let Some(h) = ray_circle(origin, dir, max_dist, c, radius, body_id) {
            if best.as_ref().map(|b| h.distance < b.distance).unwrap_or(true) {
                best = Some(h);
            }
        }
    }
    // Cylinder body as AABB
    let half_ext = Vector2::new(radius, half.max(0.0));
    if let Some(h) = ray_aabb(origin, dir, max_dist, center, half_ext, body_id) {
        if best.as_ref().map(|b| h.distance < b.distance).unwrap_or(true) {
            best = Some(h);
        }
    }
    best
}

/// Ray vs line segment (closest intersection within max_dist).
fn ray_segment(
    origin: Vector2,
    dir: Vector2,
    max_dist: f32,
    a: Vector2,
    b: Vector2,
    body_id: BodyId,
) -> Option<RaycastHit> {
    // 2D segment-ray intersection
    let r = dir;
    let s = b - a;
    let det = r.x * s.y - r.y * s.x;
    if det.abs() < 1e-8 {
        return None;
    }
    let qp = a - origin;
    let t = (qp.x * s.y - qp.y * s.x) / det;
    let u = (qp.x * r.y - qp.y * r.x) / det;
    if t < 0.0 || t > max_dist || u < 0.0 || u > 1.0 {
        return None;
    }
    let point = origin + r * t;
    let normal = Vector2::new(-(b.y - a.y), b.x - a.x);
    let nlen = normal.length().max(1e-8);
    Some(RaycastHit {
        body_id,
        point,
        normal: Vector2::new(normal.x / nlen, normal.y / nlen),
        distance: t,
    })
}

/// Ray vs circle intersection.
fn ray_circle(
    origin: Vector2,
    dir: Vector2,
    max_dist: f32,
    center: Vector2,
    radius: f32,
    body_id: BodyId,
) -> Option<RaycastHit> {
    let oc = origin - center;
    let a = dir.dot(dir);
    let b = 2.0 * oc.dot(dir);
    let c = oc.dot(oc) - radius * radius;
    let discriminant = b * b - 4.0 * a * c;

    if discriminant < 0.0 {
        return None;
    }

    let sqrt_d = discriminant.sqrt();
    let t = (-b - sqrt_d) / (2.0 * a);

    if t < 0.0 || t > max_dist {
        // Try the far intersection
        let t2 = (-b + sqrt_d) / (2.0 * a);
        if t2 < 0.0 || t2 > max_dist {
            return None;
        }
        let point = origin + dir * t2;
        let normal = (point - center).normalized();
        return Some(RaycastHit {
            body_id,
            point,
            normal,
            distance: t2,
        });
    }

    let point = origin + dir * t;
    let normal = (point - center).normalized();
    Some(RaycastHit {
        body_id,
        point,
        normal,
        distance: t,
    })
}

/// Ray vs axis-aligned bounding box intersection.
fn ray_aabb(
    origin: Vector2,
    dir: Vector2,
    max_dist: f32,
    center: Vector2,
    half_extents: Vector2,
    body_id: BodyId,
) -> Option<RaycastHit> {
    let min = center - half_extents;
    let max = center + half_extents;

    let inv_dir = Vector2::new(
        if dir.x.abs() > 1e-10 {
            1.0 / dir.x
        } else {
            f32::INFINITY * dir.x.signum()
        },
        if dir.y.abs() > 1e-10 {
            1.0 / dir.y
        } else {
            f32::INFINITY * dir.y.signum()
        },
    );

    let t1x = (min.x - origin.x) * inv_dir.x;
    let t2x = (max.x - origin.x) * inv_dir.x;
    let t1y = (min.y - origin.y) * inv_dir.y;
    let t2y = (max.y - origin.y) * inv_dir.y;

    let t_min_x = t1x.min(t2x);
    let t_max_x = t1x.max(t2x);
    let t_min_y = t1y.min(t2y);
    let t_max_y = t1y.max(t2y);

    let t_enter = t_min_x.max(t_min_y);
    let t_exit = t_max_x.min(t_max_y);

    if t_enter > t_exit || t_exit < 0.0 || t_enter > max_dist {
        return None;
    }

    let t = if t_enter >= 0.0 { t_enter } else { t_exit };
    if t > max_dist {
        return None;
    }

    let point = origin + dir * t;

    // Determine normal based on which face was hit
    let normal = if (point.x - min.x).abs() < 1e-4 {
        Vector2::new(-1.0, 0.0)
    } else if (point.x - max.x).abs() < 1e-4 {
        Vector2::new(1.0, 0.0)
    } else if (point.y - min.y).abs() < 1e-4 {
        Vector2::new(0.0, -1.0)
    } else {
        Vector2::new(0.0, 1.0)
    };

    Some(RaycastHit {
        body_id,
        point,
        normal,
        distance: t,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::BodyType;

    const EPSILON: f32 = 1e-4;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    fn make_rigid_circle(pos: Vector2, radius: f32) -> PhysicsBody2D {
        PhysicsBody2D::new(
            BodyId(0), // will be reassigned
            BodyType::Rigid,
            pos,
            Shape2D::Circle { radius },
            1.0,
        )
    }

    fn make_static_rect(pos: Vector2, half: f32) -> PhysicsBody2D {
        PhysicsBody2D::new(
            BodyId(0),
            BodyType::Static,
            pos,
            Shape2D::Rectangle {
                half_extents: Vector2::new(half, half),
            },
            1.0,
        )
    }

    #[test]
    fn world_add_remove_body() {
        let mut world = PhysicsWorld2D::new();
        let id = world.add_body(make_rigid_circle(Vector2::ZERO, 1.0));
        assert_eq!(world.body_count(), 1);
        assert!(world.get_body(id).is_some());
        world.remove_body(id);
        assert_eq!(world.body_count(), 0);
    }

    #[test]
    fn world_step_rigid_body_moves() {
        let mut world = PhysicsWorld2D::new();
        // Isolate constant-velocity motion from gravity for this assertion.
        world.set_gravity(Vector2::ZERO);
        let mut body = make_rigid_circle(Vector2::ZERO, 1.0);
        body.linear_velocity = Vector2::new(0.0, 10.0); // falling
        let id = world.add_body(body);

        world.step(1.0);

        let b = world.get_body(id).unwrap();
        assert!(approx_eq(b.position.y, 10.0));
    }

    #[test]
    fn world_default_gravity_matches_godot() {
        let world = PhysicsWorld2D::new();
        assert_eq!(world.gravity(), Vector2::new(0.0, 980.0));
    }

    #[test]
    fn world_rigid_body_falls_under_default_gravity() {
        // Under default gravity, a rigid body should fall ~0.5*g*t² in +Y.
        let mut world = PhysicsWorld2D::new();
        let id = world.add_body(make_rigid_circle(Vector2::ZERO, 1.0));

        let dt = 1.0 / 60.0;
        let steps = 60;
        for _ in 0..steps {
            world.step(dt);
        }

        let t = dt * steps as f32;
        let expected = 0.5 * 980.0 * t * t; // ≈ 490 px
        let y = world.get_body(id).unwrap().position.y;
        assert!(
            (y - expected).abs() < expected * 0.05,
            "fell {y}, expected ≈ {expected} (0.5*g*t²)"
        );
        assert!(y > 0.0, "body must fall in +Y");
    }

    #[test]
    fn world_gravity_scale_zero_and_double() {
        // gravity_scale=0 does not fall; gravity_scale=2 falls twice as far.
        let dt = 1.0 / 60.0;
        let steps = 30;

        let mut world = PhysicsWorld2D::new();
        let mut b0 = make_rigid_circle(Vector2::ZERO, 1.0);
        b0.collision_layer = 0; // avoid any inter-body collision
        b0.collision_mask = 0;
        b0.gravity_scale = 0.0;
        let mut b1 = make_rigid_circle(Vector2::new(100.0, 0.0), 1.0);
        b1.collision_layer = 0;
        b1.collision_mask = 0;
        b1.gravity_scale = 1.0;
        let mut b2 = make_rigid_circle(Vector2::new(200.0, 0.0), 1.0);
        b2.collision_layer = 0;
        b2.collision_mask = 0;
        b2.gravity_scale = 2.0;
        let id0 = world.add_body(b0);
        let id1 = world.add_body(b1);
        let id2 = world.add_body(b2);

        for _ in 0..steps {
            world.step(dt);
        }

        let y0 = world.get_body(id0).unwrap().position.y;
        let y1 = world.get_body(id1).unwrap().position.y;
        let y2 = world.get_body(id2).unwrap().position.y;
        assert!(y0.abs() < 1e-5, "gravity_scale=0 should not fall, got {y0}");
        assert!(y1 > 0.0, "gravity_scale=1 should fall, got {y1}");
        // Integration is linear in gravity, so 2x scale falls exactly 2x.
        assert!(
            (y2 - 2.0 * y1).abs() < 1e-2,
            "gravity_scale=2 ({y2}) should fall 2x gravity_scale=1 ({y1})"
        );
    }

    #[test]
    fn world_linear_damp_decays_velocity() {
        // No gravity + linear damping: velocity decays by (1 - dt*d) per step.
        let mut world = PhysicsWorld2D::new();
        world.set_gravity(Vector2::ZERO);
        let dt = 1.0 / 60.0;
        let d = 3.0_f32;
        let mut body = make_rigid_circle(Vector2::ZERO, 1.0);
        body.collision_layer = 0;
        body.collision_mask = 0;
        body.linear_velocity = Vector2::new(50.0, 0.0);
        body.linear_damp = d;
        let id = world.add_body(body);

        let steps = 45;
        for _ in 0..steps {
            world.step(dt);
        }

        let expected = 50.0 * (1.0 - dt * d).powi(steps);
        let vx = world.get_body(id).unwrap().linear_velocity.x;
        assert!(
            (vx - expected).abs() < 1e-2,
            "damped velocity {vx} != expected {expected}"
        );
        assert!(vx < 50.0 && vx > 0.0, "velocity should decay, got {vx}");
    }

    #[test]
    fn world_step_with_area_overrides_applies_per_body_gravity_and_damp() {
        use std::collections::HashMap;

        // Body A gets a zero-gravity override (as if inside an anti-gravity
        // area); body B is absent from the map and falls under world gravity.
        let mut world = PhysicsWorld2D::new();
        let mut a = make_rigid_circle(Vector2::ZERO, 1.0);
        a.collision_layer = 0;
        a.collision_mask = 0;
        let mut b = make_rigid_circle(Vector2::new(1000.0, 0.0), 1.0);
        b.collision_layer = 0;
        b.collision_mask = 0;
        let id_a = world.add_body(a);
        let id_b = world.add_body(b);

        let mut gravity_overrides: HashMap<BodyId, Vector2> = HashMap::new();
        gravity_overrides.insert(id_a, Vector2::ZERO); // A: no gravity
        let damp_overrides: HashMap<BodyId, f32> = HashMap::new();

        let dt = 1.0 / 60.0;
        for _ in 0..30 {
            world.step_with_area_overrides(dt, &gravity_overrides, &damp_overrides);
        }

        let ya = world.get_body(id_a).unwrap().position.y;
        let yb = world.get_body(id_b).unwrap().position.y;
        assert!(ya.abs() < 1e-5, "override to zero gravity must not fall, got {ya}");
        assert!(yb > 0.0, "body without override must fall, got {yb}");

        // A damp override replaces the body's own damping during integration and
        // is restored afterwards (the stored field is left untouched).
        let mut world2 = PhysicsWorld2D::new();
        world2.set_gravity(Vector2::ZERO);
        let mut c = make_rigid_circle(Vector2::ZERO, 1.0);
        c.collision_layer = 0;
        c.collision_mask = 0;
        c.linear_velocity = Vector2::new(100.0, 0.0);
        c.linear_damp = 0.0; // no self-damping
        let id_c = world2.add_body(c);

        let mut damp2: HashMap<BodyId, f32> = HashMap::new();
        damp2.insert(id_c, 5.0); // strong area damping
        let no_gravity: HashMap<BodyId, Vector2> = HashMap::new();
        for _ in 0..30 {
            world2.step_with_area_overrides(dt, &no_gravity, &damp2);
        }
        let body_c = world2.get_body(id_c).unwrap();
        assert!(
            body_c.linear_velocity.x < 100.0,
            "damp override must decay velocity, got {}",
            body_c.linear_velocity.x
        );
        assert_eq!(
            body_c.linear_damp, 0.0,
            "the body's own linear_damp must be restored after the step"
        );
    }

    #[test]
    fn world_static_body_unaffected_by_gravity() {
        let mut world = PhysicsWorld2D::new();
        let id = world.add_body(make_static_rect(Vector2::new(7.0, 3.0), 5.0));
        for _ in 0..30 {
            world.step(1.0 / 60.0);
        }
        let b = world.get_body(id).unwrap();
        assert_eq!(b.position, Vector2::new(7.0, 3.0), "static must not fall");
        assert_eq!(b.linear_velocity, Vector2::ZERO);
    }

    #[test]
    fn world_two_bodies_collide_and_separate() {
        let mut world = PhysicsWorld2D::new();

        // Two circles overlapping
        let id_a = world.add_body(make_rigid_circle(Vector2::new(0.0, 0.0), 5.0));
        let id_b = world.add_body(make_rigid_circle(Vector2::new(8.0, 0.0), 5.0));

        world.step(0.0); // zero dt — just collision detection

        let a = world.get_body(id_a).unwrap();
        let b = world.get_body(id_b).unwrap();
        // Bodies should have been pushed apart
        let dist = (b.position - a.position).length();
        assert!(
            dist >= 10.0 - EPSILON,
            "Bodies should be separated: dist = {dist}"
        );
    }

    #[test]
    fn world_static_body_not_moved_by_collision() {
        let mut world = PhysicsWorld2D::new();
        let static_id = world.add_body(make_static_rect(Vector2::ZERO, 5.0));
        let rigid_id = world.add_body(make_rigid_circle(Vector2::new(6.0, 0.0), 2.0));

        world.step(0.0);

        let s = world.get_body(static_id).unwrap();
        assert_eq!(s.position, Vector2::ZERO, "Static body must not move");

        let r = world.get_body(rigid_id).unwrap();
        assert!(
            r.position.x > 6.0,
            "Rigid body should be pushed away from static"
        );
    }

    // pat-nzgdu.58: the gdphysics2d query layer behind RayCast2D/ShapeCast2D —
    // filtered raycast (exclusions + collision mask) and a shape overlap cast.
    // (Per-node tick state + interpreter dispatch are a separate lane.)
    #[test]
    fn raycast2d_shapecast2d_physics_tick() {
        // Two bodies on the +x axis: a wall on layer 2 at x=5, a target on
        // layer 1 at x=10 (both radius 1).
        let mut world = PhysicsWorld2D::new();
        let mut wall = make_rigid_circle(Vector2::new(5.0, 0.0), 1.0);
        wall.collision_layer = 2;
        let wall_id = world.add_body(wall);
        let mut target = make_rigid_circle(Vector2::new(10.0, 0.0), 1.0);
        target.collision_layer = 1;
        let target_id = world.add_body(target);

        let px = Vector2::new(1.0, 0.0);

        // Unfiltered ray (mask 0) hits the CLOSEST body — the wall at x=5.
        let hit = world
            .raycast_filtered(Vector2::ZERO, px, 100.0, &[], 0)
            .expect("ray should hit the wall");
        assert_eq!(hit.body_id, wall_id);

        // Excluding the wall lets the ray reach the target behind it, with a
        // normal facing back toward the ray origin.
        let hit = world
            .raycast_filtered(Vector2::ZERO, px, 100.0, &[wall_id], 0)
            .expect("ray should reach the target when the wall is excluded");
        assert_eq!(hit.body_id, target_id);
        assert!(
            hit.normal.x < 0.0,
            "collision normal must face the ray, got {:?}",
            hit.normal
        );

        // A mask matching only layer 1 skips the layer-2 wall and hits the target.
        let hit = world
            .raycast_filtered(Vector2::ZERO, px, 100.0, &[], 1)
            .expect("layer-1 mask should hit the target");
        assert_eq!(hit.body_id, target_id);
        // A mask matching only layer 2 hits the wall.
        let hit = world
            .raycast_filtered(Vector2::ZERO, px, 100.0, &[], 2)
            .expect("layer-2 mask should hit the wall");
        assert_eq!(hit.body_id, wall_id);

        // ShapeCast: a unit circle placed on the wall overlaps it.
        let mut at_wall = Transform2D::IDENTITY;
        at_wall.origin = Vector2::new(5.0, 0.0);
        let cast = world.shape_cast(&Shape2D::Circle { radius: 1.0 }, at_wall, &[], 0);
        assert!(!cast.is_empty(), "collision_count must be > 0 against the wall");
        assert!(
            cast.iter().any(|h| h.body_id == wall_id),
            "shape_cast must report the overlapping wall; got {cast:?}"
        );

        // Excluding the wall removes it from the overlap results.
        let cast = world.shape_cast(&Shape2D::Circle { radius: 1.0 }, at_wall, &[wall_id], 0);
        assert!(
            cast.iter().all(|h| h.body_id != wall_id),
            "excluded wall must not appear in shape_cast"
        );

        // A shape far from every body reports no overlaps.
        let mut far = Transform2D::IDENTITY;
        far.origin = Vector2::new(1000.0, 0.0);
        let cast = world.shape_cast(&Shape2D::Circle { radius: 1.0 }, far, &[], 0);
        assert_eq!(cast.len(), 0);
    }

    #[test]
    fn raycast_hits_circle() {
        let mut world = PhysicsWorld2D::new();
        world.add_body(make_rigid_circle(Vector2::new(10.0, 0.0), 2.0));

        let hit = world.raycast(Vector2::ZERO, Vector2::new(1.0, 0.0), 100.0);
        assert!(hit.is_some());
        let hit = hit.unwrap();
        assert!(approx_eq(hit.distance, 8.0)); // 10 - radius 2
        assert!(approx_eq(hit.point.x, 8.0));
    }

    #[test]
    fn raycast_misses_all_bodies() {
        let mut world = PhysicsWorld2D::new();
        world.add_body(make_rigid_circle(Vector2::new(10.0, 10.0), 1.0));

        let hit = world.raycast(Vector2::ZERO, Vector2::new(1.0, 0.0), 100.0);
        assert!(hit.is_none());
    }

    #[test]
    fn raycast_respects_max_distance() {
        let mut world = PhysicsWorld2D::new();
        world.add_body(make_rigid_circle(Vector2::new(50.0, 0.0), 2.0));

        let hit = world.raycast(Vector2::ZERO, Vector2::new(1.0, 0.0), 10.0);
        assert!(hit.is_none(), "Body is beyond max_distance");
    }

    // ---- point_query (input-picking foundation) tests ----

    #[test]
    fn point_query_hits_body_containing_point() {
        let mut world = PhysicsWorld2D::new();
        let id = world.add_body(make_rigid_circle(Vector2::new(0.0, 0.0), 5.0));
        // A world point inside the radius-5 circle centred at the origin.
        let hits = world.point_query(Vector2::new(1.0, 2.0));
        assert!(hits.contains(&id), "point inside circle must hit the body");
    }

    #[test]
    fn point_query_misses_point_outside() {
        let mut world = PhysicsWorld2D::new();
        let id = world.add_body(make_rigid_circle(Vector2::new(0.0, 0.0), 5.0));
        // A world point well outside the radius-5 circle.
        let hits = world.point_query(Vector2::new(100.0, 100.0));
        assert!(!hits.contains(&id), "point outside circle must not hit the body");
        assert!(hits.is_empty(), "no bodies should match, got {hits:?}");
    }

    #[test]
    fn point_query_respects_body_position() {
        let mut world = PhysicsWorld2D::new();
        // Body offset to (100, 0) with radius 5 — proves the world->local transform.
        let id = world.add_body(make_rigid_circle(Vector2::new(100.0, 0.0), 5.0));
        // A point at the body's world position maps to local (0,0) → inside.
        assert!(
            world.point_query(Vector2::new(100.0, 0.0)).contains(&id),
            "point at the body's world position must hit it"
        );
        // A point at the world origin is 100 units away → outside.
        assert!(
            !world.point_query(Vector2::new(0.0, 0.0)).contains(&id),
            "world origin must miss a body offset to (100, 0)"
        );
    }

    #[test]
    fn point_query_returns_all_overlapping_bodies_sorted() {
        let mut world = PhysicsWorld2D::new();
        // Two bodies both covering the world origin; ids are assigned
        // incrementally, so id_a.0 < id_b.0.
        let id_a = world.add_body(make_rigid_circle(Vector2::new(0.0, 0.0), 5.0));
        let id_b = world.add_body(make_static_rect(Vector2::new(0.0, 0.0), 5.0));
        assert!(id_a.0 < id_b.0, "ids must be incremental for this assertion");

        let hits = world.point_query(Vector2::new(0.0, 0.0));
        assert_eq!(
            hits,
            vec![id_a, id_b],
            "both overlapping bodies, ascending BodyId order"
        );

        // The convenience returns the smallest matching id.
        assert_eq!(
            world.point_query_first(Vector2::new(0.0, 0.0)),
            Some(id_a),
            "point_query_first returns the smallest matching BodyId"
        );
        assert_eq!(
            world.point_query_first(Vector2::new(1000.0, 1000.0)),
            None,
            "point_query_first returns None when nothing matches"
        );
    }

    #[test]
    fn collision_layers_prevent_collision() {
        let mut world = PhysicsWorld2D::new();

        let mut body_a = make_rigid_circle(Vector2::new(0.0, 0.0), 5.0);
        body_a.collision_layer = 1;
        body_a.collision_mask = 1;

        let mut body_b = make_rigid_circle(Vector2::new(8.0, 0.0), 5.0);
        body_b.collision_layer = 2; // Different layer
        body_b.collision_mask = 2;

        let id_a = world.add_body(body_a);
        let id_b = world.add_body(body_b);

        let events = world.step(0.0);
        assert!(
            events.is_empty(),
            "Bodies on different layers should not collide"
        );

        // Positions should be unchanged
        let a = world.get_body(id_a).unwrap();
        assert!(approx_eq(a.position.x, 0.0));
        let b = world.get_body(id_b).unwrap();
        assert!(approx_eq(b.position.x, 8.0));
    }

    #[test]
    fn collision_layers_allow_matching_pairs() {
        let mut world = PhysicsWorld2D::new();

        let mut body_a = make_rigid_circle(Vector2::new(0.0, 0.0), 5.0);
        body_a.collision_layer = 1;
        body_a.collision_mask = 2; // Scans layer 2

        let mut body_b = make_rigid_circle(Vector2::new(8.0, 0.0), 5.0);
        body_b.collision_layer = 2; // On layer 2
        body_b.collision_mask = 1;

        let id_a = world.add_body(body_a);
        let id_b = world.add_body(body_b);

        let events = world.step(0.0);
        assert!(
            !events.is_empty(),
            "Matching layer/mask should produce collision events"
        );

        let a = world.get_body(id_a).unwrap();
        let b = world.get_body(id_b).unwrap();
        let dist = (b.position - a.position).length();
        assert!(dist >= 10.0 - EPSILON, "Bodies should be separated");
    }

    #[test]
    fn step_returns_collision_events() {
        let mut world = PhysicsWorld2D::new();
        world.add_body(make_rigid_circle(Vector2::new(0.0, 0.0), 5.0));
        world.add_body(make_rigid_circle(Vector2::new(8.0, 0.0), 5.0));

        let events = world.step(0.0);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].state, ContactState::Entered);
    }

    #[test]
    fn step_narrowphase_is_rotation_aware() {
        use std::f32::consts::FRAC_PI_2;

        // Thin horizontal bar shape (4 wide, 1 tall).
        let make_bar = |pos: Vector2, rot: f32| {
            let mut b = PhysicsBody2D::new(
                BodyId(0),
                BodyType::Static,
                pos,
                Shape2D::Rectangle {
                    half_extents: Vector2::new(2.0, 0.5),
                },
                1.0,
            );
            b.rotation = rot;
            b
        };

        // Axis-aligned bars 0.2 apart on Y produce no contact.
        let mut world = PhysicsWorld2D::new();
        world.add_body(make_bar(Vector2::new(0.0, 0.0), 0.0));
        world.add_body(make_bar(Vector2::new(0.0, 1.2), 0.0));
        assert!(
            world.step(0.0).is_empty(),
            "axis-aligned bars 0.2 apart must not collide"
        );

        // Rotating bar A by 90° makes it span Y ∈ [-2, 2] and reach bar B.
        let mut rotated_world = PhysicsWorld2D::new();
        rotated_world.add_body(make_bar(Vector2::new(0.0, 0.0), FRAC_PI_2));
        rotated_world.add_body(make_bar(Vector2::new(0.0, 1.2), 0.0));
        assert!(
            !rotated_world.step(0.0).is_empty(),
            "rotating bar A by 90° must create a contact (narrow-phase reads rotation)"
        );
    }

    #[test]
    fn persisting_contact_detected() {
        let mut world = PhysicsWorld2D::new();
        // Two overlapping static rects — they won't move apart
        let id_a = world.add_body(make_static_rect(Vector2::new(0.0, 0.0), 5.0));
        let id_b = world.add_body(make_static_rect(Vector2::new(4.0, 0.0), 5.0));

        let events1 = world.step(0.0);
        assert!(events1.iter().any(|e| e.state == ContactState::Entered));

        // Second step: still overlapping
        let events2 = world.step(0.0);
        assert!(
            events2.iter().any(|e| e.state == ContactState::Persisting),
            "Should detect persisting contact, got {:?}",
            events2
        );

        // Cleanup: ensure IDs are valid
        assert!(world.get_body(id_a).is_some());
        assert!(world.get_body(id_b).is_some());
    }

    #[test]
    fn exited_contact_detected() {
        let mut world = PhysicsWorld2D::new();
        let id_a = world.add_body(make_rigid_circle(Vector2::new(0.0, 0.0), 5.0));
        let id_b = world.add_body(make_rigid_circle(Vector2::new(8.0, 0.0), 5.0));

        // First step: collision
        let events = world.step(0.0);
        assert!(!events.is_empty());

        // Move them far apart
        world.get_body_mut(id_a).unwrap().position = Vector2::new(-100.0, 0.0);
        world.get_body_mut(id_b).unwrap().position = Vector2::new(100.0, 0.0);
        world.get_body_mut(id_a).unwrap().linear_velocity = Vector2::ZERO;
        world.get_body_mut(id_b).unwrap().linear_velocity = Vector2::ZERO;

        let events2 = world.step(0.0);
        assert!(events2.iter().any(|e| e.state == ContactState::Exited));
    }

    #[test]
    fn one_way_collision_allows_pass_through() {
        let mut world = PhysicsWorld2D::new();

        // Platform with one-way collision (only collide from above, i.e. body approaching in +Y)
        let mut platform = make_static_rect(Vector2::new(0.0, 10.0), 5.0);
        platform.one_way_collision = true;
        platform.one_way_direction = Vector2::new(0.0, -1.0); // Normal points up

        // Body moving upward through the platform
        let mut body = make_rigid_circle(Vector2::new(0.0, 12.0), 1.0);
        body.linear_velocity = Vector2::new(0.0, -10.0); // Moving up

        let id_body = world.add_body(body);
        world.add_body(platform);

        let events = world.step(0.0);
        // The body is moving upward, approaching from below — should pass through
        let body = world.get_body(id_body).unwrap();
        // Either no collision events or body wasn't pushed
        // (since approach direction dot one_way_direction >= 0 means skip)
        assert!(events.is_empty() || body.position.y < 12.0);
    }

    #[test]
    fn determinism_same_setup_same_result() {
        fn run_simulation() -> Vec<(f32, f32)> {
            let mut world = PhysicsWorld2D::new();
            let id_a = world.add_body(make_rigid_circle(Vector2::new(0.0, 0.0), 3.0));
            let id_b = world.add_body(make_rigid_circle(Vector2::new(5.0, 0.0), 3.0));

            // Give them velocities toward each other
            world.get_body_mut(id_a).unwrap().linear_velocity = Vector2::new(1.0, 0.0);
            world.get_body_mut(id_b).unwrap().linear_velocity = Vector2::new(-1.0, 0.0);

            for _ in 0..10 {
                world.step(1.0 / 60.0);
            }

            let a = world.get_body(id_a).unwrap();
            let b = world.get_body(id_b).unwrap();
            vec![(a.position.x, a.position.y), (b.position.x, b.position.y)]
        }

        let run1 = run_simulation();
        let run2 = run_simulation();
        assert_eq!(run1, run2, "Physics must be deterministic");
    }

    // ---- Joint integration tests ----

    #[test]
    fn world_add_remove_joint() {
        use crate::joint::PinJoint2D;

        let mut world = PhysicsWorld2D::new();
        let id_a = world.add_body(make_rigid_circle(Vector2::new(0.0, 0.0), 1.0));
        let id_b = world.add_body(make_rigid_circle(Vector2::new(5.0, 0.0), 1.0));

        let pin = Joint2D::Pin(PinJoint2D::new(id_a, id_b, Vector2::new(2.5, 0.0)));
        let jid = world.add_joint(pin);
        assert_eq!(world.joint_count(), 1);
        assert!(world.get_joint(jid).is_some());

        world.remove_joint(jid);
        assert_eq!(world.joint_count(), 0);
    }

    #[test]
    fn world_pin_joint_constrains_during_step() {
        use crate::joint::PinJoint2D;

        let mut world = PhysicsWorld2D::new();
        // Disable collision between bodies so collision resolution doesn't
        // push them apart after the pin constraint snaps them together.
        let mut ba = make_rigid_circle(Vector2::new(-5.0, 0.0), 0.5);
        ba.collision_layer = 0;
        ba.collision_mask = 0;
        let mut bb = make_rigid_circle(Vector2::new(5.0, 0.0), 0.5);
        bb.collision_layer = 0;
        bb.collision_mask = 0;
        let id_a = world.add_body(ba);
        let id_b = world.add_body(bb);

        let pin = Joint2D::Pin(PinJoint2D::new(id_a, id_b, Vector2::ZERO));
        world.add_joint(pin);

        world.step(0.0);

        let a = world.get_body(id_a).unwrap();
        let b = world.get_body(id_b).unwrap();
        assert!(
            approx_eq(a.position.x, 0.0),
            "Body A should be at anchor, got {}",
            a.position.x
        );
        assert!(
            approx_eq(b.position.x, 0.0),
            "Body B should be at anchor, got {}",
            b.position.x
        );
    }

    #[test]
    fn world_spring_joint_applies_velocity_during_step() {
        use crate::joint::DampedSpringJoint2D;

        let mut world = PhysicsWorld2D::new();
        // Bodies far apart, small shapes to avoid collision
        let id_a = world.add_body(make_rigid_circle(Vector2::new(0.0, 0.0), 0.1));
        let id_b = world.add_body(make_rigid_circle(Vector2::new(20.0, 0.0), 0.1));

        let spring = Joint2D::DampedSpring(DampedSpringJoint2D::new(id_a, id_b, 5.0, 100.0, 1.0));
        world.add_joint(spring);

        let dt = 1.0 / 60.0;
        world.step(dt);

        let a = world.get_body(id_a).unwrap();
        let b = world.get_body(id_b).unwrap();
        // Spring should pull them together
        assert!(a.linear_velocity.x > 0.0, "Body A should move toward B");
        assert!(b.linear_velocity.x < 0.0, "Body B should move toward A");
    }

    #[test]
    fn world_spring_converges_over_many_steps() {
        use crate::joint::DampedSpringJoint2D;

        let mut world = PhysicsWorld2D::new();
        let id_a = world.add_body(make_rigid_circle(Vector2::new(0.0, 0.0), 0.1));
        let id_b = world.add_body(make_rigid_circle(Vector2::new(15.0, 0.0), 0.1));

        let spring = Joint2D::DampedSpring(DampedSpringJoint2D::new(id_a, id_b, 5.0, 80.0, 10.0));
        world.add_joint(spring);

        let dt = 1.0 / 60.0;
        for _ in 0..300 {
            world.step(dt);
        }

        let a = world.get_body(id_a).unwrap();
        let b = world.get_body(id_b).unwrap();
        let dist = (b.position - a.position).length();
        assert!(
            (dist - 5.0).abs() < 1.0,
            "Spring should converge to rest length 5.0, got {dist}"
        );
    }

    #[test]
    fn world_disabled_joint_has_no_effect() {
        use crate::joint::PinJoint2D;

        let mut world = PhysicsWorld2D::new();
        let mut ba = make_rigid_circle(Vector2::new(-5.0, 0.0), 0.5);
        ba.collision_layer = 0;
        ba.collision_mask = 0;
        let mut bb = make_rigid_circle(Vector2::new(5.0, 0.0), 0.5);
        bb.collision_layer = 0;
        bb.collision_mask = 0;
        let id_a = world.add_body(ba);
        let id_b = world.add_body(bb);

        let mut pin = PinJoint2D::new(id_a, id_b, Vector2::ZERO);
        pin.base.enabled = false;
        let jid = world.add_joint(Joint2D::Pin(pin));

        world.step(0.0);

        let a = world.get_body(id_a).unwrap();
        let b = world.get_body(id_b).unwrap();
        assert!(
            approx_eq(a.position.x, -5.0),
            "Disabled joint should not move bodies"
        );
        assert!(
            approx_eq(b.position.x, 5.0),
            "Disabled joint should not move bodies"
        );

        // Now enable and re-step
        world.get_joint_mut(jid).unwrap().set_enabled(true);
        world.step(0.0);

        let a = world.get_body(id_a).unwrap();
        let b = world.get_body(id_b).unwrap();
        assert!(
            approx_eq(a.position.x, 0.0),
            "Enabled joint should constrain"
        );
        assert!(
            approx_eq(b.position.x, 0.0),
            "Enabled joint should constrain"
        );
    }

    #[test]
    fn world_joint_with_missing_body_is_skipped() {
        use crate::joint::PinJoint2D;

        let mut world = PhysicsWorld2D::new();
        let id_a = world.add_body(make_rigid_circle(Vector2::new(0.0, 0.0), 1.0));

        // Joint references a non-existent body
        let pin = Joint2D::Pin(PinJoint2D::new(id_a, BodyId(9999), Vector2::ZERO));
        world.add_joint(pin);

        // Should not panic
        world.step(1.0 / 60.0);
        assert!(world.get_body(id_a).is_some());
    }
}

#[cfg(test)]
mod capsule_segment_ray_tests {
    use super::*;
    use crate::body::{BodyType, PhysicsBody2D};
    use crate::shape::Shape2D;
    use gdcore::math::Vector2;

    #[test]
    fn raycast_hits_capsule() {
        let mut world = PhysicsWorld2D::new();
        let body = PhysicsBody2D::new(
            BodyId(0),
            BodyType::Static,
            Vector2::new(0.0, 0.0),
            Shape2D::Capsule {
                radius: 1.0,
                height: 4.0,
            },
            0.0,
        );
        world.add_body(body);
        let hit = world.raycast(Vector2::new(-10.0, 0.0), Vector2::new(1.0, 0.0), 20.0);
        assert!(hit.is_some(), "should hit capsule");
    }

    #[test]
    fn raycast_hits_segment() {
        let mut world = PhysicsWorld2D::new();
        let body = PhysicsBody2D::new(
            BodyId(0),
            BodyType::Static,
            Vector2::new(0.0, 0.0),
            Shape2D::Segment {
                a: Vector2::new(0.0, -2.0),
                b: Vector2::new(0.0, 2.0),
            },
            0.0,
        );
        world.add_body(body);
        let hit = world.raycast(Vector2::new(-5.0, 0.0), Vector2::new(1.0, 0.0), 20.0);
        assert!(hit.is_some());
    }

    // -----------------------------------------------------------------------
    // pat-u3l8c.50 — a 10-box stack resting on a static floor must stay
    // stable: it must not explode, tunnel through the floor, or collapse.
    // -----------------------------------------------------------------------
    #[test]
    fn ten_box_stack_stays_stable() {
        // Gravity is (0, +980): +y points *down*, so smaller y is higher up.
        let mut world = PhysicsWorld2D::new();

        // Static floor centred at y = 0 with half-height 5 → top surface at y = -5.
        const FLOOR_TOP: f32 = -5.0;
        const BOX_HALF: f32 = 2.0; // 4×4 boxes
        world.add_body(PhysicsBody2D::new(
            BodyId(0),
            BodyType::Static,
            Vector2::new(0.0, 0.0),
            Shape2D::Rectangle {
                half_extents: Vector2::new(100.0, 5.0),
            },
            0.0,
        ));

        // Ten dynamic boxes, axis-aligned and centred on x = 0, each starting a
        // hair above its resting position so they settle onto one another.
        const N: usize = 10;
        // Resting centre of box i: FLOOR_TOP - BOX_HALF - i * (2*BOX_HALF).
        let rest_y = |i: usize| FLOOR_TOP - BOX_HALF - (i as f32) * (2.0 * BOX_HALF);
        let mut ids = Vec::new();
        for i in 0..N {
            // 0.1 gap between boxes so nothing starts inter-penetrating.
            let y = rest_y(i) - 0.1 * (i as f32 + 1.0);
            let body = PhysicsBody2D::new(
                BodyId(0),
                BodyType::Rigid,
                Vector2::new(0.0, y),
                Shape2D::Rectangle {
                    half_extents: Vector2::new(BOX_HALF, BOX_HALF),
                },
                1.0,
            );
            ids.push(world.add_body(body));
        }

        // Simulate 5 seconds at 60 Hz — plenty of time to settle or blow up.
        for _ in 0..300 {
            world.step(1.0 / 60.0);
        }

        let ys: Vec<f32> = ids
            .iter()
            .map(|&id| world.get_body(id).unwrap().position.y)
            .collect();
        let xs: Vec<f32> = ids
            .iter()
            .map(|&id| world.get_body(id).unwrap().position.x)
            .collect();

        for i in 0..N {
            // 1. Nothing diverged to NaN/inf (catches solver explosions).
            assert!(
                xs[i].is_finite() && ys[i].is_finite(),
                "box {i} position must stay finite, got ({}, {})",
                xs[i],
                ys[i]
            );
            // 2. No box tunnels through the floor. A box centre below
            //    FLOOR_TOP - BOX_HALF + slop means it sank into the floor.
            assert!(
                ys[i] <= FLOOR_TOP - BOX_HALF + 1.0,
                "box {i} sank into/through the floor (y = {})",
                ys[i]
            );
            // 3. Centred contacts impart no horizontal drift.
            assert!(
                xs[i].abs() < 1.0,
                "box {i} drifted horizontally (x = {})",
                xs[i]
            );
            // 4. The whole stack stayed within a generous bounding region
            //    (no upward explosion).
            assert!(
                ys[i] > rest_y(N - 1) - 10.0,
                "box {i} flew upward out of the stack (y = {})",
                ys[i]
            );
        }

        // 5. The stack did not collapse into a single point — the top box is
        //    still well above the bottom one (rest separation is 36 units).
        assert!(
            ys[N - 1] < ys[0] - 20.0,
            "stack collapsed: top box y = {} vs bottom box y = {}",
            ys[N - 1],
            ys[0]
        );
    }
}

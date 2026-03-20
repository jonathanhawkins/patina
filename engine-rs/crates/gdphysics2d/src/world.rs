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
}

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
        }
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
        // Phase 1: Integrate
        for body in self.bodies.values_mut() {
            body.integrate(dt);
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
                    shape_b,
                    pos_b,
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
                        b.shape,
                        b.position,
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

                let tf_a = Transform2D::translated(pos_a);
                let tf_b = Transform2D::translated(pos_b);

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
}

impl Default for PhysicsWorld2D {
    fn default() -> Self {
        Self::new()
    }
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
        _ => None, // Segment and capsule raycasts not yet implemented
    }
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
        let mut body = make_rigid_circle(Vector2::ZERO, 1.0);
        body.linear_velocity = Vector2::new(0.0, 10.0); // falling
        let id = world.add_body(body);

        world.step(1.0);

        let b = world.get_body(id).unwrap();
        assert!(approx_eq(b.position.y, 10.0));
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

//! Physics world state and simulation stepping.
//!
//! The [`PhysicsWorld2D`] owns all physics bodies and runs the simulation loop:
//! integration, broad-phase culling, narrow-phase collision detection, and
//! overlap resolution.

use std::collections::HashMap;

use gdcore::math::{Transform2D, Vector2};

use crate::body::{BodyId, PhysicsBody2D};
use crate::collision;
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

/// A 2D physics world that owns bodies and steps the simulation.
pub struct PhysicsWorld2D {
    bodies: HashMap<BodyId, PhysicsBody2D>,
    next_id: u64,
}

impl PhysicsWorld2D {
    /// Creates an empty physics world.
    pub fn new() -> Self {
        Self {
            bodies: HashMap::new(),
            next_id: 1,
        }
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

    /// Steps the physics simulation by `dt` seconds.
    ///
    /// 1. Integrate all rigid/kinematic bodies.
    /// 2. Detect collisions between all body pairs.
    /// 3. Resolve overlaps and apply impulses.
    pub fn step(&mut self, dt: f32) {
        // Phase 1: Integrate
        for body in self.bodies.values_mut() {
            body.integrate(dt);
        }

        // Phase 2 & 3: Collision detection and resolution
        // Collect body IDs for iteration (need to borrow mutably later).
        let ids: Vec<BodyId> = self.bodies.keys().copied().collect();

        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                let id_a = ids[i];
                let id_b = ids[j];

                // Read shapes and positions
                let (shape_a, pos_a, shape_b, pos_b) = {
                    let a = &self.bodies[&id_a];
                    let b = &self.bodies[&id_b];
                    (a.shape, a.position, b.shape, b.position)
                };

                let tf_a = Transform2D::translated(pos_a);
                let tf_b = Transform2D::translated(pos_b);

                if let Some(result) = collision::test_collision(&shape_a, &tf_a, &shape_b, &tf_b) {
                    if result.colliding && result.depth > 0.0 {
                        // We need to split the borrow to get two mutable refs.
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
}

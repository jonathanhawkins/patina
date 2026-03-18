//! 3D physics world state and simulation stepping.
//!
//! The [`PhysicsWorld3D`] owns all 3D physics bodies and runs the simulation
//! loop: gravity application, integration, collision detection, and resolution.

use std::collections::HashMap;

use gdcore::math::Vector3;

use crate::body3d::{BodyId3D, BodyType3D, PhysicsBody3D};
use crate::collision3d;
use crate::shape3d::Shape3D;

/// Result of a 3D raycast query.
#[derive(Debug, Clone, Copy)]
pub struct RaycastHit3D {
    /// The body that was hit.
    pub body_id: BodyId3D,
    /// The hit point in world space.
    pub point: Vector3,
    /// The surface normal at the hit point.
    pub normal: Vector3,
    /// Distance from the ray origin to the hit point.
    pub distance: f32,
}

/// A 3D physics world that owns bodies and steps the simulation.
pub struct PhysicsWorld3D {
    bodies: HashMap<BodyId3D, PhysicsBody3D>,
    next_id: u64,
    /// Gravity acceleration applied to all rigid bodies each step.
    pub gravity: Vector3,
}

impl PhysicsWorld3D {
    /// Creates an empty 3D physics world with default gravity (0, -9.8, 0).
    pub fn new() -> Self {
        Self {
            bodies: HashMap::new(),
            next_id: 1,
            gravity: Vector3::new(0.0, -9.8, 0.0),
        }
    }

    /// Adds a body to the world and returns its unique ID.
    pub fn add_body(&mut self, mut body: PhysicsBody3D) -> BodyId3D {
        let id = BodyId3D(self.next_id);
        self.next_id += 1;
        body.id = id;
        self.bodies.insert(id, body);
        id
    }

    /// Removes a body from the world by ID.
    pub fn remove_body(&mut self, id: BodyId3D) -> Option<PhysicsBody3D> {
        self.bodies.remove(&id)
    }

    /// Returns a reference to a body by ID.
    pub fn get_body(&self, id: BodyId3D) -> Option<&PhysicsBody3D> {
        self.bodies.get(&id)
    }

    /// Returns a mutable reference to a body by ID.
    pub fn get_body_mut(&mut self, id: BodyId3D) -> Option<&mut PhysicsBody3D> {
        self.bodies.get_mut(&id)
    }

    /// Returns the number of bodies in the world.
    pub fn body_count(&self) -> usize {
        self.bodies.len()
    }

    /// Steps the physics simulation by `dt` seconds.
    ///
    /// 1. Apply gravity to rigid bodies.
    /// 2. Integrate all rigid/kinematic bodies.
    /// 3. Detect collisions between all body pairs.
    /// 4. Resolve overlaps and apply impulses.
    pub fn step(&mut self, dt: f32) {
        // Phase 1: Apply gravity and integrate
        let gravity = self.gravity;
        for body in self.bodies.values_mut() {
            if body.body_type == BodyType3D::Rigid {
                body.apply_force(gravity * body.mass);
            }
            body.integrate(dt);
        }

        // Phase 2 & 3: Collision detection and resolution
        let ids: Vec<BodyId3D> = self.bodies.keys().copied().collect();

        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                let id_a = ids[i];
                let id_b = ids[j];

                let (shape_a, pos_a, shape_b, pos_b) = {
                    let a = &self.bodies[&id_a];
                    let b = &self.bodies[&id_b];
                    (a.shape, a.position, b.shape, b.position)
                };

                if let Some(result) =
                    collision3d::test_collision_3d(&shape_a, pos_a, &shape_b, pos_b)
                {
                    if result.colliding && result.depth > 0.0 {
                        // SAFETY: id_a != id_b because i != j and IDs are unique.
                        let ptr = &mut self.bodies as *mut HashMap<BodyId3D, PhysicsBody3D>;
                        let body_a = unsafe { &mut *ptr }.get_mut(&id_a).unwrap();
                        let body_b = unsafe { &mut *ptr }.get_mut(&id_b).unwrap();
                        collision3d::separate_bodies_3d(body_a, body_b, &result);
                    }
                }
            }
        }
    }

    /// Casts a ray and returns the closest hit, if any.
    ///
    /// Tests against all bodies' shapes using simple ray-shape intersection.
    pub fn raycast_3d(
        &self,
        origin: Vector3,
        direction: Vector3,
        max_distance: f32,
    ) -> Option<RaycastHit3D> {
        let dir = direction.normalized();
        if dir.length_squared() < 1e-10 {
            return None;
        }

        let mut closest: Option<RaycastHit3D> = None;

        for body in self.bodies.values() {
            if let Some(hit) = raycast_shape_3d(origin, dir, max_distance, body) {
                if closest.is_none() || hit.distance < closest.as_ref().unwrap().distance {
                    closest = Some(hit);
                }
            }
        }

        closest
    }
}

impl Default for PhysicsWorld3D {
    fn default() -> Self {
        Self::new()
    }
}

/// Ray-shape intersection test for a single 3D body.
fn raycast_shape_3d(
    origin: Vector3,
    dir: Vector3,
    max_distance: f32,
    body: &PhysicsBody3D,
) -> Option<RaycastHit3D> {
    match body.shape {
        Shape3D::Sphere { radius } => {
            ray_sphere(origin, dir, max_distance, body.position, radius, body.id)
        }
        Shape3D::BoxShape { half_extents } => ray_aabb_3d(
            origin,
            dir,
            max_distance,
            body.position,
            half_extents,
            body.id,
        ),
        _ => None,
    }
}

/// Ray vs sphere intersection.
fn ray_sphere(
    origin: Vector3,
    dir: Vector3,
    max_dist: f32,
    center: Vector3,
    radius: f32,
    body_id: BodyId3D,
) -> Option<RaycastHit3D> {
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
        let t2 = (-b + sqrt_d) / (2.0 * a);
        if t2 < 0.0 || t2 > max_dist {
            return None;
        }
        let point = origin + dir * t2;
        let normal = (point - center).normalized();
        return Some(RaycastHit3D {
            body_id,
            point,
            normal,
            distance: t2,
        });
    }

    let point = origin + dir * t;
    let normal = (point - center).normalized();
    Some(RaycastHit3D {
        body_id,
        point,
        normal,
        distance: t,
    })
}

/// Ray vs axis-aligned bounding box intersection in 3D.
fn ray_aabb_3d(
    origin: Vector3,
    dir: Vector3,
    max_dist: f32,
    center: Vector3,
    half_extents: Vector3,
    body_id: BodyId3D,
) -> Option<RaycastHit3D> {
    let min = center - half_extents;
    let max = center + half_extents;

    let inv_x = if dir.x.abs() > 1e-10 {
        1.0 / dir.x
    } else {
        f32::INFINITY * dir.x.signum()
    };
    let inv_y = if dir.y.abs() > 1e-10 {
        1.0 / dir.y
    } else {
        f32::INFINITY * dir.y.signum()
    };
    let inv_z = if dir.z.abs() > 1e-10 {
        1.0 / dir.z
    } else {
        f32::INFINITY * dir.z.signum()
    };

    let t1x = (min.x - origin.x) * inv_x;
    let t2x = (max.x - origin.x) * inv_x;
    let t1y = (min.y - origin.y) * inv_y;
    let t2y = (max.y - origin.y) * inv_y;
    let t1z = (min.z - origin.z) * inv_z;
    let t2z = (max.z - origin.z) * inv_z;

    let t_enter = t1x.min(t2x).max(t1y.min(t2y)).max(t1z.min(t2z));
    let t_exit = t1x.max(t2x).min(t1y.max(t2y)).min(t1z.max(t2z));

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
        Vector3::new(-1.0, 0.0, 0.0)
    } else if (point.x - max.x).abs() < 1e-4 {
        Vector3::new(1.0, 0.0, 0.0)
    } else if (point.y - min.y).abs() < 1e-4 {
        Vector3::new(0.0, -1.0, 0.0)
    } else if (point.y - max.y).abs() < 1e-4 {
        Vector3::new(0.0, 1.0, 0.0)
    } else if (point.z - min.z).abs() < 1e-4 {
        Vector3::new(0.0, 0.0, -1.0)
    } else {
        Vector3::new(0.0, 0.0, 1.0)
    };

    Some(RaycastHit3D {
        body_id,
        point,
        normal,
        distance: t,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body3d::BodyType3D;

    const EPSILON: f32 = 1e-4;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    fn make_rigid_sphere(pos: Vector3, radius: f32) -> PhysicsBody3D {
        PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            pos,
            Shape3D::Sphere { radius },
            1.0,
        )
    }

    fn make_static_box(pos: Vector3, half: f32) -> PhysicsBody3D {
        PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            pos,
            Shape3D::BoxShape {
                half_extents: Vector3::new(half, half, half),
            },
            1.0,
        )
    }

    #[test]
    fn world3d_add_remove_body() {
        let mut world = PhysicsWorld3D::new();
        let id = world.add_body(make_rigid_sphere(Vector3::ZERO, 1.0));
        assert_eq!(world.body_count(), 1);
        assert!(world.get_body(id).is_some());
        world.remove_body(id);
        assert_eq!(world.body_count(), 0);
    }

    #[test]
    fn world3d_default_gravity() {
        let world = PhysicsWorld3D::new();
        assert!(approx_eq(world.gravity.y, -9.8));
        assert!(approx_eq(world.gravity.x, 0.0));
        assert!(approx_eq(world.gravity.z, 0.0));
    }

    #[test]
    fn world3d_gravity_accelerates_body() {
        let mut world = PhysicsWorld3D::new();
        let id = world.add_body(make_rigid_sphere(Vector3::new(0.0, 10.0, 0.0), 1.0));

        world.step(1.0);

        let b = world.get_body(id).unwrap();
        // After 1s of gravity, should have moved down
        assert!(b.position.y < 10.0, "Body should fall: y={}", b.position.y);
    }

    #[test]
    fn world3d_no_gravity_mode() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        let mut body = make_rigid_sphere(Vector3::ZERO, 1.0);
        body.linear_velocity = Vector3::new(5.0, 0.0, 0.0);
        let id = world.add_body(body);

        world.step(1.0);

        let b = world.get_body(id).unwrap();
        assert!(approx_eq(b.position.x, 5.0));
        assert!(approx_eq(b.position.y, 0.0));
    }

    #[test]
    fn world3d_two_bodies_collide_and_separate() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;

        let id_a = world.add_body(make_rigid_sphere(Vector3::ZERO, 5.0));
        let id_b = world.add_body(make_rigid_sphere(Vector3::new(8.0, 0.0, 0.0), 5.0));

        world.step(0.0);

        let a = world.get_body(id_a).unwrap();
        let b = world.get_body(id_b).unwrap();
        let dist = (b.position - a.position).length();
        assert!(
            dist >= 10.0 - EPSILON,
            "Bodies should be separated: dist = {dist}"
        );
    }

    #[test]
    fn world3d_static_body_not_moved() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        let static_id = world.add_body(make_static_box(Vector3::ZERO, 5.0));
        let rigid_id = world.add_body(make_rigid_sphere(Vector3::new(6.0, 0.0, 0.0), 2.0));

        world.step(0.0);

        let s = world.get_body(static_id).unwrap();
        assert_eq!(s.position, Vector3::ZERO, "Static body must not move");

        let r = world.get_body(rigid_id).unwrap();
        assert!(r.position.x > 6.0, "Rigid body should be pushed away");
    }

    #[test]
    fn raycast3d_hits_sphere() {
        let mut world = PhysicsWorld3D::new();
        world.add_body(make_rigid_sphere(Vector3::new(10.0, 0.0, 0.0), 2.0));

        let hit = world.raycast_3d(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0), 100.0);
        assert!(hit.is_some());
        let hit = hit.unwrap();
        assert!(approx_eq(hit.distance, 8.0));
        assert!(approx_eq(hit.point.x, 8.0));
    }

    #[test]
    fn raycast3d_misses() {
        let mut world = PhysicsWorld3D::new();
        world.add_body(make_rigid_sphere(Vector3::new(10.0, 10.0, 10.0), 1.0));

        let hit = world.raycast_3d(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0), 100.0);
        assert!(hit.is_none());
    }

    #[test]
    fn raycast3d_respects_max_distance() {
        let mut world = PhysicsWorld3D::new();
        world.add_body(make_rigid_sphere(Vector3::new(50.0, 0.0, 0.0), 2.0));

        let hit = world.raycast_3d(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0), 10.0);
        assert!(hit.is_none());
    }

    #[test]
    fn determinism_3d() {
        fn run_sim() -> Vec<(f32, f32, f32)> {
            let mut world = PhysicsWorld3D::new();
            world.gravity = Vector3::ZERO;
            let id_a = world.add_body(make_rigid_sphere(Vector3::ZERO, 3.0));
            let id_b = world.add_body(make_rigid_sphere(Vector3::new(5.0, 0.0, 0.0), 3.0));

            world.get_body_mut(id_a).unwrap().linear_velocity = Vector3::new(1.0, 0.0, 0.0);
            world.get_body_mut(id_b).unwrap().linear_velocity = Vector3::new(-1.0, 0.0, 0.0);

            for _ in 0..10 {
                world.step(1.0 / 60.0);
            }

            let a = world.get_body(id_a).unwrap();
            let b = world.get_body(id_b).unwrap();
            vec![
                (a.position.x, a.position.y, a.position.z),
                (b.position.x, b.position.y, b.position.z),
            ]
        }

        let run1 = run_sim();
        let run2 = run_sim();
        assert_eq!(run1, run2, "3D physics must be deterministic");
    }
}

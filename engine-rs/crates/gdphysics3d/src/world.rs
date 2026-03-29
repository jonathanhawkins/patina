//! 3D physics world state and simulation stepping.

use std::collections::BTreeMap;

use gdcore::math::Vector3;

#[cfg(test)]
use crate::body::BodyType3D;
use crate::body::{BodyId3D, ContactPoint3D, PhysicsBody3D};
use crate::collision;
use crate::shape::Shape3D;

/// Result of a 3D raycast query.
#[derive(Debug, Clone, Copy)]
pub struct RaycastHit3D {
    /// The body that was hit.
    pub body_id: BodyId3D,
    /// The hit point in world space.
    pub point: Vector3,
    /// The surface normal at the hit point.
    pub normal: Vector3,
    /// Distance from the ray origin.
    pub distance: f32,
}

/// A 3D physics world that owns bodies and steps the simulation.
pub struct PhysicsWorld3D {
    bodies: BTreeMap<BodyId3D, PhysicsBody3D>,
    next_id: u64,
    /// Gravity acceleration applied to all rigid bodies each step.
    pub gravity: Vector3,
}

impl PhysicsWorld3D {
    /// Creates an empty 3D physics world with default gravity (0, -9.8, 0).
    pub fn new() -> Self {
        Self {
            bodies: BTreeMap::new(),
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

    /// Steps the simulation by `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        let gravity = self.gravity;

        // 0. Clear contacts from previous step.
        for body in self.bodies.values_mut() {
            body.clear_contacts();
        }

        // 1. Integrate all rigid bodies.
        for body in self.bodies.values_mut() {
            body.integrate(dt, gravity);
        }

        // 2. Detect and resolve collisions (sphere-sphere only for now).
        let ids: Vec<BodyId3D> = self.bodies.keys().copied().collect();
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                let id_a = ids[i];
                let id_b = ids[j];

                let (pos_a, shape_a, pos_b, shape_b) = {
                    let a = &self.bodies[&id_a];
                    let b = &self.bodies[&id_b];
                    (a.position, a.shape.clone(), b.position, b.shape.clone())
                };

                let result = collision::test_collision(pos_a, &shape_a, pos_b, &shape_b);
                if result.colliding {
                    // Record contacts.
                    let contact_pos = pos_a + result.normal * (pos_b - pos_a).length() * 0.5;
                    if let Some(a) = self.bodies.get_mut(&id_a) {
                        a.add_contact(ContactPoint3D {
                            position: contact_pos,
                            normal: result.normal,
                            depth: result.depth,
                            other_body: id_b,
                        });
                    }
                    if let Some(b) = self.bodies.get_mut(&id_b) {
                        b.add_contact(ContactPoint3D {
                            position: contact_pos,
                            normal: result.normal * -1.0,
                            depth: result.depth,
                            other_body: id_a,
                        });
                    }

                    let a_inv = self.bodies[&id_a].inverse_mass();
                    let b_inv = self.bodies[&id_b].inverse_mass();
                    let total_inv = a_inv + b_inv;

                    if total_inv > 0.0 {
                        // Position correction.
                        let correction = result.normal * result.depth;
                        if let Some(a) = self.bodies.get_mut(&id_a) {
                            a.position = a.position - correction * (a_inv / total_inv);
                        }
                        if let Some(b) = self.bodies.get_mut(&id_b) {
                            b.position = b.position + correction * (b_inv / total_inv);
                        }

                        // Impulse-based velocity response with restitution and friction.
                        let vel_a = self.bodies[&id_a].linear_velocity;
                        let vel_b = self.bodies[&id_b].linear_velocity;
                        let relative_vel = vel_a - vel_b;
                        let vel_along_normal = relative_vel.dot(result.normal);

                        // Only resolve if bodies are moving toward each other.
                        if vel_along_normal > 0.0 {
                            let restitution =
                                self.bodies[&id_a].bounce.min(self.bodies[&id_b].bounce);
                            let j = -(1.0 + restitution) * vel_along_normal / total_inv;
                            let impulse = result.normal * j;

                            // impulse = normal * j where j < 0 (opposing approach).
                            // vel_a += impulse / m_a, vel_b -= impulse / m_b.
                            if let Some(a) = self.bodies.get_mut(&id_a) {
                                a.linear_velocity = a.linear_velocity + impulse * a_inv;
                            }
                            if let Some(b) = self.bodies.get_mut(&id_b) {
                                b.linear_velocity = b.linear_velocity - impulse * b_inv;
                            }

                            // Friction impulse (tangential).
                            let tangent = relative_vel - result.normal * vel_along_normal;
                            let tangent_len = tangent.length();
                            if tangent_len > 1e-8 {
                                let tangent_dir = tangent * (1.0 / tangent_len);
                                let friction_coeff = (self.bodies[&id_a].friction
                                    * self.bodies[&id_b].friction)
                                    .sqrt();
                                let jt = -tangent_len / total_inv;
                                // Coulomb friction clamp.
                                let friction_impulse = if jt.abs() < j.abs() * friction_coeff {
                                    tangent_dir * jt
                                } else {
                                    tangent_dir * (-j.abs() * friction_coeff)
                                };

                                if let Some(a) = self.bodies.get_mut(&id_a) {
                                    a.linear_velocity =
                                        a.linear_velocity + friction_impulse * a_inv;
                                }
                                if let Some(b) = self.bodies.get_mut(&id_b) {
                                    b.linear_velocity =
                                        b.linear_velocity - friction_impulse * b_inv;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Casts a ray and returns the closest hit (sphere bodies only).
    pub fn raycast(&self, origin: Vector3, direction: Vector3) -> Option<RaycastHit3D> {
        let dir = direction.normalized();
        let mut closest: Option<RaycastHit3D> = None;

        for body in self.bodies.values() {
            if let Shape3D::Sphere { radius } = body.shape {
                let oc = origin - body.position;
                let a = dir.dot(dir);
                let b = 2.0 * oc.dot(dir);
                let c = oc.dot(oc) - radius * radius;
                let discriminant = b * b - 4.0 * a * c;

                if discriminant >= 0.0 {
                    let t = (-b - discriminant.sqrt()) / (2.0 * a);
                    if t > 0.0 {
                        let point = origin + dir * t;
                        let normal = (point - body.position).normalized();
                        let hit = RaycastHit3D {
                            body_id: body.id,
                            point,
                            normal,
                            distance: t,
                        };
                        if closest.as_ref().map_or(true, |c| t < c.distance) {
                            closest = Some(hit);
                        }
                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sphere_body(pos: Vector3) -> PhysicsBody3D {
        PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            pos,
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        )
    }

    #[test]
    fn add_and_get_body() {
        let mut world = PhysicsWorld3D::new();
        let id = world.add_body(sphere_body(Vector3::ZERO));
        assert!(world.get_body(id).is_some());
        assert_eq!(world.body_count(), 1);
    }

    #[test]
    fn remove_body() {
        let mut world = PhysicsWorld3D::new();
        let id = world.add_body(sphere_body(Vector3::ZERO));
        let removed = world.remove_body(id);
        assert!(removed.is_some());
        assert_eq!(world.body_count(), 0);
    }

    #[test]
    fn step_applies_gravity() {
        let mut world = PhysicsWorld3D::new();
        let id = world.add_body(sphere_body(Vector3::new(0.0, 10.0, 0.0)));
        world.step(1.0 / 60.0);
        let body = world.get_body(id).unwrap();
        assert!(body.position.y < 10.0, "body should fall");
    }

    #[test]
    fn collision_separates_overlapping_spheres() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        let id_a = world.add_body(sphere_body(Vector3::ZERO));
        let id_b = world.add_body(sphere_body(Vector3::new(1.0, 0.0, 0.0)));
        world.step(1.0 / 60.0);
        let a = world.get_body(id_a).unwrap();
        let b = world.get_body(id_b).unwrap();
        let dist = (b.position - a.position).length();
        assert!(
            dist >= 1.9,
            "spheres should be separated after collision, dist={dist}"
        );
    }

    #[test]
    fn raycast_hits_sphere() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        world.add_body(sphere_body(Vector3::new(0.0, 0.0, 10.0)));
        let hit = world.raycast(Vector3::ZERO, Vector3::new(0.0, 0.0, 1.0));
        assert!(hit.is_some());
        let h = hit.unwrap();
        assert!((h.distance - 9.0).abs() < 1e-3);
    }

    #[test]
    fn raycast_misses() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        world.add_body(sphere_body(Vector3::new(10.0, 0.0, 0.0)));
        let hit = world.raycast(Vector3::ZERO, Vector3::new(0.0, 0.0, 1.0));
        assert!(hit.is_none());
    }

    #[test]
    fn deterministic_stepping() {
        let run = || {
            let mut world = PhysicsWorld3D::new();
            let id = world.add_body(sphere_body(Vector3::new(0.0, 100.0, 0.0)));
            for _ in 0..60 {
                world.step(1.0 / 60.0);
            }
            world.get_body(id).unwrap().position
        };
        let a = run();
        let b = run();
        assert_eq!(a, b, "simulation must be deterministic");
    }

    #[test]
    fn bounce_reverses_velocity() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;

        // Rigid ball moving right toward a static wall.
        let mut ball = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        );
        ball.bounce = 1.0; // perfect bounce
        ball.linear_velocity = Vector3::new(10.0, 0.0, 0.0);
        ball.can_sleep = false;
        let ball_id = world.add_body(ball);

        // Static wall at x=1.5 (overlap with sphere at x=0, radius 1 + radius 1 = 2 > 1.5 → colliding)
        let mut wall = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::new(1.5, 0.0, 0.0),
            Shape3D::Sphere { radius: 1.0 },
            0.0,
        );
        wall.bounce = 1.0; // match ball's bounce for perfect reflection
        world.add_body(wall);

        world.step(1.0 / 60.0);

        let body = world.get_body(ball_id).unwrap();
        // Ball should have reversed its X velocity (or at least be moving left after bounce).
        assert!(
            body.linear_velocity.x < 0.0,
            "ball should bounce back, got vx={}",
            body.linear_velocity.x
        );
    }

    #[test]
    fn zero_bounce_absorbs_velocity() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;

        let mut ball = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        );
        ball.bounce = 0.0;
        ball.linear_velocity = Vector3::new(10.0, 0.0, 0.0);
        ball.can_sleep = false;
        let ball_id = world.add_body(ball);

        let wall = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::new(1.5, 0.0, 0.0),
            Shape3D::Sphere { radius: 1.0 },
            0.0,
        );
        world.add_body(wall);

        world.step(1.0 / 60.0);

        let body = world.get_body(ball_id).unwrap();
        // With zero bounce, normal velocity should be absorbed (near zero or slightly negative from separation).
        assert!(
            body.linear_velocity.x.abs() < 1.0,
            "zero-bounce should absorb velocity, got vx={}",
            body.linear_velocity.x
        );
    }

    #[test]
    fn contact_recording_after_collision() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;

        let mut ball = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        );
        ball.contact_monitor = true;
        ball.max_contacts_reported = 4;
        ball.linear_velocity = Vector3::new(10.0, 0.0, 0.0);
        ball.can_sleep = false;
        let ball_id = world.add_body(ball);

        let wall = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::new(1.5, 0.0, 0.0),
            Shape3D::Sphere { radius: 1.0 },
            0.0,
        );
        let wall_id = world.add_body(wall);

        world.step(1.0 / 60.0);

        let body = world.get_body(ball_id).unwrap();
        assert!(body.get_contact_count() > 0, "should record contact");
        let colliders = body.get_colliding_bodies();
        assert!(colliders.contains(&wall_id));
    }

    #[test]
    fn static_body_not_moved_by_collision() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;

        let mut ball = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        );
        ball.linear_velocity = Vector3::new(10.0, 0.0, 0.0);
        ball.can_sleep = false;
        world.add_body(ball);

        let wall = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::new(1.5, 0.0, 0.0),
            Shape3D::Sphere { radius: 1.0 },
            0.0,
        );
        let wall_id = world.add_body(wall);

        world.step(1.0 / 60.0);

        let wall_body = world.get_body(wall_id).unwrap();
        assert!(
            (wall_body.position.x - 1.5).abs() < 1e-3,
            "static body should not move"
        );
    }

    #[test]
    fn get_body_mut_modifies() {
        let mut world = PhysicsWorld3D::new();
        let id = world.add_body(sphere_body(Vector3::ZERO));
        if let Some(body) = world.get_body_mut(id) {
            body.linear_velocity = Vector3::new(5.0, 0.0, 0.0);
        }
        assert!((world.get_body(id).unwrap().linear_velocity.x - 5.0).abs() < 1e-5);
    }

    #[test]
    fn default_gravity() {
        let world = PhysicsWorld3D::new();
        assert!((world.gravity.y - (-9.8)).abs() < 1e-5);
    }
}

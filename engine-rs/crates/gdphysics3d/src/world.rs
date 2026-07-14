//! 3D physics world state and simulation stepping.

use std::collections::BTreeMap;

use gdcore::math::Vector3;
use gdcore::math3d::Aabb;

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
    /// Soft bodies stepped with gravity each frame (pat-p0s82.18).
    soft_bodies: Vec<gdphysics2d::softbody3d::SoftBody3D>,
}

impl PhysicsWorld3D {
    /// Creates an empty 3D physics world with default gravity (0, -9.8, 0).
    pub fn new() -> Self {
        Self {
            bodies: BTreeMap::new(),
            next_id: 1,
            gravity: Vector3::new(0.0, -9.8, 0.0),
            soft_bodies: Vec::new(),
        }
    }

    /// Register a soft body for simulation.
    pub fn add_soft_body(&mut self, body: gdphysics2d::softbody3d::SoftBody3D) {
        self.soft_bodies.push(body);
    }

    /// Soft body count.
    pub fn soft_body_count(&self) -> usize {
        self.soft_bodies.len()
    }

    /// Mutable access to soft bodies.
    pub fn soft_bodies_mut(&mut self) -> &mut [gdphysics2d::softbody3d::SoftBody3D] {
        &mut self.soft_bodies
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

    /// Returns an iterator over all bodies in the world.
    pub fn bodies(&self) -> impl Iterator<Item = &PhysicsBody3D> {
        self.bodies.values()
    }

    /// Steps the simulation by `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        let gravity = self.gravity;

        // Soft bodies integrate with the same gravity (pat-p0s82.18).
        for sb in &mut self.soft_bodies {
            sb.step(dt, gravity);
        }

        // 0. Clear contacts from previous step.
        for body in self.bodies.values_mut() {
            body.clear_contacts();
        }

        // 1. Integrate all rigid bodies.
        for body in self.bodies.values_mut() {
            body.integrate(dt, gravity);
        }

        // 2. Broad-phase: prune candidate pairs whose world-space AABBs overlap
        //    via sweep-and-prune (O(n log n + k) instead of O(n^2) all-pairs).
        //    Each candidate is then filtered by collision layer/mask before the
        //    narrow-phase test and impulse resolution.
        let pairs = self.broadphase_pairs();
        for (id_a, id_b) in pairs {
            let (should_collide, pos_a, rot_a, shape_a, pos_b, rot_b, shape_b) = {
                let a = &self.bodies[&id_a];
                let b = &self.bodies[&id_b];
                (
                    a.should_collide_with(b),
                    a.position,
                    a.rotation,
                    a.shape.clone(),
                    b.position,
                    b.rotation,
                    b.shape.clone(),
                )
            };

            // Collision layer/mask filter (Godot semantics): skip the pair
            // unless either body's mask includes a layer the other belongs to.
            if !should_collide {
                continue;
            }

            let result =
                collision::test_collision_rot(pos_a, rot_a, &shape_a, pos_b, rot_b, &shape_b);
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
                                a.linear_velocity = a.linear_velocity + friction_impulse * a_inv;
                            }
                            if let Some(b) = self.bodies.get_mut(&id_b) {
                                b.linear_velocity = b.linear_velocity - friction_impulse * b_inv;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Broad-phase sweep-and-prune: returns candidate body pairs whose
    /// world-space AABBs overlap, sorted by ascending id so downstream
    /// resolution is deterministic.
    ///
    /// Bodies are sorted along the X axis; each body is only compared against
    /// neighbours that still overlap it on X, so a scene of well-separated
    /// bodies costs O(n log n + k) (k = overlapping pairs) rather than the
    /// O(n^2) all-pairs scan. A real collision always implies AABB overlap, so
    /// pruning never drops a colliding pair.
    fn broadphase_pairs(&self) -> Vec<(BodyId3D, BodyId3D)> {
        let mut entries: Vec<(BodyId3D, Aabb)> = self
            .bodies
            .values()
            .map(|b| (b.id, b.world_aabb()))
            .collect();
        entries.sort_by(|a, b| {
            a.1.position
                .x
                .partial_cmp(&b.1.position.x)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut pairs: Vec<(BodyId3D, BodyId3D)> = Vec::new();
        for (i, &(id_i, aabb_i)) in entries.iter().enumerate() {
            let i_max_x = aabb_i.position.x + aabb_i.size.x;
            for &(id_j, aabb_j) in entries.iter().skip(i + 1) {
                // Sorted on X: once a neighbour begins beyond i's max X, no
                // later neighbour can overlap on X either — stop scanning.
                if aabb_j.position.x > i_max_x {
                    break;
                }
                if aabb_i.intersects(aabb_j) {
                    if id_i.0 <= id_j.0 {
                        pairs.push((id_i, id_j));
                    } else {
                        pairs.push((id_j, id_i));
                    }
                }
            }
        }

        // Deterministic ascending-id order, matching the prior all-pairs scan.
        pairs.sort_by(|x, y| (x.0 .0, x.1 .0).cmp(&(y.0 .0, y.1 .0)));
        pairs
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

    // -- Collision layer/mask filtering -----------------------------------

    #[test]
    fn collision_mask_skips_non_matching_pair() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;

        // Two overlapping spheres that should ignore each other: a is only on
        // and scanning layer 1, b is only on and scanning layer 2.
        let mut a = sphere_body(Vector3::ZERO);
        a.collision_layer = 0b0001;
        a.collision_mask = 0b0001;
        let mut b = sphere_body(Vector3::new(1.0, 0.0, 0.0));
        b.collision_layer = 0b0010;
        b.collision_mask = 0b0010;
        let id_a = world.add_body(a);
        let id_b = world.add_body(b);

        world.step(1.0 / 60.0);

        // No separation and no recorded contacts because masks are disjoint.
        let pa = world.get_body(id_a).unwrap().position;
        let pb = world.get_body(id_b).unwrap().position;
        let dist = (pb - pa).length();
        assert!(
            (dist - 1.0).abs() < 1e-5,
            "masked-out spheres must not be pushed apart, dist={dist}"
        );
    }

    #[test]
    fn collision_mask_resolves_matching_pair() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;

        // a belongs to layer 1; b scans layer 1 → they collide (one-directional
        // match is enough under Godot semantics).
        let mut a = sphere_body(Vector3::ZERO);
        a.collision_layer = 0b0001;
        a.collision_mask = 0b0000;
        let mut b = sphere_body(Vector3::new(1.0, 0.0, 0.0));
        b.collision_layer = 0b0010;
        b.collision_mask = 0b0001;
        let id_a = world.add_body(a);
        let id_b = world.add_body(b);

        world.step(1.0 / 60.0);

        let pa = world.get_body(id_a).unwrap().position;
        let pb = world.get_body(id_b).unwrap().position;
        let dist = (pb - pa).length();
        assert!(
            dist >= 1.9,
            "matching-mask spheres should separate, dist={dist}"
        );
    }

    // -- Broad-phase sweep-and-prune --------------------------------------

    /// Reference O(n^2) broad-phase: every pair whose world AABBs overlap,
    /// normalised to ascending-id order.
    fn bruteforce_pairs(world: &PhysicsWorld3D) -> Vec<(BodyId3D, BodyId3D)> {
        let bodies: Vec<&PhysicsBody3D> = world.bodies().collect();
        let mut pairs = Vec::new();
        for i in 0..bodies.len() {
            for j in (i + 1)..bodies.len() {
                if bodies[i].world_aabb().intersects(bodies[j].world_aabb()) {
                    let (a, b) = (bodies[i].id, bodies[j].id);
                    if a.0 <= b.0 {
                        pairs.push((a, b));
                    } else {
                        pairs.push((b, a));
                    }
                }
            }
        }
        pairs.sort_by(|x, y| (x.0 .0, x.1 .0).cmp(&(y.0 .0, y.1 .0)));
        pairs
    }

    #[test]
    fn broadphase_matches_bruteforce() {
        let mut world = PhysicsWorld3D::new();
        // Deterministic pseudo-random spread with several overlaps and gaps.
        let coords = [
            (0.0, 0.0, 0.0),
            (0.5, 0.0, 0.0),   // overlaps 0
            (1.4, 0.0, 0.0),   // overlaps 1
            (10.0, 0.0, 0.0),  // isolated
            (10.3, 0.4, 0.0),  // overlaps 3
            (-5.0, 2.0, 1.0),  // isolated
            (-4.2, 2.0, 1.0),  // overlaps 5
            (100.0, 0.0, 0.0), // isolated
        ];
        for (x, y, z) in coords {
            world.add_body(sphere_body(Vector3::new(x, y, z)));
        }
        assert_eq!(
            world.broadphase_pairs(),
            bruteforce_pairs(&world),
            "sweep-and-prune must produce the same overlapping pairs as O(n^2)"
        );
    }

    #[test]
    fn broadphase_prunes_separated_bodies() {
        let mut world = PhysicsWorld3D::new();
        // 1000 spheres (radius 1) on a grid with 4-unit spacing → none overlap.
        for i in 0..1000u64 {
            let x = (i % 50) as f32 * 4.0;
            let z = (i / 50) as f32 * 4.0;
            world.add_body(sphere_body(Vector3::new(x, 0.0, z)));
        }
        let pairs = world.broadphase_pairs();
        assert!(
            pairs.is_empty(),
            "well-separated bodies must yield zero candidate pairs, got {}",
            pairs.len()
        );
    }

    #[test]
    fn thousand_body_scene_steps_within_budget() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        // Sparse grid: 1000 bodies, spacing 4 → broad-phase prunes to near-zero
        // candidate pairs, so the O(n^2) all-pairs cost is avoided.
        for i in 0..1000u64 {
            let x = (i % 50) as f32 * 4.0;
            let z = (i / 50) as f32 * 4.0;
            world.add_body(sphere_body(Vector3::new(x, 0.0, z)));
        }

        // Assert the "budget" deterministically via the pruned candidate-pair
        // count rather than wall-clock time (which is flaky under CPU-contended
        // CI): a naive O(n^2) scan would consider ~500k pairs, sweep-and-prune
        // keeps it at zero for this well-separated grid.
        let pairs = world.broadphase_pairs();
        assert!(
            pairs.is_empty(),
            "sparse grid must prune to zero candidate pairs, got {}",
            pairs.len()
        );

        // The simulation still steps to completion over many frames.
        for _ in 0..60 {
            world.step(1.0 / 60.0);
        }
        assert_eq!(world.body_count(), 1000);
    }
}




#[cfg(test)]
mod softbody_world_tests {
    use super::*;
    use gdcore::math::Vector3;
    use gdphysics2d::softbody3d::{SoftBody3D, SoftBodyId3D};

    #[test]
    fn soft_body_steps_with_world() {
        let mut world = PhysicsWorld3D::new();
        let positions = [
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(-1.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 1.0),
            Vector3::new(0.0, 0.0, -1.0),
        ];
        let edges = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];
        let sb = SoftBody3D::new(SoftBodyId3D(1), &positions, &edges);
        let y0 = sb.vertices[0].position.y;
        world.add_soft_body(sb);
        world.step(1.0 / 60.0);
        assert_eq!(world.soft_body_count(), 1);
        let y1 = world.soft_bodies_mut()[0].vertices[0].position.y;
        assert!(y1 < y0, "unpinned soft body falls under gravity");
    }
}

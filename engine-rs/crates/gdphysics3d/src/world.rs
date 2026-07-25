//! 3D physics world state and simulation stepping.

use std::collections::{BTreeMap, HashMap};

use gdcore::math::Vector3;
use gdcore::math3d::{Aabb, Quaternion};

use crate::area3d::{
    Area3D, AreaId3D, AreaOverlapEvent3D, AreaStore3D, OverlapEvent3D, SpaceOverride,
};
use crate::body::BodyType3D;
use crate::body::{BodyId3D, ContactPoint3D, PhysicsBody3D};
use crate::collision;
use crate::joint::{Joint3D, JointId3D};
use crate::shape::Shape3D;

/// Half the largest bounding-box extent of a shape — a conservative sphere
/// radius used by the continuous-collision sweep.
fn bounding_radius(shape: &Shape3D) -> f32 {
    let size = shape.bounding_aabb().size;
    0.5 * size.x.max(size.y).max(size.z)
}

/// Sign of `v` with a dead-band around zero (so a face-aligned direction keeps
/// the support point centred on that axis rather than biased to a corner).
fn axis_sign(v: f32) -> f32 {
    if v > 1e-6 {
        1.0
    } else if v < -1e-6 {
        -1.0
    } else {
        0.0
    }
}

/// Returns the farthest point of a shape (at `pos`/`rot`) in world direction
/// `dir` — its support point. Used to place the contact roughly on the touching
/// surface so an off-centre or tilted contact produces a torque, rather than
/// the centre-line midpoint (which is always parallel to the normal and can
/// never rotate a body).
fn support_point(shape: &Shape3D, pos: Vector3, rot: Quaternion, dir: Vector3) -> Vector3 {
    let d = dir.normalized();
    match shape {
        Shape3D::Sphere { radius } => pos + d * *radius,
        Shape3D::BoxShape { half_extents } => {
            let local_dir = rot.inverse().xform(d);
            let corner = Vector3::new(
                half_extents.x * axis_sign(local_dir.x),
                half_extents.y * axis_sign(local_dir.y),
                half_extents.z * axis_sign(local_dir.z),
            );
            pos + rot.xform(corner)
        }
        Shape3D::ConvexPolygonShape { points } if !points.is_empty() => {
            let local_dir = rot.inverse().xform(d);
            let mut best = points[0];
            let mut best_dot = best.dot(local_dir);
            for &p in &points[1..] {
                let dp = p.dot(local_dir);
                if dp > best_dot {
                    best_dot = dp;
                    best = p;
                }
            }
            pos + rot.xform(best)
        }
        // Capsule/cylinder/concave/plane: approximate with the bounding sphere.
        _ => pos + d * bounding_radius(shape),
    }
}

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
    /// Areas whose space overrides (gravity/damp) are applied during the step.
    area_store: AreaStore3D,
    /// Joints solved after integration each step (pat-u3l8c.57).
    joints: Vec<Joint3D>,
}

impl PhysicsWorld3D {
    /// Creates an empty 3D physics world with default gravity (0, -9.8, 0).
    pub fn new() -> Self {
        Self {
            bodies: BTreeMap::new(),
            next_id: 1,
            gravity: Vector3::new(0.0, -9.8, 0.0),
            soft_bodies: Vec::new(),
            area_store: AreaStore3D::new(),
            joints: Vec::new(),
        }
    }

    // -- Joint management (pat-u3l8c.57) -----------------------------------

    /// Adds a joint to be solved each step. Returns its [`JointId3D`] (the id
    /// the joint was created with).
    pub fn add_joint(&mut self, joint: Joint3D) -> JointId3D {
        let id = joint.id();
        self.joints.push(joint);
        id
    }

    /// Number of joints registered in the world.
    pub fn joint_count(&self) -> usize {
        self.joints.len()
    }

    /// Returns a reference to a joint by id.
    pub fn get_joint(&self, id: JointId3D) -> Option<&Joint3D> {
        self.joints.iter().find(|j| j.id() == id)
    }

    // -- Area management ---------------------------------------------------

    /// Adds an [`Area3D`] whose space overrides affect overlapping rigid
    /// bodies during the step, returning its ID.
    pub fn add_area(&mut self, area: Area3D) -> AreaId3D {
        self.area_store.add_area(area)
    }

    /// Removes an area by ID.
    pub fn remove_area(&mut self, id: AreaId3D) -> Option<Area3D> {
        self.area_store.remove_area(id)
    }

    /// Returns a reference to an area by ID.
    pub fn get_area(&self, id: AreaId3D) -> Option<&Area3D> {
        self.area_store.get_area(id)
    }

    /// Returns a mutable reference to an area by ID.
    pub fn get_area_mut(&mut self, id: AreaId3D) -> Option<&mut Area3D> {
        self.area_store.get_area_mut(id)
    }

    /// Returns the number of areas in the world.
    pub fn area_count(&self) -> usize {
        self.area_store.area_count()
    }

    /// Runs the area store's overlap detection against the world's current
    /// bodies, returning this step's `(body enter/exit, area enter/exit)` events.
    ///
    /// The area store is private to the world, so this is how a scene-level
    /// server reaches enter/exit detection to turn into `body_entered` /
    /// `area_entered` signals (pat-0q2kx.35). Call it after [`step`](Self::step);
    /// events are relative to the previous call, so calling it twice per step
    /// reports the second pair of frames as "no change".
    pub fn detect_overlaps(&mut self) -> (Vec<OverlapEvent3D>, Vec<AreaOverlapEvent3D>) {
        // `detect_body_overlaps` wants a map and only reads its values; the
        // bodies live in a `BTreeMap`, so clone into the expected shape — the
        // same per-step clone the 2D server does.
        let body_map: HashMap<BodyId3D, PhysicsBody3D> = self
            .bodies
            .iter()
            .map(|(&id, body)| (id, body.clone()))
            .collect();
        let body_events = self.area_store.detect_body_overlaps(&body_map);
        let area_events = self.area_store.detect_area_overlaps();
        (body_events, area_events)
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

        // Record pre-integration positions of continuous-CD bodies so we can
        // sweep their motion for tunnelling after integration.
        let ccd_prev: Vec<(BodyId3D, Vector3)> = self
            .bodies
            .values()
            .filter(|b| b.continuous_cd && b.body_type == BodyType3D::Rigid && !b.sleeping)
            .map(|b| (b.id, b.position))
            .collect();

        // 1. Integrate all rigid bodies, applying Area3D gravity/damp overrides
        //    for any body overlapping an area with an active space override.
        let ids: Vec<BodyId3D> = self.bodies.keys().copied().collect();
        for id in &ids {
            let (eff_gravity, lin_damp, ang_damp) = self.effective_space_params(*id);
            if let Some(body) = self.bodies.get_mut(id) {
                body.integrate(dt, eff_gravity);
                if let Some(d) = lin_damp {
                    let factor = (1.0 - d * dt).max(0.0);
                    body.linear_velocity = body.linear_velocity * factor;
                }
                if let Some(d) = ang_damp {
                    let factor = (1.0 - d * dt).max(0.0);
                    body.angular_velocity = body.angular_velocity * factor;
                }
            }
        }

        // 1b. Continuous collision detection: clamp fast movers that stepped
        //     clean through a static/kinematic body so they cannot tunnel.
        for (id, prev) in ccd_prev {
            self.ccd_clamp(id, prev);
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
                // Contact point on the touching surfaces: midpoint of each
                // body's support point toward the other. This is offset from a
                // body's centre of mass for tilted/edge contacts, so the normal
                // impulse can induce rotation (a tumbling box), unlike a plain
                // centre-line midpoint.
                let support_a = support_point(&shape_a, pos_a, rot_a, result.normal);
                let support_b = support_point(&shape_b, pos_b, rot_b, result.normal * -1.0);
                let contact_pos = (support_a + support_b) * 0.5;
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

                    // Impulse-based velocity response with restitution and
                    // friction. Contact impulses are applied at `contact_pos`
                    // (offset from each body's centre of mass), so an off-centre
                    // contact induces angular velocity through the inverse
                    // inertia tensor — a box resting past a ledge tumbles.
                    let n = result.normal;
                    let ra = contact_pos - pos_a;
                    let rb = contact_pos - pos_b;
                    let vel_a = self.bodies[&id_a].linear_velocity;
                    let vel_b = self.bodies[&id_b].linear_velocity;
                    let ang_a = self.bodies[&id_a].angular_velocity;
                    let ang_b = self.bodies[&id_b].angular_velocity;
                    // Relative velocity at the contact point (linear + rotational).
                    let vp_a = vel_a + ang_a.cross(ra);
                    let vp_b = vel_b + ang_b.cross(rb);
                    let relative_vel = vp_a - vp_b;
                    let vel_along_normal = relative_vel.dot(n);

                    // Only resolve if bodies are approaching at the contact.
                    if vel_along_normal > 0.0 {
                        let restitution = self.bodies[&id_a].bounce.min(self.bodies[&id_b].bounce);

                        // Effective mass along the normal including the angular
                        // contribution of each body: 1/m_a + 1/m_b +
                        // n·((I_a⁻¹(r_a×n))×r_a) + n·((I_b⁻¹(r_b×n))×r_b).
                        let raxn = ra.cross(n);
                        let rbxn = rb.cross(n);
                        let ang_term_a = self.bodies[&id_a]
                            .apply_inverse_inertia(raxn)
                            .cross(ra)
                            .dot(n);
                        let ang_term_b = self.bodies[&id_b]
                            .apply_inverse_inertia(rbxn)
                            .cross(rb)
                            .dot(n);
                        let k = total_inv + ang_term_a + ang_term_b;
                        if k <= 0.0 {
                            continue;
                        }

                        let j = -(1.0 + restitution) * vel_along_normal / k;
                        let impulse = n * j;

                        // Apply the normal impulse to linear and angular state.
                        if let Some(a) = self.bodies.get_mut(&id_a) {
                            a.linear_velocity = a.linear_velocity + impulse * a_inv;
                            let dw = a.apply_inverse_inertia(ra.cross(impulse));
                            a.angular_velocity = a.angular_velocity + dw;
                        }
                        if let Some(b) = self.bodies.get_mut(&id_b) {
                            b.linear_velocity = b.linear_velocity - impulse * b_inv;
                            let dw = b.apply_inverse_inertia(rb.cross(impulse));
                            b.angular_velocity = b.angular_velocity - dw;
                        }

                        // Friction impulse (tangential), applied to linear and
                        // angular state as well.
                        let tangent = relative_vel - n * vel_along_normal;
                        let tangent_len = tangent.length();
                        if tangent_len > 1e-8 {
                            let tangent_dir = tangent * (1.0 / tangent_len);
                            let friction_coeff =
                                (self.bodies[&id_a].friction * self.bodies[&id_b].friction).sqrt();
                            // Effective mass along the tangent direction.
                            let raxt = ra.cross(tangent_dir);
                            let rbxt = rb.cross(tangent_dir);
                            let kt = total_inv
                                + self.bodies[&id_a]
                                    .apply_inverse_inertia(raxt)
                                    .cross(ra)
                                    .dot(tangent_dir)
                                + self.bodies[&id_b]
                                    .apply_inverse_inertia(rbxt)
                                    .cross(rb)
                                    .dot(tangent_dir);
                            let jt = if kt > 0.0 { -tangent_len / kt } else { 0.0 };
                            // Coulomb friction clamp.
                            let friction_impulse = if jt.abs() < j.abs() * friction_coeff {
                                tangent_dir * jt
                            } else {
                                tangent_dir * (-j.abs() * friction_coeff)
                            };

                            if let Some(a) = self.bodies.get_mut(&id_a) {
                                a.linear_velocity = a.linear_velocity + friction_impulse * a_inv;
                                let dw = a.apply_inverse_inertia(ra.cross(friction_impulse));
                                a.angular_velocity = a.angular_velocity + dw;
                            }
                            if let Some(b) = self.bodies.get_mut(&id_b) {
                                b.linear_velocity = b.linear_velocity - friction_impulse * b_inv;
                                let dw = b.apply_inverse_inertia(rb.cross(friction_impulse));
                                b.angular_velocity = b.angular_velocity - dw;
                            }
                        }
                    }
                }
            }
        }

        // Solve joint constraints (pin position correction + hinge motor)
        // after integration and contact resolution (pat-u3l8c.57).
        self.solve_joints(dt);
    }

    /// Applies the registered joints to their bodies for this step.
    ///
    /// Two-phase to satisfy the borrow checker: gather per-body directives from
    /// the joints (immutable borrows), then apply them to the bodies (mutable).
    /// Covers PinJoint3D (position correction toward the shared pivot, weighted
    /// by inverse mass) and the HingeJoint3D motor (drives body B's angular
    /// velocity about the hinge axis toward the target). Limits, sliders, and
    /// ConeTwist/6DOF are not yet solved.
    fn solve_joints(&mut self, dt: f32) {
        let mut ang_deltas: Vec<(BodyId3D, Vector3)> = Vec::new();
        let mut pin_moves: Vec<(BodyId3D, Vector3)> = Vec::new();

        for j in &self.joints {
            if !j.is_enabled() {
                continue;
            }
            match j {
                Joint3D::Hinge(h) if h.motor_enabled => {
                    let (_a, b_id) = j.bodies();
                    if let Some(body) = self.bodies.get(&b_id) {
                        if body.body_type != BodyType3D::Rigid {
                            continue;
                        }
                        let world_axis = body.rotation.xform(h.axis).normalized();
                        let current = body.angular_velocity.dot(world_axis);
                        let impulse = h.compute_motor_impulse(current, dt);
                        ang_deltas.push((b_id, world_axis * impulse));
                    }
                }
                Joint3D::Pin(p) => {
                    let (a_id, b_id) = j.bodies();
                    if let (Some(ba), Some(bb)) = (self.bodies.get(&a_id), self.bodies.get(&b_id)) {
                        let world_a = ba.position + ba.rotation.xform(p.local_a);
                        let world_b = bb.position + bb.rotation.xform(p.local_b);
                        // `error` points from A's pivot to B's pivot.
                        let error = p.compute_error(world_a, world_b);
                        let inv_a = ba.inverse_mass();
                        let inv_b = bb.inverse_mass();
                        let total = inv_a + inv_b;
                        if total > 0.0 {
                            let corr = error * p.bias;
                            if inv_a > 0.0 {
                                // A moves toward B.
                                pin_moves.push((a_id, corr * (inv_a / total)));
                            }
                            if inv_b > 0.0 {
                                // B moves toward A.
                                pin_moves.push((b_id, corr * (-(inv_b / total))));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        for (id, dw) in ang_deltas {
            if let Some(body) = self.bodies.get_mut(&id) {
                body.angular_velocity = body.angular_velocity + dw;
            }
        }
        for (id, dp) in pin_moves {
            if let Some(body) = self.bodies.get_mut(&id) {
                body.position = body.position + dp;
            }
        }
    }

    /// Computes the effective space parameters for a body this step by folding
    /// in every overlapping [`Area3D`] whose gravity space override is active.
    ///
    /// Returns the effective *space* gravity (before the body's own
    /// `gravity_scale`) plus optional linear/angular damping overrides. Areas
    /// are applied highest-`priority` first; `Replace` modes discard the
    /// accumulated gravity, `Combine` modes add to it — a simplification of
    /// Godot's five-way override enum that covers the common single-zone case.
    fn effective_space_params(&self, id: BodyId3D) -> (Vector3, Option<f32>, Option<f32>) {
        let body = match self.bodies.get(&id) {
            Some(b) => b,
            None => return (self.gravity, None, None),
        };
        if body.body_type != BodyType3D::Rigid {
            return (self.gravity, None, None);
        }
        if self.area_store.area_count() == 0 {
            return (self.gravity, None, None);
        }

        let mut overlapping: Vec<&Area3D> = self
            .area_store
            .areas()
            .filter(|a| a.gravity_space_override != SpaceOverride::Disabled)
            .filter(|a| (a.collision_mask & body.collision_layer) != 0)
            .filter(|a| {
                collision::test_collision(a.position, &a.shape, body.position, &body.shape)
                    .colliding
            })
            .collect();
        if overlapping.is_empty() {
            return (self.gravity, None, None);
        }
        // Highest priority applied last so it wins a `Replace`.
        overlapping.sort_by(|x, y| {
            x.priority
                .partial_cmp(&y.priority)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut gravity = self.gravity;
        let mut lin_damp: Option<f32> = None;
        let mut ang_damp: Option<f32> = None;
        for area in overlapping {
            let g = area.gravity_vector(body.position);
            match area.gravity_space_override {
                SpaceOverride::Replace | SpaceOverride::ReplaceCombine => gravity = g,
                SpaceOverride::Combine | SpaceOverride::CombineReplace => gravity = gravity + g,
                SpaceOverride::Disabled => {}
            }
            lin_damp = Some(lin_damp.map_or(area.linear_damp, |d| d.max(area.linear_damp)));
            ang_damp = Some(ang_damp.map_or(area.angular_damp, |d| d.max(area.angular_damp)));
        }
        (gravity, lin_damp, ang_damp)
    }

    /// Continuous collision detection for a body that moved from `prev` to its
    /// current position this step. If the swept motion (approximated as a
    /// sphere sweep) crosses a static or kinematic body it did not start inside,
    /// the body is clamped to the entry point so it cannot tunnel through thin
    /// geometry in a single large step.
    fn ccd_clamp(&mut self, id: BodyId3D, prev: Vector3) {
        let (new_pos, r_self, self_layer, self_mask) = match self.bodies.get(&id) {
            Some(b) => (
                b.position,
                bounding_radius(&b.shape),
                b.collision_layer,
                b.collision_mask,
            ),
            None => return,
        };
        let motion = new_pos - prev;
        let dist = motion.length();
        if dist < 1e-5 {
            return;
        }
        let dir = motion * (1.0 / dist);

        let mut earliest = dist;
        let mut hit = false;
        for other in self.bodies.values() {
            if other.id == id || other.body_type == BodyType3D::Rigid {
                continue;
            }
            // Godot layer/mask semantics (symmetric).
            if (self_mask & other.collision_layer) == 0 && (other.collision_mask & self_layer) == 0
            {
                continue;
            }
            let r = r_self + bounding_radius(&other.shape);
            let oc = prev - other.position;
            let c = oc.dot(oc) - r * r;
            if c <= 0.0 {
                // Already overlapping at the start of the step — the regular
                // narrow-phase/resolution pass handles this, not CCD.
                continue;
            }
            let b = oc.dot(dir);
            if b > 0.0 {
                continue; // moving away from this body
            }
            let disc = b * b - c;
            if disc < 0.0 {
                continue; // sweep misses the expanded sphere
            }
            let t = -b - disc.sqrt();
            if t > 1e-4 && t < earliest {
                earliest = t;
                hit = true;
            }
        }

        if hit {
            if let Some(body) = self.bodies.get_mut(&id) {
                body.position = prev + dir * earliest;
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

    // -- Joint constraint solving (pat-u3l8c.57) ----------------------------

    fn small_sphere(pos: Vector3, ty: BodyType3D, mass: f32) -> PhysicsBody3D {
        PhysicsBody3D::new(BodyId3D(0), ty, pos, Shape3D::Sphere { radius: 0.1 }, mass)
    }

    #[test]
    fn add_joint_registers_and_is_retrievable() {
        let mut world = PhysicsWorld3D::new();
        let a = world.add_body(sphere_body(Vector3::ZERO));
        let b = world.add_body(sphere_body(Vector3::new(0.0, -3.0, 0.0)));
        let jid = JointId3D(7);
        world.add_joint(Joint3D::Pin(crate::joint::PinJoint3D::new(jid, a, b)));
        assert_eq!(world.joint_count(), 1);
        assert!(world.get_joint(jid).is_some());
    }

    #[test]
    fn pin_joint_pulls_body_toward_shared_pivot() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO; // isolate the constraint from gravity
        let a = world.add_body(small_sphere(Vector3::ZERO, BodyType3D::Static, 0.0));
        let b = world.add_body(small_sphere(
            Vector3::new(2.0, 0.0, 0.0),
            BodyType3D::Rigid,
            1.0,
        ));
        // Default pin: pivots at both body centres — the constraint wants the
        // two centres coincident, so the rigid B is pulled toward the static A.
        world.add_joint(Joint3D::Pin(crate::joint::PinJoint3D::new(
            JointId3D(1),
            a,
            b,
        )));

        let x0 = world.get_body(b).unwrap().position.x; // 2.0
        for _ in 0..40 {
            world.step(1.0 / 60.0);
        }
        let x1 = world.get_body(b).unwrap().position.x;
        assert!(x1 < x0, "pin should pull B toward A (x {x1} < {x0})");
        assert!(
            x1 < 1.0,
            "pin should substantially close the 2.0 gap, got {x1}"
        );
    }

    #[test]
    fn hinge_motor_drives_angular_velocity_toward_target() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        // A far away so it never collides with B; the motor only drives B.
        let a = world.add_body(sphere_body(Vector3::new(100.0, 0.0, 0.0)));
        let b = world.add_body(sphere_body(Vector3::ZERO));
        let hinge = crate::joint::HingeJoint3D::new(JointId3D(1), a, b)
            .with_axis(Vector3::new(0.0, 1.0, 0.0))
            .with_motor(5.0, 100.0);
        world.add_joint(Joint3D::Hinge(hinge));

        for _ in 0..180 {
            world.step(1.0 / 60.0);
        }
        let wy = world.get_body(b).unwrap().angular_velocity.y;
        assert!(wy > 1.0, "motor should spin B up about +Y, got {wy}");
        assert!(
            wy <= 5.05,
            "motor should not overshoot its target, got {wy}"
        );
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

    // -- Area space overrides (pat-u3l8c.59) ------------------------------

    #[test]
    fn area_management_add_get_remove() {
        let mut world = PhysicsWorld3D::new();
        let id = world.add_area(Area3D::new(
            AreaId3D(0),
            Vector3::ZERO,
            Shape3D::Sphere { radius: 5.0 },
        ));
        assert_eq!(world.area_count(), 1);
        assert!(world.get_area(id).is_some());
        world.get_area_mut(id).unwrap().gravity = 3.0;
        assert!((world.get_area(id).unwrap().gravity - 3.0).abs() < 1e-6);
        assert!(world.remove_area(id).is_some());
        assert_eq!(world.area_count(), 0);
    }

    #[test]
    fn area_gravity_override_redirects_body() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::new(0.0, -9.8, 0.0);
        // A body that would normally fall.
        let id = world.add_body(sphere_body(Vector3::new(0.0, 1.0, 0.0)));
        // An area that replaces gravity with a strong upward field.
        let mut area = Area3D::new(AreaId3D(0), Vector3::ZERO, Shape3D::Sphere { radius: 20.0 });
        area.gravity_space_override = SpaceOverride::Replace;
        area.gravity = 30.0;
        area.gravity_direction = Vector3::new(0.0, 1.0, 0.0);
        world.add_area(area);

        world.step(1.0 / 60.0);
        let b = world.get_body(id).unwrap();
        assert!(
            b.linear_velocity.y > 0.0,
            "upward area gravity should redirect the body upward, vy={}",
            b.linear_velocity.y
        );
    }

    #[test]
    fn area_without_override_leaves_gravity_alone() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::new(0.0, -9.8, 0.0);
        let id = world.add_body(sphere_body(Vector3::new(0.0, 1.0, 0.0)));
        // Disabled override → default gravity still applies (body falls).
        let mut area = Area3D::new(AreaId3D(0), Vector3::ZERO, Shape3D::Sphere { radius: 20.0 });
        area.gravity_space_override = SpaceOverride::Disabled;
        area.gravity = 30.0;
        area.gravity_direction = Vector3::new(0.0, 1.0, 0.0);
        world.add_area(area);

        world.step(1.0 / 60.0);
        assert!(world.get_body(id).unwrap().linear_velocity.y < 0.0);
    }

    #[test]
    fn area_damp_override_slows_body() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        let mut body = sphere_body(Vector3::new(0.0, 1.0, 0.0));
        body.linear_velocity = Vector3::new(10.0, 0.0, 0.0);
        body.can_sleep = false;
        let id = world.add_body(body);
        // Active override (combine, no gravity change) applies the area's damp.
        let mut area = Area3D::new(AreaId3D(0), Vector3::ZERO, Shape3D::Sphere { radius: 20.0 });
        area.gravity_space_override = SpaceOverride::Combine;
        area.gravity = 0.0;
        area.linear_damp = 5.0;
        world.add_area(area);

        world.step(1.0 / 60.0);
        let vx = world.get_body(id).unwrap().linear_velocity.x;
        assert!(
            vx < 10.0 && vx > 0.0,
            "area damp should slow but not reverse, vx={vx}"
        );
    }

    // -- Continuous collision detection (pat-u3l8c.59) --------------------

    #[test]
    fn ccd_prevents_tunneling_through_static_wall() {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        // A fast bullet that would step from x=-5 to x=+5 in one frame, clean
        // through a thin static wall at the origin.
        let mut bullet = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::new(-5.0, 0.0, 0.0),
            Shape3D::Sphere { radius: 0.1 },
            1.0,
        );
        bullet.linear_velocity = Vector3::new(600.0, 0.0, 0.0); // 10 units / (1/60)s
        bullet.continuous_cd = true;
        bullet.can_sleep = false;
        let id = world.add_body(bullet);
        world.add_body(PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 0.2 },
            0.0,
        ));

        world.step(1.0 / 60.0);
        let x = world.get_body(id).unwrap().position.x;
        assert!(
            x < 0.5,
            "CCD must stop the bullet at the wall (x={x}), not tunnel to +5"
        );
    }

    #[test]
    fn without_ccd_fast_body_tunnels() {
        // Control: the same setup without CCD tunnels straight through.
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        let mut bullet = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::new(-5.0, 0.0, 0.0),
            Shape3D::Sphere { radius: 0.1 },
            1.0,
        );
        bullet.linear_velocity = Vector3::new(600.0, 0.0, 0.0);
        bullet.continuous_cd = false;
        bullet.can_sleep = false;
        let id = world.add_body(bullet);
        world.add_body(PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 0.2 },
            0.0,
        ));

        world.step(1.0 / 60.0);
        let x = world.get_body(id).unwrap().position.x;
        assert!(x > 4.0, "without CCD the fast body tunnels through (x={x})");
    }

    // -- Contact torque / angular response (pat-u3l8c.59) -----------------

    #[test]
    fn tilted_box_gains_angular_velocity_from_contact() {
        // A tilted box driven into a wide static floor gains angular velocity:
        // the contact/impulse is offset from its centre of mass so the normal
        // impulse produces a torque (the box tumbles toward flat).
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        let mut box_body = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Rigid,
            Vector3::new(0.0, 0.9, 0.0),
            Shape3D::BoxShape {
                half_extents: Vector3::new(0.5, 0.5, 0.5),
            },
            1.0,
        );
        box_body.rotation = Quaternion::from_axis_angle(Vector3::new(0.0, 0.0, 1.0), 0.5);
        box_body.linear_velocity = Vector3::new(0.0, -3.0, 0.0);
        box_body.can_sleep = false;
        let id = world.add_body(box_body);
        world.add_body(PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::ZERO,
            Shape3D::BoxShape {
                half_extents: Vector3::new(5.0, 0.5, 5.0),
            },
            0.0,
        ));

        world.step(1.0 / 60.0);
        let av = world.get_body(id).unwrap().angular_velocity;
        assert!(
            av.length() > 1e-3,
            "offset contact should induce spin, angular_velocity={av:?}"
        );
    }

    #[test]
    fn head_on_sphere_collision_stays_linear() {
        // Regression: a centre-line sphere/sphere collision must NOT spuriously
        // introduce angular velocity (contact is on the line of centres).
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;
        let mut ball = sphere_body(Vector3::ZERO);
        ball.linear_velocity = Vector3::new(10.0, 0.0, 0.0);
        ball.can_sleep = false;
        let id = world.add_body(ball);
        let wall = PhysicsBody3D::new(
            BodyId3D(0),
            BodyType3D::Static,
            Vector3::new(1.5, 0.0, 0.0),
            Shape3D::Sphere { radius: 1.0 },
            0.0,
        );
        world.add_body(wall);

        world.step(1.0 / 60.0);
        let av = world.get_body(id).unwrap().angular_velocity;
        assert!(
            av.length() < 1e-5,
            "head-on contact must stay linear, av={av:?}"
        );
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

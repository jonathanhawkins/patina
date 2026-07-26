//! Iterative contact solver with manifolds (pat-0q2kx.116).
//!
//! Godot ground truth: `upstream/godot/modules/godot_physics_2d/godot_body_pair_2d.cpp`.
//!
//! The world used to resolve each colliding pair exactly once, from a single
//! contact point, with an arithmetic-mean friction and no impulse accumulation
//! ([`crate::collision::separate_bodies`]). That is enough to stop one box on
//! one floor and not much else: a second box stacked on the first re-opens the
//! first contact every time the second one is solved, so a stack shivers and
//! sinks.
//!
//! This module is the real thing, following Godot's structure:
//!
//! - **Manifolds.** A box resting on a floor touches along an *edge*, not a
//!   point. [`build_manifold`] clips the incident edge against the reference
//!   edge to recover up to two contact points, which is what stops a stacked
//!   box from rocking between its corners.
//! - **Accumulated impulses.** Each point carries `acc_normal` / `acc_tangent`
//!   across the iteration sweep, clamped (normal ≥ 0, tangent to the Coulomb
//!   cone). Accumulating is what lets several passes converge instead of each
//!   pass undoing the last.
//! - **Godot's combine rules.** `bounce = clamp(a + b, 0, 1)` and
//!   `friction = |min(a, b)|` (`godot_body_pair_2d.cpp:242-248`), not the
//!   `max`/mean the old path used.
//! - **A restitution threshold.** Bounce is only applied to the pre-solve
//!   closing speed, and dropped under [`RESTITUTION_VELOCITY_THRESHOLD`], so a
//!   resting body does not jitter itself apart on its own settling velocity.
//! - **A separate bias channel.** Penetration is pushed out through a
//!   *pseudo*-velocity that moves the transform but is discarded afterwards, so
//!   depenetration adds no kinetic energy (Godot's `biased_linear_velocity`).
//! - **Surface velocity.** Each body's `constant_linear_velocity` /
//!   `constant_angular_velocity` is folded into the tangential relative
//!   velocity, so a conveyor drags what rests on it.

use std::collections::HashMap;

use gdcore::math::{Transform2D, Vector2};

use crate::body::{BodyId, BodyType, PhysicsBody2D};
use crate::collision::{world_vertices, CollisionResult};
use crate::shape::Shape2D;

// ---------------------------------------------------------------------------
// Godot ProjectSettings defaults (servers/physics_2d/physics_server_2d.cpp)
// ---------------------------------------------------------------------------

/// `physics/2d/solver/solver_iterations`.
pub const DEFAULT_SOLVER_ITERATIONS: usize = 16;
/// `physics/2d/solver/default_contact_bias`.
pub const DEFAULT_CONTACT_BIAS: f32 = 0.8;
/// `physics/2d/solver/contact_max_allowed_penetration` — the slop a contact is
/// allowed to keep. Correcting penetration all the way to zero makes resting
/// contacts oscillate, so the last fraction of a pixel is left alone.
pub const DEFAULT_MAX_PENETRATION: f32 = 0.3;

/// Closing speed under which restitution is dropped entirely.
///
/// Without it a body at rest keeps converting its one-tick settling velocity
/// back into an upward hop and never sleeps.
pub const RESTITUTION_VELOCITY_THRESHOLD: f32 = 1.0;

/// Godot `combine_bounce`: `CLAMP(A + B, 0, 1)` (`godot_body_pair_2d.cpp:243`).
pub fn combine_bounce(a: f32, b: f32) -> f32 {
    (a + b).clamp(0.0, 1.0)
}

/// Godot `combine_friction`: `abs(MIN(A, B))` (`godot_body_pair_2d.cpp:247`).
///
/// The *minimum*, not the mean: one slippery surface is enough to make a pair
/// slippery, which is why ice under a rubber crate slides.
pub fn combine_friction(a: f32, b: f32) -> f32 {
    a.min(b).abs()
}

// ---------------------------------------------------------------------------
// Manifold generation
// ---------------------------------------------------------------------------

/// Up to two world-space contact points sharing one normal and depth.
#[derive(Debug, Clone)]
pub struct ContactManifold2D {
    /// Contact normal, pointing from A toward B (same convention as
    /// [`CollisionResult::normal`]).
    pub normal: Vector2,
    /// One point for curved/degenerate contacts, two for face-on-face.
    pub points: Vec<Vector2>,
    /// Penetration depth along `normal`.
    pub depth: f32,
}

/// The edge of a convex polygon whose outward normal is most aligned with
/// `dir`, as `(start, end, outward_normal, alignment)`.
///
/// Normals are oriented outward using the centroid, matching
/// `collision::nearest_edge_outward` — the vertex lists here come in either
/// winding order, so a winding assumption would silently invert half of them.
fn best_edge(verts: &[Vector2], dir: Vector2) -> Option<(Vector2, Vector2, Vector2, f32)> {
    let n = verts.len();
    if n < 2 {
        return None;
    }
    let center = verts.iter().fold(Vector2::ZERO, |acc, v| acc + *v) * (1.0 / n as f32);

    let mut best: Option<(Vector2, Vector2, Vector2, f32)> = None;
    // A 2-vertex "polygon" (a segment) has one edge, not two.
    let edge_count = if n == 2 { 1 } else { n };
    for i in 0..edge_count {
        let a = verts[i];
        let b = verts[(i + 1) % n];
        let edge = b - a;
        let len = edge.length();
        if len < 1e-9 {
            continue;
        }
        let mut normal = Vector2::new(-edge.y, edge.x) * (1.0 / len);
        if (a - center).dot(normal) < 0.0 {
            normal = -normal;
        }
        let alignment = normal.dot(dir);
        if best.is_none() || alignment > best.unwrap().3 {
            best = Some((a, b, normal, alignment));
        }
    }
    best
}

/// Clips the segment `[p0, p1]` to the half-space `dot(p, n) >= offset`.
/// Returns `None` when the segment lies entirely outside it.
fn clip_segment(p0: Vector2, p1: Vector2, n: Vector2, offset: f32) -> Option<(Vector2, Vector2)> {
    let d0 = n.dot(p0) - offset;
    let d1 = n.dot(p1) - offset;
    if d0 >= 0.0 && d1 >= 0.0 {
        return Some((p0, p1));
    }
    if d0 < 0.0 && d1 < 0.0 {
        return None;
    }
    let denom = d0 - d1;
    if denom.abs() < 1e-9 {
        return Some((p0, p1));
    }
    let crossing = p0 + (p1 - p0) * (d0 / denom);
    if d0 >= 0.0 {
        Some((p0, crossing))
    } else {
        Some((crossing, p1))
    }
}

/// Expands a narrow-phase hit into a contact manifold.
///
/// Falls back to the single point the narrow phase reported whenever a
/// two-point manifold is not meaningful: anything with a rounding radius
/// (circles, capsules) touches at one point, and half-plane/concave shapes have
/// no finite vertex list to clip against.
pub fn build_manifold(
    shape_a: &Shape2D,
    tf_a: &Transform2D,
    shape_b: &Shape2D,
    tf_b: &Transform2D,
    result: &CollisionResult,
) -> ContactManifold2D {
    let single = ContactManifold2D {
        normal: result.normal,
        points: vec![result.point],
        depth: result.depth,
    };

    let (Some((verts_a, radius_a)), Some((verts_b, radius_b))) =
        (world_vertices(shape_a, tf_a), world_vertices(shape_b, tf_b))
    else {
        return single;
    };
    if radius_a > 0.0 || radius_b > 0.0 || verts_a.len() < 2 || verts_b.len() < 2 {
        return single;
    }

    let normal = result.normal;
    let (Some(edge_a), Some(edge_b)) = (best_edge(&verts_a, normal), best_edge(&verts_b, -normal))
    else {
        return single;
    };

    // The face that is more square-on to the normal is the reference; the other
    // is clipped against it. Picking the wrong one puts both contact points on
    // a single corner and the manifold degenerates back to a point.
    let use_b_as_reference = edge_b.3 > edge_a.3;
    let (ref_start, ref_end, ref_normal) = if use_b_as_reference {
        (edge_b.0, edge_b.1, edge_b.2)
    } else {
        (edge_a.0, edge_a.1, edge_a.2)
    };
    let (inc_start, inc_end) = if use_b_as_reference {
        (edge_a.0, edge_a.1)
    } else {
        (edge_b.0, edge_b.1)
    };

    let ref_dir = (ref_end - ref_start).normalized();
    if ref_dir.length_squared() < 0.5 {
        return single;
    }

    // Clip the incident edge to the reference face's two side planes.
    let Some((c0, c1)) = clip_segment(inc_start, inc_end, ref_dir, ref_dir.dot(ref_start)) else {
        return single;
    };
    let Some((c0, c1)) = clip_segment(c0, c1, -ref_dir, (-ref_dir).dot(ref_end)) else {
        return single;
    };

    // Keep only the clipped points that are actually behind the reference face.
    let plane_offset = ref_normal.dot(ref_start);
    let mut points = Vec::with_capacity(2);
    for p in [c0, c1] {
        if ref_normal.dot(p) - plane_offset <= 1e-4 {
            points.push(p);
        }
    }
    if points.len() == 2 && (points[0] - points[1]).length_squared() < 1e-6 {
        points.pop();
    }
    if points.is_empty() {
        return single;
    }

    ContactManifold2D {
        normal,
        points,
        depth: result.depth,
    }
}

// ---------------------------------------------------------------------------
// Constraints
// ---------------------------------------------------------------------------

/// One solved contact point.
#[derive(Debug, Clone, Copy)]
struct ContactPoint2D {
    /// Offset from A's centre of mass to the contact.
    r_a: Vector2,
    /// Offset from B's centre of mass to the contact.
    r_b: Vector2,
    /// `1 / k` along the normal, including both bodies' angular terms.
    mass_normal: f32,
    /// `1 / k` along the tangent.
    mass_tangent: f32,
    /// Penetration push-out target for the bias channel, in units per second.
    bias: f32,
    /// Restitution target, computed once from the pre-solve closing speed.
    bounce: f32,
    /// The pair's constant surface motion, `(surface_b − surface_a) · tangent`.
    /// Added to the rigid relative tangential velocity, because stiction means
    /// the *surfaces* are at rest relative to each other, not the centres.
    surface_tangent: f32,
    acc_normal: f32,
    acc_tangent: f32,
    acc_bias: f32,
}

/// Every contact between one pair of bodies for this step.
#[derive(Debug, Clone)]
pub struct ContactConstraint2D {
    /// First body.
    pub body_a: BodyId,
    /// Second body.
    pub body_b: BodyId,
    /// Shared contact normal, A toward B.
    pub normal: Vector2,
    /// Combined friction coefficient for the pair.
    pub friction: f32,
    points: Vec<ContactPoint2D>,
}

impl ContactConstraint2D {
    /// Builds a constraint from a manifold and the two bodies it touches.
    ///
    /// Returns `None` when neither body can move — two static bodies overlapping
    /// is a level-design problem, not a constraint.
    pub fn new(
        a: &PhysicsBody2D,
        b: &PhysicsBody2D,
        manifold: &ContactManifold2D,
        dt: f32,
        contact_bias: f32,
        max_penetration: f32,
    ) -> Option<Self> {
        let inv_mass_a = a.inverse_mass();
        let inv_mass_b = b.inverse_mass();
        let inv_inertia_a = a.inverse_inertia();
        let inv_inertia_b = b.inverse_inertia();
        if inv_mass_a + inv_mass_b <= 0.0 {
            return None;
        }
        let inv_dt = if dt > 1e-6 { 1.0 / dt } else { 0.0 };

        let normal = manifold.normal;
        let tangent = Vector2::new(-normal.y, normal.x);
        let com_a = a.position + a.center_of_mass;
        let com_b = b.position + b.center_of_mass;

        let mut points = Vec::with_capacity(manifold.points.len());
        for &world_point in &manifold.points {
            let r_a = world_point - com_a;
            let r_b = world_point - com_b;

            let rn_a = r_a.dot(normal);
            let rn_b = r_b.dot(normal);
            let k_normal = inv_mass_a
                + inv_mass_b
                + inv_inertia_a * (r_a.dot(r_a) - rn_a * rn_a)
                + inv_inertia_b * (r_b.dot(r_b) - rn_b * rn_b);

            let rt_a = r_a.dot(tangent);
            let rt_b = r_b.dot(tangent);
            let k_tangent = inv_mass_a
                + inv_mass_b
                + inv_inertia_a * (r_a.dot(r_a) - rt_a * rt_a)
                + inv_inertia_b * (r_b.dot(r_b) - rt_b * rt_b);

            if k_normal <= 1e-9 || k_tangent <= 1e-9 {
                continue;
            }

            // Godot: `-bias * inv_dt * MIN(0, -depth + max_penetration)`, i.e.
            // nothing at all until the overlap exceeds the allowed slop.
            let bias = -contact_bias * inv_dt * (max_penetration - manifold.depth).min(0.0);

            // Restitution is read off the *approach* velocity, before any
            // impulse has been applied, and only when the approach is fast
            // enough to be a real bounce.
            let approach = relative_velocity_at(a, b, r_a, r_b).dot(normal);
            let bounce = if approach < -RESTITUTION_VELOCITY_THRESHOLD {
                combine_bounce(a.bounce, b.bounce) * approach
            } else {
                0.0
            };

            // The conveyor term: the surface each body presents at the contact,
            // projected onto the tangent. `constant_linear_velocity` does not
            // move the body, so a solver reading rigid velocities alone sees a
            // stationary platform and drags nothing.
            let surface_tangent = (b.surface_velocity_at(world_point)
                - a.surface_velocity_at(world_point))
            .dot(tangent);

            points.push(ContactPoint2D {
                r_a,
                r_b,
                mass_normal: 1.0 / k_normal,
                mass_tangent: 1.0 / k_tangent,
                bias,
                bounce,
                surface_tangent,
                acc_normal: 0.0,
                acc_tangent: 0.0,
                acc_bias: 0.0,
            });
        }

        if points.is_empty() {
            return None;
        }
        Some(Self {
            body_a: a.id,
            body_b: b.id,
            normal,
            friction: combine_friction(a.friction, b.friction),
            points,
        })
    }
}

/// Relative velocity of B's contact material point with respect to A's.
fn relative_velocity_at(
    a: &PhysicsBody2D,
    b: &PhysicsBody2D,
    r_a: Vector2,
    r_b: Vector2,
) -> Vector2 {
    let va = a.linear_velocity + Vector2::new(-r_a.y, r_a.x) * a.angular_velocity;
    let vb = b.linear_velocity + Vector2::new(-r_b.y, r_b.x) * b.angular_velocity;
    vb - va
}

// ---------------------------------------------------------------------------
// Solver
// ---------------------------------------------------------------------------

/// A body's mutable solver state, snapshotted so both ends of a constraint can
/// be read and written without holding two `&mut` into the body map.
#[derive(Clone, Copy)]
struct SolverBody2D {
    linear: Vector2,
    angular: f32,
    /// Pseudo-velocity used only for depenetration; moves the transform, then
    /// is discarded, so pushing bodies apart adds no energy.
    bias_linear: Vector2,
    bias_angular: f32,
    inv_mass: f32,
    inv_inertia: f32,
    movable: bool,
}

impl SolverBody2D {
    fn of(body: &PhysicsBody2D) -> Self {
        Self {
            linear: body.linear_velocity,
            angular: body.angular_velocity,
            bias_linear: Vector2::ZERO,
            bias_angular: 0.0,
            inv_mass: body.inverse_mass(),
            inv_inertia: body.inverse_inertia(),
            movable: body.body_type == BodyType::Rigid,
        }
    }

    fn velocity_at(&self, r: Vector2) -> Vector2 {
        self.linear + Vector2::new(-r.y, r.x) * self.angular
    }

    fn bias_velocity_at(&self, r: Vector2) -> Vector2 {
        self.bias_linear + Vector2::new(-r.y, r.x) * self.bias_angular
    }

    fn apply_impulse(&mut self, r: Vector2, impulse: Vector2) {
        self.linear = self.linear + impulse * self.inv_mass;
        self.angular += (r.x * impulse.y - r.y * impulse.x) * self.inv_inertia;
    }

    fn apply_bias_impulse(&mut self, r: Vector2, impulse: Vector2) {
        self.bias_linear = self.bias_linear + impulse * self.inv_mass;
        self.bias_angular += (r.x * impulse.y - r.y * impulse.x) * self.inv_inertia;
    }
}

/// The iterated contact solver.
#[derive(Debug, Clone, Copy)]
pub struct ContactSolver2D {
    /// Gauss-Seidel sweeps per step (`physics/2d/solver/solver_iterations`).
    pub iterations: usize,
    /// Fraction of the excess penetration corrected per step
    /// (`physics/2d/solver/default_contact_bias`).
    pub contact_bias: f32,
    /// Overlap left uncorrected (`solver/contact_max_allowed_penetration`).
    pub max_penetration: f32,
}

impl Default for ContactSolver2D {
    fn default() -> Self {
        Self {
            iterations: DEFAULT_SOLVER_ITERATIONS,
            contact_bias: DEFAULT_CONTACT_BIAS,
            max_penetration: DEFAULT_MAX_PENETRATION,
        }
    }
}

impl ContactSolver2D {
    /// Solves every constraint, writing the resulting velocities back onto the
    /// bodies and returning the per-body depenetration pseudo-velocity.
    ///
    /// The caller applies that pseudo-velocity to the transform during position
    /// integration and then drops it — it must never persist into the next
    /// step, or depenetration turns into propulsion.
    pub fn solve(
        &self,
        bodies: &mut HashMap<BodyId, PhysicsBody2D>,
        constraints: &mut [ContactConstraint2D],
    ) -> HashMap<BodyId, (Vector2, f32)> {
        let mut states: HashMap<BodyId, SolverBody2D> = HashMap::new();
        for c in constraints.iter() {
            for id in [c.body_a, c.body_b] {
                if let Some(body) = bodies.get(&id) {
                    states.entry(id).or_insert_with(|| SolverBody2D::of(body));
                }
            }
        }

        for _ in 0..self.iterations.max(1) {
            for c in constraints.iter_mut() {
                if c.body_a == c.body_b {
                    continue;
                }
                let (Some(mut a), Some(mut b)) = (
                    states.get(&c.body_a).copied(),
                    states.get(&c.body_b).copied(),
                ) else {
                    continue;
                };
                let tangent = Vector2::new(-c.normal.y, c.normal.x);

                for point in c.points.iter_mut() {
                    // -- depenetration, on its own velocity channel ------------
                    let bias_relative = (b.bias_velocity_at(point.r_b)
                        - a.bias_velocity_at(point.r_a))
                    .dot(c.normal);
                    let jb = (point.bias - bias_relative) * point.mass_normal;
                    let old_bias = point.acc_bias;
                    point.acc_bias = (old_bias + jb).max(0.0);
                    let bias_impulse = c.normal * (point.acc_bias - old_bias);
                    a.apply_bias_impulse(point.r_a, -bias_impulse);
                    b.apply_bias_impulse(point.r_b, bias_impulse);

                    // -- normal ------------------------------------------------
                    let relative = b.velocity_at(point.r_b) - a.velocity_at(point.r_a);
                    let vn = relative.dot(c.normal);
                    let jn = -(point.bounce + vn) * point.mass_normal;
                    let old_normal = point.acc_normal;
                    // Clamping the *accumulated* impulse, not the increment, is
                    // what makes repeated sweeps converge: a later pass may take
                    // impulse back out, it just may never end up pulling.
                    point.acc_normal = (old_normal + jn).max(0.0);

                    // -- friction ----------------------------------------------
                    let vt = relative.dot(tangent) + point.surface_tangent;
                    let jt = -vt * point.mass_tangent;
                    let max_tangent = c.friction * point.acc_normal;
                    let old_tangent = point.acc_tangent;
                    point.acc_tangent = (old_tangent + jt).clamp(-max_tangent, max_tangent);

                    let impulse = c.normal * (point.acc_normal - old_normal)
                        + tangent * (point.acc_tangent - old_tangent);
                    a.apply_impulse(point.r_a, -impulse);
                    b.apply_impulse(point.r_b, impulse);
                }

                states.insert(c.body_a, a);
                states.insert(c.body_b, b);
            }
        }

        let mut bias_velocities = HashMap::new();
        for (id, state) in states {
            if !state.movable {
                continue;
            }
            if let Some(body) = bodies.get_mut(&id) {
                body.linear_velocity = state.linear;
                body.angular_velocity = state.angular;
            }
            bias_velocities.insert(id, (state.bias_linear, state.bias_angular));
        }
        bias_velocities
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_body(
        id: u64,
        ty: BodyType,
        position: Vector2,
        half: Vector2,
        mass: f32,
    ) -> PhysicsBody2D {
        PhysicsBody2D::new(
            BodyId(id),
            ty,
            position,
            Shape2D::Rectangle { half_extents: half },
            mass,
        )
    }

    /// A thin floor spanning `y ∈ [92, 108]` and a 32 px box overlapping its top
    /// surface by 0.5 px — the geometry a resting contact actually has.
    fn floor_and_box() -> (PhysicsBody2D, PhysicsBody2D) {
        let floor = box_body(
            1,
            BodyType::Static,
            Vector2::new(0.0, 100.0),
            Vector2::new(200.0, 8.0),
            0.0,
        );
        let falling = box_body(
            2,
            BodyType::Rigid,
            Vector2::new(0.0, 76.5),
            Vector2::new(16.0, 16.0),
            1.0,
        );
        (floor, falling)
    }

    /// Narrow phase + manifold + constraint for a `floor_and_box` pair.
    fn contact_for(falling: &PhysicsBody2D, floor: &PhysicsBody2D) -> ContactConstraint2D {
        let tf_floor = Transform2D::translated(floor.position);
        let tf_falling = Transform2D::translated(falling.position);
        let result =
            crate::collision::test_collision(&falling.shape, &tf_falling, &floor.shape, &tf_floor)
                .expect("the box overlaps the floor");
        let manifold = build_manifold(
            &falling.shape,
            &tf_falling,
            &floor.shape,
            &tf_floor,
            &result,
        );
        ContactConstraint2D::new(
            falling,
            floor,
            &manifold,
            1.0 / 60.0,
            DEFAULT_CONTACT_BIAS,
            DEFAULT_MAX_PENETRATION,
        )
        .expect("one body is movable")
    }

    #[test]
    fn combine_rules_match_godot() {
        // bounce adds and saturates; friction takes the minimum.
        assert!((combine_bounce(0.3, 0.4) - 0.7).abs() < 1e-6);
        assert!((combine_bounce(0.8, 0.9) - 1.0).abs() < 1e-6);
        assert!((combine_friction(0.2, 0.9) - 0.2).abs() < 1e-6);
        // The old path averaged, which is a different number entirely — a
        // frictionless surface must make the *pair* frictionless.
        assert!(combine_friction(0.0, 1.0).abs() < 1e-6);
    }

    #[test]
    fn box_on_box_yields_a_two_point_manifold() {
        // A 32 px box resting squarely on another touches along a whole edge.
        // One point here is what lets a stacked box rock corner to corner.
        let lower = Shape2D::Rectangle {
            half_extents: Vector2::new(16.0, 16.0),
        };
        let upper = lower;
        let tf_lower = Transform2D::translated(Vector2::new(0.0, 32.0));
        let tf_upper = Transform2D::translated(Vector2::new(0.0, 0.5));
        let result = crate::collision::test_collision(&upper, &tf_upper, &lower, &tf_lower)
            .expect("the boxes overlap");
        let manifold = build_manifold(&upper, &tf_upper, &lower, &tf_lower, &result);
        assert_eq!(
            manifold.points.len(),
            2,
            "a face-on-face box contact must produce two points, got {:?}",
            manifold.points
        );
    }

    #[test]
    fn circle_contact_stays_a_single_point() {
        let circle = Shape2D::Circle { radius: 8.0 };
        let tf_a = Transform2D::translated(Vector2::ZERO);
        let tf_b = Transform2D::translated(Vector2::new(14.0, 0.0));
        let result = crate::collision::test_collision(&circle, &tf_a, &circle, &tf_b).unwrap();
        let manifold = build_manifold(&circle, &tf_a, &circle, &tf_b, &result);
        assert_eq!(manifold.points.len(), 1, "circles touch at one point");
    }

    #[test]
    fn solver_cancels_the_approach_velocity_of_a_falling_box() {
        let (floor, mut falling) = floor_and_box();
        falling.linear_velocity = Vector2::new(0.0, 200.0);
        let mut constraints = vec![contact_for(&falling, &floor)];

        let mut bodies = HashMap::new();
        bodies.insert(falling.id, falling.clone());
        bodies.insert(floor.id, floor.clone());
        ContactSolver2D::default().solve(&mut bodies, &mut constraints);

        let settled = bodies[&falling.id].linear_velocity.y;
        assert!(
            settled.abs() < 1.0,
            "the solver must cancel the 200 px/s approach, left {settled}"
        );
    }

    /// Depenetration must ride the bias channel, never the real velocity —
    /// otherwise resolving an overlap launches the body.
    #[test]
    fn depenetration_never_becomes_kinetic_energy() {
        let (floor, falling) = floor_and_box();
        let mut constraints = vec![contact_for(&falling, &floor)];

        let mut bodies = HashMap::new();
        bodies.insert(falling.id, falling.clone());
        bodies.insert(floor.id, floor.clone());
        let bias = ContactSolver2D::default().solve(&mut bodies, &mut constraints);

        assert!(
            bodies[&falling.id].linear_velocity.length() < 1e-3,
            "a body at rest in a 0.5 px overlap must keep zero *real* velocity, \
             got {:?}",
            bodies[&falling.id].linear_velocity
        );
        let (push, _) = bias[&falling.id];
        assert!(
            push.y < 0.0,
            "the overlap must still be pushed out, upward, on the bias channel; \
             got {push:?}"
        );
    }

    #[test]
    fn a_static_body_is_never_moved_by_the_solver() {
        let (mut floor, mut falling) = floor_and_box();
        floor.linear_velocity = Vector2::new(7.0, 0.0);
        falling.linear_velocity = Vector2::new(0.0, 200.0);
        let mut constraints = vec![contact_for(&falling, &floor)];

        let mut bodies = HashMap::new();
        bodies.insert(falling.id, falling.clone());
        bodies.insert(floor.id, floor.clone());
        let bias = ContactSolver2D::default().solve(&mut bodies, &mut constraints);

        assert_eq!(
            bodies[&floor.id].linear_velocity,
            Vector2::new(7.0, 0.0),
            "a static body's velocity is the scene's business, not the solver's"
        );
        assert!(
            !bias.contains_key(&floor.id),
            "a static body must get no depenetration push either"
        );
    }

    /// The conveyor clause, at the solver level: a static platform that does not
    /// move still drags what rests on it.
    #[test]
    fn constant_linear_velocity_drags_a_resting_body() {
        let (mut floor, mut falling) = floor_and_box();
        floor.constant_linear_velocity = Vector2::new(100.0, 0.0);
        floor.friction = 1.0;
        falling.friction = 1.0;
        // Pressing into the surface is what gives friction something to work
        // with — a Coulomb clamp of zero normal impulse drags nothing.
        falling.linear_velocity = Vector2::new(0.0, 60.0);
        let mut constraints = vec![contact_for(&falling, &floor)];

        let mut bodies = HashMap::new();
        bodies.insert(falling.id, falling.clone());
        bodies.insert(floor.id, floor.clone());
        ContactSolver2D::default().solve(&mut bodies, &mut constraints);

        assert!(
            bodies[&falling.id].linear_velocity.x > 0.0,
            "the conveyor must drag the box along +x, got {:?}",
            bodies[&falling.id].linear_velocity
        );
    }
}

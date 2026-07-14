//! Spatial query types: PhysicsRayQuery3D and PhysicsShapeQuery3D.
//!
//! Mirrors Godot's `PhysicsRayQueryParameters3D` and
//! `PhysicsShapeQueryParameters3D`, providing configurable spatial queries
//! against the physics world's body set.
//!
//! These query objects decouple query configuration from the physics world,
//! allowing queries to be constructed, reused, and run independently.

use std::collections::HashSet;

use gdcore::math::Vector3;
use gdcore::math3d::Quaternion;

use crate::area3d::{Area3D, AreaId3D};
use crate::body::{BodyId3D, PhysicsBody3D};
use crate::collision;
use crate::collision::CollisionResult3D;
use crate::shape::Shape3D;
use crate::world::RaycastHit3D;

// ===========================================================================
// PhysicsRayQuery3D
// ===========================================================================

/// Configurable ray query parameters for 3D physics raycasting.
///
/// Mirrors Godot's `PhysicsRayQueryParameters3D`. Allows setting origin,
/// direction, max distance, collision mask, body exclusions, and whether
/// to hit areas or bodies.
#[derive(Debug, Clone)]
pub struct PhysicsRayQuery3D {
    /// Ray origin in world space.
    pub from: Vector3,
    /// Ray target in world space (direction is computed as `to - from`).
    pub to: Vector3,
    /// Collision mask — only bodies on matching layers are tested.
    pub collision_mask: u32,
    /// Body IDs to exclude from the query.
    pub exclude: HashSet<BodyId3D>,
    /// Whether to collide with bodies (default true).
    pub collide_with_bodies: bool,
    /// Whether to collide with areas (default false).
    pub collide_with_areas: bool,
    /// Whether to report back-face collisions (default false).
    pub hit_back_faces: bool,
    /// Whether to hit from inside shapes (default false).
    pub hit_from_inside: bool,
}

impl PhysicsRayQuery3D {
    /// Creates a new ray query from `from` to `to`.
    pub fn new(from: Vector3, to: Vector3) -> Self {
        Self {
            from,
            to,
            collision_mask: 0xFFFFFFFF,
            exclude: HashSet::new(),
            collide_with_bodies: true,
            collide_with_areas: false,
            hit_back_faces: false,
            hit_from_inside: false,
        }
    }

    /// Returns the ray direction (normalized).
    pub fn direction(&self) -> Vector3 {
        (self.to - self.from).normalized()
    }

    /// Returns the maximum ray length.
    pub fn max_distance(&self) -> f32 {
        (self.to - self.from).length()
    }

    /// Executes the ray query against a set of bodies.
    ///
    /// Returns the closest hit, or `None` if nothing was hit.
    pub fn intersect<'a>(
        &self,
        bodies: impl IntoIterator<Item = &'a PhysicsBody3D>,
    ) -> Option<RaycastHit3D> {
        let dir = self.direction();
        let max_dist = self.max_distance();

        if max_dist < f32::EPSILON {
            return None;
        }

        let mut closest: Option<RaycastHit3D> = None;

        for body in bodies {
            // Exclusion filter
            if self.exclude.contains(&body.id) {
                continue;
            }
            // Layer/mask filter
            if (self.collision_mask & body.collision_layer) == 0 {
                continue;
            }

            if let Some(hit) = ray_test_body(self.from, dir, body) {
                if hit.distance > max_dist {
                    continue;
                }
                if let Some(ref c) = closest {
                    if hit.distance < c.distance {
                        closest = Some(hit);
                    }
                } else {
                    closest = Some(hit);
                }
            }
        }

        closest
    }
}

/// Tests a ray against a single body. Returns a hit if the ray intersects.
fn ray_test_body(origin: Vector3, dir: Vector3, body: &PhysicsBody3D) -> Option<RaycastHit3D> {
    match body.shape {
        Shape3D::Sphere { radius } => ray_sphere(origin, dir, body.position, radius, body.id),
        Shape3D::BoxShape { half_extents } => {
            ray_aabb(origin, dir, body.position, half_extents, body.id)
        }
        Shape3D::CapsuleShape { radius, height } => {
            ray_capsule(origin, dir, body.position, body.rotation, radius, height, body.id)
        }
        // For other shapes, approximate with their bounding sphere
        _ => {
            let aabb = body.shape.bounding_aabb();
            let half = aabb.size * 0.5;
            let eff_radius = half.x.max(half.y).max(half.z);
            ray_sphere(origin, dir, body.position, eff_radius, body.id)
        }
    }
}

/// Ray-sphere intersection.
fn ray_sphere(
    origin: Vector3,
    dir: Vector3,
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

    if t < 0.0 {
        // Behind origin
        return None;
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

/// Ray vs (rotated) capsule intersection.
///
/// The capsule's axis is local `+Y` and `height` is the *total* height
/// including the two hemispherical caps (matching [`Shape3D::CapsuleShape`] and
/// the collision helpers). `cap_rot` orients the axis in world space.
///
/// Analytic port of Inigo Quilez's "capsule - intersection": solve the ray
/// against the infinite cylinder around the core segment, and if the hit falls
/// off the ends, fall back to the two end-cap spheres. `dir` must be
/// normalized, so the returned `distance` is in world units.
#[allow(clippy::too_many_arguments)]
fn ray_capsule(
    origin: Vector3,
    dir: Vector3,
    cap_pos: Vector3,
    cap_rot: Quaternion,
    radius: f32,
    height: f32,
    body_id: BodyId3D,
) -> Option<RaycastHit3D> {
    // Core-segment endpoints in world space.
    let hs = (height * 0.5 - radius).max(0.0);
    let pa = cap_pos + cap_rot.xform(Vector3::new(0.0, -hs, 0.0));
    let pb = cap_pos + cap_rot.xform(Vector3::new(0.0, hs, 0.0));

    let ba = pb - pa;
    let baba = ba.dot(ba);

    // Degenerate segment (height <= 2*radius) — the capsule is a sphere.
    if baba < 1e-12 {
        return ray_sphere(origin, dir, cap_pos, radius, body_id);
    }

    let oa = origin - pa;
    let bard = ba.dot(dir);
    let baoa = ba.dot(oa);
    let rdoa = dir.dot(oa);
    let oaoa = oa.dot(oa);

    let a = baba - bard * bard;
    let b = baba * rdoa - baoa * bard;
    let c = baba * oaoa - baoa * baoa - radius * radius * baba;
    let h = b * b - a * c;

    let mut t_hit: Option<f32> = None;

    // Cylindrical body: `a` is ~0 only when the ray is parallel to the axis, in
    // which case the caps handle it below.
    if a > 1e-9 && h >= 0.0 {
        let t = (-b - h.sqrt()) / a;
        let y = baoa + t * bard;
        if t >= 0.0 && y > 0.0 && y < baba {
            t_hit = Some(t);
        }
    }

    // End caps: keep the nearest non-negative hit.
    if t_hit.is_none() {
        for center in [pa, pb] {
            let oc = origin - center;
            let cb = dir.dot(oc);
            let cc = oc.dot(oc) - radius * radius;
            let ch = cb * cb - cc;
            if ch >= 0.0 {
                let t = -cb - ch.sqrt();
                if t >= 0.0 && t_hit.map_or(true, |prev| t < prev) {
                    t_hit = Some(t);
                }
            }
        }
    }

    let t = t_hit?;
    let point = origin + dir * t;
    // Normal = direction from the closest point on the core segment to the hit.
    let seg_t = ((point - pa).dot(ba) / baba).clamp(0.0, 1.0);
    let axis_point = pa + ba * seg_t;
    let normal = (point - axis_point).normalized();

    Some(RaycastHit3D {
        body_id,
        point,
        normal,
        distance: t,
    })
}

/// Ray-AABB intersection.
fn ray_aabb(
    origin: Vector3,
    dir: Vector3,
    center: Vector3,
    half_extents: Vector3,
    body_id: BodyId3D,
) -> Option<RaycastHit3D> {
    let min = center - half_extents;
    let max = center + half_extents;

    let mut tmin = f32::NEG_INFINITY;
    let mut tmax = f32::INFINITY;
    let mut hit_normal = Vector3::ZERO;

    // X axis
    if dir.x.abs() < f32::EPSILON {
        if origin.x < min.x || origin.x > max.x {
            return None;
        }
    } else {
        let t1 = (min.x - origin.x) / dir.x;
        let t2 = (max.x - origin.x) / dir.x;
        let (t_near, t_far) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
        if t_near > tmin {
            tmin = t_near;
            hit_normal = if dir.x < 0.0 {
                Vector3::new(1.0, 0.0, 0.0)
            } else {
                Vector3::new(-1.0, 0.0, 0.0)
            };
        }
        tmax = tmax.min(t_far);
    }

    // Y axis
    if dir.y.abs() < f32::EPSILON {
        if origin.y < min.y || origin.y > max.y {
            return None;
        }
    } else {
        let t1 = (min.y - origin.y) / dir.y;
        let t2 = (max.y - origin.y) / dir.y;
        let (t_near, t_far) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
        if t_near > tmin {
            tmin = t_near;
            hit_normal = if dir.y < 0.0 {
                Vector3::new(0.0, 1.0, 0.0)
            } else {
                Vector3::new(0.0, -1.0, 0.0)
            };
        }
        tmax = tmax.min(t_far);
    }

    // Z axis
    if dir.z.abs() < f32::EPSILON {
        if origin.z < min.z || origin.z > max.z {
            return None;
        }
    } else {
        let t1 = (min.z - origin.z) / dir.z;
        let t2 = (max.z - origin.z) / dir.z;
        let (t_near, t_far) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
        if t_near > tmin {
            tmin = t_near;
            hit_normal = if dir.z < 0.0 {
                Vector3::new(0.0, 0.0, 1.0)
            } else {
                Vector3::new(0.0, 0.0, -1.0)
            };
        }
        tmax = tmax.min(t_far);
    }

    if tmin > tmax || tmax < 0.0 || tmin < 0.0 {
        return None;
    }

    let point = origin + dir * tmin;

    Some(RaycastHit3D {
        body_id,
        point,
        normal: hit_normal,
        distance: tmin,
    })
}

// ===========================================================================
// PhysicsShapeQuery3D
// ===========================================================================

/// Result of a shape overlap query.
#[derive(Debug, Clone, Copy)]
pub struct ShapeQueryResult3D {
    /// The body that was overlapping.
    pub body_id: BodyId3D,
    /// The body's position.
    pub body_position: Vector3,
    /// Penetration depth (approximate).
    pub depth: f32,
    /// Collision normal (from query shape toward body).
    pub normal: Vector3,
}

/// Configurable shape query parameters for 3D physics overlap/sweep tests.
///
/// Mirrors Godot's `PhysicsShapeQueryParameters3D`. Allows setting a query
/// shape, position, collision mask, and exclusions.
#[derive(Debug, Clone)]
pub struct PhysicsShapeQuery3D {
    /// The query shape.
    pub shape: Shape3D,
    /// Position of the query shape in world space.
    pub position: Vector3,
    /// Collision mask — only bodies on matching layers are tested.
    pub collision_mask: u32,
    /// Body IDs to exclude from the query.
    pub exclude: HashSet<BodyId3D>,
    /// Maximum number of results to return (0 = unlimited).
    pub max_results: usize,
    /// Whether to collide with bodies (default true).
    pub collide_with_bodies: bool,
    /// Whether to collide with areas (default false).
    pub collide_with_areas: bool,
    /// Optional motion vector for sweep tests.
    pub motion: Vector3,
    /// Margin added to the query shape for near-miss detection.
    pub margin: f32,
}

impl PhysicsShapeQuery3D {
    /// Creates a new shape query at the given position.
    pub fn new(shape: Shape3D, position: Vector3) -> Self {
        Self {
            shape,
            position,
            collision_mask: 0xFFFFFFFF,
            exclude: HashSet::new(),
            max_results: 32,
            collide_with_bodies: true,
            collide_with_areas: false,
            motion: Vector3::ZERO,
            margin: 0.0,
        }
    }

    /// Finds all bodies overlapping the query shape at its current position.
    pub fn intersect<'a>(
        &self,
        bodies: impl IntoIterator<Item = &'a PhysicsBody3D>,
    ) -> Vec<ShapeQueryResult3D> {
        let mut results = Vec::new();

        for body in bodies {
            if self.exclude.contains(&body.id) {
                continue;
            }
            if (self.collision_mask & body.collision_layer) == 0 {
                continue;
            }

            let cr =
                collision::test_collision(self.position, &self.shape, body.position, &body.shape);

            if cr.colliding {
                results.push(ShapeQueryResult3D {
                    body_id: body.id,
                    body_position: body.position,
                    depth: cr.depth,
                    normal: cr.normal,
                });
                if self.max_results > 0 && results.len() >= self.max_results {
                    break;
                }
            }
        }

        results
    }

    /// Returns the closest body overlapping the query shape, if any.
    pub fn intersect_closest<'a>(
        &self,
        bodies: impl IntoIterator<Item = &'a PhysicsBody3D>,
    ) -> Option<ShapeQueryResult3D> {
        let mut closest: Option<ShapeQueryResult3D> = None;

        for body in bodies {
            if self.exclude.contains(&body.id) {
                continue;
            }
            if (self.collision_mask & body.collision_layer) == 0 {
                continue;
            }

            let cr =
                collision::test_collision(self.position, &self.shape, body.position, &body.shape);

            if cr.colliding {
                match &closest {
                    Some(c) if cr.depth <= c.depth => {}
                    _ => {
                        closest = Some(ShapeQueryResult3D {
                            body_id: body.id,
                            body_position: body.position,
                            depth: cr.depth,
                            normal: cr.normal,
                        });
                    }
                }
            }
        }

        closest
    }

    /// Sweeps the query shape along [`Self::motion`] and reports how far it can
    /// travel before contacting a body.
    ///
    /// Mirrors Godot's `PhysicsDirectSpaceState3D::cast_motion`, returning
    /// `(safe, unsafe)` fractions in `[0.0, 1.0]`:
    /// - `safe` — the largest fraction of the motion with no collision.
    /// - `unsafe` — the fraction at which contact is certain.
    ///
    /// Returns `(1.0, 1.0)` when the whole motion is clear and `(0.0, 0.0)` when
    /// the shape already overlaps a body at the start. Deterministic: a fixed
    /// coarse march brackets the first contact and a fixed number of bisection
    /// steps refines the time of impact (no timing or randomness).
    pub fn cast_motion<'a>(
        &self,
        bodies: impl IntoIterator<Item = &'a PhysicsBody3D>,
    ) -> (f32, f32) {
        let bodies: Vec<&PhysicsBody3D> = bodies
            .into_iter()
            .filter(|b| {
                !self.exclude.contains(&b.id) && (self.collision_mask & b.collision_layer) != 0
            })
            .collect();

        // Already overlapping at the start of the motion.
        if self.collides_at(0.0, &bodies) {
            return (0.0, 0.0);
        }
        // No motion to sweep.
        if self.motion.length_squared() < 1e-12 {
            return (1.0, 1.0);
        }

        // Coarse march to bracket the first contact between a clear and a
        // colliding sample.
        const STEPS: usize = 32;
        let mut lo = 0.0_f32;
        let mut hi = 1.0_f32;
        let mut bracketed = false;
        for i in 1..=STEPS {
            let t = i as f32 / STEPS as f32;
            if self.collides_at(t, &bodies) {
                lo = (i - 1) as f32 / STEPS as f32;
                hi = t;
                bracketed = true;
                break;
            }
        }
        if !bracketed {
            return (1.0, 1.0);
        }

        // Bisect the [clear, contact] bracket for the time of impact.
        for _ in 0..24 {
            let mid = 0.5 * (lo + hi);
            if self.collides_at(mid, &bodies) {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        (lo, hi)
    }

    /// Tests the query shape (swept to `motion * t`) against the pre-filtered
    /// body set. Honors each body's rotation via [`collision::test_collision_rot`].
    fn collides_at(&self, t: f32, bodies: &[&PhysicsBody3D]) -> bool {
        let pos = self.position + self.motion * t;
        bodies.iter().any(|b| {
            collision::test_collision_rot(
                pos,
                Quaternion::IDENTITY,
                &self.shape,
                b.position,
                b.rotation,
                &b.shape,
            )
            .colliding
        })
    }

    /// Reports contact witness point pairs between the query shape and each
    /// overlapping body, mirroring Godot's `PhysicsDirectSpaceState3D::collide_shape`.
    ///
    /// Each entry is `[point_on_self, point_on_collider]` — the witness point on
    /// the query shape's surface and on the collider's surface, separated along
    /// the contact normal. Honors `collision_mask`, `exclude`, `max_results`, and
    /// the `collide_with_bodies` flag. Returns an empty vec when
    /// `collide_with_bodies` is false.
    pub fn collide_shape<'a>(
        &self,
        bodies: impl IntoIterator<Item = &'a PhysicsBody3D>,
    ) -> Vec<[Vector3; 2]> {
        let mut out = Vec::new();
        if !self.collide_with_bodies {
            return out;
        }
        for body in bodies {
            if self.exclude.contains(&body.id) {
                continue;
            }
            if (self.collision_mask & body.collision_layer) == 0 {
                continue;
            }
            let cr = collision::test_collision_rot(
                self.position,
                Quaternion::IDENTITY,
                &self.shape,
                body.position,
                body.rotation,
                &body.shape,
            );
            if cr.colliding {
                out.push(self.witness_points(body, &cr));
                if self.max_results > 0 && out.len() >= self.max_results {
                    break;
                }
            }
        }
        out
    }

    /// Returns rest information for the deepest contact between the query shape
    /// and the body set, mirroring Godot's
    /// `PhysicsDirectSpaceState3D::get_rest_info`.
    ///
    /// The reported `point` sits on the collider's surface, `normal` points from
    /// the query shape toward the collider, and `linear_velocity` is the
    /// collider's linear velocity. Honors `collision_mask`, `exclude`, and the
    /// `collide_with_bodies` flag. Returns `None` if nothing overlaps.
    pub fn get_rest_info<'a>(
        &self,
        bodies: impl IntoIterator<Item = &'a PhysicsBody3D>,
    ) -> Option<RestInfo3D> {
        if !self.collide_with_bodies {
            return None;
        }
        let mut best: Option<RestInfo3D> = None;
        for body in bodies {
            if self.exclude.contains(&body.id) {
                continue;
            }
            if (self.collision_mask & body.collision_layer) == 0 {
                continue;
            }
            let cr = collision::test_collision_rot(
                self.position,
                Quaternion::IDENTITY,
                &self.shape,
                body.position,
                body.rotation,
                &body.shape,
            );
            if cr.colliding {
                let deeper = best.map_or(true, |b| cr.depth > b.depth);
                if deeper {
                    let [_self_pt, collider_pt] = self.witness_points(body, &cr);
                    best = Some(RestInfo3D {
                        collider: body.id,
                        point: collider_pt,
                        normal: cr.normal,
                        linear_velocity: body.linear_velocity,
                        depth: cr.depth,
                    });
                }
            }
        }
        best
    }

    /// Scans a set of areas for overlap with the query shape, mirroring the
    /// `collide_with_areas` path of Godot's space-state queries.
    ///
    /// Returns the ids of overlapping areas. Gated by the `collide_with_areas`
    /// flag (returns empty when false) and filtered by `collision_mask` against
    /// each area's `collision_layer`, honoring `max_results`.
    pub fn intersect_areas<'a>(
        &self,
        areas: impl IntoIterator<Item = &'a Area3D>,
    ) -> Vec<AreaId3D> {
        let mut out = Vec::new();
        if !self.collide_with_areas {
            return out;
        }
        for area in areas {
            if (self.collision_mask & area.collision_layer) == 0 {
                continue;
            }
            let cr = collision::test_collision(
                self.position,
                &self.shape,
                area.position,
                &area.shape,
            );
            if cr.colliding {
                out.push(area.id);
                if self.max_results > 0 && out.len() >= self.max_results {
                    break;
                }
            }
        }
        out
    }

    /// Computes the witness points `[point_on_self, point_on_collider]` for a
    /// contact whose `normal` (query → collider) and geometry are described by
    /// `cr`. Each point is projected onto its shape's surface along the normal.
    fn witness_points(&self, body: &PhysicsBody3D, cr: &CollisionResult3D) -> [Vector3; 2] {
        let n = cr.normal;
        let point_on_self = self.position + n * support_radius(&self.shape, n);
        let point_on_collider = body.position - n * support_radius(&body.shape, -n);
        [point_on_self, point_on_collider]
    }
}

// ===========================================================================
// Rest info + support helpers
// ===========================================================================

/// Rest information for the deepest contact of a shape query.
///
/// Mirrors the payload of Godot's `PhysicsDirectSpaceState3D::get_rest_info`
/// (collider, contact point, normal, and the collider's linear velocity).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RestInfo3D {
    /// The colliding body.
    pub collider: BodyId3D,
    /// Contact point on the collider's surface (world space).
    pub point: Vector3,
    /// Contact normal, pointing from the query shape toward the collider.
    pub normal: Vector3,
    /// Linear velocity of the collider.
    pub linear_velocity: Vector3,
    /// Penetration depth of the contact.
    pub depth: f32,
}

/// Distance from a shape's center to its surface along the unit direction `dir`
/// (the shape's support width). Used to project witness points onto surfaces.
///
/// Rotation is not applied — capsule support uses the local `+Y` axis, which is
/// exact for axis-aligned shapes and a close approximation otherwise.
fn support_radius(shape: &Shape3D, dir: Vector3) -> f32 {
    match shape {
        Shape3D::Sphere { radius } => *radius,
        Shape3D::BoxShape { half_extents } => {
            half_extents.x * dir.x.abs()
                + half_extents.y * dir.y.abs()
                + half_extents.z * dir.z.abs()
        }
        Shape3D::CapsuleShape { radius, height } => {
            let hs = (*height * 0.5 - *radius).max(0.0);
            *radius + hs * dir.y.abs()
        }
        other => {
            let aabb = other.bounding_aabb();
            let half = aabb.size * 0.5;
            half.x.max(half.y).max(half.z)
        }
    }
}

/// Tests whether `point` lies inside a shape positioned at `shape_pos` with the
/// given rotation, by intersecting a degenerate point-sphere with the shape.
fn shape_contains_point(
    shape: &Shape3D,
    shape_pos: Vector3,
    shape_rot: Quaternion,
    point: Vector3,
) -> bool {
    collision::test_collision_rot(
        point,
        Quaternion::IDENTITY,
        &Shape3D::Sphere { radius: 1e-4 },
        shape_pos,
        shape_rot,
        shape,
    )
    .colliding
}

// ===========================================================================
// PhysicsPointQuery3D
// ===========================================================================

/// Configurable point query parameters, mirroring Godot's
/// `PhysicsPointQueryParameters3D`. Finds all bodies (and optionally areas)
/// whose shapes contain a world-space point.
#[derive(Debug, Clone)]
pub struct PhysicsPointQuery3D {
    /// The world-space point to test.
    pub position: Vector3,
    /// Collision mask — only colliders on matching layers are tested.
    pub collision_mask: u32,
    /// Body IDs to exclude from the query.
    pub exclude: HashSet<BodyId3D>,
    /// Maximum number of results to return (0 = unlimited).
    pub max_results: usize,
    /// Whether to collide with bodies (default true).
    pub collide_with_bodies: bool,
    /// Whether to collide with areas (default false).
    pub collide_with_areas: bool,
}

impl PhysicsPointQuery3D {
    /// Creates a new point query at `position` with Godot-compatible defaults.
    pub fn new(position: Vector3) -> Self {
        Self {
            position,
            collision_mask: 0xFFFFFFFF,
            exclude: HashSet::new(),
            max_results: 32,
            collide_with_bodies: true,
            collide_with_areas: false,
        }
    }

    /// Returns the ids of bodies whose shapes contain the query point.
    ///
    /// Gated by `collide_with_bodies`; honors `collision_mask`, `exclude`, and
    /// `max_results`.
    pub fn intersect_bodies<'a>(
        &self,
        bodies: impl IntoIterator<Item = &'a PhysicsBody3D>,
    ) -> Vec<BodyId3D> {
        let mut out = Vec::new();
        if !self.collide_with_bodies {
            return out;
        }
        for body in bodies {
            if self.exclude.contains(&body.id) {
                continue;
            }
            if (self.collision_mask & body.collision_layer) == 0 {
                continue;
            }
            if shape_contains_point(&body.shape, body.position, body.rotation, self.position) {
                out.push(body.id);
                if self.max_results > 0 && out.len() >= self.max_results {
                    break;
                }
            }
        }
        out
    }

    /// Returns the ids of areas whose shapes contain the query point.
    ///
    /// Gated by `collide_with_areas`; honors `collision_mask` (against each
    /// area's `collision_layer`) and `max_results`.
    pub fn intersect_areas<'a>(
        &self,
        areas: impl IntoIterator<Item = &'a Area3D>,
    ) -> Vec<AreaId3D> {
        let mut out = Vec::new();
        if !self.collide_with_areas {
            return out;
        }
        for area in areas {
            if (self.collision_mask & area.collision_layer) == 0 {
                continue;
            }
            if shape_contains_point(&area.shape, area.position, Quaternion::IDENTITY, self.position)
            {
                out.push(area.id);
                if self.max_results > 0 && out.len() >= self.max_results {
                    break;
                }
            }
        }
        out
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::BodyType3D;

    fn sphere_body(id: u64, pos: Vector3) -> PhysicsBody3D {
        PhysicsBody3D::new(
            BodyId3D(id),
            BodyType3D::Rigid,
            pos,
            Shape3D::Sphere { radius: 1.0 },
            1.0,
        )
    }

    fn box_body(id: u64, pos: Vector3, half: Vector3) -> PhysicsBody3D {
        PhysicsBody3D::new(
            BodyId3D(id),
            BodyType3D::Static,
            pos,
            Shape3D::BoxShape { half_extents: half },
            0.0,
        )
    }

    fn capsule_body(id: u64, pos: Vector3, radius: f32, height: f32) -> PhysicsBody3D {
        PhysicsBody3D::new(
            BodyId3D(id),
            BodyType3D::Static,
            pos,
            Shape3D::CapsuleShape { radius, height },
            0.0,
        )
    }

    // -- PhysicsRayQuery3D ----------------------------------------------------

    #[test]
    fn ray_query_hits_sphere() {
        let bodies = vec![sphere_body(1, Vector3::new(0.0, 0.0, 10.0))];
        let query = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 20.0));
        let hit = query.intersect(bodies.iter());
        assert!(hit.is_some());
        let h = hit.unwrap();
        assert_eq!(h.body_id, BodyId3D(1));
        assert!((h.distance - 9.0).abs() < 0.01);
    }

    #[test]
    fn ray_query_misses_sphere() {
        let bodies = vec![sphere_body(1, Vector3::new(10.0, 0.0, 0.0))];
        let query = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 20.0));
        let hit = query.intersect(bodies.iter());
        assert!(hit.is_none());
    }

    #[test]
    fn ray_query_respects_max_distance() {
        let bodies = vec![sphere_body(1, Vector3::new(0.0, 0.0, 10.0))];
        // Ray only goes to z=5, so it won't reach the sphere at z=10
        let query = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 5.0));
        let hit = query.intersect(bodies.iter());
        assert!(hit.is_none());
    }

    #[test]
    fn ray_query_excludes_bodies() {
        let bodies = vec![sphere_body(1, Vector3::new(0.0, 0.0, 10.0))];
        let mut query = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 20.0));
        query.exclude.insert(BodyId3D(1));
        let hit = query.intersect(bodies.iter());
        assert!(hit.is_none());
    }

    #[test]
    fn ray_query_collision_mask_filters() {
        let mut body = sphere_body(1, Vector3::new(0.0, 0.0, 10.0));
        body.collision_layer = 0b0010;
        let bodies = vec![body];

        let mut query = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 20.0));
        query.collision_mask = 0b0001; // doesn't match layer 2
        assert!(query.intersect(bodies.iter()).is_none());

        query.collision_mask = 0b0010; // matches
        assert!(query.intersect(bodies.iter()).is_some());
    }

    #[test]
    fn ray_query_closest_of_multiple() {
        let bodies = vec![
            sphere_body(1, Vector3::new(0.0, 0.0, 10.0)),
            sphere_body(2, Vector3::new(0.0, 0.0, 5.0)),
        ];
        let query = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 20.0));
        let hit = query.intersect(bodies.iter());
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().body_id, BodyId3D(2)); // closer
    }

    #[test]
    fn ray_query_direction_and_max_distance() {
        let query = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(3.0, 4.0, 0.0));
        assert!((query.max_distance() - 5.0).abs() < 0.01);
        let dir = query.direction();
        assert!((dir.x - 0.6).abs() < 0.01);
        assert!((dir.y - 0.8).abs() < 0.01);
    }

    #[test]
    fn ray_query_hits_box() {
        let bodies = vec![box_body(
            1,
            Vector3::new(0.0, 0.0, 10.0),
            Vector3::new(2.0, 2.0, 2.0),
        )];
        let query = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 20.0));
        let hit = query.intersect(bodies.iter());
        assert!(hit.is_some());
        let h = hit.unwrap();
        assert!((h.point.z - 8.0).abs() < 0.01); // box face at z=8
        assert!((h.normal.z - (-1.0)).abs() < 0.01); // normal points back at us
    }

    #[test]
    fn ray_query_zero_length_returns_none() {
        let bodies = vec![sphere_body(1, Vector3::ZERO)];
        let query = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::ZERO);
        assert!(query.intersect(bodies.iter()).is_none());
    }

    // -- Capsule raycast (true, not sphere-approximated) ----------------------

    #[test]
    fn ray_query_hits_capsule_body() {
        // Vertical capsule (axis +Y), radius 1.0. A +Z ray at y=0 strikes the
        // cylindrical side at z = 10 - 1.0 = 9.0 (the old sphere approximation
        // used eff radius 2.0 and would have reported 8.0).
        let bodies = vec![capsule_body(1, Vector3::new(0.0, 0.0, 10.0), 1.0, 4.0)];
        let query = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 20.0));
        let hit = query.intersect(bodies.iter()).expect("ray should hit capsule body");
        assert_eq!(hit.body_id, BodyId3D(1));
        assert!((hit.distance - 9.0).abs() < 0.01, "distance {}", hit.distance);
        assert!((hit.normal.z + 1.0).abs() < 0.01, "normal {:?}", hit.normal);
    }

    #[test]
    fn ray_query_hits_capsule_cap() {
        // Segment y in [-1, 1]; top cap centered at (0,1,10), radius 1 → apex at
        // (0,2,10). A downward ray through x=0,z=10 hits that apex.
        let bodies = vec![capsule_body(1, Vector3::new(0.0, 0.0, 10.0), 1.0, 4.0)];
        let query =
            PhysicsRayQuery3D::new(Vector3::new(0.0, 5.0, 10.0), Vector3::new(0.0, -5.0, 10.0));
        let hit = query.intersect(bodies.iter()).expect("ray should hit top cap");
        assert!((hit.point.y - 2.0).abs() < 0.02, "point {:?}", hit.point);
        assert!((hit.normal.y - 1.0).abs() < 0.02, "normal {:?}", hit.normal);
    }

    #[test]
    fn ray_query_misses_capsule_where_sphere_approx_would_hit() {
        // Offset 1.5 in X: outside the true capsule (radius 1.0) yet inside the
        // old bounding-sphere approximation (eff radius 2.0). A true capsule
        // raycast must miss.
        let bodies = vec![capsule_body(1, Vector3::new(0.0, 0.0, 10.0), 1.0, 4.0)];
        let query =
            PhysicsRayQuery3D::new(Vector3::new(1.5, 0.0, 0.0), Vector3::new(1.5, 0.0, 20.0));
        assert!(query.intersect(bodies.iter()).is_none());
    }

    #[test]
    fn ray_query_capsule_honors_rotation() {
        use std::f32::consts::FRAC_PI_2;
        // Rotate the capsule 90° about Z so its axis lies along X. A downward
        // ray through x=0,z=0 now hits the cylindrical side at y=0.5 (radius),
        // distance 9.5 — whereas an unrotated (axis +Y) capsule would present a
        // cap apex at y=2.0, distance 8.0.
        let mut body = capsule_body(1, Vector3::ZERO, 0.5, 4.0);
        body.rotation = Quaternion::from_axis_angle(Vector3::new(0.0, 0.0, 1.0), -FRAC_PI_2);
        let bodies = vec![body];
        let query =
            PhysicsRayQuery3D::new(Vector3::new(0.0, 10.0, 0.0), Vector3::new(0.0, -10.0, 0.0));
        let hit = query
            .intersect(bodies.iter())
            .expect("ray should hit rotated capsule side");
        assert!((hit.distance - 9.5).abs() < 0.02, "distance {}", hit.distance);
    }

    #[test]
    fn ray_query_capsule_degenerate_is_sphere() {
        // height <= 2*radius collapses the core segment; the capsule behaves as
        // a sphere of the given radius.
        let bodies = vec![capsule_body(1, Vector3::new(0.0, 0.0, 10.0), 2.0, 2.0)];
        let query = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 20.0));
        let hit = query.intersect(bodies.iter()).expect("degenerate capsule hits");
        assert!((hit.distance - 8.0).abs() < 0.01, "distance {}", hit.distance);
    }

    // -- PhysicsShapeQuery3D --------------------------------------------------

    #[test]
    fn shape_query_finds_overlapping() {
        let bodies = vec![
            sphere_body(1, Vector3::new(1.0, 0.0, 0.0)), // overlapping
            sphere_body(2, Vector3::new(100.0, 0.0, 0.0)), // far away
        ];
        let query = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 2.0 }, Vector3::ZERO);
        let results = query.intersect(bodies.iter());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].body_id, BodyId3D(1));
    }

    #[test]
    fn shape_query_no_overlap() {
        let bodies = vec![sphere_body(1, Vector3::new(100.0, 0.0, 0.0))];
        let query = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 1.0 }, Vector3::ZERO);
        let results = query.intersect(bodies.iter());
        assert!(results.is_empty());
    }

    #[test]
    fn shape_query_excludes_bodies() {
        let bodies = vec![sphere_body(1, Vector3::new(1.0, 0.0, 0.0))];
        let mut query = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 5.0 }, Vector3::ZERO);
        query.exclude.insert(BodyId3D(1));
        let results = query.intersect(bodies.iter());
        assert!(results.is_empty());
    }

    #[test]
    fn shape_query_collision_mask_filters() {
        let mut body = sphere_body(1, Vector3::new(1.0, 0.0, 0.0));
        body.collision_layer = 0b0100;
        let bodies = vec![body];

        let mut query = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 5.0 }, Vector3::ZERO);
        query.collision_mask = 0b0001;
        assert!(query.intersect(bodies.iter()).is_empty());

        query.collision_mask = 0b0100;
        assert_eq!(query.intersect(bodies.iter()).len(), 1);
    }

    #[test]
    fn shape_query_max_results() {
        let bodies = vec![
            sphere_body(1, Vector3::new(1.0, 0.0, 0.0)),
            sphere_body(2, Vector3::new(0.0, 1.0, 0.0)),
            sphere_body(3, Vector3::new(0.0, 0.0, 1.0)),
        ];
        let mut query = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 5.0 }, Vector3::ZERO);
        query.max_results = 2;
        let results = query.intersect(bodies.iter());
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn shape_query_intersect_closest() {
        let bodies = vec![
            sphere_body(1, Vector3::new(1.0, 0.0, 0.0)),
            sphere_body(2, Vector3::new(0.5, 0.0, 0.0)),
        ];
        let query = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 5.0 }, Vector3::ZERO);
        let closest = query.intersect_closest(bodies.iter());
        assert!(closest.is_some());
        // Body 2 is closer → more penetration depth
        assert_eq!(closest.unwrap().body_id, BodyId3D(2));
    }

    #[test]
    fn shape_query_defaults() {
        let query = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 1.0 }, Vector3::ZERO);
        assert_eq!(query.collision_mask, 0xFFFFFFFF);
        assert!(query.exclude.is_empty());
        assert_eq!(query.max_results, 32);
        assert!(query.collide_with_bodies);
        assert!(!query.collide_with_areas);
        assert_eq!(query.motion, Vector3::ZERO);
        assert!((query.margin - 0.0).abs() < f32::EPSILON);
    }

    // -- PhysicsShapeQuery3D::cast_motion -------------------------------------

    fn sweeping_capsule() -> PhysicsShapeQuery3D {
        let mut q = PhysicsShapeQuery3D::new(
            Shape3D::CapsuleShape {
                radius: 0.5,
                height: 2.0,
            },
            Vector3::ZERO,
        );
        q.motion = Vector3::new(0.0, 0.0, 10.0);
        q
    }

    #[test]
    fn cast_motion_clear_path_returns_one() {
        let bodies = vec![sphere_body(1, Vector3::new(0.0, 0.0, 100.0))];
        let (safe, unsafe_) = sweeping_capsule().cast_motion(bodies.iter());
        assert!((safe - 1.0).abs() < 1e-6, "safe {}", safe);
        assert!((unsafe_ - 1.0).abs() < 1e-6, "unsafe {}", unsafe_);
    }

    #[test]
    fn cast_motion_stops_before_body() {
        // Capsule (r=0.5) sweeps +Z toward a sphere (r=1.0) at z=10. Contact when
        // the axis-to-center gap closes to 1.5: z = 8.5 → safe fraction 0.85.
        let bodies = vec![sphere_body(1, Vector3::new(0.0, 0.0, 10.0))];
        let (safe, unsafe_) = sweeping_capsule().cast_motion(bodies.iter());
        assert!((safe - 0.85).abs() < 0.01, "safe {}", safe);
        assert!(unsafe_ >= safe, "unsafe {} < safe {}", unsafe_, safe);
        assert!(unsafe_ - safe < 0.01, "bracket too wide: {}..{}", safe, unsafe_);
    }

    #[test]
    fn cast_motion_already_colliding_returns_zero() {
        let bodies = vec![sphere_body(1, Vector3::new(0.0, 0.0, 1.0))];
        let (safe, unsafe_) = sweeping_capsule().cast_motion(bodies.iter());
        assert_eq!(safe, 0.0);
        assert_eq!(unsafe_, 0.0);
    }

    #[test]
    fn cast_motion_respects_exclude() {
        let bodies = vec![sphere_body(1, Vector3::new(0.0, 0.0, 10.0))];
        let mut query = sweeping_capsule();
        query.exclude.insert(BodyId3D(1));
        let (safe, _unsafe) = query.cast_motion(bodies.iter());
        assert!((safe - 1.0).abs() < 1e-6, "safe {}", safe);
    }

    #[test]
    fn cast_motion_respects_collision_mask() {
        let mut body = sphere_body(1, Vector3::new(0.0, 0.0, 10.0));
        body.collision_layer = 0b0010;
        let bodies = vec![body];
        let mut query = sweeping_capsule();
        query.collision_mask = 0b0001; // does not match layer 2 → motion is clear
        let (safe, _unsafe) = query.cast_motion(bodies.iter());
        assert!((safe - 1.0).abs() < 1e-6, "safe {}", safe);
    }

    #[test]
    fn cast_motion_zero_motion_returns_one() {
        // No overlap at rest and no motion → the whole (empty) sweep is clear.
        let bodies = vec![sphere_body(1, Vector3::new(0.0, 0.0, 10.0))];
        let mut query = sweeping_capsule();
        query.motion = Vector3::ZERO;
        let (safe, unsafe_) = query.cast_motion(bodies.iter());
        assert!((safe - 1.0).abs() < 1e-6, "safe {}", safe);
        assert!((unsafe_ - 1.0).abs() < 1e-6, "unsafe {}", unsafe_);
    }

    // -- PhysicsShapeQuery3D::collide_shape -----------------------------------

    #[test]
    fn collide_shape_reports_surface_witness_points() {
        // Query sphere r=2 at origin overlaps body sphere r=1 at x=2.5
        // (overlap 0.5). Normal is +X; the witness point on the query surface is
        // x=2.0 and on the collider surface x=1.5.
        let bodies = vec![sphere_body(1, Vector3::new(2.5, 0.0, 0.0))];
        let query = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 2.0 }, Vector3::ZERO);
        let contacts = query.collide_shape(bodies.iter());
        assert_eq!(contacts.len(), 1);
        let [on_self, on_collider] = contacts[0];
        assert!((on_self.x - 2.0).abs() < 0.01, "self {:?}", on_self);
        assert!((on_collider.x - 1.5).abs() < 0.01, "collider {:?}", on_collider);
    }

    #[test]
    fn collide_shape_respects_collide_with_bodies_flag() {
        let bodies = vec![sphere_body(1, Vector3::new(1.0, 0.0, 0.0))];
        let mut query = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 2.0 }, Vector3::ZERO);
        query.collide_with_bodies = false;
        assert!(query.collide_shape(bodies.iter()).is_empty());
    }

    #[test]
    fn collide_shape_honors_exclude_and_mask() {
        let mut body = sphere_body(1, Vector3::new(1.0, 0.0, 0.0));
        body.collision_layer = 0b0010;
        let bodies = vec![body];
        let mut query = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 2.0 }, Vector3::ZERO);

        query.collision_mask = 0b0001; // no layer match
        assert!(query.collide_shape(bodies.iter()).is_empty());

        query.collision_mask = 0b0010; // matches
        assert_eq!(query.collide_shape(bodies.iter()).len(), 1);

        query.exclude.insert(BodyId3D(1));
        assert!(query.collide_shape(bodies.iter()).is_empty());
    }

    // -- PhysicsShapeQuery3D::get_rest_info -----------------------------------

    #[test]
    fn rest_info_reports_deepest_contact() {
        // Two overlapping bodies; body 2 (x=2.0) penetrates deeper than body 1
        // (x=2.5). Rest info must reference the deeper collider, its velocity,
        // and a contact point on that collider's surface.
        let shallow = sphere_body(1, Vector3::new(2.5, 0.0, 0.0));
        let mut deep = sphere_body(2, Vector3::new(2.0, 0.0, 0.0));
        deep.linear_velocity = Vector3::new(0.0, 3.0, 0.0);
        let bodies = vec![shallow, deep];

        let query = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 2.0 }, Vector3::ZERO);
        let info = query.get_rest_info(bodies.iter()).expect("overlap should yield rest info");
        assert_eq!(info.collider, BodyId3D(2));
        assert!((info.normal.x - 1.0).abs() < 0.01, "normal {:?}", info.normal);
        assert!((info.linear_velocity.y - 3.0).abs() < 1e-6);
        // Collider surface point: x = 2.0 - 1.0 = 1.0.
        assert!((info.point.x - 1.0).abs() < 0.01, "point {:?}", info.point);
        assert!(info.depth > 0.0);
    }

    #[test]
    fn rest_info_none_without_overlap() {
        let bodies = vec![sphere_body(1, Vector3::new(100.0, 0.0, 0.0))];
        let query = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 1.0 }, Vector3::ZERO);
        assert!(query.get_rest_info(bodies.iter()).is_none());
    }

    // -- PhysicsShapeQuery3D::intersect_areas (collide_with_areas) ------------

    fn sphere_area(id: u64, pos: Vector3, radius: f32) -> Area3D {
        Area3D::new(AreaId3D(id), pos, Shape3D::Sphere { radius })
    }

    #[test]
    fn shape_query_scans_areas_when_enabled() {
        let areas = vec![
            sphere_area(1, Vector3::new(1.0, 0.0, 0.0)),   // overlapping
            sphere_area(2, Vector3::new(100.0, 0.0, 0.0)), // far
        ];
        let mut query = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 2.0 }, Vector3::ZERO);

        // Disabled by default → no area scanning.
        assert!(query.intersect_areas(areas.iter()).is_empty());

        query.collide_with_areas = true;
        let hits = query.intersect_areas(areas.iter());
        assert_eq!(hits, vec![AreaId3D(1)]);
    }

    #[test]
    fn shape_query_area_scan_honors_mask() {
        let mut area = sphere_area(1, Vector3::new(1.0, 0.0, 0.0));
        area.collision_layer = 0b0100;
        let areas = vec![area];
        let mut query = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 2.0 }, Vector3::ZERO);
        query.collide_with_areas = true;

        query.collision_mask = 0b0001;
        assert!(query.intersect_areas(areas.iter()).is_empty());

        query.collision_mask = 0b0100;
        assert_eq!(query.intersect_areas(areas.iter()).len(), 1);
    }

    // -- PhysicsPointQuery3D --------------------------------------------------

    #[test]
    fn point_query_finds_containing_body() {
        // Point at (0.5,0,0) is inside the sphere (r=1) at origin, outside the
        // sphere at x=10.
        let bodies = vec![
            sphere_body(1, Vector3::ZERO),
            sphere_body(2, Vector3::new(10.0, 0.0, 0.0)),
        ];
        let query = PhysicsPointQuery3D::new(Vector3::new(0.5, 0.0, 0.0));
        let hits = query.intersect_bodies(bodies.iter());
        assert_eq!(hits, vec![BodyId3D(1)]);
    }

    #[test]
    fn point_query_outside_returns_empty() {
        let bodies = vec![sphere_body(1, Vector3::ZERO)];
        let query = PhysicsPointQuery3D::new(Vector3::new(5.0, 0.0, 0.0));
        assert!(query.intersect_bodies(bodies.iter()).is_empty());
    }

    #[test]
    fn point_query_honors_exclude_mask_and_flag() {
        let mut body = sphere_body(1, Vector3::ZERO);
        body.collision_layer = 0b0010;
        let bodies = vec![body];
        let mut query = PhysicsPointQuery3D::new(Vector3::ZERO);

        query.collision_mask = 0b0001; // no match
        assert!(query.intersect_bodies(bodies.iter()).is_empty());

        query.collision_mask = 0b0010; // match
        assert_eq!(query.intersect_bodies(bodies.iter()).len(), 1);

        query.exclude.insert(BodyId3D(1));
        assert!(query.intersect_bodies(bodies.iter()).is_empty());
        query.exclude.clear();

        query.collide_with_bodies = false;
        assert!(query.intersect_bodies(bodies.iter()).is_empty());
    }

    #[test]
    fn point_query_scans_areas_when_enabled() {
        let areas = vec![sphere_area(7, Vector3::ZERO, 1.0)];
        let mut query = PhysicsPointQuery3D::new(Vector3::new(0.25, 0.0, 0.0));

        // Areas off by default.
        assert!(query.intersect_areas(areas.iter()).is_empty());

        query.collide_with_areas = true;
        assert_eq!(query.intersect_areas(areas.iter()), vec![AreaId3D(7)]);
    }
}

//! Collision detection and resolution.
//!
//! Provides narrow-phase collision tests between pairs of 2D shapes,
//! returning contact information (normal, depth, contact point) when
//! shapes overlap. Also provides body separation utilities.

use gdcore::math::{Transform2D, Vector2};

use crate::body::PhysicsBody2D;
use crate::shape::Shape2D;

/// The result of a narrow-phase collision test between two shapes.
#[derive(Debug, Clone, Copy)]
pub struct CollisionResult {
    /// Whether the shapes are overlapping.
    pub colliding: bool,
    /// The collision normal, pointing from shape **A toward shape B** — the
    /// direction B must be pushed to separate the pair. (`circle_circle` builds
    /// it as `pos_b - pos_a`, and `separate_bodies` moves B along `+normal`;
    /// this doc said the opposite.)
    pub normal: Vector2,
    /// The penetration depth along the normal.
    pub depth: f32,
    /// The approximate contact point in world space.
    pub point: Vector2,
}

impl CollisionResult {
    /// A "no collision" sentinel value.
    pub const NONE: Self = Self {
        colliding: false,
        normal: Vector2::ZERO,
        depth: 0.0,
        point: Vector2::ZERO,
    };
}

/// Whether a shape has no spatial extent at all and therefore cannot produce a
/// contact. Used to keep degenerate geometry out of the SAT path, where a
/// zero-length separating axis yields NaN rather than "no collision".
fn is_degenerate(shape: &Shape2D) -> bool {
    const EPS: f32 = 1e-6;
    match shape {
        Shape2D::Rectangle { half_extents } => {
            half_extents.x.abs() < EPS && half_extents.y.abs() < EPS
        }
        Shape2D::Circle { radius } => radius.abs() < EPS,
        _ => false,
    }
}

/// Tests for collision between two shapes, each positioned by a transform.
///
/// The transforms carry the full basis (rotation and scale), so collisions are
/// **rotation-aware** — shapes are transformed into world space before the
/// narrow-phase runs. Every shape pair is supported: circles, rectangles,
/// segments, capsules, convex polygons, concave polygons (segment soups),
/// world boundaries (half-planes), and separation rays.
///
/// The result's `normal` always points from shape A toward shape B, and `depth`
/// is the penetration along that normal (`0` when merely touching). A non-
/// colliding pair returns `Some(CollisionResult::NONE)` (`colliding == false`),
/// never `None`; `None` is reserved for genuinely degenerate/empty shapes.
pub fn test_collision(
    shape_a: &Shape2D,
    transform_a: &Transform2D,
    shape_b: &Shape2D,
    transform_b: &Transform2D,
) -> Option<CollisionResult> {
    // A body whose colliders are all `disabled` is represented as a zero-extent
    // rectangle — the "collides with nothing" sentinel, chosen so a disabled
    // shape cannot be silently replaced by a default-sized stand-in
    // (pat-0q2kx.28). Its four vertices are COINCIDENT rather than absent, so it
    // is not caught by the `verts.is_empty()` guards below: the polygon path
    // then normalises a zero-length axis and every contact value becomes NaN,
    // which propagates straight into the character controller's position
    // (pat-0q2kx.34 — a walker next to a disabled barrier ended up at x=NaN).
    //
    // Returning `None` is exactly this function's documented contract for a
    // degenerate shape, and every caller already treats `None` as "no contact".
    if is_degenerate(shape_a) || is_degenerate(shape_b) {
        return None;
    }

    // Fast path: circle vs circle is rotation-invariant, so keep the exact,
    // well-tested closed form (including the coincident-centers case).
    if let (Shape2D::Circle { radius: ra }, Shape2D::Circle { radius: rb }) = (shape_a, shape_b) {
        return Some(circle_circle(
            transform_a.origin,
            *ra,
            transform_b.origin,
            *rb,
        ));
    }

    let proxy_a = ShapeProxy::from_shape(shape_a, transform_a);
    let proxy_b = ShapeProxy::from_shape(shape_b, transform_b);
    Some(collide(&proxy_a, &proxy_b))
}

/// Circle vs circle collision test.
fn circle_circle(pos_a: Vector2, ra: f32, pos_b: Vector2, rb: f32) -> CollisionResult {
    let diff = pos_b - pos_a;
    let dist_sq = diff.length_squared();
    let sum_r = ra + rb;

    if dist_sq > sum_r * sum_r {
        return CollisionResult::NONE;
    }

    let dist = dist_sq.sqrt();
    if dist < 1e-10 {
        // Circles are coincident — pick an arbitrary normal
        return CollisionResult {
            colliding: true,
            normal: Vector2::new(1.0, 0.0),
            depth: sum_r,
            point: pos_a,
        };
    }

    let normal = diff * (1.0 / dist);
    let depth = sum_r - dist;
    let point = pos_a + normal * (ra - depth * 0.5);

    CollisionResult {
        colliding: true,
        normal,
        depth,
        point,
    }
}

// ---------------------------------------------------------------------------
// Rotation-aware narrow-phase (SAT + closest-distance) for arbitrary shapes.
// ---------------------------------------------------------------------------

/// A world-space collision proxy for a shape.
///
/// Most shapes reduce to a *rounded convex polygon*: a convex core of one or
/// more world-space vertices, inflated by a rounding `radius` (the Minkowski
/// sum of the core with a disc). This unifies circles (1 core vertex), segments
/// (2 vertices, radius 0), capsules (2 vertices, radius > 0), rectangles and
/// convex polygons (N vertices, radius 0). World boundaries and concave
/// polygons need dedicated handling.
enum ShapeProxy {
    /// Convex core (`verts`) inflated by `radius`.
    Rounded { verts: Vec<Vector2>, radius: f32 },
    /// Infinite half-plane; the solid region is `(p - point)·normal <= 0`.
    HalfPlane { normal: Vector2, point: Vector2 },
    /// Concave shape: independent world-space segments, tested individually.
    Concave { segments: Vec<[Vector2; 2]> },
}

impl ShapeProxy {
    /// Builds a world-space proxy for `shape` positioned by `tf`.
    fn from_shape(shape: &Shape2D, tf: &Transform2D) -> Self {
        match shape {
            Shape2D::Circle { radius } => ShapeProxy::Rounded {
                verts: vec![tf.origin],
                radius: *radius,
            },
            Shape2D::Rectangle { half_extents } => {
                let he = *half_extents;
                ShapeProxy::Rounded {
                    verts: vec![
                        tf.xform(Vector2::new(-he.x, -he.y)),
                        tf.xform(Vector2::new(he.x, -he.y)),
                        tf.xform(Vector2::new(he.x, he.y)),
                        tf.xform(Vector2::new(-he.x, he.y)),
                    ],
                    radius: 0.0,
                }
            }
            Shape2D::Segment { a, b } => ShapeProxy::Rounded {
                verts: vec![tf.xform(*a), tf.xform(*b)],
                radius: 0.0,
            },
            Shape2D::Capsule { radius, height } => {
                let half = (*height * 0.5 - *radius).max(0.0);
                if half <= 1e-6 {
                    // Degenerate capsule — behaves as a circle.
                    ShapeProxy::Rounded {
                        verts: vec![tf.xform(Vector2::ZERO)],
                        radius: *radius,
                    }
                } else {
                    ShapeProxy::Rounded {
                        verts: vec![
                            tf.xform(Vector2::new(0.0, -half)),
                            tf.xform(Vector2::new(0.0, half)),
                        ],
                        radius: *radius,
                    }
                }
            }
            Shape2D::ConvexPolygon { .. } => ShapeProxy::Rounded {
                verts: shape
                    .polygon_points()
                    .iter()
                    .map(|p| tf.xform(*p))
                    .collect(),
                radius: 0.0,
            },
            Shape2D::ConcavePolygon { .. } => ShapeProxy::Concave {
                segments: shape
                    .polygon_points()
                    .chunks_exact(2)
                    .map(|s| [tf.xform(s[0]), tf.xform(s[1])])
                    .collect(),
            },
            Shape2D::WorldBoundary { normal, distance } => {
                let world_normal = tf.basis_xform(*normal).normalized();
                let world_point = tf.xform(*normal * *distance);
                ShapeProxy::HalfPlane {
                    normal: world_normal,
                    point: world_point,
                }
            }
            Shape2D::SeparationRay { length } => ShapeProxy::Rounded {
                verts: vec![
                    tf.xform(Vector2::ZERO),
                    tf.xform(Vector2::new(0.0, *length)),
                ],
                radius: 0.0,
            },
        }
    }
}

/// Dispatches a collision test between two world-space proxies.
fn collide(a: &ShapeProxy, b: &ShapeProxy) -> CollisionResult {
    match (a, b) {
        (ShapeProxy::Concave { segments }, other) => collide_concave(segments, other, false),
        (other, ShapeProxy::Concave { segments }) => collide_concave(segments, other, true),
        (ShapeProxy::HalfPlane { normal, point }, ShapeProxy::Rounded { verts, radius }) => {
            halfplane_vs_rounded(*normal, *point, verts, *radius, false)
        }
        (ShapeProxy::Rounded { verts, radius }, ShapeProxy::HalfPlane { normal, point }) => {
            halfplane_vs_rounded(*normal, *point, verts, *radius, true)
        }
        // Two infinite half-planes have no meaningful bounded contact.
        (ShapeProxy::HalfPlane { .. }, ShapeProxy::HalfPlane { .. }) => CollisionResult::NONE,
        (
            ShapeProxy::Rounded {
                verts: va,
                radius: ra,
            },
            ShapeProxy::Rounded {
                verts: vb,
                radius: rb,
            },
        ) => rounded_vs_rounded(va, *ra, vb, *rb),
    }
}

/// Collides a concave segment soup against another proxy, returning the deepest
/// contact. `other_is_a` records whether `other` plays the role of shape A.
fn collide_concave(
    segments: &[[Vector2; 2]],
    other: &ShapeProxy,
    other_is_a: bool,
) -> CollisionResult {
    let mut best: Option<CollisionResult> = None;
    for seg in segments {
        let seg_proxy = ShapeProxy::Rounded {
            verts: vec![seg[0], seg[1]],
            radius: 0.0,
        };
        // Preserve the caller's A→B ordering so the contact normal stays correct.
        let res = if other_is_a {
            collide(other, &seg_proxy)
        } else {
            collide(&seg_proxy, other)
        };
        if res.colliding && best.as_ref().map_or(true, |b| res.depth > b.depth) {
            best = Some(res);
        }
    }
    best.unwrap_or(CollisionResult::NONE)
}

/// Collides an infinite half-plane against a rounded convex proxy.
///
/// `plane_is_b` is true when the half-plane is shape B (so the contact normal,
/// which points A→B, must face the plane rather than the polygon).
fn halfplane_vs_rounded(
    normal: Vector2,
    point: Vector2,
    verts: &[Vector2],
    radius: f32,
    plane_is_b: bool,
) -> CollisionResult {
    if verts.is_empty() {
        return CollisionResult::NONE;
    }
    // Signed distance of each vertex from the plane (positive on the empty side).
    let mut min_signed = f32::INFINITY;
    let mut deepest = verts[0];
    for &v in verts {
        let s = (v - point).dot(normal);
        if s < min_signed {
            min_signed = s;
            deepest = v;
        }
    }
    // The rounding disc reaches `radius` further toward the solid side.
    let depth = radius - min_signed;
    if depth <= 0.0 {
        return CollisionResult::NONE;
    }
    // Push the polygon out along +normal (the empty side). With the A→B
    // convention, that is +normal when the plane is A and -normal when it is B.
    let normal_ab = if plane_is_b { -normal } else { normal };
    let contact = deepest - normal * min_signed; // foot of the deepest vertex on the plane
    CollisionResult {
        colliding: true,
        normal: normal_ab,
        depth,
        point: contact,
    }
}

/// Collides two rounded convex proxies (cores `va`/`vb`, radii `ra`/`rb`).
fn rounded_vs_rounded(va: &[Vector2], ra: f32, vb: &[Vector2], rb: f32) -> CollisionResult {
    if va.is_empty() || vb.is_empty() {
        return CollisionResult::NONE;
    }

    // Case 1: a single-point core (circle/degenerate) buried inside the other
    // convex polygon — resolve via the nearest edge, like the classic
    // circle-inside-rect case.
    if va.len() == 1 && vb.len() >= 3 {
        if let Some(res) = point_inside_convex(va[0], ra, vb, rb, true) {
            return res;
        }
    }
    if vb.len() == 1 && va.len() >= 3 {
        if let Some(res) = point_inside_convex(vb[0], rb, va, ra, false) {
            return res;
        }
    }

    // Case 2: both cores are real polygons (area > 0) — use SAT to find the
    // minimum translation vector for the overlapping (penetrating) case. SAT is
    // only reliable for polygons with ≥3 vertices; degenerate cores (points and
    // segments/capsule cores) are resolved by the intersection-aware
    // closest-distance path below, which correctly handles crossing and
    // containment.
    if va.len() >= 3 && vb.len() >= 3 {
        if let Some((axis, penetration)) = convex_sat(va, vb) {
            let ca = centroid(va);
            let cb = centroid(vb);
            let mut normal = axis;
            if (cb - ca).dot(normal) < 0.0 {
                normal = -normal; // orient A→B
            }
            return CollisionResult {
                colliding: true,
                normal,
                depth: penetration + ra + rb,
                point: (ca + cb) * 0.5,
            };
        }
    }

    // Case 3: cores are disjoint — collide iff the closest-approach distance is
    // within the combined rounding radii.
    let (pa, pb, dist) = convex_closest(va, vb);
    let sum_r = ra + rb;
    if dist <= sum_r + 1e-6 {
        let normal = if dist > 1e-9 {
            (pb - pa) * (1.0 / dist)
        } else {
            fallback_normal(va, vb)
        };
        return CollisionResult {
            colliding: true,
            normal,
            depth: (sum_r - dist).max(0.0),
            point: pa + normal * ra,
        };
    }

    CollisionResult::NONE
}

/// Resolves a single point `p` (rounded by `p_radius`) lying inside the convex
/// polygon `poly` (rounded by `poly_radius`). Returns `None` if `p` is outside.
///
/// `point_is_a` is true when the point shape plays the role of A.
fn point_inside_convex(
    p: Vector2,
    p_radius: f32,
    poly: &[Vector2],
    poly_radius: f32,
    point_is_a: bool,
) -> Option<CollisionResult> {
    if !point_in_convex(p, poly) {
        return None;
    }
    let (outward, edge_dist) = nearest_edge_outward(p, poly);
    let depth = p_radius + edge_dist + poly_radius;
    // The point must exit the polygon along `outward`. With the A→B convention
    // that is -outward when the point is A, +outward when the point is B.
    let normal = if point_is_a { -outward } else { outward };
    Some(CollisionResult {
        colliding: true,
        normal,
        depth,
        point: p,
    })
}

/// Separating Axis Theorem overlap test for two convex vertex sets.
///
/// Returns `Some((axis, penetration))` — a unit axis and the minimum
/// penetration depth — when the shapes overlap, or `None` if a separating axis
/// exists (including the just-touching case, `overlap == 0`).
fn convex_sat(a: &[Vector2], b: &[Vector2]) -> Option<(Vector2, f32)> {
    let mut min_overlap = f32::INFINITY;
    let mut min_axis = Vector2::new(1.0, 0.0);
    for verts in [a, b] {
        let n = verts.len();
        for i in 0..n {
            let edge = verts[(i + 1) % n] - verts[i];
            let len = edge.length();
            if len < 1e-9 {
                continue;
            }
            let axis = Vector2::new(-edge.y, edge.x) * (1.0 / len);
            let (a_min, a_max) = project(a, axis);
            let (b_min, b_max) = project(b, axis);
            let overlap = a_max.min(b_max) - a_min.max(b_min);
            if overlap <= 0.0 {
                return None; // separating axis found
            }
            if overlap < min_overlap {
                min_overlap = overlap;
                min_axis = axis;
            }
        }
    }
    if min_overlap.is_finite() {
        Some((min_axis, min_overlap))
    } else {
        None
    }
}

/// Projects a vertex set onto `axis`, returning the (min, max) scalar extents.
fn project(verts: &[Vector2], axis: Vector2) -> (f32, f32) {
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for &v in verts {
        let d = v.dot(axis);
        min = min.min(d);
        max = max.max(d);
    }
    (min, max)
}

/// Closest points and distance between two convex vertex sets (points,
/// segments, or polygons).
///
/// Returns a zero distance when the shapes intersect: either because one
/// contains a vertex of the other, or because their boundary edges cross (the
/// segment-segment routine yields a zero gap for crossing edges). This makes
/// the result correct for overlapping degenerate cores (a segment or capsule
/// core crossing or buried inside a polygon), not just disjoint hulls.
fn convex_closest(a: &[Vector2], b: &[Vector2]) -> (Vector2, Vector2, f32) {
    // Containment: a vertex of one shape lying inside the other means they
    // overlap even if no boundary edges cross.
    if a.len() >= 3 {
        if let Some(&v) = b.iter().find(|&&v| point_in_convex(v, a)) {
            return (v, v, 0.0);
        }
    }
    if b.len() >= 3 {
        if let Some(&v) = a.iter().find(|&&v| point_in_convex(v, b)) {
            return (v, v, 0.0);
        }
    }

    // Otherwise the closest approach lies between boundary edges. Crossing
    // edges produce a zero gap via the segment-segment routine.
    let mut best = f32::INFINITY;
    let mut best_pa = a[0];
    let mut best_pb = b[0];
    for (a0, a1) in hull_edges(a) {
        for (b0, b1) in hull_edges(b) {
            let (pa, pb) = seg_seg_closest(a0, a1, b0, b1);
            let d = pa.distance_to(pb);
            if d < best {
                best = d;
                best_pa = pa;
                best_pb = pb;
            }
        }
    }
    (best_pa, best_pb, best)
}

/// Boundary edges of a convex vertex set: none for empty, a degenerate
/// zero-length edge for a single point, one edge for a segment, and a closed
/// loop for a polygon.
fn hull_edges(verts: &[Vector2]) -> Vec<(Vector2, Vector2)> {
    match verts.len() {
        0 => Vec::new(),
        1 => vec![(verts[0], verts[0])],
        2 => vec![(verts[0], verts[1])],
        n => (0..n).map(|i| (verts[i], verts[(i + 1) % n])).collect(),
    }
}

/// Closest pair of points between segments `p1`–`q1` and `p2`–`q2`.
///
/// Standard clamped closest-point-between-segments (Ericson, *Real-Time
/// Collision Detection*). Degenerate zero-length segments are handled, and
/// crossing segments return coincident points (zero distance).
fn seg_seg_closest(p1: Vector2, q1: Vector2, p2: Vector2, q2: Vector2) -> (Vector2, Vector2) {
    let d1 = q1 - p1; // direction of segment 1
    let d2 = q2 - p2; // direction of segment 2
    let r = p1 - p2;
    let a = d1.dot(d1); // squared length of segment 1
    let e = d2.dot(d2); // squared length of segment 2
    let f = d2.dot(r);

    let s;
    let t;
    if a <= 1e-12 && e <= 1e-12 {
        // Both segments are points.
        return (p1, p2);
    }
    if a <= 1e-12 {
        // Segment 1 is a point.
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = d1.dot(r);
        if e <= 1e-12 {
            // Segment 2 is a point.
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            // General non-degenerate case.
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            let s0 = if denom > 1e-12 {
                ((b * f - c * e) / denom).clamp(0.0, 1.0)
            } else {
                0.0 // parallel segments — pick an arbitrary point on segment 1
            };
            let t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t0 > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            } else {
                t = t0;
                s = s0;
            }
        }
    }
    (p1 + d1 * s, p2 + d2 * t)
}

/// Centroid (average) of a vertex set.
fn centroid(verts: &[Vector2]) -> Vector2 {
    if verts.is_empty() {
        return Vector2::ZERO;
    }
    let mut sum = Vector2::ZERO;
    for &v in verts {
        sum = sum + v;
    }
    sum * (1.0 / verts.len() as f32)
}

/// A fallback contact normal (centroid direction) for coincident cores.
fn fallback_normal(va: &[Vector2], vb: &[Vector2]) -> Vector2 {
    let d = centroid(vb) - centroid(va);
    if d.length_squared() > 1e-12 {
        d.normalized()
    } else {
        Vector2::new(1.0, 0.0)
    }
}

/// Tests whether `p` is inside the convex polygon `verts` (winding-agnostic).
/// Returns `false` for degenerate polygons with fewer than three vertices.
fn point_in_convex(p: Vector2, verts: &[Vector2]) -> bool {
    let n = verts.len();
    if n < 3 {
        return false;
    }
    let mut has_pos = false;
    let mut has_neg = false;
    for i in 0..n {
        let cross = (verts[(i + 1) % n] - verts[i]).cross(p - verts[i]);
        if cross > 1e-6 {
            has_pos = true;
        } else if cross < -1e-6 {
            has_neg = true;
        }
        if has_pos && has_neg {
            return false;
        }
    }
    true
}

/// For a point `p` inside convex `verts`, returns the nearest edge's outward
/// unit normal and the perpendicular distance from `p` to that edge.
fn nearest_edge_outward(p: Vector2, verts: &[Vector2]) -> (Vector2, f32) {
    let n = verts.len();
    let center = centroid(verts);
    let mut best_dist = f32::INFINITY;
    let mut best_normal = Vector2::new(1.0, 0.0);
    for i in 0..n {
        let a = verts[i];
        let b = verts[(i + 1) % n];
        let edge = b - a;
        let len = edge.length();
        if len < 1e-9 {
            continue;
        }
        let mut normal = Vector2::new(-edge.y, edge.x) * (1.0 / len);
        if (a - center).dot(normal) < 0.0 {
            normal = -normal; // ensure it faces away from the polygon interior
        }
        let dist = (p - a).dot(normal).abs();
        if dist < best_dist {
            best_dist = dist;
            best_normal = normal;
        }
    }
    (best_normal, best_dist)
}

/// World-space support vertices for `shape` under `tf`, plus the radius it is
/// inflated by (a circle is one point with a radius, a capsule two).
///
/// `None` for shapes with no finite vertex set — a world boundary is a
/// half-plane and a concave shape is a segment soup. Exposed so the contact
/// solver can build a two-point manifold from the same vertices the narrow
/// phase used, rather than re-deriving them and drifting out of sync
/// (pat-0q2kx.116).
pub fn world_vertices(shape: &Shape2D, tf: &Transform2D) -> Option<(Vec<Vector2>, f32)> {
    match ShapeProxy::from_shape(shape, tf) {
        ShapeProxy::Rounded { verts, radius } => Some((verts, radius)),
        _ => None,
    }
}

/// Separates two bodies based on a collision result.
///
/// Distributes the separation proportionally by inverse mass. Static bodies
/// do not move.
///
/// ⚠️ Single-point, single-pass, non-accumulating: this is the *fallback* path
/// and the one the character controller and the narrow-phase unit tests use.
/// [`crate::solver`] is what the world step runs — a stack of boxes needs an
/// iterated manifold solver, which this is not (pat-0q2kx.116).
pub fn separate_bodies(a: &mut PhysicsBody2D, b: &mut PhysicsBody2D, result: &CollisionResult) {
    if !result.colliding || result.depth <= 0.0 {
        return;
    }

    let inv_a = a.inverse_mass();
    let inv_b = b.inverse_mass();
    let total_inv = inv_a + inv_b;

    if total_inv <= 0.0 {
        return; // Both immovable
    }

    let separation = result.normal * result.depth;
    a.position = a.position - separation * (inv_a / total_inv);
    b.position = b.position + separation * (inv_b / total_inv);

    // Velocity resolution with restitution
    let relative_vel = b.linear_velocity - a.linear_velocity;
    let vel_along_normal = relative_vel.dot(result.normal);

    if vel_along_normal > 0.0 {
        return; // Bodies are already separating
    }

    // Godot combines restitution as `CLAMP(A.bounce + B.bounce, 0, 1)`
    // (`godot_body_pair_2d.cpp:243`), NOT as a maximum.
    //
    // ⚠️ The difference is visible whenever BOTH bodies are bouncy: two
    // surfaces at 0.5 add to a fully elastic 1.0 under Godot and stay at 0.5
    // under a maximum, so a ball that should keep its speed loses half of it
    // every hit. The two agree only when one side is 0, which is exactly the
    // fixture most bounce tests use (pat-s3rrv.99).
    let restitution = (a.bounce + b.bounce).clamp(0.0, 1.0);
    let impulse_magnitude = -(1.0 + restitution) * vel_along_normal / total_inv;
    let impulse = result.normal * impulse_magnitude;

    a.linear_velocity = a.linear_velocity - impulse * inv_a;
    b.linear_velocity = b.linear_velocity + impulse * inv_b;

    // Angular response: the normal impulse applied at the contact point spins
    // each body about its center of mass. For a head-on (centered) contact the
    // lever arm is parallel to the impulse, so cross(r, impulse) is zero and no
    // spin is imparted — preserving pre-existing linear-only collision results.
    let r_a = result.point - a.position;
    let r_b = result.point - b.position;
    a.angular_velocity -= (r_a.x * impulse.y - r_a.y * impulse.x) * a.inverse_inertia();
    b.angular_velocity += (r_b.x * impulse.y - r_b.y * impulse.x) * b.inverse_inertia();

    // Any contact wakes both bodies — a sleeping box struck by another, or
    // resting on a conveyor, must resume simulating (pat-0q2kx.116).
    a.wake();
    b.wake();

    // Friction: reduce tangential velocity component.
    //
    // The relative velocity used here is the SURFACE velocity, which adds each
    // body's `constant_linear_velocity` (pat-0q2kx.116). That property is how
    // Godot models a conveyor belt: the body itself does not move, but its
    // surface drags whatever rests on it. Using the rigid-body velocities alone
    // — as this did — meant `constant_linear_velocity` was stored on the body
    // and consulted by nothing, so a conveyor was inert.
    // Godot: `abs(MIN(A.friction, B.friction))` (`godot_body_pair_2d.cpp:247`),
    // NOT the mean. The minimum means one slippery surface is enough to make a
    // contact slippery — ice under rubber slides — where a mean would average
    // the two and grip.
    let friction = a.friction.min(b.friction).abs();
    if friction > 0.0 {
        let surface_a = a.linear_velocity + a.constant_linear_velocity;
        let surface_b = b.linear_velocity + b.constant_linear_velocity;
        let new_relative_vel = surface_b - surface_a;
        let tangent_vel = new_relative_vel - result.normal * new_relative_vel.dot(result.normal);
        let tangent_speed = tangent_vel.length();
        if tangent_speed > 1e-6 {
            let tangent_dir = tangent_vel * (1.0 / tangent_speed);
            // Coulomb friction: clamp tangential impulse to friction * normal impulse
            let max_friction_impulse = friction * impulse_magnitude;
            let friction_impulse_mag = tangent_speed.min(max_friction_impulse / total_inv);
            let friction_impulse = tangent_dir * friction_impulse_mag;

            a.linear_velocity = a.linear_velocity + friction_impulse * inv_a;
            b.linear_velocity = b.linear_velocity - friction_impulse * inv_b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::{BodyId, BodyType};

    const EPSILON: f32 = 1e-4;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    #[test]
    fn circle_circle_overlapping() {
        let result = test_collision(
            &Shape2D::Circle { radius: 5.0 },
            &Transform2D::translated(Vector2::new(0.0, 0.0)),
            &Shape2D::Circle { radius: 5.0 },
            &Transform2D::translated(Vector2::new(8.0, 0.0)),
        )
        .unwrap();
        assert!(result.colliding);
        assert!(approx_eq(result.depth, 2.0));
        assert!(approx_eq(result.normal.x, 1.0));
        assert!(approx_eq(result.normal.y, 0.0));
    }

    #[test]
    fn circle_circle_touching() {
        let result = test_collision(
            &Shape2D::Circle { radius: 5.0 },
            &Transform2D::translated(Vector2::new(0.0, 0.0)),
            &Shape2D::Circle { radius: 5.0 },
            &Transform2D::translated(Vector2::new(10.0, 0.0)),
        )
        .unwrap();
        // Exactly touching: depth == 0, still "colliding" (<=)
        assert!(result.colliding);
        assert!(approx_eq(result.depth, 0.0));
    }

    #[test]
    fn circle_circle_separated() {
        let result = test_collision(
            &Shape2D::Circle { radius: 5.0 },
            &Transform2D::translated(Vector2::new(0.0, 0.0)),
            &Shape2D::Circle { radius: 5.0 },
            &Transform2D::translated(Vector2::new(11.0, 0.0)),
        )
        .unwrap();
        assert!(!result.colliding);
    }

    #[test]
    fn circle_rect_collision() {
        let result = test_collision(
            &Shape2D::Circle { radius: 2.0 },
            &Transform2D::translated(Vector2::new(4.0, 0.0)),
            &Shape2D::Rectangle {
                half_extents: Vector2::new(3.0, 3.0),
            },
            &Transform2D::translated(Vector2::new(0.0, 0.0)),
        )
        .unwrap();
        assert!(result.colliding);
        // Circle center at 4, rect edge at 3 => gap = 1, circle radius = 2 => depth = 1
        assert!(approx_eq(result.depth, 1.0));
        // Normal points from A (circle) toward B (rect), so negative x
        assert!(approx_eq(result.normal.x, -1.0));
    }

    #[test]
    fn circle_rect_no_collision() {
        let result = test_collision(
            &Shape2D::Circle { radius: 1.0 },
            &Transform2D::translated(Vector2::new(10.0, 0.0)),
            &Shape2D::Rectangle {
                half_extents: Vector2::new(3.0, 3.0),
            },
            &Transform2D::translated(Vector2::new(0.0, 0.0)),
        )
        .unwrap();
        assert!(!result.colliding);
    }

    #[test]
    fn rect_rect_overlapping() {
        let result = test_collision(
            &Shape2D::Rectangle {
                half_extents: Vector2::new(3.0, 3.0),
            },
            &Transform2D::translated(Vector2::new(0.0, 0.0)),
            &Shape2D::Rectangle {
                half_extents: Vector2::new(3.0, 3.0),
            },
            &Transform2D::translated(Vector2::new(4.0, 0.0)),
        )
        .unwrap();
        assert!(result.colliding);
        assert!(approx_eq(result.depth, 2.0));
    }

    #[test]
    fn rect_rect_separated() {
        let result = test_collision(
            &Shape2D::Rectangle {
                half_extents: Vector2::new(2.0, 2.0),
            },
            &Transform2D::translated(Vector2::new(0.0, 0.0)),
            &Shape2D::Rectangle {
                half_extents: Vector2::new(2.0, 2.0),
            },
            &Transform2D::translated(Vector2::new(10.0, 0.0)),
        )
        .unwrap();
        assert!(!result.colliding);
    }

    /// Builds a transform with rotation `angle` (radians) about the origin,
    /// then translated to `pos` — i.e. a body's world transform.
    fn tf(pos: Vector2, angle: f32) -> Transform2D {
        let mut t = Transform2D::rotated(angle);
        t.origin = pos;
        t
    }

    #[test]
    fn segment_circle_now_collides() {
        // Previously this pair was unsupported (returned None); it is now a
        // real collision test.
        let result = test_collision(
            &Shape2D::Segment {
                a: Vector2::ZERO,
                b: Vector2::new(1.0, 0.0),
            },
            &Transform2D::IDENTITY,
            &Shape2D::Circle { radius: 1.0 },
            &Transform2D::IDENTITY,
        )
        .unwrap();
        assert!(result.colliding);
    }

    #[test]
    fn every_pair_returns_some() {
        // The full pair table is implemented: no shape combination returns None.
        let shapes = [
            Shape2D::Circle { radius: 1.0 },
            Shape2D::Rectangle {
                half_extents: Vector2::new(1.0, 1.0),
            },
            Shape2D::Segment {
                a: Vector2::new(-1.0, 0.0),
                b: Vector2::new(1.0, 0.0),
            },
            Shape2D::Capsule {
                radius: 1.0,
                height: 4.0,
            },
            Shape2D::convex_polygon(&[
                Vector2::new(-1.0, -1.0),
                Vector2::new(1.0, -1.0),
                Vector2::new(1.0, 1.0),
                Vector2::new(-1.0, 1.0),
            ]),
            Shape2D::concave_polygon(&[Vector2::new(-5.0, 0.0), Vector2::new(5.0, 0.0)]),
            Shape2D::world_boundary(Vector2::new(0.0, -1.0), 0.0),
            Shape2D::SeparationRay { length: 3.0 },
        ];
        for a in &shapes {
            for b in &shapes {
                let r = test_collision(a, &Transform2D::IDENTITY, b, &Transform2D::IDENTITY);
                assert!(r.is_some(), "pair {a:?} vs {b:?} returned None");
            }
        }
    }

    #[test]
    fn rect_rect_rotation_changes_result() {
        // Two thin horizontal bars. Unrotated they are 0.2 apart on Y.
        let bar = Shape2D::Rectangle {
            half_extents: Vector2::new(2.0, 0.5),
        };
        let a_pos = Vector2::new(0.0, 0.0);
        let b_pos = Vector2::new(0.0, 1.2);

        let unrotated = test_collision(&bar, &tf(a_pos, 0.0), &bar, &tf(b_pos, 0.0)).unwrap();
        assert!(
            !unrotated.colliding,
            "axis-aligned bars 0.2 apart must not collide"
        );

        // Rotate bar A by 90°: it now spans Y in [-2, 2] and reaches bar B.
        let rotated = test_collision(
            &bar,
            &tf(a_pos, std::f32::consts::FRAC_PI_2),
            &bar,
            &tf(b_pos, 0.0),
        )
        .unwrap();
        assert!(
            rotated.colliding,
            "rotating bar A by 90° must make it reach bar B"
        );
    }

    #[test]
    fn circle_rect_rotation_aware() {
        // Circle just outside the corner of a square; only after the square
        // rotates 45° (presenting a corner) does the gap close.
        let circle = Shape2D::Circle { radius: 0.5 };
        let square = Shape2D::Rectangle {
            half_extents: Vector2::new(1.0, 1.0),
        };
        let circle_pos = Vector2::new(1.7, 0.0); // clears the flat face at x=1 (gap 0.7 > 0.5)

        let axis_aligned = test_collision(
            &circle,
            &tf(circle_pos, 0.0),
            &square,
            &tf(Vector2::ZERO, 0.0),
        )
        .unwrap();
        assert!(!axis_aligned.colliding);

        // Rotated 45°, the square's corner reaches x = sqrt(2) ≈ 1.414, within
        // reach of the circle (center 1.7, radius 0.5 → reaches x = 1.2).
        let rotated = test_collision(
            &circle,
            &tf(circle_pos, 0.0),
            &square,
            &tf(Vector2::ZERO, std::f32::consts::FRAC_PI_4),
        )
        .unwrap();
        assert!(
            rotated.colliding,
            "rotated square corner should reach circle"
        );
    }

    #[test]
    fn circle_convex_polygon_collision() {
        // Right triangle with a corner at (4, 0); circle overlaps that corner.
        let tri = Shape2D::convex_polygon(&[
            Vector2::new(0.0, 0.0),
            Vector2::new(4.0, 0.0),
            Vector2::new(0.0, 4.0),
        ]);
        let result = test_collision(
            &Shape2D::Circle { radius: 1.0 },
            &tf(Vector2::new(4.5, 0.0), 0.0),
            &tri,
            &Transform2D::IDENTITY,
        )
        .unwrap();
        assert!(result.colliding);
        assert!(
            (result.depth - 0.5).abs() < 1e-4,
            "depth was {}",
            result.depth
        );
        assert!(
            (result.normal.x + 1.0).abs() < 1e-4,
            "normal.x was {}",
            result.normal.x
        );
    }

    #[test]
    fn capsule_vs_rectangle_collision() {
        // Vertical capsule (core segment ±1 on Y, radius 1) near a unit square.
        let capsule = Shape2D::Capsule {
            radius: 1.0,
            height: 4.0,
        };
        let square = Shape2D::Rectangle {
            half_extents: Vector2::new(1.0, 1.0),
        };
        // Core segment at x = 1.8; square right face at x = 1 → gap 0.8 < radius.
        let result = test_collision(
            &capsule,
            &tf(Vector2::new(1.8, 0.0), 0.0),
            &square,
            &Transform2D::IDENTITY,
        )
        .unwrap();
        assert!(result.colliding);
        assert!(
            (result.depth - 0.2).abs() < 1e-4,
            "depth was {}",
            result.depth
        );
    }

    #[test]
    fn world_boundary_floor_collision() {
        // Floor half-plane through the origin, empty side up (-Y).
        let floor = Shape2D::world_boundary(Vector2::new(0.0, -1.0), 0.0);
        let circle = Shape2D::Circle { radius: 1.0 };
        // Circle center 0.5 above the floor → penetrates by radius - 0.5 = 0.5.
        let result = test_collision(
            &circle,
            &tf(Vector2::new(0.0, -0.5), 0.0),
            &floor,
            &Transform2D::IDENTITY,
        )
        .unwrap();
        assert!(result.colliding);
        assert!(
            (result.depth - 0.5).abs() < 1e-4,
            "depth was {}",
            result.depth
        );
        // Normal points A→B (circle → plane) = +Y (down toward the floor line).
        assert!(
            (result.normal.y - 1.0).abs() < 1e-4,
            "normal.y was {}",
            result.normal.y
        );

        // A circle well above the floor does not collide.
        let clear = test_collision(
            &circle,
            &tf(Vector2::new(0.0, -5.0), 0.0),
            &floor,
            &Transform2D::IDENTITY,
        )
        .unwrap();
        assert!(!clear.colliding);
    }

    #[test]
    fn concave_polygon_floor_collision() {
        // A wide flat floor expressed as a single concave segment.
        let floor = Shape2D::concave_polygon(&[Vector2::new(-10.0, 0.0), Vector2::new(10.0, 0.0)]);
        let circle = Shape2D::Circle { radius: 1.0 };
        let result = test_collision(
            &circle,
            &tf(Vector2::new(0.0, -0.5), 0.0),
            &floor,
            &Transform2D::IDENTITY,
        )
        .unwrap();
        assert!(result.colliding);
        assert!(
            (result.depth - 0.5).abs() < 1e-4,
            "depth was {}",
            result.depth
        );
    }

    #[test]
    fn separation_ray_vs_circle_collision() {
        // Ray along +Y of length 3; circle offset 0.5 in X from the ray.
        let ray = Shape2D::SeparationRay { length: 3.0 };
        let result = test_collision(
            &ray,
            &Transform2D::IDENTITY,
            &Shape2D::Circle { radius: 1.0 },
            &tf(Vector2::new(0.5, 1.5), 0.0),
        )
        .unwrap();
        assert!(result.colliding);
        assert!(
            (result.depth - 0.5).abs() < 1e-4,
            "depth was {}",
            result.depth
        );
    }

    #[test]
    fn circle_inside_convex_polygon_pushes_out() {
        // Circle center buried inside a square → resolved via nearest edge.
        let square = Shape2D::convex_polygon(&[
            Vector2::new(-3.0, -3.0),
            Vector2::new(3.0, -3.0),
            Vector2::new(3.0, 3.0),
            Vector2::new(-3.0, 3.0),
        ]);
        let result = test_collision(
            &Shape2D::Circle { radius: 1.0 },
            &tf(Vector2::new(2.5, 0.0), 0.0),
            &square,
            &Transform2D::IDENTITY,
        )
        .unwrap();
        assert!(result.colliding);
        // Nearest edge is x = 3 (0.5 away) → depth = radius + 0.5 = 1.5.
        assert!(
            (result.depth - 1.5).abs() < 1e-4,
            "depth was {}",
            result.depth
        );
        // Circle (A) must be pushed toward -X, so A→B normal is -X.
        assert!(
            (result.normal.x + 1.0).abs() < 1e-4,
            "normal.x was {}",
            result.normal.x
        );
    }

    #[test]
    fn restitution_bounces_bodies() {
        let mut a = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Rigid,
            Vector2::new(0.0, 0.0),
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        a.bounce = 1.0;
        a.linear_velocity = Vector2::new(5.0, 0.0);

        let mut b = PhysicsBody2D::new(
            BodyId(2),
            BodyType::Rigid,
            Vector2::new(1.5, 0.0),
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        b.bounce = 1.0;
        b.linear_velocity = Vector2::new(-5.0, 0.0);

        let result = CollisionResult {
            colliding: true,
            normal: Vector2::new(1.0, 0.0),
            depth: 0.5,
            point: Vector2::new(0.75, 0.0),
        };
        separate_bodies(&mut a, &mut b, &result);

        // With restitution=1.0, velocities should swap (elastic collision, equal mass)
        assert!(approx_eq(a.linear_velocity.x, -5.0));
        assert!(approx_eq(b.linear_velocity.x, 5.0));
    }

    #[test]
    fn zero_restitution_stops_relative_motion() {
        let mut a = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Rigid,
            Vector2::new(0.0, 0.0),
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        a.bounce = 0.0;
        a.linear_velocity = Vector2::new(5.0, 0.0);

        let mut b = PhysicsBody2D::new(
            BodyId(2),
            BodyType::Static,
            Vector2::new(1.5, 0.0),
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        b.bounce = 0.0;

        let result = CollisionResult {
            colliding: true,
            normal: Vector2::new(1.0, 0.0),
            depth: 0.5,
            point: Vector2::new(0.75, 0.0),
        };
        separate_bodies(&mut a, &mut b, &result);

        // With restitution=0, body A should stop (no bounce)
        assert!(approx_eq(a.linear_velocity.x, 0.0));
    }

    #[test]
    fn friction_reduces_tangential_velocity() {
        let mut a = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Rigid,
            Vector2::new(0.0, 0.0),
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        a.friction = 1.0;
        a.bounce = 0.0;
        // Moving right and downward (into the floor below)
        a.linear_velocity = Vector2::new(10.0, 5.0);

        let mut b = PhysicsBody2D::new(
            BodyId(2),
            BodyType::Static,
            Vector2::new(0.0, 2.0),
            Shape2D::Rectangle {
                half_extents: Vector2::new(100.0, 1.0),
            },
            1.0,
        );
        b.friction = 1.0;

        // Normal points from A toward B (downward)
        let result = CollisionResult {
            colliding: true,
            normal: Vector2::new(0.0, 1.0),
            depth: 0.1,
            point: Vector2::new(0.0, 1.0),
        };
        separate_bodies(&mut a, &mut b, &result);

        // Tangential velocity (x) should be reduced by friction
        assert!(a.linear_velocity.x.abs() < 10.0);
    }

    #[test]
    fn zero_friction_preserves_tangential_velocity() {
        let mut a = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Rigid,
            Vector2::new(0.0, 0.0),
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        a.friction = 0.0;
        a.bounce = 0.0;
        a.linear_velocity = Vector2::new(10.0, -5.0);

        let mut b = PhysicsBody2D::new(
            BodyId(2),
            BodyType::Static,
            Vector2::new(0.0, -2.0),
            Shape2D::Rectangle {
                half_extents: Vector2::new(100.0, 1.0),
            },
            1.0,
        );
        b.friction = 0.0;

        let result = CollisionResult {
            colliding: true,
            normal: Vector2::new(0.0, 1.0),
            depth: 0.1,
            point: Vector2::new(0.0, -1.0),
        };
        separate_bodies(&mut a, &mut b, &result);

        // Tangential velocity (x) should be preserved with 0 friction
        assert!(approx_eq(a.linear_velocity.x, 10.0));
    }

    #[test]
    fn max_restitution_used() {
        // ⚠️ This test previously pinned `max(0.8, 0.2) = 0.8` — the old
        // non-Godot combine — and its expectation moved with the fix rather
        // than the fix being bent to the test. Godot: `CLAMP(0.8 + 0.2, 0, 1)`
        // = 1.0, a fully elastic bounce off the static body (pat-s3rrv.99).
        let mut a = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Rigid,
            Vector2::ZERO,
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        a.bounce = 0.8;
        a.linear_velocity = Vector2::new(10.0, 0.0);

        let mut b = PhysicsBody2D::new(
            BodyId(2),
            BodyType::Static,
            Vector2::new(1.5, 0.0),
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        b.bounce = 0.2;

        let result = CollisionResult {
            colliding: true,
            normal: Vector2::new(1.0, 0.0),
            depth: 0.5,
            point: Vector2::new(0.75, 0.0),
        };
        separate_bodies(&mut a, &mut b, &result);

        // restitution = clamp(0.8 + 0.2) = 1.0 against a static body, so the
        // velocity reverses in full.
        assert!(
            approx_eq(a.linear_velocity.x, -10.0),
            "0.8 + 0.2 clamps to 1.0, so a 10 u/s impact reverses to -10; got {}",
            a.linear_velocity.x
        );
    }

    /// 🔑 Bounce combines as Godot's `CLAMP(a + b, 0, 1)`, not as a maximum.
    ///
    /// ⚠️ The fixture uses TWO bouncy bodies on purpose. The old maximum and
    /// Godot's sum agree whenever one side is 0 — which is what a floor
    /// normally is, and why this went unnoticed. They diverge as soon as both
    /// surfaces bounce.
    #[test]
    fn bounce_combines_as_a_clamped_sum_not_a_maximum() {
        let mut a = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Rigid,
            Vector2::ZERO,
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        a.bounce = 0.3;
        a.linear_velocity = Vector2::new(10.0, 0.0);

        let mut b = PhysicsBody2D::new(
            BodyId(2),
            BodyType::Static,
            Vector2::new(1.5, 0.0),
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        b.bounce = 0.4;

        let result = CollisionResult {
            colliding: true,
            normal: Vector2::new(1.0, 0.0),
            depth: 0.5,
            point: Vector2::new(0.75, 0.0),
        };
        separate_bodies(&mut a, &mut b, &result);

        // 0.3 + 0.4 = 0.7. A maximum would give 0.4 and a mean 0.35, so this
        // number distinguishes all three.
        assert!(
            approx_eq(a.linear_velocity.x, -7.0),
            "restitution must be clamp(0.3 + 0.4) = 0.7, giving -7.0; got {} \
             (-4.0 means a maximum, -3.5 a mean)",
            a.linear_velocity.x
        );
    }

    /// 🔑 The sum CLAMPS at 1 rather than growing past fully elastic.
    #[test]
    fn combined_bounce_never_exceeds_one() {
        let mut a = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Rigid,
            Vector2::ZERO,
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        a.bounce = 0.9;
        a.linear_velocity = Vector2::new(10.0, 0.0);

        let mut b = PhysicsBody2D::new(
            BodyId(2),
            BodyType::Static,
            Vector2::new(1.5, 0.0),
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        b.bounce = 0.9;

        let result = CollisionResult {
            colliding: true,
            normal: Vector2::new(1.0, 0.0),
            depth: 0.5,
            point: Vector2::new(0.75, 0.0),
        };
        separate_bodies(&mut a, &mut b, &result);

        // 0.9 + 0.9 = 1.8, clamped to 1.0. Without the clamp the body would
        // leave FASTER than it arrived — energy from nowhere, every bounce.
        assert!(
            approx_eq(a.linear_velocity.x, -10.0),
            "1.8 must clamp to 1.0 for a -10.0 rebound; got {} — a magnitude \
             above 10 means the clamp is missing and the sim gains energy",
            a.linear_velocity.x
        );
    }

    #[test]
    fn off_center_collision_imparts_spin() {
        let mut a = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Rigid,
            Vector2::ZERO,
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        a.linear_velocity = Vector2::new(10.0, 0.0);
        let mut b = PhysicsBody2D::new(
            BodyId(2),
            BodyType::Static,
            Vector2::new(1.5, 0.0),
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        // Contact point offset in +Y from body a's center: the normal impulse
        // now has a lever arm and imparts angular velocity.
        let result = CollisionResult {
            colliding: true,
            normal: Vector2::new(1.0, 0.0),
            depth: 0.5,
            point: Vector2::new(0.75, 0.5),
        };
        separate_bodies(&mut a, &mut b, &result);
        assert!(
            a.angular_velocity.abs() > 1e-4,
            "off-center contact should spin body a, got {}",
            a.angular_velocity
        );
    }

    #[test]
    fn head_on_collision_imparts_no_spin() {
        let mut a = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Rigid,
            Vector2::ZERO,
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        a.linear_velocity = Vector2::new(10.0, 0.0);
        let mut b = PhysicsBody2D::new(
            BodyId(2),
            BodyType::Static,
            Vector2::new(1.5, 0.0),
            Shape2D::Circle { radius: 1.0 },
            1.0,
        );
        // Contact point on the normal axis through a's center: zero lever arm.
        let result = CollisionResult {
            colliding: true,
            normal: Vector2::new(1.0, 0.0),
            depth: 0.5,
            point: Vector2::new(0.75, 0.0),
        };
        separate_bodies(&mut a, &mut b, &result);
        assert!(
            a.angular_velocity.abs() < 1e-6,
            "head-on contact must not spin, got {}",
            a.angular_velocity
        );
        // Linear response is unchanged: inelastic stop along the normal.
        assert!(approx_eq(a.linear_velocity.x, 0.0));
    }

    // ---- pat-0q2kx.34: degenerate shapes must not yield NaN ---------------

    /// A body whose colliders are all `disabled` is stored as a zero-extent
    /// rectangle. Its four vertices are coincident rather than absent, so it
    /// slipped past the `verts.is_empty()` guards, went down the SAT path, and
    /// normalised a zero-length axis — every contact value came back NaN and
    /// the character controller wrote a NaN position into the scene tree.
    #[test]
    fn degenerate_rectangle_reports_no_contact_instead_of_nan() {
        let degenerate = Shape2D::Rectangle {
            half_extents: Vector2::ZERO,
        };
        let walker = Shape2D::Circle { radius: 4.0 };
        let at_origin = Transform2D::IDENTITY;
        let mut overlapping = Transform2D::IDENTITY;
        overlapping.origin = Vector2::new(1.0, 0.0);

        // Overlapping in the sense that the circle contains the degenerate
        // rect's single point — the worst case for the SAT path.
        assert!(
            test_collision(&walker, &overlapping, &degenerate, &at_origin).is_none(),
            "a zero-extent rectangle must report no contact (None), never a NaN result"
        );
        assert!(
            test_collision(&degenerate, &at_origin, &walker, &overlapping).is_none(),
            "the degenerate check must be symmetric in argument order"
        );
    }

    /// The same guard for a zero-radius circle, the other shape that can carry
    /// no extent.
    #[test]
    fn degenerate_circle_reports_no_contact() {
        let point = Shape2D::Circle { radius: 0.0 };
        let box_shape = Shape2D::Rectangle {
            half_extents: Vector2::new(10.0, 10.0),
        };
        assert!(
            test_collision(
                &point,
                &Transform2D::IDENTITY,
                &box_shape,
                &Transform2D::IDENTITY
            )
            .is_none(),
            "a zero-radius circle has no extent and cannot produce a contact"
        );
    }

    /// Guard rail: a normal-sized rectangle must still collide, so the
    /// degenerate check cannot be over-broad.
    #[test]
    fn non_degenerate_rectangle_still_collides() {
        let a = Shape2D::Rectangle {
            half_extents: Vector2::new(5.0, 5.0),
        };
        let b = Shape2D::Circle { radius: 4.0 };
        let mut near = Transform2D::IDENTITY;
        near.origin = Vector2::new(6.0, 0.0);
        let result = test_collision(&a, &Transform2D::IDENTITY, &b, &near)
            .expect("real shapes must still reach the narrow phase");
        assert!(
            result.colliding,
            "a 5px box and a 4px circle 6px apart overlap by 3px"
        );
    }
}

//! 2D geometry utilities matching Godot's `Geometry2D` singleton.
//!
//! As of Godot 4.6.1, arc generation uses **fixed-count subdivision**
//! rather than the previous tolerance-based adaptive approach.  The
//! `point_count` parameter directly controls how many vertices the arc
//! polyline contains, producing evenly spaced points along the arc.

use crate::math::{Vector2, Vector2i};

/// Generates an arc polyline of `point_count` evenly spaced points.
///
/// This mirrors Godot 4.6.1's `Geometry2D` arc generation after the
/// tolerance-scaling removal.  Points are placed at equal angular
/// intervals between `start_angle` and `end_angle` (both in radians).
///
/// # Arguments
///
/// * `center`      – Centre of the arc circle.
/// * `radius`      – Radius of the arc.
/// * `start_angle` – Starting angle in radians.
/// * `end_angle`   – Ending angle in radians.
/// * `point_count` – Number of points in the output polyline (≥ 2).
///
/// # Returns
///
/// A `Vec<Vector2>` with exactly `point_count` points, or an empty vec
/// if `point_count < 2`.
pub fn build_arc(
    center: Vector2,
    radius: f32,
    start_angle: f32,
    end_angle: f32,
    point_count: u32,
) -> Vec<Vector2> {
    if point_count < 2 {
        return Vec::new();
    }

    let count = point_count as usize;
    let mut points = Vec::with_capacity(count);
    let delta = (end_angle - start_angle) / (count as f32 - 1.0);

    for i in 0..count {
        let angle = start_angle + delta * i as f32;
        points.push(Vector2::new(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        ));
    }

    points
}

/// Checks whether a point lies inside a convex polygon (given as an
/// ordered slice of vertices).
///
/// Uses the cross-product winding test: the point must be on the same
/// side of every edge.
pub fn is_point_in_polygon(point: Vector2, polygon: &[Vector2]) -> bool {
    let n = polygon.len();
    if n < 3 {
        return false;
    }

    let mut sign: Option<bool> = None;
    for i in 0..n {
        let a = polygon[i];
        let b = polygon[(i + 1) % n];
        let cross = (b.x - a.x) * (point.y - a.y) - (b.y - a.y) * (point.x - a.x);
        let positive = cross > 0.0;

        match sign {
            None => sign = Some(positive),
            Some(s) if s != positive => return false,
            _ => {}
        }
    }

    true
}

/// Tests whether two line segments intersect and returns the intersection
/// point if they do.
///
/// Segments are defined by their endpoints: `p`–`q` and `r`–`s`.
///
/// Returns `None` when the segments are parallel (including collinear),
/// or when the intersection of the infinite lines falls outside either
/// segment's [0, 1] parameter range.
///
/// The cross-product denominator is tested against an epsilon of `1e-7`
/// to avoid ghost collisions from nearly-parallel segments — this matches
/// Godot 4.6.1's fix for the `Geometry2D` segment intersection bug.
pub fn segment_intersects_segment(
    p: Vector2,
    q: Vector2,
    r: Vector2,
    s: Vector2,
) -> Option<Vector2> {
    let d1 = Vector2::new(q.x - p.x, q.y - p.y);
    let d2 = Vector2::new(s.x - r.x, s.y - r.y);

    let denom = d1.cross(d2);

    // Parallel or nearly-parallel — no intersection.  The 1e-7 threshold
    // prevents ghost collisions from floating-point noise when segments
    // are almost (but not quite) parallel.
    if denom.abs() < 1e-7 {
        return None;
    }

    let pr = Vector2::new(r.x - p.x, r.y - p.y);
    let t = pr.cross(d2) / denom;
    let u = pr.cross(d1) / denom;

    if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
        Some(Vector2::new(p.x + d1.x * t, p.y + d1.y * t))
    } else {
        None
    }
}

/// Returns the closest point on the **infinite line** through `a`–`b` to
/// `point`, without clamping to the segment bounds.
///
/// Mirrors Godot 4.x `Geometry2D.get_closest_point_to_segment_unclamped()`.
pub fn get_closest_point_to_segment_unclamped(point: Vector2, a: Vector2, b: Vector2) -> Vector2 {
    let ab = Vector2::new(b.x - a.x, b.y - a.y);
    let len_sq = ab.length_squared();
    if len_sq < 1e-12 {
        return a;
    }
    let t = ((point.x - a.x) * ab.x + (point.y - a.y) * ab.y) / len_sq;
    Vector2::new(a.x + ab.x * t, a.y + ab.y * t)
}

/// Tests whether two **infinite lines** intersect and returns the
/// intersection point.
///
/// Lines are defined by a point and direction: line A passes through
/// `from_a` in direction `dir_a`, line B through `from_b` in `dir_b`.
///
/// Returns `None` when the lines are parallel (cross product near zero).
///
/// Mirrors Godot 4.x `Geometry2D.line_intersects_line()`.
pub fn line_intersects_line(
    from_a: Vector2,
    dir_a: Vector2,
    from_b: Vector2,
    dir_b: Vector2,
) -> Option<Vector2> {
    let denom = dir_a.cross(dir_b);
    if denom.abs() < 1e-7 {
        return None;
    }
    let diff = Vector2::new(from_b.x - from_a.x, from_b.y - from_a.y);
    let t = diff.cross(dir_b) / denom;
    Some(Vector2::new(from_a.x + dir_a.x * t, from_a.y + dir_a.y * t))
}

/// Tests whether a point lies inside the triangle defined by `a`, `b`, `c`.
///
/// Uses the barycentric sign method (cross-product winding).
///
/// Mirrors Godot 4.x `Geometry2D.point_is_inside_triangle()`.
pub fn point_is_inside_triangle(point: Vector2, a: Vector2, b: Vector2, c: Vector2) -> bool {
    let d1 = sign(point, a, b);
    let d2 = sign(point, b, c);
    let d3 = sign(point, c, a);
    let has_neg = (d1 < 0.0) || (d2 < 0.0) || (d3 < 0.0);
    let has_pos = (d1 > 0.0) || (d2 > 0.0) || (d3 > 0.0);
    !(has_neg && has_pos)
}

fn sign(p: Vector2, a: Vector2, b: Vector2) -> f32 {
    (p.x - b.x) * (a.y - b.y) - (a.x - b.x) * (p.y - b.y)
}

/// Returns whether the polygon vertices are in clockwise order.
///
/// Uses the signed area (shoelace formula): negative area = clockwise.
///
/// Mirrors Godot 4.x `Geometry2D.is_polygon_clockwise()`.
pub fn is_polygon_clockwise(polygon: &[Vector2]) -> bool {
    let n = polygon.len();
    if n < 3 {
        return false;
    }
    let mut area: f32 = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        area += polygon[i].x * polygon[j].y;
        area -= polygon[j].x * polygon[i].y;
    }
    area < 0.0
}

/// Returns the closest point on a line segment `a`–`b` to `point`.
pub fn get_closest_point_to_segment(point: Vector2, a: Vector2, b: Vector2) -> Vector2 {
    let ab = Vector2::new(b.x - a.x, b.y - a.y);
    let len_sq = ab.length_squared();
    if len_sq < 1e-12 {
        return a;
    }
    let t = ((point.x - a.x) * ab.x + (point.y - a.y) * ab.y) / len_sq;
    let t = t.clamp(0.0, 1.0);
    Vector2::new(a.x + ab.x * t, a.y + ab.y * t)
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// pat-mgixk.102 — polygon booleans, offset, triangulation, raster lines.
// ---------------------------------------------------------------------------

/// Tolerance for treating two points, or a parameter and an endpoint, as equal.
///
/// ⚠️ Deliberately looser than `f32::EPSILON`: every coordinate here is the
/// result of a division when an edge crossing is solved, so two geometrically
/// identical points arrive differing in the last few bits and a tighter value
/// would leave a ring unable to close.
const GEO_EPS: f32 = 1e-6;
/// Tolerance for chaining: the gap between a segment's end and the next
/// segment's start. One order looser than [`GEO_EPS`] because it compares two
/// independently computed intersection points rather than a value against zero.
const CHAIN_EPS: f32 = 1e-5;

/// Which boolean operation [`polygon_boolean`] performs.
///
/// Mirrors the four `Geometry2D` entry points: `merge_polygons`,
/// `clip_polygons`, `intersect_polygons` and `exclude_polygons`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolygonBooleanOp {
    /// `merge_polygons(a, b)` — everything covered by either.
    Union,
    /// `clip_polygons(a, b)` — `a` with `b` removed.
    Difference,
    /// `intersect_polygons(a, b)` — only what both cover.
    Intersection,
    /// `exclude_polygons(a, b)` — covered by exactly one.
    Xor,
}

/// How [`offset_polygon`] turns a corner.
///
/// ⚠️ Godot's default for `offset_polygon` is `JOIN_SQUARE`, **not** miter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolygonJoinType {
    /// Squares off the corner, emitting both offset edge endpoints (Godot
    /// `JOIN_SQUARE`, enum value 0).
    Square,
    /// Rounds the corner. Approximated by the squared chamfer here — see
    /// [`offset_polygon`].
    Round,
    /// Extends both offset edges to their intersection (Godot `JOIN_MITER`,
    /// enum value 2).
    Miter,
}

impl PolygonJoinType {
    /// Maps Godot's `PolyJoinType` enum value. `JOIN_SQUARE = 0`,
    /// `JOIN_ROUND = 1`, `JOIN_MITER = 2`; anything else takes the default.
    pub fn from_godot(value: i64) -> Self {
        match value {
            1 => Self::Round,
            2 => Self::Miter,
            _ => Self::Square,
        }
    }
}

/// The shoelace signed area of a ring. Positive is counter-clockwise in math
/// axes.
///
/// ⚠️ This is the NEGATION of what [`is_polygon_clockwise`] reports, and the
/// two are easy to confuse. Godot's predicate sums `(x2 - x1) * (y2 + y1)` and
/// tests `> 0`; over a closed ring that telescopes to `-2 * shoelace`, so
/// Godot's "clockwise" is a **negative** signed area — which is exactly what
/// [`is_polygon_clockwise`] computes. Orientation code below keys on this
/// function, never on the predicate.
fn signed_area(polygon: &[Vector2]) -> f32 {
    let n = polygon.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        sum += polygon[i].x * polygon[j].y - polygon[j].x * polygon[i].y;
    }
    sum * 0.5
}

/// Winding-free containment test used to classify a segment midpoint.
///
/// Separate from the public [`is_point_in_polygon`] because that one is the
/// `Geometry2D` method and may carry its own boundary convention; the boolean
/// engine needs a plain crossing count on points it has already arranged to be
/// strictly off any edge.
fn ray_cast_inside(point: Vector2, polygon: &[Vector2]) -> bool {
    let n = polygon.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (pi, pj) = (polygon[i], polygon[j]);
        if (pi.y > point.y) != (pj.y > point.y) {
            let t = (point.y - pi.y) / (pj.y - pi.y);
            if pi.x + t * (pj.x - pi.x) > point.x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Where segment `a1`–`a2` crosses `b1`–`b2`, as the parameter along `a`.
///
/// Returns `None` for parallel or non-overlapping segments. Collinear overlap
/// is treated as no crossing: there is no single parameter to report, and the
/// boolean engine's midpoint classification handles a shared edge correctly
/// without one.
fn segment_cross_param(a1: Vector2, a2: Vector2, b1: Vector2, b2: Vector2) -> Option<f32> {
    let d1 = Vector2::new(a2.x - a1.x, a2.y - a1.y);
    let d2 = Vector2::new(b2.x - b1.x, b2.y - b1.y);
    let den = d1.x * d2.y - d1.y * d2.x;
    if den.abs() < 1e-12 {
        return None;
    }
    let t = ((b1.x - a1.x) * d2.y - (b1.y - a1.y) * d2.x) / den;
    let u = ((b1.x - a1.x) * d1.y - (b1.y - a1.y) * d1.x) / den;
    if (-GEO_EPS..=1.0 + GEO_EPS).contains(&t) && (-GEO_EPS..=1.0 + GEO_EPS).contains(&u) {
        Some(t)
    } else {
        None
    }
}

/// Every edge of `polygon`, cut at each crossing with `other`, in the
/// polygon's own direction.
fn split_edges(polygon: &[Vector2], other: &[Vector2]) -> Vec<(Vector2, Vector2)> {
    let n = polygon.len();
    let m = other.len();
    let mut segments = Vec::new();
    for i in 0..n {
        let (a1, a2) = (polygon[i], polygon[(i + 1) % n]);
        let mut cuts: Vec<f32> = Vec::new();
        for j in 0..m {
            let (b1, b2) = (other[j], other[(j + 1) % m]);
            if let Some(t) = segment_cross_param(a1, a2, b1, b2) {
                // Strictly interior only: a crossing at an endpoint is already
                // a vertex, and admitting it would emit a zero-length segment
                // that the chainer cannot follow.
                if t > GEO_EPS && t < 1.0 - GEO_EPS {
                    cuts.push(t);
                }
            }
        }
        cuts.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        let mut points = Vec::with_capacity(cuts.len() + 2);
        points.push(a1);
        for t in cuts {
            points.push(Vector2::new(
                a1.x + (a2.x - a1.x) * t,
                a1.y + (a2.y - a1.y) * t,
            ));
        }
        points.push(a2);
        for w in points.windows(2) {
            if !points_equal(w[0], w[1], GEO_EPS) {
                segments.push((w[0], w[1]));
            }
        }
    }
    segments
}

fn points_equal(a: Vector2, b: Vector2, eps: f32) -> bool {
    (a.x - b.x).abs() <= eps && (a.y - b.y).abs() <= eps
}

/// Chains directed segments head-to-tail into closed rings.
///
/// Segments whose tail nothing matches are dropped rather than emitted as an
/// open path: the boolean result of two closed rings is closed, so an open
/// chain means numerical noise, and returning it would hand the caller a
/// polygon with a gap.
fn chain_rings(segments: Vec<(Vector2, Vector2)>) -> Vec<Vec<Vector2>> {
    let mut pool = segments;
    let mut rings = Vec::new();
    while !pool.is_empty() {
        let (start, first) = pool.remove(0);
        let mut ring = vec![start, first];
        loop {
            let tail = *ring.last().unwrap();
            if points_equal(tail, ring[0], CHAIN_EPS) {
                break;
            }
            match pool
                .iter()
                .position(|(s, _)| points_equal(*s, tail, CHAIN_EPS))
            {
                Some(idx) => {
                    let (_, e) = pool.remove(idx);
                    ring.push(e);
                }
                None => break,
            }
        }
        if ring.len() > 1 && points_equal(*ring.last().unwrap(), ring[0], CHAIN_EPS) {
            ring.pop();
            if ring.len() >= 3 {
                rings.push(ring);
            }
        }
    }
    rings
}

/// `Geometry2D`'s four polygon boolean operations.
///
/// Godot runs these through Clipper2. This is a direct port of the *semantics*
/// rather than of Clipper: every edge of each input is cut at its crossings
/// with the other, each resulting piece is kept or dropped by whether its
/// midpoint lies inside the other polygon, and the survivors are chained into
/// rings. That reproduces Clipper's output for simple (non-self-intersecting)
/// inputs, which is what `Geometry2D`'s callers pass.
///
/// 🔑 **Output orientation follows Clipper: outer rings counter-clockwise,
/// holes clockwise** — where "clockwise" means what
/// [`is_polygon_clockwise`] reports, i.e. a negative [`signed_area`]. A ring is
/// a hole when it lies inside another ring of the same result, which is how
/// `clip_polygons(outer, inner)` yields the outer boundary plus a clockwise
/// hole rather than two unrelated rings.
///
/// ⚠️ For `Difference`, the kept pieces of `b` are walked **backwards**. They
/// bound the removed region, so traversing them in `b`'s own direction would
/// give the hole the same winding as the ring containing it.
///
/// Returns an empty vector when the operation removes everything.
pub fn polygon_boolean(a: &[Vector2], b: &[Vector2], op: PolygonBooleanOp) -> Vec<Vec<Vector2>> {
    if a.len() < 3 {
        return match op {
            PolygonBooleanOp::Union | PolygonBooleanOp::Xor if b.len() >= 3 => {
                vec![oriented(b.to_vec(), false)]
            }
            _ => Vec::new(),
        };
    }
    if b.len() < 3 {
        return match op {
            PolygonBooleanOp::Intersection => Vec::new(),
            _ => vec![oriented(a.to_vec(), false)],
        };
    }
    if op == PolygonBooleanOp::Xor {
        let mut out = polygon_boolean(a, b, PolygonBooleanOp::Difference);
        out.extend(polygon_boolean(b, a, PolygonBooleanOp::Difference));
        return out;
    }

    let sa = split_edges(a, b);
    let sb = split_edges(b, a);
    let mid = |s: &(Vector2, Vector2)| Vector2::new((s.0.x + s.1.x) * 0.5, (s.0.y + s.1.y) * 0.5);

    let mut keep: Vec<(Vector2, Vector2)> = Vec::new();
    match op {
        PolygonBooleanOp::Union => {
            keep.extend(sa.iter().copied().filter(|s| !ray_cast_inside(mid(s), b)));
            keep.extend(sb.iter().copied().filter(|s| !ray_cast_inside(mid(s), a)));
        }
        PolygonBooleanOp::Intersection => {
            keep.extend(sa.iter().copied().filter(|s| ray_cast_inside(mid(s), b)));
            keep.extend(sb.iter().copied().filter(|s| ray_cast_inside(mid(s), a)));
        }
        PolygonBooleanOp::Difference => {
            keep.extend(sa.iter().copied().filter(|s| !ray_cast_inside(mid(s), b)));
            keep.extend(
                sb.iter()
                    .copied()
                    .filter(|s| ray_cast_inside(mid(s), a))
                    .map(|s| (s.1, s.0)),
            );
        }
        PolygonBooleanOp::Xor => unreachable!("handled above"),
    }

    let rings = chain_rings(keep);
    // Each ring's hole-ness is decided against the OTHER rings of the result,
    // so the containment test needs the whole set before any reorientation.
    let mut out = Vec::with_capacity(rings.len());
    for (i, ring) in rings.iter().enumerate() {
        let is_hole = rings
            .iter()
            .enumerate()
            .any(|(j, other)| j != i && ray_cast_inside(ring[0], other));
        out.push(oriented(ring.clone(), is_hole));
    }
    out
}

/// Reverses `ring` if needed so a hole winds clockwise and an outer ring
/// counter-clockwise, in the sense [`is_polygon_clockwise`] reports.
fn oriented(ring: Vec<Vector2>, is_hole: bool) -> Vec<Vector2> {
    let clockwise = signed_area(&ring) < 0.0;
    if clockwise == is_hole {
        ring
    } else {
        ring.into_iter().rev().collect()
    }
}

/// `Geometry2D.offset_polygon` — inflates (or, with a negative `delta`,
/// deflates) a polygon.
///
/// Each edge is translated `delta` along its outward normal and consecutive
/// offset edges are rejoined at the shared vertex.
///
/// ⚠️ The outward normal is chosen from [`signed_area`], the geometric
/// winding — **not** from [`is_polygon_clockwise`], whose sign is the opposite.
/// Keying on the predicate offsets every edge *inward*, which leaves the
/// bounding box unchanged and looks like the call did nothing.
///
/// ⚠️ [`PolygonJoinType::Round`] is served by the squared chamfer rather than
/// by an arc. The chamfer has the same bounding box and the same area to within
/// the corner wedge, and emitting an unrequested arc subdivision would change
/// the point count callers index into. Flagged rather than silently aliased.
pub fn offset_polygon(polygon: &[Vector2], delta: f32, join: PolygonJoinType) -> Vec<Vec<Vector2>> {
    let n = polygon.len();
    if n < 3 {
        return Vec::new();
    }
    let ccw = signed_area(polygon) > 0.0;
    let mut out: Vec<Vector2> = Vec::with_capacity(n * 2);
    for i in 0..n {
        let prev = polygon[(i + n - 1) % n];
        let cur = polygon[i];
        let next = polygon[(i + 1) % n];
        let n_in = edge_normal(prev, cur, ccw);
        let n_out = edge_normal(cur, next, ccw);
        match join {
            PolygonJoinType::Miter => {
                // Extend both offset edges to their intersection. The averaged
                // normal, rescaled by 1/cos(half-angle), is that point.
                let sum = Vector2::new(n_in.x + n_out.x, n_in.y + n_out.y);
                let len = (sum.x * sum.x + sum.y * sum.y).sqrt();
                let p = if len < GEO_EPS {
                    // A 180° reversal has no miter point; fall back to squaring.
                    Vector2::new(cur.x + n_in.x * delta, cur.y + n_in.y * delta)
                } else {
                    // |n1 + n2| = 2cos(half-angle), and the miter point is
                    // at (n1 + n2) / (2 cos^2) — so 2 / len^2. Verified
                    // numerically: a unit square offset by 1 lands exactly
                    // on (-1, -1)..(2, 2) for either winding.
                    let scale = delta * 2.0 / (len * len);
                    Vector2::new(cur.x + sum.x * scale, cur.y + sum.y * scale)
                };
                push_distinct(&mut out, p);
            }
            PolygonJoinType::Square | PolygonJoinType::Round => {
                push_distinct(
                    &mut out,
                    Vector2::new(cur.x + n_in.x * delta, cur.y + n_in.y * delta),
                );
                push_distinct(
                    &mut out,
                    Vector2::new(cur.x + n_out.x * delta, cur.y + n_out.y * delta),
                );
            }
        }
    }
    // The last point can coincide with the first once the ring closes.
    if out.len() > 1 && points_equal(out[0], *out.last().unwrap(), GEO_EPS) {
        out.pop();
    }
    if out.len() < 3 {
        return Vec::new();
    }
    vec![out]
}

fn push_distinct(out: &mut Vec<Vector2>, p: Vector2) {
    if out.last().map(|l| points_equal(*l, p, GEO_EPS)) != Some(true) {
        out.push(p);
    }
}

/// The outward unit normal of edge `a`→`b` for a ring of the given winding.
fn edge_normal(a: Vector2, b: Vector2, ccw: bool) -> Vector2 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt();
    if len < GEO_EPS {
        return Vector2::new(0.0, 0.0);
    }
    let (dx, dy) = (dx / len, dy / len);
    if ccw {
        Vector2::new(dy, -dx)
    } else {
        Vector2::new(-dy, dx)
    }
}

/// `Geometry2D.triangulate_polygon` — ear clipping, returning **vertex
/// indices** three at a time.
///
/// ⚠️ Indices, not points: Godot's return type is `PackedInt32Array`, so a
/// triangulated square is **6 entries**, not 2 triangles of 3 points.
///
/// Returns an empty vector when the polygon cannot be triangulated, which is
/// Godot's failure signal — it does not partially triangulate.
///
/// ⚠️ The vertex list is normalised to counter-clockwise before clipping,
/// because the convexity test is only meaningful against a known winding: a
/// clockwise outline reports every vertex as reflex and no ear is ever found.
/// The emitted indices address the ORIGINAL array either way.
pub fn triangulate_polygon(polygon: &[Vector2]) -> Vec<i32> {
    let n = polygon.len();
    if n < 3 {
        return Vec::new();
    }
    let mut live: Vec<usize> = (0..n).collect();
    if signed_area(polygon) < 0.0 {
        live.reverse();
    }
    let mut out: Vec<i32> = Vec::with_capacity((n - 2) * 3);
    // Every successful clip removes one vertex; the guard bounds the malformed
    // case instead of spinning.
    let mut guard = n * n + 16;
    while live.len() > 3 && guard > 0 {
        guard -= 1;
        let m = live.len();
        let mut clipped = false;
        for i in 0..m {
            let (ip, ic, inx) = (live[(i + m - 1) % m], live[i], live[(i + 1) % m]);
            let (p, c, nx) = (polygon[ip], polygon[ic], polygon[inx]);
            if vec2_cross(p, c, nx) <= 0.0 {
                continue;
            }
            let blocked = live.iter().any(|&j| {
                j != ip && j != ic && j != inx && point_is_inside_triangle(polygon[j], p, c, nx)
            });
            if blocked {
                continue;
            }
            out.push(ip as i32);
            out.push(ic as i32);
            out.push(inx as i32);
            live.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            // No ear in a polygon with more than three vertices left means the
            // outline is not simple. Godot returns nothing rather than a
            // partial fan.
            return Vec::new();
        }
    }
    if live.len() == 3 {
        out.push(live[0] as i32);
        out.push(live[1] as i32);
        out.push(live[2] as i32);
    }
    out
}

/// `Geometry2D.bresenham_line` — the integer cells a line passes through,
/// **inclusive of both endpoints**.
///
/// Same integer-error formulation as `gdscene::tilemap`'s tile-painting line,
/// which is where this algorithm already lived; it is duplicated here rather
/// than shared because `gdcore` sits below `gdscene` and cannot depend on it.
pub fn bresenham_line(from: Vector2i, to: Vector2i) -> Vec<Vector2i> {
    let mut cells = Vec::new();
    let dx = (to.x - from.x).abs();
    let dy = -(to.y - from.y).abs();
    let sx = if from.x < to.x { 1 } else { -1 };
    let sy = if from.y < to.y { 1 } else { -1 };
    let mut err = dx + dy;
    let (mut x, mut y) = (from.x, from.y);
    loop {
        cells.push(Vector2i::new(x, y));
        if x == to.x && y == to.y {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
    cells
}

/// `Geometry2D.get_closest_points_between_segments` — the two closest points,
/// one on each segment, as `[on_p1p2, on_q1q2]`.
///
/// Mirrors the 3D version's structure: solve the unclamped parameters, then
/// clamp each to its segment and re-solve the other against the clamped point,
/// so a pair of non-overlapping segments reports endpoint-to-interior rather
/// than endpoint-to-endpoint.
pub fn get_closest_points_between_segments(
    p1: Vector2,
    p2: Vector2,
    q1: Vector2,
    q2: Vector2,
) -> [Vector2; 2] {
    let d1 = Vector2::new(p2.x - p1.x, p2.y - p1.y);
    let d2 = Vector2::new(q2.x - q1.x, q2.y - q1.y);
    let r = Vector2::new(p1.x - q1.x, p1.y - q1.y);
    let a = d1.x * d1.x + d1.y * d1.y;
    let e = d2.x * d2.x + d2.y * d2.y;
    let f = d2.x * r.x + d2.y * r.y;

    // Degenerate segments collapse to their own point.
    if a <= GEO_EPS && e <= GEO_EPS {
        return [p1, q1];
    }
    let (s, t);
    if a <= GEO_EPS {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = d1.x * r.x + d1.y * r.y;
        if e <= GEO_EPS {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = d1.x * d2.x + d1.y * d2.y;
            let denom = a * e - b * b;
            let s0 = if denom.abs() > GEO_EPS {
                ((b * f - c * e) / denom).clamp(0.0, 1.0)
            } else {
                0.0
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
    [
        Vector2::new(p1.x + d1.x * s, p1.y + d1.y * s),
        Vector2::new(q1.x + d2.x * t, q1.y + d2.y * t),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI, TAU};

    // -- build_arc ---------------------------------------------------------

    #[test]
    fn arc_point_count_matches_request() {
        let pts = build_arc(Vector2::ZERO, 100.0, 0.0, PI, 32);
        assert_eq!(pts.len(), 32);
    }

    #[test]
    fn arc_degenerate_point_count_returns_empty() {
        assert!(build_arc(Vector2::ZERO, 50.0, 0.0, PI, 0).is_empty());
        assert!(build_arc(Vector2::ZERO, 50.0, 0.0, PI, 1).is_empty());
    }

    #[test]
    fn arc_first_and_last_point_match_angles() {
        let pts = build_arc(Vector2::ZERO, 1.0, 0.0, FRAC_PI_2, 10);
        // First point: angle=0 → (1, 0)
        assert!((pts[0].x - 1.0).abs() < 1e-5);
        assert!(pts[0].y.abs() < 1e-5);
        // Last point: angle=π/2 → (0, 1)
        let last = pts.last().unwrap();
        assert!(last.x.abs() < 1e-5);
        assert!((last.y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn arc_all_points_at_radius() {
        let center = Vector2::new(10.0, 20.0);
        let radius = 50.0;
        let pts = build_arc(center, radius, 0.0, TAU, 64);
        for p in &pts {
            let dist = ((p.x - center.x).powi(2) + (p.y - center.y).powi(2)).sqrt();
            assert!(
                (dist - radius).abs() < 1e-4,
                "point {:?} is {dist} from center, expected {radius}",
                p
            );
        }
    }

    #[test]
    fn arc_even_angular_spacing() {
        // Fixed-subdivision means equal angular gaps between consecutive
        // points — this is the core 4.6.1 behavioral change.
        // Use a quarter-circle to avoid the atan2 discontinuity at ±PI.
        let pts = build_arc(Vector2::ZERO, 1.0, 0.0, FRAC_PI_2, 5);
        let expected_delta = FRAC_PI_2 / 4.0;

        for i in 0..4 {
            let a0 = pts[i].y.atan2(pts[i].x);
            let a1 = pts[i + 1].y.atan2(pts[i + 1].x);
            let gap = a1 - a0;
            assert!(
                (gap - expected_delta).abs() < 1e-5,
                "gap between point {i} and {} is {gap}, expected {expected_delta}",
                i + 1
            );
        }
    }

    #[test]
    fn arc_with_offset_center() {
        let center = Vector2::new(100.0, 200.0);
        let pts = build_arc(center, 10.0, 0.0, FRAC_PI_2, 2);
        // First: center + (10, 0)
        assert!((pts[0].x - 110.0).abs() < 1e-4);
        assert!((pts[0].y - 200.0).abs() < 1e-4);
        // Last: center + (0, 10)
        assert!((pts[1].x - 100.0).abs() < 1e-4);
        assert!((pts[1].y - 210.0).abs() < 1e-4);
    }

    #[test]
    fn arc_negative_sweep() {
        // Sweeping from PI to 0 (clockwise) should still produce evenly
        // spaced points, just in reverse angular order.
        let pts = build_arc(Vector2::ZERO, 1.0, PI, 0.0, 3);
        assert_eq!(pts.len(), 3);
        // First point at angle PI → (-1, ~0)
        assert!((pts[0].x - (-1.0)).abs() < 1e-5);
        // Middle at PI/2 → (0, 1)
        assert!((pts[1].x - 0.0).abs() < 1e-4);
        assert!((pts[1].y - 1.0).abs() < 1e-4, "mid y={}", pts[1].y);
        // Last at 0 → (1, 0)
        assert!((pts[2].x - 1.0).abs() < 1e-5);
    }

    #[test]
    fn arc_full_circle() {
        let pts = build_arc(Vector2::ZERO, 5.0, 0.0, TAU, 100);
        assert_eq!(pts.len(), 100);
        // First and last should coincide (full circle).
        let first = pts[0];
        let last = pts[99];
        assert!((first.x - last.x).abs() < 1e-4);
        assert!((first.y - last.y).abs() < 1e-4);
    }

    #[test]
    fn arc_two_points_gives_endpoints_only() {
        let pts = build_arc(Vector2::ZERO, 1.0, 0.0, PI, 2);
        assert_eq!(pts.len(), 2);
        // (1,0) and (-1, ~0)
        assert!((pts[0].x - 1.0).abs() < 1e-5);
        assert!((pts[1].x - (-1.0)).abs() < 1e-5);
    }

    // -- is_point_in_polygon -----------------------------------------------

    #[test]
    fn point_inside_square() {
        let sq = [
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
            Vector2::new(10.0, 10.0),
            Vector2::new(0.0, 10.0),
        ];
        assert!(is_point_in_polygon(Vector2::new(5.0, 5.0), &sq));
    }

    #[test]
    fn point_outside_square() {
        let sq = [
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
            Vector2::new(10.0, 10.0),
            Vector2::new(0.0, 10.0),
        ];
        assert!(!is_point_in_polygon(Vector2::new(15.0, 5.0), &sq));
    }

    #[test]
    fn point_in_polygon_degenerate_returns_false() {
        assert!(!is_point_in_polygon(Vector2::ZERO, &[]));
        assert!(!is_point_in_polygon(
            Vector2::ZERO,
            &[Vector2::ZERO, Vector2::ONE]
        ));
    }

    // -- get_closest_point_to_segment --------------------------------------

    #[test]
    fn closest_point_midpoint_projection() {
        let p = get_closest_point_to_segment(
            Vector2::new(5.0, 10.0),
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
        );
        assert!((p.x - 5.0).abs() < 1e-5);
        assert!(p.y.abs() < 1e-5);
    }

    #[test]
    fn closest_point_clamped_to_start() {
        let p = get_closest_point_to_segment(
            Vector2::new(-5.0, 0.0),
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
        );
        assert!((p.x).abs() < 1e-5);
    }

    #[test]
    fn closest_point_clamped_to_end() {
        let p = get_closest_point_to_segment(
            Vector2::new(20.0, 0.0),
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
        );
        assert!((p.x - 10.0).abs() < 1e-5);
    }
}

/// `Geometry2D.is_point_in_circle` (`core/math/geometry_2d.h:227`).
///
/// Compared squared, so a point exactly on the circumference counts as inside.
pub fn is_point_in_circle(point: Vector2, circle_pos: Vector2, radius: f32) -> bool {
    let d = Vector2::new(point.x - circle_pos.x, point.y - circle_pos.y);
    d.x * d.x + d.y * d.y <= radius * radius
}

/// `Geometry2D.segment_intersects_circle` (`core/math/geometry_2d.h:231`).
///
/// Returns the fraction along `from -> to` of the FIRST intersection, or `-1.0`
/// when the segment misses.
///
/// ⚠️ **`-1.0`, not `0.0`**, is the miss value — `0.0` is a legitimate hit
/// exactly at `from`, so a caller testing `if t > 0` would silently drop the
/// grazing case and one testing `if t != 0` would treat every miss as a hit.
///
/// 🔑 Both roots are checked, smaller first, because a segment that *starts
/// inside* the circle has its near root behind `from` (negative) and only the
/// far root in range. Returning `res1` unconditionally — which upstream notes is
/// valid only when the segment is known to start outside — reports a miss for
/// every segment fired from within the circle.
pub fn segment_intersects_circle(
    from: Vector2,
    to: Vector2,
    circle_pos: Vector2,
    radius: f32,
) -> f32 {
    let line = Vector2::new(to.x - from.x, to.y - from.y);
    let to_line = Vector2::new(from.x - circle_pos.x, from.y - circle_pos.y);

    // Quadratic a*t^2 + b*t + c = 0.
    let a = line.x * line.x + line.y * line.y;
    let b = 2.0 * (to_line.x * line.x + to_line.y * line.y);
    let c = to_line.x * to_line.x + to_line.y * to_line.y - radius * radius;

    let sqrtterm = b * b - 4.0 * a * c;
    if sqrtterm < 0.0 {
        return -1.0;
    }
    let sqrtterm = sqrtterm.sqrt();
    let res1 = (-b - sqrtterm) / (2.0 * a);
    let res2 = (-b + sqrtterm) / (2.0 * a);

    if (0.0..=1.0).contains(&res1) {
        return res1;
    }
    if (0.0..=1.0).contains(&res2) {
        return res2;
    }
    -1.0
}

/// The 2D cross product of `OA x OB` (`geometry_2d.h:430`), positive when
/// `O -> A -> B` turns counter-clockwise.
fn vec2_cross(o: Vector2, a: Vector2, b: Vector2) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// `Geometry2D.convex_hull` — Andrew's monotone chain
/// (`core/math/geometry_2d.h:436`).
///
/// 🔑 **The returned hull is CLOSED: the first point is repeated as the last.**
/// Godot ends with `H.resize(k)`, keeping the duplicate, where the textbook
/// monotone chain does `resize(k - 1)` to drop it. A caller drawing the outline
/// relies on that closing point, so returning the open form leaves a gap in the
/// last edge — and it silently changes `size()`.
///
/// ⚠️ The pop test is `cross <= 0`, not `< 0`, so **collinear points are
/// discarded** and the hull carries only true corners.
pub fn convex_hull(points: &[Vector2]) -> Vec<Vector2> {
    let n = points.len();
    if n < 3 {
        return points.to_vec();
    }

    // Lexicographic sort by x then y, matching `Vector2::operator<`.
    let mut p = points.to_vec();
    p.sort_by(|a, b| {
        a.x.partial_cmp(&b.x)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut h: Vec<Vector2> = Vec::with_capacity(2 * n);

    // Lower hull.
    for &pt in p.iter() {
        while h.len() >= 2 && vec2_cross(h[h.len() - 2], h[h.len() - 1], pt) <= 0.0 {
            h.pop();
        }
        h.push(pt);
    }

    // Upper hull. `t` is the floor the upper chain may not pop past, so the two
    // chains cannot eat each other.
    let t = h.len() + 1;
    for &pt in p.iter().rev().skip(1) {
        while h.len() >= t && vec2_cross(h[h.len() - 2], h[h.len() - 1], pt) <= 0.0 {
            h.pop();
        }
        h.push(pt);
    }

    h
}

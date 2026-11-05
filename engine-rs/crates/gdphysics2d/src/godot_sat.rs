//! A line-for-line port of Godot 4.6's 2D narrow phase
//! (`modules/godot_physics_2d/godot_collision_solver_2d.cpp`,
//! `godot_collision_solver_2d_sat.cpp` and the shape support/projection
//! functions in `godot_shape_2d.{h,cpp}`).
//!
//! 🔑 Why a second narrow phase exists. [`crate::collision`] answers "how deep
//! and which way" for the rigid solver. Godot's kinematic queries
//! (`body_test_motion`, which every `move_and_slide` runs on) need something
//! else: a SAT that grows shape A by a safe MARGIN, casts it along a motion,
//! and reports CONTACT POINT PAIRS (one per supporting vertex), not one normal.
//! Recovery pushes the body out by 40% of each pair's depth, the binary-search
//! cast keeps the safe fraction, and the rest pass keeps the deepest pair — all
//! three depend on exactly which pairs the SAT emits. Approximating any of it
//! moved the official 2D Platformer's player off Godot's trace on the first
//! frame (it rested at y = 636.75 where Godot rests at 636.7495).
//!
//! The structure mirrors Godot's so the two can be diffed: one separator per
//! shape pair, the same axis order (the first axis with the smallest depth
//! wins ties, so ORDER IS BEHAVIOUR), the same support functions and the same
//! `_generate_contacts_*` helpers. `f32` throughout, as Godot's `real_t`.

use gdcore::math::{Rect2, Transform2D, Vector2};

use crate::shape::Shape2D;

/// Godot's `CMP_EPSILON`.
pub(crate) const CMP_EPSILON: f32 = 0.00001;
/// `GodotShape2D::segment_is_valid_support_threshold` (a `double` in Godot).
const SUPPORT_THRESHOLD: f64 = 0.99998;

/// `sqrt(1 - t^2)` — `segment_is_valid_support_threshold_lower`.
fn support_threshold_lower() -> f64 {
    (1.0 - SUPPORT_THRESHOLD * SUPPORT_THRESHOLD).sqrt()
}

// ---------------------------------------------------------------------------
// Godot vector helpers, spelled exactly as `core/math/vector2.h` does them.
// ---------------------------------------------------------------------------

/// `Vector2::normalized` — a zero vector stays zero.
pub(crate) fn normalized(v: Vector2) -> Vector2 {
    let l = v.x * v.x + v.y * v.y;
    if l != 0.0 {
        let l = l.sqrt();
        Vector2::new(v.x / l, v.y / l)
    } else {
        v
    }
}

/// `Vector2::orthogonal` — `(y, -x)`.
pub(crate) fn orthogonal(v: Vector2) -> Vector2 {
    Vector2::new(v.y, -v.x)
}

/// `Vector2::slide(n)`.
pub(crate) fn slide(v: Vector2, n: Vector2) -> Vector2 {
    v - n * v.dot(n)
}

/// `Vector2::plane_project(d, p)` on normal `n`.
fn plane_project(n: Vector2, d: f32, p: Vector2) -> Vector2 {
    p - n * (n.dot(p) - d)
}

/// `Math::is_zero_approx`.
pub(crate) fn is_zero_approx(f: f32) -> bool {
    f.abs() < CMP_EPSILON
}

/// `Vector2::is_zero_approx`.
pub(crate) fn vec_is_zero_approx(v: Vector2) -> bool {
    is_zero_approx(v.x) && is_zero_approx(v.y)
}

/// `Transform2D::basis_xform_inv`.
fn basis_xform_inv(t: &Transform2D, v: Vector2) -> Vector2 {
    Vector2::new(t.x.dot(v), t.y.dot(v))
}

/// `Geometry2D::get_closest_point_to_segment_uncapped`.
fn closest_point_to_segment_uncapped(p: Vector2, a: Vector2, b: Vector2) -> Vector2 {
    let rel = p - a;
    let n = b - a;
    let l2 = n.length_squared();
    if l2 < 1e-20 {
        return a;
    }
    let d = n.dot(rel) / l2;
    a + n * d
}

/// `Rect2::expand_to`.
fn rect_expand_to(r: &mut Rect2, p: Vector2) {
    let mut begin = r.position;
    let mut end = r.position + r.size;
    if p.x < begin.x {
        begin.x = p.x;
    }
    if p.y < begin.y {
        begin.y = p.y;
    }
    if p.x > end.x {
        end.x = p.x;
    }
    if p.y > end.y {
        end.y = p.y;
    }
    r.position = begin;
    r.size = end - begin;
}

/// `Transform2D::xform(Rect2)`.
pub(crate) fn xform_rect(t: &Transform2D, r: Rect2) -> Rect2 {
    let x = t.x * r.size.x;
    let y = t.y * r.size.y;
    let pos = t.xform(r.position);
    let mut out = Rect2::new(pos, Vector2::ZERO);
    rect_expand_to(&mut out, pos + x);
    rect_expand_to(&mut out, pos + y);
    rect_expand_to(&mut out, pos + x + y);
    out
}

/// `Rect2::merge`.
pub(crate) fn rect_merge(a: Rect2, b: Rect2) -> Rect2 {
    let begin = Vector2::new(
        a.position.x.min(b.position.x),
        a.position.y.min(b.position.y),
    );
    let ea = a.position + a.size;
    let eb = b.position + b.size;
    let end = Vector2::new(ea.x.max(eb.x), ea.y.max(eb.y));
    Rect2::new(begin, end - begin)
}

/// `Rect2::intersects` (borders excluded, Godot's default).
pub(crate) fn rect_intersects(a: Rect2, b: Rect2) -> bool {
    !(a.position.x >= b.position.x + b.size.x
        || a.position.x + a.size.x <= b.position.x
        || a.position.y >= b.position.y + b.size.y
        || a.position.y + a.size.y <= b.position.y)
}

/// `Rect2::intersects(p_rect, true)` — touching counts. The broadphase cull is
/// inclusive so that a shape lying exactly on the grown AABB still reaches the
/// SAT, which then decides.
pub(crate) fn rect_intersects_inclusive(a: Rect2, b: Rect2) -> bool {
    !(a.position.x > b.position.x + b.size.x
        || a.position.x + a.size.x < b.position.x
        || a.position.y > b.position.y + b.size.y
        || a.position.y + a.size.y < b.position.y)
}

// ---------------------------------------------------------------------------
// Shapes
// ---------------------------------------------------------------------------

/// A shape in the form Godot's physics server holds it.
#[derive(Debug, Clone, PartialEq)]
pub enum SatShape {
    /// `SHAPE_WORLD_BOUNDARY`.
    WorldBoundary { normal: Vector2, d: f32 },
    /// `SHAPE_SEPARATION_RAY`.
    SeparationRay { length: f32, slide_on_slope: bool },
    /// `SHAPE_SEGMENT`. `n` is NOT normalized for a `SegmentShape2D` resource
    /// (`set_data` stores `(b - a).orthogonal()`), but is for the segments a
    /// concave polygon hands out — both as in Godot.
    Segment { a: Vector2, b: Vector2, n: Vector2 },
    /// `SHAPE_CIRCLE`.
    Circle { radius: f32 },
    /// `SHAPE_RECTANGLE`.
    Rectangle { half_extents: Vector2 },
    /// `SHAPE_CAPSULE`.
    Capsule { radius: f32, height: f32 },
    /// `SHAPE_CONVEX_POLYGON`: counter-clockwise points and the outward normal
    /// of the edge each starts.
    Convex {
        points: Vec<Vector2>,
        normals: Vec<Vector2>,
    },
    /// `SHAPE_CONCAVE_POLYGON`: a segment soup.
    Concave { segments: Vec<(Vector2, Vector2)> },
}

impl SatShape {
    /// Converts Patina's shape. `None` for a shape that collides with nothing
    /// (a zero-size rectangle is how a disabled collider is registered).
    pub fn from_shape(shape: &Shape2D) -> Option<Self> {
        Some(match *shape {
            Shape2D::Circle { radius } => {
                if radius.abs() < 1e-6 {
                    return None;
                }
                SatShape::Circle { radius }
            }
            Shape2D::Rectangle { half_extents } => {
                if half_extents.x.abs() < 1e-6 && half_extents.y.abs() < 1e-6 {
                    return None;
                }
                SatShape::Rectangle { half_extents }
            }
            Shape2D::Segment { a, b } => SatShape::Segment {
                a,
                b,
                n: orthogonal(b - a),
            },
            Shape2D::Capsule { radius, height } => SatShape::Capsule { radius, height },
            Shape2D::ConvexPolygon { points, count } => {
                let pts = &points[..count as usize];
                if pts.is_empty() {
                    return None;
                }
                Self::convex(pts)
            }
            Shape2D::ConcavePolygon { points, count } => {
                let pts = &points[..count as usize];
                let segments = pts.chunks_exact(2).map(|c| (c[0], c[1])).collect();
                SatShape::Concave { segments }
            }
            Shape2D::WorldBoundary { normal, distance } => SatShape::WorldBoundary {
                normal,
                d: distance,
            },
            Shape2D::SeparationRay { length } => SatShape::SeparationRay {
                length,
                slide_on_slope: false,
            },
        })
    }

    /// `ConvexPolygonShape2D::_update_shape` + `GodotConvexPolygonShape2D::set_data`:
    /// a clockwise polygon is reversed, then each point carries the normal of
    /// the edge it starts.
    pub fn convex(points: &[Vector2]) -> Self {
        let mut pts: Vec<Vector2> = points.to_vec();
        if is_polygon_clockwise(&pts) {
            pts.reverse();
        }
        let n = pts.len();
        let normals = (0..n)
            .map(|i| normalized(orthogonal(pts[(i + 1) % n] - pts[i])))
            .collect();
        SatShape::Convex {
            points: pts,
            normals,
        }
    }

    /// Godot's `PhysicsServer2D::ShapeType` order — the SAT dispatch swaps the
    /// pair so the lower type is always A.
    fn type_index(&self) -> u8 {
        match self {
            SatShape::WorldBoundary { .. } => 0,
            SatShape::SeparationRay { .. } => 1,
            SatShape::Segment { .. } => 2,
            SatShape::Circle { .. } => 3,
            SatShape::Rectangle { .. } => 4,
            SatShape::Capsule { .. } => 5,
            SatShape::Convex { .. } => 6,
            SatShape::Concave { .. } => 7,
        }
    }

    fn is_concave(&self) -> bool {
        matches!(self, SatShape::Concave { .. })
    }

    /// `allows_one_way_collision` — every shape but a separation ray.
    pub fn allows_one_way_collision(&self) -> bool {
        !matches!(self, SatShape::SeparationRay { .. })
    }

    /// The shape's local AABB (`GodotShape2D::get_aabb`).
    pub fn aabb(&self) -> Rect2 {
        match self {
            SatShape::WorldBoundary { .. } => {
                Rect2::new(Vector2::new(-1e4, -1e4), Vector2::new(2e4, 2e4))
            }
            SatShape::SeparationRay { length, .. } => {
                let mut r = Rect2::new(Vector2::ZERO, Vector2::ZERO);
                rect_expand_to(&mut r, Vector2::new(0.0, *length));
                r.position.x -= 0.001;
                r.size.x += 0.002;
                r
            }
            SatShape::Segment { a, b, .. } => {
                let mut r = Rect2::new(*a, Vector2::ZERO);
                rect_expand_to(&mut r, *b);
                if r.size.x == 0.0 {
                    r.size.x = 0.001;
                }
                if r.size.y == 0.0 {
                    r.size.y = 0.001;
                }
                r
            }
            SatShape::Circle { radius } => Rect2::new(
                Vector2::new(-radius, -radius),
                Vector2::new(radius * 2.0, radius * 2.0),
            ),
            SatShape::Rectangle { half_extents } => Rect2::new(-*half_extents, *half_extents * 2.0),
            SatShape::Capsule { radius, height } => {
                let he = Vector2::new(*radius, height * 0.5);
                Rect2::new(-he, he * 2.0)
            }
            SatShape::Convex { points, .. } => {
                let mut r = Rect2::new(points[0], Vector2::ZERO);
                for p in points {
                    rect_expand_to(&mut r, *p);
                }
                r
            }
            SatShape::Concave { segments } => {
                let mut r: Option<Rect2> = None;
                for (a, b) in segments {
                    match r.as_mut() {
                        None => {
                            let mut nr = Rect2::new(*a, Vector2::ZERO);
                            rect_expand_to(&mut nr, *b);
                            r = Some(nr);
                        }
                        Some(r) => {
                            rect_expand_to(r, *a);
                            rect_expand_to(r, *b);
                        }
                    }
                }
                r.unwrap_or(Rect2::new(Vector2::ZERO, Vector2::ZERO))
            }
        }
    }

    /// `project_range` along world `normal` with the shape at `t`.
    fn project_range(&self, normal: Vector2, t: &Transform2D) -> (f32, f32) {
        match self {
            SatShape::WorldBoundary { .. } => (-1e10, 1e10),
            SatShape::SeparationRay { length, .. } => {
                let mut r_max = normal.dot(t.origin);
                let mut r_min = normal.dot(t.xform(Vector2::new(0.0, *length)));
                if r_max < r_min {
                    std::mem::swap(&mut r_max, &mut r_min);
                }
                (r_min, r_max)
            }
            SatShape::Segment { a, b, .. } => {
                let mut r_max = normal.dot(t.xform(*a));
                let mut r_min = normal.dot(t.xform(*b));
                if r_max < r_min {
                    std::mem::swap(&mut r_max, &mut r_min);
                }
                (r_min, r_max)
            }
            SatShape::Circle { radius } => {
                let d = normal.dot(t.origin);
                let local_normal = basis_xform_inv(t, normal);
                let scale = local_normal.length();
                (d - radius * scale, d + radius * scale)
            }
            SatShape::Rectangle { half_extents } => {
                let mut r_max = -1e20f32;
                let mut r_min = 1e20f32;
                for i in 0..4 {
                    let corner = Vector2::new(
                        ((i & 1) * 2 - 1) as f32 * half_extents.x,
                        ((i >> 1) * 2 - 1) as f32 * half_extents.y,
                    );
                    let d = normal.dot(t.xform(corner));
                    if d > r_max {
                        r_max = d;
                    }
                    if d < r_min {
                        r_min = d;
                    }
                }
                (r_min, r_max)
            }
            SatShape::Capsule { radius, height } => {
                let mut n = normalized(basis_xform_inv(t, normal));
                let h = height * 0.5 - radius;
                n = n * *radius;
                n.y += if n.y > 0.0 { h } else { -h };
                let mut r_max = normal.dot(t.xform(n));
                let mut r_min = normal.dot(t.xform(-n));
                if r_max < r_min {
                    std::mem::swap(&mut r_max, &mut r_min);
                }
                (r_min, r_max)
            }
            SatShape::Convex { points, .. } => {
                let mut r_min = normal.dot(t.xform(points[0]));
                let mut r_max = r_min;
                for p in &points[1..] {
                    let d = normal.dot(t.xform(*p));
                    if d > r_max {
                        r_max = d;
                    }
                    if d < r_min {
                        r_min = d;
                    }
                }
                (r_min, r_max)
            }
            // Godot errors here; a concave shape never reaches the SAT.
            SatShape::Concave { .. } => (0.0, 0.0),
        }
    }

    /// `DEFAULT_PROJECT_RANGE_CAST` (world boundaries stay "real large").
    fn project_range_cast(&self, cast: Vector2, normal: Vector2, t: &Transform2D) -> (f32, f32) {
        if let SatShape::WorldBoundary { .. } = self {
            return (-1e10, 1e10);
        }
        let (mina, maxa) = self.project_range(normal, t);
        let mut ofsb = *t;
        ofsb.origin = ofsb.origin + cast;
        let (minb, maxb) = self.project_range(normal, &ofsb);
        (mina.min(minb), maxa.max(maxb))
    }

    /// `get_supports` for a LOCAL `normal`. Returns up to two points.
    fn get_supports(&self, normal: Vector2) -> ([Vector2; 2], usize) {
        let mut out = [Vector2::ZERO; 2];
        match self {
            // `GodotWorldBoundaryShape2D::get_supports` returns no points.
            SatShape::WorldBoundary { .. } => (out, 0),
            SatShape::SeparationRay { length, .. } => {
                out[0] = if normal.y > 0.0 {
                    Vector2::new(0.0, *length)
                } else {
                    Vector2::ZERO
                };
                (out, 1)
            }
            SatShape::Segment { a, b, n } => {
                if (normal.dot(*n).abs() as f64) > SUPPORT_THRESHOLD {
                    out[0] = *a;
                    out[1] = *b;
                    return (out, 2);
                }
                let dp = normal.dot(*b - *a);
                out[0] = if dp > 0.0 { *b } else { *a };
                (out, 1)
            }
            SatShape::Circle { radius } => {
                out[0] = normal * *radius;
                (out, 1)
            }
            SatShape::Rectangle { half_extents } => {
                for i in 0..2 {
                    let dp = if i == 0 { normal.x } else { normal.y };
                    if (dp.abs() as f64) <= SUPPORT_THRESHOLD {
                        continue;
                    }
                    let sgn = if dp > 0.0 { 1.0 } else { -1.0 };
                    if i == 0 {
                        out[0] = Vector2::new(half_extents.x * sgn, half_extents.y);
                        out[1] = Vector2::new(half_extents.x * sgn, -half_extents.y);
                    } else {
                        out[0] = Vector2::new(half_extents.x, half_extents.y * sgn);
                        out[1] = Vector2::new(-half_extents.x, half_extents.y * sgn);
                    }
                    return (out, 2);
                }
                out[0] = Vector2::new(
                    if normal.x < 0.0 {
                        -half_extents.x
                    } else {
                        half_extents.x
                    },
                    if normal.y < 0.0 {
                        -half_extents.y
                    } else {
                        half_extents.y
                    },
                );
                (out, 1)
            }
            SatShape::Capsule { radius, height } => {
                let mut n = normal;
                let h = height * 0.5 - radius;
                if h > 0.0 && (n.x.abs() as f64) > SUPPORT_THRESHOLD {
                    n.y = 0.0;
                    // Godot's `SIGN` (only reached with |x| > 0.99998).
                    n.x = n.x.signum() * radius;
                    out[0] = n;
                    out[0].y += h;
                    out[1] = n;
                    out[1].y -= h;
                    (out, 2)
                } else {
                    n = n * *radius;
                    n.y += if n.y > 0.0 { h } else { -h };
                    out[0] = n;
                    (out, 1)
                }
            }
            SatShape::Convex { points, normals } => {
                let mut support_idx: Option<usize> = None;
                let mut d = -1e10f32;
                let count = points.len();
                for i in 0..count {
                    let ld = normal.dot(points[i]);
                    if ld > d {
                        support_idx = Some(i);
                        d = ld;
                    }
                    if (normals[i].dot(normal) as f64) > SUPPORT_THRESHOLD {
                        out[0] = points[i];
                        out[1] = points[(i + 1) % count];
                        return (out, 2);
                    }
                }
                out[0] = points[support_idx.unwrap_or(0)];
                (out, 1)
            }
            SatShape::Concave { .. } => (out, 0),
        }
    }

    /// `get_supports_transformed_cast`.
    fn get_supports_transformed_cast(
        &self,
        cast: Vector2,
        normal: Vector2,
        t: &Transform2D,
    ) -> ([Vector2; 2], usize) {
        let (mut s, mut amount) = self.get_supports(normalized(basis_xform_inv(t, normal)));
        for p in s.iter_mut().take(amount) {
            *p = t.xform(*p);
        }
        let lower = support_threshold_lower();
        let parallel = (normal.dot(normalized(cast)).abs() as f64) < lower;
        if amount == 1 {
            if parallel {
                amount = 2;
                s[1] = s[0] + cast;
            } else if cast.dot(normal) > 0.0 {
                s[0] = s[0] + cast;
            }
        } else if parallel {
            if (s[1] - s[0]).dot(cast) > 0.0 {
                s[1] = s[1] + cast;
            } else {
                s[0] = s[0] + cast;
            }
        } else if cast.dot(normal) > 0.0 {
            s[0] = s[0] + cast;
            s[1] = s[1] + cast;
        }
        (s, amount)
    }

    fn convex_points(&self) -> &[Vector2] {
        match self {
            SatShape::Convex { points, .. } => points,
            _ => &[],
        }
    }

    /// `GodotConvexPolygonShape2D::get_xformed_segment_normal`.
    fn convex_xformed_segment_normal(&self, t: &Transform2D, idx: usize) -> Vector2 {
        let pts = self.convex_points();
        let a = pts[idx];
        let b = pts[if idx + 1 == pts.len() { 0 } else { idx + 1 }];
        orthogonal(normalized(t.xform(b) - t.xform(a)))
    }

    fn segment_ab(&self) -> (Vector2, Vector2) {
        match self {
            SatShape::Segment { a, b, .. } => (*a, *b),
            _ => (Vector2::ZERO, Vector2::ZERO),
        }
    }

    fn capsule_dims(&self) -> (f32, f32) {
        match self {
            SatShape::Capsule { radius, height } => (*radius, *height),
            _ => (0.0, 0.0),
        }
    }

    fn half_extents(&self) -> Vector2 {
        match self {
            SatShape::Rectangle { half_extents } => *half_extents,
            _ => Vector2::ZERO,
        }
    }
}

/// `Geometry2D::is_polygon_clockwise`.
fn is_polygon_clockwise(p: &[Vector2]) -> bool {
    let c = p.len();
    if c < 3 {
        return false;
    }
    let mut sum = 0.0f32;
    for i in 0..c {
        let v1 = p[i];
        let v2 = p[(i + 1) % c];
        sum += (v2.x - v1.x) * (v2.y + v1.y);
    }
    sum > 0.0
}

/// `GodotRectangleShape2D::get_circle_axis`.
fn rect_circle_axis(
    he: Vector2,
    xf: &Transform2D,
    xf_inv: &Transform2D,
    circle: Vector2,
) -> Vector2 {
    let local_v = xf_inv.xform(circle);
    let h = Vector2::new(
        if local_v.x < 0.0 { -he.x } else { he.x },
        if local_v.y < 0.0 { -he.y } else { he.y },
    );
    normalized(xf.xform(h) - circle)
}

/// `GodotRectangleShape2D::get_box_axis`.
fn rect_box_axis(
    he_a: Vector2,
    xf: &Transform2D,
    xf_inv: &Transform2D,
    he_b: Vector2,
    b_xf: &Transform2D,
    b_xf_inv: &Transform2D,
) -> Vector2 {
    let a = {
        let local_v = xf_inv.xform(b_xf.origin);
        let h = Vector2::new(
            if local_v.x < 0.0 { -he_a.x } else { he_a.x },
            if local_v.y < 0.0 { -he_a.y } else { he_a.y },
        );
        xf.xform(h)
    };
    let b = {
        let local_v = b_xf_inv.xform(xf.origin);
        let h = Vector2::new(
            if local_v.x < 0.0 { -he_b.x } else { he_b.x },
            if local_v.y < 0.0 { -he_b.y } else { he_b.y },
        );
        b_xf.xform(h)
    };
    normalized(a - b)
}

// ---------------------------------------------------------------------------
// Collector + separator
// ---------------------------------------------------------------------------

/// A contact callback: `(point_on_A, point_on_B)`.
pub type ContactFn<'a> = dyn FnMut(Vector2, Vector2) + 'a;

/// `_CollectorCallback2D`.
struct Collector<'c, 'f> {
    callback: Option<&'c mut ContactFn<'f>>,
    swap: bool,
    collided: bool,
    normal: Vector2,
    sep_axis: Option<&'c mut Vector2>,
}

impl Collector<'_, '_> {
    fn call(&mut self, a: Vector2, b: Vector2) {
        if let Some(cb) = self.callback.as_mut() {
            if self.swap {
                cb(b, a);
            } else {
                cb(a, b);
            }
        }
    }
}

fn contacts_point_point(a: &[Vector2], b: &[Vector2], col: &mut Collector) {
    col.call(a[0], b[0]);
}

fn contacts_point_edge(a: &[Vector2], b: &[Vector2], col: &mut Collector) {
    let closest_b = closest_point_to_segment_uncapped(a[0], b[0], b[1]);
    col.call(a[0], closest_b);
}

fn contacts_edge_edge(pa: &[Vector2], pb: &[Vector2], col: &mut Collector) {
    let n = col.normal;
    let t = orthogonal(n);
    let d_a = n.dot(pa[0]);
    let d_b = n.dot(pb[0]);
    // (d, is_a, idx), sorted stably by d — Godot's SortArray insertion sort.
    let mut dvec = [
        (t.dot(pa[0]), true, 0usize),
        (t.dot(pa[1]), true, 1usize),
        (t.dot(pb[0]), false, 0usize),
        (t.dot(pb[1]), false, 1usize),
    ];
    // Insertion sort with a strict `<`, exactly as `SortArray::linear_insert`.
    for i in 1..4 {
        let val = dvec[i];
        let mut j = i;
        while j > 0 && val.0 < dvec[j - 1].0 {
            dvec[j] = dvec[j - 1];
            j -= 1;
        }
        dvec[j] = val;
    }
    for entry in &dvec[1..=2] {
        if entry.1 {
            let a = pa[entry.2];
            let b = plane_project(n, d_b, a);
            if n.dot(a) > n.dot(b) - CMP_EPSILON {
                continue;
            }
            col.call(a, b);
        } else {
            let b = pb[entry.2];
            let a = plane_project(n, d_a, b);
            if n.dot(a) > n.dot(b) - CMP_EPSILON {
                continue;
            }
            col.call(a, b);
        }
    }
}

fn contacts_from_supports(points_a: &[Vector2], points_b: &[Vector2], col: &mut Collector) {
    let (pa, pb) = if points_a.len() > points_b.len() {
        col.swap = !col.swap;
        col.normal = -col.normal;
        (points_b, points_a)
    } else {
        (points_a, points_b)
    };
    let va = pa.len().min(2) - 1;
    let vb = pb.len().min(2) - 1;
    match (va, vb) {
        (0, 0) => contacts_point_point(pa, pb, col),
        (0, 1) => contacts_point_edge(pa, pb, col),
        (1, 1) => contacts_edge_edge(pa, pb, col),
        _ => {}
    }
}

/// `SeparatorAxisTest2D` with the template flags as runtime fields.
struct Separator<'s, 'c, 'cc, 'f> {
    shape_a: &'s SatShape,
    shape_b: &'s SatShape,
    ta: &'s Transform2D,
    tb: &'s Transform2D,
    cast_a: bool,
    cast_b: bool,
    with_margin: bool,
    best_depth: f32,
    best_axis: Vector2,
    motion_a: Vector2,
    motion_b: Vector2,
    margin_a: f32,
    margin_b: f32,
    col: &'c mut Collector<'cc, 'f>,
}

impl Separator<'_, '_, '_, '_> {
    fn test_previous_axis(&mut self) -> bool {
        let prev = self.col.sep_axis.as_deref().copied();
        match prev {
            Some(axis) if axis != Vector2::ZERO => self.test_axis(axis),
            _ => true,
        }
    }

    fn test_cast(&mut self) -> bool {
        if self.cast_a {
            let na = normalized(self.motion_a);
            if !self.test_axis(na) {
                return false;
            }
            if !self.test_axis(orthogonal(na)) {
                return false;
            }
        }
        if self.cast_b {
            let nb = normalized(self.motion_b);
            if !self.test_axis(nb) {
                return false;
            }
            if !self.test_axis(orthogonal(nb)) {
                return false;
            }
        }
        true
    }

    fn test_axis(&mut self, p_axis: Vector2) -> bool {
        let mut axis = p_axis;
        if is_zero_approx(axis.x) && is_zero_approx(axis.y) {
            axis = Vector2::new(0.0, 1.0);
        }
        let (mut min_a, mut max_a) = if self.cast_a {
            self.shape_a
                .project_range_cast(self.motion_a, axis, self.ta)
        } else {
            self.shape_a.project_range(axis, self.ta)
        };
        let (mut min_b, mut max_b) = if self.cast_b {
            self.shape_b
                .project_range_cast(self.motion_b, axis, self.tb)
        } else {
            self.shape_b.project_range(axis, self.tb)
        };
        if self.with_margin {
            min_a -= self.margin_a;
            max_a += self.margin_a;
            min_b -= self.margin_b;
            max_b += self.margin_b;
        }
        min_b -= (max_a - min_a) * 0.5;
        max_b += (max_a - min_a) * 0.5;
        let mut dmin = min_b - (min_a + max_a) * 0.5;
        let dmax = max_b - (min_a + max_a) * 0.5;
        if dmin > 0.0 || dmax < 0.0 {
            if let Some(sep) = self.col.sep_axis.as_deref_mut() {
                *sep = axis;
            }
            return false;
        }
        dmin = dmin.abs();
        if dmax < dmin {
            if dmax < self.best_depth {
                self.best_depth = dmax;
                self.best_axis = axis;
            }
        } else if dmin < self.best_depth {
            self.best_depth = dmin;
            self.best_axis = -axis;
        }
        true
    }

    /// `TEST_POINT(m_a, m_b)` — true when a SEPARATING axis was found.
    fn test_point(&mut self, ma: Vector2, mb: Vector2) -> bool {
        if !self.test_axis(normalized(ma - mb)) {
            return true;
        }
        if self.cast_a && !self.test_axis(normalized(ma + self.motion_a - mb)) {
            return true;
        }
        if self.cast_b && !self.test_axis(normalized(ma - (mb + self.motion_b))) {
            return true;
        }
        if self.cast_a
            && self.cast_b
            && !self.test_axis(normalized(ma + self.motion_a - (mb + self.motion_b)))
        {
            return true;
        }
        false
    }

    fn generate_contacts(&mut self) {
        if self.best_axis == Vector2::ZERO {
            return;
        }
        self.col.collided = true;
        if self.col.callback.is_none() {
            return;
        }
        let (mut sa, na) = if self.cast_a {
            self.shape_a
                .get_supports_transformed_cast(self.motion_a, -self.best_axis, self.ta)
        } else {
            let (mut s, n) = self
                .shape_a
                .get_supports(normalized(basis_xform_inv(self.ta, -self.best_axis)));
            for p in s.iter_mut().take(n) {
                *p = self.ta.xform(*p);
            }
            (s, n)
        };
        if self.with_margin {
            for p in sa.iter_mut().take(na) {
                *p = *p + -self.best_axis * self.margin_a;
            }
        }
        let (mut sb, nb) = if self.cast_b {
            self.shape_b
                .get_supports_transformed_cast(self.motion_b, self.best_axis, self.tb)
        } else {
            let (mut s, n) = self
                .shape_b
                .get_supports(normalized(basis_xform_inv(self.tb, self.best_axis)));
            for p in s.iter_mut().take(n) {
                *p = self.tb.xform(*p);
            }
            (s, n)
        };
        if self.with_margin {
            for p in sb.iter_mut().take(nb) {
                *p = *p + self.best_axis * self.margin_b;
            }
        }
        if na == 0 || nb == 0 {
            return;
        }
        self.col.normal = self.best_axis;
        contacts_from_supports(&sa[..na], &sb[..nb], self.col);
        if let Some(sep) = self.col.sep_axis.as_deref_mut() {
            if *sep != Vector2::ZERO {
                *sep = Vector2::ZERO;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Per-pair separators (`_collision_<a>_<b>`)
// ---------------------------------------------------------------------------

fn pair_collision(s: &mut Separator) {
    if !s.test_previous_axis() {
        return;
    }
    if !s.test_cast() {
        return;
    }
    let (ta, tb) = (*s.ta, *s.tb);
    let (ma, mb) = (s.motion_a, s.motion_b);
    let (cast_a, cast_b, with_margin) = (s.cast_a, s.cast_b, s.with_margin);
    match (s.shape_a.type_index(), s.shape_b.type_index()) {
        // segment - segment
        (2, 2) => {
            let (a0, a1) = s.shape_a.segment_ab();
            let (b0, b1) = s.shape_b.segment_ab();
            if !s.test_axis(orthogonal(normalized(ta.xform(a1) - ta.xform(a0)))) {
                return;
            }
            if !s.test_axis(orthogonal(normalized(tb.xform(b1) - tb.xform(b0)))) {
                return;
            }
            if with_margin {
                for (pa, pb) in [(a0, b0), (a0, b1), (a1, b0), (a1, b1)] {
                    if s.test_point(ta.xform(pa), tb.xform(pb)) {
                        return;
                    }
                }
            }
        }
        // segment - circle
        (2, 3) => {
            let (a0, a1) = s.shape_a.segment_ab();
            if !s.test_axis(orthogonal(normalized(ta.xform(a1) - ta.xform(a0)))) {
                return;
            }
            if s.test_point(ta.xform(a0), tb.origin) {
                return;
            }
            if s.test_point(ta.xform(a1), tb.origin) {
                return;
            }
        }
        // segment - rectangle
        (2, 4) => {
            let (a0, a1) = s.shape_a.segment_ab();
            if !s.test_axis(orthogonal(normalized(ta.xform(a1) - ta.xform(a0)))) {
                return;
            }
            if !s.test_axis(normalized(tb.x)) {
                return;
            }
            if !s.test_axis(normalized(tb.y)) {
                return;
            }
            if with_margin {
                let inv = tb.affine_inverse();
                let he = s.shape_b.half_extents();
                let a = ta.xform(a0);
                let b = ta.xform(a1);
                let mut pts = vec![a, b];
                if cast_a {
                    pts.push(a + ma);
                    pts.push(b + ma);
                }
                if cast_b {
                    pts.push(a - mb);
                    pts.push(b - mb);
                }
                if cast_a && cast_b {
                    pts.push(a - mb + ma);
                    pts.push(b - mb + ma);
                }
                for p in pts {
                    if !s.test_axis(rect_circle_axis(he, &tb, &inv, p)) {
                        return;
                    }
                }
            }
        }
        // segment - capsule
        (2, 5) => {
            let (a0, a1) = s.shape_a.segment_ab();
            if !s.test_axis(orthogonal(normalized(ta.xform(a1) - ta.xform(a0)))) {
                return;
            }
            if !s.test_axis(normalized(tb.x)) {
                return;
            }
            let (r, h) = s.shape_b.capsule_dims();
            let dir = h * 0.5 - r;
            let up = tb.origin + tb.y * dir;
            let down = tb.origin - tb.y * dir;
            for (pa, pb) in [(a0, up), (a0, down), (a1, up), (a1, down)] {
                if s.test_point(ta.xform(pa), pb) {
                    return;
                }
            }
        }
        // segment - convex
        (2, 6) => {
            let (a0, a1) = s.shape_a.segment_ab();
            if !s.test_axis(orthogonal(normalized(ta.xform(a1) - ta.xform(a0)))) {
                return;
            }
            let count = s.shape_b.convex_points().len();
            for i in 0..count {
                let axis = s.shape_b.convex_xformed_segment_normal(&tb, i);
                if !s.test_axis(axis) {
                    return;
                }
                if with_margin {
                    let p = tb.xform(s.shape_b.convex_points()[i]);
                    if s.test_point(ta.xform(a0), p) {
                        return;
                    }
                    if s.test_point(ta.xform(a1), p) {
                        return;
                    }
                }
            }
        }
        // circle - circle
        (3, 3) => {
            if s.test_point(ta.origin, tb.origin) {
                return;
            }
        }
        // circle - rectangle
        (3, 4) => {
            let sphere = ta.origin;
            if !s.test_axis(normalized(tb.x)) {
                return;
            }
            if !s.test_axis(normalized(tb.y)) {
                return;
            }
            let binv = tb.affine_inverse();
            let he = s.shape_b.half_extents();
            if !s.test_axis(rect_circle_axis(he, &tb, &binv, sphere)) {
                return;
            }
            if cast_a && !s.test_axis(rect_circle_axis(he, &tb, &binv, sphere + ma)) {
                return;
            }
            if cast_b && !s.test_axis(rect_circle_axis(he, &tb, &binv, sphere - mb)) {
                return;
            }
            if cast_a && cast_b && !s.test_axis(rect_circle_axis(he, &tb, &binv, sphere - mb + ma))
            {
                return;
            }
        }
        // circle - capsule
        (3, 5) => {
            if !s.test_axis(normalized(tb.x)) {
                return;
            }
            let (r, h) = s.shape_b.capsule_dims();
            let dir = h * 0.5 - r;
            if s.test_point(ta.origin, tb.origin + tb.y * dir) {
                return;
            }
            if s.test_point(ta.origin, tb.origin - tb.y * dir) {
                return;
            }
        }
        // circle - convex
        (3, 6) => {
            let count = s.shape_b.convex_points().len();
            for i in 0..count {
                let p = tb.xform(s.shape_b.convex_points()[i]);
                if s.test_point(ta.origin, p) {
                    return;
                }
                let axis = s.shape_b.convex_xformed_segment_normal(&tb, i);
                if !s.test_axis(axis) {
                    return;
                }
            }
        }
        // rectangle - rectangle
        (4, 4) => {
            if !s.test_axis(normalized(ta.x)) {
                return;
            }
            if !s.test_axis(normalized(ta.y)) {
                return;
            }
            if !s.test_axis(normalized(tb.x)) {
                return;
            }
            if !s.test_axis(normalized(tb.y)) {
                return;
            }
            if with_margin {
                let inv_a = ta.affine_inverse();
                let inv_b = tb.affine_inverse();
                let he_a = s.shape_a.half_extents();
                let he_b = s.shape_b.half_extents();
                if !s.test_axis(rect_box_axis(he_a, &ta, &inv_a, he_b, &tb, &inv_b)) {
                    return;
                }
                if cast_a || cast_b {
                    let mut aofs = ta;
                    aofs.origin = aofs.origin + ma;
                    let mut bofs = tb;
                    bofs.origin = bofs.origin + mb;
                    let aofsinv = aofs.affine_inverse();
                    let bofsinv = bofs.affine_inverse();
                    if cast_a
                        && !s.test_axis(rect_box_axis(he_a, &aofs, &aofsinv, he_b, &tb, &inv_b))
                    {
                        return;
                    }
                    if cast_b
                        && !s.test_axis(rect_box_axis(he_a, &ta, &inv_a, he_b, &bofs, &bofsinv))
                    {
                        return;
                    }
                    if cast_a
                        && cast_b
                        && !s.test_axis(rect_box_axis(he_a, &aofs, &aofsinv, he_b, &bofs, &bofsinv))
                    {
                        return;
                    }
                }
            }
        }
        // rectangle - capsule
        (4, 5) => {
            if !s.test_axis(normalized(ta.x)) {
                return;
            }
            if !s.test_axis(normalized(ta.y)) {
                return;
            }
            if !s.test_axis(normalized(tb.x)) {
                return;
            }
            let boxinv = ta.affine_inverse();
            let he = s.shape_a.half_extents();
            let (r, h) = s.shape_b.capsule_dims();
            let mut dir = h * 0.5 - r;
            for _ in 0..2 {
                let ep = tb.origin + tb.y * dir;
                if !s.test_axis(rect_circle_axis(he, &ta, &boxinv, ep)) {
                    return;
                }
                if cast_a && !s.test_axis(rect_circle_axis(he, &ta, &boxinv, ep - ma)) {
                    return;
                }
                if cast_b && !s.test_axis(rect_circle_axis(he, &ta, &boxinv, ep + mb)) {
                    return;
                }
                if cast_a
                    && cast_b
                    && !s.test_axis(rect_circle_axis(he, &ta, &boxinv, ep - ma + mb))
                {
                    return;
                }
                dir *= -1.0;
            }
        }
        // rectangle - convex
        (4, 6) => {
            if !s.test_axis(normalized(ta.x)) {
                return;
            }
            if !s.test_axis(normalized(ta.y)) {
                return;
            }
            let boxinv = if with_margin {
                ta.affine_inverse()
            } else {
                Transform2D::IDENTITY
            };
            let he = s.shape_a.half_extents();
            let count = s.shape_b.convex_points().len();
            for i in 0..count {
                let axis = s.shape_b.convex_xformed_segment_normal(&tb, i);
                if !s.test_axis(axis) {
                    return;
                }
                if with_margin {
                    let p = tb.xform(s.shape_b.convex_points()[i]);
                    if !s.test_axis(rect_circle_axis(he, &ta, &boxinv, p)) {
                        return;
                    }
                    if cast_a && !s.test_axis(rect_circle_axis(he, &ta, &boxinv, p - ma)) {
                        return;
                    }
                    if cast_b && !s.test_axis(rect_circle_axis(he, &ta, &boxinv, p + mb)) {
                        return;
                    }
                    if cast_a
                        && cast_b
                        && !s.test_axis(rect_circle_axis(he, &ta, &boxinv, p + mb - ma))
                    {
                        return;
                    }
                }
            }
        }
        // capsule - capsule
        (5, 5) => {
            if !s.test_axis(normalized(tb.x)) {
                return;
            }
            if !s.test_axis(normalized(ta.x)) {
                return;
            }
            let (ra, ha) = s.shape_a.capsule_dims();
            let (rb, hb) = s.shape_b.capsule_dims();
            let mut dir_a = ha * 0.5 - ra;
            for _ in 0..2 {
                let ep_a = ta.origin + ta.y * dir_a;
                let mut dir_b = hb * 0.5 - rb;
                for _ in 0..2 {
                    let ep_b = tb.origin + tb.y * dir_b;
                    if s.test_point(ep_a, ep_b) {
                        return;
                    }
                    dir_b *= -1.0;
                }
                dir_a *= -1.0;
            }
        }
        // capsule - convex
        (5, 6) => {
            if !s.test_axis(normalized(ta.x)) {
                return;
            }
            let (r, h) = s.shape_a.capsule_dims();
            let count = s.shape_b.convex_points().len();
            for i in 0..count {
                let cpoint = tb.xform(s.shape_b.convex_points()[i]);
                let mut dir = h * 0.5 - r;
                for _ in 0..2 {
                    let ep = ta.origin + ta.y * dir;
                    if s.test_point(ep, cpoint) {
                        return;
                    }
                    dir *= -1.0;
                }
                let axis = s.shape_b.convex_xformed_segment_normal(&tb, i);
                if !s.test_axis(axis) {
                    return;
                }
            }
        }
        // convex - convex
        (6, 6) => {
            let ca = s.shape_a.convex_points().len();
            for i in 0..ca {
                let axis = s.shape_a.convex_xformed_segment_normal(&ta, i);
                if !s.test_axis(axis) {
                    return;
                }
            }
            let cb = s.shape_b.convex_points().len();
            for i in 0..cb {
                let axis = s.shape_b.convex_xformed_segment_normal(&tb, i);
                if !s.test_axis(axis) {
                    return;
                }
            }
            if with_margin {
                for i in 0..ca {
                    for j in 0..cb {
                        let pa = ta.xform(s.shape_a.convex_points()[i]);
                        let pb = tb.xform(s.shape_b.convex_points()[j]);
                        if s.test_point(pa, pb) {
                            return;
                        }
                    }
                }
            }
        }
        _ => return,
    }
    s.generate_contacts();
}

/// `sat_2d_calculate_penetration`.
#[allow(clippy::too_many_arguments)]
fn sat_calculate_penetration<'f>(
    shape_a: &SatShape,
    ta: &Transform2D,
    motion_a: Vector2,
    shape_b: &SatShape,
    tb: &Transform2D,
    motion_b: Vector2,
    callback: Option<&mut ContactFn<'f>>,
    swap: bool,
    sep_axis: Option<&mut Vector2>,
    margin_a: f32,
    margin_b: f32,
) -> bool {
    let mut col = Collector {
        callback,
        swap,
        collided: false,
        normal: Vector2::ZERO,
        sep_axis,
    };
    let (mut a, mut b, mut xa, mut xb, mut mova, mut movb, mut mga, mut mgb) = (
        shape_a, shape_b, ta, tb, motion_a, motion_b, margin_a, margin_b,
    );
    if a.type_index() > b.type_index() {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut xa, &mut xb);
        std::mem::swap(&mut mova, &mut movb);
        std::mem::swap(&mut mga, &mut mgb);
        col.swap = !col.swap;
    }
    let with_margin = margin_a != 0.0 || margin_b != 0.0;
    let mut sep = Separator {
        shape_a: a,
        shape_b: b,
        ta: xa,
        tb: xb,
        cast_a: mova != Vector2::ZERO,
        cast_b: movb != Vector2::ZERO,
        with_margin,
        best_depth: 1e15,
        best_axis: Vector2::ZERO,
        motion_a: mova,
        motion_b: movb,
        margin_a: mga,
        margin_b: mgb,
        col: &mut col,
    };
    pair_collision(&mut sep);
    col.collided
}

/// `GodotCollisionSolver2D::solve_static_world_boundary`.
#[allow(clippy::too_many_arguments)]
fn solve_static_world_boundary(
    shape_a: &SatShape,
    ta: &Transform2D,
    shape_b: &SatShape,
    tb: &Transform2D,
    motion_b: Vector2,
    mut callback: Option<&mut ContactFn<'_>>,
    swap_result: bool,
    margin: f32,
) -> bool {
    let SatShape::WorldBoundary { normal, d } = shape_a else {
        return false;
    };
    if matches!(shape_b, SatShape::WorldBoundary { .. }) {
        return false;
    }
    let n = normalized(ta.basis_xform(*normal));
    let p = ta.xform(*normal * *d);
    let dd = n.dot(p);
    let (mut supports, count) =
        shape_b.get_supports(normalized(tb.affine_inverse().basis_xform(-n)));
    let mut found = false;
    for s in supports.iter_mut().take(count) {
        *s = *s + normalized(*s) * margin;
        *s = tb.xform(*s);
        *s = *s + motion_b;
        let pd = n.dot(*s);
        if pd >= dd {
            continue;
        }
        found = true;
        let support_a = *s - n * (pd - dd);
        if let Some(cb) = callback.as_mut() {
            if swap_result {
                cb(*s, support_a);
            } else {
                cb(support_a, *s);
            }
        }
    }
    found
}

/// `GodotCollisionSolver2D::solve_concave`.
#[allow(clippy::too_many_arguments)]
fn solve_concave<'f>(
    shape_a: &SatShape,
    ta: &Transform2D,
    motion_a: Vector2,
    shape_b: &SatShape,
    tb: &Transform2D,
    motion_b: Vector2,
    mut callback: Option<&mut ContactFn<'f>>,
    swap_result: bool,
    mut sep_axis: Option<&mut Vector2>,
    margin_a: f32,
    margin_b: f32,
) -> bool {
    let SatShape::Concave { segments } = shape_b else {
        return false;
    };
    let mut rel = *ta;
    rel.origin = rel.origin - tb.origin;
    let mut local_aabb = Rect2::new(Vector2::ZERO, Vector2::ZERO);
    for i in 0..2 {
        let axis_raw = if i == 0 { tb.x } else { tb.y };
        let axis_scale = 1.0 / axis_raw.length();
        let axis = axis_raw * axis_scale;
        let (smin, smax) = shape_a.project_range(axis, &rel);
        let (smin, smax) = (smin * axis_scale, smax * axis_scale);
        if i == 0 {
            local_aabb.position.x = smin;
            local_aabb.size.x = smax - smin;
        } else {
            local_aabb.position.y = smin;
            local_aabb.size.y = smax - smin;
        }
    }
    if motion_a != Vector2::ZERO {
        let mut moved = local_aabb;
        moved.position = moved.position + motion_a;
        local_aabb = rect_merge(local_aabb, moved);
    }
    let mut collided = false;
    for (a, b) in segments {
        let seg = SatShape::Segment {
            a: *a,
            b: *b,
            n: normalized(orthogonal(*b - *a)),
        };
        let mut seg_aabb = Rect2::new(*a, Vector2::ZERO);
        rect_expand_to(&mut seg_aabb, *b);
        if !rect_intersects(local_aabb, seg_aabb) {
            continue;
        }
        let hit = sat_calculate_penetration(
            shape_a,
            ta,
            motion_a,
            &seg,
            tb,
            motion_b,
            callback.as_deref_mut(),
            swap_result,
            sep_axis.as_deref_mut(),
            margin_a,
            margin_b,
        );
        if hit {
            collided = true;
            if callback.is_none() {
                return true;
            }
        }
    }
    collided
}

/// `GodotCollisionSolver2D::solve`: does `shape_a` at `ta` swept along
/// `motion_a` touch `shape_b` at `tb`? Each supporting contact is reported to
/// `callback` as `(point_on_A, point_on_B)`.
#[allow(clippy::too_many_arguments)]
pub fn solve<'f>(
    shape_a: &SatShape,
    ta: &Transform2D,
    motion_a: Vector2,
    shape_b: &SatShape,
    tb: &Transform2D,
    motion_b: Vector2,
    callback: Option<&mut ContactFn<'f>>,
    sep_axis: Option<&mut Vector2>,
    p_margin_a: f32,
    p_margin_b: f32,
) -> bool {
    let mut type_a = shape_a.type_index();
    let mut type_b = shape_b.type_index();
    let mut concave_a = shape_a.is_concave();
    let mut concave_b = shape_b.is_concave();
    let mut margin_a = p_margin_a;
    let mut margin_b = p_margin_b;
    let mut swap = false;
    if type_a > type_b {
        std::mem::swap(&mut type_a, &mut type_b);
        std::mem::swap(&mut concave_a, &mut concave_b);
        std::mem::swap(&mut margin_a, &mut margin_b);
        swap = true;
    }
    if type_a == 0 {
        if type_b == 0 {
            return false;
        }
        return if swap {
            solve_static_world_boundary(
                shape_b, tb, shape_a, ta, motion_a, callback, true, p_margin_a,
            )
        } else {
            solve_static_world_boundary(
                shape_a, ta, shape_b, tb, motion_b, callback, false, p_margin_b,
            )
        };
    }
    if type_a == 1 {
        // Separation rays are not ported (no Patina scene relies on them
        // for kinematic motion); they collide with nothing here.
        return false;
    }
    if concave_b {
        if concave_a {
            return false;
        }
        return if !swap {
            solve_concave(
                shape_a, ta, motion_a, shape_b, tb, motion_b, callback, false, sep_axis, margin_a,
                margin_b,
            )
        } else {
            solve_concave(
                shape_b, tb, motion_b, shape_a, ta, motion_a, callback, true, sep_axis, margin_a,
                margin_b,
            )
        };
    }
    // ⚠️ Godot hands the margins over ALREADY SWAPPED while the shapes are
    // not (`collision_solver(p_shape_A, ..., margin_A, margin_B)` after the
    // swap above), and the SAT then swaps shapes and margins again. Net: the
    // margin lands on whichever shape has the LOWER type. Depths are the same
    // either way; contact points shift by the margin. Kept as Godot has it.
    sat_calculate_penetration(
        shape_a, ta, motion_a, shape_b, tb, motion_b, callback, false, sep_axis, margin_a, margin_b,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f32, h: f32) -> SatShape {
        SatShape::Rectangle {
            half_extents: Vector2::new(w * 0.5, h * 0.5),
        }
    }

    /// Two boxes stacked with a 0.05 gap and a 0.08 margin: the SAT reports
    /// the two corner pairs of the overlap, each 0.03 deep along +Y.
    #[test]
    fn margin_contacts_are_the_two_overlap_corners() {
        let a = rect(10.0, 10.0);
        let b = rect(40.0, 10.0);
        let ta = Transform2D::translated(Vector2::new(0.0, 0.0));
        let tb = Transform2D::translated(Vector2::new(0.0, 10.05));
        let mut pairs = Vec::new();
        let mut cb = |pa: Vector2, pb: Vector2| pairs.push((pa, pb));
        let hit = solve(
            &a,
            &ta,
            Vector2::ZERO,
            &b,
            &tb,
            Vector2::ZERO,
            Some(&mut cb),
            None,
            0.08,
            0.0,
        );
        assert!(hit);
        assert_eq!(pairs.len(), 2, "{pairs:?}");
        for (pa, pb) in &pairs {
            assert!(((pa.y - pb.y) - 0.03).abs() < 1e-4, "{pa:?} {pb:?}");
            assert!((pa.x - pb.x).abs() < 1e-5);
        }
    }

    #[test]
    fn separated_beyond_margin_does_not_collide() {
        let a = rect(10.0, 10.0);
        let b = rect(40.0, 10.0);
        let ta = Transform2D::IDENTITY;
        let tb = Transform2D::translated(Vector2::new(0.0, 10.1));
        assert!(!solve(
            &a,
            &ta,
            Vector2::ZERO,
            &b,
            &tb,
            Vector2::ZERO,
            None,
            None,
            0.08,
            0.0
        ));
    }

    #[test]
    fn cast_collides_only_when_the_sweep_reaches() {
        let a = rect(10.0, 10.0);
        let b = rect(40.0, 10.0);
        let ta = Transform2D::IDENTITY;
        let tb = Transform2D::translated(Vector2::new(0.0, 12.0));
        assert!(!solve(
            &a,
            &ta,
            Vector2::new(0.0, 1.9),
            &b,
            &tb,
            Vector2::ZERO,
            None,
            None,
            0.0,
            0.0
        ));
        assert!(solve(
            &a,
            &ta,
            Vector2::new(0.0, 2.1),
            &b,
            &tb,
            Vector2::ZERO,
            None,
            None,
            0.0,
            0.0
        ));
    }
}

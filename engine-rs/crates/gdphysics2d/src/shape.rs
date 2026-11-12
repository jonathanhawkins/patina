//! Collision shape definitions.
//!
//! Provides 2D collision shapes for the physics engine: circles, rectangles,
//! line segments, and capsules. Each shape can compute its axis-aligned
//! bounding box and test point containment.

use gdcore::math::{Rect2, Vector2};

/// Maximum number of vertices stored inline for polygon collision shapes.
///
/// Convex and concave polygon shapes store their vertices in a fixed-size
/// array so that [`Shape2D`] remains `Copy` (avoiding heap allocation and a
/// `Clone`-only ripple through every body/area that stores a shape). Polygons
/// with more vertices than this are truncated at construction time — see
/// [`Shape2D::convex_polygon`] / [`Shape2D::concave_polygon`].
pub const MAX_POLY_VERTS: usize = 12;

/// A 2D collision shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape2D {
    /// A circle centered at the origin with the given radius.
    Circle { radius: f32 },
    /// An axis-aligned rectangle defined by half-extents from the origin.
    Rectangle { half_extents: Vector2 },
    /// A line segment between two local points.
    Segment { a: Vector2, b: Vector2 },
    /// A capsule oriented vertically (along Y), defined by radius and total height.
    Capsule { radius: f32, height: f32 },
    /// A convex polygon defined by up to [`MAX_POLY_VERTS`] local-space
    /// vertices. Only `points[..count]` are valid; the vertices describe a
    /// convex hull in either winding order (Godot's `ConvexPolygonShape2D`).
    ConvexPolygon {
        /// Local-space vertices; only the first `count` entries are used.
        points: [Vector2; MAX_POLY_VERTS],
        /// Number of valid vertices in `points`.
        count: u8,
    },
    /// A concave polygon stored as a soup of line segments, matching Godot's
    /// `ConcavePolygonShape2D` "segments" layout: consecutive pairs
    /// `(points[0], points[1]), (points[2], points[3]), …` each form one
    /// segment. Only `points[..count]` are valid.
    ConcavePolygon {
        /// Local-space segment endpoints; only the first `count` are used.
        points: [Vector2; MAX_POLY_VERTS],
        /// Number of valid points (`count / 2` segments).
        count: u8,
    },
    /// An infinite half-plane (Godot's `WorldBoundaryShape2D`). The solid
    /// region is everything on the side of the line opposite `normal`; the
    /// line passes through `normal * distance` in local space.
    WorldBoundary {
        /// Unit outward normal of the boundary line (points to the empty side).
        normal: Vector2,
        /// Signed distance of the line from the origin along `normal`.
        distance: f32,
    },
    /// A separation ray (Godot's `SeparationRayShape2D`): a ray of length
    /// `length` pointing along local +Y, used by kinematic bodies to stay
    /// separated from surfaces.
    SeparationRay {
        /// Length of the ray along local +Y.
        length: f32,
    },
}

/// Packs a slice of vertices into a fixed-size buffer, truncating to
/// [`MAX_POLY_VERTS`]. Returns the buffer and the number of valid entries.
fn pack_points(points: &[Vector2]) -> ([Vector2; MAX_POLY_VERTS], u8) {
    // Oversized polygons are truncated to `MAX_POLY_VERTS` rather than rejected,
    // so this must never panic on `points.len() > MAX_POLY_VERTS` — that is the
    // documented contract (`convex_polygon`/`concave_polygon`) and is exercised
    // by `convex_polygon_truncates_to_max`.
    let mut buf = [Vector2::ZERO; MAX_POLY_VERTS];
    let n = points.len().min(MAX_POLY_VERTS);
    buf[..n].copy_from_slice(&points[..n]);
    (buf, n as u8)
}

/// Computes the axis-aligned bounding rectangle covering `points`.
///
/// Returns a zero rect at the origin when `points` is empty.
fn points_bounding_rect(points: &[Vector2]) -> Rect2 {
    let Some(first) = points.first() else {
        return Rect2::new(Vector2::ZERO, Vector2::ZERO);
    };
    let mut min = *first;
    let mut max = *first;
    for p in &points[1..] {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
    }
    Rect2::new(min, Vector2::new(max.x - min.x, max.y - min.y))
}

impl Shape2D {
    /// Builds a [`Shape2D::ConvexPolygon`] from local-space vertices.
    ///
    /// At most [`MAX_POLY_VERTS`] vertices are stored; any beyond that are
    /// silently dropped (truncated at construction time).
    pub fn convex_polygon(points: &[Vector2]) -> Self {
        let (points, count) = pack_points(points);
        Shape2D::ConvexPolygon { points, count }
    }

    /// Builds a [`Shape2D::ConcavePolygon`] from segment endpoints.
    ///
    /// Points are interpreted as consecutive segment pairs. At most
    /// [`MAX_POLY_VERTS`] points are stored.
    pub fn concave_polygon(points: &[Vector2]) -> Self {
        let (points, count) = pack_points(points);
        Shape2D::ConcavePolygon { points, count }
    }

    /// Builds a [`Shape2D::WorldBoundary`] from a normal and signed distance.
    ///
    /// The normal is normalized; a degenerate (zero) normal defaults to
    /// pointing up (`(0, -1)`), matching Godot's default boundary.
    pub fn world_boundary(normal: Vector2, distance: f32) -> Self {
        let normal = if normal.length_squared() > 1e-12 {
            normal.normalized()
        } else {
            Vector2::new(0.0, -1.0)
        };
        Shape2D::WorldBoundary { normal, distance }
    }

    /// Returns the valid vertices of a polygon shape (convex or concave), or an
    /// empty slice for non-polygon shapes.
    pub fn polygon_points(&self) -> &[Vector2] {
        match self {
            Shape2D::ConvexPolygon { points, count }
            | Shape2D::ConcavePolygon { points, count } => {
                &points[..(*count as usize).min(MAX_POLY_VERTS)]
            }
            _ => &[],
        }
    }

    /// Computes the axis-aligned bounding rectangle for broad-phase collision.
    ///
    /// The returned `Rect2` is in local space (centered on the shape origin).
    pub fn bounding_rect(&self) -> Rect2 {
        match *self {
            Shape2D::Circle { radius } => Rect2::new(
                Vector2::new(-radius, -radius),
                Vector2::new(radius * 2.0, radius * 2.0),
            ),
            Shape2D::Rectangle { half_extents } => Rect2::new(
                Vector2::new(-half_extents.x, -half_extents.y),
                Vector2::new(half_extents.x * 2.0, half_extents.y * 2.0),
            ),
            Shape2D::Segment { a, b } => {
                let min_x = a.x.min(b.x);
                let min_y = a.y.min(b.y);
                let max_x = a.x.max(b.x);
                let max_y = a.y.max(b.y);
                Rect2::new(
                    Vector2::new(min_x, min_y),
                    Vector2::new(max_x - min_x, max_y - min_y),
                )
            }
            Shape2D::Capsule { radius, height } => {
                let half_height = height / 2.0;
                Rect2::new(
                    Vector2::new(-radius, -half_height),
                    Vector2::new(radius * 2.0, height),
                )
            }
            Shape2D::ConvexPolygon { .. } | Shape2D::ConcavePolygon { .. } => {
                points_bounding_rect(self.polygon_points())
            }
            Shape2D::WorldBoundary { .. } => {
                // A half-plane is unbounded; approximate with a very large AABB
                // so broad-phase always considers it against nearby bodies.
                const LARGE: f32 = 1.0e9;
                Rect2::new(
                    Vector2::new(-LARGE, -LARGE),
                    Vector2::new(LARGE * 2.0, LARGE * 2.0),
                )
            }
            Shape2D::SeparationRay { length } => {
                let min_y = length.min(0.0);
                Rect2::new(Vector2::new(0.0, min_y), Vector2::new(0.0, length.abs()))
            }
        }
    }

    /// Tests whether a point (in local shape space) is contained within the shape.
    pub fn contains_point(&self, point: Vector2) -> bool {
        match *self {
            Shape2D::Circle { radius } => point.length_squared() <= radius * radius,
            Shape2D::Rectangle { half_extents } => {
                point.x.abs() <= half_extents.x && point.y.abs() <= half_extents.y
            }
            Shape2D::Segment { a, b } => {
                // A segment has zero area; use a small tolerance for point-on-segment.
                let ab = b - a;
                let ap = point - a;
                let t = if ab.length_squared() < 1e-12 {
                    0.0
                } else {
                    ap.dot(ab) / ab.length_squared()
                };
                if !(0.0..=1.0).contains(&t) {
                    return false;
                }
                let closest = a + ab * t;
                (point - closest).length_squared() < 1e-6
            }
            Shape2D::Capsule { radius, height } => {
                let half_h = height / 2.0 - radius;
                // Clamp the y component to the capsule's line segment
                let clamped_y = point.y.clamp(-half_h.max(0.0), half_h.max(0.0));
                let closest = Vector2::new(0.0, clamped_y);
                (point - closest).length_squared() <= radius * radius
            }
            Shape2D::ConvexPolygon { .. } => point_in_convex(point, self.polygon_points()),
            Shape2D::ConcavePolygon { .. } => {
                // Zero-area segment soup: the point is "contained" only if it
                // lies on one of the segments (within tolerance).
                let pts = self.polygon_points();
                pts.chunks_exact(2)
                    .any(|seg| point_on_segment(point, seg[0], seg[1]))
            }
            Shape2D::WorldBoundary { normal, distance } => {
                // Solid region is the side opposite the outward normal.
                point.dot(normal) <= distance
            }
            Shape2D::SeparationRay { length } => {
                point_on_segment(point, Vector2::ZERO, Vector2::new(0.0, length))
            }
        }
    }
}

/// Tests whether `point` lies on the segment `a`–`b` within a small tolerance.
fn point_on_segment(point: Vector2, a: Vector2, b: Vector2) -> bool {
    let ab = b - a;
    let ap = point - a;
    let len_sq = ab.length_squared();
    let t = if len_sq < 1e-12 {
        0.0
    } else {
        (ap.dot(ab) / len_sq).clamp(0.0, 1.0)
    };
    let closest = a + ab * t;
    (point - closest).length_squared() < 1e-6
}

/// Tests whether `point` is inside the convex polygon `verts` (winding-agnostic).
///
/// Returns `false` for degenerate polygons with fewer than three vertices.
fn point_in_convex(point: Vector2, verts: &[Vector2]) -> bool {
    let n = verts.len();
    if n < 3 {
        return false;
    }
    let mut has_pos = false;
    let mut has_neg = false;
    for i in 0..n {
        let a = verts[i];
        let b = verts[(i + 1) % n];
        let cross = (b - a).cross(point - a);
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

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-5;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    // --- AABB tests ---

    #[test]
    fn circle_bounding_rect() {
        let shape = Shape2D::Circle { radius: 5.0 };
        let r = shape.bounding_rect();
        assert!(approx_eq(r.position.x, -5.0));
        assert!(approx_eq(r.position.y, -5.0));
        assert!(approx_eq(r.size.x, 10.0));
        assert!(approx_eq(r.size.y, 10.0));
    }

    #[test]
    fn rectangle_bounding_rect() {
        let shape = Shape2D::Rectangle {
            half_extents: Vector2::new(3.0, 4.0),
        };
        let r = shape.bounding_rect();
        assert!(approx_eq(r.position.x, -3.0));
        assert!(approx_eq(r.position.y, -4.0));
        assert!(approx_eq(r.size.x, 6.0));
        assert!(approx_eq(r.size.y, 8.0));
    }

    #[test]
    fn segment_bounding_rect() {
        let shape = Shape2D::Segment {
            a: Vector2::new(-2.0, 1.0),
            b: Vector2::new(3.0, -4.0),
        };
        let r = shape.bounding_rect();
        assert!(approx_eq(r.position.x, -2.0));
        assert!(approx_eq(r.position.y, -4.0));
        assert!(approx_eq(r.size.x, 5.0));
        assert!(approx_eq(r.size.y, 5.0));
    }

    #[test]
    fn capsule_bounding_rect() {
        let shape = Shape2D::Capsule {
            radius: 2.0,
            height: 10.0,
        };
        let r = shape.bounding_rect();
        assert!(approx_eq(r.position.x, -2.0));
        assert!(approx_eq(r.position.y, -5.0));
        assert!(approx_eq(r.size.x, 4.0));
        assert!(approx_eq(r.size.y, 10.0));
    }

    // --- Point containment tests ---

    #[test]
    fn circle_contains_point() {
        let shape = Shape2D::Circle { radius: 5.0 };
        assert!(shape.contains_point(Vector2::ZERO));
        assert!(shape.contains_point(Vector2::new(3.0, 4.0))); // exactly on boundary
        assert!(!shape.contains_point(Vector2::new(4.0, 4.0))); // outside
    }

    #[test]
    fn rectangle_contains_point() {
        let shape = Shape2D::Rectangle {
            half_extents: Vector2::new(3.0, 4.0),
        };
        assert!(shape.contains_point(Vector2::ZERO));
        assert!(shape.contains_point(Vector2::new(3.0, 4.0))); // on boundary
        assert!(!shape.contains_point(Vector2::new(3.1, 0.0))); // outside
    }

    #[test]
    fn zero_radius_circle_bounding_rect() {
        let shape = Shape2D::Circle { radius: 0.0 };
        let r = shape.bounding_rect();
        assert!(approx_eq(r.size.x, 0.0));
        assert!(approx_eq(r.size.y, 0.0));
    }

    #[test]
    fn zero_size_rectangle_contains_origin() {
        let shape = Shape2D::Rectangle {
            half_extents: Vector2::ZERO,
        };
        // The origin point itself should be "on the boundary" (<=)
        assert!(shape.contains_point(Vector2::ZERO));
        assert!(!shape.contains_point(Vector2::new(0.1, 0.0)));
    }

    // --- New shape variants ---

    #[test]
    fn convex_polygon_bounding_rect() {
        let shape = Shape2D::convex_polygon(&[
            Vector2::new(-2.0, -1.0),
            Vector2::new(3.0, -1.0),
            Vector2::new(3.0, 4.0),
            Vector2::new(-2.0, 4.0),
        ]);
        let r = shape.bounding_rect();
        assert!(approx_eq(r.position.x, -2.0));
        assert!(approx_eq(r.position.y, -1.0));
        assert!(approx_eq(r.size.x, 5.0));
        assert!(approx_eq(r.size.y, 5.0));
    }

    #[test]
    fn convex_polygon_contains_point() {
        // A unit-ish diamond around the origin.
        let shape = Shape2D::convex_polygon(&[
            Vector2::new(0.0, -2.0),
            Vector2::new(2.0, 0.0),
            Vector2::new(0.0, 2.0),
            Vector2::new(-2.0, 0.0),
        ]);
        assert!(shape.contains_point(Vector2::ZERO));
        assert!(shape.contains_point(Vector2::new(0.5, 0.5)));
        assert!(!shape.contains_point(Vector2::new(1.5, 1.5))); // outside a corner
        assert!(!shape.contains_point(Vector2::new(3.0, 0.0)));
    }

    #[test]
    fn convex_polygon_winding_agnostic() {
        // Same square, clockwise winding — containment must still work.
        let cw = Shape2D::convex_polygon(&[
            Vector2::new(-1.0, -1.0),
            Vector2::new(-1.0, 1.0),
            Vector2::new(1.0, 1.0),
            Vector2::new(1.0, -1.0),
        ]);
        assert!(cw.contains_point(Vector2::ZERO));
        assert!(!cw.contains_point(Vector2::new(2.0, 0.0)));
    }

    #[test]
    fn convex_polygon_truncates_to_max() {
        // More than MAX_POLY_VERTS vertices: extras are dropped, count clamps.
        let many: Vec<Vector2> = (0..(MAX_POLY_VERTS + 5))
            .map(|i| Vector2::new(i as f32, 0.0))
            .collect();
        let shape = Shape2D::convex_polygon(&many);
        assert_eq!(shape.polygon_points().len(), MAX_POLY_VERTS);
    }

    #[test]
    fn concave_polygon_bounding_rect() {
        // Two segments forming an open corner.
        let shape = Shape2D::concave_polygon(&[
            Vector2::new(-5.0, 0.0),
            Vector2::new(5.0, 0.0),
            Vector2::new(5.0, 0.0),
            Vector2::new(5.0, 10.0),
        ]);
        let r = shape.bounding_rect();
        assert!(approx_eq(r.position.x, -5.0));
        assert!(approx_eq(r.position.y, 0.0));
        assert!(approx_eq(r.size.x, 10.0));
        assert!(approx_eq(r.size.y, 10.0));
    }

    #[test]
    fn concave_polygon_contains_point_on_segment() {
        let shape = Shape2D::concave_polygon(&[Vector2::new(0.0, 0.0), Vector2::new(10.0, 0.0)]);
        assert!(shape.contains_point(Vector2::new(5.0, 0.0)));
        assert!(!shape.contains_point(Vector2::new(5.0, 1.0)));
    }

    #[test]
    fn world_boundary_contains_point() {
        // Floor: outward normal points up (-Y), line through origin.
        let shape = Shape2D::world_boundary(Vector2::new(0.0, -1.0), 0.0);
        // Solid region is the side opposite the normal => +Y (below the floor).
        assert!(shape.contains_point(Vector2::new(0.0, 5.0)));
        assert!(!shape.contains_point(Vector2::new(0.0, -5.0)));
    }

    #[test]
    fn world_boundary_normalizes_normal() {
        let shape = Shape2D::world_boundary(Vector2::new(0.0, -3.0), 0.0);
        match shape {
            Shape2D::WorldBoundary { normal, .. } => {
                assert!(approx_eq(normal.length(), 1.0));
            }
            _ => panic!("expected world boundary"),
        }
    }

    #[test]
    fn separation_ray_bounding_rect() {
        let shape = Shape2D::SeparationRay { length: 20.0 };
        let r = shape.bounding_rect();
        assert!(approx_eq(r.position.x, 0.0));
        assert!(approx_eq(r.position.y, 0.0));
        assert!(approx_eq(r.size.y, 20.0));
    }
}

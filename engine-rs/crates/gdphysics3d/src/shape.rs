//! 3D collision shape definitions.

use gdcore::math::Vector3;
use gdcore::math3d::Aabb;

/// A 3D collision shape.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape3D {
    /// A sphere centered at the origin with the given radius.
    Sphere { radius: f32 },
    /// An axis-aligned box defined by half-extents from the origin.
    BoxShape { half_extents: Vector3 },
    /// A capsule oriented vertically (along Y), defined by radius and total height.
    CapsuleShape { radius: f32, height: f32 },
    /// A cylinder oriented vertically (along Y), defined by radius and total height.
    CylinderShape { radius: f32, height: f32 },
    /// A convex polygon defined by a set of 3D vertices (convex hull).
    ConvexPolygonShape { points: Vec<Vector3> },
    /// A concave polygon (trimesh) defined by triangle faces.
    ///
    /// `backface_collision` mirrors `ConcavePolygonShape3D.backface_collision`:
    /// when `false` (Godot's default) a body approaching a triangle from behind
    /// its winding normal passes straight through it.
    ConcavePolygonShape {
        faces: Vec<Vector3>,
        backface_collision: bool,
    },
    /// A heightmap grid on the XZ plane, `map_width` × `map_depth` samples of
    /// `map_data` (row-major, one row per Z). Cells are one unit across and the
    /// grid is centred on the shape origin, matching `HeightMapShape3D`.
    HeightMapShape {
        map_width: i32,
        map_depth: i32,
        map_data: Vec<f32>,
    },
    /// A zero-thickness ray from the shape origin along local `+Z` for `length`
    /// units, used to push a body out of geometry it has sunk into. Mirrors
    /// `SeparationRayShape3D`.
    SeparationRayShape { length: f32, slide_on_slope: bool },
    /// An infinite plane defined by a normal and distance from origin.
    WorldBoundaryShape { normal: Vector3, distance: f32 },
}

/// Local-space position of heightmap sample `(x, z)`, matching Godot's
/// `GodotHeightMapShape3D::_get_point`: the grid is centred on the origin with
/// unit cell spacing and the sample height on `Y`.
pub fn heightmap_point(
    map_width: i32,
    map_depth: i32,
    map_data: &[f32],
    x: i32,
    z: i32,
) -> Vector3 {
    let height = if x >= 0 && z >= 0 && x < map_width && z < map_depth {
        map_data
            .get((z * map_width + x) as usize)
            .copied()
            .unwrap_or(0.0)
    } else {
        0.0
    };
    Vector3::new(
        x as f32 - 0.5 * (map_width as f32 - 1.0),
        height,
        z as f32 - 0.5 * (map_depth as f32 - 1.0),
    )
}

/// Expands a heightmap into a flat triangle soup in the shape's local frame,
/// using Godot's per-cell winding (`GodotHeightMapShape3D::cull`): triangle one
/// is `(x,z) (x+1,z) (x,z+1)`, triangle two is `(x+1,z) (x+1,z+1) (x,z+1)`.
///
/// `bounds` optionally restricts generation to the cells overlapping the
/// local-space X/Z window `(min_x, max_x, min_z, max_z)` — this is the
/// "sample triangles on demand" path used by the narrow phase, so a large
/// terrain never materialises its whole soup for one query.
pub fn heightmap_faces(
    map_width: i32,
    map_depth: i32,
    map_data: &[f32],
    bounds: Option<(f32, f32, f32, f32)>,
) -> Vec<Vector3> {
    if map_width < 2 || map_depth < 2 {
        return Vec::new();
    }
    let off_x = 0.5 * (map_width as f32 - 1.0);
    let off_z = 0.5 * (map_depth as f32 - 1.0);

    // Cell index window. Cell (i, j) spans local X in [i - off_x, i + 1 - off_x].
    let (mut x0, mut x1, mut z0, mut z1) = (0, map_width - 1, 0, map_depth - 1);
    if let Some((min_x, max_x, min_z, max_z)) = bounds {
        x0 = ((min_x + off_x).floor() as i32).clamp(0, map_width - 1);
        x1 = ((max_x + off_x).ceil() as i32).clamp(0, map_width - 1);
        z0 = ((min_z + off_z).floor() as i32).clamp(0, map_depth - 1);
        z1 = ((max_z + off_z).ceil() as i32).clamp(0, map_depth - 1);
    }

    let mut faces = Vec::new();
    for z in z0..z1 {
        for x in x0..x1 {
            let p00 = heightmap_point(map_width, map_depth, map_data, x, z);
            let p10 = heightmap_point(map_width, map_depth, map_data, x + 1, z);
            let p01 = heightmap_point(map_width, map_depth, map_data, x, z + 1);
            let p11 = heightmap_point(map_width, map_depth, map_data, x + 1, z + 1);
            faces.extend_from_slice(&[p00, p10, p01]);
            faces.extend_from_slice(&[p10, p11, p01]);
        }
    }
    faces
}

impl Shape3D {
    /// Computes the axis-aligned bounding box for broad-phase collision.
    pub fn bounding_aabb(&self) -> Aabb {
        match *self {
            Shape3D::Sphere { radius } => Aabb::new(
                Vector3::new(-radius, -radius, -radius),
                Vector3::new(radius * 2.0, radius * 2.0, radius * 2.0),
            ),
            Shape3D::BoxShape { half_extents } => Aabb::new(
                Vector3::new(-half_extents.x, -half_extents.y, -half_extents.z),
                Vector3::new(
                    half_extents.x * 2.0,
                    half_extents.y * 2.0,
                    half_extents.z * 2.0,
                ),
            ),
            Shape3D::CapsuleShape { radius, height } => {
                let half_height = height / 2.0;
                Aabb::new(
                    Vector3::new(-radius, -half_height, -radius),
                    Vector3::new(radius * 2.0, height, radius * 2.0),
                )
            }
            Shape3D::CylinderShape { radius, height } => {
                let half_height = height / 2.0;
                Aabb::new(
                    Vector3::new(-radius, -half_height, -radius),
                    Vector3::new(radius * 2.0, height, radius * 2.0),
                )
            }
            Shape3D::ConvexPolygonShape { ref points } => {
                if points.is_empty() {
                    return Aabb::new(Vector3::ZERO, Vector3::ZERO);
                }
                let mut min = points[0];
                let mut max = points[0];
                for p in &points[1..] {
                    min.x = min.x.min(p.x);
                    min.y = min.y.min(p.y);
                    min.z = min.z.min(p.z);
                    max.x = max.x.max(p.x);
                    max.y = max.y.max(p.y);
                    max.z = max.z.max(p.z);
                }
                Aabb::new(
                    min,
                    Vector3::new(max.x - min.x, max.y - min.y, max.z - min.z),
                )
            }
            Shape3D::ConcavePolygonShape { ref faces, .. } => {
                if faces.is_empty() {
                    return Aabb::new(Vector3::ZERO, Vector3::ZERO);
                }
                let mut min = faces[0];
                let mut max = faces[0];
                for p in &faces[1..] {
                    min.x = min.x.min(p.x);
                    min.y = min.y.min(p.y);
                    min.z = min.z.min(p.z);
                    max.x = max.x.max(p.x);
                    max.y = max.y.max(p.y);
                    max.z = max.z.max(p.z);
                }
                Aabb::new(
                    min,
                    Vector3::new(max.x - min.x, max.y - min.y, max.z - min.z),
                )
            }
            Shape3D::HeightMapShape {
                map_width,
                map_depth,
                ref map_data,
            } => {
                if map_width < 1 || map_depth < 1 || map_data.is_empty() {
                    return Aabb::new(Vector3::ZERO, Vector3::ZERO);
                }
                let half_x = 0.5 * (map_width as f32 - 1.0);
                let half_z = 0.5 * (map_depth as f32 - 1.0);
                let mut min_y = f32::INFINITY;
                let mut max_y = f32::NEG_INFINITY;
                for h in map_data {
                    min_y = min_y.min(*h);
                    max_y = max_y.max(*h);
                }
                Aabb::new(
                    Vector3::new(-half_x, min_y, -half_z),
                    Vector3::new(half_x * 2.0, (max_y - min_y).max(0.0), half_z * 2.0),
                )
            }
            Shape3D::SeparationRayShape { length, .. } => Aabb::new(
                Vector3::new(0.0, 0.0, length.min(0.0)),
                Vector3::new(0.0, 0.0, length.abs()),
            ),
            Shape3D::WorldBoundaryShape { .. } => {
                // Infinite plane — return a very large AABB as a sentinel.
                let big = 1e6_f32;
                Aabb::new(
                    Vector3::new(-big, -big, -big),
                    Vector3::new(big * 2.0, big * 2.0, big * 2.0),
                )
            }
        }
    }

    /// Tests whether a point (in local shape space) is contained within the shape.
    pub fn contains_point(&self, point: Vector3) -> bool {
        match *self {
            Shape3D::Sphere { radius } => point.length_squared() <= radius * radius,
            Shape3D::BoxShape { half_extents } => {
                point.x.abs() <= half_extents.x
                    && point.y.abs() <= half_extents.y
                    && point.z.abs() <= half_extents.z
            }
            Shape3D::CapsuleShape { radius, height } => {
                let half_h = (height / 2.0 - radius).max(0.0);
                let clamped_y = point.y.clamp(-half_h, half_h);
                let closest = Vector3::new(0.0, clamped_y, 0.0);
                let diff = point - closest;
                diff.length_squared() <= radius * radius
            }
            Shape3D::CylinderShape { radius, height } => {
                let half_h = height / 2.0;
                if point.y.abs() > half_h {
                    return false;
                }
                let xz_dist_sq = point.x * point.x + point.z * point.z;
                xz_dist_sq <= radius * radius
            }
            Shape3D::ConvexPolygonShape { ref points } => {
                // Exact: inside every face half-space of the hull. Mirrors
                // `GodotConvexPolygonShape3D::intersect_point`.
                let planes = convex_face_planes(points);
                if planes.is_empty() {
                    // Degenerate (fewer than 4 points) or too large to derive
                    // planes for — fall back to the bounding box.
                    return self.bounding_aabb().contains_point(point);
                }
                planes.iter().all(|(n, d)| n.dot(point) <= *d + 1e-5)
            }
            Shape3D::ConcavePolygonShape { .. } => {
                // A triangle soup has no interior — `GodotConcavePolygonShape3D`
                // returns false unconditionally ("face is flat"). Reporting the
                // bounding box here would make every ray originating inside a
                // terrain's AABB silently miss it.
                false
            }
            Shape3D::HeightMapShape { .. } => {
                // Same as the trimesh above: a heightmap is a surface, not a
                // solid, so no point is "inside" it.
                false
            }
            Shape3D::SeparationRayShape { .. } => {
                // A zero-thickness ray contains no volume.
                false
            }
            Shape3D::WorldBoundaryShape { normal, distance } => {
                let dot = normal.x * point.x + normal.y * point.y + normal.z * point.z;
                dot <= distance
            }
        }
    }
}

/// Above this many hull points the O(n^4) face-plane derivation is skipped and
/// callers fall back to the hull's bounding box. Authored
/// `ConvexPolygonShape3D` hulls are far below this.
const MAX_HULL_POINTS_FOR_PLANES: usize = 24;

/// Derives the face planes `(normal, d)` of the convex hull of `points`, with
/// the interior satisfying `normal · p <= d` for every plane.
///
/// Brute force: every vertex triple spans a candidate plane, and it is a face
/// plane exactly when all remaining points lie on one side of it. Godot gets
/// these planes for free because `ConvexPolygonShape3D` stores a precomputed
/// mesh (`Geometry3D::build_convex_mesh`); [`Shape3D::ConvexPolygonShape`]
/// stores only the point cloud, so they are recovered here.
///
/// Returns an empty vec for degenerate (`< 4` points) or oversized
/// (`> MAX_HULL_POINTS_FOR_PLANES`) inputs — callers fall back to the
/// bounding box in that case.
///
/// 🔑 Memoized by the exact point bits (pat-2ifdx). The derivation is O(n^4)
/// — about 50k dot products for a 24-point hull — and the ray and point tests
/// call it per query, so a GridMap level's convex tiles (the same few hulls
/// repeated across thousands of cells) re-derived identical planes several
/// times every tick. The cache is bounded and simply cleared when full.
pub fn convex_face_planes(points: &[Vector3]) -> Vec<(Vector3, f32)> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    const CACHE_LIMIT: usize = 4096;
    #[allow(clippy::type_complexity)]
    static CACHE: OnceLock<Mutex<HashMap<Vec<u32>, Vec<(Vector3, f32)>>>> = OnceLock::new();

    let n = points.len();
    if !(4..=MAX_HULL_POINTS_FOR_PLANES).contains(&n) {
        return Vec::new();
    }
    let key: Vec<u32> = points
        .iter()
        .flat_map(|p| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()])
        .collect();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(map) = cache.lock() {
        if let Some(planes) = map.get(&key) {
            return planes.clone();
        }
    }
    let planes = derive_convex_face_planes(points);
    if let Ok(mut map) = cache.lock() {
        if map.len() >= CACHE_LIMIT {
            map.clear();
        }
        map.insert(key, planes.clone());
    }
    planes
}

/// The uncached derivation behind [`convex_face_planes`].
fn derive_convex_face_planes(points: &[Vector3]) -> Vec<(Vector3, f32)> {
    let n = points.len();

    const EPS: f32 = 1e-4;
    let mut planes: Vec<(Vector3, f32)> = Vec::new();

    for i in 0..n {
        for j in (i + 1)..n {
            for k in (j + 1)..n {
                let raw = (points[j] - points[i]).cross(points[k] - points[i]);
                let len = raw.length();
                if len < 1e-6 {
                    continue; // collinear triple
                }
                let mut normal = raw * (1.0 / len);
                let mut d = normal.dot(points[i]);

                let mut any_above = false;
                let mut any_below = false;
                for p in points {
                    let s = normal.dot(*p) - d;
                    if s > EPS {
                        any_above = true;
                    } else if s < -EPS {
                        any_below = true;
                    }
                }
                if any_above && any_below {
                    continue; // slices through the hull — not a face
                }
                if any_above {
                    // Flip so the hull lies in the `normal · p <= d` half-space.
                    normal = normal * -1.0;
                    d = -d;
                }

                let duplicate = planes
                    .iter()
                    .any(|(pn, pd)| pn.dot(normal) > 0.999 && (pd - d).abs() < EPS);
                if !duplicate {
                    planes.push((normal, d));
                }
            }
        }
    }

    planes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sphere_aabb() {
        let shape = Shape3D::Sphere { radius: 2.0 };
        let aabb = shape.bounding_aabb();
        assert_eq!(aabb.position, Vector3::new(-2.0, -2.0, -2.0));
        assert_eq!(aabb.size, Vector3::new(4.0, 4.0, 4.0));
    }

    #[test]
    fn box_aabb() {
        let shape = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 2.0, 3.0),
        };
        let aabb = shape.bounding_aabb();
        assert_eq!(aabb.position, Vector3::new(-1.0, -2.0, -3.0));
        assert_eq!(aabb.size, Vector3::new(2.0, 4.0, 6.0));
    }

    #[test]
    fn capsule_aabb() {
        let shape = Shape3D::CapsuleShape {
            radius: 1.0,
            height: 4.0,
        };
        let aabb = shape.bounding_aabb();
        assert_eq!(aabb.position, Vector3::new(-1.0, -2.0, -1.0));
        assert_eq!(aabb.size, Vector3::new(2.0, 4.0, 2.0));
    }

    #[test]
    fn sphere_contains_origin() {
        let shape = Shape3D::Sphere { radius: 1.0 };
        assert!(shape.contains_point(Vector3::ZERO));
    }

    #[test]
    fn sphere_excludes_outside() {
        let shape = Shape3D::Sphere { radius: 1.0 };
        assert!(!shape.contains_point(Vector3::new(2.0, 0.0, 0.0)));
    }

    #[test]
    fn box_contains_interior() {
        let shape = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        assert!(shape.contains_point(Vector3::new(0.5, 0.5, 0.5)));
    }

    #[test]
    fn box_excludes_exterior() {
        let shape = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        assert!(!shape.contains_point(Vector3::new(1.5, 0.0, 0.0)));
    }

    #[test]
    fn capsule_contains_center() {
        let shape = Shape3D::CapsuleShape {
            radius: 1.0,
            height: 4.0,
        };
        assert!(shape.contains_point(Vector3::ZERO));
    }

    #[test]
    fn capsule_contains_top_cap() {
        let shape = Shape3D::CapsuleShape {
            radius: 1.0,
            height: 4.0,
        };
        // Top hemisphere center at y=1.0 (half_height - radius = 2.0 - 1.0 = 1.0)
        assert!(shape.contains_point(Vector3::new(0.0, 1.5, 0.0)));
    }
}

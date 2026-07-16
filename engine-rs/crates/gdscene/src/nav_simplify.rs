//! Path simplification for navigation queries (pat-u3l8c.66).
//!
//! Implements the Ramer–Douglas–Peucker (RDP) algorithm that Godot's
//! `NavigationPathQueryParameters.simplify_path` / `simplify_epsilon` use to
//! collapse a dense navigation poly-line down to its significant corners while
//! keeping every point within `epsilon` of the original path. Pure, self
//! contained functions over [`gdcore::math::Vector2`] / [`gdcore::math::Vector3`].
//!
//! # Scope / remaining work (pat-u3l8c.66, a nav pack)
//!
//! This delivers only the `simplify_path` epsilon reduction. The rest of the
//! pack — unifying the duplicate `gdscene`/`gdserver3d` nav models into one 3D
//! model, honoring `navigation_layers` bitmasks and `enter_cost`/`travel_cost`
//! in the search, and the `NavigationPathQueryParameters`/result objects — is
//! not implemented here.

use gdcore::math::{Vector2, Vector3};

/// Simplifies a 2D path with RDP, keeping points that deviate more than
/// `epsilon` from the straight line between retained neighbors. The first and
/// last points are always kept; paths of two or fewer points are returned
/// unchanged.
pub fn simplify_path_2d(points: &[Vector2], epsilon: f32) -> Vec<Vector2> {
    let n = points.len();
    if n <= 2 {
        return points.to_vec();
    }
    let eps = epsilon.max(0.0);
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    rdp_2d(points, 0, n - 1, eps, &mut keep);
    collect(points, &keep)
}

/// Simplifies a 3D path with RDP. See [`simplify_path_2d`].
pub fn simplify_path_3d(points: &[Vector3], epsilon: f32) -> Vec<Vector3> {
    let n = points.len();
    if n <= 2 {
        return points.to_vec();
    }
    let eps = epsilon.max(0.0);
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    rdp_3d(points, 0, n - 1, eps, &mut keep);
    collect(points, &keep)
}

fn collect<T: Copy>(points: &[T], keep: &[bool]) -> Vec<T> {
    points
        .iter()
        .zip(keep.iter())
        .filter_map(|(p, &k)| if k { Some(*p) } else { None })
        .collect()
}

fn rdp_2d(points: &[Vector2], start: usize, end: usize, eps: f32, keep: &mut [bool]) {
    if end <= start + 1 {
        return; // no interior points to consider
    }
    let a = points[start];
    let b = points[end];
    let mut max_dist = -1.0_f32;
    let mut split = start;
    for i in (start + 1)..end {
        let d = perp_distance_2d(points[i], a, b);
        if d > max_dist {
            max_dist = d;
            split = i;
        }
    }
    if max_dist > eps {
        keep[split] = true;
        rdp_2d(points, start, split, eps, keep);
        rdp_2d(points, split, end, eps, keep);
    }
}

fn rdp_3d(points: &[Vector3], start: usize, end: usize, eps: f32, keep: &mut [bool]) {
    if end <= start + 1 {
        return;
    }
    let a = points[start];
    let b = points[end];
    let mut max_dist = -1.0_f32;
    let mut split = start;
    for i in (start + 1)..end {
        let d = perp_distance_3d(points[i], a, b);
        if d > max_dist {
            max_dist = d;
            split = i;
        }
    }
    if max_dist > eps {
        keep[split] = true;
        rdp_3d(points, start, split, eps, keep);
        rdp_3d(points, split, end, eps, keep);
    }
}

/// Perpendicular distance from `p` to the segment line through `a`,`b` (2D).
fn perp_distance_2d(p: Vector2, a: Vector2, b: Vector2) -> f32 {
    let ab = b - a;
    let len = ab.length();
    if len < 1e-9 {
        return (p - a).length();
    }
    (ab.cross(p - a)).abs() / len
}

/// Perpendicular distance from `p` to the line through `a`,`b` (3D).
fn perp_distance_3d(p: Vector3, a: Vector3, b: Vector3) -> f32 {
    let ab = b - a;
    let len = ab.length();
    if len < 1e-9 {
        return (p - a).length();
    }
    ab.cross(p - a).length() / len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_paths_are_returned_unchanged() {
        let one = [Vector2::new(1.0, 1.0)];
        assert_eq!(simplify_path_2d(&one, 0.5), one.to_vec());
        let two = [Vector2::new(0.0, 0.0), Vector2::new(3.0, 4.0)];
        assert_eq!(simplify_path_2d(&two, 0.5), two.to_vec());
    }

    #[test]
    fn collinear_points_collapse_to_endpoints() {
        let path = [
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 0.0),
            Vector2::new(2.0, 0.0),
            Vector2::new(3.0, 0.0),
        ];
        let out = simplify_path_2d(&path, 0.01);
        assert_eq!(out, vec![Vector2::new(0.0, 0.0), Vector2::new(3.0, 0.0)]);
    }

    #[test]
    fn significant_corner_is_kept() {
        // The middle point sits 0.5 off the (0,0)->(2,0) line.
        let path = [
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 0.5),
            Vector2::new(2.0, 0.0),
        ];
        let out = simplify_path_2d(&path, 0.1);
        assert_eq!(out.len(), 3, "corner should be preserved: {out:?}");
        assert_eq!(out[1], Vector2::new(1.0, 0.5));
    }

    #[test]
    fn small_corner_is_dropped_at_large_epsilon() {
        let path = [
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 0.5),
            Vector2::new(2.0, 0.0),
        ];
        // Epsilon 1.0 > the 0.5 deviation, so the corner is removed.
        let out = simplify_path_2d(&path, 1.0);
        assert_eq!(out, vec![Vector2::new(0.0, 0.0), Vector2::new(2.0, 0.0)]);
    }

    #[test]
    fn endpoints_always_survive() {
        let path = [
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 0.0),
            Vector2::new(2.0, 0.0),
        ];
        let out = simplify_path_2d(&path, 100.0);
        assert_eq!(out.first(), Some(&Vector2::new(0.0, 0.0)));
        assert_eq!(out.last(), Some(&Vector2::new(2.0, 0.0)));
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn keeps_multiple_corners_of_a_zigzag() {
        // A zig-zag where every interior point is a sharp corner.
        let path = [
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 1.0),
            Vector2::new(2.0, 0.0),
            Vector2::new(3.0, 1.0),
            Vector2::new(4.0, 0.0),
        ];
        let out = simplify_path_2d(&path, 0.1);
        assert_eq!(out.len(), 5, "all corners kept: {out:?}");
    }

    #[test]
    fn simplify_3d_collinear_and_corner() {
        let straight = [
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 1.0, 1.0),
            Vector3::new(2.0, 2.0, 2.0),
            Vector3::new(3.0, 3.0, 3.0),
        ];
        let out = simplify_path_3d(&straight, 0.01);
        assert_eq!(
            out,
            vec![Vector3::new(0.0, 0.0, 0.0), Vector3::new(3.0, 3.0, 3.0)]
        );

        // A point pushed off the diagonal in Z is kept.
        let bent = [
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 1.0, 1.5),
            Vector3::new(2.0, 2.0, 2.0),
        ];
        let out2 = simplify_path_3d(&bent, 0.1);
        assert_eq!(out2.len(), 3, "3d corner kept: {out2:?}");
    }

    #[test]
    fn order_is_preserved() {
        let path = [
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 2.0),
            Vector2::new(2.0, 0.0),
            Vector2::new(3.0, 2.0),
            Vector2::new(4.0, 0.0),
        ];
        let out = simplify_path_2d(&path, 0.1);
        // Whatever survives must be a subsequence in the original order.
        let mut idx = 0;
        for p in &out {
            while idx < path.len() && path[idx] != *p {
                idx += 1;
            }
            assert!(idx < path.len(), "point {p:?} out of order");
            idx += 1;
        }
    }
}

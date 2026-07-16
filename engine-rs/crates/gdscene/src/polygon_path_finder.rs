//! Godot `PolygonPathFinder` resource — visibility-graph pathfinding
//! (pat-nzgdu.201).
//!
//! `PolygonPathFinder` is a low-level navigation resource: it is [`setup`] with
//! a set of `points` (waypoints, typically the vertices of an obstacle outline)
//! and `connections` (index pairs forming the obstacle **boundary edges**), then
//! queried with [`find_path`] for the shortest poly-line from a start to a goal
//! that does not cross any boundary edge.
//!
//! # Algorithm
//!
//! This is a classic **visibility graph** shortest-path search, implemented as a
//! self-contained pure-geometry primitive over [`gdcore::math::Vector2`]:
//!
//! * The graph nodes are the `points`, plus the temporary `from` / `to` of a
//!   query.
//! * Two nodes are connected by a straight edge iff the segment between them
//!   does not *properly* cross any boundary edge (segments that merely touch at
//!   a shared vertex or graze a corner are allowed, so paths may travel along
//!   and around the obstacle outline).
//! * Edge cost is Euclidean distance plus the destination point's penalty.
//! * The shortest path is found with Dijkstra's algorithm over that graph.
//!
//! [`is_point_inside`] uses even-odd ray casting against the boundary edges, and
//! [`bounds`] returns the axis-aligned extent of the points.
//!
//! # Scope / remaining work (pat-nzgdu.201)
//!
//! This module is the pure resource/algorithm core. The full bead additionally
//! needs `PolygonPathFinder` registered in `class_db.rs` as a `Resource`
//! (`class_exists("PolygonPathFinder")`), and the script-facing bindings that
//! adapt Godot's flat `PackedInt32Array` connections / `Rect2` bounds. Those
//! are not implemented here.
//!
//! [`setup`]: PolygonPathFinder::setup
//! [`find_path`]: PolygonPathFinder::find_path
//! [`is_point_inside`]: PolygonPathFinder::is_point_inside
//! [`bounds`]: PolygonPathFinder::bounds

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use gdcore::math::Vector2;

/// A visibility-graph pathfinder over a set of waypoints and obstacle edges.
///
/// ```
/// use gdscene::polygon_path_finder::PolygonPathFinder;
/// use gdcore::math::Vector2;
///
/// // A single wall from (0,-1) to (0,1) with a waypoint at each end.
/// let mut pf = PolygonPathFinder::new();
/// pf.setup(
///     vec![Vector2::new(0.0, -1.0), Vector2::new(0.0, 1.0)],
///     &[(0, 1)],
/// );
/// // A straight shot from left to right is blocked, so the path detours
/// // through a wall endpoint.
/// let path = pf.find_path(Vector2::new(-1.0, 0.0), Vector2::new(1.0, 0.0));
/// assert!(path.len() >= 3);
/// ```
#[derive(Debug, Clone, Default)]
pub struct PolygonPathFinder {
    points: Vec<Vector2>,
    /// Boundary edges as index pairs into `points`.
    edges: Vec<(usize, usize)>,
    /// Per-point traversal penalty (added to the cost of arriving at a point).
    penalties: HashMap<usize, f32>,
}

impl PolygonPathFinder {
    /// Creates an empty pathfinder. Call [`setup`](Self::setup) before querying.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the graph with `points` and boundary `connections` (index pairs
    /// into `points`). Out-of-range or self-referential connections are ignored.
    /// Clears any previously set point penalties.
    pub fn setup(&mut self, points: Vec<Vector2>, connections: &[(usize, usize)]) {
        let n = points.len();
        self.points = points;
        self.penalties.clear();
        self.edges = connections
            .iter()
            .copied()
            .filter(|&(a, b)| a != b && a < n && b < n)
            .collect();
    }

    /// The waypoints backing this graph.
    pub fn points(&self) -> &[Vector2] {
        &self.points
    }

    /// Sets a non-negative traversal `penalty` added to the cost of routing
    /// through the point at `index`. Out-of-range indices are ignored.
    pub fn set_point_penalty(&mut self, index: usize, penalty: f32) {
        if index < self.points.len() {
            self.penalties.insert(index, penalty.max(0.0));
        }
    }

    /// The penalty previously set for `index` (0.0 if none / out of range).
    pub fn get_point_penalty(&self, index: usize) -> f32 {
        self.penalties.get(&index).copied().unwrap_or(0.0)
    }

    /// Returns the graph point nearest to `point`, or `None` if the graph is
    /// empty.
    pub fn get_closest_point(&self, point: Vector2) -> Option<Vector2> {
        self.closest_index(point).map(|i| self.points[i])
    }

    /// The axis-aligned `(min, max)` corners spanning all points, or `None` if
    /// the graph is empty.
    pub fn bounds(&self) -> Option<(Vector2, Vector2)> {
        let mut it = self.points.iter().copied();
        let first = it.next()?;
        let mut min = first;
        let mut max = first;
        for p in it {
            min = Vector2::new(min.x.min(p.x), min.y.min(p.y));
            max = Vector2::new(max.x.max(p.x), max.y.max(p.y));
        }
        Some((min, max))
    }

    /// Even-odd ray-cast test: is `point` inside the closed region bounded by
    /// the connection edges? Casts a ray toward +X and counts crossings.
    pub fn is_point_inside(&self, point: Vector2) -> bool {
        let mut inside = false;
        for &(ai, bi) in &self.edges {
            let a = self.points[ai];
            let b = self.points[bi];
            // Does a rightward ray from `point` cross edge (a, b)?
            let crosses_y = (a.y > point.y) != (b.y > point.y);
            if crosses_y {
                let t = (point.y - a.y) / (b.y - a.y);
                let x_cross = a.x + t * (b.x - a.x);
                if x_cross > point.x {
                    inside = !inside;
                }
            }
        }
        inside
    }

    /// Finds the shortest visibility-graph path from `from` to `to`.
    ///
    /// Returns the poly-line of waypoints (including `from` and `to`) or an
    /// empty vector if no unobstructed route exists. If `from` can see `to`
    /// directly, the result is `[from, to]`.
    pub fn find_path(&self, from: Vector2, to: Vector2) -> Vec<Vector2> {
        // Node list: [points…, from, to].
        let mut nodes = self.points.clone();
        let from_idx = nodes.len();
        nodes.push(from);
        let to_idx = nodes.len();
        nodes.push(to);
        let count = nodes.len();

        // Adjacency by visibility. `penalty_of` maps a node index to its
        // traversal penalty (0 for the synthetic from/to nodes).
        let penalty_of = |idx: usize| -> f32 {
            if idx < self.points.len() {
                self.penalties.get(&idx).copied().unwrap_or(0.0)
            } else {
                0.0
            }
        };

        let mut adjacency: Vec<Vec<(usize, f32)>> = vec![Vec::new(); count];
        for i in 0..count {
            for j in (i + 1)..count {
                if self.segment_is_clear(nodes[i], nodes[j]) {
                    let dist = nodes[i].distance_to(nodes[j]);
                    adjacency[i].push((j, dist + penalty_of(j)));
                    adjacency[j].push((i, dist + penalty_of(i)));
                }
            }
        }

        // Dijkstra from `from_idx` to `to_idx`.
        let mut dist = vec![f32::INFINITY; count];
        let mut prev = vec![usize::MAX; count];
        let mut heap = BinaryHeap::new();
        dist[from_idx] = 0.0;
        heap.push(State {
            cost: 0.0,
            node: from_idx,
        });

        while let Some(State { cost, node }) = heap.pop() {
            if node == to_idx {
                break;
            }
            if cost > dist[node] {
                continue;
            }
            for &(next, step) in &adjacency[node] {
                let nd = cost + step;
                if nd < dist[next] {
                    dist[next] = nd;
                    prev[next] = node;
                    heap.push(State {
                        cost: nd,
                        node: next,
                    });
                }
            }
        }

        if dist[to_idx].is_infinite() {
            return Vec::new();
        }

        // Reconstruct.
        let mut path = Vec::new();
        let mut cur = to_idx;
        while cur != usize::MAX {
            path.push(nodes[cur]);
            if cur == from_idx {
                break;
            }
            cur = prev[cur];
        }
        path.reverse();
        path
    }

    fn closest_index(&self, point: Vector2) -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        for (i, p) in self.points.iter().enumerate() {
            let d = p.distance_to(point);
            match best {
                Some((_, bd)) if bd <= d => {}
                _ => best = Some((i, d)),
            }
        }
        best.map(|(i, _)| i)
    }

    /// True if the open segment `(a, b)` does not *properly* cross any boundary
    /// edge (touching at a shared vertex or a collinear graze is allowed).
    fn segment_is_clear(&self, a: Vector2, b: Vector2) -> bool {
        for &(ei, ej) in &self.edges {
            let p = self.points[ei];
            let q = self.points[ej];
            if segments_properly_cross(a, b, p, q) {
                return false;
            }
        }
        true
    }
}

/// Signed area (2× triangle) of `a→b→c`; positive when `c` is left of `a→b`.
#[inline]
fn orient(a: Vector2, b: Vector2, c: Vector2) -> f32 {
    (b - a).cross(c - a)
}

/// True iff open segments `(p1,p2)` and `(p3,p4)` cross at an interior point of
/// both. Endpoint touches and collinear overlaps return `false`, so a path may
/// travel along an obstacle edge or through a shared vertex.
fn segments_properly_cross(p1: Vector2, p2: Vector2, p3: Vector2, p4: Vector2) -> bool {
    let d1 = orient(p3, p4, p1);
    let d2 = orient(p3, p4, p2);
    let d3 = orient(p1, p2, p3);
    let d4 = orient(p1, p2, p4);
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

/// A Dijkstra frontier entry ordered as a min-heap by `cost`.
#[derive(Copy, Clone)]
struct State {
    cost: f32,
    node: usize,
}

impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost
    }
}

impl Eq for State {}

impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reversed so `BinaryHeap` (a max-heap) yields the smallest cost first.
        other.cost.total_cmp(&self.cost)
    }
}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: Vector2, b: Vector2) -> bool {
        a.distance_to(b) < 1e-4
    }

    fn path_length(path: &[Vector2]) -> f32 {
        path.windows(2).map(|w| w[0].distance_to(w[1])).sum()
    }

    #[test]
    fn direct_path_when_unobstructed() {
        // No edges → full visibility → straight line.
        let mut pf = PolygonPathFinder::new();
        pf.setup(vec![Vector2::new(5.0, 5.0)], &[]);
        let path = pf.find_path(Vector2::new(0.0, 0.0), Vector2::new(10.0, 0.0));
        assert_eq!(path.len(), 2);
        assert!(approx(path[0], Vector2::new(0.0, 0.0)));
        assert!(approx(path[1], Vector2::new(10.0, 0.0)));
    }

    #[test]
    fn polygon_path_finder_routes_around_gap() {
        // A vertical wall from (0,-2)..(0,2) blocks the straight left→right
        // route; waypoints sit just past each wall end so the path must detour
        // around a tip instead of cutting through.
        let mut pf = PolygonPathFinder::new();
        pf.setup(
            vec![
                Vector2::new(0.0, -2.0), // 0: wall bottom
                Vector2::new(0.0, 2.0),  // 1: wall top
                Vector2::new(0.0, -2.5), // 2: waypoint below the wall
                Vector2::new(0.0, 2.5),  // 3: waypoint above the wall
            ],
            &[(0, 1)], // the wall
        );
        let from = Vector2::new(-3.0, 0.0);
        let to = Vector2::new(3.0, 0.0);
        let path = pf.find_path(from, to);

        assert!(path.len() >= 3, "path should detour, got {path:?}");
        assert!(approx(path[0], from), "starts at from");
        assert!(approx(*path.last().unwrap(), to), "ends at to");

        // The detour must be strictly longer than the (blocked) straight line.
        let straight = from.distance_to(to);
        assert!(
            path_length(&path) > straight + 1e-3,
            "detour {} should exceed straight {}",
            path_length(&path),
            straight
        );

        // Every leg of the returned path must itself be unobstructed.
        for w in path.windows(2) {
            assert!(
                pf.segment_is_clear(w[0], w[1]),
                "leg {:?}->{:?} crosses the wall",
                w[0],
                w[1]
            );
        }
    }

    #[test]
    fn get_closest_point_returns_nearest() {
        let mut pf = PolygonPathFinder::new();
        pf.setup(
            vec![
                Vector2::new(0.0, 0.0),
                Vector2::new(10.0, 0.0),
                Vector2::new(0.0, 10.0),
            ],
            &[],
        );
        let c = pf.get_closest_point(Vector2::new(9.0, 1.0)).unwrap();
        assert!(approx(c, Vector2::new(10.0, 0.0)), "got {c:?}");
        let c2 = pf.get_closest_point(Vector2::new(1.0, 9.0)).unwrap();
        assert!(approx(c2, Vector2::new(0.0, 10.0)), "got {c2:?}");
    }

    #[test]
    fn get_closest_point_empty_is_none() {
        let pf = PolygonPathFinder::new();
        assert!(pf.get_closest_point(Vector2::new(0.0, 0.0)).is_none());
    }

    #[test]
    fn is_point_inside_square() {
        let mut pf = PolygonPathFinder::new();
        pf.setup(
            vec![
                Vector2::new(-1.0, -1.0),
                Vector2::new(1.0, -1.0),
                Vector2::new(1.0, 1.0),
                Vector2::new(-1.0, 1.0),
            ],
            &[(0, 1), (1, 2), (2, 3), (3, 0)],
        );
        assert!(pf.is_point_inside(Vector2::new(0.0, 0.0)));
        assert!(pf.is_point_inside(Vector2::new(0.5, -0.5)));
        assert!(!pf.is_point_inside(Vector2::new(2.0, 0.0)));
        assert!(!pf.is_point_inside(Vector2::new(0.0, 5.0)));
        assert!(!pf.is_point_inside(Vector2::new(-3.0, -3.0)));
    }

    #[test]
    fn bounds_span_all_points() {
        let mut pf = PolygonPathFinder::new();
        pf.setup(
            vec![
                Vector2::new(-2.0, 3.0),
                Vector2::new(4.0, -1.0),
                Vector2::new(0.0, 7.0),
            ],
            &[],
        );
        let (min, max) = pf.bounds().unwrap();
        assert!(approx(min, Vector2::new(-2.0, -1.0)), "min {min:?}");
        assert!(approx(max, Vector2::new(4.0, 7.0)), "max {max:?}");
    }

    #[test]
    fn bounds_empty_is_none() {
        assert!(PolygonPathFinder::new().bounds().is_none());
    }

    #[test]
    fn penalty_pushes_path_off_a_costly_waypoint() {
        // A wall (0,-3)..(0,3); the cheapest way around is via a wall tip, with
        // a farther fallback waypoint at (0,-4). Penalizing both tips forces the
        // longer detour through the fallback.
        let mut pf = PolygonPathFinder::new();
        pf.setup(
            vec![
                Vector2::new(0.0, -3.0), // 0: wall bottom tip
                Vector2::new(0.0, 3.0),  // 1: wall top tip
                Vector2::new(0.0, -4.0), // 2: farther fallback detour
            ],
            &[(0, 1)],
        );
        let from = Vector2::new(-2.0, 0.0);
        let to = Vector2::new(2.0, 0.0);

        // Un-penalized: routes around a wall tip.
        let cheap = pf.find_path(from, to);
        let cheap_len = path_length(&cheap);
        let uses_tip = cheap
            .iter()
            .any(|p| approx(*p, Vector2::new(0.0, 3.0)) || approx(*p, Vector2::new(0.0, -3.0)));
        assert!(
            uses_tip,
            "un-penalized route should hug a wall tip: {cheap:?}"
        );

        // Penalize both tips: the search must take the longer fallback.
        pf.set_point_penalty(0, 100.0);
        pf.set_point_penalty(1, 100.0);
        let rerouted = pf.find_path(from, to);
        assert!(!rerouted.is_empty());
        let uses_fallback = rerouted.iter().any(|p| approx(*p, Vector2::new(0.0, -4.0)));
        assert!(
            uses_fallback,
            "penalty should force the fallback: {rerouted:?}"
        );
        assert!(
            path_length(&rerouted) > cheap_len + 1e-3,
            "penalized detour {} should be geometrically longer than {}",
            path_length(&rerouted),
            cheap_len
        );
    }

    #[test]
    fn segments_properly_cross_basics() {
        // Clear crossing.
        assert!(segments_properly_cross(
            Vector2::new(-1.0, 0.0),
            Vector2::new(1.0, 0.0),
            Vector2::new(0.0, -1.0),
            Vector2::new(0.0, 1.0),
        ));
        // Shared endpoint only — not a proper cross.
        assert!(!segments_properly_cross(
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 0.0),
            Vector2::new(0.0, 0.0),
            Vector2::new(0.0, 1.0),
        ));
        // Disjoint.
        assert!(!segments_properly_cross(
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 0.0),
            Vector2::new(0.0, 2.0),
            Vector2::new(1.0, 2.0),
        ));
        // T-touch (endpoint lies on the other segment's interior) — grazing,
        // allowed.
        assert!(!segments_properly_cross(
            Vector2::new(-1.0, 0.0),
            Vector2::new(1.0, 0.0),
            Vector2::new(0.0, 0.0),
            Vector2::new(0.0, 1.0),
        ));
    }
}

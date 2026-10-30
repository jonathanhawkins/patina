//! Navigation and pathfinding for the Patina Engine.
//!
//! Provides 2D navigation meshes, A\* pathfinding, navigation agents,
//! and obstacles, mirroring Godot's navigation system. Includes basic
//! 3D stubs for future expansion.

use std::collections::{BinaryHeap, HashMap};

use gdcore::math::{Vector2, Vector3};

use crate::node::NodeId;

/// The polygon index lists of a baked navmesh, clamped to `vert_count`.
///
/// Godot stores a `NavigationPolygon` as a vertex pool plus a list of index
/// arrays, one per convex polygon. A region baked from a single outline has no
/// polygon list at all, which is why an absent `__navmesh_polygons__` with three
/// or more vertices means "one polygon over every vertex" rather than "no
/// polygons".
///
/// ⚠️ Out-of-range indices are DROPPED rather than clamped or treated as an
/// error. A `.tscn` can name a vertex the pool does not have, and indexing the
/// pool with it panics; dropping loses one corner of one polygon, which is the
/// least destructive reading of a file that is already wrong.
///
/// Shared with `main_loop`, which held a byte-identical copy until pat-s3rrv.82.
/// Two hand-maintained copies of one parser is the shape that drifts silently:
/// the overlay and the runtime navmesh would disagree about which polygons
/// exist, and nothing would say so.
pub(crate) fn navmesh_polygon_indices(
    node: &crate::node::Node,
    vert_count: usize,
) -> Vec<Vec<usize>> {
    match node.get_property("__navmesh_polygons__") {
        gdvariant::Variant::Array(entries) => entries
            .to_vec()
            .iter()
            .filter_map(|entry| match entry {
                gdvariant::Variant::PackedInt32Array(ids) => Some(
                    ids.iter()
                        .filter_map(|&i| usize::try_from(i).ok())
                        .filter(|&i| i < vert_count)
                        .collect::<Vec<usize>>(),
                ),
                _ => None,
            })
            .collect(),
        _ if vert_count >= 3 => vec![(0..vert_count).collect()],
        _ => Vec::new(),
    }
}
/// A baked `NavigationRegion2D`'s polygons, in **node-local** space.
///
/// The canonical reader for the `__navmesh_vertices__` / `__navmesh_polygons__`
/// pair the bake route writes. It lives here, in `gdscene`, because every
/// consumer depends on this crate: the editor overlay, the editor viewport's
/// immediate-mode chrome, the performance monitors, and `patina-runner`, which
/// depends on `gdscene` and not on `gdeditor`.
///
/// Two behaviours a re-implementation would drift from:
/// - a resource carrying vertices but **no** `polygons` entry is one polygon
///   over all of them, which is what a hand-authored single-face mesh looks
///   like;
/// - out-of-range indices are dropped rather than panicking, so a malformed
///   `.tscn` costs that polygon and not the frame.
///
/// Polygons of fewer than three points are dropped, so every one returned is
/// drawable and countable.
///
/// ⚠️ **Local space, deliberately.** Callers know where the node is — the
/// overlay adds the region's position, the editor viewport has already mapped
/// the global position into screen space. Handing out pre-offset points is how
/// a caller ends up applying the position twice.
pub fn navmesh_polygons_local(node: &crate::node::Node) -> Vec<Vec<Vector2>> {
    let gdvariant::Variant::PackedVector2Array(verts) = node.get_property("__navmesh_vertices__")
    else {
        return Vec::new();
    };
    navmesh_polygon_indices(node, verts.len())
        .into_iter()
        .map(|ids| ids.iter().map(|&i| verts[i]).collect::<Vec<Vector2>>())
        .filter(|points| points.len() >= 3)
        .collect()
}
use crate::scene_tree::SceneTree;

// ---------------------------------------------------------------------------
// NavPolygon
// ---------------------------------------------------------------------------

/// A convex navigation polygon defined by its vertices.
///
/// Used as the building block for [`NavMesh2D`]. Vertices should be in
/// winding order (CW or CCW — the ray-casting point-in-polygon test works
/// with either).
#[derive(Debug, Clone)]
pub struct NavPolygon {
    /// Ordered vertices of the polygon.
    pub vertices: Vec<Vector2>,
}

impl NavPolygon {
    /// Creates a new navigation polygon from the given vertices.
    pub fn new(vertices: Vec<Vector2>) -> Self {
        Self { vertices }
    }

    /// Returns `true` if `point` lies inside the polygon (ray-casting algorithm).
    pub fn contains_point(&self, point: Vector2) -> bool {
        let n = self.vertices.len();
        if n < 3 {
            return false;
        }
        let mut inside = false;
        let mut j = n - 1;
        for i in 0..n {
            let vi = self.vertices[i];
            let vj = self.vertices[j];
            if ((vi.y > point.y) != (vj.y > point.y))
                && (point.x < (vj.x - vi.x) * (point.y - vi.y) / (vj.y - vi.y) + vi.x)
            {
                inside = !inside;
            }
            j = i;
        }
        inside
    }

    /// Returns the centroid of the polygon.
    pub fn centroid(&self) -> Vector2 {
        if self.vertices.is_empty() {
            return Vector2::ZERO;
        }
        let mut cx = 0.0_f32;
        let mut cy = 0.0_f32;
        for v in &self.vertices {
            cx += v.x;
            cy += v.y;
        }
        let n = self.vertices.len() as f32;
        Vector2::new(cx / n, cy / n)
    }
}

// ---------------------------------------------------------------------------
// NavigationObstacle2D
// ---------------------------------------------------------------------------

/// A circular obstacle that blocks navigation paths.
#[derive(Debug, Clone, Copy)]
pub struct NavigationObstacle2D {
    /// Center position of the obstacle.
    pub position: Vector2,
    /// Radius of the obstacle.
    pub radius: f32,
}

impl NavigationObstacle2D {
    /// Creates a new obstacle.
    pub fn new(position: Vector2, radius: f32) -> Self {
        Self { position, radius }
    }

    /// Returns `true` if the line segment from `a` to `b` intersects this obstacle.
    pub fn blocks_segment(&self, a: Vector2, b: Vector2) -> bool {
        // Closest point on segment to circle center
        let ab = b - a;
        let len_sq = ab.length_squared();
        if len_sq < 1e-12 {
            return (a - self.position).length_squared() < self.radius * self.radius;
        }
        let t = ((self.position - a).dot(ab) / len_sq).clamp(0.0, 1.0);
        let closest = Vector2::new(a.x + ab.x * t, a.y + ab.y * t);
        (closest - self.position).length_squared() < self.radius * self.radius
    }
}

// ---------------------------------------------------------------------------
// NavMesh2D
// ---------------------------------------------------------------------------

/// A 2D navigation mesh composed of connected [`NavPolygon`]s.
///
/// The connectivity graph is built by detecting shared edges between polygons.
#[derive(Debug, Clone)]
pub struct NavMesh2D {
    /// The polygons that make up this navigation mesh.
    pub polygons: Vec<NavPolygon>,
    /// Adjacency list: `connections[i]` contains indices of polygons adjacent to polygon `i`.
    pub connections: Vec<Vec<usize>>,
}

impl NavMesh2D {
    /// Builds a navigation mesh and computes polygon connectivity from shared edges.
    ///
    /// Two polygons are considered connected if they share at least two vertices
    /// that are within `edge_tolerance` distance of each other.
    pub fn new(polygons: Vec<NavPolygon>, edge_tolerance: f32) -> Self {
        let n = polygons.len();
        let mut connections = vec![Vec::new(); n];
        let tol_sq = edge_tolerance * edge_tolerance;

        for i in 0..n {
            for j in (i + 1)..n {
                if Self::shares_edge(&polygons[i], &polygons[j], tol_sq) {
                    connections[i].push(j);
                    connections[j].push(i);
                }
            }
        }

        Self {
            polygons,
            connections,
        }
    }

    /// Returns the index of the polygon containing `point`, if any.
    pub fn find_polygon(&self, point: Vector2) -> Option<usize> {
        self.polygons.iter().position(|p| p.contains_point(point))
    }

    fn shares_edge(a: &NavPolygon, b: &NavPolygon, tol_sq: f32) -> bool {
        let mut shared = 0;
        for va in &a.vertices {
            for vb in &b.vertices {
                if (*va - *vb).length_squared() < tol_sq {
                    shared += 1;
                    if shared >= 2 {
                        return true;
                    }
                }
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// A* pathfinding (generic)
// ---------------------------------------------------------------------------

/// A node in the A\* open set.
#[derive(Debug)]
struct AStarEntry {
    index: usize,
    f_score: f32,
}

impl PartialEq for AStarEntry {
    fn eq(&self, other: &Self) -> bool {
        self.f_score.to_bits() == other.f_score.to_bits()
    }
}

impl Eq for AStarEntry {}

impl PartialOrd for AStarEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AStarEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse order for min-heap behavior
        other
            .f_score
            .partial_cmp(&self.f_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

/// Runs A\* on a graph defined by `neighbors` and `positions`.
///
/// Returns the sequence of node indices from `start` to `goal`, or `None`
/// if no path exists.
pub fn astar_find_path(
    start: usize,
    goal: usize,
    positions: &[Vector2],
    neighbors: &[Vec<usize>],
) -> Option<Vec<usize>> {
    if start == goal {
        return Some(vec![start]);
    }

    let heuristic = |a: usize, b: usize| -> f32 { (positions[a] - positions[b]).length() };

    let mut open_set = BinaryHeap::new();
    let mut came_from: HashMap<usize, usize> = HashMap::new();
    let mut g_score: HashMap<usize, f32> = HashMap::new();

    g_score.insert(start, 0.0);
    open_set.push(AStarEntry {
        index: start,
        f_score: heuristic(start, goal),
    });

    while let Some(current) = open_set.pop() {
        if current.index == goal {
            // Reconstruct path
            let mut path = vec![goal];
            let mut node = goal;
            while let Some(&prev) = came_from.get(&node) {
                path.push(prev);
                node = prev;
            }
            path.reverse();
            return Some(path);
        }

        let current_g = g_score[&current.index];

        for &neighbor in &neighbors[current.index] {
            let tentative_g = current_g + heuristic(current.index, neighbor);
            let is_better = match g_score.get(&neighbor) {
                Some(&existing) => tentative_g < existing,
                None => true,
            };
            if is_better {
                came_from.insert(neighbor, current.index);
                g_score.insert(neighbor, tentative_g);
                open_set.push(AStarEntry {
                    index: neighbor,
                    f_score: tentative_g + heuristic(neighbor, goal),
                });
            }
        }
    }

    None
}

/// Runs A\* on a graph with 3D positions.
pub fn astar_find_path_3d(
    start: usize,
    goal: usize,
    positions: &[Vector3],
    neighbors: &[Vec<usize>],
) -> Option<Vec<usize>> {
    if start == goal {
        return Some(vec![start]);
    }

    let heuristic = |a: usize, b: usize| -> f32 { (positions[a] - positions[b]).length() };

    let mut open_set = BinaryHeap::new();
    let mut came_from: HashMap<usize, usize> = HashMap::new();
    let mut g_score: HashMap<usize, f32> = HashMap::new();

    g_score.insert(start, 0.0);
    open_set.push(AStarEntry {
        index: start,
        f_score: heuristic(start, goal),
    });

    while let Some(current) = open_set.pop() {
        if current.index == goal {
            let mut path = vec![goal];
            let mut node = goal;
            while let Some(&prev) = came_from.get(&node) {
                path.push(prev);
                node = prev;
            }
            path.reverse();
            return Some(path);
        }

        let current_g = g_score[&current.index];

        for &neighbor in &neighbors[current.index] {
            let tentative_g = current_g + heuristic(current.index, neighbor);
            let is_better = match g_score.get(&neighbor) {
                Some(&existing) => tentative_g < existing,
                None => true,
            };
            if is_better {
                came_from.insert(neighbor, current.index);
                g_score.insert(neighbor, tentative_g);
                open_set.push(AStarEntry {
                    index: neighbor,
                    f_score: tentative_g + heuristic(neighbor, goal),
                });
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Portal funnel + stitching helpers
// ---------------------------------------------------------------------------

/// Vertex-match tolerance used to extract the shared-edge geometry between two
/// polygons already known to be adjacent within a region.
const PORTAL_MATCH_TOL: f32 = 1.0;

/// A stitched navigation graph spanning every region: polygon centroids, an
/// adjacency list, and the portal edge (a `(left, right)` pair once oriented)
/// crossed when moving between each adjacent pair.
struct GlobalGraph {
    positions: Vec<Vector2>,
    neighbors: Vec<Vec<usize>>,
    portals: HashMap<(usize, usize), (Vector2, Vector2)>,
}

/// Midpoint of two points.
fn midpoint(a: Vector2, b: Vector2) -> Vector2 {
    Vector2::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5)
}

/// `true` when two points are within a hair of each other (funnel apex checks).
fn same_point(a: Vector2, b: Vector2) -> bool {
    (a - b).length_squared() < 1e-10
}

/// Records an undirected portal edge between polygons `i` and `j`, avoiding
/// duplicate neighbor entries.
fn add_edge(
    neighbors: &mut [Vec<usize>],
    portals: &mut HashMap<(usize, usize), (Vector2, Vector2)>,
    i: usize,
    j: usize,
    edge: (Vector2, Vector2),
) {
    if !neighbors[i].contains(&j) {
        neighbors[i].push(j);
    }
    if !neighbors[j].contains(&i) {
        neighbors[j].push(i);
    }
    portals.insert((i, j), edge);
    portals.insert((j, i), edge);
}

/// Returns the shared edge between two polygons as `(p, q)` when at least two of
/// their vertices coincide within `tol`, or `None` when they touch at fewer than
/// two points. The two most widely separated matched midpoints are chosen so the
/// portal spans the full shared boundary. Used both for intra-region edges
/// (small `tol`) and cross-region stitching (`edge_connection_margin`).
fn shared_edge_within(a: &NavPolygon, b: &NavPolygon, tol: f32) -> Option<(Vector2, Vector2)> {
    let tol_sq = tol * tol;
    let mut matched: Vec<Vector2> = Vec::new();
    for va in &a.vertices {
        for vb in &b.vertices {
            if (*va - *vb).length_squared() <= tol_sq {
                let mid = midpoint(*va, *vb);
                if !matched.iter().any(|m| same_point(*m, mid)) {
                    matched.push(mid);
                }
            }
        }
    }
    if matched.len() < 2 {
        return None;
    }
    let mut best = (0usize, 1usize);
    let mut best_d = -1.0_f32;
    for i in 0..matched.len() {
        for j in (i + 1)..matched.len() {
            let d = (matched[i] - matched[j]).length_squared();
            if d > best_d {
                best_d = d;
                best = (i, j);
            }
        }
    }
    Some((matched[best.0], matched[best.1]))
}

/// Orients a portal's two endpoints as `(left, right)` relative to travel from
/// `ref_pt` along `forward`, so the funnel algorithm's winding tests are
/// consistent. "Left" is the endpoint more counter-clockwise of the direction
/// of travel (larger 2D cross product).
fn orient_portal(p: Vector2, q: Vector2, ref_pt: Vector2, forward: Vector2) -> (Vector2, Vector2) {
    let cp = forward.cross(p - ref_pt);
    let cq = forward.cross(q - ref_pt);
    if cp >= cq {
        (p, q)
    } else {
        (q, p)
    }
}

/// Twice the signed area of triangle `a,b,c`, using the clockwise-positive
/// convention the funnel algorithm below is written against (positive when `c`
/// is to the *right* of `a`→`b`). This is the sign primitive at the heart of
/// the "simple stupid funnel" traversal — its handedness must match how
/// [`orient_portal`] labels the `left`/`right` funnel walls, or the funnel
/// rounds the wrong (outer) corner.
fn tri_area2(a: Vector2, b: Vector2, c: Vector2) -> f32 {
    (c.x - a.x) * (b.y - a.y) - (b.x - a.x) * (c.y - a.y)
}

/// The "simple stupid funnel" algorithm (Mikko Mononen): given the start and end
/// points and the ordered `(left, right)` portal edges of a corridor, returns
/// the tautest path that stays inside the corridor. A straight shot collapses to
/// `[start, end]`; genuine corners emit intermediate waypoints hugging the
/// portal vertices.
fn string_pull(start: Vector2, end: Vector2, portals: &[(Vector2, Vector2)]) -> Vec<Vector2> {
    // Full portal list: a degenerate start portal, the corridor, then a
    // degenerate end portal.
    let mut ports: Vec<(Vector2, Vector2)> = Vec::with_capacity(portals.len() + 2);
    ports.push((start, start));
    ports.extend_from_slice(portals);
    ports.push((end, end));

    let mut pts: Vec<Vector2> = vec![start];
    let mut apex = start;
    let mut portal_left = start;
    let mut portal_right = start;
    let mut apex_index = 0usize;
    let mut left_index = 0usize;
    let mut right_index = 0usize;

    let mut i = 1usize;
    while i < ports.len() {
        let (left, right) = ports[i];

        // Update the right vertex.
        if tri_area2(apex, portal_right, right) <= 0.0 {
            if same_point(apex, portal_right) || tri_area2(apex, portal_left, right) > 0.0 {
                portal_right = right;
                right_index = i;
            } else {
                // Right crossed over left: left vertex becomes the new apex.
                if !same_point(*pts.last().unwrap(), portal_left) {
                    pts.push(portal_left);
                }
                apex = portal_left;
                apex_index = left_index;
                portal_left = apex;
                portal_right = apex;
                left_index = apex_index;
                right_index = apex_index;
                i = apex_index + 1;
                continue;
            }
        }

        // Update the left vertex.
        if tri_area2(apex, portal_left, left) >= 0.0 {
            if same_point(apex, portal_left) || tri_area2(apex, portal_right, left) < 0.0 {
                portal_left = left;
                left_index = i;
            } else {
                // Left crossed over right: right vertex becomes the new apex.
                if !same_point(*pts.last().unwrap(), portal_right) {
                    pts.push(portal_right);
                }
                apex = portal_right;
                apex_index = right_index;
                portal_left = apex;
                portal_right = apex;
                left_index = apex_index;
                right_index = apex_index;
                i = apex_index + 1;
                continue;
            }
        }

        i += 1;
    }

    if !same_point(*pts.last().unwrap(), end) {
        pts.push(end);
    }
    pts
}

/// Distance from point `c` to segment `a`→`b`.
fn point_segment_distance(c: Vector2, a: Vector2, b: Vector2) -> f32 {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq < 1e-12 {
        return (c - a).length();
    }
    let t = ((c - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    let closest = Vector2::new(a.x + ab.x * t, a.y + ab.y * t);
    (c - closest).length()
}

/// Closest point on segment `a`→`b` to point `c` (the point form of
/// [`point_segment_distance`]). Used by the closest-point queries.
fn closest_point_on_segment(c: Vector2, a: Vector2, b: Vector2) -> Vector2 {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq < 1e-12 {
        return a;
    }
    let t = ((c - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    Vector2::new(a.x + ab.x * t, a.y + ab.y * t)
}

/// Computes a detour waypoint that steers the segment `a`→`b` clear of a
/// circular obstacle centered at `c` with radius `r`. The point is placed on the
/// perpendicular through the obstacle, on the side already favored by the
/// segment, at `r` plus a clearance margin from the center.
fn detour_point(a: Vector2, b: Vector2, c: Vector2, r: f32) -> Vector2 {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq < 1e-12 {
        return Vector2::new(c.x, c.y + r * 1.5 + 1.0);
    }
    let t = ((c - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    let closest = Vector2::new(a.x + ab.x * t, a.y + ab.y * t);
    // Unit perpendicular to the segment.
    let inv_len = 1.0 / len_sq.sqrt();
    let mut perp = Vector2::new(-ab.y * inv_len, ab.x * inv_len);
    // Prefer the side the segment already leans toward (away from the center).
    let away = closest - c;
    if away.length_squared() > 1e-8 && perp.dot(away) < 0.0 {
        perp = Vector2::new(-perp.x, -perp.y);
    }
    let clearance = r * 0.5 + 1.0;
    Vector2::new(
        c.x + perp.x * (r + clearance),
        c.y + perp.y * (r + clearance),
    )
}

// ---------------------------------------------------------------------------
// NavigationServer2D
// ---------------------------------------------------------------------------

/// The central 2D navigation server.
///
/// Holds navigation mesh regions and obstacles, and provides pathfinding
/// via A\* on the polygon connectivity graph with string-pulling for
/// smooth paths.
#[derive(Debug, Clone)]
pub struct NavigationServer2D {
    /// Registered navigation mesh regions.
    pub regions: Vec<NavMesh2D>,
    /// Registered obstacles.
    pub obstacles: Vec<NavigationObstacle2D>,
    /// Registered navigation links that bridge otherwise-disjoint regions.
    pub links: Vec<NavigationLink2D>,
    /// Maximum distance between the edges of two *separate* regions for them to
    /// be automatically stitched into one navigation graph, mirroring Godot's
    /// `NavigationServer2D.map_set_edge_connection_margin`. Region edges whose
    /// endpoints fall within this margin are treated as a shared portal so an
    /// agent can walk from one region into the next without an explicit link.
    pub edge_connection_margin: f32,
}

/// Default edge-connection margin used when a server is created. Matches the
/// spirit of Godot's small positive default so that touching (or nearly
/// touching) regions stitch, while clearly separated regions do not.
pub const DEFAULT_EDGE_CONNECTION_MARGIN: f32 = 1.0;

impl NavigationServer2D {
    /// Creates a new, empty navigation server.
    pub fn new() -> Self {
        Self {
            regions: Vec::new(),
            obstacles: Vec::new(),
            links: Vec::new(),
            edge_connection_margin: DEFAULT_EDGE_CONNECTION_MARGIN,
        }
    }

    /// Sets the [`edge_connection_margin`](Self::edge_connection_margin) used to
    /// stitch adjacent regions, mirroring Godot's
    /// `map_set_edge_connection_margin`. Negative values are clamped to `0`
    /// (no cross-region stitching).
    pub fn set_edge_connection_margin(&mut self, margin: f32) {
        self.edge_connection_margin = margin.max(0.0);
    }

    /// Adds a navigation mesh region and returns its index.
    pub fn add_region(&mut self, mesh: NavMesh2D) -> usize {
        self.regions.push(mesh);
        self.regions.len() - 1
    }

    /// Returns the point on the navigation mesh closest to `to_point`
    /// (Godot `NavigationServer2D.map_get_closest_point`).
    ///
    /// If `to_point` already lies inside a region polygon it is returned
    /// unchanged (it is already on the mesh); otherwise the nearest point on any
    /// region polygon's boundary is returned. With no regions, `to_point` is
    /// returned unchanged.
    pub fn map_get_closest_point(&self, to_point: Vector2) -> Vector2 {
        let mut best = to_point;
        let mut best_dist = f32::INFINITY;
        let mut found = false;
        for mesh in &self.regions {
            for poly in &mesh.polygons {
                if poly.contains_point(to_point) {
                    return to_point;
                }
                let verts = &poly.vertices;
                let n = verts.len();
                for i in 0..n {
                    let a = verts[i];
                    let b = verts[(i + 1) % n];
                    let cp = closest_point_on_segment(to_point, a, b);
                    let d = (cp - to_point).length_squared();
                    if d < best_dist {
                        best_dist = d;
                        best = cp;
                        found = true;
                    }
                }
            }
        }
        if found {
            best
        } else {
            to_point
        }
    }

    /// Returns the index of the region owning the mesh point closest to
    /// `to_point` (Godot `map_get_closest_point_owner`), or `None` when there
    /// are no regions.
    pub fn map_get_closest_point_owner(&self, to_point: Vector2) -> Option<usize> {
        let mut best_owner: Option<usize> = None;
        let mut best_dist = f32::INFINITY;
        for (ri, mesh) in self.regions.iter().enumerate() {
            for poly in &mesh.polygons {
                if poly.contains_point(to_point) {
                    return Some(ri);
                }
                let verts = &poly.vertices;
                let n = verts.len();
                for i in 0..n {
                    let cp = closest_point_on_segment(to_point, verts[i], verts[(i + 1) % n]);
                    let d = (cp - to_point).length_squared();
                    if d < best_dist {
                        best_dist = d;
                        best_owner = Some(ri);
                    }
                }
            }
        }
        best_owner
    }

    /// Returns the point on the navigation mesh closest to the segment
    /// `start`→`end` (Godot `map_get_closest_point_to_segment`). The segment is
    /// sampled and the mesh point nearest any sample is returned.
    pub fn map_get_closest_point_to_segment(&self, start: Vector2, end: Vector2) -> Vector2 {
        const SAMPLES: usize = 16;
        let mut best = self.map_get_closest_point(start);
        let mut best_dist = (best - start).length_squared();
        for i in 0..=SAMPLES {
            let t = i as f32 / SAMPLES as f32;
            let s = Vector2::new(
                start.x + (end.x - start.x) * t,
                start.y + (end.y - start.y) * t,
            );
            let cp = self.map_get_closest_point(s);
            let d = (cp - s).length_squared();
            if d < best_dist {
                best_dist = d;
                best = cp;
            }
        }
        best
    }

    /// Adds an obstacle and returns its index.
    pub fn add_obstacle(&mut self, obstacle: NavigationObstacle2D) -> usize {
        self.obstacles.push(obstacle);
        self.obstacles.len() - 1
    }

    /// Adds a [`NavigationLink2D`] and returns its index.
    pub fn add_link(&mut self, link: NavigationLink2D) -> usize {
        self.links.push(link);
        self.links.len() - 1
    }

    /// Whether any registered link is enabled (pat-0q2kx.98).
    ///
    /// [`find_path`](Self::find_path) already threads enabled links into its
    /// stitched graph, so 2D needs this only to let callers tell "no links" from
    /// "links that are all switched off".
    pub fn has_enabled_links(&self) -> bool {
        self.links.iter().any(|l| l.enabled)
    }

    /// Finds a path from `from` to `to` across *all* registered regions, using
    /// A\* on the stitched polygon graph and portal-edge **funnel smoothing**
    /// (the "simple stupid funnel" algorithm) for a taut, corner-hugging result.
    ///
    /// Improvements over the old first-region-only, centroid-routed search
    /// (pat-u3l8c.62):
    /// * **Cross-region stitching.** Every region's polygons are flattened into
    ///   one graph; polygons from *different* regions whose edges fall within
    ///   [`edge_connection_margin`](Self::edge_connection_margin) are joined by a
    ///   shared portal, and enabled [`NavigationLink2D`]s bridge disjoint
    ///   regions — so a path can leave one region and continue in the next.
    /// * **Funnel smoothing.** The corridor of portal edges is string-pulled so
    ///   a straight shot collapses to `[from, to]` instead of zig-zagging
    ///   through polygon centroids; genuine corners still produce waypoints.
    /// * **Obstacle detours.** When an obstacle intersects the smoothed path we
    ///   route *around* it (Godot never nulls a static path); the returned path
    ///   is only empty when the endpoints are genuinely off-mesh or in
    ///   graph-disconnected regions.
    pub fn find_path(&self, from: Vector2, to: Vector2) -> Vec<Vector2> {
        let graph = self.build_global_graph();
        if graph.positions.is_empty() {
            return Vec::new();
        }

        let start = match self.global_polygon_containing(from) {
            Some(i) => i,
            None => return Vec::new(),
        };
        let goal = match self.global_polygon_containing(to) {
            Some(i) => i,
            None => return Vec::new(),
        };

        if start == goal {
            // Same polygon: a direct hop, detoured around any obstacle.
            return self.route_around_obstacles(&[from, to]);
        }

        let poly_path = match astar_find_path(start, goal, &graph.positions, &graph.neighbors) {
            Some(p) => p,
            None => return Vec::new(),
        };

        // Collect the ordered portal edges the corridor crosses and orient each
        // as (left, right) relative to the direction of travel, then funnel.
        let mut portals: Vec<(Vector2, Vector2)> = Vec::with_capacity(poly_path.len());
        for w in poly_path.windows(2) {
            let (a, b) = (w[0], w[1]);
            let edge = graph.portals.get(&(a, b)).copied().unwrap_or_else(|| {
                // No recorded portal geometry (should not happen for A*-adjacent
                // nodes) — fall back to the midpoint of the two centroids so the
                // corridor still threads through a sensible waypoint.
                let m = midpoint(graph.positions[a], graph.positions[b]);
                (m, m)
            });
            let forward = graph.positions[b] - graph.positions[a];
            portals.push(orient_portal(edge.0, edge.1, graph.positions[a], forward));
        }

        let smooth = string_pull(from, to, &portals);
        self.route_around_obstacles(&smooth)
    }

    /// Builds the stitched, portal-annotated navigation graph spanning every
    /// region (see [`find_path`](Self::find_path)).
    fn build_global_graph(&self) -> GlobalGraph {
        let mut positions: Vec<Vector2> = Vec::new();
        let mut polys: Vec<&NavPolygon> = Vec::new();
        let mut region_of: Vec<usize> = Vec::new();
        let mut base_indices: Vec<usize> = Vec::new();
        for (ri, region) in self.regions.iter().enumerate() {
            base_indices.push(positions.len());
            for p in &region.polygons {
                positions.push(p.centroid());
                polys.push(p);
                region_of.push(ri);
            }
        }
        let n = positions.len();
        let mut neighbors = vec![Vec::new(); n];
        let mut portals: HashMap<(usize, usize), (Vector2, Vector2)> = HashMap::new();

        // Intra-region connectivity (already computed from shared edges).
        for (ri, region) in self.regions.iter().enumerate() {
            let base = base_indices[ri];
            for (i, conns) in region.connections.iter().enumerate() {
                for &j in conns {
                    let (gi, gj) = (base + i, base + j);
                    let edge = shared_edge_within(polys[gi], polys[gj], PORTAL_MATCH_TOL)
                        .unwrap_or_else(|| {
                            let m = midpoint(positions[gi], positions[gj]);
                            (m, m)
                        });
                    add_edge(&mut neighbors, &mut portals, gi, gj, edge);
                }
            }
        }

        // Cross-region stitching within the edge-connection margin.
        let margin = self.edge_connection_margin.max(0.0);
        if margin > 0.0 {
            for a in 0..n {
                for b in (a + 1)..n {
                    if region_of[a] == region_of[b] {
                        continue;
                    }
                    if let Some(edge) = shared_edge_within(polys[a], polys[b], margin) {
                        add_edge(&mut neighbors, &mut portals, a, b, edge);
                    }
                }
            }
        }

        // Explicit links bridge regions that share no geometry. The link segment
        // itself acts as the portal the funnel threads through.
        for link in &self.links {
            if !link.enabled {
                continue;
            }
            let a = self.global_polygon_near(&positions, link.start);
            let b = self.global_polygon_near(&positions, link.end);
            if let (Some(a), Some(b)) = (a, b) {
                if a != b {
                    let edge = (link.start, link.end);
                    if !neighbors[a].contains(&b) {
                        neighbors[a].push(b);
                    }
                    portals.insert((a, b), edge);
                    if link.bidirectional {
                        if !neighbors[b].contains(&a) {
                            neighbors[b].push(a);
                        }
                        portals.insert((b, a), edge);
                    }
                }
            }
        }

        GlobalGraph {
            positions,
            neighbors,
            portals,
        }
    }

    /// Reroutes a path around any circular obstacles that intersect its
    /// segments, inserting detour waypoints so the returned path is never empty
    /// on account of an obstacle (Godot keeps the static path). Segments clear
    /// of obstacles are returned unchanged.
    fn route_around_obstacles(&self, path: &[Vector2]) -> Vec<Vector2> {
        if path.len() < 2 {
            return path.to_vec();
        }
        if self.obstacles.is_empty() {
            return path.to_vec();
        }
        let mut out = vec![path[0]];
        for i in 0..path.len() - 1 {
            let seg = self.detour_segment(path[i], path[i + 1], 0);
            // `seg` starts at path[i]; append everything after it.
            out.extend(seg.into_iter().skip(1));
        }
        out
    }

    /// Recursively splits the segment `a`→`b` around the obstacle it penetrates
    /// most deeply, bailing out (returning the raw segment) once `MAX_DETOUR_DEPTH`
    /// is reached so a fully-boxed-in path still returns a corridor rather than
    /// nothing.
    fn detour_segment(&self, a: Vector2, b: Vector2, depth: usize) -> Vec<Vector2> {
        const MAX_DETOUR_DEPTH: usize = 6;
        if depth >= MAX_DETOUR_DEPTH {
            return vec![a, b];
        }
        // Pick the obstacle with the deepest penetration of this segment.
        let mut worst: Option<(&NavigationObstacle2D, f32)> = None;
        for o in &self.obstacles {
            if o.blocks_segment(a, b) {
                let pen = o.radius - point_segment_distance(o.position, a, b);
                if worst.map_or(true, |(_, wp)| pen > wp) {
                    worst = Some((o, pen));
                }
            }
        }
        let o = match worst {
            Some((o, _)) => o,
            None => return vec![a, b],
        };

        let via = detour_point(a, b, o.position, o.radius);
        let left = self.detour_segment(a, via, depth + 1);
        let right = self.detour_segment(via, b, depth + 1);
        let mut combined = left;
        combined.extend(right.into_iter().skip(1));
        combined
    }

    /// Finds a path from `from` to `to`, bridging otherwise-disjoint regions
    /// through any enabled [`NavigationLink2D`].
    ///
    /// Unlike [`Self::find_path`], which searches each region in isolation, this
    /// flattens every region's polygons into a single A\* graph and adds a
    /// bridging edge for each link, so an agent can cross between regions that
    /// share no navmesh geometry. Returns an empty `Vec` if no path exists.
    pub fn find_path_linked(&self, from: Vector2, to: Vector2) -> Vec<Vector2> {
        // Flatten every region's polygons into one global graph. Each region's
        // local connectivity is offset by the number of polygons already added.
        let mut positions: Vec<Vector2> = Vec::new();
        let mut neighbors: Vec<Vec<usize>> = Vec::new();
        for region in &self.regions {
            let base = positions.len();
            for p in &region.polygons {
                positions.push(p.centroid());
            }
            for conns in &region.connections {
                neighbors.push(conns.iter().map(|&c| c + base).collect());
            }
        }

        // Bridge disjoint regions with each enabled link.
        for link in &self.links {
            if !link.enabled {
                continue;
            }
            let a = self.global_polygon_near(&positions, link.start);
            let b = self.global_polygon_near(&positions, link.end);
            if let (Some(a), Some(b)) = (a, b) {
                if a != b {
                    neighbors[a].push(b);
                    if link.bidirectional {
                        neighbors[b].push(a);
                    }
                }
            }
        }

        // Locate start/goal polygons by containment (agent must be on a navmesh).
        let start = match self.global_polygon_containing(from) {
            Some(i) => i,
            None => return Vec::new(),
        };
        let goal = match self.global_polygon_containing(to) {
            Some(i) => i,
            None => return Vec::new(),
        };
        if start == goal {
            return self.route_around_obstacles(&[from, to]);
        }

        if let Some(poly_path) = astar_find_path(start, goal, &positions, &neighbors) {
            let mut path = vec![from];
            for &pi in &poly_path[1..poly_path.len().saturating_sub(1)] {
                path.push(positions[pi]);
            }
            path.push(to);
            return self.route_around_obstacles(&path);
        }
        Vec::new()
    }

    /// Returns the global polygon index (across all regions, in registration
    /// order) that contains `point`, if any.
    fn global_polygon_containing(&self, point: Vector2) -> Option<usize> {
        let mut base = 0usize;
        for region in &self.regions {
            if let Some(i) = region.find_polygon(point) {
                return Some(base + i);
            }
            base += region.polygons.len();
        }
        None
    }

    /// Resolves a link endpoint to a global polygon: prefers the containing
    /// polygon, falling back to the nearest centroid so endpoints placed on a
    /// region boundary still attach.
    fn global_polygon_near(&self, positions: &[Vector2], point: Vector2) -> Option<usize> {
        if let Some(i) = self.global_polygon_containing(point) {
            return Some(i);
        }
        positions
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let da = (**a - point).length_squared();
                let db = (**b - point).length_squared();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
    }
}

impl Default for NavigationServer2D {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// NavigationAgent2D
// ---------------------------------------------------------------------------

/// A navigation milestone signal produced by an agent this step (pat-u3l8c.64),
/// mirroring Godot's `NavigationAgent2D` signals. The runtime drains these via
/// [`NavigationAgent2D::drain_signals`] and emits the matching Godot signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavAgentSignal {
    /// The path was recomputed (`path_changed`).
    PathChanged,
    /// An intermediate path waypoint was reached (`waypoint_reached`).
    WaypointReached,
    /// The final target was reached (`target_reached`).
    TargetReached,
    /// The path was fully traversed (`navigation_finished`).
    NavigationFinished,
    /// A safe avoidance velocity was computed (`velocity_computed`).
    VelocityComputed,
}

/// Per-node navigation agent helpers, mirroring Godot's `NavigationAgent2D`.
#[derive(Debug, Clone)]
pub struct NavigationAgent2D {
    /// The target position the agent is navigating toward.
    pub target_position: Vector2,
    /// The computed path.
    pub path: Vec<Vector2>,
    /// Index of the next waypoint in the path.
    pub path_index: usize,
    /// Godot's `simplify_path`: run the computed path through a
    /// Ramer-Douglas-Peucker reduction before following it.
    ///
    /// ⚠️ The reduction itself (`nav_simplify::simplify_path_2d` / `_3d`) has
    /// existed and been unit-tested since pat-u3l8c.66 with **no callers at
    /// all** — these two fields plus the call in `set_target_position` are what
    /// let the scene-authored knobs reach it (pat-s3rrv.181).
    pub simplify_path: bool,
    /// Godot's `simplify_epsilon`: the RDP tolerance in world units. Zero leaves
    /// the path untouched even when `simplify_path` is set.
    pub simplify_epsilon: f32,
    /// Distance at which a waypoint is considered reached.
    pub path_desired_distance: f32,
    /// Distance at which the **target** is considered reached
    /// (Godot `NavigationAgent2D.target_desired_distance`, default 10.0).
    ///
    /// Godot uses a separate threshold for arrival than for advancing
    /// waypoints; Patina previously reused `path_desired_distance` for both, so
    /// an agent tuned with a wide waypoint tolerance declared itself finished
    /// far from the goal (pat-0q2kx.25).
    pub target_desired_distance: f32,
    /// Whether avoidance is enabled for this agent.
    pub avoidance_enabled: bool,
    /// Avoidance radius of this agent (Godot `NavigationAgent2D.radius`, default 10.0).
    pub radius: f32,
    /// Maximum speed used when integrating avoidance (Godot default 100.0).
    pub max_speed: f32,
    /// Neighbors farther than this are ignored by avoidance (pat-nzgdu.116).
    pub neighbor_distance: f32,
    /// Upper bound on how many neighbors constrain the avoidance solve.
    pub max_neighbors: usize,
    /// Seconds of lookahead for agent-vs-agent collisions.
    pub time_horizon_agents: f32,
    /// Seconds of lookahead for agent-vs-obstacle collisions.
    pub time_horizon_obstacles: f32,
    /// Bitmask this agent is visible on to other agents' avoidance masks.
    pub avoidance_layers: u32,
    /// Bitmask of avoidance layers this agent reacts to.
    pub avoidance_mask: u32,
    /// How strongly this agent holds its course: a higher priority yields less
    /// of the mutual avoidance effort to neighbors (Godot default 1.0).
    pub avoidance_priority: f32,
    /// The velocity most recently requested via [`set_velocity`](Self::set_velocity)
    /// — Godot's `velocity`, i.e. the *desired* velocity before avoidance.
    pub velocity: Vector2,
    /// The collision-free velocity the avoidance solve produced for the last
    /// [`set_velocity`](Self::set_velocity) — the payload a game reads when
    /// `velocity_computed` fires. `NavAgentSignal` is a payload-free `Copy`
    /// enum, so the value is carried here rather than on the signal.
    pub safe_velocity: Vector2,
    /// Whether `navigation_finished` has already been emitted for the current
    /// target — so the runtime fires it exactly once per navigation. Reset by
    /// [`set_target_position`](Self::set_target_position). (pat-nzgdu.10)
    pub finished_emitted: bool,
    /// Navigation milestone signals produced since the last drain (pat-u3l8c.64).
    pending_signals: Vec<NavAgentSignal>,
}

impl NavigationAgent2D {
    /// Creates a new navigation agent.
    pub fn new() -> Self {
        Self {
            target_position: Vector2::ZERO,
            path: Vec::new(),
            path_index: 0,
            // Godot defaults: reduction off, zero tolerance.
            simplify_path: false,
            simplify_epsilon: 0.0,
            path_desired_distance: 4.0,
            target_desired_distance: 10.0,
            avoidance_enabled: false,
            radius: 10.0,
            max_speed: 100.0,
            neighbor_distance: DEFAULT_NEIGHBOR_DISTANCE,
            max_neighbors: DEFAULT_MAX_NEIGHBORS,
            time_horizon_agents: DEFAULT_TIME_HORIZON_AGENTS,
            time_horizon_obstacles: DEFAULT_TIME_HORIZON_OBSTACLES,
            avoidance_layers: 1,
            avoidance_mask: 1,
            avoidance_priority: 1.0,
            velocity: Vector2::ZERO,
            safe_velocity: Vector2::ZERO,
            finished_emitted: false,
            pending_signals: Vec::new(),
        }
    }

    /// Builds this agent's [`OrcaAgent`] view for the avoidance solve, using
    /// `position` as its current world position.
    ///
    /// The solver's `velocity` is the agent's *current* velocity — the last
    /// safe velocity it was given — not the desired one, since ORCA reasons
    /// about where agents are actually heading.
    pub fn orca_agent(&self, position: Vector2) -> OrcaAgent {
        OrcaAgent {
            position,
            velocity: self.safe_velocity,
            radius: self.radius,
            max_speed: self.max_speed,
            time_horizon_agents: self.time_horizon_agents,
            time_horizon_obstacles: self.time_horizon_obstacles,
            neighbor_distance: self.neighbor_distance,
            max_neighbors: self.max_neighbors,
            avoidance_layers: self.avoidance_layers,
            avoidance_mask: self.avoidance_mask,
            avoidance_priority: self.avoidance_priority,
        }
    }

    /// Sets the target position and computes a path using the given server.
    pub fn set_target_position(
        &mut self,
        target: Vector2,
        current: Vector2,
        server: &NavigationServer2D,
    ) {
        self.target_position = target;
        self.path = server.find_path(current, target);
        if self.simplify_path && self.simplify_epsilon > 0.0 {
            self.path = crate::nav_simplify::simplify_path_2d(&self.path, self.simplify_epsilon);
        }
        self.path_index = if self.path.len() > 1 { 1 } else { 0 };
        // A fresh target restarts the navigation lifecycle.
        self.finished_emitted = false;
        self.pending_signals.push(NavAgentSignal::PathChanged);
    }

    /// Returns the next position along the path, advancing the waypoint index
    /// if the agent is close enough.
    pub fn get_next_path_position(&mut self, current: Vector2) -> Vector2 {
        if self.path.is_empty() {
            return current;
        }
        // Advance past waypoints that are close enough
        while self.path_index < self.path.len() {
            let wp = self.path[self.path_index];
            if (wp - current).length() <= self.path_desired_distance {
                self.path_index += 1;
                self.pending_signals.push(NavAgentSignal::WaypointReached);
            } else {
                break;
            }
        }
        if self.path_index < self.path.len() {
            self.path[self.path_index]
        } else {
            // Path fully consumed → arrived at the target (fire once).
            if !self.finished_emitted {
                self.finished_emitted = true;
                self.pending_signals.push(NavAgentSignal::TargetReached);
                self.pending_signals
                    .push(NavAgentSignal::NavigationFinished);
            }
            self.target_position
        }
    }

    /// Returns `true` if navigation is finished (no more waypoints).
    pub fn is_navigation_finished(&self) -> bool {
        self.path.is_empty() || self.path_index >= self.path.len()
    }

    /// Enables or disables avoidance.
    pub fn set_avoidance_enabled(&mut self, enabled: bool) {
        self.avoidance_enabled = enabled;
    }

    /// Returns the preferred velocity toward the next path waypoint, scaled to
    /// [`max_speed`](Self::max_speed).
    ///
    /// Returns `Vector2::ZERO` when the agent has arrived or has no path, so a
    /// stepping loop naturally comes to rest.
    pub fn desired_velocity(&mut self, current: Vector2) -> Vector2 {
        let next = self.get_next_path_position(current);
        let dir = next - current;
        let len = dir.length();
        if len < 1e-6 {
            return Vector2::ZERO;
        }
        Vector2::new(dir.x / len * self.max_speed, dir.y / len * self.max_speed)
    }

    /// Filters `preferred` through avoidance given nearby agents `others`
    /// (each entry is `(position, radius)`).
    ///
    /// When [`avoidance_enabled`](Self::avoidance_enabled) is `false` the
    /// preferred velocity is returned unchanged; otherwise it is deflected away
    /// from neighbors via [`rvo_avoidance_velocity`], mirroring Godot's
    /// `set_velocity` → `velocity_computed` flow.
    pub fn compute_safe_velocity(
        &self,
        current: Vector2,
        preferred: Vector2,
        others: &[(Vector2, f32)],
    ) -> Vector2 {
        if !self.avoidance_enabled {
            return preferred;
        }
        // Positional-only neighbors carry no velocity, so they enter the solve
        // as stationary agents. Callers that know neighbor velocities should
        // prefer `compute_safe_velocity_against`, which is the full-fidelity
        // reciprocal solve.
        let neighbors: Vec<OrcaAgent> = others
            .iter()
            .map(|(position, radius)| OrcaAgent::new(*position, *radius, self.max_speed))
            .collect();
        self.compute_safe_velocity_against(
            current,
            preferred,
            &neighbors,
            &[],
            NOMINAL_AVOIDANCE_DT,
        )
    }

    /// The full ORCA solve: reciprocal avoidance of `neighbors` (whose real
    /// velocities are honoured) and non-reciprocal avoidance of `obstacles`.
    ///
    /// Returns `preferred` unchanged when avoidance is disabled, mirroring
    /// Godot, where a `NavigationAgent2D` with `avoidance_enabled == false`
    /// passes the requested velocity straight through.
    pub fn compute_safe_velocity_against(
        &self,
        current: Vector2,
        preferred: Vector2,
        neighbors: &[OrcaAgent],
        obstacles: &[OrcaAgent],
        dt: f32,
    ) -> Vector2 {
        if !self.avoidance_enabled {
            return preferred;
        }
        orca_safe_velocity(
            &self.orca_agent(current),
            preferred,
            neighbors,
            obstacles,
            dt,
        )
    }

    /// Godot's `set_velocity`: filters `preferred` through avoidance (see
    /// [`compute_safe_velocity`](Self::compute_safe_velocity)), records a
    /// `velocity_computed` milestone carrying the safe velocity, and returns it.
    ///
    /// The safe velocity is also stored on
    /// [`safe_velocity`](Self::safe_velocity), which is what the runtime hands
    /// to the `velocity_computed` signal.
    pub fn set_velocity(
        &mut self,
        current: Vector2,
        preferred: Vector2,
        others: &[(Vector2, f32)],
    ) -> Vector2 {
        let safe = self.compute_safe_velocity(current, preferred, others);
        self.velocity = preferred;
        self.safe_velocity = safe;
        self.pending_signals.push(NavAgentSignal::VelocityComputed);
        safe
    }

    /// Godot's `set_velocity_forced`: adopt `velocity` immediately as both the
    /// desired *and* the current velocity, bypassing the avoidance solve.
    ///
    /// Godot uses this on teleports, where the agent's previous velocity would
    /// otherwise make the next ORCA solve reason from a stale trajectory. No
    /// `velocity_computed` milestone is produced, because nothing was computed.
    pub fn set_velocity_forced(&mut self, velocity: Vector2) {
        self.velocity = velocity;
        self.safe_velocity = velocity;
    }

    /// Removes and returns the navigation milestone signals produced since the
    /// last drain, so the runtime can emit the matching Godot signals.
    pub fn drain_signals(&mut self) -> Vec<NavAgentSignal> {
        std::mem::take(&mut self.pending_signals)
    }

    /// Non-draining view of the pending signals (for inspection/tests).
    pub fn pending_signals(&self) -> &[NavAgentSignal] {
        &self.pending_signals
    }

    /// Advances the agent by `dt` seconds along its path, avoiding the nearby
    /// agents `others` (each `(position, radius)`) when avoidance is enabled.
    ///
    /// This is what actually *wires* RVO into the agent's stepping loop: the
    /// preferred velocity toward the next waypoint is run through
    /// [`compute_safe_velocity`](Self::compute_safe_velocity) before being
    /// integrated. Returns the agent's new position.
    pub fn step(&mut self, current: Vector2, dt: f32, others: &[(Vector2, f32)]) -> Vector2 {
        let preferred = self.desired_velocity(current);
        let safe = self.compute_safe_velocity(current, preferred, others);
        Vector2::new(current.x + safe.x * dt, current.y + safe.y * dt)
    }
}

impl Default for NavigationAgent2D {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// NavigationAgent3D
// ---------------------------------------------------------------------------

/// Per-node 3D navigation agent, mirroring Godot's `NavigationAgent3D`.
#[derive(Debug, Clone)]
pub struct NavigationAgent3D {
    /// The target position the agent is navigating toward.
    pub target_position: Vector3,
    /// The computed path (world-space waypoints).
    pub path: Vec<Vector3>,
    /// Index of the next waypoint in the path.
    pub path_index: usize,
    /// Godot's `simplify_path`: run the computed path through a
    /// Ramer-Douglas-Peucker reduction before following it.
    ///
    /// ⚠️ The reduction itself (`nav_simplify::simplify_path_2d` / `_3d`) has
    /// existed and been unit-tested since pat-u3l8c.66 with **no callers at
    /// all** — these two fields plus the call in `set_target_position` are what
    /// let the scene-authored knobs reach it (pat-s3rrv.181).
    pub simplify_path: bool,
    /// Godot's `simplify_epsilon`: the RDP tolerance in world units. Zero leaves
    /// the path untouched even when `simplify_path` is set.
    pub simplify_epsilon: f32,
    /// Distance at which a waypoint is considered reached.
    pub path_desired_distance: f32,
    /// Distance at which the **target** is considered reached
    /// (Godot `NavigationAgent2D.target_desired_distance`, default 10.0).
    ///
    /// Godot uses a separate threshold for arrival than for advancing
    /// waypoints; Patina previously reused `path_desired_distance` for both, so
    /// an agent tuned with a wide waypoint tolerance declared itself finished
    /// far from the goal (pat-0q2kx.25).
    pub target_desired_distance: f32,
    /// Whether avoidance is enabled for this agent.
    pub avoidance_enabled: bool,
    /// Avoidance radius of this agent (Godot `NavigationAgent3D.radius`, default 0.5).
    pub radius: f32,
    /// Maximum speed used when integrating avoidance (Godot default 10.0).
    pub max_speed: f32,
    /// Whether `navigation_finished` has already been emitted for the current
    /// target (see [`NavigationAgent2D::finished_emitted`]). (pat-nzgdu.10)
    pub finished_emitted: bool,

    // -- 3D avoidance parameters (pat-0q2kx.97) ----------------------------
    //
    // Godot runs 3D avoidance in two modes (doc/classes/NavigationAgent3D.xml).
    // The default is **2D-layered**: agents are cylinders, the ORCA solve runs
    // on the XZ plane, and two agents interact only when their vertical extents
    // overlap — which is what lets a bridge and the ground beneath it hold
    // independent crowds. `use_3d_avoidance` switches to full 3D spheres.
    /// Vertical extent of the agent's cylinder in layered mode (Godot default 1.0).
    pub height: f32,
    /// `true` selects full 3D sphere RVO instead of the layered XZ solve.
    pub use_3d_avoidance: bool,
    /// Whether the solved velocity keeps the agent's own Y component rather
    /// than the one avoidance produced — Godot's `keep_y_velocity`, which stops
    /// the layered solve from flattening a jump or a fall.
    pub keep_y_velocity: bool,
    /// Neighbors beyond this distance are ignored (Godot default 50.0).
    pub neighbor_distance: f32,
    /// At most this many nearest neighbors constrain the solve.
    pub max_neighbors: usize,
    /// Seconds of lookahead for agent-agent collisions.
    pub time_horizon_agents: f32,
    /// Seconds of lookahead for obstacle collisions.
    pub time_horizon_obstacles: f32,
    /// Bitmask this agent is visible on.
    pub avoidance_layers: u32,
    /// Bitmask of layers this agent avoids.
    pub avoidance_mask: u32,
    /// Share of avoidance effort; higher yields less.
    pub avoidance_priority: f32,
    /// The desired velocity the solve starts from (Godot `velocity`).
    pub velocity: Vector3,
    /// The most recent solved velocity (Godot publishes this via
    /// `velocity_computed`).
    pub safe_velocity: Vector3,
}

/// A 3D avoidance obstacle — Godot `NavigationObstacle3D` (pat-0q2kx.97).
///
/// Godot's obstacle contributes two things: a moving radius(+height) cylinder
/// that agents steer around, and a static vertex outline. Only the cylinder is
/// modelled here, which is the part that participates in the RVO solve; the
/// static outline belongs with the navmesh-baking path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NavigationObstacle3D {
    /// World position of the obstacle.
    pub position: Vector3,
    /// Avoidance radius.
    pub radius: f32,
    /// Vertical extent, used for the same height-overlap test as agents.
    pub height: f32,
    /// Velocity, so a moving platform pushes rather than merely blocks.
    pub velocity: Vector3,
    /// Bitmask this obstacle is visible on.
    pub avoidance_layers: u32,
}

impl NavigationObstacle3D {
    /// Creates an obstacle at `position` with `radius` and `height`.
    pub fn new(position: Vector3, radius: f32, height: f32) -> Self {
        Self {
            position,
            radius,
            height,
            velocity: Vector3::ZERO,
            avoidance_layers: 1,
        }
    }
}

/// Whether two vertical extents overlap, given each one's origin and height.
///
/// Godot's layered mode treats an agent as a cylinder whose base is at its
/// origin, so two agents interact only when `[y, y + height)` ranges intersect
/// (`modules/navigation_3d`). Touching exactly at a boundary does not count as
/// overlap, which is what keeps an agent standing precisely on a platform from
/// negotiating with whatever is under it.
pub fn height_ranges_overlap(y_a: f32, height_a: f32, y_b: f32, height_b: f32) -> bool {
    let (a_lo, a_hi) = (y_a, y_a + height_a.max(0.0));
    let (b_lo, b_hi) = (y_b, y_b + height_b.max(0.0));
    a_lo < b_hi && b_lo < a_hi
}

impl NavigationAgent3D {
    /// Creates a new 3D navigation agent.
    pub fn new() -> Self {
        Self {
            target_position: Vector3::ZERO,
            path: Vec::new(),
            path_index: 0,
            // Godot defaults: reduction off, zero tolerance.
            simplify_path: false,
            simplify_epsilon: 0.0,
            path_desired_distance: 1.0,
            target_desired_distance: 1.0,
            avoidance_enabled: false,
            radius: 0.5,
            max_speed: 10.0,
            finished_emitted: false,
            // Godot `NavigationAgent3D` defaults (doc/classes/NavigationAgent3D.xml).
            height: 1.0,
            use_3d_avoidance: false,
            keep_y_velocity: true,
            neighbor_distance: 50.0,
            max_neighbors: 10,
            time_horizon_agents: 1.0,
            time_horizon_obstacles: 0.0,
            avoidance_layers: 1,
            avoidance_mask: 1,
            avoidance_priority: 1.0,
            velocity: Vector3::ZERO,
            safe_velocity: Vector3::ZERO,
        }
    }

    /// The layered-2D view of this agent for the shared ORCA solver
    /// (pat-0q2kx.97).
    ///
    /// Godot's default 3D avoidance *is* the 2D solve run on the XZ plane, so
    /// this projects onto XZ and hands the exact same [`OrcaAgent`] the 2D path
    /// uses — one solver, one set of semantics, rather than a second
    /// implementation that could drift.
    pub fn orca_agent_xz(&self, position: Vector3) -> OrcaAgent {
        let mut agent = OrcaAgent::new(
            Vector2::new(position.x, position.z),
            self.radius,
            self.max_speed,
        );
        agent.velocity = Vector2::new(self.velocity.x, self.velocity.z);
        agent.time_horizon_agents = self.time_horizon_agents;
        agent.time_horizon_obstacles = self.time_horizon_obstacles;
        agent.neighbor_distance = self.neighbor_distance;
        agent.max_neighbors = self.max_neighbors;
        agent.avoidance_layers = self.avoidance_layers;
        agent.avoidance_mask = self.avoidance_mask;
        agent.avoidance_priority = self.avoidance_priority;
        agent
    }

    /// Solves this agent's safe velocity against 3D neighbours and obstacles,
    /// in whichever mode `use_3d_avoidance` selects (pat-0q2kx.97).
    ///
    /// `neighbors` carries each other agent's `(position, height, orca view)` so
    /// the layered mode can reject the ones whose vertical extent does not
    /// overlap this agent's before the XZ solve ever sees them.
    pub fn solve_safe_velocity(
        &self,
        position: Vector3,
        neighbors: &[(Vector3, f32, OrcaAgent)],
        obstacles: &[NavigationObstacle3D],
        dt: f32,
    ) -> Vector3 {
        if !self.avoidance_enabled {
            return self.velocity;
        }

        if self.use_3d_avoidance {
            // Full 3D: spheres, no height filtering — vertical separation is
            // just distance like any other axis.
            let others: Vec<(Vector3, f32)> = neighbors
                .iter()
                .map(|(p, _, view)| (*p, view.radius))
                .chain(obstacles.iter().map(|o| (o.position, o.radius)))
                .collect();
            return rvo_avoidance_velocity_3d(
                position,
                self.velocity,
                &others,
                self.radius,
                self.max_speed,
            );
        }

        // Layered 2D: only cylinders whose vertical extents overlap interact.
        let planar_neighbors: Vec<OrcaAgent> = neighbors
            .iter()
            .filter(|(p, h, _)| height_ranges_overlap(position.y, self.height, p.y, *h))
            .map(|(_, _, view)| *view)
            .collect();
        let planar_obstacles: Vec<OrcaAgent> = obstacles
            .iter()
            .filter(|o| height_ranges_overlap(position.y, self.height, o.position.y, o.height))
            .map(|o| {
                let mut view =
                    OrcaAgent::new(Vector2::new(o.position.x, o.position.z), o.radius, 0.0);
                view.velocity = Vector2::new(o.velocity.x, o.velocity.z);
                view.avoidance_layers = o.avoidance_layers;
                view
            })
            .collect();

        let planar = self.orca_agent_xz(position);
        let safe = orca_safe_velocity(
            &planar,
            Vector2::new(self.velocity.x, self.velocity.z),
            &planar_neighbors,
            &planar_obstacles,
            dt,
        );
        // `keep_y_velocity` preserves the agent's own vertical motion, so a
        // layered solve cannot cancel a jump or a fall — the XZ plane is the
        // only thing it is entitled to steer.
        let y = if self.keep_y_velocity {
            self.velocity.y
        } else {
            0.0
        };
        Vector3::new(safe.x, y, safe.y)
    }

    /// Sets the target and computes a path via the 3D navigation server.
    pub fn set_target_position(
        &mut self,
        target: Vector3,
        current: Vector3,
        server: &NavigationServer3D,
    ) {
        self.target_position = target;
        // The 3D `find_path` searches one region at a time and never looks at
        // links, so a scene whose regions are bridged only by a NavigationLink3D
        // needs the flattened search instead (pat-0q2kx.98). Gated on links
        // actually being present so link-free scenes keep the region-local
        // search — and its existing paths — untouched.
        self.path = if server.has_enabled_links() {
            server.find_path_linked(current, target)
        } else {
            server.find_path(current, target)
        };
        if self.simplify_path && self.simplify_epsilon > 0.0 {
            self.path = crate::nav_simplify::simplify_path_3d(&self.path, self.simplify_epsilon);
        }
        self.path_index = if self.path.len() > 1 { 1 } else { 0 };
        self.finished_emitted = false;
    }

    /// Returns the next path position, advancing when close enough.
    pub fn get_next_path_position(&mut self, current: Vector3) -> Vector3 {
        if self.path.is_empty() {
            return current;
        }
        while self.path_index < self.path.len() {
            let wp = self.path[self.path_index];
            if (wp - current).length() <= self.path_desired_distance {
                self.path_index += 1;
            } else {
                break;
            }
        }
        if self.path_index < self.path.len() {
            self.path[self.path_index]
        } else {
            self.target_position
        }
    }

    /// Returns `true` when there are no remaining waypoints.
    pub fn is_navigation_finished(&self) -> bool {
        self.path.is_empty() || self.path_index >= self.path.len()
    }

    /// Enables or disables avoidance.
    pub fn set_avoidance_enabled(&mut self, enabled: bool) {
        self.avoidance_enabled = enabled;
    }

    /// Returns the preferred velocity toward the next path waypoint, scaled to
    /// [`max_speed`](Self::max_speed). Returns `Vector3::ZERO` when arrived.
    pub fn desired_velocity(&mut self, current: Vector3) -> Vector3 {
        let next = self.get_next_path_position(current);
        let dir = next - current;
        let len = dir.length();
        if len < 1e-6 {
            return Vector3::ZERO;
        }
        Vector3::new(
            dir.x / len * self.max_speed,
            dir.y / len * self.max_speed,
            dir.z / len * self.max_speed,
        )
    }

    /// Filters `preferred` through avoidance given nearby agents `others`
    /// (each `(position, radius)`). Returns `preferred` unchanged when
    /// avoidance is disabled; otherwise deflects away from neighbors via
    /// [`rvo_avoidance_velocity_3d`].
    pub fn compute_safe_velocity(
        &self,
        current: Vector3,
        preferred: Vector3,
        others: &[(Vector3, f32)],
    ) -> Vector3 {
        if !self.avoidance_enabled {
            return preferred;
        }
        rvo_avoidance_velocity_3d(current, preferred, others, self.radius, self.max_speed)
    }

    /// Advances the agent by `dt` seconds along its path, avoiding nearby
    /// agents `others` (each `(position, radius)`) when avoidance is enabled.
    /// Returns the agent's new position.
    pub fn step(&mut self, current: Vector3, dt: f32, others: &[(Vector3, f32)]) -> Vector3 {
        let preferred = self.desired_velocity(current);
        let safe = self.compute_safe_velocity(current, preferred, others);
        Vector3::new(
            current.x + safe.x * dt,
            current.y + safe.y * dt,
            current.z + safe.z * dt,
        )
    }
}

impl Default for NavigationAgent3D {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 3D stubs
// ---------------------------------------------------------------------------

/// A 3D navigation polygon (stub for future expansion).
#[derive(Debug, Clone)]
pub struct NavPolygon3D {
    /// Ordered vertices of the polygon.
    pub vertices: Vec<Vector3>,
}

impl NavPolygon3D {
    /// Creates a new 3D navigation polygon.
    pub fn new(vertices: Vec<Vector3>) -> Self {
        Self { vertices }
    }

    /// Returns the centroid of the polygon.
    pub fn centroid(&self) -> Vector3 {
        if self.vertices.is_empty() {
            return Vector3::ZERO;
        }
        let mut cx = 0.0_f32;
        let mut cy = 0.0_f32;
        let mut cz = 0.0_f32;
        for v in &self.vertices {
            cx += v.x;
            cy += v.y;
            cz += v.z;
        }
        let n = self.vertices.len() as f32;
        Vector3::new(cx / n, cy / n, cz / n)
    }
}

/// A 3D navigation mesh (stub).
#[derive(Debug, Clone)]
pub struct NavMesh3D {
    /// The polygons composing this mesh.
    pub polygons: Vec<NavPolygon3D>,
    /// Adjacency list.
    pub connections: Vec<Vec<usize>>,
}

impl NavMesh3D {
    /// Builds a 3D navigation mesh with manual connections.
    pub fn new(polygons: Vec<NavPolygon3D>, connections: Vec<Vec<usize>>) -> Self {
        Self {
            polygons,
            connections,
        }
    }
}

/// A basic 3D navigation server (stub).
#[derive(Debug, Clone)]
pub struct NavigationServer3D {
    /// Registered 3D navigation meshes.
    pub regions: Vec<NavMesh3D>,
    /// Registered navigation links that bridge otherwise-disjoint regions.
    pub links: Vec<NavigationLink3D>,
}

impl NavigationServer3D {
    /// Creates a new, empty 3D navigation server.
    pub fn new() -> Self {
        Self {
            regions: Vec::new(),
            links: Vec::new(),
        }
    }

    /// Adds a 3D navigation mesh region and returns its index.
    pub fn add_region(&mut self, mesh: NavMesh3D) -> usize {
        self.regions.push(mesh);
        self.regions.len() - 1
    }

    /// Adds a [`NavigationLink3D`] and returns its index.
    pub fn add_link(&mut self, link: NavigationLink3D) -> usize {
        self.links.push(link);
        self.links.len() - 1
    }

    /// Whether any registered link is enabled (pat-0q2kx.98).
    ///
    /// Unlike the 2D server, [`find_path`](Self::find_path) here searches each
    /// region in isolation and never consults links, so an agent has to pick
    /// [`find_path_linked`](Self::find_path_linked) instead when this is true.
    pub fn has_enabled_links(&self) -> bool {
        self.links.iter().any(|l| l.enabled)
    }

    /// Finds a path from `from` to `to`, bridging otherwise-disjoint regions
    /// through any enabled [`NavigationLink3D`].
    ///
    /// Unlike [`Self::find_path`], which searches each region in isolation, this
    /// flattens every region's polygons into a single A\* graph and adds a
    /// bridging edge for each link. Returns an empty `Vec` if no path exists.
    pub fn find_path_linked(&self, from: Vector3, to: Vector3) -> Vec<Vector3> {
        let mut positions: Vec<Vector3> = Vec::new();
        let mut neighbors: Vec<Vec<usize>> = Vec::new();
        for region in &self.regions {
            let base = positions.len();
            for p in &region.polygons {
                positions.push(p.centroid());
            }
            for conns in &region.connections {
                neighbors.push(conns.iter().map(|&c| c + base).collect());
            }
        }

        for link in &self.links {
            if !link.enabled {
                continue;
            }
            let a = Self::closest_polygon(&positions, link.start);
            let b = Self::closest_polygon(&positions, link.end);
            if let (Some(a), Some(b)) = (a, b) {
                if a != b {
                    neighbors[a].push(b);
                    if link.bidirectional {
                        neighbors[b].push(a);
                    }
                }
            }
        }

        let start = match Self::closest_polygon(&positions, from) {
            Some(i) => i,
            None => return Vec::new(),
        };
        let goal = match Self::closest_polygon(&positions, to) {
            Some(i) => i,
            None => return Vec::new(),
        };
        if start == goal {
            return vec![from, to];
        }

        if let Some(poly_path) = astar_find_path_3d(start, goal, &positions, &neighbors) {
            let mut path = vec![from];
            for &pi in &poly_path[1..poly_path.len().saturating_sub(1)] {
                path.push(positions[pi]);
            }
            path.push(to);
            return path;
        }
        Vec::new()
    }

    /// Finds a path through the 3D navigation mesh using A\*.
    ///
    /// Returns an empty `Vec` if no path is found.
    pub fn find_path(&self, from: Vector3, to: Vector3) -> Vec<Vector3> {
        for region in &self.regions {
            let positions: Vec<Vector3> = region.polygons.iter().map(|p| p.centroid()).collect();

            // Find closest polygon to `from` and `to`.
            let start = Self::closest_polygon(&positions, from);
            let goal = Self::closest_polygon(&positions, to);

            if let Some((start_idx, goal_idx)) = start.zip(goal) {
                if start_idx == goal_idx {
                    return vec![from, to];
                }
                if let Some(poly_path) =
                    astar_find_path_3d(start_idx, goal_idx, &positions, &region.connections)
                {
                    let mut path = vec![from];
                    for &pi in &poly_path[1..poly_path.len().saturating_sub(1)] {
                        path.push(positions[pi]);
                    }
                    path.push(to);
                    return path;
                }
            }
        }
        Vec::new()
    }

    fn closest_polygon(positions: &[Vector3], point: Vector3) -> Option<usize> {
        positions
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let da = (**a - point).length_squared();
                let db = (**b - point).length_squared();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
    }
}

impl Default for NavigationServer3D {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// NavigationLink + RVO avoidance (pat-p0s82.14)
// ---------------------------------------------------------------------------

/// Bidirectional or one-way link between two positions on a 2D nav mesh.
///
/// A link connects two points that may lie on separate, geometrically disjoint
/// [`NavMesh2D`] regions, letting pathfinding cross between them.
#[derive(Debug, Clone)]
pub struct NavigationLink2D {
    pub start: Vector2,
    pub end: Vector2,
    pub bidirectional: bool,
    pub enabled: bool,
}

impl NavigationLink2D {
    /// Creates a bidirectional, enabled link between `start` and `end`.
    pub fn new(start: Vector2, end: Vector2) -> Self {
        Self {
            start,
            end,
            bidirectional: true,
            enabled: true,
        }
    }
}

/// Bidirectional or one-way link between two positions on a 3D nav mesh.
///
/// A link connects two points that may lie on separate, geometrically disjoint
/// [`NavMesh3D`] regions, letting pathfinding cross between them.
#[derive(Debug, Clone)]
pub struct NavigationLink3D {
    pub start: Vector3,
    pub end: Vector3,
    pub bidirectional: bool,
    pub enabled: bool,
}

impl NavigationLink3D {
    /// Creates a bidirectional, enabled link between `start` and `end`.
    pub fn new(start: Vector3, end: Vector3) -> Self {
        Self {
            start,
            end,
            bidirectional: true,
            enabled: true,
        }
    }
}

/// Simple ORCA-lite: push agent velocity away from nearby agents.
///
/// `preferred` is the un-avoided velocity; returns a modified velocity.
pub fn rvo_avoidance_velocity(
    position: Vector2,
    preferred: Vector2,
    others: &[(Vector2, f32)],
    radius: f32,
    max_speed: f32,
) -> Vector2 {
    let mut v = preferred;
    for (op, orad) in others {
        let mut delta = position - *op;
        let dist = delta.length();
        let min_dist = radius + *orad;
        if dist < 1e-4 {
            delta = Vector2::new(1.0, 0.0);
        }
        if dist < min_dist * 2.0 {
            let push = (min_dist * 2.0 - dist).max(0.0) / (min_dist * 2.0);
            let n = if dist > 1e-4 {
                Vector2::new(delta.x / dist, delta.y / dist)
            } else {
                Vector2::new(1.0, 0.0)
            };
            v = v + n * push * max_speed;
        }
    }
    let sp = v.length();
    if sp > max_speed && sp > 1e-6 {
        v = Vector2::new(v.x / sp * max_speed, v.y / sp * max_speed);
    }
    v
}

/// 3D counterpart of [`rvo_avoidance_velocity`]: push agent velocity away from
/// nearby agents in full 3D space.
///
/// `preferred` is the un-avoided velocity; `others` are `(position, radius)`
/// pairs. The result is clamped to `max_speed`.
pub fn rvo_avoidance_velocity_3d(
    position: Vector3,
    preferred: Vector3,
    others: &[(Vector3, f32)],
    radius: f32,
    max_speed: f32,
) -> Vector3 {
    let mut v = preferred;
    for (op, orad) in others {
        let delta = position - *op;
        let dist = delta.length();
        let min_dist = radius + *orad;
        if dist < min_dist * 2.0 {
            let push = (min_dist * 2.0 - dist).max(0.0) / (min_dist * 2.0);
            let n = if dist > 1e-4 {
                Vector3::new(delta.x / dist, delta.y / dist, delta.z / dist)
            } else {
                // Degenerate overlap: pick an arbitrary but stable separation axis.
                Vector3::new(1.0, 0.0, 0.0)
            };
            v = Vector3::new(
                v.x + n.x * push * max_speed,
                v.y + n.y * push * max_speed,
                v.z + n.z * push * max_speed,
            );
        }
    }
    let sp = v.length();
    if sp > max_speed && sp > 1e-6 {
        v = Vector3::new(
            v.x / sp * max_speed,
            v.y / sp * max_speed,
            v.z / sp * max_speed,
        );
    }
    v
}

// ---------------------------------------------------------------------------
// ORCA / RVO2 avoidance (pat-nzgdu.116)
// ---------------------------------------------------------------------------

/// Tolerance used when testing ORCA half-plane determinants for degeneracy.
const ORCA_EPSILON: f32 = 1e-5;

/// A directed ORCA half-plane constraint.
///
/// The permitted velocities are those on the left of the line through `point`
/// along `direction` — i.e. every `v` with `direction.cross(point - v) <= 0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrcaLine {
    /// A point on the constraint boundary.
    pub point: Vector2,
    /// Unit direction of the constraint boundary.
    pub direction: Vector2,
}

/// One agent's input to the ORCA solver, mirroring Godot's avoidance
/// parameters on `NavigationAgent2D` / `NavigationObstacle2D`.
///
/// This is the *simulation* view of an agent — [`NavigationAgent2D`] builds one
/// of these per tick via [`NavigationAgent2D::orca_agent`].
#[derive(Debug, Clone, Copy)]
pub struct OrcaAgent {
    /// World position this tick.
    pub position: Vector2,
    /// Current velocity (the velocity the agent is actually moving at).
    pub velocity: Vector2,
    /// Avoidance radius.
    pub radius: f32,
    /// Speed cap applied to the returned safe velocity.
    pub max_speed: f32,
    /// How far ahead (seconds) collisions with other agents are anticipated.
    pub time_horizon_agents: f32,
    /// How far ahead (seconds) collisions with obstacles are anticipated.
    pub time_horizon_obstacles: f32,
    /// Neighbors beyond this distance are ignored.
    pub neighbor_distance: f32,
    /// At most this many nearest neighbors constrain the solve.
    pub max_neighbors: usize,
    /// Bitmask this agent is visible on (checked against others' masks).
    pub avoidance_layers: u32,
    /// Bitmask of layers this agent avoids.
    pub avoidance_mask: u32,
    /// Share of the avoidance effort this agent shoulders relative to a
    /// neighbor; a higher priority means it yields less. Godot default `1.0`.
    pub avoidance_priority: f32,
}

impl OrcaAgent {
    /// Creates an agent with Godot's `NavigationAgent2D` avoidance defaults.
    pub fn new(position: Vector2, radius: f32, max_speed: f32) -> Self {
        Self {
            position,
            velocity: Vector2::ZERO,
            radius,
            max_speed,
            time_horizon_agents: DEFAULT_TIME_HORIZON_AGENTS,
            time_horizon_obstacles: DEFAULT_TIME_HORIZON_OBSTACLES,
            neighbor_distance: DEFAULT_NEIGHBOR_DISTANCE,
            max_neighbors: DEFAULT_MAX_NEIGHBORS,
            avoidance_layers: 1,
            avoidance_mask: 1,
            avoidance_priority: 1.0,
        }
    }

    /// Whether `self` takes `other` into account, per Godot's layer/mask rule:
    /// an agent avoids another when its mask intersects the other's layers.
    pub fn avoids(&self, other: &OrcaAgent) -> bool {
        self.avoidance_mask & other.avoidance_layers != 0
    }

    /// The fraction of the mutual avoidance effort `self` shoulders against
    /// `other`. Equal priorities split the work evenly (the reciprocal `0.5`
    /// of standard ORCA); a higher priority shifts the burden onto `other`.
    fn effort_share(&self, other: &OrcaAgent) -> f32 {
        let total = self.avoidance_priority + other.avoidance_priority;
        if total <= 1e-6 {
            return 0.5;
        }
        (other.avoidance_priority / total).clamp(0.0, 1.0)
    }
}

/// Godot's `NavigationAgent2D.neighbor_distance` default.
///
/// 2D navigation works in pixels, so this is `500.0` — the `50.0` that
/// `NavigationAgent3D` uses would cull neighbors barely a few agent-widths away.
/// Matches the ClassDB registration for `NavigationAgent2D`.
pub const DEFAULT_NEIGHBOR_DISTANCE: f32 = 500.0;
/// Godot's `NavigationAgent2D.max_neighbors` default.
pub const DEFAULT_MAX_NEIGHBORS: usize = 10;
/// Godot's `NavigationAgent2D.time_horizon_agents` default.
pub const DEFAULT_TIME_HORIZON_AGENTS: f32 = 1.0;
/// Godot's `NavigationAgent2D.time_horizon_obstacles` default.
pub const DEFAULT_TIME_HORIZON_OBSTACLES: f32 = 0.0;

/// Timestep assumed by avoidance entry points that are not given one.
///
/// `dt` only matters for resolving agents that already overlap, so a nominal
/// 60 Hz tick is a safe stand-in when the caller has no better value.
pub const NOMINAL_AVOIDANCE_DT: f32 = 1.0 / 60.0;

/// Builds the ORCA half-plane that keeps `agent` clear of `other`.
///
/// This is the standard RVO2 construction: outside the combined radius the
/// constraint is taken from the velocity-obstacle cut-off circle or one of its
/// legs (whichever the current relative velocity projects onto); already
/// overlapping agents fall back to a `dt`-horizon constraint that pushes them
/// apart. `share` splits the correction between the two agents.
fn orca_line_for_agent(
    agent: &OrcaAgent,
    other: &OrcaAgent,
    time_horizon: f32,
    dt: f32,
    share: f32,
) -> OrcaLine {
    let relative_position = other.position - agent.position;
    let relative_velocity = agent.velocity - other.velocity;
    let dist_sq = relative_position.length_squared();
    let combined_radius = agent.radius + other.radius;
    let combined_radius_sq = combined_radius * combined_radius;

    let direction;
    let u;

    if dist_sq > combined_radius_sq {
        // No collision yet: constrain against the velocity obstacle truncated
        // at `time_horizon`.
        let inv_time_horizon = 1.0 / time_horizon.max(ORCA_EPSILON);
        let w = relative_velocity - relative_position * inv_time_horizon;
        let w_length_sq = w.length_squared();
        let dot1 = w.dot(relative_position);

        if dot1 < 0.0 && dot1 * dot1 > combined_radius_sq * w_length_sq {
            // Project on the cut-off circle.
            let w_length = w_length_sq.sqrt();
            if w_length <= ORCA_EPSILON {
                // Degenerate: pick a stable axis rather than dividing by zero.
                return OrcaLine {
                    point: agent.velocity,
                    direction: Vector2::new(0.0, 1.0),
                };
            }
            let unit_w = w * (1.0 / w_length);
            direction = Vector2::new(unit_w.y, -unit_w.x);
            u = unit_w * (combined_radius * inv_time_horizon - w_length);
        } else {
            // Project on the closer leg of the velocity obstacle.
            let leg = (dist_sq - combined_radius_sq).max(0.0).sqrt();
            let inv_dist_sq = 1.0 / dist_sq;
            direction = if relative_position.cross(w) > 0.0 {
                Vector2::new(
                    relative_position.x * leg - relative_position.y * combined_radius,
                    relative_position.x * combined_radius + relative_position.y * leg,
                ) * inv_dist_sq
            } else {
                Vector2::new(
                    relative_position.x * leg + relative_position.y * combined_radius,
                    -relative_position.x * combined_radius + relative_position.y * leg,
                ) * -inv_dist_sq
            };
            let dot2 = relative_velocity.dot(direction);
            u = direction * dot2 - relative_velocity;
        }
    } else {
        // Already overlapping: resolve over a single timestep.
        let inv_time_step = 1.0 / dt.max(ORCA_EPSILON);
        let w = relative_velocity - relative_position * inv_time_step;
        let w_length = w.length();
        if w_length <= ORCA_EPSILON {
            return OrcaLine {
                point: agent.velocity,
                direction: Vector2::new(0.0, 1.0),
            };
        }
        let unit_w = w * (1.0 / w_length);
        direction = Vector2::new(unit_w.y, -unit_w.x);
        u = unit_w * (combined_radius * inv_time_step - w_length);
    }

    OrcaLine {
        point: agent.velocity + u * share,
        direction,
    }
}

/// RVO2 `linearProgram1`: optimize along the boundary of `lines[line_no]`
/// subject to the earlier constraints and the `radius` speed circle.
///
/// Returns `false` when the constraints are infeasible along that line.
fn linear_program1(
    lines: &[OrcaLine],
    line_no: usize,
    radius: f32,
    opt_velocity: Vector2,
    direction_opt: bool,
    result: &mut Vector2,
) -> bool {
    let line = lines[line_no];
    let dot_product = line.point.dot(line.direction);
    let discriminant = dot_product * dot_product + radius * radius - line.point.length_squared();

    if discriminant < 0.0 {
        // The speed circle is entirely on the wrong side of this constraint.
        return false;
    }

    let sqrt_discriminant = discriminant.sqrt();
    let mut t_left = -dot_product - sqrt_discriminant;
    let mut t_right = -dot_product + sqrt_discriminant;

    for other in lines.iter().take(line_no) {
        let denominator = line.direction.cross(other.direction);
        let numerator = other.direction.cross(line.point - other.point);

        if denominator.abs() <= ORCA_EPSILON {
            // Lines are parallel: either always satisfied, or never.
            if numerator < 0.0 {
                return false;
            }
            continue;
        }

        let t = numerator / denominator;
        if denominator >= 0.0 {
            t_right = t_right.min(t);
        } else {
            t_left = t_left.max(t);
        }
        if t_left > t_right {
            return false;
        }
    }

    if direction_opt {
        // Optimizing a direction rather than a point.
        *result = if opt_velocity.dot(line.direction) > 0.0 {
            line.point + line.direction * t_right
        } else {
            line.point + line.direction * t_left
        };
    } else {
        let t = line.direction.dot(opt_velocity - line.point);
        *result = if t < t_left {
            line.point + line.direction * t_left
        } else if t > t_right {
            line.point + line.direction * t_right
        } else {
            line.point + line.direction * t
        };
    }

    true
}

/// RVO2 `linearProgram2`: find the velocity closest to `opt_velocity` that
/// satisfies every half-plane and the `radius` speed circle.
///
/// Returns the index of the first infeasible constraint, or `lines.len()` when
/// the whole program was satisfied.
fn linear_program2(
    lines: &[OrcaLine],
    radius: f32,
    opt_velocity: Vector2,
    direction_opt: bool,
    result: &mut Vector2,
) -> usize {
    if direction_opt {
        *result = opt_velocity * radius;
    } else if opt_velocity.length_squared() > radius * radius {
        *result = opt_velocity.normalized() * radius;
    } else {
        *result = opt_velocity;
    }

    for i in 0..lines.len() {
        if lines[i].direction.cross(lines[i].point - *result) > 0.0 {
            // `result` violates constraint i — re-optimize along its boundary.
            let temp_result = *result;
            if !linear_program1(lines, i, radius, opt_velocity, direction_opt, result) {
                *result = temp_result;
                return i;
            }
        }
    }

    lines.len()
}

/// RVO2 `linearProgram3`: densely-constrained fallback. When the program is
/// infeasible, relax it to the velocity that minimizes the worst constraint
/// violation, so a boxed-in agent still gets a sensible answer instead of none.
fn linear_program3(
    lines: &[OrcaLine],
    num_obst_lines: usize,
    begin_line: usize,
    radius: f32,
    result: &mut Vector2,
) {
    let mut distance = 0.0;

    for i in begin_line..lines.len() {
        if lines[i].direction.cross(lines[i].point - *result) <= distance {
            continue;
        }

        // Obstacle constraints are hard: they seed the sub-program unprojected
        // so the relaxed answer still clears static geometry.
        let mut proj_lines: Vec<OrcaLine> = lines[..num_obst_lines].to_vec();
        for j in num_obst_lines..i {
            let determinant = lines[i].direction.cross(lines[j].direction);
            let point = if determinant.abs() <= ORCA_EPSILON {
                if lines[i].direction.dot(lines[j].direction) > 0.0 {
                    // Same direction: constraint j is redundant here.
                    continue;
                }
                (lines[i].point + lines[j].point) * 0.5
            } else {
                lines[i].point
                    + lines[i].direction
                        * (lines[j].direction.cross(lines[i].point - lines[j].point) / determinant)
            };
            let direction = (lines[j].direction - lines[i].direction).normalized();
            proj_lines.push(OrcaLine { point, direction });
        }

        let temp_result = *result;
        let opt = Vector2::new(-lines[i].direction.y, lines[i].direction.x);
        if linear_program2(&proj_lines, radius, opt, true, result) < proj_lines.len() {
            // Should not normally happen; keep the best answer so far.
            *result = temp_result;
        }
        distance = lines[i].direction.cross(lines[i].point - *result);
    }
}

/// Computes a collision-free ("safe") velocity for `agent` via ORCA.
///
/// This is the real reciprocal-avoidance solve Godot's NavigationServer runs:
/// each considered neighbor contributes one half-plane, static `obstacles`
/// contribute non-reciprocal half-planes (they take none of the effort), and
/// the result is the feasible velocity closest to `preferred`, capped at
/// `agent.max_speed`.
///
/// Neighbors are filtered by [`OrcaAgent::avoids`] and
/// `neighbor_distance`, then the nearest `max_neighbors` are kept — matching
/// Godot, where those parameters bound the solve rather than merely hint it.
///
/// `dt` is the timestep, used only to resolve already-overlapping agents.
pub fn orca_safe_velocity(
    agent: &OrcaAgent,
    preferred: Vector2,
    neighbors: &[OrcaAgent],
    obstacles: &[OrcaAgent],
    dt: f32,
) -> Vector2 {
    let mut lines: Vec<OrcaLine> = Vec::new();

    // Static obstacles first: they never yield, so the agent takes the whole
    // correction (share = 1.0) and these constraints are honoured before the
    // reciprocal agent ones in the dense fallback.
    let horizon_obstacles = if agent.time_horizon_obstacles > 0.0 {
        agent.time_horizon_obstacles
    } else {
        agent.time_horizon_agents
    };
    for obstacle in obstacles {
        if !agent.avoids(obstacle) {
            continue;
        }
        let range = agent.neighbor_distance + obstacle.radius;
        if (obstacle.position - agent.position).length_squared() > range * range {
            continue;
        }
        lines.push(orca_line_for_agent(
            agent,
            obstacle,
            horizon_obstacles,
            dt,
            1.0,
        ));
    }
    let obstacle_lines = lines.len();

    // Reciprocal agent constraints, nearest `max_neighbors` first.
    let mut considered: Vec<(f32, &OrcaAgent)> = neighbors
        .iter()
        .filter(|other| agent.avoids(other))
        .filter_map(|other| {
            let range = agent.neighbor_distance + other.radius;
            let dist_sq = (other.position - agent.position).length_squared();
            (dist_sq <= range * range).then_some((dist_sq, other))
        })
        .collect();
    considered.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    considered.truncate(agent.max_neighbors);

    for (_, other) in &considered {
        let share = agent.effort_share(other);
        lines.push(orca_line_for_agent(
            agent,
            other,
            agent.time_horizon_agents,
            dt,
            share,
        ));
    }

    let mut result = Vector2::ZERO;
    let failed = linear_program2(&lines, agent.max_speed, preferred, false, &mut result);
    if failed < lines.len() {
        // Infeasible — relax from the first failing constraint toward the
        // least-bad velocity, still honouring the obstacle half-planes.
        linear_program3(&lines, obstacle_lines, failed, agent.max_speed, &mut result);
    }
    result
}

// ---------------------------------------------------------------------------
// Runtime stepping — wire agents into a running game
// ---------------------------------------------------------------------------

/// Advances a [`NavigationAgent2D`] one frame against a live [`SceneTree`] node.
///
/// This is the per-frame wiring a running game's process loop calls so an agent
/// actually moves along its path: it reads the node's `position`, integrates the
/// agent toward the next path waypoint (deflecting around `others` when
/// avoidance is enabled), writes the new `position` back onto the node, and
/// returns it. Previously nothing referenced the agent stepping logic from the
/// node/runtime layer, so a `NavigationAgent2D` stayed frozen at its defaults
/// even with a target set.
///
/// `others` is the list of nearby `(position, radius)` neighbors used for
/// avoidance. Call [`NavigationAgent2D::set_target_position`] first so the agent
/// has a path to follow.
pub fn step_navigation_agent_2d(
    scene: &mut SceneTree,
    node_id: NodeId,
    agent: &mut NavigationAgent2D,
    dt: f32,
    others: &[(Vector2, f32)],
) -> Vector2 {
    let current = crate::node2d::get_position(scene, node_id);
    let next = agent.step(current, dt, others);
    crate::node2d::set_position(scene, node_id, next);
    next
}

/// Advances a [`NavigationAgent3D`] one frame against a live [`SceneTree`] node.
///
/// The 3D counterpart of [`step_navigation_agent_2d`]: reads the node's 3D
/// `position`, integrates the agent toward its next waypoint (with avoidance
/// over `others`), writes it back, and returns the new position.
pub fn step_navigation_agent_3d(
    scene: &mut SceneTree,
    node_id: NodeId,
    agent: &mut NavigationAgent3D,
    dt: f32,
    others: &[(Vector3, f32)],
) -> Vector3 {
    let current = crate::node3d::get_position(scene, node_id);
    let next = agent.step(current, dt, others);
    crate::node3d::set_position(scene, node_id, next);
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-5;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    // -- NavPolygon / point-in-polygon --------------------------------------

    #[test]
    fn point_in_triangle() {
        let tri = NavPolygon::new(vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
            Vector2::new(5.0, 10.0),
        ]);
        assert!(tri.contains_point(Vector2::new(5.0, 3.0)));
    }

    #[test]
    fn point_outside_triangle() {
        let tri = NavPolygon::new(vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
            Vector2::new(5.0, 10.0),
        ]);
        assert!(!tri.contains_point(Vector2::new(20.0, 20.0)));
    }

    #[test]
    fn point_in_square() {
        let sq = NavPolygon::new(vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
            Vector2::new(10.0, 10.0),
            Vector2::new(0.0, 10.0),
        ]);
        assert!(sq.contains_point(Vector2::new(5.0, 5.0)));
        assert!(!sq.contains_point(Vector2::new(15.0, 5.0)));
    }

    #[test]
    fn point_in_polygon_degenerate_line() {
        let line = NavPolygon::new(vec![Vector2::new(0.0, 0.0), Vector2::new(10.0, 0.0)]);
        assert!(!line.contains_point(Vector2::new(5.0, 0.0)));
    }

    #[test]
    fn polygon_centroid() {
        let sq = NavPolygon::new(vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
            Vector2::new(10.0, 10.0),
            Vector2::new(0.0, 10.0),
        ]);
        let c = sq.centroid();
        assert!(approx_eq(c.x, 5.0));
        assert!(approx_eq(c.y, 5.0));
    }

    // -- A* pathfinding -----------------------------------------------------

    #[test]
    fn astar_simple_grid() {
        // 0 -- 1 -- 2
        // |         |
        // 3 -- 4 -- 5
        let positions = vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 0.0),
            Vector2::new(2.0, 0.0),
            Vector2::new(0.0, 1.0),
            Vector2::new(1.0, 1.0),
            Vector2::new(2.0, 1.0),
        ];
        let neighbors = vec![
            vec![1, 3], // 0
            vec![0, 2], // 1
            vec![1, 5], // 2
            vec![0, 4], // 3
            vec![3, 5], // 4
            vec![2, 4], // 5
        ];
        let path = astar_find_path(0, 5, &positions, &neighbors).unwrap();
        assert_eq!(path[0], 0);
        assert_eq!(*path.last().unwrap(), 5);
        assert!(path.len() <= 4); // Optimal is 3 hops
    }

    #[test]
    fn astar_start_equals_goal() {
        let positions = vec![Vector2::new(0.0, 0.0)];
        let neighbors = vec![vec![]];
        let path = astar_find_path(0, 0, &positions, &neighbors).unwrap();
        assert_eq!(path, vec![0]);
    }

    #[test]
    fn astar_no_path() {
        let positions = vec![Vector2::new(0.0, 0.0), Vector2::new(10.0, 10.0)];
        let neighbors = vec![vec![], vec![]];
        assert!(astar_find_path(0, 1, &positions, &neighbors).is_none());
    }

    #[test]
    fn astar_direct_neighbor() {
        let positions = vec![Vector2::new(0.0, 0.0), Vector2::new(1.0, 0.0)];
        let neighbors = vec![vec![1], vec![0]];
        let path = astar_find_path(0, 1, &positions, &neighbors).unwrap();
        assert_eq!(path, vec![0, 1]);
    }

    #[test]
    fn astar_prefers_shorter_path() {
        // Triangle: 0 -- 1, 0 -- 2, 1 -- 2
        // 0->2 direct should be preferred over 0->1->2 if closer
        let positions = vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 10.0),
            Vector2::new(1.0, 0.0),
        ];
        let neighbors = vec![vec![1, 2], vec![0, 2], vec![0, 1]];
        let path = astar_find_path(0, 2, &positions, &neighbors).unwrap();
        assert_eq!(path, vec![0, 2]);
    }

    // -- NavMesh2D ----------------------------------------------------------

    fn make_two_square_mesh() -> NavMesh2D {
        // Two adjacent squares sharing edge at x=10
        let left = NavPolygon::new(vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
            Vector2::new(10.0, 10.0),
            Vector2::new(0.0, 10.0),
        ]);
        let right = NavPolygon::new(vec![
            Vector2::new(10.0, 0.0),
            Vector2::new(20.0, 0.0),
            Vector2::new(20.0, 10.0),
            Vector2::new(10.0, 10.0),
        ]);
        NavMesh2D::new(vec![left, right], 0.01)
    }

    #[test]
    fn navmesh_connectivity() {
        let mesh = make_two_square_mesh();
        assert!(mesh.connections[0].contains(&1));
        assert!(mesh.connections[1].contains(&0));
    }

    #[test]
    fn navmesh_find_polygon() {
        let mesh = make_two_square_mesh();
        assert_eq!(mesh.find_polygon(Vector2::new(5.0, 5.0)), Some(0));
        assert_eq!(mesh.find_polygon(Vector2::new(15.0, 5.0)), Some(1));
        assert_eq!(mesh.find_polygon(Vector2::new(25.0, 5.0)), None);
    }

    // -- NavigationServer2D -------------------------------------------------

    #[test]
    fn server_find_path_same_polygon() {
        let mut server = NavigationServer2D::new();
        let mesh = make_two_square_mesh();
        server.add_region(mesh);

        let path = server.find_path(Vector2::new(2.0, 5.0), Vector2::new(8.0, 5.0));
        assert_eq!(path.len(), 2);
        assert!(approx_eq(path[0].x, 2.0));
        assert!(approx_eq(path[1].x, 8.0));
    }

    #[test]
    fn server_find_path_across_polygons() {
        let mut server = NavigationServer2D::new();
        let mesh = make_two_square_mesh();
        server.add_region(mesh);

        let path = server.find_path(Vector2::new(5.0, 5.0), Vector2::new(15.0, 5.0));
        assert!(!path.is_empty());
        assert!(approx_eq(path[0].x, 5.0));
        assert!(approx_eq(path.last().unwrap().x, 15.0));
    }

    #[test]
    fn server_no_path_outside_mesh() {
        let mut server = NavigationServer2D::new();
        let mesh = make_two_square_mesh();
        server.add_region(mesh);

        let path = server.find_path(Vector2::new(5.0, 5.0), Vector2::new(50.0, 50.0));
        assert!(path.is_empty());
    }

    #[test]
    fn server_path_detours_around_obstacle() {
        // pat-u3l8c.62: Godot never nulls a *static* path just because an
        // obstacle sits on it — it routes around. The old behavior returned an
        // empty Vec here; the new behavior must return a non-empty detour whose
        // every segment clears the obstacle.
        let mut server = NavigationServer2D::new();
        let poly = NavPolygon::new(vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(20.0, 0.0),
            Vector2::new(20.0, 20.0),
            Vector2::new(0.0, 20.0),
        ]);
        server.add_region(NavMesh2D::new(vec![poly], 0.01));

        // Obstacle straddling the straight-line path from left to right.
        let obs = NavigationObstacle2D::new(Vector2::new(10.0, 10.0), 5.0);
        server.add_obstacle(obs);

        let from = Vector2::new(1.0, 10.0);
        let to = Vector2::new(19.0, 10.0);
        let path = server.find_path(from, to);

        assert!(
            path.len() >= 3,
            "expected a detour with an intermediate waypoint, got {path:?}"
        );
        assert!(
            (path[0] - from).length() < EPSILON,
            "path must start at `from`"
        );
        assert!(
            (path[path.len() - 1] - to).length() < EPSILON,
            "path must end at `to`"
        );
        // No segment of the detour may pass through the obstacle.
        for seg in path.windows(2) {
            assert!(
                !obs.blocks_segment(seg[0], seg[1]),
                "detour segment {:?}->{:?} still crosses the obstacle",
                seg[0],
                seg[1]
            );
        }
    }

    // -- Funnel string-pulling (pat-u3l8c.62) -------------------------------

    /// Three 10x10 squares in a row (`[0,10]`,`[10,20]`,`[20,30]`), each sharing
    /// an edge with the next.
    fn strip_of_three() -> NavMesh2D {
        NavMesh2D::new(
            vec![
                NavPolygon::new(vec![
                    Vector2::new(0.0, 0.0),
                    Vector2::new(10.0, 0.0),
                    Vector2::new(10.0, 10.0),
                    Vector2::new(0.0, 10.0),
                ]),
                NavPolygon::new(vec![
                    Vector2::new(10.0, 0.0),
                    Vector2::new(20.0, 0.0),
                    Vector2::new(20.0, 10.0),
                    Vector2::new(10.0, 10.0),
                ]),
                NavPolygon::new(vec![
                    Vector2::new(20.0, 0.0),
                    Vector2::new(30.0, 0.0),
                    Vector2::new(30.0, 10.0),
                    Vector2::new(20.0, 10.0),
                ]),
            ],
            0.01,
        )
    }

    #[test]
    fn funnel_collapses_straight_corridor() {
        // A straight shot down a straight corridor must string-pull to exactly
        // two points — no zig-zag through polygon centroids (the old bug).
        let mut server = NavigationServer2D::new();
        server.add_region(strip_of_three());

        let from = Vector2::new(5.0, 5.0);
        let to = Vector2::new(25.0, 5.0);
        let path = server.find_path(from, to);

        assert_eq!(path.len(), 2, "straight corridor should collapse: {path:?}");
        assert!((path[0] - from).length() < EPSILON);
        assert!((path[1] - to).length() < EPSILON);
    }

    /// An L-shaped mesh: bottom square, one above it, one to the right of that,
    /// meeting at the reflex corner `(10,10)`.
    fn l_shaped_mesh() -> NavMesh2D {
        NavMesh2D::new(
            vec![
                NavPolygon::new(vec![
                    Vector2::new(0.0, 0.0),
                    Vector2::new(10.0, 0.0),
                    Vector2::new(10.0, 10.0),
                    Vector2::new(0.0, 10.0),
                ]),
                NavPolygon::new(vec![
                    Vector2::new(0.0, 10.0),
                    Vector2::new(10.0, 10.0),
                    Vector2::new(10.0, 20.0),
                    Vector2::new(0.0, 20.0),
                ]),
                NavPolygon::new(vec![
                    Vector2::new(10.0, 10.0),
                    Vector2::new(20.0, 10.0),
                    Vector2::new(20.0, 20.0),
                    Vector2::new(10.0, 20.0),
                ]),
            ],
            0.01,
        )
    }

    #[test]
    fn funnel_bends_around_corner() {
        // Around an L-bend the taut path cannot be a straight line — it must
        // round the inner corner at (10,10), yielding an intermediate waypoint.
        let mut server = NavigationServer2D::new();
        server.add_region(l_shaped_mesh());

        let from = Vector2::new(5.0, 5.0);
        let to = Vector2::new(15.0, 15.0);
        let path = server.find_path(from, to);

        assert!(
            path.len() >= 3,
            "L-bend must produce a corner waypoint: {path:?}"
        );
        assert!((path[0] - from).length() < EPSILON);
        assert!((path[path.len() - 1] - to).length() < EPSILON);
        // Some interior waypoint hugs the reflex corner (10,10).
        let corner = Vector2::new(10.0, 10.0);
        assert!(
            path[1..path.len() - 1]
                .iter()
                .any(|w| (*w - corner).length() < 1e-3),
            "expected a waypoint at the reflex corner, got {path:?}"
        );
    }

    // -- Cross-region stitching (pat-u3l8c.62) ------------------------------

    #[test]
    fn find_path_stitches_adjacent_regions() {
        // Two separately-registered regions sharing the edge at x=10 must be
        // stitched via edge_connection_margin so a path crosses between them.
        let mut server = NavigationServer2D::new();
        server.add_region(NavMesh2D::new(
            vec![NavPolygon::new(vec![
                Vector2::new(0.0, 0.0),
                Vector2::new(10.0, 0.0),
                Vector2::new(10.0, 10.0),
                Vector2::new(0.0, 10.0),
            ])],
            0.01,
        ));
        server.add_region(NavMesh2D::new(
            vec![NavPolygon::new(vec![
                Vector2::new(10.0, 0.0),
                Vector2::new(20.0, 0.0),
                Vector2::new(20.0, 10.0),
                Vector2::new(10.0, 10.0),
            ])],
            0.01,
        ));

        let from = Vector2::new(5.0, 5.0);
        let to = Vector2::new(15.0, 5.0);
        let path = server.find_path(from, to);
        assert!(
            !path.is_empty(),
            "adjacent regions should stitch into a crossable path"
        );
        assert!((path[0] - from).length() < EPSILON);
        assert!((path[path.len() - 1] - to).length() < EPSILON);
    }

    #[test]
    fn find_path_no_stitch_across_gap() {
        // Regions with a 10-unit gap are far beyond the edge-connection margin,
        // so without an explicit link they remain mutually unreachable.
        let server = disjoint_two_region_server_2d();
        let path = server.find_path(Vector2::new(5.0, 5.0), Vector2::new(25.0, 5.0));
        assert!(
            path.is_empty(),
            "regions separated by a gap must not auto-stitch: {path:?}"
        );
    }

    #[test]
    fn set_edge_connection_margin_clamps_negative() {
        let mut server = NavigationServer2D::new();
        server.set_edge_connection_margin(-5.0);
        assert_eq!(server.edge_connection_margin, 0.0);
        server.set_edge_connection_margin(3.5);
        assert_eq!(server.edge_connection_margin, 3.5);
    }

    // -- NavigationObstacle2D -----------------------------------------------

    #[test]
    fn obstacle_blocks_through_center() {
        let obs = NavigationObstacle2D::new(Vector2::new(5.0, 5.0), 2.0);
        assert!(obs.blocks_segment(Vector2::new(0.0, 5.0), Vector2::new(10.0, 5.0)));
    }

    #[test]
    fn obstacle_does_not_block_far_segment() {
        let obs = NavigationObstacle2D::new(Vector2::new(5.0, 5.0), 1.0);
        assert!(!obs.blocks_segment(Vector2::new(0.0, 0.0), Vector2::new(10.0, 0.0)));
    }

    // -- NavigationAgent2D --------------------------------------------------

    #[test]
    fn agent_basic_navigation() {
        let mut server = NavigationServer2D::new();
        let mesh = make_two_square_mesh();
        server.add_region(mesh);

        let mut agent = NavigationAgent2D::new();
        agent.set_target_position(Vector2::new(15.0, 5.0), Vector2::new(5.0, 5.0), &server);

        assert!(!agent.is_navigation_finished());
        let next = agent.get_next_path_position(Vector2::new(5.0, 5.0));
        // Should point toward the path
        assert!(next.x > 5.0);
    }

    #[test]
    fn agent_finished_when_no_path() {
        let agent = NavigationAgent2D::new();
        assert!(agent.is_navigation_finished());
    }

    #[test]
    fn agent_avoidance_toggle() {
        let mut agent = NavigationAgent2D::new();
        assert!(!agent.avoidance_enabled);
        agent.set_avoidance_enabled(true);
        assert!(agent.avoidance_enabled);
    }

    // -- NavigationAgent3D --------------------------------------------------

    #[test]
    fn agent3d_basic_navigation() {
        let p0 = NavPolygon3D::new(vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(5.0, 0.0, 0.0),
            Vector3::new(5.0, 0.0, 5.0),
        ]);
        let p1 = NavPolygon3D::new(vec![
            Vector3::new(5.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 5.0),
        ]);
        let mesh = NavMesh3D::new(vec![p0, p1], vec![vec![1], vec![0]]);
        let mut server = NavigationServer3D::new();
        server.add_region(mesh);

        let mut agent = NavigationAgent3D::new();
        agent.set_target_position(
            Vector3::new(9.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 1.0),
            &server,
        );
        assert!(!agent.is_navigation_finished());
        let next = agent.get_next_path_position(Vector3::new(1.0, 0.0, 1.0));
        assert!(next.x > 1.0);
    }

    // -- 3D stubs -----------------------------------------------------------

    #[test]
    fn navmesh3d_basic_path() {
        let p0 = NavPolygon3D::new(vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(5.0, 0.0, 0.0),
            Vector3::new(5.0, 0.0, 5.0),
        ]);
        let p1 = NavPolygon3D::new(vec![
            Vector3::new(5.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 5.0),
        ]);
        let mesh = NavMesh3D::new(vec![p0, p1], vec![vec![1], vec![0]]);

        let mut server = NavigationServer3D::new();
        server.add_region(mesh);

        let path = server.find_path(Vector3::new(1.0, 0.0, 1.0), Vector3::new(9.0, 0.0, 1.0));
        assert!(!path.is_empty());
        assert!(approx_eq(path[0].x, 1.0));
        assert!(approx_eq(path.last().unwrap().x, 9.0));
    }

    #[test]
    fn navmesh3d_no_path_disconnected() {
        let p0 = NavPolygon3D::new(vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(5.0, 0.0, 0.0),
            Vector3::new(5.0, 0.0, 5.0),
        ]);
        let p1 = NavPolygon3D::new(vec![
            Vector3::new(50.0, 0.0, 0.0),
            Vector3::new(55.0, 0.0, 0.0),
            Vector3::new(55.0, 0.0, 5.0),
        ]);
        // No connections
        let mesh = NavMesh3D::new(vec![p0, p1], vec![vec![], vec![]]);

        let mut server = NavigationServer3D::new();
        server.add_region(mesh);

        let path = server.find_path(Vector3::new(1.0, 0.0, 1.0), Vector3::new(52.0, 0.0, 1.0));
        assert!(path.is_empty());
    }

    #[test]
    fn navpolygon3d_centroid() {
        let p = NavPolygon3D::new(vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 10.0),
            Vector3::new(0.0, 0.0, 10.0),
        ]);
        let c = p.centroid();
        assert!(approx_eq(c.x, 5.0));
        assert!(approx_eq(c.z, 5.0));
    }

    #[test]
    fn astar_3d_basic() {
        let positions = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(2.0, 0.0, 0.0),
        ];
        let neighbors = vec![vec![1], vec![0, 2], vec![1]];
        let path = astar_find_path_3d(0, 2, &positions, &neighbors).unwrap();
        assert_eq!(path, vec![0, 1, 2]);
    }

    // -- Additional edge cases ----------------------------------------------

    #[test]
    fn empty_navmesh_no_path() {
        let server = NavigationServer2D::new();
        let path = server.find_path(Vector2::new(0.0, 0.0), Vector2::new(10.0, 10.0));
        assert!(path.is_empty());
    }

    #[test]
    fn obstacle_point_segment() {
        // Degenerate segment (point)
        let obs = NavigationObstacle2D::new(Vector2::new(5.0, 5.0), 2.0);
        assert!(obs.blocks_segment(Vector2::new(5.0, 5.0), Vector2::new(5.0, 5.0)));
        assert!(!obs.blocks_segment(Vector2::new(50.0, 50.0), Vector2::new(50.0, 50.0)));
    }

    #[test]
    fn rvo_separates_agents() {
        let pref = Vector2::new(1.0, 0.0);
        let v = rvo_avoidance_velocity(
            Vector2::new(0.0, 0.0),
            pref,
            &[(Vector2::new(0.5, 0.0), 0.5)],
            0.5,
            2.0,
        );
        // Should push away from neighbor (negative or reduced x / nonzero y-ish)
        assert!(v.x < pref.x + 0.5);
        assert!(v.length() <= 2.0 + 1e-3);
    }

    // -- NavigationLink2D/3D disjoint-region bridging -----------------------

    /// Two square regions separated by a gap on the x axis (x in [0,10] and
    /// [20,30]). They share no geometry, so pathfinding between them requires a
    /// link.
    fn disjoint_two_region_server_2d() -> NavigationServer2D {
        let mut server = NavigationServer2D::new();
        server.add_region(NavMesh2D::new(
            vec![NavPolygon::new(vec![
                Vector2::new(0.0, 0.0),
                Vector2::new(10.0, 0.0),
                Vector2::new(10.0, 10.0),
                Vector2::new(0.0, 10.0),
            ])],
            0.01,
        ));
        server.add_region(NavMesh2D::new(
            vec![NavPolygon::new(vec![
                Vector2::new(20.0, 0.0),
                Vector2::new(30.0, 0.0),
                Vector2::new(30.0, 10.0),
                Vector2::new(20.0, 10.0),
            ])],
            0.01,
        ));
        server
    }

    fn disjoint_two_region_server_3d() -> NavigationServer3D {
        let mut server = NavigationServer3D::new();
        server.add_region(NavMesh3D::new(
            vec![NavPolygon3D::new(vec![
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(10.0, 0.0, 0.0),
                Vector3::new(10.0, 0.0, 10.0),
                Vector3::new(0.0, 0.0, 10.0),
            ])],
            vec![vec![]],
        ));
        server.add_region(NavMesh3D::new(
            vec![NavPolygon3D::new(vec![
                Vector3::new(20.0, 0.0, 0.0),
                Vector3::new(30.0, 0.0, 0.0),
                Vector3::new(30.0, 0.0, 10.0),
                Vector3::new(20.0, 0.0, 10.0),
            ])],
            vec![vec![]],
        ));
        server
    }

    #[test]
    fn navigation_link_bridges_disjoint_regions_2d() {
        let mut server = disjoint_two_region_server_2d();
        let from = Vector2::new(5.0, 5.0);
        let to = Vector2::new(25.0, 5.0);

        // Without a link the two regions are mutually unreachable.
        assert!(
            server.find_path_linked(from, to).is_empty(),
            "disjoint regions must not be reachable without a link"
        );

        // A link bridging the regions makes `to` reachable from `from`.
        server.add_link(NavigationLink2D::new(
            Vector2::new(8.0, 5.0),
            Vector2::new(22.0, 5.0),
        ));
        let path = server.find_path_linked(from, to);
        assert!(!path.is_empty(), "link should connect disjoint regions");
        assert!((path[0] - from).length() < EPSILON);
        assert!((path[path.len() - 1] - to).length() < EPSILON);
    }

    #[test]
    fn navigation_link_disabled_does_not_bridge_2d() {
        let mut server = disjoint_two_region_server_2d();
        let mut link = NavigationLink2D::new(Vector2::new(8.0, 5.0), Vector2::new(22.0, 5.0));
        link.enabled = false;
        server.add_link(link);
        assert!(
            server
                .find_path_linked(Vector2::new(5.0, 5.0), Vector2::new(25.0, 5.0))
                .is_empty(),
            "a disabled link must not bridge regions"
        );
    }

    #[test]
    fn navigation_link_one_way_2d() {
        let mut server = disjoint_two_region_server_2d();
        let a = Vector2::new(5.0, 5.0); // region A
        let b = Vector2::new(25.0, 5.0); // region B
        let mut link = NavigationLink2D::new(Vector2::new(8.0, 5.0), Vector2::new(22.0, 5.0));
        link.bidirectional = false; // A -> B only
        server.add_link(link);
        assert!(
            !server.find_path_linked(a, b).is_empty(),
            "forward direction A->B should work"
        );
        assert!(
            server.find_path_linked(b, a).is_empty(),
            "reverse direction B->A should be blocked on a one-way link"
        );
    }

    #[test]
    fn navigation_link_bridges_disjoint_regions_3d() {
        let mut server = disjoint_two_region_server_3d();
        let from = Vector3::new(5.0, 0.0, 5.0);
        let to = Vector3::new(25.0, 0.0, 5.0);

        assert!(
            server.find_path_linked(from, to).is_empty(),
            "disjoint 3D regions must not be reachable without a link"
        );

        server.add_link(NavigationLink3D::new(
            Vector3::new(8.0, 0.0, 5.0),
            Vector3::new(22.0, 0.0, 5.0),
        ));
        let path = server.find_path_linked(from, to);
        assert!(!path.is_empty(), "3D link should connect disjoint regions");
        assert!((path[0] - from).length() < EPSILON);
        assert!((path[path.len() - 1] - to).length() < EPSILON);
    }

    // -- RVO avoidance wired into agent stepping (pat-p0s82.14) --------------

    /// Builds a 2D agent moving straight along +x with the given avoidance flag.
    fn straight_x_agent_2d(avoidance: bool) -> NavigationAgent2D {
        let mut a = NavigationAgent2D::new();
        a.avoidance_enabled = avoidance;
        a.max_speed = 10.0;
        a.radius = 5.0;
        a.path = vec![Vector2::new(0.0, 0.0), Vector2::new(200.0, 0.0)];
        a.path_index = 1;
        a.target_position = Vector2::new(200.0, 0.0);
        a
    }

    #[test]
    fn agent2d_step_moves_toward_waypoint() {
        let mut a = straight_x_agent_2d(false);
        let next = a.step(Vector2::ZERO, 1.0, &[]);
        // max_speed 10, dt 1, straight +x, no neighbors.
        assert!(approx_eq(next.x, 10.0), "x = {}", next.x);
        assert!(approx_eq(next.y, 0.0), "y = {}", next.y);
    }

    #[test]
    fn agent2d_avoidance_deflects_around_neighbor() {
        let mut a = straight_x_agent_2d(true);
        // Neighbor slightly off the +y side of the straight path.
        let others = [(Vector2::new(8.0, 1.0), 5.0)];
        let next = a.step(Vector2::ZERO, 1.0, &others);
        // Deflected to the -y side (away from the neighbor) and slowed in x.
        assert!(next.y < -0.1, "expected -y deflection, got y = {}", next.y);
        assert!(
            next.x > 0.0 && next.x < 10.0,
            "expected slowed forward x, got {}",
            next.x
        );
    }

    #[test]
    fn agent2d_avoidance_disabled_ignores_neighbor() {
        // Same neighbor as the deflection test, but avoidance is OFF: the agent
        // must walk straight through — proving avoidance is gated by the flag
        // and actually wired into stepping.
        let mut a = straight_x_agent_2d(false);
        let others = [(Vector2::new(8.0, 1.0), 5.0)];
        let next = a.step(Vector2::ZERO, 1.0, &others);
        assert!(approx_eq(next.x, 10.0), "x = {}", next.x);
        assert!(approx_eq(next.y, 0.0), "y = {}", next.y);
    }

    #[test]
    fn agent2d_avoidance_displaces_laterally_over_time() {
        // A neighbor sits on the straight-line path at (30, 5).
        let neighbor = [(Vector2::new(30.0, 5.0), 5.0)];

        // With avoidance the agent must at some point be pushed off the axis.
        let mut on = straight_x_agent_2d(true);
        let mut p_on = Vector2::ZERO;
        let mut min_y_on = 0.0_f32;
        for _ in 0..30 {
            p_on = on.step(p_on, 0.1, &neighbor);
            min_y_on = min_y_on.min(p_on.y);
        }

        // Without avoidance the agent stays exactly on the axis the whole time.
        let mut off = straight_x_agent_2d(false);
        let mut p_off = Vector2::ZERO;
        let mut min_y_off = 0.0_f32;
        for _ in 0..30 {
            p_off = off.step(p_off, 0.1, &neighbor);
            min_y_off = min_y_off.min(p_off.y);
        }

        assert!(
            min_y_on < -0.1,
            "avoidance should deflect off-axis, min_y = {min_y_on}"
        );
        assert!(
            approx_eq(min_y_off, 0.0),
            "no-avoidance path must stay on axis, min_y = {min_y_off}"
        );
        assert!(
            p_on.x > 1.0 && p_off.x > 1.0,
            "agents must still progress along x"
        );
    }

    #[test]
    fn rvo_3d_separates_agents() {
        let pref = Vector3::new(1.0, 0.0, 0.0);
        let v = rvo_avoidance_velocity_3d(
            Vector3::ZERO,
            pref,
            &[(Vector3::new(0.5, 0.0, 0.0), 0.5)],
            0.5,
            2.0,
        );
        assert!(v.x < pref.x, "should be pushed back in x, got {}", v.x);
        assert!(
            v.length() <= 2.0 + 1e-3,
            "speed must be clamped, got {}",
            v.length()
        );
    }

    /// Builds a 3D agent moving straight along +x with the given avoidance flag.
    fn straight_x_agent_3d(avoidance: bool) -> NavigationAgent3D {
        let mut a = NavigationAgent3D::new();
        a.avoidance_enabled = avoidance;
        a.max_speed = 10.0;
        a.radius = 5.0;
        a.path = vec![Vector3::new(0.0, 0.0, 0.0), Vector3::new(200.0, 0.0, 0.0)];
        a.path_index = 1;
        a.target_position = Vector3::new(200.0, 0.0, 0.0);
        a
    }

    #[test]
    fn agent3d_avoidance_deflects_around_neighbor() {
        let mut a = straight_x_agent_3d(true);
        let others = [(Vector3::new(8.0, 0.0, 1.0), 5.0)];
        let next = a.step(Vector3::ZERO, 1.0, &others);
        // Deflected on -z (away from neighbor), y untouched, slowed in x.
        assert!(next.z < -0.1, "expected -z deflection, got z = {}", next.z);
        assert!(approx_eq(next.y, 0.0), "y = {}", next.y);
        assert!(
            next.x > 0.0 && next.x < 10.0,
            "expected slowed forward x, got {}",
            next.x
        );
    }

    #[test]
    fn agent3d_avoidance_disabled_ignores_neighbor() {
        let mut a = straight_x_agent_3d(false);
        let others = [(Vector3::new(8.0, 0.0, 1.0), 5.0)];
        let next = a.step(Vector3::ZERO, 1.0, &others);
        assert!(approx_eq(next.x, 10.0), "x = {}", next.x);
        assert!(approx_eq(next.z, 0.0), "z = {}", next.z);
    }

    // -- Closest-point query family (pat-u3l8c.63) --------------------------

    fn square_region_server() -> NavigationServer2D {
        let mut server = NavigationServer2D::new();
        server.add_region(NavMesh2D::new(
            vec![NavPolygon::new(vec![
                Vector2::new(0.0, 0.0),
                Vector2::new(10.0, 0.0),
                Vector2::new(10.0, 10.0),
                Vector2::new(0.0, 10.0),
            ])],
            0.1,
        ));
        server
    }

    #[test]
    fn map_closest_point_inside_returns_point() {
        let server = square_region_server();
        let inside = Vector2::new(5.0, 5.0);
        assert_eq!(server.map_get_closest_point(inside), inside);
    }

    #[test]
    fn map_closest_point_outside_snaps_to_boundary() {
        let server = square_region_server();
        // 5 units right of the square's right edge (x=10) at y=5.
        let cp = server.map_get_closest_point(Vector2::new(15.0, 5.0));
        assert!((cp.x - 10.0).abs() < 1e-4, "x -> right edge, got {}", cp.x);
        assert!((cp.y - 5.0).abs() < 1e-4, "y stays 5, got {}", cp.y);
    }

    #[test]
    fn map_closest_point_owner_returns_region() {
        let server = square_region_server();
        assert_eq!(
            server.map_get_closest_point_owner(Vector2::new(15.0, 5.0)),
            Some(0)
        );
        assert_eq!(
            server.map_get_closest_point_owner(Vector2::new(5.0, 5.0)),
            Some(0)
        );
        assert_eq!(
            NavigationServer2D::new().map_get_closest_point_owner(Vector2::ZERO),
            None
        );
    }

    #[test]
    fn map_closest_point_to_segment_snaps_to_mesh() {
        let server = square_region_server();
        // A vertical segment at x=15, right of the square.
        let cp = server
            .map_get_closest_point_to_segment(Vector2::new(15.0, 0.0), Vector2::new(15.0, 10.0));
        assert!((cp.x - 10.0).abs() < 1e-4, "x -> right edge, got {}", cp.x);
        assert!(
            cp.y >= -1e-4 && cp.y <= 10.0 + 1e-4,
            "y in [0,10], got {}",
            cp.y
        );
    }

    // -- NavigationAgent2D milestone signals (pat-u3l8c.64) -----------------

    #[test]
    fn agent_signals_fire_at_navigation_milestones() {
        let mut server = NavigationServer2D::new();
        server.add_region(NavMesh2D::new(
            vec![NavPolygon::new(vec![
                Vector2::new(0.0, 0.0),
                Vector2::new(100.0, 0.0),
                Vector2::new(100.0, 100.0),
                Vector2::new(0.0, 100.0),
            ])],
            0.1,
        ));
        let mut agent = NavigationAgent2D::new();
        let start = Vector2::new(10.0, 10.0);
        agent.set_target_position(Vector2::new(90.0, 10.0), start, &server);
        assert!(
            agent
                .pending_signals()
                .contains(&NavAgentSignal::PathChanged),
            "set_target_position should emit path_changed"
        );

        // Walk the agent along its path to the target.
        let mut current = start;
        for _ in 0..500 {
            let next = agent.get_next_path_position(current);
            let dir = next - current;
            let len = dir.length();
            if len < 1e-6 {
                break;
            }
            let step = len.min(5.0);
            current = Vector2::new(
                current.x + dir.x / len * step,
                current.y + dir.y / len * step,
            );
        }

        let signals = agent.drain_signals();
        assert!(
            signals.contains(&NavAgentSignal::WaypointReached),
            "should reach a waypoint: {signals:?}"
        );
        assert!(
            signals.contains(&NavAgentSignal::TargetReached),
            "should reach the target: {signals:?}"
        );
        assert!(
            signals.contains(&NavAgentSignal::NavigationFinished),
            "navigation should finish: {signals:?}"
        );
        // Finished fires exactly once; draining again yields nothing.
        assert!(agent.drain_signals().is_empty());
    }

    #[test]
    fn set_velocity_passthrough_and_signals_without_avoidance() {
        let mut agent = NavigationAgent2D::new(); // avoidance off by default
        let preferred = Vector2::new(50.0, 25.0);
        let safe = agent.set_velocity(Vector2::ZERO, preferred, &[]);
        assert_eq!(safe, preferred, "no avoidance -> velocity passes through");
        assert_eq!(
            agent.drain_signals(),
            vec![NavAgentSignal::VelocityComputed]
        );
    }

    #[test]
    fn set_velocity_fires_signal_with_avoidance_enabled() {
        let mut agent = NavigationAgent2D::new();
        agent.set_avoidance_enabled(true);
        let others = [(Vector2::new(5.0, 0.0), 5.0)];
        let _ = agent.set_velocity(Vector2::ZERO, Vector2::new(50.0, 0.0), &others);
        assert!(agent
            .pending_signals()
            .contains(&NavAgentSignal::VelocityComputed));
    }

    // -- ORCA / RVO2 solver (pat-nzgdu.116) --------------------------------

    /// A solver agent at `position` moving at `velocity`, radius 10, speed 100.
    ///
    /// `neighbor_distance` is pinned here rather than inherited from
    /// [`DEFAULT_NEIGHBOR_DISTANCE`] on purpose. These fixtures exercise the ORCA
    /// *math*, and must not fail — as an inscrutable "no deflection" error —
    /// merely because a default moved: the fixtures sit ~60 units apart, which is
    /// within a hair of the cull range at some plausible defaults. The default
    /// itself is pinned separately by
    /// [`orca_defaults_match_godot_navigation_agent_2d`].
    fn orca_at(position: Vector2, velocity: Vector2) -> OrcaAgent {
        let mut a = OrcaAgent::new(position, 10.0, 100.0);
        a.velocity = velocity;
        a.neighbor_distance = 500.0;
        a
    }

    #[test]
    fn orca_defaults_match_godot_navigation_agent_2d() {
        // 2D navigation works in PIXELS. NavigationAgent3D's 50.0 (meters) would
        // cull neighbors barely a few agent-widths away, producing avoidance that
        // compiles, unit-tests green, and does almost nothing on screen. These
        // must agree with the NavigationAgent2D ClassDB registration.
        assert_eq!(
            DEFAULT_NEIGHBOR_DISTANCE, 500.0,
            "2D neighbor_distance is 500.0 (pixels); 50.0 is the 3D value"
        );
        assert_eq!(DEFAULT_MAX_NEIGHBORS, 10);
        assert_eq!(DEFAULT_TIME_HORIZON_AGENTS, 1.0);
        assert_eq!(DEFAULT_TIME_HORIZON_OBSTACLES, 0.0);

        let agent = NavigationAgent2D::new();
        assert_eq!(agent.neighbor_distance, DEFAULT_NEIGHBOR_DISTANCE);
        assert_eq!(agent.max_neighbors, DEFAULT_MAX_NEIGHBORS);
        assert_eq!(agent.radius, 10.0);
        assert_eq!(agent.max_speed, 100.0);
        assert_eq!(agent.avoidance_priority, 1.0);
        assert_eq!(agent.avoidance_layers, 1);
        assert_eq!(agent.avoidance_mask, 1);
    }

    #[test]
    fn orca_with_no_neighbors_returns_the_preferred_velocity() {
        let agent = orca_at(Vector2::ZERO, Vector2::new(50.0, 0.0));
        let preferred = Vector2::new(50.0, 0.0);
        let safe = orca_safe_velocity(&agent, preferred, &[], &[], 0.1);
        assert!(
            (safe - preferred).length() < 1e-3,
            "an unobstructed agent must keep its preferred velocity, got {safe:?}"
        );
    }

    #[test]
    fn orca_caps_the_result_at_max_speed() {
        let agent = orca_at(Vector2::ZERO, Vector2::ZERO);
        // Preferred well above the 100.0 cap.
        let safe = orca_safe_velocity(&agent, Vector2::new(500.0, 0.0), &[], &[], 0.1);
        assert!(
            safe.length() <= 100.0 + 1e-3,
            "safe velocity must respect max_speed, got {}",
            safe.length()
        );
    }

    #[test]
    fn orca_deflects_two_converging_agents() {
        // Head-on, slightly offset so the configuration is not perfectly
        // symmetric (a pure head-on tie has no preferred side).
        let preferred = Vector2::new(50.0, 0.0);
        let agent = orca_at(Vector2::new(-30.0, -0.5), preferred);
        let other = orca_at(Vector2::new(30.0, 0.5), Vector2::new(-50.0, 0.0));

        let safe = orca_safe_velocity(&agent, preferred, &[other], &[], 0.1);

        assert!(
            (safe - preferred).length() > 1e-3,
            "a converging neighbor must change the velocity, got {safe:?}"
        );
        assert!(
            safe.length() <= 100.0 + 1e-3,
            "deflection must still respect max_speed"
        );
    }

    #[test]
    fn orca_ignores_neighbors_beyond_neighbor_distance() {
        let mut agent = orca_at(Vector2::ZERO, Vector2::new(50.0, 0.0));
        agent.neighbor_distance = 20.0;
        // Far away, and heading at us — but outside neighbor_distance.
        let far = orca_at(Vector2::new(400.0, 0.0), Vector2::new(-50.0, 0.0));

        let preferred = Vector2::new(50.0, 0.0);
        let safe = orca_safe_velocity(&agent, preferred, &[far], &[], 0.1);
        assert!(
            (safe - preferred).length() < 1e-3,
            "a neighbor beyond neighbor_distance must not constrain the solve"
        );
    }

    #[test]
    fn orca_layer_mask_filters_neighbors() {
        let preferred = Vector2::new(50.0, 0.0);
        let mut agent = orca_at(Vector2::new(-30.0, -0.5), preferred);
        let mut other = orca_at(Vector2::new(30.0, 0.5), Vector2::new(-50.0, 0.0));

        // The neighbor lives only on layer 2; the agent only avoids layer 1.
        other.avoidance_layers = 0b10;
        agent.avoidance_mask = 0b01;
        assert!(!agent.avoids(&other));
        let safe = orca_safe_velocity(&agent, preferred, &[other], &[], 0.1);
        assert!(
            (safe - preferred).length() < 1e-3,
            "a neighbor outside the avoidance mask must be ignored"
        );

        // Widening the mask to include layer 2 brings it back into the solve.
        agent.avoidance_mask = 0b11;
        assert!(agent.avoids(&other));
        let safe = orca_safe_velocity(&agent, preferred, &[other], &[], 0.1);
        assert!(
            (safe - preferred).length() > 1e-3,
            "a neighbor inside the avoidance mask must constrain the solve"
        );
    }

    #[test]
    fn orca_priority_shifts_effort_onto_the_lower_priority_agent() {
        let preferred = Vector2::new(50.0, 0.0);
        let other = orca_at(Vector2::new(30.0, 0.5), Vector2::new(-50.0, 0.0));

        let mut low = orca_at(Vector2::new(-30.0, -0.5), preferred);
        low.avoidance_priority = 0.1;
        let mut high = orca_at(Vector2::new(-30.0, -0.5), preferred);
        high.avoidance_priority = 10.0;

        let low_dev =
            (orca_safe_velocity(&low, preferred, &[other], &[], 0.1) - preferred).length();
        let high_dev =
            (orca_safe_velocity(&high, preferred, &[other], &[], 0.1) - preferred).length();

        assert!(
            low_dev > high_dev,
            "the lower-priority agent must yield more (low {low_dev} vs high {high_dev})"
        );
    }

    #[test]
    fn orca_effort_share_is_even_at_equal_priority() {
        let a = orca_at(Vector2::ZERO, Vector2::ZERO);
        let b = orca_at(Vector2::new(10.0, 0.0), Vector2::ZERO);
        assert!((a.effort_share(&b) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn orca_separates_overlapping_agents() {
        // Two agents already inside each other's radius, both trying to hold
        // still: the solver must still produce a separating velocity.
        let agent = orca_at(Vector2::ZERO, Vector2::ZERO);
        let other = orca_at(Vector2::new(4.0, 0.0), Vector2::ZERO);

        let safe = orca_safe_velocity(&agent, Vector2::ZERO, &[other], &[], 0.1);
        assert!(
            safe.x < -1e-3,
            "an overlapped agent must be pushed away from its neighbor, got {safe:?}"
        );
    }

    #[test]
    fn orca_static_obstacle_does_not_take_a_share_of_the_effort() {
        // An obstacle never yields, so the agent shoulders the whole correction
        // and is deflected at least as much as it would be by a mobile peer.
        let preferred = Vector2::new(50.0, 0.0);
        let agent = orca_at(Vector2::new(-30.0, -0.5), preferred);
        let blocker = orca_at(Vector2::new(30.0, 0.5), Vector2::ZERO);

        let as_obstacle = orca_safe_velocity(&agent, preferred, &[], &[blocker], 0.1);
        let as_agent = orca_safe_velocity(&agent, preferred, &[blocker], &[], 0.1);

        let obstacle_dev = (as_obstacle - preferred).length();
        let agent_dev = (as_agent - preferred).length();
        assert!(
            obstacle_dev > agent_dev - 1e-3,
            "obstacle avoidance must not be split reciprocally \
             (obstacle {obstacle_dev} vs agent {agent_dev})"
        );
    }

    #[test]
    fn set_velocity_records_desired_and_safe_velocities() {
        let mut agent = NavigationAgent2D::new();
        agent.avoidance_enabled = true;
        let preferred = Vector2::new(50.0, 0.0);
        // A neighbor dead ahead forces a non-trivial safe velocity.
        let others = [(Vector2::new(15.0, 0.5), 10.0)];

        let returned = agent.set_velocity(Vector2::ZERO, preferred, &others);

        assert_eq!(agent.velocity, preferred, "`velocity` is the DESIRED value");
        assert_eq!(
            agent.safe_velocity, returned,
            "`safe_velocity` must hold what the solve returned"
        );
        assert!(
            agent
                .pending_signals()
                .contains(&NavAgentSignal::VelocityComputed),
            "set_velocity must record a velocity_computed milestone"
        );
    }

    #[test]
    fn set_velocity_forced_bypasses_avoidance_and_emits_nothing() {
        let mut agent = NavigationAgent2D::new();
        agent.avoidance_enabled = true;
        let teleport_velocity = Vector2::new(-25.0, 7.0);

        agent.set_velocity_forced(teleport_velocity);

        assert_eq!(agent.velocity, teleport_velocity);
        assert_eq!(
            agent.safe_velocity, teleport_velocity,
            "forced velocity is adopted verbatim, not filtered through ORCA"
        );
        assert!(
            agent.pending_signals().is_empty(),
            "set_velocity_forced computes nothing, so it must not signal"
        );
    }

    #[test]
    fn disabled_avoidance_passes_the_preferred_velocity_through() {
        let agent = NavigationAgent2D::new();
        assert!(!agent.avoidance_enabled, "avoidance is off by default");
        let preferred = Vector2::new(50.0, 0.0);
        let blocker = OrcaAgent::new(Vector2::new(15.0, 0.0), 10.0, 100.0);

        let safe = agent.compute_safe_velocity_against(
            Vector2::ZERO,
            preferred,
            &[blocker],
            &[],
            1.0 / 60.0,
        );
        assert_eq!(
            safe, preferred,
            "a disabled agent must not be deflected even by an imminent collision"
        );
    }

    #[test]
    fn neighbor_velocity_is_honoured_by_the_reciprocal_solve() {
        // The same neighbor, once fleeing and once closing head-on. A neighbor
        // moving away is far less constraining than one bearing down, which is
        // exactly what the (position, radius)-only entry point cannot express.
        let mut agent = NavigationAgent2D::new();
        agent.avoidance_enabled = true;
        // Pinned, not inherited — see `orca_at`: this fixture is ~60 units wide,
        // so a changed default would surface here as a bogus math failure.
        agent.neighbor_distance = 500.0;
        let preferred = Vector2::new(50.0, 0.0);
        let position = Vector2::new(-30.0, -0.5);

        let mut closing = OrcaAgent::new(Vector2::new(30.0, 0.5), 10.0, 100.0);
        closing.velocity = Vector2::new(-50.0, 0.0);
        let mut fleeing = OrcaAgent::new(Vector2::new(30.0, 0.5), 10.0, 100.0);
        fleeing.velocity = Vector2::new(50.0, 0.0);

        let closing_dev =
            (agent.compute_safe_velocity_against(position, preferred, &[closing], &[], 0.1)
                - preferred)
                .length();
        let fleeing_dev =
            (agent.compute_safe_velocity_against(position, preferred, &[fleeing], &[], 0.1)
                - preferred)
                .length();

        assert!(
            closing_dev > fleeing_dev,
            "a closing neighbor must constrain more than a fleeing one \
             (closing {closing_dev} vs fleeing {fleeing_dev})"
        );
    }

    /// 🔑 THE CANONICAL NAVMESH READER'S TWO NON-OBVIOUS BEHAVIOURS.
    ///
    /// These are what a re-implementation drifts on, and this parser had been
    /// copied into `gdscene::main_loop` and `gdeditor::navigation_overlay`
    /// independently, so pinning them here is the point of having one home.
    #[test]
    fn navmesh_polygons_local_handles_the_two_edge_cases() {
        use crate::node::Node;
        use gdvariant::Variant;

        // 1. Vertices with NO `polygons` entry are one polygon over all of them
        //    — what a hand-authored single-face mesh looks like.
        let mut implicit = Node::new("R", "NavigationRegion2D");
        implicit.set_property(
            "__navmesh_vertices__",
            Variant::PackedVector2Array(vec![
                Vector2::new(0.0, 0.0),
                Vector2::new(4.0, 0.0),
                Vector2::new(4.0, 4.0),
            ]),
        );
        let polys = navmesh_polygons_local(&implicit);
        assert_eq!(polys.len(), 1, "no polygons entry means one implicit face");
        assert_eq!(polys[0].len(), 3);

        // 2. Out-of-range indices are DROPPED, not panicked on — a malformed
        //    .tscn should cost that polygon, not the frame. Index 9 does not
        //    exist, which leaves two points, which is not a polygon.
        let mut malformed = Node::new("R", "NavigationRegion2D");
        malformed.set_property(
            "__navmesh_vertices__",
            Variant::PackedVector2Array(vec![
                Vector2::new(0.0, 0.0),
                Vector2::new(4.0, 0.0),
                Vector2::new(4.0, 4.0),
            ]),
        );
        malformed.set_property(
            "__navmesh_polygons__",
            Variant::Array(
                vec![
                    Variant::PackedInt32Array(vec![0, 1, 9]),
                    Variant::PackedInt32Array(vec![0, 1, 2]),
                ]
                .into(),
            ),
        );
        let polys = navmesh_polygons_local(&malformed);
        assert_eq!(
            polys.len(),
            1,
            "the polygon naming a missing vertex is dropped; the valid one stays"
        );
        assert_eq!(
            polys[0],
            vec![
                Vector2::new(0.0, 0.0),
                Vector2::new(4.0, 0.0),
                Vector2::new(4.0, 4.0)
            ]
        );

        // An unbaked region yields nothing at all, rather than an empty face.
        assert!(navmesh_polygons_local(&Node::new("R", "NavigationRegion2D")).is_empty());
    }
}

//! Navigation and pathfinding for the Patina Engine.
//!
//! Provides 2D navigation meshes, A\* pathfinding, navigation agents,
//! and obstacles, mirroring Godot's navigation system. Includes basic
//! 3D stubs for future expansion.

use std::collections::{BinaryHeap, HashMap};

use gdcore::math::{Vector2, Vector3};

use crate::node::NodeId;
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
}

impl NavigationServer2D {
    /// Creates a new, empty navigation server.
    pub fn new() -> Self {
        Self {
            regions: Vec::new(),
            obstacles: Vec::new(),
            links: Vec::new(),
        }
    }

    /// Adds a navigation mesh region and returns its index.
    pub fn add_region(&mut self, mesh: NavMesh2D) -> usize {
        self.regions.push(mesh);
        self.regions.len() - 1
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

    /// Finds a path from `from` to `to` using A\* on the navigation mesh,
    /// then applies string-pulling for a smoother result.
    ///
    /// Returns an empty `Vec` if no path is found.
    pub fn find_path(&self, from: Vector2, to: Vector2) -> Vec<Vector2> {
        // For now, search the first region that contains both points.
        for region in &self.regions {
            let start_poly = match region.find_polygon(from) {
                Some(i) => i,
                None => continue,
            };
            let goal_poly = match region.find_polygon(to) {
                Some(i) => i,
                None => continue,
            };

            if start_poly == goal_poly {
                // Check obstacles
                if !self.segment_blocked(from, to) {
                    return vec![from, to];
                }
                return Vec::new();
            }

            // Build positions (centroids) for A*
            let positions: Vec<Vector2> = region.polygons.iter().map(|p| p.centroid()).collect();

            if let Some(poly_path) =
                astar_find_path(start_poly, goal_poly, &positions, &region.connections)
            {
                // String-pulling: build path through polygon centroids
                let mut path = vec![from];
                for &pi in &poly_path[1..poly_path.len().saturating_sub(1)] {
                    path.push(positions[pi]);
                }
                path.push(to);

                // Filter out segments blocked by obstacles
                if self.path_blocked(&path) {
                    return Vec::new();
                }

                return path;
            }
        }

        Vec::new()
    }

    fn segment_blocked(&self, a: Vector2, b: Vector2) -> bool {
        self.obstacles.iter().any(|o| o.blocks_segment(a, b))
    }

    fn path_blocked(&self, path: &[Vector2]) -> bool {
        for i in 0..path.len().saturating_sub(1) {
            if self.segment_blocked(path[i], path[i + 1]) {
                return true;
            }
        }
        false
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
            if !self.segment_blocked(from, to) {
                return vec![from, to];
            }
            return Vec::new();
        }

        if let Some(poly_path) = astar_find_path(start, goal, &positions, &neighbors) {
            let mut path = vec![from];
            for &pi in &poly_path[1..poly_path.len().saturating_sub(1)] {
                path.push(positions[pi]);
            }
            path.push(to);
            if self.path_blocked(&path) {
                return Vec::new();
            }
            return path;
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

/// Per-node navigation agent helpers, mirroring Godot's `NavigationAgent2D`.
#[derive(Debug, Clone)]
pub struct NavigationAgent2D {
    /// The target position the agent is navigating toward.
    pub target_position: Vector2,
    /// The computed path.
    pub path: Vec<Vector2>,
    /// Index of the next waypoint in the path.
    pub path_index: usize,
    /// Distance at which a waypoint is considered reached.
    pub path_desired_distance: f32,
    /// Whether avoidance is enabled for this agent.
    pub avoidance_enabled: bool,
    /// Avoidance radius of this agent (Godot `NavigationAgent2D.radius`, default 10.0).
    pub radius: f32,
    /// Maximum speed used when integrating avoidance (Godot default 100.0).
    pub max_speed: f32,
    /// Whether `navigation_finished` has already been emitted for the current
    /// target — so the runtime fires it exactly once per navigation. Reset by
    /// [`set_target_position`](Self::set_target_position). (pat-nzgdu.10)
    pub finished_emitted: bool,
}

impl NavigationAgent2D {
    /// Creates a new navigation agent.
    pub fn new() -> Self {
        Self {
            target_position: Vector2::ZERO,
            path: Vec::new(),
            path_index: 0,
            path_desired_distance: 4.0,
            avoidance_enabled: false,
            radius: 10.0,
            max_speed: 100.0,
            finished_emitted: false,
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
        self.path_index = if self.path.len() > 1 { 1 } else { 0 };
        // A fresh target restarts the navigation lifecycle.
        self.finished_emitted = false;
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
        rvo_avoidance_velocity(current, preferred, others, self.radius, self.max_speed)
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
    /// Distance at which a waypoint is considered reached.
    pub path_desired_distance: f32,
    /// Whether avoidance is enabled for this agent.
    pub avoidance_enabled: bool,
    /// Avoidance radius of this agent (Godot `NavigationAgent3D.radius`, default 0.5).
    pub radius: f32,
    /// Maximum speed used when integrating avoidance (Godot default 10.0).
    pub max_speed: f32,
    /// Whether `navigation_finished` has already been emitted for the current
    /// target (see [`NavigationAgent2D::finished_emitted`]). (pat-nzgdu.10)
    pub finished_emitted: bool,
}

impl NavigationAgent3D {
    /// Creates a new 3D navigation agent.
    pub fn new() -> Self {
        Self {
            target_position: Vector3::ZERO,
            path: Vec::new(),
            path_index: 0,
            path_desired_distance: 1.0,
            avoidance_enabled: false,
            radius: 0.5,
            max_speed: 10.0,
            finished_emitted: false,
        }
    }

    /// Sets the target and computes a path via the 3D navigation server.
    pub fn set_target_position(
        &mut self,
        target: Vector3,
        current: Vector3,
        server: &NavigationServer3D,
    ) {
        self.target_position = target;
        self.path = server.find_path(current, target);
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
        v = Vector3::new(v.x / sp * max_speed, v.y / sp * max_speed, v.z / sp * max_speed);
    }
    v
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
    fn server_path_blocked_by_obstacle() {
        let mut server = NavigationServer2D::new();
        // Single large polygon
        let poly = NavPolygon::new(vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(20.0, 0.0),
            Vector2::new(20.0, 20.0),
            Vector2::new(0.0, 20.0),
        ]);
        let mesh = NavMesh2D::new(vec![poly], 0.01);
        server.add_region(mesh);

        // Place obstacle right in the middle of the direct path
        server.add_obstacle(NavigationObstacle2D::new(Vector2::new(10.0, 10.0), 5.0));

        let path = server.find_path(Vector2::new(1.0, 10.0), Vector2::new(19.0, 10.0));
        assert!(path.is_empty());
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
        assert!(next.x > 0.0 && next.x < 10.0, "expected slowed forward x, got {}", next.x);
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

        assert!(min_y_on < -0.1, "avoidance should deflect off-axis, min_y = {min_y_on}");
        assert!(approx_eq(min_y_off, 0.0), "no-avoidance path must stay on axis, min_y = {min_y_off}");
        assert!(p_on.x > 1.0 && p_off.x > 1.0, "agents must still progress along x");
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
        assert!(v.length() <= 2.0 + 1e-3, "speed must be clamped, got {}", v.length());
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
        assert!(next.x > 0.0 && next.x < 10.0, "expected slowed forward x, got {}", next.x);
    }

    #[test]
    fn agent3d_avoidance_disabled_ignores_neighbor() {
        let mut a = straight_x_agent_3d(false);
        let others = [(Vector3::new(8.0, 0.0, 1.0), 5.0)];
        let next = a.step(Vector3::ZERO, 1.0, &others);
        assert!(approx_eq(next.x, 10.0), "x = {}", next.x);
        assert!(approx_eq(next.z, 0.0), "z = {}", next.z);
    }
}

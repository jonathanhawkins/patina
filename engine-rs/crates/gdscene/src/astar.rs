//! Godot `AStar2D` / `AStar3D` pathfinding graph classes (pat-u3l8c.61).
//!
//! A user-facing A* graph: points are added by integer id with a position and a
//! per-point `weight_scale`, connected by (optionally bidirectional) segments,
//! and queried for shortest paths. Semantics mirror Godot 4.x:
//!
//! * The step cost from a point `u` to a neighbour `v` is
//!   `u.position.distance_to(v.position) * v.weight_scale` (the default
//!   `_compute_cost` times the destination's weight).
//! * The heuristic used to order the frontier is the straight-line distance to
//!   the goal (`_estimate_cost`), which is admissible for `weight_scale >= 1`.
//! * Disabled points are skipped as intermediate nodes and rejected as
//!   endpoints, and negative ids / negative weights are rejected on insert.
//!
//! `AStar2D` and `AStar3D` are the `Vector2` / `Vector3` instantiations of the
//! shared [`AStar`] core.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

use gdcore::math::{Vector2, Vector2i, Vector3};

/// Position behaviour shared by [`AStar2D`] (`Vector2`) and [`AStar3D`]
/// (`Vector3`).
pub trait AStarVector: Copy {
    /// Euclidean distance between two positions.
    fn dist(self, other: Self) -> f32;
}

impl AStarVector for Vector2 {
    fn dist(self, other: Self) -> f32 {
        self.distance_to(other)
    }
}

impl AStarVector for Vector3 {
    fn dist(self, other: Self) -> f32 {
        self.distance_to(other)
    }
}

#[derive(Debug, Clone)]
struct Point<V> {
    position: V,
    weight_scale: f32,
    disabled: bool,
    /// Directed out-edges: the ids this point can travel *to*.
    neighbors: HashSet<i64>,
}

/// A Godot-compatible A* graph over arbitrary integer point ids.
#[derive(Debug, Clone)]
pub struct AStar<V: AStarVector> {
    points: HashMap<i64, Point<V>>,
}

impl<V: AStarVector> Default for AStar<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: AStarVector> AStar<V> {
    pub fn new() -> Self {
        Self {
            points: HashMap::new(),
        }
    }

    /// Adds a point, or updates the position/weight of an existing one.
    ///
    /// Returns `false` (a no-op) for a negative id or a negative `weight_scale`,
    /// matching Godot's `ERR_FAIL_COND` guards.
    pub fn add_point(&mut self, id: i64, position: V, weight_scale: f32) -> bool {
        if id < 0 || weight_scale < 0.0 {
            return false;
        }
        match self.points.get_mut(&id) {
            Some(p) => {
                p.position = position;
                p.weight_scale = weight_scale;
            }
            None => {
                self.points.insert(
                    id,
                    Point {
                        position,
                        weight_scale,
                        disabled: false,
                        neighbors: HashSet::new(),
                    },
                );
            }
        }
        true
    }

    pub fn has_point(&self, id: i64) -> bool {
        self.points.contains_key(&id)
    }

    /// Removes a point and every connection referencing it.
    pub fn remove_point(&mut self, id: i64) -> bool {
        if self.points.remove(&id).is_none() {
            return false;
        }
        for p in self.points.values_mut() {
            p.neighbors.remove(&id);
        }
        true
    }

    pub fn get_point_count(&self) -> usize {
        self.points.len()
    }

    /// All point ids, ascending (deterministic for tests).
    pub fn get_point_ids(&self) -> Vec<i64> {
        let mut v: Vec<i64> = self.points.keys().copied().collect();
        v.sort_unstable();
        v
    }

    pub fn get_point_position(&self, id: i64) -> Option<V> {
        self.points.get(&id).map(|p| p.position)
    }

    pub fn set_point_position(&mut self, id: i64, position: V) -> bool {
        match self.points.get_mut(&id) {
            Some(p) => {
                p.position = position;
                true
            }
            None => false,
        }
    }

    pub fn get_point_weight_scale(&self, id: i64) -> Option<f32> {
        self.points.get(&id).map(|p| p.weight_scale)
    }

    pub fn set_point_weight_scale(&mut self, id: i64, weight_scale: f32) -> bool {
        if weight_scale < 0.0 {
            return false;
        }
        match self.points.get_mut(&id) {
            Some(p) => {
                p.weight_scale = weight_scale;
                true
            }
            None => false,
        }
    }

    pub fn set_point_disabled(&mut self, id: i64, disabled: bool) -> bool {
        match self.points.get_mut(&id) {
            Some(p) => {
                p.disabled = disabled;
                true
            }
            None => false,
        }
    }

    pub fn is_point_disabled(&self, id: i64) -> bool {
        self.points.get(&id).is_some_and(|p| p.disabled)
    }

    /// Connects `id1 -> id2` (and `id2 -> id1` when `bidirectional`).
    ///
    /// Returns `false` if the ids are equal or either point is missing.
    pub fn connect_points(&mut self, id1: i64, id2: i64, bidirectional: bool) -> bool {
        if id1 == id2 || !self.points.contains_key(&id1) || !self.points.contains_key(&id2) {
            return false;
        }
        self.points.get_mut(&id1).unwrap().neighbors.insert(id2);
        if bidirectional {
            self.points.get_mut(&id2).unwrap().neighbors.insert(id1);
        }
        true
    }

    /// Removes the `id1 -> id2` edge (and `id2 -> id1` when `bidirectional`).
    pub fn disconnect_points(&mut self, id1: i64, id2: i64, bidirectional: bool) -> bool {
        let mut changed = false;
        if let Some(p) = self.points.get_mut(&id1) {
            changed |= p.neighbors.remove(&id2);
        }
        if bidirectional {
            if let Some(p) = self.points.get_mut(&id2) {
                changed |= p.neighbors.remove(&id1);
            }
        }
        changed
    }

    /// Whether a connection exists. With `bidirectional`, an edge in *either*
    /// direction counts.
    pub fn are_points_connected(&self, id1: i64, id2: i64, bidirectional: bool) -> bool {
        if self
            .points
            .get(&id1)
            .is_some_and(|p| p.neighbors.contains(&id2))
        {
            return true;
        }
        if bidirectional {
            return self
                .points
                .get(&id2)
                .is_some_and(|p| p.neighbors.contains(&id1));
        }
        false
    }

    /// Out-edge ids of `id`, ascending.
    pub fn get_point_connections(&self, id: i64) -> Vec<i64> {
        let mut v: Vec<i64> = self
            .points
            .get(&id)
            .map(|p| p.neighbors.iter().copied().collect())
            .unwrap_or_default();
        v.sort_unstable();
        v
    }

    /// Shortest path of ids from `from` to `to` (both inclusive).
    ///
    /// Returns an empty vector when either endpoint is missing or disabled, or
    /// when no route exists.
    pub fn get_id_path(&self, from: i64, to: i64) -> Vec<i64> {
        let (Some(start), Some(goal)) = (self.points.get(&from), self.points.get(&to)) else {
            return Vec::new();
        };
        if start.disabled || goal.disabled {
            return Vec::new();
        }
        if from == to {
            return vec![from];
        }
        let goal_pos = goal.position;

        let mut g_score: HashMap<i64, f32> = HashMap::new();
        let mut came_from: HashMap<i64, i64> = HashMap::new();
        let mut closed: HashSet<i64> = HashSet::new();
        let mut open: BinaryHeap<HeapEntry> = BinaryHeap::new();

        g_score.insert(from, 0.0);
        open.push(HeapEntry {
            f: start.position.dist(goal_pos),
            id: from,
        });

        while let Some(HeapEntry { id: current, .. }) = open.pop() {
            if current == to {
                return reconstruct(&came_from, from, to);
            }
            if !closed.insert(current) {
                // Already finalized via a cheaper entry.
                continue;
            }
            let cur = &self.points[&current];
            let cur_g = g_score[&current];
            for &nb in &cur.neighbors {
                let np = match self.points.get(&nb) {
                    Some(p) if !p.disabled => p,
                    _ => continue,
                };
                if closed.contains(&nb) {
                    continue;
                }
                let tentative = cur_g + cur.position.dist(np.position) * np.weight_scale;
                if tentative < *g_score.get(&nb).unwrap_or(&f32::INFINITY) {
                    g_score.insert(nb, tentative);
                    came_from.insert(nb, current);
                    open.push(HeapEntry {
                        f: tentative + np.position.dist(goal_pos),
                        id: nb,
                    });
                }
            }
        }
        Vec::new()
    }

    /// Positions along [`get_id_path`].
    pub fn get_point_path(&self, from: i64, to: i64) -> Vec<V> {
        self.get_id_path(from, to)
            .into_iter()
            .filter_map(|id| self.get_point_position(id))
            .collect()
    }

    /// Id of the point nearest `position`, or `-1` when empty. Disabled points
    /// are skipped unless `include_disabled`. Ties break to the lowest id.
    pub fn get_closest_point(&self, position: V, include_disabled: bool) -> i64 {
        let mut best = -1i64;
        let mut best_d = f32::INFINITY;
        for (&id, p) in &self.points {
            if p.disabled && !include_disabled {
                continue;
            }
            let d = position.dist(p.position);
            if d < best_d || (d == best_d && (best < 0 || id < best)) {
                best_d = d;
                best = id;
            }
        }
        best
    }

    pub fn clear(&mut self) {
        self.points.clear();
    }
}

/// Rebuilds the id path from the `came_from` chain.
fn reconstruct(came_from: &HashMap<i64, i64>, from: i64, to: i64) -> Vec<i64> {
    let mut path = vec![to];
    let mut c = to;
    while c != from {
        match came_from.get(&c) {
            Some(&prev) => {
                path.push(prev);
                c = prev;
            }
            None => break,
        }
    }
    path.reverse();
    path
}

/// Frontier entry ordered so the [`BinaryHeap`] (a max-heap) yields the entry
/// with the smallest f-score first.
struct HeapEntry {
    f: f32,
    id: i64,
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.f == other.f && self.id == other.id
    }
}
impl Eq for HeapEntry {}
impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse f so smaller f-scores sort as "greater" and pop first; break
        // ties on lower id (also reversed) for deterministic ordering.
        other
            .f
            .partial_cmp(&self.f)
            .unwrap_or(Ordering::Equal)
            .then_with(|| other.id.cmp(&self.id))
    }
}

/// Godot `AStar2D`: an A* graph over `Vector2` positions.
pub type AStar2D = AStar<Vector2>;
/// Godot `AStar3D`: an A* graph over `Vector3` positions.
pub type AStar3D = AStar<Vector3>;

// ---------------------------------------------------------------------------
// AStarGrid2D
// ---------------------------------------------------------------------------

/// When (and whether) diagonal moves are permitted, matching Godot's
/// `AStarGrid2D.DiagonalMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagonalMode {
    /// Diagonals are always allowed (target cell permitting). Godot value `0`.
    Always,
    /// Diagonals are never allowed. Godot value `1`.
    Never,
    /// A diagonal is allowed if at least one of the two shared orthogonal
    /// neighbours is walkable. Godot value `2`.
    AtLeastOneWalkable,
    /// A diagonal is allowed only if both shared orthogonal neighbours are
    /// walkable. Godot value `3`.
    OnlyIfNoObstacles,
}

/// Distance heuristic, matching Godot's `AStarGrid2D.Heuristic`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heuristic {
    /// `sqrt(dx² + dy²)`. Godot value `0`.
    Euclidean,
    /// `dx + dy`. Godot value `1`.
    Manhattan,
    /// `max(dx,dy) + (√2 - 1)·min(dx,dy)`. Godot value `2`.
    Octile,
    /// `max(dx, dy)`. Godot value `3`.
    Chebyshev,
}

impl Heuristic {
    /// Heuristic value for a `(dx, dy)` displacement.
    fn cost(self, dx: f32, dy: f32) -> f32 {
        let dx = dx.abs();
        let dy = dy.abs();
        match self {
            Heuristic::Euclidean => (dx * dx + dy * dy).sqrt(),
            Heuristic::Manhattan => dx + dy,
            Heuristic::Octile => {
                let (lo, hi) = if dx < dy { (dx, dy) } else { (dy, dx) };
                hi + (std::f32::consts::SQRT_2 - 1.0) * lo
            }
            Heuristic::Chebyshev => dx.max(dy),
        }
    }
}

#[derive(Debug, Clone)]
struct GridCell {
    solid: bool,
    weight_scale: f32,
}

impl Default for GridCell {
    fn default() -> Self {
        Self {
            solid: false,
            weight_scale: 1.0,
        }
    }
}

/// Godot `AStarGrid2D`: A* over a rectangular grid of cells addressed by
/// `Vector2i`, with configurable diagonal rules, heuristics, solid (blocked)
/// cells and per-cell weights.
#[derive(Debug, Clone)]
pub struct AStarGrid2D {
    region_pos: Vector2i,
    region_size: Vector2i,
    cell_size: Vector2,
    offset: Vector2,
    diagonal_mode: DiagonalMode,
    compute_heuristic: Heuristic,
    estimate_heuristic: Heuristic,
    jumping_enabled: bool,
    /// Per-cell overrides; cells absent here use [`GridCell::default`].
    cells: HashMap<Vector2i, GridCell>,
}

impl Default for AStarGrid2D {
    fn default() -> Self {
        Self::new()
    }
}

impl AStarGrid2D {
    pub fn new() -> Self {
        Self {
            region_pos: Vector2i::ZERO,
            region_size: Vector2i::ZERO,
            cell_size: Vector2::new(1.0, 1.0),
            offset: Vector2::new(0.0, 0.0),
            diagonal_mode: DiagonalMode::Always,
            compute_heuristic: Heuristic::Euclidean,
            estimate_heuristic: Heuristic::Euclidean,
            jumping_enabled: false,
            cells: HashMap::new(),
        }
    }

    /// Sets the grid region as an origin and size (in cells).
    pub fn set_region(&mut self, position: Vector2i, size: Vector2i) {
        self.region_pos = position;
        self.region_size = size;
        // Drop overrides that fell outside the new region.
        self.cells
            .retain(|id, _| Self::in_region(position, size, *id));
    }

    pub fn get_region(&self) -> (Vector2i, Vector2i) {
        (self.region_pos, self.region_size)
    }

    pub fn set_cell_size(&mut self, cell_size: Vector2) {
        self.cell_size = cell_size;
    }

    pub fn set_offset(&mut self, offset: Vector2) {
        self.offset = offset;
    }

    pub fn set_diagonal_mode(&mut self, mode: DiagonalMode) {
        self.diagonal_mode = mode;
    }

    pub fn set_default_compute_heuristic(&mut self, h: Heuristic) {
        self.compute_heuristic = h;
    }

    pub fn set_default_estimate_heuristic(&mut self, h: Heuristic) {
        self.estimate_heuristic = h;
    }

    pub fn set_jumping_enabled(&mut self, enabled: bool) {
        self.jumping_enabled = enabled;
    }

    /// Present for Godot API compatibility. Queries read the region directly, so
    /// no rebuild is required; this is a no-op.
    pub fn update(&mut self) {}

    fn in_region(pos: Vector2i, size: Vector2i, id: Vector2i) -> bool {
        id.x >= pos.x && id.x < pos.x + size.x && id.y >= pos.y && id.y < pos.y + size.y
    }

    pub fn is_in_bounds(&self, x: i32, y: i32) -> bool {
        Self::in_region(self.region_pos, self.region_size, Vector2i::new(x, y))
    }

    pub fn is_in_boundsv(&self, id: Vector2i) -> bool {
        Self::in_region(self.region_pos, self.region_size, id)
    }

    pub fn set_point_solid(&mut self, id: Vector2i, solid: bool) -> bool {
        if !self.is_in_boundsv(id) {
            return false;
        }
        self.cells.entry(id).or_default().solid = solid;
        true
    }

    pub fn is_point_solid(&self, id: Vector2i) -> bool {
        self.cells.get(&id).is_some_and(|c| c.solid)
    }

    pub fn set_point_weight_scale(&mut self, id: Vector2i, weight_scale: f32) -> bool {
        if !self.is_in_boundsv(id) || weight_scale < 0.0 {
            return false;
        }
        self.cells.entry(id).or_default().weight_scale = weight_scale;
        true
    }

    pub fn get_point_weight_scale(&self, id: Vector2i) -> f32 {
        self.cells.get(&id).map_or(1.0, |c| c.weight_scale)
    }

    /// World position of a cell: `offset + id * cell_size`.
    pub fn get_point_position(&self, id: Vector2i) -> Vector2 {
        Vector2::new(
            self.offset.x + id.x as f32 * self.cell_size.x,
            self.offset.y + id.y as f32 * self.cell_size.y,
        )
    }

    /// A cell is walkable when it is inside the region and not solid.
    fn walkable(&self, id: Vector2i) -> bool {
        self.is_in_boundsv(id) && !self.is_point_solid(id)
    }

    /// Whether a diagonal step from `cur` by `(dx, dy)` is allowed under the
    /// current diagonal mode, based on the two shared orthogonal neighbours.
    fn diagonal_allowed(&self, cur: Vector2i, dx: i32, dy: i32) -> bool {
        let side_a = self.walkable(Vector2i::new(cur.x + dx, cur.y));
        let side_b = self.walkable(Vector2i::new(cur.x, cur.y + dy));
        match self.diagonal_mode {
            DiagonalMode::Always => true,
            DiagonalMode::Never => false,
            DiagonalMode::AtLeastOneWalkable => side_a || side_b,
            DiagonalMode::OnlyIfNoObstacles => side_a && side_b,
        }
    }

    /// Neighbouring walkable cells reachable from `cur` under the diagonal rules.
    fn neighbors(&self, cur: Vector2i) -> Vec<Vector2i> {
        let mut out = Vec::with_capacity(8);
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let nb = Vector2i::new(cur.x + dx, cur.y + dy);
            if self.walkable(nb) {
                out.push(nb);
            }
        }
        if self.diagonal_mode != DiagonalMode::Never {
            for (dx, dy) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
                let nb = Vector2i::new(cur.x + dx, cur.y + dy);
                if self.walkable(nb) && self.diagonal_allowed(cur, dx, dy) {
                    out.push(nb);
                }
            }
        }
        // Deterministic expansion order.
        out.sort_unstable_by_key(|v| (v.x, v.y));
        out
    }

    /// Shortest grid path of cell ids from `from` to `to` (inclusive). Empty
    /// when an endpoint is out of bounds or solid, or no route exists. With
    /// `jumping_enabled`, collinear intermediate cells are collapsed so only
    /// turn points and the endpoints remain.
    pub fn get_id_path(&self, from: Vector2i, to: Vector2i) -> Vec<Vector2i> {
        if !self.walkable(from) || !self.walkable(to) {
            return Vec::new();
        }
        if from == to {
            return vec![from];
        }
        let to_pos = (to.x as f32, to.y as f32);

        let mut g_score: HashMap<Vector2i, f32> = HashMap::new();
        let mut came_from: HashMap<Vector2i, Vector2i> = HashMap::new();
        let mut closed: HashSet<Vector2i> = HashSet::new();
        let mut open: BinaryHeap<GridHeapEntry> = BinaryHeap::new();

        g_score.insert(from, 0.0);
        open.push(GridHeapEntry {
            f: self
                .estimate_heuristic
                .cost(to_pos.0 - from.x as f32, to_pos.1 - from.y as f32),
            id: from,
        });

        while let Some(GridHeapEntry { id: current, .. }) = open.pop() {
            if current == to {
                let path = reconstruct_grid(&came_from, from, to);
                return if self.jumping_enabled {
                    compress_collinear(&path)
                } else {
                    path
                };
            }
            if !closed.insert(current) {
                continue;
            }
            let cur_g = g_score[&current];
            for nb in self.neighbors(current) {
                if closed.contains(&nb) {
                    continue;
                }
                let step = self
                    .compute_heuristic
                    .cost((nb.x - current.x) as f32, (nb.y - current.y) as f32)
                    * self.get_point_weight_scale(nb);
                let tentative = cur_g + step;
                if tentative < *g_score.get(&nb).unwrap_or(&f32::INFINITY) {
                    g_score.insert(nb, tentative);
                    came_from.insert(nb, current);
                    let h = self
                        .estimate_heuristic
                        .cost(to_pos.0 - nb.x as f32, to_pos.1 - nb.y as f32);
                    open.push(GridHeapEntry {
                        f: tentative + h,
                        id: nb,
                    });
                }
            }
        }
        Vec::new()
    }

    /// World positions along [`get_id_path`].
    pub fn get_point_path(&self, from: Vector2i, to: Vector2i) -> Vec<Vector2> {
        self.get_id_path(from, to)
            .into_iter()
            .map(|id| self.get_point_position(id))
            .collect()
    }
}

/// Rebuilds a grid id path from the `came_from` chain.
fn reconstruct_grid(
    came_from: &HashMap<Vector2i, Vector2i>,
    from: Vector2i,
    to: Vector2i,
) -> Vec<Vector2i> {
    let mut path = vec![to];
    let mut c = to;
    while c != from {
        match came_from.get(&c) {
            Some(&prev) => {
                path.push(prev);
                c = prev;
            }
            None => break,
        }
    }
    path.reverse();
    path
}

/// Collapses runs of collinear steps, keeping only endpoints and turn points
/// (the observable effect of Godot's jumping option on open corridors).
fn compress_collinear(path: &[Vector2i]) -> Vec<Vector2i> {
    if path.len() <= 2 {
        return path.to_vec();
    }
    let mut out = vec![path[0]];
    for i in 1..path.len() - 1 {
        let prev = (path[i].x - path[i - 1].x, path[i].y - path[i - 1].y);
        let next = (path[i + 1].x - path[i].x, path[i + 1].y - path[i].y);
        if prev != next {
            out.push(path[i]);
        }
    }
    out.push(path[path.len() - 1]);
    out
}

/// Grid frontier entry ordered so the max-heap yields the lowest f-score first.
struct GridHeapEntry {
    f: f32,
    id: Vector2i,
}
impl PartialEq for GridHeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.f == other.f && self.id == other.id
    }
}
impl Eq for GridHeapEntry {}
impl PartialOrd for GridHeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for GridHeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .f
            .partial_cmp(&self.f)
            .unwrap_or(Ordering::Equal)
            .then_with(|| (other.id.x, other.id.y).cmp(&(self.id.x, self.id.y)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v2(x: f32, y: f32) -> Vector2 {
        Vector2::new(x, y)
    }

    #[test]
    fn add_get_remove_point() {
        let mut a = AStar2D::new();
        assert!(a.add_point(1, v2(0.0, 0.0), 1.0));
        assert!(a.add_point(2, v2(5.0, 0.0), 2.0));
        assert!(a.has_point(1));
        assert_eq!(a.get_point_count(), 2);
        assert_eq!(a.get_point_weight_scale(2), Some(2.0));
        assert_eq!(a.get_point_ids(), vec![1, 2]);

        // Negative id and negative weight are rejected.
        assert!(!a.add_point(-1, v2(0.0, 0.0), 1.0));
        assert!(!a.add_point(3, v2(0.0, 0.0), -1.0));
        assert!(!a.has_point(3));

        assert!(a.remove_point(1));
        assert!(!a.has_point(1));
        assert!(!a.remove_point(1));
    }

    #[test]
    fn simple_line_path() {
        let mut a = AStar2D::new();
        a.add_point(1, v2(0.0, 0.0), 1.0);
        a.add_point(2, v2(1.0, 0.0), 1.0);
        a.add_point(3, v2(2.0, 0.0), 1.0);
        a.connect_points(1, 2, true);
        a.connect_points(2, 3, true);

        assert_eq!(a.get_id_path(1, 3), vec![1, 2, 3]);
        let pts = a.get_point_path(1, 3);
        assert_eq!(pts.len(), 3);
        assert!((pts[2].x - 2.0).abs() < 1e-5);
        // Path to itself is a single point.
        assert_eq!(a.get_id_path(2, 2), vec![2]);
    }

    #[test]
    fn weight_scale_changes_route() {
        // Two routes from 1 to 4. The geometrically shorter route (via 2) has a
        // high weight, so the longer-but-cheaper route (via 3) wins.
        let mut a = AStar2D::new();
        a.add_point(1, v2(0.0, 0.0), 1.0);
        a.add_point(2, v2(1.0, 0.0), 10.0); // on the straight line, heavy
        a.add_point(3, v2(1.0, 1.0), 1.0); // detour, light
        a.add_point(4, v2(2.0, 0.0), 1.0);
        a.connect_points(1, 2, true);
        a.connect_points(2, 4, true);
        a.connect_points(1, 3, true);
        a.connect_points(3, 4, true);

        // via 2: 1*10 + 1*1 = 11 ; via 3: sqrt2*1 + sqrt2*1 ≈ 2.83
        assert_eq!(a.get_id_path(1, 4), vec![1, 3, 4]);
    }

    #[test]
    fn disabled_point_blocks_and_endpoint_rejected() {
        let mut a = AStar2D::new();
        a.add_point(1, v2(0.0, 0.0), 1.0);
        a.add_point(2, v2(1.0, 0.0), 1.0);
        a.add_point(3, v2(2.0, 0.0), 1.0);
        a.connect_points(1, 2, true);
        a.connect_points(2, 3, true);

        a.set_point_disabled(2, true);
        assert!(a.is_point_disabled(2));
        // Only route runs through the disabled midpoint -> no path.
        assert!(a.get_id_path(1, 3).is_empty());

        // Disabled endpoint also yields no path.
        a.set_point_disabled(2, false);
        a.set_point_disabled(3, true);
        assert!(a.get_id_path(1, 3).is_empty());
    }

    #[test]
    fn directed_connection() {
        let mut a = AStar2D::new();
        a.add_point(1, v2(0.0, 0.0), 1.0);
        a.add_point(2, v2(1.0, 0.0), 1.0);
        a.connect_points(1, 2, false);

        assert!(a.are_points_connected(1, 2, false));
        assert!(!a.are_points_connected(2, 1, false));
        assert!(a.are_points_connected(2, 1, true));
        assert_eq!(a.get_point_connections(1), vec![2]);
        assert_eq!(a.get_point_connections(2), Vec::<i64>::new());

        assert_eq!(a.get_id_path(1, 2), vec![1, 2]);
        assert!(a.get_id_path(2, 1).is_empty());
    }

    #[test]
    fn disconnect_and_remove_clear_edges() {
        let mut a = AStar2D::new();
        a.add_point(1, v2(0.0, 0.0), 1.0);
        a.add_point(2, v2(1.0, 0.0), 1.0);
        a.add_point(3, v2(2.0, 0.0), 1.0);
        a.connect_points(1, 2, true);
        a.connect_points(2, 3, true);

        assert!(a.disconnect_points(1, 2, true));
        assert!(!a.are_points_connected(1, 2, true));

        // Removing 3 drops 2's edge to it.
        a.connect_points(1, 2, true);
        assert!(a.remove_point(3));
        assert_eq!(a.get_point_connections(2), vec![1]);
    }

    #[test]
    fn closest_point() {
        let mut a = AStar2D::new();
        a.add_point(1, v2(0.0, 0.0), 1.0);
        a.add_point(2, v2(10.0, 0.0), 1.0);
        a.add_point(3, v2(3.0, 0.0), 1.0);

        assert_eq!(a.get_closest_point(v2(2.5, 0.0), false), 3);
        assert_eq!(a.get_closest_point(v2(-1.0, 0.0), false), 1);

        // Disabling the nearest skips it unless include_disabled.
        a.set_point_disabled(3, true);
        assert_eq!(a.get_closest_point(v2(2.5, 0.0), false), 1);
        assert_eq!(a.get_closest_point(v2(2.5, 0.0), true), 3);

        assert_eq!(AStar2D::new().get_closest_point(v2(0.0, 0.0), false), -1);
    }

    #[test]
    fn astar3d_path() {
        let mut a = AStar3D::new();
        a.add_point(1, Vector3::new(0.0, 0.0, 0.0), 1.0);
        a.add_point(2, Vector3::new(0.0, 1.0, 0.0), 1.0);
        a.add_point(3, Vector3::new(0.0, 2.0, 0.0), 1.0);
        a.add_point(4, Vector3::new(0.0, 2.0, 5.0), 1.0);
        a.connect_points(1, 2, true);
        a.connect_points(2, 3, true);
        a.connect_points(3, 4, true);

        assert_eq!(a.get_id_path(1, 4), vec![1, 2, 3, 4]);
        let path = a.get_point_path(1, 4);
        assert_eq!(path.len(), 4);
        assert!((path[3].z - 5.0).abs() < 1e-5);
    }

    #[test]
    fn no_path_when_disconnected() {
        let mut a = AStar2D::new();
        a.add_point(1, v2(0.0, 0.0), 1.0);
        a.add_point(2, v2(1.0, 0.0), 1.0);
        // No connection between them.
        assert!(a.get_id_path(1, 2).is_empty());
        // Missing endpoints.
        assert!(a.get_id_path(1, 99).is_empty());
    }

    // ----- AStarGrid2D -----

    fn vi(x: i32, y: i32) -> Vector2i {
        Vector2i::new(x, y)
    }

    #[test]
    fn grid_diagonal_path() {
        let mut g = AStarGrid2D::new();
        g.set_region(vi(0, 0), vi(5, 5));
        // Default: diagonals ALWAYS, EUCLIDEAN heuristics -> straight diagonal.
        assert_eq!(
            g.get_id_path(vi(0, 0), vi(2, 2)),
            vec![vi(0, 0), vi(1, 1), vi(2, 2)]
        );
    }

    #[test]
    fn grid_never_diagonal_orthogonal_only() {
        let mut g = AStarGrid2D::new();
        g.set_region(vi(0, 0), vi(5, 5));
        g.set_diagonal_mode(DiagonalMode::Never);
        g.set_default_compute_heuristic(Heuristic::Manhattan);
        g.set_default_estimate_heuristic(Heuristic::Manhattan);
        let path = g.get_id_path(vi(0, 0), vi(2, 2));
        // 4 orthogonal steps -> 5 cells; exact route is tie-broken but bounded.
        assert_eq!(path.len(), 5);
        assert_eq!(path.first(), Some(&vi(0, 0)));
        assert_eq!(path.last(), Some(&vi(2, 2)));
        // No diagonal steps: each consecutive pair differs on exactly one axis.
        for w in path.windows(2) {
            let d = ((w[1].x - w[0].x).abs(), (w[1].y - w[0].y).abs());
            assert_eq!(d.0 + d.1, 1);
        }
    }

    #[test]
    fn grid_solid_forces_detour() {
        let mut g = AStarGrid2D::new();
        g.set_region(vi(0, 0), vi(3, 3));
        g.set_diagonal_mode(DiagonalMode::Never);
        // Wall on column x=1 for rows 0 and 1; open at row 2.
        assert!(g.set_point_solid(vi(1, 0), true));
        assert!(g.set_point_solid(vi(1, 1), true));
        assert!(g.is_point_solid(vi(1, 0)));

        let path = g.get_id_path(vi(0, 0), vi(2, 0));
        assert!(!path.is_empty());
        assert_eq!(path.first(), Some(&vi(0, 0)));
        assert_eq!(path.last(), Some(&vi(2, 0)));
        assert!(!path.contains(&vi(1, 0)));
        assert!(!path.contains(&vi(1, 1)));
    }

    #[test]
    fn grid_invalid_endpoints() {
        let mut g = AStarGrid2D::new();
        g.set_region(vi(0, 0), vi(3, 3));
        // Out of bounds start.
        assert!(g.get_id_path(vi(-1, 0), vi(2, 2)).is_empty());
        // Solid endpoint.
        g.set_point_solid(vi(2, 2), true);
        assert!(g.get_id_path(vi(0, 0), vi(2, 2)).is_empty());
        // set_point_solid rejects out-of-bounds.
        assert!(!g.set_point_solid(vi(9, 9), true));
    }

    #[test]
    fn grid_diagonal_mode_gating() {
        let mut g = AStarGrid2D::new();
        g.set_region(vi(0, 0), vi(2, 2));
        // Block one of the two orthogonal cells shared by the (0,0)->(1,1) diagonal.
        g.set_point_solid(vi(1, 0), true);

        g.set_diagonal_mode(DiagonalMode::AtLeastOneWalkable);
        // (0,1) side is walkable -> diagonal allowed and cheapest.
        assert_eq!(g.get_id_path(vi(0, 0), vi(1, 1)), vec![vi(0, 0), vi(1, 1)]);

        g.set_diagonal_mode(DiagonalMode::OnlyIfNoObstacles);
        // (1,0) side is solid -> diagonal blocked; must route through (0,1).
        assert_eq!(
            g.get_id_path(vi(0, 0), vi(1, 1)),
            vec![vi(0, 0), vi(0, 1), vi(1, 1)]
        );
    }

    #[test]
    fn grid_jumping_compresses_straight_run() {
        let mut g = AStarGrid2D::new();
        g.set_region(vi(0, 0), vi(5, 1));
        g.set_diagonal_mode(DiagonalMode::Never);
        g.set_jumping_enabled(true);
        // Straight corridor collapses to just the endpoints.
        assert_eq!(g.get_id_path(vi(0, 0), vi(4, 0)), vec![vi(0, 0), vi(4, 0)]);
    }

    #[test]
    fn grid_point_position_uses_cell_size_and_offset() {
        let mut g = AStarGrid2D::new();
        g.set_region(vi(0, 0), vi(4, 4));
        g.set_cell_size(Vector2::new(2.0, 2.0));
        g.set_offset(Vector2::new(10.0, 20.0));
        let p = g.get_point_position(vi(2, 3));
        assert!((p.x - 14.0).abs() < 1e-5);
        assert!((p.y - 26.0).abs() < 1e-5);
    }

    #[test]
    fn grid_weight_scale_biases_route() {
        let mut g = AStarGrid2D::new();
        g.set_region(vi(0, 0), vi(3, 3));
        g.set_diagonal_mode(DiagonalMode::Never);
        g.set_default_compute_heuristic(Heuristic::Manhattan);
        g.set_default_estimate_heuristic(Heuristic::Manhattan);
        // Make the whole middle row expensive so the path hugs row 0.
        for x in 0..3 {
            assert!(g.set_point_weight_scale(vi(x, 1), 50.0));
        }
        let path = g.get_id_path(vi(0, 0), vi(2, 0));
        // Cheapest is straight along row 0 (weight 1) rather than dipping down.
        assert_eq!(path, vec![vi(0, 0), vi(1, 0), vi(2, 0)]);
    }
}

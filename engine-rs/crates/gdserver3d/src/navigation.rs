//! NavigationRegion3D with 3D navigation mesh baking.
//!
//! Provides [`NavigationMesh3D`] (the nav mesh resource) and
//! [`NavigationRegion3D`] (the node that holds it). Supports polygon-based
//! navigation meshes with vertex/polygon storage, AABB queries, and a
//! simple baking pipeline from source geometry.
//!
//! Mirrors Godot's `NavigationMesh` resource and `NavigationRegion3D` node.

use std::collections::HashMap;

use gdcore::math::Vector3;
use gdcore::math3d::{Aabb, Transform3D};

// ---------------------------------------------------------------------------
// NavigationMesh3D — the nav mesh resource
// ---------------------------------------------------------------------------

/// A polygon within a navigation mesh, referencing vertex indices.
#[derive(Debug, Clone, PartialEq)]
pub struct NavPolygon3D {
    /// Indices into the parent mesh's vertex array.
    pub indices: Vec<u32>,
}

/// A 3D navigation mesh resource.
///
/// Mirrors Godot's `NavigationMesh`. Contains a set of vertices and polygons
/// that define walkable surfaces. Used by `NavigationRegion3D` for pathfinding.
#[derive(Debug, Clone, Default)]
pub struct NavigationMesh3D {
    /// Vertices of the navigation mesh in local space.
    pub vertices: Vec<Vector3>,
    /// Polygons referencing vertex indices. Each polygon defines a convex
    /// walkable region.
    pub polygons: Vec<NavPolygon3D>,
    /// Cell size for voxelization during baking (meters). Godot default: 0.25.
    pub cell_size: f32,
    /// Cell height for voxelization during baking (meters). Godot default: 0.25.
    pub cell_height: f32,
    /// Agent height for baking (meters). Godot default: 1.5.
    pub agent_height: f32,
    /// Agent radius for baking (meters). Godot default: 0.5.
    pub agent_radius: f32,
    /// Agent max climb (step height, meters). Godot default: 0.25.
    pub agent_max_climb: f32,
    /// Agent max slope (degrees). Godot default: 45.0.
    pub agent_max_slope: f32,
    /// Minimum region area to keep after baking (m²). Godot default: 8.0.
    pub region_min_size: f32,
    /// Margin applied to edges during baking (meters). Godot default: 0.6.
    pub edge_max_length: f32,
}

impl NavigationMesh3D {
    /// Creates a new empty navigation mesh with Godot-compatible defaults.
    pub fn new() -> Self {
        Self {
            vertices: Vec::new(),
            polygons: Vec::new(),
            cell_size: 0.25,
            cell_height: 0.25,
            agent_height: 1.5,
            agent_radius: 0.5,
            agent_max_climb: 0.25,
            agent_max_slope: 45.0,
            region_min_size: 8.0,
            edge_max_length: 0.6,
        }
    }

    /// Returns the number of polygons in the mesh.
    pub fn polygon_count(&self) -> usize {
        self.polygons.len()
    }

    /// Returns the number of vertices in the mesh.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Adds a polygon from vertex indices. Returns `true` if all indices are valid.
    pub fn add_polygon(&mut self, indices: &[u32]) -> bool {
        let n = self.vertices.len() as u32;
        if indices.iter().any(|&i| i >= n) {
            return false;
        }
        self.polygons.push(NavPolygon3D {
            indices: indices.to_vec(),
        });
        true
    }

    /// Returns the AABB of the navigation mesh vertices.
    pub fn get_aabb(&self) -> Aabb {
        if self.vertices.is_empty() {
            return Aabb::new(Vector3::ZERO, Vector3::ZERO);
        }
        let mut min = self.vertices[0];
        let mut max = self.vertices[0];
        for v in &self.vertices[1..] {
            min.x = min.x.min(v.x);
            min.y = min.y.min(v.y);
            min.z = min.z.min(v.z);
            max.x = max.x.max(v.x);
            max.y = max.y.max(v.y);
            max.z = max.z.max(v.z);
        }
        Aabb::new(
            min,
            Vector3::new(max.x - min.x, max.y - min.y, max.z - min.z),
        )
    }

    /// Returns the center of a polygon (average of its vertices).
    pub fn polygon_center(&self, poly_idx: usize) -> Option<Vector3> {
        let poly = self.polygons.get(poly_idx)?;
        if poly.indices.is_empty() {
            return None;
        }
        let mut sum = Vector3::ZERO;
        for &idx in &poly.indices {
            let v = self.vertices[idx as usize];
            sum.x += v.x;
            sum.y += v.y;
            sum.z += v.z;
        }
        let n = poly.indices.len() as f32;
        Some(Vector3::new(sum.x / n, sum.y / n, sum.z / n))
    }

    /// Finds the closest polygon to a given point (by polygon center distance).
    /// Returns the polygon index, or `None` if the mesh is empty.
    pub fn find_closest_polygon(&self, point: Vector3) -> Option<usize> {
        let mut best_idx = None;
        let mut best_dist = f32::MAX;
        for (i, _) in self.polygons.iter().enumerate() {
            if let Some(center) = self.polygon_center(i) {
                let dx = point.x - center.x;
                let dy = point.y - center.y;
                let dz = point.z - center.z;
                let dist = dx * dx + dy * dy + dz * dz;
                if dist < best_dist {
                    best_dist = dist;
                    best_idx = Some(i);
                }
            }
        }
        best_idx
    }

    /// Resolves a polygon's vertex indices to positions, in winding order.
    ///
    /// The pathfinding side stores polygons as position lists rather than
    /// indices, so this is the bridge between the two representations
    /// (pat-nzgdu.114).
    pub fn polygon_vertices(&self, poly_idx: usize) -> Option<Vec<Vector3>> {
        let poly = self.polygons.get(poly_idx)?;
        poly.indices
            .iter()
            .map(|&i| self.vertices.get(i as usize).copied())
            .collect()
    }

    /// Computes polygon adjacency: for each polygon, the indices of the polygons
    /// it shares an edge with.
    ///
    /// This is the piece a baked mesh is missing before it can be pathfound
    /// over — [`bake_navigation_mesh`] emits vertices and polygons but no
    /// connectivity, and a navigation map needs an adjacency list to run A*
    /// across (pat-nzgdu.114).
    ///
    /// **Edges are matched by position, not by vertex index**, which is not a
    /// nicety: the bake pushes three fresh vertices for every triangle, so two
    /// touching triangles never share an index and an index-based match would
    /// find no neighbours at all and silently produce an unpathable mesh.
    /// Positions are quantised to [`WELD_EPSILON`] before hashing so vertices
    /// that coincide within float error weld together.
    pub fn compute_adjacency(&self) -> Vec<Vec<usize>> {
        // Canonical key for an undirected edge: the two quantised endpoints,
        // ordered so (a,b) and (b,a) collide.
        let mut edge_owners: HashMap<(WeldKey, WeldKey), Vec<usize>> = HashMap::new();

        for (poly_idx, poly) in self.polygons.iter().enumerate() {
            let n = poly.indices.len();
            if n < 3 {
                continue; // Not a face; contributes no edges.
            }
            for i in 0..n {
                let a = match self.vertices.get(poly.indices[i] as usize) {
                    Some(v) => weld_key(*v),
                    None => continue,
                };
                let b = match self.vertices.get(poly.indices[(i + 1) % n] as usize) {
                    Some(v) => weld_key(*v),
                    None => continue,
                };
                if a == b {
                    continue; // Degenerate edge.
                }
                let key = if a <= b { (a, b) } else { (b, a) };
                let owners = edge_owners.entry(key).or_default();
                // A polygon can touch the same welded edge twice only if it is
                // degenerate; keep the list unique so it cannot self-connect.
                if !owners.contains(&poly_idx) {
                    owners.push(poly_idx);
                }
            }
        }

        let mut connections: Vec<Vec<usize>> = vec![Vec::new(); self.polygons.len()];
        for owners in edge_owners.values() {
            // Every pair sharing this edge becomes mutual neighbours. An edge
            // shared by 3+ polygons (a non-manifold seam) still connects them
            // all, matching how a nav mesh treats a junction.
            for (i, &a) in owners.iter().enumerate() {
                for &b in &owners[i + 1..] {
                    if !connections[a].contains(&b) {
                        connections[a].push(b);
                    }
                    if !connections[b].contains(&a) {
                        connections[b].push(a);
                    }
                }
            }
        }
        // Sort each neighbour list. The loop above walks a HashMap, whose order
        // is randomised per process, so without this a polygon's neighbours come
        // out in a different order every run — which makes A* explore ties in a
        // different order and return a *different* (equal-cost) path run to run.
        // A non-deterministic path is a miserable thing to debug, and it would
        // also make any exact-Vec assertion downstream flaky.
        for neighbours in &mut connections {
            neighbours.sort_unstable();
        }
        connections
    }

    /// Clears all vertices and polygons.
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.polygons.clear();
    }
}

/// Grid size used to bucket vertex positions when finding shared edges (metres).
///
/// This is a *quantisation* grid, not a true epsilon weld: two points are
/// treated as one only when they round to the same cell, so a pair straddling a
/// cell boundary (say 0.00004 and 0.00006) will not weld even though they are
/// far closer together than the grid. That is sound for the bake's own output,
/// which is what this serves — [`bake_navigation_mesh`] copies its vertices
/// straight from the source triangles, so touching faces carry bit-identical
/// coordinates and land in the same cell no matter where the grid falls.
///
/// If source geometry ever arrives from independent computation paths (e.g.
/// two nodes' transforms producing the "same" corner), this needs a real
/// tolerant weld that also probes neighbouring cells. The failure mode is quiet:
/// a missed adjacency edge, i.e. an unpathable seam rather than an error.
const WELD_EPSILON: f32 = 1e-4;

/// A position quantised to the weld grid, usable as a hash key.
type WeldKey = (i64, i64, i64);

/// Quantises a position onto the [`WELD_EPSILON`] grid.
fn weld_key(v: Vector3) -> WeldKey {
    let q = |f: f32| -> i64 { (f / WELD_EPSILON).round() as i64 };
    (q(v.x), q(v.y), q(v.z))
}

// ---------------------------------------------------------------------------
// Simple baking: generate a nav mesh from source triangles
// ---------------------------------------------------------------------------

/// Source geometry for nav mesh baking — a list of triangles in world space.
#[derive(Debug, Clone, Default)]
pub struct BakeSourceGeometry3D {
    /// Triangle vertices (every 3 consecutive vertices form one triangle).
    pub triangles: Vec<Vector3>,
}

impl BakeSourceGeometry3D {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a triangle to the source geometry.
    pub fn add_triangle(&mut self, a: Vector3, b: Vector3, c: Vector3) {
        self.triangles.push(a);
        self.triangles.push(b);
        self.triangles.push(c);
    }

    /// Returns the number of triangles.
    pub fn triangle_count(&self) -> usize {
        self.triangles.len() / 3
    }
}

/// Bakes a navigation mesh from source geometry.
///
/// This is a simplified baking pipeline that:
/// 1. Filters triangles by max slope (keeps only walkable surfaces).
/// 2. Collects unique vertices from walkable triangles.
/// 3. Creates one polygon per walkable triangle.
///
/// A production implementation would use voxelization + watershed, but this
/// captures the essential Godot API contract.
pub fn bake_navigation_mesh(
    params: &NavigationMesh3D,
    source: &BakeSourceGeometry3D,
) -> NavigationMesh3D {
    let max_slope_cos = (params.agent_max_slope.to_radians()).cos();
    let up = Vector3::new(0.0, 1.0, 0.0);

    let mut vertices: Vec<Vector3> = Vec::new();
    let mut polygons: Vec<NavPolygon3D> = Vec::new();

    let tri_count = source.triangle_count();
    for i in 0..tri_count {
        let a = source.triangles[i * 3];
        let b = source.triangles[i * 3 + 1];
        let c = source.triangles[i * 3 + 2];

        // Compute triangle normal.
        let edge1 = Vector3::new(b.x - a.x, b.y - a.y, b.z - a.z);
        let edge2 = Vector3::new(c.x - a.x, c.y - a.y, c.z - a.z);
        let normal = Vector3::new(
            edge1.y * edge2.z - edge1.z * edge2.y,
            edge1.z * edge2.x - edge1.x * edge2.z,
            edge1.x * edge2.y - edge1.y * edge2.x,
        );
        let len = (normal.x * normal.x + normal.y * normal.y + normal.z * normal.z).sqrt();
        if len < 1e-6 {
            continue; // Degenerate triangle.
        }
        let norm = Vector3::new(normal.x / len, normal.y / len, normal.z / len);

        // Slope test: |dot(normal, up)| >= cos(max_slope).
        // Use absolute value to handle both winding orders.
        let dot = (norm.x * up.x + norm.y * up.y + norm.z * up.z).abs();
        if dot < max_slope_cos {
            continue; // Too steep.
        }

        // Narrow-area filter (agent-radius offset, pat-u3l8c.65): a triangle
        // whose inscribed circle is smaller than the agent radius cannot fit the
        // agent, so it is excluded from the walkable mesh. `len` == 2 * area, and
        // the inradius == area / semiperimeter == len / perimeter.
        if params.agent_radius > 0.0 {
            let ab = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2) + (b.z - a.z).powi(2)).sqrt();
            let bc = ((c.x - b.x).powi(2) + (c.y - b.y).powi(2) + (c.z - b.z).powi(2)).sqrt();
            let ca = ((a.x - c.x).powi(2) + (a.y - c.y).powi(2) + (a.z - c.z).powi(2)).sqrt();
            let perimeter = ab + bc + ca;
            if perimeter > 1e-6 && len / perimeter < params.agent_radius {
                continue; // Too narrow for the agent.
            }
        }

        // Add vertices and polygon.
        let base_idx = vertices.len() as u32;
        vertices.push(a);
        vertices.push(b);
        vertices.push(c);
        polygons.push(NavPolygon3D {
            indices: vec![base_idx, base_idx + 1, base_idx + 2],
        });
    }

    NavigationMesh3D {
        vertices,
        polygons,
        cell_size: params.cell_size,
        cell_height: params.cell_height,
        agent_height: params.agent_height,
        agent_radius: params.agent_radius,
        agent_max_climb: params.agent_max_climb,
        agent_max_slope: params.agent_max_slope,
        region_min_size: params.region_min_size,
        edge_max_length: params.edge_max_length,
    }
}

// ---------------------------------------------------------------------------
// NavigationRegion3D — the scene node
// ---------------------------------------------------------------------------

/// A 3D navigation region that holds a navigation mesh.
///
/// Mirrors Godot's `NavigationRegion3D` node. Placed in the scene tree to
/// define walkable areas for `NavigationAgent3D` pathfinding.
#[derive(Debug, Clone)]
pub struct NavigationRegion3D {
    /// The navigation mesh resource.
    pub navigation_mesh: Option<NavigationMesh3D>,
    /// Whether this region is active for navigation.
    pub enabled: bool,
    /// Navigation layers bitmask (1-based layers, like collision layers).
    pub navigation_layers: u32,
    /// Cost to enter this region (for pathfinding cost calculation).
    pub enter_cost: f32,
    /// Cost multiplier for traveling within this region.
    pub travel_cost: f32,
    /// World-space transform of this region.
    pub transform: Transform3D,
}

impl Default for NavigationRegion3D {
    fn default() -> Self {
        Self {
            navigation_mesh: None,
            enabled: true,
            navigation_layers: 1,
            enter_cost: 0.0,
            travel_cost: 1.0,
            transform: Transform3D::IDENTITY,
        }
    }
}

impl NavigationRegion3D {
    /// Creates a new navigation region with no mesh.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new navigation region with the given mesh.
    pub fn with_mesh(mesh: NavigationMesh3D) -> Self {
        Self {
            navigation_mesh: Some(mesh),
            ..Self::default()
        }
    }

    /// Returns the world-space AABB of the navigation mesh, or `None`.
    pub fn get_world_aabb(&self) -> Option<Aabb> {
        let mesh = self.navigation_mesh.as_ref()?;
        if mesh.vertices.is_empty() {
            return None;
        }
        let local_aabb = mesh.get_aabb();
        Some(transform_aabb(&local_aabb, &self.transform))
    }

    /// Returns whether a given navigation layer (1-based) is enabled.
    pub fn get_navigation_layer_value(&self, layer: u32) -> bool {
        if layer == 0 || layer > 32 {
            return false;
        }
        (self.navigation_layers >> (layer - 1)) & 1 != 0
    }

    /// Sets or clears a navigation layer (1-based).
    pub fn set_navigation_layer_value(&mut self, layer: u32, value: bool) {
        if layer == 0 || layer > 32 {
            return;
        }
        if value {
            self.navigation_layers |= 1 << (layer - 1);
        } else {
            self.navigation_layers &= !(1 << (layer - 1));
        }
    }

    /// Bakes the navigation mesh from source geometry.
    pub fn bake(&mut self, source: &BakeSourceGeometry3D) {
        let params = self
            .navigation_mesh
            .as_ref()
            .cloned()
            .unwrap_or_else(NavigationMesh3D::new);
        self.navigation_mesh = Some(bake_navigation_mesh(&params, source));
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Transforms a local-space AABB to world space (conservative, axis-aligned).
fn transform_aabb(aabb: &Aabb, t: &Transform3D) -> Aabb {
    let corners = [
        Vector3::new(aabb.position.x, aabb.position.y, aabb.position.z),
        Vector3::new(
            aabb.position.x + aabb.size.x,
            aabb.position.y,
            aabb.position.z,
        ),
        Vector3::new(
            aabb.position.x,
            aabb.position.y + aabb.size.y,
            aabb.position.z,
        ),
        Vector3::new(
            aabb.position.x + aabb.size.x,
            aabb.position.y + aabb.size.y,
            aabb.position.z,
        ),
        Vector3::new(
            aabb.position.x,
            aabb.position.y,
            aabb.position.z + aabb.size.z,
        ),
        Vector3::new(
            aabb.position.x + aabb.size.x,
            aabb.position.y,
            aabb.position.z + aabb.size.z,
        ),
        Vector3::new(
            aabb.position.x,
            aabb.position.y + aabb.size.y,
            aabb.position.z + aabb.size.z,
        ),
        Vector3::new(
            aabb.position.x + aabb.size.x,
            aabb.position.y + aabb.size.y,
            aabb.position.z + aabb.size.z,
        ),
    ];
    let first = t.xform(corners[0]);
    let mut min = first;
    let mut max = first;
    for &c in &corners[1..] {
        let w = t.xform(c);
        min.x = min.x.min(w.x);
        min.y = min.y.min(w.y);
        min.z = min.z.min(w.z);
        max.x = max.x.max(w.x);
        max.y = max.y.max(w.y);
        max.z = max.z.max(w.z);
    }
    Aabb::new(
        min,
        Vector3::new(max.x - min.x, max.y - min.y, max.z - min.z),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-4;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    // -- NavigationMesh3D --

    #[test]
    fn nav_mesh_defaults() {
        let mesh = NavigationMesh3D::new();
        assert!(approx_eq(mesh.cell_size, 0.25));
        assert!(approx_eq(mesh.cell_height, 0.25));
        assert!(approx_eq(mesh.agent_height, 1.5));
        assert!(approx_eq(mesh.agent_radius, 0.5));
        assert!(approx_eq(mesh.agent_max_climb, 0.25));
        assert!(approx_eq(mesh.agent_max_slope, 45.0));
        assert_eq!(mesh.polygon_count(), 0);
        assert_eq!(mesh.vertex_count(), 0);
    }

    #[test]
    fn nav_mesh_add_polygon_valid() {
        let mut mesh = NavigationMesh3D::new();
        mesh.vertices = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ];
        assert!(mesh.add_polygon(&[0, 1, 2]));
        assert_eq!(mesh.polygon_count(), 1);
    }

    #[test]
    fn nav_mesh_add_polygon_invalid_index() {
        let mut mesh = NavigationMesh3D::new();
        mesh.vertices = vec![Vector3::ZERO];
        assert!(!mesh.add_polygon(&[0, 1, 2]));
        assert_eq!(mesh.polygon_count(), 0);
    }

    #[test]
    fn nav_mesh_aabb() {
        let mut mesh = NavigationMesh3D::new();
        mesh.vertices = vec![
            Vector3::new(-1.0, 0.0, -1.0),
            Vector3::new(3.0, 0.0, -1.0),
            Vector3::new(3.0, 0.0, 2.0),
            Vector3::new(-1.0, 0.0, 2.0),
        ];
        let aabb = mesh.get_aabb();
        assert!(approx_eq(aabb.position.x, -1.0));
        assert!(approx_eq(aabb.position.z, -1.0));
        assert!(approx_eq(aabb.size.x, 4.0));
        assert!(approx_eq(aabb.size.z, 3.0));
    }

    #[test]
    fn nav_mesh_empty_aabb() {
        let mesh = NavigationMesh3D::new();
        let aabb = mesh.get_aabb();
        assert!(approx_eq(aabb.size.x, 0.0));
    }

    #[test]
    fn nav_mesh_polygon_center() {
        let mut mesh = NavigationMesh3D::new();
        mesh.vertices = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(3.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 3.0),
        ];
        mesh.add_polygon(&[0, 1, 2]);
        let center = mesh.polygon_center(0).unwrap();
        assert!(approx_eq(center.x, 1.0));
        assert!(approx_eq(center.z, 1.0));
    }

    #[test]
    fn nav_mesh_find_closest_polygon() {
        let mut mesh = NavigationMesh3D::new();
        mesh.vertices = vec![
            // Polygon 0: centered around (0, 0, 0)
            Vector3::new(-1.0, 0.0, -1.0),
            Vector3::new(1.0, 0.0, -1.0),
            Vector3::new(0.0, 0.0, 1.0),
            // Polygon 1: centered around (10, 0, 0)
            Vector3::new(9.0, 0.0, -1.0),
            Vector3::new(11.0, 0.0, -1.0),
            Vector3::new(10.0, 0.0, 1.0),
        ];
        mesh.add_polygon(&[0, 1, 2]);
        mesh.add_polygon(&[3, 4, 5]);

        assert_eq!(
            mesh.find_closest_polygon(Vector3::new(0.0, 0.0, 0.0)),
            Some(0)
        );
        assert_eq!(
            mesh.find_closest_polygon(Vector3::new(10.0, 0.0, 0.0)),
            Some(1)
        );
    }

    #[test]
    fn nav_mesh_clear() {
        let mut mesh = NavigationMesh3D::new();
        mesh.vertices = vec![
            Vector3::ZERO,
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ];
        mesh.add_polygon(&[0, 1, 2]);
        mesh.clear();
        assert_eq!(mesh.vertex_count(), 0);
        assert_eq!(mesh.polygon_count(), 0);
    }

    // -- BakeSourceGeometry3D --

    #[test]
    fn source_geometry_add_triangle() {
        let mut src = BakeSourceGeometry3D::new();
        src.add_triangle(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        );
        assert_eq!(src.triangle_count(), 1);
        assert_eq!(src.triangles.len(), 3);
    }

    // -- Baking --

    #[test]
    fn bake_flat_floor_produces_polygons() {
        let params = NavigationMesh3D::new();
        let mut src = BakeSourceGeometry3D::new();
        // Flat floor triangle (normal = up).
        src.add_triangle(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 10.0),
        );
        let result = bake_navigation_mesh(&params, &src);
        assert_eq!(result.polygon_count(), 1);
        assert_eq!(result.vertex_count(), 3);
    }

    #[test]
    fn bake_steep_wall_excluded() {
        let params = NavigationMesh3D::new(); // max_slope = 45 degrees
        let mut src = BakeSourceGeometry3D::new();
        // Vertical wall (normal = horizontal).
        src.add_triangle(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 10.0, 0.0),
            Vector3::new(0.0, 0.0, 10.0),
        );
        let result = bake_navigation_mesh(&params, &src);
        assert_eq!(
            result.polygon_count(),
            0,
            "Vertical wall should be excluded"
        );
    }

    #[test]
    fn bake_slope_at_limit() {
        let mut params = NavigationMesh3D::new();
        params.agent_max_slope = 90.0; // Allow all slopes.
        let mut src = BakeSourceGeometry3D::new();
        // 45-degree slope.
        src.add_triangle(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 10.0, 0.0),
            Vector3::new(0.0, 0.0, 10.0),
        );
        let result = bake_navigation_mesh(&params, &src);
        assert_eq!(result.polygon_count(), 1);
    }

    #[test]
    fn bake_degenerate_triangle_skipped() {
        let params = NavigationMesh3D::new();
        let mut src = BakeSourceGeometry3D::new();
        // Degenerate triangle (all same point).
        src.add_triangle(Vector3::ZERO, Vector3::ZERO, Vector3::ZERO);
        let result = bake_navigation_mesh(&params, &src);
        assert_eq!(result.polygon_count(), 0);
    }

    #[test]
    fn bake_preserves_params() {
        let mut params = NavigationMesh3D::new();
        params.cell_size = 0.5;
        params.agent_height = 2.0;
        let src = BakeSourceGeometry3D::new();
        let result = bake_navigation_mesh(&params, &src);
        assert!(approx_eq(result.cell_size, 0.5));
        assert!(approx_eq(result.agent_height, 2.0));
    }

    // -- NavigationRegion3D --

    #[test]
    fn region_defaults() {
        let region = NavigationRegion3D::new();
        assert!(region.enabled);
        assert!(region.navigation_mesh.is_none());
        assert_eq!(region.navigation_layers, 1);
        assert!(approx_eq(region.enter_cost, 0.0));
        assert!(approx_eq(region.travel_cost, 1.0));
    }

    #[test]
    fn region_with_mesh() {
        let mesh = NavigationMesh3D::new();
        let region = NavigationRegion3D::with_mesh(mesh);
        assert!(region.navigation_mesh.is_some());
    }

    #[test]
    fn region_world_aabb_none_without_mesh() {
        let region = NavigationRegion3D::new();
        assert!(region.get_world_aabb().is_none());
    }

    #[test]
    fn region_world_aabb_with_mesh() {
        let mut mesh = NavigationMesh3D::new();
        mesh.vertices = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(5.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 5.0),
        ];
        let region = NavigationRegion3D::with_mesh(mesh);
        let aabb = region.get_world_aabb().unwrap();
        assert!(approx_eq(aabb.position.x, 0.0));
        assert!(approx_eq(aabb.size.x, 5.0));
    }

    #[test]
    fn region_world_aabb_translated() {
        let mut mesh = NavigationMesh3D::new();
        mesh.vertices = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ];
        let mut region = NavigationRegion3D::with_mesh(mesh);
        region.transform.origin = Vector3::new(10.0, 0.0, 0.0);
        let aabb = region.get_world_aabb().unwrap();
        assert!(approx_eq(aabb.position.x, 10.0));
    }

    #[test]
    fn region_navigation_layers() {
        let mut region = NavigationRegion3D::new();
        assert!(region.get_navigation_layer_value(1));
        assert!(!region.get_navigation_layer_value(2));

        region.set_navigation_layer_value(2, true);
        assert!(region.get_navigation_layer_value(2));

        region.set_navigation_layer_value(1, false);
        assert!(!region.get_navigation_layer_value(1));
    }

    #[test]
    fn region_navigation_layer_bounds() {
        let region = NavigationRegion3D::new();
        assert!(!region.get_navigation_layer_value(0));
        assert!(!region.get_navigation_layer_value(33));
    }

    #[test]
    fn bake_excludes_narrow_triangles_by_agent_radius() {
        let mut params = NavigationMesh3D::new();
        params.agent_radius = 0.5;

        // A wide flat triangle (inradius ~1.24 > 0.5) is walkable.
        let mut wide = BakeSourceGeometry3D::new();
        wide.add_triangle(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(4.0, 0.0, 0.0),
            Vector3::new(2.0, 0.0, 4.0),
        );
        assert_eq!(bake_navigation_mesh(&params, &wide).polygon_count(), 1);

        // A thin sliver (inradius ~0.1 < 0.5) is too narrow for the agent.
        let mut thin = BakeSourceGeometry3D::new();
        thin.add_triangle(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(5.0, 0.0, 0.2),
        );
        assert_eq!(bake_navigation_mesh(&params, &thin).polygon_count(), 0);
    }

    #[test]
    fn bake_zero_agent_radius_keeps_thin_triangles() {
        let mut params = NavigationMesh3D::new();
        params.agent_radius = 0.0; // disables the narrow-area filter
        let mut thin = BakeSourceGeometry3D::new();
        thin.add_triangle(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(5.0, 0.0, 0.2),
        );
        assert_eq!(bake_navigation_mesh(&params, &thin).polygon_count(), 1);
    }

    #[test]
    fn region_bake() {
        let mut region = NavigationRegion3D::new();
        let mut src = BakeSourceGeometry3D::new();
        src.add_triangle(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 10.0),
        );
        region.bake(&src);
        assert!(region.navigation_mesh.is_some());
        let mesh = region.navigation_mesh.as_ref().unwrap();
        assert_eq!(mesh.polygon_count(), 1);
    }

    #[test]
    fn region_bake_preserves_existing_params() {
        let mut mesh = NavigationMesh3D::new();
        mesh.cell_size = 0.1;
        mesh.agent_height = 3.0;
        let mut region = NavigationRegion3D::with_mesh(mesh);
        let mut src = BakeSourceGeometry3D::new();
        src.add_triangle(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(5.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 5.0),
        );
        region.bake(&src);
        let mesh = region.navigation_mesh.as_ref().unwrap();
        assert!(approx_eq(mesh.cell_size, 0.1));
        assert!(approx_eq(mesh.agent_height, 3.0));
    }

    // -- Adjacency (pat-nzgdu.114) --
    //
    // A baked mesh carries no connectivity, and a navigation map needs an
    // adjacency list to run A* over — so without these the bake's output is
    // unpathable. The end-to-end proof (bake -> map -> path) lives in
    // gdscene's `navmesh_bake_from_geometry_feeds_map`; these pin the algorithm
    // itself here, in the crate that owns it.

    /// Adds the two triangles of an axis-aligned floor quad spanning
    /// `[x0,x1] x [z0,z1]`, wound so the normal points up.
    fn add_floor_quad(src: &mut BakeSourceGeometry3D, x0: f32, x1: f32, z0: f32, z1: f32) {
        src.add_triangle(
            Vector3::new(x0, 0.0, z0),
            Vector3::new(x0, 0.0, z1),
            Vector3::new(x1, 0.0, z1),
        );
        src.add_triangle(
            Vector3::new(x0, 0.0, z0),
            Vector3::new(x1, 0.0, z1),
            Vector3::new(x1, 0.0, z0),
        );
    }

    #[test]
    fn adjacency_matches_shared_edges_by_position_not_index() {
        let mut src = BakeSourceGeometry3D::new();
        add_floor_quad(&mut src, 0.0, 4.0, 0.0, 4.0);
        let baked = bake_navigation_mesh(&NavigationMesh3D::new(), &src);

        assert_eq!(baked.polygon_count(), 2, "a quad bakes to two triangles");
        // The bake pushes three FRESH vertices per triangle, so the two halves
        // of one quad share no vertex index at all...
        assert_eq!(baked.vertex_count(), 6, "no vertex index is reused");
        let a = &baked.polygons[0].indices;
        let b = &baked.polygons[1].indices;
        assert!(
            a.iter().all(|i| !b.contains(i)),
            "the two triangles share no index: {a:?} vs {b:?}"
        );

        // ...yet they are neighbours, because they share the diagonal by
        // POSITION. An index-based match would find nothing here and silently
        // produce an unpathable mesh.
        let adj = baked.compute_adjacency();
        assert_eq!(adj[0], vec![1]);
        assert_eq!(adj[1], vec![0]);
    }

    #[test]
    fn adjacency_links_abutting_quads_into_one_connected_strip() {
        let mut src = BakeSourceGeometry3D::new();
        add_floor_quad(&mut src, 0.0, 4.0, 0.0, 4.0);
        add_floor_quad(&mut src, 4.0, 8.0, 0.0, 4.0);
        add_floor_quad(&mut src, 8.0, 12.0, 0.0, 4.0);
        let baked = bake_navigation_mesh(&NavigationMesh3D::new(), &src);
        let adj = baked.compute_adjacency();

        assert_eq!(baked.polygon_count(), 6);
        assert!(
            adj.iter().all(|n| !n.is_empty()),
            "every triangle of a connected floor has a neighbour: {adj:?}"
        );
        // Reachability across the whole strip — what makes it pathable end to end.
        let mut seen = vec![false; 6];
        let mut stack = vec![0usize];
        while let Some(p) = stack.pop() {
            if seen[p] {
                continue;
            }
            seen[p] = true;
            stack.extend(adj[p].iter().copied());
        }
        assert!(seen.iter().all(|&s| s), "all six triangles are reachable from 0");
    }

    #[test]
    fn adjacency_keeps_disconnected_islands_apart() {
        let mut src = BakeSourceGeometry3D::new();
        add_floor_quad(&mut src, 0.0, 4.0, 0.0, 4.0);
        add_floor_quad(&mut src, 100.0, 104.0, 0.0, 4.0);
        let baked = bake_navigation_mesh(&NavigationMesh3D::new(), &src);
        let adj = baked.compute_adjacency();

        assert_eq!(baked.polygon_count(), 4);
        for (i, n) in adj.iter().enumerate() {
            assert_eq!(n.len(), 1, "polygon {i} touches only its own quad: {n:?}");
        }
        // Island A is {0,1}; island B is {2,3}. They must never link.
        assert!(!adj[0].contains(&2) && !adj[0].contains(&3));
        assert!(!adj[1].contains(&2) && !adj[1].contains(&3));
    }

    #[test]
    fn adjacency_is_sorted_so_pathfinding_is_deterministic() {
        // A fan of quads gives an interior triangle several neighbours. The
        // lists are built by walking a HashMap, whose order is randomised per
        // process — sorting keeps A*'s tie-breaking (and any assertion on these
        // lists) stable run to run.
        let mut src = BakeSourceGeometry3D::new();
        add_floor_quad(&mut src, 0.0, 4.0, 0.0, 4.0);
        add_floor_quad(&mut src, 4.0, 8.0, 0.0, 4.0);
        add_floor_quad(&mut src, 8.0, 12.0, 0.0, 4.0);
        let adj = bake_navigation_mesh(&NavigationMesh3D::new(), &src).compute_adjacency();

        for n in &adj {
            let mut sorted = n.clone();
            sorted.sort_unstable();
            assert_eq!(n, &sorted, "neighbour lists must be sorted: {n:?}");
        }
    }

    #[test]
    fn adjacency_is_empty_for_an_empty_or_single_polygon_mesh() {
        let empty = NavigationMesh3D::new();
        assert!(empty.compute_adjacency().is_empty(), "no polygons, no lists");

        let mut src = BakeSourceGeometry3D::new();
        src.add_triangle(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(4.0, 0.0, 0.0),
            Vector3::new(2.0, 0.0, 4.0),
        );
        let one = bake_navigation_mesh(&NavigationMesh3D::new(), &src);
        assert_eq!(one.polygon_count(), 1);
        assert_eq!(
            one.compute_adjacency(),
            vec![Vec::<usize>::new()],
            "a lone triangle has no neighbour — and must not link to itself"
        );
    }

    #[test]
    fn polygon_vertices_resolves_indices_and_rejects_a_bad_index() {
        let mut src = BakeSourceGeometry3D::new();
        src.add_triangle(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(4.0, 0.0, 0.0),
            Vector3::new(2.0, 0.0, 4.0),
        );
        let baked = bake_navigation_mesh(&NavigationMesh3D::new(), &src);
        let vs = baked.polygon_vertices(0).expect("polygon 0 resolves");
        assert_eq!(vs.len(), 3);
        assert!(approx_eq(vs[1].x, 4.0));
        assert!(baked.polygon_vertices(99).is_none(), "no such polygon");

        // An index pointing past the vertex array yields None rather than panicking.
        let mut broken = NavigationMesh3D::new();
        broken.vertices.push(Vector3::ZERO);
        broken.polygons.push(NavPolygon3D {
            indices: vec![0, 7, 9],
        });
        assert!(
            broken.polygon_vertices(0).is_none(),
            "a dangling vertex index must not panic"
        );
    }
}

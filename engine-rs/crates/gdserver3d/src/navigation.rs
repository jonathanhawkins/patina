//! NavigationRegion3D with 3D navigation mesh baking.
//!
//! Provides [`NavigationMesh3D`] (the nav mesh resource) and
//! [`NavigationRegion3D`] (the node that holds it). Supports polygon-based
//! navigation meshes with vertex/polygon storage, AABB queries, and a
//! simple baking pipeline from source geometry.
//!
//! Mirrors Godot's `NavigationMesh` resource and `NavigationRegion3D` node.

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

    /// Clears all vertices and polygons.
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.polygons.clear();
    }
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
}

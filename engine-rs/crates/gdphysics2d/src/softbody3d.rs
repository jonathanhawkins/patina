//! SoftBody3D vertex-based deformable mesh simulation.
//!
//! Implements a mass-spring-damper system where mesh vertices are particles
//! connected by distance constraints. Supports vertex pinning, configurable
//! stiffness and damping, and integration with the physics world gravity.
//!
//! Matches Godot's SoftBody3D behavior:
//! - Each vertex is a point mass affected by gravity
//! - Edges between vertices act as springs with stiffness/damping
//! - Individual vertices can be pinned to fixed world positions
//! - The simulation pressure parameter inflates the soft body

use gdcore::math::Vector3;

/// Unique identifier for a soft body in the physics world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SoftBodyId3D(pub u64);

/// A single particle (vertex) in the soft body mesh.
#[derive(Debug, Clone)]
pub struct SoftBodyVertex {
    /// Current world-space position.
    pub position: Vector3,
    /// Current velocity.
    pub velocity: Vector3,
    /// Inverse mass (0 = pinned / infinite mass).
    pub inv_mass: f32,
    /// If true, this vertex is pinned and will not move.
    pub pinned: bool,
    /// Accumulated force for the current step.
    accumulated_force: Vector3,
}

impl SoftBodyVertex {
    /// Creates a new vertex at the given position with the given mass.
    pub fn new(position: Vector3, mass: f32) -> Self {
        let inv_mass = if mass > 0.0 { 1.0 / mass } else { 0.0 };
        Self {
            position,
            velocity: Vector3::ZERO,
            inv_mass,
            pinned: false,
            accumulated_force: Vector3::ZERO,
        }
    }
}

/// A distance constraint (spring) between two vertices.
#[derive(Debug, Clone, Copy)]
pub struct SoftBodyEdge {
    /// Index of the first vertex.
    pub v0: usize,
    /// Index of the second vertex.
    pub v1: usize,
    /// Rest length of the spring.
    pub rest_length: f32,
}

/// A deformable soft body with vertex-based simulation.
///
/// Models Godot's SoftBody3D: a mesh whose vertices are simulated as
/// point masses connected by spring constraints.
#[derive(Debug, Clone)]
pub struct SoftBody3D {
    /// Unique identifier.
    pub id: SoftBodyId3D,
    /// Vertices (particles) of the soft body.
    pub vertices: Vec<SoftBodyVertex>,
    /// Edge constraints connecting vertices.
    pub edges: Vec<SoftBodyEdge>,
    /// Spring stiffness coefficient (higher = stiffer). Range: [0, 1].
    pub stiffness: f32,
    /// Damping coefficient applied to vertex velocities. Range: [0, 1].
    pub damping: f32,
    /// Pressure pushing vertices outward from the centroid.
    pub pressure: f32,
    /// Number of constraint solver iterations per step.
    pub solver_iterations: u32,
    /// Linear velocity damping per step.
    pub linear_damp: f32,
    /// Collision layer bitmask.
    pub collision_layer: u32,
}

impl SoftBody3D {
    /// Creates a new soft body with the given vertices and edges.
    pub fn new(
        id: SoftBodyId3D,
        positions: &[Vector3],
        edges: &[(usize, usize)],
    ) -> Self {
        let vertices: Vec<SoftBodyVertex> = positions
            .iter()
            .map(|&p| SoftBodyVertex::new(p, 1.0))
            .collect();

        let edge_constraints: Vec<SoftBodyEdge> = edges
            .iter()
            .map(|&(v0, v1)| {
                let rest_length = (positions[v0] - positions[v1]).length();
                SoftBodyEdge {
                    v0,
                    v1,
                    rest_length,
                }
            })
            .collect();

        Self {
            id,
            vertices,
            edges: edge_constraints,
            stiffness: 0.5,
            damping: 0.01,
            pressure: 0.0,
            solver_iterations: 4,
            linear_damp: 0.0,
            collision_layer: 1,
        }
    }

    /// Pins a vertex at the given index so it will not move during simulation.
    pub fn pin_vertex(&mut self, index: usize) {
        if let Some(v) = self.vertices.get_mut(index) {
            v.pinned = true;
            v.inv_mass = 0.0;
            v.velocity = Vector3::ZERO;
        }
    }

    /// Unpins a vertex, restoring it to dynamic simulation with the given mass.
    pub fn unpin_vertex(&mut self, index: usize, mass: f32) {
        if let Some(v) = self.vertices.get_mut(index) {
            v.pinned = false;
            v.inv_mass = if mass > 0.0 { 1.0 / mass } else { 0.0 };
        }
    }

    /// Returns true if the vertex at the given index is pinned.
    pub fn is_vertex_pinned(&self, index: usize) -> bool {
        self.vertices.get(index).map_or(false, |v| v.pinned)
    }

    /// Returns the number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Returns the number of edge constraints.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Returns the centroid (average position) of all vertices.
    pub fn centroid(&self) -> Vector3 {
        if self.vertices.is_empty() {
            return Vector3::ZERO;
        }
        let sum = self
            .vertices
            .iter()
            .fold(Vector3::ZERO, |acc, v| acc + v.position);
        sum * (1.0 / self.vertices.len() as f32)
    }

    /// Steps the soft body simulation by `dt` seconds.
    ///
    /// 1. Apply gravity to all unpinned vertices
    /// 2. Apply pressure force (outward from centroid)
    /// 3. Integrate velocity and position
    /// 4. Solve distance constraints iteratively
    /// 5. Apply damping
    pub fn step(&mut self, dt: f32, gravity: Vector3) {
        if dt <= 0.0 {
            return;
        }

        // Phase 1: Apply gravity as force
        for vertex in &mut self.vertices {
            if !vertex.pinned {
                vertex.accumulated_force = gravity;
            }
        }

        // Phase 2: Apply pressure (outward from centroid)
        if self.pressure > 0.0 {
            let center = self.centroid();
            for vertex in &mut self.vertices {
                if !vertex.pinned {
                    let dir = vertex.position - center;
                    let dist = dir.length();
                    if dist > 1e-6 {
                        let pressure_force = dir * (self.pressure / dist);
                        vertex.accumulated_force = vertex.accumulated_force + pressure_force;
                    }
                }
            }
        }

        // Phase 3: Integrate (semi-implicit Euler)
        for vertex in &mut self.vertices {
            if vertex.pinned {
                continue;
            }
            let acceleration = vertex.accumulated_force * vertex.inv_mass;
            vertex.velocity = vertex.velocity + acceleration * dt;

            // Apply linear damping
            if self.linear_damp > 0.0 {
                let damp = (1.0 - self.linear_damp * dt).max(0.0);
                vertex.velocity = vertex.velocity * damp;
            }

            vertex.position = vertex.position + vertex.velocity * dt;
            vertex.accumulated_force = Vector3::ZERO;
        }

        // Phase 4: Solve distance constraints (position-based dynamics)
        for _ in 0..self.solver_iterations {
            self.solve_constraints(dt);
        }

        // Phase 5: Apply damping to velocities
        if self.damping > 0.0 {
            let damp_factor = (1.0 - self.damping).max(0.0);
            for vertex in &mut self.vertices {
                if !vertex.pinned {
                    vertex.velocity = vertex.velocity * damp_factor;
                }
            }
        }
    }

    /// Solves distance constraints using position-based dynamics.
    fn solve_constraints(&mut self, _dt: f32) {
        for edge_idx in 0..self.edges.len() {
            let edge = self.edges[edge_idx];
            let v0_pos = self.vertices[edge.v0].position;
            let v1_pos = self.vertices[edge.v1].position;
            let v0_inv = self.vertices[edge.v0].inv_mass;
            let v1_inv = self.vertices[edge.v1].inv_mass;

            let total_inv = v0_inv + v1_inv;
            if total_inv < 1e-10 {
                continue; // Both pinned
            }

            let delta = v1_pos - v0_pos;
            let current_length = delta.length();
            if current_length < 1e-10 {
                continue; // Degenerate
            }

            let error = current_length - edge.rest_length;
            let correction = delta * (error / current_length * self.stiffness);

            if !self.vertices[edge.v0].pinned {
                let w = v0_inv / total_inv;
                self.vertices[edge.v0].position =
                    self.vertices[edge.v0].position + correction * w;
            }
            if !self.vertices[edge.v1].pinned {
                let w = v1_inv / total_inv;
                self.vertices[edge.v1].position =
                    self.vertices[edge.v1].position - correction * w;
            }
        }
    }

    /// Returns the axis-aligned bounding box of all vertices as (min, max).
    pub fn bounding_box(&self) -> (Vector3, Vector3) {
        if self.vertices.is_empty() {
            return (Vector3::ZERO, Vector3::ZERO);
        }
        let mut min = self.vertices[0].position;
        let mut max = self.vertices[0].position;
        for v in &self.vertices[1..] {
            min.x = min.x.min(v.position.x);
            min.y = min.y.min(v.position.y);
            min.z = min.z.min(v.position.z);
            max.x = max.x.max(v.position.x);
            max.y = max.y.max(v.position.y);
            max.z = max.z.max(v.position.z);
        }
        (min, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a simple 4-vertex tetrahedron soft body for testing.
    fn make_tetrahedron() -> SoftBody3D {
        let positions = vec![
            Vector3::new(0.0, 1.0, 0.0),  // top
            Vector3::new(-1.0, 0.0, -1.0), // front-left
            Vector3::new(1.0, 0.0, -1.0),  // front-right
            Vector3::new(0.0, 0.0, 1.0),   // back
        ];
        let edges = vec![
            (0, 1),
            (0, 2),
            (0, 3),
            (1, 2),
            (1, 3),
            (2, 3),
        ];
        SoftBody3D::new(SoftBodyId3D(1), &positions, &edges)
    }

    /// Creates a simple 2-vertex spring for basic tests.
    fn make_spring() -> SoftBody3D {
        let positions = vec![
            Vector3::new(0.0, 5.0, 0.0),
            Vector3::new(0.0, 3.0, 0.0),
        ];
        let edges = vec![(0, 1)];
        SoftBody3D::new(SoftBodyId3D(1), &positions, &edges)
    }

    #[test]
    fn soft_body_creation() {
        let body = make_tetrahedron();
        assert_eq!(body.vertex_count(), 4);
        assert_eq!(body.edge_count(), 6);
    }

    #[test]
    fn soft_body_rest_lengths_computed() {
        let body = make_spring();
        assert_eq!(body.edges.len(), 1);
        let expected_rest = 2.0; // distance from (0,5,0) to (0,3,0)
        assert!((body.edges[0].rest_length - expected_rest).abs() < 1e-5);
    }

    #[test]
    fn soft_body_pin_vertex() {
        let mut body = make_tetrahedron();
        body.pin_vertex(0);
        assert!(body.is_vertex_pinned(0));
        assert!(!body.is_vertex_pinned(1));
        assert!((body.vertices[0].inv_mass).abs() < 1e-10);
    }

    #[test]
    fn soft_body_unpin_vertex() {
        let mut body = make_tetrahedron();
        body.pin_vertex(0);
        assert!(body.is_vertex_pinned(0));
        body.unpin_vertex(0, 1.0);
        assert!(!body.is_vertex_pinned(0));
        assert!((body.vertices[0].inv_mass - 1.0).abs() < 1e-5);
    }

    #[test]
    fn soft_body_gravity_moves_vertices() {
        let mut body = make_tetrahedron();
        let initial_centroid = body.centroid();
        let gravity = Vector3::new(0.0, -9.8, 0.0);

        for _ in 0..10 {
            body.step(1.0 / 60.0, gravity);
        }

        let final_centroid = body.centroid();
        assert!(
            final_centroid.y < initial_centroid.y,
            "soft body should fall under gravity: initial_y={}, final_y={}",
            initial_centroid.y,
            final_centroid.y
        );
    }

    #[test]
    fn soft_body_pinned_vertex_stays_fixed() {
        let mut body = make_spring();
        let pin_pos = body.vertices[0].position;
        body.pin_vertex(0);

        let gravity = Vector3::new(0.0, -9.8, 0.0);
        for _ in 0..60 {
            body.step(1.0 / 60.0, gravity);
        }

        assert_eq!(
            body.vertices[0].position, pin_pos,
            "pinned vertex must not move"
        );
    }

    #[test]
    fn soft_body_unpinned_falls_while_pinned_stays() {
        let mut body = make_spring();
        body.pin_vertex(0); // pin top vertex

        let gravity = Vector3::new(0.0, -9.8, 0.0);
        for _ in 0..30 {
            body.step(1.0 / 60.0, gravity);
        }

        // Top stays
        assert!(
            (body.vertices[0].position.y - 5.0).abs() < 1e-5,
            "pinned vertex should stay at y=5"
        );
        // Bottom falls (though constraint pulls it back)
        assert!(
            body.vertices[1].position.y < 3.0,
            "unpinned vertex should fall: y={}",
            body.vertices[1].position.y
        );
    }

    #[test]
    fn soft_body_constraints_maintain_shape() {
        let mut body = make_tetrahedron();
        body.stiffness = 1.0;
        body.solver_iterations = 10;

        // Step with no gravity — shape should stay roughly the same
        for _ in 0..10 {
            body.step(1.0 / 60.0, Vector3::ZERO);
        }

        // Check all edges are near rest length
        for edge in &body.edges {
            let len =
                (body.vertices[edge.v1].position - body.vertices[edge.v0].position).length();
            assert!(
                (len - edge.rest_length).abs() < 0.01,
                "edge ({},{}) length {} should be near rest {}",
                edge.v0,
                edge.v1,
                len,
                edge.rest_length
            );
        }
    }

    #[test]
    fn soft_body_stiffness_affects_deformation() {
        // Soft body with low stiffness deforms more than high stiffness
        let gravity = Vector3::new(0.0, -9.8, 0.0);

        let mut soft = make_tetrahedron();
        soft.stiffness = 0.1;
        soft.pin_vertex(0);
        for _ in 0..60 {
            soft.step(1.0 / 60.0, gravity);
        }

        let mut stiff = make_tetrahedron();
        stiff.stiffness = 1.0;
        stiff.solver_iterations = 10;
        stiff.pin_vertex(0);
        for _ in 0..60 {
            stiff.step(1.0 / 60.0, gravity);
        }

        // Compute average deviation from rest lengths
        let deviation = |body: &SoftBody3D| -> f32 {
            body.edges
                .iter()
                .map(|e| {
                    let len = (body.vertices[e.v1].position
                        - body.vertices[e.v0].position)
                        .length();
                    (len - e.rest_length).abs()
                })
                .sum::<f32>()
                / body.edges.len() as f32
        };

        let soft_dev = deviation(&soft);
        let stiff_dev = deviation(&stiff);
        assert!(
            soft_dev > stiff_dev,
            "soft body (dev={soft_dev}) should deform more than stiff (dev={stiff_dev})"
        );
    }

    #[test]
    fn soft_body_damping_reduces_velocity() {
        let mut body = make_tetrahedron();
        body.damping = 0.5;

        // Give initial velocity to all vertices
        for v in &mut body.vertices {
            v.velocity = Vector3::new(10.0, 0.0, 0.0);
        }

        body.step(1.0 / 60.0, Vector3::ZERO);

        // Velocities should be reduced by damping
        for v in &body.vertices {
            assert!(
                v.velocity.x < 10.0,
                "damping should reduce velocity: vx={}",
                v.velocity.x
            );
        }
    }

    #[test]
    fn soft_body_zero_dt_is_noop() {
        let mut body = make_tetrahedron();
        let positions_before: Vec<Vector3> =
            body.vertices.iter().map(|v| v.position).collect();

        body.step(0.0, Vector3::new(0.0, -9.8, 0.0));

        for (i, v) in body.vertices.iter().enumerate() {
            assert_eq!(v.position, positions_before[i]);
        }
    }

    #[test]
    fn soft_body_deterministic() {
        let gravity = Vector3::new(0.0, -9.8, 0.0);

        let run = || {
            let mut body = make_tetrahedron();
            body.stiffness = 0.7;
            body.damping = 0.02;
            body.pin_vertex(0);
            for _ in 0..100 {
                body.step(1.0 / 60.0, gravity);
            }
            body.vertices
                .iter()
                .map(|v| v.position)
                .collect::<Vec<_>>()
        };

        let a = run();
        let b = run();
        assert_eq!(a, b, "soft body simulation must be deterministic");
    }

    #[test]
    fn soft_body_centroid() {
        let body = make_spring();
        let centroid = body.centroid();
        assert!((centroid.x).abs() < 1e-5);
        assert!((centroid.y - 4.0).abs() < 1e-5);
        assert!((centroid.z).abs() < 1e-5);
    }

    #[test]
    fn soft_body_bounding_box() {
        let body = make_tetrahedron();
        let (min, max) = body.bounding_box();
        assert!((min.x - (-1.0)).abs() < 1e-5);
        assert!((min.y - 0.0).abs() < 1e-5);
        assert!((max.x - 1.0).abs() < 1e-5);
        assert!((max.y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn soft_body_pressure_inflates() {
        let mut body = make_tetrahedron();
        body.pressure = 10.0;
        body.stiffness = 0.0; // disable springs so pressure effect is clear

        let initial_centroid = body.centroid();
        body.step(1.0 / 60.0, Vector3::ZERO);

        // Vertices should move outward from centroid
        let (min_before, max_before) = {
            let positions = vec![
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(-1.0, 0.0, -1.0),
                Vector3::new(1.0, 0.0, -1.0),
                Vector3::new(0.0, 0.0, 1.0),
            ];
            let mut min = positions[0];
            let mut max = positions[0];
            for p in &positions[1..] {
                min.x = min.x.min(p.x);
                min.y = min.y.min(p.y);
                min.z = min.z.min(p.z);
                max.x = max.x.max(p.x);
                max.y = max.y.max(p.y);
                max.z = max.z.max(p.z);
            }
            (min, max)
        };
        let (min_after, max_after) = body.bounding_box();
        let size_before = max_before - min_before;
        let size_after = max_after - min_after;
        let _ = initial_centroid; // used indirectly via centroid() in step

        // At least one dimension should have expanded
        let expanded = size_after.x > size_before.x
            || size_after.y > size_before.y
            || size_after.z > size_before.z;
        assert!(expanded, "pressure should inflate the body");
    }

    #[test]
    fn soft_body_empty() {
        let body = SoftBody3D::new(SoftBodyId3D(0), &[], &[]);
        assert_eq!(body.vertex_count(), 0);
        assert_eq!(body.edge_count(), 0);
        assert_eq!(body.centroid(), Vector3::ZERO);
        assert_eq!(body.bounding_box(), (Vector3::ZERO, Vector3::ZERO));
    }

    #[test]
    fn soft_body_single_vertex() {
        let mut body = SoftBody3D::new(
            SoftBodyId3D(0),
            &[Vector3::new(0.0, 10.0, 0.0)],
            &[],
        );
        let gravity = Vector3::new(0.0, -9.8, 0.0);
        body.step(1.0, gravity);
        assert!(
            body.vertices[0].position.y < 10.0,
            "single vertex should fall under gravity"
        );
    }
}

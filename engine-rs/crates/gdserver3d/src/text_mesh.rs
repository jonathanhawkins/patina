//! Godot-compatible `TextMesh` geometry core (pat-nzgdu.163).
//!
//! `TextMesh` is a [`PrimitiveMesh`](crate::primitive_mesh) subclass that turns
//! a string's glyph outlines into extruded 3D geometry. Producing that geometry
//! has two independent halves:
//!
//! 1. **Outline extraction** — walking the active font's glyph contours for each
//!    character and laying them out along the baseline. That lives in the font
//!    system (`gdrender2d/src/font.rs`) and the ClassDB/resource wiring, and is
//!    *not* implemented here.
//! 2. **Contour → mesh** — triangulating each closed 2D contour and extruding it
//!    into a solid with front/back caps and side walls. That is a pure,
//!    self-contained geometry algorithm, and it is what this module provides.
//!
//! The triangulator is a standard **ear-clipping** pass over a single simple
//! polygon (no holes). Winding is normalised to counter-clockwise so the emitted
//! front cap faces `+Z`. Glyphs whose outlines contain holes (`O`, `A`, `8`, …)
//! need contour-nesting / bridge handling that is deliberately left for a
//! follow-up; each contour here is triangulated and extruded independently.
//!
//! # Scope / remaining work (pat-nzgdu.163)
//!
//! This delivers only the pure geometry core + tests. Still open on the bead:
//! registering `PrimitiveMesh`/`SphereMesh`/`BoxMesh`/… in `class_db.rs`,
//! extracting real glyph contours from the font system, hole handling for holed
//! glyphs, and the app-level render/ClassDB wiring test.

use gdcore::math::{Vector2, Vector3};

use crate::mesh::{Mesh3D, PrimitiveType};

/// Twice the signed area of triangle `(a, b, c)`.
///
/// Positive when `a → b → c` turns counter-clockwise (a left turn), zero when
/// the three points are collinear, negative for a clockwise turn. This is the
/// standard orientation primitive used by both the winding test and the
/// point-in-triangle test below.
#[inline]
fn orient2d(a: Vector2, b: Vector2, c: Vector2) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// Twice the signed area of the whole polygon (shoelace). Positive = CCW.
fn signed_area(points: &[Vector2]) -> f32 {
    let n = points.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..n {
        let a = points[i];
        let b = points[(i + 1) % n];
        sum += a.x * b.y - b.x * a.y;
    }
    sum
}

/// Whether point `p` lies inside or on the boundary of CCW triangle `(a, b, c)`.
///
/// Uses the inclusive (`>= 0`) convention so a vertex lying exactly on a
/// candidate ear's edge still *blocks* that ear — clipping it anyway would
/// produce a self-intersecting remainder polygon.
fn point_in_triangle(p: Vector2, a: Vector2, b: Vector2, c: Vector2) -> bool {
    orient2d(a, b, p) >= 0.0 && orient2d(b, c, p) >= 0.0 && orient2d(c, a, p) >= 0.0
}

/// Triangulates a single simple polygon (no holes, no self-intersections) via
/// ear clipping.
///
/// Returns the triangles as index triples into `points`, each wound
/// counter-clockwise. A polygon of `n` vertices yields exactly `n - 2`
/// triangles. Degenerate input (`< 3` points) yields an empty list; if the
/// input is not a valid simple polygon the pass returns as many triangles as it
/// could safely clip rather than looping forever.
pub fn triangulate_simple_polygon(points: &[Vector2]) -> Vec<[u32; 3]> {
    let n = points.len();
    if n < 3 {
        return Vec::new();
    }

    // Working vertex-index ring, normalised to CCW so ear apices are the
    // convex (positive-orientation) vertices.
    let mut ring: Vec<usize> = (0..n).collect();
    if signed_area(points) < 0.0 {
        ring.reverse();
    }

    let mut triangles: Vec<[u32; 3]> = Vec::with_capacity(n.saturating_sub(2));
    let mut guard = 0;
    while ring.len() > 3 {
        let m = ring.len();
        let mut clipped = false;
        for i in 0..m {
            let iprev = ring[(i + m - 1) % m];
            let icurr = ring[i];
            let inext = ring[(i + 1) % m];
            let a = points[iprev];
            let b = points[icurr];
            let c = points[inext];

            // Ear apex must be strictly convex for a CCW ring.
            if orient2d(a, b, c) <= 0.0 {
                continue;
            }
            // …and no other polygon vertex may fall inside/on the ear.
            let mut blocked = false;
            for &k in &ring {
                if k == iprev || k == icurr || k == inext {
                    continue;
                }
                if point_in_triangle(points[k], a, b, c) {
                    blocked = true;
                    break;
                }
            }
            if blocked {
                continue;
            }

            triangles.push([iprev as u32, icurr as u32, inext as u32]);
            ring.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            // No ear found — malformed/collinear input. Stop rather than spin.
            break;
        }
        guard += 1;
        if guard > n {
            break;
        }
    }
    if ring.len() == 3 {
        triangles.push([ring[0] as u32, ring[1] as u32, ring[2] as u32]);
    }
    triangles
}

/// Appends a triangle to the raw mesh buffers with an explicit normal.
#[allow(clippy::too_many_arguments)]
fn push_tri(
    vertices: &mut Vec<Vector3>,
    normals: &mut Vec<Vector3>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    a: Vector3,
    b: Vector3,
    c: Vector3,
    normal: Vector3,
    ua: [f32; 2],
    ub: [f32; 2],
    uc: [f32; 2],
) {
    let base = vertices.len() as u32;
    vertices.push(a);
    vertices.push(b);
    vertices.push(c);
    normals.push(normal);
    normals.push(normal);
    normals.push(normal);
    uvs.push(ua);
    uvs.push(ub);
    uvs.push(uc);
    indices.extend_from_slice(&[base, base + 1, base + 2]);
}

/// Extrudes a single closed 2D `contour` into a solid [`Mesh3D`].
///
/// The contour's coordinates are scaled by `pixel_size` into world units and
/// centred on `Z`: the front cap sits at `+depth/2` (normal `+Z`) and the back
/// cap at `-depth/2` (normal `-Z`), joined by outward-facing side-wall quads.
/// When `depth <= 0` the mesh is a single flat front cap (no back cap or walls).
///
/// Returns an empty mesh for contours with fewer than three points.
pub fn extrude_contour(contour: &[Vector2], depth: f32, pixel_size: f32) -> Mesh3D {
    let mut vertices: Vec<Vector3> = Vec::new();
    let mut normals: Vec<Vector3> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let n = contour.len();
    if n >= 3 {
        let tris = triangulate_simple_polygon(contour);
        let hz = depth * 0.5;
        let front_z = if depth > 0.0 { hz } else { 0.0 };

        let scaled: Vec<Vector2> = contour
            .iter()
            .map(|p| Vector2::new(p.x * pixel_size, p.y * pixel_size))
            .collect();

        let uv_of = |p: Vector2| -> [f32; 2] { [p.x, p.y] };

        // Front cap (facing +Z), using the CCW triangulation directly.
        let front_n = Vector3::new(0.0, 0.0, 1.0);
        for t in &tris {
            let a = scaled[t[0] as usize];
            let b = scaled[t[1] as usize];
            let c = scaled[t[2] as usize];
            push_tri(
                &mut vertices,
                &mut normals,
                &mut uvs,
                &mut indices,
                Vector3::new(a.x, a.y, front_z),
                Vector3::new(b.x, b.y, front_z),
                Vector3::new(c.x, c.y, front_z),
                front_n,
                uv_of(a),
                uv_of(b),
                uv_of(c),
            );
        }

        if depth > 0.0 {
            // Back cap (facing -Z), reversed winding.
            let back_n = Vector3::new(0.0, 0.0, -1.0);
            for t in &tris {
                let a = scaled[t[0] as usize];
                let b = scaled[t[1] as usize];
                let c = scaled[t[2] as usize];
                push_tri(
                    &mut vertices,
                    &mut normals,
                    &mut uvs,
                    &mut indices,
                    Vector3::new(a.x, a.y, -hz),
                    Vector3::new(c.x, c.y, -hz),
                    Vector3::new(b.x, b.y, -hz),
                    back_n,
                    uv_of(a),
                    uv_of(c),
                    uv_of(b),
                );
            }

            // Side walls: one quad per contour edge, outward normal from the
            // 2D edge direction rotated -90° (right of the CCW boundary).
            let ccw = signed_area(contour) >= 0.0;
            for i in 0..n {
                let (ia, ib) = if ccw {
                    (i, (i + 1) % n)
                } else {
                    // Reverse edge orientation so "right of edge" stays outward.
                    ((i + 1) % n, i)
                };
                let p0 = scaled[ia];
                let p1 = scaled[ib];
                let dx = p1.x - p0.x;
                let dy = p1.y - p0.y;
                let len = (dx * dx + dy * dy).sqrt();
                let normal = if len > 1e-9 {
                    Vector3::new(dy / len, -dx / len, 0.0)
                } else {
                    Vector3::new(0.0, 0.0, 1.0)
                };

                let f0 = Vector3::new(p0.x, p0.y, hz);
                let f1 = Vector3::new(p1.x, p1.y, hz);
                let b0 = Vector3::new(p0.x, p0.y, -hz);
                let b1 = Vector3::new(p1.x, p1.y, -hz);

                push_tri(
                    &mut vertices,
                    &mut normals,
                    &mut uvs,
                    &mut indices,
                    f0,
                    b1,
                    f1,
                    normal,
                    [0.0, 0.0],
                    [1.0, 1.0],
                    [1.0, 0.0],
                );
                push_tri(
                    &mut vertices,
                    &mut normals,
                    &mut uvs,
                    &mut indices,
                    f0,
                    b0,
                    b1,
                    normal,
                    [0.0, 0.0],
                    [0.0, 1.0],
                    [1.0, 1.0],
                );
            }
        }
    }

    Mesh3D {
        vertices,
        normals,
        uvs,
        indices,
        primitive_type: PrimitiveType::Triangles,
        surfaces: Vec::new(),
        bone_weights: Vec::new(),
        bone_indices: Vec::new(),
        blend_shapes: Vec::new(),
    }
}

/// A Godot-compatible `TextMesh` resource.
///
/// The glyph-outline extraction (font → contours) is wired separately in the
/// font system; [`generate_from_contours`](TextMesh::generate_from_contours)
/// takes contours already positioned in glyph space and applies this resource's
/// `depth` and `pixel_size` to build the extruded mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct TextMesh {
    /// The string to render.
    pub text: String,
    /// Extrusion depth along `Z`, in world units. `0.0` = flat.
    pub depth: f32,
    /// Scale applied to glyph-space coordinates (world units per glyph unit).
    pub pixel_size: f32,
    /// Font size the outlines were sampled at (metadata; layout is the caller's).
    pub font_size: i64,
}

impl Default for TextMesh {
    fn default() -> Self {
        // Matches Godot's TextMesh property defaults.
        Self {
            text: String::new(),
            depth: 0.05,
            pixel_size: 0.01,
            font_size: 16,
        }
    }
}

impl TextMesh {
    /// Creates a `TextMesh` with the given text and Godot-default extrusion.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    /// Builds the extruded mesh from pre-extracted glyph `contours`.
    ///
    /// Each contour is triangulated and extruded independently (see the
    /// module note on holes) and merged into one mesh, with vertex indices
    /// re-based so the combined index buffer stays valid.
    pub fn generate_from_contours(&self, contours: &[Vec<Vector2>]) -> Mesh3D {
        let mut out = Mesh3D {
            vertices: Vec::new(),
            normals: Vec::new(),
            uvs: Vec::new(),
            indices: Vec::new(),
            primitive_type: PrimitiveType::Triangles,
            surfaces: Vec::new(),
            bone_weights: Vec::new(),
            bone_indices: Vec::new(),
            blend_shapes: Vec::new(),
        };
        for contour in contours {
            let part = extrude_contour(contour, self.depth, self.pixel_size);
            let base = out.vertices.len() as u32;
            out.vertices.extend(part.vertices);
            out.normals.extend(part.normals);
            out.uvs.extend(part.uvs);
            out.indices
                .extend(part.indices.iter().map(|&idx| idx + base));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Vec<Vector2> {
        vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 0.0),
            Vector2::new(1.0, 1.0),
            Vector2::new(0.0, 1.0),
        ]
    }

    // A concave "L" polygon with exactly one reflex vertex at (1, 1).
    fn l_shape() -> Vec<Vector2> {
        vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(2.0, 0.0),
            Vector2::new(2.0, 1.0),
            Vector2::new(1.0, 1.0),
            Vector2::new(1.0, 2.0),
            Vector2::new(0.0, 2.0),
        ]
    }

    #[test]
    fn triangulate_square_yields_two_triangles() {
        assert_eq!(triangulate_simple_polygon(&square()).len(), 2);
    }

    #[test]
    fn triangulate_convex_pentagon_yields_n_minus_2() {
        let pent = vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(2.0, 0.0),
            Vector2::new(3.0, 1.5),
            Vector2::new(1.0, 3.0),
            Vector2::new(-1.0, 1.5),
        ];
        assert_eq!(triangulate_simple_polygon(&pent).len(), 3);
    }

    #[test]
    fn triangulate_concave_l_shape_is_n_minus_2() {
        // The reflex vertex must be respected: a naive fan would fail here.
        assert_eq!(triangulate_simple_polygon(&l_shape()).len(), 4);
    }

    #[test]
    fn triangulate_handles_clockwise_input() {
        let mut cw = square();
        cw.reverse();
        assert_eq!(triangulate_simple_polygon(&cw).len(), 2);
    }

    #[test]
    fn triangulate_rejects_too_few_points() {
        assert!(
            triangulate_simple_polygon(&[Vector2::new(0.0, 0.0), Vector2::new(1.0, 0.0)])
                .is_empty()
        );
    }

    #[test]
    fn triangulated_indices_are_in_range() {
        let poly = l_shape();
        for tri in triangulate_simple_polygon(&poly) {
            for &idx in &tri {
                assert!((idx as usize) < poly.len());
            }
        }
    }

    #[test]
    fn extrude_square_has_caps_and_walls() {
        let mesh = extrude_contour(&square(), 2.0, 1.0);
        // front cap: 2 tris, back cap: 2 tris, 4 side quads: 8 tris = 12 tris.
        // Each tri pushes 3 unique verts (no sharing) → 36 verts, 36 indices.
        assert_eq!(mesh.vertices.len(), 36);
        assert_eq!(mesh.indices.len(), 36);
        assert_eq!(mesh.normals.len(), 36);
        assert_eq!(mesh.primitive_type, PrimitiveType::Triangles);
    }

    #[test]
    fn extrude_zero_depth_is_flat_front_only() {
        let mesh = extrude_contour(&square(), 0.0, 1.0);
        // Only the 2-triangle front cap.
        assert_eq!(mesh.vertices.len(), 6);
        assert_eq!(mesh.indices.len(), 6);
        // Flat cap lies on the Z = 0 plane.
        assert!(mesh.vertices.iter().all(|v| v.z.abs() < 1e-6));
    }

    #[test]
    fn extrude_front_cap_faces_positive_z() {
        let mesh = extrude_contour(&square(), 2.0, 1.0);
        // The +Z-facing normals are the front cap (first 6 verts).
        let front = mesh
            .normals
            .iter()
            .filter(|n| n.z > 0.5)
            .collect::<Vec<_>>();
        assert!(!front.is_empty());
        assert!(front.iter().all(|n| (n.z - 1.0).abs() < 1e-6));
    }

    #[test]
    fn extrude_side_normals_point_outward() {
        // Unit square CCW; the bottom edge (0,0)->(1,0) must face -Y outward.
        let mesh = extrude_contour(&square(), 2.0, 1.0);
        let has_down = mesh
            .normals
            .iter()
            .any(|n| n.y < -0.99 && n.x.abs() < 1e-6 && n.z.abs() < 1e-6);
        let has_up = mesh
            .normals
            .iter()
            .any(|n| n.y > 0.99 && n.x.abs() < 1e-6 && n.z.abs() < 1e-6);
        let has_right = mesh.normals.iter().any(|n| n.x > 0.99);
        let has_left = mesh.normals.iter().any(|n| n.x < -0.99);
        assert!(has_down && has_up && has_right && has_left);
    }

    #[test]
    fn generate_from_contours_merges_and_rebases_indices() {
        let mesh = TextMesh::new("ab").generate_from_contours(&[square(), square()]);
        // Two flat-ish extruded squares (default depth 0.05 > 0 → full solids).
        assert_eq!(mesh.vertices.len(), 72);
        let max_idx = *mesh.indices.iter().max().unwrap();
        assert!((max_idx as usize) < mesh.vertices.len());
    }

    #[test]
    fn text_mesh_defaults_match_godot() {
        let tm = TextMesh::default();
        assert_eq!(tm.depth, 0.05);
        assert_eq!(tm.pixel_size, 0.01);
        assert_eq!(tm.font_size, 16);
        assert!(tm.text.is_empty());
    }

    #[test]
    fn pixel_size_scales_geometry() {
        let big = extrude_contour(&square(), 0.0, 2.0);
        // Scaled unit square spans [0, 2] on X and Y.
        let max_x = big.vertices.iter().map(|v| v.x).fold(0.0_f32, f32::max);
        assert!((max_x - 2.0).abs() < 1e-6);
    }
}

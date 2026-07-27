//! `TriangleMesh` — Godot's CPU BVH over a mesh's triangles (pat-0q2kx.256).
//!
//! Godot ground truth: `scene/resources/triangle_mesh.{h,cpp}`,
//! `doc/classes/TriangleMesh.xml`, `core/register_core_types.cpp:258`
//! (`GDREGISTER_CLASS`, so it is concrete).
//!
//! Built via `Mesh.generate_triangle_mesh()` and used for `intersect_ray` /
//! `intersect_segment` and trimesh collision-shape generation. It was entirely
//! absent, so there was no CPU-side ray query against mesh geometry at all.
//!
//! The numeric core is NOT reimplemented here: `gdcore::geometry3d` already
//! carries a vetted Möller–Trumbore, so this module contributes the BVH and the
//! nearest-hit selection and delegates every triangle test to it. Rewriting the
//! intersection would mean two copies of the same maths drifting apart.
//!
//! ⚠️ The BVH is an ACCELERATION structure, not a semantic one: it must return
//! exactly what a linear scan over every face would. The tests below assert
//! that equivalence directly on a mesh large enough to force several levels of
//! split, because a subtly wrong split plane or slab test shows up only as
//! occasional missed hits — never as an error.

use gdcore::geometry3d::{ray_intersects_triangle, segment_intersects_triangle};
use gdcore::math::Vector3;

use crate::mesh::{Mesh3D, PrimitiveType};

/// Faces per leaf. Small enough that leaves are cheap to scan, large enough
/// that the tree does not degenerate into one node per triangle.
const MAX_LEAF_FACES: usize = 4;

/// Guards against unbounded recursion when many faces share a centroid — a
/// median split cannot separate coincident centroids, so without this a mesh
/// of duplicated triangles would recurse forever.
const MAX_DEPTH: u32 = 32;

/// A ray/segment hit against a [`TriangleMesh`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TriangleMeshHit {
    /// Where the ray met the face, in mesh-local space.
    pub position: Vector3,
    /// The face's geometric normal, from its winding order.
    pub normal: Vector3,
    /// Index of the face that was hit, into [`TriangleMesh::faces`].
    pub face_index: usize,
}

/// One BVH node. `count > 0` marks a leaf covering `order[start..start+count]`;
/// otherwise `left`/`right` index into the node array.
#[derive(Debug, Clone, Copy)]
struct BvhNode {
    min: Vector3,
    max: Vector3,
    start: u32,
    count: u32,
    left: u32,
    right: u32,
}

/// A BVH over a fixed set of triangles.
#[derive(Debug, Clone, Default)]
pub struct TriangleMesh {
    /// Every triangle, as three corners in winding order.
    faces: Vec<[Vector3; 3]>,
    /// Face indices grouped so each leaf owns a contiguous run.
    order: Vec<u32>,
    /// Flat node array; node 0 is the root when non-empty.
    nodes: Vec<BvhNode>,
}

impl TriangleMesh {
    /// Builds from a flat corner list — every three entries form one face.
    ///
    /// Mirrors `TriangleMesh.create_from_faces`. A trailing partial triangle is
    /// dropped rather than padded: inventing a corner would put a face in the
    /// tree that the caller never supplied.
    pub fn create_from_faces(corners: &[Vector3]) -> Self {
        let faces: Vec<[Vector3; 3]> = corners
            .chunks_exact(3)
            .map(|c| [c[0], c[1], c[2]])
            .collect();
        Self::from_faces(faces)
    }

    /// Builds from whole faces.
    pub fn from_faces(faces: Vec<[Vector3; 3]>) -> Self {
        let mut mesh = Self {
            faces,
            order: Vec::new(),
            nodes: Vec::new(),
        };
        mesh.build();
        mesh
    }

    /// The faces, flattened to three corners each — Godot's `get_faces()`.
    pub fn get_faces(&self) -> Vec<Vector3> {
        self.faces.iter().flat_map(|f| f.iter().copied()).collect()
    }

    /// Number of triangles in the tree.
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// Whether the tree holds no geometry. Godot's `generate_triangle_mesh`
    /// returns null for such a mesh rather than an empty structure.
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// Nearest hit of the ray `begin` + t·`dir` (t >= 0), or `None`.
    pub fn intersect_ray(&self, begin: Vector3, dir: Vector3) -> Option<TriangleMeshHit> {
        self.query(begin, dir, false)
    }

    /// Nearest hit of the segment `begin`–`end`, or `None`.
    ///
    /// Distinct from a ray with the same direction: a face beyond `end` is a
    /// miss. Sharing one implementation would make `intersect_segment` report
    /// hits the segment never reaches.
    pub fn intersect_segment(&self, begin: Vector3, end: Vector3) -> Option<TriangleMeshHit> {
        self.query(begin, end - begin, true)
    }

    /// Shared traversal. `bounded` limits the hit to `dir`'s length (segment
    /// semantics); otherwise `dir` is an unbounded direction.
    fn query(&self, begin: Vector3, dir: Vector3, bounded: bool) -> Option<TriangleMeshHit> {
        if self.nodes.is_empty() || dir.length_squared() <= 0.0 {
            return None;
        }

        let mut best: Option<(f32, TriangleMeshHit)> = None;
        // Explicit stack: a mesh deep enough to matter is also deep enough that
        // recursion here would be a stack-depth risk in a hot query path.
        let mut stack = vec![0u32];
        while let Some(idx) = stack.pop() {
            let node = self.nodes[idx as usize];
            // Prune against the best distance so far, so a nearer hit found
            // early spares whole subtrees.
            let limit = best.as_ref().map(|(d, _)| *d);
            if !ray_hits_aabb(begin, dir, node.min, node.max, bounded, limit) {
                continue;
            }
            if node.count == 0 {
                stack.push(node.left);
                stack.push(node.right);
                continue;
            }
            for slot in node.start..node.start + node.count {
                let fi = self.order[slot as usize] as usize;
                let [v0, v1, v2] = self.faces[fi];
                let point = if bounded {
                    segment_intersects_triangle(begin, begin + dir, v0, v1, v2)
                } else {
                    ray_intersects_triangle(begin, dir, v0, v1, v2)
                };
                let Some(point) = point else { continue };
                // Rank by distance from the origin, not by the ray parameter —
                // `dir` need not be normalised, so `t` is not a distance.
                let d = (point - begin).length_squared();
                if best.as_ref().map_or(true, |(bd, _)| d < *bd) {
                    best = Some((
                        d,
                        TriangleMeshHit {
                            position: point,
                            normal: face_normal(v0, v1, v2),
                            face_index: fi,
                        },
                    ));
                }
            }
        }
        best.map(|(_, hit)| hit)
    }

    /// Builds the node array over `faces`.
    fn build(&mut self) {
        self.order = (0..self.faces.len() as u32).collect();
        self.nodes.clear();
        if self.faces.is_empty() {
            return;
        }
        let len = self.order.len() as u32;
        self.build_node(0, len, 0);
    }

    /// Builds the subtree covering `order[start..start+count]` and returns its
    /// node index.
    fn build_node(&mut self, start: u32, count: u32, depth: u32) -> u32 {
        let (min, max) = self.bounds_of(start, count);
        let idx = self.nodes.len() as u32;
        self.nodes.push(BvhNode {
            min,
            max,
            start,
            count,
            left: 0,
            right: 0,
        });

        if count as usize <= MAX_LEAF_FACES || depth >= MAX_DEPTH {
            return idx;
        }

        // Split on the widest axis of the CENTROID bounds, not of the face
        // bounds: a few large faces would otherwise dominate the extent and
        // push every split onto an axis that separates nothing.
        let (cmin, cmax) = self.centroid_bounds_of(start, count);
        let extent = cmax - cmin;
        let axis = if extent.x >= extent.y && extent.x >= extent.z {
            0
        } else if extent.y >= extent.z {
            1
        } else {
            2
        };

        let slice = &mut self.order[start as usize..(start + count) as usize];
        let faces = &self.faces;
        slice.sort_unstable_by(|a, b| {
            let ca = axis_of(centroid(&faces[*a as usize]), axis);
            let cb = axis_of(centroid(&faces[*b as usize]), axis);
            ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
        });

        let half = count / 2;
        // A median split of coincident centroids can put everything on one
        // side; `half` is taken from the count rather than from a spatial
        // midpoint precisely so both children are always non-empty and the
        // recursion terminates.
        let left = self.build_node(start, half, depth + 1);
        let right = self.build_node(start + half, count - half, depth + 1);
        self.nodes[idx as usize].left = left;
        self.nodes[idx as usize].right = right;
        self.nodes[idx as usize].count = 0;
        idx
    }

    fn bounds_of(&self, start: u32, count: u32) -> (Vector3, Vector3) {
        let mut min = Vector3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let mut max = Vector3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for slot in start..start + count {
            for v in self.faces[self.order[slot as usize] as usize] {
                min = Vector3::new(min.x.min(v.x), min.y.min(v.y), min.z.min(v.z));
                max = Vector3::new(max.x.max(v.x), max.y.max(v.y), max.z.max(v.z));
            }
        }
        (min, max)
    }

    fn centroid_bounds_of(&self, start: u32, count: u32) -> (Vector3, Vector3) {
        let mut min = Vector3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let mut max = Vector3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for slot in start..start + count {
            let c = centroid(&self.faces[self.order[slot as usize] as usize]);
            min = Vector3::new(min.x.min(c.x), min.y.min(c.y), min.z.min(c.z));
            max = Vector3::new(max.x.max(c.x), max.y.max(c.y), max.z.max(c.z));
        }
        (min, max)
    }
}

fn centroid(f: &[Vector3; 3]) -> Vector3 {
    (f[0] + f[1] + f[2]) * (1.0 / 3.0)
}

fn axis_of(v: Vector3, axis: u8) -> f32 {
    match axis {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

/// Geometric normal from winding order, matching Godot's `Face3::get_plane`
/// convention.
fn face_normal(v0: Vector3, v1: Vector3, v2: Vector3) -> Vector3 {
    (v1 - v0).cross(v2 - v0).normalized()
}

/// Slab test. `bounded` clamps the ray to `dir`'s length; `limit` is the best
/// squared hit distance so far, used to prune.
///
/// Division by a zero direction component is deliberate rather than guarded:
/// IEEE gives ±infinity, which makes the slab comparisons resolve correctly for
/// a ray parallel to that axis. The one case infinity gets wrong is a zero
/// component exactly on a slab boundary (0 · ∞ = NaN), so that is handled
/// explicitly below.
fn ray_hits_aabb(
    from: Vector3,
    dir: Vector3,
    min: Vector3,
    max: Vector3,
    bounded: bool,
    limit: Option<f32>,
) -> bool {
    let mut tmin = 0.0f32;
    let mut tmax = if bounded { 1.0f32 } else { f32::INFINITY };

    for axis in 0..3u8 {
        let o = axis_of(from, axis);
        let d = axis_of(dir, axis);
        let lo = axis_of(min, axis);
        let hi = axis_of(max, axis);

        if d == 0.0 {
            // Parallel to this slab: a miss only if the origin is outside it.
            if o < lo || o > hi {
                return false;
            }
            continue;
        }
        let inv = 1.0 / d;
        let (mut t0, mut t1) = ((lo - o) * inv, (hi - o) * inv);
        if t0 > t1 {
            std::mem::swap(&mut t0, &mut t1);
        }
        tmin = tmin.max(t0);
        tmax = tmax.min(t1);
        if tmin > tmax {
            return false;
        }
    }

    // Prune: `limit` is a SQUARED distance, and tmin is in units of `dir`, so
    // the entry point has to be converted before they can be compared.
    if let Some(best_sq) = limit {
        let entry_sq = (dir * tmin).length_squared();
        if entry_sq > best_sq {
            return false;
        }
    }
    true
}

impl Mesh3D {
    /// Builds a [`TriangleMesh`] over every triangle in this mesh, across all
    /// surfaces. Mirrors `Mesh.generate_triangle_mesh()`.
    ///
    /// Returns `None` when the mesh contributes no triangles, matching Godot,
    /// which hands back a null ref rather than an empty acceleration structure.
    ///
    /// ⚠️ Unlike the existing `Mesh3D::aabb()` and `triangle_count()`, which
    /// look only at the primary surface, this walks `surfaces` too — a
    /// multi-surface mesh whose extra surfaces were ignored would raycast
    /// against a hole where geometry visibly is.
    pub fn generate_triangle_mesh(&self) -> Option<TriangleMesh> {
        let mut faces = Vec::new();
        collect_faces(
            &self.vertices,
            &self.indices,
            self.primitive_type,
            &mut faces,
        );
        for s in &self.surfaces {
            collect_faces(&s.vertices, &s.indices, s.primitive_type, &mut faces);
        }
        if faces.is_empty() {
            return None;
        }
        Some(TriangleMesh::from_faces(faces))
    }
}

/// Appends one surface's triangles. Non-triangle surfaces contribute nothing —
/// a line or point list has no face to hit, and treating its vertices as
/// triangles would fabricate geometry that is not rendered.
fn collect_faces(
    vertices: &[Vector3],
    indices: &[u32],
    primitive_type: PrimitiveType,
    out: &mut Vec<[Vector3; 3]>,
) {
    if primitive_type != PrimitiveType::Triangles {
        return;
    }
    if indices.is_empty() {
        // Non-indexed: consecutive triples.
        for c in vertices.chunks_exact(3) {
            out.push([c[0], c[1], c[2]]);
        }
        return;
    }
    for t in indices.chunks_exact(3) {
        let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
        // An out-of-range index is skipped rather than panicking: mesh data can
        // arrive from an importer, and one bad triangle must not take down a
        // raycast against the rest of the model.
        if a < vertices.len() && b < vertices.len() && c < vertices.len() {
            out.push([vertices[a], vertices[b], vertices[c]]);
        }
    }
}

//! 3D mesh data structures and primitive constructors.
//!
//! Provides `Mesh3D` for storing vertex data and `PrimitiveType` for
//! specifying how vertices are interpreted by the rendering pipeline.

use gdcore::math::Vector3;
use gdcore::math3d::Aabb;

/// How vertices are interpreted during rasterization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrimitiveType {
    /// Every three vertices form a triangle.
    #[default]
    Triangles,
    /// Every two vertices form a line segment.
    Lines,
    /// Each vertex is rendered as a point.
    Points,
}

/// A single surface within a 3D mesh.
///
/// In Godot, a mesh can contain multiple surfaces, each with its own
/// vertex data and default material. Surface indices are used by
/// `MeshInstance3D::set_surface_override_material()` for per-surface
/// material assignment.
#[derive(Debug, Clone, PartialEq)]
pub struct Surface3D {
    /// Vertex positions.
    pub vertices: Vec<Vector3>,
    /// Per-vertex normals.
    pub normals: Vec<Vector3>,
    /// Per-vertex UV coordinates.
    pub uvs: Vec<[f32; 2]>,
    /// Triangle/line/point indices into the vertex arrays.
    pub indices: Vec<u32>,
    /// How to interpret the vertex data.
    pub primitive_type: PrimitiveType,
    /// Coarser index buffers, ordered from finest to coarsest.
    ///
    /// Empty means "no LOD" — draw `indices` always, which is what every
    /// surface did before pat-s3rrv.205.
    pub lods: Vec<SurfaceLod>,
    /// Per-vertex bone weights, parallel to `vertices`; empty when unrigged.
    ///
    /// ⚠️ Every surface of a skinned Godot mesh carries its own
    /// `ARRAY_BONES`/`ARRAY_WEIGHTS`. Keeping influences only on the primary
    /// surface left every other surface at its bind-space coordinates while the
    /// first one followed the skeleton — the platformer robot's second
    /// primitive drew at 0.3x scale and unposed, which read as a crumpled
    /// model (pat-ghw2s).
    pub bone_weights: Vec<[f32; 4]>,
    /// Per-vertex bind indices (into `Skin::binds`), parallel to `vertices`.
    pub bone_indices: Vec<[u32; 4]>,
}

/// Smoothing-group value meaning "do not smooth" — Godot's `UINT32_MAX`.
///
/// ⚠️ NOT the default. Godot defaults `last_smooth_group` to 0, a real group,
/// so meshes smooth unless a script opts out.
pub const NO_SMOOTH_GROUP: u32 = u32::MAX;

/// Procedural mesh builder mirroring Godot's `SurfaceTool`.
///
/// Per-vertex attributes are staged with [`set_normal`](Self::set_normal) /
/// [`set_uv`](Self::set_uv) and committed by [`add_vertex`](Self::add_vertex)
/// (which snapshots the current attributes), matching Godot's imperative API;
/// [`commit`](Self::commit) produces a [`Mesh3D`]. This is the step-by-step
/// counterpart to [`crate::primitive_mesh::ArrayMesh::add_surface_from_arrays`].
#[derive(Debug, Clone)]
pub struct SurfaceTool {
    primitive_type: PrimitiveType,
    vertices: Vec<Vector3>,
    normals: Vec<Vector3>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    current_normal: Vector3,
    current_uv: [f32; 2],
    /// Per-vertex colours staged by `set_color` (pat-s3rrv.201).
    ///
    /// ⚠️ Kept on the tool but NOT carried into [`Mesh3D`], which has no colour
    /// channel. Storing them is still right: `set_color` is ClassDB-registered,
    /// so a script may call it, and dropping the value at the setter would lose
    /// it before anything could ever use it. Wiring colours through to the
    /// renderer needs a `colors` field on `Mesh3D`, which is a workspace-wide
    /// change (73 struct literals, many without `..Default`).
    colors: Vec<gdcore::math::Color>,
    current_color: gdcore::math::Color,
    /// Per-vertex skin influences, staged like every other attribute.
    bone_indices: Vec<[u32; 4]>,
    bone_weights: Vec<[f32; 4]>,
    current_bones: [u32; 4],
    current_weights: [f32; 4],
    /// Per-vertex smoothing group. `NO_SMOOTH_GROUP` means flat-shade.
    smooth_groups: Vec<u32>,
    current_smooth_group: u32,
    /// Whether the script ever called `set_bones`/`set_weights`.
    ///
    /// ⚠️ Load-bearing. [`Mesh3D::is_skinned`] is true whenever the bone arrays
    /// merely MATCH the vertex count, and `render_server_3d` gates skinning on
    /// it. Emitting default bone data for every vertex would therefore make
    /// every procedural mesh report itself skinned, and the render path would
    /// deform it against whatever skeleton resolved — with all-zero weights,
    /// collapsing it to the origin. Godot avoids this with its
    /// `ARRAY_FORMAT_BONES` flag; this is that flag.
    has_skin: bool,
}

impl SurfaceTool {
    /// Creates an empty builder (defaults to a `Triangles` surface).
    pub fn new() -> Self {
        Self {
            primitive_type: PrimitiveType::Triangles,
            vertices: Vec::new(),
            normals: Vec::new(),
            uvs: Vec::new(),
            indices: Vec::new(),
            current_normal: Vector3::ZERO,
            current_uv: [0.0, 0.0],
            colors: Vec::new(),
            bone_indices: Vec::new(),
            bone_weights: Vec::new(),
            current_bones: [0; 4],
            current_weights: [0.0; 4],
            smooth_groups: Vec::new(),
            // ⚠️ Godot's default is 0 — a REAL group — so `generate_normals()`
            // smooths by default. Defaulting to NO_SMOOTH_GROUP here would
            // flat-shade every mesh and silently diverge from the engine.
            current_smooth_group: 0,
            has_skin: false,
            // Godot stages white until `set_color` says otherwise, so an
            // uncoloured vertex is opaque white rather than transparent black.
            current_color: gdcore::math::Color::WHITE,
        }
    }

    /// Clears accumulated geometry and sets the primitive type
    /// (`SurfaceTool.begin`).
    pub fn begin(&mut self, primitive_type: PrimitiveType) {
        self.primitive_type = primitive_type;
        self.vertices.clear();
        self.normals.clear();
        self.uvs.clear();
        self.indices.clear();
        self.current_normal = Vector3::ZERO;
        self.current_uv = [0.0, 0.0];
    }

    /// Sets the normal applied to vertices added afterwards.
    pub fn set_normal(&mut self, normal: Vector3) {
        self.current_normal = normal;
    }

    /// Sets the UV applied to vertices added afterwards.
    pub fn set_uv(&mut self, uv: [f32; 2]) {
        self.current_uv = uv;
    }

    /// Adds a vertex carrying the currently-staged normal and UV
    /// (`SurfaceTool.add_vertex`).
    pub fn add_vertex(&mut self, position: Vector3) {
        self.vertices.push(position);
        self.normals.push(self.current_normal);
        self.uvs.push(self.current_uv);
        // ⚠️ Pushed here, not in `set_color`. These arrays are read BY INDEX
        // alongside `vertices`, so one that advances on a different event
        // desynchronises the moment a script sets a colour twice, or none at
        // all, before a vertex.
        self.colors.push(self.current_color);
        // Pushed unconditionally to stay in lockstep with `vertices`; whether
        // they are EMITTED is decided by `has_skin` at commit time.
        self.bone_indices.push(self.current_bones);
        self.bone_weights.push(self.current_weights);
        self.smooth_groups.push(self.current_smooth_group);
    }

    /// Adds an explicit index into the vertex array (`SurfaceTool.add_index`).
    /// Number of vertices staged so far.
    ///
    /// Restored after the same bad splice that removed `add_index`. ⚠️ A name
    /// grep said `vertex_count` still existed — but the survivor belongs to
    /// `impl Mesh3D`, a different type. Only impl attribution shows the loss.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Adds an explicit index into the vertex array.
    ///
    /// Restored after a bad splice removed it (pat-s3rrv.201); `scripting.rs`
    /// dispatches `add_index` and would not have compiled without it.
    pub fn add_index(&mut self, index: u32) {
        self.indices.push(index);
    }

    /// Stages the bone influences applied to subsequent vertices.
    ///
    /// Godot takes a `PackedInt32Array`; entries past the fourth are ignored,
    /// and a short array leaves the remaining slots at zero.
    pub fn set_bones(&mut self, bones: &[i32]) {
        self.current_bones = [0; 4];
        for (slot, &b) in self.current_bones.iter_mut().zip(bones) {
            *slot = b.max(0) as u32;
        }
        self.has_skin = true;
    }

    /// Stages the bone weights applied to subsequent vertices.
    pub fn set_weights(&mut self, weights: &[f32]) {
        self.current_weights = [0.0; 4];
        for (slot, &w) in self.current_weights.iter_mut().zip(weights) {
            *slot = w;
        }
        self.has_skin = true;
    }

    /// Stages the colour applied to subsequent vertices, as Godot's
    /// `SurfaceTool.set_color` does.
    pub fn set_color(&mut self, color: gdcore::math::Color) {
        self.current_color = color;
    }

    /// Colours staged so far, one per vertex added since `begin`.
    pub fn colors(&self) -> &[gdcore::math::Color] {
        &self.colors
    }

    /// Discards everything, returning the tool to its pre-`begin` state.
    ///
    /// Godot's `clear()` also resets the staged attributes, not just the
    /// accumulated arrays — a colour set before `clear` must not leak onto the
    /// first vertex of the next surface.
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.normals.clear();
        self.uvs.clear();
        self.colors.clear();
        self.indices.clear();
        self.current_normal = Vector3::ZERO;
        self.current_uv = [0.0, 0.0];
        self.current_color = gdcore::math::Color::WHITE;
        self.bone_indices.clear();
        self.bone_weights.clear();
        self.smooth_groups.clear();
        self.current_smooth_group = 0;
        self.current_bones = [0; 4];
        self.current_weights = [0.0; 4];
        self.has_skin = false;
    }

    /// Emits a triangle fan as individual triangles.
    ///
    /// Ported from `SurfaceTool::add_triangle_fan`. Godot expands the fan on the
    /// spot rather than storing a fan primitive: for `n` points it emits `n - 2`
    /// triangles, each `(0, i+1, i+2)`.
    ///
    /// ⚠️ Vertex 0 is therefore re-emitted for **every** triangle — the fan's
    /// hub is duplicated `n - 2` times, so `n` points produce `(n - 2) * 3`
    /// vertices, not `n`. Expecting `n` is the natural guess and it is wrong.
    ///
    /// Per-point attributes are indexed by the same index as the vertex, so the
    /// hub's colour and UV are re-applied on each repeat. Shorter attribute
    /// slices simply leave the staged value in place, exactly as upstream's
    /// `if (p_colors.size() > n)` guards do.
    ///
    /// Returns false without emitting anything when the surface is not
    /// TRIANGLES or there are fewer than three points, mirroring Godot's
    /// `ERR_FAIL_COND`s — a partial fan would be worse than none.
    pub fn add_triangle_fan(
        &mut self,
        vertices: &[Vector3],
        uvs: &[[f32; 2]],
        colors: &[gdcore::math::Color],
        normals: &[Vector3],
    ) -> bool {
        if self.primitive_type != PrimitiveType::Triangles || vertices.len() < 3 {
            return false;
        }

        for i in 0..vertices.len() - 2 {
            for n in [0usize, i + 1, i + 2] {
                if let Some(&c) = colors.get(n) {
                    self.set_color(c);
                }
                if let Some(&uv) = uvs.get(n) {
                    self.set_uv(uv);
                }
                if let Some(&nm) = normals.get(n) {
                    self.set_normal(nm);
                }
                self.add_vertex(vertices[n]);
            }
        }
        true
    }

    /// Appends another mesh's geometry, transformed.
    ///
    /// Ported from `SurfaceTool::append_from`. Three details matter and only
    /// one is obvious:
    ///
    /// 1. Vertices take the **full** transform.
    /// 2. ⚠️ Normals take the **basis only** — `p_xform.basis.xform(v.normal)`
    ///    upstream. A normal is a direction; putting it through the full
    ///    transform adds the translation, so any appended mesh at a non-zero
    ///    offset would light as though its surface faced the world origin.
    /// 3. Indices are offset by the vertex count already accumulated,
    ///    otherwise the appended triangles index into the FIRST mesh's vertices.
    ///
    /// An empty tool adopts the source's primitive type, as Godot does.
    ///
    /// ⚠️ Matching upstream: if this tool already holds vertices but no index
    /// buffer, those first vertices' implicit indices are not materialised, so
    /// the resulting index array covers only the appended part. Godot has the
    /// same edge (it warns "Index array not a multiple of 3"). Kept identical
    /// rather than quietly improved, so behaviour matches the engine we port.
    pub fn append_from(&mut self, mesh: &Mesh3D, xform: gdcore::math3d::Transform3D) {
        if self.vertices.is_empty() {
            self.primitive_type = mesh.primitive_type;
        }
        let vfrom = self.vertices.len() as u32;

        for (i, &v) in mesh.vertices.iter().enumerate() {
            self.vertices.push(xform.xform(v));
            let n = mesh.normals.get(i).copied().unwrap_or(Vector3::ZERO);
            // A zero normal carries no direction; rotating it is meaningless
            // and a scaled basis would leave it zero anyway.
            self.normals.push(if n == Vector3::ZERO {
                n
            } else {
                xform.basis.xform(n)
            });
            self.uvs
                .push(mesh.uvs.get(i).copied().unwrap_or([0.0, 0.0]));
            self.colors.push(gdcore::math::Color::WHITE);
        }

        let source: Vec<u32> = if mesh.indices.is_empty() {
            (0..mesh.vertices.len() as u32).collect()
        } else {
            mesh.indices.clone()
        };
        for i in source {
            self.indices.push(i + vfrom);
        }
    }

    /// Replaces this tool's contents with another mesh's geometry.
    ///
    /// Godot's `create_from(mesh, surface)` — a clear followed by an untransformed
    /// append, which is exactly how upstream implements it.
    pub fn create_from(&mut self, mesh: &Mesh3D) {
        self.clear();
        self.append_from(mesh, gdcore::math3d::Transform3D::IDENTITY);
    }

    /// Collapses identical vertices and builds an index buffer.
    ///
    /// Ported from `SurfaceTool::index` in
    /// `upstream/godot/scene/resources/surface_tool.cpp`.
    ///
    /// ⚠️ Two vertices merge only when **every** attribute matches — position,
    /// normal, UV and colour. Deduping on position alone would weld the seam of
    /// any UV-unwrapped mesh, where the same corner legitimately carries two
    /// different UVs, and the texture would tear there.
    ///
    /// Godot returns early if the surface is already indexed, so calling this
    /// twice is not an error and does not re-index an existing buffer.
    pub fn index(&mut self) {
        if !self.indices.is_empty() {
            return;
        }

        // Bit patterns rather than float equality: this is Godot's "identical
        // vertex" test, not a weld with a tolerance, so 0.1 + 0.2 must not
        // match 0.3.
        type Key = ([u32; 3], [u32; 3], [u32; 2], [u32; 4]);
        let mut seen: std::collections::HashMap<Key, u32> = std::collections::HashMap::new();
        let mut unique_v: Vec<Vector3> = Vec::new();
        let mut unique_n: Vec<Vector3> = Vec::new();
        let mut unique_uv: Vec<[f32; 2]> = Vec::new();
        let mut unique_c: Vec<gdcore::math::Color> = Vec::new();
        let mut indices: Vec<u32> = Vec::with_capacity(self.vertices.len());

        for i in 0..self.vertices.len() {
            let v = self.vertices[i];
            let n = self.normals.get(i).copied().unwrap_or(Vector3::ZERO);
            let uv = self.uvs.get(i).copied().unwrap_or([0.0, 0.0]);
            let c = self
                .colors
                .get(i)
                .copied()
                .unwrap_or(gdcore::math::Color::WHITE);
            let key: Key = (
                [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()],
                [n.x.to_bits(), n.y.to_bits(), n.z.to_bits()],
                [uv[0].to_bits(), uv[1].to_bits()],
                [c.r.to_bits(), c.g.to_bits(), c.b.to_bits(), c.a.to_bits()],
            );

            let idx = match seen.get(&key) {
                Some(&existing) => existing,
                None => {
                    let fresh = unique_v.len() as u32;
                    seen.insert(key, fresh);
                    unique_v.push(v);
                    unique_n.push(n);
                    unique_uv.push(uv);
                    unique_c.push(c);
                    fresh
                }
            };
            indices.push(idx);
        }

        self.vertices = unique_v;
        self.normals = unique_n;
        self.uvs = unique_uv;
        self.colors = unique_c;
        self.indices = indices;
    }

    /// Expands an indexed surface back into a flat vertex list.
    ///
    /// Ported from `SurfaceTool::deindex`. Godot returns early when there is no
    /// index buffer — "nothing to deindex" — rather than clearing the vertices,
    /// which is what a naive implementation does and which silently empties a
    /// perfectly good non-indexed surface.
    pub fn deindex(&mut self) {
        if self.indices.is_empty() {
            return;
        }

        let old_v = std::mem::take(&mut self.vertices);
        let old_n = std::mem::take(&mut self.normals);
        let old_uv = std::mem::take(&mut self.uvs);
        let old_c = std::mem::take(&mut self.colors);
        let old_bi = std::mem::take(&mut self.bone_indices);
        let old_bw = std::mem::take(&mut self.bone_weights);
        let old_sg = std::mem::take(&mut self.smooth_groups);

        for &i in &self.indices {
            let i = i as usize;
            // An out-of-range index is malformed input; Godot ERR_FAIL_CONDs the
            // whole call. Skipping the entry keeps the parallel arrays the same
            // length as each other, which a partial push would not.
            let (Some(&v), Some(&n)) = (old_v.get(i), old_n.get(i)) else {
                continue;
            };
            self.vertices.push(v);
            self.normals.push(n);
            self.uvs.push(old_uv.get(i).copied().unwrap_or([0.0, 0.0]));
            self.colors
                .push(old_c.get(i).copied().unwrap_or(gdcore::math::Color::WHITE));
            self.bone_indices
                .push(old_bi.get(i).copied().unwrap_or([0; 4]));
            self.bone_weights
                .push(old_bw.get(i).copied().unwrap_or([0.0; 4]));
            self.smooth_groups.push(old_sg.get(i).copied().unwrap_or(0));
        }
        self.indices.clear();
    }

    /// Stages the smoothing group for subsequent vertices.
    ///
    /// [`NO_SMOOTH_GROUP`] flat-shades. Any other value groups vertices that
    /// share a position, so `generate_normals` averages the faces meeting there.
    pub fn set_smooth_group(&mut self, group: u32) {
        self.current_smooth_group = group;
    }

    /// Recomputes vertex normals, honouring smoothing groups.
    ///
    /// Ported from `SurfaceTool::generate_normals`. Three things upstream does
    /// that a from-the-docs reading misses:
    ///
    /// 1. ⚠️ **It deindexes first.** Without that, a vertex shared by two
    ///    triangles is written twice and the LAST triangle silently wins,
    ///    instead of each face getting its own copy.
    /// 2. ⚠️ **The default smoothing group is 0, a real group** — so the
    ///    default behaviour is to SMOOTH: face normals are accumulated per
    ///    (position, group) and normalised. Flat shading is the opt-in, via
    ///    [`NO_SMOOTH_GROUP`]. Defaulting to flat looks reasonable and diverges
    ///    from Godot on every mesh whose faces meet at an angle.
    /// 3. `flip` reverses the triangle's winding rather than negating the
    ///    result — equivalent for a plane normal, and this keeps the shape of
    ///    the upstream code.
    ///
    /// A non-TRIANGLES surface, or a vertex count that is not a multiple of
    /// three, is refused as upstream `ERR_FAIL_COND`s.
    pub fn generate_normals(&mut self, flip: bool) {
        if self.primitive_type != PrimitiveType::Triangles {
            return;
        }
        self.deindex();
        if self.vertices.is_empty() || !self.vertices.len().is_multiple_of(3) {
            return;
        }

        self.normals = vec![Vector3::ZERO; self.vertices.len()];
        // Keyed on the quantised position plus the group, matching Godot's
        // SmoothGroupVertex hash: two vertices smooth together only if they
        // coincide AND share a group.
        let mut smooth: std::collections::HashMap<([u32; 3], u32), Vector3> =
            std::collections::HashMap::new();

        for tri in 0..self.vertices.len() / 3 {
            let (a, b, c) = (tri * 3, tri * 3 + 1, tri * 3 + 2);
            let (pa, pb, pc) = (self.vertices[a], self.vertices[b], self.vertices[c]);
            let raw = if flip {
                (pb - pc).cross(pa - pc)
            } else {
                (pb - pa).cross(pc - pa)
            };
            let len = raw.length();
            // A degenerate triangle has no defined normal; normalising it would
            // write NaN, which propagates and blackens the surface.
            let normal = if len <= f32::EPSILON {
                Vector3::ZERO
            } else {
                raw / len
            };

            for i in [a, b, c] {
                let group = self.smooth_groups.get(i).copied().unwrap_or(0);
                if group == NO_SMOOTH_GROUP {
                    self.normals[i] = normal;
                } else {
                    let p = self.vertices[i];
                    let key = ([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()], group);
                    // ⚠️ `entry()` holds a mutable borrow; reading `smooth`
                    // again in the same expression is a second borrow and
                    // does not compile. Bind the slot, then add through it.
                    let slot = smooth.entry(key).or_insert(Vector3::ZERO);
                    *slot = *slot + normal;
                }
            }
        }

        for i in 0..self.vertices.len() {
            let group = self.smooth_groups.get(i).copied().unwrap_or(0);
            if group == NO_SMOOTH_GROUP {
                continue;
            }
            let p = self.vertices[i];
            let key = ([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()], group);
            let sum = smooth.get(&key).copied().unwrap_or(Vector3::ZERO);
            let len = sum.length();
            self.normals[i] = if len <= f32::EPSILON {
                Vector3::ZERO
            } else {
                sum / len
            };
        }
    }

    /// Builds a [`Mesh3D`] from the staged geometry. When no explicit indices
    /// were added, a sequential index per vertex is generated so the surface is
    /// renderable as-is (`SurfaceTool.commit`).
    pub fn commit(&self) -> Mesh3D {
        let mut mesh = Mesh3D::new(self.primitive_type);
        mesh.vertices = self.vertices.clone();
        mesh.normals = self.normals.clone();
        mesh.uvs = self.uvs.clone();
        // ⚠️ Only when the script actually set skin data. See `has_skin`.
        if self.has_skin {
            mesh.bone_indices = self.bone_indices.clone();
            mesh.bone_weights = self.bone_weights.clone();
        }
        mesh.indices = if self.indices.is_empty() {
            (0..self.vertices.len() as u32).collect()
        } else {
            self.indices.clone()
        };
        mesh
    }
}

impl Default for SurfaceTool {
    fn default() -> Self {
        Self::new()
    }
}

/// One level of detail for a surface: a coarser index buffer plus the
/// screen-space error at which it becomes acceptable (pat-s3rrv.205).
///
/// Godot stores LOD index arrays per surface and picks the coarsest level whose
/// error is still under `Viewport.mesh_lod_threshold`, scaled by the instance's
/// `lod_bias`.
///
/// ⚠️ `error` is a **screen-space** quantity in pixels, not a world-space
/// distance — the same units as `mesh_lod_threshold`. Comparing it against a
/// camera distance would be wrong at every field of view, and the mistake is
/// invisible at the one FOV a fixture happens to use.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SurfaceLod {
    /// Screen-space error, in pixels, this level introduces.
    pub error: f32,
    /// The index buffer to draw when this level is selected.
    pub indices: Vec<u32>,
}

impl Surface3D {
    /// Whether this surface carries bone influences parallel to its vertices.
    pub fn is_skinned(&self) -> bool {
        !self.vertices.is_empty()
            && self.bone_weights.len() == self.vertices.len()
            && self.bone_indices.len() == self.vertices.len()
    }

    /// Chooses the index buffer to draw for a given screen-space error budget.
    ///
    /// Mirrors Godot: take the **coarsest** level whose error still fits the
    /// budget. `threshold` is `Viewport.mesh_lod_threshold` already scaled by
    /// the instance's `lod_bias`, in pixels.
    ///
    /// ⚠️ Returns the full-detail `indices` when nothing qualifies — including
    /// when `lods` is empty, which is every surface that predates LOD support.
    /// Returning an empty slice there would make existing meshes vanish rather
    /// than merely stop switching.
    ///
    /// ⚠️ A threshold of 0 (or negative) means "never degrade": no level can
    /// have an error below it, so full detail is correct. That is also what a
    /// caller gets if it forgets to set the viewport threshold, which should
    /// look like today's behaviour rather than like maximum degradation.
    pub fn select_lod(&self, threshold: f32) -> &[u32] {
        let mut chosen: Option<&SurfaceLod> = None;
        for lod in &self.lods {
            if lod.error <= threshold && !lod.indices.is_empty() {
                // Coarsest wins, so keep the largest error that still fits.
                if chosen.is_none_or(|c| lod.error > c.error) {
                    chosen = Some(lod);
                }
            }
        }
        chosen.map_or(self.indices.as_slice(), |l| l.indices.as_slice())
    }
}

/// A 3D mesh containing vertex data, optionally split into multiple surfaces.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mesh3D {
    /// Vertex positions.
    pub vertices: Vec<Vector3>,
    /// Per-vertex normals.
    pub normals: Vec<Vector3>,
    /// Per-vertex UV coordinates.
    pub uvs: Vec<[f32; 2]>,
    /// Triangle/line/point indices into the vertex arrays.
    pub indices: Vec<u32>,
    /// How to interpret the vertex data.
    pub primitive_type: PrimitiveType,
    /// Additional surfaces beyond the primary one.
    ///
    /// When empty, the mesh is treated as a single-surface mesh using the
    /// top-level vertex/normal/uv/index data. When populated, each entry
    /// represents a distinct surface that can receive its own material
    /// override in MeshInstance3D.
    pub surfaces: Vec<Surface3D>,
    /// Per-vertex bone weights for skeletal skinning (up to 4 influences).
    ///
    /// Parallel to `vertices`. Each `[f32; 4]` holds the blend weights for the
    /// four bones referenced by the matching `bone_indices` entry; weights are
    /// normally normalized to sum to `1.0`. Empty when the mesh is not rigged.
    /// See [`crate::skin`] for the skinning pipeline.
    pub bone_weights: Vec<[f32; 4]>,
    /// Per-vertex bone indices for skeletal skinning (up to 4 influences).
    ///
    /// Parallel to `vertices`. Each `[u32; 4]` selects up to four binds in a
    /// [`crate::skin::Skin`] (indices into `Skin::binds`, matching Godot's
    /// `Skin` indirection — not raw skeleton bone indices). Empty when the
    /// mesh is not rigged.
    pub bone_indices: Vec<[u32; 4]>,
    /// Named blend shapes (Godot's morph targets), empty for a mesh without
    /// any.
    ///
    /// Populated by the glTF importer from `primitives[].targets`
    /// (pat-s3rrv.23). `gdrender3d::morph` has carried the blending maths and
    /// its unit tests all along with nothing able to supply it data; this is
    /// where the data now lives.
    pub blend_shapes: Vec<BlendShape3D>,
}

/// A named blend shape: per-vertex position deltas from the base mesh.
///
/// ⚠️ **Deltas, not absolute positions.** glTF morph-target accessors hold
/// displacements, and `gdrender3d::morph::apply_morph` adds them to the base.
/// Godot converts them to absolutes on import (`gltf_document.cpp:1817`,
/// `w_varr[l] = r_varr[l] + r_src_varr[l]`) because its ArrayMesh blend-shape
/// format stores absolutes — storing absolutes here instead would add the base
/// twice and read as a scale bug rather than a bad import.
///
/// A `vertex_deltas` shorter than the mesh's vertex array is legal and affects
/// only the vertices it covers, which is what Godot's pad-with-base amounts to:
/// a base position is a zero delta.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BlendShape3D {
    /// Blend-shape name. Godot addresses shapes by name, and glTF supplies it
    /// through `mesh.extras.targetNames`; `morph_<k>` is the fallback Godot
    /// uses when the file names none.
    pub name: String,
    /// Per-vertex position deltas, parallel to [`Mesh3D::vertices`].
    pub vertex_deltas: Vec<Vector3>,
}

impl Mesh3D {
    /// Creates an empty mesh with the given primitive type.
    pub fn new(primitive_type: PrimitiveType) -> Self {
        Self {
            vertices: Vec::new(),
            normals: Vec::new(),
            uvs: Vec::new(),
            indices: Vec::new(),
            primitive_type,
            surfaces: Vec::new(),
            bone_weights: Vec::new(),
            bone_indices: Vec::new(),
            blend_shapes: Vec::new(),
        }
    }

    /// Returns the number of vertices in this mesh.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// The local-space AABB enclosing every vertex (pat-nzgdu.211).
    ///
    /// `None` for a mesh with no vertices: an empty mesh has no bounds, and a
    /// zero-sized AABB at the origin would be a *lie* a culler would act on —
    /// it would test as a real (if tiny) box sitting at the model origin.
    ///
    /// ⚠️ Covers `vertices` only. Additional `surfaces` carry their own vertex
    /// arrays, so a multi-surface mesh's bounds are wider than this. The 3D
    /// renderer's draw list keeps geometry in the top-level arrays, which is
    /// what the occlusion cull reads; a caller that walks `surfaces` must merge
    /// their bounds in itself.
    pub fn aabb(&self) -> Option<Aabb> {
        let mut iter = self.vertices.iter();
        let first = *iter.next()?;
        let (mut min, mut max) = (first, first);
        for v in iter {
            min = Vector3::new(min.x.min(v.x), min.y.min(v.y), min.z.min(v.z));
            max = Vector3::new(max.x.max(v.x), max.y.max(v.y), max.z.max(v.z));
        }
        Some(Aabb::new(min, max - min))
    }

    /// Returns the number of triangles (only valid for `Triangles` primitive type).
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Returns true if this mesh carries per-vertex bone weights/indices for
    /// skeletal skinning (both arrays present and parallel to `vertices`).
    ///
    /// A skinned mesh can be deformed with [`crate::skin::skin_mesh`].
    ///
    /// True when the primary surface OR any additional surface is rigged: a
    /// mesh whose rig lives only on surface 1+ is still a skinned mesh, and
    /// gating on the primary alone would leave those surfaces unposed.
    pub fn is_skinned(&self) -> bool {
        self.primary_is_skinned() || self.surfaces.iter().any(Surface3D::is_skinned)
    }

    /// Whether the PRIMARY surface (the top-level arrays) is rigged.
    pub fn primary_is_skinned(&self) -> bool {
        !self.vertices.is_empty()
            && self.bone_weights.len() == self.vertices.len()
            && self.bone_indices.len() == self.vertices.len()
    }

    /// Generates a unit cube centered at the origin, scaled by `size`.
    pub fn cube(size: f32) -> Self {
        let h = size * 0.5;

        let mut vertices = Vec::with_capacity(24);
        let mut normals = Vec::with_capacity(24);
        let mut uvs = Vec::with_capacity(24);
        let mut indices = Vec::with_capacity(36);

        let faces: [(Vector3, Vector3, Vector3); 6] = [
            (
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, -1.0),
                Vector3::new(0.0, 1.0, 0.0),
            ),
            (
                Vector3::new(-1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, 1.0, 0.0),
            ),
            (
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            ),
            (
                Vector3::new(0.0, -1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, -1.0),
            ),
            (
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            ),
            (
                Vector3::new(0.0, 0.0, -1.0),
                Vector3::new(-1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            ),
        ];

        for (normal, u_dir, v_dir) in &faces {
            let base = vertices.len() as u32;
            let center = *normal * h;
            let u = *u_dir * h;
            let v = *v_dir * h;

            vertices.push(center - u - v);
            vertices.push(center + u - v);
            vertices.push(center + u + v);
            vertices.push(center - u + v);

            for _ in 0..4 {
                normals.push(*normal);
            }

            uvs.push([0.0, 0.0]);
            uvs.push([1.0, 0.0]);
            uvs.push([1.0, 1.0]);
            uvs.push([0.0, 1.0]);

            // 🔑 Wound CLOCKWISE as seen from outside, on EVERY face — Godot's
            // front-face convention (`doc/classes/ArrayMesh.xml:49`). The quad
            // `0,1,2 / 0,2,3` is counter-clockwise exactly when `u × v` points
            // along the outward normal, which held for the X and Z faces but not
            // the Y faces, so this mesh used to be half inside-out: under
            // back-face culling the top of a box was dropped and its BOTTOM face
            // (lit by a downward normal) drew in its place. Only the index order
            // changes; vertices and UVs are untouched.
            if u_dir.cross(*v_dir).dot(*normal) > 0.0 {
                indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
            } else {
                indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }

        Self {
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

    /// Generates a UV sphere centered at the origin.
    pub fn sphere(radius: f32, segments: u32) -> Self {
        let rings = segments;
        let sectors = segments;

        let mut vertices = Vec::new();
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();

        for r in 0..=rings {
            let phi = std::f32::consts::PI * r as f32 / rings as f32;
            let (sin_phi, cos_phi) = phi.sin_cos();

            for s in 0..=sectors {
                let theta = 2.0 * std::f32::consts::PI * s as f32 / sectors as f32;
                let (sin_theta, cos_theta) = theta.sin_cos();

                let x = cos_theta * sin_phi;
                let y = cos_phi;
                let z = sin_theta * sin_phi;

                let normal = Vector3::new(x, y, z);
                vertices.push(normal * radius);
                normals.push(normal);
                uvs.push([s as f32 / sectors as f32, r as f32 / rings as f32]);
            }
        }

        for r in 0..rings {
            for s in 0..sectors {
                let cur = r * (sectors + 1) + s;
                let next = cur + sectors + 1;

                indices.push(cur);
                indices.push(next);
                indices.push(cur + 1);
                indices.push(cur + 1);
                indices.push(next);
                indices.push(next + 1);
            }
        }

        Self {
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

    /// Generates a torus (ring) lying in the XZ plane, matching Godot's
    /// `TorusMesh`. `inner_radius` is the hole radius and `outer_radius` the
    /// outer edge; `ring_sides` segments go around the main ring and
    /// `ring_segments` around the tube's circular cross-section.
    pub fn torus(
        inner_radius: f32,
        outer_radius: f32,
        ring_sides: u32,
        ring_segments: u32,
    ) -> Self {
        let center_radius = (outer_radius + inner_radius) * 0.5;
        let tube_radius = (outer_radius - inner_radius) * 0.5;

        let mut vertices = Vec::new();
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();

        for i in 0..=ring_sides {
            let u = 2.0 * std::f32::consts::PI * i as f32 / ring_sides as f32;
            let (sin_u, cos_u) = u.sin_cos();
            for j in 0..=ring_segments {
                let v = 2.0 * std::f32::consts::PI * j as f32 / ring_segments as f32;
                let (sin_v, cos_v) = v.sin_cos();
                let ring = center_radius + tube_radius * cos_v;
                vertices.push(Vector3::new(
                    ring * cos_u,
                    tube_radius * sin_v,
                    ring * sin_u,
                ));
                normals.push(Vector3::new(cos_v * cos_u, sin_v, cos_v * sin_u));
                uvs.push([
                    i as f32 / ring_sides as f32,
                    j as f32 / ring_segments as f32,
                ]);
            }
        }

        let row = ring_segments + 1;
        for i in 0..ring_sides {
            for j in 0..ring_segments {
                let cur = i * row + j;
                let next = cur + row;
                indices.push(cur);
                indices.push(next);
                indices.push(cur + 1);
                indices.push(cur + 1);
                indices.push(next);
                indices.push(next + 1);
            }
        }

        Self {
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

    /// Generates a flat plane on the XZ axis centered at the origin.
    pub fn plane(size: f32) -> Self {
        let h = size * 0.5;
        let normal = Vector3::UP;

        Self {
            vertices: vec![
                Vector3::new(-h, 0.0, -h),
                Vector3::new(h, 0.0, -h),
                Vector3::new(h, 0.0, h),
                Vector3::new(-h, 0.0, h),
            ],
            normals: vec![normal; 4],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            indices: vec![0, 1, 2, 0, 2, 3],
            primitive_type: PrimitiveType::Triangles,
            surfaces: Vec::new(),
            bone_weights: Vec::new(),
            bone_indices: Vec::new(),
            blend_shapes: Vec::new(),
        }
    }

    /// Generates a capsule (cylinder with hemisphere caps) along the Y axis.
    ///
    /// `radius` controls the cap/cylinder radius, `height` is the total height
    /// including caps. The body section has `radial_segments` around the
    /// circumference and `rings` subdivisions along the shaft.
    pub fn capsule(radius: f32, height: f32, radial_segments: u32, rings: u32) -> Self {
        let mid_height = (height - 2.0 * radius).max(0.0);
        let half_mid = mid_height * 0.5;
        let cap_rings = (rings / 2).max(2);

        let mut vertices = Vec::new();
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();

        let total_rings = cap_rings * 2 + rings + 1;

        // Top hemisphere
        for r in 0..=cap_rings {
            let phi = std::f32::consts::FRAC_PI_2 * r as f32 / cap_rings as f32;
            let (sin_phi, cos_phi) = phi.sin_cos();
            let y = cos_phi * radius + half_mid;

            for s in 0..=radial_segments {
                let theta = 2.0 * std::f32::consts::PI * s as f32 / radial_segments as f32;
                let (sin_t, cos_t) = theta.sin_cos();

                let nx = cos_t * sin_phi;
                let nz = sin_t * sin_phi;
                let ny = cos_phi;

                vertices.push(Vector3::new(nx * radius, y, nz * radius));
                normals.push(Vector3::new(nx, ny, nz));
                uvs.push([
                    s as f32 / radial_segments as f32,
                    r as f32 / total_rings as f32,
                ]);
            }
        }

        // Cylinder body
        for r in 1..=rings {
            let t = r as f32 / (rings + 1) as f32;
            let y = half_mid - mid_height * t;

            for s in 0..=radial_segments {
                let theta = 2.0 * std::f32::consts::PI * s as f32 / radial_segments as f32;
                let (sin_t, cos_t) = theta.sin_cos();

                vertices.push(Vector3::new(cos_t * radius, y, sin_t * radius));
                normals.push(Vector3::new(cos_t, 0.0, sin_t));
                uvs.push([
                    s as f32 / radial_segments as f32,
                    (cap_rings + r) as f32 / total_rings as f32,
                ]);
            }
        }

        // Bottom hemisphere
        for r in 0..=cap_rings {
            let phi = std::f32::consts::FRAC_PI_2
                + std::f32::consts::FRAC_PI_2 * r as f32 / cap_rings as f32;
            let (sin_phi, cos_phi) = phi.sin_cos();
            let y = cos_phi * radius - half_mid;

            for s in 0..=radial_segments {
                let theta = 2.0 * std::f32::consts::PI * s as f32 / radial_segments as f32;
                let (sin_t, cos_t) = theta.sin_cos();

                let nx = cos_t * sin_phi;
                let nz = sin_t * sin_phi;
                let ny = cos_phi;

                vertices.push(Vector3::new(nx * radius, y, nz * radius));
                normals.push(Vector3::new(nx, ny, nz));
                uvs.push([
                    s as f32 / radial_segments as f32,
                    (cap_rings + rings + 1 + r) as f32 / total_rings as f32,
                ]);
            }
        }

        // Build triangle indices
        let row_len = radial_segments + 1;
        let total_rows = cap_rings + 1 + rings + cap_rings + 1;
        for r in 0..(total_rows - 1) {
            for s in 0..radial_segments {
                let cur = r * row_len + s;
                let next = cur + row_len;

                indices.push(cur);
                indices.push(next);
                indices.push(cur + 1);
                indices.push(cur + 1);
                indices.push(next);
                indices.push(next + 1);
            }
        }

        Self {
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

    /// Generates a cylinder along the Y axis centered at the origin.
    ///
    /// `top_radius` and `bottom_radius` control the radii at each end (set
    /// equal for a regular cylinder, set one to zero for a cone).
    /// `height` is the full height. Caps are generated when the corresponding
    /// radius is > 0.
    pub fn cylinder(
        top_radius: f32,
        bottom_radius: f32,
        height: f32,
        radial_segments: u32,
        rings: u32,
    ) -> Self {
        let half_h = height * 0.5;

        let mut vertices = Vec::new();
        let mut normals = Vec::new();
        let mut uvs = Vec::new();
        let mut indices = Vec::new();

        // Side surface
        let slope = (bottom_radius - top_radius) / height;
        let slope_len = (1.0 + slope * slope).sqrt();
        let ny = 1.0 / slope_len;
        let nr = if slope.abs() < 1e-8 {
            1.0
        } else {
            -slope / slope_len
        };

        for r in 0..=rings {
            let t = r as f32 / rings as f32;
            let y = half_h - height * t;
            let radius = top_radius + (bottom_radius - top_radius) * t;

            for s in 0..=radial_segments {
                let theta = 2.0 * std::f32::consts::PI * s as f32 / radial_segments as f32;
                let (sin_t, cos_t) = theta.sin_cos();

                vertices.push(Vector3::new(cos_t * radius, y, sin_t * radius));
                let n = Vector3::new(
                    cos_t * nr,
                    ny * slope.signum().max(0.0) + (1.0 - slope.abs().min(1.0)),
                    sin_t * nr,
                );
                // For straight cylinders, normal is purely radial.
                let normal = if slope.abs() < 1e-6 {
                    Vector3::new(cos_t, 0.0, sin_t)
                } else {
                    let _ = n;
                    // Cone normal: radial component + vertical tilt
                    let radial_n = Vector3::new(cos_t, 0.0, sin_t);
                    let tilt = (-slope / slope_len, 1.0 / slope_len);
                    Vector3::new(radial_n.x * tilt.1, tilt.0, radial_n.z * tilt.1)
                };
                normals.push(normal);
                uvs.push([s as f32 / radial_segments as f32, t]);
            }
        }

        // Side indices
        let row_len = radial_segments + 1;
        for r in 0..rings {
            for s in 0..radial_segments {
                let cur = r * row_len + s;
                let next = cur + row_len;

                indices.push(cur);
                indices.push(next);
                indices.push(cur + 1);
                indices.push(cur + 1);
                indices.push(next);
                indices.push(next + 1);
            }
        }

        // Top cap
        if top_radius > 0.0 {
            let center_idx = vertices.len() as u32;
            vertices.push(Vector3::new(0.0, half_h, 0.0));
            normals.push(Vector3::UP);
            uvs.push([0.5, 0.5]);

            for s in 0..=radial_segments {
                let theta = 2.0 * std::f32::consts::PI * s as f32 / radial_segments as f32;
                let (sin_t, cos_t) = theta.sin_cos();

                vertices.push(Vector3::new(cos_t * top_radius, half_h, sin_t * top_radius));
                normals.push(Vector3::UP);
                uvs.push([cos_t * 0.5 + 0.5, sin_t * 0.5 + 0.5]);
            }

            for s in 0..radial_segments {
                indices.push(center_idx);
                indices.push(center_idx + 1 + s);
                indices.push(center_idx + 2 + s);
            }
        }

        // Bottom cap
        if bottom_radius > 0.0 {
            let center_idx = vertices.len() as u32;
            vertices.push(Vector3::new(0.0, -half_h, 0.0));
            normals.push(Vector3::new(0.0, -1.0, 0.0));
            uvs.push([0.5, 0.5]);

            for s in 0..=radial_segments {
                let theta = 2.0 * std::f32::consts::PI * s as f32 / radial_segments as f32;
                let (sin_t, cos_t) = theta.sin_cos();

                vertices.push(Vector3::new(
                    cos_t * bottom_radius,
                    -half_h,
                    sin_t * bottom_radius,
                ));
                normals.push(Vector3::new(0.0, -1.0, 0.0));
                uvs.push([cos_t * 0.5 + 0.5, sin_t * 0.5 + 0.5]);
            }

            for s in 0..radial_segments {
                indices.push(center_idx);
                indices.push(center_idx + 2 + s);
                indices.push(center_idx + 1 + s);
            }
        }

        Self {
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

    /// Returns the number of surfaces in this mesh.
    ///
    /// A mesh with no explicit surfaces counts as 1 (the primary surface).
    pub fn surface_count(&self) -> usize {
        if self.surfaces.is_empty() {
            1
        } else {
            // Primary surface (top-level vertex data) + extra surfaces.
            1 + self.surfaces.len()
        }
    }

    /// Parses a Wavefront OBJ string into a `Mesh3D`.
    ///
    /// Supports `v` (vertex positions), `vn` (normals), `vt` (texture coords),
    /// and `f` (faces) directives. Faces may be triangles or quads (quads are
    /// automatically triangulated). Face indices may use any of the forms:
    /// `v`, `v/vt`, `v/vt/vn`, or `v//vn`.
    ///
    /// Returns an error string if parsing fails.
    pub fn from_obj(source: &str) -> Result<Self, String> {
        let mut positions: Vec<Vector3> = Vec::new();
        let mut tex_coords: Vec<[f32; 2]> = Vec::new();
        let mut obj_normals: Vec<Vector3> = Vec::new();

        // Unique (position, uv, normal) combinations → output vertex index.
        let mut vertex_map: std::collections::HashMap<(u32, u32, u32), u32> =
            std::collections::HashMap::new();

        let mut vertices: Vec<Vector3> = Vec::new();
        let mut normals: Vec<Vector3> = Vec::new();
        let mut uvs: Vec<[f32; 2]> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        for (line_num, line) in source.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let mut parts = line.split_whitespace();
            let directive = match parts.next() {
                Some(d) => d,
                None => continue,
            };

            match directive {
                "v" => {
                    let coords: Vec<f32> = parts
                        .map(|s| s.parse::<f32>())
                        .collect::<Result<_, _>>()
                        .map_err(|e| format!("line {}: bad vertex coord: {e}", line_num + 1))?;
                    if coords.len() < 3 {
                        return Err(format!(
                            "line {}: vertex needs at least 3 components, got {}",
                            line_num + 1,
                            coords.len()
                        ));
                    }
                    positions.push(Vector3::new(coords[0], coords[1], coords[2]));
                }
                "vn" => {
                    let coords: Vec<f32> = parts
                        .map(|s| s.parse::<f32>())
                        .collect::<Result<_, _>>()
                        .map_err(|e| format!("line {}: bad normal coord: {e}", line_num + 1))?;
                    if coords.len() < 3 {
                        return Err(format!(
                            "line {}: normal needs 3 components, got {}",
                            line_num + 1,
                            coords.len()
                        ));
                    }
                    obj_normals.push(Vector3::new(coords[0], coords[1], coords[2]));
                }
                "vt" => {
                    let coords: Vec<f32> = parts
                        .map(|s| s.parse::<f32>())
                        .collect::<Result<_, _>>()
                        .map_err(|e| format!("line {}: bad texcoord: {e}", line_num + 1))?;
                    if coords.is_empty() {
                        return Err(format!(
                            "line {}: texcoord needs at least 1 component",
                            line_num + 1
                        ));
                    }
                    let u = coords[0];
                    let v = if coords.len() > 1 { coords[1] } else { 0.0 };
                    tex_coords.push([u, v]);
                }
                "f" => {
                    let face_verts: Vec<&str> = parts.collect();
                    if face_verts.len() < 3 {
                        return Err(format!(
                            "line {}: face needs at least 3 vertices, got {}",
                            line_num + 1,
                            face_verts.len()
                        ));
                    }

                    // Parse each face vertex into an output index.
                    let mut face_indices: Vec<u32> = Vec::with_capacity(face_verts.len());
                    for fv in &face_verts {
                        let idx = Self::parse_obj_face_vertex(
                            fv,
                            line_num + 1,
                            &positions,
                            &tex_coords,
                            &obj_normals,
                            &mut vertex_map,
                            &mut vertices,
                            &mut uvs,
                            &mut normals,
                        )?;
                        face_indices.push(idx);
                    }

                    // Triangulate: fan from first vertex.
                    for i in 1..face_indices.len() - 1 {
                        indices.push(face_indices[0]);
                        indices.push(face_indices[i]);
                        indices.push(face_indices[i + 1]);
                    }
                }
                // Ignore unknown directives (mtllib, usemtl, g, o, s, etc.)
                _ => {}
            }
        }

        if positions.is_empty() {
            return Err("OBJ file contains no vertices".into());
        }

        // If no normals were provided, compute flat normals per triangle.
        let needs_normals = normals
            .iter()
            .all(|n| n.x == 0.0 && n.y == 0.0 && n.z == 0.0);
        if needs_normals && !indices.is_empty() {
            Self::compute_flat_normals(&vertices, &indices, &mut normals);
        }

        // OBJ winds front faces counter-clockwise; Godot's are CLOCKWISE
        // (`doc/classes/ArrayMesh.xml:49`), and its OBJ importer swaps the first
        // two corners of every triangle (`resource_importer_obj.cpp:336-338`,
        // `idx = 1 ^ idx`). Done AFTER the flat normals above, which read the
        // file's own CCW order as outward.
        for tri in indices.chunks_exact_mut(3) {
            tri.swap(0, 1);
        }

        Ok(Self {
            vertices,
            normals,
            uvs,
            indices,
            primitive_type: PrimitiveType::Triangles,
            surfaces: Vec::new(),
            bone_weights: Vec::new(),
            bone_indices: Vec::new(),
            blend_shapes: Vec::new(),
        })
    }

    /// Parses a single OBJ face vertex (e.g., "1/2/3", "1//3", "1/2", "1").
    ///
    /// Returns the output vertex index, creating a new vertex if this
    /// position/uv/normal combination hasn't been seen before.
    #[allow(clippy::too_many_arguments)]
    fn parse_obj_face_vertex(
        token: &str,
        line_num: usize,
        positions: &[Vector3],
        tex_coords: &[[f32; 2]],
        obj_normals: &[Vector3],
        vertex_map: &mut std::collections::HashMap<(u32, u32, u32), u32>,
        out_verts: &mut Vec<Vector3>,
        out_uvs: &mut Vec<[f32; 2]>,
        out_normals: &mut Vec<Vector3>,
    ) -> Result<u32, String> {
        let parts: Vec<&str> = token.split('/').collect();

        let vi = Self::parse_obj_index(parts[0], positions.len(), line_num, "vertex")?;

        let ti = if parts.len() > 1 && !parts[1].is_empty() {
            Self::parse_obj_index(parts[1], tex_coords.len(), line_num, "texcoord")?
        } else {
            0
        };

        let ni = if parts.len() > 2 && !parts[2].is_empty() {
            Self::parse_obj_index(parts[2], obj_normals.len(), line_num, "normal")?
        } else {
            0
        };

        let key = (vi as u32, ti as u32, ni as u32);
        if let Some(&existing) = vertex_map.get(&key) {
            return Ok(existing);
        }

        let new_idx = out_verts.len() as u32;
        out_verts.push(positions[vi]);
        out_uvs.push(if ti < tex_coords.len() {
            tex_coords[ti]
        } else {
            [0.0, 0.0]
        });
        out_normals.push(if ni < obj_normals.len() {
            obj_normals[ni]
        } else {
            Vector3::ZERO
        });
        vertex_map.insert(key, new_idx);
        Ok(new_idx)
    }

    /// Parses a 1-based (possibly negative) OBJ index into a 0-based index.
    fn parse_obj_index(
        s: &str,
        count: usize,
        line_num: usize,
        kind: &str,
    ) -> Result<usize, String> {
        let idx: i64 = s
            .parse()
            .map_err(|e| format!("line {line_num}: bad {kind} index '{s}': {e}"))?;
        let resolved = if idx > 0 {
            (idx - 1) as usize
        } else if idx < 0 {
            // Negative indices count backward from the end.
            let abs = (-idx) as usize;
            if abs > count {
                return Err(format!(
                    "line {line_num}: negative {kind} index {idx} out of range (count={count})"
                ));
            }
            count - abs
        } else {
            return Err(format!("line {line_num}: {kind} index must not be zero"));
        };
        if resolved >= count {
            return Err(format!(
                "line {line_num}: {kind} index {idx} out of range (count={count})"
            ));
        }
        Ok(resolved)
    }

    /// Computes flat (per-face) normals for triangles and overwrites the normal array.
    fn compute_flat_normals(vertices: &[Vector3], indices: &[u32], normals: &mut [Vector3]) {
        // Accumulate face normals per vertex, then normalize.
        for n in normals.iter_mut() {
            *n = Vector3::ZERO;
        }
        let mut i = 0;
        while i + 2 < indices.len() {
            let i0 = indices[i] as usize;
            let i1 = indices[i + 1] as usize;
            let i2 = indices[i + 2] as usize;
            i += 3;

            if i0 >= vertices.len() || i1 >= vertices.len() || i2 >= vertices.len() {
                continue;
            }

            let edge1 = vertices[i1] - vertices[i0];
            let edge2 = vertices[i2] - vertices[i0];
            let face_normal = edge1.cross(edge2);

            normals[i0] = normals[i0] + face_normal;
            normals[i1] = normals[i1] + face_normal;
            normals[i2] = normals[i2] + face_normal;
        }
        for n in normals.iter_mut() {
            let len = n.length();
            if len > 1e-8 {
                *n = *n * (1.0 / len);
            } else {
                *n = Vector3::UP;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_vertex_and_index_counts() {
        let mesh = Mesh3D::cube(1.0);
        assert_eq!(mesh.vertex_count(), 24);
        assert_eq!(mesh.triangle_count(), 12);
        assert_eq!(mesh.indices.len(), 36);
    }

    #[test]
    fn cube_vertices_within_bounds() {
        let mesh = Mesh3D::cube(2.0);
        for v in &mesh.vertices {
            assert!(v.x.abs() <= 1.0 + 1e-6);
            assert!(v.y.abs() <= 1.0 + 1e-6);
            assert!(v.z.abs() <= 1.0 + 1e-6);
        }
    }

    #[test]
    fn cube_normals_unit_length() {
        let mesh = Mesh3D::cube(1.0);
        for n in &mesh.normals {
            assert!((n.length() - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn cube_indices_in_range() {
        let mesh = Mesh3D::cube(1.0);
        let max_idx = mesh.vertices.len() as u32;
        for &idx in &mesh.indices {
            assert!(idx < max_idx);
        }
    }

    #[test]
    fn sphere_structure() {
        let mesh = Mesh3D::sphere(1.0, 8);
        assert!(!mesh.vertices.is_empty());
        assert_eq!(mesh.vertices.len(), mesh.normals.len());
        assert_eq!(mesh.vertices.len(), mesh.uvs.len());
    }

    #[test]
    fn sphere_vertices_at_radius() {
        let mesh = Mesh3D::sphere(3.0, 8);
        for v in &mesh.vertices {
            assert!((v.length() - 3.0).abs() < 1e-4);
        }
    }

    #[test]
    fn torus_structure_and_bounds() {
        let inner = 0.5;
        let outer = 1.0;
        let mesh = Mesh3D::torus(inner, outer, 24, 12);
        assert!(!mesh.vertices.is_empty());
        assert_eq!(mesh.vertices.len(), mesh.normals.len());
        assert_eq!(mesh.vertices.len(), mesh.uvs.len());
        let tube = (outer - inner) * 0.5;
        for v in &mesh.vertices {
            // Every vertex lies within the torus's tube radius of the ring.
            assert!(v.y.abs() <= tube + 1e-4, "y within tube radius: {}", v.y);
            let radial = (v.x * v.x + v.z * v.z).sqrt();
            assert!(
                radial >= inner - 1e-4 && radial <= outer + 1e-4,
                "radial distance {radial} within [{inner}, {outer}]"
            );
        }
        // Normals are unit length.
        for n in &mesh.normals {
            assert!((n.length() - 1.0).abs() < 1e-3);
        }
    }

    #[test]
    fn plane_counts() {
        let mesh = Mesh3D::plane(5.0);
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.triangle_count(), 2);
    }

    #[test]
    fn plane_on_xz() {
        let mesh = Mesh3D::plane(4.0);
        for v in &mesh.vertices {
            assert!(v.y.abs() < 1e-6);
        }
    }

    #[test]
    fn capsule_structure() {
        let mesh = Mesh3D::capsule(0.5, 2.0, 12, 4);
        assert!(!mesh.vertices.is_empty());
        assert_eq!(mesh.vertices.len(), mesh.normals.len());
        assert_eq!(mesh.vertices.len(), mesh.uvs.len());
        assert_eq!(mesh.primitive_type, PrimitiveType::Triangles);
    }

    #[test]
    fn capsule_vertices_within_bounds() {
        let mesh = Mesh3D::capsule(0.5, 2.0, 8, 4);
        for v in &mesh.vertices {
            let radial = (v.x * v.x + v.z * v.z).sqrt();
            assert!(radial <= 0.5 + 1e-3, "radial {radial} exceeds radius");
            assert!(v.y.abs() <= 1.0 + 1e-3, "y {} exceeds half-height", v.y);
        }
    }

    #[test]
    fn capsule_indices_in_range() {
        let mesh = Mesh3D::capsule(0.3, 1.5, 8, 4);
        let max_idx = mesh.vertices.len() as u32;
        for &idx in &mesh.indices {
            assert!(idx < max_idx, "index {idx} >= vertex count {max_idx}");
        }
    }

    #[test]
    fn cylinder_structure() {
        let mesh = Mesh3D::cylinder(0.5, 0.5, 1.0, 12, 4);
        assert!(!mesh.vertices.is_empty());
        assert_eq!(mesh.vertices.len(), mesh.normals.len());
        assert_eq!(mesh.vertices.len(), mesh.uvs.len());
    }

    #[test]
    fn cylinder_has_caps() {
        let mesh = Mesh3D::cylinder(0.5, 0.5, 1.0, 8, 2);
        let has_up = mesh.normals.iter().any(|n| (n.y - 1.0).abs() < 1e-4);
        let has_down = mesh.normals.iter().any(|n| (n.y + 1.0).abs() < 1e-4);
        assert!(has_up, "missing top cap");
        assert!(has_down, "missing bottom cap");
    }

    #[test]
    fn cylinder_indices_in_range() {
        let mesh = Mesh3D::cylinder(0.3, 0.5, 2.0, 12, 3);
        let max_idx = mesh.vertices.len() as u32;
        for &idx in &mesh.indices {
            assert!(idx < max_idx, "index {idx} >= vertex count {max_idx}");
        }
    }

    #[test]
    fn cylinder_cone_no_top_cap() {
        let mesh = Mesh3D::cylinder(0.0, 0.5, 1.0, 8, 2);
        // No vertices at top cap center with UP normal
        let top_centers = mesh
            .vertices
            .iter()
            .zip(mesh.normals.iter())
            .filter(|(v, n)| {
                (v.y - 0.5).abs() < 1e-4
                    && (n.y - 1.0).abs() < 1e-4
                    && v.x.abs() < 1e-4
                    && v.z.abs() < 1e-4
            })
            .count();
        assert_eq!(top_centers, 0);
    }

    #[test]
    fn empty_mesh() {
        let mesh = Mesh3D::new(PrimitiveType::Lines);
        assert!(mesh.vertices.is_empty());
        assert_eq!(mesh.primitive_type, PrimitiveType::Lines);
    }

    // ── Skinning attribute fields ──

    #[test]
    fn primitive_mesh_has_no_bone_data() {
        let mesh = Mesh3D::cube(1.0);
        assert!(mesh.bone_weights.is_empty());
        assert!(mesh.bone_indices.is_empty());
        assert!(!mesh.is_skinned());
    }

    #[test]
    fn mesh_with_parallel_bone_data_is_skinned() {
        let mut mesh = Mesh3D::plane(1.0); // 4 vertices
        mesh.bone_weights = vec![[1.0, 0.0, 0.0, 0.0]; 4];
        mesh.bone_indices = vec![[0, 0, 0, 0]; 4];
        assert!(mesh.is_skinned());
        // A mismatched-length attribute array must not count as skinned.
        mesh.bone_weights.pop();
        assert!(!mesh.is_skinned());
    }

    #[test]
    fn default_mesh_is_empty_triangles() {
        let mesh = Mesh3D::default();
        assert!(mesh.vertices.is_empty());
        assert_eq!(mesh.primitive_type, PrimitiveType::Triangles);
        assert!(mesh.bone_weights.is_empty());
        assert!(mesh.bone_indices.is_empty());
    }

    #[test]
    fn single_surface_mesh_count() {
        let mesh = Mesh3D::cube(1.0);
        assert_eq!(mesh.surface_count(), 1);
        assert!(mesh.surfaces.is_empty());
    }

    // pat-nzgdu.75: SurfaceTool procedural builder stages per-vertex attributes
    // and commits a renderable Mesh3D.
    #[test]
    fn surface_tool_builds_triangle_mesh() {
        let mut st = SurfaceTool::new();
        st.begin(PrimitiveType::Triangles);
        // One triangle: a shared face normal, distinct per-vertex UVs.
        st.set_normal(Vector3::new(0.0, 0.0, 1.0));
        st.set_uv([0.0, 0.0]);
        st.add_vertex(Vector3::new(0.0, 0.0, 0.0));
        st.set_uv([1.0, 0.0]);
        st.add_vertex(Vector3::new(1.0, 0.0, 0.0));
        st.set_uv([0.0, 1.0]);
        st.add_vertex(Vector3::new(0.0, 1.0, 0.0));
        assert_eq!(st.vertex_count(), 3);

        let mesh = st.commit();
        assert_eq!(mesh.vertices.len(), 3);
        assert_eq!(mesh.vertices[1], Vector3::new(1.0, 0.0, 0.0));
        // The staged normal is snapshotted onto every added vertex.
        assert_eq!(mesh.normals.len(), 3);
        assert!(mesh
            .normals
            .iter()
            .all(|n| *n == Vector3::new(0.0, 0.0, 1.0)));
        // Per-vertex UVs are preserved in add order.
        assert_eq!(mesh.uvs[2], [0.0, 1.0]);
        // No explicit indices -> a sequential index per vertex (renderable).
        assert_eq!(mesh.indices, vec![0, 1, 2]);
        assert_eq!(mesh.primitive_type, PrimitiveType::Triangles);

        // Explicit indices are honored verbatim instead of being generated.
        let mut st2 = SurfaceTool::new();
        st2.add_vertex(Vector3::ZERO);
        st2.add_vertex(Vector3::new(1.0, 0.0, 0.0));
        st2.add_vertex(Vector3::new(0.0, 1.0, 0.0));
        st2.add_index(0);
        st2.add_index(2);
        st2.add_index(1);
        assert_eq!(st2.commit().indices, vec![0, 2, 1]);
    }

    #[test]
    fn multi_surface_mesh_count() {
        let mut mesh = Mesh3D::cube(1.0);
        mesh.surfaces.push(Surface3D {
            vertices: vec![Vector3::ZERO],
            normals: vec![Vector3::UP],
            uvs: vec![[0.0, 0.0]],
            indices: vec![0],
            primitive_type: PrimitiveType::Points,
            bone_weights: Vec::new(),
            bone_indices: Vec::new(),
            lods: Vec::new(),
        });
        mesh.surfaces.push(Surface3D {
            vertices: vec![Vector3::new(1.0, 0.0, 0.0)],
            normals: vec![Vector3::UP],
            uvs: vec![[1.0, 0.0]],
            indices: vec![0],
            primitive_type: PrimitiveType::Points,
            bone_weights: Vec::new(),
            bone_indices: Vec::new(),
            lods: Vec::new(),
        });
        assert_eq!(mesh.surface_count(), 3);
    }

    // ── OBJ loader tests ──

    #[test]
    fn obj_simple_triangle() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
f 1 2 3
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.triangle_count(), 1);
        // OBJ's CCW `1 2 3` stored Godot-clockwise: the importer swaps the first
        // two corners (`resource_importer_obj.cpp:336-338`).
        assert_eq!(mesh.indices, vec![1, 0, 2]);
        assert_eq!(mesh.primitive_type, PrimitiveType::Triangles);
    }

    #[test]
    fn obj_with_normals_and_uvs() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
vn 0.0 0.0 1.0
vt 0.0 0.0
vt 1.0 0.0
vt 0.0 1.0
f 1/1/1 2/2/1 3/3/1
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.normals.len(), 3);
        assert_eq!(mesh.uvs.len(), 3);
        // All normals should be (0, 0, 1).
        for n in &mesh.normals {
            assert!((n.z - 1.0).abs() < 1e-5);
        }
        assert!((mesh.uvs[1][0] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn obj_quad_is_triangulated() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 1.0 1.0 0.0
v 0.0 1.0 0.0
f 1 2 3 4
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        assert_eq!(mesh.vertex_count(), 4);
        // Quad → 2 triangles → 6 indices.
        assert_eq!(mesh.triangle_count(), 2);
        assert_eq!(mesh.indices.len(), 6);
    }

    #[test]
    fn obj_vertex_position_slash_normal() {
        let obj = "\
v 1.0 2.0 3.0
v 4.0 5.0 6.0
v 7.0 8.0 9.0
vn 0.0 1.0 0.0
f 1//1 2//1 3//1
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        assert_eq!(mesh.vertex_count(), 3);
        for n in &mesh.normals {
            assert!((n.y - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn obj_comments_and_blank_lines_ignored() {
        let obj = "\
# This is a comment
v 0.0 0.0 0.0

# Another comment
v 1.0 0.0 0.0
v 0.0 1.0 0.0

f 1 2 3
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        assert_eq!(mesh.vertex_count(), 3);
    }

    #[test]
    fn obj_negative_indices() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
f -3 -2 -1
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        assert_eq!(mesh.vertex_count(), 3);
        // Same vertices as `f 1 2 3`, so the same Godot-clockwise order.
        assert_eq!(mesh.indices, vec![1, 0, 2]);
    }

    #[test]
    fn obj_multiple_faces() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 1.0 1.0 0.0
v 0.0 1.0 0.0
f 1 2 3
f 1 3 4
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.triangle_count(), 2);
    }

    #[test]
    fn obj_shared_vertices_deduped() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 1.0 1.0 0.0
v 0.0 1.0 0.0
vn 0.0 0.0 1.0
f 1//1 2//1 3//1
f 1//1 3//1 4//1
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        // Vertices 1//1 and 3//1 appear in both faces — should be deduped.
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.triangle_count(), 2);
    }

    #[test]
    fn obj_different_normals_create_new_vertices() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
vn 0.0 0.0 1.0
vn 0.0 0.0 -1.0
f 1//1 2//1 3//1
f 1//2 2//2 3//2
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        // Same positions but different normals → 6 unique vertices.
        assert_eq!(mesh.vertex_count(), 6);
    }

    #[test]
    fn obj_auto_normals_when_none_provided() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
f 1 2 3
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        // Should have auto-generated normals pointing in +Z.
        for n in &mesh.normals {
            let len = n.length();
            assert!((len - 1.0).abs() < 1e-4, "normal not unit length: {len}");
        }
    }

    #[test]
    fn obj_empty_fails() {
        let result = Mesh3D::from_obj("");
        assert!(result.is_err());
    }

    #[test]
    fn obj_bad_vertex_fails() {
        let obj = "v 1.0 abc 3.0\nf 1 1 1\n";
        assert!(Mesh3D::from_obj(obj).is_err());
    }

    #[test]
    fn obj_index_out_of_range_fails() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
f 1 2 4
";
        assert!(Mesh3D::from_obj(obj).is_err());
    }

    #[test]
    fn obj_zero_index_fails() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
f 0 1 2
";
        assert!(Mesh3D::from_obj(obj).is_err());
    }

    #[test]
    fn obj_unknown_directives_ignored() {
        let obj = "\
mtllib cube.mtl
o Cube
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
usemtl Material
s off
f 1 2 3
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        assert_eq!(mesh.vertex_count(), 3);
    }

    #[test]
    fn obj_cube_roundtrip() {
        // A simple cube in OBJ format.
        let obj = "\
v -0.5 -0.5  0.5
v  0.5 -0.5  0.5
v  0.5  0.5  0.5
v -0.5  0.5  0.5
v -0.5 -0.5 -0.5
v  0.5 -0.5 -0.5
v  0.5  0.5 -0.5
v -0.5  0.5 -0.5
f 1 2 3 4
f 5 8 7 6
f 1 4 8 5
f 2 6 7 3
f 4 3 7 8
f 1 5 6 2
";
        let mesh = Mesh3D::from_obj(obj).unwrap();
        // 6 quads → 12 triangles.
        assert_eq!(mesh.triangle_count(), 12);
        assert_eq!(mesh.indices.len(), 36);
        // All indices should be valid.
        let max = mesh.vertex_count() as u32;
        for &idx in &mesh.indices {
            assert!(idx < max);
        }
    }

    #[test]
    fn obj_face_with_only_two_vertices_fails() {
        let obj = "\
v 0.0 0.0 0.0
v 1.0 0.0 0.0
f 1 2
";
        assert!(Mesh3D::from_obj(obj).is_err());
    }

    // -- aabb (pat-nzgdu.211) ----------------------------------------------

    /// ⚠️ An empty mesh has NO bounds — `None`, not a zero-sized box. A
    /// zero-sized AABB at the origin is a real (if tiny) box as far as a culler
    /// is concerned, so returning one would be a lie it would act on.
    #[test]
    fn aabb_is_none_for_an_empty_mesh() {
        assert!(Mesh3D::new(PrimitiveType::Triangles).aabb().is_none());
    }

    #[test]
    fn aabb_encloses_a_cubes_vertices() {
        // cube(2.0) is FULL size, so it spans -1..1 on each axis.
        let aabb = Mesh3D::cube(2.0).aabb().expect("a cube has bounds");
        assert!((aabb.position.x - (-1.0)).abs() < 1e-4, "{aabb:?}");
        assert!((aabb.position.y - (-1.0)).abs() < 1e-4, "{aabb:?}");
        assert!((aabb.position.z - (-1.0)).abs() < 1e-4, "{aabb:?}");
        assert!((aabb.size.x - 2.0).abs() < 1e-4, "{aabb:?}");
        assert!((aabb.size.y - 2.0).abs() < 1e-4, "{aabb:?}");
        assert!((aabb.size.z - 2.0).abs() < 1e-4, "{aabb:?}");
    }

    /// A single vertex yields a zero-SIZED box — which is correct here, unlike
    /// the empty case: the mesh really does occupy exactly that point.
    #[test]
    fn aabb_of_a_single_vertex_is_a_degenerate_box_at_that_point() {
        let mut mesh = Mesh3D::new(PrimitiveType::Points);
        mesh.vertices.push(Vector3::new(3.0, -4.0, 5.0));

        let aabb = mesh.aabb().expect("one vertex is still bounds");
        assert!((aabb.position.x - 3.0).abs() < 1e-4, "{aabb:?}");
        assert!((aabb.position.y - (-4.0)).abs() < 1e-4, "{aabb:?}");
        assert!(aabb.size.x.abs() < 1e-4, "a point has no extent: {aabb:?}");
    }

    /// The bounds must enclose vertices in every direction, including negative
    /// ones — a min/max mix-up survives a symmetric cube but not this.
    #[test]
    fn aabb_covers_asymmetric_vertices() {
        let mut mesh = Mesh3D::new(PrimitiveType::Triangles);
        for v in [
            Vector3::new(-7.0, 2.0, 0.0),
            Vector3::new(1.0, 9.0, -3.0),
            Vector3::new(0.0, 2.5, 4.0),
        ] {
            mesh.vertices.push(v);
        }

        let aabb = mesh.aabb().expect("bounds");
        assert!((aabb.position.x - (-7.0)).abs() < 1e-4, "{aabb:?}");
        assert!((aabb.position.y - 2.0).abs() < 1e-4, "{aabb:?}");
        assert!((aabb.position.z - (-3.0)).abs() < 1e-4, "{aabb:?}");
        assert!((aabb.size.x - 8.0).abs() < 1e-4, "{aabb:?}");
        assert!((aabb.size.y - 7.0).abs() < 1e-4, "{aabb:?}");
        assert!((aabb.size.z - 7.0).abs() < 1e-4, "{aabb:?}");
    }
}

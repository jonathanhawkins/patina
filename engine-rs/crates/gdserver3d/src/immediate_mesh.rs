//! `ImmediateMesh` — build geometry a surface at a time, every frame
//! (pat-s3rrv.197).
//!
//! Godot ground truth: `scene/resources/immediate_mesh.cpp`,
//! `doc/classes/ImmediateMesh.xml`.
//!
//! `surface_begin(PRIMITIVE_LINES)` → `surface_add_vertex(a)` →
//! `surface_add_vertex(b)` → `surface_end()` is the standard way to draw debug
//! geometry, gizmos, trajectory previews and dynamic ribbons in 3D. Patina had
//! no implementation of any kind: the class appeared only in the editor's
//! generated property-schema table, never in ClassDB.
//!
//! # Attribute semantics
//!
//! ⚠️ `surface_set_color` / `_normal` / `_uv` set the attributes the NEXT
//! `surface_add_vertex` will use, and they persist until changed — they do not
//! decorate the previous vertex. Applying them backwards produces geometry that
//! is off by one vertex, which looks like a winding or indexing bug rather than
//! an attribute bug. Godot documents this explicitly: "call this before
//! `surface_add_vertex`".

use gdcore::math::{Color, Vector3};

use crate::mesh::{Mesh3D, PrimitiveType};

/// Godot's `Mesh.PrimitiveType` ordinals.
///
/// ⚠️ Transcribed from `doc/classes/Mesh.xml`, NOT derived from our enum's
/// declaration order. Godot numbers them POINTS 0, LINES 1, LINE_STRIP 2,
/// **TRIANGLES 3**, TRIANGLE_STRIP 4 — while [`PrimitiveType`] is declared
/// `Triangles, Lines, Points`. The two orders agree nowhere, so mapping by
/// position would silently turn every `PRIMITIVE_TRIANGLES` into something else.
pub fn primitive_from_godot_int(value: i64) -> Option<PrimitiveType> {
    match value {
        0 => Some(PrimitiveType::Points),
        1 => Some(PrimitiveType::Lines),
        3 => Some(PrimitiveType::Triangles),
        // LINE_STRIP (2) and TRIANGLE_STRIP (4) are real Godot values this
        // renderer has no representation for. Reported as unsupported rather
        // than quietly rounded to a neighbour, which would draw the wrong shape.
        _ => None,
    }
}

/// One finished surface of an [`ImmediateMesh`].
///
/// Colours live HERE rather than on [`Mesh3D`]. `Mesh3D` has no colour channel,
/// and adding one would touch ~10 exhaustive struct literals across
/// `text_mesh`, `navigation`, `extra_meshes` and `csg` — a wide blast radius for
/// a field only this type populates. The acceptance asks that colours reach "the
/// resulting surface arrays", and these are those arrays.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImmediateSurface {
    /// How the vertices are assembled.
    pub primitive_type: PrimitiveType,
    /// Vertex positions in the order they were added.
    pub vertices: Vec<Vector3>,
    /// Per-vertex normals. Empty when the surface set none.
    pub normals: Vec<Vector3>,
    /// Per-vertex UVs. Empty when the surface set none.
    pub uvs: Vec<[f32; 2]>,
    /// Per-vertex colours. Empty when the surface set none.
    pub colors: Vec<Color>,
}

/// Why a surface call was rejected.
///
/// ⚠️ Godot ERRORS on these rather than silently doing the wrong thing, and the
/// acceptance requires the same: a `surface_add_vertex` outside a begin/end pair
/// that quietly appended would corrupt whichever surface happened to be last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImmediateMeshError {
    /// A vertex or attribute call arrived with no surface open.
    NoSurfaceOpen,
    /// `surface_begin` was called while a surface was already open.
    SurfaceAlreadyOpen,
    /// `surface_begin` was given a primitive this renderer cannot draw.
    UnsupportedPrimitive,
    /// `surface_end` closed a surface with no vertices in it.
    EmptySurface,
}

/// A mesh assembled by explicit `surface_*` calls.
#[derive(Debug, Clone, Default)]
pub struct ImmediateMesh {
    finished: Vec<ImmediateSurface>,
    open: Option<ImmediateSurface>,
    pending_normal: Option<Vector3>,
    pending_uv: Option<[f32; 2]>,
    pending_color: Option<Color>,
}

impl ImmediateMesh {
    /// An empty mesh with no surfaces.
    pub fn new() -> Self {
        Self::default()
    }

    /// Godot's `surface_begin(primitive)`.
    pub fn surface_begin(&mut self, primitive: i64) -> Result<(), ImmediateMeshError> {
        if self.open.is_some() {
            return Err(ImmediateMeshError::SurfaceAlreadyOpen);
        }
        let primitive_type =
            primitive_from_godot_int(primitive).ok_or(ImmediateMeshError::UnsupportedPrimitive)?;
        self.open = Some(ImmediateSurface {
            primitive_type,
            ..Default::default()
        });
        // Attributes do not carry across a surface boundary.
        self.pending_normal = None;
        self.pending_uv = None;
        self.pending_color = None;
        Ok(())
    }

    /// Godot's `surface_set_normal`, applied to subsequent vertices.
    pub fn surface_set_normal(&mut self, normal: Vector3) -> Result<(), ImmediateMeshError> {
        self.require_open()?;
        self.pending_normal = Some(normal);
        Ok(())
    }

    /// Godot's `surface_set_uv`.
    pub fn surface_set_uv(&mut self, uv: [f32; 2]) -> Result<(), ImmediateMeshError> {
        self.require_open()?;
        self.pending_uv = Some(uv);
        Ok(())
    }

    /// Godot's `surface_set_color`.
    pub fn surface_set_color(&mut self, color: Color) -> Result<(), ImmediateMeshError> {
        self.require_open()?;
        self.pending_color = Some(color);
        Ok(())
    }

    /// Godot's `surface_add_vertex`, consuming the attributes set so far.
    ///
    /// ⚠️ The per-vertex arrays are kept PARALLEL to `vertices`: a surface that
    /// set a normal only for its second vertex still gets a normal for the
    /// first, because arrays read by index are coupled through their length and
    /// a short one silently misaligns every later vertex.
    pub fn surface_add_vertex(&mut self, position: Vector3) -> Result<(), ImmediateMeshError> {
        let (normal, uv, color) = (self.pending_normal, self.pending_uv, self.pending_color);
        let surface = self
            .open
            .as_mut()
            .ok_or(ImmediateMeshError::NoSurfaceOpen)?;
        surface.vertices.push(position);
        if let Some(n) = normal {
            while surface.normals.len() + 1 < surface.vertices.len() {
                surface.normals.push(Vector3::ZERO);
            }
            surface.normals.push(n);
        }
        if let Some(u) = uv {
            while surface.uvs.len() + 1 < surface.vertices.len() {
                surface.uvs.push([0.0, 0.0]);
            }
            surface.uvs.push(u);
        }
        if let Some(c) = color {
            while surface.colors.len() + 1 < surface.vertices.len() {
                surface.colors.push(Color::new(1.0, 1.0, 1.0, 1.0));
            }
            surface.colors.push(c);
        }
        Ok(())
    }

    /// Godot's `surface_end`, committing the open surface.
    pub fn surface_end(&mut self) -> Result<(), ImmediateMeshError> {
        let surface = self.open.take().ok_or(ImmediateMeshError::NoSurfaceOpen)?;
        if surface.vertices.is_empty() {
            return Err(ImmediateMeshError::EmptySurface);
        }
        self.finished.push(surface);
        Ok(())
    }

    /// Godot's `clear_surfaces`: drops every surface, open one included.
    pub fn clear_surfaces(&mut self) {
        self.finished.clear();
        self.open = None;
        self.pending_normal = None;
        self.pending_uv = None;
        self.pending_color = None;
    }

    /// Godot's `get_surface_count` — finished surfaces only.
    pub fn surface_count(&self) -> usize {
        self.finished.len()
    }

    /// The finished surfaces, in the order they were committed.
    pub fn surfaces(&self) -> &[ImmediateSurface] {
        &self.finished
    }

    /// Whether a surface is currently open.
    pub fn is_surface_open(&self) -> bool {
        self.open.is_some()
    }

    /// Flattens the finished surfaces into a renderable [`Mesh3D`].
    ///
    /// Surface 0 becomes the mesh's primary vertex data and the rest become
    /// `Mesh3D::surfaces`, which is the layout the renderer already expects.
    /// Returns `None` for a mesh with no finished surfaces, so a cleared
    /// `ImmediateMesh` draws nothing rather than an empty husk.
    pub fn to_mesh(&self) -> Option<Mesh3D> {
        let first = self.finished.first()?;
        let mut mesh = Mesh3D {
            vertices: first.vertices.clone(),
            normals: first.normals.clone(),
            uvs: first.uvs.clone(),
            indices: (0..first.vertices.len() as u32).collect(),
            primitive_type: first.primitive_type,
            ..Default::default()
        };
        for extra in self.finished.iter().skip(1) {
            mesh.surfaces.push(crate::mesh::Surface3D {
                vertices: extra.vertices.clone(),
                normals: extra.normals.clone(),
                uvs: extra.uvs.clone(),
                indices: (0..extra.vertices.len() as u32).collect(),
                primitive_type: extra.primitive_type,
                lods: Vec::new(),
            });
        }
        Some(mesh)
    }

    fn require_open(&self) -> Result<(), ImmediateMeshError> {
        if self.open.is_some() {
            Ok(())
        } else {
            Err(ImmediateMeshError::NoSurfaceOpen)
        }
    }
}

//! `ArrayMesh` resources saved by Godot — binary `.res` or text `.tres` —
//! decoded into a renderable [`Mesh3D`] (pat-33wmr).
//!
//! Godot does not store an `ArrayMesh` as vertex arrays: it stores each
//! surface as the RenderingServer's own GPU buffers (`_surfaces[i]` holds
//! `format`, `vertex_data`, `attribute_data`, `index_data`, `aabb`,
//! `uv_scale`, …), the form `RenderingServer::mesh_add_surface` takes. This
//! module is the inverse — a port of `RenderingServer::_get_array_from_surface`
//! (`servers/rendering/rendering_server.cpp`) for the format-version-2 layout
//! every Godot ≥ 4.2 file uses.
//!
//! Before it existed, no `ArrayMesh` file reached the renderer at all: a
//! `MeshInstance3D` or GridMap cell whose mesh was a `.res`/`.tres` fell
//! through every path heuristic to the placeholder cube.
//!
//! ## Format v2 layout (`mesh_surface_make_offsets_from_format`)
//!
//! * `vertex_data` = the POSITION stream for every vertex, then the
//!   NORMAL/TANGENT stream for every vertex (two separate streams, not one
//!   interleaved record).
//! * `attribute_data` = COLOR, UV, UV2, CUSTOM0-3 interleaved per vertex.
//! * `index_data` = `u16` indices when `vertex_count <= 65536`, else `u32`.
//! * With `ARRAY_FLAG_COMPRESS_ATTRIBUTES`, positions are `u16 × 4`
//!   normalized into the surface `aabb` (the 4th lane is the tangent angle),
//!   the normal stream has no tangent, and UVs are `u16 × 2` scaled by
//!   `uv_scale`.

use std::cell::RefCell;
use std::collections::HashMap;

use gdcore::math::Vector3;
use gdresource::Resource;
use gdserver3d::mesh::{Mesh3D, PrimitiveType, Surface3D};
use gdvariant::Variant;

// `RenderingServer::ArrayType` bit positions.
const ARRAY_VERTEX: u32 = 0;
const ARRAY_NORMAL: u32 = 1;
const ARRAY_TANGENT: u32 = 2;
const ARRAY_COLOR: u32 = 3;
const ARRAY_TEX_UV: u32 = 4;
const ARRAY_TEX_UV2: u32 = 5;
const ARRAY_CUSTOM0: u32 = 6;
const ARRAY_BONES: u32 = 10;
const ARRAY_WEIGHTS: u32 = 11;
const ARRAY_INDEX: u32 = 12;
const ARRAY_MAX: u32 = 13;

const FORMAT_CUSTOM_BASE: u32 = ARRAY_INDEX + 1;
const FORMAT_CUSTOM_BITS: u32 = 3;
const COMPRESS_FLAGS_BASE: u32 = 25;
const FLAG_USE_2D_VERTICES: u64 = 1 << COMPRESS_FLAGS_BASE;
const FLAG_USE_8_BONE_WEIGHTS: u64 = 1 << (COMPRESS_FLAGS_BASE + 2);
const FLAG_COMPRESS_ATTRIBUTES: u64 = 1 << (COMPRESS_FLAGS_BASE + 4);
const FLAG_FORMAT_VERSION_SHIFT: u32 = COMPRESS_FLAGS_BASE + 10;
const FLAG_FORMAT_VERSION_MASK: u64 = 0xFF;
const FORMAT_VERSION_2: u64 = 1;

// `Mesh::PrimitiveType`.
const PRIMITIVE_POINTS: i64 = 0;
const PRIMITIVE_LINES: i64 = 1;
const PRIMITIVE_LINE_STRIP: i64 = 2;
const PRIMITIVE_TRIANGLES: i64 = 3;
const PRIMITIVE_TRIANGLE_STRIP: i64 = 4;

/// One decoded surface plus the material reference it carries
/// (`"ExtResource:<id>"` / `"SubResource:<id>"` as the loaders spell it).
#[derive(Debug, Clone)]
pub struct DecodedSurface {
    pub surface: Surface3D,
    pub material: Option<String>,
    pub name: Option<String>,
}

/// Decodes every surface of an `ArrayMesh` resource. Surfaces that cannot be
/// decoded (an unknown format version, inconsistent buffer sizes) are
/// skipped, never guessed at.
pub fn decode_surfaces(resource: &Resource) -> Vec<DecodedSurface> {
    let Some(Variant::Array(surfaces)) = resource.get_property("_surfaces") else {
        return Vec::new();
    };
    surfaces
        .to_vec()
        .iter()
        .filter_map(|s| match s {
            Variant::Dictionary(d) => decode_surface(&d.to_dict()),
            _ => None,
        })
        .collect()
}

/// Builds a [`Mesh3D`] from an `ArrayMesh` resource, following the glTF
/// converter's convention: surface 0 is the primary data, further surfaces go
/// in [`Mesh3D::surfaces`]. `None` when the resource is not an `ArrayMesh` or
/// no surface decodes.
pub fn mesh3d_from_array_mesh(resource: &Resource) -> Option<Mesh3D> {
    if resource.class_name != "ArrayMesh" {
        return None;
    }
    let mut decoded = decode_surfaces(resource).into_iter();
    let first = decoded.next()?.surface;
    Some(Mesh3D {
        vertices: first.vertices,
        normals: first.normals,
        uvs: first.uvs,
        indices: first.indices,
        primitive_type: first.primitive_type,
        surfaces: decoded.map(|d| d.surface).collect(),
        ..Default::default()
    })
}

thread_local! {
    static MESH_FILE_CACHE: RefCell<HashMap<String, Option<Mesh3D>>> = RefCell::new(HashMap::new());
}

/// Loads an `ArrayMesh` file (`.res` binary or `.tres` text) into a
/// [`Mesh3D`], memoized per thread by path — the render paths ask for the same
/// GridMap tile once per cell per frame.
///
/// `path` may be `res://…` (resolved through the mounted packs and the project
/// root) or a filesystem path. Returns `None` for anything that is not a
/// loadable `ArrayMesh`.
pub fn load_array_mesh_file(path: &str) -> Option<Mesh3D> {
    if let Some(hit) = MESH_FILE_CACHE.with(|c| c.borrow().get(path).cloned()) {
        return hit;
    }
    let mesh = read_resource_file(path).and_then(|res| mesh3d_from_array_mesh(&res));
    MESH_FILE_CACHE.with(|c| c.borrow_mut().insert(path.to_string(), mesh.clone()));
    mesh
}

/// A surface material an `ArrayMesh` file names, as the property bag of the
/// material resource (pat-zwvgw).
///
/// Every texture reference in `props` is already resolved to a loadable path
/// (`res://…`), so the bag can go straight to
/// [`crate::render_server_3d::standard_material_from_props`].
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceMaterialProps {
    /// The material's class (`StandardMaterial3D`, `ORMMaterial3D`, …).
    pub class: String,
    /// The material's own properties.
    pub props: HashMap<String, Variant>,
}

thread_local! {
    static SURFACE_MATERIAL_CACHE: RefCell<HashMap<String, Vec<Option<SurfaceMaterialProps>>>> =
        RefCell::new(HashMap::new());
}

/// The material each surface of the `ArrayMesh` file at `path` carries
/// (`_surfaces[i].material`), in surface order, memoized per thread.
///
/// 🔑 Godot draws a mesh surface with its OWN material whenever nothing
/// overrides it (`render_forward_clustered.cpp`: `material_override`, else the
/// instance's surface override, else the mesh surface's material). The
/// official 3D platformer's whole level is GridMap cells and MeshInstance3Ds of
/// `.res` tiles whose only material is this one, so before this every tile drew
/// untextured grey: [`decode_surfaces`] read the reference and nothing used it.
///
/// `None` entries are surfaces with no material, or one that is a
/// `ShaderMaterial` (that is a separate render channel) or cannot be read.
pub fn load_array_mesh_surface_materials(path: &str) -> Vec<Option<SurfaceMaterialProps>> {
    if let Some(hit) = SURFACE_MATERIAL_CACHE.with(|c| c.borrow().get(path).cloned()) {
        return hit;
    }
    let out: Vec<Option<SurfaceMaterialProps>> = read_resource_file(path)
        .filter(|res| res.class_name == "ArrayMesh")
        .map(|res| {
            decode_surfaces(&res)
                .iter()
                .map(|s| s.material.as_deref().and_then(|r| material_from_ref(&res, r)))
                .collect()
        })
        .unwrap_or_default();
    SURFACE_MATERIAL_CACHE.with(|c| c.borrow_mut().insert(path.to_string(), out.clone()));
    out
}

/// Resolves a `SubResource:<id>` / `ExtResource:<id>` material reference held by
/// `owner` into the material's property bag.
fn material_from_ref(owner: &Resource, reference: &str) -> Option<SurfaceMaterialProps> {
    let material: std::sync::Arc<Resource> =
        if let Some(id) = reference.strip_prefix("SubResource:") {
            owner.subresources.get(id)?.clone()
        } else if let Some(id) = reference.strip_prefix("ExtResource:") {
            let ext = owner.ext_resources.get(id)?;
            read_resource_file(&ext.path)?
        } else if reference.starts_with("res://") {
            read_resource_file(reference)?
        } else {
            return None;
        };
    if material.class_name == "ShaderMaterial" {
        return None;
    }
    // A sub-resource material resolves its textures against the file that holds
    // it; an external one against its own file.
    let texture_owner: &Resource = if reference.starts_with("SubResource:") {
        owner
    } else {
        &material
    };
    let props = material
        .properties()
        .map(|(k, v)| {
            let v = match v {
                Variant::String(s) => match texture_ref_path(texture_owner, s) {
                    Some(p) => Variant::String(p),
                    None => v.clone(),
                },
                _ => v.clone(),
            };
            (k.clone(), v)
        })
        .collect();
    Some(SurfaceMaterialProps {
        class: material.class_name.clone(),
        props,
    })
}

/// The `res://` path an `ExtResource:<id>` reference names in `owner`, or `None`
/// when `value` is not such a reference.
fn texture_ref_path(owner: &Resource, value: &str) -> Option<String> {
    let id = value.strip_prefix("ExtResource:")?;
    owner.ext_resources.get(id).map(|e| e.path.clone())
}

/// Whether `path` names a resource file an `ArrayMesh` can live in.
pub fn is_mesh_resource_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".res") || lower.ends_with(".tres") || lower.ends_with(".mesh")
}

/// Reads and parses a resource file of either format.
///
/// A `res://` path with no project root configured falls back to the path
/// relative to the working directory, which is how the editor and runner are
/// launched (from the project directory).
pub fn read_resource_file(path: &str) -> Option<std::sync::Arc<Resource>> {
    let bytes = gdcore::file_access::FileAccess::read_file_bytes(path)
        .ok()
        .or_else(|| std::fs::read(path.strip_prefix("res://").unwrap_or(path)).ok())?;
    match gdresource::load_resource_bytes(&bytes, path) {
        Ok(res) => Some(res),
        Err(e) => {
            tracing::warn!("cannot load resource {path}: {e}");
            None
        }
    }
}

fn int(d: &gdvariant::Dictionary, key: &str) -> Option<i64> {
    match d.get(key)? {
        Variant::Int(i) => Some(*i),
        _ => None,
    }
}

fn bytes<'a>(d: &'a gdvariant::Dictionary, key: &str) -> &'a [u8] {
    match d.get(key) {
        Some(Variant::PackedByteArray(b)) => b,
        _ => &[],
    }
}

/// Element sizes and stream offsets (`mesh_surface_make_offsets_from_format`).
struct Layout {
    offsets: [usize; ARRAY_MAX as usize],
    vertex_size: usize,
    normal_size: usize,
    attrib_size: usize,
}

fn layout(format: u64, vertex_count: usize) -> Option<Layout> {
    let compressed = format & FLAG_COMPRESS_ATTRIBUTES != 0;
    let mut l = Layout {
        offsets: [0; ARRAY_MAX as usize],
        vertex_size: 0,
        normal_size: 0,
        attrib_size: 0,
    };
    #[derive(Clone, Copy)]
    enum Acc {
        None,
        Vertex,
        Normal,
        Attrib,
        Skin,
    }
    let mut acc = Acc::None;
    let mut skin_size = 0usize;
    for i in 0..ARRAY_MAX {
        match i {
            ARRAY_VERTEX => acc = Acc::Vertex,
            ARRAY_NORMAL => acc = Acc::Normal,
            ARRAY_COLOR => acc = Acc::Attrib,
            ARRAY_BONES => acc = Acc::Skin,
            _ => {}
        }
        if format & (1 << i) == 0 {
            continue;
        }
        let elem = match i {
            ARRAY_VERTEX => {
                if format & FLAG_USE_2D_VERTICES != 0 {
                    8
                } else if compressed {
                    8
                } else {
                    12
                }
            }
            ARRAY_NORMAL => 4,
            ARRAY_TANGENT => {
                if compressed {
                    0
                } else {
                    4
                }
            }
            ARRAY_COLOR => 4,
            ARRAY_TEX_UV | ARRAY_TEX_UV2 => {
                if compressed {
                    4
                } else {
                    8
                }
            }
            c if (ARRAY_CUSTOM0..ARRAY_CUSTOM0 + 4).contains(&c) => {
                let shift = FORMAT_CUSTOM_BASE + FORMAT_CUSTOM_BITS * (c - ARRAY_CUSTOM0);
                match (format >> shift) & 0x7 {
                    0 | 1 | 2 | 4 => 4, // RGBA8 unorm/snorm, RG half, R float
                    3 | 5 => 8,         // RGBA half, RG float
                    6 => 12,            // RGB float
                    7 => 16,            // RGBA float
                    _ => return None,
                }
            }
            ARRAY_WEIGHTS | ARRAY_BONES => {
                if format & FLAG_USE_8_BONE_WEIGHTS != 0 {
                    16
                } else {
                    8
                }
            }
            ARRAY_INDEX => continue,
            _ => return None,
        };
        let size = match acc {
            Acc::Vertex => &mut l.vertex_size,
            Acc::Normal => &mut l.normal_size,
            Acc::Attrib => &mut l.attrib_size,
            Acc::Skin => &mut skin_size,
            Acc::None => return None,
        };
        l.offsets[i as usize] = *size;
        *size += elem;
    }
    // The normal/tangent stream starts after the whole position stream.
    let shift = l.vertex_size * vertex_count;
    l.offsets[ARRAY_NORMAL as usize] += shift;
    l.offsets[ARRAY_TANGENT as usize] += shift;
    Some(l)
}

fn read_u16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn read_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn read_f32(b: &[u8], at: usize) -> Option<f32> {
    Some(f32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// `Vector3::octahedron_decode`.
fn octahedron_decode(x: f32, y: f32) -> Vector3 {
    let fx = x * 2.0 - 1.0;
    let fy = y * 2.0 - 1.0;
    let mut n = Vector3::new(fx, fy, 1.0 - fx.abs() - fy.abs());
    let t = (-n.z).clamp(0.0, 1.0);
    n.x += if n.x >= 0.0 { -t } else { t };
    n.y += if n.y >= 0.0 { -t } else { t };
    n.normalized()
}

/// `_get_tbn_from_axis_angle`, keeping only the normal (row 2 of the basis
/// rotating about `axis` by the decoded angle).
fn normal_from_axis_angle(axis: Vector3, angle01: f32) -> Vector3 {
    let angle = (angle01 * 2.0 - 1.0).abs() * std::f32::consts::PI;
    let basis = gdcore::math3d::Basis::from_quaternion(
        gdcore::math3d::Quaternion::from_axis_angle(axis, angle),
    );
    // Basis::rows[2] — the third row.
    Vector3::new(basis.x.z, basis.y.z, basis.z.z)
}

fn decode_surface(d: &gdvariant::Dictionary) -> Option<DecodedSurface> {
    let format = int(d, "format")? as u64;
    let version = (format >> FLAG_FORMAT_VERSION_SHIFT) & FLAG_FORMAT_VERSION_MASK;
    if version != FORMAT_VERSION_2 {
        // Version 1 (Godot 4.0/4.1) interleaves every attribute in one
        // record; Godot itself upgrades it on load. Not decoded here.
        tracing::warn!("ArrayMesh surface format version {version} is not supported");
        return None;
    }
    if format & (1 << ARRAY_VERTEX) == 0 {
        return None;
    }
    let count = usize::try_from(int(d, "vertex_count")?).ok()?;
    let vdata = bytes(d, "vertex_data");
    let adata = bytes(d, "attribute_data");
    let idata = bytes(d, "index_data");
    let l = layout(format, count)?;
    let compressed = format & FLAG_COMPRESS_ATTRIBUTES != 0;
    let two_d = format & FLAG_USE_2D_VERTICES != 0;

    let aabb = match d.get("aabb") {
        Some(Variant::Aabb(a)) => *a,
        _ => gdcore::math3d::Aabb::new(Vector3::ZERO, Vector3::ZERO),
    };
    let uv_scale = match d.get("uv_scale") {
        Some(Variant::Vector4(v)) => [v.x, v.y, v.z, v.w],
        _ => [0.0; 4],
    };

    let mut vertices = Vec::with_capacity(count);
    let mut normals = Vec::new();
    for j in 0..count {
        let at = j * l.vertex_size + l.offsets[ARRAY_VERTEX as usize];
        let v = if two_d {
            Vector3::new(read_f32(vdata, at)?, read_f32(vdata, at + 4)?, 0.0)
        } else if compressed {
            let q = |k: usize| read_u16(vdata, at + 2 * k).map(|x| x as f32 / 65535.0);
            Vector3::new(
                q(0)? * aabb.size.x + aabb.position.x,
                q(1)? * aabb.size.y + aabb.position.y,
                q(2)? * aabb.size.z + aabb.position.z,
            )
        } else {
            Vector3::new(
                read_f32(vdata, at)?,
                read_f32(vdata, at + 4)?,
                read_f32(vdata, at + 8)?,
            )
        };
        vertices.push(v);
    }

    if format & (1 << ARRAY_NORMAL) != 0 {
        normals.reserve(count);
        for j in 0..count {
            let w = read_u32(vdata, j * l.normal_size + l.offsets[ARRAY_NORMAL as usize])?;
            let axis = octahedron_decode(
                (w & 0xFFFF) as f32 / 65535.0,
                ((w >> 16) & 0xFFFF) as f32 / 65535.0,
            );
            let n = if compressed && !two_d {
                // The position record's 4th u16 lane is the tangent angle.
                let at = j * l.vertex_size + l.offsets[ARRAY_VERTEX as usize] + 6;
                normal_from_axis_angle(axis, read_u16(vdata, at)? as f32 / 65535.0)
            } else {
                axis
            };
            normals.push(n);
        }
    }

    let mut uvs = Vec::new();
    if format & (1 << ARRAY_TEX_UV) != 0 {
        uvs.reserve(count);
        for j in 0..count {
            let at = j * l.attrib_size + l.offsets[ARRAY_TEX_UV as usize];
            let uv = if compressed {
                let mut u = read_u16(adata, at)? as f32 / 65535.0;
                let mut v = read_u16(adata, at + 2)? as f32 / 65535.0;
                if uv_scale.iter().any(|s| s.abs() > f32::EPSILON) {
                    u = (u - 0.5) * uv_scale[0];
                    v = (v - 0.5) * uv_scale[1];
                }
                [u, v]
            } else {
                [read_f32(adata, at)?, read_f32(adata, at + 4)?]
            };
            uvs.push(uv);
        }
    }

    let mut indices: Vec<u32> = Vec::new();
    if format & (1 << ARRAY_INDEX) != 0 {
        let index_count = usize::try_from(int(d, "index_count").unwrap_or(0)).ok()?;
        let wide = !(count > 0 && count <= 1 << 16);
        indices.reserve(index_count);
        for j in 0..index_count {
            let i = if wide {
                read_u32(idata, j * 4)?
            } else {
                read_u16(idata, j * 2)? as u32
            };
            if i as usize >= count {
                tracing::warn!("ArrayMesh index {i} out of range ({count} vertices)");
                return None;
            }
            indices.push(i);
        }
    }

    let primitive = int(d, "primitive").unwrap_or(PRIMITIVE_TRIANGLES);
    let (primitive_type, indices) = match primitive {
        PRIMITIVE_POINTS => (PrimitiveType::Points, indices),
        PRIMITIVE_LINES => (PrimitiveType::Lines, indices),
        PRIMITIVE_TRIANGLES => (PrimitiveType::Triangles, indices),
        PRIMITIVE_LINE_STRIP => (PrimitiveType::Lines, strip_to_list(&indices, count, false)),
        PRIMITIVE_TRIANGLE_STRIP => (
            PrimitiveType::Triangles,
            strip_to_list(&indices, count, true),
        ),
        _ => return None,
    };

    let material = match d.get("material") {
        Some(Variant::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    let name = match d.get("name") {
        Some(Variant::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    Some(DecodedSurface {
        surface: Surface3D {
            vertices,
            normals,
            uvs,
            indices,
            primitive_type,
            bone_weights: Vec::new(),
            bone_indices: Vec::new(),
            lods: Vec::new(),
        },
        material,
        name,
    })
}

/// Expands a strip (indexed, or implicit `0..count` when unindexed) into an
/// explicit list, keeping Godot's clockwise winding for triangle strips by
/// swapping every odd triangle.
fn strip_to_list(indices: &[u32], count: usize, triangles: bool) -> Vec<u32> {
    let seq: Vec<u32> = if indices.is_empty() {
        (0..count as u32).collect()
    } else {
        indices.to_vec()
    };
    let mut out = Vec::new();
    if triangles {
        for k in 2..seq.len() {
            if k % 2 == 0 {
                out.extend_from_slice(&[seq[k - 2], seq[k - 1], seq[k]]);
            } else {
                out.extend_from_slice(&[seq[k - 1], seq[k - 2], seq[k]]);
            }
        }
    } else {
        for k in 1..seq.len() {
            out.extend_from_slice(&[seq[k - 1], seq[k]]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn platformer_3d(rel: &str) -> std::sync::Arc<Resource> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../games/platformer_3d")
            .join(rel);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        gdresource::load_resource_bytes(&bytes, &format!("res://{rel}")).unwrap()
    }

    /// `floor.res` is a 2×2 quad on the floor of a 2-unit cell (y = -1),
    /// facing up. Positions, UVs and the octahedral normals must all decode.
    #[test]
    fn real_floor_tile_decodes_to_an_upward_quad() {
        let res = platformer_3d("stage/meshes/floor.res");
        let mesh = mesh3d_from_array_mesh(&res).expect("floor.res decodes");
        assert_eq!(mesh.vertices.len(), 4);
        // The u16 index buffer exactly as stored in the file.
        assert_eq!(mesh.indices, vec![2, 0, 3, 2, 1, 0]);
        for v in &mesh.vertices {
            assert_eq!(v.y, -1.0, "cell floor at y=-1: {v:?}");
            assert_eq!(v.x.abs(), 1.0);
            assert_eq!(v.z.abs(), 1.0);
        }
        assert_eq!(mesh.normals.len(), 4);
        for n in &mesh.normals {
            assert!(n.y > 0.99, "floor normal points up: {n:?}");
        }
        assert_eq!(mesh.uvs.len(), 4);
        assert!(mesh
            .uvs
            .iter()
            .all(|uv| (0.0..=1.0).contains(&uv[0]) && (0.0..=1.0).contains(&uv[1])));
        assert_eq!(mesh.primitive_type, PrimitiveType::Triangles);
    }

    /// Every mesh the level's MeshLibrary uses must decode — each with
    /// in-range indices and unit normals — or that tile draws as a cube.
    #[test]
    fn every_real_tile_mesh_decodes() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../games/platformer_3d/stage/meshes");
        let mut decoded = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("res") {
                continue;
            }
            let rel = format!(
                "stage/meshes/{}",
                path.file_name().unwrap().to_str().unwrap()
            );
            let res = platformer_3d(&rel);
            let mesh =
                mesh3d_from_array_mesh(&res).unwrap_or_else(|| panic!("{rel} did not decode"));
            assert!(!mesh.vertices.is_empty(), "{rel}");
            assert_eq!(mesh.indices.len() % 3, 0, "{rel}");
            assert!(
                mesh.indices
                    .iter()
                    .all(|&i| (i as usize) < mesh.vertices.len()),
                "{rel}"
            );
            for n in &mesh.normals {
                assert!(
                    (n.length() - 1.0).abs() < 1e-3,
                    "{rel}: non-unit normal {n:?}"
                );
            }
            // Tiles are authored inside a 2-unit cell (some trunks overhang).
            for v in &mesh.vertices {
                assert!(
                    v.x.abs() <= 3.0 && v.y.abs() <= 6.0 && v.z.abs() <= 3.0,
                    "{rel}: {v:?}"
                );
            }
            decoded += 1;
        }
        assert_eq!(decoded, 18);
    }

    #[test]
    fn a_non_array_mesh_or_unknown_format_is_none() {
        let res = platformer_3d("stage/collision/corner.res");
        assert!(mesh3d_from_array_mesh(&res).is_none());

        // Format version 1 is refused rather than misread.
        let floor = platformer_3d("stage/meshes/floor.res");
        let mut copy = (*floor).clone();
        if let Some(Variant::Array(s)) = copy.get_property("_surfaces").cloned() {
            let surf = match s.get(0).unwrap() {
                Variant::Dictionary(d) => d.to_dict(),
                _ => unreachable!(),
            };
            let mut v1 = surf.clone();
            let fmt = match surf.get("format") {
                Some(Variant::Int(f)) => *f,
                _ => unreachable!(),
            };
            v1.insert(
                "format",
                Variant::Int(fmt & !(0xFF << FLAG_FORMAT_VERSION_SHIFT)),
            );
            copy.set_property(
                "_surfaces",
                Variant::Array(gdvariant::SharedArray::from_vec(vec![Variant::Dictionary(
                    gdvariant::SharedDictionary::from(v1),
                )])),
            );
        }
        assert!(mesh3d_from_array_mesh(&copy).is_none());
    }

    #[test]
    fn octahedron_decode_axes() {
        let up = octahedron_decode(0.5, 1.0);
        assert!((up.y - 1.0).abs() < 1e-6, "{up:?}");
        let fwd = octahedron_decode(0.5, 0.5);
        assert!((fwd.z - 1.0).abs() < 1e-6, "{fwd:?}");
    }

    #[test]
    fn triangle_strip_expands_with_alternating_winding() {
        assert_eq!(strip_to_list(&[], 4, true), vec![0, 1, 2, 2, 1, 3]);
        assert_eq!(strip_to_list(&[5, 6, 7], 8, false), vec![5, 6, 6, 7]);
    }
}

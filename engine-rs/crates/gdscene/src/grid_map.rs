//! GridMap runtime (pat-p0s82.16).

use std::collections::HashMap;

use gdcore::error::EngineResult;
use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Transform3D};
use gdvariant::Variant;

use crate::packed_scene::PackedScene;

/// Sentinel item id used by Godot for an empty cell (`GridMap::INVALID_CELL_ITEM`).
const INVALID_CELL_ITEM: i32 = 0xFFFF;

/// Mesh library entry.
#[derive(Debug, Clone)]
pub struct MeshLibraryItem {
    pub name: String,
    pub mesh_path: String,
    /// `item/<id>/mesh_transform`: where the mesh sits inside its cell. Godot
    /// draws a cell's mesh at `cell_xform * mesh_transform` (grid_map.cpp,
    /// `_octant_update`), so a library that offsets or rotates a tile mesh
    /// relies on this. Identity when unauthored.
    pub mesh_transform: Transform3D,
    /// `item/<id>/shapes`: the item's collision shapes as `(shape resource
    /// path, transform in the cell)`, in authored order. Godot's flat
    /// `[Shape3D, Transform3D, Shape3D, Transform3D, ...]` array, paired up.
    ///
    /// ⚠️ NOT affected by `mesh_transform` — Godot places shapes at
    /// `cell_xform * shape_xform` only.
    pub shapes: Vec<(String, Transform3D)>,
}

/// MeshLibrary resource (subset).
#[derive(Debug, Clone, Default)]
pub struct MeshLibrary {
    items: HashMap<i32, MeshLibraryItem>,
}

impl MeshLibrary {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set_item(&mut self, id: i32, name: impl Into<String>, mesh_path: impl Into<String>) {
        self.items.insert(
            id,
            MeshLibraryItem {
                name: name.into(),
                mesh_path: mesh_path.into(),
                mesh_transform: Transform3D::IDENTITY,
                shapes: Vec::new(),
            },
        );
    }
    pub fn get_item(&self, id: i32) -> Option<&MeshLibraryItem> {
        self.items.get(&id)
    }
    /// Loads a Godot `MeshLibrary` `.tres` (pat-s3rrv.89).
    ///
    /// A `GridMap` node references its library by PATH; nothing ever turned that
    /// path into items, so `mesh_instances()` returned empty for every cell and
    /// a populated grid could not resolve to geometry.
    ///
    /// Godot writes one flat property block per item — `item/<id>/name`,
    /// `item/<id>/mesh`, plus `mesh_transform` / `shapes` / `navigation_mesh` /
    /// `preview`. The name, mesh path, `mesh_transform` and `shapes` are read
    /// (pat-2ifdx); navigation and preview are not consumed yet.
    ///
    /// An item with a name but **no** mesh is skipped: it contributes no
    /// geometry, and admitting it would put an entry with an empty path into the
    /// library for the render path to try to load.
    ///
    /// Returns `Ok(None)` when the resource is not a `MeshLibrary`.
    pub fn from_tres(source: &str) -> EngineResult<Option<MeshLibrary>> {
        let resource = gdresource::TresLoader::new().parse_str(source, "res://meshlibrary.tres")?;
        Ok(Self::from_resource(&resource))
    }

    /// Loads a MeshLibrary file in either format — text `.tres` or binary
    /// `.res` (pat-33wmr) — from a `res://` or filesystem path. `None` when
    /// it cannot be read or is not a `MeshLibrary`.
    pub fn load(path: &str) -> Option<MeshLibrary> {
        let resource = crate::array_mesh::read_resource_file(path)?;
        Self::from_resource(&resource)
    }

    /// [`from_tres`](Self::from_tres) for an already-loaded resource.
    pub fn from_resource(resource: &gdresource::Resource) -> Option<MeshLibrary> {
        if resource.class_name != "MeshLibrary" {
            return None;
        }

        // Collect both fields before building items: `properties()` iterates a
        // HashMap, so `name` and `mesh` arrive in either order.
        let mut names: HashMap<i32, String> = HashMap::new();
        let mut meshes: HashMap<i32, String> = HashMap::new();
        let mut mesh_transforms: HashMap<i32, Transform3D> = HashMap::new();
        let mut shapes: HashMap<i32, Vec<(String, Transform3D)>> = HashMap::new();
        // `ExtResource("id")` arrives as `"ExtResource:<id>"`; the path lives on
        // the header entry.
        let ext_path = |reference: &str| -> Option<String> {
            let ext_id = reference.strip_prefix("ExtResource:")?;
            resource.ext_resources.get(ext_id).map(|e| e.path.clone())
        };
        for (key, value) in resource.properties() {
            let Some(rest) = key.strip_prefix("item/") else {
                continue;
            };
            let Some((id_text, field)) = rest.split_once('/') else {
                continue;
            };
            let Ok(id) = id_text.parse::<i32>() else {
                continue;
            };
            match (field, value) {
                ("name", Variant::String(name)) => {
                    names.insert(id, name.clone());
                }
                ("mesh", Variant::String(reference)) => {
                    if let Some(path) = ext_path(reference) {
                        meshes.insert(id, path);
                    }
                }
                ("mesh_transform", Variant::Transform3D(t)) => {
                    mesh_transforms.insert(id, *t);
                }
                // `[Shape3D, Transform3D, Shape3D, Transform3D, ...]`. A shape
                // without a following transform sits at the cell centre, which
                // is what Godot's setter does with a short array.
                ("shapes", Variant::Array(items)) => {
                    let items = items.to_vec();
                    let mut list = Vec::new();
                    let mut i = 0;
                    while i < items.len() {
                        let path = match &items[i] {
                            Variant::String(r) => ext_path(r),
                            _ => None,
                        };
                        let xform = match items.get(i + 1) {
                            Some(Variant::Transform3D(t)) => *t,
                            _ => Transform3D::IDENTITY,
                        };
                        if let Some(path) = path {
                            list.push((path, xform));
                        }
                        i += 2;
                    }
                    shapes.insert(id, list);
                }
                _ => {}
            }
        }

        let mut library = MeshLibrary::new();
        for (id, mesh_path) in meshes {
            let name = names.get(&id).cloned().unwrap_or_default();
            library.set_item(id, name, mesh_path);
            if let Some(item) = library.items.get_mut(&id) {
                if let Some(t) = mesh_transforms.get(&id) {
                    item.mesh_transform = *t;
                }
                if let Some(list) = shapes.remove(&id) {
                    item.shapes = list;
                }
            }
        }
        Some(library)
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }
}

/// GridMap cell key (x,y,z).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GridCell {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// Godot's 24 orthogonal cell orientations (`_ortho_bases`, grid_map.cpp:461).
///
/// ⚠️ **TRANSPOSED on the way in, deliberately.** Godot's nine-argument
/// `Basis(xx, xy, xz, yx, yy, yz, zx, zy, zz)` constructor fills ROWS
/// (`core/math/basis.h:211`), while this engine's `Basis` stores COLUMNS
/// (`x`/`y`/`z` are the column vectors). Copying the numbers across in source
/// order would transpose every rotation — and entry 0 is the identity, which
/// reads correct under either convention, so the mistake would survive any test
/// that only checked an unrotated cell. [`orientation_basis`] has a test that
/// pins an asymmetric entry for exactly that reason.
const ORTHO_BASES: [Basis; 24] = [
    Basis {
        x: Vector3::new(1.0, 0.0, 0.0),
        y: Vector3::new(0.0, 1.0, 0.0),
        z: Vector3::new(0.0, 0.0, 1.0),
    },
    Basis {
        x: Vector3::new(0.0, 1.0, 0.0),
        y: Vector3::new(-1.0, 0.0, 0.0),
        z: Vector3::new(0.0, 0.0, 1.0),
    },
    Basis {
        x: Vector3::new(-1.0, 0.0, 0.0),
        y: Vector3::new(0.0, -1.0, 0.0),
        z: Vector3::new(0.0, 0.0, 1.0),
    },
    Basis {
        x: Vector3::new(0.0, -1.0, 0.0),
        y: Vector3::new(1.0, 0.0, 0.0),
        z: Vector3::new(0.0, 0.0, 1.0),
    },
    Basis {
        x: Vector3::new(1.0, 0.0, 0.0),
        y: Vector3::new(0.0, 0.0, 1.0),
        z: Vector3::new(0.0, -1.0, 0.0),
    },
    Basis {
        x: Vector3::new(0.0, 1.0, 0.0),
        y: Vector3::new(0.0, 0.0, 1.0),
        z: Vector3::new(1.0, 0.0, 0.0),
    },
    Basis {
        x: Vector3::new(-1.0, 0.0, 0.0),
        y: Vector3::new(0.0, 0.0, 1.0),
        z: Vector3::new(0.0, 1.0, 0.0),
    },
    Basis {
        x: Vector3::new(0.0, -1.0, 0.0),
        y: Vector3::new(0.0, 0.0, 1.0),
        z: Vector3::new(-1.0, 0.0, 0.0),
    },
    Basis {
        x: Vector3::new(1.0, 0.0, 0.0),
        y: Vector3::new(0.0, -1.0, 0.0),
        z: Vector3::new(0.0, 0.0, -1.0),
    },
    Basis {
        x: Vector3::new(0.0, 1.0, 0.0),
        y: Vector3::new(1.0, 0.0, 0.0),
        z: Vector3::new(0.0, 0.0, -1.0),
    },
    Basis {
        x: Vector3::new(-1.0, 0.0, 0.0),
        y: Vector3::new(0.0, 1.0, 0.0),
        z: Vector3::new(0.0, 0.0, -1.0),
    },
    Basis {
        x: Vector3::new(0.0, -1.0, 0.0),
        y: Vector3::new(-1.0, 0.0, 0.0),
        z: Vector3::new(0.0, 0.0, -1.0),
    },
    Basis {
        x: Vector3::new(1.0, 0.0, 0.0),
        y: Vector3::new(0.0, 0.0, -1.0),
        z: Vector3::new(0.0, 1.0, 0.0),
    },
    Basis {
        x: Vector3::new(0.0, 1.0, 0.0),
        y: Vector3::new(0.0, 0.0, -1.0),
        z: Vector3::new(-1.0, 0.0, 0.0),
    },
    Basis {
        x: Vector3::new(-1.0, 0.0, 0.0),
        y: Vector3::new(0.0, 0.0, -1.0),
        z: Vector3::new(0.0, -1.0, 0.0),
    },
    Basis {
        x: Vector3::new(0.0, -1.0, 0.0),
        y: Vector3::new(0.0, 0.0, -1.0),
        z: Vector3::new(1.0, 0.0, 0.0),
    },
    Basis {
        x: Vector3::new(0.0, 0.0, -1.0),
        y: Vector3::new(0.0, 1.0, 0.0),
        z: Vector3::new(1.0, 0.0, 0.0),
    },
    Basis {
        x: Vector3::new(0.0, 0.0, -1.0),
        y: Vector3::new(-1.0, 0.0, 0.0),
        z: Vector3::new(0.0, 1.0, 0.0),
    },
    Basis {
        x: Vector3::new(0.0, 0.0, -1.0),
        y: Vector3::new(0.0, -1.0, 0.0),
        z: Vector3::new(-1.0, 0.0, 0.0),
    },
    Basis {
        x: Vector3::new(0.0, 0.0, -1.0),
        y: Vector3::new(1.0, 0.0, 0.0),
        z: Vector3::new(0.0, -1.0, 0.0),
    },
    Basis {
        x: Vector3::new(0.0, 0.0, 1.0),
        y: Vector3::new(0.0, -1.0, 0.0),
        z: Vector3::new(1.0, 0.0, 0.0),
    },
    Basis {
        x: Vector3::new(0.0, 0.0, 1.0),
        y: Vector3::new(1.0, 0.0, 0.0),
        z: Vector3::new(0.0, 1.0, 0.0),
    },
    Basis {
        x: Vector3::new(0.0, 0.0, 1.0),
        y: Vector3::new(0.0, 1.0, 0.0),
        z: Vector3::new(-1.0, 0.0, 0.0),
    },
    Basis {
        x: Vector3::new(0.0, 0.0, 1.0),
        y: Vector3::new(-1.0, 0.0, 0.0),
        z: Vector3::new(0.0, -1.0, 0.0),
    },
];

/// The rotation for Godot's 24-way orthogonal cell orientation index.
///
/// Out-of-range values answer with the identity rather than panicking: the field
/// is 5 bits and can physically hold 0..31, so a hand-authored or corrupt scene
/// can carry 24..31, and a malformed cell should draw unrotated rather than take
/// the editor down.
pub fn orientation_basis(orientation: u8) -> Basis {
    ORTHO_BASES
        .get(orientation as usize)
        .copied()
        .unwrap_or(Basis::IDENTITY)
}

/// One decoded `data/cells` entry, including the orientation and layer that
/// [`GridMap::decode_cells`]'s `(cell, item)` form drops (pat-0q2kx.196).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedCell {
    /// Grid coordinate.
    pub cell: GridCell,
    /// MeshLibrary item id.
    pub item: i32,
    /// Godot's 24-way orthogonal orientation index (`rot`, 5 bits).
    pub orientation: u8,
    /// Godot's cell layer (8 bits).
    pub layer: u8,
}

/// GridMap node data.
#[derive(Debug, Clone, Default)]
pub struct GridMap {
    pub cell_size: f32,
    pub mesh_library: MeshLibrary,
    /// Resource path (`res://…`) of the `mesh_library` referenced by the scene,
    /// captured on `.tscn` load. The `MeshLibrary` contents themselves live in a
    /// separate resource file and are populated by a future MeshLibrary loader.
    pub mesh_library_resource: Option<String>,
    /// `cell_center_x/y/z` (all `true` in Godot). A centred axis places the
    /// cell's origin half a cell in from its corner — Godot's `_get_offset`.
    pub cell_center: [bool; 3],
    cells: HashMap<GridCell, i32>,
    /// Per-cell orientation; absent means Godot's default of 0.
    orientations: HashMap<GridCell, u8>,
}

impl GridMap {
    pub fn new() -> Self {
        Self {
            cell_size: 2.0,
            mesh_library: MeshLibrary::new(),
            mesh_library_resource: None,
            cell_center: [true; 3],
            cells: HashMap::new(),
            orientations: HashMap::new(),
        }
    }

    /// Godot's `_get_offset`: half a cell on every axis whose `cell_center_*`
    /// is set.
    pub fn cell_offset(&self) -> Vector3 {
        let half = |on: bool| if on { self.cell_size * 0.5 } else { 0.0 };
        Vector3::new(
            half(self.cell_center[0]),
            half(self.cell_center[1]),
            half(self.cell_center[2]),
        )
    }

    /// Godot's `map_to_local`: the cell's origin in the GridMap's local space.
    pub fn map_to_local(&self, cell: GridCell) -> Vector3 {
        Vector3::new(
            cell.x as f32 * self.cell_size,
            cell.y as f32 * self.cell_size,
            cell.z as f32 * self.cell_size,
        ) + self.cell_offset()
    }

    /// The cell's placement in GridMap-local space: its orientation basis at
    /// [`map_to_local`](Self::map_to_local). Godot builds exactly this in
    /// `_octant_update` (`xform.basis = _ortho_bases[rot]; xform.origin =
    /// cellpos * cell_size + ofs`) and composes both the item's
    /// `mesh_transform` and every shape transform onto it.
    pub fn cell_transform(&self, cell: GridCell) -> Transform3D {
        Transform3D {
            basis: orientation_basis(self.cell_orientation(cell)),
            origin: self.map_to_local(cell),
        }
    }

    /// Every collision shape the used cells contribute, in GridMap-local space:
    /// `(cell_xform * shape_xform, shape resource path)` per item shape
    /// (pat-2ifdx). Empty until a library with `item/<id>/shapes` is loaded.
    ///
    /// Sorted by cell so the order — and so the physics body ids built from
    /// it — is deterministic despite the cell `HashMap`.
    pub fn collision_shapes(&self) -> Vec<(Transform3D, String)> {
        let mut cells: Vec<(&GridCell, &i32)> = self.cells.iter().collect();
        cells.sort_by_key(|(c, _)| (c.x, c.y, c.z));
        let mut out = Vec::new();
        for (cell, id) in cells {
            let Some(item) = self.mesh_library.get_item(*id) else {
                continue;
            };
            let xform = self.cell_transform(*cell);
            for (path, shape_xform) in &item.shapes {
                out.push((xform * *shape_xform, path.clone()));
            }
        }
        out
    }

    pub fn set_cell_item(&mut self, cell: GridCell, item: i32) {
        if item < 0 {
            self.cells.remove(&cell);
            // Clear the orientation with the cell. Leaving it behind would let a
            // cleared-then-reset cell inherit a stale rotation (pat-0q2kx.196).
            self.orientations.remove(&cell);
        } else {
            self.cells.insert(cell, item);
        }
    }

    pub fn get_cell_item(&self, cell: GridCell) -> i32 {
        self.cells.get(&cell).copied().unwrap_or(-1)
    }

    pub fn used_cells(&self) -> Vec<GridCell> {
        self.cells.keys().copied().collect()
    }

    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// Instance list for rendering: (world position, mesh path).
    pub fn mesh_instances(&self) -> Vec<(gdcore::math::Vector3, String)> {
        let mut out = Vec::new();
        for (cell, id) in &self.cells {
            if let Some(item) = self.mesh_library.get_item(*id) {
                out.push((self.map_to_local(*cell), item.mesh_path.clone()));
            }
        }
        out
    }

    /// Instance list for rendering, keeping each cell's rotation:
    /// `(world position, basis, mesh path)`.
    ///
    /// ⚠️ Prefer this over [`mesh_instances`](Self::mesh_instances) in a render
    /// path. That one drops the orientation, so every cell draws unrotated —
    /// which on a tiled map reads as the art being authored wrong rather than as
    /// missing data.
    pub fn mesh_instances_oriented(&self) -> Vec<(Vector3, Basis, String)> {
        let mut out = Vec::new();
        for (cell, id) in &self.cells {
            if let Some(item) = self.mesh_library.get_item(*id) {
                // 🔑 Godot draws the mesh at `cell_xform * mesh_transform`,
                // where `cell_xform.origin` is the cell CENTRE (`map_to_local`,
                // half a cell in from the corner on every `cell_center_*`
                // axis). Placing it at `cell * cell_size` shifted the whole
                // level by half a cell against its own collision (pat-2ifdx).
                let t = self.cell_transform(*cell) * item.mesh_transform;
                out.push((t.origin, t.basis, item.mesh_path.clone()));
            }
        }
        out
    }

    /// Decodes Godot's `data/cells` `PackedInt32Array` encoding into
    /// `(cell, item)` pairs. Godot stores each cell as three consecutive
    /// `int32`s: the low and high halves of a 64-bit key packing three signed
    /// `int16` coordinates (`x | y<<16 | z<<32`), followed by a packed cell
    /// value whose low 16 bits are the mesh-library item id. Empty cells
    /// (`item == INVALID_CELL_ITEM`) and any trailing partial triple are
    /// skipped.
    ///
    /// This short form **discards the orientation and layer** the packed value
    /// also carries. Use [`decode_cells_full`](Self::decode_cells_full) to keep
    /// them — the render path needs the rotation, or every cell draws unrotated.
    pub fn decode_cells(data: &[i32]) -> Vec<(GridCell, i32)> {
        Self::decode_cells_full(data)
            .into_iter()
            .map(|c| (c.cell, c.item))
            .collect()
    }

    /// [`decode_cells`](Self::decode_cells) keeping the **orientation and layer**
    /// the short form discards (pat-0q2kx.196).
    ///
    /// Godot packs the cell value as a bitfield (`modules/gridmap/grid_map.h`):
    /// `item : 16`, `rot : 5`, `layer : 8`. Masking `value & 0xFFFF` for the item
    /// therefore throws away the rotation — which is why every GridMap cell would
    /// render unrotated even once the render path expands them, with no error to
    /// notice.
    ///
    /// `rot` indexes Godot's 24 orthogonal orientations. The field is 5 bits, so
    /// it can physically hold 0..31; values above 23 are not valid orientations
    /// but are preserved verbatim rather than clamped, so a malformed scene
    /// round-trips unchanged instead of being silently "corrected".
    pub fn decode_cells_full(data: &[i32]) -> Vec<DecodedCell> {
        let mut out = Vec::new();
        for chunk in data.chunks_exact(3) {
            let lo = chunk[0] as u32;
            let hi = chunk[1] as u32;
            let value = chunk[2] as u32;

            let x = (lo & 0xFFFF) as u16 as i16 as i32;
            let y = ((lo >> 16) & 0xFFFF) as u16 as i16 as i32;
            let z = (hi & 0xFFFF) as u16 as i16 as i32;
            let item = (value & 0xFFFF) as i32;

            if item == INVALID_CELL_ITEM {
                continue;
            }
            out.push(DecodedCell {
                cell: GridCell { x, y, z },
                item,
                orientation: ((value >> 16) & 0x1F) as u8,
                layer: ((value >> 21) & 0xFF) as u8,
            });
        }
        out
    }

    /// Applies a decoded `data/cells` array to this GridMap.
    pub fn load_cells(&mut self, data: &[i32]) {
        // Uses the full decode so the orientation survives the load. Going
        // through `decode_cells` here would drop it again at the last step —
        // the whole point of decoding it (pat-0q2kx.196).
        for decoded in Self::decode_cells_full(data) {
            self.set_cell_item_oriented(decoded.cell, decoded.item, decoded.orientation);
        }
    }

    /// [`set_cell_item`](Self::set_cell_item) with Godot's 24-way orientation.
    ///
    /// Clearing a cell (`item < 0`) drops its orientation too, so a cleared and
    /// re-set cell cannot inherit a stale rotation.
    pub fn set_cell_item_oriented(&mut self, cell: GridCell, item: i32, orientation: u8) {
        self.set_cell_item(cell, item);
        if item < 0 {
            self.orientations.remove(&cell);
        } else if orientation != 0 {
            self.orientations.insert(cell, orientation);
        } else {
            // Godot's default orientation is 0; storing it would only bloat the
            // map, and absence already means 0.
            self.orientations.remove(&cell);
        }
    }

    /// The cell's orientation index, `0` (unrotated) when it has none.
    pub fn cell_orientation(&self, cell: GridCell) -> u8 {
        self.orientations.get(&cell).copied().unwrap_or(0)
    }

    /// Loads the first `GridMap` node found in a `.tscn` scene source into a
    /// runtime `GridMap`. Reads `cell_size`, the `mesh_library` external
    /// resource reference, and the `data/cells` map. Returns `Ok(None)` when the
    /// scene contains no `GridMap` node.
    pub fn from_tscn(source: &str) -> EngineResult<Option<GridMap>> {
        let scene = PackedScene::from_tscn(source)?;
        let nodes = scene.instance()?;

        let node = match nodes.iter().find(|n| n.class_name() == "GridMap") {
            Some(n) => n,
            None => return Ok(None),
        };

        let mut grid = GridMap::new();

        // cell_size: Godot stores a Vector3 (per-axis); this subset keeps a
        // single uniform size and takes the x component.
        if let Variant::Vector3(v) = node.get_property("cell_size") {
            grid.cell_size = v.x;
        }
        grid.cell_center = read_cell_center(node);

        // mesh_library: `ExtResource("id")` -> resolved `res://` path.
        if let Variant::String(ref ext_ref) = node.get_property("mesh_library") {
            if let Some(path) = scene.resolve_ext_resource_path(ext_ref) {
                grid.mesh_library_resource = Some(path.to_string());
            }
        }

        // data = { "cells": PackedInt32Array(...) }
        if let Variant::Dictionary(dict) = node.get_property("data") {
            if let Some(Variant::PackedInt32Array(cells)) = dict.get("cells") {
                grid.load_cells(&cells);
            }
        }

        Ok(Some(grid))
    }

    /// Loads a `GridMap` from a **live scene node**, the form the editor and
    /// Play paths actually hold (pat-s3rrv.89).
    ///
    /// [`GridMap::from_tscn`] parses `.tscn` *source* and resolves the mesh
    /// library through the `PackedScene`. By render time that scene is gone and
    /// only the `SceneTree` remains, so nothing on the runtime side could build
    /// a `GridMap` at all — which is why the render list has never emitted one.
    ///
    /// The property reads mirror `from_tscn` exactly, with one difference:
    /// `mesh_library` on an instanced node is an `ExtResource` reference whose
    /// resolved path `packed_scene` parked in the `__extres__mesh_library`
    /// companion, so that is preferred and the raw value is the fallback.
    ///
    /// Returns `None` for a node that is not a `GridMap`.
    pub fn from_node(
        tree: &crate::scene_tree::SceneTree,
        node_id: crate::node::NodeId,
    ) -> Option<GridMap> {
        let node = tree.get_node(node_id)?;
        if node.class_name() != "GridMap" {
            return None;
        }

        let mut grid = GridMap::new();

        // Godot stores a per-axis Vector3; this subset keeps one uniform size
        // and takes x, exactly as `from_tscn` does.
        if let Variant::Vector3(v) = node.get_property("cell_size") {
            grid.cell_size = v.x;
        }
        grid.cell_center = read_cell_center(node);

        match node.get_property("__extres__mesh_library") {
            Variant::String(path) => grid.mesh_library_resource = Some(path),
            // No companion: an un-instanced or hand-built node may carry the
            // path directly. An unresolved `ExtResource("…")` reference is NOT
            // a path, so it is rejected rather than stored as one.
            _ => {
                if let Variant::String(raw) = node.get_property("mesh_library") {
                    if !raw.starts_with("ExtResource(") && !raw.starts_with("SubResource:") {
                        grid.mesh_library_resource = Some(raw);
                    }
                }
            }
        }

        // `data = { "cells": PackedInt32Array(...) }`.
        if let Variant::Dictionary(dict) = node.get_property("data") {
            if let Some(Variant::PackedInt32Array(cells)) = dict.get("cells") {
                grid.load_cells(&cells);
            }
        }

        Some(grid)
    }
}

/// `cell_center_x/y/z`, each defaulting to Godot's `true` when absent or not a
/// bool.
fn read_cell_center(node: &crate::node::Node) -> [bool; 3] {
    let on = |key: &str| !matches!(node.get_property(key), Variant::Bool(false));
    [on("cell_center_x"), on("cell_center_y"), on("cell_center_z")]
}

// ---------------------------------------------------------------------------
// Script-facing method dispatch (pat-mgixk.93)
// ---------------------------------------------------------------------------

/// Re-encodes one cell into Godot's `data/cells` triple.
///
/// The exact inverse of [`GridMap::decode_cells_full`]: `item : 16`, `rot : 5`,
/// `layer : 8`, with the three signed `int16` coordinates split across the
/// first two words. `item < 0` encodes to `0xFFFF`, the sentinel the decoder
/// skips, because `-1i32 as u32 & 0xFFFF` is exactly `INVALID_CELL_ITEM`.
fn encode_cell_triple(cell: GridCell, item: i32, orientation: u8, layer: u8) -> [i32; 3] {
    let lo = (cell.x as i16 as u16 as u32) | ((cell.y as i16 as u16 as u32) << 16);
    let hi = cell.z as i16 as u16 as u32;
    let value = (item as u32 & 0xFFFF)
        | ((orientation as u32 & 0x1F) << 16)
        | ((layer as u32 & 0xFF) << 21);
    [lo as i32, hi as i32, value as i32]
}

/// Decodes just the coordinates of the triple starting at `i`.
fn decode_triple_cell(data: &[i32], i: usize) -> GridCell {
    let lo = data[i] as u32;
    let hi = data[i + 1] as u32;
    GridCell {
        x: (lo & 0xFFFF) as u16 as i16 as i32,
        y: ((lo >> 16) & 0xFFFF) as u16 as i16 as i32,
        z: (hi & 0xFFFF) as u16 as i16 as i32,
    }
}

/// Applies one `set_cell_item` to a raw `data/cells` array **in place**.
///
/// 🔑 Surgical on purpose. Re-encoding the whole array from a [`GridMap`] would
/// be simpler and silently lossy: [`GridMap::load_cells`] drops each cell's
/// `layer` field (it passes only cell/item/orientation to
/// `set_cell_item_oriented`), so a round trip through the struct would zero the
/// layer of every cell in the scene — including cells this call never touched.
/// Editing the one triple keeps every other cell's bits verbatim, and preserves
/// the edited cell's own layer by reading it back out of the old value.
fn apply_set_cell_item(data: &mut Vec<i32>, cell: GridCell, item: i32, orientation: u8) {
    let mut i = 0;
    while i + 3 <= data.len() {
        if decode_triple_cell(data, i) == cell {
            if item < 0 {
                // Godot erases the cell rather than storing a negative item.
                data.drain(i..i + 3);
            } else {
                let layer = ((data[i + 2] as u32 >> 21) & 0xFF) as u8;
                let triple = encode_cell_triple(cell, item, orientation, layer);
                data[i..i + 3].copy_from_slice(&triple);
            }
            return;
        }
        i += 3;
    }
    // Not present: appending an erase would write a cell the decoder skips, so
    // only a real item is added.
    if item >= 0 {
        data.extend_from_slice(&encode_cell_triple(cell, item, orientation, 0));
    }
}

/// Reads the node's raw `data/cells` array.
fn node_cell_data(tree: &crate::scene_tree::SceneTree, node_id: crate::node::NodeId) -> Vec<i32> {
    match tree.get_node(node_id).map(|n| n.get_property("data")) {
        // `SharedDictionary::get` hands back an OWNED Variant, not a reference.
        Some(Variant::Dictionary(dict)) => match dict.get("cells") {
            Some(Variant::PackedInt32Array(cells)) => cells,
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// Writes `cells` back onto the node's `data` dictionary.
///
/// This is the whole wiring requirement: `GridMap::from_node` — which the render
/// path goes through — reads `data = { "cells": PackedInt32Array(...) }`, so a
/// script's `set_cell_item` only reaches the next frame's render snapshot by
/// landing here. Any other `data` keys the scene authored are preserved.
fn write_node_cell_data(
    tree: &mut crate::scene_tree::SceneTree,
    node_id: crate::node::NodeId,
    cells: &[i32],
) {
    let existing = tree.get_node(node_id).map(|n| n.get_property("data"));
    let dict = match existing {
        Some(Variant::Dictionary(d)) => d,
        _ => gdvariant::SharedDictionary::new(),
    };
    // ⚠️ `SharedDictionary::insert` takes `&self` (interior mutability) and
    // RETURNS A RESULT: it refuses a read-only or key-typed dictionary. Dropping
    // that error would leave the cells unwritten while every accessor still
    // answered from the old array — a set_cell_item that reports success and
    // changes nothing. A refusal falls back to a fresh dictionary so the write
    // stays observable.
    let value = Variant::PackedInt32Array(cells.to_vec());
    if dict.insert("cells", value.clone()).is_err() {
        let fresh = gdvariant::SharedDictionary::new();
        let _ = fresh.insert("cells", value);
        if let Some(node) = tree.get_node_mut(node_id) {
            node.set_property("data", Variant::Dictionary(fresh));
        }
        return;
    }
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("data", Variant::Dictionary(dict));
    }
}

/// The node's uniform cell size. Godot stores a per-axis `Vector3`; this engine
/// keeps one uniform size and takes `x`, exactly as `GridMap::from_node` does.
fn node_cell_size(tree: &crate::scene_tree::SceneTree, node_id: crate::node::NodeId) -> f32 {
    match tree.get_node(node_id).map(|n| n.get_property("cell_size")) {
        Some(Variant::Vector3(v)) if v.x != 0.0 => v.x,
        _ => GridMap::new().cell_size,
    }
}

/// Godot's `_get_offset`: half a cell on each axis whose `cell_center_*` is set.
///
/// ⚠️ ClassDB seeds `cell_center_x/y/z` to `true` (`class_db.rs:15903-15905`),
/// matching Godot, so an unauthored GridMap centres — reading the property is
/// therefore correct rather than merely defaulting.
fn node_cell_offset(tree: &crate::scene_tree::SceneTree, node_id: crate::node::NodeId) -> Vector3 {
    let cs = node_cell_size(tree, node_id);
    let centered = |key: &str| match tree.get_node(node_id).map(|n| n.get_property(key)) {
        Some(Variant::Bool(b)) => b,
        // Absent reads as Godot's default of true.
        _ => true,
    };
    Vector3::new(
        if centered("cell_center_x") {
            cs * 0.5
        } else {
            0.0
        },
        if centered("cell_center_y") {
            cs * 0.5
        } else {
            0.0
        },
        if centered("cell_center_z") {
            cs * 0.5
        } else {
            0.0
        },
    )
}

/// Dispatches `GridMap`'s registered methods, declining every other class.
///
/// # What was broken
///
/// `set_cell_item`, `get_cell_item`, `get_used_cells`, `local_to_map` and the
/// rest are all registered in ClassDB (`class_db.rs:15129`), so `has_method`
/// answered **true** and a call returned **Nil** — the silent failure this
/// sweep exists to close. The implementations already existed on [`GridMap`];
/// nothing connected them to a script.
pub fn call_gridmap_method(
    tree: &mut crate::scene_tree::SceneTree,
    node_id: crate::node::NodeId,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    if tree.get_node(node_id)?.class_name() != "GridMap" {
        return None;
    }

    // A script may hand a float `Vector3` to a coordinate parameter; Godot
    // coerces rather than failing.
    let cell_arg = |i: usize| -> GridCell {
        match args.get(i) {
            Some(Variant::Vector3i(v)) => GridCell {
                x: v.x,
                y: v.y,
                z: v.z,
            },
            Some(Variant::Vector3(v)) => GridCell {
                x: v.x as i32,
                y: v.y as i32,
                z: v.z as i32,
            },
            _ => GridCell { x: 0, y: 0, z: 0 },
        }
    };
    let int_arg = |i: usize, default: i32| -> i32 {
        match args.get(i) {
            Some(Variant::Int(v)) => *v as i32,
            Some(Variant::Float(f)) => *f as i32,
            _ => default,
        }
    };

    Some(match method {
        "set_cell_item" => {
            let cell = cell_arg(0);
            // Godot's signature is (position, item, orientation = 0).
            let item = int_arg(1, -1);
            let orientation = int_arg(2, 0).clamp(0, 31) as u8;
            let mut data = node_cell_data(tree, node_id);
            apply_set_cell_item(&mut data, cell, item, orientation);
            write_node_cell_data(tree, node_id, &data);
            Variant::Nil
        }
        "get_cell_item" => {
            let cell = cell_arg(0);
            let grid = GridMap::from_node(tree, node_id)?;
            // Already Godot's script-facing `INVALID_CELL_ITEM` (-1) for an
            // empty cell; the 0xFFFF sentinel is the packed form only.
            Variant::Int(grid.get_cell_item(cell) as i64)
        }
        "get_cell_item_orientation" => {
            let cell = cell_arg(0);
            let grid = GridMap::from_node(tree, node_id)?;
            // Godot answers -1 for an empty cell, not orientation 0 — which is a
            // real orientation and would read as "present, unrotated".
            if grid.get_cell_item(cell) < 0 {
                Variant::Int(-1)
            } else {
                Variant::Int(grid.cell_orientation(cell) as i64)
            }
        }
        "get_cell_item_basis" => {
            let cell = cell_arg(0);
            let grid = GridMap::from_node(tree, node_id)?;
            Variant::Basis(orientation_basis(grid.cell_orientation(cell)))
        }
        "get_used_cells" => {
            let grid = GridMap::from_node(tree, node_id)?;
            Variant::Array(
                grid.used_cells()
                    .into_iter()
                    .map(|c| Variant::Vector3i(gdcore::math::Vector3i::new(c.x, c.y, c.z)))
                    .collect(),
            )
        }
        "get_used_cells_by_item" => {
            let item = int_arg(0, -1);
            let grid = GridMap::from_node(tree, node_id)?;
            Variant::Array(
                grid.used_cells()
                    .into_iter()
                    .filter(|c| grid.get_cell_item(*c) == item)
                    .map(|c| Variant::Vector3i(gdcore::math::Vector3i::new(c.x, c.y, c.z)))
                    .collect(),
            )
        }
        "local_to_map" => {
            let cs = node_cell_size(tree, node_id);
            let p = match args.first() {
                Some(Variant::Vector3(v)) => *v,
                Some(Variant::Vector3i(v)) => Vector3::new(v.x as f32, v.y as f32, v.z as f32),
                _ => Vector3::ZERO,
            };
            // Godot floors the scaled position and does NOT subtract the centre
            // offset, which is what makes map_to_local round-trip back to the
            // same cell.
            Variant::Vector3i(gdcore::math::Vector3i::new(
                (p.x / cs).floor() as i32,
                (p.y / cs).floor() as i32,
                (p.z / cs).floor() as i32,
            ))
        }
        "map_to_local" => {
            let cell = cell_arg(0);
            let cs = node_cell_size(tree, node_id);
            let off = node_cell_offset(tree, node_id);
            // Godot's map_to_local returns the cell CENTRE — the same origin
            // `GridMap::map_to_local` gives the render and collision paths.
            Variant::Vector3(Vector3::new(
                cell.x as f32 * cs + off.x,
                cell.y as f32 * cs + off.y,
                cell.z as f32 * cs + off.z,
            ))
        }
        "clear" => {
            write_node_cell_data(tree, node_id, &[]);
            Variant::Nil
        }
        "get_cell_size" => {
            let cs = node_cell_size(tree, node_id);
            Variant::Vector3(Vector3::new(cs, cs, cs))
        }
        "set_cell_size" => {
            if let Some(Variant::Vector3(v)) = args.first() {
                let v = *v;
                if let Some(node) = tree.get_node_mut(node_id) {
                    node.set_property("cell_size", Variant::Vector3(v));
                }
            }
            Variant::Nil
        }
        "get_mesh_library" => tree
            .get_node(node_id)
            .map(|n| match n.get_property("__extres__mesh_library") {
                Variant::String(p) => Variant::String(p),
                _ => n.get_property("mesh_library"),
            })
            .unwrap_or(Variant::Nil),
        "set_mesh_library" => {
            if let Some(Variant::String(path)) = args.first() {
                let path = path.clone();
                if let Some(node) = tree.get_node_mut(node_id) {
                    // The companion is what `from_node` prefers, so writing it
                    // is what actually re-points the library.
                    node.set_property("__extres__mesh_library", Variant::String(path));
                }
            }
            Variant::Nil
        }
        // This engine bakes no meshes, so there is nothing to discard. Returning
        // Nil is the correct OUTCOME here rather than the "registered but
        // unarmed" Nil this bead removes elsewhere.
        "clear_baked_meshes" | "make_baked_meshes" => Variant::Nil,
        _ => return None,
    })
}

#[cfg(test)]
mod tests_orientation {
    use super::*;

    /// 🔑 The orientation table is not transposed.
    ///
    /// ⚠️ Orientation 1 is Godot's `Basis(0, -1, 0, 1, 0, 0, 0, 0, 1)` — a +90°
    /// turn about Z, which sends +X to +Y. Under a transposed table it would send
    /// +X to -Y instead: the same rotation backwards, which on a tiled map looks
    /// like the art was authored wrong rather than like a matrix bug.
    ///
    /// Entry 0 is the identity and is symmetric, so it passes under BOTH
    /// conventions and can never catch this. That is exactly why the assertion
    /// below uses an asymmetric entry.
    #[test]
    fn gridmap_orientation_basis_is_not_transposed() {
        let turned = orientation_basis(1).xform(Vector3::new(1.0, 0.0, 0.0));
        assert!(
            (turned.x - 0.0).abs() < 1e-6
                && (turned.y - 1.0).abs() < 1e-6
                && (turned.z - 0.0).abs() < 1e-6,
            "orientation 1 must send +X to +Y (a +90 degree turn about Z); got \
             {turned:?}. A result of (0, -1, 0) means the table was copied in \
             Godot's ROW order into this engine's COLUMN-major Basis",
        );
    }

    /// The identity orientation leaves a cell alone, and every entry is a pure
    /// rotation (no scale, no reflection).
    #[test]
    fn gridmap_orientation_basis_covers_all_24() {
        assert_eq!(
            orientation_basis(0),
            Basis::IDENTITY,
            "orientation 0 is Godot's default and must not rotate",
        );
        for o in 0u8..24 {
            let b = orientation_basis(o);
            // An orthonormal rotation maps the unit axes to unit vectors.
            for v in [
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            ] {
                let r = b.xform(v);
                let len = (r.x * r.x + r.y * r.y + r.z * r.z).sqrt();
                assert!(
                    (len - 1.0).abs() < 1e-6,
                    "orientation {o} is not a rotation: it scales a unit axis to {len}",
                );
            }
        }
        // Out of range is the identity, not a panic: the field is 5 bits.
        assert_eq!(orientation_basis(31), Basis::IDENTITY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn set_get_cell() {
        let mut g = GridMap::new();
        g.mesh_library.set_item(1, "cube", "res://meshes/cube.tres");
        let c = GridCell { x: 1, y: 0, z: -2 };
        g.set_cell_item(c, 1);
        assert_eq!(g.get_cell_item(c), 1);
        assert_eq!(g.used_cells().len(), 1);
        let inst = g.mesh_instances();
        assert_eq!(inst.len(), 1);
        // Godot's map_to_local: x = 1 * 2 + 1 (cell_center_x), the cell's
        // CENTRE (pat-2ifdx).
        assert!((inst[0].0.x - 3.0).abs() < 1e-5);
    }

    #[test]
    fn decode_cells_positive_and_negative_coords() {
        // (0,0,0) item 1 ; (1,0,0) item 0 ; (0,0,1) item 2 ; (-1,-1,0) item 3
        let data = [
            0,
            0,
            1, // key=0                 -> (0,0,0), item 1
            1,
            0,
            0, // lo=1 (x=1)            -> (1,0,0), item 0
            0,
            1,
            2, // hi=1 (z=1)            -> (0,0,1), item 2
            0xFFFF_FFFF_u32 as i32,
            0,
            3, // lo=-1 (x=-1,y=-1), hi=0 (z=0) -> item 3
        ];
        let decoded = GridMap::decode_cells(&data);
        assert_eq!(decoded.len(), 4);
        let map: HashMap<GridCell, i32> = decoded.into_iter().collect();
        assert_eq!(map[&GridCell { x: 0, y: 0, z: 0 }], 1);
        assert_eq!(map[&GridCell { x: 1, y: 0, z: 0 }], 0);
        assert_eq!(map[&GridCell { x: 0, y: 0, z: 1 }], 2);
        assert_eq!(map[&GridCell { x: -1, y: -1, z: 0 }], 3);
    }

    #[test]
    fn decode_cells_skips_empty_and_partial() {
        // One valid cell, one empty (item 0xFFFF), plus a trailing partial triple.
        let data = [0, 0, 5, 2, 0, INVALID_CELL_ITEM, 7, 7];
        let decoded = GridMap::decode_cells(&data);
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].0, GridCell { x: 0, y: 0, z: 0 });
        assert_eq!(decoded[0].1, 5);
    }

    #[test]
    fn from_tscn_loads_gridmap_node() {
        // A GridMap child of a Node3D root, in Godot's on-disk format with a
        // multi-line `data` dictionary. Cells: (0,0,0)->0, (1,0,0)->1, (0,0,1)->0.
        let src = r#"
[gd_scene load_steps=2 format=3 uid="uid://abc123"]

[ext_resource type="MeshLibrary" path="res://tiles.meshlib.tres" id="1_mesh"]

[node name="Root" type="Node3D"]

[node name="GridMap" type="GridMap" parent="."]
mesh_library = ExtResource("1_mesh")
cell_size = Vector3(2, 2, 2)
data = {
"cells": PackedInt32Array(0, 0, 0, 1, 0, 1, 0, 1, 0)
}
"#;
        let grid = GridMap::from_tscn(src)
            .expect("parse ok")
            .expect("scene has a GridMap node");

        assert!((grid.cell_size - 2.0).abs() < 1e-5);
        assert_eq!(
            grid.mesh_library_resource.as_deref(),
            Some("res://tiles.meshlib.tres")
        );
        assert_eq!(grid.cell_count(), 3);
        assert_eq!(grid.get_cell_item(GridCell { x: 0, y: 0, z: 0 }), 0);
        assert_eq!(grid.get_cell_item(GridCell { x: 1, y: 0, z: 0 }), 1);
        assert_eq!(grid.get_cell_item(GridCell { x: 0, y: 0, z: 1 }), 0);
        // An unset cell reports -1.
        assert_eq!(grid.get_cell_item(GridCell { x: 9, y: 9, z: 9 }), -1);
    }

    #[test]
    fn from_tscn_without_gridmap_returns_none() {
        let src = r#"
[gd_scene format=3]

[node name="Root" type="Node3D"]
"#;
        assert!(GridMap::from_tscn(src).expect("parse ok").is_none());
    }
}

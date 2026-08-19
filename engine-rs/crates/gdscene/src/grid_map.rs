//! GridMap runtime (pat-p0s82.16).

use std::collections::HashMap;

use gdcore::error::EngineResult;
use gdcore::math::Vector3;
use gdcore::math3d::Basis;
use gdvariant::Variant;

use crate::packed_scene::PackedScene;

/// Sentinel item id used by Godot for an empty cell (`GridMap::INVALID_CELL_ITEM`).
const INVALID_CELL_ITEM: i32 = 0xFFFF;

/// Mesh library entry.
#[derive(Debug, Clone)]
pub struct MeshLibraryItem {
    pub name: String,
    pub mesh_path: String,
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
    /// `preview`. Only the name and the mesh path are read here; the rest is
    /// data the GridMap render path does not consume yet.
    ///
    /// An item with a name but **no** mesh is skipped: it contributes no
    /// geometry, and admitting it would put an entry with an empty path into the
    /// library for the render path to try to load.
    ///
    /// Returns `Ok(None)` when the resource is not a `MeshLibrary`.
    pub fn from_tres(source: &str) -> EngineResult<Option<MeshLibrary>> {
        let resource = gdresource::TresLoader::new().parse_str(source, "res://meshlibrary.tres")?;
        if resource.class_name != "MeshLibrary" {
            return Ok(None);
        }

        // Collect both fields before building items: `properties()` iterates a
        // HashMap, so `name` and `mesh` arrive in either order.
        let mut names: HashMap<i32, String> = HashMap::new();
        let mut meshes: HashMap<i32, String> = HashMap::new();
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
                    // The loader stores `ExtResource("id")` as a string with an
                    // `ExtResource:` prefix; the path lives on the header entry.
                    if let Some(ext_id) = reference.strip_prefix("ExtResource:") {
                        if let Some(ext) = resource.ext_resources.get(ext_id) {
                            meshes.insert(id, ext.path.clone());
                        }
                    }
                }
                _ => {}
            }
        }

        let mut library = MeshLibrary::new();
        for (id, mesh_path) in meshes {
            let name = names.get(&id).cloned().unwrap_or_default();
            library.set_item(id, name, mesh_path);
        }
        Ok(Some(library))
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
            cells: HashMap::new(),
            orientations: HashMap::new(),
        }
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
                let pos = gdcore::math::Vector3::new(
                    cell.x as f32 * self.cell_size,
                    cell.y as f32 * self.cell_size,
                    cell.z as f32 * self.cell_size,
                );
                out.push((pos, item.mesh_path.clone()));
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
                let pos = Vector3::new(
                    cell.x as f32 * self.cell_size,
                    cell.y as f32 * self.cell_size,
                    cell.z as f32 * self.cell_size,
                );
                out.push((
                    pos,
                    orientation_basis(self.cell_orientation(*cell)),
                    item.mesh_path.clone(),
                ));
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

        // mesh_library: `ExtResource("id")` -> resolved `res://` path.
        if let Variant::String(ref ext_ref) = node.get_property("mesh_library") {
            if let Some(path) = scene.resolve_ext_resource_path(ext_ref) {
                grid.mesh_library_resource = Some(path.to_string());
            }
        }

        // data = { "cells": PackedInt32Array(...) }
        if let Variant::Dictionary(dict) = node.get_property("data") {
            if let Some(Variant::PackedInt32Array(cells)) = dict.get("cells") {
                grid.load_cells(cells);
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
                grid.load_cells(cells);
            }
        }

        Some(grid)
    }
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
        assert!((inst[0].0.x - 2.0).abs() < 1e-5);
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

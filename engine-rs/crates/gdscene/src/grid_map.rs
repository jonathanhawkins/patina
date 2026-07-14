//! GridMap runtime (pat-p0s82.16).

use std::collections::HashMap;

use gdcore::error::EngineResult;
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
}

impl GridMap {
    pub fn new() -> Self {
        Self {
            cell_size: 2.0,
            mesh_library: MeshLibrary::new(),
            mesh_library_resource: None,
            cells: HashMap::new(),
        }
    }

    pub fn set_cell_item(&mut self, cell: GridCell, item: i32) {
        if item < 0 {
            self.cells.remove(&cell);
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

    /// Decodes Godot's `data/cells` `PackedInt32Array` encoding into
    /// `(cell, item)` pairs. Godot stores each cell as three consecutive
    /// `int32`s: the low and high halves of a 64-bit key packing three signed
    /// `int16` coordinates (`x | y<<16 | z<<32`), followed by a packed cell
    /// value whose low 16 bits are the mesh-library item id (upper bits hold
    /// orientation/layer, which this subset does not retain). Empty cells
    /// (`item == INVALID_CELL_ITEM`) and any trailing partial triple are
    /// skipped.
    pub fn decode_cells(data: &[i32]) -> Vec<(GridCell, i32)> {
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
            out.push((GridCell { x, y, z }, item));
        }
        out
    }

    /// Applies a decoded `data/cells` array to this GridMap.
    pub fn load_cells(&mut self, data: &[i32]) {
        for (cell, item) in Self::decode_cells(data) {
            self.set_cell_item(cell, item);
        }
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

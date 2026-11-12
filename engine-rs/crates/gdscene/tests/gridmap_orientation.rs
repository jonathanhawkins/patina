//! pat-0q2kx.196 — GridMap cells must keep their orientation.
//!
//! Godot packs each cell value as a bitfield (`modules/gridmap/grid_map.h`):
//! `item : 16`, `rot : 5`, `layer : 8`. `decode_cells` masked `value & 0xFFFF`
//! for the item and threw the rest away, and its own doc comment admitted the
//! orientation "is not retained".
//!
//! That matters for this bead specifically: it asks for the render path to
//! expand cells into instances. Had the render work landed on the old decode,
//! **every cell would have drawn unrotated** — geometry in the right places with
//! the wrong facing, and nothing anywhere to signal the loss.
//!
//! These are an **integration** test rather than in-module because
//! `gdscene`'s `#[cfg(test)]` build (`-p gdscene --lib`) does not compile: a
//! pre-existing duplicate `#[test]` in `control.rs` breaks it, reported under
//! pat-0q2kx.175. Every API used here is public, so the coverage is unaffected.

use gdscene::grid_map::{GridCell, GridMap};

/// Builds a packed cell value the way Godot does.
fn packed(item: u32, rot: u32, layer: u32) -> i32 {
    (item | (rot << 16) | (layer << 21)) as i32
}

const ORIGIN: GridCell = GridCell { x: 0, y: 0, z: 0 };

#[test]
fn decode_full_keeps_orientation_and_layer() {
    let data = [0, 0, packed(7, 22, 3)];
    let decoded = GridMap::decode_cells_full(&data);

    assert_eq!(decoded.len(), 1);
    let d = decoded[0];
    assert_eq!(d.cell, ORIGIN);
    assert_eq!(d.item, 7, "the item must survive alongside the new fields");
    assert_eq!(d.orientation, 22, "rot occupies bits 16..21");
    assert_eq!(d.layer, 3, "layer occupies bits 21..29");
}

/// The orientation must not bleed into the item. The bug this replaces was a
/// masking error, so the boundary is pinned at its extreme.
#[test]
fn orientation_bits_do_not_corrupt_the_item() {
    // Largest item that is still a real cell (0xFFFF is the empty sentinel),
    // with the largest values the rot and layer fields can hold.
    let data = [0, 0, packed(0xFFFE, 31, 0xFF)];
    let decoded = GridMap::decode_cells_full(&data);

    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].item, 0xFFFE, "a large item id must stay intact");
    assert_eq!(decoded[0].orientation, 31);
    assert_eq!(decoded[0].layer, 0xFF);

    // An oriented *empty* cell is still empty: the sentinel check has to look at
    // the item bits, not the whole packed value.
    let empty = [0, 0, packed(0xFFFF, 12, 1)];
    assert!(
        GridMap::decode_cells_full(&empty).is_empty(),
        "an empty cell with a non-zero rotation is still empty"
    );
}

/// The short form still returns `(cell, item)` unchanged, so its existing
/// callers keep working — the new decode is additive.
#[test]
fn short_decode_is_unchanged_by_the_full_decode() {
    let data = [0, 0, packed(7, 22, 3)];
    assert_eq!(GridMap::decode_cells(&data), vec![(ORIGIN, 7)]);
}

/// Loading a scene's cells carries the rotation into the map, not just into the
/// decode — the step where it was previously lost for good.
#[test]
fn load_cells_stores_the_orientation() {
    let mut gm = GridMap::new();
    gm.load_cells(&[0, 0, packed(7, 22, 0)]);

    assert_eq!(gm.get_cell_item(ORIGIN), 7);
    assert_eq!(gm.cell_orientation(ORIGIN), 22);

    // An unrotated cell reports 0 without needing a stored entry.
    let mut plain = GridMap::new();
    plain.load_cells(&[0, 0, packed(3, 0, 0)]);
    assert_eq!(plain.cell_orientation(ORIGIN), 0);
}

/// Clearing a cell drops its orientation, so a cleared-then-reset cell cannot
/// inherit a stale rotation.
#[test]
fn clearing_a_cell_clears_its_orientation() {
    let mut gm = GridMap::new();
    let cell = GridCell { x: 2, y: 0, z: 1 };

    gm.set_cell_item_oriented(cell, 4, 17);
    assert_eq!(gm.cell_orientation(cell), 17);

    gm.set_cell_item(cell, -1);
    assert_eq!(gm.get_cell_item(cell), -1, "the cell is gone");
    assert_eq!(
        gm.cell_orientation(cell),
        0,
        "and its rotation went with it"
    );

    gm.set_cell_item(cell, 9);
    assert_eq!(
        gm.cell_orientation(cell),
        0,
        "a re-set cell must not resurrect the old rotation"
    );
}

//! pat-s3rrv.65 item (1) — the atlas is a clickable tile GRID, not two number
//! inputs.
//!
//! # The gap
//!
//! Every per-tile action in the TileSet panel (collision, one-way, navigation,
//! occlusion, custom data) reads the tile to edit from `tileset-tile-x` /
//! `tileset-tile-y`, which the user had to TYPE. Godot's
//! `tile_set_atlas_source_editor.cpp` shows the source texture sliced into its
//! tiles and you click the one you mean; Patina showed a source list and two
//! spinners, so choosing a tile meant reading coordinates off the atlas by eye.
//!
//! # What this pins
//!
//! ⚠️ Assertions on served markup are weak by nature — a string can be present
//! and unreachable. So this checks the three things that make the grid *work*
//! rather than merely exist: the host element, that the builder is CALLED (not
//! just defined), and that a cell click writes the same inputs the manual path
//! uses. The last one is the whole integration: it is why no existing handler
//! had to change.

use gdeditor::editor_ui::EDITOR_HTML;

#[test]
fn the_tileset_panel_serves_an_atlas_grid_host() {
    assert!(
        EDITOR_HTML.contains(r#"id="tileset-atlas-grid""#),
        "the TileSet panel must serve a host element for the atlas grid"
    );
}

/// 🔑 Defined is not wired. `buildTileSetAtlasGrid` must be invoked from the
/// panel's rebuild path, or the grid is a function nobody calls and the panel
/// looks exactly as it did before.
#[test]
fn the_atlas_grid_builder_is_actually_called() {
    let defined = EDITOR_HTML
        .matches("function buildTileSetAtlasGrid(")
        .count();
    let calls = EDITOR_HTML.matches("buildTileSetAtlasGrid(").count() - defined;
    assert_eq!(
        defined, 1,
        "expected exactly one definition of buildTileSetAtlasGrid, found {defined}"
    );
    assert!(
        calls >= 1,
        "buildTileSetAtlasGrid is defined but never called — the grid would \
         never render, which is indistinguishable from not having built it"
    );
}

/// 🔑 A clicked cell writes the SAME inputs a user could type into.
///
/// ⚠️ This is what keeps every existing action working untouched: collision,
/// nav, occlusion and custom-data all post `tileSetAtlasCoords()`, so as long as
/// the grid drives those two fields it needs no cooperation from any of them. A
/// grid that tracked its own selection variable instead would render correctly
/// and edit the wrong tile.
#[test]
fn clicking_a_cell_drives_the_existing_coordinate_inputs() {
    assert!(
        EDITOR_HTML.contains("getElementById('tileset-tile-x')")
            && EDITOR_HTML.contains("getElementById('tileset-tile-y')"),
        "the coordinate inputs must still be the single source of truth"
    );
    // The cell builder is where the click handler lives; it must reach for
    // those inputs rather than a private selection variable.
    let cell_fn = EDITOR_HTML
        .split("function tileSetAtlasCell(")
        .nth(1)
        .expect("tileSetAtlasCell must exist");
    let body = &cell_fn[..cell_fn.len().min(2000)];
    assert!(
        body.contains("tileset-tile-x") && body.contains("tileset-tile-y"),
        "the cell click handler must write the atlas coords into the existing \
         inputs; without that the grid selects one tile and the buttons edit \
         another"
    );
}

/// The grid must degrade rather than render an empty box when the atlas texture
/// cannot be fetched — a missing `res://` path is ordinary in a broken project.
#[test]
fn a_missing_atlas_texture_degrades_visibly() {
    assert!(
        EDITOR_HTML.contains("atlas preview unavailable"),
        "an image that fails to load must say so; an empty grid host reads as \
         'this atlas has no tiles'"
    );
}

/// ⚠️ A large atlas must not build unbounded DOM. 1024px of 16px tiles is
/// 64x64 = 4096 cells, which locks the panel; the builder caps the rendered
/// window and must say that it did rather than silently show part of the atlas
/// as if it were all of it.
#[test]
fn a_large_atlas_is_capped_and_says_so() {
    assert!(
        EDITOR_HTML.contains("showing first "),
        "a truncated atlas grid must report the truncation"
    );
}

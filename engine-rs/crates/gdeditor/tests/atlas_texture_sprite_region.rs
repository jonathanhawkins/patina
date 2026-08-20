//! pat-s3rrv.117 — an `AtlasTexture` assigned to a Sprite2D draws its sub-rect.
//!
//! Godot's AtlasTexture presents a sub-rect of a sheet as a standalone texture,
//! and it is the backbone of sprite-sheet workflows. Patina had no path for it
//! at all: `node2d::get_texture_path` found a `SubResource(...)` where it wanted
//! a file path, gave up, and the sprite drew nothing.
//!
//! 🔑 **An AtlasTexture is a redirect PLUS a rect**, so both halves have to
//! land. The drawn image is the `atlas` — an ExtResource naming the sheet file —
//! and the sub-rect is the AtlasTexture's own `region`. Resolving only the
//! region leaves the texture unresolved; resolving only the path draws the whole
//! sheet at full size. Each test below fails if either half is missing.

use gdeditor::viewport_2d_render::{build_runtime_render_list, Render2DKind};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// The shape Godot writes: the sprite points at an AtlasTexture, which points at
/// the sheet. The region is the sheet's bottom-right 16x16 cell.
const ATLAS: &str = r#"[gd_scene load_steps=3 format=3]

[ext_resource type="Texture2D" path="res://sheet.png" id="1_sheet"]

[sub_resource type="AtlasTexture" id="AtlasTexture_1"]
atlas = ExtResource("1_sheet")
region = Rect2(16, 16, 16, 16)

[node name="World" type="Node2D"]

[node name="Sprite" type="Sprite2D" parent="."]
texture = SubResource("AtlasTexture_1")
"#;

/// The control: the same sheet assigned directly, with no atlas in between.
const PLAIN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Texture2D" path="res://sheet.png" id="1_sheet"]

[node name="World" type="Node2D"]

[node name="Sprite" type="Sprite2D" parent="."]
texture = ExtResource("1_sheet")
"#;

/// A sprite that regions an atlas-backed texture itself. Godot keeps the two
/// mechanisms independent, and the node's own rect is the one that applies.
const NODE_REGION_WINS: &str = r#"[gd_scene load_steps=3 format=3]

[ext_resource type="Texture2D" path="res://sheet.png" id="1_sheet"]

[sub_resource type="AtlasTexture" id="AtlasTexture_1"]
atlas = ExtResource("1_sheet")
region = Rect2(16, 16, 16, 16)

[node name="World" type="Node2D"]

[node name="Sprite" type="Sprite2D" parent="."]
texture = SubResource("AtlasTexture_1")
region_enabled = true
region_rect = Rect2(0, 0, 8, 8)
"#;

fn load(tscn: &str) -> SceneTree {
    // ClassDB starts empty in every nextest process, and the canvas walk admits
    // a node only if `is_class("CanvasItem")` resolves — without this the scene
    // collects to `[]`, which reads like the mapping dropped the node.
    gdobject::class_db::register_2d_classes();
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    tree
}

/// The sprite's resolved texture path and sheet settings, as the rasterizer
/// receives them.
fn sprite(tscn: &str) -> (String, gdeditor::viewport_2d_render::SpriteSheet) {
    let tree = load(tscn);
    build_runtime_render_list(&tree)
        .into_iter()
        .find_map(|item| match item.kind {
            Render2DKind::Sprite { texture, sheet, .. } => Some((texture, sheet)),
            _ => None,
        })
        .expect("the Sprite2D must produce a Sprite render item")
}

/// **The core of the bead.** Both halves land: the texture resolves to the
/// SHEET, and the sprite samples only the atlas's region.
#[test]
fn an_atlas_texture_resolves_to_the_sheet_and_its_region() {
    let (texture, sheet) = sprite(ATLAS);

    // ⚠️ The path is the sheet's, not the AtlasTexture's. Before this the
    // property held `SubResource("AtlasTexture_1")`, which is not a path, so the
    // sprite produced no render item at all.
    assert_eq!(
        texture, "res://sheet.png",
        "the drawn image is the atlas's sheet"
    );

    assert!(
        sheet.region_enabled,
        "an atlas-backed sprite samples a sub-rect"
    );
    assert_eq!(sheet.region_rect.position.x, 16.0);
    assert_eq!(sheet.region_rect.position.y, 16.0);
    assert_eq!(sheet.region_rect.size.x, 16.0);
    assert_eq!(sheet.region_rect.size.y, 16.0);
}

/// The negative control. Without it, a change that switched `region_enabled` on
/// unconditionally would pass the test above while cropping every ordinary
/// sprite in the project to a zero-sized rect.
#[test]
fn a_plain_texture_is_unaffected() {
    let (texture, sheet) = sprite(PLAIN);
    assert_eq!(texture, "res://sheet.png");
    assert!(
        !sheet.region_enabled,
        "a sprite with no atlas and no region draws its whole texture"
    );
}

/// ⚠️ The node's own `region_enabled`/`region_rect` win. Godot keeps the two
/// independent — a Sprite2D can region a texture that is itself an
/// AtlasTexture — and letting the atlas overwrite the authored rect would
/// silently change what an existing scene draws.
#[test]
fn an_authored_node_region_beats_the_atlas_region() {
    let (_, sheet) = sprite(NODE_REGION_WINS);
    assert!(sheet.region_enabled);
    assert_eq!(
        (sheet.region_rect.size.x, sheet.region_rect.size.y),
        (8.0, 8.0),
        "the node's authored 8x8 rect wins over the atlas's 16x16"
    );
}

/// The companions the two halves travel on. Asserted directly so a failure in
/// the tests above can be localised to the loader or the reader rather than
/// being ambiguous between them.
#[test]
fn the_loader_parks_the_region_and_redirects_the_path() {
    let tree = load(ATLAS);
    let node_id = tree
        .all_nodes_in_tree_order()
        .into_iter()
        .find(|id| {
            tree.get_node(*id)
                .map(|n| n.name() == "Sprite")
                .unwrap_or(false)
        })
        .expect("no node named Sprite");
    let node = tree.get_node(node_id).unwrap();

    assert!(
        matches!(node.get_property("__atlas_region"), Variant::Rect2(r) if r.size.x == 16.0),
        "the atlas region must be parked on the node"
    );
    assert!(
        matches!(node.get_property("__extres__texture"), Variant::String(ref s) if s == "res://sheet.png"),
        "the texture must be redirected to the sheet"
    );
    // The property itself is untouched, so the scene still round-trips and the
    // sub-resource body is still emitted.
    assert!(
        matches!(node.get_property("texture"), Variant::String(ref s) if s.contains("AtlasTexture_1")),
        "the authored `texture` value must survive for round-tripping"
    );
}

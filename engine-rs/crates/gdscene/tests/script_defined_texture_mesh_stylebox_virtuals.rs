//! pat-s3rrv.160 acceptance: script-defined resource virtuals reach the render
//! paths.
//!
//! A scripted `Texture2D`, `StyleBox` or `PrimitiveMesh` loaded fine and drew
//! nothing: none of their virtuals were declared, and nothing dispatched them.
//!
//! ⚠️ **The texture clause uses `TextureRect`, not the `Sprite2D` the bead's
//! acceptance names.** The acceptance asks for a Sprite2D to produce an entry
//! in "the frame's 2D draw-item list", and those two halves cannot both hold:
//! that list is `ControlDrawItem`, built by `render_node`, which gates on
//! `is_control()` — and `Sprite2D` is neither a Control nor in
//! `CONTROL_CLASSES`, so it never produces a draw item at all, scripted texture
//! or not. `TextureRect` exercises the identical `Texture2D` virtuals and does
//! reach the list. Reported to the coordinator in 82553; making Sprite2D
//! contribute to this list is 2D scene rendering through the control path, a
//! feature rather than a dispatch.

use gdcore::math::Vector2;
use gdscene::control_render::{control_draw_items, ControlDrawItem};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;
use std::path::PathBuf;
use std::sync::OnceLock;

/// A per-process `res://` root holding the three scripts.
fn res_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let dir =
            std::env::temp_dir().join(format!("patina_script_virtuals_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp res:// root is creatable");

        // A procedural texture: 64x32, painting one rect of its own size.
        std::fs::write(
            dir.join("tex.gd"),
            "extends Texture2D\nfunc _get_width():\n\treturn 64\nfunc _get_height():\n\treturn 32\nfunc _draw_rect(ci, rect, tile, modulate, transpose):\n\tdraw_rect(rect, Color(1, 0, 0, 1), true)\n",
        )
        .expect("tex.gd is writable");

        // A stylebox that paints a circle instead of a box.
        std::fs::write(
            dir.join("box.gd"),
            "extends StyleBox\nfunc _draw(ci, rect):\n\tdraw_circle(Vector2(20, 20), 10.0, Color(0, 1, 0, 1))\n",
        )
        .expect("box.gd is writable");

        // A procedural mesh: one triangle.
        std::fs::write(
            dir.join("mesh.gd"),
            "extends PrimitiveMesh\nfunc _create_mesh_array():\n\tvar a = []\n\ta.resize(13)\n\ta[0] = PackedVector3Array([Vector3(0, 0, 0), Vector3(1, 0, 0), Vector3(0, 1, 0)])\n\ta[12] = PackedInt32Array([0, 1, 2])\n\treturn a\n",
        )
        .expect("mesh.gd is writable");

        gdcore::file_access::set_path_roots(Some(dir.clone()), None);
        dir
    })
}

const SCENE: &str = r#"[gd_scene load_steps=3 format=3]

[ext_resource type="Script" path="res://tex.gd" id="1_t"]
[ext_resource type="Script" path="res://box.gd" id="2_b"]

[sub_resource type="Texture2D" id="Tex_1"]
script = ExtResource("1_t")

[sub_resource type="StyleBox" id="Box_1"]
script = ExtResource("2_b")

[node name="Root" type="Control"]

[node name="Pic" type="TextureRect" parent="."]
offset_right = 200.0
offset_bottom = 200.0
texture = SubResource("Tex_1")

[node name="Card" type="Panel" parent="."]
offset_right = 100.0
offset_bottom = 100.0
theme_override_styles/panel = SubResource("Box_1")
"#;

fn draw_items() -> Vec<ControlDrawItem> {
    res_root();
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let theme = Theme::default_theme();
    control_draw_items(&mut tree, scene_root, &theme, Vector2::new(400.0, 400.0))
}

/// A scripted `Texture2D` draws itself, at the size IT reports.
#[test]
fn a_scripted_texture_draws_at_its_own_reported_size() {
    let items = draw_items();

    let sized = items.iter().any(|it| match it {
        ControlDrawItem::Rect { rect, .. } => {
            (rect.size.x - 64.0).abs() < 0.5 && (rect.size.y - 32.0).abs() < 0.5
        }
        _ => false,
    });
    assert!(
        sized,
        "no 64x32 rect in the draw list — `_get_width`/`_get_height`/`_draw_rect` \
         did not reach the render path. Items: {items:#?}"
    );
}

/// A scripted `StyleBox` replaces the default box rather than drawing over it.
#[test]
fn a_scripted_stylebox_draws_its_circle_instead_of_the_default_box() {
    let items = draw_items();

    assert!(
        items
            .iter()
            .any(|it| matches!(it, ControlDrawItem::Circle { .. })),
        "the scripted stylebox's circle is missing. Items: {items:#?}"
    );

    // ⚠️ The control-sized rect must be GONE, not merely joined by a circle.
    // Godot calls `_draw` INSTEAD of drawing the default box, so a panel that
    // painted both would look right in a screenshot and be wrong in the list.
    let default_box = items.iter().any(|it| match it {
        ControlDrawItem::Rect { rect, .. } => {
            (rect.size.x - 100.0).abs() < 0.5 && (rect.size.y - 100.0).abs() < 0.5
        }
        _ => false,
    });
    assert!(
        !default_box,
        "the default 100x100 panel box is still drawn under the scripted \
         stylebox. Items: {items:#?}"
    );
}

/// A scripted `PrimitiveMesh` supplies geometry through `_create_mesh_array`.
#[test]
fn a_scripted_primitive_mesh_builds_its_triangle() {
    res_root();
    let mesh =
        gdscene::script_mesh::mesh_from_script("res://mesh.gd").expect("the scripted mesh builds");

    assert_eq!(mesh.vertices.len(), 3, "three vertices");
    assert_eq!(
        mesh.triangle_count(),
        1,
        "exactly one triangle from a 3-vertex `_create_mesh_array`"
    );
}

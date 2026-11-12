//! pat-s3rrv.183 — `CanvasGroup`, `BackBufferCopy` and `MultiMeshInstance2D`
//! draw instead of falling through to the editor's `Marker` placeholder.
//!
//! Godot ground truth: `scene/2d/canvas_group.cpp` (the subtree is rendered to
//! an offscreen buffer and composited ONCE, so `modulate` applies to the group
//! rather than to each child), `back_buffer_copy.cpp` (the framebuffer is
//! copied into the back buffer a canvas_item shader samples as
//! `SCREEN_TEXTURE`), and `multimesh_instance_2d.cpp`.
//!
//! All three were registered in ClassDB with no behaviour anywhere, so each hit
//! the `Render2DKind::Marker` catch-all and showed a placeholder glyph in the
//! running game.
//!
//! The file name is short on purpose: the verifier truncates a test command at
//! 58 characters.

use gdcore::math::{Color, Vector2};
use gdeditor::texture_cache::TextureCache;
use gdeditor::viewport_2d_render::{
    build_runtime_render_list, collect_canvas_groups, collect_canvas_items,
    rasterize_render_list_with_groups, Render2DKind,
};
use gdrender2d::texture::Texture2D;
use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const W: u32 = 40;
const H: u32 = 40;

/// An axis-aligned square as a `Polygon2D` point list.
///
/// Polygon2D is used rather than ColorRect because it is `Node2D`-based: a
/// Control would need its layout resolved before it has any size at all, and a
/// zero-sized fixture would pass a "no darker seam" assertion for entirely the
/// wrong reason.
fn square(x: f32, y: f32, size: f32) -> Variant {
    Variant::PackedVector2Array(vec![
        Vector2::new(x, y),
        Vector2::new(x + size, y),
        Vector2::new(x + size, y + size),
        Vector2::new(x, y + size),
    ])
}

fn pixel(fb: &gdrender2d::renderer::FrameBuffer, x: u32, y: u32) -> Color {
    fb.pixels[(y * fb.width + x) as usize]
}

/// Two overlapping opaque white squares, optionally under a CanvasGroup whose
/// `modulate` is half-transparent.
fn overlapping_squares(group_alpha: Option<f32>, child_alpha: f32) -> SceneTree {
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = match group_alpha {
        Some(a) => {
            let g = tree
                .add_child(root, Node::new("Group", "CanvasGroup"))
                .unwrap();
            tree.get_node_mut(g)
                .unwrap()
                .set_property("modulate", Variant::Color(Color::new(1.0, 1.0, 1.0, a)));
            g
        }
        None => root,
    };

    for (name, x) in [("A", 0.0f32), ("B", 10.0f32)] {
        let p = tree
            .add_child(parent, Node::new(name, "Polygon2D"))
            .unwrap();
        let node = tree.get_node_mut(p).unwrap();
        node.set_property("polygon", square(x, x, 20.0));
        node.set_property("color", Variant::Color(Color::new(1.0, 1.0, 1.0, 1.0)));
        node.set_property(
            "modulate",
            Variant::Color(Color::new(1.0, 1.0, 1.0, child_alpha)),
        );
    }
    tree
}

fn render(tree: &SceneTree) -> gdrender2d::renderer::FrameBuffer {
    let items = build_runtime_render_list(tree);
    let groups = collect_canvas_groups(tree);
    rasterize_render_list_with_groups(&items, &groups, W, H, Color::new(0.0, 0.0, 0.0, 1.0), None)
}

/// **The acceptance's CanvasGroup clause.** One composite, not two blends.
///
/// Two opaque squares flatten against each other at full opacity inside the
/// group's buffer, so the crossing is no more opaque than either square alone.
/// The single composite at alpha 0.5 then tints the whole flattened result
/// uniformly.
#[test]
fn canvas_group_composites_the_subtree_once() {
    let fb = render(&overlapping_squares(Some(0.5), 1.0));

    let only_a = pixel(&fb, 5, 5);
    let overlap = pixel(&fb, 15, 15);
    let only_b = pixel(&fb, 25, 25);

    assert!(
        (only_a.r - 0.5).abs() < 0.02,
        "a single square under a half-alpha group should be half grey, got {only_a:?}"
    );
    assert!(
        (overlap.r - only_a.r).abs() < 0.02,
        "the overlap must match the non-overlap — one composite, not two blends. \
         overlap {overlap:?} vs single {only_a:?}"
    );
    assert!(
        (only_b.r - only_a.r).abs() < 0.02,
        "both squares should tint identically, got {only_b:?} vs {only_a:?}"
    );
}

/// Positive control: WITHOUT the group, the overlap really is brighter.
///
/// This is what makes the test above meaningful. If overlapping squares
/// happened to blend to the same value either way, the group assertion would
/// pass on a completely unimplemented CanvasGroup.
#[test]
fn without_a_group_the_overlap_is_visibly_brighter() {
    // Same half-alpha, but applied per child with no group to flatten them.
    let fb = render(&overlapping_squares(None, 0.5));

    let single = pixel(&fb, 5, 5);
    let overlap = pixel(&fb, 15, 15);
    assert!(
        overlap.r > single.r + 0.1,
        "two independent half-alpha blends must stack brighter than one \
         ({overlap:?} vs {single:?}); if they do not, the group test proves nothing"
    );
}

/// **The acceptance's MultiMeshInstance2D clause.** Three instances draw.
#[test]
fn multimesh_instance_2d_emits_one_quad_per_instance() {
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mm = tree
        .add_child(root, Node::new("MM", "MultiMeshInstance2D"))
        .unwrap();

    // Three translations at x = 0, 10, 20. Row-major 2x4 per instance:
    // [xx, yx, 0, ox,  xy, yy, 0, oy].
    let mut buffer = Vec::new();
    for i in 0..3 {
        buffer.extend_from_slice(&[1.0, 0.0, 0.0, i as f32 * 10.0, 0.0, 1.0, 0.0, 0.0]);
    }
    let node = tree.get_node_mut(mm).unwrap();
    node.set_property("__multimesh__buffer", Variant::PackedFloat32Array(buffer));
    node.set_property("__multimesh__instance_count", Variant::Int(3));

    let kind = collect_canvas_items(&tree)
        .into_iter()
        .map(|i| i.kind)
        .find(|k| matches!(k, Render2DKind::MultiMesh { .. }))
        .expect("MultiMeshInstance2D must emit MultiMesh, not fall through to Marker");

    let Render2DKind::MultiMesh { instances, .. } = kind else {
        unreachable!()
    };
    assert_eq!(instances.len(), 3, "one entry per authored instance");
    let xs: Vec<f32> = instances.iter().map(|(t, _)| t.origin.x).collect();
    assert_eq!(
        xs,
        vec![0.0, 10.0, 20.0],
        "instance origins come from the buffer, not stacked at the node origin"
    );
    // The buffer is row-major and Transform2D is column-major, so a transposed
    // read would put the translation in the basis and leave every origin at 0.
    assert!(
        instances
            .iter()
            .all(|(t, _)| t.origin.y == 0.0 && t.x.x == 1.0),
        "identity basis with an x-only translation, not a transposed read"
    );
}

/// **The acceptance's BackBufferCopy clause, first half.** The node is a real
/// draw-list entry rather than a Marker, and carries Godot's defaults.
#[test]
fn back_buffer_copy_is_a_real_item_with_godot_defaults() {
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("BBC", "BackBufferCopy"))
        .unwrap();

    let kind = collect_canvas_items(&tree)
        .into_iter()
        .map(|i| i.kind)
        .find(|k| matches!(k, Render2DKind::BackBufferCopy { .. }))
        .expect("BackBufferCopy must not fall through to Marker");

    let Render2DKind::BackBufferCopy { copy_mode, rect } = kind else {
        unreachable!()
    };
    // ⚠️ doc/classes/BackBufferCopy.xml gives copy_mode default="1"
    // (COPY_MODE_RECT), NOT 0 — an unconfigured node copies.
    assert_eq!(copy_mode, 1, "copy_mode defaults to COPY_MODE_RECT");
    assert_eq!(rect.position, Vector2::new(-100.0, -100.0));
    assert_eq!(rect.size, Vector2::new(200.0, 200.0));
}

// ---------------------------------------------------------------------------
// BackBufferCopy -> SCREEN_TEXTURE
// ---------------------------------------------------------------------------

/// Canvas size for the screen-texture scenes, and the sprite's texture size.
const S: u32 = 16;
const TEX: u32 = 8;

/// A red full-canvas background, optionally a `BackBufferCopy`, then a sprite
/// whose shader reads `SCREEN_TEXTURE`.
///
/// The shader form is taken from this crate's own passing tests
/// (`texture(...)` with an identifier first argument, `COLOR = ...`), not from
/// the bead's prose: a body the front end cannot parse produces no
/// instructions at all and the sprite would render its plain texture — which
/// looks exactly like a shader that ran and sampled the wrong thing.
fn screen_texture_scene(with_copy: bool) -> String {
    let copy = if with_copy {
        "\n[node name=\"BBC\" type=\"BackBufferCopy\" parent=\".\"]\ncopy_mode = 2\n"
    } else {
        ""
    };
    format!(
        r#"[gd_scene load_steps=3 format=3]

[sub_resource type="Shader" id="Shader_1"]
code = "shader_type canvas_item; void fragment() {{ COLOR = texture(SCREEN_TEXTURE, UV); }}"

[sub_resource type="ShaderMaterial" id="ShaderMaterial_1"]
shader = SubResource("Shader_1")

[node name="Root" type="Node2D"]

[node name="Bg" type="Polygon2D" parent="."]
polygon = PackedVector2Array(0, 0, 16, 0, 16, 16, 0, 16)
color = Color(1, 0, 0, 1)
{copy}
[node name="Shaded" type="Sprite2D" parent="."]
texture = "tex"
position = Vector2(8, 8)
material = SubResource("ShaderMaterial_1")
"#
    )
}

fn render_screen_scene(with_copy: bool) -> Vec<Color> {
    gdobject::class_db::register_2d_classes();
    let scene = PackedScene::from_tscn(&screen_texture_scene(with_copy)).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let mut textures = TextureCache::new(".");
    // WHITE on purpose: the sprite's own texture is the fallback the sampler
    // used before it looked at the sampler name, so a white result means the
    // screen read did not happen.
    textures.insert(
        "tex",
        Texture2D::solid(TEX, TEX, Color::new(1.0, 1.0, 1.0, 1.0)),
    );

    let items = build_runtime_render_list(&tree);
    let groups = collect_canvas_groups(&tree);
    rasterize_render_list_with_groups(
        &items,
        &groups,
        S,
        S,
        Color::new(0.0, 0.0, 0.0, 1.0),
        Some(&mut textures),
    )
    .pixels
}

/// **The acceptance's BackBufferCopy clause, second half.** A shader sampling
/// `SCREEN_TEXTURE` reads the copied frame, not black and not its own texture.
#[test]
fn back_buffer_copy_feeds_screen_texture_to_a_canvas_shader() {
    let with = render_screen_scene(true);
    // The sprite is centred on (8, 8) over an 8x8 texture, so it covers 4..12.
    let px = with[(8 * S + 8) as usize];

    assert!(
        px.r > 0.5,
        "SCREEN_TEXTURE should have sampled the red frame beneath, got {px:?}"
    );
    assert!(
        px.g < 0.5 && px.b < 0.5,
        "the sampled colour should be the red background, not the white texture          or a blend, got {px:?}"
    );
}

/// Control: with no `BackBufferCopy` there is no back buffer, so the shader
/// must NOT come back red.
///
/// This is what proves the test above measures the copy rather than the
/// background showing through. Before this work the sampler ignored the sampler
/// name entirely and handed back the item's own texture, so both scenes would
/// have rendered identically.
#[test]
fn without_a_back_buffer_copy_the_shader_does_not_read_the_frame() {
    let without = render_screen_scene(false);
    let px = without[(8 * S + 8) as usize];

    assert!(
        !(px.r > 0.5 && px.g < 0.5 && px.b < 0.5),
        "with no BackBufferCopy the shader must not sample the red frame, got {px:?}"
    );
}

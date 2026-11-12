//! pat-0q2kx.109: a `.tscn` Label3D produces real text geometry in the frame.
//!
//! Godot's `Label3D` (upstream/godot/scene/3d/label_3d.cpp) rasterizes shaped
//! text into a camera-facing quad tinted by `modulate`. Patina had the whole
//! model — `gdserver3d::label3d` with glyph layout and billboard basis — as an
//! **orphan library**: a workspace-wide grep for callers outside its own crate
//! returned nothing, and the ClassDB entry was a bare parent-only registration
//! with no properties. A `.tscn` `Label3D text="HI"` therefore had no property
//! to land on and produced no geometry.
//!
//! This drives `RenderServer3DAdapter::render_frame` — the sync the running
//! game and the editor viewport both use — and asserts on the instance it
//! produced, so "the library exists but nothing reads it" fails.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;
use gdserver3d::instance::Instance3D;

const W: u32 = 160;
const H: u32 = 120;

fn load(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

/// Builds a one-Label3D scene with the given property block and returns every
/// synced instance.
fn synced(props: &str) -> Vec<Instance3D> {
    let tscn = format!(
        r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 4)
current = true

[node name="Text" type="Label3D" parent="."]
{props}
"#
    );
    let tree = load(&tscn);
    let mut adapter = RenderServer3DAdapter::new(W, H);
    let _ = adapter.render_frame(&tree);
    adapter.instances().to_vec()
}

/// The mesh a synced instance carries. `Instance3D::mesh` is optional, so a
/// `None` here means the sync produced an instance with no geometry at all.
fn mesh_of(inst: &Instance3D) -> &gdserver3d::mesh::Mesh3D {
    inst.mesh
        .as_ref()
        .expect("a synced Label3D instance must carry a mesh")
}

/// The single instance a one-label scene must produce.
fn only_instance(props: &str) -> Instance3D {
    let instances = synced(props);
    assert_eq!(
        instances.len(),
        1,
        "a Label3D must sync exactly one render instance; got {} — zero means \
         no extraction path reads Label3D at all, which is the bug",
        instances.len()
    );
    instances[0].clone()
}

/// **The acceptance test.** Green billboard text produces real geometry, and
/// editing `text` re-renders different geometry.
#[test]
fn label3d_renders_text_quad() {
    // -- (1) text="HI", green, billboard -> real tinted geometry -------------
    let inst = only_instance(
        r#"text = "HI"
modulate = Color(0, 1, 0, 1)
billboard = 1"#,
    );

    // Two visible glyphs -> two quads -> 8 vertices, 12 indices. A marker cube
    // or an empty mesh would not match, which is what distinguishes "renders
    // the text" from "renders something".
    assert_eq!(
        mesh_of(&inst).vertices.len(),
        8,
        "\"HI\" is 2 visible glyphs = 2 quads = 8 vertices, got {}",
        mesh_of(&inst).vertices.len()
    );
    assert_eq!(
        mesh_of(&inst).indices.len(),
        12,
        "2 quads = 12 triangle indices"
    );

    // The modulate tint reaches the material, so the pixels come out green
    // rather than the default white/orange marker.
    let a = inst
        .material
        .as_ref()
        .expect("a synced Label3D must carry a material")
        .albedo;
    assert!(
        a.g > 0.9 && a.r < 0.1 && a.b < 0.1,
        "modulate = green must reach the material albedo, got {a:?}"
    );

    assert!(
        inst.visible,
        "a Label3D with no `visible = false` must sync as visible"
    );

    // -- (2) changing the text re-renders different geometry -----------------
    let longer = only_instance(
        r#"text = "HELLO"
modulate = Color(0, 1, 0, 1)
billboard = 1"#,
    );
    assert_eq!(
        mesh_of(&longer).vertices.len(),
        20,
        "\"HELLO\" is 5 glyphs = 20 vertices; the mesh must be rebuilt from the \
         current text, not cached from the first sync"
    );
    assert_ne!(
        mesh_of(&longer).vertices.len(),
        mesh_of(&inst).vertices.len(),
        "a different `text` must produce different geometry"
    );
}

/// Whitespace advances the layout cursor but emits no quad, so it must not
/// inflate the vertex count — otherwise " " would look like a rendered glyph.
#[test]
fn whitespace_emits_no_geometry() {
    let inst = only_instance(r#"text = "A B""#);
    assert_eq!(
        mesh_of(&inst).vertices.len(),
        8,
        "\"A B\" has 2 visible glyphs; the space must advance the cursor \
         without emitting a quad"
    );
}

/// An empty label still syncs an instance (so it can be selected and edited)
/// but contributes no geometry.
#[test]
fn empty_text_produces_no_geometry() {
    let inst = only_instance(r#"text = """#);
    assert!(
        mesh_of(&inst).vertices.is_empty(),
        "empty text must produce an empty mesh, got {} vertices",
        mesh_of(&inst).vertices.len()
    );
}

/// A hidden Label3D syncs but is not visible — the mirror of the acceptance
/// case, so "always visible" cannot pass it.
#[test]
fn hidden_label_is_not_visible() {
    let inst = only_instance(
        r#"text = "HI"
visible = false"#,
    );
    assert!(
        !inst.visible,
        "a Label3D with visible = false must sync as not visible"
    );
}

/// `billboard = 0` (disabled) keeps the node's own basis; enabling it orients
/// the quad toward the camera instead. The two must differ, or billboarding is
/// not being applied.
#[test]
fn billboard_changes_the_instance_basis() {
    // Placed off-axis so a camera-facing basis is visibly different from the
    // node's identity basis; head-on they could legitimately coincide.
    let off_axis = "transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 3, 2, 0)\n";
    let fixed = only_instance(&format!("{off_axis}text = \"HI\"\nbillboard = 0"));
    let billboarded = only_instance(&format!("{off_axis}text = \"HI\"\nbillboard = 1"));

    assert_ne!(
        format!("{:?}", fixed.transform.basis),
        format!("{:?}", billboarded.transform.basis),
        "enabling billboard must reorient the quad toward the camera; an \
         unchanged basis means the billboard mode is being ignored"
    );
}

/// A Label3D removed from the tree frees its instance rather than leaving a
/// ghost row of text in the frame.
#[test]
fn removing_the_label_frees_its_instance() {
    let tree = load(
        r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 4)
current = true

[node name="Text" type="Label3D" parent="."]
text = "HI"
"#,
    );
    let mut adapter = RenderServer3DAdapter::new(W, H);
    let _ = adapter.render_frame(&tree);
    assert_eq!(adapter.instances().len(), 1);

    let mut tree = tree;
    let label = tree
        .get_node_by_path("/root/Root/Text")
        .expect("label node");
    tree.remove_node(label).expect("remove the label node");
    let _ = adapter.render_frame(&tree);
    assert!(
        adapter.instances().is_empty(),
        "removing the Label3D must free its render instance, leaving no ghost \
         geometry"
    );
}

//! pat-s3rrv.256 — the graph-organization nodes carry no shader code and a
//! reroute compiles away entirely.
//!
//! # The gap
//!
//! `VisualShaderNodeFrame`, `VisualShaderNodeComment`, `VisualShaderNodeReroute`,
//! `VisualShaderNodeGroupBase` and `VisualShaderNodeCustom` were all absent from
//! ClassDB even though their base `VisualShaderNodeResizableBase` and 66 other
//! `VisualShaderNode*` classes were registered — so
//! `get_inheriters_from_class("VisualShaderNodeResizableBase")` was truncated
//! exactly at the group/frame level.
//!
//! The loader half was worse than a gap. `visual_shader_from_resource` drops a
//! node class it cannot map to a `VisualShaderNodeType`, and for a **reroute**
//! that silently breaks the graph: the reroute's id disappears while the
//! connection quads still name it, so `A -> R` and `R -> B` both dangle and the
//! edge the author drew is gone. The shader still compiles, which is precisely
//! what makes it dangerous — a wire elbow, which by definition changes nothing,
//! changed the output.
//!
//! ⚠️ **`VisualShaderNodeCustom` is deliberately not exercised here.** The bead
//! lists it, but pat-s3rrv.232 already landed the script-defined-node path in
//! `gdscene/src/visual_shader_custom.rs` with its own coverage in
//! `visual_shader_node_custom_code_emission.rs`. Re-testing it here would
//! duplicate that bead rather than prove anything about this one; the ClassDB
//! registration it was missing is asserted below.

use gdobject::class_db::{class_exists, get_class_info, register_all_classes};
use gdresource::loader::TresLoader;
use gdresource::saver::TresSaver;
use gdscene::visual_shader_load::visual_shader_from_resource;
use gdserver2d::material::VisualShader;
use gdvariant::Variant;

/// The reference graph: a colour constant multiplied by UV, into the output.
///
/// Taken from `visualshader_tres.rs`, which already proves this exact shape
/// loads and compiles — so a failure here is about the organization nodes and
/// not about the fixture.
const PLAIN: &str = r#"[gd_resource type="VisualShader" format=3]

[sub_resource type="VisualShaderNodeColorConstant" id="Color_1"]
constant = Color(1, 0, 0, 1)

[sub_resource type="VisualShaderNodeInput" id="Input_1"]
input_name = "uv"

[sub_resource type="VisualShaderNodeVectorOp" id="VecOp_1"]
operator = 2

[resource]
nodes/fragment/2/node = SubResource("Color_1")
nodes/fragment/2/position = Vector2(-400, 40)
nodes/fragment/3/node = SubResource("Input_1")
nodes/fragment/3/position = Vector2(-400, 200)
nodes/fragment/4/node = SubResource("VecOp_1")
nodes/fragment/4/position = Vector2(-160, 100)
nodes/fragment/connections = PackedInt32Array(2, 0, 4, 0, 3, 0, 4, 1, 4, 0, 0, 0)
"#;

/// The same graph with the UV wire routed through a reroute dot (id 5).
const REROUTED: &str = r#"[gd_resource type="VisualShader" format=3]

[sub_resource type="VisualShaderNodeColorConstant" id="Color_1"]
constant = Color(1, 0, 0, 1)

[sub_resource type="VisualShaderNodeInput" id="Input_1"]
input_name = "uv"

[sub_resource type="VisualShaderNodeVectorOp" id="VecOp_1"]
operator = 2

[sub_resource type="VisualShaderNodeReroute" id="Reroute_1"]

[resource]
nodes/fragment/2/node = SubResource("Color_1")
nodes/fragment/2/position = Vector2(-400, 40)
nodes/fragment/3/node = SubResource("Input_1")
nodes/fragment/3/position = Vector2(-400, 200)
nodes/fragment/4/node = SubResource("VecOp_1")
nodes/fragment/4/position = Vector2(-160, 100)
nodes/fragment/5/node = SubResource("Reroute_1")
nodes/fragment/5/position = Vector2(-280, 200)
nodes/fragment/connections = PackedInt32Array(2, 0, 4, 0, 3, 0, 5, 0, 5, 0, 4, 1, 4, 0, 0, 0)
"#;

/// The reference graph plus a Frame annotating it.
const FRAMED: &str = r#"[gd_resource type="VisualShader" format=3]

[sub_resource type="VisualShaderNodeColorConstant" id="Color_1"]
constant = Color(1, 0, 0, 1)

[sub_resource type="VisualShaderNodeInput" id="Input_1"]
input_name = "uv"

[sub_resource type="VisualShaderNodeVectorOp" id="VecOp_1"]
operator = 2

[sub_resource type="VisualShaderNodeFrame" id="Frame_1"]
title = "Tinting"
attached_nodes = PackedInt32Array(2, 3, 4)

[resource]
nodes/fragment/2/node = SubResource("Color_1")
nodes/fragment/2/position = Vector2(-400, 40)
nodes/fragment/3/node = SubResource("Input_1")
nodes/fragment/3/position = Vector2(-400, 200)
nodes/fragment/4/node = SubResource("VecOp_1")
nodes/fragment/4/position = Vector2(-160, 100)
nodes/fragment/6/node = SubResource("Frame_1")
nodes/fragment/6/position = Vector2(-460, 0)
nodes/fragment/connections = PackedInt32Array(2, 0, 4, 0, 3, 0, 4, 1, 4, 0, 0, 0)
"#;

fn load(tres: &str) -> VisualShader {
    let res = TresLoader::new()
        .parse_str(tres, "res://graph.tres")
        .expect("the .tres parses");
    visual_shader_from_resource(&res).expect("it is a VisualShader")
}

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** A reroute compiles away to byte-identical source,
/// and a Frame instantiates under `VisualShaderResizableBase`, emits nothing,
/// and survives a save/load round trip with its title and attachments.
#[test]
fn visual_shader_graph_organization_nodes_round_trip() {
    register_all_classes();

    // -- the Frame's registration -------------------------------------------
    assert!(
        class_exists("VisualShaderNodeFrame"),
        "VisualShaderNodeFrame must be registered — without it the resizable \
         subtree is truncated at the frame level"
    );
    assert_eq!(
        get_class_info("VisualShaderNodeFrame")
            .expect("registered")
            .parent_class,
        "VisualShaderNodeResizableBase",
        "Frame inherits VisualShaderNodeResizableBase per its doc XML"
    );

    // -- a reroute changes nothing ------------------------------------------
    // Byte-identical, not merely equivalent: a reroute is a wire elbow, so any
    // difference at all in the emitted source is a bug.
    let plain = load(PLAIN);
    let rerouted = load(REROUTED);
    assert_eq!(
        rerouted.get_code(),
        plain.get_code(),
        "UV -> Reroute -> Multiply must compile byte-identically to \
         UV -> Multiply; a dropped reroute leaves the UV wire dangling and \
         silently changes the shader"
    );

    // The control for the comparison above: if `get_code` returned a constant
    // — a passthrough emitted because no output node was found, say — the two
    // graphs would match no matter how badly the reroute was handled.
    assert!(
        plain.get_code().contains("UV"),
        "the reference graph must actually compile the UV wire into its \
         source, or the equality above compares two identical stubs:\n{}",
        plain.get_code()
    );

    // -- a Frame contributes no code ----------------------------------------
    let framed = load(FRAMED);
    assert_eq!(
        framed.get_code(),
        plain.get_code(),
        "a Frame is an annotation; it must not alter the generated shader"
    );

    // ⚠️ Not just "no code" — no *complaint* either. An unmapped class is
    // reported as "dropped from the graph", which for a Frame is a false alarm
    // that buries the real diagnostics sharing that list.
    assert!(
        !framed
            .diagnostics
            .iter()
            .any(|d| d.contains("VisualShaderNodeFrame")),
        "a Frame carries no shader code by design, so it must not be reported \
         as a dropped node: {:?}",
        framed.diagnostics
    );

    // -- the Frame survives a save/load round trip ---------------------------
    // Through the real TresSaver, because that is what the editor writes back
    // over the file when a graph is edited.
    let parsed = TresLoader::new()
        .parse_str(FRAMED, "res://graph.tres")
        .expect("the framed .tres parses");
    let saved = TresSaver::new()
        .save_to_string(&parsed)
        .expect("the resource serialises");
    let reparsed = TresLoader::new()
        .parse_str(&saved, "res://graph.tres")
        .expect("the saved .tres parses again");

    let frame = reparsed
        .subresources
        .values()
        .find(|s| s.class_name == "VisualShaderNodeFrame")
        .expect("the Frame sub-resource must survive the round trip");
    assert_eq!(
        frame.get_property("title"),
        Some(&Variant::String("Tinting".into())),
        "the Frame's title must survive save/load"
    );
    assert_eq!(
        frame.get_property("attached_nodes"),
        Some(&Variant::PackedInt32Array(vec![2, 3, 4])),
        "and so must the nodes it visually contains"
    );
}

/// The other four classes the bead names are registered under their Godot
/// parents.
///
/// ⚠️ `Reroute` and `Custom` inherit **`VisualShaderNode`**, not `GroupBase` —
/// the bead's prose groups all five together, which invites registering them
/// under a common base they do not share.
#[test]
fn visual_shader_graph_organization_nodes_round_trip_family_parents() {
    register_all_classes();

    for (class, parent) in [
        ("VisualShaderNodeGroupBase", "VisualShaderNodeResizableBase"),
        ("VisualShaderNodeComment", "VisualShaderNodeFrame"),
        ("VisualShaderNodeReroute", "VisualShaderNode"),
        ("VisualShaderNodeCustom", "VisualShaderNode"),
    ] {
        assert!(class_exists(class), "{class} must be registered");
        assert_eq!(
            get_class_info(class).expect("registered").parent_class,
            parent,
            "{class} inherits {parent} per its doc XML"
        );
    }

    // The control: `get_class_info` reports what was registered rather than
    // answering yes to anything asked of it.
    assert!(
        get_class_info("VisualShaderNodeNotARealClass").is_none(),
        "a class lookup that invents entries would satisfy every assertion above"
    );
}

/// A chain of reroutes collapses transitively, and a reroute feeding nothing
/// drops its edge instead of hanging the loader.
#[test]
fn visual_shader_graph_organization_nodes_round_trip_reroute_chain() {
    // `A -> R1 -> R2 -> B` must resolve all the way back to A; a single-hop
    // resolver passes the acceptance above and fails here.
    let chained = REROUTED
        .replace(
            r#"[sub_resource type="VisualShaderNodeReroute" id="Reroute_1"]"#,
            "[sub_resource type=\"VisualShaderNodeReroute\" id=\"Reroute_1\"]\n\n\
             [sub_resource type=\"VisualShaderNodeReroute\" id=\"Reroute_2\"]",
        )
        .replace(
            "nodes/fragment/connections = PackedInt32Array(2, 0, 4, 0, 3, 0, 5, 0, 5, 0, 4, 1, 4, 0, 0, 0)",
            "nodes/fragment/7/node = SubResource(\"Reroute_2\")\n\
             nodes/fragment/7/position = Vector2(-220, 200)\n\
             nodes/fragment/connections = PackedInt32Array(2, 0, 4, 0, 3, 0, 5, 0, 5, 0, 7, 0, 7, 0, 4, 1, 4, 0, 0, 0)",
        );
    assert!(
        chained.contains("Reroute_2"),
        "the fixture edit must have applied, or this test proves nothing"
    );

    assert_eq!(
        load(&chained).get_code(),
        load(PLAIN).get_code(),
        "a chain of reroutes must collapse transitively to the same source"
    );

    // A reroute with no input: the edge out of it carries no value, so it is
    // dropped. The graph must still load rather than dangle or spin.
    let orphan = REROUTED.replace("3, 0, 5, 0, ", "");
    assert!(
        orphan.contains("Reroute_1"),
        "the orphan fixture keeps the reroute, only its input is removed"
    );
    let loaded = load(&orphan);
    assert!(
        !loaded.get_code().is_empty(),
        "an unfed reroute must not prevent the graph compiling"
    );
}

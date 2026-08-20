//! pat-s3rrv.256 — the graph-organization pre-pass, tested directly.
//!
//! The bead's acceptance
//! (`visual_shader_graph_organization_nodes_round_trip`) drives this through
//! `visual_shader_from_resource` and asserts on generated shader source. That is
//! the right end-to-end statement, but shader text is a lossy view of the
//! rewrite: several different bugs in the connection quads produce the same
//! source, and a passing source comparison would not tell you the quads are
//! actually right.
//!
//! So this file asserts on the rewritten `Resource` itself — the exact quads,
//! the exact removed keys — which is where the reroute logic can actually be
//! wrong.
//!
//! ⚠️ **The most important assertion here is that the caller's `Resource` is
//! left alone.** The rewrite exists only to feed codegen; the editor saves the
//! *original* back over the `.tres`, so a pre-pass that mutated in place would
//! silently delete every Frame and Reroute from the user's file the first time
//! they edited a graph. That is data loss, not a rendering bug.

use gdresource::loader::TresLoader;
use gdresource::resource::Resource;
use gdscene::visual_shader_organization::collapse_graph_organization;
use gdvariant::Variant;

const PLAIN: &str = r#"[gd_resource type="VisualShader" format=3]

[sub_resource type="VisualShaderNodeColorConstant" id="Color_1"]
constant = Color(1, 0, 0, 1)

[sub_resource type="VisualShaderNodeInput" id="Input_1"]
input_name = "uv"

[sub_resource type="VisualShaderNodeVectorOp" id="VecOp_1"]
operator = 2

[resource]
nodes/fragment/2/node = SubResource("Color_1")
nodes/fragment/3/node = SubResource("Input_1")
nodes/fragment/4/node = SubResource("VecOp_1")
nodes/fragment/connections = PackedInt32Array(2, 0, 4, 0, 3, 0, 4, 1, 4, 0, 0, 0)
"#;

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
nodes/fragment/3/node = SubResource("Input_1")
nodes/fragment/4/node = SubResource("VecOp_1")
nodes/fragment/5/node = SubResource("Reroute_1")
nodes/fragment/5/position = Vector2(-280, 200)
nodes/fragment/connections = PackedInt32Array(2, 0, 4, 0, 3, 0, 5, 0, 5, 0, 4, 1, 4, 0, 0, 0)
"#;

fn parse(tres: &str) -> Resource {
    (*TresLoader::new()
        .parse_str(tres, "res://graph.tres")
        .expect("the .tres parses"))
    .clone()
}

fn connections(res: &Resource) -> Vec<i32> {
    match res.get_property("nodes/fragment/connections") {
        Some(Variant::PackedInt32Array(v)) => v.clone(),
        other => panic!("expected a connections array, got {other:?}"),
    }
}

// ===========================================================================
// The reroute rewrite
// ===========================================================================

/// `A -> R -> B` becomes exactly the quads of the hand-drawn `A -> B` graph.
#[test]
fn visual_shader_organization_prepass_collapses_a_reroute() {
    let original = parse(REROUTED);
    let collapsed = collapse_graph_organization(&original).expect("the graph has a reroute");

    // Quad-for-quad, including order: the reroute's outgoing edge takes the
    // place its own edge occupied, so the result is not merely the same set.
    assert_eq!(
        connections(&collapsed),
        connections(&parse(PLAIN)),
        "the collapsed graph's connection quads must equal the reroute-free \
         graph's, in the same order"
    );

    // The reroute's own rows are gone, so the loader never sees a node it
    // cannot map and never reports one as dropped.
    for key in ["nodes/fragment/5/node", "nodes/fragment/5/position"] {
        assert!(
            collapsed.get_property(key).is_none(),
            "{key} must be removed along with the reroute"
        );
    }

    // ⚠️ The load-bearing one: the caller's resource is untouched, because the
    // editor writes *it* back over the user's file.
    assert!(
        original.get_property("nodes/fragment/5/node").is_some(),
        "the pre-pass must not mutate the caller's resource — the editor saves \
         that one back to disk, so dropping the reroute here is data loss"
    );
    assert_eq!(
        connections(&original),
        vec![2, 0, 4, 0, 3, 0, 5, 0, 5, 0, 4, 1, 4, 0, 0, 0],
        "…and its connections are still the authored ones"
    );
}

/// A graph with no organization nodes is reported as needing no rewrite.
///
/// The `None` is what keeps every existing graph on the old path with no clone;
/// a version that always returned `Some(clone)` would pass every other test here
/// while making the common case allocate.
#[test]
fn visual_shader_organization_prepass_leaves_an_ordinary_graph_alone() {
    assert!(
        collapse_graph_organization(&parse(PLAIN)).is_none(),
        "a graph with no Frame, Comment or Reroute needs no rewrite"
    );
    // Not a VisualShader at all.
    assert!(
        collapse_graph_organization(&Resource::new("Shader")).is_none(),
        "a non-VisualShader resource is not this pass's business"
    );
}

/// `A -> R1 -> R2 -> B` resolves all the way back to A.
#[test]
fn visual_shader_organization_prepass_collapses_a_reroute_chain() {
    let chained = REROUTED
        .replace(
            r#"[sub_resource type="VisualShaderNodeReroute" id="Reroute_1"]"#,
            "[sub_resource type=\"VisualShaderNodeReroute\" id=\"Reroute_1\"]\n\n\
             [sub_resource type=\"VisualShaderNodeReroute\" id=\"Reroute_2\"]",
        )
        .replace(
            "nodes/fragment/connections = PackedInt32Array(2, 0, 4, 0, 3, 0, 5, 0, 5, 0, 4, 1, 4, 0, 0, 0)",
            "nodes/fragment/7/node = SubResource(\"Reroute_2\")\n\
             nodes/fragment/connections = PackedInt32Array(2, 0, 4, 0, 3, 0, 5, 0, 5, 0, 7, 0, 7, 0, 4, 1, 4, 0, 0, 0)",
        );
    assert!(
        chained.contains("Reroute_2"),
        "the fixture edit must have applied, or this test proves nothing"
    );

    let collapsed = collapse_graph_organization(&parse(&chained)).expect("two reroutes");
    // A single-hop resolver yields `5, 0, 4, 1` here — the id of a node that no
    // longer exists — and the graph loses the UV wire exactly as before.
    assert_eq!(
        connections(&collapsed),
        connections(&parse(PLAIN)),
        "a chain of reroutes must resolve transitively back to the real source"
    );
}

/// A reroute fed by nothing drops its outgoing edge rather than emitting one
/// from a node that no longer exists.
#[test]
fn visual_shader_organization_prepass_drops_an_unfed_reroute() {
    let orphan = REROUTED.replace("3, 0, 5, 0, ", "");
    assert!(
        orphan.contains("Reroute_1") && !orphan.contains("3, 0, 5, 0"),
        "the orphan fixture keeps the reroute and removes only its input"
    );

    let collapsed = collapse_graph_organization(&parse(&orphan)).expect("has a reroute");
    let quads = connections(&collapsed);
    assert!(
        !quads.chunks_exact(4).any(|q| q[0] == 5 || q[2] == 5),
        "no edge may still name the removed reroute: {quads:?}"
    );
    // The rest of the graph is intact — the colour wire and the output edge.
    assert_eq!(
        quads,
        vec![2, 0, 4, 0, 4, 0, 0, 0],
        "only the unfed reroute's edge is dropped"
    );
}

/// A reroute whose input is itself terminates instead of hanging the loader.
///
/// Godot's editor cannot draw this, but a hand-edited or generated `.tres` can
/// contain it, and an unbounded resolver walk would hang the caller rather than
/// fail it — the worst failure mode available here.
#[test]
fn visual_shader_organization_prepass_survives_a_reroute_cycle() {
    let cyclic = REROUTED.replace("3, 0, 5, 0, 5, 0, 4, 1", "5, 0, 5, 0, 5, 0, 4, 1");
    assert!(
        cyclic.contains("5, 0, 5, 0"),
        "the fixture must actually contain the self-edge"
    );

    let collapsed = collapse_graph_organization(&parse(&cyclic)).expect("has a reroute");
    let quads = connections(&collapsed);
    assert!(
        !quads.chunks_exact(4).any(|q| q[0] == 5 || q[2] == 5),
        "the cyclic reroute must not survive into the rewritten graph: {quads:?}"
    );
}

// ===========================================================================
// Annotation nodes
// ===========================================================================

/// Frames and Comments are removed from the codegen copy and kept in the
/// caller's.
#[test]
fn visual_shader_organization_prepass_removes_annotation_nodes() {
    let framed = PLAIN.replace(
        "[resource]",
        "[sub_resource type=\"VisualShaderNodeFrame\" id=\"Frame_1\"]\n\
         title = \"Tinting\"\n\n\
         [sub_resource type=\"VisualShaderNodeComment\" id=\"Comment_1\"]\n\n\
         [resource]\n\
         nodes/fragment/6/node = SubResource(\"Frame_1\")\n\
         nodes/fragment/8/node = SubResource(\"Comment_1\")",
    );
    let original = parse(&framed);
    let collapsed = collapse_graph_organization(&original).expect("has annotations");

    for key in ["nodes/fragment/6/node", "nodes/fragment/8/node"] {
        assert!(
            collapsed.get_property(key).is_none(),
            "{key} is an annotation node and must not reach the loader"
        );
        assert!(
            original.get_property(key).is_some(),
            "{key} must survive in the caller's resource for the save round trip"
        );
    }

    // The real nodes and their wiring are untouched — the control that stops
    // this passing for a pre-pass that simply emptied the graph.
    assert_eq!(
        connections(&collapsed),
        connections(&parse(PLAIN)),
        "removing annotations must not disturb the graph's connections"
    );
    assert!(
        collapsed.get_property("nodes/fragment/2/node").is_some(),
        "the colour constant is a real node and must remain"
    );
}

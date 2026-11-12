//! pat-0q2kx.226 (partial) — a `.tres` `VisualShader` loads into a graph and
//! compiles to spatial shader source.
//!
//! Godot ground truth: `scene/resources/visual_shader.cpp`. The resource stores
//! `nodes/<function>/<id>/node = SubResource(...)` plus
//! `nodes/<function>/connections = PackedInt32Array(from, from_port, to,
//! to_port, …)`, and `get_code()` emits for the graph's declared mode.
//!
//! ⚠️ **Two parts of the bead description are stale and worth correcting.**
//!
//! 1. The verifier note says `get_code()` "always emits `shader_type
//!    canvas_item`, so spatial graphs are impossible". **pat-0q2kx.171 already
//!    landed `VisualShaderMode`**, and `get_code()` has emitted
//!    `shader_type spatial` with `ALBEDO = (…).rgb` since. That half was done.
//! 2. It also says the `.tres` side needs parsing work in `res_loader.rs`.
//!    `TresLoader` has always parsed *any* resource generically —
//!    `[sub_resource type=…]` sections and `[resource]` properties both land in
//!    `Resource`. What genuinely did not exist is anything converting that
//!    generic bag into a graph, which is what `visual_shader_load` adds.
//!
//! NOTE ON THE FILE NAME: the acceptance names `visualshader_tres_load_render`;
//! that is the test **function**. The binary is `visualshader_tres` so the
//! command stays under the verifier's 58-character truncation.
//!
//! ⚠️ NOT delivered: the ~100-class node set (this maps the constants, the
//! op family, Mix, Texture and the `uv` Input), Parameter/uniform emission,
//! Expression nodes, the particles/sky/fog modes, and routing a VisualShader
//! sub-resource through `packed_scene`'s ShaderMaterial flattening so a scene
//! renders it end to end. See the completion report.

use gdresource::loader::TresLoader;
use gdscene::visual_shader_load::visual_shader_from_resource;
use gdserver2d::material::{VisualShaderMode, VisualShaderNodeType};

/// The bead's graph: a colour constant multiplied by UV, into the albedo output.
///
/// `mode` is omitted deliberately — Godot writes no `mode` line for
/// `MODE_SPATIAL` (enum 0), so this is exactly what the editor saves for a
/// spatial graph, and it is the case a loader is most likely to get wrong.
const SPATIAL_TRES: &str = r#"[gd_resource type="VisualShader" format=3]

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

fn load(tres: &str) -> gdserver2d::material::VisualShader {
    let res = TresLoader::new()
        .parse_str(tres, "res://graph.tres")
        .expect("the .tres parses");
    visual_shader_from_resource(&res).expect("it is a VisualShader")
}

/// **The acceptance.** The graph loads, and compiles to spatial source carrying
/// the authored colour.
#[test]
fn visualshader_tres_load_render() {
    let shader = load(SPATIAL_TRES);

    // -- the graph came across ----------------------------------------------
    assert_eq!(
        shader.mode,
        VisualShaderMode::Spatial,
        "a .tres with no `mode` line is MODE_SPATIAL — Godot omits a property at \
         its default, so reading the absence as canvas_item would compile every \
         default graph to the wrong stage"
    );
    // Three authored nodes plus the implicit output.
    assert_eq!(
        shader.nodes.len(),
        4,
        "three authored nodes and the implicit output node, got {:?}",
        shader.nodes.iter().map(|n| n.id).collect::<Vec<_>>()
    );
    assert!(
        shader
            .nodes
            .iter()
            .any(|n| n.id == 0 && n.node_type == VisualShaderNodeType::Output),
        "Godot does not serialise the output node — it must be synthesised at \
         id 0, or codegen finds none and emits the passthrough"
    );
    assert!(
        shader
            .nodes
            .iter()
            .any(|n| n.node_type == VisualShaderNodeType::Uv),
        "the `uv` Input node must map to the UV kind"
    );
    assert!(
        shader
            .nodes
            .iter()
            .any(|n| n.node_type == VisualShaderNodeType::Multiply),
        "operator = 2 is OP_MUL, so the VectorOp must be a multiply"
    );
    assert_eq!(
        shader.connections.len(),
        3,
        "the PackedInt32Array holds three quads"
    );

    // -- and it compiles to spatial source ----------------------------------
    let code = shader.get_code();
    assert!(
        code.contains("shader_type spatial"),
        "the acceptance's `shader_type spatial` is missing:\n{code}"
    );
    assert!(
        code.contains("ALBEDO"),
        "a spatial graph writes ALBEDO:\n{code}"
    );
    // The authored red must reach the expression, not be dropped on the way.
    assert!(
        code.contains("vec4(1.0, 0.0, 0.0, 1.0)"),
        "the ColorConstant's value must appear in the generated source:\n{code}"
    );
    // ⚠️ `vec2 * vec4` does not compile in GDShader, so the UV operand is
    // widened. A bare `UV *` here would be source that fails at shader-compile
    // time, far from the graph that caused it.
    assert!(
        code.contains("vec4(UV, 0.0, 1.0)"),
        "the UV operand must be widened to vec4 for the multiply:\n{code}"
    );
    assert!(
        !code.contains("ALBEDO = ALBEDO"),
        "the passthrough means the output node was never connected:\n{code}"
    );
}

/// `mode = 1` is CANVAS_ITEM, so the same graph compiles to the 2D stage — the
/// control proving the spatial result above comes from the mode and not from a
/// hardcoded branch.
#[test]
fn an_explicit_canvas_item_mode_is_honoured() {
    let tres = SPATIAL_TRES.replace("[resource]", "[resource]\nmode = 1");
    let shader = load(&tres);

    assert_eq!(shader.mode, VisualShaderMode::CanvasItem);
    let code = shader.get_code();
    assert!(
        code.contains("shader_type canvas_item") && code.contains("COLOR ="),
        "an explicit mode = 1 must compile to the canvas_item stage:\n{code}"
    );
}

/// A resource that is not a VisualShader returns `None`, so a caller can fall
/// through to the plain-Shader path without sniffing class names itself.
#[test]
fn a_non_visualshader_resource_is_declined() {
    let res = TresLoader::new()
        .parse_str(
            "[gd_resource type=\"StyleBoxFlat\" format=3]\n\n[resource]\n",
            "res://box.tres",
        )
        .expect("parses");
    assert!(visual_shader_from_resource(&res).is_none());
}

/// An unsupported node class is skipped rather than mapped to something wrong —
/// the graph still loads and still compiles.
///
/// ⚠️ The fixture was `VisualShaderNodeBillboard` until pat-s3rrv.255, which
/// gave Billboard real codegen. The test then failed for the best possible
/// reason: its premise had become false. Swapped to a class that is still
/// unmapped, because the behaviour under test — an unknown class must not be
/// guessed into some other kind — is unchanged and still worth pinning.
#[test]
fn an_unsupported_node_class_is_skipped_not_guessed() {
    let tres = r#"[gd_resource type="VisualShader" format=3]

[sub_resource type="VisualShaderNodeCubemap" id="Cube_1"]

[resource]
nodes/fragment/2/node = SubResource("Cube_1")
"#;
    let shader = load(tres);
    assert_eq!(
        shader.nodes.len(),
        1,
        "only the synthesised output survives; an unknown class must not be \
         guessed into some other kind"
    );
    assert!(
        shader.get_code().contains("shader_type spatial"),
        "and the graph still compiles"
    );
    // pat-s3rrv.255: skipping is right, but skipping in SILENCE was not — the
    // graph used to load smaller than it was authored with nothing to say so.
    assert!(
        shader
            .diagnostics
            .iter()
            .any(|d| d.contains("VisualShaderNodeCubemap")),
        "the skipped class must be reported; got {:?}",
        shader.diagnostics
    );
}

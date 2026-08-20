//! pat-s3rrv.255 — the VisualShader function/parameter nodes emit real code.
//!
//! Godot ground truth: `upstream/godot/scene/resources/visual_shader_nodes.cpp`
//! (`VisualShaderNode{Clamp,ColorFunc,Remap,RandomRange,Billboard,
//! BooleanParameter}`) and `visual_shader.h` for `GlobalExpression`.
//!
//! ## Why the helper bodies are global functions
//!
//! Godot's `generate_code` emits *statements* into a block that assigns to an
//! output variable; Patina's codegen builds an *expression* per node. Rather
//! than restructure the codegen, the four colour functions and
//! `hash_noise_range` are emitted as global helper functions and the node
//! becomes a call — which is exactly what Godot itself does for
//! `hash_noise_range` via `generate_global_per_node`. It also means a graph
//! using the same function twice declares it once.
//!
//! ## What "silently wrong" meant before
//!
//! An unmapped node CLASS was dropped with a bare `continue` and an unmapped
//! OPERATOR became a multiply. Either way the graph compiled to valid source
//! that did the wrong thing, with nothing anywhere to say so. Both now record a
//! diagnostic on the loaded graph.

use gdresource::loader::TresLoader;
use gdscene::visual_shader_load::visual_shader_from_resource;
use gdserver2d::material::VisualShader;

/// The acceptance graph: `UV -> Remap -> Clamp -> ColorFunc(RGB2HSV) -> Output`,
/// with a BooleanParameter gate and a GlobalExpression block.
///
/// `function = 2` is FUNC_RGB2HSV; `mode = 1` is MODE_CANVAS_ITEM.
const GRAPH_TRES: &str = r#"[gd_resource type="VisualShader" format=3]

[sub_resource type="VisualShaderNodeInput" id="Input_1"]
input_name = "uv"

[sub_resource type="VisualShaderNodeRemap" id="Remap_1"]

[sub_resource type="VisualShaderNodeClamp" id="Clamp_1"]

[sub_resource type="VisualShaderNodeColorFunc" id="CFunc_1"]
function = 2

[sub_resource type="VisualShaderNodeBooleanParameter" id="Bool_1"]
parameter_name = "use_tint"
default_value = true

[sub_resource type="VisualShaderNodeGlobalExpression" id="Global_1"]
expression = "const float PATINA_MARKER = 0.25;"

[resource]
mode = 1
nodes/fragment/3/node = SubResource("Input_1")
nodes/fragment/4/node = SubResource("Remap_1")
nodes/fragment/5/node = SubResource("Clamp_1")
nodes/fragment/6/node = SubResource("CFunc_1")
nodes/fragment/7/node = SubResource("Bool_1")
nodes/fragment/8/node = SubResource("Global_1")
nodes/fragment/connections = PackedInt32Array(3, 0, 4, 0, 4, 0, 5, 0, 5, 0, 6, 0, 6, 0, 0, 0)
"#;

/// The same shape, but the operator node uses OP_POW (5), which this codegen
/// cannot emit, and a node class that has no mapping at all.
const UNMAPPED_TRES: &str = r#"[gd_resource type="VisualShader" format=3]

[sub_resource type="VisualShaderNodeInput" id="Input_1"]
input_name = "uv"

[sub_resource type="VisualShaderNodeVectorOp" id="VecOp_1"]
operator = 5

[sub_resource type="VisualShaderNodeCubemap" id="Cube_1"]

[resource]
mode = 1
nodes/fragment/2/node = SubResource("Input_1")
nodes/fragment/3/node = SubResource("VecOp_1")
nodes/fragment/4/node = SubResource("Cube_1")
nodes/fragment/connections = PackedInt32Array(2, 0, 3, 0, 3, 0, 0, 0)
"#;

fn load(tres: &str) -> VisualShader {
    let res = TresLoader::new()
        .parse_str(tres, "res://graph.tres")
        .expect("the .tres parses");
    visual_shader_from_resource(&res).expect("it is a VisualShader")
}

/// **The acceptance.**
#[test]
fn visual_shader_function_nodes_emit_code() {
    let shader = load(GRAPH_TRES);
    let code = shader.get_code();

    // Every authored node survived the load. Before this change the five
    // unmapped classes were dropped, so the graph was UV straight into output.
    assert_eq!(
        shader.nodes.len(),
        7,
        "six authored nodes plus the implicit output; got {:?}",
        shader
            .nodes
            .iter()
            .map(|n| (n.id, n.node_type))
            .collect::<Vec<_>>()
    );
    assert!(
        shader.diagnostics.is_empty(),
        "a fully mapped graph must report nothing; got {:?}",
        shader.diagnostics
    );

    // -- the clamp call -----------------------------------------------------
    assert!(
        code.contains("clamp("),
        "the Clamp node must emit a clamp call:\n{code}"
    );

    // -- the remap arithmetic ------------------------------------------------
    //
    // Godot computes `out_min + __output_range * ((value - in_min) /
    // __input_range)`. The temporaries are inlined, so the assertion is on the
    // division by the input range, which is the part that distinguishes a remap
    // from any other arithmetic.
    assert!(
        code.contains("/ (1.0 - 0.0)"),
        "the Remap node must divide by its input range; Godot's port defaults \
         are in_min 0.0 and in_max 1.0:\n{code}"
    );

    // -- the RGB2HSV body ----------------------------------------------------
    assert!(
        code.contains("vec3 rgb2hsv(vec3 c)"),
        "the RGB2HSV helper must be declared:\n{code}"
    );
    assert!(
        code.contains("vec4 K = vec4(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);"),
        "…with Godot's exact body, not a paraphrase:\n{code}"
    );
    assert!(
        code.contains("rgb2hsv("),
        "…and the node must call it:\n{code}"
    );
    // ⚠️ `function = 2` is RGB2HSV. Defaulting a missing or misread property to 0
    // would silently emit GRAYSCALE, which still compiles.
    assert!(
        !code.contains("vec3 grayscale(vec3 c)"),
        "function = 2 is RGB2HSV, not the zeroth constant GRAYSCALE:\n{code}"
    );

    // -- the boolean uniform -------------------------------------------------
    assert!(
        code.contains("uniform bool use_tint = true;"),
        "the BooleanParameter must declare its uniform with its default:\n{code}"
    );

    // -- global scope, not inside fragment() ---------------------------------
    let marker = code
        .find("const float PATINA_MARKER = 0.25;")
        .unwrap_or_else(|| panic!("the GlobalExpression text must be emitted:\n{code}"));
    let fragment = code
        .find("void fragment()")
        .expect("the shader has a fragment function");
    assert!(
        marker < fragment,
        "the GlobalExpression must sit at GLOBAL scope, above fragment(), not \
         inside it — at {marker} vs fragment at {fragment}:\n{code}"
    );
    // The uniform and the helper are global too.
    assert!(
        code.find("uniform bool use_tint").expect("declared") < fragment,
        "the uniform must be global:\n{code}"
    );
    assert!(
        code.find("vec3 rgb2hsv(vec3 c)").expect("declared") < fragment,
        "the helper function must be global:\n{code}"
    );
}

/// An unmapped class and an unmapped operator each report a diagnostic.
///
/// This is the half of the acceptance that a source-content assertion cannot
/// reach: the old behaviour produced *valid* source, so only the absence of a
/// complaint distinguished it.
#[test]
fn visual_shader_function_nodes_emit_code_reports_unmapped_nodes() {
    let shader = load(UNMAPPED_TRES);
    let joined = shader.diagnostics.join("\n");

    assert!(
        shader
            .diagnostics
            .iter()
            .any(|d| d.contains("VisualShaderNodeCubemap")),
        "a class with no codegen must be reported, not dropped in silence; got \
         {:?}",
        shader.diagnostics
    );
    assert!(
        joined.contains("operator 5"),
        "an operator that falls back to multiply must say so; got {:?}",
        shader.diagnostics
    );
    assert!(
        joined.contains("multiply"),
        "…and name the fallback it used; got {:?}",
        shader.diagnostics
    );

    // The graph still compiles — a diagnostic is a report, not a failure.
    assert!(
        shader.get_code().contains("void fragment()"),
        "codegen must still succeed so the shader remains debuggable"
    );
}

/// A graph with none of the new nodes emits byte-identical source to before.
///
/// The globals block is spliced into the same `format!` that produced the old
/// output, so this pins that an empty block changes nothing — otherwise every
/// existing VisualShader would gain a stray blank line.
#[test]
fn visual_shader_function_nodes_emit_code_leaves_plain_graphs_unchanged() {
    const PLAIN: &str = r#"[gd_resource type="VisualShader" format=3]

[sub_resource type="VisualShaderNodeColorConstant" id="Color_1"]
constant = Color(1, 0, 0, 1)

[resource]
mode = 1
nodes/fragment/2/node = SubResource("Color_1")
nodes/fragment/connections = PackedInt32Array(2, 0, 0, 0)
"#;
    let code = load(PLAIN).get_code();
    assert!(
        code.starts_with("shader_type canvas_item;\n\nvoid fragment()"),
        "a graph with no globals must emit the original header exactly, with no \
         extra blank line:\n{code:?}"
    );
}

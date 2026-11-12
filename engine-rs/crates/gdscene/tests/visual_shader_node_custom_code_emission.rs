//! pat-s3rrv.232 — a script `extends VisualShaderNodeCustom` contributes its
//! `_get_global_code` and `_get_code` to the generated shader source.
//!
//! Godot ground truth: `upstream/godot/scene/resources/visual_shader.cpp`
//! (`VisualShaderNodeCustom::generate_code`, `generate_global_per_node`,
//! `_is_valid_code`, `get_caption`) and
//! `upstream/godot/doc/classes/VisualShaderNodeCustom.xml` for the 21 virtual
//! signatures. `extension_api.json`, which the bead cites, is not vendored —
//! the doc XML carries the same surface and is.
//!
//! # 🔑 The script does the substitution, not the engine
//!
//! Godot hands `_get_code` the real generated variable names as two `String`
//! arrays and splices the returned text **verbatim**. So the script writes
//! `"%s = %s * K;" % [output_vars[0], input_vars[0]]`, and what proves the
//! wiring is that the *caller's* names come back out inside the shader source.
//! A test that only asserted "the body line is present" would pass against an
//! implementation that ignored the names entirely.
//!
//! # ⚠️ Two things the bead gets wrong
//!
//! - It names `gdrender3d/src/shader.rs`. The generator is
//!   `VisualShader::get_code()` in `gdserver2d/src/material.rs`; this module
//!   lives in `gdscene` because that is the only crate that can see both the
//!   graph type and the interpreter (see the module docs for why).
//! - `_is_available` is **arity 2** `(mode, type)`, not a bare predicate.
//!
//! ⚠️ The acceptance's final clause — "`_is_available` returning false excludes
//! the node from the available-node list" — has **no destination in Patina**:
//! there is no available-node list anywhere in the workspace. The virtual is
//! implemented and asserted directly here; wiring it to a registry is tracked
//! as remaining scope rather than faked.

use gdscene::visual_shader_custom::{shader_mode_int, CustomShaderNode, TYPE_FRAGMENT};
use gdserver2d::material::{VisualShader, VisualShaderMode};

/// The acceptance's node: one input, one output, a global constant, and a body
/// line that formats in the caller's variable names.
const DOUBLE_IT: &str = r#"@tool
extends VisualShaderNodeCustom

func _get_name():
    return "DoubleIt"

func _get_description():
    return "Multiplies its input by a global constant"

func _get_category():
    return "MyNodes"

func _get_input_port_count():
    return 1

func _get_output_port_count():
    return 1

func _get_input_port_name(port):
    return "in"

func _get_output_port_name(port):
    return "out"

func _get_global_code(mode):
    return "const float K = 2.0;"

func _get_code(input_vars, output_vars, mode, type):
    return "%s = %s * K;" % [output_vars[0], input_vars[0]]
"#;

fn node() -> CustomShaderNode {
    CustomShaderNode::from_source(DOUBLE_IT).expect("the custom-node script instantiates")
}

/// 🔑 **THE ACCEPTANCE.** The generated source carries the script's global
/// constant *and* its body line, with the variable names the caller supplied.
#[test]
fn a_visual_shader_node_custom_code_emission() {
    let mut n = node();
    let shader = VisualShader::default();
    let base = shader.get_code();

    // The names the generator would mint for this node's ports. They are
    // deliberately distinctive: the assertions below check for THESE strings,
    // so an implementation that invented its own names fails.
    let inputs = vec!["n_in7p0".to_string()];
    let outputs = vec!["n_out7p0".to_string()];

    let global = n
        .global_code(VisualShaderMode::CanvasItem)
        .expect("_get_global_code contributes");
    let body = n
        .code(
            &inputs,
            &outputs,
            VisualShaderMode::CanvasItem,
            TYPE_FRAGMENT,
        )
        .expect("_get_code contributes");

    let source = gdscene::visual_shader_custom::splice_custom_code(&base, &[global], &[body]);

    assert!(
        source.contains("const float K = 2.0;"),
        "the global section must carry _get_global_code's text.\n{source}"
    );
    assert!(
        source.contains("n_out7p0 = n_in7p0 * K;"),
        "the body must carry _get_code's text WITH THE CALLER'S NAMES \
         substituted by the script's own `%` formatting.\n{source}"
    );

    // 🔑 Placement, not just presence. The global must sit outside the
    // function and the body inside it — a build that appended both to the end
    // would satisfy both `contains` assertions above and emit invalid GLSL.
    let k = source.find("const float K").expect("global present");
    let open = source.find("void fragment() {").expect("fragment present");
    let line = source.find("n_out7p0 =").expect("body present");
    assert!(
        k < open,
        "the global constant must be declared BEFORE fragment(), not inside it.\n{source}"
    );
    assert!(
        line > open,
        "the node's body must be INSIDE fragment().\n{source}"
    );

    // The base shader's own output assignment must survive the splice.
    assert!(
        source.contains("COLOR = COLOR;"),
        "splicing must not drop what the graph already generated.\n{source}"
    );
    assert!(
        source.starts_with("shader_type canvas_item;"),
        "the shader_type line stays first.\n{source}"
    );
}

/// ⚠️ The mode integer handed to the virtuals is Godot's `Shader.Mode`, not the
/// Rust discriminant.
///
/// 🔑 Patina's `VisualShaderMode` lists `CanvasItem` **first**, so `as i64`
/// would give it 0 — but Godot's `MODE_SPATIAL` is 0 and `MODE_CANVAS_ITEM` is
/// 1. A script branching on `mode == 0` would then take the spatial path while
/// generating a canvas_item shader, silently. This is the guard for that.
#[test]
fn b_mode_int_is_godots_numbering_not_the_rust_discriminant() {
    assert_eq!(shader_mode_int(VisualShaderMode::Spatial), 0);
    assert_eq!(shader_mode_int(VisualShaderMode::CanvasItem), 1);

    // And the script actually receives it. This reads the value back out
    // through `_get_global_code`, which is the only channel a script has.
    let src = r#"extends VisualShaderNodeCustom

func _get_global_code(mode):
    return "// mode=%d" % [mode]
"#;
    let mut n = CustomShaderNode::from_source(src).expect("instantiates");
    let spatial = n
        .global_code(VisualShaderMode::Spatial)
        .expect("contributes");
    assert!(
        spatial.contains("mode=0"),
        "Spatial must reach the script as Godot's MODE_SPATIAL = 0, got {spatial:?}"
    );
    let canvas = n
        .global_code(VisualShaderMode::CanvasItem)
        .expect("contributes");
    assert!(
        canvas.contains("mode=1"),
        "CanvasItem must reach the script as MODE_CANVAS_ITEM = 1, got {canvas:?}"
    );
}

/// `_is_available(mode, type)` — arity 2, defaulting to available.
///
/// ⚠️ There is no available-node list in Patina to exclude the node *from*, so
/// this asserts the virtual's own answer. The exclusion wiring is remaining
/// scope on the bead, not something this test can stand in for.
#[test]
fn c_is_available_is_honoured_and_defaults_to_true() {
    // Not overridden -> available, as in Godot.
    let mut plain = node();
    assert!(
        plain.is_available(VisualShaderMode::CanvasItem, TYPE_FRAGMENT),
        "a script that does not override _is_available is available everywhere"
    );

    let src = r#"extends VisualShaderNodeCustom

func _is_available(mode, type):
    return false
"#;
    let mut hidden = CustomShaderNode::from_source(src).expect("instantiates");
    assert!(
        !hidden.is_available(VisualShaderMode::CanvasItem, TYPE_FRAGMENT),
        "_is_available returning false must be honoured"
    );

    // 🔑 It receives both arguments, so a node can be available in one mode and
    // not another. A 0-arity implementation would ignore this entirely.
    let src = r#"extends VisualShaderNodeCustom

func _is_available(mode, type):
    return mode == 0
"#;
    let mut spatial_only = CustomShaderNode::from_source(src).expect("instantiates");
    assert!(
        spatial_only.is_available(VisualShaderMode::Spatial, TYPE_FRAGMENT),
        "available in spatial"
    );
    assert!(
        !spatial_only.is_available(VisualShaderMode::CanvasItem, TYPE_FRAGMENT),
        "…and not in canvas_item — proving both parameters arrive"
    );
}

/// Ports and captions come from the script.
#[test]
fn d_port_introspection_and_caption() {
    let mut n = node();
    assert_eq!(n.caption(), "DoubleIt");
    assert_eq!(n.description(), "Multiplies its input by a global constant");
    assert_eq!(n.category(), "MyNodes");
    assert_eq!(n.input_port_count(), 1);
    assert_eq!(n.output_port_count(), 1);
    assert_eq!(n.input_port_name(0), "in");
    assert_eq!(n.output_port_name(0), "out");

    // A script overriding nothing: Godot's documented fallbacks, not a panic.
    let mut bare = CustomShaderNode::from_source("extends VisualShaderNodeCustom\n")
        .expect("an empty custom node still instantiates");
    assert_eq!(
        bare.caption(),
        "Unnamed",
        "get_caption() falls back to \"Unnamed\""
    );
    assert_eq!(bare.input_port_count(), 0);
    assert_eq!(bare.output_port_count(), 0);
    assert!(
        bare.global_code(VisualShaderMode::CanvasItem).is_none(),
        "a node contributing no global code must emit NOTHING — not an empty \
         `// Unnamed` comment header"
    );
    assert!(
        bare.code(&[], &[], VisualShaderMode::CanvasItem, TYPE_FRAGMENT)
            .is_none(),
        "and no body"
    );
}

/// `_is_valid_code`: empty and the literal `"null"` contribute nothing.
///
/// ⚠️ The `"null"` case is not hypothetical — it is what a GDScript function
/// that falls off the end marshals to when stringified, and Godot rejects it
/// explicitly. Without the check the word `null` lands in the shader source.
#[test]
fn e_invalid_code_contributes_nothing() {
    for body in ["\"\"", "\"null\""] {
        let src = format!(
            "extends VisualShaderNodeCustom\n\nfunc _get_global_code(mode):\n    return {body}\n"
        );
        let mut n = CustomShaderNode::from_source(&src).expect("instantiates");
        assert!(
            n.global_code(VisualShaderMode::CanvasItem).is_none(),
            "{body} must be rejected by _is_valid_code"
        );
    }
}

/// A splice with nothing to add returns the source byte-for-byte.
///
/// ⚠️ The control that stops the splice from corrupting a graph with no custom
/// nodes at all — by far the common case.
#[test]
fn f_splice_is_identity_when_there_is_nothing_to_add() {
    let base = VisualShader::default().get_code();
    assert_eq!(
        gdscene::visual_shader_custom::splice_custom_code(&base, &[], &[]),
        base,
        "no custom nodes must leave the generated source untouched"
    );

    // And a source missing the anchors is returned unchanged rather than
    // half-spliced — a splice that cannot find its anchors must do nothing.
    let junk = "not a shader at all";
    assert_eq!(
        gdscene::visual_shader_custom::splice_custom_code(
            junk,
            &["x".to_string()],
            &["y".to_string()]
        ),
        junk,
        "an unrecognisable base must not be mangled"
    );
}

/// Two custom nodes each get their own block, so their locals cannot collide.
#[test]
fn g_each_node_body_is_wrapped_in_its_own_block() {
    let src = r#"extends VisualShaderNodeCustom

func _get_code(input_vars, output_vars, mode, type):
    return "float t = 1.0;"
"#;
    let mut a = CustomShaderNode::from_source(src).expect("instantiates");
    let mut b = CustomShaderNode::from_source(src).expect("instantiates");
    let ba = a
        .code(&[], &[], VisualShaderMode::CanvasItem, TYPE_FRAGMENT)
        .expect("contributes");
    let bb = b
        .code(&[], &[], VisualShaderMode::CanvasItem, TYPE_FRAGMENT)
        .expect("contributes");

    assert_eq!(
        ba, "\t{\n\t\tfloat t = 1.0;\n\t}\n",
        "Godot's block wrapping"
    );

    let source = gdscene::visual_shader_custom::splice_custom_code(
        &VisualShader::default().get_code(),
        &[],
        &[ba, bb],
    );
    assert_eq!(
        source.matches("float t = 1.0;").count(),
        2,
        "both nodes contribute"
    );
    assert_eq!(
        source.matches('{').count(),
        // `void fragment() {` plus one block per node.
        3,
        "each body is braced separately, so two nodes declaring `t` do not \
         collide.\n{source}"
    );
}

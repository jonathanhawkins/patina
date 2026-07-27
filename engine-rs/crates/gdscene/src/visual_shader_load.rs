//! Loading a Godot `VisualShader` `.tres` into a compilable graph
//! (pat-0q2kx.226).
//!
//! Godot ground truth: `scene/resources/visual_shader.cpp`. A `VisualShader`
//! resource serialises as a `Shader` subclass whose properties are
//! `nodes/<function>/<id>/node = SubResource(...)`, `.../position`, and
//! `nodes/<function>/connections = PackedInt32Array(from, from_port, to,
//! to_port, …)`.
//!
//! ⚠️ **This lives in `gdscene` because nothing else can see both halves.**
//! `gdresource` (which parses `.tres`) and `gdserver2d` (which owns
//! `VisualShader`) are siblings — neither depends on the other — so the
//! conversion has to sit in a crate downstream of both. `gdscene` is also where
//! `ShaderMaterial` is consumed, so it is the natural seam rather than a
//! workaround.
//!
//! The `.tres` **parsing** was never the gap: `TresLoader` already reads any
//! resource generically into `Resource { class_name, properties, subresources }`.
//! What did not exist was anything that turned that generic bag into a graph —
//! `grep VisualShader crates/gdresource` was zero. This is that step.

use gdresource::resource::Resource;
use gdserver2d::material::{
    VisualShader, VisualShaderConnection, VisualShaderMode, VisualShaderNode, VisualShaderNodeType,
};
use gdvariant::Variant;

/// Godot's `VisualShaderNodeVectorOp.Operator` / `VisualShaderNodeFloatOp`:
/// ADD 0, SUB 1, MUL 2, DIV 3, … Only the operators Patina's codegen can emit
/// are mapped; anything else falls back to multiply, which is the identity-ish
/// choice for a tint graph and is reported rather than silently mis-emitted.
fn op_kind(operator: i64) -> VisualShaderNodeType {
    match operator {
        0 => VisualShaderNodeType::Add,
        _ => VisualShaderNodeType::Multiply,
    }
}

/// Maps a `VisualShaderNode*` class name onto the graph's node kind.
///
/// ⚠️ Returns `None` for classes the codegen has no expression for, so an
/// unsupported node becomes a **missing** node rather than a silently wrong one.
/// The graph still compiles; the port falls back to its literal.
fn node_kind(class_name: &str, sub: &Resource) -> Option<VisualShaderNodeType> {
    match class_name {
        "VisualShaderNodeColorConstant" => Some(VisualShaderNodeType::Color),
        "VisualShaderNodeVec3Constant" | "VisualShaderNodeVec4Constant" => {
            Some(VisualShaderNodeType::Vec3)
        }
        "VisualShaderNodeFloatConstant" => Some(VisualShaderNodeType::Float),
        "VisualShaderNodeTexture" => Some(VisualShaderNodeType::Texture),
        "VisualShaderNodeMix" => Some(VisualShaderNodeType::Mix),
        "VisualShaderNodeVectorOp" | "VisualShaderNodeFloatOp" | "VisualShaderNodeIntOp" => {
            Some(op_kind(match sub.get_property("operator") {
                Some(Variant::Int(i)) => *i,
                // Godot omits a property at its default; VectorOp defaults to
                // OP_ADD (0), so an absent `operator` is ADD, not multiply.
                _ => 0,
            }))
        }
        // `VisualShaderNodeInput` is one class for every built-in; the
        // `input_name` picks which. Only `uv` has an expression here.
        "VisualShaderNodeInput" => match sub.get_property("input_name") {
            Some(Variant::String(s)) if s == "uv" => Some(VisualShaderNodeType::Uv),
            _ => None,
        },
        _ => None,
    }
}

/// Reads a `VisualShader` graph out of a parsed `.tres` [`Resource`].
///
/// Returns `None` when the resource is not a `VisualShader`, so a caller can
/// fall through to the plain-`Shader` path without inspecting class names
/// itself.
pub fn visual_shader_from_resource(res: &Resource) -> Option<VisualShader> {
    if res.class_name != "VisualShader" {
        return None;
    }
    let mut shader = VisualShader::new();

    // Godot's Shader.Mode: SPATIAL 0, CANVAS_ITEM 1, PARTICLES 2, SKY 3, FOG 4.
    // ⚠️ Absent means 0 = SPATIAL, because Godot omits a property at its
    // default — so a `.tres` with no `mode` line is a *spatial* shader. Reading
    // the absence as canvas_item (Patina's own struct default) would silently
    // compile every default-mode graph to the wrong stage.
    shader.mode = match res.get_property("mode") {
        Some(Variant::Int(1)) => VisualShaderMode::CanvasItem,
        _ => VisualShaderMode::Spatial,
    };

    // -- nodes --------------------------------------------------------------
    // Property keys look like `nodes/fragment/2/node`. The id is the segment
    // before the trailing `/node`.
    for (key, value) in res.properties() {
        let Some(rest) = key.strip_prefix("nodes/") else {
            continue;
        };
        let Some(id_part) = rest.strip_suffix("/node") else {
            continue;
        };
        let Some((_func, id_str)) = id_part.rsplit_once('/') else {
            continue;
        };
        let Ok(id) = id_str.parse::<u32>() else {
            continue;
        };
        // The value is a `SubResource("X")` reference; the loader keeps the sub
        // in `subresources` under that id.
        let Some(sub_id) = sub_resource_id(value) else {
            continue;
        };
        let Some(sub) = res.subresources.get(&sub_id) else {
            continue;
        };
        let Some(kind) = node_kind(&sub.class_name, sub) else {
            continue;
        };

        // A constant carries its value in a class-specific property; the port
        // literal is what the codegen reads.
        let literal = match sub.class_name.as_str() {
            "VisualShaderNodeColorConstant"
            | "VisualShaderNodeVec3Constant"
            | "VisualShaderNodeVec4Constant"
            | "VisualShaderNodeFloatConstant" => sub.get_property("constant").cloned(),
            _ => None,
        };
        let inputs = match literal {
            Some(v) => vec![v],
            None => vec![Variant::Nil, Variant::Nil, Variant::Nil],
        };
        shader.add_node(VisualShaderNode {
            id,
            node_type: kind,
            inputs,
            output_count: 1,
        });
    }

    // ⚠️ Godot does NOT serialise the output node — it is implicit at id 0 and
    // every graph has one. Without synthesising it here `get_code()` finds no
    // Output and emits the `ALBEDO = ALBEDO;` passthrough, i.e. a graph that
    // loads perfectly and renders nothing.
    if !shader.nodes.iter().any(|n| n.id == 0) {
        shader.add_node(VisualShaderNode {
            id: 0,
            node_type: VisualShaderNodeType::Output,
            inputs: vec![Variant::Nil, Variant::Nil, Variant::Nil],
            output_count: 0,
        });
    }

    // -- connections --------------------------------------------------------
    for (key, value) in res.properties() {
        if !key.starts_with("nodes/") || !key.ends_with("/connections") {
            continue;
        }
        let Variant::PackedInt32Array(quads) = value else {
            continue;
        };
        // Flat quads: from_node, from_port, to_node, to_port.
        for q in quads.chunks_exact(4) {
            shader.connections.push(VisualShaderConnection {
                from_node: q[0].max(0) as u32,
                from_port: q[1].max(0) as u32,
                to_node: q[2].max(0) as u32,
                to_port: q[3].max(0) as u32,
            });
        }
    }

    Some(shader)
}

/// Extracts the id from a `SubResource("X")` reference.
///
/// ⚠️ The `.tres` parser does **not** keep the literal call text. It rewrites
/// `SubResource("Color_1")` to the string `"SubResource:Color_1"`
/// (`loader.rs:444`), and that colon form is what actually arrives here.
/// Matching the call syntax instead silently finds no nodes at all: every graph
/// would load as nothing but its synthesised output and compile to a
/// passthrough — a failure that looks like "the loader does nothing" rather
/// than "one string format is wrong". The call form is still accepted so a
/// hand-built `Resource` behaves the same.
fn sub_resource_id(value: &Variant) -> Option<String> {
    let s = match value {
        Variant::String(s) => s.as_str(),
        _ => return None,
    };
    if let Some(id) = s.strip_prefix("SubResource:") {
        return Some(id.to_string());
    }
    let inner = s.strip_prefix("SubResource(")?.strip_suffix(')')?;
    Some(inner.trim_matches('"').to_string())
}

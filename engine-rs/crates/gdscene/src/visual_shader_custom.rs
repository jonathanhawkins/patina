//! Script-defined visual-shader nodes — `extends VisualShaderNodeCustom`
//! (pat-s3rrv.232).
//!
//! In Godot, `@tool extends VisualShaderNodeCustom` is the supported way to add
//! a node to the visual-shader graph from GDScript. The script describes its
//! ports and returns the shader text it contributes, and the generator splices
//! that text into the source it builds for the graph.
//!
//! # ⚠️ Why this lives in `gdscene` and not where the bead says
//!
//! The bead names `gdrender3d/src/shader.rs`. That module is unrelated: the
//! visual-shader graph→source generator is `VisualShader::get_code()` in
//! `gdserver2d/src/material.rs`.
//!
//! But `gdserver2d` depends only on `gdcore`/`gdvariant`/`gdresource` — it sits
//! **below** `gdscript-interop`, which depends on *it*. So calling a script
//! virtual from `get_code()` would be a dependency cycle, and would need an
//! inversion registry to break.
//!
//! `gdscene` depends on **both**, so no inversion is needed. That is the same
//! reason `visual_shader_load.rs` lives here, and this module follows
//! [`crate::script_mesh`], which already dispatches a script-overridden engine
//! virtual (`_create_mesh_array`) on a non-node object from exactly this layer.
//!
//! # ⚠️ The mode integer is NOT the Rust discriminant
//!
//! Godot's `Shader.Mode` is `MODE_SPATIAL = 0`, `MODE_CANVAS_ITEM = 1`.
//! Patina's [`VisualShaderMode`] lists `CanvasItem` **first**, so its derived
//! discriminant is the opposite of Godot's numbering. Passing the discriminant
//! through would hand a script `0` for canvas_item where Godot means `1`, and a
//! `if mode == MODE_SPATIAL` test inside `_get_code` would silently take the
//! wrong branch — with no error anywhere. [`shader_mode_int`] is the explicit
//! map that stops that.

use gdscript_interop::interpreter::{ClassInstance, Interpreter};
use gdserver2d::material::VisualShaderMode;
use gdvariant::Variant;

/// Godot `Shader.Mode`, transcribed from `doc/classes/Shader.xml`.
///
/// ⚠️ See the module note: this is deliberately not `mode as i64`.
pub fn shader_mode_int(mode: VisualShaderMode) -> i64 {
    match mode {
        VisualShaderMode::Spatial => 0,
        VisualShaderMode::CanvasItem => 1,
    }
}

/// Godot `VisualShader.Type.TYPE_FRAGMENT`, from `doc/classes/VisualShader.xml`.
///
/// The generator emits only a `fragment()` function, so this is the only stage
/// a custom node is currently asked to contribute to.
pub const TYPE_FRAGMENT: i64 = 1;

/// Godot's fallback caption when a script does not override `_get_name`.
const UNNAMED: &str = "Unnamed";

/// A live `VisualShaderNodeCustom` script.
///
/// Holds the interpreter alongside the instance because every virtual is a
/// separate call on the *same* instance — a script may set up state in one
/// (`_get_input_port_count`) that another reads.
pub struct CustomShaderNode {
    interp: Interpreter,
    instance: ClassInstance,
}

impl CustomShaderNode {
    /// Loads and instantiates a script from disk.
    ///
    /// `None` on every failure path — a missing file, a script that does not
    /// parse, or one that does not instantiate. Mirrors
    /// [`crate::script_mesh::mesh_from_script`].
    pub fn from_script(script_path: &str) -> Option<Self> {
        let source = gdcore::file_access::FileAccess::read_file_string(script_path).ok()?;
        Self::from_source(&source)
    }

    /// Instantiates from source text, for callers that already have it.
    pub fn from_source(source: &str) -> Option<Self> {
        let mut interp = Interpreter::new();
        let class_def = interp.run_class(source).ok()?;
        let instance = interp.instantiate_class(&class_def).ok()?;
        Some(Self { interp, instance })
    }

    /// Whether the script overrides a virtual.
    ///
    /// ⚠️ Godot guards every one of these with `GDVIRTUAL_IS_OVERRIDDEN` and
    /// treats "not overridden" as *not an error* — most of the 21 virtuals are
    /// optional. Distinguishing that from a script that raised is why this
    /// checks the class definition rather than calling and swallowing the error.
    pub fn overrides(&self, virtual_name: &str) -> bool {
        self.instance.class_def.methods.contains_key(virtual_name)
    }

    fn call(&mut self, name: &str, args: &[Variant]) -> Option<Variant> {
        if !self.overrides(name) {
            return None;
        }
        self.interp
            .call_instance_method(&mut self.instance, name, args)
            .ok()
    }

    fn call_int(&mut self, name: &str, args: &[Variant]) -> Option<i64> {
        match self.call(name, args)? {
            Variant::Int(v) => Some(v),
            Variant::Float(v) => Some(v as i64),
            _ => None,
        }
    }

    fn call_string(&mut self, name: &str, args: &[Variant]) -> Option<String> {
        match self.call(name, args)? {
            Variant::String(s) => Some(s),
            Variant::StringName(s) => Some(s.as_str().to_string()),
            _ => None,
        }
    }

    /// `_get_name`, defaulting to Godot's `"Unnamed"` caption.
    pub fn caption(&mut self) -> String {
        self.call_string("_get_name", &[])
            .unwrap_or_else(|| UNNAMED.to_string())
    }

    /// `_get_description`.
    pub fn description(&mut self) -> String {
        self.call_string("_get_description", &[])
            .unwrap_or_default()
    }

    /// `_get_category`.
    pub fn category(&mut self) -> String {
        self.call_string("_get_category", &[]).unwrap_or_default()
    }

    /// `_get_input_port_count`. A script that does not override it has no
    /// inputs, which is what Godot's zero-initialised counter yields.
    pub fn input_port_count(&mut self) -> i64 {
        self.call_int("_get_input_port_count", &[]).unwrap_or(0)
    }

    /// `_get_output_port_count`.
    pub fn output_port_count(&mut self) -> i64 {
        self.call_int("_get_output_port_count", &[]).unwrap_or(0)
    }

    /// `_get_input_port_name(port)`.
    pub fn input_port_name(&mut self, port: i64) -> String {
        self.call_string("_get_input_port_name", &[Variant::Int(port)])
            .unwrap_or_default()
    }

    /// `_get_output_port_name(port)`.
    pub fn output_port_name(&mut self, port: i64) -> String {
        self.call_string("_get_output_port_name", &[Variant::Int(port)])
            .unwrap_or_default()
    }

    /// `_get_input_port_type(port)` / `_get_output_port_type(port)` —
    /// `VisualShaderNode.PortType`.
    pub fn input_port_type(&mut self, port: i64) -> i64 {
        self.call_int("_get_input_port_type", &[Variant::Int(port)])
            .unwrap_or(0)
    }

    /// See [`Self::input_port_type`].
    pub fn output_port_type(&mut self, port: i64) -> i64 {
        self.call_int("_get_output_port_type", &[Variant::Int(port)])
            .unwrap_or(0)
    }

    /// `_is_highend`.
    pub fn is_highend(&mut self) -> bool {
        matches!(self.call("_is_highend", &[]), Some(Variant::Bool(true)))
    }

    /// `_is_available(mode, type)`.
    ///
    /// ⚠️ **Arity 2, not 0.** The bead describes this as a bare predicate; the
    /// doc XML gives it `mode` and `type`, and a node may legitimately be
    /// available in one shader mode and not another.
    ///
    /// Defaults to **true**: Godot only hides a node when the script actively
    /// says so, and a script that does not override this is available
    /// everywhere.
    pub fn is_available(&mut self, mode: VisualShaderMode, shader_type: i64) -> bool {
        match self.call(
            "_is_available",
            &[
                Variant::Int(shader_mode_int(mode)),
                Variant::Int(shader_type),
            ],
        ) {
            Some(Variant::Bool(v)) => v,
            _ => true,
        }
    }

    /// `_get_global_code(mode)`, formatted the way Godot's
    /// `generate_global_per_node` formats it.
    ///
    /// `None` when the script does not override it or returns invalid code, so
    /// the caller emits nothing rather than a stray comment header.
    pub fn global_code(&mut self, mode: VisualShaderMode) -> Option<String> {
        let code = self.call_string("_get_global_code", &[Variant::Int(shader_mode_int(mode))])?;
        if !is_valid_code(&code) {
            return None;
        }
        let caption = self.caption();
        Some(format!("// {caption}\n{code}\n"))
    }

    /// `_get_func_code(mode, type)`.
    pub fn func_code(&mut self, mode: VisualShaderMode, shader_type: i64) -> Option<String> {
        let code = self.call_string(
            "_get_func_code",
            &[
                Variant::Int(shader_mode_int(mode)),
                Variant::Int(shader_type),
            ],
        )?;
        is_valid_code(&code).then_some(code)
    }

    /// `_get_code(input_vars, output_vars, mode, type)`, wrapped in a block the
    /// way Godot's `generate_code` wraps it.
    ///
    /// 🔑 **The script performs the `%s` substitution, not us.** Godot hands the
    /// virtual the real generated variable names as two `String` arrays and
    /// splices the returned text verbatim; a script writes
    /// `return "%s = %s * K;" % [output_vars[0], input_vars[0]]`. So the names
    /// passed here are the ones that must appear in the emitted source.
    pub fn code(
        &mut self,
        input_vars: &[String],
        output_vars: &[String],
        mode: VisualShaderMode,
        shader_type: i64,
    ) -> Option<String> {
        let to_array = |v: &[String]| {
            Variant::Array((v.iter().cloned().map(Variant::String).collect::<Vec<_>>()).into())
        };
        let code = self.call_string(
            "_get_code",
            &[
                to_array(input_vars),
                to_array(output_vars),
                Variant::Int(shader_mode_int(mode)),
                Variant::Int(shader_type),
            ],
        )?;
        if !is_valid_code(&code) {
            return None;
        }
        Some(wrap_node_code(&code))
    }
}

/// Godot `VisualShaderNodeCustom::_is_valid_code`.
///
/// ⚠️ The literal string `"null"` is rejected as well as the empty string —
/// that is what a GDScript function returning nothing marshals to, so without
/// this a script with no `return` would emit the word `null` into the shader.
fn is_valid_code(code: &str) -> bool {
    !code.is_empty() && code != "null"
}

/// Wraps a node's contributed code in its own block, as Godot does:
/// one tab, `{`, the body indented by two tabs, then `}`.
///
/// The block is what keeps two custom nodes' local declarations from colliding.
fn wrap_node_code(code: &str) -> String {
    // Mirrors `generate_code` in `visual_shader.cpp` step for step.
    let ends_with_newline = code.ends_with('\n');
    let indented = format!("\t\t{}", code.replace('\n', "\n\t\t"));
    let mut out = format!("\t{{\n{indented}");
    if ends_with_newline {
        // `indented` now ends with "\n\t\t" because the body's own trailing
        // newline was indented too. Godot removes exactly ONE character here,
        // leaving "\n\t" for the brace to land after.
        //
        // ⚠️ Not a trim: stripping *all* trailing tabs would also eat tabs a
        // body legitimately ended with, and the two only differ on input a
        // test is unlikely to try.
        out.pop();
        out.push('}');
    } else {
        out.push_str("\n\t}");
    }
    out.push('\n');
    out
}

/// Splices custom-node contributions into a source produced by
/// `VisualShader::get_code()`.
///
/// `globals` go after the `shader_type` line; `bodies` go at the top of
/// `fragment()`, before the generated assignment, so a body may define locals
/// the assignment then reads.
///
/// ⚠️ Anchored on the two markers `get_code()` itself emits, and returns the
/// source **unchanged** if either is missing rather than guessing an offset —
/// a splice whose anchors are not found is how text edits silently corrupt.
pub fn splice_custom_code(base: &str, globals: &[String], bodies: &[String]) -> String {
    if globals.is_empty() && bodies.is_empty() {
        return base.to_string();
    }
    let Some((header, rest)) = base.split_once(";\n") else {
        return base.to_string();
    };
    let Some((fn_open, fn_body)) = rest.split_once("{\n") else {
        return base.to_string();
    };

    let mut out = String::with_capacity(base.len() + 256);
    out.push_str(header);
    out.push_str(";\n");
    if !globals.is_empty() {
        out.push('\n');
        for g in globals {
            out.push_str(g);
        }
    }
    out.push_str(fn_open);
    out.push_str("{\n");
    for b in bodies {
        out.push_str(b);
    }
    out.push_str(fn_body);
    out
}

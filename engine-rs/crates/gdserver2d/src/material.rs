//! Material definitions for 2D and 3D rendering.
//!
//! Provides `Material3D` / `StandardMaterial3D` for PBR surface appearance,
//! `CanvasItemMaterial` for 2D blend/light modes, and `VisualShader` for
//! node-graph-based shader authoring.

use gdcore::math::Color;
use gdvariant::variant::Variant;

// ---------------------------------------------------------------------------
// Material3D (original, kept for backwards compat)
// ---------------------------------------------------------------------------

/// A PBR-style surface material for 3D rendering.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material3D {
    /// Base surface color.
    pub albedo_color: Color,
    /// Metallic factor in [0, 1].
    pub metallic: f32,
    /// Roughness factor in [0, 1].
    pub roughness: f32,
    /// Emission color.
    pub emission: Color,
    /// Emission energy multiplier.
    pub emission_energy: f32,
}

impl Default for Material3D {
    fn default() -> Self {
        Self {
            albedo_color: Color::WHITE,
            metallic: 0.0,
            roughness: 0.5,
            emission: Color::BLACK,
            emission_energy: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// StandardMaterial3D (expanded PBR)
// ---------------------------------------------------------------------------

/// Transparency mode for a 3D material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TransparencyMode {
    /// Fully opaque.
    #[default]
    Disabled,
    /// Alpha blending.
    Alpha,
    /// Alpha-scissor (binary transparency at a threshold).
    AlphaScissor,
}

/// Face-culling mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CullMode {
    /// Cull back faces (default).
    #[default]
    Back,
    /// Cull front faces.
    Front,
    /// No culling (double-sided).
    Disabled,
}

/// An expanded PBR material matching Godot's `StandardMaterial3D`.
#[derive(Debug, Clone, PartialEq)]
pub struct StandardMaterial3D {
    /// Base surface color.
    pub albedo_color: Color,
    /// Metallic factor in [0, 1].
    pub metallic: f32,
    /// Roughness factor in [0, 1].
    pub roughness: f32,
    /// Emission color.
    pub emission_color: Color,
    /// Emission energy multiplier.
    pub emission_energy: f32,
    /// Optional path to a normal-map texture.
    pub normal_map_path: Option<String>,
    /// Transparency mode.
    pub transparency: TransparencyMode,
    /// Face-culling mode.
    pub cull_mode: CullMode,
}

impl Default for StandardMaterial3D {
    fn default() -> Self {
        Self {
            albedo_color: Color::WHITE,
            metallic: 0.0,
            roughness: 0.5,
            emission_color: Color::BLACK,
            emission_energy: 0.0,
            normal_map_path: None,
            transparency: TransparencyMode::Disabled,
            cull_mode: CullMode::Back,
        }
    }
}

// ---------------------------------------------------------------------------
// CanvasItemMaterial (2D)
// ---------------------------------------------------------------------------

/// Blend mode for a 2D canvas material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BlendMode {
    /// Normal alpha blending.
    #[default]
    Mix,
    /// Additive blending.
    Add,
    /// Subtractive blending.
    Sub,
    /// Multiplicative blending.
    Mul,
    /// Premultiplied-alpha add: the source is already multiplied by its alpha,
    /// so it is added straight on top (`CanvasItemMaterial.BLEND_MODE_PREMULT_ALPHA`).
    PremultAlpha,
}

/// Godot `CanvasItem.TextureFilter`.
///
/// `ParentNode` is the default and is *not* a sampling mode — it means "use
/// whatever my ancestor resolved to", ultimately falling back to the viewport
/// default (`rendering/textures/canvas_textures/default_texture_filter`). The
/// scene bridge resolves it before the value reaches the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextureFilter {
    /// Inherit from the parent canvas item, then the viewport default.
    #[default]
    ParentNode,
    /// Nearest-neighbour (pixel-art).
    Nearest,
    /// Bilinear interpolation of the 4 nearest texels.
    Linear,
}

/// Godot `CanvasItem.TextureRepeat`. `ParentNode` inherits, exactly as
/// [`TextureFilter::ParentNode`] does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextureRepeat {
    /// Inherit from the parent canvas item, then the viewport default.
    #[default]
    ParentNode,
    /// Clamp UVs to `[0, 1]`.
    Disabled,
    /// Wrap (tile) UVs.
    Enabled,
    /// Mirror on each repeat.
    Mirror,
}

/// Light interaction mode for a 2D canvas material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LightMode {
    /// Normal lighting.
    #[default]
    Normal,
    /// Ignore lighting (unshaded).
    Unshaded,
    /// Only show where light hits.
    LightOnly,
}

/// A 2D material controlling blend and light behaviour on `CanvasItem` nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CanvasItemMaterial {
    /// How colors are blended with the background.
    pub blend_mode: BlendMode,
    /// How the item interacts with 2D lights.
    pub light_mode: LightMode,
}

// ---------------------------------------------------------------------------
// VisualShader (node-graph)
// ---------------------------------------------------------------------------

/// The type of a visual-shader graph node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VisualShaderNodeType {
    /// The final output node.
    Output,
    /// Scalar float constant.
    Float,
    /// 3-component vector constant.
    Vec3,
    /// Color constant.
    Color,
    /// Texture sampler node.
    Texture,
    /// Addition operator.
    Add,
    /// Multiplication operator.
    Multiply,
    /// Linear interpolation (mix).
    Mix,
}

/// A single node in a visual-shader graph.
#[derive(Debug, Clone, PartialEq)]
pub struct VisualShaderNode {
    /// Unique node identifier within its graph.
    pub id: u32,
    /// The operation this node performs.
    pub node_type: VisualShaderNodeType,
    /// Input port values (port index → Variant).
    pub inputs: Vec<Variant>,
    /// Number of output ports.
    pub output_count: u32,
}

/// A connection between two ports in a visual-shader graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VisualShaderConnection {
    /// Source node id.
    pub from_node: u32,
    /// Source output port index.
    pub from_port: u32,
    /// Destination node id.
    pub to_node: u32,
    /// Destination input port index.
    pub to_port: u32,
}

/// A node-graph-based shader.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VisualShader {
    /// The nodes in the graph.
    pub nodes: Vec<VisualShaderNode>,
    /// Port-to-port connections.
    pub connections: Vec<VisualShaderConnection>,
    /// Which shader stage the graph compiles to (pat-0q2kx.171).
    pub mode: VisualShaderMode,
}

/// The shader type a [`VisualShader`] compiles to — Godot's `Shader.Mode`
/// restricted to the two stages Patina's codegen emits (pat-0q2kx.171).
///
/// ⚠️ **`CanvasItem` is the default here, but Godot's `MODE_SPATIAL` is enum
/// zero and is what a fresh `VisualShader` resource uses.** Patina's codegen has
/// only ever emitted `shader_type canvas_item`, so defaulting to `Spatial` would
/// silently change the output of every existing graph. The divergence is
/// deliberate and belongs to whoever wires `.tres` round-tripping: a resource
/// that records its own mode should set it explicitly rather than rely on this
/// default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VisualShaderMode {
    /// 2D: `shader_type canvas_item`, writing `COLOR`.
    #[default]
    CanvasItem,
    /// 3D: `shader_type spatial`, writing `ALBEDO` — what a `MeshInstance`
    /// material needs.
    Spatial,
}

impl VisualShader {
    /// Create an empty visual shader graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a node and return its id.
    pub fn add_node(&mut self, node: VisualShaderNode) -> u32 {
        let id = node.id;
        self.nodes.push(node);
        id
    }

    /// Connect two ports.
    pub fn connect(&mut self, from_node: u32, from_port: u32, to_node: u32, to_port: u32) {
        self.connections.push(VisualShaderConnection {
            from_node,
            from_port,
            to_node,
            to_port,
        });
    }

    /// Find a node by id.
    pub fn get_node(&self, id: u32) -> Option<&VisualShaderNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Removes a node **and every connection touching it**, returning whether it
    /// existed (pat-0q2kx.171).
    ///
    /// Dropping the connections is the load-bearing part: a dangling
    /// `VisualShaderConnection` would make `resolve_input` chase a `from_node`
    /// that is gone, silently falling back to the default expression instead of
    /// reporting a broken graph.
    pub fn remove_node(&mut self, id: u32) -> bool {
        let before = self.nodes.len();
        self.nodes.retain(|n| n.id != id);
        if self.nodes.len() == before {
            return false;
        }
        self.connections
            .retain(|c| c.from_node != id && c.to_node != id);
        true
    }

    /// Removes one port-to-port connection, returning whether it existed.
    pub fn disconnect(
        &mut self,
        from_node: u32,
        from_port: u32,
        to_node: u32,
        to_port: u32,
    ) -> bool {
        let before = self.connections.len();
        self.connections.retain(|c| {
            !(c.from_node == from_node
                && c.from_port == from_port
                && c.to_node == to_node
                && c.to_port == to_port)
        });
        self.connections.len() != before
    }

    /// Sets a node's input-port literal — the inline parameter edit the graph
    /// canvas performs (pat-0q2kx.171).
    ///
    /// Returns false for an unknown node **or** an out-of-range port rather than
    /// growing `inputs`: a port index the node does not have is a client bug, and
    /// silently extending the vector would produce a node whose arity no longer
    /// matches its type.
    pub fn set_param(&mut self, id: u32, port: u32, value: Variant) -> bool {
        match self.nodes.iter_mut().find(|n| n.id == id) {
            Some(node) => match node.inputs.get_mut(port as usize) {
                Some(slot) => {
                    *slot = value;
                    true
                }
                None => false,
            },
            None => false,
        }
    }

    /// Compiles the graph into GDShader source (Godot's `VisualShader.get_code`,
    /// pat-nzgdu.140).
    ///
    /// Resolution starts at the [`Output`](VisualShaderNodeType::Output) node and
    /// walks *backwards* along the connections feeding it, so unreachable nodes
    /// contribute nothing — matching Godot, where only the subgraph reaching the
    /// output is emitted. An input port that has no incoming connection falls
    /// back to that node's own literal in [`inputs`](VisualShaderNode::inputs).
    ///
    /// The result is ordinary `shader_type canvas_item` source, so a VisualShader
    /// is usable anywhere a text [`Shader`](crate::shader::Shader) is — the whole
    /// point of the class. A graph with no output node (or an empty graph) yields
    /// a valid pass-through shader rather than invalid source.
    pub fn get_code(&self) -> String {
        let output = self
            .nodes
            .iter()
            .find(|n| n.node_type == VisualShaderNodeType::Output);
        match self.mode {
            VisualShaderMode::CanvasItem => {
                let body = match output {
                    // Godot's canvas_item output takes its colour on port 0.
                    Some(output) => format!("COLOR = {};", self.resolve_input(output, 0, 0)),
                    None => "COLOR = COLOR;".to_string(),
                };
                format!("shader_type canvas_item;\n\nvoid fragment() {{\n\t{body}\n}}\n")
            }
            VisualShaderMode::Spatial => {
                // `ALBEDO` is a `vec3`, while port expressions are `vec4` (colours
                // and the fallback) or `vec3`. `.rgb` is valid on both, so the
                // whole expression is parenthesised and swizzled rather than
                // guessed at from its text.
                //
                // A graph whose output port resolves to a bare scalar would emit
                // `(0.5).rgb`, which is invalid — but that is the existing
                // untyped-port limitation, not new here: the canvas_item branch
                // equally assigns a possibly-scalar expression to `COLOR`. Real
                // port typing is the fix, and it is out of scope for this bead.
                let body = match output {
                    Some(output) => {
                        format!("ALBEDO = ({}).rgb;", self.resolve_input(output, 0, 0))
                    }
                    None => "ALBEDO = ALBEDO;".to_string(),
                };
                format!("shader_type spatial;\n\nvoid fragment() {{\n\t{body}\n}}\n")
            }
        }
    }

    /// Resolves the expression feeding `node`'s input port `port`: the connected
    /// upstream node if there is one, else the port's literal value.
    fn resolve_input(&self, node: &VisualShaderNode, port: u32, depth: u32) -> String {
        // A hand-built (or round-tripped) graph can contain a cycle; bail rather
        // than recursing forever.
        if depth > MAX_GRAPH_DEPTH {
            return "vec4(0.0, 0.0, 0.0, 1.0)".to_string();
        }

        let upstream = self
            .connections
            .iter()
            .find(|c| c.to_node == node.id && c.to_port == port)
            .and_then(|c| self.get_node(c.from_node));

        match upstream {
            Some(src) => self.node_expr(src, depth + 1),
            None => literal_expr(node.inputs.get(port as usize)),
        }
    }

    /// The GDShader expression a node evaluates to.
    fn node_expr(&self, node: &VisualShaderNode, depth: u32) -> String {
        if depth > MAX_GRAPH_DEPTH {
            return "vec4(0.0, 0.0, 0.0, 1.0)".to_string();
        }
        match node.node_type {
            VisualShaderNodeType::Output => self.resolve_input(node, 0, depth),
            VisualShaderNodeType::Float
            | VisualShaderNodeType::Vec3
            | VisualShaderNodeType::Color => literal_expr(node.inputs.first()),
            VisualShaderNodeType::Texture => "texture(TEXTURE, UV)".to_string(),
            VisualShaderNodeType::Add => format!(
                "({} + {})",
                self.resolve_input(node, 0, depth),
                self.resolve_input(node, 1, depth)
            ),
            VisualShaderNodeType::Multiply => format!(
                "({} * {})",
                self.resolve_input(node, 0, depth),
                self.resolve_input(node, 1, depth)
            ),
            VisualShaderNodeType::Mix => format!(
                "mix({}, {}, {})",
                self.resolve_input(node, 0, depth),
                self.resolve_input(node, 1, depth),
                self.resolve_input(node, 2, depth)
            ),
        }
    }
}

/// Guards against a cyclic graph recursing forever during codegen.
const MAX_GRAPH_DEPTH: u32 = 64;

/// Renders a port literal as a GDShader expression.
///
/// An absent or unsupported value becomes opaque black, so codegen always
/// produces valid source rather than an empty expression.
fn literal_expr(value: Option<&Variant>) -> String {
    match value {
        Some(Variant::Color(c)) => format!(
            "vec4({}, {}, {}, {})",
            gdshader_f32(c.r),
            gdshader_f32(c.g),
            gdshader_f32(c.b),
            gdshader_f32(c.a)
        ),
        Some(Variant::Vector3(v)) => format!(
            "vec3({}, {}, {})",
            gdshader_f32(v.x),
            gdshader_f32(v.y),
            gdshader_f32(v.z)
        ),
        Some(Variant::Float(f)) => gdshader_f32(*f as f32),
        Some(Variant::Int(i)) => gdshader_f32(*i as f32),
        _ => "vec4(0.0, 0.0, 0.0, 1.0)".to_string(),
    }
}

/// Formats a float as a GDShader literal, always with a decimal point so it
/// lexes as a float rather than an int (`1` would be invalid where a float is
/// required).
fn gdshader_f32(v: f32) -> String {
    if v.is_finite() && v == v.trunc() && v.abs() < 1e9 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Material3D (original) -----------------------------------------------

    #[test]
    fn default_material_values() {
        let mat = Material3D::default();
        assert_eq!(mat.albedo_color, Color::WHITE);
        assert_eq!(mat.metallic, 0.0);
        assert_eq!(mat.roughness, 0.5);
        assert_eq!(mat.emission, Color::BLACK);
        assert_eq!(mat.emission_energy, 0.0);
    }

    #[test]
    fn material_custom_values() {
        let mat = Material3D {
            albedo_color: Color::rgb(1.0, 0.0, 0.0),
            metallic: 1.0,
            roughness: 0.1,
            emission: Color::rgb(0.0, 1.0, 0.0),
            emission_energy: 2.0,
        };
        assert_eq!(mat.metallic, 1.0);
        assert_eq!(mat.roughness, 0.1);
        assert_eq!(mat.emission_energy, 2.0);
    }

    #[test]
    fn material_clone() {
        let a = Material3D::default();
        let b = a;
        assert_eq!(a, b);
    }

    // -- StandardMaterial3D --------------------------------------------------

    #[test]
    fn standard_material_defaults() {
        let mat = StandardMaterial3D::default();
        assert_eq!(mat.albedo_color, Color::WHITE);
        assert_eq!(mat.transparency, TransparencyMode::Disabled);
        assert_eq!(mat.cull_mode, CullMode::Back);
        assert!(mat.normal_map_path.is_none());
    }

    #[test]
    fn standard_material_with_normal_map() {
        let mat = StandardMaterial3D {
            normal_map_path: Some("res://normal.png".to_string()),
            ..Default::default()
        };
        assert_eq!(mat.normal_map_path.as_deref(), Some("res://normal.png"));
    }

    #[test]
    fn standard_material_alpha_scissor() {
        let mat = StandardMaterial3D {
            transparency: TransparencyMode::AlphaScissor,
            cull_mode: CullMode::Disabled,
            ..Default::default()
        };
        assert_eq!(mat.transparency, TransparencyMode::AlphaScissor);
        assert_eq!(mat.cull_mode, CullMode::Disabled);
    }

    // -- CanvasItemMaterial ---------------------------------------------------

    #[test]
    fn canvas_material_defaults() {
        let mat = CanvasItemMaterial::default();
        assert_eq!(mat.blend_mode, BlendMode::Mix);
        assert_eq!(mat.light_mode, LightMode::Normal);
    }

    #[test]
    fn canvas_material_additive() {
        let mat = CanvasItemMaterial {
            blend_mode: BlendMode::Add,
            light_mode: LightMode::Unshaded,
        };
        assert_eq!(mat.blend_mode, BlendMode::Add);
        assert_eq!(mat.light_mode, LightMode::Unshaded);
    }

    // -- VisualShader ---------------------------------------------------------

    #[test]
    fn visual_shader_empty() {
        let vs = VisualShader::new();
        assert!(vs.nodes.is_empty());
        assert!(vs.connections.is_empty());
    }

    // -- pat-nzgdu.140: VisualShader.get_code() graph codegen -----------------

    fn node(id: u32, node_type: VisualShaderNodeType, inputs: Vec<Variant>) -> VisualShaderNode {
        VisualShaderNode {
            id,
            node_type,
            inputs,
            output_count: 1,
        }
    }

    fn output_node(id: u32) -> VisualShaderNode {
        VisualShaderNode {
            id,
            node_type: VisualShaderNodeType::Output,
            inputs: vec![Variant::Nil],
            output_count: 0,
        }
    }

    /// Runs generated source through the real software shader pipeline —
    /// the whole point of `get_code()` is that its output is ordinary GDShader.
    fn render(code: &str) -> Color {
        use crate::shader::{execute_fragment, parse_fragment_body, FragmentContext};
        use std::collections::HashMap;

        let instructions = parse_fragment_body(code);
        assert!(
            !instructions.is_empty(),
            "generated source did not parse as a fragment program:\n{code}",
        );
        execute_fragment(&instructions, &FragmentContext::default(), &HashMap::new())
    }

    #[test]
    fn get_code_compiles_a_color_constant_graph_and_renders_it() {
        // ColorConstant -> Output, the acceptance's shape.
        let mut vs = VisualShader::new();
        vs.add_node(node(
            0,
            VisualShaderNodeType::Color,
            vec![Variant::Color(Color::new(1.0, 0.5, 0.25, 1.0))],
        ));
        vs.add_node(output_node(1));
        vs.connect(0, 0, 1, 0);

        let code = vs.get_code();
        assert!(code.starts_with("shader_type canvas_item;"), "{code}");
        assert!(code.contains("void fragment()"), "{code}");
        assert!(
            code.contains("COLOR = vec4(1.0, 0.5, 0.25, 1.0);"),
            "{code}"
        );

        // The generated source really runs and yields the graph's colour.
        let out = render(&code);
        assert_eq!((out.r, out.g, out.b, out.a), (1.0, 0.5, 0.25, 1.0));
    }

    #[test]
    fn get_code_compiles_a_mix_graph_and_renders_the_blend() {
        // mix(red, blue, 0.5) -> Output.
        let mut vs = VisualShader::new();
        vs.add_node(node(
            0,
            VisualShaderNodeType::Color,
            vec![Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0))],
        ));
        vs.add_node(node(
            1,
            VisualShaderNodeType::Color,
            vec![Variant::Color(Color::new(0.0, 0.0, 1.0, 1.0))],
        ));
        vs.add_node(node(
            2,
            VisualShaderNodeType::Mix,
            // Port 2 (the weight) has no connection, so its literal is used.
            vec![Variant::Nil, Variant::Nil, Variant::Float(0.5)],
        ));
        vs.add_node(output_node(3));
        vs.connect(0, 0, 2, 0);
        vs.connect(1, 0, 2, 1);
        vs.connect(2, 0, 3, 0);

        let code = vs.get_code();
        assert!(
            code.contains("mix(vec4(1.0, 0.0, 0.0, 1.0), vec4(0.0, 0.0, 1.0, 1.0), 0.5)"),
            "{code}"
        );

        let out = render(&code);
        assert_eq!((out.r, out.b, out.a), (0.5, 0.5, 1.0));
    }

    #[test]
    fn get_code_walks_backwards_from_output_and_ignores_orphans() {
        let mut vs = VisualShader::new();
        vs.add_node(node(
            0,
            VisualShaderNodeType::Color,
            vec![Variant::Color(Color::new(0.0, 1.0, 0.0, 1.0))],
        ));
        // An unreachable node: connected to nothing, so it must not appear.
        vs.add_node(node(
            9,
            VisualShaderNodeType::Color,
            vec![Variant::Color(Color::new(0.123, 0.0, 0.0, 1.0))],
        ));
        vs.add_node(output_node(1));
        vs.connect(0, 0, 1, 0);

        let code = vs.get_code();
        assert!(code.contains("vec4(0.0, 1.0, 0.0, 1.0)"), "{code}");
        assert!(
            !code.contains("0.123"),
            "orphan node leaked into codegen:\n{code}"
        );
        assert_eq!(render(&code).g, 1.0);
    }

    #[test]
    fn get_code_uses_the_port_literal_when_nothing_is_connected() {
        // Output with an unconnected port falls back to its own literal.
        let mut vs = VisualShader::new();
        vs.add_node(VisualShaderNode {
            id: 0,
            node_type: VisualShaderNodeType::Output,
            inputs: vec![Variant::Color(Color::new(0.25, 0.25, 0.25, 1.0))],
            output_count: 0,
        });

        let code = vs.get_code();
        assert!(
            code.contains("COLOR = vec4(0.25, 0.25, 0.25, 1.0);"),
            "{code}"
        );
        assert_eq!(render(&code).r, 0.25);
    }

    #[test]
    fn get_code_on_an_empty_graph_is_still_valid_source() {
        // No output node: a pass-through shader, not invalid source.
        let code = VisualShader::new().get_code();
        assert!(code.starts_with("shader_type canvas_item;"), "{code}");
        assert!(code.contains("COLOR = COLOR;"), "{code}");
        // Parses and leaves the incoming colour untouched.
        let out = render(&code);
        assert_eq!(out, Color::WHITE, "pass-through keeps the default colour");
    }

    #[test]
    fn get_code_emits_float_and_vec3_literals_with_a_decimal_point() {
        // `1` would not lex as a float in GDShader; it must be `1.0`.
        let mut vs = VisualShader::new();
        vs.add_node(node(
            0,
            VisualShaderNodeType::Vec3,
            vec![Variant::Vector3(gdcore::math::Vector3::new(1.0, 0.0, 0.5))],
        ));
        vs.add_node(output_node(1));
        vs.connect(0, 0, 1, 0);

        let code = vs.get_code();
        assert!(code.contains("vec3(1.0, 0.0, 0.5)"), "{code}");
    }

    #[test]
    fn get_code_survives_a_cyclic_graph() {
        // A cycle must terminate rather than recurse forever.
        let mut vs = VisualShader::new();
        vs.add_node(node(
            0,
            VisualShaderNodeType::Add,
            vec![Variant::Nil, Variant::Nil],
        ));
        vs.add_node(output_node(1));
        vs.connect(0, 0, 1, 0);
        vs.connect(0, 0, 0, 0); // self-feeding input

        let code = vs.get_code();
        assert!(code.starts_with("shader_type canvas_item;"), "{code}");
    }

    #[test]
    fn visual_shader_add_node_and_connect() {
        let mut vs = VisualShader::new();
        let color_node = VisualShaderNode {
            id: 0,
            node_type: VisualShaderNodeType::Color,
            inputs: vec![],
            output_count: 1,
        };
        let output_node = VisualShaderNode {
            id: 1,
            node_type: VisualShaderNodeType::Output,
            inputs: vec![Variant::Nil],
            output_count: 0,
        };
        vs.add_node(color_node);
        vs.add_node(output_node);
        vs.connect(0, 0, 1, 0);

        assert_eq!(vs.nodes.len(), 2);
        assert_eq!(vs.connections.len(), 1);
        assert_eq!(vs.connections[0].from_node, 0);
        assert_eq!(vs.connections[0].to_node, 1);
    }

    #[test]
    fn visual_shader_get_node() {
        let mut vs = VisualShader::new();
        vs.add_node(VisualShaderNode {
            id: 5,
            node_type: VisualShaderNodeType::Float,
            inputs: vec![],
            output_count: 1,
        });
        assert!(vs.get_node(5).is_some());
        assert!(vs.get_node(99).is_none());
    }

    #[test]
    fn visual_shader_node_types() {
        // Ensure all node types can be compared
        assert_ne!(VisualShaderNodeType::Output, VisualShaderNodeType::Float);
        assert_ne!(VisualShaderNodeType::Add, VisualShaderNodeType::Multiply);
        assert_eq!(VisualShaderNodeType::Mix, VisualShaderNodeType::Mix);
    }

    // -- spatial shader mode (pat-0q2kx.171) --------------------------------
    //
    // `.171`'s acceptance path is "ColorConstant -> Output **albedo**" driving a
    // **MeshInstance**. `ALBEDO` is a spatial output; `get_code()` only ever
    // emitted `shader_type canvas_item` with `COLOR`, so that path was
    // unreachable however the editor routes were wired.

    /// The default must stay `canvas_item`, because every pre-existing caller and
    /// test depends on that output. (Godot's own enum-zero is `MODE_SPATIAL`, so
    /// this is a deliberate divergence — see `VisualShaderMode`.)
    #[test]
    fn default_mode_is_canvas_item_not_godot_enum_zero() {
        let vs = VisualShader::new();
        assert_eq!(vs.mode, VisualShaderMode::CanvasItem);
        assert!(
            vs.get_code().contains("shader_type canvas_item;"),
            "the default must keep emitting canvas_item"
        );
    }

    /// A spatial graph writes `ALBEDO` in a `spatial` shader.
    #[test]
    fn spatial_mode_emits_albedo_from_a_colour_node() {
        let mut vs = VisualShader::new();
        vs.mode = VisualShaderMode::Spatial;
        let color = vs.add_node(node(
            1,
            VisualShaderNodeType::Color,
            vec![Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0))],
        ));
        let out = vs.add_node(output_node(2));
        vs.connect(color, 0, out, 0);

        let code = vs.get_code();
        assert!(
            code.contains("shader_type spatial;"),
            "spatial mode must declare shader_type spatial, got:\n{code}"
        );
        assert!(
            code.contains("ALBEDO ="),
            "spatial mode must write ALBEDO, not COLOR, got:\n{code}"
        );
        assert!(
            !code.contains("COLOR ="),
            "spatial mode must NOT write COLOR, got:\n{code}"
        );
        // `ALBEDO` is a vec3, so the vec4 colour expression is swizzled.
        assert!(
            code.contains(".rgb"),
            "the vec4 expression must be narrowed to vec3 for ALBEDO, got:\n{code}"
        );
        assert!(
            code.contains("1.0") && code.contains("0.0"),
            "the node's colour literal must reach the source, got:\n{code}"
        );
    }

    /// With no Output node a spatial graph must still emit compilable source
    /// rather than an empty or half-written body.
    #[test]
    fn spatial_mode_without_an_output_is_still_valid_source() {
        let mut vs = VisualShader::new();
        vs.mode = VisualShaderMode::Spatial;
        let code = vs.get_code();
        assert!(code.contains("shader_type spatial;"));
        assert!(code.contains("ALBEDO = ALBEDO;"), "got:\n{code}");
        assert!(code.contains("void fragment()"));
    }

    /// Removing a node must take its connections with it, or `resolve_input`
    /// chases a `from_node` that no longer exists and silently yields the default
    /// expression instead of surfacing a broken graph.
    #[test]
    fn removing_a_node_drops_its_connections() {
        let mut vs = VisualShader::new();
        let color = vs.add_node(node(
            1,
            VisualShaderNodeType::Color,
            vec![Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0))],
        ));
        let out = vs.add_node(output_node(2));
        vs.connect(color, 0, out, 0);
        assert_eq!(vs.connections.len(), 1);

        assert!(vs.remove_node(color));
        assert!(vs.get_node(color).is_none());
        assert_eq!(
            vs.connections.len(),
            0,
            "the connection feeding the removed node must go with it"
        );
        assert!(!vs.remove_node(color), "already gone");

        // The graph still compiles, now falling back to the default expression.
        assert!(vs.get_code().contains("void fragment()"));
    }

    #[test]
    fn disconnect_removes_only_the_named_connection() {
        let mut vs = VisualShader::new();
        let a = vs.add_node(node(
            1,
            VisualShaderNodeType::Float,
            vec![Variant::Float(1.0)],
        ));
        let b = vs.add_node(node(
            2,
            VisualShaderNodeType::Float,
            vec![Variant::Float(2.0)],
        ));
        // A `Mix` node genuinely has several input ports, so feeding two of them
        // models a real graph. `output_node` has only port 0, and connecting to
        // its port 1 would describe a graph that cannot exist — the model does not
        // validate port ranges, so such a test would pass while lying.
        let mix = vs.add_node(node(
            3,
            VisualShaderNodeType::Mix,
            vec![Variant::Nil, Variant::Nil, Variant::Float(0.5)],
        ));
        vs.connect(a, 0, mix, 0);
        vs.connect(b, 0, mix, 1);

        assert!(vs.disconnect(a, 0, mix, 0));
        assert_eq!(vs.connections.len(), 1, "only one goes");
        assert_eq!(vs.connections[0].from_node, b, "the other survives");
        assert_eq!(vs.connections[0].to_port, 1, "on its own port");

        // A connection that does not exist must report false, not silently pass.
        assert!(!vs.disconnect(a, 0, mix, 0));
        assert!(!vs.disconnect(99, 0, mix, 0));
    }

    /// `set_param` edits an existing port and refuses unknown nodes/ports rather
    /// than growing `inputs`, which would leave a node whose arity no longer
    /// matches its type.
    #[test]
    fn set_param_edits_a_port_and_refuses_bad_targets() {
        let mut vs = VisualShader::new();
        let color = vs.add_node(node(
            1,
            VisualShaderNodeType::Color,
            vec![Variant::Color(Color::new(0.0, 0.0, 0.0, 1.0))],
        ));
        let out = vs.add_node(output_node(2));
        vs.connect(color, 0, out, 0);

        assert!(vs.set_param(color, 0, Variant::Color(Color::new(0.25, 0.5, 0.75, 1.0))));
        assert!(
            vs.get_code().contains("vec4(0.25, 0.5, 0.75, 1.0)"),
            "the edited literal must reach the generated source:\n{}",
            vs.get_code()
        );

        assert!(!vs.set_param(color, 7, Variant::Float(1.0)), "no such port");
        assert!(!vs.set_param(999, 0, Variant::Float(1.0)), "no such node");
        assert_eq!(
            vs.get_node(color).unwrap().inputs.len(),
            1,
            "a rejected set_param must not grow the input list"
        );
    }

    /// Switching mode must change only the stage/output, leaving the resolved
    /// expression identical — the graph itself is mode-independent.
    #[test]
    fn mode_switch_reuses_the_same_resolved_expression() {
        let mut vs = VisualShader::new();
        let color = vs.add_node(node(
            1,
            VisualShaderNodeType::Color,
            vec![Variant::Color(Color::new(0.25, 0.5, 0.75, 1.0))],
        ));
        let out = vs.add_node(output_node(2));
        vs.connect(color, 0, out, 0);

        let canvas = vs.get_code();
        vs.mode = VisualShaderMode::Spatial;
        let spatial = vs.get_code();

        // The same vec4 literal appears in both, just assigned differently.
        let literal = "vec4(0.25, 0.5, 0.75, 1.0)";
        assert!(canvas.contains(literal), "canvas_item:\n{canvas}");
        assert!(spatial.contains(literal), "spatial:\n{spatial}");
        assert_ne!(canvas, spatial, "the two stages must differ");
    }
}

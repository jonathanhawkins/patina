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
        let body = match self.nodes.iter().find(|n| n.node_type == VisualShaderNodeType::Output) {
            // Godot's canvas_item output takes its colour on port 0.
            Some(output) => format!("COLOR = {};", self.resolve_input(output, 0, 0)),
            None => "COLOR = COLOR;".to_string(),
        };
        format!("shader_type canvas_item;\n\nvoid fragment() {{\n\t{body}\n}}\n")
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
        execute_fragment(
            &instructions,
            &FragmentContext::default(),
            &HashMap::new(),
        )
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
        assert!(code.contains("COLOR = vec4(1.0, 0.5, 0.25, 1.0);"), "{code}");

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
        assert!(code.contains("mix(vec4(1.0, 0.0, 0.0, 1.0), vec4(0.0, 0.0, 1.0, 1.0), 0.5)"), "{code}");

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
        assert!(!code.contains("0.123"), "orphan node leaked into codegen:\n{code}");
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
        assert!(code.contains("COLOR = vec4(0.25, 0.25, 0.25, 1.0);"), "{code}");
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
        vs.add_node(node(0, VisualShaderNodeType::Add, vec![Variant::Nil, Variant::Nil]));
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
}

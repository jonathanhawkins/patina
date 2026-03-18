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

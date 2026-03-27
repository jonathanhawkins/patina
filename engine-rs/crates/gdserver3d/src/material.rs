//! 3D material types for the rendering server.
//!
//! Provides [`Material3D`] for basic PBR surfaces and [`StandardMaterial3D`]
//! for Godot-compatible materials with albedo, metallic, roughness, and
//! normal-map texture slots.

use gdcore::math::Color;
use gdvariant::Variant;

/// Shading mode for a 3D material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadingMode {
    /// Unlit — no lighting calculations.
    Unlit,
    /// Lambert diffuse shading.
    Lambert,
    /// Phong specular shading.
    Phong,
}

impl Default for ShadingMode {
    fn default() -> Self {
        Self::Lambert
    }
}

/// A 3D surface material.
#[derive(Debug, Clone, PartialEq)]
pub struct Material3D {
    /// Base albedo color.
    pub albedo: Color,
    /// Roughness factor (0.0 = mirror, 1.0 = fully rough).
    pub roughness: f32,
    /// Metallic factor (0.0 = dielectric, 1.0 = metallic).
    pub metallic: f32,
    /// Emission color (additive).
    pub emission: Color,
    /// Shading model.
    pub shading_mode: ShadingMode,
    /// Whether the material is double-sided.
    pub double_sided: bool,
}

impl Default for Material3D {
    fn default() -> Self {
        Self {
            albedo: Color::new(1.0, 1.0, 1.0, 1.0),
            roughness: 0.5,
            metallic: 0.0,
            emission: Color::new(0.0, 0.0, 0.0, 0.0),
            shading_mode: ShadingMode::Lambert,
            double_sided: false,
        }
    }
}

/// A texture channel reference — stores the resource path to a texture.
///
/// In Godot, texture properties are `Texture2D` sub-resources or external
/// resource references. Here we store the path string which can be resolved
/// through the resource cache at render time.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureSlot {
    /// Resource path (e.g. `"res://textures/albedo.png"`).
    pub path: String,
}

impl TextureSlot {
    /// Creates a new texture slot with the given path.
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

/// A Godot-compatible `StandardMaterial3D` with PBR texture slots.
///
/// Maps to Godot's `StandardMaterial3D` resource type with support for
/// albedo, metallic, roughness, and normal-map channels. Each channel has
/// a scalar/color value and an optional texture slot.
#[derive(Debug, Clone, PartialEq)]
pub struct StandardMaterial3D {
    /// Base albedo color.
    pub albedo_color: Color,
    /// Optional albedo texture.
    pub albedo_texture: Option<TextureSlot>,
    /// Metallic factor (0.0 = dielectric, 1.0 = metallic).
    pub metallic: f32,
    /// Optional metallic texture (sampled from the blue channel in Godot).
    pub metallic_texture: Option<TextureSlot>,
    /// Roughness factor (0.0 = mirror, 1.0 = fully rough).
    pub roughness: f32,
    /// Optional roughness texture (sampled from the green channel in Godot).
    pub roughness_texture: Option<TextureSlot>,
    /// Whether the normal map is enabled.
    pub normal_enabled: bool,
    /// Optional normal-map texture.
    pub normal_texture: Option<TextureSlot>,
    /// Normal map strength multiplier.
    pub normal_scale: f32,
    /// Emission color (additive).
    pub emission: Color,
    /// Shading model.
    pub shading_mode: ShadingMode,
    /// Whether the material is double-sided.
    pub double_sided: bool,
}

impl Default for StandardMaterial3D {
    fn default() -> Self {
        Self {
            albedo_color: Color::new(1.0, 1.0, 1.0, 1.0),
            albedo_texture: None,
            metallic: 0.0,
            metallic_texture: None,
            roughness: 1.0,
            roughness_texture: None,
            normal_enabled: false,
            normal_texture: None,
            normal_scale: 1.0,
            emission: Color::new(0.0, 0.0, 0.0, 0.0),
            shading_mode: ShadingMode::Lambert,
            double_sided: false,
        }
    }
}

impl StandardMaterial3D {
    /// Converts this standard material to a basic [`Material3D`] by
    /// discarding texture references.
    pub fn to_material3d(&self) -> Material3D {
        Material3D {
            albedo: self.albedo_color,
            roughness: self.roughness,
            metallic: self.metallic,
            emission: self.emission,
            shading_mode: self.shading_mode,
            double_sided: self.double_sided,
        }
    }

    /// Constructs a `StandardMaterial3D` from a [`gdresource::Resource`]
    /// property bag.
    ///
    /// Reads Godot-standard property names (`albedo_color`, `metallic`,
    /// `roughness`, `normal_enabled`, `normal_scale`, etc.) and maps
    /// texture references (stored as `"ExtResource:<id>"` or
    /// `"SubResource:<id>"` strings) into [`TextureSlot`] values.
    pub fn from_properties<'a>(
        properties: impl Iterator<Item = (&'a String, &'a Variant)>,
    ) -> Self {
        let mut mat = Self::default();
        for (key, value) in properties {
            match key.as_str() {
                "albedo_color" => {
                    if let Variant::Color(c) = value {
                        mat.albedo_color = *c;
                    }
                }
                "albedo_texture" => {
                    if let Variant::String(s) = value {
                        mat.albedo_texture = Some(TextureSlot::new(s.as_str()));
                    }
                }
                "metallic" => {
                    if let Variant::Float(f) = value {
                        mat.metallic = *f as f32;
                    }
                }
                "metallic_texture" => {
                    if let Variant::String(s) = value {
                        mat.metallic_texture = Some(TextureSlot::new(s.as_str()));
                    }
                }
                "roughness" => {
                    if let Variant::Float(f) = value {
                        mat.roughness = *f as f32;
                    }
                }
                "roughness_texture" => {
                    if let Variant::String(s) = value {
                        mat.roughness_texture = Some(TextureSlot::new(s.as_str()));
                    }
                }
                "normal_enabled" => {
                    if let Variant::Bool(b) = value {
                        mat.normal_enabled = *b;
                    }
                }
                "normal_texture" => {
                    if let Variant::String(s) = value {
                        mat.normal_texture = Some(TextureSlot::new(s.as_str()));
                    }
                }
                "normal_scale" => {
                    if let Variant::Float(f) = value {
                        mat.normal_scale = *f as f32;
                    }
                }
                _ => {} // Ignore unknown properties.
            }
        }
        mat
    }

    /// Returns `true` if any texture slot is populated.
    pub fn has_textures(&self) -> bool {
        self.albedo_texture.is_some()
            || self.metallic_texture.is_some()
            || self.roughness_texture.is_some()
            || self.normal_texture.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_material() {
        let mat = Material3D::default();
        assert_eq!(mat.albedo, Color::new(1.0, 1.0, 1.0, 1.0));
        assert!((mat.roughness - 0.5).abs() < f32::EPSILON);
        assert!(mat.metallic.abs() < f32::EPSILON);
        assert!(!mat.double_sided);
        assert_eq!(mat.shading_mode, ShadingMode::Lambert);
    }

    #[test]
    fn custom_material() {
        let mat = Material3D {
            albedo: Color::new(1.0, 0.0, 0.0, 1.0),
            roughness: 0.8,
            metallic: 1.0,
            emission: Color::new(0.5, 0.5, 0.0, 1.0),
            shading_mode: ShadingMode::Phong,
            double_sided: true,
        };
        assert_eq!(mat.shading_mode, ShadingMode::Phong);
        assert!(mat.double_sided);
        assert!((mat.metallic - 1.0).abs() < f32::EPSILON);
    }

    // ── TextureSlot ──

    #[test]
    fn texture_slot_creation() {
        let slot = TextureSlot::new("res://textures/albedo.png");
        assert_eq!(slot.path, "res://textures/albedo.png");
    }

    // ── StandardMaterial3D ──

    #[test]
    fn standard_material_defaults() {
        let mat = StandardMaterial3D::default();
        assert_eq!(mat.albedo_color, Color::new(1.0, 1.0, 1.0, 1.0));
        assert!(mat.albedo_texture.is_none());
        assert!(mat.metallic.abs() < f32::EPSILON);
        assert!(mat.metallic_texture.is_none());
        assert!((mat.roughness - 1.0).abs() < f32::EPSILON);
        assert!(mat.roughness_texture.is_none());
        assert!(!mat.normal_enabled);
        assert!(mat.normal_texture.is_none());
        assert!((mat.normal_scale - 1.0).abs() < f32::EPSILON);
        assert_eq!(mat.shading_mode, ShadingMode::Lambert);
        assert!(!mat.double_sided);
        assert!(!mat.has_textures());
    }

    #[test]
    fn standard_material_with_all_textures() {
        let mat = StandardMaterial3D {
            albedo_color: Color::new(0.8, 0.2, 0.1, 1.0),
            albedo_texture: Some(TextureSlot::new("res://albedo.png")),
            metallic: 0.9,
            metallic_texture: Some(TextureSlot::new("res://metallic.png")),
            roughness: 0.3,
            roughness_texture: Some(TextureSlot::new("res://roughness.png")),
            normal_enabled: true,
            normal_texture: Some(TextureSlot::new("res://normal.png")),
            normal_scale: 1.5,
            emission: Color::new(0.0, 0.0, 0.0, 0.0),
            shading_mode: ShadingMode::Phong,
            double_sided: false,
        };
        assert!(mat.has_textures());
        assert_eq!(
            mat.albedo_texture.as_ref().unwrap().path,
            "res://albedo.png"
        );
        assert_eq!(
            mat.metallic_texture.as_ref().unwrap().path,
            "res://metallic.png"
        );
        assert_eq!(
            mat.roughness_texture.as_ref().unwrap().path,
            "res://roughness.png"
        );
        assert_eq!(
            mat.normal_texture.as_ref().unwrap().path,
            "res://normal.png"
        );
        assert!(mat.normal_enabled);
        assert!((mat.normal_scale - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn standard_material_to_material3d() {
        let std_mat = StandardMaterial3D {
            albedo_color: Color::new(1.0, 0.0, 0.0, 1.0),
            metallic: 0.8,
            roughness: 0.2,
            emission: Color::new(0.1, 0.1, 0.0, 0.0),
            shading_mode: ShadingMode::Phong,
            double_sided: true,
            ..Default::default()
        };
        let basic = std_mat.to_material3d();
        assert_eq!(basic.albedo, Color::new(1.0, 0.0, 0.0, 1.0));
        assert!((basic.metallic - 0.8).abs() < f32::EPSILON);
        assert!((basic.roughness - 0.2).abs() < f32::EPSILON);
        assert_eq!(basic.emission, Color::new(0.1, 0.1, 0.0, 0.0));
        assert_eq!(basic.shading_mode, ShadingMode::Phong);
        assert!(basic.double_sided);
    }

    #[test]
    fn standard_material_from_properties_full() {
        let mut props = std::collections::HashMap::new();
        props.insert(
            "albedo_color".to_string(),
            Variant::Color(Color::new(0.5, 0.5, 0.5, 1.0)),
        );
        props.insert(
            "albedo_texture".to_string(),
            Variant::String("ExtResource:1".to_string()),
        );
        props.insert("metallic".to_string(), Variant::Float(0.7));
        props.insert(
            "metallic_texture".to_string(),
            Variant::String("ExtResource:2".to_string()),
        );
        props.insert("roughness".to_string(), Variant::Float(0.4));
        props.insert(
            "roughness_texture".to_string(),
            Variant::String("SubResource:3".to_string()),
        );
        props.insert("normal_enabled".to_string(), Variant::Bool(true));
        props.insert(
            "normal_texture".to_string(),
            Variant::String("ExtResource:4".to_string()),
        );
        props.insert("normal_scale".to_string(), Variant::Float(2.0));

        let mat = StandardMaterial3D::from_properties(props.iter());
        assert_eq!(mat.albedo_color, Color::new(0.5, 0.5, 0.5, 1.0));
        assert_eq!(
            mat.albedo_texture.as_ref().unwrap().path,
            "ExtResource:1"
        );
        assert!((mat.metallic - 0.7).abs() < f32::EPSILON);
        assert_eq!(
            mat.metallic_texture.as_ref().unwrap().path,
            "ExtResource:2"
        );
        assert!((mat.roughness - 0.4).abs() < f32::EPSILON);
        assert_eq!(
            mat.roughness_texture.as_ref().unwrap().path,
            "SubResource:3"
        );
        assert!(mat.normal_enabled);
        assert_eq!(
            mat.normal_texture.as_ref().unwrap().path,
            "ExtResource:4"
        );
        assert!((mat.normal_scale - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn standard_material_from_properties_partial() {
        let mut props = std::collections::HashMap::new();
        props.insert("metallic".to_string(), Variant::Float(0.5));
        props.insert("normal_enabled".to_string(), Variant::Bool(true));

        let mat = StandardMaterial3D::from_properties(props.iter());
        // Explicitly set.
        assert!((mat.metallic - 0.5).abs() < f32::EPSILON);
        assert!(mat.normal_enabled);
        // Defaults for everything else.
        assert_eq!(mat.albedo_color, Color::new(1.0, 1.0, 1.0, 1.0));
        assert!(mat.albedo_texture.is_none());
        assert!((mat.roughness - 1.0).abs() < f32::EPSILON);
        assert!((mat.normal_scale - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn standard_material_from_properties_ignores_unknown() {
        let mut props = std::collections::HashMap::new();
        props.insert("albedo_color".to_string(), Variant::Color(Color::WHITE));
        props.insert("unknown_field".to_string(), Variant::Int(42));
        props.insert("another_thing".to_string(), Variant::Bool(true));

        let mat = StandardMaterial3D::from_properties(props.iter());
        assert_eq!(mat.albedo_color, Color::WHITE);
        // Should not panic or change defaults for unknown keys.
        assert!(!mat.normal_enabled);
    }

    #[test]
    fn standard_material_from_properties_wrong_types_ignored() {
        let mut props = std::collections::HashMap::new();
        // Pass wrong types — should be silently ignored.
        props.insert("metallic".to_string(), Variant::String("bad".into()));
        props.insert("normal_enabled".to_string(), Variant::Int(1));
        props.insert("albedo_color".to_string(), Variant::Float(0.5));

        let mat = StandardMaterial3D::from_properties(props.iter());
        // All should remain at defaults since types didn't match.
        assert!(mat.metallic.abs() < f32::EPSILON);
        assert!(!mat.normal_enabled);
        assert_eq!(mat.albedo_color, Color::new(1.0, 1.0, 1.0, 1.0));
    }

    #[test]
    fn standard_material_has_textures_single() {
        let mut mat = StandardMaterial3D::default();
        assert!(!mat.has_textures());
        mat.normal_texture = Some(TextureSlot::new("res://n.png"));
        assert!(mat.has_textures());
    }

    #[test]
    fn standard_material_clone_eq() {
        let mat = StandardMaterial3D {
            albedo_color: Color::new(1.0, 0.0, 0.0, 1.0),
            albedo_texture: Some(TextureSlot::new("res://a.png")),
            normal_enabled: true,
            normal_scale: 2.0,
            ..Default::default()
        };
        let cloned = mat.clone();
        assert_eq!(mat, cloned);
    }
}

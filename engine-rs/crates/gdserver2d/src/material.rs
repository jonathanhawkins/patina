//! 3D material definitions.
//!
//! Provides `Material3D` for describing surface appearance in the
//! 3D rendering pipeline (PBR-style properties).

use gdcore::math::Color;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}

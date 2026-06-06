//! 3D instance types for the rendering server.

use gdcore::math3d::Transform3D;

use crate::material::Material3D;
use crate::mesh::Mesh3D;
use crate::multimesh::MultiMesh3D;
use crate::shader::ShaderMaterial3D;

/// Unique identifier for a 3D render instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Instance3DId(pub u64);

/// A renderable 3D instance in the scene.
#[derive(Debug, Clone)]
pub struct Instance3D {
    /// Unique identifier.
    pub id: Instance3DId,
    /// Mesh geometry (if assigned).
    pub mesh: Option<Mesh3D>,
    /// Surface material (if assigned).
    pub material: Option<Material3D>,
    /// Shader material override (takes precedence over `material` for color).
    pub shader_material: Option<ShaderMaterial3D>,
    /// MultiMesh resource for instanced rendering (if assigned).
    pub multimesh: Option<MultiMesh3D>,
    /// World-space transform.
    pub transform: Transform3D,
    /// Whether the instance is visible.
    pub visible: bool,
}

impl Instance3D {
    /// Creates a new instance with default settings.
    pub fn new(id: Instance3DId) -> Self {
        Self {
            id,
            mesh: None,
            material: None,
            shader_material: None,
            multimesh: None,
            transform: Transform3D::IDENTITY,
            visible: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Vector3;
    use gdcore::math3d::Basis;

    #[test]
    fn instance_creation_defaults() {
        let inst = Instance3D::new(Instance3DId(1));
        assert_eq!(inst.id, Instance3DId(1));
        assert!(inst.visible);
        assert!(inst.mesh.is_none());
        assert!(inst.material.is_none());
        assert_eq!(inst.transform, Transform3D::IDENTITY);
    }

    #[test]
    fn instance_id_equality() {
        assert_eq!(Instance3DId(42), Instance3DId(42));
        assert_ne!(Instance3DId(42), Instance3DId(99));
    }

    #[test]
    fn instance_set_transform() {
        let mut inst = Instance3D::new(Instance3DId(10));
        let t = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(5.0, 10.0, 15.0),
        };
        inst.transform = t;
        assert_eq!(inst.transform.origin, Vector3::new(5.0, 10.0, 15.0));
    }

    #[test]
    fn instance_visibility_toggle() {
        let mut inst = Instance3D::new(Instance3DId(1));
        assert!(inst.visible);
        inst.visible = false;
        assert!(!inst.visible);
    }
}

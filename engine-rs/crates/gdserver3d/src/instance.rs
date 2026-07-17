//! 3D instance types for the rendering server.

use gdcore::math3d::{Aabb, Transform3D};

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

    /// This instance's world-space AABB — its mesh bounds under `transform`
    /// (pat-nzgdu.211).
    ///
    /// `None` when there is no mesh, or the mesh has no vertices: an instance
    /// with nothing to draw has no bounds, and callers must treat that as
    /// "unknown", never as an empty box at the origin.
    ///
    /// ⚠️ Built with the same `transform_aabb` the occluder uses for its own
    /// world AABB, so the two are directly comparable. It is conservative
    /// (axis-aligned re-fit), so a rotated instance's box is larger than its
    /// geometry — which is the safe direction for culling: it can only make an
    /// occlusion test fail, never wrongly cull something visible.
    ///
    /// ⚠️ Ignores `multimesh`: a MultiMesh's per-instance transforms scatter
    /// geometry far outside this box, so a multimesh instance must not be culled
    /// on this bound. `Renderer::is_occluded` skips them for that reason.
    pub fn world_aabb(&self) -> Option<Aabb> {
        let local = self.mesh.as_ref()?.aabb()?;
        Some(crate::occluder::transform_aabb(&local, &self.transform))
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

    // -- world_aabb (pat-nzgdu.211) ----------------------------------------

    /// ⚠️ No mesh ⇒ `None`, never an empty box. A caller culling on a zero-sized
    /// AABB at the origin would be acting on a bound that does not exist.
    #[test]
    fn world_aabb_is_none_without_a_mesh() {
        assert!(Instance3D::new(Instance3DId(1)).world_aabb().is_none());
    }

    /// A mesh with no vertices has no bounds either.
    #[test]
    fn world_aabb_is_none_for_an_empty_mesh() {
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::new(crate::mesh::PrimitiveType::Triangles));
        assert!(inst.world_aabb().is_none());
    }

    #[test]
    fn world_aabb_is_the_mesh_bounds_moved_by_the_transform() {
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(2.0));
        inst.transform = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(10.0, 0.0, -5.0),
        };

        let aabb = inst.world_aabb().expect("a cube has bounds");
        // cube(2.0) spans -1..1 locally, so the world box is centred on the
        // transform's origin.
        assert!((aabb.position.x - 9.0).abs() < 1e-4, "{aabb:?}");
        assert!((aabb.position.z - (-6.0)).abs() < 1e-4, "{aabb:?}");
        assert!((aabb.size.x - 2.0).abs() < 1e-4, "{aabb:?}");
        assert!((aabb.size.z - 2.0).abs() < 1e-4, "{aabb:?}");
    }

    /// The identity transform must not move or resize the bounds — otherwise
    /// every cull would be comparing a box against a subtly shifted copy.
    #[test]
    fn world_aabb_under_identity_equals_the_mesh_aabb() {
        let mut inst = Instance3D::new(Instance3DId(1));
        inst.mesh = Some(Mesh3D::cube(1.0));
        let local = inst.mesh.as_ref().unwrap().aabb().unwrap();
        let world = inst.world_aabb().unwrap();
        assert!((world.position.x - local.position.x).abs() < 1e-4);
        assert!((world.size.y - local.size.y).abs() < 1e-4);
    }
}

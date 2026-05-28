//! MultiMesh3D: instanced rendering data for MultiMeshInstance3D nodes.
//!
//! A [`MultiMesh3D`] holds a single shared [`Mesh3D`] and per-instance
//! transforms, matching Godot's `MultiMesh` resource semantics. The CPU
//! renderer expands each instance into a separate draw call; a future GPU
//! backend can use hardware instancing.

use gdcore::math::Color;
use gdcore::math3d::Transform3D;

use crate::mesh::Mesh3D;

/// Transform format for MultiMesh instances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformFormat {
    /// 2D transforms (Transform2D) — not yet supported, stored as 3D identity.
    Transform2D,
    /// 3D transforms (Transform3D).
    Transform3D,
}

impl Default for TransformFormat {
    fn default() -> Self {
        Self::Transform3D
    }
}

/// A multi-mesh resource: one shared mesh drawn at many positions.
///
/// Mirrors Godot's `MultiMesh` resource. Each instance has its own
/// [`Transform3D`] and an optional per-instance color.
#[derive(Debug, Clone)]
pub struct MultiMesh3D {
    /// The shared mesh geometry drawn for every instance.
    pub mesh: Option<Mesh3D>,
    /// Transform format (2D or 3D).
    pub transform_format: TransformFormat,
    /// Per-instance transforms. Length equals instance count.
    pub instance_transforms: Vec<Transform3D>,
    /// Optional per-instance colors. When non-empty, length must equal
    /// `instance_transforms.len()`.
    pub instance_colors: Vec<Color>,
    /// Whether per-instance custom data is used.
    pub use_custom_data: bool,
}

impl MultiMesh3D {
    /// Creates a new empty MultiMesh with the given instance count.
    ///
    /// All transforms default to [`Transform3D::IDENTITY`].
    pub fn new(instance_count: usize) -> Self {
        Self {
            mesh: None,
            transform_format: TransformFormat::Transform3D,
            instance_transforms: vec![Transform3D::IDENTITY; instance_count],
            instance_colors: Vec::new(),
            use_custom_data: false,
        }
    }

    /// Returns the number of instances.
    pub fn instance_count(&self) -> usize {
        self.instance_transforms.len()
    }

    /// Sets the transform for a specific instance. No-op if index is out of range.
    pub fn set_instance_transform(&mut self, index: usize, transform: Transform3D) {
        if index < self.instance_transforms.len() {
            self.instance_transforms[index] = transform;
        }
    }

    /// Gets the transform for a specific instance, or [`Transform3D::IDENTITY`]
    /// if out of range.
    pub fn get_instance_transform(&self, index: usize) -> Transform3D {
        self.instance_transforms
            .get(index)
            .copied()
            .unwrap_or(Transform3D::IDENTITY)
    }

    /// Sets the per-instance color for a specific instance.
    ///
    /// Initializes the color array to white if not yet allocated.
    pub fn set_instance_color(&mut self, index: usize, color: Color) {
        if self.instance_colors.is_empty() && !self.instance_transforms.is_empty() {
            self.instance_colors = vec![Color::WHITE; self.instance_transforms.len()];
        }
        if index < self.instance_colors.len() {
            self.instance_colors[index] = color;
        }
    }

    /// Gets the per-instance color, defaulting to white.
    pub fn get_instance_color(&self, index: usize) -> Color {
        self.instance_colors
            .get(index)
            .copied()
            .unwrap_or(Color::WHITE)
    }

    /// Resizes the instance count, truncating or extending with identity
    /// transforms.
    pub fn set_instance_count(&mut self, count: usize) {
        self.instance_transforms
            .resize(count, Transform3D::IDENTITY);
        if !self.instance_colors.is_empty() {
            self.instance_colors.resize(count, Color::WHITE);
        }
    }

    /// Flattens this multimesh into the per-instance buffer that a
    /// GPU-instanced draw call would upload.
    ///
    /// Each entry pairs the instance's world-space transform (composed as
    /// `base * per_instance` so the host node's transform stacks first) with
    /// the per-instance colour. If `instance_colors` is empty the buffer
    /// reports `Color::WHITE` for every instance, matching Godot's default.
    /// The returned `Vec` length always matches `instance_count`, so the
    /// renderer can issue one instanced draw of `len()` rather than expanding
    /// to `len()` separate per-instance draws.
    pub fn flatten_instance_data(&self, base: Transform3D) -> Vec<MultiMeshInstanceData> {
        let has_colors = !self.instance_colors.is_empty();
        self.instance_transforms
            .iter()
            .enumerate()
            .map(|(i, &t)| MultiMeshInstanceData {
                transform: base * t,
                color: if has_colors {
                    self.get_instance_color(i)
                } else {
                    Color::WHITE
                },
            })
            .collect()
    }
}

/// One row of a GPU-instanced draw buffer for a [`MultiMesh3D`].
///
/// Mirrors the layout the wgpu pipeline binds as `@vertex` per-instance
/// inputs: a world-space transform plus a colour. The CPU rasterizer can use
/// the same struct to issue one batched draw rather than expanding to
/// `instance_count` per-instance draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MultiMeshInstanceData {
    /// World-space transform for this instance.
    pub transform: Transform3D,
    /// Per-instance colour modulation.
    pub color: Color,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Vector3;
    use gdcore::math3d::Basis;

    #[test]
    fn new_creates_identity_transforms() {
        let mm = MultiMesh3D::new(5);
        assert_eq!(mm.instance_count(), 5);
        for i in 0..5 {
            assert_eq!(mm.get_instance_transform(i), Transform3D::IDENTITY);
        }
    }

    #[test]
    fn set_get_instance_transform() {
        let mut mm = MultiMesh3D::new(3);
        let t = Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(1.0, 2.0, 3.0),
        };
        mm.set_instance_transform(1, t);
        assert_eq!(
            mm.get_instance_transform(1).origin,
            Vector3::new(1.0, 2.0, 3.0)
        );
        assert_eq!(mm.get_instance_transform(0), Transform3D::IDENTITY);
    }

    #[test]
    fn out_of_range_transform_returns_identity() {
        let mm = MultiMesh3D::new(2);
        assert_eq!(mm.get_instance_transform(99), Transform3D::IDENTITY);
    }

    #[test]
    fn set_instance_color_initializes_array() {
        let mut mm = MultiMesh3D::new(3);
        assert!(mm.instance_colors.is_empty());
        mm.set_instance_color(0, Color::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(mm.instance_colors.len(), 3);
        assert_eq!(mm.get_instance_color(0), Color::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(mm.get_instance_color(1), Color::WHITE);
    }

    #[test]
    fn resize_instance_count() {
        let mut mm = MultiMesh3D::new(2);
        mm.set_instance_count(5);
        assert_eq!(mm.instance_count(), 5);
        mm.set_instance_count(1);
        assert_eq!(mm.instance_count(), 1);
    }

    #[test]
    fn empty_multimesh() {
        let mm = MultiMesh3D::new(0);
        assert_eq!(mm.instance_count(), 0);
        assert_eq!(mm.get_instance_color(0), Color::WHITE);
    }

    #[test]
    fn default_transform_format() {
        let mm = MultiMesh3D::new(1);
        assert_eq!(mm.transform_format, TransformFormat::Transform3D);
    }
}

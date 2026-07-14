//! Blend shapes / morph targets (pat-u3l8c.37).
//!
//! Mesh3D had no blend-shape/morph data. This module provides morph targets
//! (per-vertex position deltas) and weighted application: the final vertex
//! positions are `base + Σ weight_t * delta_t`, matching Godot's blend-shape
//! (`MESH_BLEND_SHAPE_MODE_NORMALIZED`-style additive) evaluation. glTF morph
//! import can populate [`MorphTarget`]s as a follow-up.

use gdcore::math::Vector3;

/// A named morph target: per-vertex position deltas from the base mesh.
#[derive(Debug, Clone)]
pub struct MorphTarget {
    /// Blend-shape name (Godot addresses shapes by name).
    pub name: String,
    /// Per-vertex position deltas (parallel to the base vertex array).
    pub deltas: Vec<Vector3>,
}

impl MorphTarget {
    /// Creates a morph target from per-vertex deltas.
    pub fn new(name: &str, deltas: Vec<Vector3>) -> Self {
        Self {
            name: name.to_string(),
            deltas,
        }
    }
}

/// A morphable surface: base vertices, its blend-shape targets, and the current
/// per-target weight.
#[derive(Debug, Clone, Default)]
pub struct BlendShapes {
    /// Rest/base vertex positions.
    pub base: Vec<Vector3>,
    /// Blend-shape targets.
    pub targets: Vec<MorphTarget>,
    /// Current weight per target (parallel to `targets`; 0 = inactive).
    pub weights: Vec<f32>,
}

impl BlendShapes {
    /// Creates a morphable surface with the given base vertices and no shapes.
    pub fn new(base: Vec<Vector3>) -> Self {
        Self {
            base,
            targets: Vec::new(),
            weights: Vec::new(),
        }
    }

    /// Adds a blend-shape target (initial weight 0).
    pub fn add_target(&mut self, target: MorphTarget) {
        self.targets.push(target);
        self.weights.push(0.0);
    }

    /// Sets the weight of target `index` (no-op if out of range).
    pub fn set_weight(&mut self, index: usize, weight: f32) {
        if index < self.weights.len() {
            self.weights[index] = weight;
        }
    }

    /// Finds a target's index by name.
    pub fn find(&self, name: &str) -> Option<usize> {
        self.targets.iter().position(|t| t.name == name)
    }

    /// Returns the morphed vertex positions: `base + Σ weight * delta`.
    pub fn morphed(&self) -> Vec<Vector3> {
        apply_morph(&self.base, &self.targets, &self.weights)
    }
}

/// Applies `targets` with `weights` to `base`, returning morphed positions.
///
/// Vertex `i` becomes `base[i] + Σ_t weights[t] * targets[t].deltas[i]`. Targets
/// with weight 0 are skipped; a delta array shorter than the base only affects
/// its overlapping vertices.
pub fn apply_morph(base: &[Vector3], targets: &[MorphTarget], weights: &[f32]) -> Vec<Vector3> {
    let mut out = base.to_vec();
    for (t, target) in targets.iter().enumerate() {
        let w = weights.get(t).copied().unwrap_or(0.0);
        if w == 0.0 {
            continue;
        }
        for (i, d) in target.deltas.iter().enumerate() {
            if i < out.len() {
                out[i] = Vector3::new(
                    out[i].x + d.x * w,
                    out[i].y + d.y * w,
                    out[i].z + d.z * w,
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: Vector3, b: Vector3) -> bool {
        (a.x - b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4 && (a.z - b.z).abs() < 1e-4
    }

    fn base_tri() -> Vec<Vector3> {
        vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ]
    }

    #[test]
    fn zero_weight_returns_base() {
        let base = base_tri();
        let target = MorphTarget::new(
            "up",
            vec![
                Vector3::new(0.0, 5.0, 0.0),
                Vector3::new(0.0, 5.0, 0.0),
                Vector3::new(0.0, 5.0, 0.0),
            ],
        );
        let out = apply_morph(&base, std::slice::from_ref(&target), &[0.0]);
        for (o, b) in out.iter().zip(base.iter()) {
            assert!(approx(*o, *b));
        }
    }

    #[test]
    fn full_weight_applies_full_delta() {
        let base = base_tri();
        let target = MorphTarget::new("shift", vec![Vector3::new(2.0, 0.0, 0.0); 3]);
        let out = apply_morph(&base, std::slice::from_ref(&target), &[1.0]);
        assert!(approx(out[0], Vector3::new(2.0, 0.0, 0.0)));
        assert!(approx(out[1], Vector3::new(3.0, 0.0, 0.0)));
    }

    #[test]
    fn partial_weight_interpolates() {
        let base = base_tri();
        let target = MorphTarget::new("shift", vec![Vector3::new(2.0, 0.0, 0.0); 3]);
        let out = apply_morph(&base, std::slice::from_ref(&target), &[0.5]);
        assert!(approx(out[0], Vector3::new(1.0, 0.0, 0.0)));
    }

    #[test]
    fn multiple_targets_accumulate() {
        let base = base_tri();
        let a = MorphTarget::new("x", vec![Vector3::new(1.0, 0.0, 0.0); 3]);
        let b = MorphTarget::new("y", vec![Vector3::new(0.0, 2.0, 0.0); 3]);
        let out = apply_morph(&base, &[a, b], &[1.0, 0.5]);
        // vertex0 = (0,0,0) + 1*(1,0,0) + 0.5*(0,2,0) = (1,1,0)
        assert!(approx(out[0], Vector3::new(1.0, 1.0, 0.0)));
    }

    #[test]
    fn blendshapes_struct_morphs_by_name() {
        let mut bs = BlendShapes::new(base_tri());
        bs.add_target(MorphTarget::new("open", vec![Vector3::new(0.0, 0.0, 3.0); 3]));
        let idx = bs.find("open").unwrap();
        bs.set_weight(idx, 1.0);
        let out = bs.morphed();
        assert!(approx(out[0], Vector3::new(0.0, 0.0, 3.0)));
    }

    #[test]
    fn shorter_delta_array_is_safe() {
        let base = base_tri();
        // Only the first vertex has a delta.
        let target = MorphTarget::new("partial", vec![Vector3::new(9.0, 0.0, 0.0)]);
        let out = apply_morph(&base, std::slice::from_ref(&target), &[1.0]);
        assert!(approx(out[0], Vector3::new(9.0, 0.0, 0.0)));
        assert!(approx(out[1], base[1])); // untouched
        assert_eq!(out.len(), 3);
    }
}

//! Blend shapes / morph targets (pat-u3l8c.37).
//!
//! Mesh3D had no blend-shape/morph data. This module provides morph targets
//! (per-vertex position deltas) and weighted application.
//!
//! # ⚠️ Godot has TWO modes and they are not interchangeable
//!
//! `skeleton.glsl:144-148` is the whole contract:
//!
//! ```glsl
//! if (params.normalized_blend_shapes) { vertex = (1.0 - blend_total) * vertex; }
//! vertex += blend_vertex;
//! ```
//!
//! * [`BlendShapeMode::Normalized`] (`BLEND_SHAPE_MODE_NORMALIZED` = 0, and
//!   **Godot's default**) — `(1 - Σw) * base + Σ w * shape`. The base is scaled
//!   DOWN as weights rise, so a single shape at weight 1.0 replaces the base
//!   entirely.
//! * [`BlendShapeMode::Relative`] (`BLEND_SHAPE_MODE_RELATIVE` = 1) —
//!   `base + Σ w * shape`. The base is untouched and shapes are pure offsets.
//!
//! This module originally documented itself as matching `NORMALIZED` while
//! implementing the `RELATIVE` formula (pat-s3rrv.23). Both are now available
//! and [`BlendShapes`] defaults to `Normalized`, matching Godot.
//!
//! The two agree only when the base is the origin or every weight is 0, which
//! is exactly why the mismatch survived: the module's own tests used a
//! zero-origin vertex.
//!
//! glTF morph import can populate [`MorphTarget`]s as a follow-up.

use gdcore::math::Vector3;

/// How blend-shape weights combine with the base vertices — Godot's
/// `Mesh.BlendShapeMode` (`doc/classes/Mesh.xml:373-378`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlendShapeMode {
    /// `BLEND_SHAPE_MODE_NORMALIZED` (0) — Godot's default.
    ///
    /// `(1 - Σw) * base + Σ w * shape`: the base fades out as weights rise.
    #[default]
    Normalized,
    /// `BLEND_SHAPE_MODE_RELATIVE` (1) — `base + Σ w * shape`.
    Relative,
}

impl BlendShapeMode {
    /// Maps Godot's integer enum value; anything unrecognised falls back to the
    /// engine default rather than silently picking the other mode.
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            1 => Self::Relative,
            _ => Self::Normalized,
        }
    }
}

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
    /// How the weights combine with [`base`](Self::base). Defaults to
    /// [`BlendShapeMode::Normalized`], matching Godot.
    pub mode: BlendShapeMode,
}

impl BlendShapes {
    /// Creates a morphable surface with the given base vertices and no shapes.
    pub fn new(base: Vec<Vector3>) -> Self {
        Self {
            base,
            targets: Vec::new(),
            weights: Vec::new(),
            mode: BlendShapeMode::Normalized,
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

    /// Returns the morphed vertex positions, honouring [`mode`](Self::mode).
    pub fn morphed(&self) -> Vec<Vector3> {
        apply_morph_with_mode(&self.base, &self.targets, &self.weights, self.mode)
    }
}

/// Applies `targets` with `weights` to `base` in
/// [`BlendShapeMode::Relative`], returning morphed positions.
///
/// Vertex `i` becomes `base[i] + Σ_t weights[t] * targets[t].deltas[i]`. Targets
/// with weight 0 are skipped; a delta array shorter than the base only affects
/// its overlapping vertices.
///
/// ⚠️ This is Godot's **non-default** mode. For `BLEND_SHAPE_MODE_NORMALIZED`
/// — what a mesh uses unless it says otherwise — call
/// [`apply_morph_with_mode`].
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

/// Applies `targets` with `weights` to `base` under `mode`.
///
/// Port of `skeleton.glsl:144-148`: the accumulation is identical in both
/// modes, and `Normalized` additionally scales the base by `1 - Σw` first.
///
/// ⚠️ `Σw` is the sum of ALL weights, including those of targets whose delta
/// array is short or empty — Godot accumulates `blend_total` while walking the
/// weights, independently of how many vertices each shape touches. Summing only
/// the weights that contributed would leave the base over-weighted whenever a
/// shape is partial.
pub fn apply_morph_with_mode(
    base: &[Vector3],
    targets: &[MorphTarget],
    weights: &[f32],
    mode: BlendShapeMode,
) -> Vec<Vector3> {
    let relative = apply_morph(base, targets, weights);
    if mode == BlendShapeMode::Relative {
        return relative;
    }
    // Normalized: the accumulated shape contribution is `relative - base`, so
    // scaling the base by `1 - Σw` and re-adding it yields
    // `(1 - Σw) * base + Σ w * shape` without a second accumulation pass.
    let total: f32 = weights.iter().take(targets.len()).sum();
    let scale = 1.0 - total;
    relative
        .iter()
        .zip(base.iter())
        .map(|(morphed, b)| {
            Vector3::new(
                morphed.x - b.x + b.x * scale,
                morphed.y - b.y + b.y * scale,
                morphed.z - b.z + b.z * scale,
            )
        })
        .collect()
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

    // -- pat-s3rrv.23: the two modes are not interchangeable ---------------

    /// ⚠️ The case every test above misses, and the reason the mislabelling
    /// survived: they all read vertex 0, which is the ORIGIN.
    ///
    /// `Normalized` scales the base by `1 - Σw`, so at the origin
    /// `(1-w)*0 == 0` and both modes agree for any weight. Only a non-zero base
    /// vertex separates them — here vertex 1 is `(1,0,0)`:
    ///
    /// * Normalized: `(1-1)*(1,0,0) + 1*(2,0,0)` = `(2,0,0)`
    /// * Relative:   `(1,0,0) + 1*(2,0,0)`       = `(3,0,0)`
    #[test]
    fn the_two_modes_differ_on_a_non_origin_vertex() {
        let base = base_tri();
        let target = MorphTarget::new("shift", vec![Vector3::new(2.0, 0.0, 0.0); 3]);
        let targets = std::slice::from_ref(&target);

        let normalized = apply_morph_with_mode(&base, targets, &[1.0], BlendShapeMode::Normalized);
        let relative = apply_morph_with_mode(&base, targets, &[1.0], BlendShapeMode::Relative);

        assert!(
            approx(normalized[1], Vector3::new(2.0, 0.0, 0.0)),
            "NORMALIZED at weight 1 REPLACES the base: (1-1)*base + 1*shape. \
             Reading (3,0,0) means the base was not scaled down, i.e. the \
             relative formula ran under the normalized name — got {:?}",
            normalized[1]
        );
        assert!(
            approx(relative[1], Vector3::new(3.0, 0.0, 0.0)),
            "RELATIVE adds to an untouched base; got {:?}",
            relative[1]
        );
        // Vertex 0 is the origin, where the two agree — which is exactly why
        // asserting only on it proves nothing about the mode.
        assert!(approx(normalized[0], relative[0]));
    }

    /// A half-weight shape blends base and shape rather than overshooting.
    #[test]
    fn normalized_half_weight_blends_base_and_shape() {
        let base = base_tri();
        let target = MorphTarget::new("shift", vec![Vector3::new(2.0, 0.0, 0.0); 3]);
        let out = apply_morph_with_mode(
            &base,
            std::slice::from_ref(&target),
            &[0.5],
            BlendShapeMode::Normalized,
        );
        // (1-0.5)*(1,0,0) + 0.5*(2,0,0) = (0.5,0,0) + (1,0,0) = (1.5,0,0)
        assert!(
            approx(out[1], Vector3::new(1.5, 0.0, 0.0)),
            "got {:?}; the relative formula would give (2,0,0)",
            out[1]
        );
    }

    /// `BlendShapes` defaults to Godot's default mode, not the one the module
    /// used to implement under that name.
    #[test]
    fn blendshapes_defaults_to_normalized() {
        assert_eq!(BlendShapeMode::default(), BlendShapeMode::Normalized);
        assert_eq!(
            BlendShapes::new(base_tri()).mode,
            BlendShapeMode::Normalized
        );
        // Godot's ordinals, and an unknown value falls back to the default
        // rather than silently selecting the other mode.
        assert_eq!(
            BlendShapeMode::from_godot_int(0),
            BlendShapeMode::Normalized
        );
        assert_eq!(BlendShapeMode::from_godot_int(1), BlendShapeMode::Relative);
        assert_eq!(
            BlendShapeMode::from_godot_int(7),
            BlendShapeMode::Normalized
        );
    }

    /// ⚠️ `Σw` counts every weight, even for a shape whose delta array is
    /// short — Godot accumulates `blend_total` while walking the weights,
    /// independently of how many vertices each shape touches.
    ///
    /// Summing only the weights that actually contributed would leave the base
    /// over-weighted on the untouched vertices, which shows up as part of a
    /// mesh failing to fade while the rest does.
    #[test]
    fn normalized_total_counts_weights_of_partial_shapes() {
        let base = base_tri();
        // Touches vertex 0 only, but carries full weight.
        let partial = MorphTarget::new("partial", vec![Vector3::new(9.0, 0.0, 0.0)]);
        let out = apply_morph_with_mode(
            &base,
            std::slice::from_ref(&partial),
            &[1.0],
            BlendShapeMode::Normalized,
        );
        // Vertex 1 has no delta, so it is purely `(1 - 1) * (1,0,0)` = origin.
        assert!(
            approx(out[1], Vector3::new(0.0, 0.0, 0.0)),
            "an untouched vertex must still be scaled by (1 - Σw); got {:?} — \
             (1,0,0) here means Σw ignored the partial shape's weight",
            out[1]
        );
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

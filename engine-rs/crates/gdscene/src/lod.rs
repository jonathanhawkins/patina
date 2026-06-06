//! LOD (Level of Detail) distance-based switching system.
//!
//! Implements Godot's `GeometryInstance3D` visibility-range LOD:
//!
//! - Each mesh instance can define a **visibility range** via
//!   `visibility_range_begin` and `visibility_range_end` distances.
//! - At runtime the camera-to-instance distance is compared against
//!   these thresholds to decide whether the instance should be drawn.
//! - An optional **fade mode** provides smooth cross-fade transitions
//!   instead of hard pops.
//! - Multiple LOD levels for the same object are modeled as sibling
//!   nodes with non-overlapping visibility ranges.
//!
//! The renderer calls [`LodEvaluator::evaluate`] per instance each
//! frame and uses the returned [`LodVisibility`] to either skip the
//! draw, draw at full opacity, or draw with a fade alpha.

use gdcore::math::Vector3;

// ---------------------------------------------------------------------------
// Fade mode
// ---------------------------------------------------------------------------

/// Controls how transitions between LOD levels appear.
///
/// Maps to Godot's `GeometryInstance3D.VisibilityRangeFadeMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VisibilityRangeFadeMode {
    /// Hard switch — the instance pops in/out instantly.
    #[default]
    Disabled = 0,
    /// The instance fades itself (alpha ramps at range edges).
    FadeSelf = 1,
    /// Dependencies mode — cross-fade with the next LOD level.
    FadeDependencies = 2,
}

// ---------------------------------------------------------------------------
// LOD configuration per instance
// ---------------------------------------------------------------------------

/// LOD parameters attached to a single geometry instance.
///
/// These mirror the `visibility_range_*` properties on
/// `GeometryInstance3D`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LodRange {
    /// Distance (meters) at which the instance *starts* being visible.
    /// `0.0` means visible from any distance (no near-clip).
    pub begin: f64,
    /// Margin (meters) over which the begin transition fades in.
    pub begin_margin: f64,
    /// Distance (meters) at which the instance *stops* being visible.
    /// `0.0` means no far limit.
    pub end: f64,
    /// Margin (meters) over which the end transition fades out.
    pub end_margin: f64,
    /// How the fade is applied.
    pub fade_mode: VisibilityRangeFadeMode,
}

impl Default for LodRange {
    fn default() -> Self {
        Self {
            begin: 0.0,
            begin_margin: 0.0,
            end: 0.0,
            end_margin: 0.0,
            fade_mode: VisibilityRangeFadeMode::Disabled,
        }
    }
}

impl LodRange {
    /// Creates a simple LOD range with hard switching (no fade).
    pub fn new(begin: f64, end: f64) -> Self {
        Self {
            begin,
            end,
            ..Default::default()
        }
    }

    /// Builder: set begin/end margins for smooth fading.
    pub fn with_margins(mut self, begin_margin: f64, end_margin: f64) -> Self {
        self.begin_margin = begin_margin;
        self.end_margin = end_margin;
        self
    }

    /// Builder: set the fade mode.
    pub fn with_fade_mode(mut self, mode: VisibilityRangeFadeMode) -> Self {
        self.fade_mode = mode;
        self
    }

    /// Returns `true` if this range is effectively disabled
    /// (both begin and end are 0).
    pub fn is_disabled(&self) -> bool {
        self.begin == 0.0 && self.end == 0.0
    }
}

// ---------------------------------------------------------------------------
// Evaluation result
// ---------------------------------------------------------------------------

/// The visibility state of an instance after LOD evaluation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LodVisibility {
    /// Fully hidden — skip the draw call entirely.
    Hidden,
    /// Fully visible at normal opacity.
    Visible,
    /// Partially visible during a fade transition.
    /// `alpha` is in `(0.0, 1.0)`.
    Fading { alpha: f64 },
}

impl LodVisibility {
    /// Returns `true` if the instance should be drawn at all.
    pub fn is_drawn(&self) -> bool {
        !matches!(self, LodVisibility::Hidden)
    }

    /// Returns the effective alpha (`0.0` for hidden, `1.0` for fully visible).
    pub fn alpha(&self) -> f64 {
        match self {
            LodVisibility::Hidden => 0.0,
            LodVisibility::Visible => 1.0,
            LodVisibility::Fading { alpha } => *alpha,
        }
    }
}

// ---------------------------------------------------------------------------
// LodEvaluator
// ---------------------------------------------------------------------------

/// Stateless evaluator that computes LOD visibility for instances.
///
/// The evaluator holds per-frame state (camera position, LOD bias)
/// and is called once per geometry instance to determine its
/// visibility.
#[derive(Debug, Clone)]
pub struct LodEvaluator {
    /// Camera world-space position for this frame.
    pub camera_position: Vector3,
    /// Global LOD bias multiplier. Values > 1.0 push LOD transitions
    /// further away (higher quality); values < 1.0 bring them closer
    /// (better performance). Default: `1.0`.
    pub lod_bias: f64,
}

impl Default for LodEvaluator {
    fn default() -> Self {
        Self {
            camera_position: Vector3::ZERO,
            lod_bias: 1.0,
        }
    }
}

impl LodEvaluator {
    /// Creates an evaluator for a frame with the given camera position.
    pub fn new(camera_position: Vector3) -> Self {
        Self {
            camera_position,
            lod_bias: 1.0,
        }
    }

    /// Creates an evaluator with a custom LOD bias.
    pub fn with_bias(camera_position: Vector3, lod_bias: f64) -> Self {
        Self {
            camera_position,
            lod_bias: lod_bias.max(0.0),
        }
    }

    /// Evaluates the LOD visibility for an instance at `instance_position`
    /// with the given [`LodRange`].
    pub fn evaluate(&self, instance_position: Vector3, range: &LodRange) -> LodVisibility {
        // If range is disabled, always visible.
        if range.is_disabled() {
            return LodVisibility::Visible;
        }

        let distance = self.distance_to(instance_position);
        // Apply LOD bias: multiply the effective distance thresholds.
        let bias = self.lod_bias;

        let begin = range.begin * bias;
        let end = if range.end > 0.0 {
            range.end * bias
        } else {
            f64::MAX
        };
        let begin_margin = range.begin_margin * bias;
        let end_margin = range.end_margin * bias;

        // Outside the [begin, end] window → hidden.
        if distance < (begin - begin_margin).max(0.0) {
            return LodVisibility::Hidden;
        }
        if distance > end + end_margin {
            return LodVisibility::Hidden;
        }

        // Check fade regions if fade mode is not Disabled.
        if range.fade_mode != VisibilityRangeFadeMode::Disabled {
            // Begin fade-in region: [begin - begin_margin, begin]
            if begin_margin > 0.0 && distance < begin {
                let t = (distance - (begin - begin_margin)) / begin_margin;
                return LodVisibility::Fading {
                    alpha: t.clamp(0.0, 1.0),
                };
            }

            // End fade-out region: [end, end + end_margin]
            if end_margin > 0.0 && distance > end && end < f64::MAX {
                let t = (distance - end) / end_margin;
                return LodVisibility::Fading {
                    alpha: (1.0 - t).clamp(0.0, 1.0),
                };
            }
        } else {
            // Hard switching: outside [begin, end] → hidden.
            if distance < begin {
                return LodVisibility::Hidden;
            }
            if distance > end && end < f64::MAX {
                return LodVisibility::Hidden;
            }
        }

        LodVisibility::Visible
    }

    /// Computes the distance from the camera to an instance position.
    fn distance_to(&self, pos: Vector3) -> f64 {
        let dx = self.camera_position.x - pos.x;
        let dy = self.camera_position.y - pos.y;
        let dz = self.camera_position.z - pos.z;
        ((dx * dx + dy * dy + dz * dz) as f64).sqrt()
    }
}

// ---------------------------------------------------------------------------
// LodGroup — multiple LOD levels for one logical object
// ---------------------------------------------------------------------------

/// Represents a set of LOD levels for one logical object.
///
/// Each level has a name/index and a `LodRange`. The group can
/// determine which level(s) should be active at a given distance.
#[derive(Debug, Clone)]
pub struct LodGroup {
    levels: Vec<LodLevel>,
}

/// A single LOD level within a [`LodGroup`].
#[derive(Debug, Clone)]
pub struct LodLevel {
    /// Descriptive label (e.g., "LOD0", "LOD1", "LOD2").
    pub label: String,
    /// The visibility range for this level.
    pub range: LodRange,
}

impl LodGroup {
    /// Creates an empty LOD group.
    pub fn new() -> Self {
        Self { levels: Vec::new() }
    }

    /// Adds a LOD level to the group.
    pub fn add_level(&mut self, label: impl Into<String>, range: LodRange) {
        self.levels.push(LodLevel {
            label: label.into(),
            range,
        });
    }

    /// Returns the number of LOD levels.
    pub fn level_count(&self) -> usize {
        self.levels.len()
    }

    /// Evaluates all levels and returns the indices of levels that
    /// should be drawn, along with their visibility state.
    pub fn evaluate(
        &self,
        evaluator: &LodEvaluator,
        instance_position: Vector3,
    ) -> Vec<(usize, LodVisibility)> {
        self.levels
            .iter()
            .enumerate()
            .map(|(i, level)| (i, evaluator.evaluate(instance_position, &level.range)))
            .filter(|(_, vis)| vis.is_drawn())
            .collect()
    }

    /// Returns the single best (lowest-index) visible level, or `None`
    /// if all levels are hidden.
    pub fn active_level(
        &self,
        evaluator: &LodEvaluator,
        instance_position: Vector3,
    ) -> Option<(usize, LodVisibility)> {
        self.levels
            .iter()
            .enumerate()
            .map(|(i, level)| (i, evaluator.evaluate(instance_position, &level.range)))
            .find(|(_, vis)| vis.is_drawn())
    }

    /// Returns the levels slice.
    pub fn levels(&self) -> &[LodLevel] {
        &self.levels
    }
}

impl Default for LodGroup {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn cam_at(x: f32, y: f32, z: f32) -> LodEvaluator {
        LodEvaluator::new(Vector3::new(x, y, z))
    }

    #[test]
    fn disabled_range_always_visible() {
        let eval = cam_at(0.0, 0.0, 0.0);
        let range = LodRange::default(); // begin=0, end=0
        assert_eq!(
            eval.evaluate(Vector3::new(100.0, 0.0, 0.0), &range),
            LodVisibility::Visible
        );
    }

    #[test]
    fn hard_switch_within_range() {
        let eval = cam_at(0.0, 0.0, 0.0);
        let range = LodRange::new(5.0, 50.0);
        // Instance at 20m — inside range
        let vis = eval.evaluate(Vector3::new(20.0, 0.0, 0.0), &range);
        assert_eq!(vis, LodVisibility::Visible);
    }

    #[test]
    fn hard_switch_too_close() {
        let eval = cam_at(0.0, 0.0, 0.0);
        let range = LodRange::new(10.0, 50.0);
        // Instance at 5m — too close
        let vis = eval.evaluate(Vector3::new(5.0, 0.0, 0.0), &range);
        assert_eq!(vis, LodVisibility::Hidden);
    }

    #[test]
    fn hard_switch_too_far() {
        let eval = cam_at(0.0, 0.0, 0.0);
        let range = LodRange::new(10.0, 50.0);
        // Instance at 60m — too far
        let vis = eval.evaluate(Vector3::new(60.0, 0.0, 0.0), &range);
        assert_eq!(vis, LodVisibility::Hidden);
    }

    #[test]
    fn hard_switch_no_end_limit() {
        let eval = cam_at(0.0, 0.0, 0.0);
        let range = LodRange::new(5.0, 0.0); // end=0 means no far limit
        let vis = eval.evaluate(Vector3::new(1000.0, 0.0, 0.0), &range);
        assert_eq!(vis, LodVisibility::Visible);
    }

    #[test]
    fn fade_in_at_begin_margin() {
        let eval = cam_at(0.0, 0.0, 0.0);
        let range = LodRange::new(10.0, 100.0)
            .with_margins(5.0, 5.0)
            .with_fade_mode(VisibilityRangeFadeMode::FadeSelf);
        // Instance at 7.5m — in the begin fade zone [5, 10]
        let vis = eval.evaluate(Vector3::new(7.5, 0.0, 0.0), &range);
        match vis {
            LodVisibility::Fading { alpha } => {
                assert!((alpha - 0.5).abs() < 1e-9, "alpha was {}", alpha);
            }
            other => panic!("expected Fading, got {:?}", other),
        }
    }

    #[test]
    fn fade_out_at_end_margin() {
        let eval = cam_at(0.0, 0.0, 0.0);
        let range = LodRange::new(10.0, 100.0)
            .with_margins(5.0, 10.0)
            .with_fade_mode(VisibilityRangeFadeMode::FadeSelf);
        // Instance at 105m — in the end fade zone [100, 110]
        let vis = eval.evaluate(Vector3::new(105.0, 0.0, 0.0), &range);
        match vis {
            LodVisibility::Fading { alpha } => {
                assert!((alpha - 0.5).abs() < 1e-9, "alpha was {}", alpha);
            }
            other => panic!("expected Fading, got {:?}", other),
        }
    }

    #[test]
    fn fade_fully_visible_in_center() {
        let eval = cam_at(0.0, 0.0, 0.0);
        let range = LodRange::new(10.0, 100.0)
            .with_margins(5.0, 5.0)
            .with_fade_mode(VisibilityRangeFadeMode::FadeSelf);
        // Instance at 50m — well within range
        let vis = eval.evaluate(Vector3::new(50.0, 0.0, 0.0), &range);
        assert_eq!(vis, LodVisibility::Visible);
    }

    #[test]
    fn fade_hidden_beyond_margin() {
        let eval = cam_at(0.0, 0.0, 0.0);
        let range = LodRange::new(10.0, 100.0)
            .with_margins(5.0, 5.0)
            .with_fade_mode(VisibilityRangeFadeMode::FadeSelf);
        // Instance at 110m — beyond end + margin
        let vis = eval.evaluate(Vector3::new(110.0, 0.0, 0.0), &range);
        assert_eq!(vis, LodVisibility::Hidden);
    }

    #[test]
    fn lod_bias_pushes_transitions_further() {
        let eval = LodEvaluator::with_bias(Vector3::ZERO, 2.0);
        let range = LodRange::new(10.0, 50.0);
        // Without bias: 55m would be hidden. With bias 2.0: end becomes 100m.
        let vis = eval.evaluate(Vector3::new(55.0, 0.0, 0.0), &range);
        assert_eq!(vis, LodVisibility::Visible);
    }

    #[test]
    fn lod_bias_brings_transitions_closer() {
        let eval = LodEvaluator::with_bias(Vector3::ZERO, 0.5);
        let range = LodRange::new(10.0, 50.0);
        // With bias 0.5: end becomes 25m. Instance at 30m is hidden.
        let vis = eval.evaluate(Vector3::new(30.0, 0.0, 0.0), &range);
        assert_eq!(vis, LodVisibility::Hidden);
    }

    #[test]
    fn lod_visibility_alpha_values() {
        assert_eq!(LodVisibility::Hidden.alpha(), 0.0);
        assert_eq!(LodVisibility::Visible.alpha(), 1.0);
        assert_eq!(LodVisibility::Fading { alpha: 0.7 }.alpha(), 0.7);
    }

    #[test]
    fn lod_visibility_is_drawn() {
        assert!(!LodVisibility::Hidden.is_drawn());
        assert!(LodVisibility::Visible.is_drawn());
        assert!(LodVisibility::Fading { alpha: 0.1 }.is_drawn());
    }

    #[test]
    fn lod_range_is_disabled() {
        assert!(LodRange::default().is_disabled());
        assert!(!LodRange::new(0.0, 50.0).is_disabled());
        assert!(!LodRange::new(10.0, 0.0).is_disabled());
    }

    #[test]
    fn lod_range_builder() {
        let r = LodRange::new(5.0, 100.0)
            .with_margins(2.0, 3.0)
            .with_fade_mode(VisibilityRangeFadeMode::FadeDependencies);
        assert_eq!(r.begin, 5.0);
        assert_eq!(r.end, 100.0);
        assert_eq!(r.begin_margin, 2.0);
        assert_eq!(r.end_margin, 3.0);
        assert_eq!(r.fade_mode, VisibilityRangeFadeMode::FadeDependencies);
    }

    #[test]
    fn distance_calculation_3d() {
        let eval = cam_at(3.0, 4.0, 0.0);
        // Distance from (3,4,0) to origin = 5
        let range = LodRange::new(0.0, 10.0);
        let vis = eval.evaluate(Vector3::ZERO, &range);
        assert_eq!(vis, LodVisibility::Visible);
    }

    // -----------------------------------------------------------------------
    // LodGroup tests
    // -----------------------------------------------------------------------

    #[test]
    fn lod_group_empty() {
        let group = LodGroup::new();
        assert_eq!(group.level_count(), 0);
    }

    #[test]
    fn lod_group_single_level() {
        let mut group = LodGroup::new();
        group.add_level("LOD0", LodRange::new(0.0, 50.0));
        assert_eq!(group.level_count(), 1);

        let eval = cam_at(0.0, 0.0, 0.0);
        let active = group.active_level(&eval, Vector3::new(25.0, 0.0, 0.0));
        assert!(active.is_some());
        assert_eq!(active.unwrap().0, 0);
    }

    #[test]
    fn lod_group_selects_correct_level() {
        let mut group = LodGroup::new();
        group.add_level("LOD0", LodRange::new(0.0, 20.0));  // near
        group.add_level("LOD1", LodRange::new(20.0, 50.0));  // mid
        group.add_level("LOD2", LodRange::new(50.0, 0.0));   // far (no limit)

        let eval = cam_at(0.0, 0.0, 0.0);

        // Close (10m) → LOD0
        let (idx, _) = group.active_level(&eval, Vector3::new(10.0, 0.0, 0.0)).unwrap();
        assert_eq!(idx, 0);

        // Medium (30m) → LOD1
        let (idx, _) = group.active_level(&eval, Vector3::new(30.0, 0.0, 0.0)).unwrap();
        assert_eq!(idx, 1);

        // Far (100m) → LOD2
        let (idx, _) = group.active_level(&eval, Vector3::new(100.0, 0.0, 0.0)).unwrap();
        assert_eq!(idx, 2);
    }

    #[test]
    fn lod_group_evaluate_returns_multiple_during_crossfade() {
        let mut group = LodGroup::new();
        group.add_level(
            "LOD0",
            LodRange::new(0.0, 20.0)
                .with_margins(0.0, 5.0)
                .with_fade_mode(VisibilityRangeFadeMode::FadeSelf),
        );
        group.add_level(
            "LOD1",
            LodRange::new(20.0, 100.0)
                .with_margins(5.0, 0.0)
                .with_fade_mode(VisibilityRangeFadeMode::FadeSelf),
        );

        let eval = cam_at(0.0, 0.0, 0.0);
        // At 22m: LOD0 is fading out [20,25], LOD1 is fading in [15,20] — but
        // LOD1 begin=20, margin=5, so fade-in zone is [15,20]. At 22m LOD1 is visible.
        // LOD0 end=20, margin=5, so fade-out zone is [20,25]. At 22m LOD0 is fading.
        let active = group.evaluate(&eval, Vector3::new(22.0, 0.0, 0.0));
        // Both should be drawn
        assert!(active.len() >= 1);
    }

    #[test]
    fn lod_group_all_hidden() {
        let mut group = LodGroup::new();
        group.add_level("LOD0", LodRange::new(10.0, 20.0));

        let eval = cam_at(0.0, 0.0, 0.0);
        // Instance at 5m — too close for LOD0
        let active = group.active_level(&eval, Vector3::new(5.0, 0.0, 0.0));
        assert!(active.is_none());
    }

    #[test]
    fn lod_group_levels_accessor() {
        let mut group = LodGroup::new();
        group.add_level("LOD0", LodRange::new(0.0, 20.0));
        group.add_level("LOD1", LodRange::new(20.0, 50.0));
        assert_eq!(group.levels()[0].label, "LOD0");
        assert_eq!(group.levels()[1].label, "LOD1");
    }

    #[test]
    fn fade_mode_enum_values() {
        assert_eq!(VisibilityRangeFadeMode::Disabled as u32, 0);
        assert_eq!(VisibilityRangeFadeMode::FadeSelf as u32, 1);
        assert_eq!(VisibilityRangeFadeMode::FadeDependencies as u32, 2);
    }

    #[test]
    fn evaluator_default() {
        let eval = LodEvaluator::default();
        assert_eq!(eval.camera_position, Vector3::ZERO);
        assert!((eval.lod_bias - 1.0).abs() < 1e-9);
    }

    #[test]
    fn negative_bias_clamped_to_zero() {
        let eval = LodEvaluator::with_bias(Vector3::ZERO, -1.0);
        assert!(eval.lod_bias >= 0.0);
    }

    #[test]
    fn begin_only_range_visible_far_away() {
        let eval = cam_at(0.0, 0.0, 0.0);
        // begin=50, end=0 means "only visible beyond 50m"
        let range = LodRange::new(50.0, 0.0);
        let vis_near = eval.evaluate(Vector3::new(30.0, 0.0, 0.0), &range);
        let vis_far = eval.evaluate(Vector3::new(100.0, 0.0, 0.0), &range);
        assert_eq!(vis_near, LodVisibility::Hidden);
        assert_eq!(vis_far, LodVisibility::Visible);
    }
}

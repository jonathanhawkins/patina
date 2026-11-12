//! `RootMotionView` — the editor gizmo that shows the motion an animation is
//! *driving* (pat-nzgdu.184).
//!
//! Godot's RootMotionView draws a grid under the character and scrolls it by the
//! root motion an `AnimationMixer` has accumulated. That makes the motion
//! visible: a walk cycle that should carry the character forward scrolls the
//! grid backward under its feet, and one that is sliding in place does not move
//! the grid at all.
//!
//! This module owns the **accumulation and grid offset** — the part that has to
//! be right for the gizmo to mean anything. Drawing it in the editor viewport is
//! separate; see the bead notes.
//!
//! The grid is conceptually infinite, so only the sub-cell remainder is ever
//! drawn: scrolling by exactly one cell is indistinguishable from not scrolling.
//! [`RootMotionView::grid_offset`] therefore wraps **X and Z** into
//! `[0, cell_size)` — never Y, which is the height of a flat grid and has no
//! cells along it — while [`RootMotionView::accumulated`] keeps the full,
//! unwrapped travel that Godot does not retain.
//!
//! Behaviour verified against Godot's own source, vendored at
//! `upstream/godot/scene/animation/root_motion_view.cpp`, not inferred.

use gdcore::math::Vector3;

use crate::animation::RootMotionDelta;

/// Godot's defaults for the gizmo (`RootMotionView.xml`).
const DEFAULT_CELL_SIZE: f32 = 1.0;
const DEFAULT_RADIUS: f32 = 10.0;

/// Accumulates an `AnimationMixer`'s root motion into a scrolling grid offset.
#[derive(Debug, Clone, PartialEq)]
pub struct RootMotionView {
    /// Grid spacing. The drawn offset wraps into `[0, cell_size)`.
    pub cell_size: f32,
    /// How far the grid extends from the node.
    pub radius: f32,
    /// Keep the grid flat on XZ by ignoring vertical travel. True in Godot: a
    /// jump must not carry the ground plane up with the character.
    pub zero_y: bool,
    /// Total unwrapped travel, including Y even when `zero_y` is set.
    accumulated: Vector3,
}

impl Default for RootMotionView {
    fn default() -> Self {
        Self {
            cell_size: DEFAULT_CELL_SIZE,
            radius: DEFAULT_RADIUS,
            zero_y: true,
            accumulated: Vector3::ZERO,
        }
    }
}

impl RootMotionView {
    /// A view with Godot's defaults and no travel yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one frame's root motion, as produced by
    /// [`AnimationPlayer::consume_root_motion`](crate::animation::AnimationPlayer::consume_root_motion).
    ///
    /// **Known gap:** `delta.rotation_y` is discarded. Godot rotates each
    /// position delta by the accumulated basis before adding it
    /// (`root_motion_view.cpp:120-122`), so a *turning* walk cycle accumulates
    /// along a curve. Straight-line motion is unaffected; see the bead notes.
    ///
    /// **Deviation from Godot, deliberate:** Godot has no unwrapped travel — it
    /// wraps and zeroes in place, destroying the value each frame. This keeps
    /// the full delta and applies `zero_y`/wrapping when the offset is *read*,
    /// so [`accumulated`](Self::accumulated) can report the real distance walked
    /// and toggling `zero_y` cannot lose banked travel. The drawn output,
    /// [`grid_offset`](Self::grid_offset), still matches Godot exactly.
    pub fn accumulate(&mut self, delta: &RootMotionDelta) {
        self.accumulated = self.accumulated + delta.position;
    }

    /// Total unwrapped travel since the last [`reset`](Self::reset) — the honest
    /// distance the animation has driven, ignoring the grid.
    pub fn accumulated(&self) -> Vector3 {
        self.accumulated
    }

    /// Where to draw the grid: the accumulated travel wrapped into one cell.
    ///
    /// Mirrors `RootMotionView::_notification` (`root_motion_view.cpp:124-128`)
    /// exactly:
    ///
    /// * **X and Z wrap; Y never does.** Y is the height of a flat grid — there
    ///   are no cells along it to wrap into.
    /// * Wrapping uses `rem_euclid`, which is Godot's `Math::fposmod` for a
    ///   positive divisor. Plain `%` would keep the dividend's sign, so
    ///   `-0.25 % 1.0` is `-0.25` — a negative offset that drags the grid out of
    ///   its own cell instead of wrapping it round to `0.75`.
    pub fn grid_offset(&self) -> Vector3 {
        let mut p = self.accumulated;

        // Not in Godot, which does not guard: `fposmod(x, 0)` yields NaN and the
        // grid vanishes. The inspector can hold a zero mid-edit, so leave the
        // travel unwrapped rather than dividing by it.
        if self.cell_size.is_nan() || self.cell_size <= 0.0 {
            if self.zero_y {
                p.y = 0.0;
            }
            return p;
        }

        p.x = p.x.rem_euclid(self.cell_size);
        // Godot zeroes Y between the two wraps and never wraps it.
        if self.zero_y {
            p.y = 0.0;
        }
        p.z = p.z.rem_euclid(self.cell_size);
        p
    }

    /// Drops all accumulated travel — the gizmo starts again from the character.
    pub fn reset(&mut self) {
        self.accumulated = Vector3::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delta(x: f32, y: f32, z: f32) -> RootMotionDelta {
        RootMotionDelta {
            position: Vector3::new(x, y, z),
            rotation_y: 0.0,
        }
    }

    #[test]
    fn defaults_match_godot() {
        let v = RootMotionView::new();
        assert_eq!(v.cell_size, 1.0);
        assert_eq!(v.radius, 10.0);
        assert!(v.zero_y, "Godot keeps the grid flat by default");
        assert_eq!(v.accumulated(), Vector3::ZERO);
    }

    #[test]
    fn offset_advances_with_accumulation() {
        let mut v = RootMotionView::new();
        assert_eq!(v.grid_offset(), Vector3::ZERO);
        v.accumulate(&delta(0.25, 0.0, 0.0));
        assert!((v.grid_offset().x - 0.25).abs() < 1e-6);
        v.accumulate(&delta(0.25, 0.0, 0.0));
        assert!(
            (v.grid_offset().x - 0.5).abs() < 1e-6,
            "the grid must keep scrolling as motion accumulates"
        );
    }

    /// The grid is infinite: one whole cell of travel looks like none at all.
    #[test]
    fn offset_wraps_at_one_cell() {
        let mut v = RootMotionView::new();
        v.accumulate(&delta(1.0, 0.0, 0.0));
        assert!(
            v.grid_offset().x.abs() < 1e-6,
            "exactly one cell of travel is indistinguishable from zero"
        );
        // ...but the real travel is still banked.
        assert!((v.accumulated().x - 1.0).abs() < 1e-6);

        v.accumulate(&delta(0.25, 0.0, 0.0));
        assert!((v.grid_offset().x - 0.25).abs() < 1e-6);
        assert!((v.accumulated().x - 1.25).abs() < 1e-6);
    }

    /// ⚠️ Walking backwards. `%` would give -0.25 and drag the grid out of its
    /// cell; `rem_euclid` wraps it round to 0.75.
    #[test]
    fn backwards_travel_wraps_positively() {
        let mut v = RootMotionView::new();
        v.accumulate(&delta(-0.25, 0.0, 0.0));
        let off = v.grid_offset();
        assert!(
            off.x >= 0.0 && off.x < 1.0,
            "the offset must stay inside one cell, got {}",
            off.x
        );
        assert!(
            (off.x - 0.75).abs() < 1e-6,
            "-0.25 wraps to 0.75, not -0.25; got {}",
            off.x
        );
        assert!(
            (v.accumulated().x + 0.25).abs() < 1e-6,
            "travel is still -0.25"
        );
    }

    /// A jump must not lift the ground plane.
    ///
    /// ⚠️ The vertical travel here is 1.5 — deliberately MORE than one cell. At
    /// 0.5 this test would pass even if Y were wrongly wrapped, because
    /// `rem_euclid` is the identity below the divisor; it would be blind to the
    /// bug it exists to catch.
    #[test]
    fn zero_y_flattens_vertical_travel() {
        let mut v = RootMotionView::new(); // cell_size 1.0
        v.accumulate(&delta(0.25, 1.5, 0.25));
        assert_eq!(v.grid_offset().y, 0.0, "zero_y keeps the grid on XZ");
        // The vertical travel is not lost — only hidden.
        assert!((v.accumulated().y - 1.5).abs() < 1e-6);

        // ⚠️ Y is NEVER wrapped: Godot wraps X and Z only
        // (root_motion_view.cpp:124-128). Y is the height of a flat grid — there
        // are no cells along it. A wrapped Y would read 0.5 here.
        v.zero_y = false;
        assert!(
            (v.grid_offset().y - 1.5).abs() < 1e-6,
            "Y must come back unwrapped (1.5), not wrapped into a cell (0.5); got {}",
            v.grid_offset().y
        );
        // ...while X and Z still wrap.
        assert!((v.grid_offset().x - 0.25).abs() < 1e-6);
    }

    #[test]
    fn cell_size_scales_the_wrap() {
        let mut v = RootMotionView {
            cell_size: 2.0,
            ..RootMotionView::new()
        };
        v.accumulate(&delta(3.0, 0.0, 0.0));
        assert!(
            (v.grid_offset().x - 1.0).abs() < 1e-6,
            "3 units across a 2-unit cell leaves 1"
        );
    }

    /// A zero cell size must not divide by zero or produce NaN — the inspector
    /// can hold one mid-edit.
    #[test]
    fn non_positive_cell_size_does_not_produce_nan() {
        let mut v = RootMotionView {
            cell_size: 0.0,
            ..RootMotionView::new()
        };
        v.accumulate(&delta(0.75, 0.0, 0.25));
        let off = v.grid_offset();
        assert!(off.x.is_finite() && off.z.is_finite(), "got {off:?}");
        assert!((off.x - 0.75).abs() < 1e-6, "unwrapped travel is returned");
    }

    #[test]
    fn reset_clears_travel() {
        let mut v = RootMotionView::new();
        v.accumulate(&delta(0.5, 0.0, 0.5));
        v.reset();
        assert_eq!(v.accumulated(), Vector3::ZERO);
        assert_eq!(v.grid_offset(), Vector3::ZERO);
    }
}

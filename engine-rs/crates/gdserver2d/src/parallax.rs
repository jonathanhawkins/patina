//! Parallax layer for scrolling background effects.
//!
//! A [`ParallaxLayer`] offsets its contents relative to camera movement,
//! scaled by [`motion_scale`](ParallaxLayer::motion_scale), creating a
//! depth-based scrolling effect.

use gdcore::math::Vector2;

/// A parallax scrolling layer.
///
/// When the camera moves by `delta`, this layer's effective offset is
/// `delta * motion_scale + motion_offset`, optionally wrapped by
/// [`motion_mirroring`](Self::motion_mirroring) for infinite scrolling.
#[derive(Debug, Clone, PartialEq)]
pub struct ParallaxLayer {
    /// Scale factor applied to camera movement. `(1, 1)` moves with the camera;
    /// `(0.5, 0.5)` moves at half speed (distant background).
    pub motion_scale: Vector2,
    /// Static offset applied in addition to the scaled camera movement.
    pub motion_offset: Vector2,
    /// Repeat period of the layer's contents, in pixels (Godot's
    /// `ParallaxLayer.motion_mirroring`).
    ///
    /// A non-zero axis wraps the computed offset into `[0, mirroring)` so the
    /// layer tiles forever; `0` (the default) disables mirroring on that axis.
    pub motion_mirroring: Vector2,
}

impl ParallaxLayer {
    /// Creates a new parallax layer with the given motion scale.
    pub fn new(motion_scale: Vector2) -> Self {
        Self {
            motion_scale,
            motion_offset: Vector2::ZERO,
            motion_mirroring: Vector2::ZERO,
        }
    }

    /// Computes the effective offset for this layer given a camera position.
    ///
    /// Equivalent to [`compute_offset_scaled`](Self::compute_offset_scaled)
    /// with a camera zoom of `1.0`.
    pub fn compute_offset(&self, camera_position: Vector2) -> Vector2 {
        self.compute_offset_scaled(camera_position, 1.0)
    }

    /// Computes the effective offset for a base offset and camera zoom,
    /// mirroring Godot's `ParallaxLayer::set_base_offset_and_scale`.
    ///
    /// The static `motion_offset` and the mirroring period both scale with
    /// `scale`, while the base offset is scaled by `motion_scale` — that is what
    /// keeps a mirrored layer seamless as the camera zooms.
    pub fn compute_offset_scaled(&self, base_offset: Vector2, scale: f32) -> Vector2 {
        let mut ofs = Vector2::new(
            base_offset.x * self.motion_scale.x + self.motion_offset.x * scale,
            base_offset.y * self.motion_scale.y + self.motion_offset.y * scale,
        );
        // ⚠️ Godot CLAMPS a negative mirroring to zero rather than wrapping by
        // it: `mirroring = p_mirroring.maxf(0)` in `ParallaxLayer::set_mirroring`.
        // `motion_mirroring` is a plain field here with no setter to clamp in,
        // so the clamp happens at the point of use.
        ofs.x = mirror_axis(ofs.x, self.motion_mirroring.x.max(0.0) * scale);
        ofs.y = mirror_axis(ofs.y, self.motion_mirroring.y.max(0.0) * scale);
        ofs
    }
}

/// Wraps `value` into `(-period, 0]` when `period` is positive; otherwise
/// returns `value` untouched (mirroring disabled on that axis).
///
/// Godot (`scene/2d/parallax_layer.cpp:120-127`):
/// ```text
/// real_t den = mirroring.x * p_scale;
/// new_ofs.x -= den * std::ceil(new_ofs.x / den);
/// ```
///
/// ⚠️ **CEIL, not floor.** This used to wrap with `floor`, into `[0, period)`,
/// and the comment beside it cited Godot as doing the same — it does not. The
/// two results are always exactly ONE PERIOD apart, so every periodicity test
/// passes under both: `f(x + period) == f(x)` holds for either, and that is what
/// the seamless-tiling tests assert. Only a test that pins the wrapped VALUE
/// tells them apart.
///
/// The sign is not cosmetic once the repeats are drawn. Godot's canvas repeats
/// extend in the POSITIVE direction from the item
/// (`renderer_canvas_cull.cpp:388-405`), so the wrapped item must sit at or just
/// LEFT of the origin for the copies to cover the screen to its right. Wrapping
/// into `[0, period)` puts it at or just right instead, leaving a gap of up to
/// one full period at the left edge — a seam that appears only once tile
/// emission lands (still open on this bead), which is why nothing has caught it.
fn mirror_axis(value: f32, period: f32) -> f32 {
    if period <= 0.0 || !period.is_finite() {
        return value;
    }
    value - period * (value / period).ceil()
}

impl Default for ParallaxLayer {
    fn default() -> Self {
        Self {
            motion_scale: Vector2::ONE,
            motion_offset: Vector2::ZERO,
            motion_mirroring: Vector2::ZERO,
        }
    }
}

/// Drives a set of [`ParallaxLayer`]s from a single scroll position, mirroring
/// Godot's `ParallaxBackground` (a CanvasLayer whose children are layers).
///
/// The background composes one base offset from the camera's scroll position
/// and hands it to each layer, which then applies its own motion scale and
/// mirroring.
#[derive(Debug, Clone, PartialEq)]
pub struct ParallaxBackground {
    /// Scroll position, normally driven by the active Camera2D.
    pub scroll_offset: Vector2,
    /// Constant offset added to the scroll position.
    pub scroll_base_offset: Vector2,
    /// Scale applied to [`scroll_offset`](Self::scroll_offset) before layers
    /// apply their own motion scale.
    pub scroll_base_scale: Vector2,
    /// When true, layers ignore the camera's zoom (Godot's
    /// `scroll_ignore_camera_zoom`).
    pub scroll_ignore_camera_zoom: bool,
}

/// Godot's `ParallaxBackground` scroll limits plus the viewport size they are
/// measured against — the state `ParallaxBackground::_update_scroll` reads out
/// of the node and its viewport.
///
/// Carried as a separate value rather than as fields on
/// [`ParallaxBackground`] because the viewport size is not node state: Godot
/// reads it from `get_viewport_size()` at scroll time, and a background that
/// cached it would go stale on every window resize.
///
/// [`Default`] is Godot's default — `limit_begin == limit_end == (0, 0)`, which
/// DISABLES clamping on both axes (see [`ParallaxBackground::clamped_base_offset`]).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ScrollLimits {
    /// Godot's `scroll_limit_begin`: the smallest scroll position allowed.
    pub begin: Vector2,
    /// Godot's `scroll_limit_end`: the largest scroll position allowed, measured
    /// at the FAR edge of the viewport rather than its origin.
    pub end: Vector2,
    /// The viewport size the limits are measured against, Godot's
    /// `get_viewport_size()`.
    pub viewport_size: Vector2,
}

impl ParallaxBackground {
    /// Creates a background with Godot's defaults (no offset, unit base scale).
    pub fn new() -> Self {
        Self {
            scroll_offset: Vector2::ZERO,
            scroll_base_offset: Vector2::ZERO,
            scroll_base_scale: Vector2::ONE,
            scroll_ignore_camera_zoom: false,
        }
    }

    /// The base offset handed to every child layer, mirroring Godot's
    /// `ParallaxBackground::_update_scroll`.
    pub fn base_offset(&self) -> Vector2 {
        Vector2::new(
            self.scroll_base_offset.x + self.scroll_offset.x * self.scroll_base_scale.x,
            self.scroll_base_offset.y + self.scroll_offset.y * self.scroll_base_scale.y,
        )
    }

    /// The offset `layer` should be drawn at for the current scroll position and
    /// camera `zoom`. Honors [`scroll_ignore_camera_zoom`](Self::scroll_ignore_camera_zoom).
    ///
    /// Equivalent to [`layer_offset_with_screen`](Self::layer_offset_with_screen)
    /// with a ZERO screen offset, which is Godot's `ANCHOR_MODE_FIXED_TOP_LEFT`
    /// case. A camera using the default `ANCHOR_MODE_DRAG_CENTER` must use that
    /// method instead and pass `screen_size * 0.5`.
    pub fn layer_offset(&self, layer: &ParallaxLayer, zoom: f32) -> Vector2 {
        self.layer_offset_with_screen(layer, zoom, Vector2::ZERO)
    }

    /// [`layer_offset`](Self::layer_offset) with Godot's `screen_offset` term.
    ///
    /// Godot (`scene/2d/parallax_background.cpp:104-108`):
    /// ```text
    /// if (ignore_camera_zoom)
    ///     set_base_offset_and_scale((scroll_ofs + screen_offset * (scale - 1)) / scale, 1.0);
    /// else
    ///     set_base_offset_and_scale(scroll_ofs, scale);
    /// ```
    ///
    /// ⚠️ The ignore-zoom branch is NOT "pass the offset through with scale 1".
    /// It divides by the zoom and adds `screen_offset * (scale - 1)`, so that the
    /// layer stays visually pinned as the camera zooms instead of drifting. This
    /// implementation previously kept only the `1.0` scale and dropped both
    /// terms, which is exact at `zoom == 1` and wrong at every other zoom —
    /// invisible to any test that never zooms.
    ///
    /// ⚠️ `screen_offset` is `screen_size * 0.5` under Godot's default
    /// `ANCHOR_MODE_DRAG_CENTER` and `Point2()` under `ANCHOR_MODE_FIXED_TOP_LEFT`
    /// (`scene/2d/camera_2d.cpp:62`) — it is NOT the camera position and NOT the
    /// view's world-space top-left, which is a different quantity used by the
    /// standalone `Parallax2D` node.
    ///
    /// A `zoom` of 0 would divide by zero; Godot has the same division and no
    /// guard, and a Camera2D cannot reach zero zoom, so this mirrors it rather
    /// than inventing a fallback Godot does not have.
    pub fn layer_offset_with_screen(
        &self,
        layer: &ParallaxLayer,
        zoom: f32,
        screen_offset: Vector2,
    ) -> Vector2 {
        self.layer_offset_limited(layer, zoom, screen_offset, &ScrollLimits::default())
    }

    /// [`layer_offset_with_screen`](Self::layer_offset_with_screen) with Godot's
    /// `scroll_limit_begin` / `scroll_limit_end` clamp applied to the scroll
    /// position first.
    ///
    /// The limits bound the SCROLL POSITION, not each layer's offset — a layer
    /// with a small `motion_scale` still moves a small fraction of the clamped
    /// range rather than stopping at the limit itself.
    pub fn layer_offset_limited(
        &self,
        layer: &ParallaxLayer,
        zoom: f32,
        screen_offset: Vector2,
        limits: &ScrollLimits,
    ) -> Vector2 {
        let base = self.clamped_base_offset(limits);
        if self.scroll_ignore_camera_zoom {
            let adjusted = Vector2::new(
                (base.x + screen_offset.x * (zoom - 1.0)) / zoom,
                (base.y + screen_offset.y * (zoom - 1.0)) / zoom,
            );
            return layer.compute_offset_scaled(adjusted, 1.0);
        }
        layer.compute_offset_scaled(base, zoom)
    }

    /// [`base_offset`](Self::base_offset) with `limits` applied — Godot's
    /// `final_offset`, the value `get_final_offset()` returns.
    ///
    /// Godot (`scene/2d/parallax_background.cpp:78-95`):
    /// ```text
    /// scroll_ofs = -scroll_ofs;
    /// if (limit_begin.x < limit_end.x) {
    ///     if (scroll_ofs.x < limit_begin.x)             scroll_ofs.x = limit_begin.x;
    ///     else if (scroll_ofs.x + vps.x > limit_end.x)  scroll_ofs.x = limit_end.x - vps.x;
    /// }
    /// ... same for y ...
    /// scroll_ofs = -scroll_ofs;
    /// ```
    ///
    /// ⚠️ TWO details that a direct-looking port gets wrong.
    ///
    /// 1. The clamp runs on the NEGATED offset and is negated back afterwards.
    ///    The scroll offset moves opposite to the camera, so clamping it
    ///    directly would apply `begin` where `end` belongs — the limits would
    ///    hold at the wrong end of the level and still look like they worked.
    /// 2. Each axis is clamped only `if begin < end`. Equal limits — the default
    ///    `(0, 0)` on both — DISABLE that axis rather than pinning the scroll to
    ///    zero, so an unconfigured background scrolls freely. Treating the
    ///    defaults as an active `0..0` range would freeze every parallax
    ///    background in the project.
    ///
    /// The upper bound is `end - viewport_size`, i.e. `end` is where the far
    /// edge of the view stops, not its origin.
    pub fn clamped_base_offset(&self, limits: &ScrollLimits) -> Vector2 {
        let base = self.base_offset();
        let mut scroll = Vector2::new(-base.x, -base.y);
        let vps = limits.viewport_size;
        if limits.begin.x < limits.end.x {
            if scroll.x < limits.begin.x {
                scroll.x = limits.begin.x;
            } else if scroll.x + vps.x > limits.end.x {
                scroll.x = limits.end.x - vps.x;
            }
        }
        if limits.begin.y < limits.end.y {
            if scroll.y < limits.begin.y {
                scroll.y = limits.begin.y;
            } else if scroll.y + vps.y > limits.end.y {
                scroll.y = limits.end.y - vps.y;
            }
        }
        Vector2::new(-scroll.x, -scroll.y)
    }
}

impl Default for ParallaxBackground {
    fn default() -> Self {
        Self::new()
    }
}

/// Godot's standalone `Parallax2D` (4.3+), the replacement for the
/// `ParallaxBackground`/`ParallaxLayer` pair.
///
/// ⚠️ **This is NOT a variant of `ParallaxBackground`, and copying that node's
/// reasoning across gets every sign wrong.** `ParallaxBackground` is a
/// `CanvasLayer`, so its children are screen-fixed and their offset is the only
/// thing that moves them. `Parallax2D` is a `Node2D` — the camera transform DOES
/// apply to it — so it scrolls by being placed in the WORLD, and its formula is
/// correspondingly inverted (`screen_offset + scroll_offset - scroll_ofs`).
///
/// Three more differences that a shared mental model would paper over:
///
/// * its `screen_offset` is the view's world-space TOP-LEFT
///   (`camera_screen_center - screen_size * 0.5`, Godot's `adj_screen_pos`), not
///   the half-viewport `ParallaxBackground` receives;
/// * the limits are a plain `CLAMP` with no negation, guarded by
///   `limit_begin <= limit_end - viewport`, and their defaults of `±1e7` leave
///   the clamp ACTIVE — where `ParallaxBackground`'s equal `(0, 0)` defaults
///   disable it;
/// * `scroll_scale` is a `Vector2` per-axis rate, not a camera zoom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parallax2D {
    /// Per-axis scroll rate. `1` moves with the world, `0` pins to the screen —
    /// the OPPOSITE endpoints from `ParallaxLayer::motion_scale`, because this
    /// node is placed in world space rather than on a screen-fixed layer.
    pub scroll_scale: Vector2,
    /// Constant offset added to the resting position, and the accumulator
    /// [`autoscroll`](Self::autoscroll) advances.
    pub scroll_offset: Vector2,
    /// Repeat period; zero disables repeating on that axis.
    pub repeat_size: Vector2,
    /// How many copies the renderer draws either side. Carried for parity — the
    /// scroll maths does not read it.
    pub repeat_times: i32,
    /// Units per second added to `scroll_offset` by
    /// [`advance_autoscroll`](Self::advance_autoscroll).
    pub autoscroll: Vector2,
    /// World-space lower bound for the view's top-left.
    pub limit_begin: Vector2,
    /// World-space upper bound, measured at the FAR edge of the viewport.
    pub limit_end: Vector2,
    /// When false, the node is placed relative to the world origin instead of
    /// the viewport.
    pub follow_viewport: bool,
    /// Freezes [`screen_offset`](Self::screen_offset) against camera movement.
    pub ignore_camera_scroll: bool,
    /// The view's world-space top-left, normally written by the camera through
    /// [`follow_camera`](Self::follow_camera).
    pub screen_offset: Vector2,
}

impl Parallax2D {
    /// Godot's defaults (`doc/classes/Parallax2D.xml`).
    ///
    /// ⚠️ The limits default to `±1e7`, NOT to zero: the clamp is on out of the
    /// box and merely too wide to reach. Defaulting them to zero would freeze
    /// every Parallax2D in a project.
    pub fn new() -> Self {
        Self {
            scroll_scale: Vector2::ONE,
            scroll_offset: Vector2::ZERO,
            repeat_size: Vector2::ZERO,
            repeat_times: 1,
            autoscroll: Vector2::ZERO,
            limit_begin: Vector2::new(-1.0e7, -1.0e7),
            limit_end: Vector2::new(1.0e7, 1.0e7),
            follow_viewport: true,
            ignore_camera_scroll: false,
            screen_offset: Vector2::ZERO,
        }
    }

    /// Records the camera's view, mirroring `Parallax2D::_camera_moved`.
    ///
    /// `adj_screen_pos` is Godot's third `_camera_moved` argument,
    /// `camera_screen_center - screen_size * 0.5` (`camera_2d.cpp:63`) — the
    /// view's world-space top-left. `ParallaxBackground` ignores that argument
    /// and uses the second one; this node is the only consumer of it.
    ///
    /// Godot additionally floors the value when the viewport has
    /// `snap_2d_transforms_to_pixel` enabled. That is a project setting this
    /// port has no equivalent of, so the un-snapped branch is what is
    /// implemented; the snapped one differs by at most half a pixel.
    pub fn follow_camera(&mut self, adj_screen_pos: Vector2) {
        if !self.ignore_camera_scroll {
            self.screen_offset = adj_screen_pos;
        }
    }

    /// Advances `scroll_offset` by [`autoscroll`](Self::autoscroll), wrapping it
    /// into one repeat period — Godot's `NOTIFICATION_INTERNAL_PROCESS`.
    ///
    /// ⚠️ **AUTOSCROLL DOES NOTHING WITHOUT A `repeat_size`.** Godot enables the
    /// whole notification only when BOTH are set
    /// (`_update_process`: `(repeat_size.x || repeat_size.y) && (autoscroll.x ||
    /// autoscroll.y)`), which is easy to miss because the two properties are
    /// independent everywhere else. Running the step regardless would be worse
    /// than a no-op in the other direction too: the wrap would silently fold an
    /// AUTHORED `scroll_offset` into one period even when nothing is scrolling,
    /// so a background offset of 300 with a 256 period would jump to 44 on the
    /// first frame.
    ///
    /// The wrap keeps the accumulator bounded; without it a long-running
    /// autoscroll loses f32 precision as the value grows, and the drift is
    /// invisible for the first several minutes.
    pub fn advance_autoscroll(&mut self, delta: f32) {
        let repeats = self.repeat_size.x != 0.0 || self.repeat_size.y != 0.0;
        let scrolls = self.autoscroll.x != 0.0 || self.autoscroll.y != 0.0;
        if !repeats || !scrolls {
            return;
        }
        let mut offset = Vector2::new(
            self.scroll_offset.x + self.autoscroll.x * delta,
            self.scroll_offset.y + self.autoscroll.y * delta,
        );
        if self.repeat_size.x != 0.0 {
            offset.x = fposmod(offset.x, self.repeat_size.x);
        }
        if self.repeat_size.y != 0.0 {
            offset.y = fposmod(offset.y, self.repeat_size.y);
        }
        self.scroll_offset = offset;
    }

    /// The world-space `position` this node should be drawn at, mirroring
    /// `Parallax2D::_update_scroll` (`scene/2d/parallax_2d.cpp:100-141`).
    ///
    /// `node_scale` is the node's own `scale`, which stretches the repeat period
    /// — it is NOT the camera zoom.
    ///
    /// ```text
    /// scroll_ofs = screen_offset
    /// if (limit_begin.x <= limit_end.x - vps.x) CLAMP(scroll_ofs.x, limit_begin.x, limit_end.x - vps.x)
    /// scroll_ofs *= scroll_scale
    /// if (repeat_size.x) scroll_ofs.x = screen_offset.x - fposmod(scroll_ofs.x - scroll_offset.x, repeat_size.x * scale.x)
    /// else               scroll_ofs.x = screen_offset.x + scroll_offset.x - scroll_ofs.x
    /// if (!follow_viewport) scroll_ofs -= screen_offset
    /// ```
    ///
    /// ⚠️ The clamp runs BEFORE the multiply by `scroll_scale`, and the guard is
    /// `begin <= end - viewport`, not `begin < end`: an axis whose limits are
    /// closer together than the viewport is left UNCLAMPED rather than pinned,
    /// so a too-small level scrolls freely instead of freezing.
    pub fn position(&self, viewport_size: Vector2, node_scale: Vector2) -> Vector2 {
        let mut scroll = self.screen_offset;
        if self.limit_begin.x <= self.limit_end.x - viewport_size.x {
            scroll.x = scroll
                .x
                .clamp(self.limit_begin.x, self.limit_end.x - viewport_size.x);
        }
        if self.limit_begin.y <= self.limit_end.y - viewport_size.y {
            scroll.y = scroll
                .y
                .clamp(self.limit_begin.y, self.limit_end.y - viewport_size.y);
        }
        scroll = Vector2::new(
            scroll.x * self.scroll_scale.x,
            scroll.y * self.scroll_scale.y,
        );

        scroll.x = repeat_axis(
            scroll.x,
            self.screen_offset.x,
            self.scroll_offset.x,
            self.repeat_size.x * node_scale.x,
        );
        scroll.y = repeat_axis(
            scroll.y,
            self.screen_offset.y,
            self.scroll_offset.y,
            self.repeat_size.y * node_scale.y,
        );

        if !self.follow_viewport {
            scroll = Vector2::new(
                scroll.x - self.screen_offset.x,
                scroll.y - self.screen_offset.y,
            );
        }
        scroll
    }
}

impl Default for Parallax2D {
    fn default() -> Self {
        Self::new()
    }
}

/// One axis of `Parallax2D::_update_scroll`'s repeat step.
///
/// ⚠️ A zero `period` here means "no repeat", which is Godot's `if
/// (repeat_size.x)` guard — but Godot tests the UNSCALED `repeat_size` and then
/// divides by `repeat_size * scale`, so a node scaled to zero on an axis makes
/// it divide by zero and hand a NaN to `set_position`. This tests the scaled
/// denominator instead: a NaN position propagates into the transform and cannot
/// be recovered from, whereas a zero-scaled node is a legitimate authored state.
fn repeat_axis(scrolled: f32, screen_offset: f32, scroll_offset: f32, period: f32) -> f32 {
    if period == 0.0 || !period.is_finite() {
        return screen_offset + scroll_offset - scrolled;
    }
    screen_offset - fposmod(scrolled - scroll_offset, period)
}

/// Godot's `Math::fposmod`: the remainder takes the sign of `y`, so the result
/// lands in `[0, y)` for a positive `y`.
///
/// ⚠️ Rust's `%` is a truncating remainder that takes the sign of the DIVIDEND,
/// so `-1.0 % 256.0` is `-1.0` where this returns `255.0`. Using `%` directly
/// puts a scrolling background one full period out of place, but only while the
/// argument is negative — which is exactly half the time and none of the obvious
/// fixtures.
fn fposmod(x: f32, y: f32) -> f32 {
    let value = x % y;
    if (value < 0.0 && y > 0.0) || (value > 0.0 && y < 0.0) {
        value + y
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallax_default_moves_with_camera() {
        let layer = ParallaxLayer::default();
        let offset = layer.compute_offset(Vector2::new(100.0, 50.0));
        assert!((offset.x - 100.0).abs() < 1e-6);
        assert!((offset.y - 50.0).abs() < 1e-6);
    }

    #[test]
    fn parallax_half_speed() {
        let layer = ParallaxLayer::new(Vector2::new(0.5, 0.5));
        let offset = layer.compute_offset(Vector2::new(100.0, 200.0));
        assert!((offset.x - 50.0).abs() < 1e-6);
        assert!((offset.y - 100.0).abs() < 1e-6);
    }

    #[test]
    fn parallax_with_static_offset() {
        let mut layer = ParallaxLayer::new(Vector2::new(0.5, 0.5));
        layer.motion_offset = Vector2::new(10.0, 20.0);
        let offset = layer.compute_offset(Vector2::new(100.0, 200.0));
        assert!((offset.x - 60.0).abs() < 1e-6);
        assert!((offset.y - 120.0).abs() < 1e-6);
    }

    #[test]
    fn parallax_zero_scale_stays_fixed() {
        let layer = ParallaxLayer::new(Vector2::ZERO);
        let offset = layer.compute_offset(Vector2::new(999.0, 999.0));
        assert!((offset.x).abs() < 1e-6);
        assert!((offset.y).abs() < 1e-6);
    }

    // -- pat-u3l8c.31: motion mirroring + ParallaxBackground ------------------

    /// Mirroring is off by default, so the offset grows without bound exactly as
    /// it did before this bead.
    #[test]
    fn parallax_without_mirroring_does_not_wrap() {
        let layer = ParallaxLayer::default();
        assert_eq!(layer.motion_mirroring, Vector2::ZERO);
        let offset = layer.compute_offset(Vector2::new(5000.0, 0.0));
        assert!((offset.x - 5000.0).abs() < 1e-3);
    }

    /// The headline of the bead's parallax acceptance: a mirrored layer scrolls
    /// infinitely. Moving the camera by exactly one repeat period must land on
    /// the identical offset — that is what makes the tiling seamless.
    #[test]
    fn parallax_mirroring_repeats_seamlessly() {
        let mut layer = ParallaxLayer::new(Vector2::new(0.5, 1.0));
        layer.motion_mirroring = Vector2::new(256.0, 0.0);

        // motion_scale.x = 0.5, so one 256px period takes 512px of camera travel.
        let a = layer.compute_offset(Vector2::new(100.0, 0.0));
        let b = layer.compute_offset(Vector2::new(100.0 + 512.0, 0.0));
        let c = layer.compute_offset(Vector2::new(100.0 - 512.0, 0.0));
        assert!(
            (a.x - b.x).abs() < 1e-3,
            "one period of camera travel must repeat the layer offset ({} vs {})",
            a.x,
            b.x,
        );
        assert!(
            (a.x - c.x).abs() < 1e-3,
            "scrolling backwards must repeat too ({} vs {})",
            a.x,
            c.x,
        );
    }

    /// A mirrored offset always folds into `(-period, 0]`, including for negative
    /// camera positions — Godot wraps with `ceil`, so the wrapped item sits at
    /// or just LEFT of the origin and its repeated copies cover the screen to
    /// the right.
    ///
    /// ⚠️ This test previously asserted `[0, period)` — the range a `floor` wrap
    /// gives, which is what this module used to do while citing Godot as the
    /// source. The two differ by exactly one period, so the seamless-tiling test
    /// below passes either way; this is the assertion that separates them, and
    /// the reason it now reads the other interval is that the CODE was wrong,
    /// not this fixture.
    #[test]
    fn parallax_mirroring_wraps_into_period_even_when_negative() {
        let mut layer = ParallaxLayer::new(Vector2::ONE);
        layer.motion_mirroring = Vector2::new(100.0, 50.0);

        for camera_x in [-350.0f32, -1.0, 0.0, 40.0, 250.0, 1000.0] {
            let ofs = layer.compute_offset(Vector2::new(camera_x, -37.0));
            assert!(
                ofs.x > -100.0 && ofs.x <= 0.0,
                "x offset {} must fold into (-100, 0] for camera {}",
                ofs.x,
                camera_x,
            );
            assert!(
                ofs.y > -50.0 && ofs.y <= 0.0,
                "y offset {} must fold into (-50, 0]",
                ofs.y,
            );
        }
        // Both ends of a period land on the SAME point of the tiling, so the
        // boundary is pinned explicitly: an exact multiple wraps to 0, not to
        // -period.
        let ofs = layer.compute_offset(Vector2::new(1000.0, 0.0));
        assert!((ofs.x).abs() < 1e-3, "expected 0, got {}", ofs.x);
        // -350 with scale 1 wraps to -50 (a truncating % would give +50 here).
        let ofs = layer.compute_offset(Vector2::new(-350.0, 0.0));
        assert!((ofs.x + 50.0).abs() < 1e-3, "expected -50, got {}", ofs.x);
    }

    /// ⚠️ A NEGATIVE `motion_mirroring` DISABLES the axis; it does not wrap by
    /// the absolute value.
    ///
    /// Godot clamps in the setter (`mirroring = p_mirroring.maxf(0)`), which
    /// this port has no equivalent of — `motion_mirroring` is a plain field, so
    /// a `.tscn` with a negative value reaches the maths directly. Without the
    /// clamp the offset wrapped into `(period, 0]` with a negative period, i.e.
    /// the wrong side of the origin entirely.
    #[test]
    fn parallax_negative_mirroring_is_treated_as_disabled() {
        let mut layer = ParallaxLayer::new(Vector2::ONE);
        layer.motion_mirroring = Vector2::new(-100.0, -50.0);

        let ofs = layer.compute_offset(Vector2::new(250.0, 250.0));
        assert!(
            (ofs.x - 250.0).abs() < 1e-3,
            "x must not wrap, got {}",
            ofs.x
        );
        assert!(
            (ofs.y - 250.0).abs() < 1e-3,
            "y must not wrap, got {}",
            ofs.y
        );
    }

    /// Mirroring only applies per-axis: a zero axis keeps scrolling freely.
    #[test]
    fn parallax_mirroring_is_per_axis() {
        let mut layer = ParallaxLayer::new(Vector2::ONE);
        layer.motion_mirroring = Vector2::new(100.0, 0.0);
        let ofs = layer.compute_offset(Vector2::new(250.0, 250.0));
        assert!((ofs.x + 50.0).abs() < 1e-3, "x wraps, got {}", ofs.x);
        assert!(
            (ofs.y - 250.0).abs() < 1e-3,
            "y is unmirrored, got {}",
            ofs.y
        );
    }

    /// The background composes scroll_offset * base_scale + base_offset and
    /// hands that to the layer, which applies its own motion scale.
    #[test]
    fn background_composes_base_offset_for_layers() {
        let mut bg = ParallaxBackground::new();
        bg.scroll_offset = Vector2::new(100.0, 40.0);
        bg.scroll_base_offset = Vector2::new(10.0, 5.0);
        bg.scroll_base_scale = Vector2::new(2.0, 1.0);
        // base = 10 + 100*2 = 210 (x), 5 + 40*1 = 45 (y)
        assert!((bg.base_offset().x - 210.0).abs() < 1e-3);
        assert!((bg.base_offset().y - 45.0).abs() < 1e-3);

        let layer = ParallaxLayer::new(Vector2::new(0.5, 0.5));
        let ofs = bg.layer_offset(&layer, 1.0);
        assert!((ofs.x - 105.0).abs() < 1e-3, "210 * 0.5, got {}", ofs.x);
        assert!((ofs.y - 22.5).abs() < 1e-3, "45 * 0.5, got {}", ofs.y);
    }

    /// `scroll_ignore_camera_zoom` pins the layer scale to 1, so a zoomed camera
    /// does not stretch the layer's static offset or its mirroring period — AND
    /// it divides the scroll position by the zoom.
    ///
    /// ⚠️ **THIS TEST USED TO ASSERT 120, AND 120 WAS THE BUG.** It was written
    /// against an implementation that read the ignore-zoom branch as "pass the
    /// offset through with scale 1" and kept only the `1.0`. Godot's branch is
    /// `(scroll_ofs + screen_offset * (scale - 1)) / scale`
    /// (`parallax_background.cpp:104-108`), so the base is DIVIDED by the zoom
    /// as well: `100 / 2 + 20 * 1 = 70`.
    ///
    /// The two readings agree exactly at `zoom == 1`, which is why the defect
    /// survived — and this fixture, the only one in the module that zooms, had
    /// the wrong number baked into it, so it locked the defect in rather than
    /// catching it. The 120 is gone because the CODE was wrong, not this
    /// arithmetic.
    ///
    /// `layer_offset` uses a ZERO screen offset (Godot's
    /// `ANCHOR_MODE_FIXED_TOP_LEFT`), so the `screen_offset * (scale - 1)` term
    /// drops out here; `parallax_ignore_camera_zoom.rs` covers the non-zero case.
    #[test]
    fn background_ignore_camera_zoom_pins_scale() {
        let mut layer = ParallaxLayer::new(Vector2::ONE);
        layer.motion_offset = Vector2::new(20.0, 0.0);

        let mut bg = ParallaxBackground::new();
        bg.scroll_offset = Vector2::new(100.0, 0.0);

        // Following the zoom scales the static motion_offset: 100 + 20*2 = 140.
        let zoomed = bg.layer_offset(&layer, 2.0);
        assert!((zoomed.x - 140.0).abs() < 1e-3, "got {}", zoomed.x);

        // Ignoring it divides the base and pins scale to 1: 100/2 + 20*1 = 70.
        bg.scroll_ignore_camera_zoom = true;
        let pinned = bg.layer_offset(&layer, 2.0);
        assert!((pinned.x - 70.0).abs() < 1e-3, "got {}", pinned.x);

        // The part the old fixture was actually reaching for, isolated: the
        // static offset is NOT stretched by the zoom. With motion_offset zeroed
        // the ignore branch is the bare division, 100/2 = 50, so the 20 above is
        // unmistakably contributing at scale 1 rather than at scale 2.
        layer.motion_offset = Vector2::ZERO;
        let bare = bg.layer_offset(&layer, 2.0);
        assert!((bare.x - 50.0).abs() < 1e-3, "got {}", bare.x);
    }

    /// Zoom scales the mirroring period too, so a zoomed mirrored layer still
    /// tiles seamlessly (period 100 at zoom 2 repeats every 200 units).
    #[test]
    fn background_zoom_scales_mirroring_period() {
        let mut layer = ParallaxLayer::new(Vector2::ONE);
        layer.motion_mirroring = Vector2::new(100.0, 0.0);

        let mut bg = ParallaxBackground::new();
        bg.scroll_offset = Vector2::new(30.0, 0.0);
        let a = bg.layer_offset(&layer, 2.0);

        bg.scroll_offset = Vector2::new(30.0 + 200.0, 0.0);
        let b = bg.layer_offset(&layer, 2.0);
        assert!(
            (a.x - b.x).abs() < 1e-3,
            "at zoom 2 the layer must repeat every 200 units ({} vs {})",
            a.x,
            b.x,
        );
    }
    // -- pat-s3rrv.86 item (2): the standalone Parallax2D node ----------------

    fn p2d() -> Parallax2D {
        Parallax2D::new()
    }

    const VPS: Vector2 = Vector2 {
        x: 1152.0,
        y: 648.0,
    };

    fn at(node: &Parallax2D, screen_offset: Vector2) -> Vector2 {
        let mut n = *node;
        n.follow_camera(screen_offset);
        n.position(VPS, Vector2::ONE)
    }

    /// 🔑 `scroll_scale` RUNS THE OPPOSITE WAY FROM `ParallaxLayer::motion_scale`.
    ///
    /// `1` leaves the node at a FIXED WORLD POSITION — it moves with the world,
    /// at the full camera rate — and `0` pins it to the screen. On a
    /// ParallaxLayer those endpoints are reversed, because that node lives on a
    /// screen-fixed CanvasLayer. Getting this backwards inverts every background
    /// in a project, so both endpoints are pinned rather than a midpoint.
    #[test]
    fn parallax2d_scroll_scale_endpoints_are_world_and_screen() {
        let mut full = p2d();
        full.scroll_scale = Vector2::ONE;
        let a = at(&full, Vector2::new(500.0, 300.0));
        let b = at(&full, Vector2::new(1000.0, 600.0));
        assert!(
            a == Vector2::ZERO && b == Vector2::ZERO,
            "scroll_scale 1 must hold a fixed world position, got {a:?} and {b:?}"
        );

        let mut pinned = p2d();
        pinned.scroll_scale = Vector2::ZERO;
        let screen = Vector2::new(500.0, 300.0);
        assert_eq!(
            at(&pinned, screen),
            screen,
            "scroll_scale 0 must sit exactly at the view's top-left, i.e. pinned"
        );
    }

    /// The half-rate case, which is what the node is actually for.
    #[test]
    fn parallax2d_half_scroll_scale_tracks_half_the_view() {
        let mut node = p2d();
        node.scroll_scale = Vector2::new(0.5, 0.5);
        assert_eq!(
            at(&node, Vector2::new(500.0, 300.0)),
            Vector2::new(250.0, 150.0)
        );
        assert_eq!(
            at(&node, Vector2::new(1000.0, 600.0)),
            Vector2::new(500.0, 300.0)
        );
    }

    /// `scroll_offset` is a constant displacement, and `follow_viewport` removes
    /// the view term entirely.
    #[test]
    fn parallax2d_scroll_offset_and_follow_viewport() {
        let mut offset = p2d();
        offset.scroll_offset = Vector2::new(50.0, 20.0);
        assert_eq!(
            at(&offset, Vector2::new(500.0, 300.0)),
            Vector2::new(50.0, 20.0),
            "at scroll_scale 1 the node rests at scroll_offset"
        );

        let mut detached = p2d();
        detached.scroll_scale = Vector2::new(0.5, 0.5);
        detached.follow_viewport = false;
        assert_eq!(
            at(&detached, Vector2::new(500.0, 300.0)),
            Vector2::new(-250.0, -150.0),
            "follow_viewport = false subtracts the view offset"
        );
    }

    /// ⚠️ THE LIMIT GUARD IS `begin <= end - viewport`, NOT `begin <= end`.
    ///
    /// A level narrower than the viewport is left UNCLAMPED rather than pinned,
    /// so it scrolls freely instead of freezing. With limits `0..1000` against a
    /// 1152px viewport the correct answer is the unlimited one (2500); the
    /// `begin <= end` reading gives 5000.
    #[test]
    fn parallax2d_limits_narrower_than_the_viewport_are_ignored() {
        let mut node = p2d();
        node.scroll_scale = Vector2::new(0.5, 0.5);
        node.limit_begin = Vector2::new(0.0, -1.0e7);
        node.limit_end = Vector2::new(1000.0, 1.0e7);

        let mut unlimited = p2d();
        unlimited.scroll_scale = Vector2::new(0.5, 0.5);

        let screen = Vector2::new(5000.0, 0.0);
        assert_eq!(
            at(&node, screen).x,
            at(&unlimited, screen).x,
            "limits closer together than the viewport must not clamp"
        );
        assert!((at(&node, screen).x - 2500.0).abs() < 1e-3);
    }

    /// Past its limits the node is pinned to the SCREEN, not to the world.
    ///
    /// Stated as the screen-relative offset holding constant, because that is
    /// the visible effect: the background stops moving relative to the player
    /// while its world position keeps tracking the camera.
    #[test]
    fn parallax2d_past_its_limits_the_node_holds_still_on_screen() {
        let mut node = p2d();
        node.scroll_scale = Vector2::new(0.5, 0.5);
        node.limit_begin = Vector2::new(0.0, -1.0e7);
        node.limit_end = Vector2::new(2000.0, 1.0e7);

        let near = Vector2::new(5000.0, 0.0);
        let far = Vector2::new(9000.0, 0.0);
        assert!((at(&node, near).x - near.x + 424.0).abs() < 1e-3);
        assert!((at(&node, far).x - far.x + 424.0).abs() < 1e-3);
    }

    /// ⚠️ THE REPEAT WRAP NEEDS `fposmod`, NOT RUST'S `%`.
    ///
    /// Rust's remainder takes the sign of the dividend, so it agrees with Godot
    /// for a positive argument and is one full period out for a negative one —
    /// i.e. it is correct exactly while the camera stays right of the origin,
    /// and every obvious fixture does. Here the screen-relative offset must be
    /// -6; a plain `%` gives +250.
    #[test]
    fn parallax2d_repeat_wraps_with_a_positive_modulo() {
        let mut node = p2d();
        node.scroll_scale = Vector2::new(0.5, 1.0);
        node.repeat_size = Vector2::new(256.0, 0.0);

        let screen = Vector2::new(-500.0, 0.0);
        assert!(
            (at(&node, screen).x - screen.x + 6.0).abs() < 1e-3,
            "expected a screen-relative -6, got {}",
            at(&node, screen).x - screen.x
        );
    }

    /// One repeat period of camera travel returns the identical screen-relative
    /// position — the seamless-tiling property.
    ///
    /// At `scroll_scale` 0.5 a 256px period takes 512px of view travel, so the
    /// fixture cannot be satisfied by ignoring `scroll_scale`.
    #[test]
    fn parallax2d_repeat_is_seamless_over_one_period() {
        let mut node = p2d();
        node.scroll_scale = Vector2::new(0.5, 1.0);
        node.repeat_size = Vector2::new(256.0, 0.0);

        let a = Vector2::new(500.0, 0.0);
        let b = Vector2::new(1012.0, 0.0);
        assert!(
            ((at(&node, a).x - a.x) - (at(&node, b).x - b.x)).abs() < 1e-3,
            "512px of view travel must repeat: {} vs {}",
            at(&node, a).x - a.x,
            at(&node, b).x - b.x
        );
    }

    /// The repeat period is stretched by the NODE's scale, not by any zoom.
    #[test]
    fn parallax2d_node_scale_stretches_the_repeat_period() {
        let mut node = p2d();
        node.scroll_scale = Vector2::new(0.5, 1.0);
        node.repeat_size = Vector2::new(256.0, 0.0);
        node.follow_camera(Vector2::new(1524.0, 0.0));

        // Period 256 * scale 2 = 512, which takes 1024px of view travel.
        assert!((node.position(VPS, Vector2::new(2.0, 1.0)).x - 1274.0).abs() < 1e-3);
    }

    /// A node scaled to zero on an axis must not produce a NaN position.
    ///
    /// Godot tests the UNSCALED `repeat_size` and then divides by
    /// `repeat_size * scale`, so this input hands `set_position` a NaN there. A
    /// NaN propagates into the transform and cannot be recovered from, while a
    /// zero-scaled node is a legitimate authored state, so this port falls back
    /// to the un-repeated branch instead.
    #[test]
    fn parallax2d_zero_node_scale_does_not_produce_nan() {
        let mut node = p2d();
        node.scroll_scale = Vector2::new(0.5, 1.0);
        node.repeat_size = Vector2::new(256.0, 0.0);
        node.follow_camera(Vector2::new(500.0, 0.0));

        let p = node.position(VPS, Vector2::new(0.0, 1.0));
        assert!(p.x.is_finite(), "position must stay finite, got {}", p.x);
        assert!((p.x - 250.0).abs() < 1e-3);
    }

    /// `ignore_camera_scroll` freezes the recorded view.
    #[test]
    fn parallax2d_ignore_camera_scroll_freezes_the_view() {
        let mut node = p2d();
        node.scroll_scale = Vector2::new(0.5, 0.5);
        node.ignore_camera_scroll = true;
        node.follow_camera(Vector2::new(5000.0, 5000.0));
        assert_eq!(node.screen_offset, Vector2::ZERO);
    }

    /// Autoscroll advances `scroll_offset` and wraps it into one period.
    ///
    /// ⚠️ The wrap is what keeps the accumulator bounded; without it a
    /// long-running autoscroll drifts as f32 precision degrades, which no short
    /// test would ever reach.
    #[test]
    fn parallax2d_autoscroll_advances_and_wraps() {
        let mut node = p2d();
        node.autoscroll = Vector2::new(60.0, 0.0);
        node.repeat_size = Vector2::new(256.0, 0.0);

        node.advance_autoscroll(0.5);
        assert!((node.scroll_offset.x - 30.0).abs() < 1e-3);

        for _ in 0..100 {
            node.advance_autoscroll(0.5);
        }
        assert!(
            node.scroll_offset.x >= 0.0 && node.scroll_offset.x < 256.0,
            "autoscroll must stay inside one period, got {}",
            node.scroll_offset.x
        );

        // ⚠️ Godot gates the whole autoscroll notification on BOTH a repeat
        // size and an autoscroll being set, so an autoscroll with no repeat
        // period does not move at all — it is not merely unwrapped.
        let mut no_period = p2d();
        no_period.autoscroll = Vector2::new(60.0, 0.0);
        no_period.advance_autoscroll(10.0);
        assert_eq!(
            no_period.scroll_offset,
            Vector2::ZERO,
            "autoscroll without a repeat_size must not advance at all"
        );

        // And the inverse: a repeat size with no autoscroll must NOT fold an
        // authored scroll_offset into one period.
        let mut authored = p2d();
        authored.repeat_size = Vector2::new(256.0, 0.0);
        authored.scroll_offset = Vector2::new(300.0, 0.0);
        authored.advance_autoscroll(1.0);
        assert!(
            (authored.scroll_offset.x - 300.0).abs() < 1e-3,
            "an authored scroll_offset must survive when nothing autoscrolls, got {}",
            authored.scroll_offset.x
        );
    }
}

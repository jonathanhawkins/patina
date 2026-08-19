//! pat-s3rrv.69 — geometry and drag behaviour for the `Range` control family.
//!
//! `Range` itself already had correct arithmetic in [`crate::control`]:
//! `set_range_value` snaps to `step` and clamps to `[min, max - page]`, and its
//! own doc names "Slider drag, SpinBox typing" as the intended callers. Nothing
//! called it. Sliders were registered nowhere, drawn by nothing, and — because
//! `CONTROL_HIT_CLASSES` already listed them — clickable, focusable, invisible
//! and inert.
//!
//! This module supplies the missing middle: where the grabber sits, what value a
//! point on the track means, and the fill geometry a `TextureProgressBar` needs.
//! It deliberately holds no drawing and no event plumbing, so the same geometry
//! backs both the render arms and the drag path and the two cannot disagree.

use gdcore::math::{Rect2, Vector2};

use crate::control;
use crate::node::NodeId;
use crate::scene_tree::SceneTree;

/// The axis a slider runs along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliderAxis {
    /// `HSlider` — min at the left, max at the right.
    Horizontal,
    /// `VSlider` — min at the **bottom**, max at the top.
    Vertical,
}

/// The slider axis for a class name, or `None` when the class is not a slider.
pub fn slider_axis(class_name: &str) -> Option<SliderAxis> {
    match class_name {
        "HSlider" | "HScrollBar" => Some(SliderAxis::Horizontal),
        "VSlider" | "VScrollBar" => Some(SliderAxis::Vertical),
        _ => None,
    }
}

/// Extent of the grabber along the slider's axis, in pixels.
///
/// Godot takes this from the grabber *texture's* width/height
/// (`slider.cpp:69-70`). Patina's default theme ships no grabber icon, so the
/// family uses one constant instead — and the exact number matters far less
/// than that **the render arm and the drag math use the same one**. They both
/// call this function, so a grabber drawn somewhere other than where dragging
/// puts it is not expressible.
pub const GRABBER_EXTENT: f32 = 10.0;

/// The usable track length: the control's extent along its axis minus the
/// grabber, because the grabber's *centre* travels only between the half-grabber
/// insets at each end (`slider.cpp:71`).
///
/// Clamped to a positive number so a zero-sized or grabber-sized slider cannot
/// divide by zero — it simply has no travel.
pub fn track_length(size: Vector2, axis: SliderAxis) -> f32 {
    let extent = match axis {
        SliderAxis::Horizontal => size.x,
        SliderAxis::Vertical => size.y,
    };
    (extent - GRABBER_EXTENT).max(f32::EPSILON)
}

/// The `[0, 1]` ratio a point **in control-local space** selects.
///
/// Port of `Slider::gui_input` (`slider.cpp:71-78`):
///
/// ```text
/// max = (vertical ? size.height : size.width) - grabber_extent
/// horizontal: ratio =     (pos.x - grabber/2) / max
/// vertical:   ratio = 1 - (pos.y - grabber/2) / max
/// ```
///
/// ⚠️ The vertical form is **inverted**: a `VSlider` has its minimum at the
/// BOTTOM. Reusing the horizontal mapping gives a slider that works perfectly
/// and runs backwards, which no test that only checks "the value moved" would
/// catch.
pub fn ratio_at_local_point(size: Vector2, axis: SliderAxis, local: Vector2) -> f64 {
    let max = track_length(size, axis) as f64;
    let half = (GRABBER_EXTENT / 2.0) as f64;
    let ratio = match axis {
        SliderAxis::Horizontal => (local.x as f64 - half) / max,
        SliderAxis::Vertical => 1.0 - ((local.y as f64 - half) / max),
    };
    ratio.clamp(0.0, 1.0)
}

/// The grabber's rect, in the same local space [`ratio_at_local_point`] reads.
///
/// The inverse of the mapping above, so a grabber drawn here is exactly what a
/// click at its centre selects.
pub fn grabber_rect(size: Vector2, axis: SliderAxis, ratio: f32) -> Rect2 {
    let travel = track_length(size, axis) * ratio.clamp(0.0, 1.0);
    match axis {
        SliderAxis::Horizontal => Rect2::new(
            Vector2::new(travel, 0.0),
            Vector2::new(GRABBER_EXTENT, size.y),
        ),
        // Ratio 0 sits at the BOTTOM, so the offset counts down from the far end.
        SliderAxis::Vertical => Rect2::new(
            Vector2::new(0.0, track_length(size, axis) - travel),
            Vector2::new(size.x, GRABBER_EXTENT),
        ),
    }
}

/// Drags `node_id`'s slider to `local` and returns the new value **only when it
/// changed**, so the caller knows whether to emit `value_changed`.
///
/// Godot emits `value_changed` from `Range::set_value` only on a real change;
/// emitting unconditionally would fire once per mouse-move sample during a drag
/// and flood any connected callback.
pub fn drag_slider_to_local(tree: &mut SceneTree, node_id: NodeId, local: Vector2) -> Option<f64> {
    let class = tree.get_node(node_id)?.class_name().to_string();
    let axis = slider_axis(&class)?;
    // Godot's `Slider::gui_input` ignores input entirely when `editable` is
    // false. An unset property means editable, since that is the registered
    // default — treating "absent" as false would make every hand-built slider
    // inert.
    if matches!(
        tree.get_node(node_id).map(|n| n.get_property("editable")),
        Some(gdvariant::Variant::Bool(false))
    ) {
        return None;
    }
    let size = control::get_size(tree, node_id);
    let ratio = ratio_at_local_point(size, axis, local);

    let min = control::get_range_min(tree, node_id);
    let max = control::get_range_max(tree, node_id);
    let target = min + (max - min) * ratio;

    if control::set_range_value(tree, node_id, target) {
        Some(control::get_range_value(tree, node_id))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// TextureProgressBar fill geometry
// ---------------------------------------------------------------------------

/// Godot's `TextureProgressBar.FillMode` ordinals (`texture_progress_bar.h`).
pub const FILL_LEFT_TO_RIGHT: i64 = 0;
/// Fill from the right edge leftward.
pub const FILL_RIGHT_TO_LEFT: i64 = 1;
/// Fill from the top downward.
pub const FILL_TOP_TO_BOTTOM: i64 = 2;
/// Fill from the bottom upward.
pub const FILL_BOTTOM_TO_TOP: i64 = 3;
/// Sweep clockwise from `radial_initial_angle`.
pub const FILL_CLOCKWISE: i64 = 4;
/// Sweep counter-clockwise.
pub const FILL_COUNTER_CLOCKWISE: i64 = 5;
/// Sweep clockwise then counter-clockwise from the centre.
pub const FILL_CLOCKWISE_AND_COUNTER_CLOCKWISE: i64 = 7;

/// Whether a fill mode draws an arc rather than a rectangle.
pub fn is_radial_fill(fill_mode: i64) -> bool {
    (FILL_CLOCKWISE..=FILL_CLOCKWISE_AND_COUNTER_CLOCKWISE).contains(&fill_mode)
}

/// The filled sub-rect of `rect` for a linear fill mode at `ratio`.
///
/// ⚠️ The two reversed modes shrink from the far edge, so their rect's *origin
/// moves* — filling from the near edge and merely making it shorter would look
/// identical at ratio 1.0 and be wrong everywhere else.
pub fn linear_fill_rect(rect: Rect2, fill_mode: i64, ratio: f32) -> Rect2 {
    let r = ratio.clamp(0.0, 1.0);
    let (w, h) = (rect.size.x, rect.size.y);
    match fill_mode {
        FILL_RIGHT_TO_LEFT => Rect2::new(
            Vector2::new(rect.position.x + w * (1.0 - r), rect.position.y),
            Vector2::new(w * r, h),
        ),
        FILL_TOP_TO_BOTTOM => Rect2::new(rect.position, Vector2::new(w, h * r)),
        FILL_BOTTOM_TO_TOP => Rect2::new(
            Vector2::new(rect.position.x, rect.position.y + h * (1.0 - r)),
            Vector2::new(w, h * r),
        ),
        // FILL_LEFT_TO_RIGHT and anything unrecognised.
        _ => Rect2::new(rect.position, Vector2::new(w * r, h)),
    }
}

/// A filled arc as a triangle-fan polygon: centre first, then the rim.
///
/// `initial_angle` and `fill_degrees` are Godot's `radial_initial_angle` /
/// `radial_fill_degrees`, both in **degrees**, measured clockwise from twelve
/// o'clock — not the mathematical convention. `ratio` scales the swept angle.
///
/// Returns an empty vec below three points, since a degenerate arc is not a
/// polygon and a renderer handed two points would draw nothing useful anyway.
pub fn radial_fill_polygon(
    rect: Rect2,
    center_offset: Vector2,
    initial_angle: f32,
    fill_degrees: f32,
    ratio: f32,
    counter_clockwise: bool,
) -> Vec<Vector2> {
    let r = ratio.clamp(0.0, 1.0);
    let swept = fill_degrees * r;
    if swept.abs() < f32::EPSILON {
        return Vec::new();
    }
    let center = Vector2::new(
        rect.position.x + rect.size.x * 0.5 + center_offset.x,
        rect.position.y + rect.size.y * 0.5 + center_offset.y,
    );
    let radius = rect.size.x.max(rect.size.y) * 0.5;

    // One segment per 6 degrees keeps a full circle at 60 points, which is what
    // Godot's own radial fill uses for its arc approximation.
    let segments = ((swept.abs() / 6.0).ceil() as usize).max(1);
    let mut points = Vec::with_capacity(segments + 2);
    points.push(center);
    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        let deg = initial_angle + swept * t * if counter_clockwise { -1.0 } else { 1.0 };
        // Clockwise from twelve o'clock: x = sin, y = -cos.
        let rad = deg.to_radians();
        points.push(Vector2::new(
            center.x + rad.sin() * radius,
            center.y - rad.cos() * radius,
        ));
    }
    points
}

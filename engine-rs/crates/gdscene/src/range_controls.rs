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

/// A SLIDER's grabber extent: the default theme's `slider_grabber.svg`, a 16x16
/// icon (pat-9ww3e). Godot's drag maths and draw both read the icon's size
/// (`slider.cpp:69-70`, `:329-334`); now that Patina draws that icon, the
/// slider geometry below uses its size too. (Scrollbars keep
/// [`GRABBER_EXTENT`] as their minimum grabber length.)
pub const SLIDER_GRABBER_EXTENT: f32 = 16.0;

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
    (extent - SLIDER_GRABBER_EXTENT).max(f32::EPSILON)
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
    let half = (SLIDER_GRABBER_EXTENT / 2.0) as f64;
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
            Vector2::new(SLIDER_GRABBER_EXTENT, size.y),
        ),
        // Ratio 0 sits at the BOTTOM, so the offset counts down from the far end.
        SliderAxis::Vertical => Rect2::new(
            Vector2::new(0.0, track_length(size, axis) - travel),
            Vector2::new(size.x, SLIDER_GRABBER_EXTENT),
        ),
    }
}

/// A ScrollBar's grabber rect, in the same local space as [`grabber_rect`]
/// (pat-s3rrv.70).
///
/// A scrollbar's grabber is **not** the fixed-size one a slider draws: its
/// length shows how much of the content is visible, which is what makes a
/// scrollbar readable at a glance. Port of `ScrollBar::get_grabber_size` and
/// `get_grabber_offset` (`scene/gui/scroll_bar.cpp`):
///
/// ```text
/// area      = extent - grabber_min          (Godot also subtracts the scroll
///                                            stylebox margin and the two
///                                            stepper icons; Patina's theme
///                                            ships neither, so those terms are
///                                            zero here)
/// grabber   = page / (max - min) * area + grabber_min
/// offset    = area * ratio
/// ```
///
/// ⚠️ `page` is what distinguishes this from a slider. With `page` unset (0) the
/// grabber collapses to `grabber_min` — Godot's own behaviour, and the reason a
/// scrollbar authored without a page still shows a usable handle instead of
/// nothing.
pub fn scrollbar_grabber_rect(
    size: Vector2,
    axis: SliderAxis,
    ratio: f32,
    page: f64,
    min: f64,
    max: f64,
) -> Rect2 {
    let extent = match axis {
        SliderAxis::Horizontal => size.x,
        SliderAxis::Vertical => size.y,
    };
    let area = (extent - GRABBER_EXTENT).max(0.0);
    let range = max - min;
    let visible = if range > 0.0 {
        (page.max(0.0) / range).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    let grabber = (visible * area + GRABBER_EXTENT).min(extent.max(0.0));
    // Godot: `grabber_offset = area_size * get_as_ratio()`
    // (upstream/godot/scene/gui/scroll_bar.cpp:515-517).
    //
    // ⚠️ NOT `(extent - grabber) * ratio`. That discounts the grabber's length a
    // second time, because a scrollbar's value already clamps at `max - page`,
    // so its ratio tops out at `1 - page/range` rather than at 1. Under the
    // double discount a fully-scrolled bar parks short of its own track end —
    // page 20 of 100 on a 100px bar sits at 57.6 instead of 72 — which reads as
    // "there is still more to scroll" when there is not.
    //
    // The `min` keeps a hand-set value outside [min, max - page] from pushing
    // the grabber past the end. At the legal maximum the two agree exactly
    // (area * (1 - visible) == extent - grabber), so it only ever catches
    // out-of-range input.
    let travel = (area * ratio.clamp(0.0, 1.0)).min((extent - grabber).max(0.0));
    match axis {
        SliderAxis::Horizontal => {
            Rect2::new(Vector2::new(travel, 0.0), Vector2::new(grabber, size.y))
        }
        // As with the slider, ratio 0 is the FAR end on a vertical control.
        SliderAxis::Vertical => Rect2::new(
            Vector2::new(0.0, (extent - grabber).max(0.0) - travel),
            Vector2::new(size.x, grabber),
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
// ScrollBar dragging (pat-s3rrv.70)
// ---------------------------------------------------------------------------

/// A scrollbar drag in progress — Godot's `ScrollBar::drag`
/// (upstream/godot/scene/gui/scroll_bar.cpp:105-107).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollBarDrag {
    /// Pointer offset along the axis at the moment the grabber was seized.
    pub pos_at_click: f32,
    /// The bar's ratio at that moment; the drag is applied relative to it.
    pub ratio_at_click: f64,
}

/// What a press on a scrollbar resolves to, decided by where it landed
/// (`ScrollBar::gui_input`, scroll_bar.cpp:85-110).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollBarPress {
    /// The press landed ON the grabber and seized it.
    Grab(ScrollBarDrag),
    /// The press landed on bare track, which pages by this signed amount.
    Page(f64),
}

/// The pointer offset along `axis`, measured in the direction the ratio grows.
///
/// Horizontal grows rightward. Vertical grows UPWARD, because this family draws
/// ratio 0 at the bottom — the drag has to use the same convention the grabber
/// is drawn with or the handle runs away from the cursor.
fn axis_offset(size: Vector2, axis: SliderAxis, local: Vector2) -> f32 {
    match axis {
        SliderAxis::Horizontal => local.x,
        SliderAxis::Vertical => size.y - local.y,
    }
}

/// Resolves a press at `local` on a scrollbar.
///
/// ⚠️ A scrollbar is NOT a slider: pressing the track does not jump the value to
/// the cursor. Godot pages by one `page` toward the click and only a press on
/// the grabber itself begins a drag — and that drag is *relative*, preserving
/// where inside the grabber the user took hold. Jumping to the cursor makes the
/// content leap on every click and makes the grabber snap out from under the
/// pointer the moment a drag starts.
pub fn scrollbar_press(
    tree: &SceneTree,
    node_id: NodeId,
    local: Vector2,
) -> Option<ScrollBarPress> {
    let class = tree.get_node(node_id)?.class_name().to_string();
    let axis = slider_axis(&class)?;
    let size = control::get_size(tree, node_id);
    let page = control::get_range_page(tree, node_id);
    let min = control::get_range_min(tree, node_id);
    let max = control::get_range_max(tree, node_id);
    let ratio = control::get_range_ratio(tree, node_id);

    let grabber = scrollbar_grabber_rect(size, axis, ratio, page, min, max);
    let (grabber_ofs, grabber_size) = match axis {
        SliderAxis::Horizontal => (grabber.position.x, grabber.size.x),
        // The rect is in draw space, where vertical y grows downward; convert
        // its near edge back into ratio-space so both ends use one convention.
        SliderAxis::Vertical => (size.y - grabber.position.y - grabber.size.y, grabber.size.y),
    };

    let ofs = axis_offset(size, axis, local);
    if ofs < grabber_ofs {
        // Before the grabber: page toward the minimum.
        return Some(ScrollBarPress::Page(-page));
    }
    if ofs - grabber_ofs < grabber_size {
        return Some(ScrollBarPress::Grab(ScrollBarDrag {
            pos_at_click: ofs,
            ratio_at_click: ratio as f64,
        }));
    }
    Some(ScrollBarPress::Page(page))
}

/// Continues a scrollbar drag to `local`, returning the new value **only when it
/// changed**.
///
/// Godot: `diff = (ofs - drag.pos_at_click) / get_area_size()`, then
/// `set_as_ratio(drag.value_at_click + diff)` (scroll_bar.cpp:143-147). The
/// denominator is the track area, NOT the grabber's travel, so the bar keeps
/// pace with the cursor.
pub fn scrollbar_drag_to(
    tree: &mut SceneTree,
    node_id: NodeId,
    local: Vector2,
    drag: ScrollBarDrag,
) -> Option<f64> {
    let class = tree.get_node(node_id)?.class_name().to_string();
    let axis = slider_axis(&class)?;
    let size = control::get_size(tree, node_id);
    let extent = match axis {
        SliderAxis::Horizontal => size.x,
        SliderAxis::Vertical => size.y,
    };
    let area = (extent - GRABBER_EXTENT).max(0.0);
    if area <= 0.0 {
        return None;
    }
    let ofs = axis_offset(size, axis, local);
    let diff = (ofs - drag.pos_at_click) as f64 / area as f64;

    let min = control::get_range_min(tree, node_id);
    let max = control::get_range_max(tree, node_id);
    let target = min + (max - min) * (drag.ratio_at_click + diff);
    if control::set_range_value(tree, node_id, target) {
        Some(control::get_range_value(tree, node_id))
    } else {
        None
    }
}

/// Godot's `ScrollBar::PAGE_DIVISOR` (scroll_bar.h:116): one wheel notch moves a
/// scrollbar by an eighth of a page.
pub const PAGE_DIVISOR: f64 = 8.0;

/// The value delta one wheel notch applies to a range control, or `None` when
/// the class does not take the wheel.
///
/// ⚠️ The two families move in OPPOSITE directions for the same notch, and it is
/// not a typo:
/// - a **ScrollBar** scrolls *down* on wheel-down, so wheel-down **adds**
///   (`scroll_bar.cpp:83-92`);
/// - a **Slider** is a value control whose maximum is "up", so wheel-**up** adds
///   (`slider.cpp:45-56`).
///
/// A single shared sign would make one of the two run backwards — the class of
/// bug that looks fine in a screenshot and is immediately wrong in the hand.
pub fn wheel_delta(tree: &SceneTree, node_id: NodeId, down: bool) -> Option<f64> {
    let class = tree.get_node(node_id)?.class_name().to_string();
    slider_axis(&class)?;
    let step = control::get_range_step(tree, node_id);

    if matches!(class.as_str(), "HScrollBar" | "VScrollBar") {
        let page = control::get_range_page(tree, node_id);
        let min = control::get_range_min(tree, node_id);
        let max = control::get_range_max(tree, node_id);
        // `page / 8`, or a sixteenth of the whole range when no page is set, and
        // never smaller than one `step` — Godot's `MAX(change, get_step())`.
        // Without that floor a bar with a tiny page would need dozens of notches
        // to move at all.
        let change = if page != 0.0 {
            page / PAGE_DIVISOR
        } else {
            (max - min) / 16.0
        }
        .max(step);
        return Some(if down { change } else { -change });
    }

    // Sliders take the wheel only when `scrollable` (Godot's default true), and
    // move by one `step`. An unset property means scrollable, since that is the
    // registered default.
    if matches!(
        tree.get_node(node_id).map(|n| n.get_property("scrollable")),
        Some(gdvariant::Variant::Bool(false))
    ) {
        return None;
    }
    Some(if down { -step } else { step })
}

/// Applies a track-click page step, returning the new value only when it moved.
pub fn scrollbar_page(tree: &mut SceneTree, node_id: NodeId, amount: f64) -> Option<f64> {
    let current = control::get_range_value(tree, node_id);
    if control::set_range_value(tree, node_id, current + amount) {
        Some(control::get_range_value(tree, node_id))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Script-facing method surface (pat-s3rrv.73)
// ---------------------------------------------------------------------------

/// Classes carrying Godot's `Range` value model, for use when ClassDB is not
/// populated (unit tests, headless tooling) — the same fallback discipline
/// `control::node_is_base_button` uses.
const RANGE_CLASSES: &[&str] = &[
    "Range",
    "Slider",
    "HSlider",
    "VSlider",
    "ScrollBar",
    "HScrollBar",
    "VScrollBar",
    "ProgressBar",
    "SpinBox",
    "TextureProgressBar",
];

/// Whether `node_id` carries the Range value model.
pub fn is_range_control(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| {
            n.is_class("Range")
                || RANGE_CLASSES.contains(&n.class_name())
                || gdobject::class_db::is_parent_class(n.class_name(), "Range")
        })
        .unwrap_or(false)
}

/// Sets the value and reports whether it changed, so callers that must NOT
/// notify (`set_value_no_signal`) and callers that must (`set_value`) share one
/// clamping rule.
fn store_value(tree: &mut SceneTree, node_id: NodeId, v: f64) -> bool {
    control::set_range_value(tree, node_id, v)
}

/// Re-clamps the current value after the bounds move.
///
/// ⚠️ Godot's `set_min`/`set_max` end with `set_value(shared->val)`
/// (range.cpp:217, :234) precisely so raising the floor above the current value
/// drags the value up with it. Without this a slider keeps a value outside its
/// own range and every ratio derived from it is out of bounds.
fn reclamp(tree: &mut SceneTree, node_id: NodeId) -> bool {
    let current = control::get_range_value(tree, node_id);
    store_value(tree, node_id, current)
}

fn set_num_prop(tree: &mut SceneTree, node_id: NodeId, key: &str, v: f64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(key, gdvariant::Variant::Float(v));
    }
}

fn num_arg(args: &[gdvariant::Variant], i: usize) -> Option<f64> {
    match args.get(i) {
        Some(gdvariant::Variant::Float(f)) => Some(*f),
        Some(gdvariant::Variant::Int(n)) => Some(*n as f64),
        _ => None,
    }
}

/// Godot's `Range` method surface for the script bridge.
///
/// Returns `None` for every node that is not a range control, and for every
/// method this does not implement, so the caller's own table is reached
/// unchanged — the shape `call_agent_method` and `call_skeleton_method` already
/// use in `scripting.rs`.
///
/// ⚠️ Reached from BOTH script spellings: `$Bar.set_value(50)` resolves through
/// `interpreter.rs::call_method_on`, which falls back to
/// `SceneAccess::call_node_method`, and a bare `set_value(50)` in a script on the
/// node itself resolves through `call_self_func_or_native` to the same place. A
/// binding added only to the interpreter's builtin match would be unreachable
/// from the ordinary spelling.
pub fn call_range_method(
    tree: &mut SceneTree,
    node_id: NodeId,
    method: &str,
    args: &[gdvariant::Variant],
) -> Option<gdvariant::Variant> {
    use gdvariant::Variant;
    if !is_range_control(tree, node_id) {
        return None;
    }

    // Setting the value notifies exactly as `Range::set_value` does: on a real
    // change only.
    let set_and_notify = |tree: &mut SceneTree, v: f64| {
        if store_value(tree, node_id, v) {
            let now = control::get_range_value(tree, node_id);
            tree.emit_signal(node_id, "value_changed", &[Variant::Float(now)]);
        }
    };

    match method {
        "get_value" => Some(Variant::Float(control::get_range_value(tree, node_id))),
        "set_value" => {
            let v = num_arg(args, 0)?;
            set_and_notify(tree, v);
            Some(Variant::Nil)
        }
        // A real declared method (Range.xml), not a property accessor: the
        // documented way to move the value without waking connected handlers —
        // what a script uses to sync a slider TO a value it just received.
        "set_value_no_signal" => {
            let v = num_arg(args, 0)?;
            store_value(tree, node_id, v);
            Some(Variant::Nil)
        }
        "get_min" => Some(Variant::Float(control::get_range_min(tree, node_id))),
        "get_max" => Some(Variant::Float(control::get_range_max(tree, node_id))),
        "get_step" => Some(Variant::Float(control::get_range_step(tree, node_id))),
        "get_page" => Some(Variant::Float(control::get_range_page(tree, node_id))),
        "set_min" => {
            let v = num_arg(args, 0)?;
            set_num_prop(tree, node_id, "min_value", v);
            // `max = MAX(max, min)` (range.cpp:215) — the bounds may not cross.
            let max = control::get_range_max(tree, node_id);
            if max < v {
                set_num_prop(tree, node_id, "max_value", v);
            }
            if reclamp(tree, node_id) {
                let now = control::get_range_value(tree, node_id);
                tree.emit_signal(node_id, "value_changed", &[Variant::Float(now)]);
            }
            Some(Variant::Nil)
        }
        "set_max" => {
            // `max = MAX(p_max, min)` (range.cpp:227).
            let v = num_arg(args, 0)?.max(control::get_range_min(tree, node_id));
            set_num_prop(tree, node_id, "max_value", v);
            if reclamp(tree, node_id) {
                let now = control::get_range_value(tree, node_id);
                tree.emit_signal(node_id, "value_changed", &[Variant::Float(now)]);
            }
            Some(Variant::Nil)
        }
        "set_step" => {
            set_num_prop(tree, node_id, "step", num_arg(args, 0)?);
            Some(Variant::Nil)
        }
        "set_page" => {
            set_num_prop(tree, node_id, "page", num_arg(args, 0)?);
            // `page` narrows the effective maximum to `max - page`, so the
            // current value can fall outside it.
            if reclamp(tree, node_id) {
                let now = control::get_range_value(tree, node_id);
                tree.emit_signal(node_id, "value_changed", &[Variant::Float(now)]);
            }
            Some(Variant::Nil)
        }
        "get_as_ratio" => Some(Variant::Float(
            control::get_range_ratio(tree, node_id) as f64
        )),
        "set_as_ratio" => {
            let r = num_arg(args, 0)?;
            let min = control::get_range_min(tree, node_id);
            let max = control::get_range_max(tree, node_id);
            set_and_notify(tree, min + (max - min) * r);
            Some(Variant::Nil)
        }
        _ => None,
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

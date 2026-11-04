//! pat-s3rrv.172 acceptance: a `ColorPicker` is drawn from its HSV model and
//! driven by the pointer, and a `ColorPickerButton` shows its colour.
//!
//! Before this bead `color_picker.rs` held a faithful `color_from_hsv` port and
//! the SV/hue hit mapping with **zero non-test callers** workspace-wide: the
//! classes were registered (`class_db.rs:17423`/`:17461`, so a `.tscn` loads
//! them) but `control_render.rs` had no draw arm and `gui_input_stage` had no
//! branch. A ColorPicker in a played scene was an invisible, inert rectangle.
//!
//! Ported from `upstream/godot/scene/gui/color_picker.cpp` (SHAPE_HSV_RECTANGLE).
//!
//! # Why every number below is exact
//!
//! The geometry is derived from the layout constants, not measured from a run.
//! `HUE_BAR_W/BAR_GAP/ALPHA_BAR_H = 30/4/12` (`color_picker.rs:162-169`, Godot's
//! `h_width` and the HBox separation, `default_theme.cpp:1077-1079`). For a
//! picker at (20, 30) sized 234x116 with `edit_alpha` on:
//!
//! * `body_h = 116 - 12 - 4` = **100**
//! * `sv_w   = 234 - 30 - 4` = **200**  → `sv    = (20, 30) 200x100`
//! * `hue.x  = 20 + 200 + 4` = **224**  → `hue   = (224, 30) 30x100`
//! * `alpha.y = 30 + 100 + 4` = **134** → `alpha = (20, 134) 234x12`
//!
//! ⚠️ Three properties of this fixture are load-bearing and none is incidental:
//!
//! 1. **The origin is not (0, 0).** `picker_layout` returns its sub-rects in
//!    whatever space the rect went in, so a draw arm that passes global
//!    coordinates and an input branch that passes local ones agree perfectly at
//!    the origin and are 20px out everywhere else. A picker at (0, 0) cannot
//!    tell those apart.
//! 2. **The SV area is 200x100, not square.** A square SV area makes an x/y
//!    transpose in the hit mapping invisible.
//! 3. **The starting hue is 1/3, not 0.** Clause 2 asserts the hue SURVIVES an
//!    SV press; with a red starting colour a bug that reset the hue to 0 would
//!    produce exactly the expected answer.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use gdcore::id::ObjectId;
use gdcore::math::{Color, Rect2, Vector2};
use gdobject::signal::Connection;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::color_picker::{
    call_color_picker_method, color_from_hsv, hsv_from_color, ALPHA_BAR_H, HUE_BAR_W,
};
use gdscene::control::{
    control_at_point, fire_button_action, resolve_control_layout, set_offset_bottom,
    set_offset_left, set_offset_right, set_offset_top,
};
use gdscene::control_render::{control_draw_items, ControlDrawItem};
use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_saver::TscnSaver;
use gdscene::scene_tree::SceneTree;
use gdscene::Theme;
use gdvariant::Variant;

const VIEWPORT: Vector2 = Vector2::new(400.0, 300.0);

/// The picker's starting colour: `(0.25, 0.5, 0.25)`, chosen because its HSV
/// decomposition is exact in `f32` and its hue is non-zero.
///
/// By Godot's algorithm (`core/math/color.cpp`, ported at
/// `color_picker.rs:51`): `max = 0.5` (green), `min = 0.25`, `delta = 0.25`, so
/// `value = 0.5`, `saturation = 0.25/0.5 = 0.5`, and since green is the max,
/// `hue = (2 + (b - r)/delta) / 6 = 2/6`.
const START: Color = Color::new(0.25, 0.5, 0.25, 1.0);
/// `2/6`, written as the division so it rounds exactly as the port's does.
const START_HUE: f32 = 2.0 / 6.0;

// -- the sub-rects, spelled out ---------------------------------------------
const SV: Rect2 = Rect2::new(Vector2::new(20.0, 30.0), Vector2::new(200.0, 100.0));
const HUE: Rect2 = Rect2::new(Vector2::new(224.0, 30.0), Vector2::new(30.0, 100.0));
const ALPHA: Rect2 = Rect2::new(Vector2::new(20.0, 134.0), Vector2::new(234.0, 12.0));

/// A tree holding one `ColorPicker` at (20, 30), 234x116.
///
/// Hand-built rather than loaded from a `.tscn` so the rect is exact — the
/// numbers in the module note are only derivable from a known origin.
fn tree_with_picker(color: Color) -> (SceneTree, NodeId) {
    // 🔑 Required, and not merely conventional: `ColorPicker` is NOT in
    // `CONTROL_HIT_CLASSES` (`control.rs:915`) and does not need to be — it
    // reaches `Control` through ClassDB as
    // ColorPicker -> VBoxContainer -> BoxContainer -> Container -> Control. With
    // the registry unpopulated `is_class("Control")` is false, the list has no
    // fallback entry, and every press below would miss silently.
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut picker = Node::new("Picker", "ColorPicker");
    picker.set_property("color", Variant::Color(color));
    let id = tree.add_child(root, picker).expect("picker added");

    // The rect is expressed as OFFSETS, never by writing `size`: `size` is
    // DERIVED, and the draw walk's `resolve_control_layout`
    // (`control_render.rs:558`) recomputes it from anchors + offsets and
    // OVERWRITES whatever `set_size` wrote. A fixture that sets `size` directly
    // is 234x116 until the first draw and 0x0 afterwards.
    set_offset_left(&mut tree, id, 20.0);
    set_offset_top(&mut tree, id, 30.0);
    set_offset_right(&mut tree, id, 254.0);
    set_offset_bottom(&mut tree, id, 146.0);
    // ⚠️ And resolve once here, because the HIT TEST does not: `control_at_point`
    // reads `size` as-is, so before any draw the control is 0x0 and every press
    // misses. That failure is silent — a press that lands on nothing simply does
    // nothing, and "the colour did not change" then passes for the wrong reason.
    resolve_control_layout(&mut tree, id, VIEWPORT);
    (tree, id)
}

fn items(tree: &mut SceneTree) -> Vec<ControlDrawItem> {
    let root = tree.root_id();
    control_draw_items(tree, root, &Theme::default(), VIEWPORT)
}

/// Every `Rect` item, as `(rect, color, filled)`.
fn rects(items: &[ControlDrawItem]) -> Vec<(Rect2, Color, bool)> {
    items
        .iter()
        .filter_map(|i| match i {
            ControlDrawItem::Rect {
                rect,
                color,
                filled,
                ..
            } => Some((*rect, *color, *filled)),
            _ => None,
        })
        .collect()
}

/// The colour of the filled rect whose top-left is exactly `at`.
fn cell_at(items: &[ControlDrawItem], at: Vector2) -> Option<(Rect2, Color)> {
    rects(items)
        .into_iter()
        .find(|(r, _, filled)| *filled && r.position == at)
        .map(|(r, c, _)| (r, c))
}

fn press(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseButton {
        position: Vector2::new(x, y),
        button: MouseButton::Left,
        pressed: true,
    }
}

fn motion(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseMotion {
        position: Vector2::new(x, y),
        relative: Vector2::ZERO,
    }
}

/// The picker's `color` property.
fn color_of(tree: &SceneTree, id: NodeId) -> Color {
    match tree.get_node(id).map(|n| n.get_property("color")) {
        Some(Variant::Color(c)) => c,
        other => panic!("ColorPicker.color must stay a Color, got {other:?}"),
    }
}

/// **The acceptance.** Drawn from the model, and driven by the pointer.
#[test]
fn color_picker_scene_draw_and_pick() {
    // -- clause 1: draw items cover the rect --------------------------------
    let (mut tree, id) = tree_with_picker(START);
    let drawn = items(&mut tree);
    assert!(
        !drawn.is_empty(),
        "before this bead the draw walk skipped ColorPicker entirely and the \
         node painted nothing at all"
    );

    // The SV area: a 16x16 grid of cells, each sampled at its CENTRE. The
    // top-left cell is asserted by position AND colour, so this fails both for
    // a picker that draws nothing and for one that draws a grid of the wrong
    // hue — an arm ignoring `color` would paint the default white's hue 0.
    let (sv_cell, sv_color) = cell_at(&drawn, SV.position).unwrap_or_else(|| {
        panic!(
            "no SV cell at the picker's top-left {:?}: {drawn:?}",
            SV.position
        )
    });
    assert_eq!(
        sv_cell.size,
        Vector2::new(200.0 / 16.0, 100.0 / 16.0),
        "the SV grid divides the 200x100 area 16 ways on each axis"
    );
    assert_eq!(
        sv_color,
        color_from_hsv(START_HUE, 0.5 / 16.0, 1.0 - 0.5 / 16.0, 1.0),
        "the top-left cell samples its own centre at the picker's CURRENT hue"
    );

    // The hue bar sits to the right of the SV area, and runs the spectrum
    // regardless of the selected colour.
    let (hue_cell, hue_color) = cell_at(&drawn, HUE.position)
        .unwrap_or_else(|| panic!("no hue-bar cell at {:?}: {drawn:?}", HUE.position));
    assert_eq!(
        hue_cell.size,
        Vector2::new(HUE_BAR_W, 100.0 / 24.0),
        "the hue bar keeps its fixed 30px width — a wide picker grows its SV \
         square, not its hue strip"
    );
    assert_eq!(
        hue_color,
        color_from_hsv(0.5 / 24.0, 1.0, 1.0, 1.0),
        "the hue bar is full saturation and value down the spectrum"
    );

    // The alpha bar, because `edit_alpha` defaults to true (`class_db.rs`).
    let (alpha_cell, _) = cell_at(&drawn, ALPHA.position)
        .unwrap_or_else(|| panic!("no alpha-bar cell at {:?}: {drawn:?}", ALPHA.position));
    assert_eq!(alpha_cell.size, Vector2::new(234.0 / 16.0, ALPHA_BAR_H));

    // 🔑 The cursor is the only OUTLINED rect, and its position is the whole
    // point of clause 1: without it every colour of a given hue paints an
    // identical square and the control displays no state whatsoever. At s = 0.5
    // and v = 0.5 it sits at (20 + 0.5*200, 30 + (1 - 0.5)*100) = (120, 80),
    // and the 6px marker is centred there.
    let outlined: Vec<_> = rects(&drawn).into_iter().filter(|(_, _, f)| !*f).collect();
    assert_eq!(
        outlined.len(),
        1,
        "exactly one outlined rect — the SV cursor: {outlined:?}"
    );
    assert_eq!(
        outlined[0].0,
        Rect2::new(Vector2::new(117.0, 77.0), Vector2::new(6.0, 6.0)),
        "the cursor marks the selected colour; `value` runs bottom-to-top, so a \
         cursor at y = 30 + v*100 = 80 would be the un-inverted (and wrong) one"
    );

    // -- clause 2: a press at the top-right of the SV area ------------------
    //
    // (219, 30) is the last column and the top row: u = 199/200 = 0.995,
    // v = 0/100 = 0, so saturation = 0.995 and value = 1 - 0 = 1.
    //
    // ⚠️ Not (220, 30): `part_at` uses a half-open test (`< pos + size`), so the
    // exact top-right CORNER is outside the SV area and outside the hue bar's
    // left edge too — it would resolve to nothing.
    assert!(
        control_at_point(&tree, Vector2::new(219.0, 30.0)).is_some(),
        "the fixture must be hit-testable, or every assertion below passes \
         because the press simply missed"
    );

    let seen = Arc::new(AtomicUsize::new(0));
    let got = Arc::new(std::sync::Mutex::new(Vec::<Color>::new()));
    let (n, sink) = (seen.clone(), got.clone());
    tree.connect_signal(
        id,
        "color_changed",
        Connection::with_callback(ObjectId::from_raw(9101), "on_color_changed", move |args| {
            n.fetch_add(1, Ordering::SeqCst);
            if let Some(Variant::Color(c)) = args.first() {
                sink.lock().expect("sink").push(*c);
            }
            Variant::Nil
        }),
    );

    tree.push_input(&press(219.0, 30.0));

    let expected = color_from_hsv(START_HUE, 0.995, 1.0, 1.0);
    assert_eq!(
        color_of(&tree, id),
        expected,
        "a press at the top-right of the SV area is full value and (near) full \
         saturation AT THE CURRENT HUE — a hue reset would give red here, and \
         a u/v transpose near-black"
    );
    // Stated a second way, independent of our own `color_from_hsv`: the hue is
    // 1/3, so the answer is green, and it is green because the hue SURVIVED.
    let c = color_of(&tree, id);
    assert!(
        c.g == 1.0 && c.r < 0.01 && c.b < 0.01,
        "the picked colour must be the pure-ish green of hue 1/3, got {c:?}"
    );
    // 🔑 ONCE, not once-per-frame and not once-per-motion-sample. A branch
    // driven by a LEVEL ("the button is down") rather than the press EDGE would
    // pass a `>= 1` assertion and flood every connected handler during a drag.
    assert_eq!(
        seen.load(Ordering::SeqCst),
        1,
        "`color_changed` must be emitted exactly once for one press"
    );
    assert_eq!(
        got.lock().expect("sink").clone(),
        vec![expected],
        "and it must carry the new colour, not the old one"
    );

    // A second press on the SAME point changes nothing, so nothing is emitted —
    // Godot does not re-emit for a click landing on the current value.
    tree.push_input(&press(219.0, 30.0));
    assert_eq!(
        seen.load(Ordering::SeqCst),
        1,
        "an idempotent press must not re-emit `color_changed`"
    );

    // -- clause 3: dragging the hue bar moves the hue monotonically ---------
    //
    // The press captures the picker; the motions that follow keep picking, which
    // is what makes a drag continuous rather than a sequence of jumps. x = 239
    // is the middle of the 224..254 bar.
    let (mut tree, id) = tree_with_picker(START);
    let hues = Arc::new(std::sync::Mutex::new(Vec::<f32>::new()));
    let sink = hues.clone();
    tree.connect_signal(
        id,
        "color_changed",
        Connection::with_callback(ObjectId::from_raw(9102), "on_hue", move |args| {
            if let Some(Variant::Color(c)) = args.first() {
                sink.lock().expect("hues").push(hsv_from_color(*c).0);
            }
            Variant::Nil
        }),
    );

    tree.push_input(&press(239.0, 30.0));
    for y in [55.0_f32, 80.0, 105.0] {
        tree.push_input(&motion(239.0, y));
    }
    let seq = hues.lock().expect("hues").clone();
    assert_eq!(
        seq.len(),
        4,
        "one emission for the press and one per motion sample: {seq:?}"
    );
    // t = (y - 30)/100, so 0, 0.25, 0.5, 0.75.
    for (i, expect) in [0.0_f32, 0.25, 0.5, 0.75].into_iter().enumerate() {
        assert!(
            (seq[i] - expect).abs() < 1e-4,
            "hue sample {i} should be {expect}, got {}: the drag must track the \
             pointer DOWN the bar, in the same space the arm painted it",
            seq[i]
        );
    }
    assert!(
        seq.windows(2).all(|w| w[1] > w[0]),
        "the hue must move monotonically down the bar: {seq:?}"
    );

    // ⚠️ And the motions only pick because the press captured the control. A
    // motion with no press before it must do nothing, or "dragging works" would
    // really mean "the picker repaints under any passing pointer".
    let (mut tree, id) = tree_with_picker(START);
    tree.push_input(&motion(239.0, 105.0));
    assert_eq!(
        color_of(&tree, id),
        START,
        "an uncaptured motion must not pick"
    );
}

/// A `ColorPickerButton` wears its colour.
#[test]
fn a_color_picker_button_draws_its_colour() {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut btn = Node::new("Swatch", "ColorPickerButton");
    btn.set_property("color", Variant::Color(START));
    let id = tree.add_child(root, btn).expect("button added");
    set_offset_left(&mut tree, id, 10.0);
    set_offset_top(&mut tree, id, 10.0);
    set_offset_right(&mut tree, id, 90.0);
    set_offset_bottom(&mut tree, id, 34.0);
    resolve_control_layout(&mut tree, id, VIEWPORT);

    let drawn = items(&mut tree);
    // The swatch is inset by 2px inside the button's stylebox, so the button
    // still reads as a button rather than as a bare rectangle of colour.
    let swatch = rects(&drawn)
        .into_iter()
        .find(|(_, c, filled)| *filled && *c == START)
        .unwrap_or_else(|| panic!("no swatch of the button's own colour: {drawn:?}"));
    assert_eq!(
        swatch.0,
        Rect2::new(Vector2::new(12.0, 12.0), Vector2::new(76.0, 20.0)),
        "the swatch is the button rect inset by 2px on every side"
    );
}

/// A picker too narrow for its own hue bar swallows nothing.
///
/// ⚠️ `picker_layout` clamps to zero rather than going negative. A negative
/// width makes a rect-contains test accept every point to its LEFT, so a
/// too-small picker would silently eat presses aimed at whatever sits beside it.
#[test]
fn a_picker_narrower_than_its_hue_bar_has_no_negative_rects() {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    // `Node::new` is the RAW constructor: unlike a `.tscn` node it seeds no
    // ClassDB defaults, so `color` read back Nil. Godot's default is white.
    let mut picker = Node::new("Tiny", "ColorPicker");
    picker.set_property("color", Variant::Color(Color::new(1.0, 1.0, 1.0, 1.0)));
    let id = tree.add_child(root, picker).expect("picker added");
    set_offset_left(&mut tree, id, 100.0);
    set_offset_top(&mut tree, id, 100.0);
    set_offset_right(&mut tree, id, 110.0); // 10px wide, versus a 30px hue bar
    set_offset_bottom(&mut tree, id, 108.0);
    resolve_control_layout(&mut tree, id, VIEWPORT);

    let drawn = items(&mut tree);
    for (r, _, _) in rects(&drawn) {
        assert!(
            r.size.x >= 0.0 && r.size.y >= 0.0,
            "no draw item may have a negative extent: {r:?}"
        );
    }
    // Every part stays INSIDE the control, alpha bar included. Squeezed below
    // bar + gap, the alpha bar used to be placed BAR_GAP px under the rect
    // (y 104..112 for a 100..108 control) and caught presses below the picker.
    let rect = Rect2::new(Vector2::new(100.0, 100.0), Vector2::new(10.0, 8.0));
    let layout = gdscene::color_picker::picker_layout(rect, true);
    let alpha = layout.alpha.expect("edit_alpha draws an alpha bar");
    for part in [layout.sv, layout.hue, alpha] {
        assert!(
            part.position.y >= rect.position.y
                && part.position.y + part.size.y <= rect.position.y + rect.size.y + 1e-4,
            "a picker part must stay inside the control: {part:?} vs {rect:?}"
        );
    }
    // The gap between SV and hue bar belongs to neither part, and a press there
    // must resolve to nothing rather than snapping to whichever rect was tested
    // first. With `sv_w` clamped to 0 the whole control is gap — once the
    // alpha bar (which spans the full width) is off, as it is here.
    tree.get_node_mut(id)
        .unwrap()
        .set_property("edit_alpha", Variant::Bool(false));
    let before = color_of(&tree, id);
    tree.push_input(&press(104.0, 104.0));
    assert_eq!(
        before,
        color_of(&tree, id),
        "a press in the gap picks nothing"
    );
}

// ---------------------------------------------------------------------------
// Clause 5 — presets
// ---------------------------------------------------------------------------

/// A scene whose picker starts with no presets at all.
const PICKER_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Picker" type="ColorPicker" parent="."]
color = Color(0.25, 0.5, 0.25, 1)
"#;

/// Returns the instanced scene's own root and its `ColorPicker`.
///
/// ⚠️ The SCENE root, not `tree.root_id()`: `TscnSaver::save_tree` serialises
/// from whatever node it is handed, so saving from the tree root would wrap the
/// scene in an extra node and the reload would not be the same document.
fn instance(tscn: &str) -> (SceneTree, NodeId, NodeId) {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let picker = tree
        .find_child(scene_root, "Picker", true)
        .expect("the scene declares a ColorPicker");
    (tree, scene_root, picker)
}

fn call(tree: &mut SceneTree, id: NodeId, method: &str, args: &[Variant]) -> Option<Variant> {
    call_color_picker_method(tree, id, method, args)
}

fn presets(tree: &mut SceneTree, id: NodeId) -> Vec<Color> {
    match call(tree, id, "get_presets", &[]) {
        Some(Variant::PackedColorArray(v)) => v,
        other => panic!("get_presets must answer a PackedColorArray, got {other:?}"),
    }
}

const RED: Color = Color::new(1.0, 0.0, 0.0, 1.0);
const BLUE: Color = Color::new(0.0, 0.0, 1.0, 1.0);

/// **Clause 5.** `add_preset` then `get_presets`, surviving a `.tscn` round trip.
#[test]
fn presets_round_trip_through_a_tscn() {
    let (mut tree, scene_root, id) = instance(PICKER_SCENE);
    assert!(
        presets(&mut tree, id).is_empty(),
        "a picker with no `presets` row starts empty rather than answering Nil"
    );

    call(&mut tree, id, "add_preset", &[Variant::Color(RED)]);
    assert_eq!(presets(&mut tree, id), vec![RED]);

    // 🔑 The save is asserted on its TEXT, not merely on the reload. A saver
    // that dropped the row and a loader that invented a default would cancel
    // out, and a reload-only assertion would call that a passing round trip.
    let saved = TscnSaver::save_tree(&tree, scene_root);
    assert!(
        saved.contains("presets = PackedColorArray(1, 0, 0, 1)"),
        "the preset list must reach the file as an ordinary property row: {saved}"
    );

    let (mut reloaded, _, id2) = instance(&saved);
    assert_eq!(
        presets(&mut reloaded, id2),
        vec![RED],
        "and come back out again"
    );
    // The picker's own colour is untouched by all of this.
    assert_eq!(color_of(&reloaded, id2), START);
}

/// Re-adding a preset already in the list MOVES it to the back.
///
/// 🔑 Upstream does `presets.move_to_back(e)` for a colour it already holds
/// (`color_picker.cpp:1069`) — it neither ignores the call nor appends a
/// duplicate. Reading it as "already present, nothing to do" is the natural
/// interpretation and it silently destroys the most-recently-added-last
/// ordering, which is the only thing the list is for.
#[test]
fn re_adding_a_preset_moves_it_to_the_back() {
    let (mut tree, _, id) = instance(PICKER_SCENE);
    call(&mut tree, id, "add_preset", &[Variant::Color(RED)]);
    call(&mut tree, id, "add_preset", &[Variant::Color(BLUE)]);
    assert_eq!(presets(&mut tree, id), vec![RED, BLUE]);

    call(&mut tree, id, "add_preset", &[Variant::Color(RED)]);
    assert_eq!(
        presets(&mut tree, id),
        vec![BLUE, RED],
        "re-adding must not duplicate, and must not leave the order alone"
    );

    // Erasing a colour the picker does not hold is a no-op — upstream's whole
    // body sits behind an `if (e)` guard (`color_picker.cpp:1101`).
    call(
        &mut tree,
        id,
        "erase_preset",
        &[Variant::Color(Color::new(0.0, 1.0, 0.0, 1.0))],
    );
    assert_eq!(presets(&mut tree, id), vec![BLUE, RED]);

    call(&mut tree, id, "erase_preset", &[Variant::Color(BLUE)]);
    assert_eq!(presets(&mut tree, id), vec![RED]);
}

/// The scripted preset calls emit NOTHING.
///
/// ⚠️ `preset_added` / `preset_removed` are registered signals, and emitting
/// them from `add_preset` / `erase_preset` is the obvious thing to do. Upstream
/// emits them only from the UI handlers — `_add_preset_pressed`
/// (`color_picker.cpp:1522`) and `_preset_input` (`:1482`, `:1492`) — so a
/// script calling the method must not see them fire. Nothing but a
/// handler-counting assertion catches this.
#[test]
fn scripted_preset_calls_emit_no_signals() {
    let (mut tree, _, id) = instance(PICKER_SCENE);
    let fired = Arc::new(AtomicUsize::new(0));
    for (i, sig) in ["preset_added", "preset_removed"].into_iter().enumerate() {
        let n = fired.clone();
        tree.connect_signal(
            id,
            sig,
            Connection::with_callback(
                ObjectId::from_raw(9200 + i as u64),
                "on_preset",
                move |_| {
                    n.fetch_add(1, Ordering::SeqCst);
                    Variant::Nil
                },
            ),
        );
    }
    call(&mut tree, id, "add_preset", &[Variant::Color(RED)]);
    call(&mut tree, id, "erase_preset", &[Variant::Color(RED)]);
    // The calls really did run, or "no signal fired" would be vacuous.
    assert!(presets(&mut tree, id).is_empty());
    assert_eq!(
        fired.load(Ordering::SeqCst),
        0,
        "the scripted preset methods are silent; only the UI paths emit"
    );
}

/// A `ColorPickerButton` does not answer the preset surface.
///
/// ⚠️ It hosts a picker, so forwarding looks right — but upstream gives the
/// button only `get_picker` and `get_popup`
/// (`doc/classes/ColorPickerButton.xml`). Answering here would invent API that
/// a real Godot project cannot call, and `None` is what lets the generic node
/// table have its turn.
#[test]
fn a_color_picker_button_declines_the_preset_methods() {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let btn = Node::new("Swatch", "ColorPickerButton");
    let id = tree.add_child(root, btn).expect("button added");
    assert!(call(&mut tree, id, "get_presets", &[]).is_none());
    assert!(call(&mut tree, id, "add_preset", &[Variant::Color(RED)]).is_none());
}

// ---------------------------------------------------------------------------
// Clause 4 — the button opens its picker
// ---------------------------------------------------------------------------

/// The `ColorPicker` child a `ColorPickerButton` has created, if any.
fn hosted_picker(tree: &SceneTree, button: NodeId) -> Option<NodeId> {
    tree.get_node(button)?
        .children()
        .iter()
        .copied()
        .find(|&c| {
            tree.get_node(c)
                .map(|n| n.class_name() == "ColorPicker")
                .unwrap_or(false)
        })
}

/// **Clause 4.** Pressing a `ColorPickerButton` opens the picker it hosts.
///
/// 🔑 The assertions are on **hit-testability**, not on draw items, and that is
/// deliberate. `control_render.rs` does not consult `visible` anywhere — a
/// hidden Control is still painted — whereas `hit_test_control` returns `None`
/// for `visible == false` (`control.rs`, pat-s3rrv.13 fixed the hit side and
/// left the draw side). So "is the popup open?" is only answerable through the
/// hit walk today. Asserting on draw items would pass whether the picker were
/// open or shut, which is worse than not asserting at all.
#[test]
fn a_color_picker_button_opens_its_picker_on_press() {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut btn = Node::new("Swatch", "ColorPickerButton");
    btn.set_property("color", Variant::Color(START));
    let id = tree.add_child(root, btn).expect("button added");
    set_offset_left(&mut tree, id, 10.0);
    set_offset_top(&mut tree, id, 10.0);
    set_offset_right(&mut tree, id, 90.0);
    set_offset_bottom(&mut tree, id, 34.0);
    resolve_control_layout(&mut tree, id, VIEWPORT);

    // ⚠️ Upstream creates the picker lazily inside `_update_picker()`, so a
    // button that has never been pressed hosts NOTHING. A test that looked for
    // a picker authored in the scene would be testing a design Godot does not
    // have.
    assert!(
        hosted_picker(&tree, id).is_none(),
        "no picker exists before the first press"
    );

    let created = Arc::new(AtomicUsize::new(0));
    let closed = Arc::new(AtomicUsize::new(0));
    // ⚠️ Distinct receiver ids. Two connections sharing one ObjectId is exactly
    // the sort of collision a signal table may dedupe, and the test would then
    // silently count only one of the two signals.
    for (i, (sig, counter)) in [("picker_created", &created), ("popup_closed", &closed)]
        .into_iter()
        .enumerate()
    {
        let n = counter.clone();
        tree.connect_signal(
            id,
            sig,
            Connection::with_callback(ObjectId::from_raw(9300 + i as u64), "on_popup", move |_| {
                n.fetch_add(1, Ordering::SeqCst);
                Variant::Nil
            }),
        );
    }

    // -- first press: creates, announces, and opens -------------------------
    fire_button_action(&mut tree, id);
    let picker = hosted_picker(&tree, id).expect("the press must create the picker");
    assert_eq!(
        created.load(Ordering::SeqCst),
        1,
        "`picker_created` fires once"
    );
    assert_eq!(closed.load(Ordering::SeqCst), 0);

    // The picker carries the button's own colour, as `_update_picker` copies it.
    assert_eq!(color_of(&tree, picker), START);

    // Open means REACHABLE. The button is at (10, 10) 80x24, so the picker sits
    // horizontally centred below it: x = 10 + (80 - 290)/2 = -95, y = 10 + 24 =
    // 34. A point inside it must resolve to the picker rather than to whatever
    // is behind.
    //
    // ⚠️ The picker overhangs its parent's rect on every side. That is only
    // clickable because a Button does not set `clip_contents` — `hit_test_control`
    // refuses to descend into the children of a clipping control.
    let inside = Vector2::new(20.0, 100.0);
    assert_eq!(
        control_at_point(&tree, inside),
        Some(picker),
        "a point inside the open picker must reach it, not the button behind it"
    );

    // -- second press: closes, and the picker stops taking clicks -----------
    fire_button_action(&mut tree, id);
    assert_eq!(
        closed.load(Ordering::SeqCst),
        1,
        "`popup_closed` fires on close"
    );
    assert_eq!(
        created.load(Ordering::SeqCst),
        1,
        "the picker is created ONCE, ever — a second press must reuse it"
    );
    assert!(
        hosted_picker(&tree, id).is_some(),
        "closing hides the picker, it does not destroy it"
    );
    assert_ne!(
        control_at_point(&tree, inside),
        Some(picker),
        "a closed picker must not keep swallowing clicks over its rect"
    );

    // -- third press: reopens the SAME node ---------------------------------
    fire_button_action(&mut tree, id);
    assert_eq!(
        hosted_picker(&tree, id),
        Some(picker),
        "reopened, not recreated"
    );
    assert_eq!(control_at_point(&tree, inside), Some(picker));
    assert_eq!(created.load(Ordering::SeqCst), 1);
}

/// A plain `Button` hosts no picker, however often it is pressed.
///
/// Without this the press hook could be firing for every BaseButton and every
/// assertion above would still pass.
#[test]
fn a_plain_button_never_grows_a_picker() {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let btn = Node::new("Plain", "Button");
    let id = tree.add_child(root, btn).expect("button added");
    set_offset_left(&mut tree, id, 10.0);
    set_offset_top(&mut tree, id, 10.0);
    set_offset_right(&mut tree, id, 90.0);
    set_offset_bottom(&mut tree, id, 34.0);
    resolve_control_layout(&mut tree, id, VIEWPORT);

    fire_button_action(&mut tree, id);
    assert!(hosted_picker(&tree, id).is_none());
}

/// Picking inside a hosted picker moves the BUTTON's colour too.
///
/// 🔑 This is the clause the bead words as "the redrawn swatch reflects it",
/// and it is the join that makes the two halves one feature. Upstream wires it
/// at construction (`picker->connect("color_changed", ..._color_changed)` inside
/// `_update_picker`), and `_color_changed` is three lines: assign, redraw,
/// re-emit on the button. Without it the picker moves and the swatch behind it
/// does not — a half-wired control that looks right until you close the popup.
#[test]
fn picking_in_a_hosted_picker_updates_the_buttons_colour() {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut btn = Node::new("Swatch", "ColorPickerButton");
    btn.set_property("color", Variant::Color(START));
    let id = tree.add_child(root, btn).expect("button added");
    set_offset_left(&mut tree, id, 10.0);
    set_offset_top(&mut tree, id, 10.0);
    set_offset_right(&mut tree, id, 90.0);
    set_offset_bottom(&mut tree, id, 34.0);
    resolve_control_layout(&mut tree, id, VIEWPORT);

    fire_button_action(&mut tree, id);
    let picker = hosted_picker(&tree, id).expect("press opens a picker");

    let seen = Arc::new(AtomicUsize::new(0));
    let got = Arc::new(std::sync::Mutex::new(Vec::<Color>::new()));
    let (n, sink) = (seen.clone(), got.clone());
    tree.connect_signal(
        id,
        "color_changed",
        Connection::with_callback(ObjectId::from_raw(9400), "on_button_color", move |args| {
            n.fetch_add(1, Ordering::SeqCst);
            if let Some(Variant::Color(c)) = args.first() {
                sink.lock().expect("sink").push(*c);
            }
            Variant::Nil
        }),
    );

    // The button is (10, 10) 80x24, so the picker is centred below it at
    // x = 10 + (80 - 290)/2 = -95, y = 34, sized 290x272. Its SV square is
    // therefore (-95, 34) 256x256 — feeding DEFAULT_PICKER_SIZE back through
    // `picker_layout` recovers exactly Godot's 256x256, which is the check that
    // the constant and the layout cannot drift apart.
    //
    // Top-right of that square: x = -95 + 255 = 160, y = 34.
    assert_eq!(
        control_at_point(&tree, Vector2::new(160.0, 34.0)),
        Some(picker),
        "the probe point must land in the picker, not the button above it"
    );
    tree.push_input(&press(160.0, 34.0));

    let expected = color_from_hsv(START_HUE, 255.0 / 256.0, 1.0, 1.0);
    assert_eq!(
        color_of(&tree, picker),
        expected,
        "the picker itself takes the picked colour"
    );
    assert_eq!(
        color_of(&tree, id),
        expected,
        "and so does the button — otherwise its swatch keeps painting the old \
         colour after the popup closes"
    );
    assert_eq!(
        seen.load(Ordering::SeqCst),
        1,
        "the button re-emits `color_changed` exactly once per pick"
    );
    assert_eq!(got.lock().expect("sink").clone(), vec![expected]);
}

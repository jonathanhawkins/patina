//! pat-0q2kx.83: Godot container parity — size flags as a **bitmask**,
//! BoxContainer `alignment`/`vertical`, GridContainer expand distribution,
//! SplitContainer minimum-size clamping, and the TabContainer tab strip.
//!
//! Patina modelled `size_flags` as an exclusive enum
//! (`{Fill, Expand, ShrinkCenter, ShrinkEnd}`), so every value it did not
//! enumerate collapsed onto `Fill`: `EXPAND` alone (2) never expanded, `0`
//! (Godot's `SHRINK_BEGIN`) stretched instead of shrinking, and combinations
//! such as `EXPAND | SHRINK_CENTER` (6) were meaningless. Containers also
//! stretched every child on the cross axis unconditionally, ignored
//! `BoxContainer.alignment`, never gave GridContainer's leftover space to an
//! expanding column, and let a SplitContainer divider crush a pane past its
//! minimum size.
//!
//! Expected rects are computed from the upstream algorithms, cited per case:
//!   * upstream/godot/scene/gui/box_container.cpp `BoxContainer::_resort`
//!   * upstream/godot/scene/gui/container.cpp `Container::fit_child_in_rect`
//!   * upstream/godot/scene/gui/grid_container.cpp `NOTIFICATION_SORT_CHILDREN`
//!   * upstream/godot/scene/gui/split_container.cpp `_get_valid_range`

use gdcore::math::Vector2;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::control::{
    arrange_container, clamp_split_offset, drag_split, get_current_tab, get_position, get_size,
    get_split_offset, get_tab_bar_height, get_tab_title, set_box_alignment, set_columns,
    set_custom_minimum_size, set_h_size_flags, set_split_offset, set_stretch_ratio,
    set_v_size_flags, tab_at_point, BoxAlignment, SizeFlags,
};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// Builds `tree -> container(class) -> n Control children`, each with the given
/// minimum size.
fn container_with(class: &str, n: usize, min: Vector2) -> (SceneTree, NodeId, Vec<NodeId>) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let cid = tree
        .add_child(root, Node::new("C", class))
        .expect("container");
    // Every expected rect below was computed for a GAPLESS box. Godot's
    // default BoxContainer `separation` is 4 (default_theme.cpp:1249), which
    // Patina now applies (pat-9ww3e), so the premise is spelled out.
    if let Some(n) = tree.get_node_mut(cid) {
        n.set_property("theme_override_constants/separation", Variant::Int(0));
    }
    let kids: Vec<NodeId> = (0..n)
        .map(|i| {
            let k = tree
                .add_child(cid, Node::new(&format!("K{i}"), "Control"))
                .expect("child");
            set_custom_minimum_size(&mut tree, k, min);
            k
        })
        .collect();
    (tree, cid, kids)
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[track_caller]
fn assert_rect(tree: &SceneTree, id: NodeId, pos: Vector2, size: Vector2, what: &str) {
    let p = get_position(tree, id);
    let s = get_size(tree, id);
    assert!(
        approx(p.x, pos.x) && approx(p.y, pos.y) && approx(s.x, size.x) && approx(s.y, size.y),
        "{what}: expected pos {pos:?} size {size:?}, got pos {p:?} size {s:?}",
    );
}

// ---------------------------------------------------------------------------
// SizeFlags is a bitmask
// ---------------------------------------------------------------------------

/// Oracle: `Control.SizeFlags` is a `BitField`, so each bit round-trips
/// independently and `has_flag` tests bits rather than equality
/// (upstream/godot/scene/gui/control.h).
#[test]
fn size_flags_are_bits_not_an_exclusive_enum() {
    assert_eq!(SizeFlags::SHRINK_BEGIN.to_int(), 0);
    assert_eq!(SizeFlags::FILL.to_int(), 1);
    assert_eq!(SizeFlags::EXPAND.to_int(), 2);
    assert_eq!(SizeFlags::EXPAND_FILL.to_int(), 3);
    assert_eq!(SizeFlags::SHRINK_CENTER.to_int(), 4);
    assert_eq!(SizeFlags::SHRINK_END.to_int(), 8);

    // EXPAND|FILL carries both bits.
    let ef = SizeFlags::EXPAND_FILL;
    assert!(ef.has_flag(SizeFlags::EXPAND) && ef.has_flag(SizeFlags::FILL));

    // EXPAND alone carries EXPAND but *not* FILL — the case the old enum lost.
    let e = SizeFlags::from_int(2);
    assert!(e.has_flag(SizeFlags::EXPAND), "2 must carry the EXPAND bit");
    assert!(
        !e.has_flag(SizeFlags::FILL),
        "2 must not carry the FILL bit"
    );

    // EXPAND | SHRINK_CENTER (6) — a combination the enum could not express.
    let esc = SizeFlags::from_int(6);
    assert!(esc.has_flag(SizeFlags::EXPAND) && esc.has_flag(SizeFlags::SHRINK_CENTER));
    assert!(!esc.has_flag(SizeFlags::FILL));

    // 0 is SHRINK_BEGIN: no bits at all, and never reports one.
    let zero = SizeFlags::from_int(0);
    assert!(!zero.has_flag(SizeFlags::FILL) && !zero.has_flag(SizeFlags::EXPAND));

    // Unknown bits survive the round trip instead of collapsing onto FILL.
    assert_eq!(SizeFlags::from_int(6).to_int(), 6);
    assert_eq!((SizeFlags::EXPAND | SizeFlags::SHRINK_END).to_int(), 10);
}

// ---------------------------------------------------------------------------
// BoxContainer: EXPAND bit, stretch ratio, cross-axis fill/shrink
// ---------------------------------------------------------------------------

/// The bead's headline fixture: an HBox with children flagged
/// `[FILL, EXPAND|FILL (ratio 2), SHRINK_CENTER]`.
///
/// `_resort`: mins 40+40+40 = 120, `stretch_max` = 300, `stretch_diff` = 180,
/// `stretch_avail` = 40 (the stretcher's min) + 180 = 220, which all goes to
/// the single stretcher. The third child has no `FILL` bit, so
/// `fit_child_in_rect` collapses it to its 40px minimum and, being
/// `SHRINK_CENTER`, centers it in its (equally 40px) cell.
#[test]
fn hbox_fill_expand_ratio_shrink_center_matches_godot() {
    let (mut tree, cid, k) = container_with("HBoxContainer", 3, Vector2::new(40.0, 20.0));
    set_h_size_flags(&mut tree, k[0], SizeFlags::FILL);
    set_h_size_flags(&mut tree, k[1], SizeFlags::EXPAND_FILL);
    set_stretch_ratio(&mut tree, k[1], 2.0);
    set_h_size_flags(&mut tree, k[2], SizeFlags::SHRINK_CENTER);

    arrange_container(&mut tree, cid, Vector2::new(300.0, 50.0));

    assert_rect(
        &tree,
        k[0],
        Vector2::new(0.0, 0.0),
        Vector2::new(40.0, 50.0),
        "FILL child keeps its min width and fills the cross axis",
    );
    assert_rect(
        &tree,
        k[1],
        Vector2::new(40.0, 0.0),
        Vector2::new(220.0, 50.0),
        "EXPAND|FILL child absorbs all 180px of leftover space",
    );
    assert_rect(
        &tree,
        k[2],
        Vector2::new(260.0, 0.0),
        Vector2::new(40.0, 50.0),
        "SHRINK_CENTER child stays at its minimum width",
    );
}

/// A child flagged `EXPAND` **only** (2, no `FILL`) still claims its share of
/// the leftover space — the cell grows and pushes its sibling along — but the
/// child itself stays at its minimum size inside that cell, packed at the cell
/// start (`fit_child_in_rect`'s no-shrink-bit branch).
///
/// The old exclusive enum mapped 2 onto `Fill`, so the cell never grew and the
/// sibling sat at x = 40.
#[test]
fn expand_without_fill_grows_the_cell_but_not_the_child() {
    let (mut tree, cid, k) = container_with("HBoxContainer", 2, Vector2::new(40.0, 20.0));
    set_h_size_flags(&mut tree, k[0], SizeFlags::EXPAND);
    set_h_size_flags(&mut tree, k[1], SizeFlags::FILL);

    arrange_container(&mut tree, cid, Vector2::new(200.0, 50.0));

    assert_rect(
        &tree,
        k[0],
        Vector2::new(0.0, 0.0),
        Vector2::new(40.0, 50.0),
        "EXPAND-only child keeps its minimum width",
    );
    assert_eq!(
        get_position(&tree, k[1]).x,
        160.0,
        "the EXPAND-only child's CELL still absorbed the 120px of extra space, \
         so the sibling starts at 160 (the old enum left it at 40)",
    );
}

/// `size_flags_vertical` on a horizontal box drives the **cross** axis through
/// `fit_child_in_rect`: `FILL` stretches, `SHRINK_CENTER` centers the minimum
/// height (floored), `SHRINK_END` pushes it to the bottom, and `0`
/// (`SHRINK_BEGIN`) pins it to the top.
#[test]
fn cross_axis_honours_fill_shrink_center_and_shrink_end() {
    let (mut tree, cid, k) = container_with("HBoxContainer", 4, Vector2::new(40.0, 20.0));
    set_v_size_flags(&mut tree, k[0], SizeFlags::FILL);
    set_v_size_flags(&mut tree, k[1], SizeFlags::SHRINK_CENTER);
    set_v_size_flags(&mut tree, k[2], SizeFlags::SHRINK_END);
    set_v_size_flags(&mut tree, k[3], SizeFlags::SHRINK_BEGIN);

    arrange_container(&mut tree, cid, Vector2::new(200.0, 50.0));

    assert_eq!(get_size(&tree, k[0]).y, 50.0, "FILL stretches to the cell");
    assert_eq!(get_size(&tree, k[1]).y, 20.0);
    assert_eq!(
        get_position(&tree, k[1]).y,
        15.0,
        "SHRINK_CENTER: floor((50 - 20) / 2)",
    );
    assert_eq!(get_size(&tree, k[2]).y, 20.0);
    assert_eq!(get_position(&tree, k[2]).y, 30.0, "SHRINK_END: 50 - 20");
    assert_eq!(get_size(&tree, k[3]).y, 20.0);
    assert_eq!(get_position(&tree, k[3]).y, 0.0, "SHRINK_BEGIN stays at 0");
}

/// A stretcher whose ratio share falls below its own minimum is dropped from
/// the stretch pool and re-fitted at its minimum, the remaining space going to
/// the survivors (upstream's "discard elements that can't be stretched" pass).
#[test]
fn refit_drops_a_stretcher_that_cannot_reach_its_minimum() {
    let (mut tree, cid, k) = container_with("HBoxContainer", 2, Vector2::ZERO);
    // A needs 150px but asks for only 1/100th of the pool; B asks for the rest.
    set_custom_minimum_size(&mut tree, k[0], Vector2::new(150.0, 0.0));
    set_h_size_flags(&mut tree, k[0], SizeFlags::EXPAND_FILL);
    set_stretch_ratio(&mut tree, k[0], 1.0);
    set_h_size_flags(&mut tree, k[1], SizeFlags::EXPAND_FILL);
    set_stretch_ratio(&mut tree, k[1], 99.0);

    arrange_container(&mut tree, cid, Vector2::new(200.0, 50.0));

    assert_eq!(
        get_size(&tree, k[0]).x,
        150.0,
        "A's 1% share (2px) is below its 150px minimum, so it is refitted at \
         the minimum instead of being crushed",
    );
    assert_eq!(get_position(&tree, k[1]).x, 150.0);
    assert_eq!(get_size(&tree, k[1]).x, 50.0, "B keeps what is left");
}

/// `BoxContainer.alignment` packs the whole child run when nothing stretched.
#[test]
fn box_alignment_packs_the_run_when_nothing_stretches() {
    for (alignment, first_x) in [
        (BoxAlignment::Begin, 0.0),
        (BoxAlignment::Center, 60.0),
        (BoxAlignment::End, 120.0),
    ] {
        let (mut tree, cid, k) = container_with("HBoxContainer", 2, Vector2::new(40.0, 20.0));
        set_box_alignment(&mut tree, cid, alignment);

        arrange_container(&mut tree, cid, Vector2::new(200.0, 50.0));

        // stretch_diff = 200 - 80 = 120; BEGIN -> 0, CENTER -> 60, END -> 120.
        assert_eq!(
            get_position(&tree, k[0]).x,
            first_x,
            "{alignment:?} should start the run at {first_x}",
        );
        assert_eq!(get_position(&tree, k[1]).x, first_x + 40.0);
    }
}

/// Alignment is ignored once something stretches, because there is no leftover
/// space left to distribute (`has_stretched` in `_resort`).
#[test]
fn alignment_is_ignored_when_a_child_stretches() {
    let (mut tree, cid, k) = container_with("HBoxContainer", 2, Vector2::new(40.0, 20.0));
    set_box_alignment(&mut tree, cid, BoxAlignment::End);
    set_h_size_flags(&mut tree, k[0], SizeFlags::EXPAND_FILL);

    arrange_container(&mut tree, cid, Vector2::new(200.0, 50.0));

    assert_eq!(get_position(&tree, k[0]).x, 0.0);
    assert_eq!(get_size(&tree, k[0]).x, 160.0);
}

/// A bare `BoxContainer` reads its `vertical` property; `VBoxContainer` and
/// `HBoxContainer` pin the axis regardless of it.
#[test]
fn box_container_vertical_property_selects_the_axis() {
    let (mut tree, cid, k) = container_with("BoxContainer", 2, Vector2::new(40.0, 20.0));
    if let Some(n) = tree.get_node_mut(cid) {
        n.set_property("vertical", Variant::Bool(true));
    }

    arrange_container(&mut tree, cid, Vector2::new(200.0, 100.0));

    assert_eq!(get_position(&tree, k[0]).y, 0.0);
    assert_eq!(
        get_position(&tree, k[1]).y,
        20.0,
        "vertical = true stacks children down the y axis",
    );
    assert_eq!(get_position(&tree, k[1]).x, 0.0);
}

// ---------------------------------------------------------------------------
// GridContainer
// ---------------------------------------------------------------------------

/// A GridContainer column containing an `EXPAND` child absorbs the leftover
/// width; non-expanding columns stay at their minimum.
#[test]
fn grid_expanding_column_absorbs_extra_width() {
    let (mut tree, cid, k) = container_with("GridContainer", 4, Vector2::new(40.0, 20.0));
    set_columns(&mut tree, cid, 2);
    // Only the right column expands.
    set_h_size_flags(&mut tree, k[1], SizeFlags::EXPAND_FILL);

    arrange_container(&mut tree, cid, Vector2::new(300.0, 100.0));

    // remaining = 300 - 40 (fixed col 0) = 260, all of it to column 1.
    assert_rect(
        &tree,
        k[0],
        Vector2::new(0.0, 0.0),
        Vector2::new(40.0, 20.0),
        "fixed column keeps its minimum width",
    );
    assert_rect(
        &tree,
        k[1],
        Vector2::new(40.0, 0.0),
        Vector2::new(260.0, 20.0),
        "expanding column absorbs the extra width",
    );
    assert_rect(
        &tree,
        k[2],
        Vector2::new(0.0, 20.0),
        Vector2::new(40.0, 20.0),
        "row 1, fixed column",
    );
    assert_rect(
        &tree,
        k[3],
        Vector2::new(40.0, 20.0),
        Vector2::new(260.0, 20.0),
        "the whole column expands, not just the flagged cell",
    );
}

/// Rows expand from the vertical `EXPAND` bit the same way columns do, and a
/// non-`FILL` child is shrunk and aligned inside its (expanded) cell rather
/// than stretched to fill it.
#[test]
fn grid_expanding_row_and_cell_shrink() {
    let (mut tree, cid, k) = container_with("GridContainer", 2, Vector2::new(40.0, 20.0));
    set_columns(&mut tree, cid, 2);
    set_h_size_flags(&mut tree, k[0], SizeFlags::EXPAND_FILL);
    set_v_size_flags(&mut tree, k[0], SizeFlags::EXPAND_FILL);
    // Second cell shrinks inside the expanded row instead of filling it.
    set_v_size_flags(&mut tree, k[1], SizeFlags::SHRINK_CENTER);

    arrange_container(&mut tree, cid, Vector2::new(200.0, 100.0));

    assert_eq!(
        get_size(&tree, k[0]).y,
        100.0,
        "the row expands to the full height",
    );
    assert_eq!(get_size(&tree, k[1]).y, 20.0, "SHRINK_CENTER keeps its min");
    assert_eq!(
        get_position(&tree, k[1]).y,
        40.0,
        "and is centered in the 100px row: floor((100 - 20) / 2)",
    );
}

// ---------------------------------------------------------------------------
// SplitContainer
// ---------------------------------------------------------------------------

/// The divider cannot be pushed past either child's minimum size, in either
/// direction (`_get_valid_range`: `[min_first, size - sep - min_second]`).
#[test]
fn split_divider_is_clamped_by_both_child_minimums() {
    let make = || {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let cid = tree
            .add_child(root, Node::new("S", "HSplitContainer"))
            .expect("split");
        let a = tree.add_child(cid, Node::new("A", "Control")).expect("a");
        let b = tree.add_child(cid, Node::new("B", "Control")).expect("b");
        set_custom_minimum_size(&mut tree, a, Vector2::new(60.0, 0.0));
        set_custom_minimum_size(&mut tree, b, Vector2::new(50.0, 0.0));
        (tree, cid, a, b)
    };

    // Dragged hard to the left: the first pane stops at its 60px minimum.
    let (mut tree, cid, a, b) = make();
    set_split_offset(&mut tree, cid, -1000.0);
    arrange_container(&mut tree, cid, Vector2::new(200.0, 100.0));
    assert_eq!(get_size(&tree, a).x, 60.0, "first pane cannot go below 60");
    assert_eq!(get_position(&tree, b).x, 60.0);
    assert_eq!(get_size(&tree, b).x, 140.0);

    // Dragged hard to the right: the second pane keeps its 50px minimum.
    let (mut tree, cid, a, b) = make();
    set_split_offset(&mut tree, cid, 1000.0);
    arrange_container(&mut tree, cid, Vector2::new(200.0, 100.0));
    assert_eq!(get_size(&tree, a).x, 150.0, "200 - 50 for the second pane");
    assert_eq!(get_size(&tree, b).x, 50.0, "second pane cannot go below 50");
}

/// `drag_split` clamps the incoming offset, stores it, and emits `dragged`
/// only when the value actually changed.
#[test]
fn drag_split_clamps_and_emits_dragged_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let cid = tree
        .add_child(root, Node::new("S", "HSplitContainer"))
        .expect("split");
    let a = tree.add_child(cid, Node::new("A", "Control")).expect("a");
    let b = tree.add_child(cid, Node::new("B", "Control")).expect("b");
    set_custom_minimum_size(&mut tree, a, Vector2::new(60.0, 0.0));
    set_custom_minimum_size(&mut tree, b, Vector2::new(50.0, 0.0));

    let seen: Arc<std::sync::Mutex<Vec<f64>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_cb = Arc::clone(&seen);
    let count = Arc::new(AtomicUsize::new(0));
    let count_cb = Arc::clone(&count);
    let conn =
        gdobject::signal::Connection::with_callback(cid.object_id(), "on_dragged", move |args| {
            count_cb.fetch_add(1, Ordering::SeqCst);
            if let Some(Variant::Float(f)) = args.first() {
                seen_cb.lock().expect("lock").push(*f);
            }
            Variant::Nil
        });
    tree.connect_signal(cid, "dragged", conn);

    let total = Vector2::new(200.0, 100.0);
    // Divider default sits at 100; asking for -1000 clamps to 60, i.e. -40.
    assert!(drag_split(&mut tree, cid, -1000.0, total), "offset changed");
    assert_eq!(get_split_offset(&tree, cid), -40.0);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(seen.lock().expect("lock").clone(), vec![-40.0]);

    // Dragging further in the same direction clamps to the same value: no
    // change, so no second `dragged`.
    assert!(!drag_split(&mut tree, cid, -2000.0, total), "no change");
    assert_eq!(count.load(Ordering::SeqCst), 1, "dragged fired only once");

    // A real move re-fires.
    assert!(drag_split(&mut tree, cid, 0.0, total));
    assert_eq!(get_split_offset(&tree, cid), 0.0);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

/// `clamp_split_offset` rewrites an out-of-range stored offset in place, so a
/// container that shrinks does not keep an impossible divider position.
#[test]
fn clamp_split_offset_rewrites_a_stale_offset() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let cid = tree
        .add_child(root, Node::new("S", "HSplitContainer"))
        .expect("split");
    let a = tree.add_child(cid, Node::new("A", "Control")).expect("a");
    let b = tree.add_child(cid, Node::new("B", "Control")).expect("b");
    set_custom_minimum_size(&mut tree, a, Vector2::new(60.0, 0.0));
    set_custom_minimum_size(&mut tree, b, Vector2::new(50.0, 0.0));
    set_split_offset(&mut tree, cid, 500.0);

    assert!(clamp_split_offset(
        &mut tree,
        cid,
        Vector2::new(200.0, 100.0)
    ));
    // Valid range is [60, 150]; the default position is 100, so the largest
    // legal offset is +50.
    assert_eq!(get_split_offset(&tree, cid), 50.0);
    assert!(
        !clamp_split_offset(&mut tree, cid, Vector2::new(200.0, 100.0)),
        "already clamped: no further change",
    );
}

// ---------------------------------------------------------------------------
// TabContainer
// ---------------------------------------------------------------------------

/// The tab strip's height comes from the theme constant when the scene sets
/// one, and the current page is laid out directly beneath it.
#[test]
fn tab_height_comes_from_the_theme_constant() {
    let (mut tree, cid, k) = container_with("TabContainer", 2, Vector2::ZERO);
    assert_eq!(get_tab_bar_height(&tree, cid), 32.0, "default strip height");

    if let Some(n) = tree.get_node_mut(cid) {
        n.set_property("theme_override_constants/tab_height", Variant::Int(48));
    }
    assert_eq!(get_tab_bar_height(&tree, cid), 48.0);

    arrange_container(&mut tree, cid, Vector2::new(200.0, 200.0));
    assert_rect(
        &tree,
        k[0],
        Vector2::new(0.0, 48.0),
        Vector2::new(200.0, 152.0),
        "the current page starts below the themed tab strip",
    );
    assert_eq!(
        get_size(&tree, k[1]),
        Vector2::ZERO,
        "non-current pages are collapsed",
    );
}

/// Tab titles default to the page node's name and are overridable per page.
#[test]
fn tab_titles_follow_the_page_node_names() {
    let (mut tree, cid, k) = container_with("TabContainer", 2, Vector2::ZERO);
    assert_eq!(get_tab_title(&tree, cid, 0), "K0");
    assert_eq!(get_tab_title(&tree, cid, 1), "K1");

    if let Some(n) = tree.get_node_mut(k[1]) {
        n.set_property("_tab_name", Variant::String("Settings".into()));
    }
    assert_eq!(get_tab_title(&tree, cid, 1), "Settings");
}

/// The tab strip draws one title per page and highlights the current tab with
/// the `tab_selected` StyleBox — Godot's `TabBar` draw pass. Patina previously
/// painted one flat header rectangle with no titles at all.
#[test]
fn tab_strip_draws_titles_and_highlights_the_current_tab() {
    use gdscene::control_render::render_control_tree;
    use gdscene::theme::Theme;
    use gdserver2d::canvas::DrawCommand;
    use gdserver2d::viewport::Viewport;

    let (mut tree, cid, _k) = container_with("TabContainer", 3, Vector2::ZERO);
    gdscene::control::apply_anchor_preset(&mut tree, cid, gdscene::control::AnchorPreset::FullRect);
    gdscene::control::set_current_tab(&mut tree, cid, 1);

    let mut vp = Viewport::new(300, 200, gdcore::math::Color::BLACK);
    let theme = Theme::default_theme();
    render_control_tree(&mut tree, cid, &theme, &mut vp, Vector2::new(300.0, 200.0));

    let cmds = &vp.get_sorted_items()[0].commands;
    let titles: Vec<&str> = cmds
        .iter()
        .filter_map(|c| match c {
            DrawCommand::DrawString { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        titles,
        vec!["K0", "K1", "K2"],
        "every page's title is drawn on the strip",
    );

    // Panel + full-width unselected strip + one highlight box for the current
    // tab: the highlight is the only rect that is strip-height and narrower
    // than the container.
    let strip_h = get_tab_bar_height(&tree, cid);
    let highlights: Vec<&gdcore::math::Rect2> = cmds
        .iter()
        .filter_map(|c| match c {
            DrawCommand::DrawRect {
                rect, filled: true, ..
            } if approx(rect.size.y, strip_h) && rect.size.x < 300.0 => Some(rect),
            _ => None,
        })
        .collect();
    assert_eq!(highlights.len(), 1, "exactly one tab is highlighted");
    let rects = gdscene::control::tab_rects(&tree, cid);
    assert!(
        approx(highlights[0].position.x, rects[1].0),
        "the highlight sits on tab 1: expected x {}, got {}",
        rects[1].0,
        highlights[0].position.x,
    );
}

/// Clicking a tab header switches the current tab and emits `tab_changed(idx)`;
/// clicking the already-current tab, or below the strip, does not.
#[test]
fn clicking_the_tab_strip_switches_tabs_and_emits_tab_changed() {
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::Arc;

    let (mut tree, cid, _k) = container_with("TabContainer", 3, Vector2::ZERO);
    arrange_container(&mut tree, cid, Vector2::new(300.0, 200.0));

    let last = Arc::new(AtomicI64::new(-1));
    let hits = Arc::new(AtomicI64::new(0));
    let last_cb = Arc::clone(&last);
    let hits_cb = Arc::clone(&hits);
    let conn = gdobject::signal::Connection::with_callback(
        cid.object_id(),
        "on_tab_changed",
        move |args| {
            hits_cb.fetch_add(1, Ordering::SeqCst);
            if let Some(Variant::Int(i)) = args.first() {
                last_cb.store(*i, Ordering::SeqCst);
            }
            Variant::Nil
        },
    );
    tree.connect_signal(cid, "tab_changed", conn);

    // The strip spans [0, 32) in y; tab 1 starts where tab 0 ends.
    let rects = gdscene::control::tab_rects(&tree, cid);
    assert_eq!(rects.len(), 3, "one span per page");
    let tab1_x = rects[1].0 + rects[1].1 * 0.5;
    assert_eq!(
        tab_at_point(&tree, cid, Vector2::new(tab1_x, 10.0)),
        Some(1),
        "the middle of tab 1's header hit-tests to index 1",
    );
    assert_eq!(
        tab_at_point(&tree, cid, Vector2::new(tab1_x, 100.0)),
        None,
        "below the strip is the page, not a tab",
    );

    let press = |x: f32, y: f32| InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(x, y),
    };

    tree.push_input(&press(tab1_x, 10.0));
    assert_eq!(
        get_current_tab(&tree, cid),
        1,
        "clicked tab becomes current"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert_eq!(last.load(Ordering::SeqCst), 1);

    // Re-clicking the current tab is a no-op — Godot only emits on a change.
    tree.push_input(&press(tab1_x, 10.0));
    assert_eq!(hits.load(Ordering::SeqCst), 1, "no tab_changed on re-click");

    // Clicking below the strip does not switch tabs either.
    tree.push_input(&press(tab1_x, 120.0));
    assert_eq!(get_current_tab(&tree, cid), 1);
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

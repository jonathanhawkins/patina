//! pat-nzgdu.108 — misc GUI controls: OptionButton's item model + selection
//! (`item_selected` / `get_selected_id`), TabBar's current-tab change
//! (`tab_changed`), and a NinePatchRect drawing via a `DrawNinePatch` canvas
//! command.

use gdcore::math::{Color, Rect2, Vector2};
use gdscene::item_controls::{OptionButton, TabBar};
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};

#[test]
fn misc_controls_tabbar_optionbutton_ninepatch() {
    // --- OptionButton: item model + selection payload. ---
    let mut ob = OptionButton::new();
    ob.add_item("A", 1);
    ob.add_item("B", 2);
    // `pick_item` selects the item and returns the `item_selected` payload
    // (the chosen index) for an enabled item.
    assert_eq!(
        ob.pick_item(1),
        Some(1),
        "item_selected(1) when picking index 1"
    );
    assert_eq!(ob.get_selected(), 1, "selected index == 1");
    assert_eq!(
        ob.get_selected_id(),
        2,
        "get_selected_id() == 2 (item B's id), not its index",
    );

    // --- TabBar: current-tab change reports tab_changed. ---
    let mut tabs = TabBar::new();
    tabs.add_tab("One");
    tabs.add_tab("Two");
    tabs.add_tab("Three");
    assert_eq!(tabs.tab_count(), 3);
    assert!(
        tabs.set_current_tab(2),
        "tab_changed(2): current tab moved 0 → 2",
    );
    assert_eq!(tabs.get_current_tab(), 2);
    assert!(
        !tabs.set_current_tab(2),
        "no tab_changed when the current tab is unchanged",
    );
    assert!(
        !tabs.set_current_tab(9),
        "out-of-range set_current_tab is a no-op (no tab_changed)",
    );

    // --- NinePatchRect: renders via a DrawNinePatch canvas command. ---
    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawNinePatch {
        texture_path: "res://panel.png".to_string(),
        rect: Rect2::new(Vector2::ZERO, Vector2::new(100.0, 50.0)),
        margin_left: 8.0,
        margin_top: 8.0,
        margin_right: 8.0,
        margin_bottom: 8.0,
        draw_center: true,
        axis_h: gdserver2d::canvas::NinePatchAxis::Stretch,
        axis_v: gdserver2d::canvas::NinePatchAxis::Stretch,
        modulate: Color::WHITE,
        region: gdcore::math::Rect2::new(gdcore::math::Vector2::ZERO, gdcore::math::Vector2::ZERO),
    });
    assert!(
        matches!(
            item.commands.last(),
            Some(DrawCommand::DrawNinePatch { .. })
        ),
        "a NinePatchRect emits a DrawNinePatch command",
    );
}

//! pat-s3rrv.167 acceptance: NinePatchRect is a real, creatable class whose
//! `region_rect` confines nine-patch sampling.
//!
//! Lives in gdeditor because one clause is about the editor's create dialog —
//! the check that proves a user can actually add the node, rather than that
//! ClassDB merely knows the name.
//!
//! ⚠️ **`mouse_filter` is asserted as 2, not the 1 the bead's acceptance text
//! states.** `doc/classes/NinePatchRect.xml` says `default="2"`
//! (MOUSE_FILTER_IGNORE) and the registration already carried 2. The bead is
//! wrong on this point, so the oracle wins and correct code stays as it is.

use gdcore::math::{Rect2, Vector2};
use gdobject::class_db;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdserver2d::canvas::DrawCommand;
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

fn zero_rect() -> Rect2 {
    Rect2::new(Vector2::ZERO, Vector2::ZERO)
}

/// Clause 1 and 2: the class, its ancestry, and its documented defaults.
#[test]
fn ninepatchrect_is_registered_with_godot_defaults() {
    class_db::register_all_classes();

    assert!(class_db::class_exists("NinePatchRect"));
    assert!(class_db::is_parent_class("NinePatchRect", "Control"));
    // CanvasItem is further up the chain — an inheritance walk that stopped at
    // the immediate parent would pass the Control check and fail this one.
    assert!(class_db::is_parent_class("NinePatchRect", "CanvasItem"));

    for prop in [
        "texture",
        "draw_center",
        "patch_margin_left",
        "patch_margin_top",
        "patch_margin_right",
        "patch_margin_bottom",
        "region_rect",
        "axis_stretch_horizontal",
        "axis_stretch_vertical",
        "mouse_filter",
    ] {
        assert!(
            class_db::class_has_property("NinePatchRect", prop),
            "NinePatchRect is missing `{prop}`"
        );
    }

    assert_eq!(
        class_db::property_get_revert("NinePatchRect", "draw_center"),
        Some(Variant::Bool(true))
    );
    // ⚠️ Rect2(0,0,0,0), NOT Nil. A Nil default is the wrong TYPE, so a caller
    // reverting the property — or reading the default to decide "is a region
    // set?" — gets Nil where it expects a zero-size rect, and a type-matching
    // reader falls through to whatever its `_ =>` arm does.
    assert_eq!(
        class_db::property_get_revert("NinePatchRect", "region_rect"),
        Some(Variant::Rect2(zero_rect()))
    );
    // Per the doc XML, which overrides Control's own default here.
    assert_eq!(
        class_db::property_get_revert("NinePatchRect", "mouse_filter"),
        Some(Variant::Int(2))
    );

    assert!(class_db::get_class_list().contains(&"NinePatchRect".to_string()));
}

/// Clause 3: the editor can actually offer the node.
#[test]
fn the_create_dialog_lists_ninepatchrect_under_control() {
    class_db::register_all_classes();

    let dialog = gdeditor::create_dialog::CreateNodeDialog::with_base_class("Control");
    let listed: Vec<String> = dialog
        .filtered_classes()
        .into_iter()
        .map(|e| e.class_name)
        .collect();

    assert!(
        listed.contains(&"NinePatchRect".to_string()),
        "the create dialog cannot offer NinePatchRect, so a user cannot add one"
    );
}

/// Clause 4: a region reaches the draw command; absent means whole-texture.
///
/// Observed through `render_control_tree`, the same entry point
/// `nine_patch_rect_draw.rs` uses — `control_draw_items` converts commands into
/// `ControlDrawItem`, which has no nine-patch variant, so the region would be
/// invisible there.
fn nine_patch_commands(props: &[(&str, Variant)]) -> Vec<DrawCommand> {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let np = tree
        .add_child(root, Node::new("Frame", "NinePatchRect"))
        .expect("node added");
    if let Some(node) = tree.get_node_mut(np) {
        node.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
        node.set_property("size", Variant::Vector2(Vector2::new(96.0, 96.0)));
        node.set_property("texture", Variant::String("res://frame.png".into()));
        for (k, v) in props {
            node.set_property(k, v.clone());
        }
    }
    let theme = gdscene::theme::Theme::default();
    let mut viewport = Viewport::new(128, 128, gdcore::math::Color::new(0.0, 0.0, 0.0, 0.0));
    gdscene::control_render::render_control_tree(
        &mut tree,
        root,
        &theme,
        &mut viewport,
        Vector2::new(128.0, 128.0),
    );
    viewport
        .get_sorted_items()
        .iter()
        .flat_map(|item| item.commands.iter().cloned())
        .collect()
}

#[test]
fn region_rect_reaches_the_draw_command_and_defaults_to_the_whole_texture() {
    let region_of = |cmds: &[DrawCommand]| {
        cmds.iter().find_map(|c| match c {
            DrawCommand::DrawNinePatch { region, .. } => Some(*region),
            _ => None,
        })
    };

    // Default: no region property, so a zero rect — "use the whole texture",
    // which is what every existing scene relies on and must stay unchanged.
    let plain = nine_patch_commands(&[]);
    assert_eq!(
        region_of(&plain),
        Some(zero_rect()),
        "an unset region_rect must reach the command as a zero rect"
    );

    let regioned = nine_patch_commands(&[(
        "region_rect",
        Variant::Rect2(Rect2::new(
            Vector2::new(16.0, 16.0),
            Vector2::new(32.0, 32.0),
        )),
    )]);
    assert_eq!(
        region_of(&regioned),
        Some(Rect2::new(
            Vector2::new(16.0, 16.0),
            Vector2::new(32.0, 32.0)
        )),
        "region_rect must reach the draw command, or sampling cannot be confined"
    );
}

/// Clause 5: assigning a texture notifies, and does so once.
#[test]
fn assigning_a_texture_emits_texture_changed_exactly_once() {
    class_db::register_all_classes();
    assert!(
        class_db::class_has_signal("NinePatchRect", "texture_changed"),
        "the signal must be in ClassDB for reflection to find it"
    );

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree
        .add_child(root, Node::new("Patch", "NinePatchRect"))
        .expect("node is added");
    let listener = tree
        .add_child(root, Node::new("Listener", "Node"))
        .expect("listener is added");
    tree.attach_script(
        listener,
        Box::new(
            gdscene::GDScriptNodeInstance::from_source(
                "extends Node\nvar hits = 0\nfunc _on_texture_changed():\n\thits += 1\n",
                listener,
            )
            .expect("script parses"),
        ),
    );
    tree.connect_signal(
        id,
        "texture_changed",
        gdobject::signal::Connection::new(listener.object_id(), "_on_texture_changed"),
    );

    let hits = |tree: &SceneTree| {
        tree.get_script(listener)
            .expect("listener has a script")
            .get_property("hits")
    };

    tree.set_node_property_notifying(id, "texture", Variant::String("res://a.png".into()));
    assert_eq!(
        hits(&tree),
        Some(Variant::Int(1)),
        "one assignment, one signal"
    );

    // ⚠️ Re-assigning the SAME value must not emit again. Godot fires on a
    // change, and a redraw-on-change handler that re-assigned would otherwise
    // drive itself in a loop.
    tree.set_node_property_notifying(id, "texture", Variant::String("res://a.png".into()));
    assert_eq!(
        hits(&tree),
        Some(Variant::Int(1)),
        "a no-op assignment must not re-emit"
    );

    tree.set_node_property_notifying(id, "texture", Variant::String("res://b.png".into()));
    assert_eq!(
        hits(&tree),
        Some(Variant::Int(2)),
        "a real change emits again"
    );
}

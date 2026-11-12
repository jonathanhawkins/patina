//! pat-s3rrv.72, MenuBar half: the title strip draws, opens the menu you
//! clicked, and switches menus on hover while one is open.
//!
//! Godot ground truth: `scene/gui/menu_bar.cpp` — a Control-drawn strip of
//! titles, one per child `PopupMenu`, that switches menus on hover **while a
//! menu is already open**. Moving across a closed bar opens nothing.
//!
//! ⚠️ `menu::MenuBarModel` — the open/hover state machine — had **zero callers
//! anywhere in the workspace** before this. Its own module doc records that it
//! was written to fix exactly this for `PopupMenu` and then repeated the
//! pattern: a model with real behaviour that nothing could reach.
//!
//! The two assertions that matter are about *which* menu opens. A MenuBar owns
//! one PopupMenu per title, so a press must open the menu under the pointer —
//! the existing `open_popup_for` takes the FIRST PopupMenu child, which is
//! right for a MenuButton and wrong for every title but the leftmost.

use gdcore::math::Vector2;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::control_render::ControlDrawItem;
use gdscene::menu::MenuBarModel;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::Theme;
use gdvariant::{Dictionary, Variant};

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

fn press(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(x, y),
    }
}

fn release(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
        position: Vector2::new(x, y),
    }
}

fn motion(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseMotion {
        position: Vector2::new(x, y),
        relative: Vector2::new(0.0, 0.0),
    }
}

fn item(text: &str, id: i64) -> Variant {
    let mut d = Dictionary::new();
    d.insert(Variant::String("text".into()), Variant::String(text.into()));
    d.insert(Variant::String("id".into()), Variant::Int(id));
    Variant::Dictionary(d.into())
}

/// A MenuBar at the origin with two menus, each holding one uniquely-named row
/// so the drawn panel says which menu is open:
/// `File` -> `NewFile` (id 1), `Edit` -> `CopyEdit` (id 2).
///
/// Both titles are 4 characters, so each cell is
/// `4 * TITLE_CHAR_WIDTH + 2 * TITLE_PADDING` wide and title 1 begins at that
/// offset — derived below rather than hard-coded.
fn bar_scene() -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let bar = tree.add_child(root, Node::new("Bar", "MenuBar")).unwrap();
    gdscene::control::set_position(&mut tree, bar, Vector2::new(0.0, 0.0));
    gdscene::control::set_size(&mut tree, bar, Vector2::new(200.0, 24.0));

    for (name, row, id) in [("File", "NewFile", 1i64), ("Edit", "CopyEdit", 2)] {
        let popup = tree.add_child(bar, Node::new(name, "PopupMenu")).unwrap();
        tree.get_node_mut(popup)
            .unwrap()
            .set_property("items", Variant::Array(vec![item(row, id)].into()));
    }
    (tree, bar)
}

fn draw_texts(tree: &mut SceneTree) -> Vec<String> {
    let root = tree.root_id();
    gdscene::control_render::control_draw_items(
        tree,
        root,
        &Theme::default(),
        Vector2::new(400.0, 200.0),
    )
    .into_iter()
    .filter_map(|d| match d {
        ControlDrawItem::Text { text, .. } => Some(text),
        _ => None,
    })
    .collect()
}

/// The centre of title `index`, in the bar's own space (the bar sits at 0,0).
fn title_centre(index: usize) -> f32 {
    let w = MenuBarModel::title_width("File");
    index as f32 * w + w * 0.5
}

// ---------------------------------------------------------------------------

/// The strip draws one title per child PopupMenu.
///
/// ⚠️ This also pins the membership gate: `MenuBar` was absent from
/// `control_render::CONTROL_CLASSES`, and the draw walk skips any node that
/// fails it — so the arm was unreachable and the strip would have been laid
/// out, clickable, and invisible.
#[test]
fn the_title_strip_draws_one_title_per_menu() {
    let (mut tree, _bar) = bar_scene();
    let texts = draw_texts(&mut tree);

    for title in ["File", "Edit"] {
        assert!(
            texts.iter().any(|t| t == title),
            "the strip must draw the title {title:?}; got {texts:?}"
        );
    }
}

/// A press opens the menu under the pointer, not the first one.
///
/// This is the whole difference between a MenuBar and a MenuButton. Opening the
/// first child would look correct for as long as anyone only ever clicked the
/// leftmost title.
#[test]
fn pressing_a_title_opens_that_titles_menu() {
    let (mut tree, _bar) = bar_scene();

    tree.push_input(&press(title_centre(1), 12.0));
    let texts = draw_texts(&mut tree);

    assert!(
        texts.iter().any(|t| t == "CopyEdit"),
        "pressing the SECOND title must open the second menu; got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t == "NewFile"),
        "and not the first menu's rows — opening the first PopupMenu child is \
         right for a MenuButton and wrong for every title but the leftmost. \
         Got {texts:?}"
    );
}

/// While a menu is open, hovering another title switches to it.
#[test]
fn hovering_another_title_switches_menus_while_open() {
    let (mut tree, _bar) = bar_scene();

    tree.push_input(&press(title_centre(1), 12.0));
    tree.push_input(&motion(title_centre(0), 12.0));
    let texts = draw_texts(&mut tree);

    assert!(
        texts.iter().any(|t| t == "NewFile"),
        "hovering the first title while the second's menu is open must switch \
         to it; got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t == "CopyEdit"),
        "and the previously open menu must close; got {texts:?}"
    );
}

/// ⚠️ Moving across a CLOSED bar opens nothing.
///
/// This is why `MenuBarModel` keeps "open" and "hovered" apart, and it is the
/// assertion that stops the fix for the test above from becoming
/// open-on-hover — a menu bar that ambushes the pointer on its way past.
#[test]
fn hovering_a_closed_bar_opens_nothing() {
    let (mut tree, _bar) = bar_scene();

    tree.push_input(&motion(title_centre(0), 12.0));
    tree.push_input(&motion(title_centre(1), 12.0));
    let texts = draw_texts(&mut tree);

    for row in ["NewFile", "CopyEdit"] {
        assert!(
            !texts.iter().any(|t| t == row),
            "no menu may open from hover alone; got {texts:?}"
        );
    }
}

/// Choosing a row from a bar menu activates it and closes the popup.
#[test]
fn choosing_a_row_from_a_bar_menu_closes_it() {
    let (mut tree, _bar) = bar_scene();

    tree.push_input(&press(title_centre(0), 12.0));
    // The panel hangs under the strip, so its first row starts at TITLE_HEIGHT.
    let row_y = MenuBarModel::TITLE_HEIGHT + 4.0;
    tree.push_input(&press(title_centre(0), row_y));
    tree.push_input(&release(title_centre(0), row_y));

    let texts = draw_texts(&mut tree);
    assert!(
        !texts.iter().any(|t| t == "NewFile"),
        "activating a row closes the menu; got {texts:?}"
    );
    // The strip itself is still drawn — only the panel went away.
    assert!(
        texts.iter().any(|t| t == "File"),
        "the strip survives the menu closing; got {texts:?}"
    );
}

//! pat-8g7ea: the top menu bar + run toolbar permanent chrome, enumerated from
//! Godot's `editor_node.cpp` and asserted against the served DOM.
//!
//! GODOT GROUND TRUTH — `EditorNode::EditorNode()` fills `title_bar` in this
//! order (upstream/godot/editor/editor_node.cpp):
//!   1. `main_menu_bar` (MenuBar: Scene, Project, Debug, Editor, Help) OR
//!      `main_menu_button` ("Main Menu", TripleBar icon) when
//!      `interface/editor/collapse_main_menu` is set — `_update_main_menu_type()`
//!      keeps exactly one of the two in the tree.
//!   2. `left_spacer` holding `project_title`, whose text `_update_title()`
//!      formats as `(*) <scene file> - <project name>`.
//!   3. `main_editor_button_hb` — the main-screen switch (2D/3D/Script/Game/
//!      AssetLib).
//!   4. `right_spacer`, then `project_run_bar` (editor_run_bar.cpp: play, pause,
//!      stop, play scene, play custom scene, movie maker).
//!   5. `right_menu_hb` — `renderer` (OptionButton), then `update_spinner`
//!      (MenuButton: Update Continuously / Update When Changed / sep / Hide
//!      Update Spinner).
//!
//! Patina already served 1 (the bar form), 3, 4 and the renderer. MISSING and
//! added by this bead: `project_title`, `update_spinner`, and the collapsed
//! `main_menu_button` — plus Godot's own renderer tooltip, which Patina had
//! replaced with "Rendering method".
//!
//! ⚠️ This file inspects the INITIAL document only, like its sibling chrome
//! tests. That is the right scope for *permanent* chrome — Godot builds these
//! in the constructor and only ever toggles visibility — but it means a
//! renderer that later clears the top bar would not be caught here.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn make_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("World", "Node2D")).unwrap();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

fn editor_html(port: u16) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream
        .write_all(b"GET /editor HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}

/// The enumeration itself: every element Godot's title bar carries.
///
/// Most of these already passed before this bead; they are asserted anyway
/// because the diff that added the new chrome edits the SAME `#menu-bar` block,
/// so they are the regression guard for it.
#[test]
fn top_bar_carries_every_element_godot_puts_in_the_title_bar() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    // 1. The five menus Godot's MenuBar has (`_add_to_main_menu` calls).
    for menu in ["scene", "project", "debug", "editor", "help"] {
        assert!(
            html.contains(&format!(r#"data-menu="{menu}""#)),
            "menu bar is missing Godot's `{menu}` menu"
        );
    }

    // 3. The main-screen switch. Godot 4.4 added Game between Script and
    //    AssetLib; the order is the order of the buttons in the editor.
    for mode in ["2d", "3d", "script", "game", "assetlib"] {
        assert!(
            html.contains(&format!(r#"data-mode="{mode}""#)),
            "main-screen switch is missing `{mode}`"
        );
    }

    // 4. The run toolbar (editor_run_bar.cpp).
    for btn in [
        r#"id="btn-play""#,
        r#"id="btn-pause""#,
        r#"id="btn-stop""#,
        r#"id="btn-play-current""#,
        r#"id="btn-play-custom""#,
        r#"id="btn-movie-maker""#,
    ] {
        assert!(html.contains(btn), "run toolbar is missing `{btn}`");
    }

    // 5. The renderer, then the spinner — in that order, as in `right_menu_hb`.
    let renderer_at = html
        .find(r#"id="renderer-select""#)
        .expect("renderer dropdown missing from the top bar");
    let spinner_at = html
        .find(r#"id="update-spinner""#)
        .expect("update spinner missing from the top bar");
    assert!(
        renderer_at < spinner_at,
        "Godot adds `renderer` to right_menu_hb before `update_spinner`; the \
         served order is reversed"
    );

    handle.stop();
}

/// `project_title` — the label Godot puts in `left_spacer`.
///
/// 🔑 THE INITIAL TEXT IS THE FALLBACK, NOT A PLACEHOLDER. `_update_title()`
/// substitutes "Unnamed Project" when `application/config/name` is empty, so
/// that exact string is what a fresh project must show.
#[test]
fn top_bar_has_the_project_title_label() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    for needle in [
        r#"id="project-title""#,
        r#"class="project-title""#,
        "Unnamed Project",
        // The `_update_title()` rule itself, in the renderer that applies it.
        "function updateProjectTitle(",
        r#"<span class="unsaved">(*)</span> "#,
        "' - ' + appname",
        // Fed from the same two facts as the scene-file indicator, so the two
        // cannot disagree about whether the scene is modified.
        "updateProjectTitle(lastSceneFile, lastSceneModified)",
        // The project name comes from `application/config/name`.
        "data.project_name",
    ] {
        assert!(
            html.contains(needle),
            "project title chrome is missing `{needle}`"
        );
    }

    handle.stop();
}

/// `update_spinner` and its popup, verbatim from `editor_node.cpp`.
#[test]
fn top_bar_has_the_update_spinner_and_its_popup() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    for needle in [
        r#"id="update-spinner-wrap""#,
        r#"id="update-spinner""#,
        r#"id="update-spinner-popup""#,
        r#"aria-label="Update Mode""#,
        // Godot's three popup entries, in order, with the separator between
        // the radio pair and the hide action.
        "Update Continuously",
        "Update When Changed",
        "Hide Update Spinner",
        r#"data-spinner-mode="continuously""#,
        r#"data-spinner-mode="when-changed""#,
        r#"data-spinner-action="hide""#,
        // Both tooltips. The continuous one carries the power warning.
        "Spins when the editor window redraws.",
        "Update Continuously is enabled, which can increase power usage.",
    ] {
        assert!(
            html.contains(needle),
            "update spinner chrome is missing `{needle}`"
        );
    }

    // The radio pair must come before the hide action, with the separator in
    // between — Godot's `add_separator()` sits between them.
    let cont = html.find("Update Continuously").unwrap();
    let sep = html.find(r#"class="spinner-sep""#).unwrap();
    let hide = html.find("Hide Update Spinner").unwrap();
    assert!(
        cont < sep && sep < hide,
        "the popup order must be the two radio items, a separator, then the \
         hide action"
    );

    handle.stop();
}

/// The collapsed main menu (`main_menu_button`).
///
/// Godot shows EITHER the MenuBar or this button, never both —
/// `_update_main_menu_type()` deletes whichever is not in use. The served
/// document carries both and `setCollapseMainMenu` hides one, so the test that
/// matters is that the hiding exists and that the button starts hidden.
#[test]
fn top_bar_has_the_collapsed_main_menu_button() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    for needle in [
        r#"id="main-menu-button-wrap""#,
        r#"id="main-menu-button""#,
        r#"id="main-menu-popup""#,
        // Godot's text for it.
        "Main Menu",
        // Hidden unless the setting turns it on — the `.collapsed` class is
        // what reveals it, and the wrap defaults to `display: none`.
        "#main-menu-button-wrap { position: relative; display: none;",
        "#main-menu-button-wrap.collapsed { display: flex; }",
        "function setCollapseMainMenu(",
        "interface/editor/collapse_main_menu",
        // The collapsed entries reopen the SAME dropdowns, so no action is
        // defined twice.
        r#"document.querySelector('.menu-item[data-menu="' + name + '"]')"#,
    ] {
        assert!(
            html.contains(needle),
            "collapsed main menu chrome is missing `{needle}`"
        );
    }

    // Both menu forms must be hideable against each other: the bar's items are
    // what `setCollapseMainMenu` toggles.
    assert!(
        html.contains("document.querySelectorAll('#menu-bar > .menu-item')"),
        "setCollapseMainMenu must hide the MenuBar items when the collapsed \
         button is shown, or every action is offered twice"
    );

    handle.stop();
}

/// Godot's own renderer tooltip, which Patina had replaced with the
/// non-Godot string "Rendering method".
#[test]
fn renderer_dropdown_carries_godots_tooltip() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    for needle in [
        r#"aria-label="Renderer""#,
        "Choose a renderer.",
        "On mobile platforms, the Mobile renderer is used if Forward+ is selected here.",
        "On the web platform, the Compatibility renderer is always used.",
    ] {
        assert!(
            html.contains(needle),
            "renderer tooltip is missing `{needle}`"
        );
    }
    assert!(
        !html.contains(r#"title="Rendering method""#),
        "`Rendering method` is Patina's wording, not Godot's, and it drops both \
         caveats that make the choice meaningful"
    );

    handle.stop();
}

/// 🔑 REGRESSION GUARD FOR THIS DIFF, not for Godot parity.
///
/// `setupMenuBar` binds a click handler to EVERY `.menu-action` in the document
/// and reads `data-action` off it. The two popups added by this bead are rows in
/// a dropdown and the obvious thing to reuse was `.menu-action` — which would
/// have fired `handleMenuAction(null)` on every click, and arrived unstyled
/// besides, since the padding rule is scoped to `.menu-dropdown`. They use
/// `.spinner-mi` / `.collapsed-mi` instead.
#[test]
fn the_new_popup_rows_do_not_reuse_the_global_menu_action_hook() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    assert!(
        html.contains(r#"<div class="spinner-mi" data-spinner-mode="continuously">"#),
        "the spinner rows must not be `.menu-action`"
    );
    assert!(
        html.contains(r#"<div class="collapsed-mi" data-collapsed-menu="scene">"#),
        "the collapsed main-menu rows must not be `.menu-action`"
    );
    // The global hook is still there — that is why the classes above matter.
    assert!(
        html.contains("document.querySelectorAll('.menu-dropdown-item, .menu-action')"),
        "this guard assumes setupMenuBar still binds every .menu-action; if that \
         changed, re-derive whether the separate classes are still needed"
    );

    handle.stop();
}

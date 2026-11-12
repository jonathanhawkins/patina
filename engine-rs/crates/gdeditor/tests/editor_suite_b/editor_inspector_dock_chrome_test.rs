//! Visual-fidelity tests for the Inspector dock's permanent chrome — every
//! widget `InspectorDock::InspectorDock` builds unconditionally
//! (upstream/godot/editor/docks/inspector_dock.cpp:717), enumerated 1:1.
//!
//! Godot's constructor builds, in order: `general_options_hb`
//! (resource_new_button, resource_load_button, resource_save_button,
//! resource_extra_button, spacer, backward_button, forward_button,
//! history_menu), `subresource_hb` (object_selector, open_docs_button), and
//! `property_tools_hb` (a LineEdit placeholdered "Filter Properties",
//! object_menu). None of it depends on a selection, which is why the assertions
//! below are about the dock's *static* structure and Godot's *verbatim* strings.
//!
//! ⚠️ The previous version of this header claimed the open-docs button (pat-5b63c)
//! was "the one missing permanent widget" and the inspector was "otherwise at
//! parity". Re-enumerating against the constructor for pat-w8qn8 found five more
//! divergences: seven tooltips that paraphrased Godot instead of quoting it, a
//! `Filter properties...` placeholder where Godot says `Filter Properties`, the
//! extra-resource ellipsis carrying Expand/Collapse (Godot's `object_menu`
//! items) instead of its own clipboard/copy/show-in-filesystem/make-built-in
//! popup, no Save/Save As... popup on the save button, and — the headline one —
//! an empty inspector that wiped the dock to a single sentence, so NONE of this
//! chrome was on screen until a node was selected. Do not re-add a
//! "now at parity" claim here without re-walking the constructor.
//!
//! The object-selector row is rendered by the inspector's client script, which
//! is served inline in the `/editor` document — so the button's id and Godot
//! tooltip ship in the served page (the same way the polling-guard test asserts
//! served-JS content).

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
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
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

/// The served editor carries the Inspector's Open Documentation button matching
/// Godot's `open_docs_button` (id + tooltip), wired to request object help.
#[test]
fn inspector_dock_renders_open_docs_button() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    for needle in [
        // The button the object-selector row builds (Godot open_docs_button).
        "insp-open-docs",
        // Godot's exact tooltip (TTRC("Open documentation for this object.")).
        "Open documentation for this object.",
        // It is wired to request documentation for the object.
        "Open Documentation",
    ] {
        assert!(
            html.contains(needle),
            "served Inspector dock is missing the Open Documentation widget `{needle}` (Godot builds open_docs_button)"
        );
    }

    handle.stop();
}

/// Godot's `general_options_hb`: the four resource buttons, a spacer, then the
/// two history-navigation buttons and the history menu. The tooltips are
/// asserted verbatim because a paraphrase is exactly the kind of drift a
/// side-by-side comparison with Godot catches and a behavioural test does not.
#[test]
fn inspector_dock_renders_permanent_resource_toolbar() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    for needle in [
        // resource_new_button, resource_load_button, resource_save_button,
        // resource_extra_button — ids plus Godot's own tooltip text.
        "id=\"insp-new-res\"",
        "Create a new resource in memory and edit it.",
        "id=\"insp-load-res\"",
        "Load an existing resource from disk and edit it.",
        "id=\"insp-save-res\"",
        "Save the currently edited resource.",
        "id=\"insp-tool-menu\"",
        "Extra resource options.",
        // general_options_hb->add_spacer()
        "insp-toolbar-spacer",
        // backward_button / forward_button / history_menu
        "id=\"inspector-history-back\"",
        "Go to previous edited object in history.",
        "id=\"inspector-history-forward\"",
        "Go to next edited object in history.",
        "id=\"insp-history-btn\"",
        "History of recently edited objects.",
    ] {
        assert!(
            html.contains(needle),
            "served Inspector dock is missing permanent toolbar chrome `{needle}` \
             (Godot's InspectorDock constructor always builds it)"
        );
    }

    handle.stop();
}

/// Godot's `property_tools_hb`: a search LineEdit whose placeholder is the
/// literal TTRC("Filter Properties"), and `object_menu` ("Manage object
/// properties."). Patina shipped `Filter properties...`, which reads as a
/// different product.
#[test]
fn inspector_dock_renders_filter_properties_box() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    for needle in [
        "insp-filter-input",
        // Verbatim Godot placeholder — capital P, no ellipsis.
        "Filter Properties",
        "insp-filter-tools",
        "Manage object properties.",
    ] {
        assert!(
            html.contains(needle),
            "served Inspector dock is missing the property-tools row `{needle}`"
        );
    }
    assert!(
        !html.contains("Filter properties..."),
        "the old paraphrased placeholder `Filter properties...` is still served; \
         Godot's LineEdit says `Filter Properties`"
    );

    handle.stop();
}

/// The two MenuButtons carry Godot's popup items, not a stand-in. Asserted with
/// the leading `['` of each menu entry so a match cannot come from an unrelated
/// occurrence of the same words elsewhere in the editor (the FileSystem dock
/// also has a "Show in FileSystem" idea, for instance).
#[test]
fn inspector_dock_resource_menus_match_godot_popups() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    for needle in [
        // resource_save_button popup: Save, Save As...
        "['Save As...'",
        // resource_extra_button popup, in Godot's order.
        "['Edit Resource from Clipboard'",
        "['Copy Resource'",
        "['Show in FileSystem'",
        "['Make Resource Built-In'",
    ] {
        assert!(
            html.contains(needle),
            "served Inspector dock is missing resource-menu item `{needle}`"
        );
    }

    handle.stop();
}

/// The headline parity gap: Godot builds this chrome in the *constructor*, so an
/// inspector with nothing selected still shows the toolbar, the object-selector
/// row and the filter box — with `resource_save_button`, `backward_button`,
/// `forward_button` and `open_docs_button` disabled. Patina's empty state used
/// to replace the whole dock with one sentence, so none of it was on screen at
/// boot. Same defect class as the FileSystem dock's missing "Favorites:" row.
#[test]
fn inspector_chrome_is_permanent_with_nothing_selected() {
    let (handle, port) = make_server();
    let html = editor_html(port);

    for needle in [
        // The chrome exists as its own builder...
        "function inspectorPermanentChromeHtml",
        // ...and the empty state actually renders it, rather than wiping the dock.
        "innerHTML = inspectorPermanentChromeHtml() +",
        // The widgets Godot disables until something is edited are disabled here.
        "placeholder=\"Filter Properties\" disabled",
        "title=\"Save the currently edited resource.\" disabled",
        "title=\"Open documentation for this object.\" disabled",
        // The selection-independent handlers are shared with the populated
        // inspector instead of being duplicated or left dead in the empty state.
        "function wireInspectorResourceToolbar",
        "wireInspectorResourceToolbar();",
    ] {
        assert!(
            html.contains(needle),
            "the Inspector dock's chrome is not permanent: missing `{needle}`"
        );
    }

    handle.stop();
}

//! Frontend-wiring regression guard for `Scene > New Scene` (Godot
//! `EditorNode::new_scene` + `SceneTreeDock`'s "Create Root Node:" panel).
//!
//! Why this test exists — the bug it guards: the server already had a correct
//! `POST /api/scene/new` endpoint (creates a fresh empty untitled tab) and its
//! server-side test was green. But the served browser JS wired the `New Scene`
//! menu item to `POST /api/scene/load {path:""}` instead — an empty path that
//! the loader rejects — so clicking `Scene > New Scene` did nothing at all
//! ("when the user goes to new scene it doesn't seem to make a new scene"). A
//! pure endpoint test cannot catch that: the gap is in the client wiring.
//!
//! It also guards the Godot-parity UX: a new scene has no root node, so the
//! Scene dock must show the "Create Root Node:" panel (2D Scene / 3D Scene /
//! User Interface / Other Node / Paste From Clipboard) in place of the tree,
//! and those buttons must actually create the corresponding root node.
//!
//! Inspects the static `EDITOR_HTML` constant directly (no server boot), so it
//! is fast and deterministic and sidesteps the host's code-sign/AMFI launch path.

const HTML: &str = gdeditor::editor_ui::EDITOR_HTML;

fn count(needle: &str) -> usize {
    HTML.matches(needle).count()
}

/// The body of the `menu-new-scene` case, up to the next `break;`. Used to prove
/// the handler is wired to `newScene()` and NOT to the old broken load-empty call.
fn new_scene_case_body() -> &'static str {
    let start = HTML
        .find("case 'menu-new-scene':")
        .expect("the Scene menu has a New Scene action");
    let rest = &HTML[start..];
    let end = rest.find("break;").expect("the case ends with break;");
    &rest[..end]
}

#[test]
fn new_scene_menu_is_wired_to_the_new_scene_endpoint() {
    // The menu action must call the newScene() helper, not the old no-op.
    let body = new_scene_case_body();
    assert!(
        body.contains("newScene()"),
        "the New Scene menu item must call newScene(); got: {body:?}"
    );
    // The bug was wiring New Scene to `/api/scene/load` with an empty path. That
    // must never come back for this action.
    assert!(
        !body.contains("/api/scene/load"),
        "New Scene must NOT call /api/scene/load (empty path is rejected -> no-op); got: {body:?}"
    );

    // newScene() is defined and hits the dedicated create endpoint.
    assert!(
        HTML.contains("function newScene"),
        "a newScene() helper is defined"
    );
    assert!(
        HTML.contains("'/api/scene/new'") || HTML.contains("\"/api/scene/new\""),
        "newScene() posts to /api/scene/new (Godot EditorNode::new_scene)"
    );
    // After creating the scene it must refresh both the tree and the tab bar so
    // the new Untitled tab actually shows up.
    assert!(
        HTML.contains("function newScene")
            && HTML[HTML.find("function newScene").unwrap()..]
                .split("\n  }")
                .next()
                .unwrap_or("")
                .contains("fetchScene()"),
        "newScene() refreshes the scene tree after creating the scene"
    );
}

#[test]
fn empty_scene_shows_the_create_root_node_panel() {
    // The Scene dock swaps the tree for the Create Root Node panel when empty
    // (Godot SceneTreeDock NOTIFICATION_PROCESS: edited_scene_root == nullptr).
    assert!(
        HTML.contains("function sceneIsEmpty"),
        "an emptiness check (sceneIsEmpty) is defined"
    );
    assert!(
        count("sceneIsEmpty()") >= 2,
        "sceneIsEmpty() is actually consulted by refreshTree (defined + called)"
    );
    assert!(
        HTML.contains("function renderCreateRootPanel"),
        "the Create Root Node panel renderer is defined"
    );
    assert!(
        count("renderCreateRootPanel(") >= 2,
        "renderCreateRootPanel is actually called from the tree refresh"
    );
    // The exact Godot header + button labels, in the Scene dock.
    assert!(
        HTML.contains("Create Root Node:"),
        "the panel shows Godot's 'Create Root Node:' header"
    );
    for label in [
        "2D Scene",
        "3D Scene",
        "User Interface",
        "Other Node",
        "Paste From Clipboard",
    ] {
        assert!(
            HTML.contains(label),
            "the Create Root Node panel offers the '{label}' button (Godot parity)"
        );
    }
}

#[test]
fn create_root_buttons_create_the_matching_root_node() {
    // createRootNode is defined and adds a node via /api/node/add.
    assert!(
        HTML.contains("function createRootNode"),
        "createRootNode() is defined"
    );
    assert!(
        count("createRootNode(") >= 2,
        "createRootNode() is actually wired to the 2D/3D/UI buttons"
    );
    // The three beginner roots map to the Godot node types (SceneTreeDock
    // TOOL_CREATE_2D_SCENE -> Node2D, _3D_SCENE -> Node3D, _USER_INTERFACE ->
    // Control).
    for cls in ["Node2D", "Node3D", "Control"] {
        assert!(HTML.contains(cls), "createRootNode can create a {cls} root");
    }
    let start = HTML.find("function createRootNode").unwrap();
    let func = &HTML[start..start + 1600.min(HTML.len() - start)];
    assert!(
        func.contains("/api/node/add"),
        "createRootNode() adds the root through /api/node/add"
    );
    // Godot switches the main screen to fit the new root (3D Scene -> 3D editor);
    // createRootNode re-arms the one-shot auto-view before refreshing.
    assert!(
        func.contains("autoViewPending = true"),
        "createRootNode() re-arms auto-view so a 3D root opens the 3D editor"
    );
    // "Other Node" opens the full create-node dialog (Godot TOOL_NEW).
    assert!(
        HTML.contains("openAddNodeDialog"),
        "the 'Other Node' button opens the create-node dialog"
    );
}

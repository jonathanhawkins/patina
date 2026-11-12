//! pat-s3rrv.23 (slice) — the 3D viewport's Mesh menu reaches the collision and
//! navmesh routes the server already serves.
//!
//! Godot exposes Create Trimesh/Convex Collision Sibling and Create Navigation
//! Mesh from the 3D viewport's Mesh menu
//! (upstream/godot/editor/scene/3d/mesh_instance_3d_editor_plugin.cpp).
//!
//! Patina served both endpoints — `POST /api/mesh/create_collision` and
//! `POST /api/navigation/bake` are in `editor_server.rs`'s dispatch table — and
//! the served client referenced **neither**. The 3D toolbar had only Transform
//! and View menus, so a user had no way to invoke either one: the feature was
//! reachable by `curl` and by nothing else.
//!
//! ⚠️ This asserts on `EDITOR_HTML`, the string actually served to the browser.
//! A test that only checked the routes exist server-side would have passed
//! throughout the entire period the feature was unreachable — that is precisely
//! the gap this bead documents.

use gdeditor::editor_ui::EDITOR_HTML;

/// The menu exists in the served markup, with the three Godot entries.
#[test]
fn mesh_menu_is_present_in_the_served_3d_toolbar() {
    assert!(
        EDITOR_HTML.contains("vp3d-meshmenu-btn"),
        "the 3D toolbar must carry a Mesh menu button"
    );
    assert!(
        EDITOR_HTML.contains("vp3d-meshmenu-dropdown"),
        "the Mesh menu must have a dropdown"
    );
    for action in [
        "mesh-trimesh-collision",
        "mesh-convex-collision",
        "mesh-navmesh",
    ] {
        assert!(
            EDITOR_HTML.contains(action),
            "the Mesh menu must offer `{action}`"
        );
    }
}

/// **The slice's acceptance.** The client actually posts to the served routes.
///
/// This is the assertion that would have failed for the whole life of the bug:
/// both routes existed and had zero references in the client.
#[test]
fn mesh_menu_posts_to_the_collision_and_navmesh_routes() {
    assert!(
        EDITOR_HTML.contains("/api/mesh/create_collision"),
        "the client must POST to /api/mesh/create_collision — the route was \
         served with 0 client references, so the feature was unreachable"
    );
    assert!(
        EDITOR_HTML.contains("/api/navigation/bake"),
        "the client must POST to /api/navigation/bake"
    );

    // The server rejects any `kind` outside this pair with 400
    // (editor_server.rs: `kind must be 'trimesh' or 'convex'`), so the client
    // must send one of exactly these.
    assert!(
        EDITOR_HTML.contains("'trimesh'") || EDITOR_HTML.contains("\"trimesh\""),
        "the client must send kind=trimesh for the trimesh entry"
    );
    assert!(
        EDITOR_HTML.contains("'convex'") || EDITOR_HTML.contains("\"convex\""),
        "the client must send kind=convex for the convex entry"
    );
}

/// The menu is registered with the same open/dispatch plumbing as the existing
/// menus — a dropdown that no one binds is markup that does nothing.
#[test]
fn mesh_menu_is_registered_with_the_menu_plumbing() {
    assert!(
        EDITOR_HTML.contains("setupVp3dMenu('vp3d-meshmenu-btn', 'vp3d-meshmenu-dropdown')"),
        "the Mesh menu must be registered with setupVp3dMenu so it opens"
    );
    assert!(
        EDITOR_HTML.contains("bindVp3dMenuActions('vp3d-meshmenu-dropdown')"),
        "its items must be bound with bindVp3dMenuActions so clicks dispatch"
    );
}

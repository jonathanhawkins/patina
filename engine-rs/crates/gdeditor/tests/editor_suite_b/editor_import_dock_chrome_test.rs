//! Visual-fidelity (DOM-structure) test for the rest of the Import dock's
//! permanent chrome (pat-93pfv).
//!
//! `editor_import_dock_test` already pins the three controls the bead names —
//! the "Import As:" field, the Preset menu and the Reimport button. Enumerating
//! `ImportDock::ImportDock()` (upstream/godot/editor/docks/import_dock.cpp:745-836)
//! end to end turns up three more widgets its constructor *always* builds, none
//! of which Patina rendered:
//!
//! * `imported` (:760-764) — a LineEdit-styled Label heading the content with
//!   the file being imported (`imported->set_text(p_path.get_file())`, :157).
//! * `advanced` (:803-805) — the "Advanced..." button sharing an HBox with
//!   Reimport. The ctor hides it (:810-811); it is shown only for importers that
//!   override `has_advanced_options()` — the scene importer
//!   (upstream/godot/editor/import/3d/resource_importer_scene.cpp:3373) and the
//!   dynamic-font importer (resource_importer_dynamic_font.cpp:145); everything
//!   else inherits `false` (upstream/godot/core/io/resource_importer.h:145).
//! * `reimport_confirm` + `cleanup_warning` + `label_warning` (:814-823) — the
//!   confirmation popped when the chosen importer differs from the one the asset
//!   was imported with (:558-576).
//!
//! Plus `select_a_resource` (:827-835) carries a specific prompt, which Patina
//! had paraphrased.
//!
//! The dock's `Advanced...` visibility is data-driven, so this also drives
//! `/api/import_settings` and asserts the `advanced` flag follows Godot's
//! `has_advanced_options()` split rather than being hard-coded either way.

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

fn http_get(port: u16, path: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}

fn json_ok(resp: &str) -> serde_json::Value {
    assert!(resp.contains("200 OK"), "expected 200, got:\n{resp}");
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or("")).expect("valid JSON")
}

/// Every widget `ImportDock`'s constructor builds unconditionally has a
/// counterpart in the served editor document — including the three the dock was
/// missing entirely.
#[test]
fn import_dock_renders_remaining_constructor_chrome() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");

    for needle in [
        // `imported` header label (import_dock.cpp:760-764).
        "id=\"import-imported\"",
        // `advanced` button (:803-805), hidden as the ctor leaves it (:810-811).
        "id=\"import-advanced\"",
        ">Advanced...<",
        // Advanced and Reimport share one HBox (:796-807).
        "class=\"import-btn-row\"",
        // `reimport_confirm` (:814-816) and its two warning labels (:819-822),
        // verbatim from the Godot strings.
        "id=\"import-reimport-confirm\"",
        "id=\"import-cleanup-warning\"",
        "The imported resource is currently loaded. All instances will be replaced and undo history will be cleared.",
        "id=\"import-type-warning\"",
        "WARNING: Assets exist that use this resource. They may stop loading properly after changing type.",
        // `select_a_resource`'s prompt (:829), verbatim.
        "Select a resource file in the filesystem or in the inspector to adjust import settings.",
        // The chrome is driven, not decorative: the header label is filled from
        // the selected path and cleared with the empty state, Advanced follows
        // the importer, and Reimport routes through the confirmation.
        "function resetImportChrome",
        "resetImportChrome()",
        "info.advanced",
    ] {
        assert!(
            html.contains(needle),
            "served Import dock is missing constructor chrome `{needle}` (Godot builds it unconditionally)"
        );
    }

    // The ctor hides `advanced`; the dock must ship it hidden too, or every
    // importer without advanced options shows a button Godot does not.
    let adv = html
        .find("id=\"import-advanced\"")
        .expect("advanced button present");
    let tag_end = html[adv..].find('>').expect("button tag closes") + adv;
    assert!(
        html[adv..tag_end].contains("display:none"),
        "the Advanced... button must start hidden (import_dock.cpp:810-811 hides it \
         in the constructor); got `{}`",
        &html[adv..tag_end]
    );

    handle.stop();
}

/// `Advanced...` is only visible for importers that override
/// `has_advanced_options()`, so the dock's data source has to distinguish them.
#[test]
fn import_settings_reports_advanced_options_per_importer() {
    let (handle, port) = make_server();

    // ResourceImporterScene::has_advanced_options() -> true
    // (upstream/godot/editor/import/3d/resource_importer_scene.cpp:3373).
    let scene = json_ok(&http_get(
        port,
        "/api/import_settings?path=res://model.gltf",
    ));
    assert_eq!(scene["importer"], serde_json::json!("scene"));
    assert_eq!(scene["importer_visible_name"], serde_json::json!("Scene"));
    assert_eq!(
        scene["advanced"],
        serde_json::json!(true),
        "the scene importer has advanced options"
    );

    // ResourceImporterDynamicFont::has_advanced_options() -> true
    // (upstream/godot/editor/import/resource_importer_dynamic_font.cpp:145).
    let font = json_ok(&http_get(port, "/api/import_settings?path=res://ui.ttf"));
    // ⚠️ "Font Data (Dynamic Font)", not "Font" — another label upstream never
    // had (editor/import/resource_importer_dynamic_font.cpp).
    assert_eq!(font["importer"], serde_json::json!("font_data_dynamic"));
    assert_eq!(
        font["importer_visible_name"],
        serde_json::json!("Font Data (Dynamic Font)")
    );
    assert_eq!(
        font["advanced"],
        serde_json::json!(true),
        "the dynamic-font importer has advanced options"
    );

    // Everything else inherits ResourceImporter::has_advanced_options() -> false
    // (upstream/godot/core/io/resource_importer.h:145), so the button stays
    // hidden — a flag that were always true would show it on a PNG.
    for (path, importer, visible) in [
        ("res://icon.png", "texture", "Texture2D"),
        ("res://music.ogg", "oggvorbisstr", "Ogg Vorbis"),
    ] {
        let info = json_ok(&http_get(
            port,
            &format!("/api/import_settings?path={path}"),
        ));
        assert_eq!(info["importer"], serde_json::json!(importer));
        assert_eq!(info["importer_visible_name"], serde_json::json!(visible));
        assert_eq!(
            info["advanced"],
            serde_json::json!(false),
            "{importer} inherits has_advanced_options() == false"
        );
    }

    handle.stop();
}

/// **The acceptance test** for pat-mgixk.407: the dock SHOWS Godot's visible
/// importer name while the API CARRIES the importer id.
///
/// Those are two different strings in upstream and they are used for two
/// different things. `ImportDock::_update_options` builds each entry as
/// `Pair(get_visible_name(), get_importer_name())`, then
/// `add_item(E.first)` for the label and `set_item_metadata(-1, E.second)` for
/// the value it acts on (`editor/docks/import_dock.cpp:133,141-142`). A
/// `.import` file stores the id; the dock displays the name.
///
/// Reporting only one of the two is what left `editor_suite_b` red: the field
/// was switched from the label to the id and the tests still wanted the label —
/// so every bead verify that included this suite failed on it.
///
/// ⚠️ **Scope of the DOM half.** The `Import As:` options are built in the
/// browser from the API payload, so a server-side test cannot observe the
/// rendered `<option>` text. The second assertion therefore checks that the
/// served page *wires* the label — it proves the generator, not the pixel. The
/// `textContent`/`value` split it checks for is the one upstream makes, and a
/// real rendered check needs the browser verifier.
#[test]
fn import_dock_shows_name_and_reports_importer_id() {
    let (handle, port) = make_server();

    let info = json_ok(&http_get(port, "/api/import_settings?path=res://icon.png"));
    assert_eq!(
        info["importer"],
        serde_json::json!("texture"),
        "the API must carry the importer ID — `ResourceImporterTexture::get_importer_name()`, \
         the string a .import file's `importer=` line holds"
    );
    assert_eq!(
        info["importer_visible_name"],
        serde_json::json!("Texture2D"),
        "...and the visible name — `get_visible_name()` \
         (editor/import/resource_importer_texture.cpp:168), which is what the dock shows"
    );

    // The served page must feed the label to the option text rather than the
    // id, or the dock would display `texture` where Godot shows `Texture2D`.
    let html = http_get(port, "/editor");
    assert!(
        html.contains("info.importer_visible_name"),
        "the served editor must render the visible name in the Import As list; \
         without it the dock shows the raw importer id"
    );

    handle.stop();
}

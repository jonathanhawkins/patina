//! pat-s3rrv.63 — script templates are served, and the body is generated
//! SERVER-side from the node's class.
//!
//! Godot's Create Script dialog populates its Template dropdown from the
//! project's `res://script_templates/`, the editor's templates and the
//! language's built-ins, searching the base class's whole ancestor chain
//! (`editor/script/script_create_dialog.cpp`, `_update_template_menu` →
//! `_get_hierarchy`), then renders the chosen template through
//! `GDScriptLanguage::make_template`.
//!
//! Patina built the script body as a **string literal in the browser**, so:
//! there was nothing to populate a dropdown from, and every new script carried
//! the same hardcoded base class whatever the node actually was. A script
//! attached to a `Control` that says `extends Node2D` is not a cosmetic
//! default — it is a runtime error the moment the script touches its real base.
//!
//! These drive the real HTTP endpoints, so a pass means the flow is wired end
//! to end rather than that a helper exists.
//!
//! ⚠️ Scope: this covers the SERVER half of the bead (`/api/script/templates`,
//! `/api/script/create`) and the removal of the client-side literals. The modal
//! Create-Script dialog itself — Inherits picker, Class Name field, Template
//! dropdown, Built-in Script toggle — is NOT implemented, so this test is
//! deliberately not named for the bead's full acceptance.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn connect_with_retry(port: u16) -> TcpStream {
    for attempt in 0..20 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 19 => thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("failed to connect: {e}"),
        }
    }
    unreachable!()
}

fn http_request(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http_request(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_request(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn json_of(resp: &str) -> serde_json::Value {
    serde_json::from_str(body_of(resp))
        .unwrap_or_else(|e| panic!("response body is not JSON ({e}): {resp}"))
}

/// Process-unique fixture dir — a fixed `/tmp` path collides across the
/// concurrent test processes in the verifier lane.
fn unique_dir(tag: &str) -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_script_tpl_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A server whose project root — and therefore `res://` — is `dir`.
fn serve(dir: &std::path::Path) -> u16 {
    gdobject::class_db::register_all_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Root", "Node2D")).unwrap();

    let mut state = EditorState::new(tree);
    state.project_root = Some(dir.to_string_lossy().to_string());

    let port = free_port();
    let _handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    port
}

/// **The acceptance.** A script created for a `Control` extends `Control`, and
/// one for a `RigidBody3D` extends `RigidBody3D` — from the same endpoint.
#[test]
fn created_script_extends_the_requested_class() {
    let dir = unique_dir("extends");
    let port = serve(&dir);

    for (class, file) in [
        ("Control", "res://scripts/ui.gd"),
        ("RigidBody3D", "res://scripts/crate.gd"),
    ] {
        let resp = json_of(&http_post(
            port,
            "/api/script/create",
            &format!(r#"{{"path":"{file}","base_class":"{class}"}}"#),
        ));
        assert_eq!(resp["ok"], true, "create failed for {class}: {resp}");
        assert_eq!(
            resp["created"], true,
            "{class} should be a new file: {resp}"
        );

        // The file must actually be on disk — a response saying `ok` while
        // nothing was written is exactly the silent failure this replaces.
        let on_disk = std::fs::read_to_string(dir.join(file.trim_start_matches("res://")))
            .unwrap_or_else(|e| panic!("{file} was not written: {e}"));
        let first = on_disk.lines().next().unwrap_or("");
        assert_eq!(
            first,
            format!("extends {class}"),
            "a {class} script must extend {class}, not something else — that is \
             the bug this bead exists for. Full body:\n{on_disk}"
        );
        // Placeholders are the template's internal language and must never
        // reach disk.
        assert!(
            !on_disk.contains("_BASE_") && !on_disk.contains("_TS_"),
            "template placeholders leaked into the generated script:\n{on_disk}"
        );
        // The Node template's body comes along, re-indented.
        assert!(
            on_disk.contains("func _ready()"),
            "the Node default template body should be present:\n{on_disk}"
        );
    }
}

/// The dropdown's data source: templates for a class include the ones its
/// ANCESTORS declare, which is why a `Sprite2D` still gets `Node`'s default.
#[test]
fn templates_are_served_across_the_ancestor_chain() {
    let dir = unique_dir("list");
    let port = serve(&dir);

    let resp = json_of(&http_get(port, "/api/script/templates?base_class=Sprite2D"));
    let templates = resp["templates"].as_array().cloned().unwrap_or_default();
    assert!(
        !templates.is_empty(),
        "Sprite2D must be offered templates via its ancestors: {resp}"
    );
    let ids: Vec<&str> = templates.iter().filter_map(|t| t["id"].as_str()).collect();
    assert!(
        ids.contains(&"Node/default"),
        "Node's default template must reach a Sprite2D through the inheritance \
         chain — an exact-class-only match would offer nothing. Got {ids:?}"
    );
    // Metadata the dropdown renders.
    let node_default = templates
        .iter()
        .find(|t| t["id"] == "Node/default")
        .unwrap();
    assert_eq!(node_default["origin"], "built_in", "{node_default}");
    assert!(
        node_default["description"]
            .as_str()
            .unwrap_or("")
            .contains("Base template for Node"),
        "the `# meta-description:` line must be parsed out as metadata: {node_default}"
    );
    assert_eq!(
        node_default["name"], "Default",
        "name falls back to the capitalized file stem: {node_default}"
    );
}

/// A project template under `res://script_templates/<Class>/` is picked up, is
/// labelled as a project template, and is offered BEFORE the built-ins.
#[test]
fn project_templates_are_discovered_and_take_precedence() {
    let dir = unique_dir("project");
    let tpl_dir = dir.join("script_templates/Node");
    std::fs::create_dir_all(&tpl_dir).unwrap();
    std::fs::write(
        tpl_dir.join("house_style.gd"),
        "# meta-name: House Style\n# meta-description: Our conventions\n\nextends _BASE_\n\n\nfunc _ready() -> void:\n\tprint(\"_CLASS_\")\n",
    )
    .unwrap();

    let port = serve(&dir);
    let resp = json_of(&http_get(port, "/api/script/templates?base_class=Node2D"));
    let templates = resp["templates"].as_array().cloned().unwrap_or_default();
    let ids: Vec<&str> = templates.iter().filter_map(|t| t["id"].as_str()).collect();
    assert!(
        ids.contains(&"Node/house_style"),
        "a template under res://script_templates/Node/ must be discovered: {ids:?}"
    );
    assert_eq!(
        ids.first(),
        Some(&"Node/house_style"),
        "project templates are searched before built-ins, so a project can \
         shadow one: {ids:?}"
    );
    let house = templates
        .iter()
        .find(|t| t["id"] == "Node/house_style")
        .unwrap();
    assert_eq!(house["origin"], "project", "{house}");
    assert_eq!(
        house["name"], "House Style",
        "`# meta-name:` overrides the file-stem default: {house}"
    );

    // And it renders: `_CLASS_` becomes the script's class name, `_BASE_` the
    // node's class.
    let created = json_of(&http_post(
        port,
        "/api/script/create",
        r#"{"path":"res://scripts/thing.gd","base_class":"Node2D","template":"Node/house_style","class_name":"MyThing"}"#,
    ));
    assert_eq!(created["ok"], true, "{created}");
    let body = created["content"].as_str().unwrap_or("");
    assert!(
        body.starts_with("extends Node2D"),
        "chosen template must still extend the node's class:\n{body}"
    );
    assert!(
        body.contains("print(\"MyThing\")"),
        "`_CLASS_` must be substituted with the class name:\n{body}"
    );
}

/// An invalid path is refused with the REASON, not accepted and silently
/// dropped. Godot validates live and shows why (`_path_changed`).
#[test]
fn an_invalid_path_reports_why() {
    let dir = unique_dir("invalid");
    let port = serve(&dir);

    for (path, expect) in [
        ("scripts/nores.gd", "res://"),
        ("res://notascript.txt", ".gd"),
        ("res://../escape.gd", "inside res://"),
    ] {
        let raw = http_post(
            port,
            "/api/script/create",
            &format!(r#"{{"path":"{path}","base_class":"Node"}}"#),
        );
        assert!(
            raw.starts_with("HTTP/1.1 400"),
            "`{path}` must be rejected, got: {raw}"
        );
        let resp = json_of(&raw);
        // ⚠️ `error` is an ENVELOPE object (`{"code":..,"message":..}`), not a
        // string — reading it as a string yields None and silently asserts
        // against "".
        let msg = resp["error"]["message"].as_str().unwrap_or("");
        assert!(
            msg.contains(expect),
            "the error must name the broken rule ({expect}) rather than failing \
             silently — got {msg:?} for {path}"
        );
    }

    // Nothing was written for any of them.
    assert!(
        !dir.join("scripts").exists(),
        "a rejected path must not create anything on disk"
    );
}

/// An existing file is reused rather than clobbered, which is what Godot does
/// when the dialog points at a script that already exists.
#[test]
fn an_existing_script_is_reused_not_overwritten() {
    let dir = unique_dir("reuse");
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(dir.join("scripts/keep.gd"), "extends Node\n# precious\n").unwrap();

    let port = serve(&dir);
    let resp = json_of(&http_post(
        port,
        "/api/script/create",
        r#"{"path":"res://scripts/keep.gd","base_class":"Control"}"#,
    ));
    assert_eq!(resp["ok"], true, "{resp}");
    assert_eq!(
        resp["created"], false,
        "an existing file is reused, not recreated: {resp}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("scripts/keep.gd")).unwrap(),
        "extends Node\n# precious\n",
        "the existing script must be left byte-for-byte alone"
    );
}

/// The browser no longer builds script bodies itself, and the Create Script
/// dialog is what asks for one.
///
/// ⚠️ Slices SPECIFIC functions out of `EDITOR_HTML`; a page-wide search would
/// match the other places `extends` and `prompt` appear.
#[test]
fn the_client_no_longer_builds_the_script_body() {
    let html = gdeditor::editor_ui::EDITOR_HTML;

    // Both entry points are now thin openers over the dialog.
    for name in ["doAttachScript", "createNewScript"] {
        let src = js_function(html, name);
        assert!(
            !src.contains("'extends "),
            "{name} must not build the script body as a string literal:\n{src}"
        );
        assert!(
            !src.contains("prompt("),
            "{name} must open the Create Script dialog rather than a prompt() — \
             a prompt cannot show the inherits class, offer a template, or \
             refuse a bad path before the file is written:\n{src}"
        );
        assert!(
            src.contains("openCreateScriptDialog"),
            "{name} must open the Create Script dialog:\n{src}"
        );
    }

    // The dialog is what talks to the server, and it sends the class.
    let confirm = js_function(html, "confirmCreateScript");
    assert!(
        confirm.contains("/api/script/create"),
        "the dialog must create scripts through the server endpoint:\n{confirm}"
    );
    assert!(
        confirm.contains("base_class") && confirm.contains("template"),
        "…passing both the class to extend and the chosen template:\n{confirm}"
    );
    assert!(
        !confirm.contains("'extends "),
        "…and still must not assemble the body itself:\n{confirm}"
    );
}

/// The dialog has the fields Godot's ScriptCreateDialog has, and populates the
/// Template dropdown from the server rather than a hardcoded list.
#[test]
fn the_create_script_dialog_is_present_and_populated() {
    let html = gdeditor::editor_ui::EDITOR_HTML;

    for id in [
        "create-script-overlay",
        "create-script-inherits",
        "create-script-class",
        "create-script-template",
        "create-script-path",
        "create-script-error",
        "create-script-confirm",
    ] {
        assert!(
            html.contains(&format!("id=\"{id}\"")),
            "the Create Script dialog must have a `{id}` element"
        );
    }

    // The dropdown's options come from the endpoint, across the ancestor chain.
    let load = js_function(html, "loadScriptTemplates");
    assert!(
        load.contains("/api/script/templates"),
        "the Template dropdown must be populated from the server:\n{load}"
    );
    assert!(
        load.contains("base_class="),
        "…for the class being inherited, so ancestors' templates are offered \
         too:\n{load}"
    );

    // Changing Inherits must re-fetch — templates are per-class.
    let setup = js_function(html, "setupCreateScriptDialog");
    assert!(
        setup.contains("loadScriptTemplates"),
        "changing the Inherits field must refresh the template list:\n{setup}"
    );
    // And the dialog must actually be wired up at startup, or it never opens.
    assert!(
        html.contains("setupCreateScriptDialog();"),
        "setupCreateScriptDialog must be called during editor setup — a dialog \
         whose listeners are never attached is inert"
    );
}

/// The source of the JS function named `name`, from its declaration to the
/// closing brace at the same indentation.
fn js_function<'a>(src: &'a str, name: &str) -> &'a str {
    let decl = format!("function {name}(");
    let start = src
        .find(&decl)
        .unwrap_or_else(|| panic!("no function {name} in EDITOR_HTML"));
    let rest = &src[start..];
    // Functions in this file are indented two spaces, so their closer is a
    // newline + exactly two spaces + `}`.
    let end = rest
        .find("\n  }")
        .unwrap_or_else(|| panic!("function {name} never closes"));
    &rest[..end]
}

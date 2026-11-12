//! pat-s3rrv.2 acceptance: the Import dock's options reach a real `.import`
//! sidecar, and Reimport writes a real artifact.
//!
//! Before this bead all three endpoints were theatre. `api_set_import_settings`
//! was literally `let _ = parse_json_body(body); send_json(stream, "{\"ok\":true}")`
//! with the comment "persisting to a `.import` sidecar is future work";
//! `api_get_import_settings` synthesised a hardcoded option list from the file
//! extension and never opened the asset's `.import`, so the dock could not show
//! what an option was actually set to; and `api_reimport` validated the
//! extension and echoed the path back. Nothing in the whole workspace could
//! *write* a `.import` sidecar or a `.godot/imported/` artifact — every artifact
//! Patina resolved had to have been produced by a real Godot editor.
//!
//! These tests drive the **real HTTP server** and assert against the **files on
//! disk**, so a regression to "route exists but persists nothing" fails here
//! rather than looking green.

use gdcore::math::Color;
use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdrender2d::renderer::FrameBuffer;
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// HTTP harness (mirrors editor_export_presets_e2e_test.rs)
// ---------------------------------------------------------------------------

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

fn http(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn json_of(resp: &str) -> serde_json::Value {
    assert!(resp.contains("200 OK"), "expected 200 OK, got:\n{resp}");
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or(""))
        .unwrap_or_else(|e| panic!("response body is not JSON ({e}):\n{resp}"))
}

/// Pulls one option out of a `/api/import_settings` response.
fn option_value(info: &serde_json::Value, name: &str) -> serde_json::Value {
    info["options"]
        .as_array()
        .unwrap_or_else(|| panic!("no options array in {info}"))
        .iter()
        .find(|o| o["name"] == serde_json::json!(name))
        .unwrap_or_else(|| panic!("no `{name}` option in {info}"))["value"]
        .clone()
}

// ---------------------------------------------------------------------------
// Project fixture
// ---------------------------------------------------------------------------

/// A real PNG of `w`x`h` filled with `color`, produced by the engine's own
/// encoder so the importer decodes a genuine file rather than a stub.
fn png_bytes(w: u32, h: u32, color: Color) -> Vec<u8> {
    gdrender2d::export::encode_png(&FrameBuffer::new(w, h, color))
}

fn temp_project(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_s3rrv2_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"ImportDockE2E\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("hero.png"),
        png_bytes(4, 2, Color::rgb(0.0, 0.0, 1.0)),
    )
    .unwrap();
    dir
}

/// Starts a server re-rooted at a fresh temp project holding a real `hero.png`.
fn server_on_temp_project(tag: &str) -> (EditorServerHandle, u16, PathBuf) {
    let dir = temp_project(tag);
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));

    let open = http_post(
        port,
        "/api/project/open",
        &serde_json::json!({ "path": dir.display().to_string() }).to_string(),
    );
    assert!(
        open.contains("200 OK"),
        "/api/project/open must succeed for the temp project, got:\n{open}"
    );
    (handle, port, dir)
}

fn sidecar_text(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("hero.png.import"))
        .unwrap_or_else(|e| panic!("hero.png.import must exist on disk: {e}"))
}

/// The `[remap] path` the sidecar currently names, as a `res://` path.
fn remap_path(dir: &Path) -> String {
    let text = sidecar_text(dir);
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with("path="))
        .unwrap_or_else(|| panic!("sidecar has no [remap] path:\n{text}"));
    line.split_once('=')
        .unwrap()
        .1
        .trim()
        .trim_matches('"')
        .to_string()
}

// ---------------------------------------------------------------------------
// The acceptance test
// ---------------------------------------------------------------------------

/// **The bead's acceptance.** Options round-trip through a real `.import` on
/// disk, and Reimport produces (and regenerates) a real artifact.
#[test]
fn import_dock_persists_options_and_reimports() {
    let (_handle, port, dir) = server_on_temp_project("acceptance");

    // -- (1) GET reports the importer default before anything is stored -------
    let info = json_of(&http_get(
        port,
        "/api/import_settings?path=res%3A//hero.png",
    ));
    assert_eq!(info["importable"], serde_json::json!(true));
    // Godot 4.6's key and default (resource_importer_texture.cpp: the
    // auto-detect preset leaves `mipmaps/generate` off). The dock used to
    // report an invented `mipmaps` key the importer never read.
    assert_eq!(
        option_value(&info, "mipmaps/generate"),
        serde_json::json!(false),
        "the importer default is mipmaps/generate=false"
    );

    // -- (2) POST persists the edit into [params] on disk ---------------------
    let set = json_of(&http_post(
        port,
        "/api/import_settings",
        r#"{"path":"res://hero.png","option":"mipmaps/generate","value":true}"#,
    ));
    assert_eq!(set["ok"], serde_json::json!(true));

    let text = sidecar_text(&dir);
    assert!(
        text.contains("[params]"),
        "the sidecar must carry a [params] section:\n{text}"
    );
    assert!(
        text.contains("mipmaps/generate=true"),
        "the edited option must be written as a bare INI literal:\n{text}"
    );
    // A bool written as the STRING "false" is how a sidecar silently stops
    // matching its own importer, so pin the unquoted form specifically.
    assert!(
        !text.contains("mipmaps/generate=\"true\""),
        "a bool must not be quoted:\n{text}"
    );

    // -- (3) GET now reports the stored value, not the default ----------------
    let after = json_of(&http_get(
        port,
        "/api/import_settings?path=res%3A//hero.png",
    ));
    assert_eq!(
        option_value(&after, "mipmaps/generate"),
        serde_json::json!(true),
        "the dock must show what the asset is actually set to"
    );

    // -- (4) Reimport writes the artifact [remap] path names ------------------
    let re = json_of(&http_post(
        port,
        "/api/reimport",
        r#"{"path":"res://hero.png"}"#,
    ));
    assert_eq!(
        re["reimported"],
        serde_json::json!("res://hero.png"),
        "reimport still reports the source path"
    );

    let artifact_res = remap_path(&dir);
    assert!(
        artifact_res.starts_with("res://.godot/imported/"),
        "the artifact must live under .godot/imported/, got {artifact_res}"
    );
    let artifact_abs = dir.join(artifact_res.trim_start_matches("res://"));
    let first = std::fs::read(&artifact_abs).unwrap_or_else(|e| {
        panic!("the artifact named by [remap] path must exist ({artifact_res}): {e}")
    });
    assert_eq!(
        &first[..4],
        b"GST2",
        "the artifact must be a real .ctex, not a placeholder"
    );

    // It decodes back to the source's extent through the engine's own loader —
    // the same path `load("res://hero.png")` takes once the remap is in place.
    let decoded = gdresource::ctex::load_ctex_bytes(&first, &artifact_abs)
        .expect("the written artifact must load through the ctex loader");
    assert_eq!(
        decoded.get_property("width"),
        Some(&gdvariant::Variant::Int(4)),
    );
    assert_eq!(
        decoded.get_property("height"),
        Some(&gdvariant::Variant::Int(2)),
    );

    // -- (5) A changed source regenerates the artifact ------------------------
    // Re-running the importer on identical bytes would produce identical bytes,
    // so change the source: this is what distinguishes a real reimport from a
    // write-once stub.
    std::fs::write(
        dir.join("hero.png"),
        png_bytes(4, 2, Color::rgb(1.0, 0.0, 0.0)),
    )
    .unwrap();
    let re2 = json_of(&http_post(
        port,
        "/api/reimport",
        r#"{"path":"res://hero.png"}"#,
    ));
    assert_eq!(re2["ok"], serde_json::json!(true));
    let second = std::fs::read(&artifact_abs).expect("artifact still present after reimport");
    assert_ne!(
        first, second,
        "reimporting a changed source must rewrite the artifact"
    );

    // -- (6) The persisted option survives the reimport -----------------------
    let final_info = json_of(&http_get(
        port,
        "/api/import_settings?path=res%3A//hero.png",
    ));
    assert_eq!(
        option_value(&final_info, "mipmaps/generate"),
        serde_json::json!(true),
        "reimport must not reset the stored options"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The sidecar a dock-only edit creates must be a *well-formed* one: Godot reads
/// `[remap] importer` to decide which importer owns the asset, so a params-only
/// file would read as never-imported.
#[test]
fn a_dock_created_sidecar_names_its_importer_and_source() {
    let (_handle, port, dir) = server_on_temp_project("sidecar_shape");

    assert!(
        !dir.join("hero.png.import").exists(),
        "no sidecar before the first edit"
    );
    json_of(&http_post(
        port,
        "/api/import_settings",
        r#"{"path":"res://hero.png","option":"compress/mode","value":"Lossy"}"#,
    ));

    let text = sidecar_text(&dir);
    assert!(
        text.contains("importer=\"texture\""),
        "a texture sidecar must name Godot's texture importer:\n{text}"
    );
    assert!(
        text.contains("source_file=\"res://hero.png\""),
        "[deps] must record the source:\n{text}"
    );
    // 🔑 An enum is stored as its INDEX, exactly as Godot writes it
    // (`compress/mode=1` for Lossy) — the importer reads
    // `params.int("compress/mode")`, so a quoted label would be ignored.
    assert!(
        text.contains("compress/mode=1"),
        "an enum label must be stored as Godot's index:\n{text}"
    );
    assert!(
        !text.contains("compress/mode=\"Lossy\""),
        "never the label:\n{text}"
    );
    // …and the dock reads that index back as its label.
    let info = json_of(&http_get(
        port,
        "/api/import_settings?path=res%3A//hero.png",
    ));
    assert_eq!(
        option_value(&info, "compress/mode"),
        serde_json::json!("Lossy"),
        "a stored enum index must display as its choice label"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// ⚠️ The sidecar's importer id must come from the **extension**, not from a
/// coarse resource type.
///
/// `.ogg` and `.wav` are both `ImportResourceType::Audio`, but Godot gives them
/// DIFFERENT importers: `oggvorbisstr`
/// (`modules/vorbis/resource_importer_ogg_vorbis.cpp`) and `wav`
/// (`editor/import/resource_importer_wav.cpp`). The dock-created sidecar used to
/// be written from the type, whose `Audio` arm answered `"wav"`, so **every**
/// `.ogg` got `importer="wav"` on disk while the dock's own GET reported
/// `oggvorbisstr` — the API and the file disagreeing about the same asset, and
/// a real Godot editor opening the project would hand the file to the WAV
/// importer (pat-mgixk.407).
#[test]
fn a_dock_created_sidecar_derives_the_importer_from_the_extension() {
    let (_handle, port, dir) = server_on_temp_project("ogg_importer");
    // A stub is enough: this endpoint resolves the path and writes a sidecar, it
    // never decodes the asset.
    std::fs::write(dir.join("music.ogg"), b"OggS\x00stub").unwrap();

    assert!(
        !dir.join("music.ogg.import").exists(),
        "no sidecar before the first edit"
    );
    json_of(&http_post(
        port,
        "/api/import_settings",
        r#"{"path":"res://music.ogg","option":"loop","value":false}"#,
    ));

    let text = std::fs::read_to_string(dir.join("music.ogg.import"))
        .expect("the dock must create a sidecar for the asset it just edited");
    assert!(
        text.contains("importer=\"oggvorbisstr\""),
        "an .ogg sidecar must name Godot's Ogg Vorbis importer:\n{text}"
    );
    // The negative matters on its own: `wav` is a real importer id, so a reader
    // seeing it would not suspect a bug — it is simply the wrong one here.
    assert!(
        !text.contains("importer=\"wav\""),
        "`wav` is the importer for .wav only; writing it for an .ogg is the defect:\n{text}"
    );

    // And the two halves must agree: the dock REPORTS the id it WRITES.
    let info = json_of(&http_get(
        port,
        "/api/import_settings?path=res%3A//music.ogg",
    ));
    assert_eq!(
        info["importer"],
        serde_json::json!("oggvorbisstr"),
        "the API and the sidecar must not disagree about the same asset"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Non-importable and traversal paths are refused rather than writing a sidecar
/// somewhere outside the project.
#[test]
fn import_settings_refuses_bad_paths() {
    let (_handle, port, dir) = server_on_temp_project("bad_paths");

    let not_importable = http_post(
        port,
        "/api/import_settings",
        r#"{"path":"res://notes.xyz","option":"mipmaps/generate","value":false}"#,
    );
    assert!(
        not_importable.contains("400"),
        "an unimportable extension must be refused, got:\n{not_importable}"
    );

    let traversal = http_post(
        port,
        "/api/import_settings",
        r#"{"path":"res://../escape.png","option":"mipmaps/generate","value":false}"#,
    );
    assert!(
        traversal.contains("403"),
        "a traversal path must be refused by the sandbox, got:\n{traversal}"
    );
    assert!(
        !dir.parent().unwrap().join("escape.png.import").exists(),
        "no sidecar may be written outside the project root"
    );

    let missing_option = http_post(port, "/api/import_settings", r#"{"path":"res://hero.png"}"#);
    assert!(
        missing_option.contains("400"),
        "a body with no option must be refused, got:\n{missing_option}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Reimporting a path with no file on disk stays a no-op acknowledgement rather
/// than a 400 — `editor_suite_b/editor_import_hooks_test.rs` drives exactly that
/// case against a server with no project, and Godot treats it as nothing to do.
#[test]
fn reimport_without_a_source_file_still_acknowledges() {
    let (_handle, port, dir) = server_on_temp_project("absent_source");

    let resp = json_of(&http_post(
        port,
        "/api/reimport",
        r#"{"path":"res://not_here.png"}"#,
    ));
    assert_eq!(resp["reimported"], serde_json::json!("res://not_here.png"));
    assert_eq!(
        resp.get("output"),
        None,
        "no artifact is claimed when there was no source to import"
    );
    assert!(
        !dir.join("not_here.png.import").exists(),
        "a no-op reimport must not leave a sidecar behind"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

//! pat-nzgdu.102: the served Theme resource editor is backed by real server
//! routes and real `.tres` persistence.
//!
//! Before this bead, `theme_editor.rs` (a complete 1200-line Theme model) was
//! orphaned — declared in `lib.rs` but called by nothing — there were no
//! `/api/theme/*` routes, and `EDITOR_HTML` never mentioned a Theme panel. The
//! bead was even auto-closed once by stale-recovery against an acceptance test
//! that did not exist. This is that test, and it guards both halves, because
//! either alone is a lie:
//!
//!  1. **Static** — `EDITOR_HTML` references the theme endpoints and defines
//!     the open/refresh/set actions, so the served UI actually calls the server
//!     instead of showing a dead panel.
//!  2. **Functional** — loading a `.tres`, then setting `Button/font_color`,
//!     round-trips through the real HTTP handlers, is reflected by
//!     `GET /api/theme`, AND is written back to the `.tres` on disk so a
//!     re-read of the file reflects the new color.

use gdcore::math::Color;
use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdeditor::theme_editor::{ThemeItem, ThemeResource};
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// (1) Static wiring guard.
// ---------------------------------------------------------------------------

#[test]
fn editor_html_wires_theme_endpoints() {
    // The endpoint the bead names explicitly, plus the rest of the surface the
    // panel drives — if any is missing the served UI can't edit a Theme.
    for needle in [
        "/api/theme/load",
        "/api/theme/item/set",
        "openThemeResource",
        "refreshThemePanel",
        "theme-panel",
        "theme-items",
    ] {
        assert!(
            EDITOR_HTML.contains(needle),
            "EDITOR_HTML must reference `{needle}` to wire the Theme editor"
        );
    }
}

// ---------------------------------------------------------------------------
// (2) Functional round-trip through the live handlers.
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

fn http_post(port: u16, path: &str, body: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

/// Process-unique fixture dir. Fixed `/tmp` paths collide across concurrent test
/// processes (the swarm's verifier lane runs alongside others) — keying on pid +
/// a counter avoids one run's `remove_dir_all` deleting another's `.tres`.
fn unique_dir(tag: &str) -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_theme_editor_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The bead's acceptance: opening a Theme `.tres`, then setting
/// `Button/font_color`, updates the resource AND persists to the `.tres` — a
/// re-read of the file reflects the new color.
#[test]
fn editor_theme_resource_editor_wired_test() {
    let dir = unique_dir("wired");
    let theme_path = dir.join("ui.theme.tres");

    // Seed a real Theme `.tres` whose Button/font_color is blue (NOT the red we
    // will set), so a passing test cannot be a no-op.
    let mut seed = ThemeResource::new();
    seed.set_item(
        "Button",
        "font_color",
        ThemeItem::color(Color::new(0.0, 0.0, 1.0, 1.0)),
    );
    std::fs::write(&theme_path, seed.to_tres()).unwrap();

    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(SceneTree::new()));
    thread::sleep(Duration::from_millis(100));

    let path_str = theme_path.to_str().unwrap();

    // Open the Theme resource.
    let load = http_post(port, "/api/theme/load", &format!(r#"{{"path":"{path_str}"}}"#));
    assert!(load.contains("200 OK"), "theme load failed:\n{load}");
    assert!(
        load.contains("Button"),
        "load response must list the seeded Button override, got:\n{load}"
    );

    // Set Button/font_color = red through the item-set route.
    let set = http_post(
        port,
        "/api/theme/item/set",
        r#"{"control_type":"Button","item_name":"font_color","data_type":"color","color":[1.0,0.0,0.0,1.0]}"#,
    );
    assert!(set.contains("200 OK"), "theme item set failed:\n{set}");

    // The served editor reports the new color (what the UI renders).
    let get = http_get(port, "/api/theme");
    assert!(get.contains("200 OK"), "GET /api/theme failed:\n{get}");
    assert!(
        get.contains("\"color\":[1"),
        "GET /api/theme must show the red Button override, got:\n{get}"
    );

    // ...and it persisted to the `.tres`: re-read the file from disk and parse.
    let on_disk = std::fs::read_to_string(&theme_path).unwrap();
    let reloaded = ThemeResource::from_tres(&on_disk);
    assert_eq!(
        reloaded.resolve_color("Button", "font_color", Color::WHITE),
        Color::new(1.0, 0.0, 0.0, 1.0),
        "setting Button/font_color must persist to the .tres; on-disk file:\n{on_disk}"
    );

    // The live EditorState model agrees with the file.
    {
        let s = handle.state().lock().unwrap();
        assert_eq!(
            s.theme_editor
                .theme()
                .resolve_color("Button", "font_color", Color::WHITE),
            Color::new(1.0, 0.0, 0.0, 1.0),
            "the in-memory Theme must match what was written to disk"
        );
    }

    handle.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Removing an item persists the removal to the `.tres` too.
#[test]
fn theme_item_remove_persists_to_tres() {
    let dir = unique_dir("remove");
    let theme_path = dir.join("ui.theme.tres");

    let mut seed = ThemeResource::new();
    seed.set_item("Button", "font_color", ThemeItem::color(Color::new(1.0, 0.0, 0.0, 1.0)));
    seed.set_item("Label", "font_size", ThemeItem::FontSize(20));
    std::fs::write(&theme_path, seed.to_tres()).unwrap();

    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(SceneTree::new()));
    thread::sleep(Duration::from_millis(100));

    let path_str = theme_path.to_str().unwrap();
    let load = http_post(port, "/api/theme/load", &format!(r#"{{"path":"{path_str}"}}"#));
    assert!(load.contains("200 OK"), "load failed:\n{load}");

    let remove = http_post(
        port,
        "/api/theme/item/remove",
        r#"{"control_type":"Button","item_name":"font_color"}"#,
    );
    assert!(remove.contains("200 OK"), "remove failed:\n{remove}");

    let on_disk = std::fs::read_to_string(&theme_path).unwrap();
    let reloaded = ThemeResource::from_tres(&on_disk);
    assert!(
        !reloaded.has_item("Button", "font_color"),
        "removed item must be gone from the .tres, got:\n{on_disk}"
    );
    // The unrelated override survives.
    assert_eq!(
        reloaded.resolve_font_size("Label", "font_size"),
        Some(20),
        "removing one item must not drop the others"
    );

    handle.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A malformed set request is rejected without corrupting the open Theme.
#[test]
fn theme_item_set_rejects_bad_request() {
    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(SceneTree::new()));
    thread::sleep(Duration::from_millis(100));

    // Missing data_type.
    let bad = http_post(
        port,
        "/api/theme/item/set",
        r#"{"control_type":"Button","item_name":"font_color"}"#,
    );
    assert!(bad.contains("400"), "missing data_type must 400, got:\n{bad}");

    // data_type present but the color payload is malformed.
    let bad2 = http_post(
        port,
        "/api/theme/item/set",
        r#"{"control_type":"Button","item_name":"font_color","data_type":"color","color":[1.0,0.0]}"#,
    );
    assert!(
        bad2.contains("400"),
        "a 2-element color must 400, got:\n{bad2}"
    );

    {
        let s = handle.state().lock().unwrap();
        assert_eq!(
            s.theme_editor.theme().override_count(),
            0,
            "a rejected set must not add an override"
        );
    }

    handle.stop();
}

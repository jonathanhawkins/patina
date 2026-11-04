//! App-level regression tests for pat-xwdlg (Game view) and pat-4jaq7 (AssetLib).
//!
//! Both main-screen modes were cosmetic: clicking GAME or ASSETLIB left the 2D
//! viewport up. The fix gives the central area dedicated Game and AssetLib
//! views, swapped in by the mode switch. These boot the real server and assert
//! the served `/editor` HTML carries each central panel wired into the switch,
//! the server reports the matching non-2D main view, and the AssetLib catalog
//! endpoint serves data.

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
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

/// A loopback stand-in for the asset catalog host, answering every request
/// with a Godot-AssetLib-shaped `{"result":[...]}` envelope.
///
/// The AssetLib endpoint no longer serves a hardcoded catalog: twelve
/// fabricated, uninstallable entries were removed in favour of an honest
/// `configured:false` empty state (pat-0q2kx.240 / pat-s3rrv.227, see
/// `api_get_assetlib_list`). A catalog is now opt-in per project via
/// `[editor] assetlib/index_url` — so to prove the browser serves real
/// entries, the fixture configures one, exactly as a project would.
fn serve_catalog() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let body = br#"{"result":[{"title":"Fixture Asset","author":"patina","category":"Scripts","cost":"MIT","version_string":"1.0","download_url":"http://example.invalid/a.zip"}]}"#;
    thread::spawn(move || {
        for _ in 0..4 {
            let Ok((mut sock, _)) = listener.accept() else {
                return;
            };
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = sock.write_all(head.as_bytes());
            let _ = sock.write_all(body);
            let _ = sock.flush();
        }
    });
    format!("http://127.0.0.1:{port}/catalog.json")
}

/// A server whose project names a catalog in `project.godot`.
fn make_server_with_catalog() -> (EditorServerHandle, u16, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "patina_assetlib_views_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("project.godot"),
        format!(
            "config_version=5\n\n[application]\n\nconfig/name=\"AssetLibViews\"\n\n[editor]\n\nassetlib/index_url=\"{}\"\n",
            serve_catalog()
        ),
    )
    .unwrap();
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let mut state = EditorState::new(tree);
    state.project_root = Some(dir.to_string_lossy().to_string());
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port, dir)
}

fn http_request(port: u16, req: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
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

#[test]
fn editor_game_mode_serves_game_view() {
    let (_h, port) = make_server();
    let html = body_of(&http_get(port, "/editor")).to_string();

    assert!(
        html.contains(r#"id="game-panel""#),
        "served /editor must contain the central game-view panel"
    );
    assert!(
        html.contains(r#"id="game-screen-img""#),
        "game view must have a screen element for the running game render"
    );
    assert!(
        html.contains("setCentralView") && html.contains(r#"id="game-toolbar""#),
        "game view must be wired into the central mode switch with its Godot-style runtime toolbar (play controls live in the top-bar run controls, matching Godot)"
    );

    let _ = http_post(port, "/api/editor/mode", r#"{"mode":"game"}"#);
    let main_view = http_get(port, "/api/editor/main_view");
    assert!(
        main_view.contains("\"is_2d_viewport\":false") && main_view.contains("game_view"),
        "game mode must serve the game_view, not the 2D viewport; got: {main_view}"
    );
}

#[test]
fn editor_assetlib_mode_serves_browser() {
    let (_h, port, dir) = make_server_with_catalog();
    let html = body_of(&http_get(port, "/editor")).to_string();

    assert!(
        html.contains(r#"id="assetlib-panel""#),
        "served /editor must contain the central AssetLib browser panel"
    );
    assert!(
        html.contains(r#"id="assetlib-search""#) && html.contains(r#"id="assetlib-grid""#),
        "AssetLib browser must have a search field and an asset grid"
    );
    assert!(
        html.contains("loadAssetLib"),
        "AssetLib browser must be wired into the central mode switch"
    );

    // The catalog endpoint must serve assets.
    let catalog = http_get(port, "/api/assetlib/list");
    assert!(
        catalog.starts_with("HTTP/1.1 200"),
        "GET /api/assetlib/list should return 200; got: {}",
        catalog.lines().next().unwrap_or("")
    );
    let cbody = body_of(&catalog);
    assert!(
        cbody.contains("\"assets\"") && cbody.contains("\"title\":\"Fixture Asset\""),
        "AssetLib catalog must contain asset entries; got: {cbody}"
    );

    let _ = http_post(port, "/api/editor/mode", r#"{"mode":"assetlib"}"#);
    let main_view = http_get(port, "/api/editor/main_view");
    assert!(
        main_view.contains("\"is_2d_viewport\":false") && main_view.contains("asset_lib"),
        "assetlib mode must serve the asset_lib browser, not the 2D viewport; got: {main_view}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

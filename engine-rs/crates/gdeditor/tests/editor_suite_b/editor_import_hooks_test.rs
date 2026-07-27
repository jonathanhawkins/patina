//! FileSystem-dock import hooks: PNG -> usable Texture2D + import settings (pat-oepf7).
//!
//! Godot's FileSystem dock recognizes importable resources, shows an Import
//! settings panel ("Import As: Texture2D" + presets/options), and reimports on
//! change; the imported PNG yields a usable Texture2D. This drives the assembled
//! server's /api/import_settings + /api/reimport hooks and decodes a real PNG
//! through the same `decode_png` path the editor's TextureCache uses.

use gdcore::math::Color;
use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdrender2d::renderer::FrameBuffer;
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

fn http(port: u16, req: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(req.as_bytes()).unwrap();
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
    http(port, &format!("POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()))
}

fn body_json(resp: &str) -> serde_json::Value {
    assert!(resp.contains("200 OK"), "expected 200, got:\n{resp}");
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or("")).expect("valid JSON")
}

#[test]
fn fs_dock_import_hooks() {
    let (handle, port) = make_server();

    // --- Import settings: a PNG is importable as a Texture2D ---
    let imp = body_json(&http_get(
        port,
        "/api/import_settings?path=res%3A//hero.png",
    ));
    assert_eq!(
        imp["importable"],
        serde_json::json!(true),
        "PNG is importable"
    );
    assert_eq!(
        imp["importer"],
        serde_json::json!("Texture2D"),
        "Import As: Texture2D"
    );
    let presets: Vec<String> = imp["presets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap().to_string())
        .collect();
    assert!(
        presets.iter().any(|p| p == "2D Pixel"),
        "texture import presets: {presets:?}"
    );
    let option_names: Vec<String> = imp["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        option_names.iter().any(|n| n == "compression"),
        "compression option present"
    );
    assert!(
        option_names.iter().any(|n| n == "filter"),
        "filter option present"
    );

    // --- Reimport-on-change hook ---
    let re = body_json(&http_post(
        port,
        "/api/reimport",
        r#"{"path":"res://hero.png"}"#,
    ));
    assert_eq!(
        re["reimported"],
        serde_json::json!("res://hero.png"),
        "reimport reports the path"
    );

    // --- The PNG produces a usable Texture2D (same decode path as TextureCache) ---
    let mut fb = FrameBuffer::new(3, 2, Color::rgb(0.0, 0.0, 1.0));
    fb.set_pixel(0, 0, Color::rgb(1.0, 0.0, 0.0));
    let png = gdrender2d::export::encode_png(&fb);
    let tex = gdrender2d::texture::decode_png(&png).expect("PNG decodes to a usable Texture2D");
    assert_eq!(tex.width, 3);
    assert_eq!(tex.height, 2);
    let p = tex.get_pixel(0, 0);
    assert!(
        (p.r - 1.0).abs() < 0.02 && p.g.abs() < 0.02 && p.b.abs() < 0.02,
        "decoded pixel matches the source"
    );

    // --- A non-importable plain resource is reported as such ---
    let txt = body_json(&http_get(
        port,
        "/api/import_settings?path=res%3A//notes.txt",
    ));
    assert_eq!(
        txt["importable"],
        serde_json::json!(false),
        ".txt is not an importable asset"
    );

    // The FileSystem dock wires the Import tab + import settings UI.
    let html = http_get(port, "/editor");
    assert!(
        html.contains("/api/import_settings"),
        "FileSystem dock wires import settings"
    );

    handle.stop();
}

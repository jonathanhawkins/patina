//! pat-vyko1: Asset browser import settings panel for each resource type.
//!
//! Integration tests covering:
//! 1. GET /api/import_settings — returns defaults per extension
//! 2. POST /api/import_settings — writes .import sidecar files
//! 3. Roundtrip: set then get import settings
//! 4. Import settings for different resource types (texture, audio, font, scene)
//! 5. Error handling for missing parameters

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::SceneTree;

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn make_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let tree = SceneTree::new();
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(300));
    (handle, port)
}

fn http_get(port: u16, path: &str) -> String {
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    http_request(port, &req)
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    http_request(port, &req)
}

fn http_request(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    String::from_utf8_lossy(&response).to_string()
}

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

// ===========================================================================
// 1. GET /api/import_settings — default params by extension
// ===========================================================================

#[test]
fn import_settings_missing_path_returns_400() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/import_settings");
    assert!(resp.contains("400"));
    handle.stop();
}

#[test]
fn import_settings_png_returns_texture_defaults() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/import_settings?path=res://icon.png");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert_eq!(v["importer"], "texture");
    assert_eq!(v["has_import_file"], false);
    // Should have texture-specific params
    let params = &v["params"];
    assert!(params["compress/mode"].is_string());
    assert!(params["mipmaps/generate"].is_boolean());
    assert!(params["flags/filter"].is_boolean());
    handle.stop();
}

#[test]
fn import_settings_wav_returns_audio_defaults() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/import_settings?path=res://sound.wav");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert_eq!(v["importer"], "wav");
    let params = &v["params"];
    assert!(params["force/8_bit"].is_boolean());
    assert!(params["force/mono"].is_boolean());
    assert!(params["edit/trim"].is_boolean());
    handle.stop();
}

#[test]
fn import_settings_ogg_returns_vorbis_defaults() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/import_settings?path=res://music.ogg");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert_eq!(v["importer"], "ogg_vorbis");
    let params = &v["params"];
    assert!(params["loop"].is_boolean());
    assert!(params["loop_offset"].is_number());
    handle.stop();
}

#[test]
fn import_settings_ttf_returns_font_defaults() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/import_settings?path=res://font.ttf");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert_eq!(v["importer"], "font_data_dynamic");
    let params = &v["params"];
    assert!(params["antialiased"].is_boolean());
    assert!(params["hinting"].is_string());
    handle.stop();
}

#[test]
fn import_settings_glb_returns_scene_defaults() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/import_settings?path=res://model.glb");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert_eq!(v["importer"], "scene");
    let params = &v["params"];
    assert!(params["animation/import"].is_boolean());
    assert!(params["animation/fps"].is_number());
    handle.stop();
}

#[test]
fn import_settings_unknown_ext_returns_empty_params() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/import_settings?path=res://data.bin");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert_eq!(v["importer"], "unknown");
    assert_eq!(v["params"], serde_json::json!({}));
    handle.stop();
}

// ===========================================================================
// 2. POST /api/import_settings — write .import sidecar
// ===========================================================================

#[test]
fn set_import_settings_missing_path_returns_400() {
    let (handle, port) = make_server();
    let resp = http_post(port, "/api/import_settings", r#"{"params":{}}"#);
    assert!(resp.contains("400"));
    handle.stop();
}

#[test]
fn set_import_settings_missing_params_returns_400() {
    let (handle, port) = make_server();
    let resp = http_post(port, "/api/import_settings", r#"{"path":"res://icon.png"}"#);
    assert!(resp.contains("400"));
    handle.stop();
}

#[test]
fn set_import_settings_writes_sidecar() {
    let dir = tempfile::TempDir::new().unwrap();
    let png_path = dir.path().join("test_icon.png");
    std::fs::write(&png_path, "PNG").unwrap();

    let (handle, port) = make_server();
    let rel = png_path.to_string_lossy().to_string();
    let body = format!(
        r#"{{"path":"{}","params":{{"compress/mode":"lossy","flags/filter":false}}}}"#,
        rel
    );
    let resp = http_post(port, "/api/import_settings", &body);
    let resp_body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(resp_body).expect("valid JSON");
    assert_eq!(v["ok"], true);
    assert_eq!(v["importer"], "texture");

    // Verify the .import sidecar was written
    let import_path = png_path.with_extension("png.import");
    assert!(import_path.exists(), ".import sidecar should be created");

    let contents = std::fs::read_to_string(&import_path).unwrap();
    assert!(contents.contains("[remap]"));
    assert!(contents.contains("[deps]"));
    assert!(contents.contains("[params]"));
    assert!(contents.contains(r#"compress/mode="lossy""#));
    assert!(contents.contains("flags/filter=false"));

    // Clean up
    std::fs::remove_file(&import_path).ok();
    handle.stop();
}

// ===========================================================================
// 3. Roundtrip: set then get
// ===========================================================================

#[test]
fn import_settings_roundtrip() {
    let dir = tempfile::TempDir::new().unwrap();
    let wav_path = dir.path().join("effect.wav");
    std::fs::write(&wav_path, "RIFF").unwrap();

    let (handle, port) = make_server();
    let rel = wav_path.to_string_lossy().to_string();

    // Set import settings
    let body = format!(
        r#"{{"path":"{}","params":{{"force/mono":true,"edit/normalize":true}}}}"#,
        rel
    );
    let resp = http_post(port, "/api/import_settings", &body);
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert_eq!(v["ok"], true);

    // Get import settings — should read from sidecar
    let resp = http_get(
        port,
        &format!("/api/import_settings?path={}", url_encode(&rel)),
    );
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert_eq!(v["has_import_file"], true);
    assert_eq!(v["importer"], "wav");
    // Params from sidecar should be present (merged as strings from INI parsing)
    let params = &v["params"];
    assert_eq!(params["force/mono"], "true");
    assert_eq!(params["edit/normalize"], "true");

    // Clean up
    std::fs::remove_file(wav_path.with_extension("wav.import")).ok();
    handle.stop();
}

/// Minimal URL encoding for query params.
fn url_encode(s: &str) -> String {
    s.replace('%', "%25")
        .replace(' ', "%20")
        .replace('/', "%2F")
        .replace(':', "%3A")
}

// ===========================================================================
// 4. ImportFile parsing (unit tests via gdresource)
// ===========================================================================

#[test]
fn parse_import_file_with_params_section() {
    let contents = r#"[remap]

importer="texture"
type="CompressedTexture2D"

[deps]

source_file="res://icon.png"

[params]

compress/mode="lossless"
mipmaps/generate=false
flags/filter=true
"#;
    let import = gdresource::parse_import_file(contents).unwrap();
    assert_eq!(import.importer(), Some("texture"));
    assert_eq!(import.resource_type(), Some("CompressedTexture2D"));
    assert_eq!(import.source_file(), Some("res://icon.png"));

    let params = import.other_sections.get("params").unwrap();
    assert_eq!(params.get("compress/mode").unwrap(), "lossless");
    assert_eq!(params.get("mipmaps/generate").unwrap(), "false");
    assert_eq!(params.get("flags/filter").unwrap(), "true");
}

#[test]
fn parse_import_file_without_params() {
    let contents = r#"[remap]

importer="wav"

[deps]

source_file="res://sound.wav"
"#;
    let import = gdresource::parse_import_file(contents).unwrap();
    assert_eq!(import.importer(), Some("wav"));
    assert!(import.other_sections.get("params").is_none());
}

// ===========================================================================
// 5. Different image extensions map to texture importer
// ===========================================================================

#[test]
fn jpg_maps_to_texture_importer() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/import_settings?path=res://photo.jpg");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert_eq!(v["importer"], "texture");
    handle.stop();
}

#[test]
fn webp_maps_to_texture_importer() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/import_settings?path=res://sprite.webp");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert_eq!(v["importer"], "texture");
    handle.stop();
}

#[test]
fn otf_maps_to_font_importer() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/import_settings?path=res://heading.otf");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert_eq!(v["importer"], "font_data_dynamic");
    handle.stop();
}

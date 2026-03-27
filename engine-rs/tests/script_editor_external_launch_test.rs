//! pat-62gpg: Script editor external editor launch support.
//!
//! Integration tests covering:
//! 1. ExternalEditorConfig — construction, placeholder expansion, is_configured
//! 2. ExternalEditorResult — all result variants
//! 3. launch_external_editor — not-configured, exec-not-found, successful launch
//! 4. ScriptEditor integration — open_in_external_editor, open_path_in_external_editor
//! 5. EditorSettings — serialization round-trip with external editor config

use gdeditor::settings::ExternalEditorConfig;
use gdeditor::{ExternalEditorResult, ScriptEditor, launch_external_editor};

// ===========================================================================
// 1. ExternalEditorConfig
// ===========================================================================

#[test]
fn config_default_is_not_configured() {
    let config = ExternalEditorConfig::default();
    assert!(!config.is_configured());
    assert!(config.exec_path.is_empty());
    assert!(config.exec_args.is_empty());
}

#[test]
fn config_with_exec_path_is_configured() {
    let config = ExternalEditorConfig {
        exec_path: "/usr/bin/code".into(),
        exec_args: vec![],
    };
    assert!(config.is_configured());
}

#[test]
fn config_build_args_default_template() {
    let config = ExternalEditorConfig {
        exec_path: "code".into(),
        exec_args: vec![], // empty → defaults to ["{file}"]
    };
    let args = config.build_args("/tmp/player.gd", 10, 5);
    assert_eq!(args, vec!["/tmp/player.gd"]);
}

#[test]
fn config_build_args_vscode_template() {
    let config = ExternalEditorConfig {
        exec_path: "code".into(),
        exec_args: vec!["--goto".into(), "{file}:{line}:{col}".into()],
    };
    let args = config.build_args("/project/main.gd", 42, 7);
    assert_eq!(args, vec!["--goto", "/project/main.gd:42:7"]);
}

#[test]
fn config_build_args_vim_template() {
    let config = ExternalEditorConfig {
        exec_path: "vim".into(),
        exec_args: vec!["+{line}".into(), "{file}".into()],
    };
    let args = config.build_args("res://enemy.gd", 100, 1);
    assert_eq!(args, vec!["+100", "res://enemy.gd"]);
}

#[test]
fn config_build_args_multiple_placeholders_in_one_arg() {
    let config = ExternalEditorConfig {
        exec_path: "editor".into(),
        exec_args: vec!["{file}:{line}:{col}".into()],
    };
    let args = config.build_args("test.gd", 5, 3);
    assert_eq!(args, vec!["test.gd:5:3"]);
}

#[test]
fn config_build_args_no_placeholders() {
    let config = ExternalEditorConfig {
        exec_path: "notepad".into(),
        exec_args: vec!["--flag".into(), "literal".into()],
    };
    let args = config.build_args("file.gd", 1, 1);
    assert_eq!(args, vec!["--flag", "literal"]);
}

// ===========================================================================
// 2. launch_external_editor — not configured
// ===========================================================================

#[test]
fn launch_not_configured() {
    let config = ExternalEditorConfig::default();
    let result = launch_external_editor(&config, "test.gd", 1, 1);
    assert_eq!(result, ExternalEditorResult::NotConfigured);
}

#[test]
fn launch_exec_not_found() {
    let config = ExternalEditorConfig {
        exec_path: "/nonexistent/editor/binary_that_does_not_exist_xyz123".into(),
        exec_args: vec!["{file}".into()],
    };
    let result = launch_external_editor(&config, "test.gd", 1, 1);
    assert!(
        matches!(result, ExternalEditorResult::ExecNotFound(ref p) if p.contains("nonexistent")),
        "expected ExecNotFound, got {result:?}"
    );
}

#[test]
fn launch_real_executable() {
    // Use `true` (always available on Unix) as a harmless executable.
    let config = ExternalEditorConfig {
        exec_path: "true".into(),
        exec_args: vec![],
    };
    let result = launch_external_editor(&config, "test.gd", 1, 1);
    assert!(
        matches!(result, ExternalEditorResult::Launched { .. }),
        "expected Launched, got {result:?}"
    );
    if let ExternalEditorResult::Launched { command, args } = result {
        assert_eq!(command, "true");
        assert_eq!(args, vec!["test.gd"]);
    }
}

// ===========================================================================
// 3. ScriptEditor integration
// ===========================================================================

#[test]
fn script_editor_open_in_external_no_active_tab() {
    let editor = ScriptEditor::new();
    let config = ExternalEditorConfig {
        exec_path: "code".into(),
        exec_args: vec![],
    };
    let result = editor.open_in_external_editor(&config);
    assert!(
        matches!(result, ExternalEditorResult::LaunchError(ref msg) if msg.contains("no active tab")),
        "expected LaunchError('no active tab'), got {result:?}"
    );
}

#[test]
fn script_editor_open_in_external_not_configured() {
    let mut editor = ScriptEditor::new();
    editor.open("res://player.gd", "extends Node2D");
    let config = ExternalEditorConfig::default();
    let result = editor.open_in_external_editor(&config);
    assert_eq!(result, ExternalEditorResult::NotConfigured);
}

#[test]
fn script_editor_open_in_external_with_cursor_position() {
    let mut editor = ScriptEditor::new();
    editor.open("res://player.gd", "extends Node2D\nvar speed = 10\n");

    // Set cursor to line 2, col 5
    editor.active_mut().unwrap().set_cursor(2, 5);

    // Use `true` as a harmless editor executable
    let config = ExternalEditorConfig {
        exec_path: "true".into(),
        exec_args: vec!["--goto".into(), "{file}:{line}:{col}".into()],
    };
    let result = editor.open_in_external_editor(&config);
    assert!(matches!(result, ExternalEditorResult::Launched { .. }));
    if let ExternalEditorResult::Launched { args, .. } = result {
        assert_eq!(args, vec!["--goto", "res://player.gd:2:5"]);
    }
}

#[test]
fn script_editor_open_path_in_external() {
    let editor = ScriptEditor::new();
    let config = ExternalEditorConfig {
        exec_path: "true".into(),
        exec_args: vec!["+{line}".into(), "{file}".into()],
    };
    let result = editor.open_path_in_external_editor(&config, "/project/main.gd", 42, 1);
    assert!(matches!(result, ExternalEditorResult::Launched { .. }));
    if let ExternalEditorResult::Launched { args, .. } = result {
        assert_eq!(args, vec!["+42", "/project/main.gd"]);
    }
}

// ===========================================================================
// 4. EditorSettings serialization with external editor
// ===========================================================================

#[test]
fn editor_settings_default_has_no_external_editor() {
    let settings = gdeditor::settings::EditorSettings::default();
    assert!(!settings.external_editor.is_configured());
    assert!(!settings.use_external_editor);
}

#[test]
fn editor_settings_roundtrip_with_external_editor() {
    let mut settings = gdeditor::settings::EditorSettings::default();
    settings.external_editor = ExternalEditorConfig {
        exec_path: "/usr/bin/code".into(),
        exec_args: vec!["--goto".into(), "{file}:{line}:{col}".into()],
    };
    settings.use_external_editor = true;

    let json = serde_json::to_string_pretty(&settings).unwrap();
    let loaded: gdeditor::settings::EditorSettings = serde_json::from_str(&json).unwrap();

    assert_eq!(loaded.external_editor.exec_path, "/usr/bin/code");
    assert_eq!(loaded.external_editor.exec_args.len(), 2);
    assert!(loaded.use_external_editor);
}

#[test]
fn editor_settings_deserialize_without_external_editor_field() {
    // Simulate loading settings saved before external_editor was added.
    let json = r#"{
        "recent_files": [],
        "window_size": [1280, 720],
        "theme": "Dark",
        "auto_save": true
    }"#;
    let settings: gdeditor::settings::EditorSettings = serde_json::from_str(json).unwrap();
    assert!(!settings.external_editor.is_configured());
    assert!(!settings.use_external_editor);
}

// ===========================================================================
// 5. Editor server API endpoints for external editor
// ===========================================================================

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

#[test]
fn api_get_external_editor_defaults() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/editor_settings/external_editor");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert_eq!(v["exec_path"], "");
    assert_eq!(v["use_external_editor"], false);
    handle.stop();
}

#[test]
fn api_set_and_get_external_editor() {
    let (handle, port) = make_server();
    let set_body = r#"{"exec_path":"/usr/bin/code","exec_args":["--goto","{file}:{line}:{col}"],"use_external_editor":true}"#;
    let resp = http_post(port, "/api/editor_settings/external_editor", set_body);
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert_eq!(v["exec_path"], "/usr/bin/code");
    assert_eq!(v["use_external_editor"], true);
    assert_eq!(v["exec_args"][0], "--goto");

    let resp = http_get(port, "/api/editor_settings/external_editor");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert_eq!(v["exec_path"], "/usr/bin/code");
    assert_eq!(v["use_external_editor"], true);
    handle.stop();
}

#[test]
fn api_open_external_not_configured() {
    let (handle, port) = make_server();
    let resp = http_post(port, "/api/script/open_external", r#"{"path":"res://main.gd"}"#);
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert_eq!(v["ok"], false);
    assert!(v["error"].as_str().unwrap().contains("configured"));
    handle.stop();
}

#[test]
fn api_open_external_missing_path() {
    let (handle, port) = make_server();
    http_post(port, "/api/editor_settings/external_editor", r#"{"exec_path":"/usr/bin/true"}"#);
    let resp = http_post(port, "/api/script/open_external", "{}");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert_eq!(v["ok"], false);
    assert!(v["error"].as_str().unwrap().contains("missing path"));
    handle.stop();
}

#[test]
fn api_open_external_exec_not_found() {
    let (handle, port) = make_server();
    http_post(port, "/api/editor_settings/external_editor", r#"{"exec_path":"/nonexistent/xyz"}"#);
    let resp = http_post(port, "/api/script/open_external", r#"{"path":"test.gd","line":1,"col":1}"#);
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert_eq!(v["ok"], false);
    assert!(v["error"].as_str().unwrap().contains("not found"));
    handle.stop();
}

#[test]
fn api_open_external_success() {
    let (handle, port) = make_server();
    http_post(port, "/api/editor_settings/external_editor", r#"{"exec_path":"/usr/bin/true","exec_args":["{file}"]}"#);
    let resp = http_post(port, "/api/script/open_external", r#"{"path":"res://player.gd","line":10,"col":5}"#);
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert_eq!(v["ok"], true);
    assert_eq!(v["command"], "/usr/bin/true");
    assert_eq!(v["args"][0], "res://player.gd");
    handle.stop();
}

// ===========================================================================
// 6. ExternalEditorResult equality and debug
// ===========================================================================

#[test]
fn external_editor_result_variants_are_distinct() {
    let not_configured = ExternalEditorResult::NotConfigured;
    let not_found = ExternalEditorResult::ExecNotFound("vim".into());
    let error = ExternalEditorResult::LaunchError("permission denied".into());
    let launched = ExternalEditorResult::Launched {
        command: "code".into(),
        args: vec!["file.gd".into()],
    };

    assert_ne!(not_configured, not_found);
    assert_ne!(not_configured, error);
    assert_ne!(not_configured, launched);
    assert_ne!(not_found, error);

    // Debug output works
    let dbg = format!("{not_configured:?}");
    assert!(dbg.contains("NotConfigured"));
}

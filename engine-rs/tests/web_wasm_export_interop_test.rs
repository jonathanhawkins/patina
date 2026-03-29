//! pat-yc4d3: Web WASM export with JavaScript interop stubs.
//!
//! Validates:
//! 1. JsBridge stub call/eval lifecycle and error handling
//! 2. JsValue boundary types and Display formatting
//! 3. JsObjectHandle creation and uniqueness
//! 4. CanvasConfig defaults, HiDPI scaling, backend selection
//! 5. WebExportConfig defaults and memory settings
//! 6. WebPlatformLayer audio context state machine
//! 7. WebPlatformLayer fullscreen toggle
//! 8. WebPlatformLayer canvas resize and pixel ratio
//! 9. WebPlatformLayer page visibility
//! 10. WebPlatformLayer download file via JS stubs
//! 11. WebStorageBackend variants and display
//! 12. ExportTemplate web .wasm output filename
//! 13. CiArtifactBuilder web target generation
//! 14. Platform targets: web has limited capabilities
//! 15. ClassDB registration for JavaScriptBridge

use gdplatform::web::{
    AudioContextState, CanvasConfig, FullscreenState, JsBridge, JsError, JsObjectHandle, JsValue,
    PageVisibility, WebExportConfig, WebPlatformLayer, WebStorageBackend,
};
use gdplatform::export::{BuildProfile, ExportConfig, ExportTemplate};
use gdplatform::platform_targets::targets_for_platform;
use gdplatform::os::Platform;

// ── JsBridge ────────────────────────────────────────────────────────

#[test]
fn js_bridge_unavailable_rejects_all_calls() {
    let mut bridge = JsBridge::new();
    assert!(!bridge.is_available());
    assert_eq!(bridge.call("window", "alert", &[]), Err(JsError::NotAvailable));
    assert_eq!(bridge.eval("1+1"), Err(JsError::NotAvailable));
}

#[test]
fn js_bridge_stub_roundtrip() {
    let mut bridge = JsBridge::new();
    bridge.set_available(true);
    bridge.stub_method("navigator", "language", JsValue::String("en-US".into()));
    let result = bridge.call("navigator", "language", &[]).unwrap();
    assert_eq!(result, JsValue::String("en-US".into()));
}

#[test]
fn js_bridge_missing_stub_errors() {
    let mut bridge = JsBridge::new();
    bridge.set_available(true);
    let result = bridge.call("window", "nonexistent", &[]);
    assert!(matches!(result, Err(JsError::PropertyNotFound(_))));
}

#[test]
fn js_bridge_call_log_records_all_calls() {
    let mut bridge = JsBridge::new();
    bridge.set_available(true);
    bridge.stub_method("console", "log", JsValue::Undefined);
    bridge.stub_method("console", "warn", JsValue::Undefined);
    let _ = bridge.call("console", "log", &[JsValue::String("hello".into())]);
    let _ = bridge.call("console", "warn", &[JsValue::Number(42.0)]);
    assert_eq!(bridge.call_log().len(), 2);
    bridge.clear_log();
    assert!(bridge.call_log().is_empty());
}

#[test]
fn js_bridge_eval_returns_undefined_stub() {
    let mut bridge = JsBridge::new();
    bridge.set_available(true);
    assert_eq!(bridge.eval("document.title"), Ok(JsValue::Undefined));
}

#[test]
fn js_bridge_handles_are_unique() {
    let mut bridge = JsBridge::new();
    let h1 = bridge.create_handle("HTMLCanvasElement");
    let h2 = bridge.create_handle("AudioContext");
    let h3 = bridge.create_handle("WebGLRenderingContext");
    assert_ne!(h1.id, h2.id);
    assert_ne!(h2.id, h3.id);
    assert_eq!(h1.type_name, "HTMLCanvasElement");
    assert_eq!(h2.type_name, "AudioContext");
}

// ── JsValue ─────────────────────────────────────────────────────────

#[test]
fn js_value_display_all_variants() {
    assert_eq!(JsValue::Undefined.to_string(), "undefined");
    assert_eq!(JsValue::Null.to_string(), "null");
    assert_eq!(JsValue::Bool(false).to_string(), "false");
    assert_eq!(JsValue::Number(2.5).to_string(), "2.5");
    assert_eq!(JsValue::String("test".into()).to_string(), "\"test\"");
    assert_eq!(JsValue::ArrayBuffer(vec![0; 8]).to_string(), "ArrayBuffer(8)");
    let handle = JsObjectHandle::new(99, "Element");
    assert_eq!(JsValue::Object(handle).to_string(), "JsObject(99, Element)");
}

#[test]
fn js_value_clone_and_eq() {
    let v1 = JsValue::ArrayBuffer(vec![1, 2, 3]);
    let v2 = v1.clone();
    assert_eq!(v1, v2);
}

// ── JsError ─────────────────────────────────────────────────────────

#[test]
fn js_error_display_all_variants() {
    assert_eq!(JsError::NotAvailable.to_string(), "WASM environment not available");
    assert_eq!(JsError::Exception("oops".into()).to_string(), "JS exception: oops");
    assert_eq!(JsError::PropertyNotFound("foo".into()).to_string(), "property 'foo' not found");
    let err = JsError::TypeMismatch { expected: "number".into(), got: "string".into() };
    assert_eq!(err.to_string(), "type mismatch: expected number, got string");
}

// ── CanvasConfig ────────────────────────────────────────────────────

#[test]
fn canvas_config_defaults() {
    let cfg = CanvasConfig::default();
    assert_eq!(cfg.element_id, "canvas");
    assert_eq!(cfg.width, 1280);
    assert_eq!(cfg.height, 720);
    assert_eq!(cfg.pixel_ratio, 1.0);
    assert!(!cfg.use_webgpu);
    assert!(cfg.antialias);
    assert!(!cfg.alpha);
}

#[test]
fn canvas_config_physical_size_hidpi() {
    let mut cfg = CanvasConfig::default();
    cfg.pixel_ratio = 2.0;
    assert_eq!(cfg.physical_size(), (2560, 1440));
}

#[test]
fn canvas_config_backend_webgl_vs_webgpu() {
    let mut cfg = CanvasConfig::default();
    assert_eq!(cfg.backend_name(), "WebGL2");
    cfg.use_webgpu = true;
    assert_eq!(cfg.backend_name(), "WebGPU");
}

// ── WebExportConfig ─────────────────────────────────────────────────

#[test]
fn web_export_config_defaults() {
    let cfg = WebExportConfig::default();
    assert_eq!(cfg.output_dir, "export/web");
    assert_eq!(cfg.html_shell, "default_shell.html");
    assert_eq!(cfg.initial_memory_mb, 32);
    assert_eq!(cfg.max_memory_mb, 2048);
    assert!(!cfg.threads_enabled);
    assert!(cfg.simd_enabled);
    assert!(cfg.show_preloader);
    assert!(cfg.focus_canvas);
}

// ── WebPlatformLayer audio ──────────────────────────────────────────

#[test]
fn audio_state_machine_uninitialized_to_running() {
    let mut layer = WebPlatformLayer::new();
    assert_eq!(layer.audio_state, AudioContextState::Uninitialized);
    assert!(layer.resume_audio().is_ok());
    assert_eq!(layer.audio_state, AudioContextState::Running);
}

#[test]
fn audio_state_machine_suspended_to_running() {
    let mut layer = WebPlatformLayer::new();
    layer.audio_state = AudioContextState::Suspended;
    assert!(layer.resume_audio().is_ok());
    assert_eq!(layer.audio_state, AudioContextState::Running);
}

#[test]
fn audio_state_machine_running_is_noop() {
    let mut layer = WebPlatformLayer::new();
    layer.audio_state = AudioContextState::Running;
    assert!(layer.resume_audio().is_ok());
    assert_eq!(layer.audio_state, AudioContextState::Running);
}

#[test]
fn audio_state_machine_closed_errors() {
    let mut layer = WebPlatformLayer::new();
    layer.audio_state = AudioContextState::Closed;
    assert!(layer.resume_audio().is_err());
}

#[test]
fn audio_context_state_display() {
    assert_eq!(AudioContextState::Uninitialized.to_string(), "uninitialized");
    assert_eq!(AudioContextState::Suspended.to_string(), "suspended");
    assert_eq!(AudioContextState::Running.to_string(), "running");
    assert_eq!(AudioContextState::Closed.to_string(), "closed");
}

// ── WebPlatformLayer fullscreen ─────────────────────────────────────

#[test]
fn fullscreen_toggle_roundtrip() {
    let mut layer = WebPlatformLayer::new();
    assert_eq!(layer.fullscreen, FullscreenState::Windowed);
    layer.request_fullscreen().unwrap();
    assert_eq!(layer.fullscreen, FullscreenState::Fullscreen);
    layer.exit_fullscreen().unwrap();
    assert_eq!(layer.fullscreen, FullscreenState::Windowed);
}

// ── WebPlatformLayer canvas ─────────────────────────────────────────

#[test]
fn canvas_resize_updates_dimensions() {
    let mut layer = WebPlatformLayer::new();
    layer.resize_canvas(1920, 1080);
    assert_eq!(layer.canvas.width, 1920);
    assert_eq!(layer.canvas.height, 1080);
}

#[test]
fn pixel_ratio_affects_physical_size() {
    let mut layer = WebPlatformLayer::new();
    layer.set_pixel_ratio(3.0);
    assert_eq!(layer.canvas.pixel_ratio, 3.0);
    let (pw, ph) = layer.canvas.physical_size();
    assert_eq!(pw, 3840);
    assert_eq!(ph, 2160);
}

// ── WebPlatformLayer visibility ─────────────────────────────────────

#[test]
fn page_visibility_toggles() {
    let mut layer = WebPlatformLayer::new();
    assert!(layer.is_page_visible());
    assert_eq!(layer.visibility, PageVisibility::Visible);
    layer.set_page_visibility(false);
    assert!(!layer.is_page_visible());
    assert_eq!(layer.visibility, PageVisibility::Hidden);
    layer.set_page_visibility(true);
    assert!(layer.is_page_visible());
}

// ── WebPlatformLayer threads ────────────────────────────────────────

#[test]
fn shared_array_buffer_requires_threads() {
    let mut layer = WebPlatformLayer::new();
    assert!(!layer.has_shared_array_buffer());
    layer.export_config.threads_enabled = true;
    assert!(layer.has_shared_array_buffer());
}

// ── WebPlatformLayer download ───────────────────────────────────────

#[test]
fn download_file_calls_js_bridge() {
    let mut layer = WebPlatformLayer::new();
    layer.js.set_available(true);
    layer.js.stub_method("URL", "createObjectURL", JsValue::String("blob:...".into()));
    layer.js.stub_method("document", "createElement", JsValue::Object(
        JsObjectHandle::new(1, "HTMLAnchorElement"),
    ));
    assert!(layer.download_file("save.dat", &[1, 2, 3]).is_ok());
    assert_eq!(layer.js.call_log().len(), 2);
}

#[test]
fn download_file_fails_without_js() {
    let mut layer = WebPlatformLayer::new();
    assert!(layer.download_file("test.dat", &[]).is_err());
}

// ── WebPlatformLayer defaults ───────────────────────────────────────

#[test]
fn web_platform_defaults() {
    let layer = WebPlatformLayer::new();
    assert_eq!(layer.audio_state, AudioContextState::Uninitialized);
    assert_eq!(layer.visibility, PageVisibility::Visible);
    assert_eq!(layer.fullscreen, FullscreenState::Windowed);
    assert_eq!(layer.storage_backend, WebStorageBackend::IndexedDB);
    assert!(layer.user_agent.is_empty());
}

// ── WebStorageBackend ───────────────────────────────────────────────

#[test]
fn storage_backend_display() {
    assert_eq!(WebStorageBackend::LocalStorage.to_string(), "localStorage");
    assert_eq!(WebStorageBackend::IndexedDB.to_string(), "IndexedDB");
    assert_eq!(WebStorageBackend::OPFS.to_string(), "OPFS");
}

// ── Export template web output ──────────────────────────────────────

#[test]
fn export_template_web_wasm_filename() {
    let config = ExportConfig::new("web", "MyGame").with_build_profile(BuildProfile::Release);
    let template = ExportTemplate::from_config(config);
    let filename = template.output_filename();
    assert!(filename.contains("wasm") || filename.contains("web"),
        "web export should produce wasm-related output, got: {}", filename);
}

#[test]
fn export_template_web_debug_and_release() {
    let config = ExportConfig::new("web", "TestApp");
    let (debug, release) = ExportTemplate::generate_debug_and_release(config);
    assert!(debug.is_debug());
    assert!(release.is_release());
}

#[test]
fn export_template_web_manifest() {
    let config = ExportConfig::new("web", "PatinaGame").with_build_profile(BuildProfile::Release);
    let template = ExportTemplate::from_config(config);
    let manifest = template.generate_manifest();
    assert!(manifest.contains("PatinaGame"), "manifest should contain app name");
    assert!(manifest.contains("web"), "manifest should reference web platform");
}

// ── Platform targets: web constraints ───────────────────────────────

#[test]
fn web_platform_targets_exist() {
    let web = targets_for_platform(Platform::Web);
    assert!(!web.is_empty(), "must define web targets");
}

#[test]
fn web_targets_have_limited_capabilities() {
    let web = targets_for_platform(Platform::Web);
    for target in &web {
        assert!(!target.gpu_supported, "Web should not claim GPU");
        assert!(!target.windowing_supported, "Web should not claim windowing");
        assert!(!target.ci_tested, "Web is not yet CI-tested");
    }
}

#[test]
fn web_target_triple_contains_wasm() {
    let web = targets_for_platform(Platform::Web);
    for target in &web {
        assert!(
            target.rust_triple.contains("wasm"),
            "Web triple should contain 'wasm', got: {}",
            target.rust_triple,
        );
    }
}

// ── ClassDB registration ────────────────────────────────────────────

#[test]
fn classdb_javascript_bridge_exists() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_exists("JavaScriptBridge"));
}

#[test]
fn classdb_javascript_bridge_has_methods() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_has_method("JavaScriptBridge", "eval"));
    assert!(gdobject::class_db::class_has_method("JavaScriptBridge", "get_interface"));
    assert!(gdobject::class_db::class_has_method("JavaScriptBridge", "create_object"));
    assert!(gdobject::class_db::class_has_method("JavaScriptBridge", "create_callback"));
    assert!(gdobject::class_db::class_has_method("JavaScriptBridge", "download_buffer"));
    assert!(gdobject::class_db::class_has_method("JavaScriptBridge", "force_fs_sync"));
}

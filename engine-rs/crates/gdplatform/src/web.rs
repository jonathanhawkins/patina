//! Web/WASM platform layer with JavaScript interop stubs.
//!
//! Provides the web platform layer for running the engine in a browser via
//! WebAssembly. Includes:
//!
//! - JavaScript interop bridge (`JsBridge`) for calling browser APIs
//! - Canvas management for WebGL/WebGPU rendering targets
//! - Browser event routing (resize, visibility, fullscreen)
//! - Web export configuration (HTML shell, preloader, memory settings)
//! - Audio context management (autoplay policy workarounds)
//! - File system access via IndexedDB/OPFS stubs
//!
//! All browser API calls are stubbed — real implementations use
//! `wasm-bindgen` / `web-sys` when targeting `wasm32-unknown-unknown`.

use std::collections::HashMap;
use std::fmt;

// ---------------------------------------------------------------------------
// JavaScript interop bridge
// ---------------------------------------------------------------------------

/// Value types that can cross the JS/WASM boundary.
#[derive(Debug, Clone, PartialEq)]
pub enum JsValue {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    /// Opaque handle to a JS object (e.g. DOM element, AudioContext).
    Object(JsObjectHandle),
    /// Typed array buffer reference.
    ArrayBuffer(Vec<u8>),
}

impl fmt::Display for JsValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Undefined => write!(f, "undefined"),
            Self::Null => write!(f, "null"),
            Self::Bool(v) => write!(f, "{}", v),
            Self::Number(v) => write!(f, "{}", v),
            Self::String(v) => write!(f, "\"{}\"", v),
            Self::Object(h) => write!(f, "{}", h),
            Self::ArrayBuffer(buf) => write!(f, "ArrayBuffer({})", buf.len()),
        }
    }
}

/// Opaque handle to a JavaScript object.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JsObjectHandle {
    pub id: u64,
    pub type_name: String,
}

impl JsObjectHandle {
    pub fn new(id: u64, type_name: impl Into<String>) -> Self {
        Self {
            id,
            type_name: type_name.into(),
        }
    }
}

impl fmt::Display for JsObjectHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JsObject({}, {})", self.id, self.type_name)
    }
}

/// Error from JavaScript interop.
#[derive(Debug, Clone, PartialEq)]
pub enum JsError {
    /// WASM environment not available.
    NotAvailable,
    /// JavaScript threw an exception.
    Exception(String),
    /// Property or method not found on the JS object.
    PropertyNotFound(String),
    /// Type conversion error.
    TypeMismatch { expected: String, got: String },
}

impl fmt::Display for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAvailable => write!(f, "WASM environment not available"),
            Self::Exception(msg) => write!(f, "JS exception: {}", msg),
            Self::PropertyNotFound(name) => write!(f, "property '{}' not found", name),
            Self::TypeMismatch { expected, got } => {
                write!(f, "type mismatch: expected {}, got {}", expected, got)
            }
        }
    }
}

/// Stub bridge for JavaScript interop from WASM.
///
/// In a real WASM build this wraps `wasm-bindgen` / `js-sys` calls.
/// In headless mode it records calls and returns stubbed values.
#[derive(Debug, Clone, Default)]
pub struct JsBridge {
    available: bool,
    call_log: Vec<(String, String, Vec<JsValue>)>,
    stubs: HashMap<String, JsValue>,
    next_handle_id: u64,
}

impl JsBridge {
    pub fn new() -> Self {
        Self {
            available: false,
            call_log: Vec::new(),
            stubs: HashMap::new(),
            next_handle_id: 1,
        }
    }

    /// Simulate the WASM environment being available.
    pub fn set_available(&mut self, available: bool) {
        self.available = available;
    }

    pub fn is_available(&self) -> bool {
        self.available
    }

    /// Register a stubbed return value for an object.method call.
    pub fn stub_method(&mut self, object: &str, method: &str, value: JsValue) {
        self.stubs
            .insert(format!("{}.{}", object, method), value);
    }

    /// Call a method on a global JS object (e.g. "document.getElementById").
    pub fn call(
        &mut self,
        object: &str,
        method: &str,
        args: &[JsValue],
    ) -> Result<JsValue, JsError> {
        if !self.available {
            return Err(JsError::NotAvailable);
        }
        let key = format!("{}.{}", object, method);
        self.call_log
            .push((object.to_string(), method.to_string(), args.to_vec()));
        self.stubs
            .get(&key)
            .cloned()
            .ok_or_else(|| JsError::PropertyNotFound(key))
    }

    /// Evaluate a JavaScript expression (stub).
    pub fn eval(&mut self, _expr: &str) -> Result<JsValue, JsError> {
        if !self.available {
            return Err(JsError::NotAvailable);
        }
        Ok(JsValue::Undefined)
    }

    /// Create a new opaque handle (simulates creating a JS object).
    pub fn create_handle(&mut self, type_name: &str) -> JsObjectHandle {
        let id = self.next_handle_id;
        self.next_handle_id += 1;
        JsObjectHandle::new(id, type_name)
    }

    /// Returns the call log for test assertions.
    pub fn call_log(&self) -> &[(String, String, Vec<JsValue>)] {
        &self.call_log
    }

    pub fn clear_log(&mut self) {
        self.call_log.clear();
    }
}

// ---------------------------------------------------------------------------
// Canvas / rendering target
// ---------------------------------------------------------------------------

/// WebGL/WebGPU canvas configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasConfig {
    /// HTML canvas element ID (e.g. "#canvas", "game-canvas").
    pub element_id: String,
    /// Requested width in CSS pixels.
    pub width: u32,
    /// Requested height in CSS pixels.
    pub height: u32,
    /// Device pixel ratio for HiDPI rendering.
    pub pixel_ratio: f64,
    /// Whether to use WebGPU (true) or WebGL2 (false).
    pub use_webgpu: bool,
    /// Enable anti-aliasing.
    pub antialias: bool,
    /// Enable alpha channel in the canvas.
    pub alpha: bool,
}

impl Default for CanvasConfig {
    fn default() -> Self {
        Self {
            element_id: "canvas".to_string(),
            width: 1280,
            height: 720,
            pixel_ratio: 1.0,
            use_webgpu: false,
            antialias: true,
            alpha: false,
        }
    }
}

impl CanvasConfig {
    /// Physical pixel dimensions accounting for device pixel ratio.
    pub fn physical_size(&self) -> (u32, u32) {
        (
            (self.width as f64 * self.pixel_ratio).round() as u32,
            (self.height as f64 * self.pixel_ratio).round() as u32,
        )
    }

    /// Returns the rendering backend name.
    pub fn backend_name(&self) -> &'static str {
        if self.use_webgpu {
            "WebGPU"
        } else {
            "WebGL2"
        }
    }
}

// ---------------------------------------------------------------------------
// Web export configuration
// ---------------------------------------------------------------------------

/// WASM export settings for packaging the engine for web deployment.
#[derive(Debug, Clone, PartialEq)]
pub struct WebExportConfig {
    /// Output directory for the exported files.
    pub output_dir: String,
    /// HTML shell template path.
    pub html_shell: String,
    /// Initial WASM heap size in MB.
    pub initial_memory_mb: u32,
    /// Maximum WASM heap size in MB (0 = unlimited growth).
    pub max_memory_mb: u32,
    /// Enable threading via SharedArrayBuffer.
    pub threads_enabled: bool,
    /// Enable SIMD instructions.
    pub simd_enabled: bool,
    /// Custom JavaScript to inject into the HTML shell.
    pub head_include: String,
    /// Progressive download: show loading bar.
    pub show_preloader: bool,
    /// Focus canvas on page load.
    pub focus_canvas: bool,
}

impl Default for WebExportConfig {
    fn default() -> Self {
        Self {
            output_dir: "export/web".to_string(),
            html_shell: "default_shell.html".to_string(),
            initial_memory_mb: 32,
            max_memory_mb: 2048,
            threads_enabled: false,
            simd_enabled: true,
            head_include: String::new(),
            show_preloader: true,
            focus_canvas: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Browser audio context
// ---------------------------------------------------------------------------

/// State of the browser AudioContext (autoplay policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioContextState {
    /// Not yet created.
    Uninitialized,
    /// Created but suspended (awaiting user gesture).
    Suspended,
    /// Running and producing audio.
    Running,
    /// Closed / released.
    Closed,
}

impl fmt::Display for AudioContextState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uninitialized => write!(f, "uninitialized"),
            Self::Suspended => write!(f, "suspended"),
            Self::Running => write!(f, "running"),
            Self::Closed => write!(f, "closed"),
        }
    }
}

// ---------------------------------------------------------------------------
// Browser visibility / fullscreen
// ---------------------------------------------------------------------------

/// Browser document visibility state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageVisibility {
    Visible,
    Hidden,
}

/// Browser fullscreen state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullscreenState {
    Windowed,
    Fullscreen,
}

// ---------------------------------------------------------------------------
// Persistent storage (IndexedDB / OPFS stubs)
// ---------------------------------------------------------------------------

/// Web storage backend for saving game data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebStorageBackend {
    /// Browser localStorage (small, synchronous).
    LocalStorage,
    /// IndexedDB (large, async).
    IndexedDB,
    /// Origin Private File System (modern, file-like).
    OPFS,
}

impl fmt::Display for WebStorageBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LocalStorage => write!(f, "localStorage"),
            Self::IndexedDB => write!(f, "IndexedDB"),
            Self::OPFS => write!(f, "OPFS"),
        }
    }
}

// ---------------------------------------------------------------------------
// Web platform layer
// ---------------------------------------------------------------------------

/// Web/WASM platform layer.
#[derive(Debug, Clone)]
pub struct WebPlatformLayer {
    pub js: JsBridge,
    pub canvas: CanvasConfig,
    pub export_config: WebExportConfig,
    pub audio_state: AudioContextState,
    pub visibility: PageVisibility,
    pub fullscreen: FullscreenState,
    pub storage_backend: WebStorageBackend,
    /// User agent string (for feature detection).
    pub user_agent: String,
}

impl Default for WebPlatformLayer {
    fn default() -> Self {
        Self {
            js: JsBridge::new(),
            canvas: CanvasConfig::default(),
            export_config: WebExportConfig::default(),
            audio_state: AudioContextState::Uninitialized,
            visibility: PageVisibility::Visible,
            fullscreen: FullscreenState::Windowed,
            storage_backend: WebStorageBackend::IndexedDB,
            user_agent: String::new(),
        }
    }
}

impl WebPlatformLayer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resume the audio context after a user gesture.
    pub fn resume_audio(&mut self) -> Result<(), JsError> {
        match self.audio_state {
            AudioContextState::Suspended => {
                self.audio_state = AudioContextState::Running;
                Ok(())
            }
            AudioContextState::Uninitialized => {
                self.audio_state = AudioContextState::Running;
                Ok(())
            }
            AudioContextState::Running => Ok(()),
            AudioContextState::Closed => Err(JsError::Exception(
                "AudioContext is closed".to_string(),
            )),
        }
    }

    /// Request fullscreen mode on the canvas element.
    pub fn request_fullscreen(&mut self) -> Result<(), JsError> {
        self.fullscreen = FullscreenState::Fullscreen;
        Ok(())
    }

    /// Exit fullscreen mode.
    pub fn exit_fullscreen(&mut self) -> Result<(), JsError> {
        self.fullscreen = FullscreenState::Windowed;
        Ok(())
    }

    /// Update canvas size (e.g. on browser resize).
    pub fn resize_canvas(&mut self, width: u32, height: u32) {
        self.canvas.width = width;
        self.canvas.height = height;
    }

    /// Update device pixel ratio (e.g. moving window between monitors).
    pub fn set_pixel_ratio(&mut self, ratio: f64) {
        self.canvas.pixel_ratio = ratio;
    }

    /// Check if SharedArrayBuffer is available (required for threads).
    pub fn has_shared_array_buffer(&self) -> bool {
        self.export_config.threads_enabled
    }

    /// Check if the page is visible (not in a background tab).
    pub fn is_page_visible(&self) -> bool {
        self.visibility == PageVisibility::Visible
    }

    /// Set page visibility (called from visibilitychange event).
    pub fn set_page_visibility(&mut self, visible: bool) {
        self.visibility = if visible {
            PageVisibility::Visible
        } else {
            PageVisibility::Hidden
        };
    }

    /// Download a file to the user's device (browser download).
    pub fn download_file(&mut self, filename: &str, data: &[u8]) -> Result<(), JsError> {
        self.js.call(
            "URL",
            "createObjectURL",
            &[JsValue::ArrayBuffer(data.to_vec())],
        )?;
        self.js.call(
            "document",
            "createElement",
            &[JsValue::String("a".to_string())],
        )?;
        // In reality: create blob URL, set href, trigger click, revoke URL
        let _ = filename; // used in real impl
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_bridge_not_available() {
        let mut bridge = JsBridge::new();
        assert!(!bridge.is_available());
        let result = bridge.call("window", "alert", &[]);
        assert_eq!(result, Err(JsError::NotAvailable));
    }

    #[test]
    fn js_bridge_stub_and_call() {
        let mut bridge = JsBridge::new();
        bridge.set_available(true);
        bridge.stub_method("document", "title", JsValue::String("My Game".into()));
        let result = bridge.call("document", "title", &[]);
        assert_eq!(result, Ok(JsValue::String("My Game".into())));
    }

    #[test]
    fn js_bridge_property_not_found() {
        let mut bridge = JsBridge::new();
        bridge.set_available(true);
        let result = bridge.call("window", "missing", &[]);
        assert!(matches!(result, Err(JsError::PropertyNotFound(_))));
    }

    #[test]
    fn js_bridge_call_log() {
        let mut bridge = JsBridge::new();
        bridge.set_available(true);
        bridge.stub_method("console", "log", JsValue::Undefined);
        let _ = bridge.call("console", "log", &[JsValue::String("hello".into())]);
        assert_eq!(bridge.call_log().len(), 1);
        assert_eq!(bridge.call_log()[0].0, "console");
        assert_eq!(bridge.call_log()[0].1, "log");
    }

    #[test]
    fn js_bridge_eval_stub() {
        let mut bridge = JsBridge::new();
        bridge.set_available(true);
        let result = bridge.eval("1 + 1");
        assert_eq!(result, Ok(JsValue::Undefined));
    }

    #[test]
    fn js_bridge_create_handle() {
        let mut bridge = JsBridge::new();
        let h1 = bridge.create_handle("HTMLCanvasElement");
        let h2 = bridge.create_handle("AudioContext");
        assert_ne!(h1.id, h2.id);
        assert_eq!(h1.type_name, "HTMLCanvasElement");
    }

    #[test]
    fn js_value_display() {
        assert_eq!(JsValue::Undefined.to_string(), "undefined");
        assert_eq!(JsValue::Null.to_string(), "null");
        assert_eq!(JsValue::Bool(true).to_string(), "true");
        assert_eq!(JsValue::Number(3.14).to_string(), "3.14");
        assert_eq!(JsValue::String("hi".into()).to_string(), "\"hi\"");
        assert_eq!(JsValue::ArrayBuffer(vec![0; 10]).to_string(), "ArrayBuffer(10)");
    }

    #[test]
    fn js_error_display() {
        assert_eq!(JsError::NotAvailable.to_string(), "WASM environment not available");
        assert_eq!(
            JsError::Exception("oops".into()).to_string(),
            "JS exception: oops"
        );
    }

    #[test]
    fn canvas_config_defaults() {
        let cfg = CanvasConfig::default();
        assert_eq!(cfg.width, 1280);
        assert_eq!(cfg.height, 720);
        assert_eq!(cfg.backend_name(), "WebGL2");
        assert_eq!(cfg.physical_size(), (1280, 720));
    }

    #[test]
    fn canvas_config_hidpi() {
        let cfg = CanvasConfig {
            pixel_ratio: 2.0,
            ..CanvasConfig::default()
        };
        assert_eq!(cfg.physical_size(), (2560, 1440));
    }

    #[test]
    fn canvas_config_webgpu() {
        let cfg = CanvasConfig {
            use_webgpu: true,
            ..CanvasConfig::default()
        };
        assert_eq!(cfg.backend_name(), "WebGPU");
    }

    #[test]
    fn web_export_config_defaults() {
        let cfg = WebExportConfig::default();
        assert_eq!(cfg.initial_memory_mb, 32);
        assert_eq!(cfg.max_memory_mb, 2048);
        assert!(cfg.simd_enabled);
        assert!(!cfg.threads_enabled);
        assert!(cfg.show_preloader);
    }

    #[test]
    fn audio_context_state_display() {
        assert_eq!(AudioContextState::Uninitialized.to_string(), "uninitialized");
        assert_eq!(AudioContextState::Suspended.to_string(), "suspended");
        assert_eq!(AudioContextState::Running.to_string(), "running");
        assert_eq!(AudioContextState::Closed.to_string(), "closed");
    }

    #[test]
    fn web_storage_backend_display() {
        assert_eq!(WebStorageBackend::LocalStorage.to_string(), "localStorage");
        assert_eq!(WebStorageBackend::IndexedDB.to_string(), "IndexedDB");
        assert_eq!(WebStorageBackend::OPFS.to_string(), "OPFS");
    }

    #[test]
    fn web_platform_audio_resume() {
        let mut layer = WebPlatformLayer::new();
        assert_eq!(layer.audio_state, AudioContextState::Uninitialized);
        assert!(layer.resume_audio().is_ok());
        assert_eq!(layer.audio_state, AudioContextState::Running);
    }

    #[test]
    fn web_platform_audio_resume_from_suspended() {
        let mut layer = WebPlatformLayer::new();
        layer.audio_state = AudioContextState::Suspended;
        assert!(layer.resume_audio().is_ok());
        assert_eq!(layer.audio_state, AudioContextState::Running);
    }

    #[test]
    fn web_platform_audio_resume_closed_errors() {
        let mut layer = WebPlatformLayer::new();
        layer.audio_state = AudioContextState::Closed;
        assert!(layer.resume_audio().is_err());
    }

    #[test]
    fn web_platform_fullscreen_toggle() {
        let mut layer = WebPlatformLayer::new();
        assert_eq!(layer.fullscreen, FullscreenState::Windowed);
        assert!(layer.request_fullscreen().is_ok());
        assert_eq!(layer.fullscreen, FullscreenState::Fullscreen);
        assert!(layer.exit_fullscreen().is_ok());
        assert_eq!(layer.fullscreen, FullscreenState::Windowed);
    }

    #[test]
    fn web_platform_resize_canvas() {
        let mut layer = WebPlatformLayer::new();
        layer.resize_canvas(1920, 1080);
        assert_eq!(layer.canvas.width, 1920);
        assert_eq!(layer.canvas.height, 1080);
    }

    #[test]
    fn web_platform_pixel_ratio() {
        let mut layer = WebPlatformLayer::new();
        layer.set_pixel_ratio(2.0);
        assert_eq!(layer.canvas.pixel_ratio, 2.0);
        assert_eq!(layer.canvas.physical_size(), (2560, 1440));
    }

    #[test]
    fn web_platform_visibility() {
        let mut layer = WebPlatformLayer::new();
        assert!(layer.is_page_visible());
        layer.set_page_visibility(false);
        assert!(!layer.is_page_visible());
        assert_eq!(layer.visibility, PageVisibility::Hidden);
        layer.set_page_visibility(true);
        assert!(layer.is_page_visible());
    }

    #[test]
    fn web_platform_shared_array_buffer() {
        let mut layer = WebPlatformLayer::new();
        assert!(!layer.has_shared_array_buffer());
        layer.export_config.threads_enabled = true;
        assert!(layer.has_shared_array_buffer());
    }

    #[test]
    fn web_platform_download_file_stub() {
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
    fn web_platform_defaults() {
        let layer = WebPlatformLayer::new();
        assert_eq!(layer.audio_state, AudioContextState::Uninitialized);
        assert_eq!(layer.visibility, PageVisibility::Visible);
        assert_eq!(layer.fullscreen, FullscreenState::Windowed);
        assert_eq!(layer.storage_backend, WebStorageBackend::IndexedDB);
        assert!(layer.user_agent.is_empty());
    }
}

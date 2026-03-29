//! # gdplatform
//!
//! Windowing, input, timing, OS integration, display management,
//! and export packaging for the Patina Engine runtime.

#![warn(clippy::all)]

pub mod backend;
pub mod ci_artifact;
pub mod clipboard;
pub mod cursor;
pub mod display;
pub mod drag_drop;
pub mod export;
pub mod http_request;
pub mod input;
pub mod native_menu;
pub mod network;
pub mod os;
pub mod platform_targets;
pub mod signing;
pub mod thread;
pub mod time;
pub mod window;

// Platform-specific modules (always compiled, but may be stubs on unsupported targets).
pub mod android;
pub mod ios;
pub mod linux;
pub mod macos;
pub mod web;
pub mod windows;

#[cfg(feature = "windowed")]
pub mod winit_backend;

// Re-export key types at the crate root.
pub use backend::{HeadlessPlatform, PlatformBackend};
pub use display::{DisplayServer, VsyncMode};
pub use export::{BuildProfile, ExportConfig, ExportTemplate, PackageError, PackageExecutor, PackageResult, ResourceEntry};
pub use input::{ActionBinding, InputEvent, InputMap, InputSnapshot, InputState, Key, MouseButton};
pub use network::{
    ConnectionStatus, MockNetwork, MockNetworkPeer, MultiplayerAPI, MultiplayerSpawner,
    MultiplayerSynchronizer, NetworkError, NetworkPeer, Packet, PeerId, RPCCall, RPCConfig,
    RPCMode, TransferMode,
};
pub use os::{current_platform, get_ticks_msec, get_ticks_usec, is_debug_build, OsInfo, Platform};
pub use platform_targets::{
    current_target, find_target_by_triple, Architecture, DesktopTarget, PlatformCapability,
    DESKTOP_TARGETS,
};
pub use time::Timer;
pub use signing::{
    sign_macos, sign_windows, MacOsSigningConfig, SigningError, SigningResult,
    WindowsSigningConfig,
};
pub use window::{HeadlessWindow, WindowConfig, WindowEvent, WindowId, WindowManager};

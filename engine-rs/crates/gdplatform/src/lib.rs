//! # gdplatform
//!
//! Windowing, input, timing, OS integration, display management,
//! and export packaging for the Patina Engine runtime.

#![warn(clippy::all)]

pub mod backend;
pub mod display;
pub mod export;
pub mod input;
pub mod network;
pub mod os;
pub mod time;
pub mod window;

#[cfg(feature = "windowed")]
pub mod winit_backend;

// Re-export key types at the crate root.
pub use backend::{HeadlessPlatform, PlatformBackend};
pub use display::{DisplayServer, VsyncMode};
pub use export::{BuildProfile, ExportConfig, ExportTemplate, PackageResult};
pub use input::{ActionBinding, InputEvent, InputMap, InputSnapshot, InputState, Key, MouseButton};
pub use network::{
    ConnectionStatus, MockNetwork, MockNetworkPeer, MultiplayerAPI, MultiplayerSpawner,
    MultiplayerSynchronizer, NetworkError, NetworkPeer, Packet, PeerId, RPCCall, RPCConfig,
    RPCMode, TransferMode,
};
pub use os::{current_platform, get_ticks_msec, get_ticks_usec, is_debug_build, OsInfo, Platform};
pub use time::Timer;
pub use window::{HeadlessWindow, WindowConfig, WindowEvent, WindowId, WindowManager};

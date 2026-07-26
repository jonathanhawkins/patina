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
pub mod enet_multiplayer;
pub mod export;
pub mod http_client;
pub mod http_request;
pub mod input;
pub mod multiplayer_peer_state;
pub mod native_menu;
pub mod network;
pub mod os;
pub mod packet_peer_stream;
pub mod packet_peer_udp;
pub mod platform_targets;
pub mod signing;
pub mod stream_peer_buffer;
pub mod stream_peer_tcp;
pub mod tcp_multiplayer;
pub mod thread;
pub mod time;
pub mod webrtc_peer;
pub mod websocket_multiplayer;
pub mod websocket_peer;
pub mod window;
pub mod xr;

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
pub use enet_multiplayer::ENetMultiplayerPeer;
pub use export::{
    BuildProfile, ExportConfig, ExportTemplate, PackageError, PackageExecutor, PackageResult,
    ResourceEntry,
};
pub use http_client::{HttpClient, HttpClientStatus, ParsedUrl};
pub use http_request::{
    HTTPRequest, HttpMethod, HttpRequestConfig, HttpResponse, HttpResultCode, RequestCompleted,
};
pub use input::{ActionBinding, InputEvent, InputMap, InputSnapshot, InputState, Key, MouseButton};
pub use network::{
    decode_rpc_packet, decode_spawn_packet, decode_sync_packet, encode_rpc_packet,
    encode_spawn_packet, encode_sync_packet, ConnectionStatus, DecodedRPC, MockNetwork,
    MockNetworkPeer, MultiplayerAPI, MultiplayerSpawner, MultiplayerSynchronizer, NetworkCommand,
    NetworkError, NetworkPeer, ObjectDecoding, Packet, PeerId, RPCCall, RPCConfig, RPCMode,
    SpawnPacket, SpawnRequest, SyncPacket, TransferMode, REPLICATION_KIND_SPAWN,
    REPLICATION_KIND_SYNC,
};
pub use os::{current_platform, get_ticks_msec, get_ticks_usec, is_debug_build, OsInfo, Platform};
pub use packet_peer_stream::PacketPeerStream;
pub use packet_peer_udp::{PacketPeerUDP, PacketPeerUdpError};
pub use platform_targets::{
    current_target, find_target_by_triple, Architecture, DesktopTarget, PlatformCapability,
    DESKTOP_TARGETS,
};
pub use signing::{
    sign_macos, sign_windows, MacOsSigningConfig, SigningError, SigningResult, WindowsSigningConfig,
};
pub use stream_peer_tcp::{StreamPeerTCP, StreamPeerTcpError, StreamPeerTcpStatus};
pub use tcp_multiplayer::TcpMultiplayerPeer;
pub use time::Timer;
pub use websocket_peer::{Frame, Opcode, WebSocketError, WebSocketPeer, WebSocketState, WriteMode};
pub use window::{HeadlessWindow, WindowConfig, WindowEvent, WindowId, WindowManager};

pub use webrtc_peer::{WebRTCMultiplayerPeer, WebRtcState};
pub use xr::{XrPose, XrServer};

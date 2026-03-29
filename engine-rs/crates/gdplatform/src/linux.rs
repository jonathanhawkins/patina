//! Linux platform layer with X11 and Wayland display backend support.
//!
//! Provides [`LinuxPlatformLayer`] which combines:
//! - A [`PlatformBackend`] for event polling and frame driving
//! - Display protocol detection (X11 vs Wayland)
//! - Linux-specific display queries (compositing, scaling, IME)
//! - Desktop environment integration (notifications, app indicators)
//!
//! All functionality works in headless mode for testing on any platform.

use crate::backend::{HeadlessPlatform, PlatformBackend};
use crate::window::{WindowConfig, WindowEvent};

// ---------------------------------------------------------------------------
// LinuxDisplayProtocol
// ---------------------------------------------------------------------------

/// The windowing protocol in use on a Linux system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinuxDisplayProtocol {
    /// X Window System (X11/Xorg).
    X11,
    /// Wayland compositor protocol.
    Wayland,
    /// XWayland — X11 compatibility layer running under Wayland.
    XWayland,
    /// No display server (headless / SSH / CI).
    Headless,
}

impl LinuxDisplayProtocol {
    /// Returns the protocol name as a human-readable string.
    pub fn name(&self) -> &'static str {
        match self {
            LinuxDisplayProtocol::X11 => "X11",
            LinuxDisplayProtocol::Wayland => "Wayland",
            LinuxDisplayProtocol::XWayland => "XWayland",
            LinuxDisplayProtocol::Headless => "Headless",
        }
    }

    /// Returns `true` if the protocol supports direct window positioning.
    ///
    /// Wayland deliberately does not allow clients to position their own
    /// windows; the compositor controls placement.
    pub fn supports_window_positioning(&self) -> bool {
        matches!(
            self,
            LinuxDisplayProtocol::X11 | LinuxDisplayProtocol::XWayland
        )
    }

    /// Returns `true` if the protocol supports global keyboard shortcuts.
    pub fn supports_global_shortcuts(&self) -> bool {
        matches!(
            self,
            LinuxDisplayProtocol::X11 | LinuxDisplayProtocol::XWayland
        )
    }

    /// Returns `true` if the protocol is Wayland-based.
    pub fn is_wayland(&self) -> bool {
        matches!(
            self,
            LinuxDisplayProtocol::Wayland | LinuxDisplayProtocol::XWayland
        )
    }

    /// Detects the current display protocol from environment variables.
    ///
    /// Checks `WAYLAND_DISPLAY` and `DISPLAY` to determine the protocol.
    /// Returns `Headless` if neither is set.
    pub fn detect() -> Self {
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            if std::env::var("DISPLAY").is_ok() {
                // Both set — likely XWayland is available.
                LinuxDisplayProtocol::XWayland
            } else {
                LinuxDisplayProtocol::Wayland
            }
        } else if std::env::var("DISPLAY").is_ok() {
            LinuxDisplayProtocol::X11
        } else {
            LinuxDisplayProtocol::Headless
        }
    }
}

// ---------------------------------------------------------------------------
// LinuxDesktopEnvironment
// ---------------------------------------------------------------------------

/// Known Linux desktop environments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinuxDesktopEnvironment {
    /// GNOME (GTK-based).
    Gnome,
    /// KDE Plasma (Qt-based).
    Kde,
    /// Xfce (lightweight GTK-based).
    Xfce,
    /// Sway, Hyprland, or other wlroots-based tiling WMs.
    Wlroots,
    /// i3, bspwm, or other X11 tiling WMs.
    TilingWm,
    /// Unknown or undetected desktop environment.
    Unknown,
}

impl LinuxDesktopEnvironment {
    /// Returns the name as a human-readable string.
    pub fn name(&self) -> &'static str {
        match self {
            LinuxDesktopEnvironment::Gnome => "GNOME",
            LinuxDesktopEnvironment::Kde => "KDE Plasma",
            LinuxDesktopEnvironment::Xfce => "Xfce",
            LinuxDesktopEnvironment::Wlroots => "wlroots",
            LinuxDesktopEnvironment::TilingWm => "Tiling WM",
            LinuxDesktopEnvironment::Unknown => "Unknown",
        }
    }

    /// Returns whether the desktop environment has a system tray / app indicator area.
    pub fn has_system_tray(&self) -> bool {
        matches!(
            self,
            LinuxDesktopEnvironment::Gnome
                | LinuxDesktopEnvironment::Kde
                | LinuxDesktopEnvironment::Xfce
        )
    }

    /// Detects the current desktop environment from environment variables.
    pub fn detect() -> Self {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let desktop_lower = desktop.to_lowercase();

        if desktop_lower.contains("gnome") || desktop_lower.contains("unity") {
            LinuxDesktopEnvironment::Gnome
        } else if desktop_lower.contains("kde") || desktop_lower.contains("plasma") {
            LinuxDesktopEnvironment::Kde
        } else if desktop_lower.contains("xfce") {
            LinuxDesktopEnvironment::Xfce
        } else if desktop_lower.contains("sway") || desktop_lower.contains("hyprland") {
            LinuxDesktopEnvironment::Wlroots
        } else if desktop_lower.contains("i3") || desktop_lower.contains("bspwm") {
            LinuxDesktopEnvironment::TilingWm
        } else {
            LinuxDesktopEnvironment::Unknown
        }
    }
}

// ---------------------------------------------------------------------------
// LinuxDisplayInfo
// ---------------------------------------------------------------------------

/// Linux-specific display information.
#[derive(Debug, Clone, PartialEq)]
pub struct LinuxDisplayInfo {
    /// The active display protocol.
    pub protocol: LinuxDisplayProtocol,
    /// The detected desktop environment.
    pub desktop: LinuxDesktopEnvironment,
    /// Display scale factor (e.g. 1.0, 1.25, 1.5, 2.0).
    pub scale_factor: f32,
    /// Whether a compositor is active (transparency, shadows).
    pub compositing_enabled: bool,
    /// Whether an input method editor (IME) is active.
    pub ime_enabled: bool,
    /// The cursor theme name (e.g. "Adwaita", "breeze_cursors").
    pub cursor_theme: String,
    /// The cursor size in pixels.
    pub cursor_size: u32,
}

impl Default for LinuxDisplayInfo {
    fn default() -> Self {
        Self {
            protocol: LinuxDisplayProtocol::Headless,
            desktop: LinuxDesktopEnvironment::Unknown,
            scale_factor: 1.0,
            compositing_enabled: false,
            ime_enabled: false,
            cursor_theme: "default".to_string(),
            cursor_size: 24,
        }
    }
}

// ---------------------------------------------------------------------------
// LinuxPlatformLayer
// ---------------------------------------------------------------------------

/// Linux-specific platform layer that integrates display protocol detection,
/// desktop environment queries, and platform backend into a single interface.
///
/// Mirrors the Linux-specific parts of Godot's `DisplayServer` and `OS`
/// singletons. Works in headless mode on any platform for testing.
#[derive(Debug)]
pub struct LinuxPlatformLayer {
    /// Linux display information.
    pub display_info: LinuxDisplayInfo,
    /// The underlying platform backend for window/event management.
    backend: HeadlessPlatform,
    /// The application name.
    app_name: String,
    /// XDG data directories for asset/resource lookup.
    xdg_data_dirs: Vec<String>,
}

impl LinuxPlatformLayer {
    /// Creates a new Linux platform layer with the given app name and window config.
    pub fn new(app_name: impl Into<String>, config: &WindowConfig) -> Self {
        Self {
            display_info: LinuxDisplayInfo {
                protocol: LinuxDisplayProtocol::detect(),
                desktop: LinuxDesktopEnvironment::detect(),
                ..LinuxDisplayInfo::default()
            },
            backend: HeadlessPlatform::from_config(config),
            app_name: app_name.into(),
            xdg_data_dirs: Self::detect_xdg_data_dirs(),
        }
    }

    /// Creates a headless Linux platform layer for testing.
    pub fn headless(app_name: impl Into<String>) -> Self {
        Self {
            display_info: LinuxDisplayInfo::default(),
            backend: HeadlessPlatform::new(1280, 720),
            app_name: app_name.into(),
            xdg_data_dirs: vec!["/usr/share".to_string(), "/usr/local/share".to_string()],
        }
    }

    /// Returns the application name.
    pub fn app_name(&self) -> &str {
        &self.app_name
    }

    // -- Display protocol queries ---------------------------------------------

    /// Returns the active display protocol.
    pub fn display_protocol(&self) -> LinuxDisplayProtocol {
        self.display_info.protocol
    }

    /// Returns `true` if running under X11 (including XWayland).
    pub fn is_x11(&self) -> bool {
        matches!(
            self.display_info.protocol,
            LinuxDisplayProtocol::X11 | LinuxDisplayProtocol::XWayland
        )
    }

    /// Returns `true` if running under a native Wayland session.
    pub fn is_wayland(&self) -> bool {
        self.display_info.protocol.is_wayland()
    }

    /// Returns `true` if running headless (no display server).
    pub fn is_headless(&self) -> bool {
        self.display_info.protocol == LinuxDisplayProtocol::Headless
    }

    /// Returns whether window positioning is supported.
    pub fn supports_window_positioning(&self) -> bool {
        self.display_info.protocol.supports_window_positioning()
    }

    /// Returns whether global keyboard shortcuts are supported.
    pub fn supports_global_shortcuts(&self) -> bool {
        self.display_info.protocol.supports_global_shortcuts()
    }

    // -- Desktop environment queries ------------------------------------------

    /// Returns the detected desktop environment.
    pub fn desktop_environment(&self) -> LinuxDesktopEnvironment {
        self.display_info.desktop
    }

    /// Returns whether the desktop has a system tray.
    pub fn has_system_tray(&self) -> bool {
        self.display_info.desktop.has_system_tray()
    }

    // -- Display properties ---------------------------------------------------

    /// Returns the display scale factor.
    pub fn scale_factor(&self) -> f32 {
        self.display_info.scale_factor
    }

    /// Sets the display scale factor.
    pub fn set_scale_factor(&mut self, scale: f32) {
        self.display_info.scale_factor = scale;
    }

    /// Returns whether compositing is active.
    pub fn compositing_enabled(&self) -> bool {
        self.display_info.compositing_enabled
    }

    /// Sets the compositing state.
    pub fn set_compositing_enabled(&mut self, enabled: bool) {
        self.display_info.compositing_enabled = enabled;
    }

    /// Returns whether an input method editor is active.
    pub fn ime_enabled(&self) -> bool {
        self.display_info.ime_enabled
    }

    /// Sets the IME state.
    pub fn set_ime_enabled(&mut self, enabled: bool) {
        self.display_info.ime_enabled = enabled;
    }

    /// Returns the cursor theme name.
    pub fn cursor_theme(&self) -> &str {
        &self.display_info.cursor_theme
    }

    /// Sets the cursor theme.
    pub fn set_cursor_theme(&mut self, theme: impl Into<String>) {
        self.display_info.cursor_theme = theme.into();
    }

    /// Returns the cursor size in pixels.
    pub fn cursor_size(&self) -> u32 {
        self.display_info.cursor_size
    }

    /// Sets the cursor size.
    pub fn set_cursor_size(&mut self, size: u32) {
        self.display_info.cursor_size = size;
    }

    // -- XDG directories ------------------------------------------------------

    /// Returns the XDG data directories.
    pub fn xdg_data_dirs(&self) -> &[String] {
        &self.xdg_data_dirs
    }

    /// Detects XDG data directories from the environment.
    fn detect_xdg_data_dirs() -> Vec<String> {
        let dirs = std::env::var("XDG_DATA_DIRS")
            .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
        dirs.split(':').map(|s| s.to_string()).collect()
    }

    // -- Backend delegation ---------------------------------------------------

    /// Returns a reference to the underlying platform backend.
    pub fn backend(&self) -> &HeadlessPlatform {
        &self.backend
    }

    /// Returns a mutable reference to the underlying platform backend.
    pub fn backend_mut(&mut self) -> &mut HeadlessPlatform {
        &mut self.backend
    }

    /// Convenience: polls window events from the backend.
    pub fn poll_window_events(&mut self) -> Vec<WindowEvent> {
        self.backend.poll_events()
    }

    /// Convenience: checks if the app should quit.
    pub fn should_quit(&self) -> bool {
        self.backend.should_quit()
    }

    /// Convenience: ends the current frame.
    pub fn end_frame(&mut self) {
        self.backend.end_frame();
    }

    /// Convenience: returns the window size.
    pub fn window_size(&self) -> (u32, u32) {
        self.backend.window_size()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- LinuxDisplayProtocol -------------------------------------------------

    #[test]
    fn protocol_names() {
        assert_eq!(LinuxDisplayProtocol::X11.name(), "X11");
        assert_eq!(LinuxDisplayProtocol::Wayland.name(), "Wayland");
        assert_eq!(LinuxDisplayProtocol::XWayland.name(), "XWayland");
        assert_eq!(LinuxDisplayProtocol::Headless.name(), "Headless");
    }

    #[test]
    fn x11_supports_window_positioning() {
        assert!(LinuxDisplayProtocol::X11.supports_window_positioning());
        assert!(LinuxDisplayProtocol::XWayland.supports_window_positioning());
    }

    #[test]
    fn wayland_no_window_positioning() {
        assert!(!LinuxDisplayProtocol::Wayland.supports_window_positioning());
        assert!(!LinuxDisplayProtocol::Headless.supports_window_positioning());
    }

    #[test]
    fn x11_supports_global_shortcuts() {
        assert!(LinuxDisplayProtocol::X11.supports_global_shortcuts());
        assert!(LinuxDisplayProtocol::XWayland.supports_global_shortcuts());
        assert!(!LinuxDisplayProtocol::Wayland.supports_global_shortcuts());
        assert!(!LinuxDisplayProtocol::Headless.supports_global_shortcuts());
    }

    #[test]
    fn is_wayland_variants() {
        assert!(!LinuxDisplayProtocol::X11.is_wayland());
        assert!(LinuxDisplayProtocol::Wayland.is_wayland());
        assert!(LinuxDisplayProtocol::XWayland.is_wayland());
        assert!(!LinuxDisplayProtocol::Headless.is_wayland());
    }

    // -- LinuxDesktopEnvironment ----------------------------------------------

    #[test]
    fn desktop_environment_names() {
        assert_eq!(LinuxDesktopEnvironment::Gnome.name(), "GNOME");
        assert_eq!(LinuxDesktopEnvironment::Kde.name(), "KDE Plasma");
        assert_eq!(LinuxDesktopEnvironment::Xfce.name(), "Xfce");
        assert_eq!(LinuxDesktopEnvironment::Wlroots.name(), "wlroots");
        assert_eq!(LinuxDesktopEnvironment::TilingWm.name(), "Tiling WM");
        assert_eq!(LinuxDesktopEnvironment::Unknown.name(), "Unknown");
    }

    #[test]
    fn system_tray_support() {
        assert!(LinuxDesktopEnvironment::Gnome.has_system_tray());
        assert!(LinuxDesktopEnvironment::Kde.has_system_tray());
        assert!(LinuxDesktopEnvironment::Xfce.has_system_tray());
        assert!(!LinuxDesktopEnvironment::Wlroots.has_system_tray());
        assert!(!LinuxDesktopEnvironment::TilingWm.has_system_tray());
        assert!(!LinuxDesktopEnvironment::Unknown.has_system_tray());
    }

    // -- LinuxDisplayInfo -----------------------------------------------------

    #[test]
    fn display_info_defaults() {
        let info = LinuxDisplayInfo::default();
        assert_eq!(info.protocol, LinuxDisplayProtocol::Headless);
        assert_eq!(info.desktop, LinuxDesktopEnvironment::Unknown);
        assert!((info.scale_factor - 1.0).abs() < f32::EPSILON);
        assert!(!info.compositing_enabled);
        assert!(!info.ime_enabled);
        assert_eq!(info.cursor_theme, "default");
        assert_eq!(info.cursor_size, 24);
    }

    // -- LinuxPlatformLayer ---------------------------------------------------

    #[test]
    fn headless_layer_creation() {
        let layer = LinuxPlatformLayer::headless("Test App");
        assert_eq!(layer.app_name(), "Test App");
        assert!(layer.is_headless());
        assert_eq!(layer.display_protocol(), LinuxDisplayProtocol::Headless);
    }

    #[test]
    fn headless_is_not_x11_or_wayland() {
        let layer = LinuxPlatformLayer::headless("App");
        assert!(!layer.is_x11());
        assert!(!layer.is_wayland());
        assert!(layer.is_headless());
    }

    #[test]
    fn headless_no_window_positioning() {
        let layer = LinuxPlatformLayer::headless("App");
        assert!(!layer.supports_window_positioning());
    }

    #[test]
    fn headless_no_global_shortcuts() {
        let layer = LinuxPlatformLayer::headless("App");
        assert!(!layer.supports_global_shortcuts());
    }

    #[test]
    fn headless_desktop_is_unknown() {
        let layer = LinuxPlatformLayer::headless("App");
        assert_eq!(
            layer.desktop_environment(),
            LinuxDesktopEnvironment::Unknown
        );
        assert!(!layer.has_system_tray());
    }

    #[test]
    fn scale_factor_default_and_set() {
        let mut layer = LinuxPlatformLayer::headless("App");
        assert!((layer.scale_factor() - 1.0).abs() < f32::EPSILON);
        layer.set_scale_factor(1.5);
        assert!((layer.scale_factor() - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn compositing_state() {
        let mut layer = LinuxPlatformLayer::headless("App");
        assert!(!layer.compositing_enabled());
        layer.set_compositing_enabled(true);
        assert!(layer.compositing_enabled());
    }

    #[test]
    fn ime_state() {
        let mut layer = LinuxPlatformLayer::headless("App");
        assert!(!layer.ime_enabled());
        layer.set_ime_enabled(true);
        assert!(layer.ime_enabled());
    }

    #[test]
    fn cursor_theme_default_and_set() {
        let mut layer = LinuxPlatformLayer::headless("App");
        assert_eq!(layer.cursor_theme(), "default");
        layer.set_cursor_theme("Adwaita");
        assert_eq!(layer.cursor_theme(), "Adwaita");
    }

    #[test]
    fn cursor_size_default_and_set() {
        let mut layer = LinuxPlatformLayer::headless("App");
        assert_eq!(layer.cursor_size(), 24);
        layer.set_cursor_size(32);
        assert_eq!(layer.cursor_size(), 32);
    }

    #[test]
    fn xdg_data_dirs_populated() {
        let layer = LinuxPlatformLayer::headless("App");
        assert!(!layer.xdg_data_dirs().is_empty());
    }

    // -- Backend delegation ---------------------------------------------------

    #[test]
    fn backend_delegation_window_size() {
        let config = WindowConfig::new().with_size(1920, 1080);
        let layer = LinuxPlatformLayer::new("App", &config);
        assert_eq!(layer.window_size(), (1920, 1080));
    }

    #[test]
    fn backend_delegation_should_quit() {
        let mut layer = LinuxPlatformLayer::headless("App");
        assert!(!layer.should_quit());
        layer.backend_mut().request_quit();
        assert!(layer.should_quit());
    }

    #[test]
    fn backend_delegation_poll_events() {
        let mut layer = LinuxPlatformLayer::headless("App");
        layer.backend_mut().push_event(WindowEvent::FocusGained);
        let events = layer.poll_window_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], WindowEvent::FocusGained);
    }

    #[test]
    fn backend_delegation_end_frame() {
        let mut layer = LinuxPlatformLayer::headless("App");
        assert_eq!(layer.backend().frames_run(), 0);
        layer.end_frame();
        assert_eq!(layer.backend().frames_run(), 1);
    }

    // -- X11/Wayland simulated configurations ---------------------------------

    #[test]
    fn simulate_x11_session() {
        let mut layer = LinuxPlatformLayer::headless("App");
        layer.display_info.protocol = LinuxDisplayProtocol::X11;
        layer.display_info.desktop = LinuxDesktopEnvironment::Gnome;
        layer.display_info.compositing_enabled = true;

        assert!(layer.is_x11());
        assert!(!layer.is_wayland());
        assert!(layer.supports_window_positioning());
        assert!(layer.supports_global_shortcuts());
        assert!(layer.compositing_enabled());
        assert!(layer.has_system_tray());
    }

    #[test]
    fn simulate_wayland_session() {
        let mut layer = LinuxPlatformLayer::headless("App");
        layer.display_info.protocol = LinuxDisplayProtocol::Wayland;
        layer.display_info.desktop = LinuxDesktopEnvironment::Gnome;
        layer.display_info.compositing_enabled = true;
        layer.display_info.scale_factor = 2.0;

        assert!(layer.is_wayland());
        assert!(!layer.is_x11());
        assert!(!layer.supports_window_positioning());
        assert!(!layer.supports_global_shortcuts());
        assert!((layer.scale_factor() - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn simulate_xwayland_session() {
        let mut layer = LinuxPlatformLayer::headless("App");
        layer.display_info.protocol = LinuxDisplayProtocol::XWayland;
        layer.display_info.desktop = LinuxDesktopEnvironment::Kde;

        assert!(layer.is_x11()); // XWayland counts as X11
        assert!(layer.is_wayland()); // Also Wayland-based
        assert!(layer.supports_window_positioning());
        assert!(layer.has_system_tray());
    }

    #[test]
    fn simulate_tiling_wm() {
        let mut layer = LinuxPlatformLayer::headless("App");
        layer.display_info.protocol = LinuxDisplayProtocol::X11;
        layer.display_info.desktop = LinuxDesktopEnvironment::TilingWm;

        assert!(layer.is_x11());
        assert!(!layer.has_system_tray());
    }

    #[test]
    fn simulate_wlroots_compositor() {
        let mut layer = LinuxPlatformLayer::headless("App");
        layer.display_info.protocol = LinuxDisplayProtocol::Wayland;
        layer.display_info.desktop = LinuxDesktopEnvironment::Wlroots;

        assert!(layer.is_wayland());
        assert!(!layer.supports_window_positioning());
        assert!(!layer.has_system_tray());
    }

    #[test]
    fn full_linux_workflow() {
        let mut layer = LinuxPlatformLayer::headless("Patina Engine");

        // Configure as a Wayland + GNOME session.
        layer.display_info.protocol = LinuxDisplayProtocol::Wayland;
        layer.display_info.desktop = LinuxDesktopEnvironment::Gnome;
        layer.display_info.compositing_enabled = true;
        layer.display_info.scale_factor = 1.25;
        layer.set_cursor_theme("Adwaita");
        layer.set_cursor_size(32);
        layer.set_ime_enabled(true);

        // Verify state.
        assert_eq!(layer.app_name(), "Patina Engine");
        assert!(layer.is_wayland());
        assert!(!layer.supports_window_positioning());
        assert!(layer.compositing_enabled());
        assert!((layer.scale_factor() - 1.25).abs() < f32::EPSILON);
        assert_eq!(layer.cursor_theme(), "Adwaita");
        assert_eq!(layer.cursor_size(), 32);
        assert!(layer.ime_enabled());
        assert!(layer.has_system_tray());

        // Simulate a few frames.
        layer.end_frame();
        layer.end_frame();
        assert_eq!(layer.backend().frames_run(), 2);
        assert!(!layer.should_quit());
    }
}

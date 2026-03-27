//! Windows platform layer with Win32 windowing backend.
//!
//! Provides [`WindowsPlatformLayer`] which combines:
//! - A [`PlatformBackend`] for event polling and frame driving
//! - DPI awareness mode detection and scaling
//! - Windows-specific display queries (DWM compositing, theme, taskbar)
//! - Taskbar integration (progress, overlay icon, flash)
//!
//! All functionality works in headless mode for testing on any platform.

use crate::backend::{HeadlessPlatform, PlatformBackend};
use crate::window::{WindowConfig, WindowEvent};

// ---------------------------------------------------------------------------
// DpiAwarenessMode
// ---------------------------------------------------------------------------

/// DPI awareness mode for Win32 applications.
///
/// Controls how Windows scales the application window on high-DPI displays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DpiAwarenessMode {
    /// The application is not DPI-aware; Windows will bitmap-stretch it.
    Unaware,
    /// System DPI awareness — one scale factor for all monitors.
    System,
    /// Per-monitor DPI awareness (v1) — the app handles DPI changes per monitor.
    PerMonitor,
    /// Per-monitor DPI awareness v2 — includes non-client area scaling.
    PerMonitorV2,
}

impl DpiAwarenessMode {
    /// Returns the mode name as a human-readable string.
    pub fn name(&self) -> &'static str {
        match self {
            DpiAwarenessMode::Unaware => "Unaware",
            DpiAwarenessMode::System => "System",
            DpiAwarenessMode::PerMonitor => "Per-Monitor",
            DpiAwarenessMode::PerMonitorV2 => "Per-Monitor V2",
        }
    }

    /// Returns whether the mode handles per-monitor DPI changes.
    pub fn is_per_monitor(&self) -> bool {
        matches!(self, DpiAwarenessMode::PerMonitor | DpiAwarenessMode::PerMonitorV2)
    }

    /// Returns the recommended mode for modern applications.
    pub fn recommended() -> Self {
        DpiAwarenessMode::PerMonitorV2
    }
}

// ---------------------------------------------------------------------------
// TaskbarProgressState
// ---------------------------------------------------------------------------

/// Progress state shown on the Windows taskbar button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskbarProgressState {
    /// No progress indicator.
    None,
    /// Indeterminate progress (pulsing green).
    Indeterminate,
    /// Normal progress (green bar, 0–100%).
    Normal,
    /// Error state (red bar).
    Error,
    /// Paused state (yellow bar).
    Paused,
}

impl TaskbarProgressState {
    /// Returns the state name as a human-readable string.
    pub fn name(&self) -> &'static str {
        match self {
            TaskbarProgressState::None => "None",
            TaskbarProgressState::Indeterminate => "Indeterminate",
            TaskbarProgressState::Normal => "Normal",
            TaskbarProgressState::Error => "Error",
            TaskbarProgressState::Paused => "Paused",
        }
    }
}

// ---------------------------------------------------------------------------
// WindowsTheme
// ---------------------------------------------------------------------------

/// The Windows system-wide appearance theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowsTheme {
    /// Light theme.
    Light,
    /// Dark theme (Windows 10 1809+).
    Dark,
}

// ---------------------------------------------------------------------------
// WindowsDisplayInfo
// ---------------------------------------------------------------------------

/// Windows-specific display information.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowsDisplayInfo {
    /// The DPI awareness mode.
    pub dpi_awareness: DpiAwarenessMode,
    /// Display scale factor (e.g. 1.0, 1.25, 1.5, 2.0).
    pub scale_factor: f32,
    /// The system DPI value (96 = 100%, 120 = 125%, 144 = 150%, 192 = 200%).
    pub system_dpi: u32,
    /// Whether DWM (Desktop Window Manager) compositing is active.
    pub dwm_compositing: bool,
    /// The system appearance theme.
    pub theme: WindowsTheme,
    /// Whether the system uses accent color on title bars.
    pub accent_on_title_bars: bool,
    /// Whether transparency effects are enabled.
    pub transparency_enabled: bool,
    /// Whether animations are enabled in system settings.
    pub animations_enabled: bool,
}

impl Default for WindowsDisplayInfo {
    fn default() -> Self {
        Self {
            dpi_awareness: DpiAwarenessMode::PerMonitorV2,
            scale_factor: 1.0,
            system_dpi: 96,
            dwm_compositing: true,
            theme: WindowsTheme::Light,
            accent_on_title_bars: false,
            transparency_enabled: true,
            animations_enabled: true,
        }
    }
}

// ---------------------------------------------------------------------------
// TaskbarState
// ---------------------------------------------------------------------------

/// Taskbar button state for the application window.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskbarState {
    /// The progress display state.
    pub progress_state: TaskbarProgressState,
    /// The progress value (0–100).
    pub progress_value: u32,
    /// Whether the taskbar button should flash to attract attention.
    pub flash_requested: bool,
    /// Optional overlay icon description (badge on the taskbar icon).
    pub overlay_description: Option<String>,
}

impl Default for TaskbarState {
    fn default() -> Self {
        Self {
            progress_state: TaskbarProgressState::None,
            progress_value: 0,
            flash_requested: false,
            overlay_description: None,
        }
    }
}

// ---------------------------------------------------------------------------
// WindowsPlatformLayer
// ---------------------------------------------------------------------------

/// Windows-specific platform layer that integrates Win32 windowing backend,
/// DPI awareness, display queries, and taskbar integration.
///
/// Mirrors the Windows-specific parts of Godot's `DisplayServer` and `OS`
/// singletons. Works in headless mode on any platform for testing.
#[derive(Debug)]
pub struct WindowsPlatformLayer {
    /// Windows display information.
    pub display_info: WindowsDisplayInfo,
    /// Taskbar integration state.
    taskbar: TaskbarState,
    /// The underlying platform backend for window/event management.
    backend: HeadlessPlatform,
    /// The application name.
    app_name: String,
    /// Whether the window is in the foreground.
    is_foreground: bool,
    /// Whether console output is attached (for CLI mode).
    console_attached: bool,
}

impl WindowsPlatformLayer {
    /// Creates a new Windows platform layer with the given app name and window config.
    pub fn new(app_name: impl Into<String>, config: &WindowConfig) -> Self {
        Self {
            display_info: WindowsDisplayInfo::default(),
            taskbar: TaskbarState::default(),
            backend: HeadlessPlatform::from_config(config),
            app_name: app_name.into(),
            is_foreground: true,
            console_attached: false,
        }
    }

    /// Creates a headless Windows platform layer for testing.
    pub fn headless(app_name: impl Into<String>) -> Self {
        Self {
            display_info: WindowsDisplayInfo::default(),
            taskbar: TaskbarState::default(),
            backend: HeadlessPlatform::new(1280, 720),
            app_name: app_name.into(),
            is_foreground: true,
            console_attached: false,
        }
    }

    /// Returns the application name.
    pub fn app_name(&self) -> &str {
        &self.app_name
    }

    // -- DPI awareness --------------------------------------------------------

    /// Returns the current DPI awareness mode.
    pub fn dpi_awareness(&self) -> DpiAwarenessMode {
        self.display_info.dpi_awareness
    }

    /// Sets the DPI awareness mode.
    ///
    /// In a real Win32 application this must be set before window creation.
    pub fn set_dpi_awareness(&mut self, mode: DpiAwarenessMode) {
        self.display_info.dpi_awareness = mode;
    }

    /// Returns whether per-monitor DPI handling is active.
    pub fn is_per_monitor_dpi(&self) -> bool {
        self.display_info.dpi_awareness.is_per_monitor()
    }

    /// Returns the system DPI value.
    pub fn system_dpi(&self) -> u32 {
        self.display_info.system_dpi
    }

    /// Sets the system DPI value and updates the scale factor accordingly.
    pub fn set_system_dpi(&mut self, dpi: u32) {
        self.display_info.system_dpi = dpi;
        self.display_info.scale_factor = dpi as f32 / 96.0;
    }

    /// Returns the display scale factor.
    pub fn scale_factor(&self) -> f32 {
        self.display_info.scale_factor
    }

    // -- Display queries ------------------------------------------------------

    /// Returns whether DWM compositing is active.
    pub fn dwm_compositing(&self) -> bool {
        self.display_info.dwm_compositing
    }

    /// Sets the DWM compositing state.
    pub fn set_dwm_compositing(&mut self, enabled: bool) {
        self.display_info.dwm_compositing = enabled;
    }

    /// Returns the current system theme.
    pub fn system_theme(&self) -> WindowsTheme {
        self.display_info.theme
    }

    /// Sets the system theme.
    pub fn set_system_theme(&mut self, theme: WindowsTheme) {
        self.display_info.theme = theme;
    }

    /// Returns whether accent color is applied to title bars.
    pub fn accent_on_title_bars(&self) -> bool {
        self.display_info.accent_on_title_bars
    }

    /// Sets the accent-on-title-bars flag.
    pub fn set_accent_on_title_bars(&mut self, enabled: bool) {
        self.display_info.accent_on_title_bars = enabled;
    }

    /// Returns whether transparency effects are enabled.
    pub fn transparency_enabled(&self) -> bool {
        self.display_info.transparency_enabled
    }

    /// Sets the transparency effects flag.
    pub fn set_transparency_enabled(&mut self, enabled: bool) {
        self.display_info.transparency_enabled = enabled;
    }

    /// Returns whether system animations are enabled.
    pub fn animations_enabled(&self) -> bool {
        self.display_info.animations_enabled
    }

    /// Sets the animations enabled flag.
    pub fn set_animations_enabled(&mut self, enabled: bool) {
        self.display_info.animations_enabled = enabled;
    }

    // -- Taskbar integration --------------------------------------------------

    /// Returns the current taskbar progress state.
    pub fn taskbar_progress_state(&self) -> TaskbarProgressState {
        self.taskbar.progress_state
    }

    /// Sets the taskbar progress state and value.
    ///
    /// `value` is clamped to 0–100. Only meaningful when `state` is
    /// [`TaskbarProgressState::Normal`], [`Error`](TaskbarProgressState::Error),
    /// or [`Paused`](TaskbarProgressState::Paused).
    pub fn set_taskbar_progress(&mut self, state: TaskbarProgressState, value: u32) {
        self.taskbar.progress_state = state;
        self.taskbar.progress_value = value.min(100);
    }

    /// Returns the current taskbar progress value (0–100).
    pub fn taskbar_progress_value(&self) -> u32 {
        self.taskbar.progress_value
    }

    /// Requests the taskbar button to flash, attracting user attention.
    pub fn flash_taskbar(&mut self) {
        self.taskbar.flash_requested = true;
    }

    /// Clears the taskbar flash request.
    pub fn clear_flash(&mut self) {
        self.taskbar.flash_requested = false;
    }

    /// Returns whether a flash has been requested.
    pub fn is_flash_requested(&self) -> bool {
        self.taskbar.flash_requested
    }

    /// Sets an overlay icon description on the taskbar button (e.g. "Playing").
    pub fn set_overlay_icon(&mut self, description: impl Into<String>) {
        self.taskbar.overlay_description = Some(description.into());
    }

    /// Clears the overlay icon.
    pub fn clear_overlay_icon(&mut self) {
        self.taskbar.overlay_description = None;
    }

    /// Returns the current overlay icon description.
    pub fn overlay_icon(&self) -> Option<&str> {
        self.taskbar.overlay_description.as_deref()
    }

    // -- Application state ----------------------------------------------------

    /// Returns whether the application window is in the foreground.
    pub fn is_foreground(&self) -> bool {
        self.is_foreground
    }

    /// Sets the foreground state.
    pub fn set_foreground(&mut self, foreground: bool) {
        self.is_foreground = foreground;
    }

    /// Returns whether a console window is attached.
    pub fn console_attached(&self) -> bool {
        self.console_attached
    }

    /// Attaches or detaches the console (for debug output in GUI apps).
    pub fn set_console_attached(&mut self, attached: bool) {
        self.console_attached = attached;
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

    // -- DpiAwarenessMode -----------------------------------------------------

    #[test]
    fn dpi_awareness_names() {
        assert_eq!(DpiAwarenessMode::Unaware.name(), "Unaware");
        assert_eq!(DpiAwarenessMode::System.name(), "System");
        assert_eq!(DpiAwarenessMode::PerMonitor.name(), "Per-Monitor");
        assert_eq!(DpiAwarenessMode::PerMonitorV2.name(), "Per-Monitor V2");
    }

    #[test]
    fn dpi_awareness_per_monitor() {
        assert!(!DpiAwarenessMode::Unaware.is_per_monitor());
        assert!(!DpiAwarenessMode::System.is_per_monitor());
        assert!(DpiAwarenessMode::PerMonitor.is_per_monitor());
        assert!(DpiAwarenessMode::PerMonitorV2.is_per_monitor());
    }

    #[test]
    fn dpi_awareness_recommended() {
        assert_eq!(DpiAwarenessMode::recommended(), DpiAwarenessMode::PerMonitorV2);
    }

    // -- TaskbarProgressState -------------------------------------------------

    #[test]
    fn taskbar_progress_state_names() {
        assert_eq!(TaskbarProgressState::None.name(), "None");
        assert_eq!(TaskbarProgressState::Indeterminate.name(), "Indeterminate");
        assert_eq!(TaskbarProgressState::Normal.name(), "Normal");
        assert_eq!(TaskbarProgressState::Error.name(), "Error");
        assert_eq!(TaskbarProgressState::Paused.name(), "Paused");
    }

    // -- WindowsDisplayInfo ---------------------------------------------------

    #[test]
    fn display_info_defaults() {
        let info = WindowsDisplayInfo::default();
        assert_eq!(info.dpi_awareness, DpiAwarenessMode::PerMonitorV2);
        assert!((info.scale_factor - 1.0).abs() < f32::EPSILON);
        assert_eq!(info.system_dpi, 96);
        assert!(info.dwm_compositing);
        assert_eq!(info.theme, WindowsTheme::Light);
        assert!(!info.accent_on_title_bars);
        assert!(info.transparency_enabled);
        assert!(info.animations_enabled);
    }

    // -- WindowsPlatformLayer -------------------------------------------------

    #[test]
    fn headless_layer_creation() {
        let layer = WindowsPlatformLayer::headless("Test App");
        assert_eq!(layer.app_name(), "Test App");
        assert!(layer.is_foreground());
        assert!(!layer.console_attached());
    }

    #[test]
    fn dpi_awareness_default_and_set() {
        let mut layer = WindowsPlatformLayer::headless("App");
        assert_eq!(layer.dpi_awareness(), DpiAwarenessMode::PerMonitorV2);
        assert!(layer.is_per_monitor_dpi());

        layer.set_dpi_awareness(DpiAwarenessMode::System);
        assert_eq!(layer.dpi_awareness(), DpiAwarenessMode::System);
        assert!(!layer.is_per_monitor_dpi());
    }

    #[test]
    fn system_dpi_updates_scale_factor() {
        let mut layer = WindowsPlatformLayer::headless("App");
        assert_eq!(layer.system_dpi(), 96);
        assert!((layer.scale_factor() - 1.0).abs() < f32::EPSILON);

        layer.set_system_dpi(144);
        assert_eq!(layer.system_dpi(), 144);
        assert!((layer.scale_factor() - 1.5).abs() < f32::EPSILON);

        layer.set_system_dpi(192);
        assert!((layer.scale_factor() - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn dwm_compositing_state() {
        let mut layer = WindowsPlatformLayer::headless("App");
        assert!(layer.dwm_compositing());
        layer.set_dwm_compositing(false);
        assert!(!layer.dwm_compositing());
    }

    #[test]
    fn system_theme_default_and_set() {
        let mut layer = WindowsPlatformLayer::headless("App");
        assert_eq!(layer.system_theme(), WindowsTheme::Light);
        layer.set_system_theme(WindowsTheme::Dark);
        assert_eq!(layer.system_theme(), WindowsTheme::Dark);
    }

    #[test]
    fn accent_on_title_bars() {
        let mut layer = WindowsPlatformLayer::headless("App");
        assert!(!layer.accent_on_title_bars());
        layer.set_accent_on_title_bars(true);
        assert!(layer.accent_on_title_bars());
    }

    #[test]
    fn transparency_effects() {
        let mut layer = WindowsPlatformLayer::headless("App");
        assert!(layer.transparency_enabled());
        layer.set_transparency_enabled(false);
        assert!(!layer.transparency_enabled());
    }

    #[test]
    fn animations_enabled() {
        let mut layer = WindowsPlatformLayer::headless("App");
        assert!(layer.animations_enabled());
        layer.set_animations_enabled(false);
        assert!(!layer.animations_enabled());
    }

    // -- Taskbar integration --------------------------------------------------

    #[test]
    fn taskbar_progress_default() {
        let layer = WindowsPlatformLayer::headless("App");
        assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::None);
        assert_eq!(layer.taskbar_progress_value(), 0);
    }

    #[test]
    fn taskbar_progress_set() {
        let mut layer = WindowsPlatformLayer::headless("App");
        layer.set_taskbar_progress(TaskbarProgressState::Normal, 75);
        assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::Normal);
        assert_eq!(layer.taskbar_progress_value(), 75);
    }

    #[test]
    fn taskbar_progress_clamps_to_100() {
        let mut layer = WindowsPlatformLayer::headless("App");
        layer.set_taskbar_progress(TaskbarProgressState::Normal, 200);
        assert_eq!(layer.taskbar_progress_value(), 100);
    }

    #[test]
    fn taskbar_progress_error_state() {
        let mut layer = WindowsPlatformLayer::headless("App");
        layer.set_taskbar_progress(TaskbarProgressState::Error, 50);
        assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::Error);
        assert_eq!(layer.taskbar_progress_value(), 50);
    }

    #[test]
    fn taskbar_progress_paused_state() {
        let mut layer = WindowsPlatformLayer::headless("App");
        layer.set_taskbar_progress(TaskbarProgressState::Paused, 30);
        assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::Paused);
    }

    #[test]
    fn taskbar_progress_indeterminate() {
        let mut layer = WindowsPlatformLayer::headless("App");
        layer.set_taskbar_progress(TaskbarProgressState::Indeterminate, 0);
        assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::Indeterminate);
    }

    #[test]
    fn taskbar_flash() {
        let mut layer = WindowsPlatformLayer::headless("App");
        assert!(!layer.is_flash_requested());
        layer.flash_taskbar();
        assert!(layer.is_flash_requested());
        layer.clear_flash();
        assert!(!layer.is_flash_requested());
    }

    #[test]
    fn overlay_icon_default_is_none() {
        let layer = WindowsPlatformLayer::headless("App");
        assert!(layer.overlay_icon().is_none());
    }

    #[test]
    fn overlay_icon_set_and_clear() {
        let mut layer = WindowsPlatformLayer::headless("App");
        layer.set_overlay_icon("Playing");
        assert_eq!(layer.overlay_icon(), Some("Playing"));
        layer.clear_overlay_icon();
        assert!(layer.overlay_icon().is_none());
    }

    // -- Application state ----------------------------------------------------

    #[test]
    fn foreground_state() {
        let mut layer = WindowsPlatformLayer::headless("App");
        assert!(layer.is_foreground());
        layer.set_foreground(false);
        assert!(!layer.is_foreground());
        layer.set_foreground(true);
        assert!(layer.is_foreground());
    }

    #[test]
    fn console_attached_state() {
        let mut layer = WindowsPlatformLayer::headless("App");
        assert!(!layer.console_attached());
        layer.set_console_attached(true);
        assert!(layer.console_attached());
    }

    // -- Backend delegation ---------------------------------------------------

    #[test]
    fn backend_delegation_window_size() {
        let config = WindowConfig::new().with_size(1920, 1080);
        let layer = WindowsPlatformLayer::new("App", &config);
        assert_eq!(layer.window_size(), (1920, 1080));
    }

    #[test]
    fn backend_delegation_should_quit() {
        let mut layer = WindowsPlatformLayer::headless("App");
        assert!(!layer.should_quit());
        layer.backend_mut().request_quit();
        assert!(layer.should_quit());
    }

    #[test]
    fn backend_delegation_poll_events() {
        let mut layer = WindowsPlatformLayer::headless("App");
        layer.backend_mut().push_event(WindowEvent::FocusGained);
        let events = layer.poll_window_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], WindowEvent::FocusGained);
    }

    #[test]
    fn backend_delegation_end_frame() {
        let mut layer = WindowsPlatformLayer::headless("App");
        assert_eq!(layer.backend().frames_run(), 0);
        layer.end_frame();
        assert_eq!(layer.backend().frames_run(), 1);
    }

    // -- Simulated Windows configurations -------------------------------------

    #[test]
    fn simulate_high_dpi_display() {
        let mut layer = WindowsPlatformLayer::headless("App");
        layer.set_system_dpi(192);
        layer.set_dpi_awareness(DpiAwarenessMode::PerMonitorV2);

        assert!(layer.is_per_monitor_dpi());
        assert!((layer.scale_factor() - 2.0).abs() < f32::EPSILON);
        assert_eq!(layer.system_dpi(), 192);
    }

    #[test]
    fn simulate_dark_mode_session() {
        let mut layer = WindowsPlatformLayer::headless("App");
        layer.set_system_theme(WindowsTheme::Dark);
        layer.set_accent_on_title_bars(true);
        layer.set_transparency_enabled(true);

        assert_eq!(layer.system_theme(), WindowsTheme::Dark);
        assert!(layer.accent_on_title_bars());
        assert!(layer.transparency_enabled());
    }

    #[test]
    fn simulate_legacy_dpi_unaware() {
        let mut layer = WindowsPlatformLayer::headless("App");
        layer.set_dpi_awareness(DpiAwarenessMode::Unaware);
        layer.set_system_dpi(144);

        assert!(!layer.is_per_monitor_dpi());
        assert_eq!(layer.dpi_awareness(), DpiAwarenessMode::Unaware);
    }

    #[test]
    fn full_windows_workflow() {
        let mut layer = WindowsPlatformLayer::headless("Patina Engine");

        // 1. Configure display.
        layer.set_system_dpi(120);
        layer.set_system_theme(WindowsTheme::Dark);
        layer.set_dwm_compositing(true);
        layer.set_dpi_awareness(DpiAwarenessMode::PerMonitorV2);

        // Verify state.
        assert_eq!(layer.app_name(), "Patina Engine");
        assert!((layer.scale_factor() - 1.25).abs() < f32::EPSILON);
        assert_eq!(layer.system_theme(), WindowsTheme::Dark);
        assert!(layer.dwm_compositing());
        assert!(layer.is_per_monitor_dpi());

        // 2. Taskbar integration.
        layer.set_taskbar_progress(TaskbarProgressState::Normal, 50);
        assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::Normal);
        assert_eq!(layer.taskbar_progress_value(), 50);

        layer.set_overlay_icon("Loading");
        assert_eq!(layer.overlay_icon(), Some("Loading"));

        layer.flash_taskbar();
        assert!(layer.is_flash_requested());

        // 3. Simulate frame loop.
        layer.end_frame();
        layer.end_frame();
        assert_eq!(layer.backend().frames_run(), 2);
        assert!(!layer.should_quit());

        // 4. App loses focus.
        layer.set_foreground(false);
        assert!(!layer.is_foreground());

        // 5. Complete progress.
        layer.set_taskbar_progress(TaskbarProgressState::None, 0);
        layer.clear_overlay_icon();
        layer.clear_flash();
        assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::None);
        assert!(layer.overlay_icon().is_none());
        assert!(!layer.is_flash_requested());
    }

    #[test]
    fn taskbar_progress_transitions() {
        let mut layer = WindowsPlatformLayer::headless("App");

        // Normal → Error → Paused → None
        layer.set_taskbar_progress(TaskbarProgressState::Normal, 60);
        assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::Normal);

        layer.set_taskbar_progress(TaskbarProgressState::Error, 60);
        assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::Error);

        layer.set_taskbar_progress(TaskbarProgressState::Paused, 60);
        assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::Paused);

        layer.set_taskbar_progress(TaskbarProgressState::None, 0);
        assert_eq!(layer.taskbar_progress_state(), TaskbarProgressState::None);
    }

    #[test]
    fn new_with_config_uses_dimensions() {
        let config = WindowConfig::new().with_size(800, 600);
        let layer = WindowsPlatformLayer::new("App", &config);
        assert_eq!(layer.window_size(), (800, 600));
    }
}

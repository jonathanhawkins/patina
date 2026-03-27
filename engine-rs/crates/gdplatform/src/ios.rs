//! iOS platform layer with UIKit bridge stubs.
//!
//! Provides [`IosPlatformLayer`] which mirrors the iOS-specific parts of
//! Godot's `DisplayServer` and `OS` singletons:
//!
//! - Safe Area insets (notch / home indicator)
//! - Device model and iOS version queries
//! - Screen properties (scale, native resolution, brightness)
//! - Haptic feedback (UIKit feedback generator stubs)
//! - Application lifecycle events (foreground, background, memory warning)
//! - Status bar and home indicator control
//!
//! On non-iOS platforms, the layer works in headless mode for testing.

use std::collections::VecDeque;

use crate::backend::{HeadlessPlatform, PlatformBackend};
use crate::window::{WindowConfig, WindowEvent};

// ---------------------------------------------------------------------------
// SafeAreaInsets
// ---------------------------------------------------------------------------

/// Safe area insets reported by UIKit, accounting for notch, home indicator,
/// and status bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SafeAreaInsets {
    /// Top inset (status bar / notch).
    pub top: f32,
    /// Bottom inset (home indicator).
    pub bottom: f32,
    /// Left inset (landscape notch).
    pub left: f32,
    /// Right inset (landscape notch).
    pub right: f32,
}

impl Default for SafeAreaInsets {
    fn default() -> Self {
        // Default iPhone 15 / 16 safe area approximation.
        Self {
            top: 59.0,
            bottom: 34.0,
            left: 0.0,
            right: 0.0,
        }
    }
}

impl SafeAreaInsets {
    /// Returns insets with all values set to zero (no notch / no home bar).
    pub fn zero() -> Self {
        Self {
            top: 0.0,
            bottom: 0.0,
            left: 0.0,
            right: 0.0,
        }
    }

    /// Returns the total horizontal inset (left + right).
    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    /// Returns the total vertical inset (top + bottom).
    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

// ---------------------------------------------------------------------------
// IosDeviceModel
// ---------------------------------------------------------------------------

/// Known iOS device models for runtime feature detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IosDeviceModel {
    /// iPhone (any model).
    IPhone,
    /// iPad (any model).
    IPad,
    /// iPod Touch.
    IPodTouch,
    /// Apple TV (tvOS shares some UIKit concepts).
    AppleTV,
    /// Simulator running on macOS.
    Simulator,
    /// Unknown device.
    Unknown,
}

impl Default for IosDeviceModel {
    fn default() -> Self {
        if cfg!(target_os = "ios") {
            Self::IPhone
        } else {
            Self::Simulator
        }
    }
}

// ---------------------------------------------------------------------------
// HapticFeedbackType
// ---------------------------------------------------------------------------

/// UIKit haptic feedback types (UIImpactFeedbackGenerator styles).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HapticFeedbackType {
    /// Light impact.
    Light,
    /// Medium impact.
    Medium,
    /// Heavy impact.
    Heavy,
    /// Selection changed.
    Selection,
    /// Success notification.
    Success,
    /// Warning notification.
    Warning,
    /// Error notification.
    Error,
}

// ---------------------------------------------------------------------------
// IosLifecycleEvent
// ---------------------------------------------------------------------------

/// iOS application lifecycle events forwarded from the UIApplicationDelegate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IosLifecycleEvent {
    /// App is entering the foreground (UIApplicationWillEnterForeground).
    WillEnterForeground,
    /// App became active (UIApplicationDidBecomeActive).
    DidBecomeActive,
    /// App is about to resign active (UIApplicationWillResignActive).
    WillResignActive,
    /// App entered the background (UIApplicationDidEnterBackground).
    DidEnterBackground,
    /// System issued a memory warning (UIApplicationDidReceiveMemoryWarning).
    MemoryWarning,
    /// App is about to terminate.
    WillTerminate,
}

// ---------------------------------------------------------------------------
// StatusBarStyle
// ---------------------------------------------------------------------------

/// iOS status bar appearance style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusBarStyle {
    /// Default style (dark text on light background).
    #[default]
    Default,
    /// Light content (white text, for dark backgrounds).
    LightContent,
    /// Dark content (dark text, explicit, iOS 13+).
    DarkContent,
}

// ---------------------------------------------------------------------------
// IosDisplayInfo
// ---------------------------------------------------------------------------

/// iOS-specific display information.
#[derive(Debug, Clone, PartialEq)]
pub struct IosDisplayInfo {
    /// Native screen scale (e.g. 2.0 for Retina, 3.0 for Super Retina).
    pub screen_scale: f32,
    /// Native screen resolution in pixels (width, height).
    pub native_resolution: (u32, u32),
    /// Current screen brightness (0.0 to 1.0).
    pub brightness: f32,
    /// Safe area insets.
    pub safe_area: SafeAreaInsets,
    /// The device model.
    pub device_model: IosDeviceModel,
    /// iOS version string (e.g. "17.4.1").
    pub ios_version: String,
}

impl Default for IosDisplayInfo {
    fn default() -> Self {
        Self {
            screen_scale: 3.0,
            native_resolution: (1179, 2556), // iPhone 15 Pro
            brightness: 0.5,
            safe_area: SafeAreaInsets::default(),
            device_model: IosDeviceModel::default(),
            ios_version: "17.4".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// IosPlatformLayer
// ---------------------------------------------------------------------------

/// iOS-specific platform layer providing UIKit bridge stubs for safe area,
/// haptics, lifecycle events, screen queries, and status bar control.
///
/// On non-iOS platforms this operates in headless mode for testing.
#[derive(Debug)]
pub struct IosPlatformLayer {
    /// iOS display information.
    pub display_info: IosDisplayInfo,
    /// Pending lifecycle events.
    lifecycle_events: VecDeque<IosLifecycleEvent>,
    /// Haptic feedback log (for testing — records triggered haptics).
    haptic_log: Vec<HapticFeedbackType>,
    /// The underlying platform backend.
    backend: HeadlessPlatform,
    /// Application bundle identifier.
    bundle_id: String,
    /// Whether the status bar is hidden.
    status_bar_hidden: bool,
    /// Status bar style.
    status_bar_style: StatusBarStyle,
    /// Whether the home indicator should auto-hide.
    home_indicator_auto_hidden: bool,
    /// Whether the app is in the foreground.
    is_foreground: bool,
    /// Whether low-power mode is active.
    low_power_mode: bool,
}

impl IosPlatformLayer {
    /// Creates a new iOS platform layer with the given bundle ID and window config.
    pub fn new(bundle_id: impl Into<String>, config: &WindowConfig) -> Self {
        Self {
            display_info: IosDisplayInfo::default(),
            lifecycle_events: VecDeque::new(),
            haptic_log: Vec::new(),
            backend: HeadlessPlatform::from_config(config),
            bundle_id: bundle_id.into(),
            status_bar_hidden: false,
            status_bar_style: StatusBarStyle::Default,
            home_indicator_auto_hidden: false,
            is_foreground: true,
            low_power_mode: false,
        }
    }

    /// Creates a headless iOS platform layer for testing.
    pub fn headless(bundle_id: impl Into<String>) -> Self {
        let config = WindowConfig::default();
        let mut layer = Self::new(bundle_id, &config);
        layer.display_info.device_model = IosDeviceModel::Simulator;
        layer
    }

    /// Returns the bundle identifier.
    pub fn bundle_id(&self) -> &str {
        &self.bundle_id
    }

    // -- Safe Area -----------------------------------------------------------

    /// Returns the current safe area insets.
    pub fn safe_area_insets(&self) -> SafeAreaInsets {
        self.display_info.safe_area
    }

    /// Updates safe area insets (called when device rotates or layout changes).
    pub fn set_safe_area_insets(&mut self, insets: SafeAreaInsets) {
        self.display_info.safe_area = insets;
    }

    // -- Screen queries ------------------------------------------------------

    /// Returns the native screen scale factor.
    pub fn screen_scale(&self) -> f32 {
        self.display_info.screen_scale
    }

    /// Returns the native screen resolution in pixels.
    pub fn native_resolution(&self) -> (u32, u32) {
        self.display_info.native_resolution
    }

    /// Returns the current screen brightness (0.0–1.0).
    pub fn brightness(&self) -> f32 {
        self.display_info.brightness
    }

    /// Sets the screen brightness (stub — on real iOS calls UIScreen).
    pub fn set_brightness(&mut self, brightness: f32) {
        self.display_info.brightness = brightness.clamp(0.0, 1.0);
    }

    // -- Device info ---------------------------------------------------------

    /// Returns the device model.
    pub fn device_model(&self) -> IosDeviceModel {
        self.display_info.device_model
    }

    /// Returns the iOS version string.
    pub fn ios_version(&self) -> &str {
        &self.display_info.ios_version
    }

    /// Returns whether the device is an iPad.
    pub fn is_ipad(&self) -> bool {
        self.display_info.device_model == IosDeviceModel::IPad
    }

    /// Returns whether running in the simulator.
    pub fn is_simulator(&self) -> bool {
        self.display_info.device_model == IosDeviceModel::Simulator
    }

    // -- Haptic feedback (UIKit stubs) ---------------------------------------

    /// Triggers haptic feedback (stub — on real iOS calls UIFeedbackGenerator).
    pub fn trigger_haptic(&mut self, feedback: HapticFeedbackType) {
        self.haptic_log.push(feedback);
    }

    /// Returns the haptic feedback log (for testing).
    pub fn haptic_log(&self) -> &[HapticFeedbackType] {
        &self.haptic_log
    }

    /// Clears the haptic log.
    pub fn clear_haptic_log(&mut self) {
        self.haptic_log.clear();
    }

    // -- Lifecycle events ----------------------------------------------------

    /// Pushes a lifecycle event (called from the UIApplicationDelegate bridge).
    pub fn push_lifecycle_event(&mut self, event: IosLifecycleEvent) {
        match event {
            IosLifecycleEvent::DidBecomeActive | IosLifecycleEvent::WillEnterForeground => {
                self.is_foreground = true;
            }
            IosLifecycleEvent::DidEnterBackground | IosLifecycleEvent::WillResignActive => {
                self.is_foreground = false;
            }
            _ => {}
        }
        self.lifecycle_events.push_back(event);
    }

    /// Drains all pending lifecycle events.
    pub fn poll_lifecycle_events(&mut self) -> Vec<IosLifecycleEvent> {
        self.lifecycle_events.drain(..).collect()
    }

    /// Returns the next pending lifecycle event without removing it.
    pub fn peek_lifecycle_event(&self) -> Option<&IosLifecycleEvent> {
        self.lifecycle_events.front()
    }

    /// Returns the number of pending lifecycle events.
    pub fn pending_lifecycle_event_count(&self) -> usize {
        self.lifecycle_events.len()
    }

    /// Returns whether the app is currently in the foreground.
    pub fn is_foreground(&self) -> bool {
        self.is_foreground
    }

    // -- Status bar / home indicator -----------------------------------------

    /// Hides or shows the status bar.
    pub fn set_status_bar_hidden(&mut self, hidden: bool) {
        self.status_bar_hidden = hidden;
    }

    /// Returns whether the status bar is hidden.
    pub fn is_status_bar_hidden(&self) -> bool {
        self.status_bar_hidden
    }

    /// Sets the status bar style.
    pub fn set_status_bar_style(&mut self, style: StatusBarStyle) {
        self.status_bar_style = style;
    }

    /// Returns the current status bar style.
    pub fn status_bar_style(&self) -> StatusBarStyle {
        self.status_bar_style
    }

    /// Sets whether the home indicator should auto-hide.
    pub fn set_home_indicator_auto_hidden(&mut self, auto_hidden: bool) {
        self.home_indicator_auto_hidden = auto_hidden;
    }

    /// Returns whether the home indicator auto-hides.
    pub fn is_home_indicator_auto_hidden(&self) -> bool {
        self.home_indicator_auto_hidden
    }

    // -- Low power mode ------------------------------------------------------

    /// Sets low-power mode flag (updated from system notifications).
    pub fn set_low_power_mode(&mut self, enabled: bool) {
        self.low_power_mode = enabled;
    }

    /// Returns whether low-power mode is active.
    pub fn is_low_power_mode(&self) -> bool {
        self.low_power_mode
    }

    // -- Backend delegation --------------------------------------------------

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

    #[test]
    fn headless_layer_creation() {
        let layer = IosPlatformLayer::headless("com.patina.test");
        assert_eq!(layer.bundle_id(), "com.patina.test");
        assert_eq!(layer.device_model(), IosDeviceModel::Simulator);
        assert!(layer.is_simulator());
        assert!(layer.is_foreground());
    }

    #[test]
    fn default_display_info() {
        let layer = IosPlatformLayer::headless("com.test");
        assert!((layer.screen_scale() - 3.0).abs() < f32::EPSILON);
        assert_eq!(layer.native_resolution(), (1179, 2556));
        assert!((layer.brightness() - 0.5).abs() < f32::EPSILON);
        assert_eq!(layer.ios_version(), "17.4");
    }

    #[test]
    fn safe_area_insets_default() {
        let layer = IosPlatformLayer::headless("com.test");
        let insets = layer.safe_area_insets();
        assert!((insets.top - 59.0).abs() < f32::EPSILON);
        assert!((insets.bottom - 34.0).abs() < f32::EPSILON);
        assert!((insets.left).abs() < f32::EPSILON);
        assert!((insets.right).abs() < f32::EPSILON);
    }

    #[test]
    fn safe_area_insets_zero() {
        let insets = SafeAreaInsets::zero();
        assert!((insets.horizontal()).abs() < f32::EPSILON);
        assert!((insets.vertical()).abs() < f32::EPSILON);
    }

    #[test]
    fn safe_area_insets_horizontal_vertical() {
        let insets = SafeAreaInsets {
            top: 10.0,
            bottom: 20.0,
            left: 5.0,
            right: 15.0,
        };
        assert!((insets.horizontal() - 20.0).abs() < f32::EPSILON);
        assert!((insets.vertical() - 30.0).abs() < f32::EPSILON);
    }

    #[test]
    fn set_safe_area_insets() {
        let mut layer = IosPlatformLayer::headless("com.test");
        let new_insets = SafeAreaInsets {
            top: 44.0,
            bottom: 0.0,
            left: 0.0,
            right: 0.0,
        };
        layer.set_safe_area_insets(new_insets);
        assert!((layer.safe_area_insets().top - 44.0).abs() < f32::EPSILON);
        assert!((layer.safe_area_insets().bottom).abs() < f32::EPSILON);
    }

    #[test]
    fn set_brightness_clamps() {
        let mut layer = IosPlatformLayer::headless("com.test");
        layer.set_brightness(1.5);
        assert!((layer.brightness() - 1.0).abs() < f32::EPSILON);
        layer.set_brightness(-0.5);
        assert!((layer.brightness()).abs() < f32::EPSILON);
        layer.set_brightness(0.75);
        assert!((layer.brightness() - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn device_model_queries() {
        let mut layer = IosPlatformLayer::headless("com.test");
        assert!(!layer.is_ipad());
        layer.display_info.device_model = IosDeviceModel::IPad;
        assert!(layer.is_ipad());
        assert!(!layer.is_simulator());
    }

    #[test]
    fn haptic_feedback_logging() {
        let mut layer = IosPlatformLayer::headless("com.test");
        assert!(layer.haptic_log().is_empty());
        layer.trigger_haptic(HapticFeedbackType::Light);
        layer.trigger_haptic(HapticFeedbackType::Success);
        assert_eq!(layer.haptic_log().len(), 2);
        assert_eq!(layer.haptic_log()[0], HapticFeedbackType::Light);
        assert_eq!(layer.haptic_log()[1], HapticFeedbackType::Success);
        layer.clear_haptic_log();
        assert!(layer.haptic_log().is_empty());
    }

    #[test]
    fn haptic_all_types() {
        let mut layer = IosPlatformLayer::headless("com.test");
        let types = [
            HapticFeedbackType::Light,
            HapticFeedbackType::Medium,
            HapticFeedbackType::Heavy,
            HapticFeedbackType::Selection,
            HapticFeedbackType::Success,
            HapticFeedbackType::Warning,
            HapticFeedbackType::Error,
        ];
        for t in &types {
            layer.trigger_haptic(*t);
        }
        assert_eq!(layer.haptic_log().len(), 7);
    }

    #[test]
    fn lifecycle_events_push_and_poll() {
        let mut layer = IosPlatformLayer::headless("com.test");
        assert_eq!(layer.pending_lifecycle_event_count(), 0);

        layer.push_lifecycle_event(IosLifecycleEvent::WillResignActive);
        layer.push_lifecycle_event(IosLifecycleEvent::DidEnterBackground);
        assert_eq!(layer.pending_lifecycle_event_count(), 2);

        let events = layer.poll_lifecycle_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], IosLifecycleEvent::WillResignActive);
        assert_eq!(events[1], IosLifecycleEvent::DidEnterBackground);
        assert_eq!(layer.pending_lifecycle_event_count(), 0);
    }

    #[test]
    fn lifecycle_foreground_tracking() {
        let mut layer = IosPlatformLayer::headless("com.test");
        assert!(layer.is_foreground());

        layer.push_lifecycle_event(IosLifecycleEvent::WillResignActive);
        assert!(!layer.is_foreground());

        layer.push_lifecycle_event(IosLifecycleEvent::DidBecomeActive);
        assert!(layer.is_foreground());

        layer.push_lifecycle_event(IosLifecycleEvent::DidEnterBackground);
        assert!(!layer.is_foreground());

        layer.push_lifecycle_event(IosLifecycleEvent::WillEnterForeground);
        assert!(layer.is_foreground());
    }

    #[test]
    fn peek_lifecycle_event_does_not_consume() {
        let mut layer = IosPlatformLayer::headless("com.test");
        layer.push_lifecycle_event(IosLifecycleEvent::MemoryWarning);
        assert!(layer.peek_lifecycle_event().is_some());
        assert_eq!(layer.pending_lifecycle_event_count(), 1);
    }

    #[test]
    fn memory_warning_does_not_change_foreground() {
        let mut layer = IosPlatformLayer::headless("com.test");
        assert!(layer.is_foreground());
        layer.push_lifecycle_event(IosLifecycleEvent::MemoryWarning);
        assert!(layer.is_foreground()); // Memory warning shouldn't change state.
    }

    #[test]
    fn status_bar_control() {
        let mut layer = IosPlatformLayer::headless("com.test");
        assert!(!layer.is_status_bar_hidden());
        assert_eq!(layer.status_bar_style(), StatusBarStyle::Default);

        layer.set_status_bar_hidden(true);
        assert!(layer.is_status_bar_hidden());

        layer.set_status_bar_style(StatusBarStyle::LightContent);
        assert_eq!(layer.status_bar_style(), StatusBarStyle::LightContent);

        layer.set_status_bar_style(StatusBarStyle::DarkContent);
        assert_eq!(layer.status_bar_style(), StatusBarStyle::DarkContent);
    }

    #[test]
    fn home_indicator_auto_hide() {
        let mut layer = IosPlatformLayer::headless("com.test");
        assert!(!layer.is_home_indicator_auto_hidden());
        layer.set_home_indicator_auto_hidden(true);
        assert!(layer.is_home_indicator_auto_hidden());
    }

    #[test]
    fn low_power_mode() {
        let mut layer = IosPlatformLayer::headless("com.test");
        assert!(!layer.is_low_power_mode());
        layer.set_low_power_mode(true);
        assert!(layer.is_low_power_mode());
    }

    #[test]
    fn backend_delegation_window_size() {
        let config = WindowConfig::new().with_size(1170, 2532);
        let layer = IosPlatformLayer::new("com.test", &config);
        assert_eq!(layer.window_size(), (1170, 2532));
    }

    #[test]
    fn backend_delegation_should_quit() {
        let mut layer = IosPlatformLayer::headless("com.test");
        assert!(!layer.should_quit());
        layer.backend_mut().request_quit();
        assert!(layer.should_quit());
    }

    #[test]
    fn backend_delegation_poll_events() {
        let mut layer = IosPlatformLayer::headless("com.test");
        layer.backend_mut().push_event(WindowEvent::FocusGained);
        let events = layer.poll_window_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], WindowEvent::FocusGained);
    }

    #[test]
    fn backend_delegation_end_frame() {
        let mut layer = IosPlatformLayer::headless("com.test");
        assert_eq!(layer.backend().frames_run(), 0);
        layer.end_frame();
        assert_eq!(layer.backend().frames_run(), 1);
    }

    #[test]
    fn full_ios_workflow() {
        let mut layer = IosPlatformLayer::headless("com.patina.game");

        // 1. Check display info.
        assert!((layer.screen_scale() - 3.0).abs() < f32::EPSILON);
        assert!(layer.is_simulator());

        // 2. Set up game UI — hide status bar, auto-hide home indicator.
        layer.set_status_bar_hidden(true);
        layer.set_home_indicator_auto_hidden(true);
        assert!(layer.is_status_bar_hidden());
        assert!(layer.is_home_indicator_auto_hidden());

        // 3. Simulate app going to background and back.
        layer.push_lifecycle_event(IosLifecycleEvent::WillResignActive);
        layer.push_lifecycle_event(IosLifecycleEvent::DidEnterBackground);
        assert!(!layer.is_foreground());

        layer.push_lifecycle_event(IosLifecycleEvent::WillEnterForeground);
        layer.push_lifecycle_event(IosLifecycleEvent::DidBecomeActive);
        assert!(layer.is_foreground());

        let events = layer.poll_lifecycle_events();
        assert_eq!(events.len(), 4);

        // 4. Trigger some haptics.
        layer.trigger_haptic(HapticFeedbackType::Medium);
        layer.trigger_haptic(HapticFeedbackType::Success);
        assert_eq!(layer.haptic_log().len(), 2);

        // 5. Adjust brightness.
        layer.set_brightness(0.8);
        assert!((layer.brightness() - 0.8).abs() < f32::EPSILON);

        // 6. Update safe area for landscape.
        layer.set_safe_area_insets(SafeAreaInsets {
            top: 0.0,
            bottom: 21.0,
            left: 59.0,
            right: 59.0,
        });
        assert!((layer.safe_area_insets().left - 59.0).abs() < f32::EPSILON);

        // 7. Frame loop.
        layer.end_frame();
        assert_eq!(layer.backend().frames_run(), 1);
    }

    #[test]
    fn device_model_default_non_ios() {
        // On non-iOS (test host), default should be Simulator.
        let model = IosDeviceModel::default();
        assert_eq!(model, IosDeviceModel::Simulator);
    }

    #[test]
    fn will_terminate_does_not_change_foreground() {
        let mut layer = IosPlatformLayer::headless("com.test");
        assert!(layer.is_foreground());
        layer.push_lifecycle_event(IosLifecycleEvent::WillTerminate);
        assert!(layer.is_foreground()); // WillTerminate doesn't flip foreground.
    }
}

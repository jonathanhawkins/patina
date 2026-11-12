//! Android platform integration with JNI bridge stubs.
//!
//! Provides the Android platform layer including JNI bridge types for
//! interfacing with the Android runtime, activity lifecycle management,
//! display cutout/inset handling, and permission management.
//!
//! All JNI calls are stubbed — real implementations will use `jni` crate
//! bindings when targeting Android. This module is always compiled so
//! cross-platform code can reference types without `#[cfg]` guards.

use std::collections::HashMap;
use std::fmt;

// ---------------------------------------------------------------------------
// JNI bridge types
// ---------------------------------------------------------------------------

/// Opaque handle to a Java object reference through JNI.
///
/// In a real Android build this wraps a `jni::objects::GlobalRef`.
/// In headless/non-Android builds it's a tagged integer for testing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JniObjectRef {
    /// Unique identifier for this reference (test/headless mode).
    pub id: u64,
    /// Java class name (e.g. "android/app/Activity").
    pub class_name: String,
}

impl JniObjectRef {
    pub fn new(id: u64, class_name: impl Into<String>) -> Self {
        Self {
            id,
            class_name: class_name.into(),
        }
    }

    /// Returns the Java class name this reference points to.
    pub fn class_name(&self) -> &str {
        &self.class_name
    }
}

impl fmt::Display for JniObjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JniRef({}, {})", self.id, self.class_name)
    }
}

/// Result of a JNI method call.
#[derive(Debug, Clone, PartialEq)]
pub enum JniValue {
    Void,
    Bool(bool),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    Object(JniObjectRef),
}

impl fmt::Display for JniValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Void => write!(f, "void"),
            Self::Bool(v) => write!(f, "{}", v),
            Self::Int(v) => write!(f, "{}", v),
            Self::Long(v) => write!(f, "{}", v),
            Self::Float(v) => write!(f, "{:.2}", v),
            Self::Double(v) => write!(f, "{:.4}", v),
            Self::String(v) => write!(f, "\"{}\"", v),
            Self::Object(r) => write!(f, "{}", r),
        }
    }
}

/// Error from a JNI bridge call.
#[derive(Debug, Clone, PartialEq)]
pub enum JniError {
    /// JNI environment not available (not running on Android).
    NotAvailable,
    /// Method not found on the Java class.
    MethodNotFound { class: String, method: String },
    /// Exception thrown by Java code.
    JavaException(String),
    /// Type mismatch in return value.
    TypeMismatch { expected: String, got: String },
}

impl fmt::Display for JniError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAvailable => write!(f, "JNI environment not available"),
            Self::MethodNotFound { class, method } => {
                write!(f, "method {}.{} not found", class, method)
            }
            Self::JavaException(msg) => write!(f, "Java exception: {}", msg),
            Self::TypeMismatch { expected, got } => {
                write!(f, "JNI type mismatch: expected {}, got {}", expected, got)
            }
        }
    }
}

/// Stub JNI bridge for calling Java methods from Rust.
///
/// On a real Android build this wraps `jni::JNIEnv`. In headless mode
/// it returns stubbed values and records calls for testing.
#[derive(Debug, Clone, Default)]
pub struct JniBridge {
    /// Whether the bridge is connected (simulates JNI attach).
    attached: bool,
    /// Recorded method calls for testing (class, method, args).
    call_log: Vec<(String, String, Vec<String>)>,
    /// Stubbed return values keyed by "class.method".
    stubs: HashMap<String, JniValue>,
}

impl JniBridge {
    pub fn new() -> Self {
        Self::default()
    }

    /// Simulate attaching to the JNI environment.
    pub fn attach(&mut self) {
        self.attached = true;
    }

    /// Simulate detaching from the JNI environment.
    pub fn detach(&mut self) {
        self.attached = false;
    }

    /// Returns true if the bridge is attached (JNI env available).
    pub fn is_attached(&self) -> bool {
        self.attached
    }

    /// Register a stubbed return value for a class.method pair.
    pub fn stub_method(&mut self, class: &str, method: &str, value: JniValue) {
        self.stubs.insert(format!("{}.{}", class, method), value);
    }

    /// Call a Java static method. Returns stubbed value or error.
    pub fn call_static(
        &mut self,
        class: &str,
        method: &str,
        args: &[&str],
    ) -> Result<JniValue, JniError> {
        if !self.attached {
            return Err(JniError::NotAvailable);
        }
        let key = format!("{}.{}", class, method);
        self.call_log.push((
            class.to_string(),
            method.to_string(),
            args.iter().map(|s| s.to_string()).collect(),
        ));
        self.stubs
            .get(&key)
            .cloned()
            .ok_or_else(|| JniError::MethodNotFound {
                class: class.to_string(),
                method: method.to_string(),
            })
    }

    /// Call a Java instance method on a JniObjectRef.
    pub fn call_method(
        &mut self,
        _obj: &JniObjectRef,
        class: &str,
        method: &str,
        args: &[&str],
    ) -> Result<JniValue, JniError> {
        self.call_static(class, method, args)
    }

    /// Returns the call log for test assertions.
    pub fn call_log(&self) -> &[(String, String, Vec<String>)] {
        &self.call_log
    }

    /// Clears the call log.
    pub fn clear_log(&mut self) {
        self.call_log.clear();
    }
}

// ---------------------------------------------------------------------------
// Android activity lifecycle
// ---------------------------------------------------------------------------

/// Android activity lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActivityState {
    Created,
    Started,
    Resumed,
    Paused,
    Stopped,
    Destroyed,
}

impl fmt::Display for ActivityState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Created => write!(f, "Created"),
            Self::Started => write!(f, "Started"),
            Self::Resumed => write!(f, "Resumed"),
            Self::Paused => write!(f, "Paused"),
            Self::Stopped => write!(f, "Stopped"),
            Self::Destroyed => write!(f, "Destroyed"),
        }
    }
}

// ---------------------------------------------------------------------------
// Display cutout / insets
// ---------------------------------------------------------------------------

/// Safe area insets for display cutouts (notch, punch-hole, rounded corners).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DisplayInsets {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

impl DisplayInsets {
    pub fn new(top: f32, bottom: f32, left: f32, right: f32) -> Self {
        Self {
            top,
            bottom,
            left,
            right,
        }
    }

    /// Returns true if any inset is non-zero.
    pub fn has_cutout(&self) -> bool {
        self.top > 0.0 || self.bottom > 0.0 || self.left > 0.0 || self.right > 0.0
    }
}

// ---------------------------------------------------------------------------
// Permissions
// ---------------------------------------------------------------------------

/// Android runtime permission state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    /// Permission has not been requested yet.
    NotRequested,
    /// Permission was granted.
    Granted,
    /// Permission was denied.
    Denied,
    /// Permission was denied with "Don't ask again".
    PermanentlyDenied,
}

/// Common Android permissions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AndroidPermission {
    Camera,
    Microphone,
    Storage,
    Location,
    Internet,
    Vibrate,
    /// Custom permission string (e.g. "android.permission.BLUETOOTH").
    Custom(String),
}

impl fmt::Display for AndroidPermission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Camera => write!(f, "android.permission.CAMERA"),
            Self::Microphone => write!(f, "android.permission.RECORD_AUDIO"),
            Self::Storage => write!(f, "android.permission.READ_EXTERNAL_STORAGE"),
            Self::Location => write!(f, "android.permission.ACCESS_FINE_LOCATION"),
            Self::Internet => write!(f, "android.permission.INTERNET"),
            Self::Vibrate => write!(f, "android.permission.VIBRATE"),
            Self::Custom(p) => write!(f, "{}", p),
        }
    }
}

// ---------------------------------------------------------------------------
// Android platform layer
// ---------------------------------------------------------------------------

/// Android platform layer providing access to Android-specific APIs.
///
/// In headless/non-Android builds this is a stub with a `JniBridge`
/// for testing JNI call patterns.
#[derive(Debug, Clone)]
pub struct AndroidPlatformLayer {
    /// JNI bridge for Java interop.
    pub jni: JniBridge,
    /// Current activity lifecycle state.
    pub activity_state: ActivityState,
    /// Display safe area insets.
    pub display_insets: DisplayInsets,
    /// Runtime permission states.
    permissions: HashMap<AndroidPermission, PermissionState>,
    /// Android API level (e.g. 33 for Android 13).
    pub api_level: u32,
    /// Package name (e.g. "com.example.game").
    pub package_name: String,
}

impl Default for AndroidPlatformLayer {
    fn default() -> Self {
        Self {
            jni: JniBridge::new(),
            activity_state: ActivityState::Created,
            display_insets: DisplayInsets::default(),
            permissions: HashMap::new(),
            api_level: 33,
            package_name: "com.patina.engine".to_string(),
        }
    }
}

impl AndroidPlatformLayer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Simulate an activity lifecycle transition.
    pub fn set_activity_state(&mut self, state: ActivityState) {
        self.activity_state = state;
    }

    /// Returns true if the activity is in a visible state (Started or Resumed).
    pub fn is_visible(&self) -> bool {
        matches!(
            self.activity_state,
            ActivityState::Started | ActivityState::Resumed
        )
    }

    /// Returns true if the activity is in the foreground (Resumed).
    pub fn is_foreground(&self) -> bool {
        self.activity_state == ActivityState::Resumed
    }

    /// Request a runtime permission. In headless mode, auto-grants.
    pub fn request_permission(&mut self, permission: AndroidPermission) -> PermissionState {
        let state = PermissionState::Granted;
        self.permissions.insert(permission, state);
        state
    }

    /// Check the current state of a permission.
    pub fn check_permission(&self, permission: &AndroidPermission) -> PermissionState {
        self.permissions
            .get(permission)
            .copied()
            .unwrap_or(PermissionState::NotRequested)
    }

    /// Set permission state (for testing denied/permanently denied flows).
    pub fn set_permission_state(&mut self, permission: AndroidPermission, state: PermissionState) {
        self.permissions.insert(permission, state);
    }

    /// Update display insets (called when the window configuration changes).
    pub fn set_display_insets(&mut self, insets: DisplayInsets) {
        self.display_insets = insets;
    }

    /// Returns true if we're running on Android API level >= the given level.
    pub fn is_api_level_at_least(&self, level: u32) -> bool {
        self.api_level >= level
    }

    /// Get the vibrator service via JNI (stub).
    pub fn vibrate(&mut self, duration_ms: i64) -> Result<(), JniError> {
        self.jni.call_static(
            "android/os/Vibrator",
            "vibrate",
            &[&duration_ms.to_string()],
        )?;
        Ok(())
    }

    /// Get a system service by name via JNI (stub).
    pub fn get_system_service(&mut self, name: &str) -> Result<JniObjectRef, JniError> {
        let result = self
            .jni
            .call_static("android/app/Activity", "getSystemService", &[name])?;
        match result {
            JniValue::Object(obj) => Ok(obj),
            other => Err(JniError::TypeMismatch {
                expected: "Object".to_string(),
                got: format!("{}", other),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jni_bridge_attach_detach() {
        let mut bridge = JniBridge::new();
        assert!(!bridge.is_attached());
        bridge.attach();
        assert!(bridge.is_attached());
        bridge.detach();
        assert!(!bridge.is_attached());
    }

    #[test]
    fn jni_bridge_not_attached_returns_error() {
        let mut bridge = JniBridge::new();
        let result = bridge.call_static("Foo", "bar", &[]);
        assert_eq!(result, Err(JniError::NotAvailable));
    }

    #[test]
    fn jni_bridge_stub_and_call() {
        let mut bridge = JniBridge::new();
        bridge.attach();
        bridge.stub_method("android/os/Build", "getVersion", JniValue::Int(33));
        let result = bridge.call_static("android/os/Build", "getVersion", &[]);
        assert_eq!(result, Ok(JniValue::Int(33)));
    }

    #[test]
    fn jni_bridge_method_not_found() {
        let mut bridge = JniBridge::new();
        bridge.attach();
        let result = bridge.call_static("Foo", "missing", &[]);
        assert!(matches!(result, Err(JniError::MethodNotFound { .. })));
    }

    #[test]
    fn jni_bridge_call_log() {
        let mut bridge = JniBridge::new();
        bridge.attach();
        bridge.stub_method("Foo", "bar", JniValue::Void);
        let _ = bridge.call_static("Foo", "bar", &["arg1", "arg2"]);
        assert_eq!(bridge.call_log().len(), 1);
        assert_eq!(bridge.call_log()[0].0, "Foo");
        assert_eq!(bridge.call_log()[0].1, "bar");
        assert_eq!(bridge.call_log()[0].2, vec!["arg1", "arg2"]);
    }

    #[test]
    fn jni_bridge_clear_log() {
        let mut bridge = JniBridge::new();
        bridge.attach();
        bridge.stub_method("A", "b", JniValue::Void);
        let _ = bridge.call_static("A", "b", &[]);
        bridge.clear_log();
        assert!(bridge.call_log().is_empty());
    }

    #[test]
    fn jni_value_display() {
        assert_eq!(JniValue::Void.to_string(), "void");
        assert_eq!(JniValue::Bool(true).to_string(), "true");
        assert_eq!(JniValue::Int(42).to_string(), "42");
        assert_eq!(JniValue::String("hello".into()).to_string(), "\"hello\"");
    }

    #[test]
    fn jni_object_ref_display() {
        let r = JniObjectRef::new(1, "android/app/Activity");
        assert_eq!(r.to_string(), "JniRef(1, android/app/Activity)");
        assert_eq!(r.class_name(), "android/app/Activity");
    }

    #[test]
    fn jni_error_display() {
        assert_eq!(
            JniError::NotAvailable.to_string(),
            "JNI environment not available"
        );
        let err = JniError::MethodNotFound {
            class: "Foo".into(),
            method: "bar".into(),
        };
        assert_eq!(err.to_string(), "method Foo.bar not found");
    }

    #[test]
    fn activity_lifecycle_states() {
        let mut layer = AndroidPlatformLayer::new();
        assert_eq!(layer.activity_state, ActivityState::Created);
        assert!(!layer.is_visible());
        assert!(!layer.is_foreground());

        layer.set_activity_state(ActivityState::Started);
        assert!(layer.is_visible());
        assert!(!layer.is_foreground());

        layer.set_activity_state(ActivityState::Resumed);
        assert!(layer.is_visible());
        assert!(layer.is_foreground());

        layer.set_activity_state(ActivityState::Paused);
        assert!(!layer.is_foreground());

        layer.set_activity_state(ActivityState::Destroyed);
        assert!(!layer.is_visible());
    }

    #[test]
    fn display_insets() {
        let insets = DisplayInsets::new(40.0, 0.0, 0.0, 0.0);
        assert!(insets.has_cutout());

        let none = DisplayInsets::default();
        assert!(!none.has_cutout());
    }

    #[test]
    fn permission_request_and_check() {
        let mut layer = AndroidPlatformLayer::new();
        assert_eq!(
            layer.check_permission(&AndroidPermission::Camera),
            PermissionState::NotRequested
        );

        let state = layer.request_permission(AndroidPermission::Camera);
        assert_eq!(state, PermissionState::Granted);
        assert_eq!(
            layer.check_permission(&AndroidPermission::Camera),
            PermissionState::Granted
        );
    }

    #[test]
    fn permission_denied_flow() {
        let mut layer = AndroidPlatformLayer::new();
        layer.set_permission_state(AndroidPermission::Location, PermissionState::Denied);
        assert_eq!(
            layer.check_permission(&AndroidPermission::Location),
            PermissionState::Denied
        );

        layer.set_permission_state(
            AndroidPermission::Location,
            PermissionState::PermanentlyDenied,
        );
        assert_eq!(
            layer.check_permission(&AndroidPermission::Location),
            PermissionState::PermanentlyDenied
        );
    }

    #[test]
    fn android_permission_display() {
        assert_eq!(
            AndroidPermission::Camera.to_string(),
            "android.permission.CAMERA"
        );
        assert_eq!(
            AndroidPermission::Internet.to_string(),
            "android.permission.INTERNET"
        );
        assert_eq!(
            AndroidPermission::Custom("com.foo.BAR".into()).to_string(),
            "com.foo.BAR"
        );
    }

    #[test]
    fn api_level_check() {
        let layer = AndroidPlatformLayer::new();
        assert!(layer.is_api_level_at_least(33));
        assert!(layer.is_api_level_at_least(21));
        assert!(!layer.is_api_level_at_least(34));
    }

    #[test]
    fn vibrate_stub() {
        let mut layer = AndroidPlatformLayer::new();
        layer.jni.attach();
        layer
            .jni
            .stub_method("android/os/Vibrator", "vibrate", JniValue::Void);
        assert!(layer.vibrate(100).is_ok());
        assert_eq!(layer.jni.call_log().len(), 1);
    }

    #[test]
    fn get_system_service_stub() {
        let mut layer = AndroidPlatformLayer::new();
        layer.jni.attach();
        let sensor_ref = JniObjectRef::new(42, "android/hardware/SensorManager");
        layer.jni.stub_method(
            "android/app/Activity",
            "getSystemService",
            JniValue::Object(sensor_ref.clone()),
        );
        let result = layer.get_system_service("sensor");
        assert_eq!(result, Ok(sensor_ref));
    }

    #[test]
    fn get_system_service_type_mismatch() {
        let mut layer = AndroidPlatformLayer::new();
        layer.jni.attach();
        layer
            .jni
            .stub_method("android/app/Activity", "getSystemService", JniValue::Int(0));
        let result = layer.get_system_service("sensor");
        assert!(matches!(result, Err(JniError::TypeMismatch { .. })));
    }

    #[test]
    fn default_platform_layer() {
        let layer = AndroidPlatformLayer::new();
        assert_eq!(layer.api_level, 33);
        assert_eq!(layer.package_name, "com.patina.engine");
        assert_eq!(layer.activity_state, ActivityState::Created);
        assert!(!layer.display_insets.has_cutout());
    }

    #[test]
    fn activity_state_display() {
        assert_eq!(ActivityState::Created.to_string(), "Created");
        assert_eq!(ActivityState::Resumed.to_string(), "Resumed");
        assert_eq!(ActivityState::Destroyed.to_string(), "Destroyed");
    }

    #[test]
    fn call_method_on_object_ref() {
        let mut bridge = JniBridge::new();
        bridge.attach();
        bridge.stub_method("SensorManager", "getSensorList", JniValue::Int(5));
        let obj = JniObjectRef::new(1, "SensorManager");
        let result = bridge.call_method(&obj, "SensorManager", "getSensorList", &[]);
        assert_eq!(result, Ok(JniValue::Int(5)));
    }
}

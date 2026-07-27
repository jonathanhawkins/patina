//! XRServer / OpenXR mock (pat-p0s82.23).

use gdcore::math::Vector3;

#[derive(Debug, Clone, Copy)]
pub struct XrPose {
    pub position: Vector3,
    pub yaw: f32,
}

#[derive(Debug, Default)]
pub struct XrServer {
    pub session_active: bool,
    pub head: XrPose,
}

impl Default for XrPose {
    fn default() -> Self {
        Self {
            position: Vector3::ZERO,
            yaw: 0.0,
        }
    }
}

impl XrServer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn start_mock_session(&mut self) {
        self.session_active = true;
        self.head = XrPose {
            position: Vector3::new(0.0, 1.6, 0.0),
            yaw: 0.0,
        };
    }
    pub fn get_head_pose(&self) -> Option<XrPose> {
        if self.session_active {
            Some(self.head)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// WebXRInterface session lifecycle (pat-nzgdu.150)
// ---------------------------------------------------------------------------

/// Godot's `WebXRInterface.session_mode` (`navigator.xr` session modes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WebXrSessionMode {
    /// `'inline'` — rendered into the page, no headset. Godot's default.
    #[default]
    Inline,
    /// `'immersive-vr'`.
    ImmersiveVr,
    /// `'immersive-ar'`.
    ImmersiveAr,
}

impl WebXrSessionMode {
    /// Parses the string Godot/WebXR uses on the wire. Unknown values map to
    /// `None` so a script typo surfaces rather than silently becoming inline.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "inline" => Some(WebXrSessionMode::Inline),
            "immersive-vr" => Some(WebXrSessionMode::ImmersiveVr),
            "immersive-ar" => Some(WebXrSessionMode::ImmersiveAr),
            _ => None,
        }
    }

    /// The `navigator.xr` string for this mode.
    pub fn as_str(self) -> &'static str {
        match self {
            WebXrSessionMode::Inline => "inline",
            WebXrSessionMode::ImmersiveVr => "immersive-vr",
            WebXrSessionMode::ImmersiveAr => "immersive-ar",
        }
    }
}

/// Where a WebXR session currently is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WebXrSessionState {
    /// No session requested yet, or the last one ended.
    #[default]
    Idle,
    /// `initialize()` called; `requestSession` is in flight.
    Requesting,
    /// A session is running.
    Active,
}

/// A signal the interface wants emitted, mirroring `WebXRInterface`'s
/// callback-driven API (pat-nzgdu.150).
///
/// WebXR is asynchronous — `is_session_supported()` and `initialize()` both
/// return immediately and answer later — so the interface records what it owes
/// the script here and the binding layer drains it.
#[derive(Debug, Clone, PartialEq)]
pub enum WebXrSignal {
    /// `session_supported(session_mode, supported)`.
    SessionSupported {
        /// The mode that was queried.
        mode: WebXrSessionMode,
        /// Whether the browser reports support.
        supported: bool,
    },
    /// `session_started()`.
    SessionStarted,
    /// `session_failed(message)`.
    SessionFailed {
        /// Why the session request was rejected.
        message: String,
    },
    /// `session_ended()`.
    SessionEnded,
    /// `visibility_state_changed()`.
    VisibilityStateChanged,
    /// `reference_space_reset()`.
    ReferenceSpaceReset,
    /// `selectstart(input_source_id)`.
    SelectStart {
        /// Which input source fired.
        input_source_id: usize,
    },
    /// `selectend(input_source_id)`.
    SelectEnd {
        /// Which input source fired.
        input_source_id: usize,
    },
    /// `select(input_source_id)`.
    ///
    /// ⚠️ Godot emits this **after** [`SelectEnd`](Self::SelectEnd), from the
    /// same browser event — see [`WebXrInterface::on_input_event`].
    Select {
        /// Which input source fired.
        input_source_id: usize,
    },
    /// `squeezestart(input_source_id)`.
    SqueezeStart {
        /// Which input source fired.
        input_source_id: usize,
    },
    /// `squeezeend(input_source_id)`.
    SqueezeEnd {
        /// Which input source fired.
        input_source_id: usize,
    },
    /// `squeeze(input_source_id)`, emitted after `squeezeend`.
    Squeeze {
        /// Which input source fired.
        input_source_id: usize,
    },
    /// `display_refresh_rate_changed()`.
    DisplayRefreshRateChanged,
}

/// The four input events `navigator.xr` delivers, matching
/// `godot_webxr.h`'s `WEBXR_INPUT_EVENT_*`.
///
/// ⚠️ There is deliberately **no `Select`/`Squeeze` variant**: the browser only
/// fires start/end, and Godot synthesises the completed-gesture signal itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebXrInputEvent {
    /// `WEBXR_INPUT_EVENT_SELECTSTART`.
    SelectStart,
    /// `WEBXR_INPUT_EVENT_SELECTEND`.
    SelectEnd,
    /// `WEBXR_INPUT_EVENT_SQUEEZESTART`.
    SqueezeStart,
    /// `WEBXR_INPUT_EVENT_SQUEEZEEND`.
    SqueezeEnd,
}

/// Godot `WebXRInterface.TargetRayMode`.
///
/// Values are Godot's (`extension_api.json`, `WebXRInterface.TargetRayMode`), so
/// a script comparing against the constant gets the same answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TargetRayMode {
    /// `TARGET_RAY_MODE_UNKNOWN` — also what an **inactive** source reports.
    #[default]
    Unknown = 0,
    /// `TARGET_RAY_MODE_GAZE`.
    Gaze = 1,
    /// `TARGET_RAY_MODE_TRACKED_POINTER`.
    TrackedPointer = 2,
    /// `TARGET_RAY_MODE_SCREEN`.
    Screen = 3,
}

impl TargetRayMode {
    /// The integer a script sees.
    pub fn as_i64(self) -> i64 {
        self as i64
    }

    /// Map the browser's numeric mode, defaulting to `Unknown`.
    pub fn from_i64(v: i64) -> Self {
        match v {
            1 => Self::Gaze,
            2 => Self::TrackedPointer,
            3 => Self::Screen,
            _ => Self::Unknown,
        }
    }
}

/// One WebXR input source, mirroring `WebXRInterfaceJS::InputSource`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WebXrInputSource {
    /// Whether the browser currently reports this source.
    pub active: bool,
    /// The `XRControllerTracker` name this source registers on `XRServer`.
    pub tracker_name: String,
    /// Godot's `target_ray_mode` for this source.
    pub target_ray_mode: TargetRayMode,
}

/// Godot's fixed tracker names (`webxr_interface_js.h:153`).
///
/// ⚠️ Ids **0 and 1 are always the left and right hands** — Godot's own comment
/// — so the names are positional, not discovered from the browser. Sources 2+
/// get `tracker_N`.
pub fn webxr_tracker_name(input_source_id: usize) -> String {
    match input_source_id {
        0 => "left_hand".to_string(),
        1 => "right_hand".to_string(),
        n => format!("tracker_{n}"),
    }
}

/// What a browser's `navigator.xr` would answer, supplied by the caller.
///
/// On the web this is backed by real `navigator.xr` promises; natively — and in
/// tests — it is set directly, which is exactly the "stubbed navigator.xr" the
/// bead's acceptance describes. Defaults model a **browser with no WebXR at
/// all**, which is also the correct native answer: `is_session_supported`
/// reports unsupported rather than panicking.
#[derive(Debug, Clone, Default)]
pub struct WebXrStub {
    /// Modes the "browser" supports.
    pub supported_modes: Vec<WebXrSessionMode>,
    /// Features the "browser" will grant; a required feature outside this list
    /// makes the session request fail, as in WebXR.
    pub available_features: Vec<String>,
    /// When set, `initialize()` fails with this message regardless of features.
    pub force_failure: Option<String>,
}

/// Godot's `WebXRInterface` — the session lifecycle and its signal queue
/// (pat-nzgdu.150).
///
/// This is the interface's own logic, driven by a [`WebXrStub`] standing in for
/// `navigator.xr`. The browser bindings (wasm32) would drive the same state
/// machine from real promises/events; nothing here is web-specific, so it is
/// testable natively.
///
/// **Not yet wired to script.** `XRServer.find_interface("WebXR")` and emitting
/// these signals to a connected callback are the remaining half — see the bead.
#[derive(Debug, Clone, Default)]
pub struct WebXrInterface {
    /// Requested session mode (Godot `session_mode`).
    pub session_mode: WebXrSessionMode,
    /// Features the session cannot start without (Godot `required_features`).
    pub required_features: Vec<String>,
    /// Features requested but not essential (Godot `optional_features`).
    pub optional_features: Vec<String>,
    /// Reference spaces to try, best first (Godot `requested_reference_space_types`).
    pub requested_reference_space_types: Vec<String>,
    state: WebXrSessionState,
    reference_space_type: String,
    enabled_features: Vec<String>,
    visibility_state: String,
    pending: Vec<WebXrSignal>,
    stub: WebXrStub,
    /// Per-source state, indexed by input source id (pat-0q2kx.264).
    input_sources: Vec<WebXrInputSource>,
    display_refresh_rate: f32,
    available_display_refresh_rates: Vec<f32>,
}

impl WebXrInterface {
    /// A fresh interface with no session and no browser support (the correct
    /// native answer).
    pub fn new() -> Self {
        Self::default()
    }

    /// Installs the stand-in for `navigator.xr`.
    pub fn set_stub(&mut self, stub: WebXrStub) {
        self.stub = stub;
    }

    /// Godot's `WebXRInterface.name`.
    pub fn name(&self) -> &'static str {
        "WebXR"
    }

    /// Current lifecycle state.
    pub fn state(&self) -> WebXrSessionState {
        self.state
    }

    /// Whether a session is running (Godot `XRInterface.is_initialized`).
    pub fn is_initialized(&self) -> bool {
        self.state == WebXrSessionState::Active
    }

    /// Resolved reference space (Godot `reference_space_type`); empty until a
    /// session starts.
    pub fn reference_space_type(&self) -> &str {
        &self.reference_space_type
    }

    /// Features the session actually granted (Godot `enabled_features`).
    pub fn enabled_features(&self) -> &[String] {
        &self.enabled_features
    }

    /// Godot `visibility_state`; empty outside a session.
    pub fn visibility_state(&self) -> &str {
        &self.visibility_state
    }

    /// Drains the queued signals (the seam the script binding emits from).
    ///
    /// Draining is idempotent: a second call returns nothing until more are
    /// queued.
    pub fn take_signals(&mut self) -> Vec<WebXrSignal> {
        std::mem::take(&mut self.pending)
    }

    /// Godot's `is_session_supported(mode)` — **returns nothing**; the answer
    /// arrives later as a `session_supported` signal.
    ///
    /// An unknown mode string answers `supported = false` (rather than
    /// erroring), matching a browser that simply doesn't know the mode.
    pub fn is_session_supported(&mut self, mode: &str) {
        match WebXrSessionMode::from_str(mode) {
            Some(m) => {
                let supported = self.stub.supported_modes.contains(&m);
                self.pending
                    .push(WebXrSignal::SessionSupported { mode: m, supported });
            }
            None => {
                self.pending.push(WebXrSignal::SessionSupported {
                    mode: WebXrSessionMode::Inline,
                    supported: false,
                });
            }
        }
    }

    /// Godot's `XRInterface.initialize()` — requests a session asynchronously.
    ///
    /// Answers with `session_started` or `session_failed(message)`. Returns
    /// `true` when the request was accepted for processing (Godot's return),
    /// **not** that the session started.
    pub fn initialize(&mut self) -> bool {
        if self.state == WebXrSessionState::Active {
            // Already running: Godot treats initialize() on a live interface as
            // a no-op success.
            return true;
        }
        self.state = WebXrSessionState::Requesting;

        if let Some(msg) = self.stub.force_failure.clone() {
            return self.fail(msg);
        }
        if !self.stub.supported_modes.contains(&self.session_mode) {
            let msg = format!(
                "session mode '{}' is not supported",
                self.session_mode.as_str()
            );
            return self.fail(msg);
        }
        // WebXR rejects the request if any required feature is unavailable.
        if let Some(missing) = self
            .required_features
            .iter()
            .find(|f| !self.stub.available_features.contains(f))
        {
            let msg = format!("required feature '{missing}' is not available");
            return self.fail(msg);
        }

        // Optional features are granted only where available — never fatal.
        let mut enabled = self.required_features.clone();
        enabled.extend(
            self.optional_features
                .iter()
                .filter(|f| self.stub.available_features.contains(f))
                .cloned(),
        );
        self.enabled_features = enabled;

        // First requested reference space wins; Godot falls back to "viewer",
        // which every session provides.
        self.reference_space_type = self
            .requested_reference_space_types
            .first()
            .cloned()
            .unwrap_or_else(|| "viewer".to_string());

        self.state = WebXrSessionState::Active;
        self.visibility_state = "visible".to_string();
        self.pending.push(WebXrSignal::SessionStarted);
        true
    }

    /// Records a failed session request and returns `true` (the request itself
    /// was accepted; the failure is reported by signal, as in WebXR).
    fn fail(&mut self, message: String) -> bool {
        self.state = WebXrSessionState::Idle;
        self.pending.push(WebXrSignal::SessionFailed { message });
        true
    }

    /// Godot's `XRInterface.uninitialize()` / the browser's session end.
    /// Emits `session_ended` only if a session was actually running.
    pub fn uninitialize(&mut self) {
        if self.state != WebXrSessionState::Active {
            return;
        }
        self.state = WebXrSessionState::Idle;
        self.reference_space_type.clear();
        self.enabled_features.clear();
        self.visibility_state.clear();
        // ⚠️ Input sources do not survive the session (pat-0q2kx.264). Godot
        // removes their trackers from XRServer on exit; leaving them active
        // here would let `is_input_source_active` report a controller for a
        // session that has ended, and a script polling it would never notice
        // the headset was taken off.
        self.input_sources.clear();
        self.available_display_refresh_rates.clear();
        self.display_refresh_rate = 0.0;
        self.pending.push(WebXrSignal::SessionEnded);
    }

    /// The browser's `visibilitychange` (Godot `visibility_state_changed`).
    /// Ignored outside a session.
    pub fn set_visibility_state(&mut self, state: &str) {
        if self.state != WebXrSessionState::Active || self.visibility_state == state {
            return;
        }
        self.visibility_state = state.to_string();
        self.pending.push(WebXrSignal::VisibilityStateChanged);
    }

    /// The browser's `reset` event on the reference space.
    pub fn notify_reference_space_reset(&mut self) {
        if self.state == WebXrSessionState::Active {
            self.pending.push(WebXrSignal::ReferenceSpaceReset);
        }
    }

    // -- Input sources (pat-0q2kx.264) -------------------------------------

    /// The browser reporting an input source connected or updated.
    ///
    /// Ids are positional: 0 is the left hand, 1 the right
    /// (`webxr_interface_js.cpp:679`, "Input source id's 0 and 1 are always the
    /// left and right hands"), so the tracker name follows from the id rather
    /// than from anything the browser says.
    pub fn set_input_source(&mut self, input_source_id: usize, target_ray_mode: TargetRayMode) {
        if self.input_sources.len() <= input_source_id {
            self.input_sources
                .resize(input_source_id + 1, WebXrInputSource::default());
        }
        self.input_sources[input_source_id] = WebXrInputSource {
            active: true,
            tracker_name: webxr_tracker_name(input_source_id),
            target_ray_mode,
        };
    }

    /// The browser reporting an input source disconnected.
    ///
    /// Marks it inactive rather than removing it, so ids stay stable — Godot
    /// indexes `input_sources` by id, and compacting would rename every source
    /// after the removed one.
    pub fn remove_input_source(&mut self, input_source_id: usize) {
        if let Some(src) = self.input_sources.get_mut(input_source_id) {
            src.active = false;
        }
    }

    /// Godot `is_input_source_active`.
    pub fn is_input_source_active(&self, input_source_id: usize) -> bool {
        self.input_sources
            .get(input_source_id)
            .is_some_and(|s| s.active)
    }

    /// Godot `get_input_source_tracker` — the tracker name this source
    /// registers on `XRServer`, or `None` when the source is not active.
    pub fn input_source_tracker_name(&self, input_source_id: usize) -> Option<&str> {
        self.input_sources
            .get(input_source_id)
            .filter(|s| s.active)
            .map(|s| s.tracker_name.as_str())
    }

    /// Godot `get_input_source_target_ray_mode`.
    ///
    /// ⚠️ An **inactive** source reports `Unknown`, not its last mode
    /// (`webxr_interface_js.cpp:177`). Returning the stale mode would let a
    /// script keep treating a disconnected controller as a tracked pointer.
    pub fn input_source_target_ray_mode(&self, input_source_id: usize) -> TargetRayMode {
        match self.input_sources.get(input_source_id) {
            Some(s) if s.active => s.target_ray_mode,
            _ => TargetRayMode::Unknown,
        }
    }

    /// A `selectstart` / `selectend` / `squeezestart` / `squeezeend` from the
    /// browser, queueing Godot's signals.
    ///
    /// ⚠️ **`select` is emitted after `selectend`, from the same event** —
    /// there is no separate browser `select` for Godot to forward. See
    /// `webxr_interface_js.cpp:843`: "Emit the 'select' event on our own
    /// (rather than intercepting the one from JavaScript)". Squeeze is the
    /// same. Emitting `select` between start and end — the order the name suggests
    /// — would fire it before the gesture completed.
    pub fn on_input_event(&mut self, event: WebXrInputEvent, input_source_id: usize) {
        match event {
            WebXrInputEvent::SelectStart => {
                self.pending
                    .push(WebXrSignal::SelectStart { input_source_id });
            }
            WebXrInputEvent::SelectEnd => {
                self.pending
                    .push(WebXrSignal::SelectEnd { input_source_id });
                self.pending.push(WebXrSignal::Select { input_source_id });
            }
            WebXrInputEvent::SqueezeStart => {
                self.pending
                    .push(WebXrSignal::SqueezeStart { input_source_id });
            }
            WebXrInputEvent::SqueezeEnd => {
                self.pending
                    .push(WebXrSignal::SqueezeEnd { input_source_id });
                self.pending.push(WebXrSignal::Squeeze { input_source_id });
            }
        }
    }

    // -- Display refresh rate (pat-0q2kx.264) ------------------------------

    /// Godot `get_display_refresh_rate`.
    pub fn display_refresh_rate(&self) -> f32 {
        self.display_refresh_rate
    }

    /// Godot `set_display_refresh_rate`.
    ///
    /// Queues `display_refresh_rate_changed` only when the value actually
    /// changes — re-emitting on every set would make a per-frame writer spam a
    /// signal Godot fires on transitions.
    pub fn set_display_refresh_rate(&mut self, rate: f32) {
        if (self.display_refresh_rate - rate).abs() > f32::EPSILON {
            self.display_refresh_rate = rate;
            self.pending.push(WebXrSignal::DisplayRefreshRateChanged);
        }
    }

    /// Godot `get_available_display_refresh_rates`.
    pub fn available_display_refresh_rates(&self) -> &[f32] {
        &self.available_display_refresh_rates
    }

    /// The browser reporting the rates this device offers.
    pub fn set_available_display_refresh_rates(&mut self, rates: Vec<f32>) {
        self.available_display_refresh_rates = rates;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- pat-nzgdu.150: WebXRInterface session lifecycle ---------------------

    /// A "browser" supporting immersive-vr with the given features.
    fn vr_stub(features: &[&str]) -> WebXrStub {
        WebXrStub {
            supported_modes: vec![WebXrSessionMode::ImmersiveVr, WebXrSessionMode::Inline],
            available_features: features.iter().map(|s| s.to_string()).collect(),
            force_failure: None,
        }
    }

    #[test]
    fn session_mode_string_roundtrip() {
        for m in [
            WebXrSessionMode::Inline,
            WebXrSessionMode::ImmersiveVr,
            WebXrSessionMode::ImmersiveAr,
        ] {
            assert_eq!(WebXrSessionMode::from_str(m.as_str()), Some(m));
        }
        // Unknown strings do not silently become inline.
        assert_eq!(WebXrSessionMode::from_str("immersive-holodeck"), None);
        assert_eq!(WebXrSessionMode::default(), WebXrSessionMode::Inline);
    }

    #[test]
    fn is_session_supported_answers_by_signal_not_return_value() {
        // WebXR is callback-based: the answer arrives as a signal.
        let mut xr = WebXrInterface::new();
        xr.set_stub(vr_stub(&[]));

        xr.is_session_supported("immersive-vr");
        assert_eq!(
            xr.take_signals(),
            vec![WebXrSignal::SessionSupported {
                mode: WebXrSessionMode::ImmersiveVr,
                supported: true,
            }],
        );

        xr.is_session_supported("immersive-ar");
        assert_eq!(
            xr.take_signals(),
            vec![WebXrSignal::SessionSupported {
                mode: WebXrSessionMode::ImmersiveAr,
                supported: false,
            }],
        );
        // Draining is idempotent.
        assert!(xr.take_signals().is_empty());
    }

    #[test]
    fn native_default_reports_unsupported_rather_than_panicking() {
        // No stub = no browser WebXR, which is the correct native answer.
        let mut xr = WebXrInterface::new();
        xr.is_session_supported("immersive-vr");
        assert_eq!(
            xr.take_signals(),
            vec![WebXrSignal::SessionSupported {
                mode: WebXrSessionMode::ImmersiveVr,
                supported: false,
            }],
        );
        assert!(!xr.is_initialized());

        // initialize() must also fail cleanly, not panic.
        assert!(xr.initialize());
        assert!(matches!(
            xr.take_signals().as_slice(),
            [WebXrSignal::SessionFailed { .. }],
        ));
        assert_eq!(xr.state(), WebXrSessionState::Idle);
    }

    #[test]
    fn initialize_starts_a_session_and_resolves_features_and_reference_space() {
        let mut xr = WebXrInterface::new();
        xr.set_stub(vr_stub(&["local-floor", "hand-tracking"]));
        xr.session_mode = WebXrSessionMode::ImmersiveVr;
        xr.required_features = vec!["local-floor".to_string()];
        // 'bounded-floor' is NOT available — optional, so it must not be fatal.
        xr.optional_features = vec!["hand-tracking".to_string(), "bounded-floor".to_string()];
        xr.requested_reference_space_types = vec!["local-floor".to_string(), "viewer".to_string()];

        assert!(xr.initialize());
        assert_eq!(xr.take_signals(), vec![WebXrSignal::SessionStarted]);
        assert!(xr.is_initialized());
        assert_eq!(xr.state(), WebXrSessionState::Active);

        // First requested reference space wins.
        assert_eq!(xr.reference_space_type(), "local-floor");
        // Granted = required + available optionals; the unavailable one is dropped.
        assert_eq!(xr.enabled_features(), ["local-floor", "hand-tracking"]);
        assert_eq!(xr.visibility_state(), "visible");
    }

    #[test]
    fn a_missing_required_feature_fails_the_session() {
        let mut xr = WebXrInterface::new();
        xr.set_stub(vr_stub(&["local-floor"]));
        xr.session_mode = WebXrSessionMode::ImmersiveVr;
        xr.required_features = vec!["hand-tracking".to_string()];

        assert!(xr.initialize(), "the request itself is accepted");
        match xr.take_signals().as_slice() {
            [WebXrSignal::SessionFailed { message }] => {
                assert!(message.contains("hand-tracking"), "{message}");
            }
            other => panic!("expected session_failed, got {other:?}"),
        }
        // A failed request leaves no session behind.
        assert!(!xr.is_initialized());
        assert_eq!(xr.state(), WebXrSessionState::Idle);
    }

    #[test]
    fn an_unsupported_mode_fails_the_session() {
        let mut xr = WebXrInterface::new();
        xr.set_stub(vr_stub(&[]));
        xr.session_mode = WebXrSessionMode::ImmersiveAr; // stub has no AR

        assert!(xr.initialize());
        match xr.take_signals().as_slice() {
            [WebXrSignal::SessionFailed { message }] => {
                assert!(message.contains("immersive-ar"), "{message}");
            }
            other => panic!("expected session_failed, got {other:?}"),
        }
    }

    #[test]
    fn reference_space_defaults_to_viewer() {
        let mut xr = WebXrInterface::new();
        xr.set_stub(vr_stub(&[]));
        xr.session_mode = WebXrSessionMode::ImmersiveVr;
        // Nothing requested: every session provides "viewer".
        assert!(xr.initialize());
        assert_eq!(xr.reference_space_type(), "viewer");
    }

    #[test]
    fn session_ends_once_and_clears_its_state() {
        let mut xr = WebXrInterface::new();
        xr.set_stub(vr_stub(&[]));
        xr.session_mode = WebXrSessionMode::ImmersiveVr;
        xr.initialize();
        let _ = xr.take_signals();

        xr.uninitialize();
        assert_eq!(xr.take_signals(), vec![WebXrSignal::SessionEnded]);
        assert!(!xr.is_initialized());
        assert_eq!(xr.reference_space_type(), "");
        assert!(xr.enabled_features().is_empty());

        // Ending an already-ended session emits nothing.
        xr.uninitialize();
        assert!(xr.take_signals().is_empty());
    }

    #[test]
    fn visibility_and_reference_space_reset_only_fire_inside_a_session() {
        let mut xr = WebXrInterface::new();
        xr.set_stub(vr_stub(&[]));

        // Outside a session these are ignored.
        xr.set_visibility_state("hidden");
        xr.notify_reference_space_reset();
        assert!(xr.take_signals().is_empty());

        xr.session_mode = WebXrSessionMode::ImmersiveVr;
        xr.initialize();
        let _ = xr.take_signals();

        xr.set_visibility_state("hidden");
        assert_eq!(xr.take_signals(), vec![WebXrSignal::VisibilityStateChanged]);
        assert_eq!(xr.visibility_state(), "hidden");
        // Same value again is not a change.
        xr.set_visibility_state("hidden");
        assert!(xr.take_signals().is_empty());

        xr.notify_reference_space_reset();
        assert_eq!(xr.take_signals(), vec![WebXrSignal::ReferenceSpaceReset]);
    }

    #[test]
    fn forced_failure_reports_the_browsers_message() {
        let mut xr = WebXrInterface::new();
        let mut stub = vr_stub(&[]);
        stub.force_failure = Some("NotAllowedError: user denied".to_string());
        xr.set_stub(stub);
        xr.session_mode = WebXrSessionMode::ImmersiveVr;

        assert!(xr.initialize());
        match xr.take_signals().as_slice() {
            [WebXrSignal::SessionFailed { message }] => {
                assert!(message.contains("NotAllowedError"), "{message}");
            }
            other => panic!("expected session_failed, got {other:?}"),
        }
    }

    #[test]
    fn interface_name_is_webxr() {
        // XRServer.find_interface("WebXR") will match on this.
        assert_eq!(WebXrInterface::new().name(), "WebXR");
    }

    #[test]
    fn mock_session_pose() {
        let mut s = XrServer::new();
        assert!(s.get_head_pose().is_none());
        s.start_mock_session();
        let p = s.get_head_pose().unwrap();
        assert!((p.position.y - 1.6).abs() < 1e-5);
    }
}

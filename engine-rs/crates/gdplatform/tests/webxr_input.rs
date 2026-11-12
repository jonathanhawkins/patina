//! pat-0q2kx.264 (native half) — WebXR input sources, select/squeeze signal
//! order, and the display-refresh-rate surface.
//!
//! Godot ground truth: `upstream/godot/modules/webxr/webxr_interface_js.cpp`
//! (`_on_input_event` :815, `_update_input_source` :655,
//! `get_input_source_target_ray_mode` :174, `get_display_refresh_rate` :210)
//! and `webxr_interface_js.h:153` for the tracker-name table.
//!
//! Patina's `WebXrInterface` had 6 of Godot's 13 signals and no input-source or
//! refresh-rate surface at all. These drive the state machine through the same
//! seam the wasm bindings will, so the native half is provable without a
//! browser — which is the point of keeping the interface browser-agnostic.

use gdplatform::xr::{
    webxr_tracker_name, TargetRayMode, WebXrInputEvent, WebXrInterface, WebXrSessionMode,
    WebXrSignal, WebXrStub,
};

/// An interface with a session already running, so input events have somewhere
/// to land.
fn started() -> WebXrInterface {
    let mut xr = WebXrInterface::new();
    xr.set_stub(WebXrStub {
        supported_modes: vec![WebXrSessionMode::ImmersiveVr],
        available_features: vec!["local-floor".to_string()],
        ..WebXrStub::default()
    });
    xr.session_mode = WebXrSessionMode::ImmersiveVr;
    xr.requested_reference_space_types = vec!["local-floor".to_string()];
    assert!(xr.initialize(), "the fixture's session must start");
    let _ = xr.take_signals();
    xr
}

/// ⚠️ **THE ORDERING THE BEAD GETS WRONG.** The acceptance says
/// "selectstart/select/selectend". Godot's real order is
/// **selectstart, selectend, select**: the browser only fires start and end,
/// and Godot synthesises `select` itself *after* `selectend`, from the same
/// event (`webxr_interface_js.cpp:840-845`, with an explicit comment saying so).
///
/// Emitting `select` between start and end — the order the name suggests, and
/// the order the bead asks for — would fire the completed-gesture signal before
/// the gesture completed. A script that starts an action on `select` would act
/// a frame early, on a press that might still be cancelled.
#[test]
fn select_is_emitted_after_selectend_not_between() {
    let mut xr = started();
    xr.set_input_source(0, TargetRayMode::TrackedPointer);

    xr.on_input_event(WebXrInputEvent::SelectStart, 0);
    xr.on_input_event(WebXrInputEvent::SelectEnd, 0);

    assert_eq!(
        xr.take_signals(),
        vec![
            WebXrSignal::SelectStart { input_source_id: 0 },
            WebXrSignal::SelectEnd { input_source_id: 0 },
            WebXrSignal::Select { input_source_id: 0 },
        ],
        "Godot emits selectend THEN select, both from the SELECTEND event"
    );
}

/// Squeeze follows the same shape, for the same reason.
#[test]
fn squeeze_is_emitted_after_squeezeend() {
    let mut xr = started();
    xr.set_input_source(1, TargetRayMode::TrackedPointer);

    xr.on_input_event(WebXrInputEvent::SqueezeStart, 1);
    xr.on_input_event(WebXrInputEvent::SqueezeEnd, 1);

    assert_eq!(
        xr.take_signals(),
        vec![
            WebXrSignal::SqueezeStart { input_source_id: 1 },
            WebXrSignal::SqueezeEnd { input_source_id: 1 },
            WebXrSignal::Squeeze { input_source_id: 1 },
        ]
    );
}

/// ⚠️ Ids 0 and 1 are **always** the left and right hands — Godot's own comment
/// at `webxr_interface_js.cpp:679`. The names are positional, not discovered
/// from the browser, so an XRController3D bound to `left_hand` resolves.
#[test]
fn tracker_names_are_positional_left_then_right() {
    assert_eq!(webxr_tracker_name(0), "left_hand");
    assert_eq!(webxr_tracker_name(1), "right_hand");
    assert_eq!(webxr_tracker_name(2), "tracker_2", "sources past the hands");
    assert_eq!(webxr_tracker_name(7), "tracker_7");

    let mut xr = started();
    xr.set_input_source(0, TargetRayMode::TrackedPointer);
    xr.set_input_source(1, TargetRayMode::TrackedPointer);
    assert_eq!(xr.input_source_tracker_name(0), Some("left_hand"));
    assert_eq!(xr.input_source_tracker_name(1), Some("right_hand"));
}

/// ⚠️ An inactive source reports `Unknown`, not its last mode
/// (`webxr_interface_js.cpp:177`). Returning the stale value would let a script
/// keep treating a disconnected controller as a tracked pointer.
#[test]
fn an_inactive_source_reports_unknown_not_its_last_mode() {
    let mut xr = started();
    xr.set_input_source(0, TargetRayMode::TrackedPointer);
    assert!(xr.is_input_source_active(0));
    assert_eq!(
        xr.input_source_target_ray_mode(0),
        TargetRayMode::TrackedPointer
    );

    xr.remove_input_source(0);
    assert!(!xr.is_input_source_active(0));
    assert_eq!(
        xr.input_source_target_ray_mode(0),
        TargetRayMode::Unknown,
        "a disconnected source must not keep advertising TrackedPointer"
    );
    assert_eq!(
        xr.input_source_tracker_name(0),
        None,
        "and it must not resolve to a tracker either"
    );
}

/// A source that was never reported is unknown rather than a panic — ids come
/// from the browser and need not be dense.
#[test]
fn an_unreported_source_is_inactive_and_unknown() {
    let xr = started();
    assert!(!xr.is_input_source_active(4));
    assert_eq!(xr.input_source_target_ray_mode(4), TargetRayMode::Unknown);
    assert_eq!(xr.input_source_tracker_name(4), None);
}

/// Removing a source must NOT compact the list: Godot indexes by id, so
/// dropping entry 0 would silently rename the right hand to `left_hand`.
#[test]
fn removing_a_source_does_not_renumber_the_others() {
    let mut xr = started();
    xr.set_input_source(0, TargetRayMode::TrackedPointer);
    xr.set_input_source(1, TargetRayMode::Gaze);

    xr.remove_input_source(0);
    assert_eq!(
        xr.input_source_tracker_name(1),
        Some("right_hand"),
        "source 1 must still be the right hand after 0 disconnects"
    );
    assert_eq!(xr.input_source_target_ray_mode(1), TargetRayMode::Gaze);
}

/// The refresh rate fires `display_refresh_rate_changed` on a change only.
/// Emitting on every set would make a per-frame writer spam a signal Godot
/// fires on transitions.
#[test]
fn refresh_rate_signals_only_on_change() {
    let mut xr = started();
    xr.set_available_display_refresh_rates(vec![72.0, 90.0, 120.0]);
    assert_eq!(xr.available_display_refresh_rates(), &[72.0, 90.0, 120.0]);

    xr.set_display_refresh_rate(90.0);
    assert_eq!(xr.display_refresh_rate(), 90.0);
    assert_eq!(
        xr.take_signals(),
        vec![WebXrSignal::DisplayRefreshRateChanged]
    );

    xr.set_display_refresh_rate(90.0);
    assert!(
        xr.take_signals().is_empty(),
        "setting the same rate again must not re-signal"
    );

    xr.set_display_refresh_rate(120.0);
    assert_eq!(
        xr.take_signals(),
        vec![WebXrSignal::DisplayRefreshRateChanged]
    );
}

/// ⚠️ Input sources do not survive the session. Godot removes their trackers
/// from XRServer on exit; leaving them active would let a script poll a
/// controller for a session that has ended and never notice the headset came
/// off.
#[test]
fn ending_the_session_clears_input_sources_and_rates() {
    let mut xr = started();
    xr.set_input_source(0, TargetRayMode::TrackedPointer);
    xr.set_available_display_refresh_rates(vec![90.0]);
    xr.set_display_refresh_rate(90.0);
    let _ = xr.take_signals();

    xr.uninitialize();
    assert_eq!(xr.take_signals(), vec![WebXrSignal::SessionEnded]);
    assert!(
        !xr.is_input_source_active(0),
        "the controller must not outlive the session"
    );
    assert_eq!(xr.input_source_tracker_name(0), None);
    assert!(xr.available_display_refresh_rates().is_empty());
}

/// The full scripted sequence the acceptance describes, in Godot's real order.
/// Kept as one test so the ordering between phases — not just within them — is
/// pinned.
#[test]
fn a_full_session_produces_godots_signal_order() {
    let mut xr = WebXrInterface::new();
    xr.set_stub(WebXrStub {
        supported_modes: vec![WebXrSessionMode::ImmersiveVr],
        available_features: vec!["local-floor".to_string()],
        ..WebXrStub::default()
    });
    xr.session_mode = WebXrSessionMode::ImmersiveVr;
    xr.requested_reference_space_types = vec!["local-floor".to_string()];

    xr.is_session_supported("immersive-vr");
    assert!(xr.initialize());
    xr.set_input_source(0, TargetRayMode::TrackedPointer);
    xr.on_input_event(WebXrInputEvent::SelectStart, 0);
    xr.on_input_event(WebXrInputEvent::SelectEnd, 0);
    xr.uninitialize();

    assert_eq!(
        xr.take_signals(),
        vec![
            WebXrSignal::SessionSupported {
                mode: WebXrSessionMode::ImmersiveVr,
                supported: true,
            },
            WebXrSignal::SessionStarted,
            WebXrSignal::SelectStart { input_source_id: 0 },
            WebXrSignal::SelectEnd { input_source_id: 0 },
            WebXrSignal::Select { input_source_id: 0 },
            WebXrSignal::SessionEnded,
        ],
        "the whole sequence, with select AFTER selectend"
    );
}

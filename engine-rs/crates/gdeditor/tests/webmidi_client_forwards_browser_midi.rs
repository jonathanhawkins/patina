//! pat-s3rrv.176 acceptance: the served editor client actually calls
//! `navigator.requestMIDIAccess()` and forwards MIDI to the running game.
//!
//! Godot ground truth: `OS.open_midi_inputs()` starts the platform MIDI driver;
//! on the web that driver *is* `navigator.requestMIDIAccess()`, whose `inputs`
//! populate `OS.get_connected_midi_inputs()` and whose `onmidimessage` events
//! become `InputEventMIDI` delivered to `_input`
//! (`upstream/godot/core/os/midi_driver.cpp`, `platform/web`).
//!
//! # What was actually missing
//!
//! Everything server-side already existed and was green: `decode_midi_message`
//! (`gdplatform/src/input.rs`), the `POST /api/runtime/input/midi` route, and
//! `midi_e2e.rs` — which passes because **it POSTs the route itself**. The
//! served client had no MIDI code at all (`grep -ci midi editor_ui.rs` was 0),
//! so no physical controller could reach a played scene. A route with a test
//! that calls it, and no caller in the product, is the shape this bead exists
//! to catch.
//!
//! These are source assertions on `EDITOR_HTML` because the browser half cannot
//! be executed here. The route half is covered by `midi_e2e.rs`, which is
//! reported alongside this file rather than duplicated into it.

use gdeditor::editor_ui::EDITOR_HTML;

/// 🔑 THE ACCEPTANCE: the client asks the browser for MIDI and forwards it.
#[test]
fn webmidi_client_forwards_browser_midi() {
    assert!(
        EDITOR_HTML.contains("navigator.requestMIDIAccess()"),
        "the served client must actually call requestMIDIAccess — without it \
         every other piece of the MIDI path is unreachable from a real device"
    );
    assert!(
        EDITOR_HTML.contains("port.onmidimessage = function(ev)"),
        "and must subscribe to incoming messages on each input port"
    );
    assert!(
        EDITOR_HTML.contains("api('POST', '/api/runtime/input/midi', { device: index, ports: midiPortNames(), data: bytes })"),
        "the handler must POST the packet to the existing route, with the port \
         INDEX as `device` — the route reads `v[\"device\"].as_u64()`, so a port \
         NAME there would silently decode as device 0"
    );
}

/// 🔑 The request is gated, not unconditional.
///
/// ⚠️ This is the half that is easy to get wrong in the user's favour and
/// wrong in fact. `requestMIDIAccess()` raises a browser permission prompt.
/// Godot only starts its MIDI driver when a game calls `OS.open_midi_inputs()`,
/// so requesting on every Play would prompt every user who ever pressed it, for
/// a device almost none of them have.
#[test]
fn the_permission_prompt_is_gated_on_the_game_asking_for_midi() {
    assert!(
        EDITOR_HTML.contains("if (r.midi_open) startWebMidi();"),
        "the request must be gated on the engine reporting that \
         OS.open_midi_inputs() was called"
    );

    // ⚠️ Exactly one call site. A second, ungated `startWebMidi()` anywhere
    // would defeat the gate above while leaving its assertion green.
    assert_eq!(
        EDITOR_HTML.matches("startWebMidi()").count(),
        2,
        "expected exactly two occurrences — the definition and the single gated \
         call. More means somewhere else requests MIDI access ungated"
    );
}

/// Subscriptions are torn down with the rest of the game input.
///
/// ⚠️ Ports outlive a run: `access.inputs` handlers stay attached after the game
/// stops, so without teardown a controller would keep POSTing into a stopped
/// runtime for the life of the page.
#[test]
fn the_subscriptions_are_torn_down_when_the_run_stops() {
    assert!(
        EDITOR_HTML.contains("stopWebMidi();"),
        "stopGameInput must drop the MIDI subscriptions"
    );
    assert!(
        EDITOR_HTML.contains("midiPorts[i].onmidimessage = null;"),
        "and detaching means clearing the handler on every port, not just \
         forgetting the array"
    );
    // ⚠️ Checked as a WINDOW after the handler opens, not as a bare
    // `contains("if (!runtimeRunning) return;")`. That string appears a dozen
    // times in EDITOR_HTML — as a guard it would be green whether or not the
    // MIDI handler had one, which is no guard at all.
    let handler = EDITOR_HTML
        .find("port.onmidimessage = function(ev)")
        .expect("the onmidimessage handler must exist");
    // ⚠️ `&EDITOR_HTML[a..b]` panics if `b` lands inside a multi-byte character,
    // which would fail this test for a reason that has nothing to do with MIDI.
    // Take the window by characters instead.
    let window: String = EDITOR_HTML[handler..].chars().take(400).collect();
    assert!(
        window.contains("if (!runtimeRunning) return;"),
        "the MIDI handler itself must guard on the runtime still running, so a \
         controller cannot POST into a stopped runtime between the last frame \
         and teardown"
    );
}

/// Hot-plugged devices reach `OS.get_connected_midi_inputs()`.
///
/// ⚠️ The ports-only POST (`data: []`) is not redundant with the packet POST: a
/// device that is connected but has not played a note yet must still appear in
/// `OS.get_connected_midi_inputs()`, and the route explicitly treats a
/// packet-less report as success rather than an error.
#[test]
fn port_names_are_republished_on_statechange() {
    assert!(
        EDITOR_HTML.contains("access.onstatechange = function() { bindMidiPorts(access); };"),
        "plugging a controller in mid-session must re-bind and re-report"
    );
    assert!(
        EDITOR_HTML.contains(
            "api('POST', '/api/runtime/input/midi', { ports: midiPortNames(), data: [] })"
        ),
        "a ports-only report keeps OS.get_connected_midi_inputs() current for a \
         device that has not sent anything yet"
    );
}

/// The packet is sent as a plain array of byte values.
///
/// ⚠️ `ev.data` is a `Uint8Array`. `JSON.stringify` turns one into an OBJECT
/// keyed by index (`{"0":144,"1":60}`), not an array — the route's
/// `v["data"].as_array()` would then see nothing and decode an empty packet.
/// Copying into a real array is what makes the byte payload survive the wire.
#[test]
fn the_packet_is_copied_out_of_the_uint8array() {
    assert!(
        EDITOR_HTML.contains("for (var b = 0; b < ev.data.length; b++) bytes.push(ev.data[b]);"),
        "the Uint8Array must be copied element-by-element into a plain array"
    );
}

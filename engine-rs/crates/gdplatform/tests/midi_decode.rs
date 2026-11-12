//! pat-0q2kx.246 — raw MIDI bytes decode exactly as Godot's driver decodes them.
//!
//! Godot ground truth: `core/os/midi_driver.cpp` — `Parser::status_to_msg_enum`
//! (:64), `Parser::channel` (:98), `Parser::expected_data` (:79) and
//! `MIDIDriver::send_event` (:105), which is the function that fills an
//! `InputEventMIDI` and hands it to `parse_input_event`.
//!
//! These cases are the decoder's contract; the end-to-end route test
//! (`webmidi_input_events`) proves the transport. Kept separate because a
//! decode bug and a routing bug look identical from the far end of an HTTP
//! request, and only one of them is arithmetic you can pin down.

use gdplatform::input::{decode_midi_message, midi_message, InputEvent};

/// Destructure a decoded event, failing loudly if the packet was rejected.
#[allow(clippy::type_complexity)]
fn decode(packet: &[u8]) -> (u8, u8, u16, u8, u8, u8, u8, u8, u32) {
    match decode_midi_message(packet, 0) {
        Some(InputEvent::Midi {
            channel,
            message,
            pitch,
            velocity,
            controller_number,
            controller_value,
            instrument,
            pressure,
            device,
        }) => (
            channel,
            message,
            pitch,
            velocity,
            controller_number,
            controller_value,
            instrument,
            pressure,
            device,
        ),
        other => panic!("{packet:02X?} did not decode to a MIDI event: {other:?}"),
    }
}

/// The acceptance's own packet: `[0x90, 60, 100]` is NOTE_ON, channel 0.
#[test]
fn note_on_decodes_pitch_velocity_and_channel() {
    let (channel, message, pitch, velocity, ..) = decode(&[0x90, 60, 100]);
    assert_eq!(message, midi_message::NOTE_ON);
    assert_eq!(message, 9, "MIDI_MESSAGE_NOTE_ON is 9");
    assert_eq!(channel, 0);
    assert_eq!(pitch, 60);
    assert_eq!(velocity, 100);

    // The channel is the status byte's low nibble, so the same note on
    // channel 5 differs only there.
    let (channel, message, pitch, velocity, ..) = decode(&[0x95, 60, 100]);
    assert_eq!(
        (channel, message, pitch, velocity),
        (5, midi_message::NOTE_ON, 60, 100)
    );
}

/// NOTE_OFF is a distinct message id, not a NOTE_ON with velocity 0 — both
/// spellings exist on real hardware and must arrive as what they are.
#[test]
fn note_off_and_zero_velocity_note_on_stay_distinct() {
    let (channel, message, pitch, velocity, ..) = decode(&[0x82, 64, 20]);
    assert_eq!(
        (channel, message, pitch, velocity),
        (2, midi_message::NOTE_OFF, 64, 20)
    );

    // Velocity-0 NOTE_ON: conventionally means "note off", but the decoder
    // reports the message the wire carried and leaves that reading to the
    // script, exactly as Godot does.
    let (_, message, pitch, velocity, ..) = decode(&[0x90, 64, 0]);
    assert_eq!((message, pitch, velocity), (midi_message::NOTE_ON, 64, 0));
}

/// CONTROL_CHANGE fills the controller pair, not pitch/velocity. Getting this
/// wrong would put a CC number into `pitch`, which reads as a plausible note.
#[test]
fn control_change_fills_the_controller_pair() {
    let (channel, message, pitch, velocity, num, val, ..) = decode(&[0xB0, 7, 127]);
    assert_eq!(message, midi_message::CONTROL_CHANGE);
    assert_eq!(channel, 0);
    assert_eq!((num, val), (7, 127), "CC7 is volume");
    assert_eq!(
        (pitch, velocity),
        (0, 0),
        "a control change must not populate the note fields"
    );
}

/// PROGRAM_CHANGE and CHANNEL_PRESSURE are the one-data-byte messages.
#[test]
fn single_data_byte_messages() {
    let (channel, message, .., instrument, pressure, _) = decode(&[0xC3, 42]);
    assert_eq!(message, midi_message::PROGRAM_CHANGE);
    assert_eq!((channel, instrument, pressure), (3, 42, 0));

    let (channel, message, .., instrument, pressure, _) = decode(&[0xD1, 90]);
    assert_eq!(message, midi_message::CHANNEL_PRESSURE);
    assert_eq!((channel, instrument, pressure), (1, 0, 90));
}

/// AFTERTOUCH is per-note: it fills `pitch` *and* `pressure`, unlike
/// CHANNEL_PRESSURE which fills only `pressure`.
#[test]
fn polyphonic_aftertouch_carries_pitch_and_pressure() {
    let (channel, message, pitch, velocity, .., pressure, _) = decode(&[0xA4, 60, 77]);
    assert_eq!(message, midi_message::AFTERTOUCH);
    assert_eq!((channel, pitch, pressure), (4, 60, 77));
    assert_eq!(velocity, 0, "aftertouch has no velocity");
}

/// PITCH_BEND is 14 bits, **LSB first**: `(data[1] << 7) | data[0]`.
///
/// The operand order is the whole test. Swapping it still yields an in-range
/// number, so the failure surfaces as a wheel that jumps around rather than as
/// a decode bug — which is why centre, minimum and maximum are all pinned.
#[test]
fn pitch_bend_is_14_bit_lsb_first() {
    let (_, message, pitch, ..) = decode(&[0xE0, 0x00, 0x40]);
    assert_eq!(message, midi_message::PITCH_BEND);
    assert_eq!(pitch, 8192, "centre is MSB=0x40, LSB=0x00");

    assert_eq!(decode(&[0xE0, 0x00, 0x00]).2, 0, "full down");
    assert_eq!(decode(&[0xE0, 0x7F, 0x7F]).2, 16383, "full up");

    // Asymmetric packet: swapping the operands would give (0x01 << 7) | 0x7F
    // = 255 instead of 8191, so this case alone catches a reversal.
    assert_eq!(decode(&[0xE0, 0x7F, 0x3F]).2, 8191);
}

/// ⚠️ System messages (`0xF0..=0xFF`) report the **whole** status byte as the
/// message and carry **no channel**. Masking the low nibble in would turn
/// `SONG_SELECT` (0xF3) into "channel 3", and every realtime byte into a
/// channel it does not have.
#[test]
fn system_messages_report_the_whole_status_byte_and_no_channel() {
    let (channel, message, ..) = decode(&[0xF0]);
    assert_eq!(message, midi_message::SYSTEM_EXCLUSIVE);
    assert_eq!(message, 240);
    assert_eq!(channel, 0);

    let (channel, message, ..) = decode(&[0xF3, 5]);
    assert_eq!(message, midi_message::SONG_SELECT);
    assert_eq!(channel, 0, "0xF3 is SONG_SELECT, not channel 3");

    // Realtime bytes carry no data at all and must still produce an event.
    let (channel, message, ..) = decode(&[0xF8]);
    assert_eq!((channel, message), (0, 248), "MIDI_MESSAGE_TIMING_CLOCK");
    assert_eq!(decode(&[0xFF]).1, 255, "MIDI_MESSAGE_SYSTEM_RESET");
}

/// Packets that cannot produce an event are dropped rather than padded.
///
/// Truncating would invent a velocity of 0 for `[0x90, 60]`, and NOTE_ON with
/// velocity 0 is the conventional spelling of NOTE_OFF — so a short packet
/// would arrive as the *opposite* of what was played.
#[test]
fn malformed_packets_are_dropped_not_padded() {
    assert_eq!(decode_midi_message(&[], 0), None, "empty");
    assert_eq!(
        decode_midi_message(&[60, 100], 0),
        None,
        "a data byte is not a status byte"
    );
    assert_eq!(
        decode_midi_message(&[0x90, 60], 0),
        None,
        "NOTE_ON needs two data bytes"
    );
    assert_eq!(
        decode_midi_message(&[0xB0], 0),
        None,
        "CONTROL_CHANGE needs two data bytes"
    );
    assert_eq!(
        decode_midi_message(&[0xC0], 0),
        None,
        "PROGRAM_CHANGE needs one data byte"
    );
    // Extra trailing bytes are tolerated — a packet may carry a running-status
    // tail, and Godot only checks the minimum.
    assert_eq!(decode(&[0x90, 60, 100, 0x77]).2, 60);
}

/// The port index rides along as Godot's `InputEvent.device`.
#[test]
fn device_index_is_carried_through() {
    match decode_midi_message(&[0x90, 60, 100], 3) {
        Some(InputEvent::Midi { device, .. }) => assert_eq!(device, 3),
        other => panic!("expected a MIDI event, got {other:?}"),
    }
}

/// MIDI carries no held state: feeding events to `InputState` must not make
/// anything report as pressed. Godot has no `is_midi_*` query either.
///
/// Comparing `Debug` output is safe here specifically because a fresh
/// `InputState` has every collection empty — an empty `HashMap` has only one
/// rendering, so there is no iteration-order flakiness to inherit. Do not
/// extend this pattern to a populated state.
#[test]
fn midi_events_do_not_accumulate_input_state() {
    let mut state = gdplatform::input::InputState::new();
    let before = format!("{state:?}");
    state.process_event(decode_midi_message(&[0x90, 60, 100], 0).unwrap());
    state.process_event(decode_midi_message(&[0x80, 60, 0], 0).unwrap());
    assert_eq!(
        format!("{state:?}"),
        before,
        "a MIDI event must leave InputState untouched"
    );
}

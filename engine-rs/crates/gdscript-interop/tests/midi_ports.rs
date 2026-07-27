//! pat-0q2kx.246 — the MIDI port store behind `OS.*_midi_inputs`.
//!
//! Godot ground truth: `core/os/os.cpp:660-683` and
//! `platform/web/webmidi_driver.cpp` (`open` :41, `close` :49,
//! `_set_input_names_callback` :71).
//!
//! ⚠️ The store is **process-global**, which is deliberate (writer and reader
//! run on different threads — see the module docs). Each test therefore calls
//! `reset()` first. Under nextest every test gets its own process so they
//! cannot collide, but `cargo test` shares one, and a test that silently
//! inherited another's ports would pass for the wrong reason.

use gdscript_interop::midi_ports;

/// The client's report is what `OS.get_connected_midi_inputs()` returns, in
/// the order it was reported.
#[test]
fn reported_ports_are_returned_in_order() {
    midi_ports::reset();
    assert!(midi_ports::connected_inputs().is_empty());

    midi_ports::set_input_names(["Launchkey 25", "IAC Driver Bus 1"]);
    assert_eq!(
        midi_ports::connected_inputs(),
        vec!["Launchkey 25".to_string(), "IAC Driver Bus 1".to_string()],
        "order is the client's, not sorted"
    );
}

/// A second report **replaces** the list. Godot's callback clears before
/// pushing, so a device unplugged between two reports has to disappear —
/// appending would grow the list monotonically and never lose a port.
#[test]
fn a_second_report_replaces_rather_than_appends() {
    midi_ports::reset();
    midi_ports::set_input_names(["Launchkey 25", "IAC Driver Bus 1"]);
    midi_ports::set_input_names(["IAC Driver Bus 1"]);
    assert_eq!(
        midi_ports::connected_inputs(),
        vec!["IAC Driver Bus 1".to_string()],
        "the unplugged device must be gone, not still listed"
    );

    // `Vec::<String>::new()` rather than `[]` — an empty array literal leaves
    // the generic element type ambiguous and needs a turbofish to compile.
    midi_ports::set_input_names(Vec::<String>::new());
    assert!(
        midi_ports::connected_inputs().is_empty(),
        "reporting no ports must empty the list"
    );
}

/// `open_midi_inputs()` raises the flag the client watches — and does **not**
/// invent ports. Godot's `open()` only starts `requestMIDIAccess`; the names
/// arrive later on a callback, so a script that opens and immediately lists
/// gets an empty array in Godot too.
#[test]
fn open_sets_the_flag_without_inventing_ports() {
    midi_ports::reset();
    assert!(!midi_ports::is_open(), "closed until asked");

    midi_ports::open_inputs();
    assert!(midi_ports::is_open());
    assert!(
        midi_ports::connected_inputs().is_empty(),
        "open() must not fabricate a port list"
    );
}

/// ⚠️ `close_midi_inputs()` clears the port list as well as the flag —
/// `MIDIDriverWebMidi::close` calls `connected_input_names.clear()` before
/// closing. Dropping only the flag would leave the script able to list ports
/// that nothing is listening to.
#[test]
fn close_clears_the_ports_not_just_the_flag() {
    midi_ports::reset();
    midi_ports::open_inputs();
    midi_ports::set_input_names(["Launchkey 25"]);
    assert_eq!(midi_ports::connected_inputs().len(), 1);

    midi_ports::close_inputs();
    assert!(!midi_ports::is_open());
    assert!(
        midi_ports::connected_inputs().is_empty(),
        "close() must clear the port list, not only the open flag"
    );
}

/// Listing ports is **not** gated on `is_open()`. Godot's
/// `OS::get_connected_midi_inputs` returns the driver's field unconditionally;
/// only `close()` empties it. Gating here would be a stricter API than Godot's.
#[test]
fn listing_is_not_gated_on_open() {
    midi_ports::reset();
    midi_ports::set_input_names(["Launchkey 25"]);
    assert!(!midi_ports::is_open());
    assert_eq!(
        midi_ports::connected_inputs(),
        vec!["Launchkey 25".to_string()],
        "Godot reads the field without checking the open flag"
    );
}

/// Reopening after a close starts from an empty list rather than resurrecting
/// the old ports — the client has to report again.
#[test]
fn reopening_does_not_resurrect_stale_ports() {
    midi_ports::reset();
    midi_ports::open_inputs();
    midi_ports::set_input_names(["Launchkey 25"]);
    midi_ports::close_inputs();
    midi_ports::open_inputs();

    assert!(midi_ports::is_open());
    assert!(
        midi_ports::connected_inputs().is_empty(),
        "ports from before the close must not come back"
    );
}

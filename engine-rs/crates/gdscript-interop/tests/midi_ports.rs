//! pat-0q2kx.246 — the MIDI port store behind `OS.*_midi_inputs`.
//!
//! Godot ground truth: `core/os/os.cpp:660-683` and
//! `platform/web/webmidi_driver.cpp` (`open` :41, `close` :49,
//! `_set_input_names_callback` :71).
//!
//! ⚠️ The store is **process-global**, which is deliberate (writer and reader
//! run on different threads — see the module docs).
//!
//! The note that used to sit here said calling `reset()` at the top of each
//! test was enough because "under nextest every test gets its own process so
//! they cannot collide". The first half is true and the conclusion is wrong:
//! `cargo test` runs all six of these as THREADS in one process, so the resets
//! and `set_input_names` writes interleave and a test reads ports it never set.
//! That is a *rearm*, not isolation, and it is why a different subset failed on
//! every `cargo test` run (pat-21oab.3). nextest passing says nothing about it.
//! The tests now serialize on [`MIDI_LOCK`], which holds under both harnesses.

use gdscript_interop::midi_ports;

/// Serializes every test in this file against the process-global port store.
static MIDI_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Holds [`MIDI_LOCK`] for the rest of the test and empties the port store on
/// drop — including on an unwind, so a failing test cannot leave its ports
/// behind for whichever test runs next.
struct MidiGuard(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

impl Drop for MidiGuard {
    fn drop(&mut self) {
        midi_ports::reset();
    }
}

/// Takes the lock and clears the store. Bind it with
/// `let _guard = midi_guard();` — binding to plain `_` would drop the guard
/// immediately and release the lock before the test body ran.
///
/// Poison-tolerant, matching `gdcore::file_access`'s `ROOTS_LOCK`: a panicking
/// test must fail on its own rather than poison the mutex and cascade into the
/// other five.
fn midi_guard() -> MidiGuard {
    let lock = MIDI_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    midi_ports::reset();
    MidiGuard(lock)
}

/// The client's report is what `OS.get_connected_midi_inputs()` returns, in
/// the order it was reported.
#[test]
fn reported_ports_are_returned_in_order() {
    let _guard = midi_guard();
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
    let _guard = midi_guard();
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
    let _guard = midi_guard();
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
    let _guard = midi_guard();
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
    let _guard = midi_guard();
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
    let _guard = midi_guard();
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

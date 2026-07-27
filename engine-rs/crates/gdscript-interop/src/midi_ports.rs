//! pat-0q2kx.246 — the MIDI input ports a client has reported, and whether a
//! script has asked for MIDI access.
//!
//! Godot ground truth: `core/os/os.cpp:660-683` (`OS::get_connected_midi_inputs`
//! / `open_midi_inputs` / `close_midi_inputs`) and
//! `platform/web/webmidi_driver.cpp` (`MIDIDriverWebMidi::open` / `close` /
//! `_set_input_names_callback`), which is the driver Godot's web export uses.
//!
//! ## Why this is a store rather than a call
//!
//! Godot's `OS.open_midi_inputs()` does not return the port list, and neither
//! does anything else synchronously: `open()` asks the browser for
//! `navigator.requestMIDIAccess`, and the names arrive **later** on
//! `set_input_names_callback`. So `get_connected_midi_inputs()` reads a field
//! that something else filled in asynchronously — exactly what this module is.
//! A script that opens and immediately lists ports gets an empty array in
//! Godot too; that is the API's shape, not a gap here.
//!
//! ## Why process-global rather than `thread_local`
//!
//! The writer and the reader are on **different threads**. The client's port
//! list arrives on whichever editor-server connection thread handles the POST,
//! while `OS.get_connected_midi_inputs()` runs on the thread stepping the
//! scene. A `thread_local` — the idiom the interpreter's *handle* tables use —
//! would file the ports under the HTTP thread and leave the script reading an
//! empty list forever: a silent no-op, not an error. `editor_commands` hit the
//! same fork for pat-0q2kx.241 and resolved it the same way.
//!
//! ## Why `gdscript-interop` and not `gdeditor`
//!
//! `gdeditor` depends on `gdscript-interop`, so a store here is visible to both
//! the interpreter (reader) and the editor server (writer). The reverse
//! placement would not be — `gdscript-interop` cannot see `gdeditor`.

use std::sync::{Mutex, OnceLock};

/// Everything a MIDI-capable host has told us.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct MidiState {
    /// A script has called `OS.open_midi_inputs()` and the client should be
    /// running `navigator.requestMIDIAccess`.
    open: bool,
    /// Port names, in the order the client reported them.
    input_names: Vec<String>,
}

fn store() -> &'static Mutex<MidiState> {
    static STORE: OnceLock<Mutex<MidiState>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(MidiState::default()))
}

/// `OS.open_midi_inputs()` — ask the host for MIDI access.
///
/// Deliberately does **not** invent port names. Godot's `open()` only kicks off
/// `requestMIDIAccess`; the names land later via the client's callback, which
/// reaches us through [`set_input_names`]. Populating anything here would give
/// a script a port list that no device backs.
pub fn open_inputs() {
    store().lock().expect("midi ports poisoned").open = true;
}

/// `OS.close_midi_inputs()`.
///
/// ⚠️ Clears the port list as well as the flag, matching
/// `MIDIDriverWebMidi::close` (`webmidi_driver.cpp:49`), which calls
/// `connected_input_names.clear()` before closing. Leaving the names behind
/// would let `get_connected_midi_inputs()` keep advertising ports that are no
/// longer being listened to.
pub fn close_inputs() {
    let mut guard = store().lock().expect("midi ports poisoned");
    guard.open = false;
    guard.input_names.clear();
}

/// Whether a script has asked for MIDI access and not closed it.
///
/// The editor client reads this to decide whether to run
/// `navigator.requestMIDIAccess` — Godot's web export gates the browser prompt
/// on the same call rather than requesting MIDI unprompted at startup.
pub fn is_open() -> bool {
    store().lock().expect("midi ports poisoned").open
}

/// The client reporting which ports it can see.
///
/// **Replaces** the list rather than appending, matching
/// `MIDIDriverWebMidi::_set_input_names_callback`
/// (`webmidi_driver.cpp:71`), which clears before pushing. A device unplugged
/// between two reports has to disappear; appending would make the list grow
/// monotonically and never lose a port.
pub fn set_input_names<I, S>(names: I)
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut guard = store().lock().expect("midi ports poisoned");
    guard.input_names = names.into_iter().map(Into::into).collect();
}

/// `OS.get_connected_midi_inputs()` — the reported port names, in order.
///
/// Returns whatever the client last reported, **without** gating on
/// [`is_open`]. That mirrors Godot: `OS::get_connected_midi_inputs`
/// (`os.cpp:660`) returns `MIDIDriver::connected_input_names` unconditionally,
/// and the only thing that empties it is `close()`. Gating here would be a
/// stricter API than Godot's, and a script that reads the list before its
/// open request has round-tripped would see an empty array either way.
pub fn connected_inputs() -> Vec<String> {
    store()
        .lock()
        .expect("midi ports poisoned")
        .input_names
        .clone()
}

/// Drop all MIDI state. For tests and for hosts that reuse the process across
/// runs — the store outlives any one `MainLoop`, so a second run would
/// otherwise inherit the first run's ports.
pub fn reset() {
    *store().lock().expect("midi ports poisoned") = MidiState::default();
}

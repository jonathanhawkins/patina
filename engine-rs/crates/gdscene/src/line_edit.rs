//! LineEdit / TextEdit runtime editing (pat-0q2kx.87).
//!
//! Godot's `LineEdit` (upstream/godot/scene/gui/line_edit.cpp `gui_input`) is a
//! small state machine over three pieces of state — the text, a caret column,
//! and a selection range — driven entirely by key events routed to whichever
//! Control holds focus. Patina had the `text` property and nothing else:
//! `control::set_line_edit_text` wrote the string and no key event ever reached
//! it, so a LineEdit in a running scene could not be typed into, had no caret,
//! and never emitted `text_changed` or `text_submitted`.
//!
//! This module is that state machine. It keeps its state on **node properties**
//! (`caret_column`, `selection_from`, `selection_to`) rather than in a side
//! table, so the renderer stays a pure function of the tree and a `.tscn` save
//! round-trips the caret the way Godot's does.
//!
//! ## Character indices, not byte indices
//!
//! Godot's `caret_column` counts **characters**. Everything here converts to
//! `Vec<char>` first; using byte offsets would put the caret inside a multi-byte
//! grapheme the moment a user typed a non-ASCII character.
//!
//! ## Where the character comes from
//!
//! Godot reads `InputEventKey::get_unicode()`, which the OS fills in per the
//! active keyboard layout. Patina's [`InputEvent::Key`](gdplatform::input::InputEvent)
//! carries no unicode field — only a [`Key`] and the modifier flags — so
//! [`char_for_key`] maps keycode + shift through a **US layout**. That is the
//! honest limit of the current event model, not a design choice: when the
//! platform layer grows a unicode field, this function is the one place to
//! change.

use gdplatform::input::Key;
use gdvariant::Variant;

use crate::node::NodeId;
use crate::scene_tree::SceneTree;

/// Node property holding the caret's character index (Godot `caret_column`).
pub const CARET_COLUMN: &str = "caret_column";
/// Node property holding the anchor end of the selection.
pub const SELECTION_FROM: &str = "selection_from";
/// Node property holding the moving end of the selection.
pub const SELECTION_TO: &str = "selection_to";

/// Classes this state machine drives. `CodeEdit` extends `TextEdit` in Godot, so
/// it edits identically; the syntax highlighting is a render-side concern.
///
/// Matched by name **and** through `is_class`, because ClassDB is not guaranteed
/// to be populated in every runtime/test context — the same belt-and-braces test
/// [`crate::control::node_is_base_button`] uses.
const TEXT_INPUT_CLASSES: &[&str] = &["LineEdit", "TextEdit", "CodeEdit"];

/// Whether `node_id` is a text-input Control this module drives.
pub fn is_text_input(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| {
            TEXT_INPUT_CLASSES.contains(&n.class_name())
                || n.is_class("LineEdit")
                || n.is_class("TextEdit")
        })
        .unwrap_or(false)
}

/// Whether `node_id` is multi-line (a `TextEdit`/`CodeEdit`). The one behavioural
/// fork in this module: Enter submits on a LineEdit and inserts a newline on a
/// TextEdit.
pub fn is_multiline(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| matches!(n.class_name(), "TextEdit" | "CodeEdit") || n.is_class("TextEdit"))
        .unwrap_or(false)
}

// -- state accessors --------------------------------------------------------

/// The node's current text (`""` when unset).
pub fn text(tree: &SceneTree, node_id: NodeId) -> String {
    match tree.get_node(node_id).map(|n| n.get_property("text")) {
        Some(Variant::String(s)) => s,
        _ => String::new(),
    }
}

fn chars_of(tree: &SceneTree, node_id: NodeId) -> Vec<char> {
    text(tree, node_id).chars().collect()
}

fn int_prop(tree: &SceneTree, node_id: NodeId, key: &str) -> Option<i64> {
    match tree.get_node(node_id)?.get_property(key) {
        Variant::Int(i) => Some(i),
        _ => None,
    }
}

fn set_int_prop(tree: &mut SceneTree, node_id: NodeId, key: &str, value: i64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(key, Variant::Int(value));
    }
}

/// The caret's character index, clamped into the current text.
///
/// Clamping on read rather than trusting the stored value is deliberate: a
/// `.tscn` can carry any integer, and a script can assign one, so every consumer
/// would otherwise need its own bounds check.
pub fn caret_column(tree: &SceneTree, node_id: NodeId) -> usize {
    let len = chars_of(tree, node_id).len() as i64;
    int_prop(tree, node_id, CARET_COLUMN)
        .unwrap_or(0)
        .clamp(0, len) as usize
}

/// Moves the caret, clamping into the text.
pub fn set_caret_column(tree: &mut SceneTree, node_id: NodeId, column: usize) {
    let len = chars_of(tree, node_id).len();
    set_int_prop(tree, node_id, CARET_COLUMN, column.min(len) as i64);
}

/// The selection as an ordered `(begin, end)` character range, or `None` when
/// nothing is selected. Godot's `has_selection()` is exactly "the two ends
/// differ".
pub fn selection(tree: &SceneTree, node_id: NodeId) -> Option<(usize, usize)> {
    let len = chars_of(tree, node_id).len() as i64;
    let from = int_prop(tree, node_id, SELECTION_FROM)?.clamp(0, len);
    let to = int_prop(tree, node_id, SELECTION_TO)?.clamp(0, len);
    if from == to {
        return None;
    }
    Some((from.min(to) as usize, from.max(to) as usize))
}

/// Whether any text is selected (Godot `LineEdit.has_selection`).
pub fn has_selection(tree: &SceneTree, node_id: NodeId) -> bool {
    selection(tree, node_id).is_some()
}

/// The selected substring, or `""`.
pub fn selected_text(tree: &SceneTree, node_id: NodeId) -> String {
    match selection(tree, node_id) {
        Some((begin, end)) => chars_of(tree, node_id)[begin..end].iter().collect(),
        None => String::new(),
    }
}

/// Clears the selection without moving the caret (Godot `deselect`).
pub fn deselect(tree: &mut SceneTree, node_id: NodeId) {
    let caret = caret_column(tree, node_id) as i64;
    set_int_prop(tree, node_id, SELECTION_FROM, caret);
    set_int_prop(tree, node_id, SELECTION_TO, caret);
}

/// Selects the whole text and parks the caret at the end (Godot `select_all`).
pub fn select_all(tree: &mut SceneTree, node_id: NodeId) {
    let len = chars_of(tree, node_id).len() as i64;
    set_int_prop(tree, node_id, SELECTION_FROM, 0);
    set_int_prop(tree, node_id, SELECTION_TO, len);
    set_int_prop(tree, node_id, CARET_COLUMN, len);
}

/// `max_length` (`0` = unlimited), Godot's cap on the total character count.
pub fn max_length(tree: &SceneTree, node_id: NodeId) -> usize {
    int_prop(tree, node_id, "max_length").unwrap_or(0).max(0) as usize
}

/// Whether the node accepts edits. Godot defaults `editable` to `true`, so an
/// unset property must read as editable — not as read-only.
pub fn is_editable(tree: &SceneTree, node_id: NodeId) -> bool {
    match tree.get_node(node_id).map(|n| n.get_property("editable")) {
        Some(Variant::Bool(b)) => b,
        _ => true,
    }
}

/// The text as it should be **displayed**: masked when `secret` is set, which is
/// Godot's `LineEdit.secret` password mode. The stored `text` is never masked —
/// only the rendering is — so a script still reads the real value.
pub fn display_text(tree: &SceneTree, node_id: NodeId) -> String {
    let raw = text(tree, node_id);
    let secret = matches!(
        tree.get_node(node_id).map(|n| n.get_property("secret")),
        Some(Variant::Bool(true))
    );
    if !secret {
        return raw;
    }
    let mask = match tree
        .get_node(node_id)
        .map(|n| n.get_property("secret_character"))
    {
        // Godot takes the FIRST character of `secret_character` and ignores the
        // rest; an empty string means "draw nothing".
        Some(Variant::String(s)) => s.chars().next(),
        // Godot's default mask.
        _ => Some('•'),
    };
    match mask {
        Some(c) => raw.chars().map(|_| c).collect(),
        None => String::new(),
    }
}

// -- edits ------------------------------------------------------------------

/// Writes `new_text` and emits `text_changed(new_text)` if it actually changed.
///
/// Godot emits `text_changed` per edit, not per keystroke — a key that changes
/// nothing (backspace at column 0, a rejected over-length insert) is silent.
fn commit_text(tree: &mut SceneTree, node_id: NodeId, new_text: String) -> bool {
    if text(tree, node_id) == new_text {
        return false;
    }
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("text", Variant::String(new_text.clone()));
    }
    tree.emit_signal(node_id, "text_changed", &[Variant::String(new_text)]);
    true
}

/// Deletes the current selection, leaving the caret at its start. Returns
/// whether anything was removed.
fn delete_selection(tree: &mut SceneTree, node_id: NodeId) -> bool {
    let Some((begin, end)) = selection(tree, node_id) else {
        return false;
    };
    let mut chars = chars_of(tree, node_id);
    chars.drain(begin..end);
    let changed = commit_text(tree, node_id, chars.into_iter().collect());
    set_int_prop(tree, node_id, CARET_COLUMN, begin as i64);
    deselect(tree, node_id);
    changed
}

/// Inserts `insert` at the caret, replacing any selection, and advances the
/// caret past what was actually inserted.
///
/// Honors `max_length` by truncating the insertion to what fits — Godot accepts
/// the prefix rather than rejecting the whole insert, which is what makes typing
/// simply *stop* at the cap instead of the last keystroke being dropped
/// wholesale.
pub fn insert_text_at_caret(tree: &mut SceneTree, node_id: NodeId, insert: &str) -> bool {
    if !is_editable(tree, node_id) {
        return false;
    }
    delete_selection(tree, node_id);

    let mut chars = chars_of(tree, node_id);
    let caret = caret_column(tree, node_id);
    let cap = max_length(tree, node_id);
    let mut incoming: Vec<char> = insert.chars().collect();
    if cap > 0 {
        let room = cap.saturating_sub(chars.len());
        incoming.truncate(room);
    }
    if incoming.is_empty() {
        return false;
    }
    let added = incoming.len();
    for (offset, c) in incoming.into_iter().enumerate() {
        chars.insert(caret + offset, c);
    }
    let changed = commit_text(tree, node_id, chars.into_iter().collect());
    set_int_prop(tree, node_id, CARET_COLUMN, (caret + added) as i64);
    deselect(tree, node_id);
    changed
}

/// Backspace: deletes the selection if there is one, else the character before
/// the caret.
pub fn backspace(tree: &mut SceneTree, node_id: NodeId) -> bool {
    if !is_editable(tree, node_id) {
        return false;
    }
    if has_selection(tree, node_id) {
        return delete_selection(tree, node_id);
    }
    let caret = caret_column(tree, node_id);
    if caret == 0 {
        return false;
    }
    let mut chars = chars_of(tree, node_id);
    chars.remove(caret - 1);
    let changed = commit_text(tree, node_id, chars.into_iter().collect());
    set_int_prop(tree, node_id, CARET_COLUMN, (caret - 1) as i64);
    deselect(tree, node_id);
    changed
}

/// Delete: deletes the selection if there is one, else the character *at* the
/// caret. The caret does not move.
pub fn delete_at_caret(tree: &mut SceneTree, node_id: NodeId) -> bool {
    if !is_editable(tree, node_id) {
        return false;
    }
    if has_selection(tree, node_id) {
        return delete_selection(tree, node_id);
    }
    let caret = caret_column(tree, node_id);
    let mut chars = chars_of(tree, node_id);
    if caret >= chars.len() {
        return false;
    }
    chars.remove(caret);
    let changed = commit_text(tree, node_id, chars.into_iter().collect());
    deselect(tree, node_id);
    changed
}

/// Moves the caret to `target`, extending the selection when `shift` is held and
/// collapsing it otherwise — Godot's shift-selection rule.
fn move_caret(tree: &mut SceneTree, node_id: NodeId, target: usize, shift: bool) {
    if shift {
        // The anchor is whatever the selection already started from; with no
        // selection yet, the caret we are leaving becomes the anchor.
        let anchor = match selection(tree, node_id) {
            Some(_) => int_prop(tree, node_id, SELECTION_FROM).unwrap_or(0),
            None => caret_column(tree, node_id) as i64,
        };
        set_int_prop(tree, node_id, SELECTION_FROM, anchor);
        set_int_prop(tree, node_id, SELECTION_TO, target as i64);
        set_caret_column(tree, node_id, target);
    } else {
        set_caret_column(tree, node_id, target);
        deselect(tree, node_id);
    }
}

// -- key routing ------------------------------------------------------------

/// Applies one key event to the focused text input. Returns `true` when the
/// event was consumed, which the caller turns into `set_input_as_handled()`.
///
/// Consuming is deliberately wider than "the text changed": a focused LineEdit
/// swallows Left/Right/Home/End so they move the caret instead of walking the
/// focus ring, and swallows Enter so it submits instead of activating a button —
/// exactly what Godot's `LineEdit::gui_input` does by calling `accept_event()`.
pub fn handle_key(
    tree: &mut SceneTree,
    node_id: NodeId,
    key: Key,
    pressed: bool,
    shift: bool,
    ctrl: bool,
) -> bool {
    if !pressed || !is_text_input(tree, node_id) {
        return false;
    }

    // Ctrl chords first: `Ctrl+A` must not fall through to "type an 'a'".
    if ctrl {
        return match key {
            Key::A => {
                select_all(tree, node_id);
                true
            }
            // Godot's cut/copy are clipboard operations; Patina has no clipboard
            // yet, so only the edit half of cut is modelled. Copy is consumed
            // (not treated as typing) so the chord never inserts a stray "c".
            Key::X => {
                delete_selection(tree, node_id);
                true
            }
            Key::C => true,
            _ => false,
        };
    }

    match key {
        Key::Backspace => {
            backspace(tree, node_id);
            true
        }
        Key::Delete => {
            delete_at_caret(tree, node_id);
            true
        }
        Key::Left => {
            let caret = caret_column(tree, node_id);
            move_caret(tree, node_id, caret.saturating_sub(1), shift);
            true
        }
        Key::Right => {
            let caret = caret_column(tree, node_id);
            let len = chars_of(tree, node_id).len();
            move_caret(tree, node_id, (caret + 1).min(len), shift);
            true
        }
        // On a single-line LineEdit, Godot binds Up to Home and Down to End —
        // `ui_text_caret_up` and `ui_text_caret_line_start` share one branch
        // (upstream/godot/scene/gui/line_edit.cpp:1026-1035, "Up = Home, Down =
        // End"). A multi-line TextEdit moves between lines instead, which this
        // module does not model yet, so there the arrows fall through rather
        // than pretending to be Home/End.
        Key::Up if !is_multiline(tree, node_id) => {
            move_caret(tree, node_id, 0, shift);
            true
        }
        Key::Down if !is_multiline(tree, node_id) => {
            let len = chars_of(tree, node_id).len();
            move_caret(tree, node_id, len, shift);
            true
        }
        Key::Home => {
            move_caret(tree, node_id, 0, shift);
            true
        }
        Key::End => {
            let len = chars_of(tree, node_id).len();
            move_caret(tree, node_id, len, shift);
            true
        }
        Key::Enter => {
            if is_multiline(tree, node_id) {
                insert_text_at_caret(tree, node_id, "\n");
            } else {
                // Godot submits the CURRENT text and leaves it in place; it does
                // not clear the field.
                let current = text(tree, node_id);
                tree.emit_signal(node_id, "text_submitted", &[Variant::String(current)]);
            }
            true
        }
        // Up/Down belong to the focus ring on a single-line LineEdit; a multi-line
        // TextEdit has no vertical caret movement here yet, so both fall through
        // rather than being silently swallowed.
        other => match char_for_key(other, shift) {
            Some(c) => {
                // A non-editable field consumes the keystroke without changing —
                // Godot's read-only LineEdit still owns the event.
                insert_text_at_caret(tree, node_id, &c.to_string());
                true
            }
            None => false,
        },
    }
}

/// Maps a keycode plus shift to the character a **US keyboard layout** would
/// produce, or `None` for keys that are not text (arrows, function keys,
/// modifiers).
///
/// See the module docs: this stands in for `InputEventKey::get_unicode()`, which
/// Patina's input event does not carry.
pub fn char_for_key(key: Key, shift: bool) -> Option<char> {
    let c = match key {
        Key::A => 'a',
        Key::B => 'b',
        Key::C => 'c',
        Key::D => 'd',
        Key::E => 'e',
        Key::F => 'f',
        Key::G => 'g',
        Key::H => 'h',
        Key::I => 'i',
        Key::J => 'j',
        Key::K => 'k',
        Key::L => 'l',
        Key::M => 'm',
        Key::N => 'n',
        Key::O => 'o',
        Key::P => 'p',
        Key::Q => 'q',
        Key::R => 'r',
        Key::S => 's',
        Key::T => 't',
        Key::U => 'u',
        Key::V => 'v',
        Key::W => 'w',
        Key::X => 'x',
        Key::Y => 'y',
        Key::Z => 'z',
        Key::Space => ' ',
        // The number row's shifted forms are punctuation, not "capital digits".
        Key::Num0 => return Some(if shift { ')' } else { '0' }),
        Key::Num1 => return Some(if shift { '!' } else { '1' }),
        Key::Num2 => return Some(if shift { '@' } else { '2' }),
        Key::Num3 => return Some(if shift { '#' } else { '3' }),
        Key::Num4 => return Some(if shift { '$' } else { '4' }),
        Key::Num5 => return Some(if shift { '%' } else { '5' }),
        Key::Num6 => return Some(if shift { '^' } else { '6' }),
        Key::Num7 => return Some(if shift { '&' } else { '7' }),
        Key::Num8 => return Some(if shift { '*' } else { '8' }),
        Key::Num9 => return Some(if shift { '(' } else { '9' }),
        Key::Comma => return Some(if shift { '<' } else { ',' }),
        Key::Period => return Some(if shift { '>' } else { '.' }),
        Key::Slash => return Some(if shift { '?' } else { '/' }),
        Key::Semicolon => return Some(if shift { ':' } else { ';' }),
        Key::Quote => return Some(if shift { '"' } else { '\'' }),
        Key::BracketLeft => return Some(if shift { '{' } else { '[' }),
        Key::BracketRight => return Some(if shift { '}' } else { ']' }),
        Key::Backslash => return Some(if shift { '|' } else { '\\' }),
        Key::Minus => return Some(if shift { '_' } else { '-' }),
        Key::Equal => return Some(if shift { '+' } else { '=' }),
        _ => return None,
    };
    Some(if shift { c.to_ascii_uppercase() } else { c })
}

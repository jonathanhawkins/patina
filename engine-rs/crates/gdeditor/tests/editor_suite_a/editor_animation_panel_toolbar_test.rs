//! Visual-fidelity (DOM-structure) tests for the Animation bottom panel's
//! toolbar (pat-wuhv1).
//!
//! Godot builds the Animation panel's toolbar in
//! `AnimationPlayerEditor::AnimationPlayerEditor()`
//! (upstream/godot/editor/animation/animation_player_editor_plugin.cpp
//! :2052-2190), in this order: a playback container of five buttons
//! (`play_bw_from`, `play_bw`, `stop`, `play`, `play_from`), the `frame`
//! SpinBox, a separator, the `scale` LineEdit (constructed then hidden), the
//! `tool_anim` MenuButton titled "Animation", the `animation` OptionButton,
//! `autoplay`, the AnimationTrackEditor's "Edit" MenuButton
//! (animation_track_editor.cpp:8336-8381), the `onion_toggle` button with its
//! `onion_skinning` options MenuButton, and `pin`.
//!
//! These assert the served editor renders each of those controls, with Godot's
//! own labels and tooltips, rather than only the subset Patina shipped first
//! (a select, +/x, record, one play, stop).

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn make_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Player", "AnimationPlayer")).unwrap();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

fn editor_html(port: u16) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream
        .write_all(b"GET /editor HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}

/// Godot's playback cluster is five buttons, not the two Patina shipped. Each
/// carries Godot's tooltip text and default shortcut
/// (animation_player_editor_plugin.cpp:1209-1213, :2313-2317).
#[test]
fn animation_panel_renders_all_five_playback_buttons() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"anim-play-bw-from-btn\"",              // play_bw_from
        "Play Animation Backwards (A)",
        "id=\"anim-play-bw-btn\"",                   // play_bw
        "Play Animation Backwards from End (Shift+A)",
        "id=\"anim-stop-btn\"",                      // stop
        "Pause/Stop Animation (S)",
        "id=\"anim-play-btn\"",                      // play
        "Play Animation from Start (Shift+D)",
        "id=\"anim-play-from-btn\"",                 // play_from
        "Play Animation (D)",
    ] {
        assert!(
            html.contains(needle),
            "served Animation toolbar is missing `{needle}` \
             (Godot AnimationPlayerEditor builds the full playback container)"
        );
    }
    handle.stop();
}

/// The `frame` SpinBox and the `scale` LineEdit. Godot gives `frame` a step of
/// 0.0001 and the tooltip "Animation position (in seconds)."; it constructs
/// `scale` and then calls `scale->hide()`, so it is present but not visible.
#[test]
fn animation_panel_renders_frame_spinbox_and_hidden_scale_field() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"anim-frame\"",
        "step=\"0.0001\"",
        "Animation position (in seconds).",
        "id=\"anim-scale\"",
        "Scale animation playback globally for the node.",
    ] {
        assert!(html.contains(needle), "served Animation toolbar is missing `{needle}`");
    }
    // Godot hides `scale` at construction; the served field must be hidden too,
    // or the toolbar gains a control Godot's never shows.
    let scale_at = html.find("id=\"anim-scale\"").expect("anim-scale present");
    let tag_end = html[scale_at..].find('>').expect("anim-scale tag closes") + scale_at;
    assert!(
        html[scale_at..tag_end].contains("display:none"),
        "the scale LineEdit must be hidden, as Godot's scale->hide() makes it: {}",
        &html[scale_at..tag_end]
    );
    handle.stop();
}

/// The `tool_anim` MenuButton ("Animation", tooltip "Animation Tools") and its
/// popup: New..., Manage Animations..., Duplicate..., Rename..., Edit
/// Transitions..., Open in Inspector, Remove.
#[test]
fn animation_panel_renders_animation_tools_menu() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"anim-tool-btn\"",
        "Animation Tools",
        "id=\"anim-tool-dropdown\"",
        "data-act=\"new-anim\"",
        "Manage Animations...",
        "Duplicate...",
        "Rename...",
        "Edit Transitions...",
        "Open in Inspector",
        "data-act=\"remove-anim\"",
    ] {
        assert!(
            html.contains(needle),
            "served Animation Tools menu is missing `{needle}` \
             (Godot tool_anim popup, animation_player_editor_plugin.cpp:2113-2128)"
        );
    }
    handle.stop();
}

/// `autoplay` ("Autoplay on Load"), `pin` ("Pin AnimationPlayer"), and the
/// `animation` OptionButton's Godot tooltip.
#[test]
fn animation_panel_renders_autoplay_pin_and_animation_selector() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"anim-autoplay-btn\"",
        "Autoplay on Load",
        "id=\"anim-pin-btn\"",
        "Pin AnimationPlayer",
        "id=\"anim-select\"",
        "Display list of animations in player.",
    ] {
        assert!(html.contains(needle), "served Animation toolbar is missing `{needle}`");
    }
    handle.stop();
}

/// The AnimationTrackEditor "Edit" MenuButton (tooltip "Animation properties.")
/// and its popup items (animation_track_editor.cpp:8336-8378).
#[test]
fn animation_panel_renders_track_editor_edit_menu() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"anim-edit-btn\"",
        "Animation properties.",
        "id=\"anim-edit-dropdown\"",
        "Copy Tracks...",
        "Paste Tracks",
        "Scale Selection...",
        "Scale From Cursor...",
        "Set Start Offset (Audio)",
        "Set End Offset (Audio)",
        "Make Easing Selection...",
        "Duplicate Selected Keys",
        "Cut Selected Keys",
        "Copy Selected Keys",
        "Paste Keys",
        "Move First Selected Key to Cursor",
        "Move Last Selected Key to Cursor",
        "Delete Selection",
        "Go to Next Step",
        "Go to Previous Step",
        "Go to Next Keyframe",
        "Go to Previous Keyframe",
        "Add tracks to RESET",
        "Apply RESET",
        "Bake Animation...",
        "Optimize Animation (no undo)...",
        "Clean-Up Animation (no undo)...",
    ] {
        assert!(
            html.contains(needle),
            "served track editor Edit menu is missing `{needle}`"
        );
    }
    handle.stop();
}

/// The onion-skinning toggle keeps Godot's "Enable Onion Skinning" tooltip and
/// gains the `onion_skinning` options MenuButton, whose popup carries the
/// Directions/Depth sections Godot builds (:2159-2178).
#[test]
fn animation_panel_renders_onion_skinning_options_menu() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"anim-onion-btn\"",
        "Enable Onion Skinning",
        "id=\"anim-onion-options-btn\"",
        "Onion Skinning Options",
        "id=\"anim-onion-dropdown\"",
        "Directions",
        "data-act=\"onion-past\"",
        "data-act=\"onion-future\"",
        "Depth",
        "1 step",
        "2 steps",
        "3 steps",
        "Differences Only",
        "Force White Modulate",
        "Include Gizmos (3D)",
    ] {
        assert!(
            html.contains(needle),
            "served Onion Skinning options menu is missing `{needle}`"
        );
    }
    handle.stop();
}

/// Regression guard for the controls Patina already had: adding Godot's chrome
/// must not drop the panel's existing buttons or its timeline.
#[test]
fn animation_panel_keeps_its_existing_controls() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"animation-panel\"",
        "id=\"anim-new-btn\"",
        "id=\"anim-delete-btn\"",
        "id=\"anim-record-btn\"",
        "id=\"anim-curve-btn\"",
        "id=\"anim-copy-key-btn\"",
        "id=\"anim-paste-key-btn\"",
        "id=\"anim-track-up-btn\"",
        "id=\"anim-track-down-btn\"",
        "id=\"anim-tree-btn\"",
        "id=\"anim-time-display\"",
        "id=\"anim-timeline-canvas\"",
        "id=\"anim-tracks\"",
        "id=\"anim-add-track-btn\"",
    ] {
        assert!(html.contains(needle), "Animation panel lost `{needle}`");
    }
    handle.stop();
}

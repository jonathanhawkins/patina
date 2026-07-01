//! Idle-viewport render gating (pat-5xrsr).
//!
//! Regression test for the busy-loop where the editor example re-rendered the
//! full software-rasterized viewport ~10x/second even when nothing changed,
//! burning 30-77% CPU while idle and starving the shared build lock.
//!
//! You can't assert a CPU percentage in a unit test, so instead we test the
//! gating *logic* that makes the loop render-on-change:
//!
//! 1. `ViewportRenderSignature` is STABLE across idle ticks (no change → the
//!    loop skips the render), and
//! 2. it CHANGES on each thing that affects the rendered frame (edit, selection,
//!    zoom/pan, resize, mode switch, camera move) so those still show up
//!    promptly, and
//! 3. `should_render_idle` renders only when the signature changed, an animation
//!    preview is active, or the keepalive elapsed.

use gdeditor::editor_server::EditorState;
use gdeditor::idle_render::{should_render_idle, ViewportRenderSignature};
use gdeditor::EditorCommand;
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::time::Duration;

fn make_state() -> EditorState {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    EditorState::new(tree)
}

fn sig(state: &EditorState) -> ViewportRenderSignature {
    ViewportRenderSignature::from_state(state)
}

/// The signature is stable while idle and changes on every frame-affecting edit.
#[test]
fn idle_render_signature_stable_and_change_sensitive() {
    let mut state = make_state();
    let base = sig(&state);

    // STABLE across idle ticks: with no mutation, the signature is unchanged, so
    // the idle loop will NOT re-render (this is what fixes the busy-loop).
    for _ in 0..8 {
        assert_eq!(sig(&state), base, "signature must be stable across idle ticks");
    }

    // Each block mutates one input and asserts the signature changed vs. the
    // immediately-preceding state. `prev` carries the latest signature forward.
    let nid = state.scene_tree.root_id();

    // 1. Edit — undo-stack depth.
    let prev = sig(&state);
    state.undo_stack.push(EditorCommand::SetProperty {
        node_id: nid,
        property: "position".into(),
        new_value: Variant::String("1".into()),
        old_value: Variant::String("0".into()),
    });
    assert_ne!(sig(&state), prev, "an edit (undo depth) must change the signature");

    // ...and redo-stack depth.
    let prev = sig(&state);
    state.redo_stack.push(EditorCommand::SetProperty {
        node_id: nid,
        property: "position".into(),
        new_value: Variant::String("2".into()),
        old_value: Variant::String("1".into()),
    });
    assert_ne!(sig(&state), prev, "redo depth must change the signature");

    // 2. Selection — primary and multi.
    let prev = sig(&state);
    state.selected_node = Some(nid);
    assert_ne!(sig(&state), prev, "selecting a node must change the signature");

    let prev = sig(&state);
    state.selected_nodes = vec![nid];
    assert_ne!(sig(&state), prev, "multi-selection must change the signature");

    // 3. Zoom / pan.
    let prev = sig(&state);
    state.viewport_zoom = 2.5;
    assert_ne!(sig(&state), prev, "zoom must change the signature");

    let prev = sig(&state);
    state.viewport_pan = (12.0, -7.0);
    assert_ne!(sig(&state), prev, "pan must change the signature");

    // 4. Resize — render width then height.
    let prev = sig(&state);
    state.render_width = 1280;
    assert_ne!(sig(&state), prev, "render width must change the signature");

    let prev = sig(&state);
    state.render_height = 720;
    assert_ne!(sig(&state), prev, "render height must change the signature");

    // 5. Mode switch.
    let prev = sig(&state);
    state.editor_mode = "3d".into();
    assert_ne!(sig(&state), prev, "a mode switch must change the signature");

    // 6. Camera move — yaw, pitch, distance, focus.
    let prev = sig(&state);
    state.editor_viewport_3d.camera.yaw += 0.1;
    assert_ne!(sig(&state), prev, "camera yaw must change the signature");

    let prev = sig(&state);
    state.editor_viewport_3d.camera.pitch += 0.1;
    assert_ne!(sig(&state), prev, "camera pitch must change the signature");

    let prev = sig(&state);
    state.editor_viewport_3d.camera.distance += 2.0;
    assert_ne!(sig(&state), prev, "camera distance must change the signature");

    let prev = sig(&state);
    state.editor_viewport_3d.camera.focus_point.x += 1.0;
    assert_ne!(sig(&state), prev, "camera focus must change the signature");

    // Still stable after all the changes: recomputing without mutation matches.
    let settled = sig(&state);
    for _ in 0..3 {
        assert_eq!(sig(&state), settled, "signature must be stable once edits stop");
    }
}

/// The gating decision: render only on change, animation preview, or keepalive.
#[test]
fn should_render_idle_gates_on_change_anim_or_keepalive() {
    let mut state = make_state();
    let keepalive = Duration::from_secs(2);

    // First tick: nothing rendered yet (None) → always render.
    let s0 = sig(&state);
    assert!(
        should_render_idle(None, &s0, false, Duration::from_millis(0), keepalive),
        "the first idle tick must render to show the initial frame"
    );

    // Idle and unchanged, no animation, before keepalive → DO NOT render. This is
    // the case that previously busy-looped.
    assert!(
        !should_render_idle(Some(&s0), &s0, false, Duration::from_millis(50), keepalive),
        "an idle, unchanged editor must not re-render (the busy-loop fix)"
    );

    // A real change → render even immediately and with no animation.
    state.viewport_zoom = 3.0;
    let s1 = sig(&state);
    assert!(
        should_render_idle(Some(&s0), &s1, false, Duration::from_millis(10), keepalive),
        "a changed signature must trigger a render"
    );

    // Animation preview active → render every tick even when unchanged.
    assert!(
        should_render_idle(Some(&s1), &s1, true, Duration::from_millis(1), keepalive),
        "an active animation preview must render every tick"
    );

    // Keepalive elapsed → render even when unchanged and no animation.
    assert!(
        should_render_idle(Some(&s1), &s1, false, Duration::from_secs(3), keepalive),
        "the keepalive must force an occasional render"
    );

    // Just under the keepalive, unchanged, no animation → still idle.
    assert!(
        !should_render_idle(Some(&s1), &s1, false, Duration::from_millis(1999), keepalive),
        "must stay idle until the keepalive actually elapses"
    );
}

/// A live drag (or animation preview) must force a render every tick: it moves
/// the node without pushing undo, so the signature doesn't change mid-drag and
/// the drag would otherwise freeze until the keepalive (pat-5xrsr follow-up).
#[test]
fn force_render_active_on_anim_and_drag() {
    use gdcore::math::Vector2;
    use gdeditor::editor_server::DragState;
    use gdeditor::idle_render::force_render_active;

    let mut state = make_state();
    // A settled, idle editor does not force a render — gating falls to the
    // signature/keepalive (this is what kills the busy-loop).
    assert!(!force_render_active(&state), "idle editor must not force-render");

    // An animation preview keyframes nodes each tick → force render.
    state.animation_playback.playing = true;
    assert!(force_render_active(&state), "animation preview forces a render");
    state.animation_playback.playing = false;
    assert!(!force_render_active(&state));

    // A node drag in progress moves the node live (no undo push until drag-end),
    // so the signature won't change — force a render so the drag stays smooth.
    let node = state.scene_tree.all_nodes_in_tree_order()[1]; // [0]=root, [1]=Main
    let s_before = sig(&state);
    state.drag_state = Some(DragState {
        node_id: node,
        start_pixel: Vector2::ZERO,
        start_node_pos: Vector2::ZERO,
        camera_offset: Vector2::ZERO,
    });
    assert_eq!(sig(&state), s_before, "a drag does not change the signature by itself");
    assert!(force_render_active(&state), "an in-progress drag forces a render");
}

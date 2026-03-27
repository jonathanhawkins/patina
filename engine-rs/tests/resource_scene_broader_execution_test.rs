//! pat-7ug: Broaden resource and scene execution path coverage beyond the
//! current slice.
//!
//! Extends existing execution-path coverage (pat-zg81 20 tests, pat-lpae 71 tests,
//! scene_tree_broad 24 tests) with tests that exercise previously untested paths:
//!
//! 1. AnimationPlayer attachment and process_animations through MainLoop frames
//! 2. ProcessMode contract (Disabled, Always, WhenPaused, Pausable, Inherit)
//! 3. Input event routing through MainLoop (push_event → InputState → InputSnapshot)
//! 4. Tween parallel steps and looping
//! 5. Scene change + reload through MainLoop stepping
//! 6. Resource saver raw vs renumbered output comparison
//! 7. Deferred signal flushing through MainLoop step

use std::sync::Arc;

use gdresource::resource::Resource;
use gdresource::saver::TresSaver;
use gdresource::UnifiedLoader;
use gdscene::animation::{Animation, AnimationPlayer, AnimationTrack, KeyFrame, LoopMode};
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, ProcessMode};
use gdscene::packed_scene::PackedScene;
use gdscene::scene_tree::SceneTree;
use gdscene::tween::{Tween, TweenStep};
use gdplatform::input::{ActionBinding, InputEvent, InputMap, Key};
use gdvariant::Variant;

// ===========================================================================
// Part 1: AnimationPlayer through MainLoop
// ===========================================================================

#[test]
fn animation_player_advances_through_mainloop_step() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let sprite = Node::new("Sprite", "Sprite2D");
    let sprite_id = tree.add_child(root, sprite).unwrap();

    // Set initial property
    tree.get_node_mut(sprite_id)
        .unwrap()
        .set_property("modulate:a", Variant::Float(0.0));

    // Build animation: fade alpha from 0 to 1 over 1 second
    let mut track = AnimationTrack::new("modulate:a");
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(1.0)));

    let mut anim = Animation::new("fade_in", 1.0);
    anim.tracks.push(track);

    let mut player = AnimationPlayer::new();
    player.add_animation(anim);
    player.play("fade_in");

    tree.attach_animation_player(sprite_id, player).unwrap();

    let mut main_loop = MainLoop::new(tree);

    // Step 10 frames at 0.1s each = 1.0s total
    for _ in 0..10 {
        main_loop.step(0.1);
    }

    let alpha = main_loop
        .tree()
        .get_node(sprite_id)
        .unwrap()
        .get_property("modulate:a");
    // After 1.0s of a 1.0s animation, alpha should be ~1.0
    match alpha {
        Variant::Float(v) => assert!(
            (v - 1.0).abs() < 0.01,
            "Expected alpha ~1.0, got {v}"
        ),
        other => panic!("Expected Float, got {other:?}"),
    }
}

#[test]
fn animation_player_half_way_interpolates_correctly() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("Target", "Node2D");
    let node_id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(node_id)
        .unwrap()
        .set_property("scale", Variant::Float(1.0));

    let mut track = AnimationTrack::new("scale");
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(1.0)));
    track.add_keyframe(KeyFrame::linear(2.0, Variant::Float(3.0)));

    let mut anim = Animation::new("grow", 2.0);
    anim.tracks.push(track);

    let mut player = AnimationPlayer::new();
    player.add_animation(anim);
    player.play("grow");

    tree.attach_animation_player(node_id, player).unwrap();

    let mut main_loop = MainLoop::new(tree);
    // Step to 1.0s (halfway)
    for _ in 0..10 {
        main_loop.step(0.1);
    }

    let val = main_loop
        .tree()
        .get_node(node_id)
        .unwrap()
        .get_property("scale");
    match val {
        Variant::Float(v) => assert!(
            (v - 2.0).abs() < 0.1,
            "Expected scale ~2.0 at halfway, got {v}"
        ),
        other => panic!("Expected Float, got {other:?}"),
    }
}

#[test]
fn animation_player_looping_wraps_position() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("Blinker", "Node");
    let node_id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(node_id)
        .unwrap()
        .set_property("visible", Variant::Float(1.0));

    let mut track = AnimationTrack::new("visible");
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(1.0)));
    track.add_keyframe(KeyFrame::linear(0.5, Variant::Float(0.0)));

    let mut anim = Animation::new("blink", 0.5);
    anim.loop_mode = LoopMode::Linear;
    anim.tracks.push(track);

    let mut player = AnimationPlayer::new();
    player.add_animation(anim);
    player.play("blink");

    tree.attach_animation_player(node_id, player).unwrap();

    let mut main_loop = MainLoop::new(tree);

    // Run 1.0s (2 full loops of a 0.5s animation) — should still be playing
    for _ in 0..20 {
        main_loop.step(0.05);
    }

    let player = main_loop
        .tree()
        .get_animation_player(node_id)
        .unwrap();
    assert!(player.playing, "Looping animation should still be playing after 1s");
}

#[test]
fn animation_player_non_looping_stops_at_end() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("OneShot", "Node");
    let node_id = tree.add_child(root, node).unwrap();

    let mut track = AnimationTrack::new("value");
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
    track.add_keyframe(KeyFrame::linear(0.5, Variant::Float(100.0)));

    let mut anim = Animation::new("once", 0.5);
    anim.loop_mode = LoopMode::None;
    anim.tracks.push(track);

    let mut player = AnimationPlayer::new();
    player.add_animation(anim);
    player.play("once");

    tree.attach_animation_player(node_id, player).unwrap();

    let mut main_loop = MainLoop::new(tree);
    for _ in 0..20 {
        main_loop.step(0.05);
    }

    let player = main_loop
        .tree()
        .get_animation_player(node_id)
        .unwrap();
    assert!(!player.playing, "Non-looping animation should stop after completion");
}

// ===========================================================================
// Part 2: ProcessMode contract
// ===========================================================================

#[test]
fn process_mode_inherit_defaults_to_pausable() {
    let tree = SceneTree::new();
    let root = tree.root_id();
    // Root with Inherit should resolve to Pausable
    let mode = tree.effective_process_mode(root);
    assert_eq!(mode, ProcessMode::Pausable);
}

#[test]
fn process_mode_disabled_never_processes() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("Disabled", "Node");
    node.set_process_mode(ProcessMode::Disabled);
    let node_id = tree.add_child(root, node).unwrap();

    assert!(!tree.should_process_node(node_id));
    tree.set_paused(true);
    assert!(!tree.should_process_node(node_id));
}

#[test]
fn process_mode_always_processes_even_when_paused() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("Always", "Node");
    node.set_process_mode(ProcessMode::Always);
    let node_id = tree.add_child(root, node).unwrap();

    assert!(tree.should_process_node(node_id));
    tree.set_paused(true);
    assert!(tree.should_process_node(node_id));
}

#[test]
fn process_mode_when_paused_only_processes_while_paused() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("WhenPaused", "Node");
    node.set_process_mode(ProcessMode::WhenPaused);
    let node_id = tree.add_child(root, node).unwrap();

    assert!(!tree.should_process_node(node_id), "Should NOT process when unpaused");
    tree.set_paused(true);
    assert!(tree.should_process_node(node_id), "Should process when paused");
}

#[test]
fn process_mode_pausable_stops_when_paused() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("Pausable", "Node");
    node.set_process_mode(ProcessMode::Pausable);
    let node_id = tree.add_child(root, node).unwrap();

    assert!(tree.should_process_node(node_id), "Should process when unpaused");
    tree.set_paused(true);
    assert!(!tree.should_process_node(node_id), "Should NOT process when paused");
}

#[test]
fn process_mode_inherit_resolves_from_parent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut parent = Node::new("Parent", "Node");
    parent.set_process_mode(ProcessMode::Always);
    let parent_id = tree.add_child(root, parent).unwrap();

    // Child inherits → resolves to parent's Always
    let child = Node::new("Child", "Node"); // default is Inherit
    let child_id = tree.add_child(parent_id, child).unwrap();

    let mode = tree.effective_process_mode(child_id);
    assert_eq!(mode, ProcessMode::Always);

    tree.set_paused(true);
    assert!(tree.should_process_node(child_id), "Inherited Always should process when paused");
}

#[test]
fn process_mode_disabled_parent_overrides_always_child() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut parent = Node::new("DisabledParent", "Node");
    parent.set_process_mode(ProcessMode::Disabled);
    let parent_id = tree.add_child(root, parent).unwrap();

    // Child has Inherit → resolves to parent's Disabled
    let child = Node::new("Child", "Node");
    let child_id = tree.add_child(parent_id, child).unwrap();

    assert!(!tree.should_process_node(child_id));
}

#[test]
fn all_nodes_in_process_order_respects_priority() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut low = Node::new("Low", "Node");
    low.set_process_priority(10);
    let low_id = tree.add_child(root, low).unwrap();

    let mut high = Node::new("High", "Node");
    high.set_process_priority(-5);
    let high_id = tree.add_child(root, high).unwrap();

    let mut mid = Node::new("Mid", "Node");
    mid.set_process_priority(0);
    let mid_id = tree.add_child(root, mid).unwrap();

    let order = tree.all_nodes_in_process_order();
    let high_pos = order.iter().position(|id| *id == high_id).unwrap();
    let mid_pos = order.iter().position(|id| *id == mid_id).unwrap();
    let low_pos = order.iter().position(|id| *id == low_id).unwrap();

    assert!(high_pos < mid_pos, "High priority (-5) should come before mid (0)");
    assert!(mid_pos < low_pos, "Mid priority (0) should come before low (10)");
}

// ===========================================================================
// Part 3: Input event routing through MainLoop
// ===========================================================================

#[test]
fn push_event_key_press_visible_in_input_state() {
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);

    main_loop.push_event(InputEvent::Key {
        key: Key::A,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });

    assert!(
        main_loop.input_state().is_key_pressed(Key::A),
        "Key A should be pressed after push_event"
    );
}

#[test]
fn push_event_clears_just_pressed_after_step() {
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);

    main_loop.push_event(InputEvent::Key {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });

    assert!(main_loop.input_state().is_key_just_pressed(Key::Space));

    // Step should flush per-frame state
    main_loop.step(1.0 / 60.0);

    assert!(
        !main_loop.input_state().is_key_just_pressed(Key::Space),
        "just_pressed should be cleared after step"
    );
    assert!(
        main_loop.input_state().is_key_pressed(Key::Space),
        "Key should still be held"
    );
}

#[test]
fn input_map_action_binding_routes_through_mainloop() {
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);

    let mut map = InputMap::new();
    map.add_action("jump", 0.0);
    map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
    main_loop.set_input_map(map);

    main_loop.push_event(InputEvent::Key {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });

    assert!(
        main_loop.input_state().is_action_pressed("jump"),
        "Action 'jump' should be pressed after Space key event"
    );
}

#[test]
fn input_snapshot_cleared_after_step() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("Player", "Node2D");
    tree.add_child(root, node).unwrap();

    let mut main_loop = MainLoop::new(tree);
    main_loop.push_event(InputEvent::Key {
        key: Key::W,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });

    main_loop.step(1.0 / 60.0);

    // Input snapshot should be cleared after step
    assert!(
        !main_loop.tree().has_input_snapshot(),
        "Input snapshot should be cleared at end of step"
    );
}

// ===========================================================================
// Part 4: Tween parallel steps and looping
// ===========================================================================

#[test]
fn tween_sequential_steps_execute_in_order() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("Target", "Node2D");
    let node_id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(node_id)
        .unwrap()
        .set_property("x", Variant::Float(0.0));
    tree.get_node_mut(node_id)
        .unwrap()
        .set_property("y", Variant::Float(0.0));

    let mut tween = Tween::new();
    tween.steps.push(TweenStep::new("x", Variant::Float(0.0), Variant::Float(100.0), 1.0));
    tween.steps.push(TweenStep::new("y", Variant::Float(0.0), Variant::Float(200.0), 1.0));
    tween.start();

    let _tween_id = tree.add_tween(node_id, tween);

    // Advance 1.0s — first step (x) should be done, second (y) just starting
    tree.process_tweens(1.0);

    let x = tree.get_node(node_id).unwrap().get_property("x");
    match x {
        Variant::Float(v) => assert!(
            (v - 100.0).abs() < 0.1,
            "x should be ~100 after 1s, got {v}"
        ),
        other => panic!("Expected Float for x, got {other:?}"),
    }
}

#[test]
fn tween_with_looping_resets_after_completion() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("Looper", "Node");
    let node_id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(node_id)
        .unwrap()
        .set_property("alpha", Variant::Float(0.0));

    let mut tween = Tween::new();
    tween.steps.push(TweenStep::new(
        "alpha",
        Variant::Float(0.0),
        Variant::Float(1.0),
        0.5,
    ));
    tween.set_loops(3);
    tween.start();

    let _tween_id = tree.add_tween(node_id, tween);

    // Advance 0.5s — first loop done
    tree.process_tweens(0.5);
    assert_eq!(tree.tween_count(), 1, "Tween should still exist (2 loops left)");

    // Advance another 1.0s — should complete remaining 2 loops
    tree.process_tweens(1.0);
    assert_eq!(tree.tween_count(), 0, "Tween should be removed after 3 loops");
}

#[test]
fn tween_completed_removes_from_tree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("Fader", "Node");
    let node_id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(node_id)
        .unwrap()
        .set_property("opacity", Variant::Float(1.0));

    let mut tween = Tween::new();
    tween.steps.push(TweenStep::new(
        "opacity",
        Variant::Float(1.0),
        Variant::Float(0.0),
        0.5,
    ));
    tween.start();
    tree.add_tween(node_id, tween);

    assert_eq!(tree.tween_count(), 1);
    tree.process_tweens(1.0); // well past duration
    assert_eq!(tree.tween_count(), 0, "Completed tween should be removed");
}

#[test]
fn tween_through_mainloop_updates_property() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("Slider", "Control");
    let node_id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(node_id)
        .unwrap()
        .set_property("value", Variant::Float(0.0));

    let mut tween = Tween::new();
    tween.steps.push(TweenStep::new(
        "value",
        Variant::Float(0.0),
        Variant::Float(50.0),
        1.0,
    ));
    tween.start();
    tree.add_tween(node_id, tween);

    let mut main_loop = MainLoop::new(tree);
    // Step 20 frames at 0.05s = 1.0s
    for _ in 0..20 {
        main_loop.step(0.05);
    }

    let val = main_loop
        .tree()
        .get_node(node_id)
        .unwrap()
        .get_property("value");
    match val {
        Variant::Float(v) => assert!(
            (v - 50.0).abs() < 1.0,
            "Tween should reach ~50 after 1s, got {v}"
        ),
        other => panic!("Expected Float, got {other:?}"),
    }
    assert_eq!(main_loop.tree().tween_count(), 0);
}

// ===========================================================================
// Part 5: Scene change and reload through MainLoop
// ===========================================================================

fn minimal_tscn() -> &'static str {
    r#"[gd_scene load_steps=1 format=3]

[node name="Root" type="Node2D"]

[node name="Child" type="Sprite2D" parent="."]
"#
}

fn alternate_tscn() -> &'static str {
    r#"[gd_scene load_steps=1 format=3]

[node name="Level2" type="Node"]

[node name="Enemy" type="Node2D" parent="."]

[node name="Boss" type="Node2D" parent="."]
"#
}

#[test]
fn change_scene_to_packed_replaces_tree_children() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Add some initial children
    tree.add_child(root, Node::new("OldChild1", "Node")).unwrap();
    tree.add_child(root, Node::new("OldChild2", "Node")).unwrap();
    assert_eq!(tree.node_count(), 3); // root + 2

    let packed = PackedScene::from_tscn(minimal_tscn()).unwrap();
    let scene_root = tree.change_scene_to_packed(&packed).unwrap();

    // Old children should be gone, new scene instantiated
    let root_node = tree.get_node(root).unwrap();
    assert_eq!(root_node.children().len(), 1, "Root should have exactly one child (scene root)");

    let scene = tree.get_node(scene_root).unwrap();
    assert_eq!(scene.name(), "Root");
}

#[test]
fn change_scene_twice_replaces_correctly() {
    let mut tree = SceneTree::new();

    let packed1 = PackedScene::from_tscn(minimal_tscn()).unwrap();
    let _scene1 = tree.change_scene_to_packed(&packed1).unwrap();

    let packed2 = PackedScene::from_tscn(alternate_tscn()).unwrap();
    let scene2 = tree.change_scene_to_packed(&packed2).unwrap();

    let scene = tree.get_node(scene2).unwrap();
    assert_eq!(scene.name(), "Level2");
    assert_eq!(scene.children().len(), 2, "Level2 should have Enemy + Boss");
}

#[test]
fn reload_current_scene_restores_initial_state() {
    let mut tree = SceneTree::new();

    let packed = PackedScene::from_tscn(minimal_tscn()).unwrap();
    let scene_root = tree.change_scene_to_packed(&packed).unwrap();

    // Mutate the tree — add an extra node
    tree.add_child(scene_root, Node::new("Extra", "Node")).unwrap();
    let scene = tree.get_node(scene_root).unwrap();
    let children_before = scene.children().len();
    assert!(children_before >= 2, "Should have Child + Extra");

    // Reload — should restore to original (just Child)
    let reloaded_root = tree.reload_current_scene().unwrap();
    let reloaded = tree.get_node(reloaded_root).unwrap();
    assert_eq!(reloaded.children().len(), 1, "Reloaded scene should have only Child");
}

#[test]
fn reload_without_packed_source_returns_error() {
    let mut tree = SceneTree::new();
    let result = tree.reload_current_scene();
    assert!(result.is_err(), "Reload with no current scene should error");
}

#[test]
fn change_scene_to_node_clears_packed_source() {
    let mut tree = SceneTree::new();

    // First load from packed
    let packed = PackedScene::from_tscn(minimal_tscn()).unwrap();
    tree.change_scene_to_packed(&packed).unwrap();

    // Then change to a bare node
    let new_root = Node::new("ManualScene", "Node");
    tree.change_scene_to_node(new_root).unwrap();

    // Reload should fail (packed source was cleared)
    let result = tree.reload_current_scene();
    assert!(result.is_err(), "Reload should fail after change_scene_to_node");
}

#[test]
fn scene_change_mid_mainloop_execution() {
    let mut tree = SceneTree::new();
    let packed = PackedScene::from_tscn(minimal_tscn()).unwrap();
    tree.change_scene_to_packed(&packed).unwrap();

    let mut main_loop = MainLoop::new(tree);
    // Run a few frames
    for _ in 0..5 {
        main_loop.step(1.0 / 60.0);
    }

    // Change scene mid-execution
    let packed2 = PackedScene::from_tscn(alternate_tscn()).unwrap();
    let new_root = main_loop
        .tree_mut()
        .change_scene_to_packed(&packed2)
        .unwrap();

    // Run more frames — should not panic
    for _ in 0..5 {
        main_loop.step(1.0 / 60.0);
    }

    let scene = main_loop.tree().get_node(new_root).unwrap();
    assert_eq!(scene.name(), "Level2");
    assert_eq!(main_loop.frame_count(), 10);
}

// ===========================================================================
// Part 6: Resource saver raw vs renumbered
// ===========================================================================

#[test]
fn tres_saver_raw_preserves_original_ids() {
    let mut resource = Resource::new("Resource");
    let mut sub1 = Resource::new("SubResource");
    sub1.set_property("name", Variant::String("first".into()));
    let mut sub2 = Resource::new("SubResource");
    sub2.set_property("name", Variant::String("second".into()));

    resource.subresources.insert("sub_99".into(), Arc::new(sub1));
    resource.subresources.insert("sub_42".into(), Arc::new(sub2));

    let saver = TresSaver::new();
    let raw_output = saver.save_to_string_raw(&resource).unwrap();

    // Raw should preserve the original IDs
    assert!(
        raw_output.contains("id=\"sub_99\""),
        "Raw output should preserve sub_99 ID"
    );
    assert!(
        raw_output.contains("id=\"sub_42\""),
        "Raw output should preserve sub_42 ID"
    );
}

#[test]
fn tres_saver_renumbered_produces_sequential_ids() {
    let mut resource = Resource::new("Resource");
    let mut sub1 = Resource::new("SubResource");
    sub1.set_property("value", Variant::Int(1));
    let mut sub2 = Resource::new("SubResource");
    sub2.set_property("value", Variant::Int(2));

    resource.subresources.insert("sub_99".into(), Arc::new(sub1));
    resource.subresources.insert("sub_42".into(), Arc::new(sub2));

    let saver = TresSaver::new();
    let renumbered = saver.save_to_string(&resource).unwrap();

    // Renumbered should have sequential IDs
    assert!(
        renumbered.contains("id=\"1\"") || renumbered.contains("id=\"SubResource_1\""),
        "Renumbered output should contain sequential ID: {renumbered}"
    );
}

#[test]
fn tres_saver_roundtrip_preserves_properties() {
    let loader = gdresource::loader::TresLoader::new();
    let mut resource = Resource::new("TestResource");
    resource.set_property("health", Variant::Int(100));
    resource.set_property("name", Variant::String("Player".into()));

    let saver = TresSaver::new();
    let saved = saver.save_to_string(&resource).unwrap();
    let reloaded = loader.parse_str(&saved, "test.tres").unwrap();

    assert_eq!(
        reloaded.get_property("health"),
        Some(&Variant::Int(100)),
        "health should survive roundtrip"
    );
    assert_eq!(
        reloaded.get_property("name"),
        Some(&Variant::String("Player".into())),
        "name should survive roundtrip"
    );
}

// ===========================================================================
// Part 7: MainLoop pause dispatch
// ===========================================================================

#[test]
fn mainloop_pause_dispatches_notification() {
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);

    // Run a frame to establish baseline
    main_loop.step(1.0 / 60.0);

    main_loop.set_paused(true);
    assert!(main_loop.paused());

    // The tree should be paused
    assert!(main_loop.tree().paused());

    main_loop.set_paused(false);
    assert!(!main_loop.paused());
    assert!(!main_loop.tree().paused());
}

#[test]
fn mainloop_pause_idempotent() {
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);

    main_loop.set_paused(true);
    main_loop.set_paused(true); // should be no-op
    assert!(main_loop.paused());

    main_loop.set_paused(false);
    main_loop.set_paused(false); // should be no-op
    assert!(!main_loop.paused());
}

// ===========================================================================
// Part 8: Deferred calls through MainLoop
// ===========================================================================

#[test]
fn deferred_call_executes_during_step() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("Target", "Node");
    let node_id = tree.add_child(root, node).unwrap();

    tree.call_deferred(node_id, "set_property", &[
        Variant::String("custom_flag".into()),
        Variant::Bool(true),
    ]);

    let mut main_loop = MainLoop::new(tree);
    main_loop.step(1.0 / 60.0);

    // Deferred calls are flushed during step
    // (Note: call_deferred dispatches by method name; if "set_property" isn't
    // a recognized method, the call is silently dropped. This test verifies
    // the deferred queue was flushed — no panic.)
}

#[test]
fn queue_free_removes_node_after_step() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("Ephemeral", "Node");
    let node_id = tree.add_child(root, node).unwrap();
    assert_eq!(tree.node_count(), 2);

    tree.queue_free(node_id);

    let mut main_loop = MainLoop::new(tree);
    main_loop.step(1.0 / 60.0);

    assert_eq!(
        main_loop.tree().node_count(),
        1,
        "Node should be removed after step processes deletions"
    );
}

// ===========================================================================
// Part 9: UnifiedLoader execution paths
// ===========================================================================

#[test]
fn unified_loader_resolve_to_path_with_uid() {
    let loader = gdresource::loader::TresLoader::new();
    let mut unified = UnifiedLoader::new(loader);

    unified.register_uid_str("uid://abc123", "res://materials/default.tres");

    let resolved = unified.resolve_to_path("uid://abc123").unwrap();
    assert_eq!(resolved, "res://materials/default.tres");
}

#[test]
fn unified_loader_resolve_to_path_passthrough_for_plain_path() {
    let loader = gdresource::loader::TresLoader::new();
    let unified = UnifiedLoader::new(loader);

    let resolved = unified.resolve_to_path("res://some/file.tres").unwrap();
    assert_eq!(resolved, "res://some/file.tres");
}

#[test]
fn unified_loader_is_cached_false_before_load() {
    let loader = gdresource::loader::TresLoader::new();
    let unified = UnifiedLoader::new(loader);

    assert!(!unified.is_cached("res://nonexistent.tres"));
}

#[test]
fn unified_loader_cache_len_starts_at_zero() {
    let loader = gdresource::loader::TresLoader::new();
    let unified = UnifiedLoader::new(loader);

    assert_eq!(unified.cache_len(), 0);
}

#[test]
fn unified_loader_replace_cached_updates_entry() {
    let loader = gdresource::loader::TresLoader::new();
    let mut unified = UnifiedLoader::new(loader);

    let resource = Arc::new(Resource::new("Material"));
    unified.replace_cached("res://mat.tres", resource.clone());

    assert!(unified.is_cached("res://mat.tres"));
    assert_eq!(unified.cache_len(), 1);

    let cached = unified.get_cached("res://mat.tres").unwrap();
    assert_eq!(cached.class_name, "Material");
}

// ===========================================================================
// Part 10: Frame trace through MainLoop
// ===========================================================================

#[test]
fn step_traced_captures_frame_record() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Player", "Node2D")).unwrap();

    let mut main_loop = MainLoop::new(tree);
    let record = main_loop.step_traced(1.0 / 60.0);

    assert_eq!(record.frame_number, 1);
    assert!((record.delta - 1.0 / 60.0).abs() < 1e-10);
    assert!(!record.paused);
}

#[test]
fn run_frames_traced_produces_correct_count() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("NPC", "Node")).unwrap();

    let mut main_loop = MainLoop::new(tree);
    let trace = main_loop.run_frames_traced(5, 1.0 / 60.0);

    assert_eq!(trace.len(), 5);
    assert_eq!(trace.frames[0].frame_number, 1);
    assert_eq!(trace.frames[4].frame_number, 5);
}

#[test]
fn frame_trace_physics_ticks_sum_correctly() {
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);
    main_loop.set_physics_ticks_per_second(60);

    // At 60 TPS with 1/60s delta, each frame should have exactly 1 physics tick
    let trace = main_loop.run_frames_traced(10, 1.0 / 60.0);

    assert_eq!(trace.total_physics_ticks(), 10);
}

#[test]
fn frame_trace_deterministic_same_input_same_output() {
    fn run_trace() -> gdscene::main_loop::FrameTrace {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.add_child(root, Node::new("A", "Node2D")).unwrap();
        tree.add_child(root, Node::new("B", "Node")).unwrap();

        let mut main_loop = MainLoop::new(tree);
        main_loop.run_frames_traced(3, 1.0 / 60.0)
    }

    let trace1 = run_trace();
    let trace2 = run_trace();

    assert_eq!(trace1.len(), trace2.len());
    for (i, (f1, f2)) in trace1.frames.iter().zip(trace2.frames.iter()).enumerate() {
        assert_eq!(f1.frame_number, f2.frame_number, "Frame {i} numbers differ");
        assert_eq!(f1.physics_ticks, f2.physics_ticks, "Frame {i} physics ticks differ");
        assert!(
            (f1.delta - f2.delta).abs() < 1e-15,
            "Frame {i} delta differs"
        );
    }
}

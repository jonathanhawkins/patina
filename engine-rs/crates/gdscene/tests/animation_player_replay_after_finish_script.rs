//! pat-mgixk.2 acceptance: Godot-exact `AnimationPlayer::play` restart,
//! queue-clear and signal semantics, driven from a script in a loaded scene.
//!
//! The bug this pins is the attack/jump pattern. `play_ex` treated a `play` on
//! the already-current clip as "resume in place", so once a non-looping clip
//! had run to its end the playhead stayed AT the end: the next `advance`
//! tripped the `position >= upper` test immediately, finished the clip again,
//! and the animation never played a second time. A `play("attack")` on every
//! key press therefore animated exactly once per scene.
//!
//! Godot's `AnimationPlayer::_play` (`animation_player.cpp:509-535`) has three
//! ordered cases for the same clip, and the order IS the behaviour:
//!   * `from_end` and the playhead at the start → seek to the end,
//!   * not `from_end` and the playhead at or past the end → seek to the start,
//!   * otherwise, if already playing → return with NO `animation_started`.
//! The last case is what keeps a `play("walk")` called every frame in
//! `_process` down to a single signal; the middle one is the replay fix.
//!
//! It also pins the two signals that were wrong: `animation_changed(old, new)`
//! fires only when the playback queue advances (`:348-350`), and
//! `current_animation_changed` — registered in ClassDB with nothing emitting it
//! — carries the clip name on a switch and the empty string on a finish
//! (`:516`, `:356`).
//!
//! Entry point: the clip is authored, written to a real `.tscn` by `TscnSaver`,
//! re-parsed by `PackedScene::from_tscn` and instanced with
//! `add_packed_scene_to_tree` (which is what rebuilds the player via
//! `attach_scene_animation_players`). A GDScript on the scene root calls
//! `$AnimationPlayer.play("attack")`, a second script counts the signals the
//! node emits, and `MainLoop::step` drives it. Every assertion is on what the
//! scene observes: the sprite's own `position`, the player node's
//! `current_animation` property, and script counters written by connected
//! handlers.

use gdcore::math::Vector2;
use gdscene::animation::{Animation, AnimationPlayer, AnimationTrack, KeyFrame, LoopMode};
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_saver::TscnSaver;
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::{NodeId, SceneTree};
use gdvariant::Variant;

/// 1/60s steps, and enough of them to clear a 0.2s clip.
const DT: f64 = 1.0 / 60.0;

/// Counts the three playback signals and records the `current_animation_changed`
/// names in order, so "the name on start, then the empty string on finish" is
/// one assertion rather than two counts.
///
/// `self.`-qualified writes copied from `animated_sprite_2d_backwards_and_signals`,
/// which is the green reference for this counter pattern.
const LISTENER_SCRIPT: &str = "\
extends Node
var started = 0
var finished = 0
var current_log = \"\"
func _on_started(anim):
    self.started = self.started + 1
func _on_finished(anim):
    self.finished = self.finished + 1
func _on_current_changed(anim):
    self.current_log = self.current_log + \"[\" + anim + \"]\"
";

/// Authors `Main -> {Sprite2D, AnimationPlayer}` with one clip driving the
/// sprite's `position`, then round-trips it through a real `.tscn`.
///
/// The `position` (not `position:x`) track shape and the save/load route are
/// copied from `animation_saves_into_tscn_roundtrip`, the green test that pins
/// this serialization path — so a failure here is about playback, not about
/// whether the clip survived the file.
fn scene_tscn(clip_name: &str, length: f64, loop_mode: LoopMode) -> String {
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let main = tree
        .add_child(root_id, Node::new("Main", "Node2D"))
        .expect("main");
    let sprite = tree
        .add_child(main, Node::new("Sprite2D", "Sprite2D"))
        .expect("sprite");
    tree.get_node_mut(sprite)
        .expect("sprite exists")
        .set_property("position", Variant::Vector2(Vector2::ZERO));
    let player_node = tree
        .add_child(main, Node::new("AnimationPlayer", "AnimationPlayer"))
        .expect("player node");

    let mut track = AnimationTrack::new("position");
    track.node_path = "Sprite2D".to_string();
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Vector2(Vector2::ZERO)));
    track.add_keyframe(KeyFrame::linear(
        length,
        Variant::Vector2(Vector2::new(100.0, 0.0)),
    ));

    let mut clip = Animation::new(clip_name, length);
    clip.loop_mode = loop_mode;
    clip.tracks.push(track);

    let mut player = AnimationPlayer::new();
    player.add_animation(clip);
    // ⚠️ No autoplay on purpose. An autoplaying clip would emit its own
    // `animation_started` before the script ever ran, and the "exactly twice"
    // count below would be measuring the scene load rather than the two
    // `play` calls under test.
    tree.attach_animation_player(player_node, player)
        .expect("attach");

    TscnSaver::save_tree(&tree, main)
}

struct Fixture {
    main_loop: MainLoop,
    main: NodeId,
    sprite: NodeId,
    player_node: NodeId,
    listener: NodeId,
}

/// Instances the saved `.tscn`, attaches `driver` to the scene root and the
/// listener to a sibling node, and wires the player's signals to it.
fn load(tscn: &str, driver: &str) -> Fixture {
    let scene = PackedScene::from_tscn(tscn).expect("the saved scene re-parses");
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let main = add_packed_scene_to_tree(&mut tree, root_id, &scene).expect("scene instances");

    let sprite = tree
        .get_node_or_null(main, "Sprite2D")
        .expect("Sprite2D survived the round-trip");
    let player_node = tree
        .get_node_or_null(main, "AnimationPlayer")
        .expect("AnimationPlayer survived the round-trip");
    // Guards the whole test: if the load did not rebuild the player, every
    // playback assertion below would fail for a reason that has nothing to do
    // with `play`.
    assert!(
        tree.get_animation_player(player_node).is_some(),
        "scene load must rebuild the AnimationPlayer from the saved libraries",
    );

    let script = GDScriptNodeInstance::from_source(driver, main).expect("driver script parses");
    tree.attach_script(main, Box::new(script));

    // The listener lives on its OWN node: a script is checked out of the arena
    // for the duration of its own call, so a handler on `main` could not run
    // while `main`'s `fire()` was the thing emitting.
    let listener = tree
        .add_child(root_id, Node::new("Listener", "Node"))
        .expect("listener");
    let ls = GDScriptNodeInstance::from_source(LISTENER_SCRIPT, listener)
        .expect("listener script parses");
    tree.attach_script(listener, Box::new(ls));
    for (signal, method) in [
        ("animation_started", "_on_started"),
        ("animation_finished", "_on_finished"),
        ("current_animation_changed", "_on_current_changed"),
    ] {
        tree.connect_signal(
            player_node,
            signal,
            gdobject::signal::Connection::new(listener.object_id(), method),
        );
    }

    let mut main_loop = MainLoop::new(tree);
    // `MainLoop::step` does not fire `_ready` for a script attached after
    // construction, and the `_process` driver below needs it to have run.
    main_loop.tree_mut().process_script_ready(main);
    Fixture {
        main_loop,
        main,
        sprite,
        player_node,
        listener,
    }
}

fn prop(ml: &MainLoop, id: NodeId, key: &str) -> Variant {
    ml.tree()
        .get_node(id)
        .expect("node exists")
        .get_property(key)
}

fn sprite_x(ml: &MainLoop, sprite: NodeId) -> f32 {
    match prop(ml, sprite, "position") {
        Variant::Vector2(v) => v.x,
        other => panic!("the sprite's position should be a Vector2, got {other:?}"),
    }
}

fn counter(ml: &MainLoop, id: NodeId, key: &str) -> i64 {
    match prop(ml, id, key) {
        Variant::Int(i) => i,
        other => panic!("{key} should be an Int, got {other:?}"),
    }
}

fn text(ml: &MainLoop, id: NodeId, key: &str) -> String {
    match prop(ml, id, key) {
        Variant::String(s) => s,
        other => panic!("{key} should be a String, got {other:?}"),
    }
}

/// **The headline acceptance.** A script firing `play("attack")` twice, with the
/// clip finishing in between, animates the sprite BOTH times.
#[test]
fn animation_player_replay_after_finish_script() {
    let tscn = scene_tscn("attack", 0.2, LoopMode::None);
    let mut fx = load(
        &tscn,
        "\
extends Node2D
func fire():
    $AnimationPlayer.play(\"attack\")
",
    );

    // -- first press: the clip runs to its end -------------------------------

    let at_rest = sprite_x(&fx.main_loop, fx.sprite);
    let _ = fx
        .main_loop
        .tree_mut()
        .call_script_method(fx.main, "fire", &[]);
    for _ in 0..20 {
        fx.main_loop.step(DT);
    }
    let after_first = sprite_x(&fx.main_loop, fx.sprite);
    assert!(
        after_first > 90.0,
        "the first play must animate the sprite to the clip's end value, got {after_first} \
         (started at {at_rest})",
    );
    assert_eq!(
        counter(&fx.main_loop, fx.listener, "finished"),
        1,
        "the non-looping clip must report finishing once",
    );

    // -- second press: THE REGRESSION. The playhead must rewind. -------------

    let _ = fx
        .main_loop
        .tree_mut()
        .call_script_method(fx.main, "fire", &[]);
    // One short step only: the sprite has to be back near the START of the
    // clip, which is the observable difference between a rewind and the old
    // "resume at the end, finish again immediately" behaviour. Asserting only
    // the final value would pass on the bug, because a clip stuck at its end
    // also reads 100.
    fx.main_loop.step(DT);
    let after_replay_start = sprite_x(&fx.main_loop, fx.sprite);
    assert!(
        after_replay_start < 50.0,
        "play() on a finished clip must rewind to the start; the sprite is at \
         {after_replay_start}, so the playhead never left the clip's end",
    );

    for _ in 0..20 {
        fx.main_loop.step(DT);
    }
    let after_second = sprite_x(&fx.main_loop, fx.sprite);
    assert!(
        after_second > 90.0,
        "the replayed clip must run to its end value again, got {after_second}",
    );

    // -- animation_started exactly twice: once per press ---------------------

    assert_eq!(
        counter(&fx.main_loop, fx.listener, "started"),
        2,
        "two play() calls on a clip that finished in between must report two starts",
    );
    assert_eq!(
        counter(&fx.main_loop, fx.listener, "finished"),
        2,
        "and the replayed clip must finish too, rather than hanging mid-flight",
    );

    // -- current_animation_changed: the name on start, "" on finish ----------

    assert_eq!(
        text(&fx.main_loop, fx.listener, "current_log"),
        "[attack][][]",
        "current_animation_changed must carry the clip name once (the first \
         switch) and the empty string at each finish",
    );
    // Godot keeps `playback.assigned` after a finish, which is exactly what
    // lets the replay above recognise the clip as already-current.
    assert_eq!(
        prop(&fx.main_loop, fx.player_node, "current_animation"),
        Variant::String("attack".to_string()),
        "the node's current_animation stays assigned after the clip finishes",
    );
}

/// A `play()` on every frame of `_process` — the other half of Godot's
/// same-clip branch. While the clip is still running `_play` returns silently,
/// so the scene sees ONE `animation_started`, not one per frame.
#[test]
fn per_frame_play_on_a_looping_clip_starts_once() {
    let tscn = scene_tscn("walk", 1.0, LoopMode::Linear);
    let mut fx = load(
        &tscn,
        "\
extends Node2D
func _process(delta):
    $AnimationPlayer.play(\"walk\")
",
    );

    for _ in 0..30 {
        fx.main_loop.step(DT);
    }

    assert_eq!(
        counter(&fx.main_loop, fx.listener, "started"),
        1,
        "a per-frame play() must start the clip once, not once per frame",
    );
    assert_eq!(
        counter(&fx.main_loop, fx.listener, "finished"),
        0,
        "a looping clip must not report finishing",
    );
    // And it has to actually be animating — a `play` that returned early
    // WITHOUT ever having started the clip would also report one start.
    let moved = sprite_x(&fx.main_loop, fx.sprite);
    assert!(
        moved > 0.0,
        "the looping clip must still drive the sprite, got {moved}",
    );
}

/// An explicit `play` clears the queue (`animation_player.cpp:509`), while the
/// finish path draining that queue does not — and the queue advance is the one
/// place `animation_changed(old, new)` is emitted (`:348-350`).
#[test]
fn explicit_play_clears_the_queue_but_the_finish_path_does_not() {
    let mut p = AnimationPlayer::new();
    p.add_animation(Animation::new("a", 0.2));
    p.add_animation(Animation::new("b", 0.2));

    // A queue entry survives until something explicitly plays again.
    p.play("a");
    p.queue("b");
    assert_eq!(p.get_queue(), vec!["b".to_string()]);
    p.play("a");
    assert!(
        p.get_queue().is_empty(),
        "an explicit play() must clear the queue",
    );

    // ...but the finish path must keep it, or it would clear the very list it
    // is walking and the queued clip would never run.
    p.play("a");
    p.queue("b");
    let _ = p.drain_signals();
    p.advance(0.5);
    assert_eq!(
        p.current_animation(),
        Some("b"),
        "the queued clip must take over when the current one finishes",
    );
    let sigs = p.drain_signals();
    assert!(
        sigs.contains(
            &gdscene::animation::AnimationPlayerSignal::AnimationChanged {
                old: "a".to_string(),
                new: "b".to_string(),
            }
        ),
        "animation_changed(old, new) fires on the queue advancing: {sigs:?}",
    );
}

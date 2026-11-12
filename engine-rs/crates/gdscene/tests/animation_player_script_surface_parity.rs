//! pat-mgixk.79 acceptance: the AnimationPlayer script surface, driven from a
//! GDScript in a loaded scene.
//!
//! # What was wrong
//!
//! `interpreter.rs`'s `play` arm read `args[0]` only and called
//! `player.play(name)`, so `custom_blend`, `custom_speed` and `from_end` were
//! dropped on the floor even though `AnimationPlayer::play_ex` implements all
//! three. More than twenty AnimationPlayer/AnimationMixer methods had no
//! dispatch arm at all (`advance`, `get_queue`, `clear_queue`,
//! `animation_set_next`, `set_blend_time`, `get_current_animation_length`,
//! `set_current_animation`, …), and `set_node_property` never routed a write
//! into the player struct — so `$AP.speed_scale = 2.0` and
//! `$AP.current_animation = "run"` landed in the node's property bag, which the
//! player never reads, and were silent no-ops.
//!
//! # The path driven
//!
//! ```text
//! .tscn -> PackedScene::from_tscn -> add_packed_scene_to_tree
//!   -> GDScriptNodeInstance::from_source + attach_script
//!     -> interpreter -> SceneTreeAccessor -> SceneTree::animation_players
//!       -> MainLoop::step -> process_animations -> the Sprite2D's position
//! ```
//!
//! Every clause is asserted on what a GAME observes: the sprite the animation
//! targets, or the player state a following script line would read. The scene
//! is built in Rust and round-tripped through a real `.tscn` via `TscnSaver`,
//! the route `animation_saves_into_tscn_roundtrip` already pins green — so a
//! failure here is about the script surface, not about whether the clip
//! survived the file.
//!
//! ⚠️ Every speed clause is DIFFERENTIAL — two runs of the same fixture that
//! differ in one argument, compared against each other. An absolute "x is about
//! 20 after 6 frames" would encode this renderer's exact sampling and
//! interpolation convention, and would break whenever that convention changed
//! rather than when playback broke. The `custom_blend` is identical in both arms
//! of the speed comparison, so any blend ramp cancels common-mode.

use gdcore::math::Vector2;
use gdscene::animation::{Animation, AnimationPlayer, AnimationTrack, KeyFrame, LoopMode};
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_saver::TscnSaver;
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::{NodeId, SceneTree};
use gdvariant::Variant;

const DT: f64 = 1.0 / 60.0;

/// Both clips are one second long, so `get_current_animation_length()` has an
/// exact expected value and `advance(0.25)` lands at a quarter of the travel.
const CLIP_LEN: f64 = 1.0;

/// End value of each clip's track. 100 at 1.0 s means "seconds elapsed times
/// 100" is the expected coordinate, which keeps every bound below readable.
const TRAVEL: f32 = 100.0;

/// `AnimationCallbackMode::Manual` as the `.tscn` integer
/// (`animation.rs:716` maps `2 => Manual`).
const CALLBACK_MODE_MANUAL: i64 = 2;

/// `Main -> {Sprite2D, AP}` with TWO clips, round-tripped through a real `.tscn`.
///
/// 🔑 `walk` moves the sprite in **X** and `run` moves it in **Y**. That is what
/// makes the `current_animation = "run"` clause falsifiable: a write that was
/// swallowed, or one that re-started `walk`, moves X and leaves Y at zero, which
/// is a different observation from the one asserted. Two clips that both moved X
/// would be indistinguishable.
fn scene_tscn() -> String {
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
        .add_child(main, Node::new("AP", "AnimationPlayer"))
        .expect("player node");

    let mut player = AnimationPlayer::new();
    for (name, end) in [
        ("walk", Vector2::new(TRAVEL, 0.0)),
        ("run", Vector2::new(0.0, TRAVEL)),
    ] {
        let mut track = AnimationTrack::new("position");
        track.node_path = "Sprite2D".to_string();
        track.add_keyframe(KeyFrame::linear(0.0, Variant::Vector2(Vector2::ZERO)));
        track.add_keyframe(KeyFrame::linear(CLIP_LEN, Variant::Vector2(end)));
        let mut clip = Animation::new(name, CLIP_LEN);
        // ⚠️ `None`, not `Linear`: a looping clip wraps past its end, so the
        // "played to completion" step loop below could read a position from the
        // NEXT lap and the `animation_set_next` chain would never fire.
        clip.loop_mode = LoopMode::None;
        clip.tracks.push(track);
        player.add_animation(clip);
    }
    // ⚠️ No autoplay: an autoplaying clip would already be advancing before the
    // first script line ran, and every clause below would be measuring the
    // scene load rather than the call under test.
    tree.attach_animation_player(player_node, player)
        .expect("attach");

    TscnSaver::save_tree(&tree, main)
}

struct Fixture {
    main_loop: MainLoop,
    main: NodeId,
    sprite: NodeId,
    player_node: NodeId,
}

/// Instances the saved `.tscn` and attaches `driver` to the scene root.
///
/// `manual_mode` writes `callback_mode_process` on the LOADED node rather than
/// into the saved `.tscn`: the clause is about the loop honouring the mode, and
/// going through the file would also be testing whether `TscnSaver` persists
/// that property — a separate concern with its own tests.
fn load(tscn: &str, driver: &str, manual_mode: bool) -> Fixture {
    let scene = PackedScene::from_tscn(tscn).expect("the saved scene re-parses");
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let main = add_packed_scene_to_tree(&mut tree, root_id, &scene).expect("scene instances");

    let sprite = tree
        .get_node_or_null(main, "Sprite2D")
        .expect("Sprite2D survived the round-trip");
    let player_node = tree
        .get_node_or_null(main, "AP")
        .expect("AP survived the round-trip");
    // Guards every clause below: if the load did not rebuild the player, each
    // assertion would fail for a reason that has nothing to do with the script
    // surface.
    let rebuilt = tree
        .get_animation_player(player_node)
        .expect("scene load must rebuild the AnimationPlayer from the saved libraries");
    // ⚠️ BOTH clips, named. The green round-trip reference
    // (`animation_saves_into_tscn_roundtrip`) pins a SINGLE clip, so "two clips
    // survive the file" is not something this fixture may assume. Losing `run`
    // would make the current_animation clause fail as if the property write were
    // broken, which is a different defect from the one under test.
    for clip in ["walk", "run"] {
        assert!(
            rebuilt.animations.contains_key(clip),
            "fixture precondition: the saved .tscn must round-trip the `{clip}` \
             clip; the reloaded player holds {:?}",
            rebuilt.animations.keys().collect::<Vec<_>>(),
        );
    }
    if manual_mode {
        tree.get_node_mut(player_node)
            .expect("player node exists")
            .set_property("callback_mode_process", Variant::Int(CALLBACK_MODE_MANUAL));
    }

    let script = GDScriptNodeInstance::from_source(driver, main).expect("driver script parses");
    tree.attach_script(main, Box::new(script));

    let mut main_loop = MainLoop::new(tree);
    // `MainLoop::step` does not fire `_ready` for a script attached after
    // construction, and the driver methods below are called explicitly.
    main_loop.tree_mut().process_script_ready(main);
    Fixture {
        main_loop,
        main,
        sprite,
        player_node,
    }
}

fn sprite_pos(fx: &Fixture) -> Vector2 {
    match fx
        .main_loop
        .tree()
        .get_node(fx.sprite)
        .expect("sprite exists")
        .get_property("position")
    {
        Variant::Vector2(v) => v,
        other => panic!("the Sprite2D's position should be a Vector2, got {other:?}"),
    }
}

/// A script variable on the driver, so a value the script READ BACK from the
/// player can be asserted on.
fn script_var(fx: &Fixture, key: &str) -> Variant {
    fx.main_loop
        .tree()
        .get_node(fx.main)
        .expect("main exists")
        .get_property(key)
}

fn call(fx: &mut Fixture, method: &str) {
    let _ = fx
        .main_loop
        .tree_mut()
        .call_script_method(fx.main, method, &[]);
}

fn step(fx: &mut Fixture, frames: usize) {
    for _ in 0..frames {
        fx.main_loop.step(DT);
    }
}

/// Frames worth 0.2 s — a fifth of the clip, so neither arm of a 2x speed
/// comparison can reach the clip's end and saturate.
const SHORT_FRAMES: usize = 12;

/// **THE ACCEPTANCE TEST (pat-mgixk.79).**
///
/// Covers four of the bead's five clauses; the MANUAL/`advance` clause needs a
/// differently-configured scene and is the test below.
#[test]
fn animation_player_script_surface_parity() {
    let tscn = scene_tscn();

    // -- (1) play()'s custom_speed reaches the player ------------------------
    //
    // Same call, same custom_blend, one argument different. The blend ramp (if
    // any) is identical in both arms, so it cancels.
    let mut fast = load(
        &tscn,
        "extends Node2D\nfunc go():\n\t$AP.play(\"walk\", 0.3, 2.0)\n",
        false,
    );
    let mut slow = load(
        &tscn,
        "extends Node2D\nfunc go():\n\t$AP.play(\"walk\", 0.3, 1.0)\n",
        false,
    );
    call(&mut fast, "go");
    call(&mut slow, "go");
    step(&mut fast, SHORT_FRAMES);
    step(&mut slow, SHORT_FRAMES);
    let (fx_x, sx_x) = (sprite_pos(&fast).x, sprite_pos(&slow).x);
    assert!(
        sx_x > 1.0,
        "fixture precondition: the 1.0x arm must be animating at all, got x={sx_x}. \
         Zero means `play` never reached the player and the ratio below is vacuous",
    );
    assert!(
        fx_x > sx_x * 1.6,
        "play(\"walk\", 0.3, 2.0) must run at DOUBLE speed: after {SHORT_FRAMES} \
         frames the 2.0x arm is at x={fx_x} and the 1.0x arm at x={sx_x}, a ratio \
         of {:.2}. A ratio near 1.0 means `custom_speed` was dropped — the \
         pre-bead behaviour, where the interpreter passed only args[0]",
        fx_x / sx_x,
    );

    // -- (2) a speed_scale property write reaches the player -----------------
    //
    // ⚠️ Assigned AFTER `play`, which is the order that used to fail silently:
    // the write landed in the node's property bag and `advance` kept scaling by
    // the struct's untouched 1.0.
    let mut half = load(
        &tscn,
        "extends Node2D\nfunc go():\n\t$AP.play(\"walk\")\n\t$AP.speed_scale = 0.5\n",
        false,
    );
    let mut full = load(
        &tscn,
        "extends Node2D\nfunc go():\n\t$AP.play(\"walk\")\n",
        false,
    );
    call(&mut half, "go");
    call(&mut full, "go");
    step(&mut half, SHORT_FRAMES);
    step(&mut full, SHORT_FRAMES);
    let (h_x, f_x) = (sprite_pos(&half).x, sprite_pos(&full).x);
    assert!(
        h_x > 0.5,
        "speed_scale = 0.5 must HALVE the advance, not stop it: got x={h_x}. \
         A frozen sprite means the write was coerced to 0.0 rather than 0.5",
    );
    assert!(
        h_x < f_x * 0.75,
        "speed_scale = 0.5 must halve the advance: the scaled arm is at x={h_x} \
         and the unscaled arm at x={f_x}, a ratio of {:.2}. A ratio near 1.0 is \
         the pre-bead behaviour — the write reached the node's property bag, \
         which the player never reads",
        h_x / f_x,
    );

    // -- (3) current_animation = "run" STARTS run ----------------------------
    //
    // 🔑 `run` moves the sprite in Y. So "the write started run" is observable
    // as Y motion, and the two failure modes are distinguishable: a swallowed
    // write (or one that merely relabelled) leaves `walk` running and moves X.
    let mut switched = load(
        &tscn,
        "extends Node2D\nfunc go():\n\t$AP.play(\"walk\")\n\t$AP.current_animation = \"run\"\n",
        false,
    );
    call(&mut switched, "go");
    step(&mut switched, SHORT_FRAMES);
    let p = sprite_pos(&switched);
    assert_eq!(
        switched
            .main_loop
            .tree()
            .get_animation_player(switched.player_node)
            .expect("player attached")
            .current_animation(),
        Some("run"),
        "Godot's `current_animation` setter STARTS the clip, so the player's \
         current animation must be `run` on the next line",
    );
    assert!(
        p.y > 1.0,
        "setting current_animation = \"run\" must start the run clip, which \
         moves the sprite in Y; got y={} (x={}). A y of 0 with a non-zero x \
         means `walk` kept playing and the write was a no-op",
        p.y,
        p.x,
    );

    // -- (5) the query surface returns Godot values --------------------------
    //
    // The script CALLS each method (which is what proves dispatch exists — an
    // absent arm raises UndefinedFunction) and stores what it read back into a
    // script variable, so the assertion is on the value a following script line
    // would see, not on the engine struct.
    let mut q = load(
        &tscn,
        "extends Node2D\n\
         var qlen = 0.0\n\
         var queued = []\n\
         func go():\n\
         \t$AP.play(\"walk\")\n\
         \t$AP.animation_set_next(\"walk\", \"run\")\n\
         \t$AP.queue(\"run\")\n\
         \tself.qlen = $AP.get_current_animation_length()\n\
         \tself.queued = $AP.get_queue()\n",
        false,
    );
    call(&mut q, "go");
    match script_var(&q, "qlen") {
        Variant::Float(len) => assert!(
            (len - CLIP_LEN).abs() < 1e-6,
            "get_current_animation_length() must return the clip's {CLIP_LEN}s \
             length, got {len}",
        ),
        other => panic!(
            "get_current_animation_length() must return a Float; got {other:?}. \
             Nil means the method has no dispatch arm"
        ),
    }
    match script_var(&q, "queued") {
        Variant::PackedStringArray(names) => assert_eq!(
            names,
            vec!["run".to_string()],
            "get_queue() must return the queued clip names",
        ),
        other => panic!(
            "get_queue() must return a PackedStringArray; got {other:?}. Nil \
             means the method has no dispatch arm"
        ),
    }
    // `animation_set_next` returns void in Godot, so its EFFECT is the
    // assertion — but it needs its OWN fixture.
    //
    // ⚠️ NOT asserted on the fixture above. That one calls `queue("run")` to
    // give `get_queue()` something to return, and a queued clip ALSO starts
    // after walk ends — so "the player is playing run" there is satisfied by
    // either mechanism and would prove nothing about the chain. One cause per
    // experiment: this fixture sets the next-animation link and never queues.
    let mut chain = load(
        &tscn,
        "extends Node2D\n\
         func go():\n\
         \t$AP.animation_set_next(\"walk\", \"run\")\n\
         \t$AP.play(\"walk\")\n",
        false,
    );
    call(&mut chain, "go");
    // 90 frames = 1.5 s at 1.0x, so the 1.0 s walk is comfortably finished.
    step(&mut chain, 90);
    assert_eq!(
        chain
            .main_loop
            .tree()
            .get_animation_player(chain.player_node)
            .expect("player attached")
            .current_animation(),
        Some("run"),
        "animation_set_next(\"walk\", \"run\") must chain: once walk reaches its \
         end the player continues into run, with nothing queued",
    );
}

/// **The MANUAL/`advance` clause (pat-mgixk.79).**
///
/// 🔑 Two halves, and the first is what makes the second mean anything. With
/// `callback_mode_process = MANUAL` the loop must NOT advance the player — if it
/// did, the sprite would already be moving and "advance(0.25) moved it" could
/// not be told apart from "the loop moved it". Only then does the explicit
/// `advance` call have to produce the 0.25 s sample.
#[test]
fn manual_mode_advances_only_on_an_explicit_advance_call() {
    let tscn = scene_tscn();
    let mut fx = load(
        &tscn,
        "extends Node2D\n\
         func go():\n\
         \t$AP.play(\"walk\")\n\
         func tick():\n\
         \t$AP.advance(0.25)\n",
        true,
    );
    call(&mut fx, "go");

    // -- the loop leaves a MANUAL player alone ------------------------------
    step(&mut fx, 30);
    let idle = sprite_pos(&fx);
    assert!(
        idle.x.abs() < 1.0,
        "a MANUAL-mode player must not be advanced by MainLoop::step; the \
         sprite moved to x={} over 30 frames. Without this the advance \
         assertion below cannot distinguish the explicit call from the loop",
        idle.x,
    );

    // -- and an explicit advance(0.25) samples a quarter in ------------------
    call(&mut fx, "tick");
    let advanced = sprite_pos(&fx);
    let expected = TRAVEL * 0.25;
    assert!(
        (advanced.x - expected).abs() < TRAVEL * 0.1,
        "advance(0.25) must move the target property to the 0.25s sample of a \
         {CLIP_LEN}s clip travelling {TRAVEL}, i.e. about {expected}; got x={}. \
         A 0 means `advance` has no dispatch arm, or it advanced the player \
         without running the apply pass that writes the track onto the node",
        advanced.x,
    );
}

//! pat-s3rrv.182 — `AnimatedSprite2D` plays clips backwards, and the two
//! signals that were declared in ClassDB and never emitted now fire.
//!
//! Ground truth: `upstream/godot/scene/2d/animated_sprite_2d.cpp`.
//! `play_backwards(name)` is literally `play(name, -1, true)` (`:520-522`), and
//! `pause()` / `stop()` are `_stop_internal(false)` / `(true)` (`:524-540`) —
//! the same call differing only in whether the playhead resets.
//!
//! Everything here drives a **real scene through `MainLoop`**, calling the
//! methods from GDScript the way a game does. `process_animated_sprites`
//! rebuilds its `AnimatedSpritePlayback` from the node's properties on every
//! tick, so a test that drove a `AnimatedSpritePlayback` directly would prove
//! the struct works and nothing at all about what a scene does.

use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Four distinct frames, so a reversed walk is unambiguous and the published
/// texture identifies the frame on its own.
const FRAMES: [&str; 4] = [
    "res://f0.png",
    "res://f1.png",
    "res://f2.png",
    "res://f3.png",
];

/// A fifth of a frame. 10fps makes a frame last exactly 0.1s, and **any** step
/// size at or above that eventually advances two frames in one tick as the
/// remainder drifts — a `[3, 2, 1, 0]` walk asserted tick-by-tick would be
/// pinned to arithmetic that has nothing to do with the feature.
///
/// So the tests sample `frame` after every sub-frame step and collapse runs of
/// equal values. What they then assert is the *shape* of the walk — each frame
/// is one below the last, or a wrap from 0 to the last frame — which is exactly
/// what the bead claims and is independent of how many ticks each frame takes.
const SUB: f64 = 0.02;

/// Consecutive-duplicate removal: the ordered list of frames actually visited.
fn visited(samples: &[i64]) -> Vec<i64> {
    let mut out: Vec<i64> = Vec::new();
    for &f in samples {
        if out.last() != Some(&f) {
            out.push(f);
        }
    }
    out
}

/// Steps `count` sub-frame slices, sampling `frame` after each — and checking on
/// every single sample that the published `texture` still matches the frame.
///
/// ⚠️ The texture check belongs *here*, per sample, not as a spot check at the
/// end. `texture` is the property every 2D render path reads; a `frame` that
/// counts down correctly while `texture` lags by a tick is a sprite that
/// animates in the inspector and not on screen.
fn walk(ml: &mut MainLoop, anim: NodeId, count: usize) -> Vec<i64> {
    let mut samples = Vec::new();
    for _ in 0..count {
        ml.step(SUB);
        let f = frame_of(ml, anim);
        assert_eq!(
            texture_of(ml, anim),
            FRAMES[f as usize],
            "the published texture must match frame {f} on every tick"
        );
        samples.push(f);
    }
    visited(&samples)
}

/// Asserts `steps` is a strictly descending walk that wraps 0 -> last, and
/// returns how many times it wrapped.
fn assert_descends_with_wraps(steps: &[i64], last: i64) -> i64 {
    let mut wraps = 0;
    for pair in steps.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if a == 0 && b == last {
            wraps += 1;
        } else {
            assert_eq!(
                b,
                a - 1,
                "a reversed clip goes one frame DOWN or wraps 0 -> {last}; \
                 {a} -> {b} is neither (full walk {steps:?})"
            );
        }
    }
    wraps
}

/// The companion keys a `.tscn`'s flattened `SpriteFrames` sub-resource produces.
fn animated_node(loops: bool) -> Node {
    let mut node = Node::new("Anim", "AnimatedSprite2D");
    // ⚠️ TWO clips. `animation_changed` fires on a switch between them, and a
    // one-clip fixture cannot express a switch at all — the signal would be
    // untestable and would stay as it was before this bead: declared in ClassDB
    // and never emitted.
    node.set_property(
        "__sf_anims__",
        Variant::PackedStringArray(vec!["walk".to_string(), "idle".to_string()]),
    );
    node.set_property("__sf_idle_speed", Variant::Float(10.0));
    node.set_property("__sf_idle_loop", Variant::Bool(true));
    node.set_property(
        "__sf_idle_frames",
        Variant::PackedStringArray(vec![FRAMES[0].to_string(), FRAMES[1].to_string()]),
    );
    node.set_property("__sf_walk_speed", Variant::Float(10.0));
    node.set_property("__sf_walk_loop", Variant::Bool(loops));
    node.set_property(
        "__sf_walk_frames",
        Variant::PackedStringArray(FRAMES.iter().map(|s| s.to_string()).collect()),
    );
    node.set_property("animation", Variant::String("walk".to_string()));
    node
}

/// `Root -> {Anim (scripted AnimatedSprite2D), Listener (signal counters)}`.
///
/// ⚠️ `process_script_ready` is called explicitly: `MainLoop::step` alone does
/// not fire `_ready` for a script attached after construction, and without it
/// the `play_backwards` under test would simply never run.
fn scene(src: &str, loops: bool) -> (MainLoop, NodeId, NodeId) {
    scene_with(src, loops, false)
}

fn scene_with(src: &str, loops: bool, autoplay: bool) -> (MainLoop, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut sprite = animated_node(loops);
    if autoplay {
        sprite.set_property("autoplay", Variant::String("walk".to_string()));
    }
    let anim = tree.add_child(root, sprite).expect("sprite");
    let script = GDScriptNodeInstance::from_source(src, anim).expect("sprite script parses");
    tree.attach_script(anim, Box::new(script));

    let listener = tree
        .add_child(root, Node::new("Listener", "Node"))
        .expect("listener");
    let lsrc = "\
extends Node
var looped = 0
var changed = 0
var finished = 0
func _on_looped():
    self.looped = self.looped + 1
func _on_changed():
    self.changed = self.changed + 1
func _on_finished():
    self.finished = self.finished + 1
";
    let ls = GDScriptNodeInstance::from_source(lsrc, listener).expect("listener parses");
    tree.attach_script(listener, Box::new(ls));
    for (signal, method) in [
        ("animation_looped", "_on_looped"),
        ("animation_changed", "_on_changed"),
        ("animation_finished", "_on_finished"),
    ] {
        tree.connect_signal(
            anim,
            signal,
            gdobject::signal::Connection::new(listener.object_id(), method),
        );
    }

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(anim);
    (ml, anim, listener)
}

fn prop(ml: &MainLoop, id: NodeId, key: &str) -> Variant {
    ml.tree()
        .get_node(id)
        .expect("node exists")
        .get_property(key)
}

fn frame_of(ml: &MainLoop, id: NodeId) -> i64 {
    match prop(ml, id, "frame") {
        Variant::Int(i) => i,
        other => panic!("frame should be an Int, got {other:?}"),
    }
}

fn progress_of(ml: &MainLoop, id: NodeId) -> f64 {
    match prop(ml, id, "frame_progress") {
        Variant::Float(f) => f,
        other => panic!("frame_progress should be a Float, got {other:?}"),
    }
}

fn texture_of(ml: &MainLoop, id: NodeId) -> String {
    match prop(ml, id, "texture") {
        Variant::String(s) => s,
        other => panic!("texture should be a String path, got {other:?}"),
    }
}

fn script_int(ml: &MainLoop, id: NodeId, name: &str) -> i64 {
    match ml
        .tree()
        .get_script(id)
        .expect("script attached")
        .get_property(name)
    {
        Some(Variant::Int(i)) => i,
        other => panic!("{name} should be an int, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// The bead's acceptance
// ---------------------------------------------------------------------------

/// 🔑 **THE ACCEPTANCE.** A scripted `play_backwards("walk")` in `_ready()`
/// walks `frame` strictly DOWN across successive `MainLoop::step` calls, wraps
/// 0 -> last on a looping clip, publishes the reversed frame's texture every
/// tick, and emits `animation_looped` exactly once per wrap.
#[test]
fn animated_sprite_2d_backwards_and_signals() {
    let src = "\
extends AnimatedSprite2D
func _ready():
    play_backwards(\"walk\")
";
    let (mut ml, anim, listener) = scene(src, true);

    // `from_end`: the playhead starts on the LAST frame, not frame 0. Reversing
    // the speed alone would leave it at 0 — already at the reverse end, where a
    // non-looping clip would finish without ever drawing.
    assert_eq!(
        frame_of(&ml, anim),
        3,
        "play_backwards starts at the clip's last frame"
    );
    assert!(
        matches!(prop(&ml, anim, "playing"), Variant::Bool(true)),
        "and starts it playing"
    );

    // 1.6s at 10fps. ⚠️ Deliberately far more than the eight advances two wraps
    // need: `0.02f32` is a shade under a fifth of `0.1f32`, so a frame takes
    // five sub-steps or six depending on the residual. The assertions below are
    // all shape-based precisely so that wobble cannot decide the result — the
    // generous count just guarantees enough laps to see the wrap repeat.
    let steps = walk(&mut ml, anim, 80);
    assert_eq!(steps[0], 3, "the walk begins on the last frame");
    assert!(
        steps.len() >= 6,
        "the clip must actually move for this to mean anything, got {steps:?}"
    );

    let wraps = assert_descends_with_wraps(&steps, 3);
    assert!(
        wraps >= 2,
        "1.6s should cover at least two wraps, got {wraps}: {steps:?}"
    );

    // ⚠️ An exact count, not `>= 1`. `animation_looped` is emitted off a wrap
    // EDGE; `>= 1` would pass just as happily on an implementation that fired it
    // on every tick after the first wrap.
    assert_eq!(
        script_int(&ml, listener, "looped"),
        wraps,
        "exactly one animation_looped per wrap — the walk wrapped {wraps} times"
    );

    // ⚠️ A looping clip never finishes. Godot makes these mutually exclusive
    // (`animated_sprite_2d.cpp:45-46`, `:69-70`); emitting both would make every
    // looping animation look like it had ended on its first lap.
    assert_eq!(
        script_int(&ml, listener, "finished"),
        0,
        "a looping clip must never emit animation_finished"
    );
}

/// ⚠️ **The control for the whole file.** The same scene played FORWARDS still
/// counts up and wraps last -> 0.
///
/// Every assertion above is about a reversed clip, and all of them would also
/// pass on a build that had broken forward playback while adding the reverse
/// path.
///
#[test]
fn forward_playback_is_unchanged() {
    let src = "\
extends AnimatedSprite2D
func _ready():
    play(\"walk\")
";
    let (mut ml, anim, listener) = scene(src, true);

    let steps = walk(&mut ml, anim, 80);
    assert_eq!(steps[0], 0, "a forward play starts on frame 0");

    let mut wraps = 0;
    for pair in steps.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if a == 3 && b == 0 {
            wraps += 1;
        } else {
            assert_eq!(
                b,
                a + 1,
                "forward playback goes one frame UP or wraps 3 -> 0; {a} -> {b} \
                 is neither (full walk {steps:?})"
            );
        }
    }
    assert!(
        wraps >= 2,
        "1.6s should cover at least two forward wraps, got {wraps}"
    );
    assert_eq!(
        script_int(&ml, listener, "looped"),
        wraps,
        "a forward wrap emits animation_looped too — the signal is not \
         reverse-only"
    );
}

/// ⚠️ `play_backwards(name)` **is** `play(name, -1, true)` in Godot
/// (`animated_sprite_2d.cpp:520-522`). Asserting they produce the same walk is
/// what stops the convenience wrapper drifting from the general method — a
/// divergence no single-method test could see, because each would be measured
/// only against itself.
#[test]
fn play_with_negative_speed_from_end_equals_play_backwards() {
    let long_way = "\
extends AnimatedSprite2D
func _ready():
    play(\"walk\", -1, true)
";
    let short_way = "\
extends AnimatedSprite2D
func _ready():
    play_backwards(\"walk\")
";
    let (mut a, anim_a, _) = scene(long_way, true);
    let (mut b, anim_b, _) = scene(short_way, true);

    assert_eq!(
        frame_of(&a, anim_a),
        frame_of(&b, anim_b),
        "both spellings start on the same frame"
    );
    let wa = walk(&mut a, anim_a, 80);
    let wb = walk(&mut b, anim_b, 80);
    assert_eq!(wa[0], 3, "and both start at the clip end");
    assert_eq!(
        wa, wb,
        "play(name, -1, true) must be indistinguishable from play_backwards(name)"
    );
}

/// The `.tscn` `autoplay` route still starts a clip without any script call.
///
/// ⚠️ The `play` arm added for this bead sits ABOVE a catch-all `"play"` arm in
/// the interpreter, and autoplay is the one path that reaches the sprite without
/// going through method dispatch at all — so it is the check that the new arm
/// did not disturb how scenes start themselves.
#[test]
fn autoplay_still_starts_a_clip_with_no_script_call() {
    let src = "\
extends AnimatedSprite2D
func _ready():
    pass
";
    let (mut ml, anim, _listener) = scene_with(src, true, true);

    let steps = walk(&mut ml, anim, 30);
    assert_eq!(steps[0], 0, "autoplay starts on frame 0");
    assert!(
        steps.len() >= 3,
        "autoplay must actually animate, got {steps:?}"
    );
}

// ---------------------------------------------------------------------------
// pause / stop
// ---------------------------------------------------------------------------

/// `pause()` freezes the playhead exactly where it is.
///
/// ⚠️ The `ticks` and "it moved first" assertions are the positive control.
/// "the frame stopped changing" is satisfied by a great many bugs — `_process`
/// never dispatching, `play_backwards` never running, the clip having no frames
/// — so without them this would pass on a sprite that never animated at all.
#[test]
fn pause_freezes_the_playhead_where_it_is() {
    let src = "\
extends AnimatedSprite2D
var ticks = 0
func _ready():
    play_backwards(\"walk\")
func _process(delta):
    self.ticks = self.ticks + 1
    if self.ticks == 8:
        pause()
";
    let (mut ml, anim, _listener) = scene(src, true);

    // 8 sub-steps is 0.16s — one full frame back, and solidly mid-interval so
    // the answer does not depend on whether `_process` runs before or after the
    // playback phase within a tick.
    for _ in 0..8 {
        ml.step(SUB);
    }
    let paused_at = frame_of(&ml, anim);
    assert_eq!(paused_at, 2, "it really was animating before the pause");
    assert!(
        matches!(prop(&ml, anim, "playing"), Variant::Bool(false)),
        "pause() clears `playing`"
    );

    // Long after: nothing moves.
    for _ in 0..40 {
        ml.step(SUB);
    }
    assert!(
        script_int(&ml, anim, "ticks") > 40,
        "_process kept being dispatched — otherwise 'nothing moved' proves nothing"
    );
    assert_eq!(
        frame_of(&ml, anim),
        paused_at,
        "a paused sprite must not advance, however many frames pass"
    );
    // ⚠️ Godot keeps the playhead on pause. Rewinding is `stop()`'s job, and
    // collapsing the two loses the ability to resume mid-clip.
    assert_ne!(
        paused_at, 0,
        "pause must freeze mid-clip, NOT rewind to the start like stop()"
    );
}

/// `stop()` freezes AND rewinds — the half that distinguishes it from `pause()`.
#[test]
fn stop_rewinds_to_the_clip_start() {
    let src = "\
extends AnimatedSprite2D
var ticks = 0
func _ready():
    play_backwards(\"walk\")
func _process(delta):
    self.ticks = self.ticks + 1
    if self.ticks == 8:
        stop()
";
    let (mut ml, anim, _listener) = scene(src, true);

    for _ in 0..7 {
        ml.step(SUB);
    }
    assert_eq!(frame_of(&ml, anim), 2, "animating before the stop");

    for _ in 0..40 {
        ml.step(SUB);
    }
    assert!(
        matches!(prop(&ml, anim, "playing"), Variant::Bool(false)),
        "stop() clears `playing`"
    );
    assert_eq!(
        frame_of(&ml, anim),
        0,
        "stop() rewinds to the clip start and stays there"
    );
    // ⚠️ Cleared by `stop()` itself, not by the next tick: the playback phase
    // skips a node whose `playing` is false, so nothing would ever recompute it.
    assert_eq!(
        progress_of(&ml, anim),
        0.0,
        "and clears the sub-frame progress with it"
    );
}

// ---------------------------------------------------------------------------
// frame_progress
// ---------------------------------------------------------------------------

/// `set_frame_and_progress(2, 0.5)` is reflected in BOTH properties.
#[test]
fn set_frame_and_progress_writes_both_properties() {
    let src = "\
extends AnimatedSprite2D
var ticks = 0
func _ready():
    play_backwards(\"walk\")
    pause()
func _process(delta):
    self.ticks = self.ticks + 1
    if self.ticks == 1:
        set_frame_and_progress(2, 0.5)
";
    let (mut ml, anim, _listener) = scene(src, true);

    ml.step(SUB);
    assert_eq!(
        script_int(&ml, anim, "ticks"),
        1,
        "the setter was actually called"
    );
    assert_eq!(frame_of(&ml, anim), 2, "the frame half of the pair");
    assert_eq!(
        progress_of(&ml, anim),
        0.5,
        "and the sub-frame progress half — a setter that writes only `frame` \
         leaves the playhead wherever the last tick left it"
    );
}

/// ⚠️ `frame_progress` stays a real 0..1 fraction while running BACKWARDS.
///
/// A reversed clip stores its sub-frame position as a *negative* `__sf_time` in
/// `(-hold, 0]`. The reader divides that by the frame duration and clamps to
/// `0.0..1.0`, which — without a direction term — pins `frame_progress` at
/// exactly **0.0** for the whole of backwards playback, the one mode this bead
/// exists to add. Godot keeps the fraction meaningful in both directions and
/// starts a `from_end` play at 1.0 (`animated_sprite_2d.cpp:520-522`).
#[test]
fn reverse_progress_is_a_real_fraction_not_a_clamped_zero() {
    let src = "\
extends AnimatedSprite2D
func _ready():
    play_backwards(\"walk\")
";
    let (mut ml, anim, _listener) = scene(src, true);

    // One sub-step: a fifth of the way back through frame 3.
    ml.step(SUB);
    assert_eq!(frame_of(&ml, anim), 3, "still inside the first frame");
    let p = progress_of(&ml, anim);
    assert!(
        p > 0.0 && p < 1.0,
        "backwards progress must be a genuine fraction, got {p} — a hard 0.0 is \
         the negative-time ratio being clamped away"
    );
    // Counting down through the frame, so a fifth in leaves four fifths.
    assert!(
        (p - 0.8).abs() < 0.05,
        "0.02s into a 0.1s frame played backwards leaves ~0.8 of it, got {p}"
    );
}

// ---------------------------------------------------------------------------
// animation_changed
// ---------------------------------------------------------------------------

/// A script switching the selected clip emits `animation_changed` — once.
///
/// ⚠️ The comparison CANNOT be made around the `advance` call. `playback` is
/// rebuilt from the node's `animation` property at the top of every tick and
/// `advance` never touches it, so a before/after pair either side of the call is
/// always equal and the signal could never fire. The switch happens *between*
/// ticks, so it is detected against a companion the previous tick published.
#[test]
fn switching_the_clip_emits_animation_changed_once() {
    let src = "\
extends AnimatedSprite2D
var ticks = 0
func _ready():
    play(\"walk\")
func _process(delta):
    self.ticks = self.ticks + 1
    if self.ticks == 10:
        animation = \"idle\"
";
    let (mut ml, anim, listener) = scene(src, true);

    for _ in 0..9 {
        ml.step(SUB);
    }
    assert_eq!(
        script_int(&ml, listener, "changed"),
        0,
        "no switch yet, so no animation_changed"
    );

    for _ in 0..30 {
        ml.step(SUB);
    }
    assert_eq!(
        prop(&ml, anim, "animation"),
        Variant::String("idle".to_string()),
        "the script's assignment reached the node"
    );
    // ⚠️ Exactly one. The signal is an EDGE on the selected clip; comparing
    // against a per-tick value that is itself re-derived from `animation` would
    // fire it every tick after the switch, and `>= 1` would not notice.
    assert_eq!(
        script_int(&ml, listener, "changed"),
        1,
        "one switch is one animation_changed, not one per tick after it"
    );
}

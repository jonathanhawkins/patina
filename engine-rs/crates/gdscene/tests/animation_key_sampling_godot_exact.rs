//! pat-mgixk.80 — key sampling matches Godot's, key for key.
//!
//! Wiring: `.tscn` -> `PackedScene::from_tscn` -> `add_packed_scene_to_tree`
//!   -> `attach_scene_animation_players` -> `MainLoop::step`
//!     -> `SceneTree::process_animations` -> the node's `position`
//!
//! Three defects, each verified against the vendored source before the fix:
//!
//! 1. **`INTERPOLATION_NEAREST` switched at the MIDPOINT** between keys
//!    (`animation.rs`'s `TransitionType::Nearest` arm). Upstream is literally
//!    `return p_keys[idx].value` where `idx` is the last key at or before the
//!    time (`animation.cpp:2673-2675`) — the floor key, held until the next one
//!    is actually reached. Every NEAREST track changed value early.
//! 2. **The per-key `transition` exponent was not read.** The loader's comment
//!    said so outright. Upstream applies it to the normalised factor before the
//!    interpolation switch, `c = Math::ease(c, tr)`, and **`tr == 0` holds the
//!    key's value** (`animation.cpp:2663-2671`). So ease-in/out keys and hold
//!    keys all played plain linear.
//! 3. **The `update` enum was Godot 3's.** Godot 4 is
//!    `CONTINUOUS = 0, DISCRETE = 1, CAPTURE = 2` (`animation.h:68-72`); the
//!    table had `TRIGGER = 2` and `CAPTURE = 3`, so a 4.x `update = 2` read as
//!    discrete and capture never matched at all.
//!
//! ⚠️ What this does NOT claim: capture is still not *implemented*. Clause 3
//! asserts only that an `update = 2` track stopped being treated as discrete —
//! blending from the property's pre-play value is a separate piece of work.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

const DT: f64 = 0.25;

/// A one-second track on `Mover:position`, parameterised so each clause varies
/// exactly one field of the same clip.
///
/// `interp`, `transitions` and `update` are the three inputs under test;
/// everything else is held constant.
fn scene(interp: i32, transitions: &str, update: i32) -> String {
    format!(
        r#"[gd_scene load_steps=3 format=3]

[sub_resource type="Animation" id="Animation_slide"]
resource_name = "slide"
length = 1.0
loop_mode = 0
tracks/0/type = "value"
tracks/0/imported = false
tracks/0/enabled = true
tracks/0/path = NodePath("Mover:position")
tracks/0/interp = {interp}
tracks/0/loop_wrap = true
tracks/0/keys = {{
"times": PackedFloat32Array(0, 1),
"transitions": PackedFloat32Array({transitions}),
"update": {update},
"values": [Vector2(0, 0), Vector2(10, 0)]
}}

[sub_resource type="AnimationLibrary" id="AnimationLibrary_main"]
_data = {{
&"slide": SubResource("Animation_slide")
}}

[node name="World" type="Node2D"]

[node name="Mover" type="Node2D" parent="."]
position = Vector2(0, 0)

[node name="Anim" type="AnimationPlayer" parent="."]
libraries/ = SubResource("AnimationLibrary_main")
autoplay = "slide"
"#
    )
}

/// `add_packed_scene_to_tree` is what attaches the scene's AnimationPlayers, so
/// the clip is already playing (`autoplay`) by the first `step`.
fn instance(tscn: &str) -> MainLoop {
    let scene = PackedScene::from_tscn(tscn).expect("parse the .tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance the scene");
    MainLoop::new(tree)
}

/// `Mover.position.x` — the property the animation writes.
fn mover_x(ml: &MainLoop) -> f32 {
    // By path, matching the proven idiom in
    // `animation_player_godot46_library_properties_load`.
    let mover = ml
        .tree()
        .get_node_by_path("/root/World/Mover")
        .expect("Mover instanced");
    match ml.tree().get_node(mover).unwrap().get_property("position") {
        Variant::Vector2(v) => v.x,
        other => panic!("Mover.position must be a Vector2; got {other:?}"),
    }
}

/// Steps `frames` frames of `DT` and returns `Mover.position.x`.
fn x_after(ml: &mut MainLoop, frames: usize) -> f32 {
    for _ in 0..frames {
        ml.step(DT);
    }
    mover_x(ml)
}

// ---------------------------------------------------------------------------

/// **The acceptance test.**
#[test]
fn animation_key_sampling_godot_exact() {
    // -- (1) the per-key transition exponent is applied -----------------------
    //
    // `ease(0.5, 2.0)` is `0.5^2 = 0.25`, so a 0->10 track reads 2.5 at the
    // midpoint instead of 5.0. Both numbers are exact in binary floating point,
    // so this needs no tolerance reasoning — and 5.0 is what the un-eased code
    // produced, which is what makes the assertion load-bearing.
    let mut ml = instance(&scene(1, "2.0, 1", 0));
    let x = x_after(&mut ml, 2); // t = 0.5
                                 // Tolerance is 0.05 on a 0..10 range, not an epsilon: the reference test
                                 // for this path (`animation_player_godot46_library_properties_load`) needs
                                 // ~1% of range for the same frame-stepped sampling, so a tighter bound
                                 // could never pass. It still separates 2.5 from the un-eased 5.0 by 50x.
    assert!(
        (x - 2.5).abs() < 0.05,
        "a key with transition 2.0 must ease the factor: 10 * ease(0.5, 2.0) = \
         10 * 0.5^2 = 2.5, got {x}. 5.0 means the exponent was ignored and the \
         segment played linear"
    );

    // -- (2) transition 0 HOLDS the floor key --------------------------------
    //
    // Upstream short-circuits before interpolating at all. Distinct from
    // NEAREST: this holds because of the *exponent*, with the track still
    // declaring linear interpolation.
    let mut ml = instance(&scene(1, "0.0, 1", 0));
    let x = x_after(&mut ml, 2); // t = 0.5
    assert_eq!(
        x, 0.0,
        "transition 0 means `return p_keys[idx].value` — the floor key, held. \
         Got {x}; 2.5 or 5.0 means it was treated as an exponent to ease with"
    );

    // -- (3) NEAREST holds the floor key until the next key ------------------
    //
    // 🔑 The midpoint is the discriminator. The old code switched at t = 0.5, so
    // sampling just past it returned the SECOND key; upstream still returns the
    // first until t reaches 1.0.
    let mut ml = instance(&scene(0, "1, 1", 0));
    let x = x_after(&mut ml, 3); // t = 0.75, past the midpoint
    assert_eq!(
        x, 0.0,
        "interp = 0 is INTERPOLATION_NEAREST, which returns the floor key — 0 \
         until t reaches 1.0. Got {x}; 10 means it switched at the midpoint, \
         half an interval early"
    );

    // …and it does reach the second key once that key's time arrives, so the
    // assertion above is not satisfied by a track that never advances.
    let x = x_after(&mut ml, 1); // t = 1.0
    assert_eq!(
        x, 10.0,
        "at t = 1.0 the floor key IS the second one, so NEAREST must now read \
         10. Got {x} — if this is still 0 the track is stuck rather than \
         holding, and the clause above proves nothing"
    );

    // -- (4) update = 2 is CAPTURE, not DISCRETE -----------------------------
    //
    // ⚠️ Behavioural, because capture itself is unimplemented: the point is that
    // a 4.x `update = 2` track no longer STEPS. Under the old Godot-3 table it
    // mapped to Discrete and held 0 across the whole segment; now it falls
    // through to interpolation and moves.
    let mut ml = instance(&scene(1, "1, 1", 2));
    let x = x_after(&mut ml, 2); // t = 0.5
    assert!(
        x > 0.0,
        "`update = 2` is UPDATE_CAPTURE in Godot 4, not discrete. Got {x}: 0 \
         means it was still classified Discrete off the Godot-3 table and the \
         track stepped instead of moving"
    );

    // The control: update = 1 really IS discrete and must still hold.
    let mut ml = instance(&scene(1, "1, 1", 1));
    let x = x_after(&mut ml, 2); // t = 0.5
    assert_eq!(
        x, 0.0,
        "`update = 1` is UPDATE_DISCRETE and must hold the floor key. Got {x} — \
         if this interpolates, the fix did not re-map the enum, it deleted the \
         discrete case"
    );
}

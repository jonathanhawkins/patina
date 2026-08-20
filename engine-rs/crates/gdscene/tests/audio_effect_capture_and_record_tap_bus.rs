//! Acceptance test for pat-s3rrv.141: `AudioEffectCapture` and
//! `AudioEffectRecord`, the two bus taps a **script drives** rather than
//! configures.
//!
//! Everything else on a bus is set once and then colours the signal. These two
//! exist to hand audio back to the game: a capture is how a visualizer, a level
//! meter or a voice-chat sender reads the frames the mixer actually produced,
//! and a recorder is how a game saves them. Neither existed — no struct, no
//! bus-layout arm, no way to name one from a script.
//!
//! The path under test is the whole chain, with no Rust-side shortcut:
//!
//! ```text
//! GDScript AudioServer.get_bus_effect_instance(bus, slot)
//!   -> a slot-addressing ObjectId handle
//!   -> interpreter instance dispatch -> SceneAccess::audio_effect_call
//!   -> SceneTreeAccessor -> the live AudioServer -> the effect in that slot
//! ```
//!
//! Semantics ported from `upstream/godot/servers/audio/effects/`
//! `audio_effect_capture.cpp` and `audio_effect_record.cpp`.

use gdaudio::effects::{AudioEffect, AudioEffectCapture, AudioEffectRecord, RecordError};
use gdaudio::mixer::AudioMixer;
use gdaudio::AudioBuffer;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const SR: u32 = 44_100;

/// One second of stereo 440 Hz at half amplitude — clearly non-silent.
fn sine_buffer() -> AudioBuffer {
    let mut samples = Vec::with_capacity(SR as usize * 2);
    for i in 0..SR as usize {
        let t = i as f32 / SR as f32;
        let v = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
        samples.push(v);
        samples.push(v);
    }
    AudioBuffer {
        samples,
        sample_rate: SR,
        channels: 2,
    }
}

/// Builds `World` carrying `script`, with one autoplaying `AudioStreamPlayer`
/// routed to a "Tap" bus that carries a capture in slot 0 and a recorder in
/// slot 1.
///
/// The effects are inserted into the live mixer *before* `_ready` runs, because
/// `get_bus_effect_instance` has to find a real effect for any of this to mean
/// anything. `enter_tree_with_scripts` is what binds the `AudioServer`
/// singleton for `_ready`.
fn scene(script: &str) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let world = tree.add_child(root, Node::new("World", "Node")).unwrap();

    let mut p = Node::new("Player", "AudioStreamPlayer");
    p.set_property("bus", Variant::String("Tap".to_string()));
    p.set_property("autoplay", Variant::Bool(true));
    let player = tree.add_child(world, p).unwrap();

    // Attached after `add_child` so the premature enter/ready that `add_child`
    // performs cannot run `_ready` before the script exists.
    tree.attach_script(
        world,
        Box::new(GDScriptNodeInstance::from_source(script, world).expect("script parses")),
    );

    let mut ml = MainLoop::new(tree);
    {
        let mixer = ml.audio_bridge_mut().server_mut().mixer_mut();
        let tap = mixer.add_bus("Tap");
        assert_eq!(tap, 1, "Tap should be the first bus after Master");
        let bus = mixer.get_bus_mut(tap).expect("the bus just added");
        bus.effects_mut().add(Box::new(AudioEffectCapture::new()));
        bus.effects_mut().add(Box::new(AudioEffectRecord::new()));
    }
    ml.audio_bridge_mut()
        .set_stream_buffer(player, sine_buffer());

    ml.enter_tree_with_scripts(world);
    (ml, world)
}

fn script_var(ml: &MainLoop, node: NodeId, prop: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("node has a script")
        .get_property(prop)
        .unwrap_or(Variant::Nil)
}

/// Drains 64 frames every frame and parks the result in `frames`.
///
/// ⓘ Deliberately does no arithmetic in GDScript: the drained array is handed
/// straight to Rust, so what the assertions read is the effect's own output and
/// not something the script computed about it.
const DRAIN: &str = "\
extends Node
var frames = []
var ring_frames = 0
var pushed = 0
var available = 0
func _process(_delta):
    var bus = AudioServer.get_bus_index(\"Tap\")
    var inst = AudioServer.get_bus_effect_instance(bus, 0)
    self.ring_frames = inst.get_buffer_length_frames()
    self.pushed = inst.get_pushed_frames()
    self.available = inst.get_frames_available()
    self.frames = inst.get_buffer(64)
";

/// Records for six frames, then stops and fetches the stream — reading the
/// capture's counters in the same call so the two taps cover the same window.
const RECORD: &str = "\
extends Node
var ticks = 0
var wav = null
var pushed = 0
var discarded = 0
var was_active = false
func _ready():
    var bus = AudioServer.get_bus_index(\"Tap\")
    var rec = AudioServer.get_bus_effect_instance(bus, 1)
    rec.set_recording_active(true)
    self.was_active = rec.is_recording_active()
func _process(_delta):
    self.ticks = self.ticks + 1
    if self.ticks == 6:
        var bus = AudioServer.get_bus_index(\"Tap\")
        var rec = AudioServer.get_bus_effect_instance(bus, 1)
        rec.set_recording_active(false)
        self.wav = rec.get_recording()
        var cap = AudioServer.get_bus_effect_instance(bus, 0)
        self.pushed = cap.get_pushed_frames()
        self.discarded = cap.get_discarded_frames()
";

/// The control: the same scene and the same players, never recording.
const NEVER_RECORDS: &str = "\
extends Node
var ticks = 0
var wav = \"unset\"
func _process(_delta):
    self.ticks = self.ticks + 1
    if self.ticks == 6:
        var bus = AudioServer.get_bus_index(\"Tap\")
        self.wav = AudioServer.get_bus_effect_instance(bus, 1).get_recording()
";

/// **The acceptance clause.** A script fetches the capture instance off a live
/// bus, drains the frames the mixer produced, and pulls an `AudioStreamWAV` out
/// of a completed recording whose length matches the frames that were mixed.
#[test]
fn audio_effect_capture_and_record_tap_bus() {
    // -- (a) a script drains the frames the mixer actually produced ---------
    let (mut ml, world) = scene(DRAIN);
    let mut drained = Vec::new();
    for _ in 0..12 {
        ml.step(1.0 / 60.0);
        if let Variant::PackedVector2Array(v) = script_var(&ml, world, "frames") {
            if !v.is_empty() {
                drained = v;
                break;
            }
        }
    }
    assert_eq!(
        drained.len(),
        64,
        "the script must receive exactly the 64 frames it asked for — \
         `get_buffer` is all-or-nothing and never short-reads",
    );
    // 🔑 The frames are the MIXED signal, not a buffer the script filled: the
    // source is a 0.5-amplitude sine and nothing else writes here.
    let peak = drained.iter().fold(0.0f32, |a, f| a.max(f.x.abs()));
    assert!(
        peak > 0.05,
        "drained frames must carry the mixed audio, peak was {peak}",
    );
    // ⚠️ Both channels, because the array is one Vector2 per FRAME. A capture
    // that filled only `x` would still pass the peak check above.
    let right_peak = drained.iter().fold(0.0f32, |a, f| a.max(f.y.abs()));
    assert!(
        right_peak > 0.05,
        "the right channel must be captured too, peak was {right_peak}",
    );

    assert_eq!(
        script_var(&ml, world, "ring_frames"),
        Variant::Int(8192),
        "🔑 the ring is `nearest_shift(mix_rate * buffer_length)` — 0.1 s asks \
         for 4410 frames at 44.1 kHz (4800 at 48 kHz) and gets the next power \
         of two either way. A version that allocated exactly the requested \
         count reports 4410 here",
    );
    let pushed = script_var(&ml, world, "pushed");
    assert!(
        matches!(pushed, Variant::Int(n) if n > 0),
        "the tap must have been handed frames by the mixer, got {pushed:?}",
    );

    // -- (b) the recorder yields a stream whose length matches the mix ------
    let (mut rec_ml, rec_world) = scene(RECORD);
    for _ in 0..8 {
        rec_ml.step(1.0 / 60.0);
    }

    assert_eq!(
        script_var(&rec_ml, rec_world, "was_active"),
        Variant::Bool(true),
        "`is_recording_active` must report the state `set_recording_active` \
         just wrote",
    );
    assert_eq!(
        script_var(&rec_ml, rec_world, "discarded"),
        Variant::Int(0),
        "no frames may have been dropped over this window, or the frame-count \
         equality below would compare two different spans of audio",
    );

    let Variant::Resource(wav) = script_var(&rec_ml, rec_world, "wav") else {
        panic!(
            "get_recording() must hand back an AudioStreamWAV, got {:?}",
            script_var(&rec_ml, rec_world, "wav")
        );
    };
    assert_eq!(wav.class_name, "AudioStreamWAV");
    assert_eq!(
        wav.properties.get("format"),
        Some(&Variant::Int(1)),
        "⚠️ the default format is 1 (16-Bit), not 0 — a zero-initialized field \
         would silently record at 8-bit",
    );
    assert_eq!(wav.properties.get("stereo"), Some(&Variant::Bool(true)));
    // Read from the live server rather than hardcoded: the stamp has to follow
    // whatever rate the bus ran at, not whatever this test assumed.
    let bus_rate = rec_ml.audio_bridge().server().output_sample_rate();
    assert_eq!(
        wav.properties.get("mix_rate"),
        Some(&Variant::Int(bus_rate as i64)),
        "the stream must be stamped with the rate the bus actually ran at",
    );

    let Some(Variant::PackedByteArray(data)) = wav.properties.get("data") else {
        panic!("the recording must carry its encoded samples");
    };
    // 16-bit stereo: two channels, two bytes each.
    let recorded_frames = data.len() / 4;
    assert!(recorded_frames > 0, "the recording must not be empty");
    assert_eq!(
        data.len() % 4,
        0,
        "a 16-bit stereo buffer cannot end mid-frame",
    );
    // **The duration clause.** The capture counts every frame the mixer handed
    // to the bus over the same window; the recorder must hold exactly that
    // many. This compares two independent counters inside the engine rather
    // than a number the test computed for itself.
    assert_eq!(
        Variant::Int(recorded_frames as i64),
        script_var(&rec_ml, rec_world, "pushed"),
        "the recorded frame count must match the frames the mixer produced \
         while recording was active",
    );

    // -- (c) the control: a take that never started is NULL -----------------
    // ⚠️ Without this, (b) proves only that *something* came back. Godot
    // returns null rather than a zero-length stream precisely so a script
    // cannot mistake "the bus was never recorded" for "silence was recorded".
    let (mut idle, idle_world) = scene(NEVER_RECORDS);
    for _ in 0..8 {
        idle.step(1.0 / 60.0);
    }
    assert_eq!(
        script_var(&idle, idle_world, "wav"),
        Variant::Nil,
        "an empty take must be a null, not an empty AudioStreamWAV",
    );
}

/// 🔑 `get_buffer` is **all or nothing**. Godot returns an EMPTY array rather
/// than a short read, and consumes nothing in that case
/// (`audio_effect_capture.cpp:39-46`).
///
/// ⚠️ A short-read version passes the acceptance test above — it would return
/// however many frames were ready. It fails here, and the failure matters: a
/// caller loop written against all-or-nothing semantics would splice a partial
/// block into its stream and hear a click it cannot explain.
#[test]
fn an_under_filled_drain_returns_nothing_and_consumes_nothing() {
    let mut cap = AudioEffectCapture::new();
    let mut block = vec![0.25f32; 200]; // 100 stereo frames
    cap.process(&mut block, SR, 2);
    assert_eq!(cap.get_frames_available(), 100);

    assert!(
        cap.get_buffer(101).is_empty(),
        "asking for more than is buffered must yield nothing",
    );
    assert_eq!(
        cap.get_frames_available(),
        100,
        "and must consume nothing — a failed drain that still advanced the \
         read head would silently drop the audio it refused to return",
    );
    assert!(!cap.can_get_buffer(101));
    assert!(cap.can_get_buffer(100));
    assert_eq!(cap.get_buffer(100).len(), 100);
    assert_eq!(cap.get_frames_available(), 0, "a full drain consumes");

    // A zero-frame request, and a request the ring could never satisfy.
    assert!(cap.get_buffer(0).is_empty());
    // ⓘ A boundary check, not a discriminating one: `frames >= ring size` is
    // unsatisfiable anyway, so the not-enough-data path above already covers
    // it. Godot's separate `ERR_FAIL_INDEX_V` guard exists to PRINT that the
    // request was impossible rather than to change the answer.
    assert!(cap.get_buffer(cap.get_buffer_length_frames()).is_empty());
}

/// ⚠️ Overflow discards the **whole incoming block**, never part of it
/// (`audio_effect_capture.cpp:127-133`).
///
/// The intuitive "write what fits, drop the rest" both miscounts
/// `discarded_frames` and tears a block in half.
#[test]
fn an_overflowing_block_is_discarded_whole() {
    let mut cap = AudioEffectCapture::with_buffer_length(0.01); // 441 -> 512 frames
    let mut block = vec![0.1f32; 600]; // 300 stereo frames
    cap.process(&mut block, SR, 2);
    assert_eq!(cap.get_buffer_length_frames(), 512);
    assert_eq!(cap.get_pushed_frames(), 300);
    assert_eq!(cap.get_discarded_frames(), 0);

    // 511 usable slots: one is reserved so "full" stays distinguishable from
    // "empty". 300 are taken, so this 300-frame block cannot fit.
    cap.process(&mut block, SR, 2);
    assert_eq!(
        cap.get_frames_available(),
        300,
        "not one frame of the overflowing block may be written",
    );
    assert_eq!(
        cap.get_discarded_frames(),
        300,
        "the whole block is counted as discarded, not just the excess",
    );
    assert_eq!(cap.get_pushed_frames(), 300, "and pushed does not grow");

    // ⓘ A flush frees the ring but leaves the lifetime counters alone, which is
    // what lets a script detect the underrun after the fact.
    cap.clear_buffer();
    assert_eq!(cap.get_frames_available(), 0);
    assert_eq!(cap.get_discarded_frames(), 300);
    assert_eq!(cap.get_pushed_frames(), 300);
}

/// The 16-bit encoding, sample-exact.
///
/// ⚠️ The scale is 32768 with an asymmetric clamp, not 32767: `-1.0` must reach
/// exactly `-32768` while `+1.0` clamps to `32767`. Using 32767 as the
/// multiplier is the natural guess and is off by one LSB on every non-zero
/// sample.
#[test]
fn the_recording_uses_godots_asymmetric_16_bit_scale() {
    let mut rec = AudioEffectRecord::new();
    assert_eq!(rec.get_format(), 1, "the default format is 16-Bit");
    assert!(!rec.is_recording_active());
    assert_eq!(
        rec.get_recording(),
        Err(RecordError::NothingRecorded),
        "an empty take is an error, never an empty stream",
    );

    rec.set_recording_active(true);
    let mut block = vec![1.0f32, -1.0, 0.5, -0.5];
    rec.process(&mut block, SR, 2);
    rec.set_recording_active(false);

    let wav = rec.get_recording().expect("a take with frames in it");
    assert_eq!(wav.frame_count(), 2);
    assert_eq!(wav.mix_rate, SR);
    assert!(wav.stereo);

    let s: Vec<i16> = wav
        .data
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    assert_eq!(
        s,
        vec![32767, -32768, 16384, -16384],
        "+1.0 clamps to 32767 while -1.0 reaches -32768 exactly",
    );

    // 🔑 Recording again CLEARS the previous take rather than appending to it —
    // Godot's `init()` calls `recording_data.clear()`. Treating this as a
    // pause/resume would silently splice two takes into one stream.
    rec.set_recording_active(true);
    let mut second = vec![0.25f32, 0.25];
    rec.process(&mut second, SR, 2);
    rec.set_recording_active(false);
    assert_eq!(
        rec.get_recording().expect("second take").frame_count(),
        1,
        "the second take must not carry the first one's frames",
    );
}

/// ⚠️ 8-bit `AudioStreamWAV` is **signed**, stored as the raw two's complement
/// byte — not the unsigned 0..255 convention a RIFF WAV file uses. Encoding
/// `v * 127 + 128` produces a file that plays fine in an audio editor and is
/// off by a half-scale DC step here.
#[test]
fn the_8_bit_recording_format_is_signed() {
    let mut rec = AudioEffectRecord::new();
    rec.set_format(0);
    rec.set_recording_active(true);
    let mut block = vec![0.0f32, -1.0];
    rec.process(&mut block, SR, 2);
    rec.set_recording_active(false);

    let wav = rec.get_recording().expect("a take");
    assert_eq!(wav.data, vec![0x00, 0x80], "silence is 0, -1.0 is -128");
    assert_eq!(wav.frame_count(), 1);

    // A format this port cannot encode is reported, not returned as a
    // valid-looking stream carrying an empty buffer.
    rec.set_format(3); // QOA
    assert_eq!(rec.get_recording(), Err(RecordError::UnsupportedFormat(3)));
}

/// Both classes are constructible from an authored bus layout.
///
/// Without these arms `effect_from_sub_resource` returned `None`, and `None`
/// means "skip this slot" — so a project whose Record bus was configured in the
/// editor booted with the recorder silently gone.
#[test]
fn a_bus_layout_builds_both_taps() {
    const LAYOUT: &str = r#"[gd_resource type="AudioBusLayout" format=3]

[sub_resource type="AudioEffectCapture" id="CAP_1"]
buffer_length = 0.25

[sub_resource type="AudioEffectRecord" id="REC_1"]
format = 0

[resource]
bus/0/name = "Master"
bus/0/volume_db = 0.0
bus/0/effect/0/effect = SubResource("CAP_1")
bus/0/effect/0/enabled = true
bus/0/effect/1/effect = SubResource("REC_1")
bus/0/effect/1/enabled = true
"#;

    let mut mixer = AudioMixer::new();
    mixer.load_bus_layout_tres(LAYOUT);
    let bus = mixer.get_bus(0).expect("master bus");
    assert_eq!(bus.effects().len(), 2, "both slots must survive the load");

    let cap = bus.effects().capture_at(0).expect("slot 0 is the capture");
    assert_eq!(
        cap.get_buffer_length(),
        0.25,
        "the authored buffer length must reach the effect",
    );
    let rec = bus.effects().record_at(1).expect("slot 1 is the recorder");
    assert_eq!(rec.get_format(), 0, "the authored format must reach it too");

    // ⚠️ The typed accessors must refuse the wrong slot rather than answering
    // with a default, or a script asking a recorder for its captured frames
    // would get a plausible zero.
    assert!(bus.effects().capture_at(1).is_none());
    assert!(bus.effects().record_at(0).is_none());
    assert!(bus.effects().capture_at(7).is_none());

    // The class names the editor's Add-Effect list and `get_bus_effect_instance`
    // key on — `name()` is a short human label and cannot say which class a
    // slot holds.
    assert_eq!(
        bus.effects().effect_at(0).map(|e| e.godot_class()),
        Some("AudioEffectCapture"),
    );
    assert_eq!(
        bus.effects().effect_at(1).map(|e| e.godot_class()),
        Some("AudioEffectRecord"),
    );
}

/// A capture is a **tap**, not a filter: the signal passes through untouched.
///
/// ⚠️ Nothing else in this file would catch a capture that zeroed or scaled its
/// input — every other assertion reads the captured copy, not the bus output.
#[test]
fn both_taps_pass_the_signal_through_unchanged() {
    let source: Vec<f32> = (0..64).map(|i| (i as f32 / 64.0) - 0.5).collect();

    let mut cap = AudioEffectCapture::new();
    let mut buf = source.clone();
    cap.process(&mut buf, SR, 2);
    assert_eq!(buf, source, "a capture must not colour the bus");

    let mut rec = AudioEffectRecord::new();
    rec.set_recording_active(true);
    let mut buf = source.clone();
    rec.process(&mut buf, SR, 2);
    assert_eq!(buf, source, "nor may a recorder while it is recording");
}

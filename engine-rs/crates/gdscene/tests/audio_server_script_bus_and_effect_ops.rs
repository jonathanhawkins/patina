//! pat-mgixk.83 — `AudioServer` bus routing, linear volume and effect-chain
//! operations reach the live mixer from GDScript.
//!
//! # The false-done this undoes
//!
//! Every operation asserted below already worked in `gdaudio` and had **no
//! script caller at all**: `AudioBus::set_send`/`send` (bus.rs:78-83),
//! `AudioMixer::move_bus` (mixer.rs:64), `AudioBus::volume_linear` /
//! `db_to_linear` (:141-149) and the whole `EffectChain`
//! add/remove/swap/set_enabled surface (effects.rs:563-720). The mixer has been
//! routing sends and running effect chains for releases; `call_audio_server_method`
//! (interpreter.rs) simply had no arm for any of it, so
//! `AudioServer.set_bus_send(...)` from a script raised `UndefinedFunction`.
//!
//! The chain under test, with no Rust-side shortcut:
//!
//! ```text
//! GDScript AudioServer.* -> interpreter dispatch arm -> SceneAccess
//!   -> SceneTreeAccessor::audio_mut (address bound by MainLoop)
//!   -> the AudioServerBridge's live AudioServer -> next frame's mix
//! ```
//!
//! # Why the assertions are on mixed PCM
//!
//! 🔑 Each clause is checked against the **peak of the mixed output**, not
//! against a value the script itself wrote. A binding that stored its argument
//! somewhere and never reached the mixer would satisfy a property read and
//! change no sample; `peak_over` is what separates "the call was accepted" from
//! "the audio changed".
//!
//! # What this test does NOT cover, and why
//!
//! ⚠️ The bead's acceptance also asks for `add_bus_effect(bus,
//! AudioEffectAmplify.new())` and `get_bus_effect(bus, 0).volume_db = 0`.
//! **Neither is expressible in this interpreter yet**, for reasons that are
//! structural and have nothing to do with `AudioServer`:
//!
//! * `AudioEffectAmplify.new()` — `.new()` on a non-`GDScript` resource returns
//!   null by construction (interpreter.rs, the `"new"` arm: *"Non-script
//!   resources have no `.new()`; Godot yields null"*). There is no constructor
//!   path for an engine resource class.
//! * `.volume_db = 0` on the handle — **no** synthetic object handle in the
//!   interpreter supports property assignment. `is_audio_effect_handle` is
//!   consulted in exactly one place, the method-call router, and the same is
//!   true of every sibling handle (image, editor-fs-dir, websocket-peer, …).
//!
//! So this test installs the effect the way a **bus layout `.tres`** does —
//! which is the one route that exists today, and the one
//! `effect_from_sub_resource` already serves — and drives the *script-facing*
//! half, `set_bus_effect_enabled`, which is what the bead is about. The two
//! structural gaps are reported, not half-built.

use gdaudio::{AudioBuffer, AudioEffectAmplify};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// One second of stereo 440 Hz at half amplitude — clearly non-silent.
fn sine_buffer() -> AudioBuffer {
    let sample_rate = 44_100u32;
    let mut samples = Vec::with_capacity(sample_rate as usize * 2);
    for i in 0..sample_rate as usize {
        let t = i as f32 / sample_rate as f32;
        let v = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
        samples.push(v);
        samples.push(v);
    }
    AudioBuffer {
        samples,
        sample_rate,
        channels: 2,
    }
}

/// `World` carrying `script`, with one autoplaying player routed to `bus`.
///
/// A `Music` and an `Sfx` bus are added to the live mixer **before** `_ready`
/// runs, so `get_bus_index` finds real buses. `amplify_db` installs an
/// `AudioEffectAmplify` on `Sfx` the way a bus-layout `.tres` would — see the
/// module note on why the effect cannot be constructed from script.
///
/// `enter_tree_with_scripts` is what binds the `AudioServer` singleton for
/// `_ready`; a bare `LifecycleManager::enter_tree` would leave it unbound.
fn scene(script: &str, bus: &str, amplify_db: Option<f32>) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let world = tree.add_child(root, Node::new("World", "Node")).unwrap();

    let mut p = Node::new("Player", "AudioStreamPlayer");
    p.set_property("bus", Variant::String(bus.to_string()));
    p.set_property("autoplay", Variant::Bool(true));
    let player = tree.add_child(world, p).unwrap();

    tree.attach_script(
        world,
        Box::new(GDScriptNodeInstance::from_source(script, world).expect("script parses")),
    );

    let mut ml = MainLoop::new(tree);
    {
        let mixer = ml.audio_bridge_mut().server_mut().mixer_mut();
        let music = mixer.add_bus("Music");
        let sfx = mixer.add_bus("Sfx");
        assert_eq!((music, sfx), (1, 2), "Master, then Music, then Sfx");
        if let Some(db) = amplify_db {
            mixer
                .get_bus_mut(sfx)
                .expect("Sfx exists")
                .effects_mut()
                .add(Box::new(AudioEffectAmplify::new(db)));
        }
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
        // `get_property`, never `list_properties().default_value` — the latter
        // is the declared initialiser and never changes.
        .get_property(prop)
        .unwrap_or(Variant::Nil)
}

/// Peak `|sample|` over `frames` mixed frames.
fn peak_over(ml: &mut MainLoop, frames: usize) -> f32 {
    let mut peak = 0.0f32;
    for _ in 0..frames {
        ml.step(1.0 / 60.0);
        peak = ml
            .audio_bridge()
            .last_mix()
            .iter()
            .fold(peak, |a, s| a.max(s.abs()));
    }
    peak
}

// ---------------------------------------------------------------------------

/// Routes `Sfx` into `Music`, then silences `Music`. If the send is live, the
/// player on `Sfx` goes quiet even though nothing touched `Sfx` itself.
const ROUTE_THEN_DUCK: &str = "\
extends Node
var sfx_idx = -99
var send_name = \"<unset>\"
func _ready():
    self.sfx_idx = AudioServer.get_bus_index(\"Sfx\")
    AudioServer.set_bus_send(self.sfx_idx, \"Music\")
    self.send_name = AudioServer.get_bus_send(self.sfx_idx)
    AudioServer.set_bus_volume_db(AudioServer.get_bus_index(\"Music\"), -80.0)
";

/// The control: the same routing call, but `Music` is left alone — so a quiet
/// result in the test above cannot be "the Sfx bus was broken by set_bus_send".
const ROUTE_ONLY: &str = "\
extends Node
var sfx_idx = -99
var send_name = \"<unset>\"
func _ready():
    self.sfx_idx = AudioServer.get_bus_index(\"Sfx\")
    AudioServer.set_bus_send(self.sfx_idx, \"Music\")
    self.send_name = AudioServer.get_bus_send(self.sfx_idx)
";

/// **Acceptance clause: `set_bus_send` reaches the mixer.**
#[test]
fn audio_server_script_bus_and_effect_ops() {
    // -- (a) the send is readable back through the script API ---------------
    let (mut ml, world) = scene(ROUTE_ONLY, "Sfx", None);
    assert_eq!(
        script_var(&ml, world, "sfx_idx"),
        Variant::Int(2),
        "the script must resolve the Sfx bus that was added to the live mixer"
    );
    assert_eq!(
        script_var(&ml, world, "send_name"),
        Variant::String("Music".to_string()),
        "`get_bus_send` must report what `set_bus_send` just wrote; \"<unset>\" \
         means the getter never ran and \"Master\" means the setter did not take"
    );

    // …and the player is still audible, since Music was not touched. This is
    // the control for (b): it proves `set_bus_send` alone does not silence the
    // bus, so a quiet frame in (b) is the DUCK arriving through the send.
    let routed_peak = peak_over(&mut ml, 4);
    assert!(
        routed_peak > 0.05,
        "routing Sfx into Music must not itself silence the player: peak \
         {routed_peak}"
    );

    // -- (b) the routing is LIVE: ducking Music ducks the Sfx player ---------
    let (mut ml, world) = scene(ROUTE_THEN_DUCK, "Sfx", None);
    assert_eq!(
        script_var(&ml, world, "send_name"),
        Variant::String("Music".to_string()),
        "precondition: the send must be set before the duck means anything"
    );
    let ducked_peak = peak_over(&mut ml, 4);
    assert!(
        ducked_peak < routed_peak * 0.2,
        "the player is on Sfx, which the script sent into Music, and Music was \
         silenced — so the mix must be far quieter than the {routed_peak} the \
         un-ducked control produced; got {ducked_peak}. A peak equal to the \
         control means `set_bus_send` was accepted and never reached the mixer"
    );
}

/// **Acceptance clause: `set_bus_volume_linear` scales the bus.**
///
/// 0.5 linear is exactly -6.0206 dB and round-trips through
/// `AudioBus::db_to_linear`, so the expected ratio is exact rather than a
/// tolerance invented to fit.
#[test]
fn set_bus_volume_linear_halves_the_master_peak() {
    const FULL: &str = "\
extends Node
var vol = -1.0
func _ready():
    self.vol = AudioServer.get_bus_volume_linear(0)
";
    const HALF: &str = "\
extends Node
var vol = -1.0
func _ready():
    AudioServer.set_bus_volume_linear(0, 0.5)
    self.vol = AudioServer.get_bus_volume_linear(0)
";

    let (mut ml, world) = scene(FULL, "Master", None);
    assert_eq!(
        script_var(&ml, world, "vol"),
        Variant::Float(1.0),
        "Master starts at unity gain, so the getter must read 1.0 — a -1.0 here \
         means `get_bus_volume_linear` never ran"
    );
    let full = peak_over(&mut ml, 4);
    assert!(full > 0.05, "the control must be audible: peak {full}");

    let (mut ml, world) = scene(HALF, "Master", None);
    match script_var(&ml, world, "vol") {
        Variant::Float(v) => assert!(
            (v - 0.5).abs() < 1e-3,
            "`get_bus_volume_linear` must read back the 0.5 that was set; got {v}"
        ),
        other => panic!("`vol` should be a Float, got {other:?}"),
    }
    let half = peak_over(&mut ml, 4);
    assert!(
        (half - full * 0.5).abs() < full * 0.1,
        "setting Master to 0.5 linear must halve the mixed peak: {full} -> \
         expected ~{}, got {half}. A peak equal to {full} means the call was \
         accepted and never reached the bus",
        full * 0.5
    );
}

/// **Acceptance clause: `set_bus_effect_enabled` toggles a live effect.**
///
/// The effect is a -60 dB amplify on `Sfx`, installed the way a bus layout
/// installs one. Enabled, it all but silences the bus; disabled, the level
/// returns. Both directions are asserted, because "quiet" alone would also be
/// satisfied by an effect that could never be turned off.
#[test]
fn set_bus_effect_enabled_toggles_the_amplify() {
    const COUNT_ONLY: &str = "\
extends Node
var n = -1
var on = false
func _ready():
    var sfx = AudioServer.get_bus_index(\"Sfx\")
    self.n = AudioServer.get_bus_effect_count(sfx)
    self.on = AudioServer.is_bus_effect_enabled(sfx, 0)
";
    const DISABLE: &str = "\
extends Node
var n = -1
var on = true
func _ready():
    var sfx = AudioServer.get_bus_index(\"Sfx\")
    self.n = AudioServer.get_bus_effect_count(sfx)
    AudioServer.set_bus_effect_enabled(sfx, 0, false)
    self.on = AudioServer.is_bus_effect_enabled(sfx, 0)
";

    // -- the effect is visible to the script, and enabled by default --------
    let (mut ml, world) = scene(COUNT_ONLY, "Sfx", Some(-60.0));
    assert_eq!(
        script_var(&ml, world, "n"),
        Variant::Int(1),
        "`get_bus_effect_count` must see the amplify installed on Sfx; 0 means \
         the binding does not reach the bus's EffectChain"
    );
    assert_eq!(
        script_var(&ml, world, "on"),
        Variant::Bool(true),
        "a freshly added effect is enabled in Godot"
    );
    let with_effect = peak_over(&mut ml, 4);

    // -- disabling it from script restores the level ------------------------
    let (mut ml, world) = scene(DISABLE, "Sfx", Some(-60.0));
    assert_eq!(
        script_var(&ml, world, "on"),
        Variant::Bool(false),
        "`is_bus_effect_enabled` must report the disable that `_ready` just \
         performed; true means `set_bus_effect_enabled` did not take"
    );
    let without_effect = peak_over(&mut ml, 4);

    assert!(
        without_effect > with_effect * 5.0,
        "a -60 dB amplify is a 0.001x gain, so disabling it must raise the \
         mixed peak by orders of magnitude: enabled {with_effect}, disabled \
         {without_effect}. Equal peaks mean the enable flag never reached the \
         chain that `process_effects` consults"
    );
    assert!(
        without_effect > 0.05,
        "with the effect off the player must be plainly audible: peak \
         {without_effect}"
    );
}

/// **The device / speaker queries and `set_bus_count`** (pat-mgixk.83).
///
/// These are the cheap half of the bead's list. The three queries are answered
/// in the interpreter with no `SceneAccess` hook, because none of them has
/// anything to ask the mixer — Patina mixes interleaved stereo unconditionally
/// and enumerates no devices. `set_bus_count` does reach the mixer.
///
/// ⚠️ Two methods from the bead's list are deliberately absent:
/// `get_driver_name` (Patina has no named driver, so any answer would be
/// invented) and `get_bus_channels` (Godot's "channels" counts stereo PAIRS,
/// which Patina's bus model does not distinguish). Both want an owner decision,
/// not a guess.
#[test]
fn audio_server_device_queries_and_bus_count() {
    const PROBE: &str = "\
extends Node
var mode = -1
var dev_count = -1
var dev_first = \"<unset>\"
var current = \"<unset>\"
var shrunk = -1
var grown = -1
var driver = \"<unset>\"
var ch_valid = -1
var ch_bad = -1
func _ready():
\tself.driver = AudioServer.get_driver_name()
\tself.ch_valid = AudioServer.get_bus_channels(0)
\tself.ch_bad = AudioServer.get_bus_channels(99)
\tself.mode = AudioServer.get_speaker_mode()
\tvar list = AudioServer.get_output_device_list()
\tself.dev_count = list.size()
\tself.dev_first = list.get(0)
\tself.current = AudioServer.get_output_device()
\tAudioServer.set_bus_count(2)
\tself.shrunk = AudioServer.get_bus_count()
\tAudioServer.set_bus_count(5)
\tself.grown = AudioServer.get_bus_count()
";

    // The player sits on Master, which `set_bus_count` can never remove — the
    // shrink below drops buses from the TAIL.
    let (ml, world) = scene(PROBE, "Master", None);

    assert_eq!(
        script_var(&ml, world, "mode"),
        Variant::Int(0),
        "`get_speaker_mode` is SPEAKER_MODE_STEREO = 0; -1 means the arm never ran"
    );
    assert_eq!(
        script_var(&ml, world, "dev_count"),
        Variant::Int(1),
        "`get_output_device_list` reports exactly one device"
    );
    assert_eq!(
        script_var(&ml, world, "dev_first"),
        Variant::String("Default".to_string()),
        "…and it is named \"Default\""
    );
    assert_eq!(
        script_var(&ml, world, "current"),
        Variant::String("Default".to_string()),
        "`get_output_device` reports the one device as current"
    );

    // -- set_bus_count both shrinks and grows --------------------------------
    //
    // The fixture starts at 3 (Master, Music, Sfx), so 2 is a real shrink and 5
    // a real grow. Asserting BOTH matters: a binding that only ever appended
    // would satisfy the grow alone, and one that cleared the table would
    // satisfy the shrink alone.
    assert_eq!(
        script_var(&ml, world, "shrunk"),
        Variant::Int(2),
        "`set_bus_count(2)` must drop the fixture's third bus; 3 means the call \
         did not reach the mixer"
    );
    assert_eq!(
        script_var(&ml, world, "grown"),
        Variant::Int(5),
        "`set_bus_count(5)` must add buses back up to 5"
    );

    // -- get_driver_name / get_bus_channels, both ported from the vendored
    //    Godot source rather than chosen ------------------------------------
    assert_eq!(
        script_var(&ml, world, "driver"),
        Variant::String("Dummy".to_string()),
        "Godot returns `AudioDriver::get_singleton()->get_name()`, and Patina \
         has no named driver — `AudioOutputStream` exposes no name — so the \
         faithful answer is Godot's own no-backend name, \
         `AudioDriverDummy::get_name()` = \"Dummy\""
    );
    assert_eq!(
        script_var(&ml, world, "ch_valid"),
        Variant::Int(1),
        "`get_bus_channels` is `channels.size()`, sized to \
         `get_channel_count()`, which is 1 for SPEAKER_MODE_STEREO \
         (audio_server.h:356-368). It must agree with the 0 that \
         `get_speaker_mode` reports — 2 would mean the two answers drifted"
    );
    assert_eq!(
        script_var(&ml, world, "ch_bad"),
        Variant::Int(0),
        "an out-of-range bus yields 0 via Godot's `ERR_FAIL_INDEX_V(p_bus, \
         buses.size(), 0)`; a 1 here means the index is not validated and the \
         assertion above would hold for any bus at all"
    );
}

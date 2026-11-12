//! pat-mgixk.90 acceptance: a game's GDScript drives the AudioServer bus,
//! effect, send and layout API, and the live mixer follows.
//!
//! # What was wrong
//!
//! `AudioServer` was script-DISPATCHED but absent from ClassDB entirely, so
//! `ClassDB.class_exists("AudioServer")` answered false and `get_method_list`
//! was empty while `AudioServer.set_bus_volume_db(..)` worked — and its sibling
//! singletons (`Engine`, `OS`, `Time`) are all registered. `AudioBusLayout` was
//! parsed at boot (`bootstrap.rs` -> `AudioMixer::load_bus_layout_tres`) but was
//! not a class and could not be loaded or applied from script at all.
//!
//! On top of that, a crate-wide grep for each quoted name returned **zero** for
//! `add_bus_effect`, `remove_bus_effect`, `get_bus_effect`,
//! `get_bus_effect_count`, `swap_bus_effects`, `set_bus_effect_enabled`,
//! `set_bus_send`, `get_bus_send`, `set_bus_count`, `move_bus`,
//! `set_bus_volume_linear`, `set_bus_layout` and `generate_bus_layout`: a
//! ported project could not build a bus graph from script.
//!
//! # Why this is two boots inside one test
//!
//! 🔑 `AudioMixer::load_bus_layout` ends with `self.buses = buses` — applying a
//! layout REPLACES the bus vector wholesale, and with it every effect chain. So
//! the "mix output changes" clause and the "a layout replaces the buses" clause
//! cannot be observed in one mixer: whichever ran second would erase the other's
//! evidence, and the effect assertions would fail for a reason that has nothing
//! to do with the effect API. Phase A drives the per-bus API; phase B drives the
//! layout. The bead names one test, so they live in one `#[test]`.
//!
//! # How each clause is kept from passing vacuously
//!
//! * **`ran`** is set on the script's LAST line and asserted FIRST. A missing
//!   dispatch arm does not return Nil — it falls through to `UndefinedFunction`,
//!   which ABORTS the calling frame. So an unimplemented method leaves every
//!   variable below it at its declared default, which for a `== 0`-style
//!   assertion can look like a pass. `ran` separates "the API answered" from
//!   "the script stopped at line 3".
//! * **The amplify gain is -12 dB, not the `AudioEffectAmplify` default.** The
//!   ClassDB default for `volume_db` is `0.0`, which is unity gain: the chain
//!   would return the buffer BIT-IDENTICAL and "the mix output changes" would be
//!   unobservable. ⚠️ Two numbers equal to the last digit mean the input was
//!   never read, so a 0 dB fixture could not distinguish a working chain from a
//!   dropped effect.
//! * **The mix clause carries a control**: the same buffer goes through bus 1,
//!   which has no effects, and must come back untouched. A buffer that changed
//!   in BOTH arms would mean something other than the effect is scaling it.
//! * **The send is set twice**, to `"Music"` and then `"Master"`, and the
//!   pre-existing value is recorded. Asserting only the final `"Master"` would
//!   pass against a stub that returns a constant, or against a default that was
//!   already `"Master"`.
//! * **The round trip clobbers between generate and set.** `generate_bus_layout`
//!   then `set_bus_layout` with nothing in between is satisfied by a
//!   `set_bus_layout` that does NOTHING AT ALL. So the script renames bus 1 and
//!   moves its volume to +42 dB first, asserts the clobber landed, and only then
//!   restores — making "the names and volumes came back" evidence that the
//!   layout was really applied.
//!
//! The round trip is asserted as the engine agreeing with ITSELF (post-restore
//! values versus post-layout values) rather than against numbers predicted here,
//! so it cannot be satisfied by two serializers sharing one bug.
//!
//! # ⚠️ Why the fixture works in `_process` and not `_ready`
//!
//! A script reaches the AudioServer only inside MainLoop's set/clear bracket
//! (`main_loop.rs:3069`, `:4202`, `:4557`). `SceneTreeAccessor::audio()` returns
//! `None` outside it, and every audio method then no-ops **silently** and
//! reports the type's default — `get_bus_send` comes back as `""` through
//! `unwrap_or_default()`, which reads exactly like a setter that did not work.
//!
//! 🔑 `patina-runner`'s boot calls `LifecycleManager::enter_tree` DIRECTLY
//! (`bootstrap.rs:562`) instead of `MainLoop::enter_tree_with_scripts`, so the
//! boot path has no audio server during `_ready` — while the MainLoop path does.
//! A game's `_ready` therefore cannot touch AudioServer under the runner, which
//! is a parity gap in its own right and is filed separately; this test does its
//! work in `_process`, which the engine's own comment at `:4199` calls
//! "ordinary Godot" for AudioServer access.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// Booting touches process-wide audio and ClassDB state.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

const SR: u32 = 48_000;

/// The gain the script authors onto the effect. Deliberately NOT the 0.0 dB
/// ClassDB default, which would be unity gain and bit-identical output.
const AMP_DB: f32 = -12.0;

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk90_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create fixture project dir");
    dir
}

/// Reads a script variable's CURRENT value off the node's script instance.
///
/// 🔑 Two readers are wrong here and this is the third.
///
/// ⚠️ NOT `node.get_property(..)`: script vars reach the NODE's property map
/// only once a process tick syncs them, and relying on that sync would couple
/// every assertion here to how many frames the test happens to run.
///
/// ⚠️ NOT `list_properties().find(..).default_value`, which several tests in
/// this repo use: that field is a *description of the shape*, and reading state
/// out of it burned three verify cycles on pat-mgixk.73 — the reported values
/// were the fixture's declared initializers every time.
///
/// `ScriptInstance::get_property` goes to `interpreter.environment.get(name)`,
/// the live scope. `SceneTree` itself spells the pair out correctly: list for
/// the NAMES, get for the VALUES (`scene_tree.rs:5983`).
fn script_var(tree: &gdscene::SceneTree, node_id: gdscene::node::NodeId, var: &str) -> Variant {
    tree.get_script(node_id)
        .and_then(|s| s.get_property(var))
        // `Nil` for a name the script does not declare, so a typo in an
        // assertion fails loudly rather than reading as a plausible default.
        .unwrap_or(Variant::Nil)
}

fn boot(dir: &Path) -> EngineBootstrap {
    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.to_path_buf(),
        headless: true,
        ..BootConfig::default()
    };
    let mut b = EngineBootstrap::new(config);
    b.run_all().expect("boot flow should complete");
    b
}

fn write_common(dir: &Path, script: &str) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"AudioServerApi\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .expect("write project.godot");
    // ⚠️ Root is a plain `Node`: nothing here needs a transform, and a typed
    // root only adds class-membership surface the assertions would have to
    // account for.
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"Script\" path=\"res://audio.gd\" id=\"1_audio\"]\n\n\
         [node name=\"Main\" type=\"Node\"]\n\
         script = ExtResource(\"1_audio\")\n",
    )
    .expect("write main.tscn");
    std::fs::write(dir.join("audio.gd"), script).expect("write audio.gd");
}

// -- phase A: buses, effects, sends, linear volume ---------------------------

const PHASE_A_GD: &str = r#"extends Node

var cls_server = false
var cls_layout = false
var send_before = "?"
var send_music = "?"
var send_master = "?"
var fx_count = -1
var fx_disabled = true
var fx_reenabled = false
var vol_db = 999.0
var ran = 0

# ⚠️ `_process`, NOT `_ready`. The AudioServer is only reachable from a script
# inside MainLoop's set/clear bracket, and `patina-runner`'s boot dispatches
# `_ready` through `LifecycleManager::enter_tree` directly, outside it — so in
# `_ready` every AudioServer call is a silent no-op that reports defaults.
func _process(_delta):
    if ran == 1:
        return
    cls_server = ClassDB.class_exists("AudioServer")
    cls_layout = ClassDB.class_exists("AudioBusLayout")

    AudioServer.add_bus()
    AudioServer.set_bus_name(1, "Fx")
    AudioServer.add_bus()
    AudioServer.set_bus_name(2, "Music")

    send_before = AudioServer.get_bus_send(1)
    AudioServer.set_bus_send(1, "Music")
    send_music = AudioServer.get_bus_send(1)
    AudioServer.set_bus_send(1, "Master")
    send_master = AudioServer.get_bus_send(1)

    var amp = AudioEffectAmplify.new()
    amp.volume_db = -12.0
    AudioServer.add_bus_effect(0, amp)
    fx_count = AudioServer.get_bus_effect_count(0)

    AudioServer.set_bus_effect_enabled(0, 0, false)
    fx_disabled = AudioServer.is_bus_effect_enabled(0, 0)
    AudioServer.set_bus_effect_enabled(0, 0, true)
    fx_reenabled = AudioServer.is_bus_effect_enabled(0, 0)

    AudioServer.set_bus_volume_linear(0, 0.5)
    vol_db = AudioServer.get_bus_volume_db(0)
    ran = 1
"#;

// -- phase B: layout apply + generate round trip -----------------------------

/// Three buses with distinct, non-default volumes, so "the layout replaced the
/// buses" is visible in both the names and the gains.
const ALT_LAYOUT_TRES: &str = r#"[gd_resource type="AudioBusLayout" format=3]

[resource]
bus/0/name = "Master"
bus/0/volume_db = -3.0
bus/1/name = "Music"
bus/1/volume_db = -9.0
bus/1/send = "Master"
bus/2/name = "Sfx"
bus/2/volume_db = -15.0
bus/2/send = "Master"
"#;

/// Reads each bus by index rather than looping, so no assertion depends on
/// `range()`/`str()` formatting — the volumes come back as Floats and are
/// compared as numbers.
const PHASE_B_GD: &str = r#"extends Node

var loaded_class = "?"
var count_before = -1
var count_after = -1
var n0 = "?"
var n1 = "?"
var n2 = "?"
var v0 = 999.0
var v1 = 999.0
var v2 = 999.0
var clobbered_name = "?"
var clobbered_vol = 999.0
var rt_count = -1
var rt_n0 = "?"
var rt_n1 = "?"
var rt_n2 = "?"
var rt_v0 = 999.0
var rt_v1 = 999.0
var rt_v2 = 999.0
var ran = 0

# `_process` for the same reason as phase A: `_ready` runs outside the bracket
# that makes the AudioServer reachable.
func _process(_delta):
    if ran == 1:
        return
    count_before = AudioServer.get_bus_count()

    var layout = load("res://alt_bus_layout.tres")
    loaded_class = layout.get_class()
    AudioServer.set_bus_layout(layout)
    count_after = AudioServer.get_bus_count()
    n0 = AudioServer.get_bus_name(0)
    n1 = AudioServer.get_bus_name(1)
    n2 = AudioServer.get_bus_name(2)
    v0 = AudioServer.get_bus_volume_db(0)
    v1 = AudioServer.get_bus_volume_db(1)
    v2 = AudioServer.get_bus_volume_db(2)

    var snap = AudioServer.generate_bus_layout()

    AudioServer.set_bus_name(1, "CLOBBERED")
    AudioServer.set_bus_volume_db(1, 42.0)
    clobbered_name = AudioServer.get_bus_name(1)
    clobbered_vol = AudioServer.get_bus_volume_db(1)

    AudioServer.set_bus_layout(snap)
    rt_count = AudioServer.get_bus_count()
    rt_n0 = AudioServer.get_bus_name(0)
    rt_n1 = AudioServer.get_bus_name(1)
    rt_n2 = AudioServer.get_bus_name(2)
    rt_v0 = AudioServer.get_bus_volume_db(0)
    rt_v1 = AudioServer.get_bus_volume_db(1)
    rt_v2 = AudioServer.get_bus_volume_db(2)
    ran = 1
"#;

/// **The acceptance test.**
#[test]
fn audio_server_bus_effect_and_layout_api() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // =====================================================================
    // Phase A — buses, effects, sends, linear volume
    // =====================================================================
    let dir_a = unique_project_dir("api");
    write_common(&dir_a, PHASE_A_GD);
    let mut boot_a = boot(&dir_a);
    // One frame, so `_process` runs inside the bracket that makes the
    // AudioServer reachable from script. The fixture guards itself with `ran`,
    // so extra frames are harmless.
    boot_a
        .main_loop_mut()
        .expect("Running phase builds a MainLoop")
        .step(1.0 / 60.0);

    {
        let tree = boot_a
            .main_loop()
            .expect("Running phase builds a MainLoop")
            .tree();
        let main = tree
            .get_node_by_path("/root/Main")
            .expect("main.tscn instanced its root");
        let var = |k: &str| script_var(tree, main, k);

        // -- 0. the script got all the way through ------------------------
        assert_eq!(
            var("ran"),
            Variant::Int(1),
            "`_process` must reach its last line. A 0 here means one of the \
             AudioServer calls above is not dispatched: `UndefinedFunction` \
             aborts the frame, so every variable below that call keeps its \
             declared default and the clauses that follow are vacuous"
        );

        // -- 1. both classes exist in ClassDB -----------------------------
        assert_eq!(
            var("cls_server"),
            Variant::Bool(true),
            "ClassDB.class_exists(\"AudioServer\") must be true — the singleton \
             was dispatched but unregistered, unlike Engine/OS/Time"
        );
        assert_eq!(
            var("cls_layout"),
            Variant::Bool(true),
            "ClassDB.class_exists(\"AudioBusLayout\") must be true — it was only \
             ever a boot-time parse, never a class"
        );

        // -- 2. set_bus_send actually routes ------------------------------
        //
        // Both writes are checked. The second alone would pass against a
        // getter returning a constant, or against a default already "Master".
        let (before, music, master) = (var("send_before"), var("send_music"), var("send_master"));
        assert_eq!(
            music,
            Variant::String("Music".into()),
            "set_bus_send(1, \"Music\") must be readable back as \"Music\"; got \
             {music:?} (the bus started at {before:?})"
        );
        assert_eq!(
            master,
            Variant::String("Master".into()),
            "set_bus_send(1, \"Master\") must then route bus 1 to Master; got {master:?}"
        );
        assert_ne!(
            before,
            Variant::String("Music".into()),
            "the fixture is vacuous if bus 1's send was ALREADY \"Music\" before \
             the first write — pick a different intermediate bus name"
        );

        // -- 3. the effect was added, and is the authored class -----------
        assert_eq!(
            var("fx_count"),
            Variant::Int(1),
            "add_bus_effect(0, AudioEffectAmplify.new()) must make \
             get_bus_effect_count(0) == 1"
        );

        // -- 4. enable/disable round-trips --------------------------------
        assert_eq!(
            var("fx_disabled"),
            Variant::Bool(false),
            "is_bus_effect_enabled must report false after \
             set_bus_effect_enabled(.., false). `true` means the setter was a \
             no-op and the getter answers a constant"
        );
        assert_eq!(
            var("fx_reenabled"),
            Variant::Bool(true),
            "...and true again after re-enabling"
        );

        // -- 5. set_bus_volume_linear(0, 0.5) => ~ -6.02 dB ---------------
        match var("vol_db") {
            Variant::Float(db) => assert!(
                (db + 6.0206).abs() < 0.01,
                "set_bus_volume_linear(0, 0.5) must read back as ~-6.02 dB \
                 (20*log10(0.5)); got {db}"
            ),
            other => panic!("`vol_db` should be a Float, got {other:?}"),
        }
    }

    // -- 6. 🔑 the mix output CHANGES, with a control --------------------
    //
    // Measured on the engine's own chain after the script built it. The
    // control arm (bus 1, no effects) is what makes this a differential: if
    // both arms changed, something other than the effect is scaling the
    // buffer.
    {
        let server = boot_a
            .main_loop_mut()
            .expect("MainLoop")
            .audio_bridge_mut()
            .server_mut();

        assert_eq!(
            server.bus_effect_class(0, 0),
            Some("AudioEffectAmplify"),
            "Master's slot 0 must hold an AudioEffectAmplify. `None` means the \
             script's resource never became an effect — add_bus_effect \
             accepted it and dropped it"
        );

        let expected = 10f32.powf(AMP_DB / 20.0);

        let mut measured = vec![1.0f32; 8];
        server
            .mixer_mut()
            .get_bus_mut(0)
            .expect("Master bus")
            .effects_mut()
            .process(&mut measured, SR, 1);

        let mut control = vec![1.0f32; 8];
        server
            .mixer_mut()
            .get_bus_mut(1)
            .expect("the script added bus 1")
            .effects_mut()
            .process(&mut control, SR, 1);

        assert!(
            control.iter().all(|s| (*s - 1.0).abs() < 1e-6),
            "CONTROL: bus 1 has no effects, so its buffer must come back \
             untouched. Got {control:?} — if this arm changed too, the scaling \
             is not coming from the effect under test"
        );
        assert!(
            measured.iter().all(|s| (*s - expected).abs() < 1e-3),
            "Master's chain must apply the authored {AMP_DB} dB, i.e. scale 1.0 \
             to ~{expected:.4}. Got {measured:?}. All 1.0 means the effect is \
             present but processed nothing — a bit-identical buffer is the \
             signature of an input that never arrived"
        );
    }

    // =====================================================================
    // Phase B — a layout is loadable, applicable, and round-trips
    // =====================================================================
    let dir_b = unique_project_dir("layout");
    write_common(&dir_b, PHASE_B_GD);
    std::fs::write(dir_b.join("alt_bus_layout.tres"), ALT_LAYOUT_TRES)
        .expect("write alt_bus_layout.tres");
    let mut boot_b = boot(&dir_b);
    boot_b.main_loop_mut().expect("MainLoop").step(1.0 / 60.0);

    let tree = boot_b.main_loop().expect("MainLoop").tree();
    let main = tree
        .get_node_by_path("/root/Main")
        .expect("main.tscn instanced its root");
    let var = |k: &str| script_var(tree, main, k);

    assert_eq!(
        var("ran"),
        Variant::Int(1),
        "phase B's `_process` must reach its last line; a 0 means one of \
         load()/set_bus_layout/generate_bus_layout aborted the frame"
    );

    // -- 7. load() hands back a real AudioBusLayout ----------------------
    assert_eq!(
        var("loaded_class"),
        Variant::String("AudioBusLayout".into()),
        "load(\"res://alt_bus_layout.tres\").get_class() must be \
         \"AudioBusLayout\" — the bead's requirement that a layout .tres be \
         loadable from script as that class"
    );

    // -- 8. applying it REPLACES the buses -------------------------------
    let (before, after) = match (var("count_before"), var("count_after")) {
        (Variant::Int(b), Variant::Int(a)) => (b, a),
        other => panic!("bus counts should be Ints, got {other:?}"),
    };
    assert_eq!(
        after, 3,
        "the layout declares 3 buses, so get_bus_count() must be 3 after \
         set_bus_layout; got {after}"
    );
    assert!(
        before < 3,
        "the fixture is vacuous if the project already had 3 buses before the \
         layout was applied (it had {before}) — then 'replaces the buses' is \
         indistinguishable from doing nothing"
    );
    assert_eq!(var("n0"), Variant::String("Master".into()));
    assert_eq!(
        var("n1"),
        Variant::String("Music".into()),
        "bus 1 must be the layout's \"Music\""
    );
    assert_eq!(
        var("n2"),
        Variant::String("Sfx".into()),
        "bus 2 must be the layout's \"Sfx\""
    );
    // The authored volumes, which also prove the .tres VALUES were read and not
    // just its bus count.
    for (key, want) in [("v0", -3.0f64), ("v1", -9.0), ("v2", -15.0)] {
        match var(key) {
            Variant::Float(db) => assert!(
                (db - want).abs() < 1e-3,
                "{key} must be the layout's authored {want} dB, got {db}"
            ),
            other => panic!("`{key}` should be a Float, got {other:?}"),
        }
    }

    // -- 9. 🔑 the clobber landed, so the restore below is not vacuous ----
    assert_eq!(
        var("clobbered_name"),
        Variant::String("CLOBBERED".into()),
        "the script renames bus 1 between generate_bus_layout() and \
         set_bus_layout(). If this rename did not take, the round-trip \
         assertions below would pass against a set_bus_layout that does nothing"
    );
    match var("clobbered_vol") {
        Variant::Float(db) => assert!(
            (db - 42.0).abs() < 1e-3,
            "...and its volume must really have moved to +42 dB first, got {db}"
        ),
        other => panic!("`clobbered_vol` should be a Float, got {other:?}"),
    }

    // -- 10. generate_bus_layout() round-trips names AND volumes ---------
    //
    // Compared against the POST-LAYOUT readings rather than literals: the
    // engine has to agree with itself across generate -> clobber -> set.
    assert_eq!(
        var("rt_count"),
        Variant::Int(3),
        "restoring the generated layout must bring the bus count back to 3"
    );
    for (rt, orig) in [("rt_n0", "n0"), ("rt_n1", "n1"), ("rt_n2", "n2")] {
        assert_eq!(
            var(rt),
            var(orig),
            "generate_bus_layout() must round-trip bus NAMES: {rt} != {orig}. \
             A \"CLOBBERED\" here means the snapshot did not carry the name \
             back; a \"Bus1\" means it carried no name at all and the loader \
             defaulted it"
        );
    }
    for (rt, orig) in [("rt_v0", "v0"), ("rt_v1", "v1"), ("rt_v2", "v2")] {
        assert_eq!(
            var(rt),
            var(orig),
            "generate_bus_layout() must round-trip bus VOLUMES exactly: \
             {rt} != {orig}. A 42 here means the generated layout omitted \
             volume_db, so the clobbered gain survived the restore"
        );
    }
}

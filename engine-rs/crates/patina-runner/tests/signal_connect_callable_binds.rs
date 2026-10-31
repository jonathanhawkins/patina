//! **Acceptance test for pat-mgixk.98, clause 1.** A script's `Callable.bind()`
//! arguments and `.unbind()` count survive `connect`, and Godot 4's
//! `disconnect(signal, Callable)` / `is_connected(signal, Callable)` shapes work.
//!
//! # What was broken
//!
//! Both script connect paths passed `&[]` for binds — `Signal.connect` and
//! `Object.connect` — because `callable_target` resolves a Callable through
//! [`CallableRef::inner_callable`], whose whole job is to strip the
//! `Bound`/`Unbound` wrappers. So:
//!
//! * `btn.pressed.connect(_on_pressed.bind(i))` silently dropped `i`, and the
//!   handler was invoked with the signal's own arguments only;
//! * `sig.connect(handler.unbind(1))` never applied the unbind, so a handler
//!   declaring fewer parameters than the signal raised instead of ignoring the
//!   tail;
//! * `obj.disconnect("sig", callable)` and `obj.is_connected("sig", callable)`
//!   — the only shapes Godot 4 documents — raised
//!   `"needs signal, target, method"`, because only the legacy
//!   `(signal, ObjectId, method)` triple was accepted. A script could connect a
//!   Callable but had no way to take it off again;
//! * `Object.connect` returned `Nil` where Godot returns an `Error` (`OK` == 0).
//!
//! The `binds`/`unbinds` plumbing already existed on `Connection` and in
//! `emit_signal` (`Connection::resolve_args`); only the script connect path
//! threw the values away.
//!
//! # Wiring
//!
//! The bead's requirement is `script connect -> SceneTree SignalStore ->
//! emit_signal uses c.resolve_args`, so this boots a real project through
//! `patina_runner` (`project.godot` + `main.tscn` + GDScript), connects in
//! `_ready`, and the **test** emits through `SceneTree::emit_signal`.
//!
//! ⚠️ The emit is driven from the test rather than from inside a script method,
//! and that is load-bearing, not stylistic: a script that is *currently
//! running* is checked out of `SceneTree.scripts`, so a signal emitted from
//! within `_ready` could not be delivered back into that same script.
//! `signal_value_connect_emit.rs:20-24` documents the same constraint for the
//! pat-0q2kx.9 acceptance, which is the green reference this test follows.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `EngineBootstrap` touches process-global state (ClassDB, resource roots), so
/// the runner tests serialize on one lock.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

/// Arguments the two-argument signal is emitted with. ⚠️ Deliberately distinct
/// and neither 0 nor -1: the vars they land in start at -1, and `A` must be
/// distinguishable from `B` so "kept the wrong end" is a visible failure rather
/// than a coincidence.
const TWO_A: i64 = 11;
const TWO_B: i64 = 22;

/// `_on_pick` folds each delivery into one integer: `sum = sum * 10 + v + 1`.
///
/// 🔑 Deliberately an encoding rather than three separate variables or an
/// Array. It pins the **values and their order in one assertion**, with no
/// dependency on GDScript builtins (no `str()`, no `Array.append`, no
/// subscripting) — and `.unbind()` is already untested ground in this suite, so
/// everything else here stays on syntax a green test already proves.
///
/// `+ 1` so a bound 0 is distinguishable from no delivery at all: emitting
/// 0, 1, 2 in order gives `((0*10+1)*10+2)*10+3` = **123**. The failure modes
/// read off the number directly:
///
/// | observed | meaning |
/// |---|---|
/// | 123 | correct: each connection delivered its own bound value, in order |
/// | 0   | `_on_pick` never ran |
/// | 111 | every connection delivered the same value (binds collapsed) |
/// | 1   | only one of the three connections delivered |
const PICKS_CORRECT: i64 = 123;

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk98_binds_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// ⚠️ The three `Button`s come from the **scene file**, not `Button.new()`.
/// The bead's prose says "creates 3 Buttons in a loop", but what it is actually
/// asserting is that three connections to the *same method* with *different*
/// bound values each deliver their own — which is identical either way, and
/// authoring them in the `.tscn` enters through the scene loader instead of
/// adding a `Button.new()` dependency this bead does not touch.
///
/// `pressed` is a real declared signal here, inherited from `BaseButton`
/// (`class_db.rs:16886`), not one lazily created by `connect`.
const MAIN_TSCN: &str = r#"[gd_scene load_steps=3 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]
[ext_resource type="Script" path="res://two.gd" id="2_two"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")

[node name="Pick0" type="Button" parent="."]

[node name="Pick1" type="Button" parent="."]

[node name="Pick2" type="Button" parent="."]

[node name="Two" type="Node2D" parent="."]
script = ExtResource("2_two")
"#;

/// Declares the signal `.unbind(1)` is tested against.
///
/// ⚠️ Declared with no parameter list. Godot allows `signal two_args(a, b)`, but
/// the green fixtures in this suite all use the bare form and a signal's
/// declared arity is not what this test is about — the emit below supplies two
/// arguments regardless.
const TWO_GD: &str = "\
extends Node2D
signal two_args
";

/// ⚠️ Writes go through `self.` in the handlers. Both idioms appear in green
/// fixtures, and this is the one `call_deferred_native_methods.rs` uses for a
/// handler that has to persist a value past its own return.
const MAIN_GD: &str = "\
extends Node2D

var picks = 0
var pick_count = 0
var one_got = -1
var one_argc = -1
var sig_connect_code = -1
var obj_connect_code = -1
var connected_before = false
var connected_after = true
var ready_done = false

func _ready():
    sig_connect_code = $Pick0.pressed.connect(_on_pick.bind(0))
    $Pick1.pressed.connect(_on_pick.bind(1))
    $Pick2.pressed.connect(_on_pick.bind(2))
    obj_connect_code = $Two.connect(\"two_args\", _on_one.unbind(1))
    connected_before = $Pick0.is_connected(\"pressed\", _on_pick)
    ready_done = true

func _on_pick(v):
    self.picks = self.picks * 10 + v + 1
    self.pick_count += 1

func _on_one(a):
    self.one_got = a
    self.one_argc = 1

func drop():
    $Pick0.disconnect(\"pressed\", _on_pick)
    connected_after = $Pick0.is_connected(\"pressed\", _on_pick)
";

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"ConnectBinds\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN_TSCN).unwrap();
    std::fs::write(dir.join("main.gd"), MAIN_GD).unwrap();
    std::fs::write(dir.join("two.gd"), TWO_GD).unwrap();
}

fn boot(dir: &Path) -> EngineBootstrap {
    // Button is a Control class, and ClassDB is not populated implicitly.
    gdobject::class_db::register_2d_classes();
    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.to_path_buf(),
        headless: true,
        ..BootConfig::default()
    };
    let mut boot = EngineBootstrap::new(config);
    boot.run_all().expect("boot succeeds");
    boot
}

/// ⚠️ `get_property`, NOT `list_properties().default_value`.
/// `list_properties` reports **parse-time** metadata, so a reader built on it
/// returns each variable's declared initializer forever and every assertion
/// below would read its own sentinel no matter what the engine did.
fn script_var(tree: &gdscene::SceneTree, node: gdscene::node::NodeId, var: &str) -> Variant {
    tree.get_script(node)
        .expect("node has a script")
        .get_property(var)
        .unwrap_or(Variant::Nil)
}

fn as_int(v: Variant, what: &str) -> i64 {
    match v {
        Variant::Int(i) => i,
        other => panic!("`{what}` should be an Int, got {other:?}"),
    }
}

/// **The acceptance test.**
#[test]
fn signal_connect_callable_binds() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("main");
    write_project(&dir);

    let mut boot = boot(&dir);

    let (main_id, pick0, pick1, pick2, two_id) = {
        let tree = boot.main_loop().expect("main loop built").tree();
        (
            tree.get_node_by_path("/root/Main").expect("Main"),
            tree.get_node_by_path("/root/Main/Pick0").expect("Pick0"),
            tree.get_node_by_path("/root/Main/Pick1").expect("Pick1"),
            tree.get_node_by_path("/root/Main/Pick2").expect("Pick2"),
            tree.get_node_by_path("/root/Main/Two").expect("Two"),
        )
    };

    // -- 0. `_ready` ran to completion --------------------------------------
    //
    // 🔑 Checked FIRST, because at HEAD this is the assertion that fires. A
    // raise aborts the rest of `_ready`, so every var below would read its
    // sentinel and look like four separate dropped deliveries instead of one
    // runtime error. Before this bead,
    // `$Pick0.is_connected("pressed", _on_pick)` raised
    // "is_connected() needs signal, target, method" — the Callable form did not
    // exist — so `ready_done` stayed false.
    {
        let tree = boot.main_loop().expect("main loop").tree();
        assert_eq!(
            script_var(tree, main_id, "ready_done"),
            Variant::Bool(true),
            "_ready must run to completion. `false` means it raised partway: \
             most likely `$Pick0.is_connected(\"pressed\", _on_pick)`, which \
             needs Godot 4's (signal, Callable) shape, or \
             `_on_one.unbind(1)` failing to resolve"
        );
        // Nothing has been emitted yet, so a handler that fired here would mean
        // the connect itself dispatched.
        assert_eq!(
            as_int(script_var(tree, main_id, "pick_count"), "pick_count"),
            0,
            "connecting must not invoke the handler"
        );
    }

    // -- 1. connect returns Godot's Error, and is_connected sees a Callable --
    {
        let tree = boot.main_loop().expect("main loop").tree();
        assert_eq!(
            as_int(
                script_var(tree, main_id, "sig_connect_code"),
                "sig_connect_code"
            ),
            0,
            "Signal.connect returns an Error; OK == 0"
        );
        assert_eq!(
            as_int(
                script_var(tree, main_id, "obj_connect_code"),
                "obj_connect_code"
            ),
            0,
            "Object.connect must return Godot's Error (OK == 0). It returned \
             `Nil` before this bead, which reads as -1 here because the var \
             keeps its sentinel when the assignment yields Nil"
        );
        assert_eq!(
            script_var(tree, main_id, "connected_before"),
            Variant::Bool(true),
            "is_connected(signal, Callable) must find the connection that \
             connect(Callable) made. `false` with `ready_done` true would mean \
             the two resolve a Callable to different (target, method) pairs"
        );
    }

    // -- 2. 🔑 each connection delivers its OWN bound argument ---------------
    //
    // The TREE emits, in a known order, with NO signal arguments — so every
    // value the handler sees can only have come from `.bind()`.
    for id in [pick0, pick1, pick2] {
        boot.main_loop_mut()
            .expect("main loop")
            .tree_mut()
            .emit_signal(id, "pressed", &[]);
    }

    {
        let tree = boot.main_loop().expect("main loop").tree();
        assert_eq!(
            as_int(script_var(tree, main_id, "pick_count"), "pick_count"),
            3,
            "all three `pressed` connections must dispatch"
        );
        assert_eq!(
            as_int(script_var(tree, main_id, "picks"), "picks"),
            PICKS_CORRECT,
            "each connection must deliver its own bound value in order \
             (see PICKS_CORRECT: 0 = never ran, 111 = every connection \
             delivered the same value because the Bound layer was stripped, \
             1 = only one connection delivered)"
        );
    }

    // -- 3. 🔑 `.unbind(1)` drops the trailing signal argument ---------------
    //
    // The handler declares ONE parameter and the signal carries two, so without
    // the unbind this is an arity mismatch: the call raises and `one_got` keeps
    // its sentinel. With it, the handler sees the FIRST argument only.
    boot.main_loop_mut()
        .expect("main loop")
        .tree_mut()
        .emit_signal(
            two_id,
            "two_args",
            &[Variant::Int(TWO_A), Variant::Int(TWO_B)],
        );

    {
        let tree = boot.main_loop().expect("main loop").tree();
        assert_eq!(
            as_int(script_var(tree, main_id, "one_argc"), "one_argc"),
            1,
            "the `.unbind(1)` handler must be invoked at all. -1 means it was \
             never reached — with the unbind dropped, a 1-parameter handler \
             called with {} and {} is an arity error",
            TWO_A,
            TWO_B
        );
        assert_eq!(
            as_int(script_var(tree, main_id, "one_got"), "one_got"),
            TWO_A,
            "`.unbind(1)` drops the LAST argument, so the handler keeps the \
             first. {} here would mean the wrong end was dropped",
            TWO_B
        );
    }

    // -- 4. disconnect(signal, Callable) actually disconnects -----------------
    boot.main_loop_mut()
        .expect("main loop")
        .tree_mut()
        .call_script_method(main_id, "drop", &[]);

    {
        let tree = boot.main_loop().expect("main loop").tree();
        assert_eq!(
            script_var(tree, main_id, "connected_after"),
            Variant::Bool(false),
            "after disconnect(signal, Callable), is_connected must answer false"
        );
    }

    // And it is a real disconnect, not just a bookkeeping flag: re-emitting on
    // the disconnected button must not reach the handler, while the other two
    // still do.
    boot.main_loop_mut()
        .expect("main loop")
        .tree_mut()
        .emit_signal(pick0, "pressed", &[]);

    {
        let tree = boot.main_loop().expect("main loop").tree();
        assert_eq!(
            as_int(script_var(tree, main_id, "pick_count"), "pick_count"),
            3,
            "Pick0 was disconnected, so its `pressed` must no longer dispatch — \
             4 means `is_connected` reported a removal that never happened"
        );
    }

    boot.main_loop_mut()
        .expect("main loop")
        .tree_mut()
        .emit_signal(pick1, "pressed", &[]);

    {
        let tree = boot.main_loop().expect("main loop").tree();
        assert_eq!(
            as_int(script_var(tree, main_id, "pick_count"), "pick_count"),
            4,
            "disconnecting Pick0 must not disturb Pick1's own connection — a \
             disconnect keyed on (target, method) alone, ignoring WHICH \
             emitter's store it lives in, would have removed all three"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

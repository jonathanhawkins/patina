//! pat-0q2kx.9 acceptance: Godot 4's `Signal` builtin, end to end from a
//! scene script.
//!
//! Before this, Patina had **no** `Variant::Signal` at all (repo-wide
//! `rg 'Variant::Signal|VariantType::Signal'` = 0 hits):
//!
//! * `$B.my_signal` read as a *property*, so it evaluated to Nil and
//!   `$B.my_signal.connect(...)` was impossible;
//! * the only `connect` shape was the removed-in-Godot-4
//!   `connect(sig, ObjectId, "method")` — a `Callable` second argument raised
//!   `TypeError: connect() target must be ObjectId`;
//! * a bareword signal became a name-only await token whose resumption matched
//!   **globally by name**, so two nodes' same-named awaits cross-resumed.
//!
//! The scripts here construct nothing by hand: the signal is declared in
//! GDScript, connected in GDScript, and emitted by the **tree** — which is the
//! bead's stated wiring requirement ("a `Button.pressed.connect` from a scene
//! script fires the handler when the tree emits pressed").
//!
//! ⚠️ The emit is deliberately driven from the test rather than from inside
//! `_ready`: a script that is *currently running* is checked out of
//! `SceneTree.scripts`, so a signal emitted from within `_ready` could not be
//! delivered back into that same script. Emitting from the tree exercises the
//! real path anyway.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

static BOOT_LOCK: Mutex<()> = Mutex::new(());

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_0q2kx9_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const MAIN_TSCN: &str = r#"[gd_scene load_steps=4 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]
[ext_resource type="Script" path="res://b.gd" id="2_b"]
[ext_resource type="Script" path="res://c.gd" id="3_c"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")

[node name="B" type="Node2D" parent="."]
script = ExtResource("2_b")

[node name="C" type="Node2D" parent="."]
script = ExtResource("3_c")
"#;

/// Declares the signal. Nothing else — the point is that a *declared* signal is
/// resolvable before anything has ever connected to it.
const B_GD: &str = "\
extends Node2D
signal my_signal
";

/// The cross-object target for `Callable($C, \"m\")`.
const C_GD: &str = "\
extends Node2D
var seen = 0

func m(v):
    seen = v
";

/// ⚠️ `_on_sig` is passed bare, with no `Callable(...)` wrapper — the Godot 4
/// idiom, and one that did not parse to a Callable in Patina before this bead.
const MAIN_GD: &str = "\
extends Node2D
var got = 0
var count = 0
var connected_after_connect = false
var connected_after_disconnect = true
var sig_name = \"\"
var sig_is_null = true

func _ready():
    var sig = $B.my_signal
    sig_name = sig.get_name()
    sig_is_null = sig.is_null()
    sig.connect(_on_sig)
    connected_after_connect = sig.is_connected(_on_sig)
    $B.connect(\"my_signal\", Callable($C, \"m\"))

func _on_sig(v):
    got = v
    count += 1

func drop():
    var sig = $B.my_signal
    sig.disconnect(_on_sig)
    connected_after_disconnect = sig.is_connected(_on_sig)
";

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"SignalBoot\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN_TSCN).unwrap();
    std::fs::write(dir.join("main.gd"), MAIN_GD).unwrap();
    std::fs::write(dir.join("b.gd"), B_GD).unwrap();
    std::fs::write(dir.join("c.gd"), C_GD).unwrap();
}

fn boot(dir: &Path) -> EngineBootstrap {
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

fn script_var(tree: &gdscene::SceneTree, node: gdscene::node::NodeId, var: &str) -> Variant {
    tree.get_script(node)
        .expect("node has a script")
        .list_properties()
        .into_iter()
        .find(|p| p.name == var)
        .map(|p| p.default_value)
        .unwrap_or(Variant::Nil)
}

fn as_int(v: Variant) -> i64 {
    match v {
        Variant::Int(i) => i,
        Variant::Float(f) => f as i64,
        other => panic!("expected a number, got {other:?}"),
    }
}

fn as_bool(v: Variant) -> bool {
    match v {
        Variant::Bool(b) => b,
        other => panic!("expected a bool, got {other:?}"),
    }
}

/// **The acceptance test.**
#[test]
fn signal_value_connect_emit() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("connect_emit");
    write_project(&dir);

    let mut boot = boot(&dir);
    let (main_id, b_id, c_id) = {
        let tree = boot.main_loop().expect("main loop").tree();
        (
            tree.get_node_by_path("/root/Main").expect("Main"),
            tree.get_node_by_path("/root/Main/B").expect("B"),
            tree.get_node_by_path("/root/Main/C").expect("C"),
        )
    };

    // `$B.my_signal` resolved to a real Signal value, not a Nil property read.
    // If it were still Nil, `get_name()` would have raised rather than
    // returning the name.
    {
        let tree = boot.main_loop().unwrap().tree();
        assert_eq!(
            script_var(tree, main_id, "sig_name"),
            Variant::String("my_signal".into()),
            "$B.my_signal must be a Signal carrying its own name"
        );
        assert!(
            !as_bool(script_var(tree, main_id, "sig_is_null")),
            "a Signal bound to a live node and a declared name is not null"
        );
        // `sig.connect(_on_sig)` — a BARE method reference, no Callable() wrapper.
        assert!(
            as_bool(script_var(tree, main_id, "connected_after_connect")),
            "is_connected must see the connection Signal.connect just made"
        );
        // Nothing has emitted yet.
        assert_eq!(as_int(script_var(tree, main_id, "count")), 0);
    }

    // The TREE emits — not the script. This is the bead's wiring requirement.
    boot.main_loop_mut()
        .unwrap()
        .tree_mut()
        .emit_signal(b_id, "my_signal", &[Variant::Int(7)]);

    {
        let tree = boot.main_loop().unwrap().tree();
        assert_eq!(
            as_int(script_var(tree, main_id, "got")),
            7,
            "the handler received the emitted argument"
        );
        assert_eq!(
            as_int(script_var(tree, main_id, "count")),
            1,
            "handler fired exactly once"
        );
        // `$B.connect("my_signal", Callable($C, "m"))` — the Godot 4 two-arg
        // form, dispatching to a DIFFERENT object than the connector.
        assert_eq!(
            as_int(script_var(tree, c_id, "seen")),
            7,
            "Callable($C, \"m\") must dispatch to C, not back to Main"
        );
    }

    // disconnect round-trip.
    let _ = boot
        .main_loop_mut()
        .unwrap()
        .tree_mut()
        .call_script_method(main_id, "drop", &[]);
    {
        let tree = boot.main_loop().unwrap().tree();
        assert!(
            !as_bool(script_var(tree, main_id, "connected_after_disconnect")),
            "is_connected must be false after Signal.disconnect"
        );
    }

    // And the disconnect really took effect: a second emit does not re-fire the
    // handler, while C — still connected — sees the new value.
    boot.main_loop_mut()
        .unwrap()
        .tree_mut()
        .emit_signal(b_id, "my_signal", &[Variant::Int(9)]);
    {
        let tree = boot.main_loop().unwrap().tree();
        assert_eq!(
            as_int(script_var(tree, main_id, "count")),
            1,
            "a disconnected handler must not fire again"
        );
        assert_eq!(
            as_int(script_var(tree, c_id, "seen")),
            9,
            "C stays connected, proving the second emit really happened"
        );
    }
}

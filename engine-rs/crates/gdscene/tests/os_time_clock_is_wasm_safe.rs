//! pat-mgixk.40 acceptance — script clocks and `randomize()` go through
//! wasm-safe time sources.
//!
//! # What was wrong
//!
//! `gdscript-interop/src/interpreter.rs` kept its own `std::time` clocks: a
//! private `PROCESS_EPOCH: OnceLock<Instant>` behind `OS.get_ticks_msec/usec`
//! and `Time.get_ticks_*`, a `SystemTime` read behind
//! `Time.get_unix_time_from_system` and `system_unix_seconds`, another inside
//! `randomize()`, and one more in the script profiler's call timer.
//!
//! **All of those panic on `wasm32-unknown-unknown`** — that target has no
//! monotonic clock and no wall clock. The repo already knew: `gdplatform/os.rs`
//! switched to `web_time` for exactly this reason, and `gdcore/src/rng.rs`
//! cfg-gates its entropy. But `patina-web` reaches the interpreter through
//! `gdscene -> gdscript-interop`, so four of the most ordinary script calls are
//! a crash in the browser — and native tests, which are green, never see it.
//!
//! # The path driven
//!
//! ```text
//! .tscn -> SceneTree -> attached GDScript -> MainLoop::step
//!   -> _ready / _process calling randomize(), OS.get_ticks_msec(),
//!      Time.get_ticks_usec(), Time.get_unix_time_from_system()
//! ```
//!
//! which is the native half of the browser's
//! `rAF -> WebEngine -> MainLoop.step -> _process`.
//!
//! ⚠️ A native test cannot observe the wasm panic itself. So this proves the fix
//! two ways: the script calls really work through `MainLoop` (and `randomize()`
//! really reseeds the stream a script can see), and a source-level guard asserts
//! the panicking constructors are gone from the interpreter. The build half,
//! `cargo check --target wasm32-unknown-unknown -p patina-web`, is reported as a verify command, since a test binary cannot run it.

use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::MainLoop;
use gdvariant::Variant;

/// `_ready` samples every clock once; the first `_process` samples them again,
/// so monotonicity is observed across two `MainLoop::step` calls rather than
/// inside one.
///
/// `randi()` is sampled after each `randomize()` because that is the only
/// script-visible evidence the reseed happened: the interpreter keeps its OWN
/// `RNG_STATE`, separate from `gdcore::rng`'s global generator, so sourcing
/// entropy from `gdcore` without feeding it back would leave this stream
/// unchanged while looking fixed.
const CLOCK_GD: &str = "\
var ticks_a = 0
var usec_a = 0
var unix_t = 0.0
var rand_a = 0
var ticks_b = 0
var usec_b = 0
var rand_b = 0
var steps = 0

func _ready():
    randomize()
    rand_a = randi()
    ticks_a = OS.get_ticks_msec()
    usec_a = Time.get_ticks_usec()
    unix_t = Time.get_unix_time_from_system()

func _process(delta):
    steps = steps + 1
    if steps == 1:
        randomize()
        rand_b = randi()
        ticks_b = OS.get_ticks_msec()
        usec_b = Time.get_ticks_usec()
";

const CLOCK_SCENE: &str = r#"[gd_scene format=3]

[node name="Clock" type="Node2D"]
"#;

fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    tree.get_script(node)
        .expect("the node has a script")
        .get_property(name)
        .unwrap_or_else(|| panic!("script var `{name}` should exist"))
}

fn script_int(tree: &SceneTree, node: NodeId, name: &str) -> i64 {
    match script_var(tree, node, name) {
        Variant::Int(i) => i,
        other => panic!("`{name}` should be an Int, got {other:?}"),
    }
}

fn script_float(tree: &SceneTree, node: NodeId, name: &str) -> f64 {
    match script_var(tree, node, name) {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("`{name}` should be a Float, got {other:?}"),
    }
}

/// Blanks `//` line comments, `/* */` block comments and string literals.
///
/// 🔑 Required, not fastidious: the fix itself documents what it removed, so
/// `interpreter.rs` now contains the literal text of those constructors inside
/// explanatory comments. A guard scanning raw bytes would fail on its own
/// documentation, and the obvious "fix" would be to delete the comments that
/// explain the hazard.
fn strip_comments_and_strings(src: &str) -> String {
    let b: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == '/' && i + 1 < b.len() && b[i + 1] == '/' {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if b[i] == '/' && i + 1 < b.len() && b[i + 1] == '*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') {
                i += 1;
            }
            i = (i + 2).min(b.len());
            continue;
        }
        if b[i] == '"' {
            i += 1;
            while i < b.len() && b[i] != '"' {
                if b[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(b.len());
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

#[test]
fn os_time_clock_is_wasm_safe() {
    // -- (1) the clocks work through MainLoop, from a script ----------------

    // Prime the engine's monotonic epoch BEFORE the script runs. `get_ticks_*`
    // measures from its first call, so without this the script's own first read
    // would legitimately be 0, and the "the clock is live" assertion below could
    // not tell a running clock from a stub returning zero.
    let primed = gdplatform::os::get_ticks_usec();

    let scene = PackedScene::from_tscn(CLOCK_SCENE).expect("the .tscn parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("the scene instances");
    let clock = tree
        .get_node_by_path("/root/Clock")
        .expect("the fixture has /root/Clock");

    tree.attach_script(
        clock,
        Box::new(GDScriptNodeInstance::from_source(CLOCK_GD, clock).expect("the script parses")),
    );

    let mut ml = MainLoop::new(tree);
    // Reaching past here at all is half the point: every call in `_ready` was a
    // std clock constructor that panics on wasm32.
    ml.enter_tree_with_scripts(clock);
    ml.step(1.0 / 60.0);
    ml.step(1.0 / 60.0);

    let tree = ml.tree();
    // `>= 1`, not `== 2`: the script fills its second sample set on the first
    // `_process`, so all this precondition needs is that `_process` ran at all.
    // Pinning the exact call count would couple this bead to MainLoop's
    // scheduling, which it does not touch.
    let steps = script_int(tree, clock, "steps");
    assert!(
        steps >= 1,
        "precondition: _process must have run, or the second sample set is \
         untouched zeros (steps = {steps})",
    );

    let (ticks_a, ticks_b) = (
        script_int(tree, clock, "ticks_a"),
        script_int(tree, clock, "ticks_b"),
    );
    let (usec_a, usec_b) = (
        script_int(tree, clock, "usec_a"),
        script_int(tree, clock, "usec_b"),
    );
    assert!(
        ticks_b >= ticks_a,
        "OS.get_ticks_msec() must not go backwards: {ticks_a} -> {ticks_b}",
    );
    assert!(
        usec_b >= usec_a,
        "Time.get_ticks_usec() must not go backwards: {usec_a} -> {usec_b}",
    );
    // ... and the clock is actually running, not a stub pinned at zero. `>=`
    // alone would pass on a constant.
    assert!(
        usec_a >= primed as i64 && usec_a > 0,
        "the script's tick read ({usec_a}) must be a live clock at or after the \
         {primed} us already on the engine epoch",
    );

    let unix_t = script_float(tree, clock, "unix_t");
    assert!(
        unix_t > 1.7e9,
        "Time.get_unix_time_from_system() must be a real Unix time (> 1.7e9), \
         got {unix_t}",
    );

    // 🔑 randomize() must reseed the stream the SCRIPT sees. The interpreter's
    // RNG_STATE is a different generator from `gdcore::rng`'s global, so a fix
    // that reseeded only `gdcore` would satisfy every assertion above and
    // silently break `randomize()`.
    let (rand_a, rand_b) = (
        script_int(tree, clock, "rand_a"),
        script_int(tree, clock, "rand_b"),
    );
    assert_ne!(
        rand_a, rand_b,
        "two randomize() calls must change what randi() returns to the script",
    );

    // -- (2) the panicking constructors are gone from the interpreter -------

    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../gdscript-interop/src/interpreter.rs"
    );
    let src =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("the guard must find {path}: {e}"));
    assert!(
        src.len() > 100_000,
        "sanity: interpreter.rs is a large file ({} bytes read) — a short read \
         would make the scan below pass vacuously",
        src.len()
    );

    let code = strip_comments_and_strings(&src);

    // Falsify the stripper before trusting it, against a LOCAL probe rather
    // than against interpreter.rs's own comment text — a self-check that
    // depended on the exact wording of a comment in another file would break
    // the moment someone reflowed it, and would then look like a real finding.
    {
        let probe = concat!(
            "let live = 1;\n",
            "// a comment mentioning Instant::now() and SystemTime::now()\n",
            "let msg = \"a string mentioning Instant::now()\";\n",
            "/* a block mentioning UNIX_EPOCH */\n",
            "let also_live = 2;\n"
        );
        let stripped = strip_comments_and_strings(probe);
        assert!(
            !stripped.contains("Instant::now")
                && !stripped.contains("SystemTime::now")
                && !stripped.contains("UNIX_EPOCH"),
            "self-check: the stripper must blank comments AND string literals; \
             got {stripped:?}",
        );
        assert!(
            stripped.contains("let live = 1;") && stripped.contains("let also_live = 2;"),
            "self-check: the stripper must keep real code; got {stripped:?}",
        );
    }

    for needle in ["Instant::now", "SystemTime::now", "UNIX_EPOCH"] {
        assert!(
            !code.contains(needle),
            "`{needle}` is still live code in gdscript-interop/src/interpreter.rs. \
             std Instant/SystemTime panic on wasm32-unknown-unknown and patina-web \
             reaches this module through gdscene, so script clocks must go through \
             gdplatform::os (web_time) and gdcore::rng instead.",
        );
    }
}

/// The engine's shared clock helpers are the ones the interpreter now calls, so
/// a regression there would break scripts too.
#[test]
fn gdplatform_clock_helpers_are_monotonic_and_dated() {
    let a = gdplatform::os::get_ticks_usec();
    let b = gdplatform::os::get_ticks_usec();
    assert!(b >= a, "get_ticks_usec must not go backwards: {a} -> {b}");
    let now = gdplatform::os::unix_time_secs_f64();
    assert!(
        now > 1.7e9,
        "unix_time_secs_f64 must be a real Unix time, got {now}",
    );
}

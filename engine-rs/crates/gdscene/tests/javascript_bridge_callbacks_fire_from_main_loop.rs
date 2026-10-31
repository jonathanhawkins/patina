//! pat-mgixk.41 — `JavaScriptBridge.create_callback` callbacks run from the
//! frame loop, the exported shell defines the hooks the bridge reads, and a PWA
//! update emits `pwa_update_available`.
//!
//! Godot ground truth: `platform/web/javascript_bridge_singleton.cpp` (the
//! `Callable` wrapped by `create_callback` runs when JavaScript calls the
//! function it handed back) and `platform/web/os_web.cpp:258-279`
//! (`update_pwa_state_callback` emits `pwa_update_available`; `force_fs_sync`
//! flushes IDBFS).
//!
//! # What was wrong
//!
//! 1. `Interpreter::pump_javascript_callbacks` had **no runtime caller** — only
//!    tests. So a JS function from `create_callback` queued its invocations
//!    forever and the `Callable` never ran. `create_callback` was documented as
//!    working and did nothing observable.
//! 2. The shell globals `js_bridge` reads by name — `__patina_fs_sync`,
//!    `__patina_pwa_update`, `__patina_pwa_update_pending` — were defined
//!    nowhere, so `force_fs_sync()`, `pwa_needs_update()` and `pwa_update()`
//!    were silent no-ops in an exported game.
//! 3. `pwa_update_available` was registered in ClassDB and emitted by nothing.
//!
//! ⚠️ The callback half is driven by `MainLoop::step` on a parsed `.tscn` with
//! an attached GDScript, as the bead's Wiring requirement asks, and **this test
//! never calls a pump itself** — that absence is the whole point. The shell half
//! asserts on `render_html_shell`'s output, which is the artifact a player's
//! browser actually loads.

use gdplatform::js_bridge::{self, JsValue, RecordingJsBackend};
use gdplatform::web::WebExportConfig;
use gdplatform::web_export;
use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::GDScriptNodeInstance;
use gdvariant::Variant;

const STEP: f64 = 1.0 / 60.0;

const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node"]
"#;

/// ⚠️ The callbacks are **lambdas that call a method**, not bare method
/// references and not lambdas that assign. A one-line GDScript lambda body is an
/// *expression*, so `func(v): self.got = v` does not parse — and a lambda
/// captures by value, so anything it assigned would never escape. Calling a
/// method is the shape `gds_lambda_capture.rs` proves, and it is how the value
/// reaches a member var where the test can see it.
const SCRIPT: &str = "\
extends Node
var got = \"unset\"
var pwa_hits = 0
var ready_ran = false
func _ready():
    JavaScriptBridge.create_callback(func(v): self._on_js(v))
    JavaScriptBridge.connect(\"pwa_update_available\", func(): self._on_pwa())
    self.ready_ran = true
func _on_js(v):
    self.got = v
func _on_pwa():
    self.pwa_hits = self.pwa_hits + 1
";

fn boot() -> (MainLoop, NodeId) {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = gdscene::scene_tree::SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let script = GDScriptNodeInstance::from_source(SCRIPT, scene_root).expect("script parses");
    tree.attach_script(scene_root, Box::new(script));
    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(scene_root);
    (ml, scene_root)
}

/// Reads a script `var`. Script vars and node properties are separate stores.
fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    use gdscript_interop::bindings::ScriptInstance;
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

/// The id `create_callback` minted, read back through the backend's own log —
/// the only observation point, since `set_backend` takes ownership.
fn minted_callback_id() -> u64 {
    let calls = js_bridge::recorded_calls();
    let registered: Vec<&String> = calls
        .iter()
        .filter(|c| c.starts_with("create_callback:"))
        .collect();
    assert_eq!(
        registered.len(),
        1,
        "create_callback must register exactly once, got {calls:?}"
    );
    registered[0]
        .trim_start_matches("create_callback:")
        .parse()
        .expect("the log carries the callback id")
}

#[test]
fn javascript_bridge_callbacks_fire_from_main_loop() {
    js_bridge::reset_backend();
    js_bridge::set_backend(Box::new(RecordingJsBackend::new()));

    let (mut ml, root) = boot();
    assert_eq!(
        script_var(&ml, root, "ready_ran"),
        Variant::Bool(true),
        "`_ready` must run to completion — if `create_callback` or `connect` \
         raised, every assertion below would be reading an initialiser"
    );

    // -- (a) the callback runs from ONE step, with no manual pump ----------
    let id = minted_callback_id();
    js_bridge::queue_callback_invocation(id, vec![JsValue::String("from-js".into())]);
    assert_eq!(
        script_var(&ml, root, "got"),
        Variant::String("unset".to_string()),
        "queueing alone must not run the callback — it runs at a frame boundary"
    );

    ml.step(STEP);

    // 🔑 THE acceptance assertion. Note what this test does NOT contain: any
    // call to `pump_javascript_callbacks`. Before the frame-loop wiring the
    // invocation sat in the queue forever and `got` kept its initialiser, so
    // reverting the non-test files turns this line red.
    assert_eq!(
        script_var(&ml, root, "got"),
        Variant::String("from-js".to_string()),
        "one MainLoop::step must run the queued JS callback with the argument \
         JavaScript supplied, with no manual pump"
    );

    // -- (b) a PWA update emits pwa_update_available -----------------------
    assert_eq!(
        script_var(&ml, root, "pwa_hits"),
        Variant::Int(0),
        "nothing has flagged an update yet"
    );
    js_bridge::set_pwa_pending(true);
    ml.step(STEP);
    assert_eq!(
        script_var(&ml, root, "pwa_hits"),
        Variant::Int(1),
        "the false->true transition of the PWA pending flag must emit \
         pwa_update_available to the connected handler"
    );

    // 🔑 EDGE, not level. The flag is still true; a level-triggered
    // implementation would re-fire every frame, which for a handler that
    // prompts the player means a dialog sixty times a second.
    ml.step(STEP);
    ml.step(STEP);
    assert_eq!(
        script_var(&ml, root, "pwa_hits"),
        Variant::Int(1),
        "the signal must fire once per transition, not once per frame while the \
         flag stays true"
    );

    js_bridge::reset_backend();
}

#[test]
fn exported_shell_defines_the_bridge_hooks() {
    let config = WebExportConfig {
        pwa_enabled: true,
        ..WebExportConfig::default()
    };
    let layout = web_export::layout("Demo", &config);
    let html = web_export::render_html_shell("Demo", &config, &layout, None);

    // `gdplatform::js_bridge` looks these up on `globalThis` BY NAME, so the
    // names are the contract — a rename on either side silently restores the
    // no-op this bead fixed.
    for hook in [
        "__patina_fs_sync",
        "__patina_pwa_update",
        "__patina_pwa_update_pending",
    ] {
        assert!(
            html.contains(hook),
            "the exported shell must define `{hook}`, which js_bridge reads off \
             globalThis; without it the matching JavaScriptBridge call is a \
             silent no-op in an exported game"
        );
    }

    // ⚠️ The service worker registration, without which the `updatefound` event
    // that SETS the pending flag has no source — the shell had none at all, so
    // the flag could never flip however correct the rest was.
    assert!(
        html.contains("serviceWorker.register(\"./sw.js\")"),
        "a PWA export must register its service worker"
    );
    assert!(
        !html.contains("{{PATINA_PWA_ENABLED}}"),
        "the PWA placeholder must be substituted, not left in the served HTML"
    );

    // The worker has to honour skip-waiting or `pwa_update()` reloads into the
    // same version forever — a no-op that looks like it works.
    let files = web_export::pwa_files("Demo", &config, &layout);
    let sw = files
        .iter()
        .find(|(name, _)| name == "sw.js")
        .map(|(_, body)| body.clone())
        .expect("a PWA export emits sw.js");
    assert!(
        sw.contains("SKIP_WAITING") && sw.contains("skipWaiting()"),
        "sw.js must handle the shell's skip-waiting message"
    );
}

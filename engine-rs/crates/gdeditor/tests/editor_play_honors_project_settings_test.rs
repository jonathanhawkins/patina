//! pat-mgixk.44 acceptance — editor Play applies the PROJECT's settings.
//!
//! Godot's Play boots the game exactly as an export would: `Main::setup` /
//! `Main::start` read every `GLOBAL_GET`, so the physics tick rate, gravity and
//! the script-visible `ProjectSettings` all come from `project.godot`
//! (`main/main.cpp`).
//!
//! Patina's `api_runtime_play` built its `MainLoop` with only the input map and
//! the two pointer-emulation flags. It never called
//! `MainLoop::configure_from_project_settings` (whose one non-test caller was
//! `patina-runner`) and never seeded the script-visible settings singleton. So
//! a project that declares 30 ticks/s and 490 gravity ran at the engine
//! defaults (60 and 980) in the editor, and
//! `ProjectSettings.get_setting("application/config/name")` answered `Nil`
//! inside a project that plainly declares it. Everything only started working
//! after an export — the worst shape for this class of bug, because the editor
//! is where you test the game.
//!
//! # What the fixture pins
//!
//! Every value is deliberately NOT the engine default, so no assertion here can
//! pass on a default:
//!
//! | setting                                 | project | engine default |
//! |-----------------------------------------|---------|----------------|
//! | `physics/common/physics_ticks_per_second` | 30    | 60             |
//! | `physics/2d/default_gravity`              | 490   | 980            |
//!
//! ⚠️ Gravity is measured, not read back. The script samples the body's
//! `linear_velocity` together with the physics tick count and `delta`, and the
//! test divides to recover the acceleration the body actually experienced.
//! Asserting on `ProjectSettings.get_setting("physics/2d/default_gravity")`
//! instead would prove only that the string reached the settings store — not
//! that it reached the solver, which is the half that makes a game behave.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

/// Ticks to advance. Enough that the integrated velocity is comfortably larger
/// than any single-step rounding.
const STEPS: usize = 30;

/// The project's declared gravity, and the default it must not fall back to.
const PROJECT_GRAVITY: f64 = 490.0;
const DEFAULT_GRAVITY: f64 = 980.0;

/// A project declaring a non-default tick rate, a non-default gravity and a
/// name, written the way Godot serialises them.
const PROJECT_GODOT: &str = r#"config_version=5

[application]

config/name="Settings Fixture"

[physics]

common/physics_ticks_per_second=30
2d/default_gravity=490.0
"#;

/// Reports what the running game can see of its own project.
///
/// ⚠️ `ticks` and `dt` are captured alongside `vy` so the test can recover the
/// acceleration without knowing the editor's frame pacing. A test that assumed
/// one frame equals one physics tick would be measuring the editor's step
/// driver, not gravity.
const MAIN_GD: &str = "\
extends Node2D
var tps = 0
var pname = \"\"
var ready_ran = false
var ticks = 0
var dt = 0.0
var vy = 0.0
var vy_a = 0.0
var ticks_a = 0

func _ready():
    self.ready_ran = true
    self.tps = Engine.physics_ticks_per_second
    self.pname = ProjectSettings.get_setting(\"application/config/name\")

func _physics_process(delta):
    self.ticks = self.ticks + 1
    self.dt = delta
    self.vy = get_node(\"Body\").linear_velocity.y
    if self.ticks == 5:
        self.vy_a = self.vy
        self.ticks_a = self.ticks
";

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn connect_with_retry(port: u16) -> TcpStream {
    for attempt in 0..40 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 39 => thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("failed to connect: {e}"),
        }
    }
    unreachable!()
}

fn http_request_str(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).unwrap();
    resp
}

fn http_get(port: u16, path: &str) -> String {
    http_request_str(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_request_str(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn json_body(resp: &str) -> serde_json::Value {
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or(""))
        .unwrap_or(serde_json::Value::Null)
}

/// Boots the real editor server over `Main -> Body(RigidBody2D) -> Shape`, in a
/// project directory carrying [`PROJECT_GODOT`].
///
/// ⚠️ BOTH project roots are required and setting only the first is silent: the
/// PLAY path resolves `_script_path` against `texture_cache.project_root()`, so
/// without it no script attaches and every script variable is simply ABSENT —
/// which reads as "the setting did not arrive" rather than "the script never
/// loaded". The sibling `editor_play_input_map.rs` records the same trap.
fn boot(port: u16) -> (EditorServerHandle, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("main.gd"), MAIN_GD).expect("write script");
    std::fs::write(dir.path().join("project.godot"), PROJECT_GODOT).expect("write project");

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut main = Node::new("Main", "Node2D");
    main.set_property("_script_path", Variant::String("main.gd".to_string()));
    let main_id = tree.add_child(root, main).unwrap();

    let mut body = Node::new("Body", "RigidBody2D");
    body.set_property("mass", Variant::Float(1.0));
    let body_id = tree.add_child(main_id, body).unwrap();

    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(8.0));
    tree.add_child(body_id, shape).unwrap();

    let mut state = EditorState::new(tree);
    let root_path = dir.path().to_string_lossy().to_string();
    state.project_root = Some(root_path.clone());
    state.texture_cache.set_project_root(root_path);

    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, dir)
}

/// ⚠️ `/api/runtime/step` single-steps a **paused** runtime and 400s otherwise;
/// an unpaused runtime is advanced by a background driver instead. Pausing is
/// what makes `step` legal and these assertions deterministic.
fn play_paused(port: u16) {
    assert!(
        http_post(port, "/api/runtime/play", "{}").contains("200 OK"),
        "the runtime must start"
    );
    http_post(port, "/api/runtime/pause", "");
}

fn step(port: u16) {
    assert!(
        http_post(port, "/api/runtime/step", "").contains("200 OK"),
        "stepping a paused runtime must succeed"
    );
}

/// ⚠️ The variable name is a **value**, not a key, and `value` is doubled.
/// Walking the JSON for a key named `tps` matches nothing, ever.
fn property_value(v: &serde_json::Value, name: &str) -> Option<serde_json::Value> {
    v.get("properties")?
        .as_array()?
        .iter()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(name))?
        .get("value")?
        .get("value")
        .cloned()
}

fn script_var(port: u16, name: &str) -> serde_json::Value {
    let node = json_body(&http_get(port, "/api/runtime/node?path=/root/Main"));
    property_value(&node, name)
        .unwrap_or_else(|| panic!("script var `{name}` missing from /api/runtime/node: {node}"))
}

fn script_int(port: u16, name: &str) -> i64 {
    let v = script_var(port, name);
    v.as_i64()
        .unwrap_or_else(|| panic!("`{name}` should be an integer, got {v}"))
}

fn script_f64(port: u16, name: &str) -> f64 {
    let v = script_var(port, name);
    v.as_f64()
        .unwrap_or_else(|| panic!("`{name}` should be a number, got {v}"))
}

fn script_string(port: u16, name: &str) -> String {
    let v = script_var(port, name);
    v.as_str()
        .unwrap_or_else(|| panic!("`{name}` should be a string, got {v}"))
        .to_string()
}

/// **The acceptance test.**
#[test]
fn editor_play_honors_project_settings_test() {
    let port = free_port();
    let (_handle, _dir) = boot(port);

    play_paused(port);
    for _ in 0..STEPS {
        step(port);
    }

    // Fixture check first: with no script attached every assertion below reads
    // an absent variable, which looks exactly like a setting that failed to
    // arrive.
    assert!(
        matches!(script_var(port, "ready_ran"), serde_json::Value::Bool(true)),
        "the script must have attached and run `_ready`, or nothing below means \
         anything"
    );

    // -- (1) the physics tick rate reaches the Engine singleton -------------
    assert_eq!(
        script_int(port, "tps"),
        30,
        "`Engine.physics_ticks_per_second` must report the project's 30, not the \
         engine default 60. Play never called \
         `MainLoop::configure_from_project_settings`, whose only non-test caller \
         was patina-runner."
    );

    // -- (2) the script-visible ProjectSettings singleton is populated ------
    //
    // ⚠️ This is the claim that needs the settings seeded on the thread that
    // RUNS the frame: the store is a thread-local and the editor serves each
    // HTTP connection on its own thread, advancing the runtime there. Seeding
    // only at Play time leaves this `Nil`.
    assert_eq!(
        script_string(port, "pname"),
        "Settings Fixture",
        "`ProjectSettings.get_setting(\"application/config/name\")` must answer \
         the project's name; Nil/empty means the settings singleton was never \
         seeded for the thread running the script"
    );

    // -- (3) the project's gravity reaches the SOLVER -----------------------
    //
    // Recovered from the body's own motion rather than read back from the
    // settings, so this cannot pass on a value that reached the store but not
    // the physics server.
    let ticks = script_int(port, "ticks");
    let dt = script_f64(port, "dt");
    let vy = script_f64(port, "vy");
    assert!(
        ticks > 0 && dt > 0.0,
        "the body must have been physics-stepped at least once; got ticks={ticks} \
         dt={dt}"
    );
    // 🔑 A SLOPE between two samples, not an average from zero.
    //
    // ⚠️ `vy / (ticks * dt)` assumes the first counted tick also integrated and
    // that vy started at exactly 0. It does not: the measured 447.6 implies
    // 13.70 effective integrations across 15 counted ticks — a NON-INTEGER, so
    // a constant offset rather than a wrong gravity. (447.6 is also nowhere near
    // the 980 engine default this assertion's own message names as the failure
    // signature, which is how we know the project's value does reach the
    // solver.) Differencing two samples cancels any such offset and measures
    // exactly what the acceptance states: that the body ACCELERATES at 490.
    let vy_a = script_f64(port, "vy_a");
    let ticks_a = script_int(port, "ticks_a");
    assert!(
        ticks_a > 0 && ticks > ticks_a,
        "the early sample must have been taken and later ticks must follow it; \
         got ticks_a={ticks_a} ticks={ticks}. Without two distinct samples there \
         is no slope to measure."
    );
    let measured_g = (vy - vy_a) / ((ticks - ticks_a) as f64 * dt);
    assert!(
        (measured_g - PROJECT_GRAVITY).abs() < 25.0,
        "the body must accelerate at the project's {PROJECT_GRAVITY} px/s^2; \
         measured {measured_g:.1} from vy={vy:.2} over {ticks} ticks of {dt:.5}s. \
         A value near {DEFAULT_GRAVITY} means Play kept the engine default and \
         the project's `physics/2d/default_gravity` never reached the solver."
    );
}

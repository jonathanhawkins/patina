//! pat-s3rrv.1 (merged finding 3) — the AnimationTree editor can edit a graph.
//!
//! `editor_server.rs` has served `/state/remove`, `/state/move`,
//! `/transition/remove` and `/transition/set` since pat-0q2kx.166, and
//! `editor_ui.rs` called **none** of them: it fetched only `/tree`,
//! `/state/add` and `/transition/add`. So the served state-machine graph was
//! add-only — a state could be created but never moved or deleted, and a
//! transition could be created but neither retimed nor removed. Four working
//! endpoints with no way to reach them.
//!
//! Two further gaps in the same surface:
//!
//! - `/state/add` accepts a `kind`, and the client never sent one, so every
//!   node was an `AnimationNodeAnimation` leaf. A blend space or a blend2 node
//!   could not be created at all.
//! - `AnimTreeGraph` keeps transitions and *connections* in separate lists and
//!   `GET /api/animation/tree` serves both, but nothing could create a
//!   connection — a blend TREE (as opposed to a state machine) was unbuildable.
//!   `/connect` and `/disconnect` are new here.
//!
//! ⚠️ The HTTP tests below assert against the graph re-read from `GET
//! /api/animation/tree`, not against the mutating call's own response. Every
//! one of these endpoints answers `{"ok":true}`, so a test reading the POST
//! response passes against a server that changed nothing.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

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

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn boot() -> (EditorServerHandle, u16) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

/// The graph as the client sees it.
fn graph(port: u16) -> serde_json::Value {
    let resp = http_get(port, "/api/animation/tree");
    assert!(resp.contains("200 OK"), "tree read failed: {resp}");
    serde_json::from_str(body_of(&resp)).expect("graph is JSON")
}

fn state_named<'a>(g: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
    g["states"]
        .as_array()?
        .iter()
        .find(|s| s["name"].as_str() == Some(name))
}

fn add_state(port: u16, name: &str) {
    let resp = http_post(
        port,
        "/api/animation/tree/state/add",
        &format!(r#"{{"name":"{name}"}}"#),
    );
    assert!(resp.contains("200 OK"), "add {name} failed: {resp}");
}

/// **The acceptance test**: every edit operation the served editor now offers
/// actually changes the graph.
#[test]
fn anim_tree_graph_edit_ops_reach_the_client() {
    let (handle, port) = boot();

    add_state(port, "Idle");
    add_state(port, "Run");
    add_state(port, "Jump");
    let resp = http_post(
        port,
        "/api/animation/tree/transition/add",
        r#"{"from":"Idle","to":"Run","xfade":0.1}"#,
    );
    assert!(resp.contains("200 OK"), "add transition failed: {resp}");

    // -- move ---------------------------------------------------------------
    let resp = http_post(
        port,
        "/api/animation/tree/state/move",
        r#"{"name":"Run","x":140,"y":60}"#,
    );
    assert!(resp.contains("200 OK"), "move failed: {resp}");
    let g = graph(port);
    let run = state_named(&g, "Run").expect("Run still present after a move");
    assert_eq!(
        (run["x"].as_f64(), run["y"].as_f64()),
        (Some(140.0), Some(60.0)),
        "the moved position must survive into the served graph — the client \
         re-lays-out from these coordinates on every poll, so a move that does \
         not persist snaps back on the next refresh",
    );

    // -- retime a transition ------------------------------------------------
    let resp = http_post(
        port,
        "/api/animation/tree/transition/set",
        r#"{"from":"Idle","to":"Run","xfade":0.45}"#,
    );
    assert!(resp.contains("200 OK"), "set xfade failed: {resp}");
    let g = graph(port);
    let xfade = g["transitions"][0]["xfade"].as_f64();
    assert_eq!(
        xfade,
        Some(0.45),
        "xfade_time must be editable in place; got {xfade:?}",
    );

    // A negative crossfade is rejected rather than stored: Godot's xfade_time
    // has a 0 minimum and a negative one would run the blend backwards.
    let resp = http_post(
        port,
        "/api/animation/tree/transition/set",
        r#"{"from":"Idle","to":"Run","xfade":-1}"#,
    );
    assert!(resp.contains("400"), "a negative xfade must 400: {resp}");
    assert_eq!(
        graph(port)["transitions"][0]["xfade"].as_f64(),
        Some(0.45),
        "…and must leave the previous value intact",
    );

    // -- remove a transition ------------------------------------------------
    let resp = http_post(
        port,
        "/api/animation/tree/transition/remove",
        r#"{"from":"Idle","to":"Run"}"#,
    );
    assert!(resp.contains("200 OK"), "remove transition failed: {resp}");
    assert_eq!(
        graph(port)["transitions"].as_array().map(|a| a.len()),
        Some(0),
        "the transition must be gone from the served graph",
    );

    // -- remove a state -----------------------------------------------------
    let resp = http_post(
        port,
        "/api/animation/tree/state/remove",
        r#"{"name":"Jump"}"#,
    );
    assert!(resp.contains("200 OK"), "remove state failed: {resp}");
    let g = graph(port);
    assert!(state_named(&g, "Jump").is_none(), "Jump must be gone: {g}",);
    assert!(
        state_named(&g, "Idle").is_some() && state_named(&g, "Run").is_some(),
        "…and removing one state must not take the others with it: {g}",
    );

    // An unknown name is a 404, not a silent success — a typo must not read as
    // a completed delete.
    let resp = http_post(
        port,
        "/api/animation/tree/state/remove",
        r#"{"name":"NoSuchState"}"#,
    );
    assert!(resp.contains("404"), "unknown state must 404: {resp}");

    handle.stop();
}

/// A node's `kind` survives the round trip, so a blend tree can be built
/// (pat-s3rrv.1).
///
/// `/state/add` has always read a `kind` field. Nothing sent one, so every node
/// came back `"animation"` and the three other `NodeKind`s were unreachable
/// from the editor.
#[test]
fn node_kind_round_trips_so_a_blend_tree_can_be_built() {
    let (handle, port) = boot();

    // Sent kind -> kind the graph reports back. `blend_space` is the pre-split
    // spelling and maps to 1D: the two blend spaces are separate Godot classes,
    // so the wire format had to grow a second name, and an older client must
    // not start creating animation leaves where it used to create a blend
    // space.
    const KINDS: [(&str, &str, &str); 6] = [
        ("Walk", "animation", "animation"),
        ("Mix", "blend2", "blend2"),
        ("Space", "blend_space_1d", "blend_space_1d"),
        ("Plane", "blend_space_2d", "blend_space_2d"),
        ("Legacy", "blend_space", "blend_space_1d"),
        ("Sub", "state_machine", "state_machine"),
    ];

    for (name, sent, _) in KINDS {
        let resp = http_post(
            port,
            "/api/animation/tree/state/add",
            &format!(r#"{{"name":"{name}","kind":"{sent}"}}"#),
        );
        assert!(resp.contains("200 OK"), "add {name} failed: {resp}");
    }

    let g = graph(port);
    for (name, sent, expected) in KINDS {
        assert_eq!(
            state_named(&g, name).and_then(|s| s["kind"].as_str()),
            Some(expected),
            "{name} was added as {sent} and must come back as {expected}, not \
             an animation leaf: {g}",
        );
    }

    handle.stop();
}

/// Blend-tree connections can be made and broken (pat-s3rrv.1).
///
/// A connection is a different edge from a transition: no crossfade, no
/// condition. `AnimTreeGraph` has always kept the two lists apart and the graph
/// endpoint has always served both — but with no route to create one, the
/// `connections` array could only ever be empty.
#[test]
fn blend_tree_connections_can_be_made_and_broken() {
    let (handle, port) = boot();
    add_state(port, "Walk");
    add_state(port, "Mix");

    let resp = http_post(
        port,
        "/api/animation/tree/connect",
        r#"{"from":"Walk","to":"Mix"}"#,
    );
    assert!(resp.contains("200 OK"), "connect failed: {resp}");
    let g = graph(port);
    assert_eq!(
        g["connections"].as_array().map(|a| a.len()),
        Some(1),
        "the connection must appear in the served graph: {g}",
    );
    assert_eq!(
        g["transitions"].as_array().map(|a| a.len()),
        Some(0),
        "…and must NOT show up as a transition — the two edge kinds are \
         distinct, and conflating them would give a blend-tree wire a \
         crossfade it has no meaning for: {g}",
    );

    // Duplicates are refused, matching how add_transition behaves.
    let resp = http_post(
        port,
        "/api/animation/tree/connect",
        r#"{"from":"Walk","to":"Mix"}"#,
    );
    assert!(
        resp.contains("400"),
        "a duplicate connection must 400: {resp}"
    );

    let resp = http_post(
        port,
        "/api/animation/tree/disconnect",
        r#"{"from":"Walk","to":"Mix"}"#,
    );
    assert!(resp.contains("200 OK"), "disconnect failed: {resp}");
    assert_eq!(
        graph(port)["connections"].as_array().map(|a| a.len()),
        Some(0),
    );

    // Disconnecting something that was never wired is a 404.
    let resp = http_post(
        port,
        "/api/animation/tree/disconnect",
        r#"{"from":"Walk","to":"Mix"}"#,
    );
    assert!(resp.contains("404"), "unknown connection must 404: {resp}");

    handle.stop();
}

/// Drift guard: the served page reaches every AnimationTree graph endpoint, and
/// each is listed in `/api/capabilities` (pat-s3rrv.1).
///
/// This is the assertion that would have caught the original bug. Four
/// endpoints were routed, tested server-side, and unreachable from the only
/// client that exists — nothing failed, because nothing compared the two sides.
#[test]
fn every_anim_tree_endpoint_is_reachable_from_the_page_and_listed() {
    const ENDPOINTS: [&str; 9] = [
        "/api/animation/tree/state/add",
        "/api/animation/tree/state/remove",
        "/api/animation/tree/state/move",
        "/api/animation/tree/transition/add",
        "/api/animation/tree/transition/remove",
        "/api/animation/tree/transition/set",
        "/api/animation/tree/connect",
        "/api/animation/tree/disconnect",
        "/api/animation/tree/node/param",
    ];

    for path in ENDPOINTS {
        assert!(
            EDITOR_HTML.contains(path),
            "{path} is routed by editor_server.rs but the served page never \
             fetches it — the graph editor cannot reach it",
        );
    }

    let (handle, port) = boot();
    let caps = http_get(port, "/api/capabilities");
    let caps_body = body_of(&caps).to_string();
    for path in ENDPOINTS {
        assert!(
            caps_body.contains(path),
            "{path} is missing from /api/capabilities — the route table and \
             the client have drifted",
        );
    }
    assert!(
        caps_body.contains("/api/animation/tree\""),
        "the graph READ endpoint must be listed too",
    );

    handle.stop();
}

/// The client commits a drag once, on release (pat-s3rrv.1).
///
/// `/state/move` rewrites the graph. Posting from `mousemove` would put a
/// request per frame on the wire for a value only the final position matters
/// for, and the intermediate responses would race the poll that redraws the
/// canvas.
#[test]
fn dragging_a_state_commits_on_release_not_per_frame() {
    // Sliced by CHARS, not bytes: the served page is not pure ASCII and a byte
    // range can land inside a multi-byte character and panic.
    let start = EDITOR_HTML
        .find("function makeAnimTreeStateDraggable")
        .expect("the drag helper must exist — without it /state/move has no caller");
    let drag: String = EDITOR_HTML[start..].chars().take(1200).collect();
    let drag = drag.as_str();

    assert!(
        drag.contains("onUp") && drag.contains("moveAnimTreeState("),
        "the move must be committed from the mouseup handler",
    );
    let move_pos = drag.find("moveAnimTreeState(").expect("commit call");
    let onmove = drag.find("function onMove").expect("onMove handler");
    let onup = drag.find("function onUp").expect("onUp handler");
    assert!(
        move_pos > onup && onup > onmove,
        "the commit must sit inside onUp, after onMove — a call in onMove \
         would POST on every mouse movement",
    );
}

/// A blend space's points round-trip through `params` (pat-s3rrv.1).
///
/// `AnimTreeGraph` has stored per-node params since pat-0q2kx.166 and
/// `GET /api/animation/tree` never emitted them, so the client could see a
/// node's name, kind and position and nothing else — a blend space was a box
/// with no contents. The key names are Godot's own
/// (`blend_point_N/node`, `blend_point_N/pos`, `min_space`, `max_space`,
/// `blend_position`) so the served graph and the resource agree.
#[test]
fn blend_space_points_round_trip_through_node_params() {
    let (handle, port) = boot();
    let resp = http_post(
        port,
        "/api/animation/tree/state/add",
        r#"{"name":"Space","kind":"blend_space_2d"}"#,
    );
    assert!(resp.contains("200 OK"), "add blend space failed: {resp}");

    for (key, value) in [
        ("min_space", "-1,-1"),
        ("max_space", "1,1"),
        ("blend_position", "0.25,0.5"),
        ("blend_point_0/node", "idle"),
        ("blend_point_0/pos", "0,0"),
        ("blend_point_1/node", "run"),
        ("blend_point_1/pos", "1,0"),
    ] {
        let resp = http_post(
            port,
            "/api/animation/tree/node/param",
            &format!(r#"{{"name":"Space","key":"{key}","value":"{value}"}}"#),
        );
        assert!(resp.contains("200 OK"), "set {key} failed: {resp}");
    }

    let g = graph(port);
    let node = state_named(&g, "Space").expect("Space present");
    assert_eq!(
        node["kind"].as_str(),
        Some("blend_space_2d"),
        "1D and 2D are separate Godot classes and the kind must say which — a \
         2D point's position has two components and a 1D point's has one, so a \
         client cannot render the editor without it: {g}",
    );
    let params = &node["params"];
    assert_eq!(params["blend_point_0/node"].as_str(), Some("idle"));
    assert_eq!(params["blend_point_1/pos"].as_str(), Some("1,0"));
    assert_eq!(
        params["blend_position"].as_str(),
        Some("0.25,0.5"),
        "the blend position must survive: it is what a running blend space \
         actually samples",
    );

    // Overwriting one key leaves the others alone.
    let resp = http_post(
        port,
        "/api/animation/tree/node/param",
        r#"{"name":"Space","key":"blend_point_0/pos","value":"0.5,0"}"#,
    );
    assert!(resp.contains("200 OK"), "overwrite failed: {resp}");
    let g = graph(port);
    let params = &state_named(&g, "Space").expect("Space present")["params"];
    assert_eq!(params["blend_point_0/pos"].as_str(), Some("0.5,0"));
    assert_eq!(
        params["blend_point_1/node"].as_str(),
        Some("run"),
        "editing one point must not disturb another",
    );

    // A param on a node that does not exist is a 404, not a silent create.
    let resp = http_post(
        port,
        "/api/animation/tree/node/param",
        r#"{"name":"Nope","key":"min_space","value":"0"}"#,
    );
    assert!(resp.contains("404"), "unknown node must 404: {resp}");

    // A node with no params still serves an object, not a missing field — the
    // client indexes into it unconditionally.
    add_state(port, "Plain");
    let g = graph(port);
    assert!(
        state_named(&g, "Plain").expect("Plain present")["params"].is_object(),
        "every node must carry a params object, empty or not: {g}",
    );

    handle.stop();
}

/// The client stops reading blend points at a CLEARED slot (pat-s3rrv.1).
///
/// The param endpoint sets; it does not delete. Removing a point therefore
/// rewrites the survivors and blanks the trailing slot, so the reader has to
/// treat an empty `blend_point_N/node` as the end of the run. Reading only for
/// `undefined` would show a phantom empty row after every delete.
#[test]
fn the_blend_point_reader_stops_at_a_cleared_slot() {
    let src = EDITOR_HTML
        .find("function animTreeBlendPoints")
        .map(|i| EDITOR_HTML[i..].chars().take(600).collect::<String>())
        .expect("the blend point reader must exist");
    assert!(
        src.contains("=== undefined") && src.contains("=== ''"),
        "the run must end on a cleared slot as well as a missing one: {src}",
    );
}

/// Edges are drawn on the canvas, and the two kinds are distinguishable
/// (pat-s3rrv.1).
///
/// A blend tree read as a text list of `from -> to` pairs is not a graph
/// editor. The states are already positioned, so the edges have real endpoints
/// to connect.
#[test]
fn transitions_and_wires_are_drawn_and_told_apart() {
    let draw = EDITOR_HTML
        .find("function drawAnimTreeEdges")
        .map(|i| EDITOR_HTML[i..].chars().take(1800).collect::<String>())
        .expect("the edge drawing helper must exist");

    assert!(
        draw.contains("createElementNS") && draw.contains("'line'"),
        "edges must be drawn as real SVG lines: {draw}",
    );
    assert!(
        draw.contains("graph.transitions") && draw.contains("graph.connections"),
        "BOTH edge lists must be drawn — connections were served all along and \
         never rendered",
    );
    assert!(
        draw.contains("stroke-dasharray"),
        "a blend-tree wire must be visually distinct from a state transition: \
         they carry different semantics and a graph can hold both",
    );
    assert!(
        draw.contains("pointerEvents = 'none'"),
        "the overlay must not swallow pointer events, or dragging a state \
         would stop working the moment an edge crossed it",
    );
}

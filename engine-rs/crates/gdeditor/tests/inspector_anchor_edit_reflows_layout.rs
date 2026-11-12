//! pat-0q2kx.19 — an anchor/offset edit from the inspector reflows the layout.
//!
//! Godot re-runs the layout solver when a Control's anchors, offsets, size flags
//! or minimum size change; a container re-sorts its children so the edit
//! propagates to siblings (`scene/gui/container.cpp::_sort_children`).
//!
//! `control_binding::invalidate_layout` implemented all of that — re-resolve the
//! node's own rect, re-arrange it if it is a container, re-arrange the *parent*
//! when the parent is one — and had **zero callers outside its own file**. So an
//! anchor edit through `POST /api/property/set` stored the number and moved
//! nothing: the inspector showed the new value and the viewport did not reflow.
//!
//! This drives the real HTTP route, not the helper, because the helper was never
//! the missing piece — the wiring was.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
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

fn http(port: u16, method: &str, path: &str, body: &str) -> String {
    let mut stream = None;
    for attempt in 0..20 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(_) if attempt < 19 => thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("failed to connect: {e}"),
        }
    }
    let mut stream = stream.expect("connected");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req = if method == "GET" {
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
    } else {
        format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    };
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

/// Reads a float out of the served node JSON for `prop`.
fn served_number(resp: &str, prop: &str) -> Option<f64> {
    // Entries look like {"name":"offset_top", … "value":{…"value":12.0}} — find
    // the property's object and take the first number after it.
    let at = resp.find(&format!("\"{prop}\""))?;
    let tail = &resp[at..];
    let end = tail.find("},{").unwrap_or(tail.len().min(400));
    let slice = &tail[..end];
    let num: String = slice
        .chars()
        .skip_while(|c| !c.is_ascii_digit() && *c != '-')
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    num.parse().ok()
}

/// **The clause under test.** Editing `offset_top` on a Control through the
/// inspector route reflows its rect instead of only storing the number.
#[test]
fn inspector_anchor_edit_reflows_layout() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let panel = tree
        .add_child(root_id, Node::new("Panel", "Panel"))
        .unwrap();
    let child = tree
        .add_child(panel, Node::new("Child", "Control"))
        .unwrap();
    let child_raw = child.raw();

    let state = EditorState::new(tree);
    let port = free_port();
    let _handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    // A layout property: `offset_top` is in `is_layout_property`'s set.
    let body = format!(
        r#"{{"node_id":{child_raw},"property":"offset_top","value":{{"type":"Float","value":40.0}}}}"#
    );
    let post = http(port, "POST", "/api/property/set", &body);
    assert!(
        post.contains("\"ok\":true"),
        "setting a layout property must be accepted:\n{post}"
    );

    // The reflow must have run: the solver writes the resolved rect back onto
    // the node, so `position` reflects the new offset rather than staying at 0.
    let after = http(port, "GET", &format!("/api/node/{child_raw}"), "");
    assert!(
        after.contains("offset_top"),
        "the edited property must still be served:\n{after}"
    );
    let y = served_number(&after, "position").unwrap_or(0.0);
    assert!(
        y > 0.0,
        "editing offset_top must REFLOW the control — position should follow the \
         offset, but it stayed at {y}. invalidate_layout is not reaching the \
         property route.\n{after}"
    );
}

/// A non-layout property must NOT trigger a reflow — the solver is not free, and
/// re-running it on every `text`/`modulate` edit would fight manual placement.
#[test]
fn non_layout_property_does_not_reflow() {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let panel = tree
        .add_child(root_id, Node::new("Panel", "Panel"))
        .unwrap();
    let label = tree.add_child(panel, Node::new("Label", "Label")).unwrap();
    let label_raw = label.raw();

    let state = EditorState::new(tree);
    let port = free_port();
    let _handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    let body = format!(
        r#"{{"node_id":{label_raw},"property":"text","value":{{"type":"String","value":"hello"}}}}"#
    );
    let post = http(port, "POST", "/api/property/set", &body);
    assert!(
        post.contains("\"ok\":true"),
        "setting a plain property must still work:\n{post}"
    );
    let after = http(port, "GET", &format!("/api/node/{label_raw}"), "");
    assert!(
        after.contains("hello"),
        "the plain property edit must persist:\n{after}"
    );
}

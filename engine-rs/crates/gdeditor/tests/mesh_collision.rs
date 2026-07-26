//! pat-0q2kx.169 — the Mesh toolbar's "Create Trimesh/Convex Collision" builds a
//! real collision body from a MeshInstance3D.
//!
//! Godot ground truth: `editor/scene/3d/mesh_instance_3d_editor_plugin.cpp` —
//! with a MeshInstance3D selected, the Mesh menu generates a StaticBody3D
//! holding a CollisionShape3D built from the mesh (trimesh = the triangle soup
//! as a ConcavePolygonShape3D, convex = a hull from the mesh points).
//!
//! Patina had no equivalent at all: searching the editor for
//! `create_trimesh_shape|create_convex_shape|create_collision` returned zero
//! hits, so blocking out a 3D level meant hand-building every collider.
//!
//! NOTE ON THE FILE NAME: the acceptance calls this `mesh_create_collision_e2e`;
//! with `-p gdeditor --test` that command is 62 characters and the verifier
//! truncates at 58, so it would run a non-existent target. The acceptance's name
//! is kept on the test function.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
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
    for attempt in 0..20 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 19 => thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("failed to connect: {e}"),
        }
    }
    unreachable!()
}

fn http_request_str(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
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

fn json_of(resp: &str) -> serde_json::Value {
    serde_json::from_str(body_of(resp))
        .unwrap_or_else(|e| panic!("response body is not JSON ({e}): {resp}"))
}

/// A 3D scene holding one `MeshInstance3D` named `Crate` with a box mesh.
/// Returns the raw node id so the route can be aimed at it.
fn server() -> (EditorServerHandle, u16, u64) {
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node3D")).unwrap();

    let mut mesh = Node::new("Crate", "MeshInstance3D");
    // `primitive:<Type>` is the marker the viewport resolves to real geometry,
    // and `get_mesh_path` falls back to `mesh` when there is no
    // `__viewport_mesh__` companion.
    mesh.set_property("mesh", Variant::String("primitive:BoxMesh".to_string()));
    let mesh_id = tree.add_child(main, mesh).unwrap();
    let raw = mesh_id.raw();

    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    (handle, port, raw)
}

/// **The acceptance test.** Trimesh collision on a box mesh produces a
/// StaticBody3D + CollisionShape3D carrying a ConcavePolygonShape3D whose
/// triangle count matches the mesh, and one undo removes both.
#[test]
fn mesh_create_collision_e2e() {
    let (_handle, port, mesh_raw) = server();

    let created = json_of(&http_post(
        port,
        "/api/mesh/create_collision",
        &format!(r#"{{"node_id":{mesh_raw},"kind":"trimesh"}}"#),
    ));

    assert_eq!(
        created["shape"], "ConcavePolygonShape3D",
        "trimesh must build a concave polygon shape: {created}"
    );
    // A box is 6 quads = 12 triangles = 36 points in the soup. Asserting the
    // exact count is what catches a shape built from the wrong geometry (or
    // from nothing) rather than merely "some triangles".
    assert_eq!(
        created["triangle_count"], 12,
        "a box mesh is 12 triangles: {created}"
    );
    assert_eq!(
        created["point_count"], 36,
        "the triangle soup is 3 points per triangle: {created}"
    );
    assert!(
        created["body_id"].as_u64().is_some() && created["shape_id"].as_u64().is_some(),
        "the route must report the created node ids: {created}"
    );

    // The nodes really are in the scene, not just in the response.
    let scene = http_get(port, "/api/scene");
    assert!(
        scene.contains("StaticBody3D"),
        "the scene must gain a StaticBody3D"
    );
    assert!(
        scene.contains("CollisionShape3D"),
        "the scene must gain a CollisionShape3D"
    );
    assert!(
        scene.contains("Crate_col"),
        "Godot names the generated body <mesh>_col"
    );

    // -- undo removes BOTH nodes in one step ---------------------------------
    let undone = json_of(&http_post(port, "/api/undo", "{}"));
    assert_eq!(undone["ok"], true, "undo failed: {undone}");

    let after = http_get(port, "/api/scene");
    assert!(
        !after.contains("StaticBody3D"),
        "one undo must remove the generated body, not leave it behind"
    );
    assert!(
        !after.contains("CollisionShape3D"),
        "one undo must remove the collision shape too — the pair is a single \
         Group command, so a half-undone state would be a bug"
    );
    // The mesh itself must survive.
    assert!(
        after.contains("Crate"),
        "undo must not remove the source MeshInstance3D"
    );
}

/// Convex builds a hull point cloud rather than a triangle soup.
#[test]
fn convex_kind_builds_a_point_cloud() {
    let (_handle, port, mesh_raw) = server();

    let created = json_of(&http_post(
        port,
        "/api/mesh/create_collision",
        &format!(r#"{{"node_id":{mesh_raw},"kind":"convex"}}"#),
    ));
    assert_eq!(
        created["shape"], "ConvexPolygonShape3D",
        "convex must build a convex polygon shape: {created}"
    );
    let points = created["point_count"].as_u64().unwrap_or(0);
    assert!(
        (8..36).contains(&points),
        "a box hull is distinct corners — more than 8 (shared normals split \
         them) but fewer than the 36-point soup; got {points}"
    );
}

/// Bad input is refused rather than producing an empty collider.
#[test]
fn non_mesh_nodes_and_bad_kinds_are_refused() {
    let (_handle, port, mesh_raw) = server();

    // An unknown kind.
    let bad_kind = http_post(
        port,
        "/api/mesh/create_collision",
        &format!(r#"{{"node_id":{mesh_raw},"kind":"banana"}}"#),
    );
    assert!(
        bad_kind.contains("400"),
        "an unknown kind must be rejected: {bad_kind}"
    );

    // A node that is not a MeshInstance3D — the root.
    let scene_before = http_get(port, "/api/scene");
    let not_a_mesh = http_post(
        port,
        "/api/mesh/create_collision",
        r#"{"node_id":1,"kind":"trimesh"}"#,
    );
    assert!(
        not_a_mesh.contains("400") || not_a_mesh.contains("404"),
        "a non-MeshInstance3D must be refused: {not_a_mesh}"
    );
    // ...and nothing was created on the way to refusing.
    let scene_after = http_get(port, "/api/scene");
    assert_eq!(
        scene_before.contains("StaticBody3D"),
        scene_after.contains("StaticBody3D"),
        "a refused request must not leave a partial body behind"
    );
}

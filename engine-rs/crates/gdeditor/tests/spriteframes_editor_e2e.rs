//! pat-0q2kx.74: SpriteFrames authoring for `AnimatedSprite2D` over HTTP.
//!
//! Godot's SpriteFrames panel manages named animations (speed, loop, autoplay)
//! and their ordered frame lists (`editor/scene/sprite_frames_editor_plugin.cpp`).
//! Patina had no route and no panel: an `AnimatedSprite2D` existed as a node
//! class whose frames could not be authored at all.
//!
//! Patina's node property bag is FLAT, so the resource lives as companion keys
//! on the node (`__sf_anims__`, `__sf_<anim>_{speed,loop,frames}`) — the same
//! flattening the scene file already uses for sub-resources, which is what makes
//! authored frames persist through the existing saver with no new serializer.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::scene_saver::TscnSaver;
use gdscene::{MainLoop, SceneTree};
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

fn json_of(resp: &str) -> serde_json::Value {
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or(""))
        .unwrap_or_else(|e| panic!("response body is not JSON ({e}): {resp}"))
}

fn server() -> (EditorServerHandle, u16, u64) {
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let sprite = tree
        .add_child(main, Node::new("Anim", "AnimatedSprite2D"))
        .unwrap();
    let raw = sprite.raw();

    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    (handle, port, raw)
}

/// **The acceptance test.** Author an animation with two frames over HTTP, set
/// its speed and loop, and confirm the state reads back.
#[test]
fn spriteframes_editor_e2e() {
    let (handle, port, raw) = server();

    // A fresh AnimatedSprite2D has no animations.
    let empty = json_of(&http_get(port, &format!("/api/spriteframes?node_id={raw}")));
    assert_eq!(empty["ok"], true, "read failed: {empty}");
    assert_eq!(
        empty["animations"].as_array().map(|a| a.len()),
        Some(0),
        "a fresh node has no animations: {empty}"
    );

    // -- create 'run' ----------------------------------------------------------
    let added = json_of(&http_post(
        port,
        "/api/spriteframes/anim/add",
        &format!(r#"{{"node_id":{raw},"name":"run"}}"#),
    ));
    assert_eq!(added["ok"], true, "anim/add failed: {added}");

    // ⚠️ Re-adding the same name is a conflict, not a silent duplicate — two
    // animations sharing a name would make every later lookup ambiguous.
    let dup = http_post(
        port,
        "/api/spriteframes/anim/add",
        &format!(r#"{{"node_id":{raw},"name":"run"}}"#),
    );
    assert!(
        dup.contains("409"),
        "adding a duplicate animation name must conflict: {dup}"
    );

    // -- two frames ------------------------------------------------------------
    for tex in ["res://run_0.png", "res://run_1.png"] {
        let f = json_of(&http_post(
            port,
            "/api/spriteframes/frame/add",
            &format!(r#"{{"node_id":{raw},"name":"run","texture":"{tex}"}}"#),
        ));
        assert_eq!(f["ok"], true, "frame/add failed for {tex}: {f}");
    }

    // -- speed + loop ----------------------------------------------------------
    let set = json_of(&http_post(
        port,
        "/api/spriteframes/anim/set",
        &format!(r#"{{"node_id":{raw},"name":"run","speed":10.0,"loop":false}}"#),
    ));
    assert_eq!(set["ok"], true, "anim/set failed: {set}");

    // -- read it all back ------------------------------------------------------
    let read = json_of(&http_get(port, &format!("/api/spriteframes?node_id={raw}")));
    let anims = read["animations"].as_array().expect("animations array");
    assert_eq!(anims.len(), 1, "exactly one animation: {read}");
    let run = &anims[0];
    assert_eq!(run["name"], "run");
    assert_eq!(run["speed"], 10.0, "speed round-trips");
    assert_eq!(run["loop"], false, "loop round-trips");
    let frames = run["frames"].as_array().expect("frames array");
    assert_eq!(frames.len(), 2, "both frames stored in order: {run}");
    assert_eq!(frames[0], "res://run_0.png");
    assert_eq!(frames[1], "res://run_1.png");

    // -- removing a frame keeps the rest in order ------------------------------
    let removed = json_of(&http_post(
        port,
        "/api/spriteframes/frame/remove",
        &format!(r#"{{"node_id":{raw},"name":"run","index":0}}"#),
    ));
    assert_eq!(removed["frame_count"], 1, "one frame left: {removed}");
    let after = json_of(&http_get(port, &format!("/api/spriteframes?node_id={raw}")));
    let frames_after = after["animations"][0]["frames"]
        .as_array()
        .expect("frames array");
    assert_eq!(
        frames_after[0], "res://run_1.png",
        "removing index 0 leaves the SECOND frame, not a hole"
    );

    // -- the edit marked the scene dirty, so a save is offered -----------------
    {
        let state = handle.state();
        let state = state.lock().unwrap();
        assert!(
            state.scene_modified,
            "authoring frames must dirty the scene so the user is prompted to save"
        );
    }
}

/// The bead's second clause is the bottom panel, which is client-side. Routes
/// with no caller are the failure mode this guards: asserting on `EDITOR_HTML`
/// is how a Rust test checks browser wiring without a browser — if the panel
/// stops calling a route, its string disappears from the document.
#[test]
fn spriteframes_bottom_panel_is_wired_in_editor_html() {
    let html = gdeditor::editor_ui::EDITOR_HTML;

    // The tab exists and is a real bottom-panel tab, not a stray button.
    assert!(
        html.contains(r#"data-tab="spriteframes""#),
        "a SpriteFrames bottom tab must exist"
    );
    assert!(
        html.contains(r#"<div class="bottom-content-tab" data-tab="spriteframes">"#),
        "the tab needs a matching content panel or clicking it opens nothing"
    );

    // ⚠️ Godot surfaces this panel ONLY while an AnimatedSprite2D is selected
    // (sprite_frames_editor_plugin.cpp `make_visible`). A permanently visible
    // tab would be the wrong shape even though it "works".
    assert!(
        html.contains("spriteframes-tab-btn"),
        "the tab button needs an id so selection can show/hide it"
    );
    assert!(
        html.contains("cls === 'AnimatedSprite2D'"),
        "visibility must be driven by the selected node's class"
    );
    assert!(
        html.contains("updateSpriteFramesTabVisibility(data.class || data.class_name"),
        "fetchSelected must drive the panel's visibility on every selection change"
    );

    // Every route this bead added has a caller.
    for route in [
        "/api/spriteframes?node_id=",
        "/api/spriteframes/anim/add",
        "/api/spriteframes/anim/set",
        "/api/spriteframes/frame/add",
        "/api/spriteframes/frame/remove",
    ] {
        assert!(html.contains(route), "no client calls {route}");
    }

    // Frames render as thumbnails through the existing preview route.
    assert!(
        html.contains("'/api/preview/file?path=' + encodeURIComponent(tex)"),
        "the frame strip must show texture thumbnails, not bare paths"
    );

    // Drag-and-drop from the FileSystem dock (the bead's third clause). The dock
    // ships `{type, path, name}` as JSON on `text/plain`, so the strip has to
    // parse it — and accept only textures.
    assert!(
        html.contains("payload.type === 'texture'"),
        "the drop target must accept textures and reject scripts/scenes"
    );
    assert!(
        html.contains("strip.addEventListener('drop'"),
        "the frame strip must be a drop target"
    );

    // The handlers are actually installed at startup.
    assert!(
        html.contains("wireSpriteFramesPanel(); // pat-0q2kx.74"),
        "the panel's handlers must be wired during editor boot"
    );
}

/// The bead's persistence clause: authored frames must leave the flat `__sf_*`
/// companion keys behind and land in the saved scene as Godot's real resource.
///
/// ⚠️ A save that emitted `__sf_run_frames = PackedStringArray(...)` would also
/// "persist" the data, and would round-trip through Patina — but Godot cannot
/// parse a property it has never heard of, so the scene would be ours alone.
#[test]
fn authored_frames_save_as_a_spriteframes_sub_resource() {
    let (handle, port, raw) = server();

    http_post(
        port,
        "/api/spriteframes/anim/add",
        &format!(r#"{{"node_id":{raw},"name":"run"}}"#),
    );
    for tex in ["res://run_0.png", "res://run_1.png"] {
        http_post(
            port,
            "/api/spriteframes/frame/add",
            &format!(r#"{{"node_id":{raw},"name":"run","texture":"{tex}"}}"#),
        );
    }
    http_post(
        port,
        "/api/spriteframes/anim/set",
        &format!(r#"{{"node_id":{raw},"name":"run","speed":10.0,"loop":false}}"#),
    );

    let saved = {
        let state = handle.state();
        let state = state.lock().unwrap();
        let root = state.scene_tree.root_id();
        TscnSaver::save_tree(&state.scene_tree, root)
    };

    assert!(
        saved.contains(r#"[sub_resource type="SpriteFrames" id="SpriteFrames_Anim"]"#),
        "the node's frames must be written as a SpriteFrames sub-resource:\n{saved}"
    );
    assert!(
        saved.contains(r#"sprite_frames = SubResource("SpriteFrames_Anim")"#),
        "the node must reference the sub-resource it owns:\n{saved}"
    );

    // Both textures become `[ext_resource]` rows the frame list points at,
    // which is how Godot stores a frame's texture.
    for tex in ["res://run_0.png", "res://run_1.png"] {
        assert!(
            saved.contains(&format!(r#"path="{tex}""#)),
            "{tex} must be declared as an ext_resource:\n{saved}"
        );
    }
    assert!(
        saved.matches(r#""texture": ExtResource("#).count() == 2,
        "both frames must reference their texture:\n{saved}"
    );

    // Godot's animation dictionary shape, including the `&"name"` StringName.
    assert!(
        saved.contains(r#""name": &"run""#),
        "the animation entry uses a StringName name:\n{saved}"
    );
    assert!(
        saved.contains(r#""speed": 10.0"#) && saved.contains(r#""loop": false"#),
        "speed and loop must persist:\n{saved}"
    );

    // The companion keys are an implementation detail of the flat property bag
    // and must NOT leak into the file.
    assert!(
        !saved.contains("__sf_"),
        "flat companion keys must be folded into the sub-resource, not emitted:\n{saved}"
    );
}

/// The bead's undo clause. Every panel edit is one UndoRedo action in Godot;
/// before this the routes wrote the property bag directly, so Ctrl-Z did
/// nothing at all.
#[test]
fn undo_removes_the_last_added_frame() {
    let (handle, port, raw) = server();

    http_post(
        port,
        "/api/spriteframes/anim/add",
        &format!(r#"{{"node_id":{raw},"name":"run"}}"#),
    );
    for tex in ["res://run_0.png", "res://run_1.png"] {
        http_post(
            port,
            "/api/spriteframes/frame/add",
            &format!(r#"{{"node_id":{raw},"name":"run","texture":"{tex}"}}"#),
        );
    }

    let undone = json_of(&http_post(port, "/api/undo", "{}"));
    assert_eq!(undone["ok"], true, "undo failed: {undone}");

    let after = json_of(&http_get(port, &format!("/api/spriteframes?node_id={raw}")));
    let frames = after["animations"][0]["frames"]
        .as_array()
        .expect("frames array");
    assert_eq!(
        frames.len(),
        1,
        "undo must drop the last-added frame, not the whole animation: {after}"
    );
    assert_eq!(frames[0], "res://run_0.png");

    // …and redo puts it back, so the edit is a real reversible command rather
    // than a one-way write the undo stack happened to shadow.
    let redone = json_of(&http_post(port, "/api/redo", "{}"));
    assert_eq!(redone["ok"], true, "redo failed: {redone}");
    let back = json_of(&http_get(port, &format!("/api/spriteframes?node_id={raw}")));
    assert_eq!(
        back["animations"][0]["frames"]
            .as_array()
            .map(|f| f.len()),
        Some(2),
        "redo restores the frame: {back}"
    );

    drop(handle);
}

/// The bead's runtime clause: play must advance the node's frame over stepped
/// time.
///
/// ⚠️ `AnimatedSpritePlayback` already existed with a full state machine and
/// its own unit tests — and ZERO callers. Passing unit tests on a type nothing
/// invokes is exactly the shape of "present but not working", so this drives
/// the real `MainLoop`, not the state machine directly.
#[test]
fn runtime_play_advances_the_frame_over_stepped_time() {
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut sprite = Node::new("Anim", "AnimatedSprite2D");
    // The same flat companions the editor routes author.
    sprite.set_property(
        "__sf_anims__",
        Variant::PackedStringArray(vec!["run".to_string()]),
    );
    sprite.set_property(
        "__sf_run_frames",
        Variant::PackedStringArray(vec![
            "res://a.png".to_string(),
            "res://b.png".to_string(),
            "res://c.png".to_string(),
        ]),
    );
    sprite.set_property("__sf_run_speed", Variant::Float(10.0));
    sprite.set_property("__sf_run_loop", Variant::Bool(true));
    sprite.set_property("animation", Variant::String("run".to_string()));
    sprite.set_property("playing", Variant::Bool(true));
    let sprite_id = tree.add_child(root, sprite).unwrap();

    let mut ml = MainLoop::new(tree);
    // ⚠️ 0.04 rather than 1/60: at 10 fps a frame lasts 0.1s, and six
    // sixtieths land exactly ON that boundary, where f32 rounding decides the
    // outcome. Steps of 0.04 clear each boundary by 0.02 every time.
    let dt = 0.04;

    let frame_of = |ml: &MainLoop| match ml.tree().get_node(sprite_id).map(|n| n.get_property("frame")) {
        Some(Variant::Int(i)) => i,
        other => panic!("frame should be an Int, got {other:?}"),
    };

    for _ in 0..3 {
        ml.step(dt);
    }
    assert_eq!(frame_of(&ml), 1, "the frame must advance at the animation FPS");

    for _ in 0..3 {
        ml.step(dt);
    }
    assert_eq!(frame_of(&ml), 2, "…and keep advancing");

    // ⚠️ The third frame is the last: a LOOPING animation wraps to 0 rather
    // than resting there, which is the branch a frame-count-off-by-one hides.
    for _ in 0..3 {
        ml.step(dt);
    }
    assert_eq!(frame_of(&ml), 0, "a looping animation wraps past its last frame");
}

/// A sprite that was never started stays put — `playing` defaults to false in
/// Godot, so a scene full of AnimatedSprite2Ds does not animate itself.
#[test]
fn an_unstarted_sprite_does_not_advance_but_autoplay_does() {
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut make = |name: &str, autoplay: Option<&str>| {
        let mut n = Node::new(name, "AnimatedSprite2D");
        n.set_property(
            "__sf_anims__",
            Variant::PackedStringArray(vec!["run".to_string()]),
        );
        n.set_property(
            "__sf_run_frames",
            Variant::PackedStringArray(vec![
                "res://a.png".to_string(),
                "res://b.png".to_string(),
                "res://c.png".to_string(),
            ]),
        );
        n.set_property("__sf_run_speed", Variant::Float(10.0));
        if let Some(a) = autoplay {
            n.set_property("autoplay", Variant::String(a.to_string()));
        }
        n
    };

    let idle = tree.add_child(root, make("Idle", None)).unwrap();
    let auto = tree.add_child(root, make("Auto", Some("run"))).unwrap();

    // ⚠️ Step size is chosen to land clear of a frame boundary AND clear of the
    // wrap. 3 frames at 10 fps is 0.1s per frame and wraps at 0.3s; 3 steps of
    // 0.04 = 0.12s sits inside frame 1 with margin at both ends. An earlier
    // version ran 0.2s over a 2-frame clip, which had already looped back to 0
    // by the time it was measured — the engine was right, the assertion was not.
    let mut ml = MainLoop::new(tree);
    for _ in 0..3 {
        ml.step(0.04);
    }

    let frame = |id| match ml.tree().get_node(id).map(|n| n.get_property("frame")) {
        Some(Variant::Int(i)) => i,
        // A node that never animated may never have had `frame` written.
        _ => 0,
    };
    assert_eq!(frame(idle), 0, "an unstarted sprite must not animate");
    assert_eq!(
        frame(auto),
        1,
        "autoplay must start the named animation, as Godot's AnimatedSprite2D does"
    );
}

/// Depth-first search for a node by name, so the assertion does not depend on
/// where in the tree the reloaded scene was instanced.
fn find_named(tree: &SceneTree, from: gdscene::node::NodeId, name: &str) -> Option<gdscene::node::NodeId> {
    let mut ids = Vec::new();
    tree.collect_subtree_top_down(from, &mut ids);
    ids.into_iter()
        .find(|id| tree.get_node(*id).map(|n| n.name() == name).unwrap_or(false))
}

/// The full loop the bead's wiring requirement describes: author in the panel →
/// save → load → the loaded scene animates.
///
/// ⚠️ A writer without a reader would still pass the persistence test above
/// while leaving the frames unusable — the saved file would be correct and
/// nothing would ever animate from it again.
#[test]
fn saved_frames_reload_and_animate() {
    let (handle, port, raw) = server();

    http_post(
        port,
        "/api/spriteframes/anim/add",
        &format!(r#"{{"node_id":{raw},"name":"run"}}"#),
    );
    for tex in ["res://a.png", "res://b.png", "res://c.png"] {
        http_post(
            port,
            "/api/spriteframes/frame/add",
            &format!(r#"{{"node_id":{raw},"name":"run","texture":"{tex}"}}"#),
        );
    }
    http_post(
        port,
        "/api/spriteframes/anim/set",
        &format!(r#"{{"node_id":{raw},"name":"run","speed":10.0,"autoplay":"run"}}"#),
    );

    let saved = {
        let state = handle.state();
        let state = state.lock().unwrap();
        let root = state.scene_tree.root_id();
        TscnSaver::save_tree(&state.scene_tree, root)
    };

    // -- reload ---------------------------------------------------------------
    let scene = gdscene::packed_scene::PackedScene::from_tscn(&saved)
        .unwrap_or_else(|e| panic!("saved scene must re-parse ({e:?}):\n{saved}"));
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    // ⚠️ Argument order is (tree, parent, scene); the return value is the new
    // subtree root, which is what to search from — the absolute path shifts
    // depending on where the scene was instanced.
    let scene_root = gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene)
        .expect("scene instantiates");

    let sprite = find_named(&tree, scene_root, "Anim")
        .unwrap_or_else(|| panic!("the AnimatedSprite2D must survive the round trip:\n{saved}"));

    // The frame list came back off the sub-resource, resolved to real paths.
    let frames = tree
        .get_node(sprite)
        .map(|n| n.get_property("__sf_run_frames"));
    assert_eq!(
        frames,
        Some(Variant::PackedStringArray(vec![
            "res://a.png".to_string(),
            "res://b.png".to_string(),
            "res://c.png".to_string(),
        ])),
        "frames must unflatten back to their texture paths:\n{saved}"
    );

    // -- and it animates ------------------------------------------------------
    let mut ml = MainLoop::new(tree);
    // 3 frames at 10 fps: 0.12s is inside frame 1, well clear of the 0.3s wrap.
    for _ in 0..3 {
        ml.step(0.04);
    }
    let frame = match ml.tree().get_node(sprite).map(|n| n.get_property("frame")) {
        Some(Variant::Int(i)) => i,
        other => panic!("frame should be an Int, got {other:?}"),
    };
    assert_eq!(
        frame, 1,
        "a reloaded scene must animate from its autoplay animation"
    );
}

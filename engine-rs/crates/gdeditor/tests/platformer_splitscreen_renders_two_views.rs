//! pat-8mb8y — the 2D platformer's split-screen scene renders BOTH views.
//!
//! `games/platformer/game_splitscreen.tscn` (unmodified upstream demo) puts the
//! level and both players in `Viewport1`, a `ParallaxBackground` alone in
//! `Viewport2`, each SubViewport under a stretching `SubViewportContainer` in
//! an `HSplitContainer`. `game_splitscreen.gd` then runs:
//!
//! ```gdscript
//! viewport_2.world_2d = viewport_1.world_2d
//! player_2.camera.custom_viewport = viewport_2
//! player_2.camera.make_current()
//! ```
//!
//! Real Godot 4.6.1 (`scripts/godot-reference.mjs games/platformer --scene
//! game_splitscreen.tscn`) shows two side-by-side views of the same level, the
//! left centred on Player1 and the right on Player2.
//!
//! At HEAD Patina failed three ways:
//! - `player_2.camera.custom_viewport = viewport_2` raised "cannot assign member
//!   'custom_viewport' on ObjectId", so `_ready` stopped there;
//! - `Camera2D.make_current()` had no dispatch arm;
//! - the Play frame was ONE canvas under ONE camera, so it drew one full-width
//!   view and no split.
//!
//! This drives the editor's own Play path (`/api/runtime/play` then
//! `prepare_play_frame`, what the browser shows) on the real scene.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::viewport_2d_render::{prepare_play_frame, Render2DItem};
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The project's base resolution, so the window stretch is the identity.
const W: u32 = 800;
const H: u32 = 480;

fn request(port: u16, method: &str, path: &str, body: &str) -> String {
    let mut s = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(60))).ok();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out.split("\r\n\r\n").nth(1).unwrap_or("").to_string()
}

fn game() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/platformer")
        .canonicalize()
        .expect("games/platformer fixture")
}

struct Frame {
    items: Vec<Render2DItem>,
    errors: Vec<String>,
    /// `__world_2d_owner__` on Viewport2 and Viewport1's raw id.
    world_owner_2: gdvariant::Variant,
    viewport_1: u64,
    viewport_2: u64,
    /// Player2's camera: the viewport it drives and whether it is current there.
    camera_2_viewport: Option<u64>,
    camera_2_current_in_2: bool,
    /// Raw node ids, resolved by path in the RUNNING tree.
    sprite_1: u64,
    sprite_2: u64,
    background_2: Vec<u64>,
}

fn play_splitscreen() -> Frame {
    gdobject::class_db::register_all_classes();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let handle = EditorServerHandle::start(port, EditorState::new(SceneTree::new()));
    let g = game();
    request(
        port,
        "POST",
        "/api/project/open",
        &format!(r#"{{"path":"{}"}}"#, g.display()),
    );
    let load = request(
        port,
        "POST",
        "/api/scene/load",
        &format!(
            r#"{{"path":"{}"}}"#,
            g.join("game_splitscreen.tscn").display()
        ),
    );
    assert!(load.contains("\"ok\""), "load: {load}");
    let r = request(port, "POST", "/api/runtime/play", "{}");
    assert!(r.contains("\"ok\":true"), "play: {r}");

    let state = handle.state();
    let mut s = state.lock().unwrap();
    let st = &mut *s;
    let settings = st
        .run_project_settings
        .clone()
        .unwrap_or_else(|| st.project_settings.clone());
    let ml = st.run_main_loop.as_mut().expect("a running main loop");
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }
    // What the editor's Play reports as the game's errors (`play` MCP, the
    // Output panel): the scripts' error channels plus failed calls.
    let mut errors = ml.tree_mut().drain_script_errors();
    errors.extend(ml.tree_mut().drain_script_output().errors);
    let base = "/root/GameSplitscreen/Black/SplitContainer";
    let id = |path: &str| {
        ml.tree()
            .get_node_by_path(path)
            .unwrap_or_else(|| panic!("running tree must contain {path}"))
    };
    let sprite_1 = id(&format!(
        "{base}/ViewportContainer1/Viewport1/Player1/Sprite2D"
    ))
    .raw();
    let sprite_2 = id(&format!(
        "{base}/ViewportContainer1/Viewport1/Player2/Sprite2D"
    ))
    .raw();
    let vp1 = id(&format!("{base}/ViewportContainer1/Viewport1"));
    let vp2 = id(&format!("{base}/ViewportContainer2/Viewport2"));
    let cam2 = id(&format!(
        "{base}/ViewportContainer1/Viewport1/Player2/Camera"
    ));
    let world_owner_2 = ml
        .tree()
        .get_node(vp2)
        .unwrap()
        .get_property(gdscene::canvas_render::WORLD_2D_OWNER_PROP);
    let camera_2_viewport =
        gdscene::behavioral_2d::camera2d_viewport(ml.tree(), cam2).map(|v| v.raw());
    let camera_2_current_in_2 =
        gdscene::behavioral_2d::current_camera2d_in(ml.tree(), Some(vp2)) == Some(cam2);
    let bg2 = id(&format!(
        "{base}/ViewportContainer2/Viewport2/ParallaxBackground"
    ));
    let mut background_2 = Vec::new();
    ml.tree().collect_subtree_top_down(bg2, &mut background_2);
    let background_2 = background_2.into_iter().map(|n| n.raw()).collect();
    let items = prepare_play_frame(ml.tree_mut(), W, H, &mut st.texture_cache, &settings)
        .items()
        .to_vec();
    drop(s);
    request(port, "POST", "/api/runtime/stop", "");
    Frame {
        items,
        errors,
        world_owner_2,
        viewport_1: vp1.raw(),
        viewport_2: vp2.raw(),
        camera_2_viewport,
        camera_2_current_in_2,
        sprite_1,
        sprite_2,
        background_2,
    }
}

/// `(screen x of the item's origin, its clip rect's left edge and width)`.
fn placements(items: &[Render2DItem], node: u64) -> Vec<(f32, f32, f32)> {
    items
        .iter()
        .filter(|i| i.node_id == node)
        .map(|i| {
            let clip = i
                .clip_rect
                .expect("a sub-viewport item is clipped to its container");
            (i.global_transform.origin.x, clip.position.x, clip.size.x)
        })
        .collect()
}

#[test]
fn splitscreen_ready_runs_to_the_end() {
    let f = play_splitscreen();
    assert!(
        f.errors.is_empty(),
        "game_splitscreen.gd's _ready must run to the end: {:?}",
        f.errors
    );
    // Its three lines, by their EFFECTS (the editor's Play forwards errors to
    // its own log, so an empty error list alone proves nothing).
    assert_eq!(
        f.world_owner_2,
        gdvariant::Variant::Int(f.viewport_1 as i64),
        "`viewport_2.world_2d = viewport_1.world_2d` must make Viewport2 draw Viewport1's world"
    );
    assert_eq!(
        f.camera_2_viewport,
        Some(f.viewport_2),
        "`player_2.camera.custom_viewport = viewport_2` must point Player2's camera at Viewport2"
    );
    assert!(
        f.camera_2_current_in_2,
        "`player_2.camera.make_current()` must leave it Viewport2's current camera"
    );
}

/// Both players live in Viewport1's world, which Viewport2 shares, so each
/// player's sprite is drawn twice: once per view. The left view (clip at x=0)
/// is centred on Player1, the right view on Player2 — Godot's picture.
#[test]
fn each_view_is_centred_on_its_own_player() {
    let f = play_splitscreen();
    let one = placements(&f.items, f.sprite_1);
    let two = placements(&f.items, f.sprite_2);
    assert_eq!(one.len(), 2, "Player1 must be drawn in BOTH views: {one:?}");
    assert_eq!(two.len(), 2, "Player2 must be drawn in BOTH views: {two:?}");

    let left = |p: &[(f32, f32, f32)]| *p.iter().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
    let right = |p: &[(f32, f32, f32)]| *p.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
    let (x1, l_x, l_w) = left(&one);
    let (x2, r_x, r_w) = right(&two);
    assert!(l_x.abs() < 1.0, "the left view starts at x=0, got {l_x}");
    assert!(
        r_x > 380.0 && r_x + r_w <= W as f32 + 0.5,
        "the right view occupies the right half, got x={r_x} w={r_w}"
    );
    assert!(
        (l_w - r_w).abs() <= 2.0 && l_w > 380.0,
        "the two views split the width evenly, got {l_w} and {r_w}"
    );
    // Neither camera is clamped horizontally at the spawn points (limit_left
    // -715), so each player sits at its view's horizontal centre.
    assert!(
        (x1 - (l_x + l_w * 0.5)).abs() < 2.0,
        "the LEFT view must centre on Player1: sprite x {x1}, view centre {}",
        l_x + l_w * 0.5
    );
    assert!(
        (x2 - (r_x + r_w * 0.5)).abs() < 2.0,
        "the RIGHT view must centre on Player2 (its camera's custom_viewport): \
         sprite x {x2}, view centre {}",
        r_x + r_w * 0.5
    );
    // Player2 stands 60 px right of Player1; in Player1's view it is drawn
    // 60 px right of the centre, which pins the shared world, not a copy.
    let (x2_in_left, _, _) = left(&two);
    assert!(
        (x2_in_left - x1 - 60.0).abs() < 2.0,
        "Player2 in the left view must sit 60 px right of Player1: {x2_in_left} vs {x1}"
    );
}

/// Viewport2's own ParallaxBackground is a CanvasLayer of Viewport2: it draws in
/// the right view only, never over the left one.
#[test]
fn the_second_viewports_background_stays_in_the_right_view() {
    let f = play_splitscreen();
    let bg: Vec<&Render2DItem> = f
        .items
        .iter()
        .filter(|i| f.background_2.contains(&i.node_id))
        .collect();
    assert!(!bg.is_empty(), "Viewport2's ParallaxBackground must draw");
    for item in bg {
        let clip = item.clip_rect.expect("clipped to its container");
        assert!(
            clip.position.x > 380.0,
            "Viewport2's background must be clipped to the right view, got {clip:?}"
        );
    }
}

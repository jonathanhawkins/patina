//! The 2D edit view must show what Play shows, on every real game in `games/`.
//!
//! On the platformer demo the edit view and Play disagreed three ways while
//! every test was green, because no test held them against each other:
//!
//! 1. `res://` roots were only installed at Play, so the edit-time parse could
//!    not read `tile_set = ExtResource(...)` — no TileSet, 16 px cells, no tiles;
//! 2. instance previews were appended after the only (canvas layer, z) sort, so
//!    a CanvasLayer -100 ParallaxBackground covered the whole level;
//! 3. tile clicks used image pixels as world coordinates.
//!
//! (3) is pinned in `painting_a_loaded_tilemap_keeps_its_existing_tiles.rs`.
//! This file pins (1) and (2) for EVERY game that has a playtest scenario,
//! loading each scene through the editor's own HTTP routes the way the browser
//! does, then comparing the edit view's scene items with Play's first frame:
//!
//! - both draw in non-decreasing canvas-layer order;
//! - both draw the same tiles, at the same tile size;
//! - every texture Play draws on its first frame is drawn in the edit view.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::scene_renderer::edit_view_scene_items;
use gdeditor::viewport_2d_render::{prepare_play_frame, Render2DItem, Render2DKind};
use gdscene::SceneTree;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

const W: u32 = 1152;
const H: u32 = 648;

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

/// `(game dir, scene path relative to it)` for every game with a scenario.
fn scenario_scenes() -> Vec<(PathBuf, String)> {
    let games = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games")
        .canonicalize()
        .unwrap();
    let mut out = Vec::new();
    for e in std::fs::read_dir(&games).unwrap().flatten() {
        let sc = e.path().join("playtest.json");
        let Ok(text) = std::fs::read_to_string(&sc) else {
            continue;
        };
        let v: serde_json::Value = serde_json::from_str(&text).expect("playtest.json parses");
        out.push((
            e.path(),
            v["scene"]
                .as_str()
                .expect("scenario names a scene")
                .to_string(),
        ));
    }
    out.sort();
    out
}

#[derive(Default, Debug)]
struct Summary {
    layers_in_order: bool,
    first_out_of_order: Option<(usize, i64, i64)>,
    /// tile size (w, h) -> how many quads.
    tiles: BTreeMap<(i64, i64), usize>,
    /// texture path -> how many sprite items.
    textures: BTreeMap<String, usize>,
}

fn summarize(items: &[Render2DItem]) -> Summary {
    let mut s = Summary {
        layers_in_order: true,
        ..Default::default()
    };
    let mut last = i64::MIN;
    for (i, it) in items.iter().enumerate() {
        if it.canvas_layer < last && s.first_out_of_order.is_none() {
            s.layers_in_order = false;
            s.first_out_of_order = Some((i, last, it.canvas_layer));
        }
        last = last.max(it.canvas_layer);
        match &it.kind {
            Render2DKind::TileMap { cells } => {
                for q in cells {
                    *s.tiles
                        .entry((q.dest.size.x.round() as i64, q.dest.size.y.round() as i64))
                        .or_default() += 1;
                }
            }
            Render2DKind::Sprite { texture, .. } if !texture.is_empty() => {
                *s.textures.entry(texture.clone()).or_default() += 1;
            }
            _ => {}
        }
    }
    s
}

fn edit_and_play(game: &Path, scene: &str) -> (Summary, Summary) {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let handle = EditorServerHandle::start(port, EditorState::new(SceneTree::new()));
    request(
        port,
        "POST",
        "/api/project/open",
        &format!(r#"{{"path":"{}"}}"#, game.display()),
    );
    let load = request(
        port,
        "POST",
        "/api/scene/load",
        &format!(r#"{{"path":"{}"}}"#, game.join(scene).display()),
    );
    assert!(
        load.contains("\"ok\""),
        "{}: load {scene}: {load}",
        game.display()
    );

    let edit = {
        let state = handle.state();
        let mut s = state.lock().unwrap();
        let root = s.texture_cache.project_root().to_string();
        edit_view_scene_items(&mut s.scene_tree, &root, W, H)
    };
    // Play Current: the same scene the edit view shows.
    let r = request(port, "POST", "/api/runtime/play", "{}");
    assert!(r.contains("\"ok\":true"), "{}: play: {r}", game.display());
    let play = {
        let state = handle.state();
        let mut s = state.lock().unwrap();
        let st = &mut *s;
        let settings = st
            .run_project_settings
            .clone()
            .unwrap_or_else(|| st.project_settings.clone());
        let ml = st.run_main_loop.as_mut().expect("a running main loop");
        prepare_play_frame(ml.tree_mut(), W, H, &mut st.texture_cache, &settings)
            .items()
            .to_vec()
    };
    request(port, "POST", "/api/runtime/stop", "");
    (summarize(&edit), summarize(&play))
}

#[test]
fn every_games_edit_view_draws_what_play_draws() {
    gdobject::class_db::register_all_classes();
    let scenes = scenario_scenes();
    assert!(
        scenes.len() >= 3,
        "expected the real games' scenarios, found {scenes:?}"
    );
    let mut failures = Vec::new();
    for (game, scene) in &scenes {
        let name = game.file_name().unwrap().to_string_lossy().to_string();
        let (edit, play) = edit_and_play(game, scene);
        if !edit.layers_in_order {
            failures.push(format!(
                "{name}: the edit view draws a lower canvas layer after a higher one {:?}",
                edit.first_out_of_order
            ));
        }
        if !play.layers_in_order {
            failures.push(format!(
                "{name}: Play draws a lower canvas layer after a higher one {:?}",
                play.first_out_of_order
            ));
        }
        if edit.tiles != play.tiles {
            failures.push(format!(
                "{name}: tiles differ — edit {:?} vs Play {:?} (size → count)",
                edit.tiles, play.tiles
            ));
        }
        let missing: Vec<&String> = play
            .textures
            .keys()
            .filter(|t| !edit.textures.contains_key(*t))
            .collect();
        if !missing.is_empty() {
            failures.push(format!(
                "{name}: Play draws textures the edit view does not: {missing:?}"
            ));
        }
        eprintln!(
            "{name} ({scene}): edit {} textures / {} tiles, Play {} textures / {} tiles",
            edit.textures.len(),
            edit.tiles.values().sum::<usize>(),
            play.textures.len(),
            play.tiles.values().sum::<usize>()
        );
    }
    assert!(
        failures.is_empty(),
        "edit view ≠ Play:\n{}",
        failures.join("\n")
    );
}

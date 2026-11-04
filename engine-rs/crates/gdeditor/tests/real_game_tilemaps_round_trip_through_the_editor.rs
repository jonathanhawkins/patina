//! Fixtures harvested from the real games, not invented.
//!
//! The synthetic tilemap fixtures all happened to use positive coordinates and
//! non-zero source ids, which is exactly where the editor dropped cells: source
//! 0 and negative cells could not live in its dense grid, so painting, erasing
//! or undoing on the platformer demo's real level silently lost them.
//!
//! This walks EVERY `TileMapLayer` in EVERY `.tscn` under `games/` and:
//!
//! 1. decodes the layer's `tile_map_data` straight from the file text with an
//!    independent decoder (base64 → 12-byte cells), and requires the editor's
//!    loaded node to hold exactly those cells;
//! 2. for each edge class the real data contains — source 0, a negative x, a
//!    negative y — takes one real cell, erases it through the editor's route,
//!    then undoes, and requires the layer to be back to the file's cells
//!    exactly; then paints that cell's source into an empty neighbour and
//!    undoes again.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::SceneTree;
use gdvariant::Variant;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

type Cells = BTreeMap<(i16, i16), (u16, u16, u16, u16)>;

fn post(port: u16, path: &str, body: &str) -> String {
    let mut s = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(60))).ok();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

/// Standard base64, written here so the test does not trust the engine's.
fn base64_decode(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => panic!("not base64: {}", c as char),
    } as u32;
    let bytes: Vec<u8> = s.bytes().filter(|&c| c != b'=').collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c) << (18 - 6 * i);
        }
        for i in 0..chunk.len().saturating_sub(1) {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    out
}

/// TileMapLayer format 0: a 2-byte header, then 12 bytes per cell —
/// x, y (i16), source, atlas x, atlas y, alternative (u16).
fn decode(bytes: &[u8]) -> Cells {
    let mut out = Cells::new();
    let mut i = 2;
    while i + 12 <= bytes.len() {
        let h = |k: usize| i16::from_le_bytes([bytes[i + k], bytes[i + k + 1]]);
        let u = |k: usize| u16::from_le_bytes([bytes[i + k], bytes[i + k + 1]]);
        out.insert((h(0), h(2)), (u(4), u(6), u(8), u(10)));
        i += 12;
    }
    out
}

/// `(scene file, layer name, cells)` for every TileMapLayer in the games.
fn real_layers() -> Vec<(PathBuf, String, Cells)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "tscn") {
                out.push(p);
            }
        }
    }
    let games = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../games").canonicalize().unwrap();
    let mut files = Vec::new();
    walk(&games, &mut files);
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        let mut name = None;
        for line in text.lines() {
            if line.starts_with("[node ") {
                name = line.contains("type=\"TileMapLayer\"").then(|| {
                    line.split("name=\"").nth(1).unwrap().split('"').next().unwrap().to_string()
                });
            } else if let (Some(n), Some(rest)) = (&name, line.strip_prefix("tile_map_data = PackedByteArray(\"")) {
                let b64 = rest.trim_end_matches("\")");
                out.push((f.clone(), n.clone(), decode(&base64_decode(b64))));
            }
        }
    }
    out
}

/// Only the cells that differ, so a failure names them instead of dumping 341.
fn diff(a: &Cells, b: &Cells) -> String {
    let mut out = Vec::new();
    for k in a.keys().chain(b.keys()).collect::<std::collections::BTreeSet<_>>() {
        if a.get(k) != b.get(k) {
            out.push(format!("{k:?}: {:?} -> {:?}", b.get(k), a.get(k)));
        }
    }
    out.join(", ")
}

fn layer_cells(handle: &EditorServerHandle, name: &str) -> (u64, Cells) {
    let state = handle.state();
    let s = state.lock().unwrap();
    let mut ids = Vec::new();
    s.scene_tree.collect_subtree_top_down(s.scene_tree.root_id(), &mut ids);
    let id = *ids
        .iter()
        .find(|&&i| {
            let n = s.scene_tree.get_node(i).unwrap();
            n.name() == name && n.class_name() == "TileMapLayer"
        })
        .unwrap_or_else(|| panic!("no TileMapLayer {name} in the loaded scene"));
    match s.scene_tree.get_node(id).unwrap().get_property("tile_map_data") {
        Variant::PackedByteArray(b) => (id.raw(), decode(&b)),
        other => panic!("{name}.tile_map_data is {other:?}"),
    }
}

#[test]
fn every_real_tilemap_loads_edits_and_undoes_without_losing_a_cell() {
    gdobject::class_db::register_all_classes();
    let layers = real_layers();
    assert!(!layers.is_empty(), "no TileMapLayer found under games/ — the harvest is broken");
    let mut classes_seen = (false, false, false);
    for (scene, layer, file_cells) in &layers {
        let game = scene.ancestors().find(|p| p.join("project.godot").exists()).unwrap();
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let handle = EditorServerHandle::start(port, EditorState::new(SceneTree::new()));
        post(port, "/api/project/open", &format!(r#"{{"path":"{}"}}"#, game.display()));
        let r = post(port, "/api/scene/load", &format!(r#"{{"path":"{}"}}"#, scene.display()));
        assert!(r.contains("200 OK"), "{}: {r}", scene.display());

        let (nid, loaded) = layer_cells(&handle, layer);
        assert!(&loaded == file_cells, "{}:{layer}: the editor holds different cells than the file: {}", scene.display(), diff(&loaded, file_cells));

        // One real cell per edge class the data has.
        let picks: Vec<(&str, (i16, i16))> = [
            ("source 0", file_cells.iter().find(|(_, v)| v.0 == 0).map(|(k, _)| *k)),
            ("negative x", file_cells.keys().find(|k| k.0 < 0).copied()),
            ("negative y", file_cells.keys().find(|k| k.1 < 0).copied()),
        ]
        .into_iter()
        .filter_map(|(what, c)| c.map(|c| (what, c)))
        .collect();
        for (what, _) in &picks {
            match *what {
                "source 0" => classes_seen.0 = true,
                "negative x" => classes_seen.1 = true,
                _ => classes_seen.2 = true,
            }
        }
        for (what, (x, y)) in picks {
            let r = post(port, "/api/tilemap/erase", &format!(r#"{{"node_id":{nid},"x":{x},"y":{y}}}"#));
            assert!(r.contains("200 OK"), "{layer} erase {what} cell ({x},{y}): {r}");
            let (_, after) = layer_cells(&handle, layer);
            assert!(!after.contains_key(&(x, y)), "{layer}: erasing the {what} cell ({x},{y}) did nothing");
            let mut expect = file_cells.clone();
            expect.remove(&(x, y));
            assert!(after == expect, "{layer}: erasing the {what} cell ({x},{y}) changed other cells: {}", diff(&after, &expect));
            post(port, "/api/undo", "{}");
            {
                let now = layer_cells(&handle, layer).1;
                assert!(&now == file_cells, "{layer}: undo of the {what} erase did not restore the file's cells: {}", diff(&now, file_cells));
            }

            // Paint that cell's source into an empty neighbour, then undo.
            let src = file_cells[&(x, y)].0;
            let (ex, ey) = (-30i16..30)
                .flat_map(|dy| (-30i16..30).map(move |dx| (x + dx, y + dy)))
                .find(|c| !file_cells.contains_key(c) && (c.0 < 0) == (x < 0) && (c.1 < 0) == (y < 0))
                .expect("an empty cell in the same quadrant");
            let r = post(port, "/api/tilemap/paint", &format!(r#"{{"node_id":{nid},"x":{ex},"y":{ey},"tile_id":{src}}}"#));
            assert!(r.contains("200 OK"), "{layer} paint source {src} at ({ex},{ey}): {r}");
            let (_, painted) = layer_cells(&handle, layer);
            assert_eq!(painted.get(&(ex, ey)).map(|v| v.0), Some(src), "{layer}: painting source {src} at ({ex},{ey}) ({what} quadrant) was dropped");
            let mut expect = file_cells.clone();
            expect.insert((ex, ey), painted[&(ex, ey)]);
            assert!(painted == expect, "{layer}: the paint at ({ex},{ey}) changed other cells: {}", diff(&painted, &expect));
            post(port, "/api/undo", "{}");
            {
                let now = layer_cells(&handle, layer).1;
                assert!(&now == file_cells, "{layer}: undo of the paint did not restore the file's cells: {}", diff(&now, file_cells));
            }
        }
    }
    // The point of harvesting: the real data DOES contain the edge cases.
    assert!(classes_seen.0 && classes_seen.1, "the real games no longer contain source-0 / negative cells: {classes_seen:?}");
}

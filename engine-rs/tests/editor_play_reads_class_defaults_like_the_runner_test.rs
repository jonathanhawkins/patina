//! pat-7puwj: editor Play must read a property the `.tscn` never sets exactly as
//! the runner does — as the class default, not `null`.
//!
//! truck_town's `car_select.gd` passes `button_sdfgi.button_pressed` (never
//! authored on that CheckBox) into a `bool` parameter. The runner read `false`;
//! editor Play read `null` and the call died with "expected element of type
//! bool, got Nil", so the town never loaded in the browser.
//!
//! The cause was boot ORDER, not a missing default: the loader seeds each node's
//! ClassDB defaults once, as it builds it, and the editor binary loaded its
//! launch scene BEFORE anything populated ClassDB. Every HTTP-driven editor test
//! opens its scene through `/api/scene/load`, long after `EditorState::new`
//! registered the classes, so none of them could see it.
//!
//! So this test drives the binary's own entry point,
//! `editor_server::load_launch_scene`, and must not register ClassDB itself
//! first. It opens EVERY game's main scene that way (all before the runner
//! boots, since `boot_project` registers ClassDB for the whole process), clones
//! it the way Play does (`clone_scene_tree`), then boots each game through the
//! runner and compares every ClassDB property of every node, node by node.
//!
//! ⚠️ Keep this file to ONE test: a sibling that registered ClassDB first in the
//! same process would make the editor path seeded and this test vacuous.

use gdeditor::editor_server::{apply_project_path_roots, clone_scene_tree, load_launch_scene};
use gdscene::node::NodeId;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::path::{Path, PathBuf};

fn games() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../games").canonicalize().unwrap();
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("project.godot").is_file())
        .collect();
    out.sort();
    out
}

fn main_scene_file(game: &Path) -> PathBuf {
    let text = std::fs::read_to_string(game.join("project.godot")).unwrap();
    let line = text
        .lines()
        .find(|l| l.starts_with("run/main_scene="))
        .unwrap_or_else(|| panic!("{}: no run/main_scene", game.display()));
    let res = line.trim_start_matches("run/main_scene=").trim_matches('"');
    let rel = res.strip_prefix("res://").unwrap_or_else(|| panic!("main_scene not res://: {res}"));
    game.join(rel)
}

/// `(path relative to the scene root, class, id)` for every node of the subtree.
fn nodes(tree: &SceneTree, root: NodeId) -> Vec<(String, String, NodeId)> {
    let mut ids = Vec::new();
    tree.collect_subtree_top_down(root, &mut ids);
    ids.into_iter()
        .filter_map(|id| {
            let rel = if id == root { ".".to_string() } else { tree.get_path_to(root, id)? };
            Some((rel, tree.get_node(id)?.class_name().to_string(), id))
        })
        .collect()
}

#[test]
fn editor_play_reads_every_unset_property_as_the_runner_does_on_every_game() {
    let games = games();
    assert!(games.len() >= 6, "expected the six real games, found {games:?}");

    // (1) The editor binary's path, for every game, BEFORE any runner boot.
    let mut editor_trees = Vec::new();
    for game in &games {
        apply_project_path_roots(&game.to_string_lossy());
        let file = main_scene_file(game);
        let (tree, _root, _source) = load_launch_scene(&file.to_string_lossy())
            .unwrap_or_else(|e| panic!("{}: editor open: {e}", game.display()));
        // Play runs a CLONE of the edited tree.
        let play = clone_scene_tree(&tree);
        editor_trees.push(play);
    }

    // The bead's concrete symptom, asserted on its own so the differential
    // below can never pass vacuously on truck_town.
    let tt = games.iter().position(|g| g.ends_with("truck_town")).expect("truck_town");
    {
        let play = &editor_trees[tt];
        let root = play.get_node(play.root_id()).unwrap().children()[0];
        for (path, want) in [
            ("OptionsPanel/MarginContainer/HBoxContainer/SDFGI", "button_pressed"),
            ("MoodPanel/MarginContainer/HBoxContainer/Sunrise", "button_pressed"),
        ] {
            let id = play.get_node_relative(root, path).unwrap_or_else(|| panic!("no {path}"));
            assert_eq!(
                play.get_node(id).unwrap().get_property(want),
                Variant::Bool(false),
                "editor Play: {path}.{want} (unset in the .tscn) must read the class default false"
            );
        }
        let sdfgi = play
            .get_node_relative(root, "OptionsPanel/MarginContainer/HBoxContainer/SDFGI")
            .unwrap();
        assert_eq!(play.get_node(sdfgi).unwrap().get_property("visible"), Variant::Bool(true));
    }

    // (2) The runner's path, and the node-by-node differential.
    let mut failures = Vec::new();
    let mut compared = 0usize;
    for (game, play) in games.iter().zip(&editor_trees) {
        let name = game.file_name().unwrap().to_string_lossy().to_string();
        let booted = patina_runner::project_boot::boot_project(game)
            .unwrap_or_else(|e| panic!("{name}: runner boot: {e}"));
        let runner = &booted.tree;
        let runner_root = booted.main_scene_root.expect("a main scene");
        let play_root = play.get_node(play.root_id()).unwrap().children()[0];
        let play_nodes = nodes(play, play_root);
        let mut game_mismatches = 0usize;
        for (path, class, rid) in nodes(runner, runner_root) {
            let Some((_, pclass, pid)) = play_nodes.iter().find(|(p, _, _)| *p == path) else {
                failures.push(format!("{name}: {path} ({class}) is in the runner's tree but not editor Play's"));
                continue;
            };
            assert_eq!(&class, pclass, "{name}: {path} class differs");
            let rnode = runner.get_node(rid).unwrap();
            let pnode = play.get_node(*pid).unwrap();
            for prop in gdobject::class_db::get_property_list(&class) {
                compared += 1;
                let (r, p) = (rnode.get_property(&prop.name), pnode.get_property(&prop.name));
                if r != p {
                    game_mismatches += 1;
                    if game_mismatches <= 8 {
                        failures.push(format!("{name}: {path} ({class}).{}: runner {r:?} vs editor Play {p:?}", prop.name));
                    }
                }
            }
        }
        if game_mismatches > 8 {
            failures.push(format!("{name}: … {} more mismatches", game_mismatches - 8));
        }
    }
    assert!(compared > 10_000, "compared only {compared} property reads — the walk is broken");
    assert!(failures.is_empty(), "editor Play ≠ runner on unset/authored properties:\n{}", failures.join("\n"));
}

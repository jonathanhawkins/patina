//! Saving every real game's scene, as the editor does, against the file Godot
//! wrote (pat-i5igl, pat-l0afa).
//!
//! The reference is Godot's own text saver: each `games/*/**/*.tscn` is an
//! unmodified upstream file. Opened for editing and saved:
//!
//! - unedited, a scene must come back byte-for-byte;
//! - with an edit, it must come back as the original plus exactly that edit —
//!   every sub-resource (nested ones included: coin.tscn's
//!   `GradientTexture2D -> Gradient`), uid, `unique_id` and an instance's
//!   untouched foreign nodes kept.
//!
//! Before, an edited scene without instances was written by the generator
//! alone, which lost the nested sub-resources and added derived
//! `basis`/`quaternion`/`position` rows; one opened through any path that took
//! no editor-side baseline (Save As, a re-opened tab, MCP) was always written
//! that way. The tree now carries its own baseline.
//!
//! The generator itself (a scene never read from a file) is checked for the
//! two defects pat-l0afa names, on the same files.

use gdscene::packed_scene::{add_packed_scene_to_tree_for_editing, PackedScene};
use gdscene::scene_saver::TscnSaver;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::path::{Path, PathBuf};

fn games() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../games")
}

/// Every `.tscn` of every game, grouped by game directory.
fn real_scenes() -> Vec<(PathBuf, Vec<PathBuf>)> {
    let mut out = Vec::new();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(games())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("project.godot").exists())
        .collect();
    dirs.sort();
    for g in dirs {
        let mut scenes = Vec::new();
        let mut stack = vec![g.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    if !p.ends_with(".godot") {
                        stack.push(p);
                    }
                } else if p.extension().is_some_and(|e| e == "tscn") {
                    scenes.push(p);
                }
            }
        }
        scenes.sort();
        out.push((g, scenes));
    }
    assert!(
        out.len() >= 6,
        "expected the six real games, found {}",
        out.len()
    );
    out
}

/// `res://` is process-global and `cargo test` runs these tests on parallel
/// threads of one process, each rooted at a different game: every test holds
/// this for its whole body.
static ROOTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn roots() -> std::sync::MutexGuard<'static, ()> {
    ROOTS.lock().unwrap_or_else(|e| e.into_inner())
}

fn open(game: &Path, scene: &Path) -> (String, SceneTree, gdscene::node::NodeId) {
    gdobject::class_db::register_all_classes();
    gdcore::set_path_roots(Some(game.to_path_buf()), None);
    let text = std::fs::read_to_string(scene).unwrap();
    let packed = PackedScene::from_tscn(&text).unwrap();
    let mut tree = SceneTree::new();
    let r = tree.root_id();
    let root = add_packed_scene_to_tree_for_editing(&mut tree, r, &packed)
        .unwrap_or_else(|e| panic!("{}: {e}", scene.display()));
    (text, tree, root)
}

#[test]
fn every_real_scene_saves_back_byte_for_byte_when_unedited() {
    let _roots = roots();
    let mut n = 0;
    for (game, scenes) in real_scenes() {
        for scene in scenes {
            let (text, tree, root) = open(&game, &scene);
            let saved = TscnSaver::save_tree(&tree, root);
            assert!(
                saved == text,
                "{} changed on an unedited save",
                scene.display()
            );
            n += 1;
        }
    }
    assert!(n >= 40, "only {n} scenes");
}

#[test]
fn every_real_scene_saves_an_edit_as_exactly_that_edit() {
    let _roots = roots();
    for (game, scenes) in real_scenes() {
        for scene in scenes {
            let (text, mut tree, root) = open(&game, &scene);
            // The root, and the scene's last node in tree order that the file
            // itself has a `[node]` section for (a node of an instance, or of
            // an inherited scene, is not declared here).
            let mut ids = Vec::new();
            tree.collect_subtree_top_down(root, &mut ids);
            let declared = |id: gdscene::node::NodeId| {
                let node = tree.get_node(id).unwrap();
                let mut parts = Vec::new();
                let mut c = node.parent();
                while let Some(p) = c {
                    if p == root {
                        break;
                    }
                    let pn = tree.get_node(p).unwrap();
                    parts.push(pn.name().to_string());
                    c = pn.parent();
                }
                parts.reverse();
                let parent = if parts.is_empty() {
                    ".".to_string()
                } else {
                    parts.join("/")
                };
                let head = format!("[node name=\"{}\" ", node.name());
                let at = format!(" parent=\"{parent}\"");
                text.lines()
                    .any(|l| l.starts_with(&head) && l.contains(&at))
            };
            let last = ids
                .iter()
                .rev()
                .copied()
                .find(|&id| id != root && declared(id));
            let mut edited = vec![root];
            edited.extend(last);
            for (i, id) in edited.iter().enumerate() {
                tree.get_node_mut(*id)
                    .unwrap()
                    .set_property("editor_description", Variant::String(format!("edit {i}")));
            }
            let saved = TscnSaver::save_tree(&tree, root);
            let mut added = 0;
            let without: String = saved
                .split_inclusive('\n')
                .filter(|l| {
                    let edit = l.starts_with("editor_description = \"edit ");
                    added += edit as usize;
                    !edit
                })
                .collect();
            assert_eq!(
                added,
                edited.len(),
                "{}: the edits were not all written:\n{saved}",
                scene.display()
            );
            if without != text {
                let first = without
                    .lines()
                    .zip(text.lines())
                    .position(|(a, b)| a != b)
                    .unwrap_or(0);
                panic!(
                    "{}: an edit changed more than the edited lines; first difference at line {}:\n  saved:    {:?}\n  original: {:?}",
                    scene.display(),
                    first + 1,
                    without.lines().nth(first),
                    text.lines().nth(first)
                );
            }
        }
    }
}

/// The edited value lands in its own node's section, the way Godot writes it,
/// and the file loads back with it.
#[test]
fn an_edited_scene_reloads_with_the_edit_and_its_nested_sub_resources() {
    let _roots = roots();
    let game = games().join("platformer_3d");
    let (_, mut tree, root) = open(&game, &game.join("coin/coin.tscn"));
    tree.get_node_mut(root)
        .unwrap()
        .set_property("editor_description", Variant::String("moved".into()));
    let saved = TscnSaver::save_tree(&tree, root);
    // The nested pair the generator alone loses.
    assert!(
        saved.contains("[sub_resource type=\"Gradient\" id=\"Gradient_cd1ha\"]"),
        "{saved}"
    );
    assert!(
        saved.contains("gradient = SubResource(\"Gradient_cd1ha\")"),
        "{saved}"
    );
    let again = PackedScene::from_tscn(&saved).unwrap();
    let mut t2 = SceneTree::new();
    let r2 = t2.root_id();
    let root2 = add_packed_scene_to_tree_for_editing(&mut t2, r2, &again).unwrap();
    assert_eq!(
        t2.get_node(root2)
            .unwrap()
            .get_property("editor_description"),
        Variant::String("moved".into())
    );
}

/// pat-l0afa, on the generator alone: a Node3D is written with `transform`
/// only (its `position`/`rotation`/`scale`/`quaternion`/`basis` are editor
/// views Godot never stores), a Dictionary one entry per line, and no `__*`
/// engine cache as a property.
#[test]
fn the_generator_writes_node3d_transforms_and_dictionaries_as_godot_does() {
    let _roots = roots();
    let game = games().join("platformer_3d");
    for scene in [
        "coin/coin.tscn",
        "player/player.tscn",
        "enemy/enemy.tscn",
        "game.tscn",
    ] {
        let (_, tree, root) = open(&game, &game.join(scene));
        let text = TscnSaver::generate(&tree, root);
        let mut in_node = false;
        for line in text.lines() {
            if line.starts_with('[') {
                in_node = line.starts_with("[node ");
                continue;
            }
            let key = line.split(" = ").next().unwrap_or("");
            if in_node {
                assert!(
                    ![
                        "basis",
                        "quaternion",
                        "rotation",
                        "position",
                        "scale",
                        "rotation_degrees"
                    ]
                    .contains(&key),
                    "{scene}: derived Node3D row `{line}`"
                );
            }
            assert!(
                !key.starts_with("__"),
                "{scene}: engine cache written as a property: `{line}`"
            );
            assert!(
                !(line.contains(" = {\"") || line.contains(" = {&")),
                "{scene}: a one-line Dictionary: `{line}`"
            );
        }
    }
    // A transform the generator wrote is the node's transform.
    let (_, tree, root) = open(&game, &game.join("coin/coin.tscn"));
    let text = TscnSaver::generate(&tree, root);
    let again = PackedScene::from_tscn(&text).unwrap();
    let mut t2 = SceneTree::new();
    let r2 = t2.root_id();
    let root2 = gdscene::packed_scene::add_packed_scene_to_tree(&mut t2, r2, &again).unwrap();
    for name in ["Circle", "GlowSprite", "CollisionShape3D"] {
        let a = tree
            .get_node(tree.get_node_relative(root, name).unwrap())
            .unwrap()
            .get_property("transform");
        let b = t2
            .get_node(t2.get_node_relative(root2, name).unwrap())
            .unwrap()
            .get_property("transform");
        assert_eq!(a, b, "{name}'s transform survives the generator");
    }
}

/// An instance writes only what differs from its scene, and no node of a
/// non-editable instance is written at all (Godot's `SceneState::_parse_node`).
#[test]
fn the_generator_writes_an_instance_as_its_overrides_only() {
    let _roots = roots();
    let game = games().join("platformer");
    let (text, tree, root) = open(&game, &game.join("level/level.tscn"));
    let generated = TscnSaver::generate(&tree, root);
    let nodes = |t: &str| t.lines().filter(|l| l.starts_with("[node ")).count();
    assert_eq!(
        nodes(&generated),
        nodes(&text),
        "one [node] per node the file declares"
    );
    // Coin2 overrides `position` only; coin.tscn's own script is not repeated.
    let coin2 = generated
        .split("[node name=\"Coin2\" parent=\"Coins/CoinsArc3\"")
        .nth(1)
        .expect("Coin2")
        .split("\n\n")
        .next()
        .unwrap()
        .to_string();
    assert_eq!(
        coin2.lines().skip(1).collect::<Vec<_>>(),
        vec!["position = Vector2(30, -10)"],
        "{coin2}"
    );
    // Two same-named AnimationPlayers get distinct sub-resource ids.
    let mut ids: Vec<&str> = generated
        .lines()
        .filter(|l| l.starts_with("[sub_resource "))
        .filter_map(|l| l.split(" id=\"").nth(1).and_then(|r| r.split('"').next()))
        .collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total, "sub-resource ids collide");
}

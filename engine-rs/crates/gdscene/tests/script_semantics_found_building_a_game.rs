//! GDScript semantics an ordinary Godot game relies on, each found broken while
//! building `games/spark_keeper` through the editor's agent interface. Every
//! test drives the script through a real scene tree, the way the game did.

use gdcore::math::Vector2;
use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::{GDScriptNodeInstance, LifecycleManager, NodeId, SceneTree};
use gdvariant::Variant;

/// Instances `tscn`, attaches `scripts` (node path relative to the scene root,
/// source), and runs the lifecycle — the runner's order.
fn boot(tscn: &str, scripts: &[(&str, &str)]) -> (SceneTree, NodeId) {
    // The class hierarchy every `is`, signal and Node2D check consults, as the
    // editor and runner populate it at startup.
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    for (path, source) in scripts {
        let id = if *path == "." {
            scene_root
        } else {
            tree.get_node_or_null(scene_root, path)
                .expect("script target exists")
        };
        let script = GDScriptNodeInstance::from_source(source, id).expect("script compiles");
        tree.attach_script(id, Box::new(script));
    }
    LifecycleManager::enter_tree(&mut tree, scene_root);
    (tree, scene_root)
}

fn var(tree: &SceneTree, id: NodeId, name: &str) -> Variant {
    tree.get_script(id)
        .and_then(|s| s.get_property(name))
        .unwrap_or(Variant::Nil)
}

fn no_errors(tree: &mut SceneTree) {
    let out = tree.drain_script_output();
    assert!(out.errors.is_empty(), "script errors: {:?}", out.errors);
}

const ONE_NODE: &str = "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\n";

#[test]
fn a_member_initializer_reads_class_constants_and_earlier_members() {
    let src = "extends Node2D\nconst START := 3\nenum Mode { A, B }\nvar lives := START\nvar double := lives * 2\nvar mode := Mode.B\n";
    // Compiling used to fail outright: "undefined variable: 'START'".
    let (tree, root) = boot(ONE_NODE, &[(".", src)]);
    assert_eq!(var(&tree, root, "lives"), Variant::Int(3));
    assert_eq!(
        var(&tree, root, "double"),
        Variant::Int(6),
        "earlier members are in scope too"
    );
    assert_eq!(var(&tree, root, "mode"), Variant::Int(1));
}

#[test]
fn a_member_holding_a_node_assigns_that_nodes_property() {
    let tscn = "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\n\n[node name=\"L\" type=\"Label\" parent=\".\"]\ntext = \"start\"\n";
    let src = "extends Node2D\n@onready var plain = $L\n@onready var typed: Label = $L\nfunc _ready():\n    plain.text = \"plain\"\n    typed.text += \"+typed\"\n";
    let (mut tree, root) = boot(tscn, &[(".", src)]);
    no_errors(&mut tree);
    let label = tree.get_node_or_null(root, "L").unwrap();
    assert_eq!(
        tree.get_node(label).unwrap().get_property("text"),
        Variant::String("plain+typed".into()),
        "`member.text = x` writes the node, as a local does"
    );
}

#[test]
fn a_2d_script_reads_and_writes_global_position() {
    let tscn = "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\nposition = Vector2(100, 0)\n\n[node name=\"Child\" type=\"Node2D\" parent=\".\"]\nposition = Vector2(5, 5)\n";
    let src = "extends Node2D\nvar seen := Vector2.ZERO\nfunc _ready():\n    seen = global_position\n    global_position = Vector2(0, 0)\n";
    let (mut tree, root) = boot(tscn, &[("Child", src)]);
    no_errors(&mut tree);
    let child = tree.get_node_or_null(root, "Child").unwrap();
    assert_eq!(
        var(&tree, child, "seen"),
        Variant::Vector2(Vector2::new(105.0, 5.0))
    );
    assert_eq!(
        tree.get_node(child).unwrap().get_property("position"),
        Variant::Vector2(Vector2::new(-100.0, 0.0)),
        "a global write lands as the local position that produces it"
    );
}

#[test]
fn a_built_in_signal_reads_as_a_value_and_connects() {
    let tscn = "[gd_scene format=3]\n\n[node name=\"Area\" type=\"Area2D\"]\n";
    let src = "extends Area2D\nvar code := -1\nvar hits := 0\nfunc _ready():\n    code = body_entered.connect(_on_body)\nfunc _on_body(_b):\n    hits += 1\n";
    let (mut tree, root) = boot(tscn, &[(".", src)]);
    no_errors(&mut tree);
    assert_eq!(
        var(&tree, root, "code"),
        Variant::Int(0),
        "connect() returned OK"
    );
    tree.emit_signal(root, "body_entered", &[Variant::Nil]);
    assert_eq!(var(&tree, root, "hits"), Variant::Int(1));
}

#[test]
fn a_constant_reads_through_an_instance() {
    let tscn = "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\n\n[node name=\"Holder\" type=\"Node\" parent=\".\"]\n";
    let holder = "extends Node\nconst LIMIT := 2.5\nconst SIZE := Vector2(640, 360)\n";
    let reader = "extends Node2D\nvar limit = null\nvar size = null\nfunc _ready():\n    limit = $Holder.LIMIT\n    size = $Holder.SIZE\n";
    let (mut tree, root) = boot(tscn, &[("Holder", holder), (".", reader)]);
    no_errors(&mut tree);
    assert_eq!(var(&tree, root, "limit"), Variant::Float(2.5));
    assert_eq!(
        var(&tree, root, "size"),
        Variant::Vector2(Vector2::new(640.0, 360.0))
    );
}

#[test]
fn is_and_as_test_a_nodes_class_and_ancestors() {
    let tscn = "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\n\n[node name=\"P\" type=\"CharacterBody2D\" parent=\".\"]\n";
    let src = "extends Node2D\nvar results := []\nfunc _ready():\n    results = [$P is Node2D, $P is CharacterBody2D, $P is Control, ($P as Node2D) != null, ($P as Control) == null]\n";
    let (mut tree, root) = boot(tscn, &[(".", src)]);
    no_errors(&mut tree);
    assert_eq!(
        var(&tree, root, "results"),
        Variant::Array(
            vec![
                Variant::Bool(true),
                Variant::Bool(true),
                Variant::Bool(false),
                Variant::Bool(true),
                Variant::Bool(true),
            ]
            .into()
        ),
        "[is Node2D, is CharacterBody2D, is Control, as Node2D != null, as Control == null]"
    );
}

#[test]
fn change_scene_expands_instanced_sub_scenes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("sub.tscn"),
        "[gd_scene format=3]\n\n[node name=\"Sub\" type=\"Node2D\"]\n\n[node name=\"Inner\" type=\"Node2D\" parent=\".\"]\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("host.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n[ext_resource type=\"PackedScene\" path=\"res://sub.tscn\" id=\"1_sub\"]\n\n[node name=\"Host\" type=\"Node2D\"]\n\n[node name=\"S\" parent=\".\" instance=ExtResource(\"1_sub\")]\nposition = Vector2(100, 0)\n",
    )
    .unwrap();
    gdcore::set_path_roots(Some(dir.path().to_path_buf()), None);
    gdobject::class_db::register_all_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let old = tree.add_child(root, Node::new("Old", "Node")).unwrap();
    tree.set_current_scene(old, None);
    tree.change_scene_to_file("res://host.tscn")
        .expect("queued");
    let host = tree
        .process_deferred_scene_change()
        .unwrap()
        .expect("a new scene");

    let s = tree.get_node_or_null(host, "S").expect("instance node");
    assert_eq!(
        tree.get_node(s).unwrap().class_name(),
        "Node2D",
        "the sub-scene's root class, not a placeholder Node"
    );
    assert!(
        tree.get_node_or_null(s, "Inner").is_some(),
        "the sub-scene's children are instanced"
    );
    assert_eq!(
        tree.get_node(s).unwrap().get_property("position"),
        Variant::Vector2(Vector2::new(100.0, 0.0)),
        "the parent scene's override is kept"
    );
    assert!(tree.get_node(old).is_none(), "the previous scene was freed");
}

#[test]
fn a_centred_vbox_stacks_its_labels_inside_its_own_rect() {
    let tscn = "[gd_scene format=3]\n\n[node name=\"UI\" type=\"Control\"]\nanchor_right = 1.0\nanchor_bottom = 1.0\n\n[node name=\"Menu\" type=\"VBoxContainer\" parent=\".\"]\nanchor_left = 0.5\nanchor_top = 0.5\nanchor_right = 0.5\nanchor_bottom = 0.5\noffset_left = -150.0\noffset_top = -50.0\noffset_right = 150.0\noffset_bottom = 50.0\n\n[node name=\"A\" type=\"Label\" parent=\"Menu\"]\ntext = \"first\"\n\n[node name=\"B\" type=\"Label\" parent=\"Menu\"]\ntext = \"second\"\n";
    let (mut tree, ui) = boot(tscn, &[]);
    let root = tree.root_id();
    let _ = gdscene::canvas_render::build_control_render_items(
        &mut tree,
        root,
        Vector2::new(640.0, 360.0),
    );
    let menu = tree.get_node_or_null(ui, "Menu").unwrap();
    let a = tree.get_node_or_null(menu, "A").unwrap();
    let b = tree.get_node_or_null(menu, "B").unwrap();
    let size = |id| gdscene::control::get_size(&tree, id);
    let pos = |id| gdscene::control::get_position(&tree, id);
    assert_eq!(
        size(menu),
        Vector2::new(300.0, 100.0),
        "the container keeps its own anchored rect, not the viewport's"
    );
    assert!(
        size(a).y > 0.0,
        "a Label's minimum height is its line height: {:?}",
        size(a)
    );
    assert_eq!(
        size(a).x,
        300.0,
        "a FILL child spans the container, not the viewport"
    );
    assert!(
        pos(b).y >= pos(a).y + size(a).y,
        "the second label sits below the first: {:?} {:?}",
        pos(a),
        pos(b)
    );
}

#[test]
fn a_2d_script_writes_global_scale_and_global_transform() {
    let tscn = "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\nposition = Vector2(100, 0)\nscale = Vector2(2, 2)\n\n[node name=\"Child\" type=\"Node2D\" parent=\".\"]\n";
    let src = "extends Node2D\nvar after_scale := Vector2.ZERO\nfunc _ready():\n    global_scale = Vector2(4, 4)\n    after_scale = scale\n    global_transform = Transform2D(0.0, Vector2(100, 100))\n";
    let (mut tree, root) = boot(tscn, &[("Child", src)]);
    no_errors(&mut tree);
    let child = tree.get_node_or_null(root, "Child").unwrap();
    assert_eq!(
        var(&tree, child, "after_scale"),
        Variant::Vector2(Vector2::new(2.0, 2.0)),
        "global 4 under a parent scaled 2 is local 2"
    );
    let node = tree.get_node(child).unwrap();
    assert_eq!(
        node.get_property("position"),
        Variant::Vector2(Vector2::new(0.0, 50.0)),
        "global (100,100) under a parent at (100,0) scaled 2 is local (0,50)"
    );
    assert_eq!(
        gdscene::node2d::get_global_position(&tree, child),
        Vector2::new(100.0, 100.0)
    );
}

#[test]
fn a_constant_that_cannot_be_evaluated_does_not_stop_the_script_loading() {
    // Constants are seeded into initialisers best-effort: one that fails must
    // leave the script as loadable as it was before constants were seeded.
    let src = "extends Node2D\nconst BROKEN = no_such_name\nconst OK := 2\nvar lives := OK + 1\n";
    let (tree, root) = boot(ONE_NODE, &[(".", src)]);
    assert_eq!(var(&tree, root, "lives"), Variant::Int(3));
}

fn write(dir: &std::path::Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap();
}

fn instance_line(i: usize) -> String {
    format!("[node name=\"I{i}\" parent=\".\" instance=ExtResource(\"1_s\")]\n")
}

#[test]
fn a_self_instancing_scene_is_refused_without_recursing() {
    let dir = tempfile::tempdir().unwrap();
    // a.tscn -> b.tscn -> a.tscn
    write(dir.path(), "a.tscn", "[gd_scene load_steps=2 format=3]\n\n[ext_resource type=\"PackedScene\" path=\"res://b.tscn\" id=\"1_s\"]\n\n[node name=\"A\" type=\"Node2D\"]\n\n[node name=\"B\" parent=\".\" instance=ExtResource(\"1_s\")]\n");
    write(dir.path(), "b.tscn", "[gd_scene load_steps=2 format=3]\n\n[ext_resource type=\"PackedScene\" path=\"res://a.tscn\" id=\"1_s\"]\n\n[node name=\"Bee\" type=\"Node2D\"]\n\n[node name=\"A\" parent=\".\" instance=ExtResource(\"1_s\")]\n");
    gdcore::set_path_roots(Some(dir.path().to_path_buf()), None);
    gdobject::class_db::register_all_classes();
    gdscene::packed_scene::begin_subscene_session();
    assert!(
        gdscene::packed_scene::load_scene_from_res("res://a.tscn").is_none(),
        "a cycle is refused, not recursed into"
    );

    let mut tree = SceneTree::new();
    tree.change_scene_to_file("res://a.tscn")
        .expect("the top scene itself still loads");
    // Its cyclic instance is left as a placeholder instead of blowing the stack.
    assert!(tree.process_deferred_scene_change().unwrap().is_some());
}

#[test]
fn a_level_with_thousands_of_instances_keeps_every_one() {
    // The old guard counted total loads and silently dropped instance 4097+.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "bullet.tscn", "[gd_scene format=3]\n\n[node name=\"Bullet\" type=\"Node2D\"]\n\n[node name=\"Tip\" type=\"Node2D\" parent=\".\"]\n");
    let mut level = String::from("[gd_scene load_steps=2 format=3]\n\n[ext_resource type=\"PackedScene\" path=\"res://bullet.tscn\" id=\"1_s\"]\n\n[node name=\"Level\" type=\"Node2D\"]\n\n");
    for i in 0..4200 {
        level.push_str(&instance_line(i));
    }
    write(dir.path(), "level.tscn", &level);
    gdcore::set_path_roots(Some(dir.path().to_path_buf()), None);
    gdobject::class_db::register_all_classes();

    let mut tree = SceneTree::new();
    tree.change_scene_to_file("res://level.tscn").unwrap();
    let level = tree.process_deferred_scene_change().unwrap().unwrap();
    let last = tree
        .get_node_or_null(level, "I4199")
        .expect("the last instance");
    assert!(
        tree.get_node_or_null(last, "Tip").is_some(),
        "instance 4200 expanded too"
    );
}

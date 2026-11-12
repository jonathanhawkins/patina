//! GDScript error parity with REAL Godot 4.6.1 (pat-w0q0w).
//!
//! Captured from `Godot --headless` on the scene-audit probe (each line is
//! Godot's own `SCRIPT ERROR:` text):
//! - `var t = self; print(t.no_such_prop)` ->
//!   "Invalid access to property or key 'no_such_prop' on a base object of
//!   type 'Node2D (main.gd)'." Patina returned Nil silently.
//! - `call("no_such_method")` -> "Invalid call. Nonexistent function
//!   'no_such_method (via call)' in base 'Node2D (main.gd)'." Patina said
//!   "method not found: 'call'".
//!
//! And the other direction, which matters more: a property that EXISTS but
//! holds Nil must keep reading Nil. Godot prints a null for every read in
//! [`NIL_BUT_REAL`] and raises nothing.

use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::PackedScene;
use gdscene::SceneTree;

static ROOTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn roots_lock() -> std::sync::MutexGuard<'static, ()> {
    ROOTS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Writes a one-scene project whose root runs `script`, boots it the way
/// patina-runner does, runs one frame and returns `(prints, errors)`.
fn run(script: &str) -> (Vec<String>, Vec<String>) {
    let _roots = roots_lock();
    let dir = tempfile::tempdir().unwrap();
    let w = |name: &str, text: &str| std::fs::write(dir.path().join(name), text).unwrap();
    w(
        "project.godot",
        "config_version=5\n\n[application]\nrun/main_scene=\"res://main.tscn\"\n",
    );
    w(
        "main.tscn",
        "[gd_scene format=3]\n\n[ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1\"]\n\n\
         [node name=\"Main\" type=\"Node2D\"]\nscript = ExtResource(\"1\")\n\n\
         [node name=\"Child\" type=\"Sprite2D\" parent=\".\"]\n",
    );
    w("main.gd", script);
    gdobject::class_db::register_all_classes();
    let root_dir = dir.path().to_path_buf();
    gdcore::file_access::set_path_roots(Some(root_dir.clone()), None);
    let text = std::fs::read_to_string(root_dir.join("project.godot")).unwrap();
    let settings = gdcore::project_settings::ProjectSettings::from_godot_format(&text).unwrap();
    gdscene::project_runtime::seed_script_settings_for_this_thread(&settings, &root_dir);
    let tscn = std::fs::read_to_string(root_dir.join("main.tscn")).unwrap();
    let packed = PackedScene::from_tscn(&tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::begin_subscene_session();
    let id = gdscene::packed_scene::instance_packed_scene_detached(
        &mut tree,
        &packed,
        &gdscene::packed_scene::load_scene_from_res,
    )
    .unwrap();
    gdscene::scripting::attach_subtree_scripts_from_res(&mut tree, id);
    let node = tree.take_node(id).unwrap();
    tree.add_child(root, node).unwrap();
    gdscene::packed_scene::finish_attached_scene(&mut tree, id);
    let mut ml = MainLoop::new(tree);
    gdscene::project_runtime::apply_project_settings(&mut ml, &settings);
    ml.step(1.0 / 60.0);
    let out = ml.tree_mut().drain_script_output();
    (out.prints, out.errors)
}

#[test]
fn reading_a_nonexistent_property_raises_godots_error() {
    let (prints, errors) = run(
        "extends Node2D\n\nfunc _ready() -> void:\n\tvar t = self\n\tprint(t.no_such_prop)\n\tprint(\"after\")\n",
    );
    assert_eq!(errors.len(), 1, "exactly one error, got {errors:?}");
    assert!(
        errors[0].contains(
            "Invalid access to property or key 'no_such_prop' on a base object of type 'Node2D (main.gd)'."
        ),
        "Godot's text, got {errors:?}"
    );
    // The error aborts the function, as in Godot.
    assert!(!prints.iter().any(|p| p == "after"), "{prints:?}");
}

#[test]
fn reading_a_nonexistent_property_of_another_node_names_its_class() {
    let (_, errors) = run(
        "extends Node2D\n\nfunc _ready() -> void:\n\tvar c = $Child\n\tprint(c.no_such_prop)\n",
    );
    assert!(
        errors.iter().any(|e| e.contains(
            "Invalid access to property or key 'no_such_prop' on a base object of type 'Sprite2D'."
        )),
        "a scriptless node is named by its class alone, got {errors:?}"
    );
}

/// Reads Godot answers with a null and no error: properties that exist and
/// hold Nil, on the script (`member`), the node (`owner` of a scene root,
/// `material`) and a child (`texture`, inherited `material`). Godot 4.6.1 prints
/// `<null>` then `<Object#null>` four times, then `done`, for this script.
const NIL_BUT_REAL: &str = "extends Node2D\n\nvar member: Node\n\nfunc _ready() -> void:\n\
    \tvar t = self\n\tprint(t.member)\n\tprint(t.owner)\n\tprint(t.material)\n\
    \tvar c = $Child\n\tprint(c.texture)\n\tprint(c.material)\n\tprint(\"done\")\n";

#[test]
fn existing_nil_properties_still_read_nil() {
    let (prints, errors) = run(NIL_BUT_REAL);
    assert!(
        errors.is_empty(),
        "every name here exists in Godot, got {errors:?}"
    );
    assert_eq!(
        prints.last().map(String::as_str),
        Some("done"),
        "{prints:?}"
    );
}

#[test]
fn call_of_a_nonexistent_method_names_the_target_method() {
    let (prints, errors) = run(
        "extends Node2D\n\nfunc ok(x):\n\treturn x * 2\n\nfunc _ready() -> void:\n\
         \tprint(call(\"ok\", 3))\n\tcall(\"no_such_method\")\n",
    );
    assert_eq!(
        prints,
        vec!["6".to_string()],
        "call() on an existing method still works"
    );
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains(
            "Invalid call. Nonexistent function 'no_such_method (via call)' in base 'Node2D (main.gd)'."
        ),
        "Godot's text, got {errors:?}"
    );
}

#[test]
fn call_on_another_node_names_the_target_method() {
    let (_, errors) = run("extends Node2D\n\nfunc _ready() -> void:\n\t$Child.call(\"nope\")\n");
    assert!(
        errors.iter().any(|e| e
            .contains("Invalid call. Nonexistent function 'nope (via call)' in base 'Sprite2D'.")),
        "got {errors:?}"
    );
}

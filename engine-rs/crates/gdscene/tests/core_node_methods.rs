//! pat-mgixk.8 acceptance — the core `Node` / `CanvasItem` / `Node2D` /
//! `Viewport` / `SceneTree` method surface reaches GDScript.
//!
//! The audit measured the gap as method spellings with no quoted literal
//! anywhere in `gdscene/src` or `gdscript-interop/src`: Node 99/121,
//! CanvasItem 71/91, Node2D 22/33, SceneTree 30/47, and no accessor fallback at
//! all. Every operation below already had a tested implementation on
//! `SceneTree` — `move_child`, `reparent`, `get_index`, `node_groups`,
//! `get_path_to`, `viewport::get_viewport` — and **none of it was reachable
//! from a script**: the call raised `UndefinedFunction`, which aborts the whole
//! handler, so every later statement in `_process` silently did not run either.
//!
//! NOTE ON THE FILE NAME: the acceptance calls its test
//! `gdscript_core_node_method_surface`, which is kept on the test FUNCTION. The
//! file is shorter so the `-p gdscene --test <target>` command that runs it is
//! 53 characters — `anim_audio_track.rs` records a verify lane that truncated a
//! longer `--test` target and then ran a binary that does not exist.
//!
//! Everything is driven from a `.tscn` through `MainLoop::step` with a real
//! GDScript attached, and every observation is either a scene-tree fact Rust
//! re-reads afterwards (child order, parent links, the current scene) or a
//! value the script itself captured through the method under test. Script vars
//! start at `null` on purpose: a clause that never ran reads back `Nil` and
//! fails loudly, instead of matching a plausible pre-seeded default.

use gdcore::math::{Color, Vector2};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Serializes the tests that write the process-wide `res://` root.
///
/// `gdcore::file_access::set_path_roots` is a process global and this binary's
/// tests run as threads under the default harness, so a test that points
/// `res://` at its own tempdir would otherwise be able to redirect a parallel
/// one. Taken ONCE per test that needs it, and poison-tolerant so one failure
/// does not cascade into "panicked while the mutex was poisoned" for the rest.
static RES_ROOT: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A `Node2D` scene with the four nodes the acceptance drives:
/// `Sprite` (with a `Dot` child, for inherited visibility), `A` and `B`.
///
/// Child order is `Sprite, A, B`, so `B.get_index()` is 2 before the move and 0
/// after it — two different numbers, which is what makes the reorder visible
/// rather than merely plausible.
const MAIN_SCENE: &str = r#"[gd_scene load_steps=1 format=3]

[node name="World" type="Node2D"]

[node name="Sprite" type="Sprite2D" parent="."]
position = Vector2(2, 3)

[node name="Dot" type="Sprite2D" parent="Sprite"]

[node name="A" type="Node2D" parent="."]

[node name="B" type="Node2D" parent="."]
"#;

/// The scene `change_scene_to_packed` swaps in. Its root name is what proves
/// the swap landed.
const SECOND_SCENE: &str = r#"[gd_scene load_steps=1 format=3]

[node name="SecondScene" type="Node2D"]
"#;

/// Loads `tscn` as the tree's CURRENT scene and attaches `script_src` to its
/// root.
///
/// ⚠️ `change_scene_to_packed` rather than `add_packed_scene_to_tree`: only the
/// former sets `current_scene_id`, and `get_tree().get_current_scene()` would
/// otherwise be `null` for a scene that is plainly running.
fn boot(tscn: &str, script_src: &str) -> (MainLoop, gdscene::node::NodeId) {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.change_scene_to_packed(&scene).expect("instance scene");
    let script = GDScriptNodeInstance::from_source(script_src, root).unwrap();
    tree.attach_script(root, Box::new(script));
    (MainLoop::new(tree), root)
}

fn script_var(ml: &MainLoop, node: gdscene::node::NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
}

fn want_bool(ml: &MainLoop, node: gdscene::node::NodeId, name: &str) -> bool {
    match script_var(ml, node, name) {
        Variant::Bool(b) => b,
        other => {
            panic!("`{name}` should be a bool, got {other:?} (Nil means the clause never ran)")
        }
    }
}

fn want_int(ml: &MainLoop, node: gdscene::node::NodeId, name: &str) -> i64 {
    match script_var(ml, node, name) {
        Variant::Int(i) => i,
        Variant::Float(f) => f as i64,
        other => {
            panic!("`{name}` should be an int, got {other:?} (Nil means the clause never ran)")
        }
    }
}

fn want_vec2(ml: &MainLoop, node: gdscene::node::NodeId, name: &str) -> Vector2 {
    match script_var(ml, node, name) {
        Variant::Vector2(v) => v,
        other => {
            panic!("`{name}` should be a Vector2, got {other:?} (Nil means the clause never ran)")
        }
    }
}

/// The node a script var holds, as the tree's own id.
fn want_node(ml: &MainLoop, node: gdscene::node::NodeId, name: &str) -> gdscene::node::NodeId {
    match script_var(ml, node, name) {
        Variant::ObjectId(id) => gdscene::node::NodeId::from_object_id(id),
        other => {
            panic!(
                "`{name}` should be a node handle, got {other:?} (Nil means it resolved to null)"
            )
        }
    }
}

/// Names of a node's children, in order — the scene fact `move_child` and
/// `reparent` are supposed to change.
fn child_names(ml: &MainLoop, node: gdscene::node::NodeId) -> Vec<String> {
    ml.tree()
        .get_node(node)
        .expect("node")
        .children()
        .iter()
        .map(|c| {
            ml.tree()
                .get_node(*c)
                .map(|n| n.name().to_owned())
                .unwrap_or_default()
        })
        .collect()
}

/// **The acceptance test** — all six clauses of pat-mgixk.8, from one script.
///
/// (a) `hide()` turns `visible` off and takes the node AND its child out of
/// `is_visible_in_tree()`. (b) `set_global_position` / `get_global_position`
/// round-trip. (c) `move_child` reorders the parent's children. (d) `reparent`
/// moves a node. (e) `get_tree().change_scene_to_packed(load(...))` swaps the
/// current scene at the frame boundary. (f) `get_viewport().get_visible_rect()`
/// reports the window size.
#[test]
fn gdscript_core_node_method_surface() {
    let _roots = RES_ROOT.lock().unwrap_or_else(|p| p.into_inner());
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("b.tscn"), SECOND_SCENE).unwrap();
    gdcore::file_access::set_path_roots(Some(dir.path().to_path_buf()), None);

    // `+=` / `elif` / `$Node` avoided for parser-independence, matching the
    // sibling scripted tests. Every observation var starts `null`.
    let src = "\
extends Node2D
var frame = 0
var vis_before = null
var vis_after = null
var in_tree_after = null
var dot_in_tree = null
var gp = null
var idx_before = null
var idx_after = null
var parent_before = null
var parent_after = null
var vrect = null
var has_sprite = null
var has_ghost = null
var scene_root = null
func _process(delta):
    self.frame = self.frame + 1
    if self.frame == 1:
        var s = get_node(\"Sprite\")
        self.vis_before = s.is_visible_in_tree()
        s.hide()
        self.vis_after = s.visible
        self.in_tree_after = s.is_visible_in_tree()
        self.dot_in_tree = get_node(\"Sprite/Dot\").is_visible_in_tree()
        s.set_global_position(Vector2(10, 20))
        self.gp = s.get_global_position()
        var b = get_node(\"B\")
        self.idx_before = b.get_index()
        move_child(b, 0)
        self.idx_after = b.get_index()
        var a = get_node(\"A\")
        self.parent_before = a.get_parent()
        a.reparent(b)
        self.parent_after = a.get_parent()
        self.vrect = get_viewport().get_visible_rect().size
        self.has_sprite = has_node(\"Sprite\")
        self.has_ghost = has_node(\"NotThere\")
        self.scene_root = get_tree().get_current_scene()
    if self.frame == 2:
        get_tree().change_scene_to_packed(load(\"res://b.tscn\"))
";
    let (mut ml, world) = boot(MAIN_SCENE, src);
    // A NON-default window size, so a `get_visible_rect()` that returns a
    // constant cannot pass by coincidence. Through `set_viewport_size`, which
    // is the one entry point a host uses — it also recomputes the content scale
    // and republishes the mirrored window properties, so the size the script
    // reads back came the same way a real resize would deliver it.
    ml.set_viewport_size(800, 600);

    let sprite = ml
        .tree()
        .get_node_by_path("/root/World/Sprite")
        .expect("Sprite");
    let b_id = ml.tree().get_node_by_path("/root/World/B").expect("B");
    let a_id = ml.tree().get_node_by_path("/root/World/A").expect("A");

    ml.step(1.0 / 60.0);

    // -- (a) hide() ---------------------------------------------------------
    assert!(
        want_bool(&ml, world, "vis_before"),
        "(a) precondition: the sprite starts visible in tree"
    );
    assert!(
        !want_bool(&ml, world, "vis_after"),
        "(a) hide() must turn the node's own `visible` off"
    );
    assert!(
        !want_bool(&ml, world, "in_tree_after"),
        "(a) a hidden node must report is_visible_in_tree() == false"
    );
    assert!(
        !want_bool(&ml, world, "dot_in_tree"),
        "(a) hiding a parent must take its CHILD out of is_visible_in_tree() \
         too — that inherited-hidden walk is what a draw path consults to \
         decide the subtree is not in the frame"
    );
    assert_eq!(
        ml.tree().get_node(sprite).unwrap().get_property("visible"),
        Variant::Bool(false),
        "(a) the scene itself must record the node as hidden"
    );

    // -- (b) global position round-trip -------------------------------------
    assert_eq!(
        want_vec2(&ml, world, "gp"),
        Vector2::new(10.0, 20.0),
        "(b) set_global_position() then get_global_position() must round-trip; \
         the sprite was authored at (2, 3), so a getter echoing the scene \
         would report that instead"
    );

    // -- (c) move_child reorders --------------------------------------------
    assert_eq!(
        want_int(&ml, world, "idx_before"),
        2,
        "(c) precondition: B is the third child as authored"
    );
    assert_eq!(
        want_int(&ml, world, "idx_after"),
        0,
        "(c) move_child(B, 0) must move B to the front, and get_index() must \
         report the new slot"
    );
    // Asserted on the TREE as well, not only on the index the script read back:
    // a `get_index` that answered 0 without the move having happened would
    // satisfy the line above on its own.
    assert_eq!(
        child_names(&ml, world).first().map(String::as_str),
        Some("B"),
        "(c) the parent's child ORDER must change, not just the reported index"
    );

    // -- (d) reparent -------------------------------------------------------
    assert_eq!(
        want_node(&ml, world, "parent_before"),
        world,
        "(d) precondition: A starts under the scene root"
    );
    assert_eq!(
        want_node(&ml, world, "parent_after"),
        b_id,
        "(d) reparent(B) must move A under B"
    );
    assert!(
        child_names(&ml, b_id).iter().any(|n| n == "A"),
        "(d) B must now hold A as a child; got {:?}",
        child_names(&ml, b_id)
    );
    assert!(
        !child_names(&ml, world).iter().any(|n| n == "A"),
        "(d) A must no longer be a child of the scene root"
    );
    assert_eq!(
        ml.tree().get_node(a_id).and_then(|n| n.parent()),
        Some(b_id),
        "(d) the tree's own parent link must follow"
    );

    // -- (f) viewport rect --------------------------------------------------
    assert_eq!(
        want_vec2(&ml, world, "vrect"),
        Vector2::new(800.0, 600.0),
        "(f) get_viewport().get_visible_rect().size must report the window \
         size this test set, not a default"
    );

    // has_node, in passing: both answers, so a method that always returned the
    // same bool cannot pass.
    assert!(want_bool(&ml, world, "has_sprite"), "has_node(\"Sprite\")");
    assert!(
        !want_bool(&ml, world, "has_ghost"),
        "has_node() must answer false for a path that is not there"
    );

    // -- (e) change_scene_to_packed -----------------------------------------
    assert_eq!(
        want_node(&ml, world, "scene_root"),
        world,
        "(e) precondition: get_current_scene() names the running scene's root, \
         NOT /root — those are different nodes and only one of them changes"
    );

    // Frame 2 calls the swap. Godot applies it at the frame boundary, through
    // the same deferred channel `change_scene_to_file` uses, so the calling
    // script is not freed mid-run.
    ml.step(1.0 / 60.0);

    let new_scene = ml
        .tree()
        .current_scene()
        .expect("(e) a scene must be current after the swap");
    assert_ne!(
        new_scene, world,
        "(e) change_scene_to_packed must replace the current scene"
    );
    assert_eq!(
        ml.tree().get_node(new_scene).map(|n| n.name().to_owned()),
        Some("SecondScene".to_owned()),
        "(e) the new current scene must be the one b.tscn declares"
    );
    assert!(
        ml.tree().get_node_by_path("/root/World").is_none(),
        "(e) the old scene must be gone from the tree"
    );
}

/// The generic ClassDB accessor fallback, which is what makes the bound getters
/// and setters work without a per-method arm.
///
/// 🔑 `is_flipped_h` / `set_flip_h` is the case that proves the fallback reads
/// the REGISTERED accessor name rather than deriving one: a `get_`/`is_`-prefix
/// rule would look for a property called `flipped_h`, which does not exist.
/// `PropertyInfo::getter`'s own docs measure that convention at 51.7% of
/// Godot's documented members, which is why deriving is forbidden.
#[test]
fn registered_property_accessors_dispatch_without_per_method_arms() {
    let src = "\
extends Node2D
var frame = 0
var flipped_before = null
var flipped_after = null
var z_before = null
var z_after = null
var modulate_after = null
func _process(delta):
    self.frame = self.frame + 1
    if self.frame == 1:
        var s = get_node(\"Sprite\")
        self.flipped_before = s.is_flipped_h()
        s.set_flip_h(true)
        self.flipped_after = s.is_flipped_h()
        self.z_before = s.get_z_index()
        s.set_z_index(5)
        self.z_after = s.get_z_index()
        s.set_modulate(Color(1, 0, 0, 1))
        self.modulate_after = s.get_modulate()
";
    let (mut ml, world) = boot(MAIN_SCENE, src);
    ml.step(1.0 / 60.0);

    // The REGISTERED default, not Nil: the scene never wrote `flip_h`, and
    // Godot's getter always answers a typed value. A Nil here would also break
    // `node.flip_h = not node.flip_h`, because the interpreter reads a Nil as
    // "no such property".
    assert!(
        !want_bool(&ml, world, "flipped_before"),
        "an unset registered bool property must read back its registered \
         default (false), not null"
    );
    assert!(
        want_bool(&ml, world, "flipped_after"),
        "set_flip_h(true) must reach the `flip_h` property its registration \
         names, and is_flipped_h() must read it back"
    );
    assert_eq!(
        want_int(&ml, world, "z_before"),
        0,
        "an unset registered int property must read back its registered default"
    );
    assert_eq!(
        want_int(&ml, world, "z_after"),
        5,
        "set_z_index(5) must reach the `z_index` property"
    );
    assert_eq!(
        script_var(&ml, world, "modulate_after"),
        Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)),
        "set_modulate()/get_modulate() must round-trip a Color through the \
         `modulate` property"
    );
    // ...and the property the accessor names is the one the scene now holds, so
    // the round trip is not two script calls agreeing with each other.
    let sprite = ml
        .tree()
        .get_node_by_path("/root/World/Sprite")
        .expect("Sprite");
    assert_eq!(
        ml.tree().get_node(sprite).unwrap().get_property("z_index"),
        Variant::Int(5),
        "the setter must write the registered PROPERTY, not a key named after \
         the method"
    );
}

/// `get_groups` / `find_children` / `get_path_to` / `get_node_count_in_group` —
/// more of the surface the bead names, each of which had a `SceneTree`
/// implementation and no binding.
#[test]
fn node_query_surface_reaches_a_script() {
    let src = "\
extends Node2D
var frame = 0
var groups = null
var found = null
var path_to_dot = null
var marked = null
func _process(delta):
    self.frame = self.frame + 1
    if self.frame == 1:
        add_to_group(\"targets\")
        self.groups = get_groups()
        self.found = find_children(\"*prite\", \"Sprite2D\", true)
        self.path_to_dot = get_path_to(get_node(\"Sprite/Dot\"))
        self.marked = get_tree().get_node_count_in_group(\"targets\")
";
    let (mut ml, world) = boot(MAIN_SCENE, src);
    ml.step(1.0 / 60.0);

    match script_var(&ml, world, "groups") {
        Variant::Array(items) => {
            let names: Vec<String> = items
                .to_vec()
                .iter()
                .map(|v| match v {
                    Variant::StringName(s) => s.as_str().to_owned(),
                    Variant::String(s) => s.clone(),
                    other => panic!("group entries should be names, got {other:?}"),
                })
                .collect();
            assert!(
                names.iter().any(|n| n == "targets"),
                "get_groups() must list the group the script just joined; got {names:?}"
            );
        }
        other => panic!("`groups` should be an Array, got {other:?}"),
    }

    match script_var(&ml, world, "found") {
        Variant::Array(items) => {
            // `*prite` matches `Sprite` but not `Dot`, and the `Sprite2D` type
            // filter keeps `A` and `B` out — so a binding that dropped either
            // argument would return a different count.
            let found = items.to_vec().len();
            assert_eq!(
                found, 1,
                "find_children(\"*prite\", \"Sprite2D\") must match exactly the \
                 Sprite; got {found} entries"
            );
        }
        other => panic!("`found` should be an Array, got {other:?}"),
    }

    assert_eq!(
        script_var(&ml, world, "path_to_dot"),
        Variant::NodePath(gdcore::node_path::NodePath::new("Sprite/Dot")),
        "get_path_to() must answer a NodePath relative to the caller, the type \
         Godot declares — not a String"
    );
    assert_eq!(
        want_int(&ml, world, "marked"),
        1,
        "get_tree().get_node_count_in_group() must count the group's members"
    );
}

/// `move_child`'s negative index counts from the end, as Godot documents.
#[test]
fn move_child_accepts_a_negative_index() {
    let src = "\
extends Node2D
var frame = 0
var idx = null
func _process(delta):
    self.frame = self.frame + 1
    if self.frame == 1:
        var s = get_node(\"Sprite\")
        move_child(s, -1)
        self.idx = s.get_index()
";
    let (mut ml, world) = boot(MAIN_SCENE, src);
    ml.step(1.0 / 60.0);

    // Three children (Sprite, A, B); -1 is the last slot, index 2.
    assert_eq!(
        want_int(&ml, world, "idx"),
        2,
        "move_child(child, -1) must move the child to the LAST slot — Godot \
         counts a negative index from the end"
    );
    assert_eq!(
        child_names(&ml, world).last().map(String::as_str),
        Some("Sprite"),
        "the parent's child order must follow; got {:?}",
        child_names(&ml, world)
    );
}

/// A scene instanced WITHOUT `change_scene_to_packed` has no current scene, and
/// `get_current_scene()` must say so rather than answering `/root`.
///
/// ⚠️ This is the inverse of the acceptance's (e) precondition, and it is the
/// one that catches the tempting shortcut: wiring `get_current_scene` to
/// `get_root()` would pass (e) — `/root` is an object handle and the swap still
/// happens — while being wrong before any scene is current and wrong again
/// after every swap.
#[test]
fn get_current_scene_is_not_the_root_window() {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(MAIN_SCENE).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let world = add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    let src = "\
extends Node2D
var frame = 0
var current = null
var root_is_viewport = null
func _process(delta):
    self.frame = self.frame + 1
    if self.frame == 1:
        self.current = get_tree().get_current_scene()
        self.root_is_viewport = get_viewport() == get_tree().get_root()
";
    let script = GDScriptNodeInstance::from_source(src, world).unwrap();
    tree.attach_script(world, Box::new(script));
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, world, "current"),
        Variant::Nil,
        "a scene added with add_packed_scene_to_tree is not the CURRENT scene, \
         so get_current_scene() must answer null"
    );
    // ...while get_viewport() still resolves, to the root Window — which is
    // exactly why the two cannot share an implementation.
    assert_eq!(
        script_var(&ml, world, "root_is_viewport"),
        Variant::Bool(true),
        "get_viewport() must climb to the root Window, which IS a Viewport"
    );
}

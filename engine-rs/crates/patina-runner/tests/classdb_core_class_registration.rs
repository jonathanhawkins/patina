//! pat-mgixk.101 acceptance: Godot's core classes and engine singletons are
//! ClassDB classes, observable from a booted game's GDScript.
//!
//! # What was wrong
//!
//! `grep '"PackedScene"' gdobject/src/class_db.rs` found **nothing** — and the
//! same for `Image`, `ImageTexture`, `Curve`, `Gradient`, `GDScript`, `Input`,
//! `InputMap`, `ResourceLoader` and the rest of the engine singletons. So
//! `ClassDB.class_exists("PackedScene")` was false, `ClassDB.instantiate("Curve")`
//! returned Nil (`interpreter.rs` answers Nil for an unregistered name), and
//! inheritance queries on these types fell back to hand-written lists.
//!
//! `ClassDB.class_exists` reads `classes.contains_key` directly, so every one of
//! those answers was a lookup miss rather than a wrong value.
//!
//! # ⚠️ Two things the bead's class list gets wrong, and why they matter here
//!
//! The bead cites `extension_api.json`, which is **not in this repo**. Godot's
//! doc XML is, and reading it needs two corrections:
//!
//! 1. `doc/classes/` alone is INCOMPLETE — module classes live in
//!    `modules/*/doc_classes/`. **`GDScript` is a module class**, so a scan of
//!    `doc/classes` silently omits a class this bead names explicitly.
//!
//! 2. 🔑 The XML dirs document Variant BUILT-INS and PRIMITIVES beside real
//!    classes: `Vector2`, `Color`, `Array`, `String`, `bool`, `int`,
//!    `@GlobalScope` — about forty of them. **Godot's ClassDB does not contain
//!    these.** `ClassDB.class_exists("Vector2")` is false in Godot, and
//!    extension_api.json keeps them in a separate `builtin_classes` array from
//!    `.classes`. A guard test that did not exclude them would demand
//!    registering `Vector2` and could never pass — so the exclusion below is
//!    what makes the guard meaningful rather than impossible.
//!
//!    The discriminator is derived, not hand-listed: every real class declares
//!    `inherits=` and only `Object` does not.
//!
//! # ⚠️ Why the fixture works in `_process` and not `_ready`
//!
//! `patina-runner`'s boot calls `LifecycleManager::enter_tree` directly
//! (`bootstrap.rs:562`) rather than `MainLoop::enter_tree_with_scripts`, so
//! `_ready` runs outside the bracket that installs the script-facing servers.
//! ClassDB itself is a static registry and would answer in `_ready`, but a
//! fixture that works in one phase and silently reports defaults in another is
//! not worth the ambiguity — and `ClassDB.instantiate` for a Node-derived class
//! needs scene access. One `step()` puts everything inside the bracket.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

static BOOT_LOCK: Mutex<()> = Mutex::new(());

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk101_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create fixture project dir");
    dir
}

/// Reads a script variable's CURRENT value off the node's script instance.
///
/// ⚠️ `ScriptInstance::get_property`, which is `interpreter.environment.get(..)`
/// — the live scope. NOT `list_properties().default_value`, whose liveness
/// depends on which `ScriptInstance` impl backs the node.
fn script_var(tree: &gdscene::SceneTree, node_id: gdscene::node::NodeId, var: &str) -> Variant {
    tree.get_script(node_id)
        .and_then(|s| s.get_property(var))
        .unwrap_or(Variant::Nil)
}

/// A trivial scene the script `preload`s, to check `is_class` on a PackedScene.
const PROBE_TSCN: &str = "[gd_scene format=3]\n\n[node name=\"Probe\" type=\"Node\"]\n";

/// ⚠️ `== false` rather than `not`, and explicit indices rather than
/// comprehensions: this fixture is evidence about ClassDB, so it leans on as
/// little interpreter surface as it can.
const PROBE_GD: &str = r#"extends Node

var missing = ""
var parent_of_imagetexture = "?"
var can_instantiate_input = true
var curve_is_nil = true
var add_point_index = -99
var curve_count = -1
var curve_first_x = -999.0
var scene_class = "?"
var scene_is_resource = false
var scene_is_node = true
var ran = 0

func _process(_delta):
    if ran == 1:
        return

    # Clause 1: the headline classes must all be known.
    var want = ["PackedScene", "Image", "Curve", "Gradient", "GDScript", "Input", "InputMap", "ResourceLoader"]
    for c in want:
        if ClassDB.class_exists(c) == false:
            missing += c + ","

    # Clause 2: the parent chain, not just the name.
    parent_of_imagetexture = ClassDB.get_parent_class("ImageTexture")

    # Clause 4: a singleton is registered but NOT constructible.
    can_instantiate_input = ClassDB.can_instantiate("Input")

    # Clause 3: instantiate reaches a real object whose add_point works.
    var curve = ClassDB.instantiate("Curve")
    curve_is_nil = curve == null
    if curve_is_nil == false:
        add_point_index = curve.add_point(Vector2(0.75, 0.25))
        curve.add_point(Vector2(0.25, 0.5))
        curve_count = curve.get_point_count()
        curve_first_x = curve.get_point_position(0).x

    # Clause 5: a loaded scene reports its class and its ancestry.
    var scene = preload("res://probe.tscn")
    scene_class = scene.get_class()
    scene_is_resource = scene.is_class("Resource")
    scene_is_node = scene.is_class("Node")

    ran = 1
"#;

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"ClassDbCore\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .expect("write project.godot");
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"Script\" path=\"res://probe.gd\" id=\"1_probe\"]\n\n\
         [node name=\"Main\" type=\"Node\"]\n\
         script = ExtResource(\"1_probe\")\n",
    )
    .expect("write main.tscn");
    std::fs::write(dir.join("probe.tscn"), PROBE_TSCN).expect("write probe.tscn");
    std::fs::write(dir.join("probe.gd"), PROBE_GD).expect("write probe.gd");
}

/// **The acceptance test.**
#[test]
fn classdb_core_class_registration() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("core");
    write_project(&dir);

    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.clone(),
        headless: true,
        ..BootConfig::default()
    };
    let mut boot = EngineBootstrap::new(config);
    boot.run_all().expect("boot flow should complete");
    // One frame, so `_process` runs inside the bracket that installs the
    // script-facing servers. The fixture guards itself with `ran`.
    boot.main_loop_mut()
        .expect("the Running phase creates a MainLoop")
        .step(1.0 / 60.0);

    let tree = boot.main_loop().expect("MainLoop").tree();
    let main = tree
        .get_node_by_path("/root/Main")
        .expect("main.tscn instanced its root");
    let var = |k: &str| script_var(tree, main, k);

    // -- 0. the script ran to completion ----------------------------------
    //
    // 🔑 Asserted first. A missing ClassDB arm raises `UndefinedFunction`,
    // which ABORTS the frame — so every variable below would keep its declared
    // default, and `missing == ""` would read as a pass.
    assert_eq!(
        var("ran"),
        Variant::Int(1),
        "`_process` must reach its last line; a 0 means one of the ClassDB \
         calls aborted the frame"
    );

    // -- 1. every headline class is known ---------------------------------
    assert_eq!(
        var("missing"),
        Variant::String(String::new()),
        "ClassDB must know each of these from a booted game; the listed ones \
         are still missing"
    );

    // -- 2. the parent chain, not just the name ---------------------------
    assert_eq!(
        var("parent_of_imagetexture"),
        Variant::String("Texture2D".into()),
        "ImageTexture derives Texture2D in Godot. An empty string means the \
         class was registered with no parent, which satisfies class_exists \
         while losing every member beneath it"
    );

    // -- 3. a singleton is registered but NOT constructible ---------------
    //
    // ⚠️ This clause is NOT red at HEAD, and saying so matters: `can_instantiate`
    // is `get_by_name(..).map(|i| !i.is_abstract).unwrap_or(false)`, so an
    // UNREGISTERED class also answers false. Its job here is to pin the abstract
    // flag — that `Input` was registered as a singleton rather than as something
    // constructible — not to prove registration happened. Clauses 1, 2, 4 and 5
    // are the ones that fail without this change.
    assert_eq!(
        var("can_instantiate_input"),
        Variant::Bool(false),
        "Input is a singleton: Godot registers the class and refuses to \
         construct it. `true` means it was registered without the abstract flag"
    );

    // -- 4. 🔑 instantiate reaches a real object, and add_point works ------
    //
    // The point of this clause is that `instantiate` must not hand back
    // something that merely EXISTS. Three independent observations, so a stub
    // returning a bare handle cannot pass: the returned index, the count, and
    // the stored position.
    assert_eq!(
        var("curve_is_nil"),
        Variant::Bool(false),
        "ClassDB.instantiate(\"Curve\") must return an object; Nil is what an \
         unregistered class answers"
    );
    assert_eq!(
        var("add_point_index"),
        Variant::Int(0),
        "the first add_point lands at index 0 and Godot returns that index"
    );
    assert_eq!(
        var("curve_count"),
        Variant::Int(2),
        "two add_point calls must leave two points — a count of 0 means the \
         mutation was not written back to the script's variable, and 1 means \
         the second overwrote the first"
    );
    // ⚠️ The points are added out of order (0.75 then 0.25) on purpose: Godot
    // keeps a Curve's points sorted by offset, so point 0 must be the SECOND
    // one added. Appending would leave 0.75 here and pass a weaker assertion.
    match var("curve_first_x") {
        Variant::Float(x) => assert!(
            (x - 0.25).abs() < 1e-5,
            "points must be kept sorted by offset, so point 0 is x=0.25; got {x}. \
             0.75 means they are stored in insertion order"
        ),
        other => panic!("`curve_first_x` should be a Float, got {other:?}"),
    }

    // -- 5. a loaded scene reports its class and its ancestry -------------
    assert_eq!(
        var("scene_class"),
        Variant::String("PackedScene".into()),
        "preload of a .tscn yields a PackedScene"
    );
    assert_eq!(
        var("scene_is_resource"),
        Variant::Bool(true),
        "PackedScene derives Resource, so is_class(\"Resource\") is true. This \
         needs BOTH the registration and an `is_class` arm on the resource \
         dispatch, which had only `get_class`"
    );
    // The negative half: without it, an `is_class` that answered true for
    // everything would satisfy the clause above.
    assert_eq!(
        var("scene_is_node"),
        Variant::Bool(false),
        "a PackedScene is NOT a Node — is_class must discriminate, not just \
         answer true"
    );
}

// ---------------------------------------------------------------------------
// The parity guard
// ---------------------------------------------------------------------------

/// Every class Godot documents, with its declared parent (`None` only for
/// `Object` and for the Variant built-ins).
///
/// Scans all three oracle directories. ⚠️ `doc/classes` alone would miss
/// `GDScript`, which lives under `modules/`.
fn oracle_classes(repo_root: &Path) -> BTreeMap<String, Option<String>> {
    let godot = repo_root.join("upstream/godot");
    let mut dirs = vec![godot.join("doc/classes")];
    for group in ["modules", "platform"] {
        if let Ok(entries) = std::fs::read_dir(godot.join(group)) {
            for e in entries.flatten() {
                let d = e.path().join("doc_classes");
                if d.is_dir() {
                    dirs.push(d);
                }
            }
        }
    }

    let mut out = BTreeMap::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("xml") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            let head = &text[..text.len().min(600)];
            let Some(rest) = head.split_once("<class name=\"") else {
                continue;
            };
            let Some((name, after)) = rest.1.split_once('"') else {
                continue;
            };
            // `inherits` only counts when it is on the <class> tag itself, i.e.
            // before the tag closes.
            let tag_end = after.find('>').unwrap_or(after.len());
            let parent = after[..tag_end]
                .split_once("inherits=\"")
                .and_then(|(_, r)| r.split_once('"').map(|(v, _)| v.to_string()));
            out.insert(name.to_string(), parent);
        }
    }
    out
}

/// The guard: every Godot class that Patina should know is registered.
///
/// 🔑 The exclusions are what make this assertable at all, and each is a rule
/// rather than a name list:
///
/// * **Variant built-ins and primitives** — `inherits` absent and not `Object`.
///   Godot's ClassDB does not contain `Vector2`/`Color`/`Array`/`String`/`int`;
///   demanding them would make this test impossible to satisfy.
/// * **`Editor*` / `OpenXR*` and anything descending from an `Editor*`** — the
///   bead scopes them out explicitly.
/// * **`CSharpScript`** — Patina has no C# module, and Godot built without Mono
///   does not register it either.
#[test]
fn classdb_registration_covers_the_godot_class_list() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root is three levels above the crate")
        .to_path_buf();

    let api = oracle_classes(&repo_root);
    // ⚠️ Asserted, not skipped. A missing oracle would make this guard pass
    // vacuously, which is worse than failing: the whole point is to notice gaps.
    assert!(
        api.len() > 800,
        "expected Godot's doc XML under {}/upstream/godot; found only {} classes. \
         A vacuous pass here would hide every registration gap",
        repo_root.display(),
        api.len()
    );
    assert!(
        api.contains_key("GDScript"),
        "the oracle must include modules/*/doc_classes — GDScript lives there, \
         and scanning only doc/classes silently omits it"
    );

    gdobject::class_db::register_all_classes();

    let ancestors = |name: &str| -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = api.get(name).cloned().flatten();
        while let Some(p) = cur {
            if out.contains(&p) {
                break;
            }
            cur = api.get(&p).cloned().flatten();
            out.push(p);
        }
        out
    };

    let mut expected = BTreeSet::new();
    for (name, parent) in &api {
        if parent.is_none() && name != "Object" {
            continue; // Variant built-in or primitive: not a ClassDB class.
        }
        if name.starts_with("OpenXR") || name == "CSharpScript" {
            continue;
        }
        if name.starts_with("Editor")
            || name.starts_with("ScriptEditor")
            || name.starts_with("ResourceImporter")
            || name == "FileSystemDock"
            || ancestors(name).iter().any(|a| a.starts_with("Editor"))
        {
            continue;
        }
        expected.insert(name.clone());
    }

    let gaps: Vec<&String> = expected
        .iter()
        .filter(|c| !gdobject::class_exists(c))
        .collect();

    assert!(
        gaps.is_empty(),
        "{} of {} expected Godot classes are missing from ClassDB: {:?}",
        gaps.len(),
        expected.len(),
        gaps
    );
}

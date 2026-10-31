//! pat-mgixk.97 acceptance: ClassDB instantiability and parent classes match
//! Godot's own `extension_api.json`.
//!
//! # What was wrong
//!
//! Patina hand-registered ~854 classes and guessed the abstract flag. The guess
//! was wrong 53 times, in both directions, because of one misconception that had
//! spread through the codebase: that `GDREGISTER_VIRTUAL_CLASS` means abstract.
//! It does not. `ClassDB::register_class(bool p_virtual)` installs
//! `creation_func` **unconditionally** (`core/object/class_db.h:255-261`) while
//! `register_abstract_class` does not, and `ClassDB::is_abstract` is exactly
//! `creation_func == nullptr` (`core/object/class_db.cpp:842`). So a virtual
//! class is instantiable; only `GDREGISTER_ABSTRACT_CLASS` is not.
//!
//! The effects a game saw: `BoxContainer.new()`, `Range.new()`,
//! `AnimationNode.new()`, `Material.new()` and 15 more raised "Cannot construct
//! abstract class" where Godot constructs them, and the editor's create-node
//! dialog (which filters on `can_instantiate`) hid them. In the other direction
//! `Noise.new()`, `Light2D.new()`, `FileAccess.new()` and 31 more constructed
//! objects Godot refuses. Five classes also had the wrong parent, which breaks
//! `is`/`is_class` tests and inherited method lookup.
//!
//! # The path driven
//!
//! ```text
//! register_all_classes()  (what res_boot.rs:131 does on project boot)
//!   -> SceneTree + attached GDScript + MainLoop
//!     -> GDScript `ClassDB.can_instantiate` / `.get_parent_class` / `X.new()`
//!       -> gdobject::class_db
//! ```
//!
//! Every assertion below is made **from GDScript**, the way a game asks, not by
//! calling `class_db` from Rust.

use std::collections::HashMap;

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::MainLoop;
use gdvariant::Variant;

/// Queried one class at a time, deliberately.
///
/// A GDScript-side `for name in ClassDB.get_class_list()` loop would put the
/// sweep's own correctness on collection iteration inside the interpreter; one
/// class per call keeps each measurement a single builtin call, so a failure
/// localises to the class it names.
///
/// The constructors are separate methods because a GDScript runtime error
/// **aborts the whole call and cannot be caught** — `Noise.new()` has to be the
/// only statement that can fail in its method, or a later assertion in the same
/// method would never run.
const PROBE: &str = "\
extends Node

func can_make(name):
\treturn ClassDB.can_instantiate(name)

func parent_of(name):
\treturn ClassDB.get_parent_class(name)

func inherits(child, parent):
\treturn ClassDB.is_parent_class(child, parent)

func make_box_container():
\treturn BoxContainer.new()

func make_range():
\treturn Range.new()

func make_control():
\treturn Control.new()

func make_noise():
\treturn Noise.new()

func make_light_2d():
\treturn Light2D.new()

func key_is_resource():
\treturn InputEventKey.new() is Resource
";

struct Fixture {
    main_loop: MainLoop,
    probe: NodeId,
}

fn boot() -> Fixture {
    // What a real project boot does (`res_boot.rs:131`). A bare `SceneTree`
    // never registers anything, so without this every `ClassDB` answer below
    // would be about an empty database — and the sweep would vacuously pass.
    gdobject::class_db::register_all_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let probe = tree
        .add_child(root, Node::new("Probe", "Node"))
        .expect("probe node added");
    let script = GDScriptNodeInstance::from_source(PROBE, probe).expect("probe script parses");
    tree.attach_script(probe, Box::new(script));
    let mut main_loop = MainLoop::new(tree);
    main_loop.tree_mut().process_script_ready(probe);
    main_loop.step(1.0 / 60.0);
    Fixture { main_loop, probe }
}

fn call(fx: &mut Fixture, method: &str, args: &[Variant]) -> Option<Variant> {
    // ⚠️ `call_script_method` answers `None` both for a method that RAISED and
    // for one the script does not define (`scene_tree.rs`: `has_method` guard,
    // then `Err(_) => result stays None`). The "must raise" assertions below
    // read `None` as "raised", so the method had better exist — otherwise a
    // typo would make this file pass while measuring nothing.
    assert!(
        fx.main_loop
            .tree()
            .get_script(fx.probe)
            .expect("probe has a script")
            .has_method(method),
        "the probe script must define `{method}`, or a `None` result below \
         would mean 'no such method' rather than 'construction refused'",
    );
    fx.main_loop
        .tree_mut()
        .call_script_method(fx.probe, method, args)
}

fn s(v: &str) -> Variant {
    Variant::String(v.to_string())
}

fn can_make(fx: &mut Fixture, class_name: &str) -> bool {
    match call(fx, "can_make", &[s(class_name)]) {
        Some(Variant::Bool(b)) => b,
        other => panic!("ClassDB.can_instantiate(\"{class_name}\") gave {other:?}"),
    }
}

fn parent_of(fx: &mut Fixture, class_name: &str) -> String {
    match call(fx, "parent_of", &[s(class_name)]) {
        Some(Variant::String(p)) => p,
        Some(Variant::StringName(p)) => p.as_str().to_string(),
        other => panic!("ClassDB.get_parent_class(\"{class_name}\") gave {other:?}"),
    }
}

/// `(name, is_instantiable, inherits)` for every class in Godot's own dump.
fn oracle() -> HashMap<String, (bool, String)> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/editor_oracle/godot_extension_api.json"
    );
    let raw = std::fs::read_to_string(path).expect("extension_api.json fixture is in-repo");
    let api: serde_json::Value = serde_json::from_str(&raw).expect("fixture is JSON");
    let classes = api["classes"].as_array().expect("api.classes is an array");
    let out: HashMap<String, (bool, String)> = classes
        .iter()
        .filter_map(|c| {
            let name = c["name"].as_str()?.to_string();
            let inst = c["is_instantiable"].as_bool()?;
            let inherits = c["inherits"].as_str().unwrap_or("").to_string();
            Some((name, (inst, inherits)))
        })
        .collect();
    // ⚠️ Guard the EXTRACTOR, not just the data: a renamed key would make every
    // `filter_map` above drop its class and the sweep would pass over an empty
    // oracle. Godot 4.6's dump has ~1000 classes.
    assert!(
        out.len() > 900,
        "parsed only {} classes out of {} entries — the oracle extractor is \
         broken, not the engine",
        out.len(),
        classes.len(),
    );
    out
}

/// **THE ACCEPTANCE TEST (pat-mgixk.97).**
#[test]
fn classdb_instantiability_matches_godot() {
    let mut fx = boot();
    let oracle = oracle();

    // -- (a) the named classes Godot CONSTRUCTS -----------------------------
    //
    // Each was `.abstract_class()` in Patina, so `.new()` raised and these
    // calls answered `None`.
    for method in ["make_box_container", "make_range"] {
        assert!(
            call(&mut fx, method, &[]).is_some(),
            "`{method}` must return an object: both BoxContainer \
             (GDREGISTER_CLASS, register_scene_types.cpp:499) and Range \
             (GDREGISTER_VIRTUAL_CLASS, :469) keep a creation func. `None` here \
             is the pre-bead behaviour — construction refused as abstract",
        );
    }
    assert!(
        can_make(&mut fx, "AnimationNode"),
        "ClassDB.can_instantiate(\"AnimationNode\") must be true — \
         GDREGISTER_CLASS(AnimationNode) at register_scene_types.cpp:584",
    );

    // -- (b) …and the ones it REFUSES ---------------------------------------
    //
    // The control matters: `make_control` has the identical shape and MUST
    // succeed, so a `None` below is "construction refused" and not "every
    // `.new()` in this script is broken".
    assert!(
        call(&mut fx, "make_control", &[]).is_some(),
        "control: Control.new() must work, or the two refusals below prove nothing",
    );
    for (method, class_name) in [("make_noise", "Noise"), ("make_light_2d", "Light2D")] {
        assert!(
            call(&mut fx, method, &[]).is_none(),
            "`{class_name}.new()` must be refused — GDREGISTER_ABSTRACT_CLASS, \
             so Godot raises `Cannot construct abstract class`. Returning an \
             object is the pre-bead behaviour",
        );
        assert!(
            !can_make(&mut fx, class_name),
            "ClassDB.can_instantiate(\"{class_name}\") must be false — this is \
             the same fact the editor's create-node dialog filters on",
        );
    }

    // -- (c) the corrected parents ------------------------------------------
    assert_eq!(
        parent_of(&mut fx, "WorldEnvironment"),
        "Node",
        "WorldEnvironment inherits Node, not Node3D — it has no transform",
    );
    assert_eq!(
        parent_of(&mut fx, "InputEvent"),
        "Resource",
        "InputEvent inherits Resource, not RefCounted",
    );
    assert_eq!(
        call(&mut fx, "key_is_resource", &[]),
        Some(Variant::Bool(true)),
        "`InputEventKey.new() is Resource` follows from InputEvent's parent, and \
         is how a script actually observes it",
    );

    // -- (d) THE SWEEP: every registered class, both facts, zero mismatches --
    let class_list = gdobject::class_db::get_class_list();
    assert!(
        class_list.len() > 800,
        "only {} classes registered — register_all_classes() did not run, and \
         the sweep below would be vacuous",
        class_list.len(),
    );

    let mut inst_mismatches = Vec::new();
    let mut parent_mismatches = Vec::new();
    let mut absent_from_oracle = Vec::new();

    for class_name in &class_list {
        let Some((want_inst, want_parent)) = oracle.get(class_name) else {
            absent_from_oracle.push(class_name.clone());
            continue;
        };
        let have_inst = can_make(&mut fx, class_name);
        if have_inst != *want_inst {
            inst_mismatches.push(format!(
                "{class_name}: patina can_instantiate={have_inst}, godot={want_inst}"
            ));
        }
        if !want_parent.is_empty() {
            let have_parent = parent_of(&mut fx, class_name);
            if !have_parent.is_empty() && have_parent != *want_parent {
                parent_mismatches.push(format!(
                    "{class_name}: patina parent={have_parent}, godot={want_parent}"
                ));
            }
        }
    }

    assert!(
        inst_mismatches.is_empty(),
        "{} classes disagree with extension_api.json on instantiability:\n{}",
        inst_mismatches.len(),
        inst_mismatches.join("\n"),
    );
    assert!(
        parent_mismatches.is_empty(),
        "{} classes disagree with extension_api.json on their parent:\n{}",
        parent_mismatches.len(),
        parent_mismatches.join("\n"),
    );

    // ⚠️ The skip list is ASSERTED, not merely logged. Left unchecked, a future
    // registration of a class Godot does not have would silently drop out of the
    // sweep above — which is exactly how a parity test rots into a no-op.
    //
    // A SUBSET check, not equality: other agents add registrations to
    // `class_db.rs` constantly, and a real Godot class is in the oracle and so
    // never lands here. Only a name Godot does not have does — a typo, or a
    // class needing a parity decision — and that should fail loudly. Equality
    // would instead break this test every time someone legitimately removed one.
    const KNOWN_ABSENT: [&str; 3] = [
        // Removed in Godot 4 (it was a 3.x module).
        "VisualScript",
        // Not a Godot class name; Godot spells its TCP peer differently.
        "TcpMultiplayerPeer",
        // A real Godot class, but `modules/text_server_fb` is not exposed in
        // extension_api.json, so the oracle cannot judge it.
        "TextServerFallback",
    ];
    absent_from_oracle.sort();
    let unexpected: Vec<&String> = absent_from_oracle
        .iter()
        .filter(|c| !KNOWN_ABSENT.contains(&c.as_str()))
        .collect();
    assert!(
        unexpected.is_empty(),
        "these registered classes are not in Godot's extension_api.json, so the \
         sweep above silently skipped them: {unexpected:?}. Either the \
         registration has a typo'd class name, or the class genuinely has no \
         Godot counterpart and belongs in KNOWN_ABSENT with a reason",
    );
}

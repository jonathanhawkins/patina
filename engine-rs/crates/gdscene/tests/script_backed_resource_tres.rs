//! pat-fk17e.1 acceptance: a GDScript that `extends Resource` is a Resource
//! object carrying a script, and a `.tres` naming it with
//! `script = ExtResource("<id>")` loads and saves as one.
//!
//! # Why this is an integration test and not an in-crate `mod tests`
//!
//! The bead asks for this "through the scene/script path, not Rust-level APIs".
//! An out-of-crate test is the only placement that can actually promise that:
//! it sees exactly the surface a host embedding the engine sees, so it cannot
//! pass against a `pub(crate)` item a real script could never reach. Every
//! assertion below is on a `Variant` that GDScript code produced — the Rust
//! here only builds the tree and asks for a method.
//!
//! That costs the error text on failure: `SceneTree::call_script_method`
//! returns `Option<Variant>` and logs the `RuntimeError` at debug level rather
//! than returning it. When `run()` comes back `None`, re-run with
//! `RUST_LOG=debug` to read the script error.
//!
//! ⚠️ Absolute fixture paths, not `res://`. `res://` resolution goes through
//! `gdcore::set_path_roots`, a process-global `RwLock`; seeding it would change
//! what every concurrent `res://` read resolves to, for a path-resolution hop
//! this bead did not touch. The `.tres`'s `[ext_resource path=..]` is handed to
//! the same `FileAccess::read_file_string` seam `instantiate_script` already
//! uses, which `accessor_instantiate_script_attaches_script_node` covers.

use gdscene::{GDScriptNodeInstance, Node, SceneTree};
use gdvariant::Variant;
use std::path::PathBuf;

/// A fresh temp directory for one test's fixture project.
fn fixture_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("patina_{tag}_{}_{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture dir");
    dir
}

/// Runs `body` as the single method of a script attached to a fresh node, and
/// returns what the script returned.
///
/// `SceneTree::call_script_method` is the public entry point — it installs a
/// real scene accessor for the duration of the call, so `preload`, `.new()` and
/// cross-object method calls all work exactly as they do in a running game.
fn run_script(body: &str) -> Option<Variant> {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree
        .add_child(root, Node::new("Holder", "Node"))
        .expect("child adds");
    let src = format!("extends Node\nfunc run():\n{body}");
    let script = GDScriptNodeInstance::from_source(&src, node).expect("script parses");
    tree.attach_script(node, Box::new(script));
    tree.call_script_method(node, "run", &[])
}

/// The observations `run()` returned, or a panic naming how to debug a failure.
fn observations(body: &str) -> Vec<Variant> {
    let Some(value) = run_script(body) else {
        panic!(
            "the script did not return — `call_script_method` logs the \
             RuntimeError at debug level instead of returning it, so re-run \
             with RUST_LOG=debug to read it"
        );
    };
    match value {
        Variant::Array(items) => items.to_vec(),
        other => panic!("expected an Array of observations, got {other:?}"),
    }
}

/// 🔑 A SCRIPT-BACKED RESOURCE IS AN OBJECT CARRYING A SCRIPT, NOT A
/// `ResourceRef` WITH A `script` FIELD.
///
/// Godot's custom data resource: a `.gd` that `extends Resource`, a `.tres`
/// naming it, one line per exported var. Everything a game does with one is
/// here — read a saved value, call a script method on it, type-test it,
/// construct a default-valued one from the script, and write it back out.
///
/// ⚠️ `is_class` is asserted as `Bool(true)`, not for truthiness. Before this
/// bead the call returned **`Nil`**: ClassDB lists `is_class` on `Object`, so
/// `node_has_method` said yes and `call_object_method` fell into its `responds`
/// branch, which answers `Nil`. A truthiness check would have read that as a
/// plain `false`, and an `is_ok()` check would have called it a pass.
#[test]
fn script_backed_resource_tres_roundtrip_and_new() {
    // `is_class("Object")` below walks the ClassDB inheritance chain, which is
    // empty until a `register_*` entry point runs. `Resource` lives in the 2D
    // table. Safe to seed here: each `tests/*.rs` is its own binary, so nothing
    // else in this process can clear it mid-test.
    gdobject::class_db::register_2d_classes();
    // A PRECONDITION, not an assertion about this bead: `register_2d_classes`
    // bootstraps `Object` (:12641) and `RefCounted` (:15715) so `Resource` can
    // parent to it, and `class_db.rs:15711` records that an unregistered base
    // "silently severs the whole chain from Object" (pat-nzgdu.98). If that
    // ever regresses, `is_class("Object")` below goes false and would read as a
    // dispatch bug in my code. This makes it name the real cause instead.
    assert!(
        gdobject::is_parent_class("Resource", "Object"),
        "precondition: ClassDB must carry Resource -> RefCounted -> Object \
         before is_class(\"Object\") can mean anything"
    );

    let dir = fixture_dir("fk17e1");
    let item_gd = dir.join("item.gd");
    std::fs::write(
        &item_gd,
        "extends Resource\n\
         @export var damage: int = 1\n\
         func describe():\n\
         \x20   return \"dmg %d\" % damage\n",
    )
    .expect("write item.gd");

    let sword = dir.join("sword.tres");
    std::fs::write(
        &sword,
        format!(
            "[gd_resource type=\"Resource\" script_class=\"Item\" format=3]\n\
             \n\
             [ext_resource type=\"Script\" path=\"{script}\" id=\"1_item\"]\n\
             \n\
             [resource]\n\
             script = ExtResource(\"1_item\")\n\
             damage = 7\n",
            script = item_gd.display()
        ),
    )
    .expect("write sword.tres");

    let out = dir.join("sword_saved.tres");
    let body = format!(
        "\x20   var res = preload(\"{sword}\")\n\
         \x20   var item_script = preload(\"{item}\")\n\
         \x20   var fresh = item_script.new()\n\
         \x20   var err = ResourceSaver.save(res, \"{out}\")\n\
         \x20   var reread = preload(\"{out}\")\n\
         \x20   return [res.damage, res.describe(), res.is_class(\"Resource\"), \
         fresh.damage, err, reread.damage, res.get_script().get_class(), \
         res.get_script().resource_path, res.is_class(\"Object\"), \
         res.is_class(\"Node2D\")]\n",
        sword = sword.display(),
        item = item_gd.display(),
        out = out.display(),
    );

    let v = observations(&body);

    assert_eq!(
        v[0],
        Variant::Int(7),
        "the `.tres`'s `damage = 7` overrides the script's `= 1` default"
    );
    assert_eq!(
        v[1],
        Variant::String("dmg 7".into()),
        "a script METHOD runs on the loaded resource, over the saved value"
    );
    assert_eq!(
        v[2],
        Variant::Bool(true),
        "`extends Resource` makes the instance report is_class(\"Resource\")"
    );
    assert_eq!(
        v[3],
        Variant::Int(1),
        "`ItemScript.new()` takes the script's declared default, NOT the \
         `.tres`'s 7 — a shared instance would show 7 here, and that is the \
         failure an assertion on `res.damage` alone cannot see"
    );
    assert_eq!(
        v[4],
        Variant::Int(0),
        "ResourceSaver.save returns Error::OK"
    );
    assert_eq!(
        v[5],
        Variant::Int(7),
        "the saved file re-loads as a script-backed resource with damage 7"
    );
    assert_eq!(
        v[6],
        Variant::String("GDScript".into()),
        "get_script() hands back the GDScript resource, not null"
    );
    assert_eq!(
        v[7],
        Variant::String(item_gd.display().to_string()),
        "and it names the .gd file — a `GDScript` with an empty path would \
         still pass the get_class() assertion above"
    );
    // The two that pin the ClassDB chain walk rather than string equality:
    // `is_class("Resource")` alone passes on `cls == name` without consulting
    // ClassDB at all, so on its own it cannot tell a chain walk from a
    // comparison.
    assert_eq!(
        v[8],
        Variant::Bool(true),
        "Resource derives Object, so is_class walks the chain, not just the \
         own-class name"
    );
    assert_eq!(
        v[9],
        Variant::Bool(false),
        "a negative control: Node2D is registered but is NOT an ancestor of \
         Resource, so a chain walk that returned true for everything \
         registered would fail here"
    );

    // The bytes themselves: the script reference and the exported var.
    let saved = std::fs::read_to_string(&out).expect("the saver wrote the file");
    let script_at = saved
        .find("script = ExtResource(\"1_item\")")
        .unwrap_or_else(|| panic!("no script reference in:\n{saved}"));
    let damage_at = saved
        .find("damage = 7")
        .unwrap_or_else(|| panic!("the exported var was not written:\n{saved}"));
    assert!(
        script_at < damage_at,
        "`script` precedes the exported vars, as resource_format_text.cpp \
         writes it; got:\n{saved}"
    );
    assert!(
        saved.contains(&format!("path=\"{}\"", item_gd.display())),
        "the ext_resource names the .gd file; got:\n{saved}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 🔑 A `.tres` WITH NO `script` LINE IS STILL A PLAIN RESOURCE.
///
/// The script-backed path is a new branch in `preload`/`load`, taken only when
/// `script = ExtResource(..)` is present. Every other `.tres` — which is nearly
/// all of them — must still come back as a `Variant::Resource` property bag,
/// because `draw_style_box`, `stylebox_from_props` and the theme loader all
/// read `class_name` and `properties` off that value. An over-eager branch here
/// would turn them all into object ids and silently draw nothing.
#[test]
fn plain_tres_without_a_script_still_loads_as_a_resource() {
    let dir = fixture_dir("fk17e1_plain");
    let box_tres = dir.join("box.tres");
    std::fs::write(
        &box_tres,
        "[gd_resource type=\"StyleBoxFlat\" format=3]\n\
         \n\
         [resource]\n\
         content_margin_left = 4.0\n",
    )
    .expect("write box.tres");

    let body = format!(
        "\x20   var res = preload(\"{p}\")\n\
         \x20   return [res.get_class(), res.get(\"content_margin_left\")]\n",
        p = box_tres.display()
    );
    let v = observations(&body);
    assert_eq!(
        v[0],
        Variant::String("StyleBoxFlat".into()),
        "a scriptless .tres keeps its own class name, read off the file"
    );
    assert_eq!(
        v[1],
        Variant::Float(4.0),
        "and its properties are still readable as a property bag"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

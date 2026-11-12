//! pat-s3rrv.57: `ClassDB.instantiate()` must build non-Node classes, and build
//! them as the real thing.
//!
//! `instantiate` handled Node-derived classes through `SceneAccess::create_node`
//! and returned `Nil` for everything else — so `ClassDB.instantiate("ConfigFile")`
//! produced nothing at all, while `ConfigFile.new()` two lines away produced a
//! working object. Godot has one factory behind both spellings
//! (`ClassDB::instantiate`, `upstream/godot/core/object/class_db.cpp`), which is
//! what makes data-driven construction — a class name read from a config file, a
//! save game, or an editor list — possible.
//!
//! The fix delegates to the `ClassName.new()` expression rather than restating
//! its ~30 bespoke handle arms, so the two spellings cannot drift apart.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// ⚠️ `ClassDB` answers out of the registry, so an unpopulated one makes every
/// case here return `Nil` and the test would "pass" against a factory that
/// builds nothing.
const SRC: &str = "extends Node\n\
                   \n\
                   func make_config():\n\
                   \x20   var c = ClassDB.instantiate(\"ConfigFile\")\n\
                   \x20   c.set_value(\"player\", \"hp\", 7)\n\
                   \x20   return c.get_value(\"player\", \"hp\", 0)\n\
                   \n\
                   func make_refcounted():\n\
                   \x20   return ClassDB.instantiate(\"RefCounted\")\n\
                   \n\
                   func make_unknown():\n\
                   \x20   return ClassDB.instantiate(\"NoSuchClassAnywhere\")\n\
                   \n\
                   func make_abstract():\n\
                   \x20   return ClassDB.instantiate(\"Light3D\")\n";

/// Runs one method of the probe script.
///
/// Mirrors the call sequence of the green `color_builtin_surface_from_scene`
/// test — `run_class` → `instantiate_class` → `call_instance_method` — rather
/// than inventing one.
fn call(method: &str) -> Result<Variant, String> {
    gdobject::class_db::register_all_classes();
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(SRC).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, method, &[])
        .map_err(|e| format!("{e:?}"))
}

/// **The acceptance case.** The object must be a *working* ConfigFile.
///
/// Asserting merely "not Nil" would pass against a bare refcounted handle —
/// an object that exists and answers nothing, which is worse than the `Nil`
/// this used to return. Only a round-trip through the class's own methods
/// distinguishes the two.
#[test]
fn instantiate_returns_a_usable_config_file() {
    assert_eq!(
        call("make_config").expect("ConfigFile instantiates"),
        Variant::Int(7),
        "the value written through the instantiated object must read back — a \
         handle whose set_value silently no-ops would return the 0 default"
    );
}

#[test]
fn instantiate_builds_a_plain_refcounted() {
    let got = call("make_refcounted").expect("RefCounted instantiates");
    assert!(
        matches!(got, Variant::ObjectId(_)),
        "RefCounted is constructible in Godot; got {got:?}"
    );
}

/// Godot returns null for a class it does not know rather than raising, and the
/// abstract case is the one that *does* raise. The pair is what proves the
/// delegation did not swallow the abstract gate: `Material.new()` on its own
/// fails with an UndefinedVariable, not with Godot's abstract-class message.
/// (The abstract probe is `Light3D` — see the note in the test.)
#[test]
fn unknown_is_nil_and_abstract_still_errors() {
    assert_eq!(
        call("make_unknown").expect("an unknown class is not an error"),
        Variant::Nil,
        "Godot returns null for an unregistered class"
    );

    // ⚠️ `Light3D`, not `Material`: Material is `GDREGISTER_VIRTUAL_CLASS`
    // (`scene/register_scene_types.cpp:439`), and a VIRTUAL class still has a
    // creation function, so Godot's `ClassDB.instantiate("Material")`
    // SUCCEEDS. `GDREGISTER_ABSTRACT_CLASS(Light3D)` is genuinely abstract.
    let err = call("make_abstract").expect_err("Light3D is abstract");
    assert!(
        err.contains("abstract"),
        "an abstract class must report Godot's abstract-class error, not a \
         construction failure; got {err}"
    );
}

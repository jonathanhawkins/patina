//! pat-nzgdu.189 — `bytes_to_var` hands scripts an inert `EncodedObjectAsID`,
//! never a live object.
//!
//! **This is a security boundary, and these tests are what hold it.** Godot's
//! `bytes_to_var` decodes with objects disallowed, so an encoded object comes
//! back as an `EncodedObjectAsID` whose only surface is `object_id`; turning it
//! into a usable object needs an explicit `instance_from_id()`. That extra step
//! is precisely what `p_allow_objects = false` buys
//! (`upstream/godot/core/io/marshalls.cpp:699-715`).
//!
//! In this port `Variant::ObjectId` is *not* an inert id — it is the same live
//! node reference `get_node()` returns, and `get_children()` on one walks the
//! scene tree (`interpreter.rs`, the `Variant::ObjectId` method arm). Handing a
//! decoded id straight back would let `bytes_to_var(untrusted).get_children()`
//! reach into the running scene, with ids that start at 1 and are trivially
//! guessable. So the interpreter re-wraps every decoded id.
//!
//! The mock below deliberately returns a **non-empty** child list: if the
//! wrapper ever regressed, "inert" must not pass merely because the scene had
//! nothing to find.
//!
//! Integration tests on purpose: `-p gdscript-interop --lib` cannot pass (a
//! pre-existing `fuzz_deeply_nested_parens` stack overflow cancels the run).

use gdscript_interop::bindings::SceneAccess;
use gdscript_interop::interpreter::{Interpreter, RuntimeErrorKind};
use gdvariant::Variant;

/// The id `create_node` hands out, and the one child it always reports.
const NODE_ID: u64 = 7;
const CHILD_ID: u64 = 8;

/// A scene with one node that has one child, so `Node.new()` yields a real,
/// plain (untagged) object id and `get_children()` has something to return.
#[derive(Default)]
struct SceneMock;

impl SceneAccess for SceneMock {
    fn get_node(&self, _from: u64, _path: &str) -> Option<u64> {
        Some(NODE_ID)
    }
    fn get_parent(&self, _node: u64) -> Option<u64> {
        None
    }
    fn get_children(&self, _node: u64) -> Vec<u64> {
        // NON-EMPTY on purpose — see the module docs.
        vec![CHILD_ID]
    }
    fn get_node_property(&self, _node: u64, _prop: &str) -> Variant {
        Variant::Nil
    }
    fn set_node_property(&mut self, _node: u64, _prop: &str, _value: Variant) {}
    fn emit_signal(&mut self, _node: u64, _signal: &str, _args: &[Variant]) {}
    fn connect_signal(&mut self, _source: u64, _signal: &str, _target: u64, _method: &str) {}
    fn get_node_name(&self, _node: u64) -> Option<String> {
        Some("Node".to_string())
    }
    fn is_input_action_pressed(&self, _action: &str) -> bool {
        false
    }
    fn is_input_action_just_released(&self, _action: &str) -> bool {
        false
    }
    fn create_node(&mut self, _class_name: &str, _name: &str) -> Option<u64> {
        Some(NODE_ID)
    }
}

fn interp() -> Interpreter {
    let mut i = Interpreter::new();
    i.set_scene_access(Box::new(SceneMock), NODE_ID);
    i
}

fn run_val(src: &str) -> Variant {
    interp()
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}"))
        .return_value
        .expect("script returned a value")
}

fn run_err(src: &str) -> gdscript_interop::interpreter::RuntimeError {
    interp().run(src).expect_err("script should have failed")
}

/// Sanity: a LIVE node really can reach the scene. Without this, the inertness
/// tests below could pass simply because nothing works.
#[test]
fn a_live_node_can_reach_the_scene() {
    assert_eq!(
        run_val("return Node.new().get_children().size()\n"),
        Variant::Int(1),
        "a live node must see its child — otherwise 'inert' proves nothing"
    );
}

/// The acceptance: an object encoded as an id, decoded, reports that id.
#[test]
fn encoded_object_as_id_roundtrip_through_script() {
    let v = run_val(
        "var n = Node.new()\n\
         var id = n.get_instance_id()\n\
         var decoded = bytes_to_var(var_to_bytes(n))\n\
         return decoded.get_object_id() == id\n",
    );
    assert_eq!(
        v,
        Variant::Bool(true),
        "the decoded EncodedObjectAsID must report the original instance id"
    );
    // ...and that id is the real one, not a coincidence.
    assert_eq!(
        run_val(
            "var decoded = bytes_to_var(var_to_bytes(Node.new()))\n\
             return decoded.get_object_id()\n"
        ),
        Variant::Int(NODE_ID as i64)
    );
}

/// ⚠️ THE SECURITY PROPERTY. A decoded id must NOT be a usable node.
///
/// If this regresses, `bytes_to_var(untrusted_bytes).get_children()` walks the
/// live scene tree — an id-guessing hole Godot does not have.
#[test]
fn a_decoded_id_is_inert_and_cannot_touch_the_scene() {
    // The live node returns 1 child (asserted above). The decoded wrapper must
    // ERROR rather than return anything — an empty list would mean the call
    // reached the scene layer at all.
    let err = run_err(
        "var decoded = bytes_to_var(var_to_bytes(Node.new()))\n\
         return decoded.get_children()\n",
    );
    assert!(
        matches!(err.kind, RuntimeErrorKind::UndefinedFunction(_)),
        "an EncodedObjectAsID must not answer Node methods — got {err:?}"
    );

    // The attacker's case: bytes naming an arbitrary object.
    let err = run_err(
        "var forged = bytes_to_var(var_to_bytes(Node.new()))\n\
         forged.set_object_id(1)\n\
         return forged.get_child_count()\n",
    );
    assert!(
        matches!(err.kind, RuntimeErrorKind::UndefinedFunction(_)),
        "a forged id must stay inert — got {err:?}"
    );
}

/// The wrapper reports its id and `set_object_id` round-trips — the class's
/// whole surface, and nothing that resolves it.
#[test]
fn object_id_is_readable_and_writable() {
    assert_eq!(
        run_val(
            "var d = bytes_to_var(var_to_bytes(Node.new()))\n\
             d.set_object_id(4242)\n\
             return d.get_object_id()\n"
        ),
        Variant::Int(4242)
    );

    // An unknown method still errors, so this is a real dispatch table and not a
    // catch-all that silently swallows Node calls.
    let err = run_err(
        "var d = bytes_to_var(var_to_bytes(Node.new()))\n\
         return d.__nope__()\n",
    );
    assert!(matches!(err.kind, RuntimeErrorKind::UndefinedFunction(_)));
}

/// ⚠️ An RPC payload is normally an ARRAY of arguments, so an id nested inside a
/// container is the realistic attack — and exactly as dangerous as a bare one.
#[test]
fn ids_nested_in_containers_are_wrapped_too() {
    assert_eq!(
        run_val(
            "var n = Node.new()\n\
             var id = n.get_instance_id()\n\
             var decoded = bytes_to_var(var_to_bytes([1, n, \"x\"]))\n\
             return decoded[1].get_object_id() == id\n"
        ),
        Variant::Bool(true),
        "an id inside an array must decode to a wrapped EncodedObjectAsID"
    );

    let err = run_err(
        "var decoded = bytes_to_var(var_to_bytes([Node.new()]))\n\
         return decoded[0].get_children()\n",
    );
    assert!(
        matches!(err.kind, RuntimeErrorKind::UndefinedFunction(_)),
        "a nested decoded id must be inert too — got {err:?}"
    );
}

/// `Marshalls.base64_to_variant` is the same decode behind a different
/// transport. Missing it would leave the hole open through a side door.
#[test]
fn base64_to_variant_wraps_ids_as_well() {
    assert_eq!(
        run_val(
            "var n = Node.new()\n\
             var id = n.get_instance_id()\n\
             var b64 = Marshalls.variant_to_base64(n)\n\
             return Marshalls.base64_to_variant(b64).get_object_id() == id\n"
        ),
        Variant::Bool(true)
    );

    let err = run_err(
        "var b64 = Marshalls.variant_to_base64(Node.new())\n\
         return Marshalls.base64_to_variant(b64).get_children()\n",
    );
    assert!(
        matches!(err.kind, RuntimeErrorKind::UndefinedFunction(_)),
        "base64-decoded ids must be inert too — got {err:?}"
    );
}

/// Each decode mints a distinct wrapper, as in Godot (every `bytes_to_var`
/// instantiates a fresh `EncodedObjectAsID`). Aliasing them would make one
/// script's write visible to another's handle.
#[test]
fn each_decode_mints_a_distinct_wrapper() {
    assert_eq!(
        run_val(
            "var bytes = var_to_bytes(Node.new())\n\
             var a = bytes_to_var(bytes)\n\
             var b = bytes_to_var(bytes)\n\
             a.set_object_id(1)\n\
             b.set_object_id(2)\n\
             return a.get_object_id() == 1 and b.get_object_id() == 2\n"
        ),
        Variant::Bool(true),
        "wrappers must be independent, not aliases of one entry"
    );
}

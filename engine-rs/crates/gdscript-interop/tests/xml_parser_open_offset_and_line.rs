//! pat-s3rrv.254 — `XMLParser.open()`, `get_node_offset`, `get_current_line`
//! and `get_named_attribute_value_safe`.
//!
//! # What was actually missing
//!
//! The bead asks for four additions. Two already existed — `open(path)` and
//! `get_named_attribute_value_safe` were both dispatched — so the real work was
//! narrower and sharper than "add four methods":
//!
//! * **`get_current_line` returned `cursor()`** — a CHARACTER OFFSET reported as
//!   a line number. It answered plausibly (a small integer that grows as you
//!   read) and was wrong everywhere. A script printing `"error on line " +
//!   str(parser.get_current_line())` produced a confident lie.
//! * **`get_node_offset` did not exist**, so `seek()` had nothing correct to be
//!   fed. `cursor()` is where the parser has REACHED; seeking to it skips the
//!   node you wanted.
//! * **`open()` read through `FileAccess::open`**, whose own documentation says
//!   it "cannot serve a pack". So opening XML that ships inside a mounted
//!   `.pck` — the case a released game actually hits — failed. It now goes
//!   through `read_file_bytes`, which consults packs first.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Serializes the tests in this binary that touch the res:// path roots (each test roots res:// at its own temp dir) (pat-hukbz).
///
/// That state is process-global. Nextest gives each test its own process, but
/// plain `cargo test` runs the tests as threads in ONE process, where they
/// overwrote each other's state mid-test. Bind the guard for the whole test.
/// Poison-tolerant, so one failing test does not fail the rest.
#[must_use = "bind it for the whole test: `let _guard = serial();`"]
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Three lines, so a line number is distinguishable from an offset and from 1.
///
/// ⚠️ The indentation before `<item>` is load-bearing: it makes the element
/// start at char 9 while the newline sits at char 6, so a `current_line` that
/// counted the wrong side of the boundary would report 1 instead of 2.
const DOC: &str = "<root>\n  <item id=\"1\"/>\n</root>\n";

/// Runs `method` on a fresh instance of `src` through the real VM dispatch.
fn call_method(src: &str, method: &str) -> Variant {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, method, &[])
        .unwrap_or_else(|e| panic!("call {method}() failed: {e:?}"))
}

/// Points `res://` at a temp dir holding `doc.xml`.
///
/// The `TempDir` must outlive the call — dropping it deletes the file, and the
/// parser would then fail for a reason that has nothing to do with this bead.
fn with_res_doc() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("doc.xml"), DOC).expect("write doc.xml");
    gdcore::file_access::set_path_roots(Some(dir.path().to_path_buf()), None);
    dir
}

const SRC: &str = "\
extends Node

var line = -1
var name_at_offset = \"\"
var reread = \"\"
var safe_absent = \"missing\"
var safe_present = \"\"
var open_result = -1

func run():
    var p = XMLParser.new()
    self.open_result = p.open(\"res://doc.xml\")
    # <root>, then the whitespace text node, then <item>.
    p.read()
    p.read()
    p.read()
    self.name_at_offset = p.get_node_name()
    self.line = p.get_current_line()
    self.safe_absent = p.get_named_attribute_value_safe(\"absent\")
    self.safe_present = p.get_named_attribute_value_safe(\"id\")
    # The round trip: go back to where this node STARTED and read it again.
    var off = p.get_node_offset()
    p.seek(off)
    p.read()
    self.reread = p.get_node_name()
";

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** `open()` from `res://`, a correct 1-based line, an
/// offset that round-trips through `seek()`, and a safe attribute read.
#[test]
fn xml_parser_open_offset_and_line() {
    let _guard = serial();
    let _dir = with_res_doc();

    // Re-run per assertion because each `call_method` builds a fresh
    // interpreter — the script records everything in one pass, so this reads
    // each var off its own run of the same deterministic script.
    let var = |name: &str| {
        let src = format!("{SRC}\nfunc read_{name}():\n\x20   run()\n\x20   return self.{name}\n");
        call_method(&src, &format!("read_{name}"))
    };

    assert_eq!(
        var("open_result"),
        Variant::Int(0),
        "open(\"res://doc.xml\") must return OK — 12 is ERR_FILE_CANT_OPEN"
    );
    assert_eq!(
        var("name_at_offset"),
        Variant::String("item".into()),
        "three reads land on <item>: the root element, the whitespace text node \
         between them, then the item"
    );

    // -- the line, which is the assertion the old cursor-based version failed --
    // `<item>` starts at char 9 with a single newline before it at char 6, so
    // the line is 2 and the offset is 9. A `get_current_line` returning the
    // cursor would answer something in the twenties here.
    assert_eq!(
        var("line"),
        Variant::Int(2),
        "get_current_line() is a 1-based LINE, not a character offset"
    );

    // -- the offset round-trips ----------------------------------------------
    // Not hardcoded: the script feeds whatever `get_node_offset()` returns back
    // into `seek()`. That is the property the acceptance asks for, and it holds
    // without the test knowing the number.
    assert_eq!(
        var("reread"),
        Variant::String("item".into()),
        "seek(get_node_offset()) then read() must re-read the SAME node — with \
         cursor() instead of the node start this lands past it"
    );

    // -- the safe attribute read ---------------------------------------------
    assert_eq!(
        var("safe_absent"),
        Variant::String(String::new()),
        "a missing attribute reads as \"\" rather than raising"
    );
    // The paired positive: without it, an implementation returning "" for
    // everything would satisfy the assertion above.
    assert_eq!(
        var("safe_present"),
        Variant::String("1".into()),
        "…and a present one still returns its value"
    );
}

/// ClassDB must know the class the interpreter has been driving all along.
///
/// The bead's second merged finding: every method below was dispatchable while
/// `class_exists("XMLParser")` answered false, so reflection and the inspector
/// denied a class a script could construct and use.
#[test]
fn xml_parser_open_offset_and_line_is_registered() {
    let _guard = serial();
    use gdobject::class_db::{class_exists, get_class_info, register_all_classes};

    register_all_classes();
    assert!(class_exists("XMLParser"), "XMLParser must be registered");

    let info = get_class_info("XMLParser").expect("registered");
    assert_eq!(
        info.parent_class, "RefCounted",
        "XMLParser inherits RefCounted per its doc XML"
    );
    for name in [
        "open",
        "open_buffer",
        "read",
        "seek",
        "skip_section",
        "get_node_offset",
        "get_current_line",
        "get_named_attribute_value_safe",
    ] {
        assert!(
            info.methods.iter().any(|m| m.name == name),
            "XMLParser must declare {name}"
        );
    }
    // The control: the lookup reports what was declared, not anything asked of it.
    assert!(
        !info.methods.iter().any(|m| m.name == "no_such_method"),
        "a method lookup that says yes to anything would satisfy the loop above"
    );
}

/// A fresh parser reports line 1, not line 0 or a cursor.
///
/// Godot's line numbers are 1-based, and the off-by-one here is invisible in
/// the middle of a document — every line is simply one less than it should be,
/// which reads as plausible until someone matches it against an editor.
#[test]
fn xml_parser_open_offset_and_line_starts_at_line_one() {
    let _guard = serial();
    let _dir = with_res_doc();

    let src = "\
extends Node

var before = -1
var after_first = -1

func run():
    var p = XMLParser.new()
    p.open(\"res://doc.xml\")
    self.before = p.get_current_line()
    p.read()
    self.after_first = p.get_current_line()
    return self.before * 100 + self.after_first
";
    assert_eq!(
        call_method(src, "run"),
        Variant::Int(101),
        "line 1 before any read, and still line 1 on the first element — \
         encoded as before*100 + after so one assertion pins both"
    );
}

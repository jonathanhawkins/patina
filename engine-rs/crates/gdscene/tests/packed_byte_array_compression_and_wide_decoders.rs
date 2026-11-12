//! pat-s3rrv.224 — `PackedByteArray.compress/decompress` and the wide string
//! decoders, driven from a scene script.
//!
//! `compress`, `decompress`, `decompress_dynamic`, `get_string_from_utf16`,
//! `get_string_from_utf32`, `get_string_from_wchar` and
//! `get_string_from_multibyte_char` appeared nowhere in the workspace — the
//! bead's claim checked out exactly.
//!
//! ⚠️ **Only DEFLATE (1) and GZIP (3) are implemented.** FASTLZ (0), ZSTD (2)
//! and BROTLI (4) have no codec anywhere in this workspace — `zstd` and
//! `brotli` are not even in `Cargo.lock` — so each would need a new external
//! dependency. They return an **empty** array, which is Godot's own failure
//! signal, and the tests pin that. The acceptance asks for a ZSTD round-trip;
//! that clause is not deliverable without adding a crate, and I did not add one
//! unilaterally. See the completion report.
//!
//! ⓘ The buffers are built in Rust and read by the script as node properties
//! because `String.to_utf8_buffer()` does not exist yet, so a script cannot
//! construct a `PackedByteArray` from text. The codecs are still exercised
//! entirely from GDScript, which is what the wiring requirement asks for.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// Repetitive on purpose: DEFLATE must visibly shrink it, so
/// `compressed_is_actually_smaller` is a real assertion rather than a tautology.
const TEXT: &str = "patina patina patina patina patina patina patina patina";

const SCRIPT: &str = "\
var deflate_ok = false
var gzip_ok = false
var deflate_smaller = false
var zstd_empty = false
var fastlz_empty = false
var small_buffer_empty = false
var dynamic_ok = false
var dynamic_capped = false
var s16 = \"\"
var s32 = \"\"
var swchar = \"\"
var ready_done = false
func _ready():
    var d = self.blob
    var original = d.get_string_from_utf8()
    var c = d.compress(1)
    self.deflate_smaller = c.size() < d.size()
    self.deflate_ok = c.decompress(d.size(), 1).get_string_from_utf8() == original
    var g = d.compress(3)
    self.gzip_ok = g.decompress(d.size(), 3).get_string_from_utf8() == original
    self.zstd_empty = d.compress(2).size() == 0
    self.fastlz_empty = d.compress(0).size() == 0
    self.small_buffer_empty = c.decompress(2, 1).size() == 0
    self.dynamic_ok = c.decompress_dynamic(-1, 1).get_string_from_utf8() == original
    self.dynamic_capped = c.decompress_dynamic(2, 1).size() == 0
    self.s16 = self.utf16.get_string_from_utf16()
    self.s32 = self.utf32.get_string_from_utf32()
    self.swchar = self.utf32.get_string_from_wchar()
    self.ready_done = true
";

/// UTF-16 LE with a BOM.
fn utf16_le_bom(s: &str) -> Vec<u8> {
    let mut out = vec![0xFF, 0xFE];
    for u in s.encode_utf16() {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out
}

/// UTF-32 LE with a BOM.
fn utf32_le_bom(s: &str) -> Vec<u8> {
    let mut out = vec![0xFF, 0xFE, 0x00, 0x00];
    for c in s.chars() {
        out.extend_from_slice(&(c as u32).to_le_bytes());
    }
    out
}

fn run() -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree
        .add_child(root, Node::new("N", "Node"))
        .expect("node added");
    {
        let n = tree.get_node_mut(node).unwrap();
        n.set_property("blob", Variant::PackedByteArray(TEXT.as_bytes().to_vec()));
        // Non-ASCII on purpose: a decoder that just casts bytes to chars would
        // pass on plain ASCII and mangle this.
        n.set_property("utf16", Variant::PackedByteArray(utf16_le_bom("héllo ✓")));
        n.set_property("utf32", Variant::PackedByteArray(utf32_le_bom("héllo ✓")));
    }
    let script = GDScriptNodeInstance::from_source(SCRIPT, node).expect("script parses");
    tree.attach_script(node, Box::new(script));
    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(node);
    (ml, node)
}

fn var_of(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    use gdscript_interop::bindings::ScriptInstance;
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

#[test]
fn the_script_runs_to_completion() {
    let (ml, node) = run();
    assert_eq!(
        var_of(&ml, node, "ready_done"),
        Variant::Bool(true),
        "`_ready` must finish, or every assertion below reads an initialiser"
    );
}

#[test]
fn deflate_round_trips_through_a_script() {
    let (ml, node) = run();
    assert_eq!(
        var_of(&ml, node, "deflate_ok"),
        Variant::Bool(true),
        "compress(DEFLATE) -> decompress must recover the original bytes"
    );
}

#[test]
fn gzip_round_trips_through_a_script() {
    // GZIP is DEFLATE plus a 10-byte header and a CRC32/ISIZE trailer, so this
    // also exercises the framing and the trailer being excluded from the
    // inflate stream.
    let (ml, node) = run();
    assert_eq!(
        var_of(&ml, node, "gzip_ok"),
        Variant::Bool(true),
        "compress(GZIP) -> decompress must recover the original bytes"
    );
}

#[test]
fn compressed_is_actually_smaller() {
    // ⚠️ The control that stops a pass-through implementation from satisfying
    // the round-trip tests: returning the input unchanged would round-trip
    // perfectly and fail only here.
    let (ml, node) = run();
    assert_eq!(
        var_of(&ml, node, "deflate_smaller"),
        Variant::Bool(true),
        "highly repetitive input must actually shrink under DEFLATE"
    );
}

#[test]
fn unimplemented_modes_report_failure_rather_than_pretending() {
    let (ml, node) = run();
    assert_eq!(
        var_of(&ml, node, "zstd_empty"),
        Variant::Bool(true),
        "ZSTD has no codec here — it must return empty (Godot's failure signal), \
         not the uncompressed input"
    );
    assert_eq!(
        var_of(&ml, node, "fastlz_empty"),
        Variant::Bool(true),
        "FASTLZ is also unimplemented, and it is Godot's DEFAULT mode — a script \
         calling bare compress() gets this path"
    );
}

#[test]
fn a_too_small_buffer_fails_like_godot() {
    let (ml, node) = run();
    assert_eq!(
        var_of(&ml, node, "small_buffer_empty"),
        Variant::Bool(true),
        "decompress() must fail when the result exceeds the caller's buffer size"
    );
}

#[test]
fn decompress_dynamic_grows_but_honours_its_cap() {
    let (ml, node) = run();
    assert_eq!(
        var_of(&ml, node, "dynamic_ok"),
        Variant::Bool(true),
        "decompress_dynamic(-1) is unbounded and must recover the original"
    );
    assert_eq!(
        var_of(&ml, node, "dynamic_capped"),
        Variant::Bool(true),
        "a max_output_size smaller than the result must still fail"
    );
}

#[test]
fn the_wide_decoders_handle_a_bom_and_non_ascii() {
    let (ml, node) = run();
    let want = Variant::String("héllo ✓".to_string());
    assert_eq!(
        var_of(&ml, node, "s16"),
        want,
        "get_string_from_utf16 must strip the BOM and decode non-ASCII"
    );
    assert_eq!(
        var_of(&ml, node, "s32"),
        want,
        "get_string_from_utf32 must strip the BOM and decode non-ASCII"
    );
    assert_eq!(
        var_of(&ml, node, "swchar"),
        want,
        "wchar_t is 32-bit on this target, so get_string_from_wchar decodes \
         UTF-32"
    );
}

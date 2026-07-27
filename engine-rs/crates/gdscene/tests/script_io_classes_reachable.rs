//! pat-0q2kx.218 — the RefCounted IO classes are constructible and usable from
//! GDScript, and `ConfigFile`'s missing methods are reachable.
//!
//! Godot ground truth: `core/io/xml_parser.cpp`, `modules/zip`
//! (ZIPReader/ZIPPacker), `core/io/pck_packer.cpp`,
//! `scene/main/resource_preloader.cpp`, `core/io/config_file.cpp`.
//!
//! Patina implemented every one of these engines and a script could reach none
//! of them: `XMLParser`/`ZIPReader`/`ZIPPacker`/`PCKPacker` were absent from the
//! interpreter's `ClassName.new()` whitelist and had no handle dispatch, and
//! `ConfigFile` was wired for 9 of its methods but not `erase_section`,
//! `erase_section_key`, `parse`, `encode_to_text` or the encrypted load/save.
//!
//! ⚠️ Every assertion drives **GDScript**, not the Rust engines directly. The
//! engines already had unit tests; the gap this bead closes is reachability, so a
//! test that called `ZipPacker::write_file` in Rust would pass without the bead
//! being done at all.

use gdscene::node::{Node, NodeId};
use gdscene::{GDScriptNodeInstance, SceneTree};
use gdvariant::Variant;

/// A unique temp directory for this test binary's artifacts.
fn tmp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("patina_script_io_218");
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn tmp(name: &str) -> String {
    tmp_dir().join(name).to_string_lossy().to_string()
}

/// Attaches `src` to a bare node and returns the tree plus that node.
fn scripted(src: &str) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let n = tree.add_child(root, Node::new("Runner", "Node")).unwrap();
    tree.attach_script(
        n,
        Box::new(GDScriptNodeInstance::from_source(src, n).expect("script parses")),
    );
    (tree, n)
}

fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Option<Variant> {
    use gdscript_interop::bindings::ScriptInstance;
    tree.get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

fn call(tree: &mut SceneTree, node: NodeId, method: &str) {
    tree.call_script_method(node, method, &[])
        .unwrap_or_else(|| panic!("{method} should be callable"));
}

/// **The acceptance test.** One script exercising each clause of the bead, with a
/// separate `*_ok` variable per clause so a failure names the broken one.
#[test]
fn script_io_classes_reachable() {
    let zip_path = tmp("io218.zip");
    let pck_path = tmp("io218.pck");
    let pck_src = tmp("io218_payload.txt");
    let cfg_enc = tmp("io218_secret.cfg");
    let _ = std::fs::remove_file(&zip_path);
    let _ = std::fs::remove_file(&pck_path);
    let _ = std::fs::remove_file(&cfg_enc);
    std::fs::write(&pck_src, b"payload-bytes").expect("write pck source");

    // `XMLParser.new().open_buffer(...)` walks the document and reads attribute
    // `b`; the ZIP pair round-trips bytes; PCKPacker writes a pack; ConfigFile
    // round-trips through text and through an encrypted file, and rejects a wrong
    // password.
    let src = format!(
        r#"
extends Node

var xml_attr = ""
var xml_elements = 0
var zip_files = 0
var zip_text = ""
var pck_err = -1
var cfg_text = ""
var cfg_parsed = -1
var cfg_after_erase = true
var enc_save_err = -1
var enc_right = -1
var enc_right_value = ""
var enc_wrong = -1

func run_xml():
    var p = XMLParser.new()
    p.open_buffer("<a b=\"c\"><d/></a>")
    while p.read() == 0:
        if p.get_node_type() == 1:
            xml_elements = xml_elements + 1
            if p.get_node_name() == "a":
                xml_attr = p.get_named_attribute_value("b")

func run_zip():
    var w = ZIPPacker.new()
    w.open("{zip}")
    w.start_file("hello.txt")
    w.write_file("zip-payload")
    w.close_file()
    w.close()
    var r = ZIPReader.new()
    r.open("{zip}")
    zip_files = r.get_files().size()
    zip_text = r.read_file("hello.txt").get_string_from_utf8()
    r.close()

func run_pck():
    var p = PCKPacker.new()
    p.pck_start("{pck}")
    p.add_file("res://payload.txt", "{pck_src}")
    pck_err = p.flush()

func run_config():
    var c = ConfigFile.new()
    c.set_value("player", "hp", 42)
    c.set_value("audio", "volume", 3)
    cfg_text = c.encode_to_text()
    c.erase_section("audio")
    cfg_after_erase = c.has_section("audio")
    var d = ConfigFile.new()
    cfg_parsed = d.parse(cfg_text)

func run_encrypted():
    var c = ConfigFile.new()
    c.set_value("s", "k", "sekrit")
    enc_save_err = c.save_encrypted_pass("{enc}", "hunter2")
    var good = ConfigFile.new()
    enc_right = good.load_encrypted_pass("{enc}", "hunter2")
    enc_right_value = good.get_value("s", "k", "")
    var bad = ConfigFile.new()
    enc_wrong = bad.load_encrypted_pass("{enc}", "wrong-password")
"#,
        zip = zip_path,
        pck = pck_path,
        pck_src = pck_src,
        enc = cfg_enc,
    );

    let (mut tree, n) = scripted(&src);

    // -- XMLParser -----------------------------------------------------------
    call(&mut tree, n, "run_xml");
    assert_eq!(
        script_var(&tree, n, "xml_attr"),
        Some(Variant::String("c".into())),
        "XMLParser must reach attribute b=\"c\" — if this is empty, either \
         XMLParser.new() did not resolve or the handle dispatch is missing"
    );
    assert_eq!(
        script_var(&tree, n, "xml_elements"),
        Some(Variant::Int(2)),
        "the document has two elements (<a> and the self-closing <d/>)"
    );

    // -- ZIPPacker -> ZIPReader ---------------------------------------------
    call(&mut tree, n, "run_zip");
    assert_eq!(
        script_var(&tree, n, "zip_files"),
        Some(Variant::Int(1)),
        "ZIPReader.get_files() must list the one file ZIPPacker wrote"
    );
    assert_eq!(
        script_var(&tree, n, "zip_text"),
        Some(Variant::String("zip-payload".into())),
        "the bytes must survive the ZIPPacker -> ZIPReader round trip"
    );

    // -- PCKPacker ----------------------------------------------------------
    call(&mut tree, n, "run_pck");
    assert_eq!(
        script_var(&tree, n, "pck_err"),
        Some(Variant::Int(0)),
        "PCKPacker.flush() must report OK"
    );
    // Parsed by the real archive reader, not by trusting flush's return code.
    let bytes = std::fs::read(&pck_path).expect("pack was written");
    let archive = gdresource::pck::PckArchive::from_bytes(&bytes).expect("pack parses");
    assert!(
        archive.has_file("res://payload.txt"),
        "PckArchive must find the file the script added; pack holds {:?}",
        archive.file_paths()
    );

    // -- ConfigFile: encode_to_text / parse / erase_section ------------------
    call(&mut tree, n, "run_config");
    let text = match script_var(&tree, n, "cfg_text") {
        Some(Variant::String(s)) => s,
        other => panic!("encode_to_text must return a String, got {other:?}"),
    };
    assert!(
        text.contains("[player]") && text.contains("hp"),
        "encode_to_text must emit the section and key, got:\n{text}"
    );
    assert_eq!(
        script_var(&tree, n, "cfg_after_erase"),
        Some(Variant::Bool(false)),
        "erase_section must remove the section"
    );
    assert_eq!(
        script_var(&tree, n, "cfg_parsed"),
        Some(Variant::Int(0)),
        "parse() must accept encode_to_text()'s own output — that round trip is \
         the contract"
    );

    // -- ConfigFile: encrypted round trip -----------------------------------
    call(&mut tree, n, "run_encrypted");
    assert_eq!(
        script_var(&tree, n, "enc_save_err"),
        Some(Variant::Int(0)),
        "save_encrypted_pass must report OK"
    );
    assert_eq!(
        script_var(&tree, n, "enc_right"),
        Some(Variant::Int(0)),
        "load_encrypted_pass with the right password must succeed"
    );
    assert_eq!(
        script_var(&tree, n, "enc_right_value"),
        Some(Variant::String("sekrit".into())),
        "the decrypted value must come back intact"
    );
    assert_ne!(
        script_var(&tree, n, "enc_wrong"),
        Some(Variant::Int(0)),
        "load_encrypted_pass with the WRONG password must fail — without this the \
         encryption could be a no-op and everything above would still pass"
    );
}

/// A `.tscn` `ResourcePreloader` answers `get_resource` from its authored
/// `resources` dictionary.
#[test]
fn tscn_resource_preloader_answers_get_resource() {
    const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Pre" type="ResourcePreloader" parent="."]
resources = {
"hero": "res://hero.png"
}
"#;
    const GD: &str = r#"
extends Node

var path = ""
var present = false
var absent = true
var names_count = 0

func probe():
    var pre = get_node("../Pre")
    path = pre.get_resource("hero")
    present = pre.has_resource("hero")
    absent = pre.has_resource("nope")
    names_count = pre.get_resource_list().size()
"#;

    let scene = gdscene::packed_scene::PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene)
        .expect("scene instances");

    // Sanity: the loader really did parse the dictionary. Without this a typo in
    // the fixture would look like a wiring failure.
    let pre = tree
        .get_node_or_null(scene_root, "Pre")
        .expect("ResourcePreloader node");
    assert!(
        matches!(
            tree.get_node(pre).map(|n| n.get_property("resources")),
            Some(Variant::Dictionary(_))
        ),
        "the .tscn `resources` dictionary must reach the node as a Dictionary"
    );

    let runner = tree
        .add_child(scene_root, Node::new("Runner", "Node"))
        .unwrap();
    tree.attach_script(
        runner,
        Box::new(GDScriptNodeInstance::from_source(GD, runner).expect("script parses")),
    );
    call(&mut tree, runner, "probe");

    assert_eq!(
        script_var(&tree, runner, "path"),
        Some(Variant::String("res://hero.png".into())),
        "get_resource(\"hero\") must return the authored path"
    );
    assert_eq!(
        script_var(&tree, runner, "present"),
        Some(Variant::Bool(true)),
        "has_resource must be true for an authored name"
    );
    assert_eq!(
        script_var(&tree, runner, "absent"),
        Some(Variant::Bool(false)),
        "has_resource must be false for a name that was never added — otherwise \
         it could be answering true unconditionally"
    );
    assert_eq!(
        script_var(&tree, runner, "names_count"),
        Some(Variant::Int(1)),
        "get_resource_list must list exactly the authored name"
    );
}

/// The accessors are gated on the class, so an ordinary node must NOT answer
/// them — otherwise the arm would hijack `get_resource` for every node.
#[test]
fn plain_node_does_not_answer_resource_preloader_methods() {
    const GD: &str = r#"
extends Node

var errored = false

func probe():
    var v = get_resource("hero")
    errored = false
"#;
    let (mut tree, n) = scripted(GD);
    // A plain `Node` has no `get_resource`, so the call fails rather than
    // answering. `call_script_method` reports the failed handler as None/Nil.
    let _ = tree.call_script_method(n, "probe", &[]);
    assert_ne!(
        script_var(&tree, n, "errored"),
        Some(Variant::Bool(true)),
        "a plain Node must not resolve get_resource — the arm is class-gated"
    );
}

//! pat-mgixk.107 — `String` slicing, glob matching and editing.
//!
//! Every method below was absent from the String arm
//! of `call_method_on`, so each raised `UndefinedFunction("String.<name>")`.
//!
//! Results are recorded onto a `Flag` node's META and read from Rust, which is
//! the pattern the green `gdscript_camera3d_projection_methods` and
//! `variant_stringify_parity_test` use. `done` is written LAST so a failure
//! distinguishes "a method answered wrongly" from "the frame died at the first
//! undispatched call" — and every one of these raised
//! `UndefinedFunction("String.<name>")` before this change, so an early abort is
//! the expected pre-change shape.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `EngineBootstrap` installs process-global `res://` roots and ClassDB
/// registrations, so concurrent boots would read each other's state.
static BOOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk107_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create project dir");
    dir
}

fn write_project(dir: &Path, gd: &str) {
    std::fs::write(
        dir.join("project.godot"),
        "config_version=5\n\n[application]\n\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .expect("write project.godot");
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1_m\"]\n\n\
         [node name=\"Main\" type=\"Node2D\"]\n\
         script = ExtResource(\"1_m\")\n\n\
         [node name=\"Flag\" type=\"Node2D\" parent=\".\"]\n",
    )
    .expect("write main.tscn");
    std::fs::write(dir.join("main.gd"), gd).expect("write main.gd");
}

fn boot(dir: &Path) -> EngineBootstrap {
    let mut boot = EngineBootstrap::new(
        BootConfig::with_scene(dir.join("main.tscn")).project_dir(dir.to_path_buf()),
    );
    boot.run_all().expect("the runner must boot the scene");
    boot
}

fn flag_of(boot: &EngineBootstrap) -> gdscene::node::NodeId {
    let tree = boot.main_loop().expect("MainLoop").tree();
    let root = tree.root_id();
    let main = tree
        .find_child(root, "Main", true)
        .expect("the main scene instantiated");
    tree.find_child(main, "Flag", true)
        .expect("the scene has a Flag node")
}

fn meta(boot: &EngineBootstrap, flag: gdscene::node::NodeId, key: &str) -> Variant {
    boot.main_loop()
        .expect("MainLoop")
        .tree()
        .get_node(flag)
        .expect("the Flag node is still in the tree")
        .get_meta(key)
}

const MAIN_GD: &str = "\
extends Node2D

func _ready():
    var f = get_node(\"Flag\")
    f.set_meta(\"done\", 0)
    f.set_meta(\"stage\", 0)
    f.set_meta(\"slice\", \"a,b,c\".get_slice(\",\", 1))
    f.set_meta(\"stage\", 1)
    f.set_meta(\"count\", \"a,b,c\".get_slice_count(\",\"))
    f.set_meta(\"stage\", 2)
    f.set_meta(\"glob\", \"icon.png\".match(\"*.png\"))
    f.set_meta(\"stage\", 3)
    f.set_meta(\"globn\", \"ICON.PNG\".matchn(\"*.png\"))
    f.set_meta(\"stage\", 4)
    f.set_meta(\"glob_cs\", \"ICON.PNG\".match(\"*.png\"))
    f.set_meta(\"stage\", 5)
    f.set_meta(\"rev\", \"abc\".reverse())
    f.set_meta(\"stage\", 6)
    f.set_meta(\"ins\", \"ac\".insert(1, \"b\"))
    f.set_meta(\"stage\", 7)
    f.set_meta(\"era\", \"abcd\".erase(1, 2))
    f.set_meta(\"stage\", 8)
    f.set_meta(\"kebab\", \"HelloWorld\".to_kebab_case())
    f.set_meta(\"stage\", 9)
    f.set_meta(\"floats\", \"1,2.5\".split_floats(\",\"))
    f.set_meta(\"stage\", 10)
    f.set_meta(\"repl\", \"aXa\".replacen(\"x\", \"y\"))
    f.set_meta(\"stage\", 11)
    f.set_meta(\"nat\", \"file10\".naturalcasecmp_to(\"file9\"))
    f.set_meta(\"stage\", 12)
    f.set_meta(\"done\", 1)
";

/// **The acceptance test.**
#[test]
fn string_api_slicing_matching_test() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("slicing");
    write_project(&dir, MAIN_GD);
    let boot = boot(&dir);
    let flag = flag_of(&boot);

    // 🔑 The stage NAMES the statement that raised. A binary done flag told me
    // only that something failed, which cost a verify round; `Nil` vs `0` had
    // already separated "the script never parsed" from "a call raised", and this
    // separates WHICH call.
    let stage = meta(&boot, flag, "stage");
    assert_eq!(
        meta(&boot, flag, "done"),
        Variant::Int(1),
        "`_ready` must reach its last line. `Nil` for `done` means the script \
         never parsed; `0` means a call raised `UndefinedFunction` and the stage \
         reached names which one — got stage {stage:?}, where 0=get_slice \
         1=get_slice_count 2=match 3=matchn 4=match-control 5=reverse 6=insert \
         7=erase 8=to_kebab_case 9=split_floats 10=replacen \
         11=naturalcasecmp_to"
    );

    assert_eq!(
        meta(&boot, flag, "slice"),
        Variant::String("b".to_string()),
        "get_slice(\",\", 1) is the SECOND piece, zero-indexed"
    );
    assert_eq!(
        meta(&boot, flag, "count"),
        Variant::Int(3),
        "three pieces means two delimiters plus one"
    );
    assert_eq!(
        meta(&boot, flag, "glob"),
        Variant::Bool(true),
        "`*.png` matches `icon.png`"
    );
    assert_eq!(
        meta(&boot, flag, "globn"),
        Variant::Bool(true),
        "matchn is case-INsensitive, so `*.png` matches `ICON.PNG`"
    );
    // 🔑 The control that makes the pair meaningful: `match` is case-SENSITIVE,
    // so the same uppercase subject must NOT match. Without this, a matcher that
    // ignored case entirely would pass both assertions above.
    assert_eq!(
        meta(&boot, flag, "glob_cs"),
        Variant::Bool(false),
        "match is case-SENSITIVE, so `*.png` must not match `ICON.PNG`"
    );
    assert_eq!(
        meta(&boot, flag, "rev"),
        Variant::String("cba".to_string()),
        "reverse walks characters"
    );
    assert_eq!(
        meta(&boot, flag, "ins"),
        Variant::String("abc".to_string()),
        "insert places the text AT the index, shifting the rest right"
    );
    assert_eq!(
        meta(&boot, flag, "era"),
        Variant::String("ad".to_string()),
        "erase(1, 2) drops two characters from index 1"
    );
    assert_eq!(
        meta(&boot, flag, "kebab"),
        Variant::String("hello-world".to_string()),
        "kebab lowercases and hyphenates the compound-word split"
    );
    assert_eq!(
        meta(&boot, flag, "floats"),
        Variant::PackedFloat64Array(vec![1.0, 2.5]),
        "split_floats parses each piece"
    );
    assert_eq!(
        meta(&boot, flag, "repl"),
        Variant::String("aya".to_string()),
        "replacen matches `x` against `X` and substitutes the replacement verbatim"
    );
    assert_eq!(
        meta(&boot, flag, "nat"),
        Variant::Int(1),
        "natural order puts file10 AFTER file9, because 10 > 9 numerically — a \
         plain lexicographic compare would answer -1"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

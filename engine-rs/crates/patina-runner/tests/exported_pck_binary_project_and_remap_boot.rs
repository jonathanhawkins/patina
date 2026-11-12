//! pat-mgixk.56 acceptance (part 1a) — a real exported `.pck` boots from
//! `res://project.binary`.
//!
//! A Godot export does **not** put `project.godot` in the pack. It writes the
//! binary ProjectSettings form instead
//! (`editor/export/editor_export_platform.cpp:1676`), and the engine loads that
//! in preference to the text file, falling back only when it is absent
//! (`core/config/project_settings.cpp:993-1002`).
//!
//! `boot_project_from_pck` required `res://project.godot` (project_boot.rs:81),
//! so only a pack assembled by hand from a loose project could start. **A
//! genuinely exported `.pck` failed before any scene was touched** — which is
//! every `.pck` a user would actually have.
//!
//! # The fixture is an independent oracle
//!
//! 🔑 The ECFG bytes here are assembled by hand from Godot's own reader
//! (`_load_settings_binary`, `project_settings.cpp:903`) and the `marshalls.cpp`
//! variant layout — **not** with `gdvariant::encode_variant`. Encoding the
//! fixture with our own encoder would make this test prove that our encoder and
//! our decoder agree with each other, which is true of any pair of mutually
//! wrong implementations. Writing the bytes from the C++ reader's field order is
//! what makes a mismatch with Godot show up here.
//!
//! ⚠️ The key in an ECFG entry is **not** padded to 4 bytes, while a `String`
//! *inside* a marshalled variant is (`_encode_string`). Getting that backwards
//! desynchronises every later entry, so `ecfg_entry` and `variant_string` below
//! deliberately differ on exactly that point.

use std::path::PathBuf;

use gdresource::pck::PckPacker;
use patina_runner::project_boot::boot_project_from_pck;

/// `res://` roots and pack mounts are process-global, so these must not
/// interleave. Same reasoning (and same shape) as `runner_boots_godot_pck`.
static BOOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn temp_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("patina_ecfg_{tag}_{}_{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn reset() {
    gdcore::file_access::clear_mounted_packs();
}

// ---------------------------------------------------------------------------
// Hand-built ECFG, from the C++ reader's field order
// ---------------------------------------------------------------------------

/// `Variant::Type` ordinals from `core/variant/variant.h`.
const T_INT: u32 = 2;
const T_STRING: u32 = 4;

fn u32le(v: u32, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// A marshalled `String`: header, then `u32 len + bytes + NUL padding to 4`
/// (`marshalls.cpp` `_encode_string`).
fn variant_string(s: &str) -> Vec<u8> {
    let mut v = Vec::new();
    u32le(T_STRING, &mut v);
    u32le(s.len() as u32, &mut v);
    v.extend_from_slice(s.as_bytes());
    let pad = (4 - s.len() % 4) % 4;
    v.resize(v.len() + pad, 0);
    v
}

/// A marshalled `int` that fits in 32 bits, so the header carries **no**
/// `HEADER_DATA_FLAG_64` and the payload is 4 bytes (`marshalls.cpp:1370`).
fn variant_int(i: i32) -> Vec<u8> {
    let mut v = Vec::new();
    u32le(T_INT, &mut v);
    u32le(i as u32, &mut v);
    v
}

/// One `project.binary` entry: `u32 key_len`, the key's **unpadded** UTF-8
/// bytes, `u32 value_len`, then the marshalled value.
fn ecfg_entry(key: &str, value: &[u8], out: &mut Vec<u8>) {
    u32le(key.len() as u32, out);
    out.extend_from_slice(key.as_bytes());
    u32le(value.len() as u32, out);
    out.extend_from_slice(value);
}

/// The whole file: `"ECFG"`, `u32 count`, then the entries.
fn project_binary(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"ECFG".to_vec();
    u32le(entries.len() as u32, &mut out);
    for (k, v) in entries {
        ecfg_entry(k, v, &mut out);
    }
    out
}

/// Godot's own default for `display/window/size/viewport_width`.
///
/// 🔑 The fixture deliberately does **not** use this. `viewport_width()` falls
/// back to 1152 when the setting is absent, so a fixture that stored 1152 would
/// pass with `project.binary` never read at all.
const VIEWPORT_WIDTH_DEFAULT: i64 = 1152;
/// What the fixture stores instead.
const FIXTURE_VIEWPORT_WIDTH: i64 = 1440;

/// Keys chosen to span both value types the acceptance names: a `String`
/// (`config/name`, `run/main_scene`) and an `int` (`viewport_width`).
fn exported_project_binary() -> Vec<u8> {
    project_binary(&[
        ("application/config/name", variant_string("Exported Game")),
        (
            "application/run/main_scene",
            variant_string("res://main.tscn"),
        ),
        (
            "display/window/size/viewport_width",
            variant_int(FIXTURE_VIEWPORT_WIDTH as i32),
        ),
    ])
}

const MAIN_TSCN: &str = r#"[gd_scene format=3]

[node name="Main" type="Node2D"]

[node name="Art" type="Sprite2D" parent="."]
"#;

// ---------------------------------------------------------------------------
// The acceptance test
// ---------------------------------------------------------------------------

/// **The acceptance test (pat-mgixk.56 part 1a).**
///
/// Packs `project.binary` and a main scene and *nothing else* — in particular no
/// `project.godot` anywhere, on disk or in the pack — then boots it.
#[test]
fn exported_pck_binary_project_and_remap_boot() {
    let _lock = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset();

    let dir = temp_dir("boot");
    let pck_path = dir.join("game.pck");

    let mut packer = PckPacker::new();
    packer.add_file("res://project.binary", exported_project_binary());
    packer.add_file("res://main.tscn", MAIN_TSCN.as_bytes().to_vec());
    let bytes = packer.pack().expect("pack");

    // The pack must not contain project.godot — otherwise the fallback could
    // satisfy every assertion below and the binary path would be untested.
    assert!(
        !String::from_utf8_lossy(&bytes).contains("project.godot"),
        "the fixture pack must not contain project.godot in any form"
    );
    std::fs::write(&pck_path, &bytes).expect("write pck");
    assert!(
        !dir.join("project.godot").exists(),
        "no loose project.godot beside the pack either"
    );

    let booted = boot_project_from_pck(&pck_path, None)
        .expect("an exported .pck with only project.binary must boot");

    // -- the settings came out of project.binary -----------------------------
    assert_eq!(
        booted.settings.project_name(),
        "Exported Game",
        "application/config/name must be decoded from project.binary"
    );

    // 🔑 The int-valued clause the acceptance names. Asserted against the
    // default as well as the expected value, so this cannot pass on a boot that
    // never read the file.
    assert_ne!(
        FIXTURE_VIEWPORT_WIDTH, VIEWPORT_WIDTH_DEFAULT,
        "the fixture width must differ from the fallback or this test is vacuous"
    );
    assert_eq!(
        booted.settings.viewport_width(),
        FIXTURE_VIEWPORT_WIDTH,
        "display/window/size/viewport_width must come from project.binary; \
         {VIEWPORT_WIDTH_DEFAULT} would mean the setting was never read and the \
         accessor fell back to Godot's default"
    );

    // -- and run/main_scene from the same file actually instanced ------------
    let main = booted
        .main_scene_root
        .expect("run/main_scene, decoded from project.binary, must instance");
    assert_eq!(
        booted.tree.get_node(main).map(|n| n.name()),
        Some("Main"),
        "the scene named by project.binary's run/main_scene is the one that loaded"
    );
    assert!(
        !booted
            .tree
            .find_children(main, "*", Some("Sprite2D"), true)
            .is_empty(),
        "the main scene's children instanced, so the scene was read through res:// \
         with only the pack backing it"
    );

    reset();
}

/// Godot prefers the binary file when both are present
/// (`_load_settings_text_or_binary` tries binary first and only falls back on
/// `ERR_FILE_NOT_FOUND`).
///
/// 🔑 This is the half that proves a *preference* rather than a mere fallback.
/// The two files disagree on `config/name`, so whichever won is named in the
/// result — a loader that read `project.godot` first would still boot, and the
/// test above could not tell.
#[test]
fn project_binary_wins_over_project_godot_when_both_are_packed() {
    let _lock = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset();

    let dir = temp_dir("both");
    let pck_path = dir.join("game.pck");

    let mut packer = PckPacker::new();
    packer.add_file("res://project.binary", exported_project_binary());
    packer.add_file(
        "res://project.godot",
        br#"config_version=5

[application]

config/name="Text Fallback"
run/main_scene="res://main.tscn"
"#
        .to_vec(),
    );
    packer.add_file("res://main.tscn", MAIN_TSCN.as_bytes().to_vec());
    std::fs::write(&pck_path, packer.pack().expect("pack")).expect("write pck");

    let booted = boot_project_from_pck(&pck_path, None).expect("boot");
    assert_eq!(
        booted.settings.project_name(),
        "Exported Game",
        "project.binary must take precedence over project.godot, as Godot does; \
         \"Text Fallback\" means the text file won"
    );

    reset();
}

/// A pack with neither file must say so, naming both — the previous message
/// mentioned only `project.godot`, which is the file an exported pack correctly
/// does not have.
#[test]
fn a_pack_with_no_settings_names_both_candidates() {
    let _lock = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset();

    let dir = temp_dir("neither");
    let pck_path = dir.join("game.pck");
    let mut packer = PckPacker::new();
    packer.add_file("res://main.tscn", MAIN_TSCN.as_bytes().to_vec());
    std::fs::write(&pck_path, packer.pack().expect("pack")).expect("write pck");

    // Matched rather than `expect_err`: that helper needs `T: Debug` to print
    // the unexpected Ok, and `BootedProject` does not implement it.
    let err = match boot_project_from_pck(&pck_path, None) {
        Ok(_) => panic!("a pack with no settings file must not boot"),
        Err(e) => e,
    };
    assert!(
        err.contains("project.binary") && err.contains("project.godot"),
        "the error must name both candidates so the reader knows which to add; got: {err}"
    );

    reset();
}

// ---------------------------------------------------------------------------
// `<path>.remap` redirects (part 1b)
// ---------------------------------------------------------------------------

/// Where an export actually puts the converted scene.
const EXPORTED_SCENE_PATH: &str = "res://.godot/exported/133200997/export-7f3a1b-main.tscn";

/// A **text** `project.godot` for the redirect tests.
///
/// 🔑 Deliberately not `project.binary`. With both halves of this bead reverted,
/// a `project.binary`-only pack cannot boot at all, so a redirect test built on
/// one would fail for the *settings* reason and prove nothing about `.remap`.
/// Booting from text — which works at HEAD — leaves the redirect as the single
/// thing these two tests can fail on.
const PROJECT_GODOT_TEXT: &[u8] = br#"config_version=5

[application]

config/name="Exported Game"
run/main_scene="res://main.tscn"
"#;

/// The redirect file an export leaves in place of `main.tscn`.
fn main_tscn_remap() -> Vec<u8> {
    format!("[remap]\n\npath=\"{EXPORTED_SCENE_PATH}\"\n").into_bytes()
}

/// A different root name from `MAIN_TSCN`, so whichever scene loaded is named in
/// the assertion rather than inferred.
const REMAPPED_TSCN: &str = r#"[gd_scene format=3]

[node name="Remapped" type="Node2D"]

[node name="Art" type="Sprite2D" parent="."]
"#;

/// **The `.remap` clause.** An export with `convert_text_resources_to_binary`
/// (the default) replaces `main.tscn` with `main.tscn.remap`.
///
/// 🔑 Both scenes are packed, with **different root names**, so this proves a
/// *precedence* and not merely a fallback: if the redirect were ignored,
/// `res://main.tscn` is right there and readable, and the root would be "Main".
/// Packing only the exported scene would leave "the redirect was followed"
/// indistinguishable from "the original was missing and the loader guessed".
#[test]
fn main_scene_loads_through_a_remap_redirect() {
    let _lock = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset();

    let dir = temp_dir("remap");
    let pck_path = dir.join("game.pck");

    let mut packer = PckPacker::new();
    packer.add_file("res://project.godot", PROJECT_GODOT_TEXT.to_vec());
    packer.add_file("res://main.tscn.remap", main_tscn_remap());
    packer.add_file(EXPORTED_SCENE_PATH, REMAPPED_TSCN.as_bytes().to_vec());
    // The decoy: readable, and the path `run/main_scene` literally names.
    packer.add_file("res://main.tscn", MAIN_TSCN.as_bytes().to_vec());
    std::fs::write(&pck_path, packer.pack().expect("pack")).expect("write pck");

    let booted = boot_project_from_pck(&pck_path, None).expect("boot");
    let main = booted
        .main_scene_root
        .expect("the main scene must instance");
    assert_eq!(
        booted.tree.get_node(main).map(|n| n.name()),
        Some("Remapped"),
        "run/main_scene names res://main.tscn, but main.tscn.remap redirects to \
         {EXPORTED_SCENE_PATH}. A root of \"Main\" means the redirect was not \
         followed and the un-exported decoy loaded instead"
    );

    reset();
}

/// A `.remap` that exists but assigns no `path` falls through to the original.
///
/// Godot's loop breaks at EOF without having seen an assignment and returns the
/// path unchanged (`resource_loader.cpp:1300-1319`), so a malformed redirect
/// must not make a loadable scene unloadable.
#[test]
fn a_remap_with_no_path_entry_falls_back_to_the_original() {
    let _lock = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset();

    let dir = temp_dir("remap_empty");
    let pck_path = dir.join("game.pck");

    let mut packer = PckPacker::new();
    packer.add_file("res://project.godot", PROJECT_GODOT_TEXT.to_vec());
    packer.add_file("res://main.tscn", MAIN_TSCN.as_bytes().to_vec());
    packer.add_file("res://main.tscn.remap", b"[remap]\n\n".to_vec());
    std::fs::write(&pck_path, packer.pack().expect("pack")).expect("write pck");

    let booted = boot_project_from_pck(&pck_path, None).expect("boot");
    let main = booted
        .main_scene_root
        .expect("the main scene must still instance");
    assert_eq!(
        booted.tree.get_node(main).map(|n| n.name()),
        Some("Main"),
        "a pathless .remap must leave the original path alone"
    );

    reset();
}

// ---------------------------------------------------------------------------
// Pack-aware `.import` artifact resolution (part 1c)
// ---------------------------------------------------------------------------

/// Where an export puts the compiled artifact for `res://icon.png`.
const IMPORTED_ARTIFACT: &str = "res://.godot/imported/icon-7f3a1b.png";

/// The repo's own 8x8 solid-opaque-red RGBA PNG.
///
/// Reused rather than hand-built: it is already proven decodable by the runner's
/// 2D frame tests, so a failure here cannot be the fixture's fault.
fn red_png_bytes() -> Vec<u8> {
    let path = format!(
        "{}/../../fixtures/scenes/sprite_fixture.png",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {path}: {e}"))
}

/// A Sprite2D whose texture is an `ext_resource` on `res://icon.png`.
///
/// `centered = false` with `position = (10, 10)` puts its 8x8 at `(10..18,
/// 10..18)` of the frame, well inside a 64x64 buffer. No `Camera2D`, so no view
/// transform is applied and the sprite stays at its world position.
const SPRITE_SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Texture2D" path="res://icon.png" id="1_icon"]

[node name="Main" type="Node2D"]

[node name="Art" type="Sprite2D" parent="."]
position = Vector2(10, 10)
centered = false
texture = ExtResource("1_icon")
"#;

/// Counts opaque-red pixels in a written PNG frame.
fn red_pixels(path: &std::path::Path) -> usize {
    let tex = gdrender2d::texture::load_png(&path.to_string_lossy()).unwrap_or_else(|| {
        panic!(
            "the runner must write a decodable PNG at {}",
            path.display()
        )
    });
    tex.pixels
        .iter()
        .filter(|c| c.r > 0.5 && c.g < 0.3 && c.b < 0.3)
        .count()
}

/// Packs a project whose only source of texture pixels is the `.import`
/// redirect, optionally omitting the sidecar to get a negative control.
fn pack_import_project(with_sidecar: bool) -> Vec<u8> {
    let mut packer = PckPacker::new();
    // Text settings, not `project.binary`: with every part of this bead reverted
    // a binary-only pack cannot boot at all, so these two tests would fail for
    // the SETTINGS reason and prove nothing about `.import`. Booting from text —
    // which works at HEAD — leaves the sidecar as the only thing they measure.
    packer.add_file("res://project.godot", PROJECT_GODOT_TEXT.to_vec());
    packer.add_file("res://main.tscn", SPRITE_SCENE.as_bytes().to_vec());
    // 🔑 `res://icon.png` is deliberately NOT packed. An export replaces the
    // source with its compiled artifact, so the redirect is the only route to
    // pixels — which is what makes this test about `.import` and not about
    // texture loading in general.
    packer.add_file(IMPORTED_ARTIFACT, red_png_bytes());
    if with_sidecar {
        packer.add_file(
            "res://icon.png.import",
            format!("[remap]\n\npath=\"{IMPORTED_ARTIFACT}\"\n").into_bytes(),
        );
    }
    packer.pack().expect("pack")
}

/// Renders a frame from `pck` through the real binary and returns the red count.
fn render_and_count_red(tag: &str, pck: Vec<u8>) -> usize {
    let dir = temp_dir(tag);
    let pck_path = dir.join("game.pck");
    std::fs::write(&pck_path, pck).expect("write pck");
    let out = dir.join("frame.png");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_patina-runner"))
        .arg(&pck_path)
        .arg("--frames")
        .arg("2")
        .arg("--frame-out")
        .arg(&out)
        .arg("--frame-size")
        .arg("64x64")
        .output()
        .expect("run patina-runner");
    assert!(
        output.status.success(),
        "patina-runner failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    red_pixels(&out)
}

/// **The `.import` clause.** A Sprite2D's `ext_resource` resolves through the
/// packed `.import` sidecar to a decoded texture.
///
/// Drives the real binary, so this is the whole chain the wiring requirement
/// names: `patina-runner game.pck` -> `boot_project_from_pck` -> settings from
/// `project.binary` -> main scene -> `TextureCache` -> written pixels.
///
/// ⚠️ The artifact here is a PNG rather than a `.ctex`. The mechanism under test
/// is "the sidecar inside a pack is found and its artifact is read from the
/// pack"; the container format is dispatched on separately and `.ctex` decoding
/// already has its own tests. Hand-building a valid `.ctex` would add a second
/// thing that could fail without testing anything more about this path.
#[test]
fn sprite_texture_resolves_through_a_packed_import_sidecar() {
    let _lock = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset();

    let red = render_and_count_red("import", pack_import_project(true));
    assert!(
        red >= 32,
        "the Sprite2D's texture must resolve through res://icon.png.import to \
         {IMPORTED_ARTIFACT} and reach the frame: found {red} red pixels for an \
         8x8 opaque-red texture. Zero means the packed sidecar was never read — \
         `TextureCache` resolves res:// to a HOST path, which inside a pack does \
         not exist."
    );

    reset();
}

/// The negative control: without the sidecar there is no route to the pixels.
///
/// 🔑 This is what proves the pixels above came from the redirect rather than
/// from some other fallback finding the artifact on its own. The two packs are
/// identical except for the `.import` file.
#[test]
fn without_the_import_sidecar_the_sprite_draws_nothing() {
    let _lock = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset();

    let red = render_and_count_red("no_import", pack_import_project(false));
    assert_eq!(
        red, 0,
        "with no .import sidecar and no res://icon.png, nothing should resolve \
         the texture; {red} red pixels means the artifact was found by some other \
         route and the test above proves less than it claims"
    );

    reset();
}

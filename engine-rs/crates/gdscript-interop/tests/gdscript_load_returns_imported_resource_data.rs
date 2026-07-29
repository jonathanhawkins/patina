//! pat-s3rrv.4 — GDScript `load()`/`preload()` route through the registered
//! resource format loaders and `.import` remaps.
//!
//! `gdresource`'s pipeline already followed `.import` sidecars to their
//! compiled `.godot/imported/*.ctex` artifact and decoded real pixels
//! (`gdresource/tests/runtime_load_follows_import_pipeline.rs` pins that), but
//! **the interpreter never called it**: `interpreter.rs`'s `"preload" | "load"`
//! arm string-matched the extension (`.png` -> `"Texture2D"`, `.wav` ->
//! `"AudioStream"`) and returned a `ResourceRef` with `properties:
//! HashMap::new()`. So `load("res://icon.png").width` was Nil, an imported
//! project whose source PNG had been removed resolved to nothing at all, and
//! `AudioStreamPlayer.stream = load("res://sfx.wav")` saw an inert object.
//!
//! These tests drive the real interpreter dispatch (`run_class` ->
//! `instantiate_class` -> `call_instance_method`, the path the runner takes to
//! fire `_ready`/`_process`) so they prove the *script-visible* result, not the
//! loader in isolation.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// `res://` roots are process-global, so these tests must not point them at
/// their own temp dirs concurrently. Each test holds this for its duration.
static ROOT_LOCK: Mutex<()> = Mutex::new(());

fn lock_roots() -> MutexGuard<'static, ()> {
    ROOT_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

/// Builds a minimal `GST2` `.ctex` wrapping raw RGBA8 pixels
/// (`DATA_FORMAT_IMAGE`), mirroring `gdresource::ctex`'s on-disk layout. Same
/// helper shape as `gdresource/tests/runtime_load_follows_import_pipeline.rs`.
fn build_ctex_image_rgba8(width: u32, height: u32, raw: &[u8]) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(b"GST2");
    d.extend_from_slice(&1u32.to_le_bytes());
    d.extend_from_slice(&width.to_le_bytes());
    d.extend_from_slice(&height.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&[0u8; 12]);
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&(width as u16).to_le_bytes());
    d.extend_from_slice(&(height as u16).to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&5u32.to_le_bytes());
    d.extend_from_slice(raw);
    d
}

/// Builds a real 8-bit mono RIFF/WAVE file of `frames` samples at 8 kHz, so the
/// decoded `length_seconds` is `frames / 8000.0`.
fn build_wav_8bit_mono(sample_rate: u32, frames: u32) -> Vec<u8> {
    let data_size = frames; // 1 channel * 8 bits = 1 byte per frame
    let mut d = Vec::new();
    d.extend_from_slice(b"RIFF");
    d.extend_from_slice(&(36 + data_size).to_le_bytes());
    d.extend_from_slice(b"WAVE");
    d.extend_from_slice(b"fmt ");
    d.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    d.extend_from_slice(&1u16.to_le_bytes()); // audio_format = PCM
    d.extend_from_slice(&1u16.to_le_bytes()); // channels
    d.extend_from_slice(&sample_rate.to_le_bytes());
    d.extend_from_slice(&sample_rate.to_le_bytes()); // byte_rate
    d.extend_from_slice(&1u16.to_le_bytes()); // block_align
    d.extend_from_slice(&8u16.to_le_bytes()); // bits_per_sample
    d.extend_from_slice(b"data");
    d.extend_from_slice(&data_size.to_le_bytes());
    d.extend(std::iter::repeat(128u8).take(data_size as usize));
    d
}

/// Lays out an *imported* project: the `.import` sidecar and the compiled
/// artifact exist, and **the source `icon.png` is absent** — the state a
/// shipped/exported project is in. A loader that reads the source instead of
/// following the remap finds nothing here.
fn write_imported_project(root: &Path) {
    std::fs::write(root.join("project.godot"), b"[application]\n").expect("project.godot");
    std::fs::write(
        root.join("icon.png.import"),
        b"[remap]\nimporter=\"texture\"\ntype=\"CompressedTexture2D\"\npath=\"res://.godot/imported/icon.ctex\"\n",
    )
    .expect("sidecar");

    let imported = root.join(".godot/imported");
    std::fs::create_dir_all(&imported).expect("imported dir");
    // 2x2 RGBA8: red, green, blue, white.
    let raw: [u8; 16] = [
        255, 0, 0, 255, //
        0, 255, 0, 255, //
        0, 0, 255, 255, //
        255, 255, 255, 255,
    ];
    std::fs::write(
        imported.join("icon.ctex"),
        build_ctex_image_rgba8(2, 2, &raw),
    )
    .expect("ctex artifact");

    std::fs::write(root.join("sfx.wav"), build_wav_8bit_mono(8000, 4000)).expect("wav");

    assert!(
        !root.join("icon.png").exists(),
        "the source PNG must stay deleted — the artifact is the only data source",
    );
    gdcore::file_access::set_path_roots(Some(root.to_path_buf()), None);
}

/// Parse `src` as a class, instantiate it, and call `method` with no args
/// through the real interpreter dispatch.
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

/// **The bead's acceptance test.** A script loading a source path whose only
/// on-disk data is the compiled import artifact gets the artifact's true
/// dimensions and a populated pixel array; a `.wav` decodes to an
/// `AudioStreamWAV` carrying a real duration.
#[test]
fn gdscript_load_returns_imported_resource_data() {
    let _guard = lock_roots();
    let tmp = tempfile::tempdir().expect("tempdir");
    write_imported_project(tmp.path());

    let src = "\
extends Node
func check() -> Array:
    var t = load(\"res://icon.png\")
    var s = load(\"res://sfx.wav\")
    return [t.width, t.height, t.pixels, t.get_class(), \
s.get_class(), s.length_seconds, s.sample_rate]
";
    let got = match call_method(src, "check") {
        Variant::Array(items) => items,
        other => panic!("expected Array, got {other:?}"),
    };

    // -- the remapped texture ------------------------------------------------
    assert_eq!(
        got[0],
        Variant::Int(2),
        "width must come from the .ctex artifact, not be Nil from the extension guess",
    );
    assert_eq!(
        got[1],
        Variant::Int(2),
        "height must come from the artifact"
    );
    let pixels = match &got[2] {
        Variant::Array(px) => px,
        other => panic!("pixels must be an Array of decoded colors, got {other:?}"),
    };
    assert_eq!(
        pixels.len(),
        4,
        "a 2x2 artifact must decode to 4 pixels, got {}",
        pixels.len(),
    );
    match &pixels[0] {
        Variant::Color(c) => assert!(
            c.r > 0.9 && c.g < 0.1 && c.b < 0.1,
            "the artifact's first pixel is red, got {c:?}",
        ),
        other => panic!("expected a Color pixel, got {other:?}"),
    }
    assert_eq!(
        got[3],
        Variant::String("Texture2D".to_string()),
        "an imported image must still report Texture2D",
    );

    // -- the audio stream ----------------------------------------------------
    assert_eq!(
        got[4],
        Variant::String("AudioStreamWAV".to_string()),
        "a .wav must report the decoder's concrete class, not the bare 'AudioStream' guess",
    );
    assert_eq!(
        got[5],
        Variant::Float(0.5),
        "4000 frames at 8 kHz is a half-second stream; Nil here means the file was never read",
    );
    assert_eq!(
        got[6],
        Variant::Int(8000),
        "sample_rate must come from the WAV fmt chunk",
    );
}

/// `preload()` shares the builtin with `load()`, so it must follow the remap
/// too — a script preloading an imported texture at parse time sees pixels.
#[test]
fn gdscript_preload_follows_the_import_remap() {
    let _guard = lock_roots();
    let tmp = tempfile::tempdir().expect("tempdir");
    write_imported_project(tmp.path());

    let src = "\
extends Node
func check():
    return preload(\"res://icon.png\").height
";
    assert_eq!(
        call_method(src, "check"),
        Variant::Int(2),
        "preload() must decode through the format loaders like load()",
    );
}

/// A path no format loader claims must keep degrading to the extension-guess
/// placeholder rather than erroring — that fallback is what stops unregistered
/// types (and missing files) from aborting a running script.
#[test]
fn gdscript_load_unloadable_path_keeps_the_placeholder() {
    let _guard = lock_roots();
    let tmp = tempfile::tempdir().expect("tempdir");
    write_imported_project(tmp.path());

    let src = "\
extends Node
func check() -> Array:
    var missing = load(\"res://gone.png\")
    var unknown = load(\"res://notes.xyz\")
    return [missing.get_class(), missing.get_path(), unknown.get_class(), missing.width]
";
    let got = match call_method(src, "check") {
        Variant::Array(items) => items,
        other => panic!("expected Array, got {other:?}"),
    };
    assert_eq!(
        got[0],
        Variant::String("Texture2D".to_string()),
        "an undecodable .png keeps its extension-guessed class",
    );
    assert_eq!(
        got[1],
        Variant::String("res://gone.png".to_string()),
        "the placeholder still carries the requested path",
    );
    assert_eq!(
        got[2],
        Variant::String("Resource".to_string()),
        "an unclaimed extension falls back to the generic Resource",
    );
    assert_eq!(
        got[3],
        Variant::Nil,
        "no data means no properties, not a runtime error",
    );
}

/// Scenes must stay on the placeholder path: instancing reads `PackedScene`
/// back off the class name, while the tscn loader reports the *root node's*
/// type. Routing `.tscn` through the format loaders would silently break
/// `load("res://x.tscn").instantiate()`.
#[test]
fn gdscript_load_scene_still_reports_packed_scene() {
    let _guard = lock_roots();
    let tmp = tempfile::tempdir().expect("tempdir");
    write_imported_project(tmp.path());
    std::fs::write(
        tmp.path().join("level.tscn"),
        b"[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\n",
    )
    .expect("write scene");

    let src = "\
extends Node
func check():
    return load(\"res://level.tscn\").get_class()
";
    assert_eq!(
        call_method(src, "check"),
        Variant::String("PackedScene".to_string()),
        "a .tscn must keep reporting PackedScene, not its root node's type",
    );
}

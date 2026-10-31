//! pat-mgixk.96 — the method surface of a `Variant::Resource` property bag.
//!
//! Every RefCounted class a script constructs becomes a `ResourceRef` property
//! bag seeded from the ClassDB property list, so its *properties* have always
//! worked while every one of its *methods* fell through to the generic
//! `UndefinedFunction("Resource.<m>")`. Measured through the prebuilt runner
//! before the fix, on a loaded project with an attached GDScript, each of these
//! six families **aborted the frame on its first call** — six "before" prints
//! and not one "after":
//!
//! ```text
//! FastNoiseLite.new().get_noise_2d(1.5, 2.5)        -> raises
//! Shader.new().set_code(..)                          -> raises
//! BitMap.new().create(Vector2i(4, 4))                -> raises
//! MultiMesh.new().set_instance_transform(1, ..)      -> raises
//! Skin.new().add_bind(3, ..)                         -> raises
//! Translation.new().add_message("FILE", "Fichier")   -> raises
//! ```
//!
//! # Why the recording goes through node metadata
//!
//! 🔑 Every value below is produced by the SCRIPT and read back from Rust.
//! The implementations the bag now reaches (`gdresource::noise`, the
//! `__multimesh__buffer` layout) were already reachable from Rust; what was
//! missing is the dispatch, so asserting the same maths directly from Rust
//! would pass against a completely unbound interpreter.
//!
//! ⚠️ `done` is written LAST, and it is the discriminator that matters here: an
//! unserved registration that returns a VALUE **raises** rather than answering
//! `Nil` (`gdscript-interop/src/native_method_gaps.rs`), which aborts
//! `_process` and leaves every later `set_meta` unrun. Without the flag a
//! wholly-unimplemented engine would fail with six confusing `Nil`s instead of
//! one clear "the frame died".

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `boot_project` mutates the process-global `res://` roots, so serialize under
/// the single-process runner. `nextest` isolates per process.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

const DT: f64 = 1.0 / 60.0;

/// Takes the boot lock, tolerating a previous test's panic, so one failure does
/// not turn every later test into an opaque `PoisonError`.
fn boot_lock() -> std::sync::MutexGuard<'static, ()> {
    BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk96_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `Main` with a `Flag` the script records onto and a `MultiMeshInstance3D`
/// whose buffer the render-side decoder is pointed at below.
fn write_project(dir: &Path, gd: &str) {
    std::fs::write(
        dir.join("project.godot"),
        "config_version=5\n\n[application]\n\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1_m\"]\n\n\
         [node name=\"Main\" type=\"Node3D\"]\n\
         script = ExtResource(\"1_m\")\n\n\
         [node name=\"Flag\" type=\"Node3D\" parent=\".\"]\n\n\
         [node name=\"Instances\" type=\"MultiMeshInstance3D\" parent=\".\"]\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.gd"), gd).unwrap();
}

fn boot(dir: &Path) -> EngineBootstrap {
    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.to_path_buf(),
        headless: true,
        ..BootConfig::default()
    };
    let mut boot = EngineBootstrap::new(config);
    boot.run_all().expect("boot succeeds");
    boot
}

/// ⚠️ `noise_type = 5` is TYPE_VALUE and `fractal_type = 0` is FRACTAL_NONE.
/// Both are pinned deliberately: Godot's `noise_type` DEFAULT is 1
/// (SIMPLEX_SMOOTH), which Patina does not implement and substitutes Perlin
/// for, so leaving the default would make this test's expectation depend on a
/// substitution rule rather than on the property plumbing under test. 5 is one
/// of the two values `noise_type_from_godot` serves exactly.
///
/// ⚠️ Every noise field the generator reads is assigned here rather than left
/// at its ClassDB default. If the Rust side below and the engine side both
/// derived a default from the same place, a wrong default would cancel out and
/// the comparison would pass while reading the wrong field.
///
/// 🔑 The 90° basis for the MultiMesh instance is not identity and not
/// symmetric. Godot's instance buffer stores basis ROWS while Patina's `Basis`
/// stores COLUMNS, so an identity (or any symmetric) basis is invariant under
/// the transpose and would let a transposed encoder pass.
const GD: &str = "\
extends Node3D
var phase = 0

func _process(delta):
    if self.phase != 0:
        return
    self.phase = 1
    var f = get_node(\"Flag\")
    f.set_meta(\"done\", 0)

    var n = FastNoiseLite.new()
    n.noise_type = 5
    n.fractal_type = 0
    n.fractal_octaves = 1
    n.fractal_lacunarity = 2.0
    n.fractal_gain = 0.5
    n.fractal_weighted_strength = 0.0
    n.frequency = 0.25
    n.seed = 7
    f.set_meta(\"noise_a\", n.get_noise_2d(1.5, 2.5))
    f.set_meta(\"noise_b\", n.get_noise_2d(1.5, 2.5))
    f.set_meta(\"noise_2dv\", n.get_noise_2dv(Vector2(1.5, 2.5)))
    f.set_meta(\"noise_1d\", n.get_noise_1d(1.5))
    f.set_meta(\"noise_3d\", n.get_noise_3d(1.5, 2.5, 3.5))
    n.seed = 99
    f.set_meta(\"noise_seed99\", n.get_noise_2d(1.5, 2.5))

    var sh = Shader.new()
    sh.set_code(\"shader_type canvas_item;\")
    f.set_meta(\"shader_code\", sh.get_code())

    var bm = BitMap.new()
    bm.create(Vector2i(4, 4))
    f.set_meta(\"bm_size\", bm.get_size())
    f.set_meta(\"bm_empty\", bm.get_true_bit_count())
    bm.set_bit(1, 1, true)
    f.set_meta(\"bm_count\", bm.get_true_bit_count())
    f.set_meta(\"bm_on\", bm.get_bit(1, 1))
    f.set_meta(\"bm_off\", bm.get_bit(2, 1))
    bm.set_bit(1, 1, false)
    f.set_meta(\"bm_cleared\", bm.get_true_bit_count())

    var mm = MultiMesh.new()
    mm.transform_format = 1
    mm.instance_count = 3
    var b = Basis(Vector3(0, 1, 0), Vector3(-1, 0, 0), Vector3(0, 0, 1))
    mm.set_instance_transform(1, Transform3D(b, Vector3(2, 0, 0)))
    f.set_meta(\"mm_x\", mm.get_instance_transform(1).origin.x)
    f.set_meta(\"mm_basis_xy\", mm.get_instance_transform(1).basis.x.y)
    f.set_meta(\"mm_other_x\", mm.get_instance_transform(0).origin.x)
    f.set_meta(\"mm_buffer\", mm.buffer)

    var sk = Skin.new()
    sk.add_bind(3, Transform3D(Basis(), Vector3(0, 5, 0)))
    f.set_meta(\"skin_count\", sk.get_bind_count())
    f.set_meta(\"skin_bone\", sk.get_bind_bone(0))
    f.set_meta(\"skin_y\", sk.get_bind_pose(0).origin.y)

    var tr = Translation.new()
    tr.add_message(\"FILE\", \"Fichier\")
    tr.add_message(\"FILE\", \"Dossier\", \"menu\")
    f.set_meta(\"tr_plain\", tr.get_message(\"FILE\"))
    f.set_meta(\"tr_ctx\", tr.get_message(\"FILE\", \"menu\"))
    f.set_meta(\"tr_miss\", tr.get_message(\"NOPE\"))
    f.set_meta(\"tr_count\", tr.get_message_count())
    f.set_meta(\"tr_list\", tr.get_message_list())

    f.set_meta(\"done\", 1)
";

fn meta(boot: &EngineBootstrap, flag: gdscene::node::NodeId, key: &str) -> Variant {
    boot.main_loop()
        .unwrap()
        .tree()
        .get_node(flag)
        .expect("the Flag node is still in the tree")
        .get_meta(key)
}

/// Reads a recorded number, accepting either numeric Variant.
///
/// ⚠️ Not pinned to `Float`: a whole-numbered value can box as `Int`, and
/// asserting the Variant KIND would fail on a correct 2.
fn num(boot: &EngineBootstrap, flag: gdscene::node::NodeId, key: &str) -> f64 {
    match meta(boot, flag, key) {
        Variant::Float(f) => f,
        Variant::Int(n) => n as f64,
        other => panic!(
            "`{key}` must be recorded as a number; `Nil` means the call that \
             produced it was never dispatched, got {other:?}"
        ),
    }
}

/// Reads a recorded string, accepting `StringName` as well as `String`.
///
/// ⚠️ `Translation.get_message` answers a `StringName` in Godot and
/// `Shader.get_code` a `String`; pinning one kind would fail on a correct
/// value of the other.
fn text(boot: &EngineBootstrap, flag: gdscene::node::NodeId, key: &str) -> String {
    match meta(boot, flag, key) {
        Variant::String(s) => s,
        Variant::StringName(s) => s.as_str().to_string(),
        other => panic!("`{key}` must be recorded as a string, got {other:?}"),
    }
}

/// Reads a recorded float array.
///
/// ⚠️ Accepts a generic `Array` as well as `PackedFloat32Array`: the value
/// travels through `set_meta`/`get_meta`, and pinning the container KIND would
/// make this fail on a correct set of floats that happened to be boxed as an
/// untyped array.
fn floats(boot: &EngineBootstrap, flag: gdscene::node::NodeId, key: &str) -> Vec<f32> {
    match meta(boot, flag, key) {
        Variant::PackedFloat32Array(v) => v,
        Variant::PackedFloat64Array(v) => v.into_iter().map(|f| f as f32).collect(),
        Variant::Array(a) => a
            .to_vec()
            .into_iter()
            .map(|v| match v {
                Variant::Float(f) => f as f32,
                Variant::Int(n) => n as f32,
                other => panic!("`{key}` must hold numbers, got {other:?}"),
            })
            .collect(),
        other => panic!(
            "`{key}` must be a float array; `Nil` means the call that produced \
             it was never dispatched, got {other:?}"
        ),
    }
}

/// Reads a recorded string array, tolerating the same container variation.
fn strings(boot: &EngineBootstrap, flag: gdscene::node::NodeId, key: &str) -> Vec<String> {
    match meta(boot, flag, key) {
        Variant::PackedStringArray(v) => v,
        Variant::Array(a) => a
            .to_vec()
            .into_iter()
            .map(|v| match v {
                Variant::String(s) => s,
                Variant::StringName(s) => s.as_str().to_string(),
                other => panic!("`{key}` must hold strings, got {other:?}"),
            })
            .collect(),
        other => panic!(
            "`{key}` must be a string array; `Nil` means the call that produced \
             it was never dispatched, got {other:?}"
        ),
    }
}

#[test]
fn gdscript_resource_methods_stage1() {
    let _guard = boot_lock();
    let dir = unique_project_dir("resmethods");
    write_project(&dir, GD);
    let mut boot = boot(&dir);

    let flag = boot
        .main_loop()
        .unwrap()
        .tree()
        .get_node_by_path("/root/Main/Flag")
        .expect("the fixture declares a Flag node");

    for _ in 0..3 {
        boot.main_loop_mut().unwrap().step(DT);
    }

    // -- the frame survived every call ---------------------------------------
    assert_eq!(
        meta(&boot, flag, "done"),
        Variant::Int(1),
        "`_process` must reach its last line; 0 or Nil means one of the \
         resource methods above raised and aborted the frame — which is \
         exactly what all six families did before this change"
    );

    // -- (1) Noise: the bag's fields actually reach the generator ------------
    //
    // 🔑 Built here from the SAME fields the script assigned, so the claim
    // under test is the plumbing (does the bag's `seed`/`frequency` reach
    // `gdresource::noise`), not the noise maths — that has its own unit tests
    // in `gdresource/src/noise.rs`.
    let expected = gdresource::noise::FastNoiseLite::new()
        .seed(7)
        .frequency(0.25)
        .noise_type(gdresource::noise::NoiseType::Value)
        .fractal(gdresource::noise::FractalType::None)
        .octaves(1)
        .lacunarity(2.0)
        .gain(0.5)
        .weighted_strength(0.0)
        .get_noise_2d(1.5, 2.5) as f64;
    let noise_a = num(&boot, flag, "noise_a");
    assert!(
        (noise_a - expected).abs() < 1e-6,
        "`get_noise_2d(1.5, 2.5)` on a seed-7 Value-noise FastNoiseLite must \
         equal `gdresource::noise` for the same fields: expected {expected}, \
         got {noise_a}. A mismatch means the property bag's fields are not the \
         ones the generator was built from"
    );
    // ⚠️ Deliberately NOT also asserting `noise_a != 0.0`. A constant stub is
    // already ruled out by the seed-change assertion below, and the expectation
    // above is built from literals rather than from the bag — so an absolute
    // magnitude check would add no power while risking a spurious failure if
    // this sample point ever genuinely lands near zero.
    assert_eq!(
        num(&boot, flag, "noise_b"),
        noise_a,
        "two identical calls must return bit-identical values — the generator \
         is rebuilt per call, so an unstable result means something in the \
         rebuild is not a pure function of the bag"
    );
    assert_eq!(
        num(&boot, flag, "noise_2dv"),
        noise_a,
        "`get_noise_2dv(Vector2(1.5, 2.5))` is the vector spelling of the same \
         sample and must agree exactly"
    );
    // 🔑 The discriminating assertion: changing `seed` must change the output.
    // Every assertion above passes against an implementation that ignores the
    // bag entirely and samples a fixed default generator.
    assert_ne!(
        num(&boot, flag, "noise_seed99"),
        noise_a,
        "re-assigning `seed` must change the sampled value; an unchanged \
         number means `get_noise_2d` never read the bag's seed"
    );
    // Godot's 1D sampler is the 2D one at y = 0. Compared against `noise_a`,
    // which is the SAME seed at y = 2.5 — so this isolates the sample point. A
    // constant-returning stub, or a 1D entry point that dropped its argument,
    // would make the two equal.
    assert_ne!(
        num(&boot, flag, "noise_1d"),
        noise_a,
        "`get_noise_1d(1.5)` samples (1.5, 0) and must differ from the same \
         generator's (1.5, 2.5)"
    );
    // `num` panics on `Nil`, which is the whole claim for the 3D entry point:
    // it answered rather than falling through to UndefinedFunction. Its exact
    // value is `gdresource::noise`'s business and is unit-tested there.
    let _ = num(&boot, flag, "noise_3d");

    // -- (2) Shader.set_code -> get_code round-trip --------------------------
    assert_eq!(
        text(&boot, flag, "shader_code"),
        "shader_type canvas_item;",
        "`set_code` then `get_code` must round-trip through the `code` property"
    );

    // -- (3) BitMap: create, set one bit, count ------------------------------
    assert_eq!(
        meta(&boot, flag, "bm_size"),
        Variant::Vector2i(gdcore::math::Vector2i::new(4, 4)),
        "`create(Vector2i(4, 4))` must be what `get_size()` reports back"
    );
    // ⚠️ The empty count is asserted BEFORE the populated one. Godot sizes the
    // mask at `(w * h) / 8 + 1` bytes, so the last byte carries bits past the
    // end of the bitmap; a popcount over whole bytes would make a fresh 4x4
    // bitmap report something other than 0 only once those spare bits were
    // set, which is far too late to notice.
    assert_eq!(
        num(&boot, flag, "bm_empty") as i64,
        0,
        "a freshly created BitMap has no bits set"
    );
    assert_eq!(
        num(&boot, flag, "bm_count") as i64,
        1,
        "`set_bit(1, 1, true)` must make `get_true_bit_count()` exactly 1"
    );
    assert_eq!(
        meta(&boot, flag, "bm_on"),
        Variant::Bool(true),
        "`get_bit(1, 1)` must read back the bit that was just set"
    );
    // 🔑 A neighbouring cell, because `ofs = width * y + x`: an implementation
    // that dropped the `* width` would set (2, 1) and (1, 1) to the same offset
    // and this would come back true.
    assert_eq!(
        meta(&boot, flag, "bm_off"),
        Variant::Bool(false),
        "the cell next to it must stay clear; a shared offset would set both"
    );
    // 🔑 Clearing must also work: a `set_bit` that only ever ORs bits in would
    // pass every assertion above.
    assert_eq!(
        num(&boot, flag, "bm_cleared") as i64,
        0,
        "`set_bit(1, 1, false)` must clear the bit again"
    );

    // -- (4) MultiMesh: the instance buffer, in Godot's own layout ----------
    assert_eq!(
        num(&boot, flag, "mm_x"),
        2.0,
        "`set_instance_transform(1, …)` then `get_instance_transform(1)` must \
         round-trip the origin"
    );
    assert_eq!(
        num(&boot, flag, "mm_basis_xy"),
        1.0,
        "the basis must round-trip too, not just the origin: the authored \
         `Basis(Vector3(0,1,0), …)` has `basis.x.y == 1`, and a transposed \
         encode/decode would answer -1 here"
    );
    // A control the change cannot touch: instance 0 was never written, and
    // Godot's allocation zero-fills, so it must still read 0.
    assert_eq!(
        num(&boot, flag, "mm_other_x"),
        0.0,
        "instance 0 was never set and must still read 0 — a writer that \
         ignored its index would have put 2 here"
    );

    // 🔑 The raw floats, because the render path does NOT call
    // `get_instance_transform`: `gdscene::node3d` decodes the buffer itself,
    // and an encoder that is self-consistently wrong would satisfy the
    // round-trip above while drawing every instance in the wrong place.
    let buffer = floats(&boot, flag, "mm_buffer");
    assert_eq!(
        buffer.len(),
        36,
        "3 instances at TRANSFORM_3D's 12 floats each; a different length means \
         the stride or the lazy allocation is wrong, and `instance_count` is a \
         plain property write that cannot resize the buffer on its own"
    );
    // Row-major: each basis ROW, then one origin component.
    assert_eq!(
        &buffer[12..24],
        &[0.0f32, -1.0, 0.0, 2.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0][..],
        "instance 1 must be stored as Godot stores it — basis ROWS with an \
         origin component after each. A column-major writer would put 1.0 at \
         index 13 and -1.0 at index 16"
    );

    // The scene-side decoder must agree with what the script wrote. Pointing
    // it at the script's own bytes is the only way to test that without the
    // `mmi.multimesh = mm` property flatten, which lives in `gdscene` and is
    // not part of this change.
    let mmi = boot
        .main_loop()
        .unwrap()
        .tree()
        .get_node_by_path("/root/Main/Instances")
        .expect("the fixture declares a MultiMeshInstance3D");
    {
        let tree = boot.main_loop_mut().unwrap().tree_mut();
        let node = tree.get_node_mut(mmi).expect("the node is in the tree");
        node.set_property("__multimesh__transform_format", Variant::Int(1));
        node.set_property("__multimesh__instance_count", Variant::Int(3));
        node.set_property(
            "__multimesh__buffer",
            Variant::PackedFloat32Array(buffer.clone()),
        );
    }
    let decoded =
        gdscene::node3d::get_multimesh_instance_transform(boot.main_loop().unwrap().tree(), mmi, 1);
    assert!(
        (decoded.origin.x - 2.0).abs() < 1e-6,
        "the render path's own decoder must read instance 1 at x = 2 out of the \
         buffer the script wrote; got {:?}",
        decoded.origin
    );
    assert!(
        (decoded.basis.x.y - 1.0).abs() < 1e-6,
        "and it must read the same basis back — this is the assertion a \
         transposed layout fails while the script-level round-trip above still \
         passes; got {:?}",
        decoded.basis
    );

    // -- (5) Skin binds -----------------------------------------------------
    assert_eq!(
        num(&boot, flag, "skin_count") as i64,
        1,
        "`add_bind` must raise `get_bind_count()` to 1"
    );
    // ⚠️ Bone 3, not 0: `get_bind_bone`'s absent-value fallback is -1 and a
    // zeroed bag would read 0, so a bone index of 0 could not tell a stored
    // value from either default.
    assert_eq!(
        num(&boot, flag, "skin_bone") as i64,
        3,
        "the bone index passed to `add_bind` must be what `get_bind_bone(0)` \
         answers"
    );
    assert_eq!(
        num(&boot, flag, "skin_y"),
        5.0,
        "and the bind pose must round-trip; `get_bind_pose`'s fallback is the \
         IDENTITY transform, whose origin.y is 0"
    );

    // -- (6) Translation messages ------------------------------------------
    assert_eq!(
        text(&boot, flag, "tr_plain"),
        "Fichier",
        "`add_message` then `get_message` must round-trip in the default context"
    );
    // 🔑 Godot's lookup is an exact `(context, source)` pair with NO fall back
    // to the context-free entry, so the two entries for "FILE" must resolve
    // separately. One shared map would answer "Fichier" (or "Dossier") to both.
    assert_eq!(
        text(&boot, flag, "tr_ctx"),
        "Dossier",
        "a message added under the \"menu\" context must resolve to its own \
         translation, not to the context-free one"
    );
    assert_eq!(
        text(&boot, flag, "tr_miss"),
        "",
        "a missing key answers the EMPTY string: `Translation.get_message` \
         returns StringName() and it is `tr()` above it that falls back to the \
         key, so returning the source here would make an untranslated string \
         indistinguishable from a translated one"
    );
    assert_eq!(
        num(&boot, flag, "tr_count") as i64,
        2,
        "both entries count: upstream keeps contextual messages in the same \
         `translation_map`, so `get_message_count()` is NOT \
         `get_message_list().size()`"
    );
    assert_eq!(
        strings(&boot, flag, "tr_list"),
        vec!["FILE".to_string()],
        "`get_message_list()` enumerates the default-context sources, once — \
         the contextual entry shares the source text and must not duplicate it"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

//! pat-3mbf5 — a glTF clip's loop mode comes from where Godot reads it.
//!
//! A `.glb` carries no loop flag. Godot loops a clip when
//!
//! 1. its name begins or ends with `loop`/`cycle` (`gltf_document.cpp`),
//! 2. with `nodes/use_name_suffixes`, its name carries a `-loop`/`_loop`/
//!    `$loop` (or `cycle`, `loop_mode`) hint, which is stripped
//!    (`resource_importer_scene.cpp` `_pre_fix_node`), or
//! 3. the `.import` sidecar's `_subresources` lists it with
//!    `settings/loop_mode` — which, when present, wins over 1 and 2.
//!
//! The official 3D platformer loops `idle`/`run`/`walk`/`jump`/`falling` ONLY
//! through 3. `games/platformer_3d` was copied without its `.import` files, so
//! these tests pair the real `player.glb` with the upstream sidecar
//! (`tests/fixtures/platformer_3d_upstream_import/player.glb.import`, from
//! godot-demo-projects b761b4c, trimmed of its 99 empty `slice_N` blocks per
//! clip) in a temporary project. Expected bone poses are Godot 4.6.1's, on the
//! same files.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use gdcore::math3d::Transform3D;
use gdscene::animation::{gltf_name_requests_loop, Animation, LoopMode};
use gdscene::gltf_scene::apply_import_animation_settings;
use gdscene::packed_scene::{
    add_packed_scene_to_tree_from_res, begin_subscene_session, load_scene_from_res,
};
use gdscene::{skeleton3d, NodeId, SceneTree};
use gdvariant::Variant;

static ROOTS: Mutex<()> = Mutex::new(());

fn platformer_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../games/platformer_3d")
}

fn upstream_sidecar() -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/platformer_3d_upstream_import/player.glb.import"),
    )
    .expect("upstream sidecar fixture")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            std::fs::copy(entry.path(), dest).unwrap();
        }
    }
}

/// The platformer's `project.godot` and `player/` folder in a temp project,
/// plus the upstream `player.glb.import`.
fn project_with_upstream_sidecar() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(platformer_root().join("project.godot"), dir.path().join("project.godot"))
        .unwrap();
    copy_dir(&platformer_root().join("player"), &dir.path().join("player"));
    std::fs::write(dir.path().join("player/player.glb.import"), upstream_sidecar()).unwrap();
    dir
}

fn boot(root: &Path, scene: &str) -> (SceneTree, NodeId) {
    gdcore::file_access::set_path_roots(Some(root.to_path_buf()), None);
    begin_subscene_session();
    let packed = load_scene_from_res(&format!("res://{scene}"))
        .unwrap_or_else(|| panic!("res://{scene} must load"));
    let mut tree = SceneTree::new();
    let tree_root = tree.root_id();
    let id = add_packed_scene_to_tree_from_res(&mut tree, tree_root, &packed)
        .unwrap_or_else(|e| panic!("res://{scene} must instance: {e}"));
    (tree, id)
}

fn loop_modes(tree: &SceneTree, glb_root: NodeId) -> Vec<(String, LoopMode)> {
    let ap = tree.get_node_relative(glb_root, "AnimationPlayer").expect("AnimationPlayer");
    let player = tree.get_animation_player(ap).expect("clips attached");
    let mut out: Vec<_> = player
        .animations
        .iter()
        .map(|(name, a)| (name.clone(), a.loop_mode))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn upstream_sidecar_loops_exactly_the_clips_godot_loops() {
    let _g = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let project = project_with_upstream_sidecar();
    let (tree, glb) = boot(project.path(), "player/player.glb");
    // Godot 4.6.1 on the same files: `Animation.loop_mode` per clip.
    assert_eq!(
        loop_modes(&tree, glb),
        [
            ("RESET".to_string(), LoopMode::None),
            ("falling".to_string(), LoopMode::Linear),
            ("idle".to_string(), LoopMode::Linear),
            ("jump".to_string(), LoopMode::Linear),
            ("run".to_string(), LoopMode::Linear),
            ("shooting".to_string(), LoopMode::None),
            ("shooting_standing".to_string(), LoopMode::None),
            ("walk".to_string(), LoopMode::Linear),
        ]
    );
}

#[test]
fn without_a_sidecar_no_platformer_clip_loops_as_in_godot() {
    let _g = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let (tree, glb) = boot(&platformer_root(), "player/player.glb");
    // Godot on the fixture as shipped (no `.import`): every loop_mode is 0.
    for (name, mode) in loop_modes(&tree, glb) {
        assert_eq!(mode, LoopMode::None, "{name}");
    }
}

/// The robot through player.tscn's AnimationTree with only `idle` weighted,
/// 90 frames at 1/60 s under the 1.5x TimeScale = 2.25 s into a 1.233 s clip.
/// With the sidecar it loops (Godot: 1.017 s into the clip); the fixture-only
/// counterpart in `platformer_3d_robot_skinning_matches_godot.rs` holds the
/// last frame instead. Godot ran 91 advances — its first is a seek to 0.
#[test]
fn idle_loops_through_the_animation_tree_like_godot_with_the_sidecar() {
    let _g = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let project = project_with_upstream_sidecar();
    let (mut tree, player) = boot(project.path(), "player/player.tscn");
    let skeleton = tree
        .get_node_relative(player, "Player/Skeleton/Skeleton3D")
        .expect("skeleton");
    let at = tree.get_node_relative(player, "AnimationTree").expect("tree");
    let anim_tree = tree.get_animation_tree_mut(at).expect("AnimationTree attached");
    for name in ["run", "speed", "state"] {
        anim_tree.set_parameter(format!("parameters/{name}/blend_amount"), Variant::Float(0.0));
    }
    for _ in 0..90 {
        anim_tree.advance(1.0 / 60.0);
    }
    skeleton3d::process_skeletons(&mut tree);

    let godot: &[(&str, [f32; 12])] = &[
        ("hip", [1.000000, 0.000000, -0.000000, 0.000000, 1.000000, 0.000001, 0.000000, -0.000001, 1.000000, 0.000000, 2.020854, 0.394217]),
        ("head", [1.000000, -0.000000, -0.000002, 0.000000, 0.999783, 0.020811, 0.000002, -0.020811, 0.999783, 0.000000, 4.897612, 0.283739]),
        ("r-forearm", [-0.929345, 0.105866, 0.353709, -0.056946, -0.987646, 0.145985, 0.364794, 0.115528, 0.923893, 1.535318, 3.291771, 0.366181]),
        ("l-foot", [-0.999785, 0.019304, -0.009299, -0.010119, -0.043012, 0.999027, 0.018879, 0.998889, 0.043198, -0.738713, 0.193314, 0.315612]),
    ];
    for (name, want) in godot {
        let bone = skeleton3d::find_bone(&tree, skeleton, name).unwrap();
        let t: Transform3D = skeleton3d::get_bone_global_pose(&tree, skeleton, bone);
        let b = &t.basis;
        let got = [
            b.x.x, b.x.y, b.x.z, b.y.x, b.y.y, b.y.z, b.z.x, b.z.y, b.z.z, t.origin.x,
            t.origin.y, t.origin.z,
        ];
        let err = got.iter().zip(want).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(err <= 1e-4, "{name}: off Godot by {err}\n  patina {got:?}\n  godot  {want:?}");
    }
}

#[test]
fn gltf_clip_names_starting_or_ending_with_loop_or_cycle_loop() {
    // `looping_idle` begins with "loop", so Godot's prefix test takes it too.
    for name in ["loop_walk", "WalkLoop", "Cycle", "run_cycle", "LOOP", "looping_idle"] {
        assert!(gltf_name_requests_loop(name), "{name}");
    }
    // The platformer's own names, and a word that merely CONTAINS one.
    for name in ["idle", "walk", "run", "falling", "recycled"] {
        assert!(!gltf_name_requests_loop(name), "{name}");
    }
}

fn clip(name: &str) -> Animation {
    Animation::new(name, 1.0)
}

#[test]
fn import_name_hints_loop_and_are_stripped_when_suffixes_are_on() {
    let mut clips = vec![clip("walk-loop"), clip("run_cycle"), clip("idle$loop"), clip("jump")];
    // `[params]` without `nodes/use_name_suffixes=false`: Godot's default is on.
    apply_import_animation_settings(&mut clips, "[params]\n\n_subresources={}\n");
    let got: Vec<_> = clips.iter().map(|c| (c.name.as_str(), c.loop_mode)).collect();
    assert_eq!(
        got,
        [
            ("walk", LoopMode::Linear),
            ("run", LoopMode::Linear),
            ("idle", LoopMode::Linear),
            ("jump", LoopMode::None),
        ]
    );

    let mut off = vec![clip("walk-loop")];
    apply_import_animation_settings(&mut off, "[params]\nnodes/use_name_suffixes=false\n");
    assert_eq!((off[0].name.as_str(), off[0].loop_mode), ("walk-loop", LoopMode::None));
}

#[test]
fn a_sidecar_entry_overrides_the_name_rules_including_back_to_none() {
    let mut clips = vec![clip("cycle"), clip("idle"), clip("wave")];
    clips[0].loop_mode = LoopMode::Linear; // what the glTF name rule gave it
    let sidecar = "[params]\n\n_subresources={\n\"animations\": {\n\"cycle\": {\n\"settings/loop_mode\": 0\n},\n\"idle\": {\n\"settings/loop_mode\": 2,\n\"slices/amount\": 0\n}\n}\n}\ngltf/naming_version=2\n";
    apply_import_animation_settings(&mut clips, sidecar);
    assert_eq!(clips[0].loop_mode, LoopMode::None, "an entry wins over the name rule");
    assert_eq!(clips[1].loop_mode, LoopMode::PingPong);
    assert_eq!(clips[2].loop_mode, LoopMode::None, "no entry: unchanged");
}

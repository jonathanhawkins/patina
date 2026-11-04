//! pat-mgixk.77 (partial) — an AnimationTree's 3D transform tracks become bone
//! poses.
//!
//! Godot's AnimationTree **is** an AnimationMixer: its blended result goes
//! through the same track application as an AnimationPlayer's, so a
//! `Skeleton3D:Bone` rotation track deforms the rig
//! (`upstream/godot/scene/animation/animation_mixer.cpp _blend_process`).
//!
//! Patina's `apply_animation_bone_tracks` gathered only from
//! `tree.get_animation_player(id)` — a grep for `animation_tree` in
//! `skeleton3d.rs` returned nothing. So a state-machine-driven
//! `Skeleton3D:Arm` rotation_3d track was written as a stray `Arm` **property**
//! on the Skeleton3D and the rig never deformed. That is Godot's main
//! AnimationTree use case, which is why this read as a far smaller gap than it
//! was.
//!
//! # Scope of this test
//!
//! ⚠️ This is **not quite** the bead's full named acceptance
//! (`animation_tree_drives_skeleton_and_method_tracks`). Covered here: the bone
//! pose matching the clip sample; the Skeleton3D having no stray `Arm`
//! property; a method key not landing as a junk property; and the method being
//! dispatched exactly once per key crossing. **Still outstanding:** emitting
//! `animation_started` from the tree node — reported as remaining work on
//! pat-mgixk.77.
//!
//! # Why the clip is constant
//!
//! ⚠️ Both keyframes carry the SAME rotation, so any sample time yields the
//! same value. A ramp would make the assertion depend on how many frames ran
//! and on the leaf clock's phase — measuring the scheduler rather than whether
//! the pose was applied at all.

use gdcore::math::Vector3;
use gdcore::math3d::Quaternion;
use gdscene::animation::{Animation, AnimationTrack, KeyFrame, TrackType};
use gdscene::animation_tree::{AnimationLeafOptions, AnimationNode, AnimationTree};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::{skeleton3d, MainLoop};
use gdscript_interop::bindings::{MethodInfo, ScriptError, ScriptInstance, ScriptPropertyInfo};
use gdvariant::Variant;
use std::cell::RefCell;
use std::rc::Rc;

const DT: f64 = 1.0 / 60.0;

type CallLog = Rc<RefCell<Vec<(String, Vec<Variant>)>>>;

/// Records every method dispatched to it, so a test can count crossings.
/// Same shape as the player-side `animation_root_motion_method_test.rs`.
struct RecordingScript {
    calls: CallLog,
}

impl ScriptInstance for RecordingScript {
    fn call_method(&mut self, name: &str, args: &[Variant]) -> Result<Variant, ScriptError> {
        self.calls
            .borrow_mut()
            .push((name.to_string(), args.to_vec()));
        Ok(Variant::Nil)
    }
    fn get_property(&self, _name: &str) -> Option<Variant> {
        None
    }
    fn set_property(&mut self, _name: &str, _value: Variant) -> bool {
        false
    }
    fn list_methods(&self) -> Vec<MethodInfo> {
        Vec::new()
    }
    fn list_properties(&self) -> Vec<ScriptPropertyInfo> {
        Vec::new()
    }
    fn get_script_name(&self) -> &str {
        "RecordingScript"
    }
    /// ⚠️ Everything EXCEPT `_init`, and the exclusion is load bearing.
    ///
    /// `SceneTree::attach_script` runs `attach_script_with_init_args`, which
    /// calls `_init` on attach whenever `has_method("_init")` answers true
    /// (`scene_tree.rs:5464`, pat-mgixk.100) — Godot's own behaviour, since
    /// `_init` fires when the instance is constructed. A double that claims
    /// EVERY method therefore records a phantom `_init` the moment it is
    /// attached, and every call-count assertion below is off by one.
    ///
    /// Fixed here rather than by filtering the log, so each assertion can keep
    /// saying what it means ("fired once") instead of carrying a subtraction.
    ///
    /// ⚠️ The same holds for EVERY lifecycle callback, not only `_init`: the
    /// main loop dispatches `_process` / `_physics_process` / `_draw` to any
    /// script that claims them (Godot runs a defined `_process` each frame),
    /// so a double claiming everything logs them too. It declares only the
    /// method the track calls.
    fn has_method(&self, name: &str) -> bool {
        name == "on_footstep"
    }
}

/// A clip whose only track rotates the `Arm` bone of a child `Skeleton3D`.
///
/// The track is addressed the way Godot addresses a bone track: the node path
/// locates the skeleton, and the property path names the bone.
fn bone_clip(rotation: Quaternion) -> Animation {
    let mut anim = Animation::new("walk", 1.0);
    let mut track = AnimationTrack::new("Arm");
    track.node_path = "Skeleton3D".to_string();
    track.track_type = TrackType::Rotation3D;
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Quaternion(rotation)));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Quaternion(rotation)));
    anim.tracks.push(track);
    anim
}

fn close(a: Quaternion, b: Quaternion) -> bool {
    (a.x - b.x).abs() < 1e-4
        && (a.y - b.y).abs() < 1e-4
        && (a.z - b.z).abs() < 1e-4
        && (a.w - b.w).abs() < 1e-4
}

/// **The acceptance for this half.** A tree-driven rotation track lands on the
/// bone, through the real frame loop.
#[test]
fn animation_tree_bone_tracks_reach_the_skeleton() {
    // Defensive, not load-bearing: `MainLoop::step` drives several subsystems
    // that consult the ClassDB. The bone helpers here do NOT need it —
    // `Node::new` seeds no defaults (`node.rs:217` starts from an empty map) and
    // `process_skeletons_with_delta` filters on `class_name()` string equality,
    // not on `is_class`. Said the opposite before this was checked.
    gdobject::class_db::register_all_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    // 🔑 Godot's scene shape, and the only one the DEFAULT `root_node` works
    // with: the mixer is its own node, sibling to the `Skeleton3D`, and
    // `AnimationTree::root_node` defaults to `".."` — the mixer node's PARENT,
    // as Godot's `AnimationMixer.root_node` documents. So `Rig` is the base and
    // the track's `Skeleton3D` path resolves as its child.
    //
    // ⚠️ This test first attached the tree straight to `Rig` on the assumption
    // that the default was `"."`. It is not: `".."` then resolved to the scene
    // root, which has no `Skeleton3D`, every path lookup returned `None`, and
    // the gather produced zero writes — so the test failed against a correct
    // implementation. Same layout as the green player case in
    // `skeleton3d.rs::animation_bone_tracks_write_bone_poses`, which sets
    // `root_node = ".."` explicitly for exactly this reason.
    let rig = tree.add_child(root, Node::new("Rig", "Node3D")).unwrap();
    let mixer = tree
        .add_child(rig, Node::new("AnimationTree", "AnimationTree"))
        .unwrap();
    let skeleton = tree
        .add_child(rig, Node::new("Skeleton3D", "Skeleton3D"))
        .unwrap();
    let arm = skeleton3d::add_bone(&mut tree, skeleton, "Arm");

    // 90 degrees about Y — far enough from identity that no rounding could
    // produce it by accident.
    let rotation =
        Quaternion::from_axis_angle(Vector3::new(0.0, 1.0, 0.0), std::f32::consts::FRAC_PI_2);

    let mut anim_tree = AnimationTree::new();
    anim_tree.add_animation(bone_clip(rotation));
    anim_tree.set_root(AnimationNode::Animation {
        name: "walk".to_string(),
        position: 0.0,
        opts: AnimationLeafOptions::default(),
    });
    tree.attach_animation_tree(mixer, anim_tree)
        .expect("the AnimationTree attaches to its own node");

    // -- control: the bone is at rest before any frame ----------------------
    //
    // Without this, "the pose matches" could be true because the rest pose
    // happened to be the keyed value.
    let before = skeleton3d::get_bone_pose_rotation(&tree, skeleton, arm);
    assert!(
        !close(before, rotation),
        "precondition: the bone must NOT already hold the keyed rotation; got \
         {before:?}"
    );

    // -- drive the real frame loop ------------------------------------------
    //
    // `MainLoop::step` advances the tree's clocks and then runs
    // `process_skeletons_with_delta`, which is where bone tracks are applied.
    // Nothing here calls the application path directly.
    let mut ml = MainLoop::new(tree);
    for _ in 0..3 {
        ml.step(DT);
    }

    let after = skeleton3d::get_bone_pose_rotation(ml.tree(), skeleton, arm);
    assert!(
        close(after, rotation),
        "the tree's rotation_3d track must become the bone's pose: expected \
         {rotation:?}, got {after:?}. An unchanged rest pose means the track was \
         sampled and then written as a plain property on the Skeleton3D instead \
         of routed to a bone — `apply_animation_bone_tracks` gathered only from \
         `get_animation_player`, so an AnimationTree drove nothing."
    );

    // -- the bead's "no stray `Arm` property" clause -------------------------
    //
    // The pose landing is only half of it. `process_animation_trees_in_mode`
    // wrote every sampled value as an ordinary property, so the SAME key also
    // left an `Arm` entry on the Skeleton3D — a value nothing reads, shadowing
    // nothing, and visible to any script that enumerates properties.
    //
    // 🔑 This is the assertion the `scene_tree.rs` half of the change is load
    // bearing for: the pose above lands from `skeleton3d.rs` alone, so without
    // this clause the whole property-loop fix could be reverted and the test
    // would still pass.
    assert!(
        !ml.tree()
            .get_node(skeleton)
            .expect("the Skeleton3D is still in the tree")
            .has_property("Arm"),
        "a bone track must NOT also be written as a node property: the \
         Skeleton3D still has an `Arm` property, so the sampled pose was \
         applied twice — once correctly as a bone pose and once as junk"
    );
}

/// A method track must not land on the node as a property either.
///
/// The same missing type information that turned a bone key into a stray `Arm`
/// property turned a method key into a stray string: a method track's sampled
/// value is the method NAME, and the tree path wrote it with
/// `set_animated_property_path` like any other value.
///
/// ⚠️ This asserts the junk is **gone**, not that the method **ran**. Method
/// dispatch for trees is still outstanding on pat-mgixk.77; conflating the two
/// would let a test that only proves the skip-set claim the dispatch as well.
#[test]
fn a_tree_method_track_is_not_written_as_a_property() {
    gdobject::class_db::register_all_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let rig = tree.add_child(root, Node::new("Rig", "Node3D")).unwrap();
    let mixer = tree
        .add_child(rig, Node::new("AnimationTree", "AnimationTree"))
        .unwrap();

    // A method track addressed at the mixer's `root_node` (`".."` → `Rig`).
    let mut anim = Animation::new("walk", 1.0);
    let mut method = AnimationTrack::new("footstep");
    method.node_path = ".".to_string();
    method.track_type = TrackType::Method;
    method.add_keyframe(KeyFrame::linear(
        0.0,
        Variant::String("footstep".to_string()),
    ));
    anim.tracks.push(method);

    let mut anim_tree = AnimationTree::new();
    anim_tree.add_animation(anim);
    anim_tree.set_root(AnimationNode::Animation {
        name: "walk".to_string(),
        position: 0.0,
        opts: AnimationLeafOptions::default(),
    });
    tree.attach_animation_tree(mixer, anim_tree).unwrap();

    let mut ml = MainLoop::new(tree);
    for _ in 0..3 {
        ml.step(DT);
    }

    let target = ml
        .tree()
        .get_node(rig)
        .expect("the mixer's root_node target is still in the tree");
    assert!(
        !target.has_property("footstep"),
        "a method track's sampled value is the method NAME — writing it as a \
         property leaves `footstep = \"footstep\"` on the node; got {:?}",
        target.get_property("footstep")
    );
}

/// The bone track must not be claimed by a player that is not there.
///
/// ⚠️ A guard against the obvious wrong fix: making every 3D transform track
/// apply to a skeleton regardless of its mixer would pass the test above while
/// breaking scenes that key a plain `Node3D` property. With no AnimationTree
/// and no AnimationPlayer attached, nothing may touch the pose.
#[test]
fn a_skeleton_with_no_mixer_keeps_its_rest_pose() {
    gdobject::class_db::register_all_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let rig = tree.add_child(root, Node::new("Rig", "Node3D")).unwrap();
    let skeleton = tree
        .add_child(rig, Node::new("Skeleton3D", "Skeleton3D"))
        .unwrap();
    let arm = skeleton3d::add_bone(&mut tree, skeleton, "Arm");
    let rest = skeleton3d::get_bone_pose_rotation(&tree, skeleton, arm);

    let mut ml = MainLoop::new(tree);
    for _ in 0..3 {
        ml.step(DT);
    }

    let after = skeleton3d::get_bone_pose_rotation(ml.tree(), skeleton, arm);
    assert!(
        close(after, rest),
        "a skeleton with no mixer attached must keep its rest pose; got \
         {after:?} instead of {rest:?}"
    );
}

/// **The bead's method clause.** A tree-crossed method key is dispatched to the
/// node the track names, exactly once per crossing, through the real frame loop.
///
/// Patina's tree path had no dispatch at all: a method key's sampled value was
/// written as a property and the method never ran
/// (`grep 'TrackType::' animation_tree.rs` was empty before pat-mgixk.77).
///
/// 🔑 The second half — advancing well past the key and asserting the count is
/// STILL one — is the load-bearing part. An implementation that exposed the
/// crossed calls without *draining* them would satisfy "the method ran" on
/// frame 2 and then re-fire `footstep()` on every frame after it, which is the
/// failure mode "exactly once per key crossing" exists to rule out.
#[test]
fn a_tree_method_track_dispatches_once_per_crossing() {
    gdobject::class_db::register_all_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let rig = tree.add_child(root, Node::new("Rig", "Node3D")).unwrap();
    let mixer = tree
        .add_child(rig, Node::new("AnimationTree", "AnimationTree"))
        .unwrap();
    // The track targets this sibling by name, so a dispatch that ignored
    // `node_path` and called the mixer's own node would leave this log empty.
    let walker = tree.add_child(rig, Node::new("Walker", "Node3D")).unwrap();

    let walker_calls: CallLog = Rc::new(RefCell::new(Vec::new()));
    let mixer_calls: CallLog = Rc::new(RefCell::new(Vec::new()));
    tree.attach_script(
        walker,
        Box::new(RecordingScript {
            calls: walker_calls.clone(),
        }),
    );
    tree.attach_script(
        mixer,
        Box::new(RecordingScript {
            calls: mixer_calls.clone(),
        }),
    );

    // ⚠️ The key sits at 0.02s, one frame-and-a-bit in, so it is crossed by the
    // SECOND step (prev 0.0167, curr 0.0333) rather than the first. A key at
    // 0.0 would never be crossed at all — the interval is half-open at the
    // bottom, `kf.time > prev`, exactly as the player's is.
    let mut anim = Animation::new("walk", 1.0);
    let mut method = AnimationTrack::with_type("Walker", "footstep", TrackType::Method);
    method.add_keyframe(KeyFrame::linear(
        0.02,
        Variant::Array(vec![Variant::String("on_footstep".into()), Variant::Int(7)].into()),
    ));
    anim.tracks.push(method);

    let mut anim_tree = AnimationTree::new();
    anim_tree.add_animation(anim);
    anim_tree.set_root(AnimationNode::Animation {
        name: "walk".to_string(),
        position: 0.0,
        opts: AnimationLeafOptions::default(),
    });
    tree.attach_animation_tree(mixer, anim_tree).unwrap();

    let mut ml = MainLoop::new(tree);
    for _ in 0..4 {
        ml.step(DT);
    }

    {
        let calls = walker_calls.borrow();
        assert_eq!(
            calls.len(),
            1,
            "the method key must be dispatched exactly once after crossing it; \
             got {calls:?}"
        );
        assert_eq!(calls[0].0, "on_footstep");
        assert_eq!(
            calls[0].1,
            vec![Variant::Int(7)],
            "the keyframe array's tail is the argument list"
        );
    }
    assert!(
        mixer_calls.borrow().is_empty(),
        "a track naming `Walker` must not call the mixer's own node; got {:?}",
        mixer_calls.borrow()
    );

    // -- the drain discriminator --------------------------------------------
    //
    // Eight more frames take the playhead to ~0.2s, far past the key and
    // nowhere near wrapping a 1.0s clip, so nothing may fire again.
    for _ in 0..8 {
        ml.step(DT);
    }
    assert_eq!(
        walker_calls.borrow().len(),
        1,
        "a crossed key must not re-fire on later frames — the queue has to be \
         DRAINED, not read; got {:?}",
        walker_calls.borrow()
    );
}

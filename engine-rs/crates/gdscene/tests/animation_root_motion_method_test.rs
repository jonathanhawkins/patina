//! pat-z35r2.10 — AnimationMixer root motion + method tracks.
//!
//! Before this bead the `process_animations` pipeline dispatched every method
//! track to the AnimationPlayer's own owner node and accumulated root motion
//! onto that same node, ignoring the track's `node_path` and the player's
//! `root_node`. These app-level tests drive the real pipeline and assert the
//! two Godot behaviors the bead calls for:
//!
//!   * a method track invokes its script method on the node named by the
//!     track `node_path` (resolved relative to `root_node`), not the player;
//!   * a root-motion delta is applied to the Node3D named by `root_node`.

use gdcore::math::Vector3;
use gdscene::animation::{Animation, AnimationPlayer, AnimationTrack, KeyFrame, TrackType};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscript_interop::bindings::{MethodInfo, ScriptError, ScriptInstance, ScriptPropertyInfo};
use gdvariant::Variant;
use std::cell::RefCell;
use std::rc::Rc;

type CallLog = Rc<RefCell<Vec<(String, Vec<Variant>)>>>;

/// Minimal script that records every method call dispatched to it.
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
    fn has_method(&self, name: &str) -> bool {
        name != "_init"
    }
}

fn method_track(
    node_path: &str,
    track_name: &str,
    at: f64,
    method: &str,
    arg: Variant,
) -> AnimationTrack {
    let mut t = AnimationTrack::with_type(node_path, track_name, TrackType::Method);
    t.add_keyframe(KeyFrame::linear(
        at,
        Variant::Array(vec![Variant::String(method.into()), arg].into()),
    ));
    t
}

#[test]
fn method_track_fires_on_targeted_child_node() {
    // Player with a child "Enemy". A method track whose node_path is "Enemy"
    // must invoke the method on Enemy's script, not on the player.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let player_node = tree
        .add_child(root, Node::new("Player", "AnimationPlayer"))
        .unwrap();
    let enemy = tree
        .add_child(player_node, Node::new("Enemy", "Node2D"))
        .unwrap();

    let enemy_calls: CallLog = Rc::new(RefCell::new(Vec::new()));
    let player_calls: CallLog = Rc::new(RefCell::new(Vec::new()));
    tree.attach_script(
        enemy,
        Box::new(RecordingScript {
            calls: enemy_calls.clone(),
        }),
    );
    tree.attach_script(
        player_node,
        Box::new(RecordingScript {
            calls: player_calls.clone(),
        }),
    );

    let mut anim = Animation::new("fx", 2.0);
    anim.tracks.push(method_track(
        "Enemy",
        "on_hit",
        1.0,
        "take_damage",
        Variant::Int(7),
    ));

    let mut player = AnimationPlayer::new();
    player.add_animation(anim);
    player.play("fx");
    tree.attach_animation_player(player_node, player).unwrap();

    // Cross t=1.0 → the method fires exactly once, on the Enemy node.
    tree.process_animations(1.5);

    let ec = enemy_calls.borrow();
    assert_eq!(ec.len(), 1, "method track should fire once on Enemy");
    assert_eq!(ec[0].0, "take_damage");
    assert_eq!(ec[0].1, vec![Variant::Int(7)]);
    assert!(
        player_calls.borrow().is_empty(),
        "player must NOT receive a node-path-targeted method call"
    );

    // Advancing without re-crossing the keyframe does not re-fire it.
    tree.process_animations(0.3);
    assert_eq!(enemy_calls.borrow().len(), 1, "keyframe must not re-fire");
}

#[test]
fn root_motion_applies_to_root_node_target_not_player() {
    // The player's root_node points at a child "Rig" (a Node3D). Root motion
    // must accumulate onto Rig's transform, leaving the player untouched.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let player_node = tree
        .add_child(root, Node::new("Anim", "AnimationPlayer"))
        .unwrap();
    let rig = tree
        .add_child(player_node, Node::new("Rig", "Node3D"))
        .unwrap();

    // A position track used as the root-motion source: 0→(0,0,0), 2→(4,0,0).
    let mut anim = Animation::new("walk", 2.0);
    let mut pos = AnimationTrack::with_type("", "position", TrackType::Property);
    pos.add_keyframe(KeyFrame::linear(0.0, Variant::Vector3(Vector3::ZERO)));
    pos.add_keyframe(KeyFrame::linear(
        2.0,
        Variant::Vector3(Vector3::new(4.0, 0.0, 0.0)),
    ));
    anim.tracks.push(pos);

    let mut player = AnimationPlayer::new();
    player.add_animation(anim);
    player.play("walk");
    player.root_node = "Rig".to_string();
    player.root_motion_track = Some("position".to_string());
    tree.attach_animation_player(player_node, player).unwrap();

    // At t=1.0 the linear delta is half of (4,0,0) = (2,0,0).
    tree.process_animations(1.0);

    // -- the mixer PUBLISHES the motion (pat-mgixk.78) ----------------------
    //
    // ⚠️ This clause used to assert that `Rig.position.x` had become 2.0 — that
    // root motion MOVED the node. Godot's `AnimationMixer` never does
    // (`AnimationMixer.xml:323-326`): it cancels the root track from the pose,
    // reports the motion through `get_root_motion_position()` and leaves the
    // user to apply it. The old assertion pinned the double-move this bead
    // removes, so it is inverted here rather than deleted — the delta still has
    // to be the (2,0,0) the clip travelled, or the feature is simply gone.
    let rm = *tree
        .get_animation_player(player_node)
        .unwrap()
        .root_motion();
    assert!(
        (rm.position_delta.x - 2.0).abs() < 1e-3
            && rm.position_delta.y.abs() < 1e-3
            && rm.position_delta.z.abs() < 1e-3,
        "the mixer must publish the clip's travel for this frame, got {:?}",
        rm.position_delta
    );

    // -- and NEITHER node moved --------------------------------------------
    //
    // `Rig` is the `root_node` the motion was aimed at, so it is the node the
    // old behaviour would have moved; the player is the one it must never have
    // moved. Both are checked, because "the mixer moves nothing" is the whole
    // claim and testing only one of them would leave the other free to drift.
    assert!(
        matches!(
            tree.get_node(rig).unwrap().get_property("position"),
            Variant::Nil
        ),
        "the mixer must not write a position onto its root_node target: {:?}",
        tree.get_node(rig).unwrap().get_property("position")
    );
    assert!(
        matches!(
            tree.get_node(player_node).unwrap().get_property("position"),
            Variant::Nil
        ),
        "player node must not receive root motion targeted at root_node"
    );
}

#[test]
fn method_track_empty_path_targets_owner_node() {
    // Regression guard: with the default root_node ("."), a method track whose
    // node_path is empty still fires on the player's own owner node.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let player_node = tree
        .add_child(root, Node::new("P", "AnimationPlayer"))
        .unwrap();

    let calls: CallLog = Rc::new(RefCell::new(Vec::new()));
    tree.attach_script(
        player_node,
        Box::new(RecordingScript {
            calls: calls.clone(),
        }),
    );

    let mut anim = Animation::new("a", 2.0);
    anim.tracks.push(method_track(
        "",
        "footstep",
        1.0,
        "on_footstep",
        Variant::Int(3),
    ));

    let mut player = AnimationPlayer::new();
    player.add_animation(anim);
    player.play("a");
    tree.attach_animation_player(player_node, player).unwrap();

    tree.process_animations(1.5);

    let c = calls.borrow();
    assert_eq!(c.len(), 1, "empty node_path method should fire on owner");
    assert_eq!(c[0].0, "on_footstep");
    assert_eq!(c[0].1, vec![Variant::Int(3)]);
}

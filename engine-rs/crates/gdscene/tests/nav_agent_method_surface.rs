//! pat-s3rrv.20: the `NavigationAgent2D` / `NavigationAgent3D` method surface.
//!
//! `nav_agent_script_api_moves_parent_body` proves the surface is *reachable
//! from a script* and drives a real body. This file pins the answers themselves
//! — the shapes, defaults and edge cases a script depends on — at the dispatch
//! layer, where a wrong value is reported as a wrong value rather than as a body
//! that ended up in the wrong place.
//!
//! Both matter: a method that returns `Nil` where Godot returns an empty array
//! crashes the *caller's* next line (`.size()`), not this one, so the defaults
//! are load-bearing.

use gdcore::math::{Vector2, Vector3};
use gdscene::navigation_agents::{call_agent_method, published};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// A body at `origin` with a `NavigationAgent2D` child at local (0,0).
fn agent_2d_scene(origin: Vector2) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut body = Node::new("Body", "CharacterBody2D");
    body.set_property("position", Variant::Vector2(origin));
    let body_id = tree.add_child(root, body).unwrap();
    let mut agent = Node::new("Agent", "NavigationAgent2D");
    agent.set_property("position", Variant::Vector2(Vector2::ZERO));
    let agent_id = tree.add_child(body_id, agent).unwrap();
    (tree, agent_id)
}

fn call(tree: &mut SceneTree, node: NodeId, method: &str, args: &[Variant]) -> Variant {
    call_agent_method(tree, node, method, args)
        .unwrap_or_else(|| panic!("`{method}` must be part of the agent method surface"))
}

/// A node that is not an agent falls through untouched, so the existing node
/// method table keeps its behaviour and its diagnostics.
#[test]
fn a_non_agent_node_is_not_claimed_by_the_agent_dispatcher() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let sprite = tree
        .add_child(root, Node::new("Sprite", "Sprite2D"))
        .unwrap();
    assert!(
        call_agent_method(&mut tree, sprite, "get_next_path_position", &[]).is_none(),
        "only NavigationAgent2D/3D may be claimed — returning Some here would \
         shadow every other node's methods",
    );
    // …and an agent still rejects a method that is not part of its surface,
    // rather than swallowing it and returning Nil.
    let (mut tree, agent) = agent_2d_scene(Vector2::ZERO);
    assert!(
        call_agent_method(&mut tree, agent, "queue_free", &[]).is_none(),
        "an unrelated method must fall through to the normal table",
    );
}

/// `distance_to_target` is live geometry measured from the agent's GLOBAL
/// position, which is what makes it follow the parent body between frames.
#[test]
fn distance_to_target_measures_from_the_agents_global_position() {
    let (mut tree, agent) = agent_2d_scene(Vector2::new(10.0, 30.0));
    tree.get_node_mut(agent).unwrap().set_property(
        "target_position",
        Variant::Vector2(Vector2::new(180.0, 30.0)),
    );

    match call(&mut tree, agent, "distance_to_target", &[]) {
        Variant::Float(d) => assert!(
            (d - 170.0).abs() < 0.001,
            "expected 170 from (10,30) to (180,30), got {d}",
        ),
        other => panic!("distance_to_target must return a float, got {other:?}"),
    }

    // Move the BODY: the agent's local position never changes, but the answer
    // must, because the agent rides its parent.
    let body = tree.get_node(agent).unwrap().parent().unwrap();
    tree.get_node_mut(body)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(80.0, 30.0)));
    match call(&mut tree, agent, "distance_to_target", &[]) {
        Variant::Float(d) => assert!(
            (d - 100.0).abs() < 0.001,
            "moving the body must change the distance; expected 100, got {d}",
        ),
        other => panic!("expected float, got {other:?}"),
    }
}

/// Getters answer with Godot's empty/zero values before any physics frame has
/// published state — never `Nil`, which would crash the caller's next chained
/// call rather than this one.
#[test]
fn unpublished_getters_answer_with_godot_empties_not_nil() {
    let (mut tree, agent) = agent_2d_scene(Vector2::new(5.0, 7.0));

    match call(&mut tree, agent, "get_current_navigation_path", &[]) {
        Variant::PackedVector2Array(p) => {
            assert!(p.is_empty(), "expected an empty path, got {p:?}")
        }
        other => panic!("a 2D agent must answer with a PackedVector2Array, got {other:?}"),
    }
    assert_eq!(
        call(&mut tree, agent, "get_current_navigation_path_index", &[]),
        Variant::Int(0),
    );
    assert_eq!(
        call(&mut tree, agent, "get_path_length", &[]),
        Variant::Float(0.0),
    );
    // With nothing published, "where should I go next" and "how far can I get"
    // are both "where I already am".
    assert_eq!(
        call(&mut tree, agent, "get_next_path_position", &[]),
        Variant::Vector2(Vector2::new(5.0, 7.0)),
    );
    assert_eq!(
        call(&mut tree, agent, "get_final_position", &[]),
        Variant::Vector2(Vector2::new(5.0, 7.0)),
    );
    assert_eq!(
        call(&mut tree, agent, "is_navigation_finished", &[]),
        Variant::Bool(false),
    );
}

/// `get_path_length` measures the whole path, summed over its segments.
#[test]
fn get_path_length_sums_the_published_path() {
    let (mut tree, agent) = agent_2d_scene(Vector2::ZERO);
    // An L: 30 across, then 40 down => 70, and deliberately NOT the straight-line
    // distance (50) between the endpoints, so a shortcut implementation fails.
    tree.get_node_mut(agent).unwrap().set_property(
        published::PATH,
        Variant::PackedVector2Array(vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(30.0, 0.0),
            Vector2::new(30.0, 40.0),
        ]),
    );
    match call(&mut tree, agent, "get_path_length", &[]) {
        Variant::Float(len) => assert!(
            (len - 70.0).abs() < 0.001,
            "path length must sum the segments (70), not the straight line (50); got {len}",
        ),
        other => panic!("expected float, got {other:?}"),
    }
}

/// Godot's layer accessors are 1-based and round-trip through the node's mask.
#[test]
fn avoidance_layer_bits_are_one_based_and_round_trip() {
    let (mut tree, agent) = agent_2d_scene(Vector2::ZERO);

    // Godot's default mask is layer 1 only.
    assert_eq!(
        call(
            &mut tree,
            agent,
            "get_avoidance_layer_value",
            &[Variant::Int(1)]
        ),
        Variant::Bool(true),
        "layer 1 is set by default",
    );
    assert_eq!(
        call(
            &mut tree,
            agent,
            "get_avoidance_layer_value",
            &[Variant::Int(3)]
        ),
        Variant::Bool(false),
    );

    call(
        &mut tree,
        agent,
        "set_avoidance_layer_value",
        &[Variant::Int(3), Variant::Bool(true)],
    );
    assert_eq!(
        call(
            &mut tree,
            agent,
            "get_avoidance_layer_value",
            &[Variant::Int(3)]
        ),
        Variant::Bool(true),
        "setting layer 3 must be observable",
    );
    // Bit 3 is 0b100 = 4, plus the default bit 1 => 5. Asserting the raw mask
    // catches an off-by-one that a get/set round-trip alone would hide.
    assert_eq!(
        tree.get_node(agent)
            .unwrap()
            .get_property("avoidance_layers"),
        Variant::Int(5),
        "layer N must map to bit N-1, not bit N",
    );

    call(
        &mut tree,
        agent,
        "set_avoidance_layer_value",
        &[Variant::Int(1), Variant::Bool(false)],
    );
    assert_eq!(
        tree.get_node(agent)
            .unwrap()
            .get_property("avoidance_layers"),
        Variant::Int(4),
        "clearing a layer must clear only its own bit",
    );

    // Out of range: Godot ignores it rather than wrapping into another layer.
    call(
        &mut tree,
        agent,
        "set_avoidance_layer_value",
        &[Variant::Int(0), Variant::Bool(true)],
    );
    assert_eq!(
        tree.get_node(agent)
            .unwrap()
            .get_property("avoidance_layers"),
        Variant::Int(4),
        "layer 0 is out of range (layers are 1..=32) and must change nothing",
    );
}

/// `set_target_position` lands on the node property the runtime reads, which is
/// what makes a script-set target take effect on the next tick.
#[test]
fn set_target_position_writes_the_property_the_runtime_reads() {
    let (mut tree, agent) = agent_2d_scene(Vector2::ZERO);
    let goal = Vector2::new(64.0, -12.0);
    call(
        &mut tree,
        agent,
        "set_target_position",
        &[Variant::Vector2(goal)],
    );
    assert_eq!(
        tree.get_node(agent)
            .unwrap()
            .get_property("target_position"),
        Variant::Vector2(goal),
        "process_navigation reads `target_position` off the node; a setter that \
         writes anywhere else is inert",
    );
    assert_eq!(
        call(&mut tree, agent, "get_target_position", &[]),
        Variant::Vector2(goal),
    );
}

/// The 3D agent answers in 3D types throughout — a 2D-shaped answer would fail
/// a script's vector arithmetic at the call site.
#[test]
fn the_3d_agent_answers_with_3d_types() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut body = Node::new("Body", "CharacterBody3D");
    body.set_property("position", Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)));
    let body_id = tree.add_child(root, body).unwrap();
    let mut agent = Node::new("Agent", "NavigationAgent3D");
    agent.set_property("position", Variant::Vector3(Vector3::ZERO));
    let agent_id = tree.add_child(body_id, agent).unwrap();

    assert_eq!(
        call(&mut tree, agent_id, "get_next_path_position", &[]),
        Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)),
        "an unpublished 3D agent answers with its own global position",
    );
    match call(&mut tree, agent_id, "get_current_navigation_path", &[]) {
        Variant::PackedVector3Array(p) => assert!(p.is_empty()),
        other => panic!("a 3D agent must answer with a PackedVector3Array, got {other:?}"),
    }
    let goal = Vector3::new(9.0, 2.0, 3.0);
    call(
        &mut tree,
        agent_id,
        "set_target_position",
        &[Variant::Vector3(goal)],
    );
    match call(&mut tree, agent_id, "distance_to_target", &[]) {
        Variant::Float(d) => assert!((d - 8.0).abs() < 0.001, "expected 8, got {d}"),
        other => panic!("expected float, got {other:?}"),
    }
}

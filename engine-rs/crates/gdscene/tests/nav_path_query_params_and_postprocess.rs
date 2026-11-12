//! pat-s3rrv.181 — `simplify_path` / `simplify_epsilon` authored on an agent
//! actually shorten its runtime path.
//!
//! Ground truth: Godot's `NavigationPathQueryParameters*.simplify_path` runs the
//! computed path through a Ramer-Douglas-Peucker reduction with
//! `simplify_epsilon` as the tolerance; the same knobs exist on
//! `NavigationAgent2D/3D` and shape the path the agent follows.
//!
//! # Scope — this covers one of the bead's six steps
//!
//! `nav_simplify::simplify_path_2d` / `_3d` were implemented and unit-tested
//! with **zero callers**, and `simplify_path` / `simplify_epsilon` were
//! registered in ClassDB and read by nobody. This file pins the connection
//! between them.
//!
//! **Not covered here**, and not implemented:
//!
//! - `NavigationServer2D/3D.query_path()` and script-side `.new()` for the
//!   query objects. The four `NavigationPathQuery*` classes ARE now registered
//!   (see `gdobject/tests/nav_path_query_classes_registered.rs`), but
//!   `Interpreter::classdb_instantiate` delegates every non-Node class to a
//!   bespoke `ClassName.new()` arm, so registration alone does not make them
//!   constructible from GDScript.
//! - The 3D corridor funnel. 3D paths still chain polygon centroids
//!   (`navigation.rs:1812-1817`, `:1840-1845`) where Godot's default
//!   `CORRIDORFUNNEL` gives a taut line.

use gdcore::math::Vector2;
use gdscene::navigation::{NavMesh2D, NavPolygon, NavigationAgent2D, NavigationServer2D};
use gdscene::navigation_agents::sync_tuning_2d;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// An L-bend of three squares, so the funnelled path turns a corner and has
/// interior waypoints worth reducing.
fn l_bend_mesh() -> NavMesh2D {
    let p0 = NavPolygon::new(vec![
        Vector2::new(0.0, 0.0),
        Vector2::new(10.0, 0.0),
        Vector2::new(10.0, 10.0),
        Vector2::new(0.0, 10.0),
    ]);
    let p1 = NavPolygon::new(vec![
        Vector2::new(0.0, 10.0),
        Vector2::new(10.0, 10.0),
        Vector2::new(10.0, 20.0),
        Vector2::new(0.0, 20.0),
    ]);
    let p2 = NavPolygon::new(vec![
        Vector2::new(10.0, 10.0),
        Vector2::new(20.0, 10.0),
        Vector2::new(20.0, 20.0),
        Vector2::new(10.0, 20.0),
    ]);
    NavMesh2D::new(vec![p0, p1, p2], 0.01)
}

fn server() -> NavigationServer2D {
    let mut s = NavigationServer2D::new();
    s.add_region(l_bend_mesh());
    s
}

/// An agent node carrying the given scene-authored properties.
fn agent_with(props: &[(&str, Variant)]) -> NavigationAgent2D {
    let mut scene = SceneTree::new();
    let root = scene.root_id();
    let node = scene
        .add_child(root, Node::new("Agent", "NavigationAgent2D"))
        .unwrap();
    if let Some(n) = scene.get_node_mut(node) {
        for (k, v) in props {
            n.set_property(k, v.clone());
        }
    }
    let mut agent = NavigationAgent2D::new();
    sync_tuning_2d(&scene, node, &mut agent);
    agent
}

/// 🔑 THE ACCEPTANCE (the simplify clause): the scene knobs reach the reducer.
#[test]
fn nav_path_query_params_and_postprocess() {
    let start = Vector2::new(5.0, 5.0);
    let target = Vector2::new(18.0, 15.0);
    let srv = server();

    // Baseline: no simplification authored.
    let mut plain = agent_with(&[]);
    plain.set_target_position(target, start, &srv);
    let plain_len = plain.path.len();
    assert!(
        plain_len >= 3,
        "the L-bend must give a multi-waypoint path to reduce, got {:?}",
        plain.path
    );

    // A large epsilon collapses the corner.
    let mut simplified = agent_with(&[
        ("simplify_path", Variant::Bool(true)),
        ("simplify_epsilon", Variant::Float(8.0)),
    ]);
    assert!(
        simplified.simplify_path,
        "sync_tuning_2d must copy simplify_path off the node"
    );
    assert_eq!(
        simplified.simplify_epsilon, 8.0,
        "and simplify_epsilon with it"
    );

    simplified.set_target_position(target, start, &srv);
    assert!(
        simplified.path.len() < plain_len,
        "an authored simplify_epsilon must shorten the waypoint list: {} vs {} \
         ({:?})",
        simplified.path.len(),
        plain_len,
        simplified.path
    );
    // RDP never drops the endpoints.
    assert!(
        simplified.path.len() >= 2,
        "the start and goal survive any reduction"
    );
}

/// ⚠️ The control. Without it, the acceptance would also pass on a build that
/// simplified unconditionally — which would silently reduce every path in every
/// project that never asked for it.
#[test]
fn an_agent_without_the_flag_keeps_its_full_path() {
    let start = Vector2::new(5.0, 5.0);
    let target = Vector2::new(18.0, 15.0);
    let srv = server();

    let mut plain = agent_with(&[]);
    plain.set_target_position(target, start, &srv);

    // Epsilon authored but the flag left off: still no reduction.
    let mut epsilon_only = agent_with(&[("simplify_epsilon", Variant::Float(8.0))]);
    epsilon_only.set_target_position(target, start, &srv);

    assert_eq!(
        epsilon_only.path.len(),
        plain.path.len(),
        "simplify_epsilon alone must not reduce anything — the flag is the switch"
    );
}

/// ⚠️ `simplify_path = true` with the default epsilon of **0.0** must not
/// reduce.
///
/// Zero is Godot's default, and RDP with a zero tolerance removes only
/// exactly-collinear points. Running it would spend the walk to change nothing
/// in the common case, and would drop waypoints a caller placed deliberately on
/// a straight run. The guard is `simplify_path && simplify_epsilon > 0.0`, and
/// this is the half of it that a flag-only check would get wrong.
#[test]
fn the_flag_without_a_tolerance_is_a_no_op() {
    let start = Vector2::new(5.0, 5.0);
    let target = Vector2::new(18.0, 15.0);
    let srv = server();

    let mut plain = agent_with(&[]);
    plain.set_target_position(target, start, &srv);

    let mut flag_only = agent_with(&[("simplify_path", Variant::Bool(true))]);
    assert_eq!(
        flag_only.simplify_epsilon, 0.0,
        "the default tolerance is zero"
    );
    flag_only.set_target_position(target, start, &srv);

    assert_eq!(
        flag_only.path.len(),
        plain.path.len(),
        "a zero tolerance leaves the path alone"
    );
}

/// An absent property keeps the engine default rather than reading as `false`/0.
///
/// ⚠️ `sync_tuning_2d` must distinguish "the .tscn never mentioned this" from
/// "the .tscn authored 0.0" — the existing `tuned` helper is built around that
/// rule, and the new bool reader has to honour it too.
#[test]
fn absent_properties_keep_the_defaults() {
    let agent = agent_with(&[]);
    assert!(!agent.simplify_path);
    assert_eq!(agent.simplify_epsilon, 0.0);

    // A node that authors only one of the pair leaves the other at its default.
    let only_flag = agent_with(&[("simplify_path", Variant::Bool(true))]);
    assert!(only_flag.simplify_path);
    assert_eq!(only_flag.simplify_epsilon, 0.0);
}

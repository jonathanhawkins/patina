//! pat-s3rrv.219 — XR nodes are driven from XRServer's trackers each frame.
//!
//! All five XR node classes were ClassDB-registered and did **nothing**:
//! `rg 'XR' crates/gdscene/src` matched three comment lines, `XROrigin3D.world_scale`
//! reached no code at all, and no method of any XR node was dispatchable. The
//! only green XR node test asserted registration and camera selection — nothing
//! about tracking.
//!
//! Two prerequisites of this bead's own acceptance were also missing and are
//! added with it: `XRPositionalTracker` had **no pose or input storage**
//! (only hand-joint data), and `XRControllerTracker` — the class the acceptance
//! names — was not constructible from script at all.
//!
//! ⚠️ `tracking_changed` is an **edge**. `a_second_frame_does_not_refire_tracking_changed`
//! is the control that pins that: a level-triggered implementation would fire
//! it every frame forever and every other assertion here would still pass.

use gdcore::math::Vector3;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// Registers the tracker and reads the controller back through script, so the
/// whole path is exercised the way a game would: no Rust helper in between.
const SCRIPT: &str = "\
var changed = 0
var tracking_before = true
func _ready():
    var c = get_node(\"XROrigin3D/XRController3D\")
    c.connect(\"tracking_changed\", _on_changed)
    self.tracking_before = c.get_has_tracking_data()
func _on_changed():
    changed += 1
func register():
    var t = XRControllerTracker.new()
    t.set_tracker_name(\"left_hand\")
    t.set_pose(\"default\", Transform3D(Basis.from_euler(Vector3(0, 0, 0), EULER_ORDER_XYZ), Vector3(0.3, 1.2, -0.4)), Vector3(0, 0, 0), Vector3(0, 0, 0), 2)
    t.set_input(\"trigger\", true)
    XRServer.add_tracker(t)
func probe():
    var c = get_node(\"XROrigin3D/XRController3D\")
    return [c.get_has_tracking_data(), c.get_is_active(), c.is_button_pressed(\"trigger\"), c.is_button_pressed(\"grip\")]
";

/// `root(script) > XROrigin3D > { XRCamera3D, XRController3D }`.
fn scene(world_scale: Option<f64>) -> (MainLoop, NodeId, NodeId) {
    // ⚠️ Load-bearing. `global_db()` hands back an EMPTY ClassDB and never
    // populates itself, so without this `is_class("XRNode3D")` is false for
    // every node, the frame walk matches nothing, and every assertion below
    // fails for a reason that has nothing to do with XR.
    gdobject::class_db::register_all_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let origin = tree
        .add_child(root, Node::new("XROrigin3D", "XROrigin3D"))
        .unwrap();
    if let Some(scale) = world_scale {
        tree.get_node_mut(origin)
            .unwrap()
            .set_property("world_scale", Variant::Float(scale));
    }
    tree.add_child(origin, Node::new("XRCamera3D", "XRCamera3D"))
        .unwrap();
    let controller = tree
        .add_child(origin, Node::new("XRController3D", "XRController3D"))
        .unwrap();
    {
        let n = tree.get_node_mut(controller).unwrap();
        n.set_property("tracker", Variant::String("left_hand".into()));
        n.set_property("show_when_tracked", Variant::Bool(true));
    }
    let script = GDScriptNodeInstance::from_source(SCRIPT, root).expect("script parses");
    tree.attach_script(root, Box::new(script));
    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(root);
    (ml, root, controller)
}

fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    use gdscript_interop::bindings::ScriptInstance;
    tree.get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

fn origin_of(ml: &MainLoop, id: NodeId) -> Vector3 {
    gdscene::node3d::get_global_transform(ml.tree(), id).origin
}

fn approx(got: Vector3, want: Vector3, what: &str) {
    let d = (got.x - want.x).abs() + (got.y - want.y).abs() + (got.z - want.z).abs();
    assert!(
        d < 1e-4,
        "{what}: expected {want:?}, got {got:?} (sum-abs delta {d})"
    );
}

/// Registers the tracker through the script, then steps once.
fn register_and_step(ml: &mut MainLoop, root: NodeId) {
    ml.tree_mut().call_script_method(root, "register", &[]);
    ml.step(1.0 / 60.0);
}

#[test]
fn before_a_pose_exists_the_node_is_untracked_and_hidden() {
    let (mut ml, root, controller) = scene(None);
    assert_eq!(
        script_var(ml.tree(), root, "tracking_before"),
        Variant::Bool(false),
        "with no tracker registered `get_has_tracking_data()` must be false"
    );
    ml.step(1.0 / 60.0);
    assert!(
        !gdscene::node3d::is_visible(ml.tree(), controller),
        "`show_when_tracked` must hide the node until a pose arrives"
    );
}

#[test]
fn a_registered_pose_moves_the_controller_in_the_scene() {
    let (mut ml, root, controller) = scene(None);
    register_and_step(&mut ml, root);
    approx(
        origin_of(&ml, controller),
        Vector3::new(0.3, 1.2, -0.4),
        "the controller must sit at the tracker's pose after one frame",
    );
    assert!(
        gdscene::node3d::is_visible(ml.tree(), controller),
        "`show_when_tracked` must reveal the node once tracking starts"
    );
}

#[test]
fn world_scale_scales_the_pose() {
    let (mut ml, root, controller) = scene(Some(2.0));
    register_and_step(&mut ml, root);
    approx(
        origin_of(&ml, controller),
        Vector3::new(0.6, 2.4, -0.8),
        "XROrigin3D.world_scale = 2.0 must double the pose offset",
    );
}

#[test]
fn tracking_changed_fires_exactly_once() {
    let (mut ml, root, _) = scene(None);
    register_and_step(&mut ml, root);
    assert_eq!(
        script_var(ml.tree(), root, "changed"),
        Variant::Int(1),
        "`tracking_changed` must fire on the transition into tracking"
    );
}

#[test]
fn a_second_frame_does_not_refire_tracking_changed() {
    // ⚠️ The control that makes the test above meaningful: a level-triggered
    // implementation fires every frame and still passes `== 1` after one step.
    let (mut ml, root, _) = scene(None);
    register_and_step(&mut ml, root);
    for _ in 0..5 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        script_var(ml.tree(), root, "changed"),
        Variant::Int(1),
        "`tracking_changed` is an edge — five more frames must not re-fire it"
    );
}

#[test]
fn the_controllers_script_surface_reads_the_tracker() {
    let (mut ml, root, _) = scene(None);
    register_and_step(&mut ml, root);
    let probe = ml
        .tree_mut()
        .call_script_method(root, "probe", &[])
        .expect("probe() returns");
    let Variant::Array(v) = probe else {
        panic!("probe() should return an array, got {probe:?}");
    };
    assert_eq!(
        v.to_vec()[0],
        Variant::Bool(true),
        "get_has_tracking_data()"
    );
    assert_eq!(v.to_vec()[1], Variant::Bool(true), "get_is_active()");
    assert_eq!(
        v.to_vec()[2],
        Variant::Bool(true),
        "is_button_pressed(\"trigger\") must reflect the tracker's input map"
    );
    assert_eq!(
        v.to_vec()[3],
        Variant::Bool(false),
        "an input the tracker never set must be false, not true-by-default"
    );
}

#[test]
fn a_node_with_no_tracker_is_left_alone() {
    // Negative control for the walk: an XRNode3D naming no tracker must not be
    // moved or hidden, and must not fire tracking_changed.
    let (mut ml, root, _) = scene(None);
    let stray = ml
        .tree_mut()
        .add_child(root, Node::new("Stray", "XRController3D"))
        .unwrap();
    ml.tree_mut()
        .get_node_mut(stray)
        .unwrap()
        .set_property("position", Variant::Vector3(Vector3::new(7.0, 8.0, 9.0)));
    register_and_step(&mut ml, root);
    approx(
        origin_of(&ml, stray),
        Vector3::new(7.0, 8.0, 9.0),
        "an XRNode3D with no tracker must keep its authored transform",
    );
}

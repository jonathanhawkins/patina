//! A running game must not grow memory without end.
//!
//! Each node kept every notification it ever received, and every frame sends
//! PROCESS / PHYSICS_PROCESS (and the INTERNAL_ twins) to every node. On the
//! platformer demo (491 nodes) a 3-minute soak in the web editor grew the
//! process ~45 MB a minute: 491 heap blocks of exactly 528 KB, one per node,
//! still doubling.

use gdobject::notification::Notification;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NOTIFICATION_LOG_CAP};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;

#[test]
fn a_nodes_notification_log_is_bounded_and_keeps_the_newest_entries() {
    let mut node = Node::new("N", "Node");
    let n = NOTIFICATION_LOG_CAP * 10 + 7;
    for i in 0..n {
        node.receive_notification(Notification::new(i as i32));
    }
    let log = node.notification_log();
    assert!(
        log.len() <= NOTIFICATION_LOG_CAP,
        "log grew to {}",
        log.len()
    );
    assert!(
        log.len() >= NOTIFICATION_LOG_CAP / 2,
        "too little history kept: {}",
        log.len()
    );
    // The newest entries survive, in order.
    let tail: Vec<i32> = log.iter().rev().take(3).map(|x| x.code()).collect();
    assert_eq!(tail, vec![n as i32 - 1, n as i32 - 2, n as i32 - 3]);
}

#[test]
fn a_short_run_still_records_every_notification_from_the_start() {
    // What the lifecycle and oracle tests read: under the cap nothing drops.
    let mut node = Node::new("N", "Node");
    let before = node.notification_log().len();
    for i in 0..100 {
        node.receive_notification(Notification::new(i));
    }
    let codes: Vec<i32> = node.notification_log().iter().map(|x| x.code()).collect();
    // Nothing from the start is lost: the first entry is still the first one
    // the node ever got, and every one of the 100 follows.
    assert_eq!(codes.len(), before + 100);
    assert!(
        codes.ends_with(&(0..100).collect::<Vec<_>>()),
        "the full short history is kept"
    );
}

#[test]
fn ten_thousand_frames_of_a_processing_scene_keep_every_log_under_the_cap() {
    gdobject::class_db::register_all_classes();
    let src = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="A" type="Node2D" parent="."]

[node name="B" type="Sprite2D" parent="A"]
"#;
    let scene = PackedScene::from_tscn(src).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let mut ml = MainLoop::new(tree);
    for _ in 0..10_000 {
        ml.step(1.0 / 60.0);
    }
    let tree = ml.tree();
    let mut ids = Vec::new();
    tree.collect_subtree_top_down(tree.root_id(), &mut ids);
    let longest = ids
        .iter()
        .map(|&id| tree.get_node(id).unwrap().notification_log().len())
        .max()
        .unwrap();
    assert!(
        longest <= NOTIFICATION_LOG_CAP,
        "after 10k frames a node holds {longest} notifications (cap {NOTIFICATION_LOG_CAP})"
    );
    // Not vacuous: the frames really delivered more than the cap would hold.
    assert!(
        longest >= NOTIFICATION_LOG_CAP / 2,
        "frames delivered only {longest} notifications"
    );
}

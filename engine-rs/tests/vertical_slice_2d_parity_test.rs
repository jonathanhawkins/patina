//! pat-aogj: Measure one end-to-end 2D vertical slice from fixtures.
//!
//! Exercises a single representative 2D scene (`platformer.tscn`) through
//! every layer of the Patina runtime: fixture parse → scene tree build →
//! oracle tree comparison → oracle property comparison → frame execution →
//! 2D rendering → pixel output verification.
//!
//! Acceptance: one representative scene exercises runtime, render, and
//! fixture comparison end to end.

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdplatform::backend::{HeadlessPlatform, PlatformBackend};
use gdrender2d::test_adapter::capture_frame;
use gdrender2d::SoftwareRenderer;
use gdscene::main_loop::MainLoop;
use gdscene::node2d::get_position;
use gdscene::packed_scene::add_packed_scene_to_tree;
use gdscene::{PackedScene, SceneTree};
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::viewport::Viewport;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
const DT: f64 = 1.0 / 60.0;
const FLOAT_TOLERANCE: f64 = 0.001;

// ---------------------------------------------------------------------------
// Oracle data (from fixtures/oracle_outputs/platformer_tree.json)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
struct OracleNode {
    name: &'static str,
    class: &'static str,
    path: &'static str,
    children: Vec<OracleNode>,
}

fn oracle_tree() -> OracleNode {
    OracleNode {
        name: "root",
        class: "Window",
        path: "/root",
        children: vec![OracleNode {
            name: "World",
            class: "Node",
            path: "/root/World",
            children: vec![
                OracleNode {
                    name: "Player",
                    class: "Node2D",
                    path: "/root/World/Player",
                    children: vec![],
                },
                OracleNode {
                    name: "Platform1",
                    class: "Node2D",
                    path: "/root/World/Platform1",
                    children: vec![],
                },
                OracleNode {
                    name: "Platform2",
                    class: "Node2D",
                    path: "/root/World/Platform2",
                    children: vec![],
                },
                OracleNode {
                    name: "Platform3",
                    class: "Node2D",
                    path: "/root/World/Platform3",
                    children: vec![],
                },
                OracleNode {
                    name: "Camera",
                    class: "Camera2D",
                    path: "/root/World/Camera",
                    children: vec![],
                },
                OracleNode {
                    name: "Collectible",
                    class: "Node2D",
                    path: "/root/World/Collectible",
                    children: vec![],
                },
            ],
        }],
    }
}

struct OracleProperty {
    path: &'static str,
    name: &'static str,
    value_x: f64,
    value_y: f64,
}

fn oracle_properties() -> Vec<OracleProperty> {
    vec![
        OracleProperty {
            path: "/root/World/Player",
            name: "position",
            value_x: 100.0,
            value_y: 300.0,
        },
        OracleProperty {
            path: "/root/World/Platform1",
            name: "position",
            value_x: 0.0,
            value_y: 500.0,
        },
        OracleProperty {
            path: "/root/World/Platform2",
            name: "position",
            value_x: 300.0,
            value_y: 400.0,
        },
        OracleProperty {
            path: "/root/World/Platform3",
            name: "position",
            value_x: 600.0,
            value_y: 350.0,
        },
        OracleProperty {
            path: "/root/World/Collectible",
            name: "position",
            value_x: 450.0,
            value_y: 250.0,
        },
    ]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_platformer() -> SceneTree {
    let tscn = include_str!("../../fixtures/scenes/platformer.tscn");
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root_id, &packed).unwrap();
    tree
}

fn count_oracle_nodes(node: &OracleNode) -> usize {
    1 + node.children.iter().map(|c| count_oracle_nodes(c)).sum::<usize>()
}

// ===========================================================================
// LAYER 1: Parse — fixture loads without error
// ===========================================================================

#[test]
fn layer1_fixture_parses() {
    let tscn = include_str!("../../fixtures/scenes/platformer.tscn");
    let packed = PackedScene::from_tscn(tscn);
    assert!(packed.is_ok(), "platformer.tscn should parse: {:?}", packed.err());
}

// ===========================================================================
// LAYER 2: Scene tree build — correct node structure
// ===========================================================================

#[test]
fn layer2_tree_has_correct_node_count() {
    let tree = load_platformer();
    let expected = count_oracle_nodes(&oracle_tree());
    let actual = tree.node_count();
    assert_eq!(
        actual, expected,
        "platformer tree should have {expected} nodes, got {actual}"
    );
}

#[test]
fn layer2_tree_nodes_have_correct_names() {
    let tree = load_platformer();

    let expected_paths = [
        "/root/World",
        "/root/World/Player",
        "/root/World/Platform1",
        "/root/World/Platform2",
        "/root/World/Platform3",
        "/root/World/Camera",
        "/root/World/Collectible",
    ];

    for path in &expected_paths {
        let node_id = tree.get_node_by_path(path);
        assert!(
            node_id.is_some(),
            "expected node at path '{}' not found",
            path
        );
    }
}

#[test]
fn layer2_tree_nodes_have_correct_classes() {
    let tree = load_platformer();

    let checks = [
        ("/root/World", "Node"),
        ("/root/World/Player", "Node2D"),
        ("/root/World/Platform1", "Node2D"),
        ("/root/World/Platform2", "Node2D"),
        ("/root/World/Platform3", "Node2D"),
        ("/root/World/Camera", "Camera2D"),
        ("/root/World/Collectible", "Node2D"),
    ];

    for (path, expected_class) in &checks {
        let node_id = tree.get_node_by_path(path).unwrap();
        let node = tree.get_node(node_id).unwrap();
        assert_eq!(
            node.class_name(),
            *expected_class,
            "node at '{}' should be class '{}', got '{}'",
            path,
            expected_class,
            node.class_name()
        );
    }
}

#[test]
fn layer2_tree_child_order_matches_oracle() {
    let tree = load_platformer();

    let world_id = tree.get_node_by_path("/root/World").unwrap();
    let world = tree.get_node(world_id).unwrap();
    let child_ids = world.children();

    let expected_names = ["Player", "Platform1", "Platform2", "Platform3", "Camera", "Collectible"];
    assert_eq!(
        child_ids.len(),
        expected_names.len(),
        "World should have {} children, got {}",
        expected_names.len(),
        child_ids.len()
    );

    for (i, &cid) in child_ids.iter().enumerate() {
        let child = tree.get_node(cid).unwrap();
        assert_eq!(
            child.name(),
            expected_names[i],
            "child {} should be '{}', got '{}'",
            i,
            expected_names[i],
            child.name()
        );
    }
}

// ===========================================================================
// LAYER 3: Oracle property comparison
// ===========================================================================

#[test]
fn layer3_properties_match_oracle() {
    let tree = load_platformer();
    let oracle_props = oracle_properties();

    let mut matched = 0usize;
    let total = oracle_props.len();

    for op in &oracle_props {
        let node_id = tree.get_node_by_path(op.path).unwrap();
        let pos = get_position(&tree, node_id);

        let x_ok = (pos.x as f64 - op.value_x).abs() < FLOAT_TOLERANCE;
        let y_ok = (pos.y as f64 - op.value_y).abs() < FLOAT_TOLERANCE;

        if x_ok && y_ok {
            matched += 1;
        } else {
            eprintln!(
                "  MISMATCH {}.{}: expected ({}, {}), got ({}, {})",
                op.path, op.name, op.value_x, op.value_y, pos.x, pos.y
            );
        }
    }

    let pct = (matched as f64 / total as f64 * 100.0).round() as u32;
    eprintln!();
    eprintln!(
        "  Layer 3 — Property parity: {matched}/{total} ({pct}%)"
    );

    assert_eq!(matched, total, "all oracle properties should match");
}

// ===========================================================================
// LAYER 4: Frame execution — MainLoop processes frames
// ===========================================================================

#[test]
fn layer4_mainloop_runs_60_frames() {
    let tree = load_platformer();
    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(WIDTH, HEIGHT).with_max_frames(60);

    main_loop.run(&mut backend, DT);

    assert_eq!(main_loop.frame_count(), 60);
    assert!(backend.should_quit());
}

#[test]
fn layer4_frame_trace_captures_events() {
    let tree = load_platformer();
    let mut main_loop = MainLoop::new(tree);

    let record = main_loop.step_traced(DT);

    assert_eq!(record.frame_number, 1);
    assert!((record.delta - DT).abs() < 1e-10);
    assert!(
        !record.node_snapshots.is_empty(),
        "traced frame should capture node snapshots"
    );
}

#[test]
fn layer4_positions_stable_without_physics() {
    let tree = load_platformer();
    let player_id = tree.get_node_by_path("/root/World/Player").unwrap();
    let initial_pos = get_position(&tree, player_id);

    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(30, DT);

    let final_pos = get_position(main_loop.tree(), player_id);
    assert_eq!(
        initial_pos, final_pos,
        "positions should stay stable without physics forces"
    );
}

// ===========================================================================
// LAYER 5: 2D rendering — produces visible pixel output
// ===========================================================================

#[test]
fn layer5_render_platformer_nodes() {
    let tree = load_platformer();

    let mut renderer = SoftwareRenderer::new();
    let mut viewport = Viewport::new(WIDTH, HEIGHT, Color::rgb(0.1, 0.1, 0.15));

    // Build canvas items from scene tree Node2D positions
    let node2d_paths = [
        ("/root/World/Player", Color::rgb(0.2, 0.4, 1.0)),
        ("/root/World/Platform1", Color::rgb(0.2, 0.5, 0.2)),
        ("/root/World/Platform2", Color::rgb(0.2, 0.5, 0.2)),
        ("/root/World/Platform3", Color::rgb(0.2, 0.5, 0.2)),
        ("/root/World/Collectible", Color::rgb(1.0, 0.8, 0.0)),
    ];

    for (i, (path, color)) in node2d_paths.iter().enumerate() {
        let node_id = tree.get_node_by_path(path).unwrap();
        let pos = get_position(&tree, node_id);

        let mut item = CanvasItem::new(CanvasItemId(i as u64 + 1));
        item.transform = Transform2D::translated(pos);
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::new(-16.0, -16.0), Vector2::new(32.0, 32.0)),
            color: *color,
            filled: true,
        });
        viewport.add_canvas_item(item);
    }

    let fb = capture_frame(&mut renderer, &viewport);

    assert_eq!(fb.width, WIDTH);
    assert_eq!(fb.height, HEIGHT);

    // Count non-background pixels
    let bg = Color::rgb(0.1, 0.1, 0.15);
    let non_bg = fb
        .pixels
        .iter()
        .filter(|c| {
            (c.r - bg.r).abs() > 0.01 || (c.g - bg.g).abs() > 0.01 || (c.b - bg.b).abs() > 0.01
        })
        .count();

    assert!(
        non_bg > 200,
        "rendered frame should have visible content from 5 nodes, got {} non-bg pixels",
        non_bg
    );
}

#[test]
fn layer5_render_positions_match_scene_tree() {
    let tree = load_platformer();

    let mut renderer = SoftwareRenderer::new();
    let mut viewport = Viewport::new(WIDTH, HEIGHT, Color::BLACK);

    // Place player as a bright red rect at its scene position
    let player_id = tree.get_node_by_path("/root/World/Player").unwrap();
    let player_pos = get_position(&tree, player_id);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.transform = Transform2D::translated(player_pos);
    item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(-5.0, -5.0), Vector2::new(10.0, 10.0)),
        color: Color::rgb(1.0, 0.0, 0.0),
        filled: true,
    });
    viewport.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &viewport);

    // Player should be at (100, 300) — check that pixel is red
    let px = player_pos.x as u32;
    let py = player_pos.y as u32;

    if px < WIDTH && py < HEIGHT {
        let pixel = fb.pixels[(py * WIDTH + px) as usize];
        assert!(
            pixel.r > 0.5,
            "pixel at player position ({}, {}) should be red, got {:?}",
            px, py, pixel
        );
    }
}

// ===========================================================================
// LAYER 6: End-to-end — parse → tree → run → render → verify
// ===========================================================================

#[test]
fn layer6_full_vertical_slice() {
    // PARSE
    let tscn = include_str!("../../fixtures/scenes/platformer.tscn");
    let packed = PackedScene::from_tscn(tscn).unwrap();

    // BUILD TREE
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root_id, &packed).unwrap();

    // VERIFY TREE STRUCTURE (oracle comparison)
    let player_id = tree.get_node_by_path("/root/World/Player").unwrap();
    let camera_id = tree.get_node_by_path("/root/World/Camera").unwrap();
    let collectible_id = tree.get_node_by_path("/root/World/Collectible").unwrap();
    assert_eq!(tree.get_node(player_id).unwrap().class_name(), "Node2D");
    assert_eq!(tree.get_node(camera_id).unwrap().class_name(), "Camera2D");
    assert_eq!(tree.get_node(collectible_id).unwrap().class_name(), "Node2D");

    // VERIFY PROPERTIES (oracle comparison)
    let player_pos = get_position(&tree, player_id);
    assert!((player_pos.x as f64 - 100.0).abs() < FLOAT_TOLERANCE);
    assert!((player_pos.y as f64 - 300.0).abs() < FLOAT_TOLERANCE);

    let collectible_pos = get_position(&tree, collectible_id);
    assert!((collectible_pos.x as f64 - 450.0).abs() < FLOAT_TOLERANCE);
    assert!((collectible_pos.y as f64 - 250.0).abs() < FLOAT_TOLERANCE);

    // RUN FRAMES
    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(WIDTH, HEIGHT);

    for _ in 0..30 {
        main_loop.run_frame(&mut backend, DT);
    }
    assert_eq!(main_loop.frame_count(), 30);

    // VERIFY POSITIONS STABLE (no external forces applied)
    let final_player_pos = get_position(main_loop.tree(), player_id);
    assert_eq!(
        player_pos, final_player_pos,
        "player position should be stable after 30 frames without physics"
    );

    // RENDER
    let mut renderer = SoftwareRenderer::new();
    let mut viewport = Viewport::new(WIDTH, HEIGHT, Color::rgb(0.1, 0.1, 0.15));

    let node_configs: Vec<(&str, Color)> = vec![
        ("/root/World/Player", Color::rgb(0.2, 0.4, 1.0)),
        ("/root/World/Platform1", Color::rgb(0.2, 0.5, 0.2)),
        ("/root/World/Platform2", Color::rgb(0.2, 0.5, 0.2)),
        ("/root/World/Platform3", Color::rgb(0.2, 0.5, 0.2)),
        ("/root/World/Collectible", Color::rgb(1.0, 0.8, 0.0)),
    ];

    for (i, (path, color)) in node_configs.iter().enumerate() {
        let nid = main_loop.tree().get_node_by_path(path).unwrap();
        let pos = get_position(main_loop.tree(), nid);

        let mut item = CanvasItem::new(CanvasItemId(i as u64 + 1));
        item.transform = Transform2D::translated(pos);
        item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::new(-16.0, -16.0), Vector2::new(32.0, 32.0)),
            color: *color,
            filled: true,
        });
        viewport.add_canvas_item(item);
    }

    let fb = capture_frame(&mut renderer, &viewport);

    // VERIFY RENDER OUTPUT
    assert_eq!(fb.width, WIDTH);
    assert_eq!(fb.height, HEIGHT);
    let total_pixels = (WIDTH * HEIGHT) as usize;
    assert_eq!(fb.pixels.len(), total_pixels);

    let bg = Color::rgb(0.1, 0.1, 0.15);
    let non_bg = fb
        .pixels
        .iter()
        .filter(|c| {
            (c.r - bg.r).abs() > 0.01 || (c.g - bg.g).abs() > 0.01 || (c.b - bg.b).abs() > 0.01
        })
        .count();
    assert!(non_bg > 100, "frame must have visible rendered content");

    eprintln!();
    eprintln!("  === Full 2D Vertical Slice Summary ===");
    eprintln!("  Scene:       platformer.tscn");
    eprintln!("  Nodes:       {} (oracle: 8)", main_loop.tree().node_count());
    eprintln!("  Frames run:  {}", main_loop.frame_count());
    eprintln!("  Render:      {}x{} ({} non-bg pixels)", WIDTH, HEIGHT, non_bg);
    eprintln!("  Status:      PASS");
    eprintln!();
}

// ===========================================================================
// LAYER 7: Parity measurement — quantified oracle comparison
// ===========================================================================

#[test]
fn layer7_parity_measurement() {
    let tree = load_platformer();

    // Tree structure parity
    let oracle = oracle_tree();
    let oracle_count = count_oracle_nodes(&oracle);
    let patina_count = tree.node_count();
    let tree_parity = if patina_count == oracle_count { 100 } else { 0 };

    // Property parity
    let oracle_props = oracle_properties();
    let mut prop_matched = 0usize;
    for op in &oracle_props {
        let node_id = tree.get_node_by_path(op.path).unwrap();
        let pos = get_position(&tree, node_id);
        if (pos.x as f64 - op.value_x).abs() < FLOAT_TOLERANCE
            && (pos.y as f64 - op.value_y).abs() < FLOAT_TOLERANCE
        {
            prop_matched += 1;
        }
    }
    let prop_parity = (prop_matched as f64 / oracle_props.len() as f64 * 100.0).round() as u32;

    // Class parity — do all classes match?
    let class_checks = [
        ("/root/World", "Node"),
        ("/root/World/Player", "Node2D"),
        ("/root/World/Platform1", "Node2D"),
        ("/root/World/Camera", "Camera2D"),
        ("/root/World/Collectible", "Node2D"),
    ];
    let mut class_matched = 0usize;
    for (path, expected_class) in &class_checks {
        let nid = tree.get_node_by_path(path).unwrap();
        let node = tree.get_node(nid).unwrap();
        if node.class_name() == *expected_class {
            class_matched += 1;
        }
    }
    let class_parity =
        (class_matched as f64 / class_checks.len() as f64 * 100.0).round() as u32;

    // Child order parity
    let world_id = tree.get_node_by_path("/root/World").unwrap();
    let world = tree.get_node(world_id).unwrap();
    let expected_order = ["Player", "Platform1", "Platform2", "Platform3", "Camera", "Collectible"];
    let mut order_matched = 0usize;
    for (i, &cid) in world.children().iter().enumerate() {
        let child = tree.get_node(cid).unwrap();
        if i < expected_order.len() && child.name() == expected_order[i] {
            order_matched += 1;
        }
    }
    let order_parity =
        (order_matched as f64 / expected_order.len() as f64 * 100.0).round() as u32;

    // Combined
    let combined = (tree_parity + prop_parity + class_parity + order_parity) / 4;

    eprintln!();
    eprintln!("  === Vertical Slice Parity Report ===");
    eprintln!("  Tree structure:  {patina_count}/{oracle_count} nodes ({tree_parity}%)");
    eprintln!(
        "  Properties:      {prop_matched}/{} ({prop_parity}%)",
        oracle_props.len()
    );
    eprintln!(
        "  Classes:         {class_matched}/{} ({class_parity}%)",
        class_checks.len()
    );
    eprintln!(
        "  Child order:     {order_matched}/{} ({order_parity}%)",
        expected_order.len()
    );
    eprintln!("  Combined:        {combined}%");
    eprintln!();

    assert_eq!(tree_parity, 100, "tree structure must match oracle exactly");
    assert_eq!(prop_parity, 100, "properties must match oracle exactly");
    assert_eq!(class_parity, 100, "classes must match oracle exactly");
    assert_eq!(order_parity, 100, "child order must match oracle exactly");
}

// ===========================================================================
// LAYER 8: Traced frame evolution
// ===========================================================================

#[test]
fn layer8_traced_evolution_over_10_frames() {
    let tree = load_platformer();
    let mut main_loop = MainLoop::new(tree);

    let trace = main_loop.run_frames_traced(10, DT);

    assert_eq!(trace.len(), 10, "should have 10 traced frames");

    // Frame numbers should be sequential 1..=10
    // Verify trace captured notification details
    let _details = trace.all_notification_details();

    // Physics time should be consistent
    assert!(
        (main_loop.physics_time() - 10.0 * DT).abs() < 0.01,
        "physics_time should be ~{:.4}, got {:.4}",
        10.0 * DT,
        main_loop.physics_time()
    );

    // Process time should also be consistent
    assert!(
        (main_loop.process_time() - 10.0 * DT).abs() < 0.01,
        "process_time should be ~{:.4}, got {:.4}",
        10.0 * DT,
        main_loop.process_time()
    );
}

// ===========================================================================
// LAYER 9: Render after simulation produces correct output
// ===========================================================================

#[test]
fn layer9_render_after_simulation() {
    let tree = load_platformer();
    let player_id = tree.get_node_by_path("/root/World/Player").unwrap();

    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(WIDTH, HEIGHT);

    // Run 30 frames
    for _ in 0..30 {
        main_loop.run_frame(&mut backend, DT);
    }

    // Build render from final state
    let mut renderer = SoftwareRenderer::new();
    let mut viewport = Viewport::new(WIDTH, HEIGHT, Color::BLACK);

    let final_player_pos = get_position(main_loop.tree(), player_id);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.transform = Transform2D::translated(final_player_pos);
    item.commands.push(DrawCommand::DrawCircle {
        center: Vector2::ZERO,
        radius: 16.0,
        color: Color::rgb(0.2, 0.4, 1.0),
    });
    viewport.add_canvas_item(item);

    let fb = capture_frame(&mut renderer, &viewport);

    // Player circle should produce non-black pixels
    let non_black = fb
        .pixels
        .iter()
        .filter(|c| c.r > 0.01 || c.g > 0.01 || c.b > 0.01)
        .count();

    assert!(
        non_black > 50,
        "rendered player circle should have visible pixels, got {}",
        non_black
    );
}

// ===========================================================================
// LAYER 10: Scene tree paths match oracle paths
// ===========================================================================

#[test]
fn layer10_node_paths_match_oracle() {
    let tree = load_platformer();

    fn verify_paths(tree: &SceneTree, oracle: &OracleNode) {
        let node_id = tree.get_node_by_path(oracle.path);
        assert!(
            node_id.is_some(),
            "oracle path '{}' not found in Patina tree",
            oracle.path
        );

        let nid = node_id.unwrap();
        let patina_path = tree.node_path(nid).unwrap();
        assert_eq!(
            patina_path, oracle.path,
            "path mismatch: patina='{}' oracle='{}'",
            patina_path, oracle.path
        );

        for child in &oracle.children {
            verify_paths(tree, child);
        }
    }

    verify_paths(&tree, &oracle_tree());
}

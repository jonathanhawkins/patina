//! Integration test for the 2D demo pipeline.
//!
//! Verifies the complete engine pipeline: scene loading, physics stepping,
//! rendering, and determinism.

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
use gdphysics2d::shape::Shape2D;
use gdphysics2d::world::PhysicsWorld2D;
use gdrender2d::test_adapter::capture_frame;
use gdrender2d::{FrameBuffer, SoftwareRenderer};
use gdscene::node2d::{get_position, set_position};
use gdscene::packed_scene::add_packed_scene_to_tree;
use gdscene::{MainLoop, PackedScene, SceneTree};
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::viewport::Viewport;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
const FRAME_COUNT: u64 = 60;
const DT: f64 = 1.0 / 60.0;

/// Runs the full demo pipeline and returns the final state for assertions.
struct DemoResult {
    frame_count: u64,
    physics_time: f64,
    final_player_pos: Vector2,
    final_enemy_pos: Vector2,
    final_ground_pos: Vector2,
    framebuffer: FrameBuffer,
}

fn run_demo() -> DemoResult {
    // Load scene.
    let tscn_source = include_str!("../fixtures/scenes/demo_2d.tscn");
    let packed_scene = PackedScene::from_tscn(tscn_source).unwrap();

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let _scene_root_id = add_packed_scene_to_tree(&mut tree, root_id, &packed_scene).unwrap();

    let player_id = tree.get_node_by_path("/root/World/Player").unwrap();
    let enemy_id = tree.get_node_by_path("/root/World/Enemy").unwrap();
    let ground_id = tree.get_node_by_path("/root/World/Ground").unwrap();

    // Set up physics.
    let mut physics = PhysicsWorld2D::new();

    let player_pos = get_position(&tree, player_id);
    let mut player_body = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Rigid,
        player_pos,
        Shape2D::Circle { radius: 16.0 },
        1.0,
    );
    player_body.linear_velocity = Vector2::new(30.0, 0.0);
    let player_body_id = physics.add_body(player_body);

    let enemy_pos = get_position(&tree, enemy_id);
    let mut enemy_body = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Rigid,
        enemy_pos,
        Shape2D::Circle { radius: 16.0 },
        1.0,
    );
    enemy_body.linear_velocity = Vector2::new(-20.0, 0.0);
    let enemy_body_id = physics.add_body(enemy_body);

    let ground_pos = get_position(&tree, ground_id);
    let ground_body = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Static,
        ground_pos,
        Shape2D::Rectangle {
            half_extents: Vector2::new(320.0, 20.0),
        },
        1.0,
    );
    physics.add_body(ground_body);

    // Run main loop.
    let mut main_loop = MainLoop::new(tree);

    for _frame in 0..FRAME_COUNT {
        if let Some(pb) = physics.get_body_mut(player_body_id) {
            pb.apply_force(Vector2::new(0.0, 200.0));
        }
        if let Some(eb) = physics.get_body_mut(enemy_body_id) {
            eb.apply_force(Vector2::new(0.0, 200.0));
        }

        physics.step(DT as f32);

        if let Some(pb) = physics.get_body(player_body_id) {
            set_position(main_loop.tree_mut(), player_id, pb.position);
        }
        if let Some(eb) = physics.get_body(enemy_body_id) {
            set_position(main_loop.tree_mut(), enemy_id, eb.position);
        }

        main_loop.step(DT);
    }

    let final_player_pos = get_position(main_loop.tree(), player_id);
    let final_enemy_pos = get_position(main_loop.tree(), enemy_id);
    let final_ground_pos = get_position(main_loop.tree(), ground_id);

    // Render final frame.
    let mut renderer = SoftwareRenderer::new();
    let mut viewport = Viewport::new(WIDTH, HEIGHT, Color::rgb(0.1, 0.1, 0.15));

    let mut ground_item = CanvasItem::new(CanvasItemId(1));
    ground_item.transform = Transform2D::translated(final_ground_pos);
    ground_item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(-320.0, -20.0), Vector2::new(640.0, 40.0)),
        color: Color::rgb(0.2, 0.5, 0.2),
        filled: true,
    });
    viewport.add_canvas_item(ground_item);

    let mut player_item = CanvasItem::new(CanvasItemId(2));
    player_item.transform = Transform2D::translated(final_player_pos);
    player_item.z_index = 1;
    player_item.commands.push(DrawCommand::DrawCircle {
        center: Vector2::ZERO,
        radius: 16.0,
        color: Color::rgb(0.2, 0.4, 1.0),
    });
    viewport.add_canvas_item(player_item);

    let mut enemy_item = CanvasItem::new(CanvasItemId(3));
    enemy_item.transform = Transform2D::translated(final_enemy_pos);
    enemy_item.z_index = 1;
    enemy_item.commands.push(DrawCommand::DrawCircle {
        center: Vector2::ZERO,
        radius: 16.0,
        color: Color::rgb(1.0, 0.2, 0.2),
    });
    viewport.add_canvas_item(enemy_item);

    let fb = capture_frame(&mut renderer, &viewport);

    let frame_count = main_loop.frame_count();
    let physics_time = main_loop.physics_time();

    DemoResult {
        frame_count,
        physics_time,
        final_player_pos,
        final_enemy_pos,
        final_ground_pos,
        framebuffer: fb,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn scene_loads_with_correct_node_count() {
    let tscn_source = include_str!("../fixtures/scenes/demo_2d.tscn");
    let packed_scene = PackedScene::from_tscn(tscn_source).unwrap();

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root_id, &packed_scene).unwrap();

    // root + World + Player + Enemy + Ground = 5 nodes
    assert_eq!(tree.node_count(), 5);

    // Verify node paths exist.
    assert!(tree.get_node_by_path("/root/World/Player").is_some());
    assert!(tree.get_node_by_path("/root/World/Enemy").is_some());
    assert!(tree.get_node_by_path("/root/World/Ground").is_some());
}

#[test]
fn scene_initial_positions_are_correct() {
    let tscn_source = include_str!("../fixtures/scenes/demo_2d.tscn");
    let packed_scene = PackedScene::from_tscn(tscn_source).unwrap();

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root_id, &packed_scene).unwrap();

    let player_id = tree.get_node_by_path("/root/World/Player").unwrap();
    let enemy_id = tree.get_node_by_path("/root/World/Enemy").unwrap();
    let ground_id = tree.get_node_by_path("/root/World/Ground").unwrap();

    assert_eq!(get_position(&tree, player_id), Vector2::new(100.0, 200.0));
    assert_eq!(get_position(&tree, enemy_id), Vector2::new(400.0, 200.0));
    assert_eq!(get_position(&tree, ground_id), Vector2::new(0.0, 500.0));
}

#[test]
fn physics_steps_and_positions_change() {
    let result = run_demo();

    // Player had rightward velocity + gravity, so position must have changed
    // from the initial (100, 200).
    assert!(
        result.final_player_pos.x != 100.0 || result.final_player_pos.y != 200.0,
        "Player position should have changed from initial: {:?}",
        result.final_player_pos
    );

    // Enemy had leftward velocity + gravity, so position must have changed
    // from the initial (400, 200).
    assert!(
        result.final_enemy_pos.x != 400.0 || result.final_enemy_pos.y != 200.0,
        "Enemy position should have changed from initial: {:?}",
        result.final_enemy_pos
    );

    // Ground is static and was not given a physics body that moves.
    assert_eq!(
        result.final_ground_pos,
        Vector2::new(0.0, 500.0),
        "Ground should not have moved"
    );
}

#[test]
fn simulation_ran_correct_number_of_frames() {
    let result = run_demo();
    assert_eq!(result.frame_count, FRAME_COUNT);
    // Physics time should be approximately 1 second (60 frames at 1/60).
    assert!(
        (result.physics_time - 1.0).abs() < 0.01,
        "Physics time should be ~1.0s, got {}",
        result.physics_time
    );
}

#[test]
fn rendered_frame_has_correct_dimensions() {
    let result = run_demo();
    assert_eq!(result.framebuffer.width, WIDTH);
    assert_eq!(result.framebuffer.height, HEIGHT);
    assert_eq!(result.framebuffer.pixels.len(), (WIDTH * HEIGHT) as usize);
}

#[test]
fn rendered_frame_has_nonzero_pixels() {
    let result = run_demo();

    // Count pixels that are not the background color.
    let bg = Color::rgb(0.1, 0.1, 0.15);
    let non_bg_count = result
        .framebuffer
        .pixels
        .iter()
        .filter(|c| {
            (c.r - bg.r).abs() > 0.01 || (c.g - bg.g).abs() > 0.01 || (c.b - bg.b).abs() > 0.01
        })
        .count();

    assert!(
        non_bg_count > 0,
        "Rendered frame should have non-background pixels"
    );
}

#[test]
fn rendered_frame_pixel_data_is_nonempty() {
    let result = run_demo();

    // At least some pixels should have color data (not all zero).
    let any_nonzero = result
        .framebuffer
        .pixels
        .iter()
        .any(|c| c.r > 0.0 || c.g > 0.0 || c.b > 0.0);

    assert!(
        any_nonzero,
        "Frame buffer should contain non-zero pixel data"
    );
}

#[test]
fn determinism_two_runs_produce_identical_frames() {
    let result1 = run_demo();
    let result2 = run_demo();

    assert_eq!(
        result1.framebuffer.pixels.len(),
        result2.framebuffer.pixels.len(),
        "Frame buffer sizes must match"
    );

    assert_eq!(
        result1.framebuffer.pixels, result2.framebuffer.pixels,
        "Two identical runs must produce identical pixel data"
    );

    // Also verify positions match.
    assert_eq!(result1.final_player_pos, result2.final_player_pos);
    assert_eq!(result1.final_enemy_pos, result2.final_enemy_pos);
}

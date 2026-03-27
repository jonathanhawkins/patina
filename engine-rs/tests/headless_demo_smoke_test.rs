//! Headless demo_2d smoke test for CI.
//!
//! Proves the V1 exit gate: "`demo_2d` runs to completion in headless mode on
//! CI without panicking." Exercises the full `MainLoop::run()` +
//! `HeadlessPlatform` path rather than manual `step()` calls.

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
use gdphysics2d::shape::Shape2D;
use gdphysics2d::world::PhysicsWorld2D;
use gdplatform::backend::{HeadlessPlatform, PlatformBackend};
use gdrender2d::test_adapter::capture_frame;
use gdrender2d::SoftwareRenderer;
use gdscene::main_loop::MainLoop;
use gdscene::node2d::{get_position, set_position};
use gdscene::packed_scene::add_packed_scene_to_tree;
use gdscene::{PackedScene, SceneTree};
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::viewport::Viewport;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
const FRAME_COUNT: u64 = 60;
const DT: f64 = 1.0 / 60.0;

/// Loads the demo_2d scene and returns the tree + node IDs.
fn load_demo_scene() -> (SceneTree, gdscene::node::NodeId, gdscene::node::NodeId, gdscene::node::NodeId) {
    let tscn_source = include_str!("../fixtures/scenes/demo_2d.tscn");
    let packed_scene = PackedScene::from_tscn(tscn_source).unwrap();

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root_id, &packed_scene).unwrap();

    let player_id = tree.get_node_by_path("/root/World/Player").unwrap();
    let enemy_id = tree.get_node_by_path("/root/World/Enemy").unwrap();
    let ground_id = tree.get_node_by_path("/root/World/Ground").unwrap();

    (tree, player_id, enemy_id, ground_id)
}

// ---------------------------------------------------------------------------
// Core smoke: MainLoop::run() with HeadlessPlatform
// ---------------------------------------------------------------------------

#[test]
fn headless_demo_2d_runs_without_panic() {
    // This is the V1 exit gate test: demo_2d runs to completion in headless
    // mode without panicking.
    let (tree, player_id, enemy_id, _ground_id) = load_demo_scene();

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
    let _player_body_id = physics.add_body(player_body);

    let enemy_pos = get_position(&tree, enemy_id);
    let mut enemy_body = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Rigid,
        enemy_pos,
        Shape2D::Circle { radius: 16.0 },
        1.0,
    );
    enemy_body.linear_velocity = Vector2::new(-20.0, 0.0);
    let _enemy_body_id = physics.add_body(enemy_body);

    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(WIDTH, HEIGHT).with_max_frames(FRAME_COUNT);

    // Run through HeadlessPlatform — the real headless API path.
    main_loop.run(&mut backend, DT);

    assert_eq!(main_loop.frame_count(), FRAME_COUNT);
    assert_eq!(backend.frames_run(), FRAME_COUNT);
    assert!(backend.should_quit());
}

#[test]
fn headless_demo_2d_physics_sync_via_run_frame() {
    // Exercises run_frame() in a loop with physics sync, matching the
    // demo_2d example's pattern but using the HeadlessPlatform path.
    let (tree, player_id, enemy_id, ground_id) = load_demo_scene();

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

    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(WIDTH, HEIGHT);

    for _ in 0..FRAME_COUNT {
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

        main_loop.run_frame(&mut backend, DT);
    }

    // Verify frame counts match on both sides.
    assert_eq!(main_loop.frame_count(), FRAME_COUNT);
    assert_eq!(backend.frames_run(), FRAME_COUNT);

    // Physics time should be ~1.0s.
    assert!(
        (main_loop.physics_time() - 1.0).abs() < 0.01,
        "physics_time should be ~1.0s, got {}",
        main_loop.physics_time()
    );

    // Positions must have changed from initial values.
    let final_player = get_position(main_loop.tree(), player_id);
    let final_enemy = get_position(main_loop.tree(), enemy_id);
    assert!(final_player.x > 100.0, "player should have moved right");
    assert!(final_enemy.x < 400.0, "enemy should have moved left");
}

#[test]
fn headless_demo_2d_render_after_headless_run() {
    // Runs the full headless pipeline and then renders a frame.
    // Proves that rendering works after a headless simulation.
    let (tree, player_id, enemy_id, ground_id) = load_demo_scene();

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

    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(WIDTH, HEIGHT);

    // Run simulation via headless backend.
    for _ in 0..FRAME_COUNT {
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
        main_loop.run_frame(&mut backend, DT);
    }

    // Render final frame.
    let final_player = get_position(main_loop.tree(), player_id);
    let final_enemy = get_position(main_loop.tree(), enemy_id);
    let final_ground = get_position(main_loop.tree(), ground_id);

    let mut renderer = SoftwareRenderer::new();
    let mut viewport = Viewport::new(WIDTH, HEIGHT, Color::rgb(0.1, 0.1, 0.15));

    let mut ground_item = CanvasItem::new(CanvasItemId(1));
    ground_item.transform = Transform2D::translated(final_ground);
    ground_item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(-320.0, -20.0), Vector2::new(640.0, 40.0)),
        color: Color::rgb(0.2, 0.5, 0.2),
        filled: true,
    });
    viewport.add_canvas_item(ground_item);

    let mut player_item = CanvasItem::new(CanvasItemId(2));
    player_item.transform = Transform2D::translated(final_player);
    player_item.z_index = 1;
    player_item.commands.push(DrawCommand::DrawCircle {
        center: Vector2::ZERO,
        radius: 16.0,
        color: Color::rgb(0.2, 0.4, 1.0),
    });
    viewport.add_canvas_item(player_item);

    let mut enemy_item = CanvasItem::new(CanvasItemId(3));
    enemy_item.transform = Transform2D::translated(final_enemy);
    enemy_item.z_index = 1;
    enemy_item.commands.push(DrawCommand::DrawCircle {
        center: Vector2::ZERO,
        radius: 16.0,
        color: Color::rgb(1.0, 0.2, 0.2),
    });
    viewport.add_canvas_item(enemy_item);

    let fb = capture_frame(&mut renderer, &viewport);

    // Verify rendered output.
    assert_eq!(fb.width, WIDTH);
    assert_eq!(fb.height, HEIGHT);

    let bg = Color::rgb(0.1, 0.1, 0.15);
    let non_bg = fb
        .pixels
        .iter()
        .filter(|c| {
            (c.r - bg.r).abs() > 0.01 || (c.g - bg.g).abs() > 0.01 || (c.b - bg.b).abs() > 0.01
        })
        .count();
    assert!(non_bg > 100, "rendered frame should have visible content, got {} non-bg pixels", non_bg);
}

#[test]
fn headless_demo_2d_deterministic_across_backend_paths() {
    // Verify that manual step() and HeadlessPlatform run_frame() produce
    // identical results — proving the headless path is semantically equivalent.
    let run_via_step = || {
        let (tree, player_id, enemy_id, ground_id) = load_demo_scene();
        let mut physics = PhysicsWorld2D::new();

        let player_pos = get_position(&tree, player_id);
        let mut pb = PhysicsBody2D::new(
            BodyId(0), BodyType::Rigid, player_pos,
            Shape2D::Circle { radius: 16.0 }, 1.0,
        );
        pb.linear_velocity = Vector2::new(30.0, 0.0);
        let pbid = physics.add_body(pb);

        let enemy_pos = get_position(&tree, enemy_id);
        let mut eb = PhysicsBody2D::new(
            BodyId(0), BodyType::Rigid, enemy_pos,
            Shape2D::Circle { radius: 16.0 }, 1.0,
        );
        eb.linear_velocity = Vector2::new(-20.0, 0.0);
        let ebid = physics.add_body(eb);

        let mut ml = MainLoop::new(tree);
        for _ in 0..FRAME_COUNT {
            if let Some(b) = physics.get_body_mut(pbid) { b.apply_force(Vector2::new(0.0, 200.0)); }
            if let Some(b) = physics.get_body_mut(ebid) { b.apply_force(Vector2::new(0.0, 200.0)); }
            physics.step(DT as f32);
            if let Some(b) = physics.get_body(pbid) { set_position(ml.tree_mut(), player_id, b.position); }
            if let Some(b) = physics.get_body(ebid) { set_position(ml.tree_mut(), enemy_id, b.position); }
            ml.step(DT);
        }
        (
            get_position(ml.tree(), player_id),
            get_position(ml.tree(), enemy_id),
            get_position(ml.tree(), ground_id),
            ml.frame_count(),
        )
    };

    let run_via_backend = || {
        let (tree, player_id, enemy_id, ground_id) = load_demo_scene();
        let mut physics = PhysicsWorld2D::new();

        let player_pos = get_position(&tree, player_id);
        let mut pb = PhysicsBody2D::new(
            BodyId(0), BodyType::Rigid, player_pos,
            Shape2D::Circle { radius: 16.0 }, 1.0,
        );
        pb.linear_velocity = Vector2::new(30.0, 0.0);
        let pbid = physics.add_body(pb);

        let enemy_pos = get_position(&tree, enemy_id);
        let mut eb = PhysicsBody2D::new(
            BodyId(0), BodyType::Rigid, enemy_pos,
            Shape2D::Circle { radius: 16.0 }, 1.0,
        );
        eb.linear_velocity = Vector2::new(-20.0, 0.0);
        let ebid = physics.add_body(eb);

        let mut ml = MainLoop::new(tree);
        let mut backend = HeadlessPlatform::new(WIDTH, HEIGHT);
        for _ in 0..FRAME_COUNT {
            if let Some(b) = physics.get_body_mut(pbid) { b.apply_force(Vector2::new(0.0, 200.0)); }
            if let Some(b) = physics.get_body_mut(ebid) { b.apply_force(Vector2::new(0.0, 200.0)); }
            physics.step(DT as f32);
            if let Some(b) = physics.get_body(pbid) { set_position(ml.tree_mut(), player_id, b.position); }
            if let Some(b) = physics.get_body(ebid) { set_position(ml.tree_mut(), enemy_id, b.position); }
            ml.run_frame(&mut backend, DT);
        }
        (
            get_position(ml.tree(), player_id),
            get_position(ml.tree(), enemy_id),
            get_position(ml.tree(), ground_id),
            ml.frame_count(),
        )
    };

    let (sp, se, sg, sf) = run_via_step();
    let (bp, be, bg, bf) = run_via_backend();

    assert_eq!(sp, bp, "player positions must match between step() and run_frame()");
    assert_eq!(se, be, "enemy positions must match");
    assert_eq!(sg, bg, "ground positions must match");
    assert_eq!(sf, bf, "frame counts must match");
}

#[test]
fn headless_demo_2d_early_quit_via_close_event() {
    // Verify that a CloseRequested event stops the headless run cleanly.
    let (tree, _player_id, _enemy_id, _ground_id) = load_demo_scene();

    let mut main_loop = MainLoop::new(tree);
    let mut backend = HeadlessPlatform::new(WIDTH, HEIGHT);

    // Run 10 frames, then inject a close request, then run more.
    for _ in 0..10 {
        main_loop.run_frame(&mut backend, DT);
    }
    assert_eq!(main_loop.frame_count(), 10);

    // Inject close event.
    backend.push_event(gdplatform::window::WindowEvent::CloseRequested);
    main_loop.run_frame(&mut backend, DT);

    // Backend should now signal quit.
    assert!(backend.should_quit());
    assert_eq!(main_loop.frame_count(), 11);

    // MainLoop::run() should exit immediately.
    main_loop.run(&mut backend, DT);
    assert_eq!(main_loop.frame_count(), 11, "run() should not advance past quit");
}

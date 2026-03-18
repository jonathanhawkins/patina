//! End-to-end 2D demo for the Patina Engine.
//!
//! Demonstrates the full pipeline: scene loading, physics simulation,
//! rendering, and frame capture. Produces a PPM image at `output/demo_frame.ppm`.

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
use gdphysics2d::shape::Shape2D;
use gdphysics2d::world::PhysicsWorld2D;
use gdrender2d::test_adapter::{capture_frame, save_ppm};
use gdrender2d::SoftwareRenderer;
use gdscene::node2d::{get_position, set_position};
use gdscene::packed_scene::add_packed_scene_to_tree;
use gdscene::{MainLoop, PackedScene, SceneTree};
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::viewport::Viewport;

/// Viewport dimensions for the demo.
const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
/// Number of frames to simulate.
const FRAME_COUNT: u64 = 60;
/// Fixed timestep (60 Hz).
const DT: f64 = 1.0 / 60.0;

fn main() {
    // -----------------------------------------------------------------------
    // 1. Load scene from .tscn fixture
    // -----------------------------------------------------------------------
    let tscn_source = include_str!("../fixtures/scenes/demo_2d.tscn");
    let packed_scene = PackedScene::from_tscn(tscn_source).expect("failed to parse demo_2d.tscn");

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let scene_root_id = add_packed_scene_to_tree(&mut tree, root_id, &packed_scene)
        .expect("failed to instance scene");

    // Look up node IDs by path.
    let player_id = tree
        .get_node_by_path("/root/World/Player")
        .expect("Player node not found");
    let enemy_id = tree
        .get_node_by_path("/root/World/Enemy")
        .expect("Enemy node not found");
    let ground_id = tree
        .get_node_by_path("/root/World/Ground")
        .expect("Ground node not found");

    println!("Scene loaded: {} nodes", tree.node_count());
    println!("  Player position: {:?}", get_position(&tree, player_id));
    println!("  Enemy position:  {:?}", get_position(&tree, enemy_id));
    println!("  Ground position: {:?}", get_position(&tree, ground_id));

    // -----------------------------------------------------------------------
    // 2. Set up physics world
    // -----------------------------------------------------------------------
    let mut physics = PhysicsWorld2D::new();

    // Player: rigid circle, slight rightward velocity + gravity.
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

    // Enemy: rigid circle, moving leftward.
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

    // Ground: static rectangle spanning the bottom of the viewport.
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
    let _ground_body_id = physics.add_body(ground_body);

    println!("Physics world: {} bodies", physics.body_count());

    // -----------------------------------------------------------------------
    // 3. Run main loop for FRAME_COUNT frames
    // -----------------------------------------------------------------------
    let mut main_loop = MainLoop::new(tree);

    for frame in 0..FRAME_COUNT {
        // Apply gravity to rigid bodies.
        if let Some(pb) = physics.get_body_mut(player_body_id) {
            pb.apply_force(Vector2::new(0.0, 200.0));
        }
        if let Some(eb) = physics.get_body_mut(enemy_body_id) {
            eb.apply_force(Vector2::new(0.0, 200.0));
        }

        // Step physics.
        physics.step(DT as f32);

        // Sync physics positions back to scene tree.
        if let Some(pb) = physics.get_body(player_body_id) {
            set_position(main_loop.tree_mut(), player_id, pb.position);
        }
        if let Some(eb) = physics.get_body(enemy_body_id) {
            set_position(main_loop.tree_mut(), enemy_id, eb.position);
        }

        // Step scene tree (process + physics notifications).
        main_loop.step(DT);

        if frame % 20 == 0 {
            let pp = get_position(main_loop.tree(), player_id);
            let ep = get_position(main_loop.tree(), enemy_id);
            println!(
                "  Frame {frame:3}: Player({:.1}, {:.1})  Enemy({:.1}, {:.1})",
                pp.x, pp.y, ep.x, ep.y
            );
        }
    }

    println!(
        "Simulation complete: {} frames, {:.3}s physics time",
        main_loop.frame_count(),
        main_loop.physics_time()
    );

    // -----------------------------------------------------------------------
    // 4. Render the final frame
    // -----------------------------------------------------------------------
    let final_player_pos = get_position(main_loop.tree(), player_id);
    let final_enemy_pos = get_position(main_loop.tree(), enemy_id);
    let final_ground_pos = get_position(main_loop.tree(), ground_id);

    let mut renderer = SoftwareRenderer::new();
    let mut viewport = Viewport::new(WIDTH, HEIGHT, Color::rgb(0.1, 0.1, 0.15));

    // Ground: dark green rectangle.
    let mut ground_item = CanvasItem::new(CanvasItemId(1));
    ground_item.transform = Transform2D::translated(final_ground_pos);
    ground_item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(-320.0, -20.0), Vector2::new(640.0, 40.0)),
        color: Color::rgb(0.2, 0.5, 0.2),
        filled: true,
    });
    viewport.add_canvas_item(ground_item);

    // Player: blue circle.
    let mut player_item = CanvasItem::new(CanvasItemId(2));
    player_item.transform = Transform2D::translated(final_player_pos);
    player_item.z_index = 1;
    player_item.commands.push(DrawCommand::DrawCircle {
        center: Vector2::ZERO,
        radius: 16.0,
        color: Color::rgb(0.2, 0.4, 1.0),
    });
    viewport.add_canvas_item(player_item);

    // Enemy: red circle.
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

    // -----------------------------------------------------------------------
    // 5. Save PPM output
    // -----------------------------------------------------------------------
    std::fs::create_dir_all("output").expect("failed to create output directory");
    save_ppm(&fb, "output/demo_frame.ppm").expect("failed to save PPM");

    // -----------------------------------------------------------------------
    // 6. Print summary
    // -----------------------------------------------------------------------
    println!("\n=== Demo Summary ===");
    println!("Scene root: {:?}", scene_root_id);
    println!("Total nodes: {}", main_loop.tree().node_count());
    println!(
        "Final Player position: ({:.1}, {:.1})",
        final_player_pos.x, final_player_pos.y
    );
    println!(
        "Final Enemy position:  ({:.1}, {:.1})",
        final_enemy_pos.x, final_enemy_pos.y
    );
    println!(
        "Final Ground position: ({:.1}, {:.1})",
        final_ground_pos.x, final_ground_pos.y
    );
    println!("Physics bodies: {}", physics.body_count());
    println!("Rendered frame: {}x{}", fb.width, fb.height);
    println!("Non-zero pixels: {}", count_nonzero_pixels(&fb));
    println!("Output saved to: output/demo_frame.ppm");
}

/// Counts pixels that differ from pure black.
fn count_nonzero_pixels(fb: &gdrender2d::FrameBuffer) -> usize {
    fb.pixels
        .iter()
        .filter(|c| c.r > 0.0 || c.g > 0.0 || c.b > 0.0)
        .count()
}

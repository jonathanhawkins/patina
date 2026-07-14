//! Integration tests for the platformer demo.
//!
//! Verifies that the capstone demo runs end-to-end and produces correct,
//! deterministic results across all subsystems.

use gdaudio::{AudioMixer, AudioStreamPlayback};
use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
use gdphysics2d::shape::Shape2D;
use gdphysics2d::world::PhysicsWorld2D;
use gdplatform::input::{ActionBinding, InputEvent, InputMap, InputState, Key};
use gdrender2d::test_adapter::capture_frame;
use gdrender2d::SoftwareRenderer;
use gdscene::node::Node;
use gdscene::node2d::{get_position, set_position};
use gdscene::particle::{ParticleEmitter, ParticleMaterial, ParticleSimulator};
use gdscene::scene_tree::SceneTree;
use gdscene::tween::{EaseType, TransFunc, TweenBuilder};
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Helper: run the demo logic and return the result
// ---------------------------------------------------------------------------

/// Result of running the platformer demo, used for testing.
#[derive(Debug, Clone)]
struct PlatformerResult {
    player_final_pos: (f32, f32),
    score: u32,
    frames_rendered: u64,
    fb_width: u32,
    fb_height: u32,
    player_y_history: Vec<f32>,
    particles_emitted: u64,
    tween_completed: bool,
    audio_bus_count: usize,
    pixel_data: Vec<Color>,
}

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
const FRAME_COUNT: u64 = 120;
const DT: f32 = 1.0 / 60.0;
const GRAVITY: f32 = 980.0;
const PLAYER_SPEED: f32 = 200.0;
const JUMP_IMPULSE: f32 = -400.0;
const COLLECT_RADIUS: f32 = 30.0;

fn build_input_sequence() -> Vec<(u64, InputEvent)> {
    let mut events = Vec::new();
    events.push((
        0,
        InputEvent::Key {
            key: Key::Right,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        },
    ));
    events.push((
        30,
        InputEvent::Key {
            key: Key::Right,
            pressed: false,
            shift: false,
            ctrl: false,
            alt: false,
        },
    ));
    events.push((
        35,
        InputEvent::Key {
            key: Key::Space,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        },
    ));
    events.push((
        36,
        InputEvent::Key {
            key: Key::Space,
            pressed: false,
            shift: false,
            ctrl: false,
            alt: false,
        },
    ));
    events.push((
        40,
        InputEvent::Key {
            key: Key::Right,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        },
    ));
    events.push((
        80,
        InputEvent::Key {
            key: Key::Right,
            pressed: false,
            shift: false,
            ctrl: false,
            alt: false,
        },
    ));
    events.push((
        90,
        InputEvent::Key {
            key: Key::Space,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        },
    ));
    events.push((
        91,
        InputEvent::Key {
            key: Key::Space,
            pressed: false,
            shift: false,
            ctrl: false,
            alt: false,
        },
    ));
    events
}

fn run_platformer() -> PlatformerResult {
    // Scene setup
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let world_node = Node::new("World", "Node");
    let world_id = tree.add_child(root_id, world_node).unwrap();

    let player_node = Node::new("Player", "Node2D");
    let player_id = tree.add_child(world_id, player_node).unwrap();
    set_position(&mut tree, player_id, Vector2::new(100.0, 400.0));

    let platform_positions = [
        Vector2::new(200.0, 450.0),
        Vector2::new(350.0, 380.0),
        Vector2::new(500.0, 320.0),
    ];
    for (i, pos) in platform_positions.iter().enumerate() {
        let pnode = Node::new(format!("Platform{}", i), "Node2D");
        let pid = tree.add_child(world_id, pnode).unwrap();
        set_position(&mut tree, pid, *pos);
    }

    let collectible_node = Node::new("Collectible", "Node2D");
    let collectible_id = tree.add_child(world_id, collectible_node).unwrap();
    set_position(&mut tree, collectible_id, Vector2::new(300.0, 350.0));

    let score_node = Node::new("ScoreLabel", "Node");
    let score_id = tree.add_child(world_id, score_node).unwrap();
    tree.get_node_mut(score_id)
        .unwrap()
        .set_property("text", Variant::String("Score: 0".to_string()));

    let ground_node = Node::new("Ground", "Node2D");
    let ground_id = tree.add_child(world_id, ground_node).unwrap();
    set_position(&mut tree, ground_id, Vector2::new(320.0, 550.0));

    // Physics setup
    let mut physics = PhysicsWorld2D::new();
    let player_body = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Rigid,
        Vector2::new(100.0, 400.0),
        Shape2D::Circle { radius: 16.0 },
        1.0,
    );
    let player_body_id = physics.add_body(player_body);

    let platform_half = Vector2::new(60.0, 10.0);
    for pos in &platform_positions {
        let pbody = PhysicsBody2D::new(
            BodyId(0),
            BodyType::Static,
            *pos,
            Shape2D::Rectangle {
                half_extents: platform_half,
            },
            1.0,
        );
        physics.add_body(pbody);
    }

    let collectible_pos = Vector2::new(300.0, 350.0);

    let ground_body = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Static,
        Vector2::new(320.0, 550.0),
        Shape2D::Rectangle {
            half_extents: Vector2::new(320.0, 20.0),
        },
        1.0,
    );
    physics.add_body(ground_body);

    // Input setup
    let mut input_map = InputMap::new();
    input_map.add_action("move_right", 0.0);
    input_map.add_action("move_left", 0.0);
    input_map.add_action("jump", 0.0);
    input_map.action_add_event("move_right", ActionBinding::KeyBinding(Key::Right));
    input_map.action_add_event("move_left", ActionBinding::KeyBinding(Key::Left));
    input_map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));

    let mut input_state = InputState::new();
    input_state.set_input_map(input_map);
    let input_sequence = build_input_sequence();

    // Audio setup
    let mut mixer = AudioMixer::new();
    mixer.add_bus("SFX");
    let mut jump_sfx = AudioStreamPlayback::new(0.5);
    jump_sfx.set_bus("SFX");

    // Particle setup
    let jump_emitter = ParticleEmitter {
        material: ParticleMaterial {
            direction: Vector2::new(0.0, -1.0),
            spread: 60.0,
            initial_velocity_min: 50.0,
            initial_velocity_max: 120.0,
            gravity: Vector2::new(0.0, 200.0),
            start_color: Color::rgb(1.0, 1.0, 0.5),
            end_color: Color::rgb(1.0, 0.3, 0.0),
            ..ParticleMaterial::default()
        },
        amount: 6,
        lifetime: 0.5,
        one_shot: true,
        explosiveness: 1.0,
        emitting: false,
        ..ParticleEmitter::default()
    };
    let mut particle_sim = ParticleSimulator::new(jump_emitter);

    // Tween setup
    let mut collect_tween = TweenBuilder::new()
        .tween_property("scale", Variant::Float(1.0), Variant::Float(0.0), 0.3)
        .set_ease(EaseType::Out)
        .set_trans(TransFunc::Quad)
        .build();
    collect_tween.stop();

    // Main loop
    let mut main_loop = gdscene::MainLoop::new(tree);
    let mut score: u32 = 0;
    let mut collected = false;
    let mut player_y_history = Vec::new();
    let mut tween_completed = false;
    let mut collectible_scale = 1.0_f32;

    for frame in 0..FRAME_COUNT {
        for (evt_frame, evt) in &input_sequence {
            if *evt_frame == frame {
                input_state.process_event(evt.clone());
            }
        }

        if let Some(player) = physics.get_body_mut(player_body_id) {
            let h_input = if input_state.is_action_pressed("move_right") {
                1.0
            } else if input_state.is_action_pressed("move_left") {
                -1.0
            } else {
                0.0
            };
            player.linear_velocity.x = h_input * PLAYER_SPEED;

            if input_state.is_action_just_pressed("jump") && player.position.y > 350.0 {
                player.apply_impulse(Vector2::new(0.0, JUMP_IMPULSE));
                particle_sim.emitter.emitting = true;
                particle_sim.total_emitted = 0;
                particle_sim.time_accumulator = 0.0;
                jump_sfx.stop();
                jump_sfx.play();
            }

            player.apply_force(Vector2::new(0.0, GRAVITY));
        }

        physics.step(DT);

        if !collected {
            if let Some(player) = physics.get_body(player_body_id) {
                let dist = (player.position - collectible_pos).length();
                if dist < COLLECT_RADIUS {
                    collected = true;
                    score += 1;
                    collect_tween.start();
                    if let Some(label) = main_loop.tree_mut().get_node_mut(score_id) {
                        label.set_property("text", Variant::String(format!("Score: {}", score)));
                    }
                }
            }
        }

        if let Some(pb) = physics.get_body(player_body_id) {
            set_position(main_loop.tree_mut(), player_id, pb.position);
        }

        if collect_tween.running {
            let done = collect_tween.advance(DT as f64);
            if done {
                tween_completed = true;
            }
            let values = collect_tween.get_current_values();
            if let Some((_, Variant::Float(s))) = values.first() {
                collectible_scale = *s as f32;
            }
        }

        particle_sim.step(DT);
        if jump_sfx.is_playing() {
            jump_sfx.advance(DT);
        }
        main_loop.step(DT as f64);

        let player_pos = get_position(main_loop.tree(), player_id);
        player_y_history.push(player_pos.y);
        input_state.flush_frame();
    }

    // Render final frame
    let final_player_pos = get_position(main_loop.tree(), player_id);
    let final_ground_pos = get_position(main_loop.tree(), ground_id);

    let mut renderer = SoftwareRenderer::new();
    let mut viewport = Viewport::new(WIDTH, HEIGHT, Color::rgb(0.4, 0.6, 0.9));

    let mut ground_item = CanvasItem::new(CanvasItemId(1));
    ground_item.transform = Transform2D::translated(final_ground_pos);
    ground_item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(-320.0, -20.0), Vector2::new(640.0, 40.0)),
        color: Color::rgb(0.55, 0.35, 0.15),
        filled: true,
    });
    viewport.add_canvas_item(ground_item);

    for (i, pos) in platform_positions.iter().enumerate() {
        let mut plat_item = CanvasItem::new(CanvasItemId(10 + i as u64));
        plat_item.transform = Transform2D::translated(*pos);
        plat_item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::new(-60.0, -10.0), Vector2::new(120.0, 20.0)),
            color: Color::rgb(0.5, 0.5, 0.5),
            filled: true,
        });
        viewport.add_canvas_item(plat_item);
    }

    if collectible_scale > 0.01 {
        let mut collect_item = CanvasItem::new(CanvasItemId(20));
        collect_item.transform = Transform2D::translated(collectible_pos);
        collect_item.z_index = 1;
        collect_item.commands.push(DrawCommand::DrawCircle {
            center: Vector2::ZERO,
            radius: 12.0 * collectible_scale,
            color: Color::rgb(1.0, 0.85, 0.0),
        });
        viewport.add_canvas_item(collect_item);
    }

    let mut player_item = CanvasItem::new(CanvasItemId(30));
    player_item.transform = Transform2D::translated(final_player_pos);
    player_item.z_index = 2;
    player_item.commands.push(DrawCommand::DrawCircle {
        center: Vector2::ZERO,
        radius: 16.0,
        color: Color::rgb(0.2, 0.4, 1.0),
    });
    viewport.add_canvas_item(player_item);

    for (i, (pos, color, scale)) in particle_sim.get_draw_commands().iter().enumerate() {
        let mut p_item = CanvasItem::new(CanvasItemId(100 + i as u64));
        p_item.transform = Transform2D::translated(final_player_pos + *pos);
        p_item.z_index = 3;
        p_item.commands.push(DrawCommand::DrawCircle {
            center: Vector2::ZERO,
            radius: 3.0 * scale,
            color: *color,
        });
        viewport.add_canvas_item(p_item);
    }

    let fb = capture_frame(&mut renderer, &viewport);

    PlatformerResult {
        player_final_pos: (final_player_pos.x, final_player_pos.y),
        score,
        frames_rendered: main_loop.frame_count(),
        fb_width: fb.width,
        fb_height: fb.height,
        player_y_history,
        particles_emitted: particle_sim.total_emitted,
        tween_completed,
        audio_bus_count: mixer.bus_count(),
        pixel_data: fb.pixels.clone(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn player_moved_right() {
    let result = run_platformer();
    assert!(
        result.player_final_pos.0 > 100.0,
        "Player should have moved right from starting x=100, got x={}",
        result.player_final_pos.0
    );
}

#[test]
fn player_jumped_and_returned() {
    let result = run_platformer();
    let initial_y = result.player_y_history[0];
    // After jump at frame 35, player should go up (y decreases)
    let min_y = result
        .player_y_history
        .iter()
        .skip(35)
        .take(30)
        .cloned()
        .fold(f32::MAX, f32::min);
    assert!(
        min_y < initial_y - 10.0,
        "Player should have jumped up: min_y={min_y}, initial_y={initial_y}"
    );
    // Player Y changed significantly during the run (jumped)
    let final_y = result.player_final_pos.1;
    assert!(
        final_y < initial_y + 50.0,
        "Player should not have fallen far below start: final_y={final_y}, initial_y={initial_y}"
    );
}

#[test]
fn score_incremented() {
    let result = run_platformer();
    assert_eq!(
        result.score, 1,
        "Score should be 1 after collecting the collectible"
    );
}

#[test]
fn correct_frame_count() {
    let result = run_platformer();
    assert_eq!(result.frames_rendered, FRAME_COUNT);
}

#[test]
fn frame_dimensions_correct() {
    let result = run_platformer();
    assert_eq!(result.fb_width, WIDTH);
    assert_eq!(result.fb_height, HEIGHT);
    assert_eq!(
        result.pixel_data.len(),
        (WIDTH * HEIGHT) as usize,
        "Pixel buffer size mismatch"
    );
}

#[test]
fn particles_were_emitted() {
    let result = run_platformer();
    assert!(
        result.particles_emitted > 0,
        "Particles should have been emitted on jump"
    );
}

#[test]
fn tween_completed_after_collect() {
    let result = run_platformer();
    assert!(
        result.tween_completed,
        "Collectible tween should have completed"
    );
}

#[test]
fn audio_buses_configured() {
    let result = run_platformer();
    assert_eq!(result.audio_bus_count, 2, "Should have Master + SFX buses");
}

#[test]
fn rendered_frame_has_non_background_pixels() {
    let result = run_platformer();
    let bg = Color::rgb(0.4, 0.6, 0.9);
    let non_bg_count = result
        .pixel_data
        .iter()
        .filter(|c| {
            (c.r - bg.r).abs() > 0.01 || (c.g - bg.g).abs() > 0.01 || (c.b - bg.b).abs() > 0.01
        })
        .count();
    assert!(
        non_bg_count > 1000,
        "Rendered frame should have many non-background pixels, got {non_bg_count}"
    );
}

#[test]
fn deterministic_two_runs_identical() {
    let result1 = run_platformer();
    let result2 = run_platformer();

    assert_eq!(
        result1.player_final_pos.0, result2.player_final_pos.0,
        "Player X should be identical across runs"
    );
    assert_eq!(
        result1.player_final_pos.1, result2.player_final_pos.1,
        "Player Y should be identical across runs"
    );
    assert_eq!(result1.score, result2.score);
    assert_eq!(result1.frames_rendered, result2.frames_rendered);
    assert_eq!(result1.particles_emitted, result2.particles_emitted);
    assert_eq!(
        result1.player_y_history, result2.player_y_history,
        "Y history should be identical (deterministic physics)"
    );
}

#[test]
fn player_y_history_has_variation() {
    let result = run_platformer();
    let min_y = result
        .player_y_history
        .iter()
        .cloned()
        .fold(f32::MAX, f32::min);
    let max_y = result
        .player_y_history
        .iter()
        .cloned()
        .fold(f32::MIN, f32::max);
    assert!(
        (max_y - min_y) > 20.0,
        "Player Y should have significant variation from jumps: range={}",
        max_y - min_y
    );
}

#[test]
fn scene_tree_has_correct_structure() {
    // Verify the scene tree is built correctly
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let world = Node::new("World", "Node");
    let world_id = tree.add_child(root_id, world).unwrap();

    let player = Node::new("Player", "Node2D");
    tree.add_child(world_id, player).unwrap();

    for i in 0..3 {
        let p = Node::new(format!("Platform{}", i), "Node2D");
        tree.add_child(world_id, p).unwrap();
    }

    let c = Node::new("Collectible", "Node2D");
    tree.add_child(world_id, c).unwrap();

    let s = Node::new("ScoreLabel", "Node");
    tree.add_child(world_id, s).unwrap();

    let g = Node::new("Ground", "Node2D");
    tree.add_child(world_id, g).unwrap();

    // root + World + Player + 3 Platforms + Collectible + ScoreLabel + Ground = 9
    assert_eq!(tree.node_count(), 9);
}

//! Playable platformer demo exercising all Patina Engine subsystems.
//!
//! Demonstrates: scene tree, physics, input, rendering, tweens, particles, audio,
//! and the main loop — all integrated into a single 120-frame simulation.
//!
//! Produces `output/platformer_frame.ppm` and prints a JSON summary.

use gdaudio::{AudioMixer, AudioStreamPlayback};
use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
use gdphysics2d::shape::Shape2D;
use gdphysics2d::world::PhysicsWorld2D;
use gdplatform::input::{ActionBinding, InputEvent, InputMap, InputState, Key};
use gdrender2d::test_adapter::{capture_frame, save_ppm};
use gdrender2d::SoftwareRenderer;
use gdscene::node::Node;
use gdscene::node2d::{get_position, set_position};
use gdscene::particle::{ParticleEmitter, ParticleMaterial, ParticleSimulator};
use gdscene::scene_tree::SceneTree;
use gdscene::tween::{TweenBuilder, EaseType, TransFunc};
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

/// Viewport dimensions.
const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
/// Number of frames to simulate.
const FRAME_COUNT: u64 = 120;
/// Fixed timestep (60 Hz).
const DT: f32 = 1.0 / 60.0;
/// Gravity strength.
const GRAVITY: f32 = 980.0;
/// Player horizontal speed.
const PLAYER_SPEED: f32 = 200.0;
/// Jump impulse strength.
const JUMP_IMPULSE: f32 = -400.0;
/// Collectible pickup radius.
const COLLECT_RADIUS: f32 = 30.0;

/// Result of running the platformer demo, used for testing.
#[derive(Debug, Clone)]
pub struct PlatformerResult {
    pub player_final_pos: (f32, f32),
    pub score: u32,
    pub frames_rendered: u64,
    pub fb_width: u32,
    pub fb_height: u32,
    pub player_y_history: Vec<f32>,
    pub particles_emitted: u64,
    pub tween_completed: bool,
    pub audio_bus_count: usize,
    pub pixel_data: Vec<Color>,
}

/// Scripted input event: (frame, event).
fn build_input_sequence() -> Vec<(u64, InputEvent)> {
    let mut events = Vec::new();

    // Frames 0-30: move right (press at 0, release at 30)
    events.push((0, InputEvent::Key {
        key: Key::Right,
        pressed: true,
        shift: false, ctrl: false, alt: false,
    }));
    events.push((30, InputEvent::Key {
        key: Key::Right,
        pressed: false,
        shift: false, ctrl: false, alt: false,
    }));

    // Frame 35: jump
    events.push((35, InputEvent::Key {
        key: Key::Space,
        pressed: true,
        shift: false, ctrl: false, alt: false,
    }));
    events.push((36, InputEvent::Key {
        key: Key::Space,
        pressed: false,
        shift: false, ctrl: false, alt: false,
    }));

    // Frames 40-80: move right
    events.push((40, InputEvent::Key {
        key: Key::Right,
        pressed: true,
        shift: false, ctrl: false, alt: false,
    }));
    events.push((80, InputEvent::Key {
        key: Key::Right,
        pressed: false,
        shift: false, ctrl: false, alt: false,
    }));

    // Frame 90: jump again
    events.push((90, InputEvent::Key {
        key: Key::Space,
        pressed: true,
        shift: false, ctrl: false, alt: false,
    }));
    events.push((91, InputEvent::Key {
        key: Key::Space,
        pressed: false,
        shift: false, ctrl: false, alt: false,
    }));

    events
}

/// Runs the full platformer demo and returns the result for testing.
pub fn run_platformer() -> PlatformerResult {
    // -----------------------------------------------------------------------
    // 1. Scene setup (programmatic)
    // -----------------------------------------------------------------------
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();

    let world_node = Node::new("World", "Node");
    let world_id = tree.add_child(root_id, world_node).unwrap();

    let player_node = Node::new("Player", "Node2D");
    let player_id = tree.add_child(world_id, player_node).unwrap();
    set_position(&mut tree, player_id, Vector2::new(100.0, 400.0));

    // 3 platforms
    let platform_positions = [
        Vector2::new(200.0, 450.0),
        Vector2::new(350.0, 380.0),
        Vector2::new(500.0, 320.0),
    ];
    let mut platform_ids = Vec::new();
    for (i, pos) in platform_positions.iter().enumerate() {
        let pnode = Node::new(format!("Platform{}", i), "Node2D");
        let pid = tree.add_child(world_id, pnode).unwrap();
        set_position(&mut tree, pid, *pos);
        platform_ids.push(pid);
    }

    let collectible_node = Node::new("Collectible", "Node2D");
    let collectible_id = tree.add_child(world_id, collectible_node).unwrap();
    set_position(&mut tree, collectible_id, Vector2::new(300.0, 350.0));

    let score_node = Node::new("ScoreLabel", "Node");
    let score_id = tree.add_child(world_id, score_node).unwrap();
    tree.get_node_mut(score_id).unwrap()
        .set_property("text", Variant::String("Score: 0".to_string()));

    let ground_node = Node::new("Ground", "Node2D");
    let ground_id = tree.add_child(world_id, ground_node).unwrap();
    set_position(&mut tree, ground_id, Vector2::new(320.0, 550.0));

    // -----------------------------------------------------------------------
    // 2. Physics setup
    // -----------------------------------------------------------------------
    let mut physics = PhysicsWorld2D::new();

    // Player: rigid circle
    let player_body = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Rigid,
        Vector2::new(100.0, 400.0),
        Shape2D::Circle { radius: 16.0 },
        1.0,
    );
    let player_body_id = physics.add_body(player_body);

    // Platform static rects
    let platform_half = Vector2::new(60.0, 10.0);
    for pos in &platform_positions {
        let pbody = PhysicsBody2D::new(
            BodyId(0),
            BodyType::Static,
            *pos,
            Shape2D::Rectangle { half_extents: platform_half },
            1.0,
        );
        physics.add_body(pbody);
    }

    // Collectible: static circle (we check overlap manually)
    let collectible_pos = Vector2::new(300.0, 350.0);

    // Ground: static rect spanning bottom
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

    // -----------------------------------------------------------------------
    // 3. Input setup
    // -----------------------------------------------------------------------
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

    // -----------------------------------------------------------------------
    // 4. Audio setup
    // -----------------------------------------------------------------------
    let mut mixer = AudioMixer::new();
    let _sfx_bus = mixer.add_bus("SFX");

    let mut jump_sfx = AudioStreamPlayback::new(0.5);
    jump_sfx.set_bus("SFX");

    // -----------------------------------------------------------------------
    // 5. Particle emitter (for jump effects)
    // -----------------------------------------------------------------------
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

    // -----------------------------------------------------------------------
    // 6. Tween setup (for collectible)
    // -----------------------------------------------------------------------
    let mut collect_tween = TweenBuilder::new()
        .tween_property(
            "scale",
            Variant::Float(1.0),
            Variant::Float(0.0),
            0.3,
        )
        .set_ease(EaseType::Out)
        .set_trans(TransFunc::Quad)
        .build();
    collect_tween.stop(); // Don't start until collected

    // -----------------------------------------------------------------------
    // 7. Main loop (120 frames)
    // -----------------------------------------------------------------------
    let mut main_loop = gdscene::MainLoop::new(tree);
    let mut score: u32 = 0;
    let mut collected = false;
    let mut player_y_history = Vec::new();
    let mut tween_completed = false;
    let mut collectible_scale = 1.0_f32;

    for frame in 0..FRAME_COUNT {
        // --- Process input ---
        // Feed scripted events for this frame
        for (evt_frame, evt) in &input_sequence {
            if *evt_frame == frame {
                input_state.process_event(evt.clone());
            }
        }

        // Apply input to player body
        if let Some(player) = physics.get_body_mut(player_body_id) {
            // Horizontal movement
            let h_input = if input_state.is_action_pressed("move_right") {
                1.0
            } else if input_state.is_action_pressed("move_left") {
                -1.0
            } else {
                0.0
            };
            player.linear_velocity.x = h_input * PLAYER_SPEED;

            // Jump (only when on/near ground)
            if input_state.is_action_just_pressed("jump") && player.position.y > 350.0 {
                player.apply_impulse(Vector2::new(0.0, JUMP_IMPULSE));

                // Trigger jump particles
                particle_sim.emitter.emitting = true;
                particle_sim.total_emitted = 0;
                particle_sim.time_accumulator = 0.0;

                // Play jump SFX
                jump_sfx.stop();
                jump_sfx.play();
            }

            // Apply gravity
            player.apply_force(Vector2::new(0.0, GRAVITY));
        }

        // --- Step physics ---
        physics.step(DT);

        // --- Check collectible overlap ---
        if !collected {
            if let Some(player) = physics.get_body(player_body_id) {
                let dist = (player.position - collectible_pos).length();
                if dist < COLLECT_RADIUS {
                    collected = true;
                    score += 1;
                    // Start tween to shrink collectible
                    collect_tween.start();
                    // Update score label
                    if let Some(label) = main_loop.tree_mut().get_node_mut(score_id) {
                        label.set_property(
                            "text",
                            Variant::String(format!("Score: {}", score)),
                        );
                    }
                }
            }
        }

        // --- Update node positions from physics ---
        if let Some(pb) = physics.get_body(player_body_id) {
            set_position(main_loop.tree_mut(), player_id, pb.position);
        }

        // --- Process tweens ---
        if collect_tween.running {
            let done = collect_tween.advance(DT as f64);
            if done {
                tween_completed = true;
            }
            // Update collectible scale from tween
            let values = collect_tween.get_current_values();
            if let Some((_, Variant::Float(s))) = values.first() {
                collectible_scale = *s as f32;
            }
        }

        // --- Step particles ---
        particle_sim.step(DT);

        // --- Advance audio ---
        if jump_sfx.is_playing() {
            jump_sfx.advance(DT);
        }

        // --- Step main loop (animations, notifications) ---
        main_loop.step(DT as f64);

        // Record player Y for history
        let player_pos = get_position(main_loop.tree(), player_id);
        player_y_history.push(player_pos.y);

        // --- Flush input frame ---
        input_state.flush_frame();
    }

    // -----------------------------------------------------------------------
    // 8. Render final frame
    // -----------------------------------------------------------------------
    let final_player_pos = get_position(main_loop.tree(), player_id);
    let final_ground_pos = get_position(main_loop.tree(), ground_id);

    let mut renderer = SoftwareRenderer::new();
    let mut viewport = Viewport::new(WIDTH, HEIGHT, Color::rgb(0.4, 0.6, 0.9));

    // Ground: brown rectangle
    let mut ground_item = CanvasItem::new(CanvasItemId(1));
    ground_item.transform = Transform2D::translated(final_ground_pos);
    ground_item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(-320.0, -20.0), Vector2::new(640.0, 40.0)),
        color: Color::rgb(0.55, 0.35, 0.15),
        filled: true,
    });
    viewport.add_canvas_item(ground_item);

    // Platforms: gray rectangles
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

    // Collectible: gold circle (if not fully tweened away)
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

    // Player: blue circle
    let mut player_item = CanvasItem::new(CanvasItemId(30));
    player_item.transform = Transform2D::translated(final_player_pos);
    player_item.z_index = 2;
    player_item.commands.push(DrawCommand::DrawCircle {
        center: Vector2::ZERO,
        radius: 16.0,
        color: Color::rgb(0.2, 0.4, 1.0),
    });
    viewport.add_canvas_item(player_item);

    // Particles: small circles
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

    let result = PlatformerResult {
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
    };

    // -----------------------------------------------------------------------
    // 9. Output
    // -----------------------------------------------------------------------
    println!("\n=== Platformer Demo Summary ===");
    println!("Player final position: ({:.1}, {:.1})", final_player_pos.x, final_player_pos.y);
    println!("Score: {}", score);
    println!("Frames rendered: {}", main_loop.frame_count());
    println!("Particles emitted: {}", particle_sim.total_emitted);
    println!("Tween completed: {}", tween_completed);
    println!("Audio buses: {}", mixer.bus_count());

    // Save PPM
    std::fs::create_dir_all("output").expect("failed to create output directory");
    save_ppm(&fb, "output/platformer_frame.ppm").expect("failed to save PPM");
    println!("Output saved to: output/platformer_frame.ppm");

    // JSON summary
    let json = serde_json::json!({
        "player_final_x": final_player_pos.x,
        "player_final_y": final_player_pos.y,
        "score": score,
        "frames": main_loop.frame_count(),
        "particles_emitted": particle_sim.total_emitted,
        "tween_completed": tween_completed,
        "fb_dimensions": [fb.width, fb.height],
    });
    println!("\nJSON: {}", serde_json::to_string_pretty(&json).unwrap());

    result
}

fn main() {
    run_platformer();
}

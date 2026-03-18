//! Space shooter mini-game exercising all Patina Engine subsystems.
//!
//! Demonstrates: scene tree, physics (Area2D overlap), input, rendering,
//! particles, audio, and the main loop — all integrated into a 300-frame
//! deterministic simulation.
//!
//! Produces `output/space_shooter_frame.ppm` and prints a JSON summary.

use std::collections::HashMap;

use gdaudio::{AudioMixer, AudioStreamPlayback};
use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdphysics2d::area2d::{Area2D, AreaId, AreaStore, OverlapState};
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
use gdphysics2d::shape::Shape2D;
use gdplatform::input::{ActionBinding, InputEvent, InputMap, InputState, Key};
use gdrender2d::test_adapter::{capture_frame, save_ppm};
use gdrender2d::SoftwareRenderer;
use gdscene::node::Node;
use gdscene::node2d::{get_position, set_position};
use gdscene::particle::{EmissionShape, ParticleEmitter, ParticleMaterial, ParticleSimulator};
use gdscene::scene_tree::SceneTree;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

/// Viewport dimensions.
const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
/// Number of frames to simulate.
const FRAME_COUNT: u64 = 300;
/// Fixed timestep (60 Hz).
const DT: f32 = 1.0 / 60.0;
/// Player horizontal speed (pixels per frame).
const PLAYER_SPEED: f32 = 4.0;
/// Bullet vertical speed (pixels per frame, upward).
const BULLET_SPEED: f32 = 6.0;
/// Enemy vertical speed (pixels per frame, downward).
const ENEMY_SPEED: f32 = 2.0;
/// Enemy spawn interval in frames.
const ENEMY_SPAWN_INTERVAL: u64 = 30;
/// Player shoot interval in frames.
const SHOOT_INTERVAL: u64 = 10;
/// Bullet collision radius.
const BULLET_RADIUS: f32 = 4.0;
/// Enemy collision half-extents.
const ENEMY_HALF: Vector2 = Vector2 { x: 15.0, y: 15.0 };

/// Deterministic PRNG (xorshift32).
struct Rng(u32);

impl Rng {
    fn new(seed: u32) -> Self {
        Self(if seed == 0 { 1 } else { seed })
    }
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    /// Returns a value in [min, max).
    fn range(&mut self, min: f32, max: f32) -> f32 {
        let t = (self.next() & 0x00FF_FFFF) as f32 / 16_777_216.0;
        min + t * (max - min)
    }
}

/// Tracks a bullet entity.
#[derive(Debug, Clone)]
struct Bullet {
    node_id: gdscene::NodeId,
    body_id: BodyId,
    area_id: AreaId,
}

/// Tracks an enemy entity.
#[derive(Debug, Clone)]
struct Enemy {
    node_id: gdscene::NodeId,
    body_id: BodyId,
}

/// Result of running the space shooter, used for testing.
#[derive(Debug, Clone)]
pub struct SpaceShooterResult {
    pub score: u32,
    pub player_final_pos: (f32, f32),
    pub enemies_killed: u32,
    pub bullets_fired: u32,
    pub enemies_spawned: u32,
    pub frames_rendered: u64,
    pub fb_width: u32,
    pub fb_height: u32,
    pub particles_emitted: u64,
    pub pixel_data: Vec<Color>,
    pub active_bullets: usize,
    pub active_enemies: usize,
}

/// Builds the scripted input sequence for 300 frames.
fn build_input_sequence() -> Vec<(u64, InputEvent)> {
    let mut events = Vec::new();

    let key_event = |key: Key, pressed: bool| InputEvent::Key {
        key,
        pressed,
        shift: false,
        ctrl: false,
        alt: false,
    };

    // Frames 0-50: move right
    events.push((0, key_event(Key::Right, true)));
    events.push((50, key_event(Key::Right, false)));

    // Frames 51-100: move left
    events.push((51, key_event(Key::Left, true)));
    events.push((100, key_event(Key::Left, false)));

    // Frames 101-200: move right
    events.push((101, key_event(Key::Right, true)));
    events.push((200, key_event(Key::Right, false)));

    // Frames 201-300: move left
    events.push((201, key_event(Key::Left, true)));

    // Shoot pattern: every 10 frames during certain ranges
    let shoot_frames: Vec<u64> = (0..FRAME_COUNT)
        .filter(|f| f % SHOOT_INTERVAL == 5)
        .collect();

    for &f in &shoot_frames {
        events.push((f, key_event(Key::Space, true)));
        events.push((f + 1, key_event(Key::Space, false)));
    }

    events
}

/// Runs the full space shooter demo and returns the result for testing.
pub fn run_space_shooter() -> SpaceShooterResult {
    // -----------------------------------------------------------------------
    // 1. Scene setup
    // -----------------------------------------------------------------------
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();

    let game_root = Node::new("GameRoot", "Node");
    let game_root_id = tree.add_child(root_id, game_root).unwrap();

    let player_node = Node::new("Player", "Node2D");
    let player_id = tree.add_child(game_root_id, player_node).unwrap();
    set_position(&mut tree, player_id, Vector2::new(320.0, 400.0));

    let spawner_node = Node::new("EnemySpawner", "Node");
    let _spawner_id = tree.add_child(game_root_id, spawner_node).unwrap();

    let bullet_container = Node::new("BulletContainer", "Node");
    let bullet_container_id = tree.add_child(game_root_id, bullet_container).unwrap();

    let enemy_container = Node::new("EnemyContainer", "Node");
    let enemy_container_id = tree.add_child(game_root_id, enemy_container).unwrap();

    let ui_node = Node::new("UI", "Node");
    let ui_id = tree.add_child(game_root_id, ui_node).unwrap();
    tree.get_node_mut(ui_id)
        .unwrap()
        .set_property("text", Variant::String("Score: 0".to_string()));

    let particle_node = Node::new("ParticleSystem", "Node");
    let _particle_id = tree.add_child(game_root_id, particle_node).unwrap();

    // -----------------------------------------------------------------------
    // 2. Input setup
    // -----------------------------------------------------------------------
    let mut input_map = InputMap::new();
    input_map.add_action("move_right", 0.0);
    input_map.add_action("move_left", 0.0);
    input_map.add_action("shoot", 0.0);
    input_map.action_add_event("move_right", ActionBinding::KeyBinding(Key::Right));
    input_map.action_add_event("move_left", ActionBinding::KeyBinding(Key::Left));
    input_map.action_add_event("shoot", ActionBinding::KeyBinding(Key::Space));

    let mut input_state = InputState::new();
    input_state.set_input_map(input_map);
    let input_sequence = build_input_sequence();

    // -----------------------------------------------------------------------
    // 3. Audio setup
    // -----------------------------------------------------------------------
    let mut mixer = AudioMixer::new();
    let _sfx_bus = mixer.add_bus("SFX");
    let mut shoot_sfx = AudioStreamPlayback::new(0.2);
    shoot_sfx.set_bus("SFX");
    let mut explosion_sfx = AudioStreamPlayback::new(0.3);
    explosion_sfx.set_bus("SFX");

    // -----------------------------------------------------------------------
    // 4. Particle system (explosions)
    // -----------------------------------------------------------------------
    let explosion_emitter = ParticleEmitter {
        material: ParticleMaterial {
            direction: Vector2::new(0.0, -1.0),
            spread: 180.0,
            initial_velocity_min: 30.0,
            initial_velocity_max: 80.0,
            gravity: Vector2::new(0.0, 50.0),
            start_color: Color::rgb(1.0, 0.8, 0.0),
            end_color: Color::rgb(1.0, 0.0, 0.0),
            ..ParticleMaterial::default()
        },
        emission_shape: EmissionShape::Circle { radius: 5.0 },
        amount: 8,
        lifetime: 0.4,
        one_shot: true,
        explosiveness: 1.0,
        emitting: false,
        ..ParticleEmitter::default()
    };

    // We keep a list of active explosion simulators
    let mut explosions: Vec<(Vector2, ParticleSimulator)> = Vec::new();

    // -----------------------------------------------------------------------
    // 5. Physics bodies and areas for collision detection
    // -----------------------------------------------------------------------
    // We use a simple HashMap-based approach for bodies, and AreaStore for
    // bullet-vs-enemy overlap detection.
    let mut bodies: HashMap<BodyId, PhysicsBody2D> = HashMap::new();
    let mut area_store = AreaStore::new();
    let mut next_body_id: u64 = 1;
    let mut rng = Rng::new(42);

    // Game state
    let mut bullets: Vec<Bullet> = Vec::new();
    let mut enemies: Vec<Enemy> = Vec::new();
    let mut score: u32 = 0;
    let mut enemies_killed: u32 = 0;
    let mut bullets_fired: u32 = 0;
    let mut enemies_spawned: u32 = 0;

    // Map from BodyId -> index in enemies vec, and AreaId -> index in bullets vec
    // Rebuilt each frame for simplicity
    let mut body_to_enemy: HashMap<BodyId, usize> = HashMap::new();
    let mut area_to_bullet: HashMap<AreaId, usize> = HashMap::new();

    // Player position (managed directly, not via physics)
    let mut player_x: f32 = 320.0;
    let player_y: f32 = 400.0;

    // -----------------------------------------------------------------------
    // 6. Main loop (300 frames)
    // -----------------------------------------------------------------------
    let mut main_loop = gdscene::MainLoop::new(tree);

    for frame in 0..FRAME_COUNT {
        // --- Process input ---
        for (evt_frame, evt) in &input_sequence {
            if *evt_frame == frame {
                input_state.process_event(evt.clone());
            }
        }

        // --- Move player ---
        if input_state.is_action_pressed("move_right") {
            player_x = (player_x + PLAYER_SPEED).min(WIDTH as f32 - 20.0);
        }
        if input_state.is_action_pressed("move_left") {
            player_x = (player_x - PLAYER_SPEED).max(20.0);
        }
        set_position(
            main_loop.tree_mut(),
            player_id,
            Vector2::new(player_x, player_y),
        );

        // --- Shoot bullet ---
        if input_state.is_action_just_pressed("shoot") {
            let bid = BodyId(next_body_id);
            next_body_id += 1;

            let bullet_pos = Vector2::new(player_x, player_y - 20.0);

            // Create a body for the bullet (used as overlap target for enemies)
            let bullet_body = PhysicsBody2D::new(
                bid,
                BodyType::Kinematic,
                bullet_pos,
                Shape2D::Circle {
                    radius: BULLET_RADIUS,
                },
                1.0,
            );
            bodies.insert(bid, bullet_body);

            // Create an area for the bullet (for overlap detection)
            let area = Area2D::new(
                AreaId(0),
                bullet_pos,
                Shape2D::Circle {
                    radius: BULLET_RADIUS,
                },
            );
            let area_id = area_store.add_area(area);

            // Create scene node
            let bnode = Node::new(format!("Bullet_{}", bullets_fired), "Node2D");
            let bnode_id = main_loop
                .tree_mut()
                .add_child(bullet_container_id, bnode)
                .unwrap();
            set_position(main_loop.tree_mut(), bnode_id, bullet_pos);

            bullets.push(Bullet {
                node_id: bnode_id,
                body_id: bid,
                area_id,
            });
            bullets_fired += 1;

            // Play shoot SFX
            shoot_sfx.stop();
            shoot_sfx.play();
        }

        // --- Spawn enemies on timer ---
        if frame % ENEMY_SPAWN_INTERVAL == 0 && frame > 0 {
            let enemy_x = rng.range(30.0, WIDTH as f32 - 30.0);
            let enemy_pos = Vector2::new(enemy_x, -20.0);

            let bid = BodyId(next_body_id);
            next_body_id += 1;

            let enemy_body = PhysicsBody2D::new(
                bid,
                BodyType::Kinematic,
                enemy_pos,
                Shape2D::Rectangle {
                    half_extents: ENEMY_HALF,
                },
                1.0,
            );
            bodies.insert(bid, enemy_body);

            let enode = Node::new(format!("Enemy_{}", enemies_spawned), "Node2D");
            let enode_id = main_loop
                .tree_mut()
                .add_child(enemy_container_id, enode)
                .unwrap();
            set_position(main_loop.tree_mut(), enode_id, enemy_pos);

            enemies.push(Enemy {
                node_id: enode_id,
                body_id: bid,
            });
            enemies_spawned += 1;
        }

        // --- Move bullets upward ---
        for bullet in &bullets {
            if let Some(body) = bodies.get_mut(&bullet.body_id) {
                body.position.y -= BULLET_SPEED;
            }
            if let Some(area) = area_store.get_area_mut(bullet.area_id) {
                if let Some(body) = bodies.get(&bullet.body_id) {
                    area.position = body.position;
                }
            }
            if let Some(body) = bodies.get(&bullet.body_id) {
                set_position(main_loop.tree_mut(), bullet.node_id, body.position);
            }
        }

        // --- Move enemies downward ---
        for enemy in &enemies {
            if let Some(body) = bodies.get_mut(&enemy.body_id) {
                body.position.y += ENEMY_SPEED;
            }
            if let Some(body) = bodies.get(&enemy.body_id) {
                set_position(main_loop.tree_mut(), enemy.node_id, body.position);
            }
        }

        // --- Check collisions (bullet areas vs enemy bodies) ---
        // Rebuild lookup maps
        body_to_enemy.clear();
        area_to_bullet.clear();
        for (i, enemy) in enemies.iter().enumerate() {
            body_to_enemy.insert(enemy.body_id, i);
        }
        for (i, bullet) in bullets.iter().enumerate() {
            area_to_bullet.insert(bullet.area_id, i);
        }

        let overlap_events = area_store.detect_overlaps(&bodies);

        let mut bullets_to_remove = Vec::new();
        let mut enemies_to_remove = Vec::new();

        for event in &overlap_events {
            if event.state == OverlapState::Entered {
                // Check if this is a bullet area hitting an enemy body
                if let (Some(&bullet_idx), Some(&enemy_idx)) = (
                    area_to_bullet.get(&event.area_id),
                    body_to_enemy.get(&event.body_id),
                ) {
                    bullets_to_remove.push(bullet_idx);
                    enemies_to_remove.push(enemy_idx);

                    // Spawn explosion at enemy position
                    if let Some(body) = bodies.get(&enemies[enemy_idx].body_id) {
                        let mut sim = ParticleSimulator::new(explosion_emitter.clone());
                        sim.emitter.emitting = true;
                        // Trigger burst immediately
                        sim.step(sim.emitter.lifetime);
                        explosions.push((body.position, sim));
                    }

                    score += 1;
                    enemies_killed += 1;

                    // Play explosion SFX
                    explosion_sfx.stop();
                    explosion_sfx.play();
                }
            }
        }

        // Remove hit bullets and enemies (in reverse order to preserve indices)
        bullets_to_remove.sort_unstable();
        bullets_to_remove.dedup();
        enemies_to_remove.sort_unstable();
        enemies_to_remove.dedup();

        for &idx in bullets_to_remove.iter().rev() {
            if idx < bullets.len() {
                let b = bullets.remove(idx);
                bodies.remove(&b.body_id);
                area_store.remove_area(b.area_id);
            }
        }
        for &idx in enemies_to_remove.iter().rev() {
            if idx < enemies.len() {
                let e = enemies.remove(idx);
                bodies.remove(&e.body_id);
            }
        }

        // --- Remove off-screen bullets (y < 0) ---
        let mut offscreen_bullets = Vec::new();
        for (i, bullet) in bullets.iter().enumerate() {
            if let Some(body) = bodies.get(&bullet.body_id) {
                if body.position.y < 0.0 {
                    offscreen_bullets.push(i);
                }
            }
        }
        for &idx in offscreen_bullets.iter().rev() {
            let b = bullets.remove(idx);
            bodies.remove(&b.body_id);
            area_store.remove_area(b.area_id);
        }

        // --- Remove off-screen enemies (y > HEIGHT + 30) ---
        let mut offscreen_enemies = Vec::new();
        for (i, enemy) in enemies.iter().enumerate() {
            if let Some(body) = bodies.get(&enemy.body_id) {
                if body.position.y > HEIGHT as f32 + 30.0 {
                    offscreen_enemies.push(i);
                }
            }
        }
        for &idx in offscreen_enemies.iter().rev() {
            let e = enemies.remove(idx);
            bodies.remove(&e.body_id);
        }

        // --- Step particle explosions ---
        for (_, sim) in &mut explosions {
            sim.step(DT);
        }
        explosions.retain(|(_, sim)| !sim.is_complete());

        // --- Advance audio ---
        if shoot_sfx.is_playing() {
            shoot_sfx.advance(DT);
        }
        if explosion_sfx.is_playing() {
            explosion_sfx.advance(DT);
        }

        // --- Update score label ---
        if let Some(label) = main_loop.tree_mut().get_node_mut(ui_id) {
            label.set_property("text", Variant::String(format!("Score: {}", score)));
        }

        // --- Step main loop ---
        main_loop.step(DT as f64);

        // --- Flush input ---
        input_state.flush_frame();
    }

    // -----------------------------------------------------------------------
    // 7. Render final frame
    // -----------------------------------------------------------------------
    let final_player_pos = get_position(main_loop.tree(), player_id);

    let mut renderer = SoftwareRenderer::new();
    let mut viewport = Viewport::new(WIDTH, HEIGHT, Color::rgb(0.05, 0.05, 0.15));

    let mut canvas_id_counter: u64 = 1;
    let mut next_canvas_id = || {
        let id = CanvasItemId(canvas_id_counter);
        canvas_id_counter += 1;
        id
    };

    // Player: blue triangle-ish rect (wider at bottom)
    let mut player_item = CanvasItem::new(next_canvas_id());
    player_item.transform = Transform2D::translated(final_player_pos);
    player_item.z_index = 2;
    // Ship body
    player_item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(-10.0, -15.0), Vector2::new(20.0, 30.0)),
        color: Color::rgb(0.2, 0.5, 1.0),
        filled: true,
    });
    // Ship nose
    player_item.commands.push(DrawCommand::DrawRect {
        rect: Rect2::new(Vector2::new(-5.0, -20.0), Vector2::new(10.0, 8.0)),
        color: Color::rgb(0.3, 0.7, 1.0),
        filled: true,
    });
    viewport.add_canvas_item(player_item);

    // Bullets: yellow small rects
    for bullet in &bullets {
        if let Some(body) = bodies.get(&bullet.body_id) {
            let mut item = CanvasItem::new(next_canvas_id());
            item.transform = Transform2D::translated(body.position);
            item.z_index = 1;
            item.commands.push(DrawCommand::DrawRect {
                rect: Rect2::new(Vector2::new(-2.0, -4.0), Vector2::new(4.0, 8.0)),
                color: Color::rgb(1.0, 1.0, 0.0),
                filled: true,
            });
            viewport.add_canvas_item(item);
        }
    }

    // Enemies: red rects
    for enemy in &enemies {
        if let Some(body) = bodies.get(&enemy.body_id) {
            let mut item = CanvasItem::new(next_canvas_id());
            item.transform = Transform2D::translated(body.position);
            item.z_index = 1;
            item.commands.push(DrawCommand::DrawRect {
                rect: Rect2::new(
                    Vector2::new(-ENEMY_HALF.x, -ENEMY_HALF.y),
                    Vector2::new(ENEMY_HALF.x * 2.0, ENEMY_HALF.y * 2.0),
                ),
                color: Color::rgb(1.0, 0.2, 0.2),
                filled: true,
            });
            viewport.add_canvas_item(item);
        }
    }

    // Explosion particles
    for (pos, sim) in &explosions {
        for (ppos, color, scale) in sim.get_draw_commands() {
            let mut item = CanvasItem::new(next_canvas_id());
            item.transform = Transform2D::translated(*pos + ppos);
            item.z_index = 3;
            item.commands.push(DrawCommand::DrawCircle {
                center: Vector2::ZERO,
                radius: 2.0 * scale,
                color,
            });
            viewport.add_canvas_item(item);
        }
    }

    let fb = capture_frame(&mut renderer, &viewport);

    // Total particles emitted across all explosions
    let total_particles_emitted: u64 = enemies_killed as u64 * explosion_emitter.amount as u64;

    let result = SpaceShooterResult {
        score,
        player_final_pos: (final_player_pos.x, final_player_pos.y),
        enemies_killed,
        bullets_fired,
        enemies_spawned,
        frames_rendered: main_loop.frame_count(),
        fb_width: fb.width,
        fb_height: fb.height,
        particles_emitted: total_particles_emitted,
        pixel_data: fb.pixels.clone(),
        active_bullets: bullets.len(),
        active_enemies: enemies.len(),
    };

    // -----------------------------------------------------------------------
    // 8. Output
    // -----------------------------------------------------------------------
    println!("\n=== Space Shooter Summary ===");
    println!(
        "Player final position: ({:.1}, {:.1})",
        final_player_pos.x, final_player_pos.y
    );
    println!("Score: {}", score);
    println!("Enemies killed: {}", enemies_killed);
    println!("Bullets fired: {}", bullets_fired);
    println!("Enemies spawned: {}", enemies_spawned);
    println!("Frames rendered: {}", main_loop.frame_count());
    println!("Particles emitted: {}", total_particles_emitted);

    std::fs::create_dir_all("output").expect("failed to create output directory");
    save_ppm(&fb, "output/space_shooter_frame.ppm").expect("failed to save PPM");
    println!("Output saved to: output/space_shooter_frame.ppm");

    let json = serde_json::json!({
        "score": score,
        "player_final_x": final_player_pos.x,
        "player_final_y": final_player_pos.y,
        "enemies_killed": enemies_killed,
        "bullets_fired": bullets_fired,
        "enemies_spawned": enemies_spawned,
        "frames_rendered": main_loop.frame_count(),
        "particles_emitted": total_particles_emitted,
        "fb_dimensions": [fb.width, fb.height],
    });
    println!("\nJSON: {}", serde_json::to_string_pretty(&json).unwrap());

    result
}

fn main() {
    run_space_shooter();
}

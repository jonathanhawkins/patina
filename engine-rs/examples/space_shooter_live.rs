//! Interactive space shooter with HTTP frame server for live browser preview.
//!
//! Starts an HTTP server on port 8080 (configurable) that streams rendered
//! frames as BMP images. Open the URL in a browser to view and control
//! the game with arrow keys and space bar.
//!
//! Usage:
//!   cargo run --example space_shooter_live
//!   cargo run --example space_shooter_live -- --port 9090
//!   cargo run --example space_shooter_live -- --frames 600
//!   cargo run --example space_shooter_live -- --headless

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdphysics2d::area2d::{Area2D, AreaId, AreaStore, OverlapState};
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
use gdphysics2d::shape::Shape2D;
use gdplatform::input::{ActionBinding, InputEvent, InputMap, InputState, Key};
use gdrender2d::frame_server::{self, BrowserInputEvent, BrowserKey, FrameServerHandle};
use gdrender2d::renderer::FrameBuffer;
use gdrender2d::test_adapter::capture_frame;
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
/// Fixed timestep (30 Hz for browser preview).
const DT: f32 = 1.0 / 30.0;
/// Player horizontal speed (pixels per frame).
const PLAYER_SPEED: f32 = 6.0;
/// Bullet vertical speed (pixels per frame, upward).
const BULLET_SPEED: f32 = 8.0;
/// Enemy vertical speed (pixels per frame, downward).
const ENEMY_SPEED: f32 = 3.0;
/// Enemy spawn interval in frames.
const ENEMY_SPAWN_INTERVAL: u64 = 20;
/// Player shoot interval in frames.
const SHOOT_INTERVAL: u64 = 6;
/// Bullet collision radius.
const BULLET_RADIUS: f32 = 4.0;
/// Enemy collision half-extents.
const ENEMY_HALF: Vector2 = Vector2 { x: 15.0, y: 15.0 };
/// Target frame time for ~30fps.
const FRAME_TIME: Duration = Duration::from_millis(33);

/// Deterministic PRNG (xorshift32).
pub struct Rng(u32);

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
pub struct Bullet {
    node_id: gdscene::NodeId,
    body_id: BodyId,
    area_id: AreaId,
}

/// Tracks an enemy entity.
#[derive(Debug, Clone)]
pub struct Enemy {
    node_id: gdscene::NodeId,
    body_id: BodyId,
}

/// Converts a [`BrowserKey`] to a [`Key`].
fn browser_key_to_key(bk: BrowserKey) -> Key {
    match bk {
        BrowserKey::A => Key::A,
        BrowserKey::B => Key::B,
        BrowserKey::C => Key::C,
        BrowserKey::D => Key::D,
        BrowserKey::E => Key::E,
        BrowserKey::F => Key::F,
        BrowserKey::G => Key::G,
        BrowserKey::H => Key::H,
        BrowserKey::I => Key::I,
        BrowserKey::J => Key::J,
        BrowserKey::K => Key::K,
        BrowserKey::L => Key::L,
        BrowserKey::M => Key::M,
        BrowserKey::N => Key::N,
        BrowserKey::O => Key::O,
        BrowserKey::P => Key::P,
        BrowserKey::Q => Key::Q,
        BrowserKey::R => Key::R,
        BrowserKey::S => Key::S,
        BrowserKey::T => Key::T,
        BrowserKey::U => Key::U,
        BrowserKey::V => Key::V,
        BrowserKey::W => Key::W,
        BrowserKey::X => Key::X,
        BrowserKey::Y => Key::Y,
        BrowserKey::Z => Key::Z,
        BrowserKey::Num0 => Key::Num0,
        BrowserKey::Num1 => Key::Num1,
        BrowserKey::Num2 => Key::Num2,
        BrowserKey::Num3 => Key::Num3,
        BrowserKey::Num4 => Key::Num4,
        BrowserKey::Num5 => Key::Num5,
        BrowserKey::Num6 => Key::Num6,
        BrowserKey::Num7 => Key::Num7,
        BrowserKey::Num8 => Key::Num8,
        BrowserKey::Num9 => Key::Num9,
        BrowserKey::Space => Key::Space,
        BrowserKey::Enter => Key::Enter,
        BrowserKey::Escape => Key::Escape,
        BrowserKey::Tab => Key::Tab,
        BrowserKey::Shift => Key::Shift,
        BrowserKey::Ctrl => Key::Ctrl,
        BrowserKey::Alt => Key::Alt,
        BrowserKey::Up => Key::Up,
        BrowserKey::Down => Key::Down,
        BrowserKey::Left => Key::Left,
        BrowserKey::Right => Key::Right,
        BrowserKey::F1 => Key::F1,
        BrowserKey::F2 => Key::F2,
        BrowserKey::F3 => Key::F3,
        BrowserKey::F4 => Key::F4,
        BrowserKey::F5 => Key::F5,
        BrowserKey::F6 => Key::F6,
        BrowserKey::F7 => Key::F7,
        BrowserKey::F8 => Key::F8,
        BrowserKey::F9 => Key::F9,
        BrowserKey::F10 => Key::F10,
        BrowserKey::F11 => Key::F11,
        BrowserKey::F12 => Key::F12,
        BrowserKey::Backspace => Key::Backspace,
        BrowserKey::Delete => Key::Delete,
        BrowserKey::Insert => Key::Insert,
        BrowserKey::Home => Key::Home,
        BrowserKey::End => Key::End,
        BrowserKey::PageUp => Key::PageUp,
        BrowserKey::PageDown => Key::PageDown,
        BrowserKey::CapsLock => Key::CapsLock,
        BrowserKey::Comma => Key::Comma,
        BrowserKey::Period => Key::Period,
        BrowserKey::Slash => Key::Slash,
        BrowserKey::Semicolon => Key::Semicolon,
        BrowserKey::Quote => Key::Quote,
        BrowserKey::BracketLeft => Key::BracketLeft,
        BrowserKey::BracketRight => Key::BracketRight,
        BrowserKey::Backslash => Key::Backslash,
        BrowserKey::Minus => Key::Minus,
        BrowserKey::Equal => Key::Equal,
    }
}

/// Converts a [`BrowserInputEvent`] to an [`InputEvent`].
pub fn browser_event_to_input(event: &BrowserInputEvent) -> InputEvent {
    match event {
        BrowserInputEvent::Key { key, pressed } => InputEvent::Key {
            key: browser_key_to_key(*key),
            pressed: *pressed,
            shift: false,
            ctrl: false,
            alt: false,
        },
    }
}

/// Game state container for the live space shooter.
pub struct LiveGame {
    pub main_loop: gdscene::MainLoop,
    pub input_state: InputState,
    pub bodies: HashMap<BodyId, PhysicsBody2D>,
    pub area_store: AreaStore,
    pub bullets: Vec<Bullet>,
    pub enemies: Vec<Enemy>,
    pub explosions: Vec<(Vector2, ParticleSimulator)>,
    pub explosion_emitter: ParticleEmitter,
    pub score: u32,
    pub enemies_killed: u32,
    pub bullets_fired: u32,
    pub enemies_spawned: u32,
    pub frame_count: u64,
    pub player_x: f32,
    pub player_y: f32,
    pub player_id: gdscene::NodeId,
    pub bullet_container_id: gdscene::NodeId,
    pub enemy_container_id: gdscene::NodeId,
    pub ui_id: gdscene::NodeId,
    pub next_body_id: u64,
    pub rng: Rng,
    pub last_shoot_frame: u64,
}

impl LiveGame {
    /// Creates a new game with all subsystems initialized.
    pub fn new() -> Self {
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

        // Input setup
        let mut input_map = InputMap::new();
        input_map.add_action("move_right", 0.0);
        input_map.add_action("move_left", 0.0);
        input_map.add_action("shoot", 0.0);
        input_map.action_add_event("move_right", ActionBinding::KeyBinding(Key::Right));
        input_map.action_add_event("move_left", ActionBinding::KeyBinding(Key::Left));
        input_map.action_add_event("shoot", ActionBinding::KeyBinding(Key::Space));

        let mut input_state = InputState::new();
        input_state.set_input_map(input_map);

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

        let main_loop = gdscene::MainLoop::new(tree);

        LiveGame {
            main_loop,
            input_state,
            bodies: HashMap::new(),
            area_store: AreaStore::new(),
            bullets: Vec::new(),
            enemies: Vec::new(),
            explosions: Vec::new(),
            explosion_emitter,
            score: 0,
            enemies_killed: 0,
            bullets_fired: 0,
            enemies_spawned: 0,
            frame_count: 0,
            player_x: 320.0,
            player_y: 400.0,
            player_id,
            bullet_container_id,
            enemy_container_id,
            ui_id,
            next_body_id: 1,
            rng: Rng::new(42),
            last_shoot_frame: 0,
        }
    }

    /// Steps one frame of game logic.
    pub fn step(&mut self) {
        let frame = self.frame_count;

        // Move player
        if self.input_state.is_action_pressed("move_right") {
            self.player_x = (self.player_x + PLAYER_SPEED).min(WIDTH as f32 - 20.0);
        }
        if self.input_state.is_action_pressed("move_left") {
            self.player_x = (self.player_x - PLAYER_SPEED).max(20.0);
        }
        set_position(
            self.main_loop.tree_mut(),
            self.player_id,
            Vector2::new(self.player_x, self.player_y),
        );

        // Shoot bullet (rate-limited)
        if self.input_state.is_action_pressed("shoot")
            && frame.saturating_sub(self.last_shoot_frame) >= SHOOT_INTERVAL
        {
            let bid = BodyId(self.next_body_id);
            self.next_body_id += 1;

            let bullet_pos = Vector2::new(self.player_x, self.player_y - 20.0);

            let bullet_body = PhysicsBody2D::new(
                bid,
                BodyType::Kinematic,
                bullet_pos,
                Shape2D::Circle {
                    radius: BULLET_RADIUS,
                },
                1.0,
            );
            self.bodies.insert(bid, bullet_body);

            let area = Area2D::new(
                AreaId(0),
                bullet_pos,
                Shape2D::Circle {
                    radius: BULLET_RADIUS,
                },
            );
            let area_id = self.area_store.add_area(area);

            let bnode = Node::new(format!("Bullet_{}", self.bullets_fired), "Node2D");
            let bnode_id = self
                .main_loop
                .tree_mut()
                .add_child(self.bullet_container_id, bnode)
                .unwrap();
            set_position(self.main_loop.tree_mut(), bnode_id, bullet_pos);

            self.bullets.push(Bullet {
                node_id: bnode_id,
                body_id: bid,
                area_id,
            });
            self.bullets_fired += 1;
            self.last_shoot_frame = frame;
        }

        // Spawn enemies on timer
        if frame % ENEMY_SPAWN_INTERVAL == 0 && frame > 0 {
            let enemy_x = self.rng.range(30.0, WIDTH as f32 - 30.0);
            let enemy_pos = Vector2::new(enemy_x, -20.0);

            let bid = BodyId(self.next_body_id);
            self.next_body_id += 1;

            let enemy_body = PhysicsBody2D::new(
                bid,
                BodyType::Kinematic,
                enemy_pos,
                Shape2D::Rectangle {
                    half_extents: ENEMY_HALF,
                },
                1.0,
            );
            self.bodies.insert(bid, enemy_body);

            let enode = Node::new(format!("Enemy_{}", self.enemies_spawned), "Node2D");
            let enode_id = self
                .main_loop
                .tree_mut()
                .add_child(self.enemy_container_id, enode)
                .unwrap();
            set_position(self.main_loop.tree_mut(), enode_id, enemy_pos);

            self.enemies.push(Enemy {
                node_id: enode_id,
                body_id: bid,
            });
            self.enemies_spawned += 1;
        }

        // Move bullets upward
        for bullet in &self.bullets {
            if let Some(body) = self.bodies.get_mut(&bullet.body_id) {
                body.position.y -= BULLET_SPEED;
            }
            if let Some(area) = self.area_store.get_area_mut(bullet.area_id) {
                if let Some(body) = self.bodies.get(&bullet.body_id) {
                    area.position = body.position;
                }
            }
            if let Some(body) = self.bodies.get(&bullet.body_id) {
                set_position(self.main_loop.tree_mut(), bullet.node_id, body.position);
            }
        }

        // Move enemies downward
        for enemy in &self.enemies {
            if let Some(body) = self.bodies.get_mut(&enemy.body_id) {
                body.position.y += ENEMY_SPEED;
            }
            if let Some(body) = self.bodies.get(&enemy.body_id) {
                set_position(self.main_loop.tree_mut(), enemy.node_id, body.position);
            }
        }

        // Check collisions
        let mut body_to_enemy: HashMap<BodyId, usize> = HashMap::new();
        let mut area_to_bullet: HashMap<AreaId, usize> = HashMap::new();
        for (i, enemy) in self.enemies.iter().enumerate() {
            body_to_enemy.insert(enemy.body_id, i);
        }
        for (i, bullet) in self.bullets.iter().enumerate() {
            area_to_bullet.insert(bullet.area_id, i);
        }

        let overlap_events = self.area_store.detect_overlaps(&self.bodies);

        let mut bullets_to_remove = Vec::new();
        let mut enemies_to_remove = Vec::new();

        for event in &overlap_events {
            if event.state == OverlapState::Entered {
                if let (Some(&bullet_idx), Some(&enemy_idx)) = (
                    area_to_bullet.get(&event.area_id),
                    body_to_enemy.get(&event.body_id),
                ) {
                    bullets_to_remove.push(bullet_idx);
                    enemies_to_remove.push(enemy_idx);

                    if let Some(body) = self.bodies.get(&self.enemies[enemy_idx].body_id) {
                        let mut sim = ParticleSimulator::new(self.explosion_emitter.clone());
                        sim.emitter.emitting = true;
                        sim.step(sim.emitter.lifetime);
                        self.explosions.push((body.position, sim));
                    }

                    self.score += 1;
                    self.enemies_killed += 1;
                }
            }
        }

        bullets_to_remove.sort_unstable();
        bullets_to_remove.dedup();
        enemies_to_remove.sort_unstable();
        enemies_to_remove.dedup();

        for &idx in bullets_to_remove.iter().rev() {
            if idx < self.bullets.len() {
                let b = self.bullets.remove(idx);
                self.bodies.remove(&b.body_id);
                self.area_store.remove_area(b.area_id);
            }
        }
        for &idx in enemies_to_remove.iter().rev() {
            if idx < self.enemies.len() {
                let e = self.enemies.remove(idx);
                self.bodies.remove(&e.body_id);
            }
        }

        // Remove off-screen bullets
        let mut offscreen_bullets = Vec::new();
        for (i, bullet) in self.bullets.iter().enumerate() {
            if let Some(body) = self.bodies.get(&bullet.body_id) {
                if body.position.y < 0.0 {
                    offscreen_bullets.push(i);
                }
            }
        }
        for &idx in offscreen_bullets.iter().rev() {
            let b = self.bullets.remove(idx);
            self.bodies.remove(&b.body_id);
            self.area_store.remove_area(b.area_id);
        }

        // Remove off-screen enemies
        let mut offscreen_enemies = Vec::new();
        for (i, enemy) in self.enemies.iter().enumerate() {
            if let Some(body) = self.bodies.get(&enemy.body_id) {
                if body.position.y > HEIGHT as f32 + 30.0 {
                    offscreen_enemies.push(i);
                }
            }
        }
        for &idx in offscreen_enemies.iter().rev() {
            let e = self.enemies.remove(idx);
            self.bodies.remove(&e.body_id);
        }

        // Step particle explosions
        for (_, sim) in &mut self.explosions {
            sim.step(DT);
        }
        self.explosions.retain(|(_, sim)| !sim.is_complete());

        // Update score label
        if let Some(label) = self.main_loop.tree_mut().get_node_mut(self.ui_id) {
            label.set_property("text", Variant::String(format!("Score: {}", self.score)));
        }

        // Step main loop
        self.main_loop.step(DT as f64);
        self.input_state.flush_frame();
        self.frame_count += 1;
    }

    /// Renders the current game state to a [`FrameBuffer`].
    pub fn render(&self) -> FrameBuffer {
        let final_player_pos = get_position(self.main_loop.tree(), self.player_id);

        let mut renderer = SoftwareRenderer::new();
        let mut viewport = Viewport::new(WIDTH, HEIGHT, Color::rgb(0.05, 0.05, 0.15));

        let mut canvas_id_counter: u64 = 1;
        let mut next_canvas_id = || {
            let id = CanvasItemId(canvas_id_counter);
            canvas_id_counter += 1;
            id
        };

        // Player ship
        let mut player_item = CanvasItem::new(next_canvas_id());
        player_item.transform = Transform2D::translated(final_player_pos);
        player_item.z_index = 2;
        player_item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::new(-10.0, -15.0), Vector2::new(20.0, 30.0)),
            color: Color::rgb(0.2, 0.5, 1.0),
            filled: true,
        });
        player_item.commands.push(DrawCommand::DrawRect {
            rect: Rect2::new(Vector2::new(-5.0, -20.0), Vector2::new(10.0, 8.0)),
            color: Color::rgb(0.3, 0.7, 1.0),
            filled: true,
        });
        viewport.add_canvas_item(player_item);

        // Bullets
        for bullet in &self.bullets {
            if let Some(body) = self.bodies.get(&bullet.body_id) {
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

        // Enemies
        for enemy in &self.enemies {
            if let Some(body) = self.bodies.get(&enemy.body_id) {
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
        for (pos, sim) in &self.explosions {
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

        capture_frame(&mut renderer, &viewport)
    }

    /// Processes browser input events from the frame server.
    pub fn process_browser_input(&mut self, events: &[BrowserInputEvent]) {
        for event in events {
            let input_event = browser_event_to_input(event);
            self.input_state.process_event(input_event);
        }
    }
}

/// Runs the live game loop with the given frame server.
pub fn run_live(server: &FrameServerHandle, max_frames: Option<u64>, running: &AtomicBool) {
    let mut game = LiveGame::new();
    let start_time = Instant::now();
    let mut last_status_frame: u64 = 0;

    while running.load(Ordering::SeqCst) {
        let frame_start = Instant::now();

        // Process browser input
        let browser_events = server.drain_input();
        game.process_browser_input(&browser_events);

        // Step game
        game.step();

        // Render and update server
        let fb = game.render();
        server.update_frame(&fb);

        // Update status
        let elapsed = start_time.elapsed().as_secs_f64();
        let fps = if elapsed > 0.0 {
            game.frame_count as f64 / elapsed
        } else {
            0.0
        };
        server.update_status(game.frame_count, fps);

        // Print status every 60 frames
        if game.frame_count - last_status_frame >= 60 {
            println!(
                "[Frame {}] Score: {} | Enemies: {} | Bullets: {} | FPS: {:.1}",
                game.frame_count,
                game.score,
                game.enemies.len(),
                game.bullets.len(),
                fps,
            );
            last_status_frame = game.frame_count;
        }

        // Check frame limit
        if let Some(max) = max_frames {
            if game.frame_count >= max {
                break;
            }
        }

        // Sleep to target ~30fps
        let elapsed_frame = frame_start.elapsed();
        if elapsed_frame < FRAME_TIME {
            std::thread::sleep(FRAME_TIME - elapsed_frame);
        }
    }

    println!(
        "\n=== Space Shooter Live Summary ===\nFrames: {} | Score: {} | Killed: {} | Bullets fired: {}",
        game.frame_count, game.score, game.enemies_killed, game.bullets_fired
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let mut port: u16 = 8080;
    let mut max_frames: Option<u64> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => {
                i += 1;
                if i < args.len() {
                    port = args[i].parse().expect("invalid port number");
                }
            }
            "--frames" => {
                i += 1;
                if i < args.len() {
                    max_frames = Some(args[i].parse().expect("invalid frame count"));
                }
            }
            "--headless" => {
                // Default mode is already headless (no window)
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
            }
        }
        i += 1;
    }

    // Set up Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = Arc::clone(&running);
    ctrlc_flag(&r);

    // Start frame server
    let server = frame_server::start(port);
    println!("Open http://localhost:{port} to view in browser");
    println!("Use arrow keys to move, space to shoot. Press Ctrl+C to quit.");

    run_live(&server, max_frames, &running);

    server.stop();
}

/// Sets a Ctrl+C handler that flips the `running` flag to false.
///
/// Uses a global atomic flag since signal handlers cannot capture closures.
fn ctrlc_flag(running: &Arc<AtomicBool>) {
    // Store the flag pointer in a global so the signal handler can access it.
    RUNNING_PTR.store(Arc::as_ptr(running) as usize, Ordering::SeqCst);
    // Increment refcount so the Arc stays alive as long as the handler exists.
    std::mem::forget(Arc::clone(running));

    // SAFETY: We register a POSIX signal handler that only writes to an atomic bool.
    // The pointer stored in RUNNING_PTR is valid for the lifetime of the program
    // because we leaked an Arc reference above.
    #[cfg(unix)]
    unsafe {
        libc_sigaction();
    }
}

static RUNNING_PTR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(unix)]
unsafe fn libc_sigaction() {
    // Use raw syscall to register SIGINT handler without libc dependency.
    // On macOS/Linux, signal() is always available via the C runtime.
    extern "C" {
        fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
    }
    signal(2 /* SIGINT */, sigint_handler);
}

#[cfg(unix)]
extern "C" fn sigint_handler(_sig: i32) {
    let ptr = RUNNING_PTR.load(Ordering::SeqCst);
    if ptr != 0 {
        // SAFETY: ptr was stored from a valid Arc<AtomicBool> that we leaked a reference to.
        let flag = unsafe { &*(ptr as *const AtomicBool) };
        flag.store(false, Ordering::SeqCst);
    }
}

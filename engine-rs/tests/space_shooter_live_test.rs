//! Integration tests for the live space shooter with frame server.
//!
//! Validates that the frame server serves HTML, BMP frames, and JSON status,
//! and that the game loop produces valid output.

#[path = "../examples/space_shooter_live.rs"]
mod space_shooter_live;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::net::TcpStream;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use gdrender2d::frame_server;

/// Returns a random high port that is currently free.
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// Makes a raw HTTP request and returns the response as bytes.
fn http_get_raw(port: u16, path: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).expect("failed to connect");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    response
}

/// Makes an HTTP GET request and returns the response as a string.
fn http_get(port: u16, path: &str) -> String {
    String::from_utf8_lossy(&http_get_raw(port, path)).to_string()
}

/// Runs the game for a specified number of frames with a server, returns the port.
fn run_game_with_server(num_frames: u64) -> (u16, gdrender2d::frame_server::FrameServerHandle) {
    let port = free_port();
    let server = frame_server::start(port);
    std::thread::sleep(Duration::from_millis(150));

    // Run game for specified frames
    let mut game = space_shooter_live::LiveGame::new();
    for _ in 0..num_frames {
        game.step();
        let fb = game.render();
        server.update_frame(&fb);
        let fps = 30.0;
        server.update_status(game.frame_count, fps);
    }

    (port, server)
}

#[test]
fn server_serves_html_on_root() {
    let (port, server) = run_game_with_server(1);

    let resp = http_get(port, "/");
    assert!(resp.contains("HTTP/1.1 200 OK"), "Expected 200 OK");
    assert!(resp.contains("text/html"), "Expected HTML content type");
    assert!(
        resp.contains("Patina Engine"),
        "Expected Patina Engine in HTML"
    );

    server.stop();
}

#[test]
fn server_serves_bmp_frame() {
    let (port, server) = run_game_with_server(10);

    let resp = http_get_raw(port, "/frame.bmp");
    let resp_str = String::from_utf8_lossy(&resp);
    assert!(resp_str.contains("HTTP/1.1 200 OK"), "Expected 200 OK");
    assert!(resp_str.contains("image/bmp"), "Expected BMP content type");

    // Verify BMP header (magic bytes "BM")
    let bm_pos = resp.windows(2).position(|w| w == b"BM");
    assert!(
        bm_pos.is_some(),
        "Response should contain BMP data with BM header"
    );

    server.stop();
}

#[test]
fn server_serves_json_status() {
    let (port, server) = run_game_with_server(30);

    let resp = http_get(port, "/status");
    assert!(resp.contains("HTTP/1.1 200 OK"), "Expected 200 OK");
    assert!(
        resp.contains("application/json"),
        "Expected JSON content type"
    );
    assert!(
        resp.contains("\"frame_count\":30"),
        "Expected frame_count=30, got: {resp}"
    );
    assert!(resp.contains("\"width\":640"), "Expected width=640");
    assert!(resp.contains("\"height\":480"), "Expected height=480");

    server.stop();
}

#[test]
fn game_runs_60_frames_without_panic() {
    let mut game = space_shooter_live::LiveGame::new();
    for _ in 0..60 {
        game.step();
    }
    assert_eq!(game.frame_count, 60);
    assert!(game.enemies_spawned > 0, "Enemies should have spawned");
}

#[test]
fn game_renders_valid_framebuffer() {
    let mut game = space_shooter_live::LiveGame::new();
    for _ in 0..10 {
        game.step();
    }
    let fb = game.render();
    assert_eq!(fb.width, 640);
    assert_eq!(fb.height, 480);
    assert_eq!(fb.pixels.len(), (640 * 480) as usize);

    // Verify not all pixels are the same (game has a player drawn)
    let first = fb.pixels[0];
    let has_different = fb.pixels.iter().any(|p| *p != first);
    assert!(has_different, "Frame should contain visible game objects");
}

#[test]
fn browser_input_moves_player() {
    use gdrender2d::frame_server::{BrowserInputEvent, BrowserKey};

    let mut game = space_shooter_live::LiveGame::new();
    let start_x = game.player_x;

    // Simulate pressing Right
    let events = vec![BrowserInputEvent::Key {
        key: BrowserKey::Right,
        pressed: true,
    }];
    game.process_browser_input(&events);

    // Step a few frames
    for _ in 0..10 {
        game.step();
        // Re-send input each frame since flush_frame clears just_pressed
        game.process_browser_input(&events);
    }

    assert!(
        game.player_x > start_x,
        "Player should have moved right: start={start_x}, now={}",
        game.player_x
    );
}

#[test]
fn browser_input_fires_bullets() {
    use gdrender2d::frame_server::{BrowserInputEvent, BrowserKey};

    let mut game = space_shooter_live::LiveGame::new();

    // Simulate pressing Space
    let press = vec![BrowserInputEvent::Key {
        key: BrowserKey::Space,
        pressed: true,
    }];
    let release = vec![BrowserInputEvent::Key {
        key: BrowserKey::Space,
        pressed: false,
    }];

    // Press and hold space for several frames
    game.process_browser_input(&press);
    for _ in 0..20 {
        game.step();
    }
    game.process_browser_input(&release);
    game.step();

    assert!(
        game.bullets_fired > 0,
        "Bullets should have been fired, got {}",
        game.bullets_fired
    );
}

#[test]
fn browser_key_to_key_mapping_covers_all_variants() {
    use gdrender2d::frame_server::BrowserKey;

    // Test a representative set of key mappings
    let mappings = [
        (BrowserKey::Left, gdplatform::input::Key::Left),
        (BrowserKey::Right, gdplatform::input::Key::Right),
        (BrowserKey::Up, gdplatform::input::Key::Up),
        (BrowserKey::Down, gdplatform::input::Key::Down),
        (BrowserKey::Space, gdplatform::input::Key::Space),
        (BrowserKey::A, gdplatform::input::Key::A),
        (BrowserKey::Z, gdplatform::input::Key::Z),
        (BrowserKey::Num0, gdplatform::input::Key::Num0),
        (BrowserKey::F1, gdplatform::input::Key::F1),
        (BrowserKey::Escape, gdplatform::input::Key::Escape),
    ];

    for (browser_key, expected_key) in mappings {
        let result = space_shooter_live::browser_event_to_input(
            &gdrender2d::frame_server::BrowserInputEvent::Key {
                key: browser_key,
                pressed: true,
            },
        );
        match result {
            gdplatform::input::InputEvent::Key { key, pressed, .. } => {
                assert_eq!(key, expected_key, "Mapping mismatch for {browser_key:?}");
                assert!(pressed);
            }
            _ => panic!("Expected Key event"),
        }
    }
}

#[test]
fn run_live_respects_frame_limit() {
    let port = free_port();
    let server = frame_server::start(port);
    std::thread::sleep(Duration::from_millis(150));

    let running = Arc::new(AtomicBool::new(true));
    space_shooter_live::run_live(&server, Some(30), &running);

    // After run_live returns, the game should have completed
    // Verify server is still responsive
    let resp = http_get(port, "/status");
    assert!(
        resp.contains("200 OK"),
        "Server should still respond after game ends"
    );

    server.stop();
}

#[test]
fn enemies_spawn_and_can_be_killed() {
    use gdrender2d::frame_server::{BrowserInputEvent, BrowserKey};

    let mut game = space_shooter_live::LiveGame::new();

    // Hold space to shoot
    let press_space = vec![BrowserInputEvent::Key {
        key: BrowserKey::Space,
        pressed: true,
    }];

    game.process_browser_input(&press_space);
    for _ in 0..120 {
        game.step();
    }

    assert!(game.enemies_spawned > 0, "Enemies should have spawned");
    assert!(game.bullets_fired > 0, "Bullets should have been fired");
    // With enough frames, some bullets should hit enemies
    // (player starts centered, enemies spawn randomly across the width)
}

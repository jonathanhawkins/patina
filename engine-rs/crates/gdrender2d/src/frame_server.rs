//! HTTP frame server for live Chrome preview.
//!
//! Serves the latest rendered frame as BMP over HTTP so a browser tab
//! can display a live preview of the engine output. Input events from
//! the browser are queued as [`BrowserInputEvent`] values.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::export::encode_bmp;
use crate::renderer::FrameBuffer;

// ---------------------------------------------------------------------------
// Input types (local to avoid cyclic dep with gdplatform)
// ---------------------------------------------------------------------------

/// Keyboard key identifiers from the browser.
///
/// Mirrors the subset of `gdplatform::input::Key` that browsers can produce.
/// Consumers can convert to `gdplatform::input::Key` at the integration layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrowserKey {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    Space,
    Enter,
    Escape,
    Tab,
    Shift,
    Ctrl,
    Alt,
    Up,
    Down,
    Left,
    Right,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    CapsLock,
    Comma,
    Period,
    Slash,
    Semicolon,
    Quote,
    BracketLeft,
    BracketRight,
    Backslash,
    Minus,
    Equal,
}

/// An input event received from the browser.
#[derive(Debug, Clone, PartialEq)]
pub enum BrowserInputEvent {
    /// A keyboard key event.
    Key {
        /// The key that was pressed or released.
        key: BrowserKey,
        /// `true` if pressed, `false` if released.
        pressed: bool,
    },
}

/// Status information about the current frame.
#[derive(Debug, Clone, Default)]
pub struct FrameStatus {
    /// Total frames rendered.
    pub frame_count: u64,
    /// Current frames per second.
    pub fps: f64,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
}

/// Shared state between the HTTP server thread and the engine.
#[derive(Debug)]
struct SharedState {
    frame: Mutex<Vec<u8>>,
    status: Mutex<FrameStatus>,
    input_queue: Mutex<Vec<BrowserInputEvent>>,
    running: AtomicBool,
}

/// Handle returned by [`start`], used to interact with the running server.
#[derive(Debug)]
pub struct FrameServerHandle {
    state: Arc<SharedState>,
    thread: Option<JoinHandle<()>>,
}

impl FrameServerHandle {
    /// Encodes the framebuffer as BMP and stores it for the next `/frame.bmp` request.
    pub fn update_frame(&self, fb: &FrameBuffer) {
        let bmp = encode_bmp(fb);
        let mut frame = self.state.frame.lock().unwrap();
        *frame = bmp;
        // Also update width/height in status.
        let mut status = self.state.status.lock().unwrap();
        status.width = fb.width;
        status.height = fb.height;
    }

    /// Updates the status info served at `/status`.
    pub fn update_status(&self, frame_count: u64, fps: f64) {
        let mut status = self.state.status.lock().unwrap();
        status.frame_count = frame_count;
        status.fps = fps;
    }

    /// Takes all pending input events from the browser.
    pub fn drain_input(&self) -> Vec<BrowserInputEvent> {
        let mut queue = self.state.input_queue.lock().unwrap();
        std::mem::take(&mut *queue)
    }

    /// Signals the server to stop and waits for the thread to finish.
    pub fn stop(mut self) {
        self.state.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

/// Starts an HTTP frame server on the given port.
///
/// Returns a [`FrameServerHandle`] for pushing frames and reading input.
pub fn start(port: u16) -> FrameServerHandle {
    let state = Arc::new(SharedState {
        frame: Mutex::new(Vec::new()),
        status: Mutex::new(FrameStatus::default()),
        input_queue: Mutex::new(Vec::new()),
        running: AtomicBool::new(true),
    });

    let state_clone = Arc::clone(&state);
    let thread = thread::spawn(move || {
        run_server(state_clone, port);
    });

    FrameServerHandle {
        state,
        thread: Some(thread),
    }
}

fn run_server(state: Arc<SharedState>, port: u16) {
    let listener = match TcpListener::bind(format!("0.0.0.0:{port}")) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Failed to bind frame server on port {port}: {e}");
            return;
        }
    };
    listener
        .set_nonblocking(true)
        .expect("failed to set non-blocking");

    while state.running.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                handle_connection(&state, stream);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                tracing::warn!("Accept error: {e}");
            }
        }
    }
}

fn handle_connection(state: &SharedState, mut stream: TcpStream) {
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();

    let mut buf = [0u8; 4096];
    let n = match stream.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return,
    };
    let request = String::from_utf8_lossy(&buf[..n]);

    let first_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return;
    }
    let method = parts[0];
    let path = parts[1].split('?').next().unwrap_or(parts[1]);

    match (method, path) {
        ("GET", "/") => serve_html(&mut stream),
        ("GET", "/frame.bmp") => serve_frame(state, &mut stream),
        ("GET", "/status") => serve_status(state, &mut stream),
        ("POST", "/input") => handle_input(state, &request, &mut stream),
        ("OPTIONS", _) => serve_cors_preflight(&mut stream),
        _ => serve_404(&mut stream),
    }
}

const HTML_PAGE: &str = r#"<html><head><title>Patina Engine Viewer</title>
<style>body{background:#111;color:#eee;font-family:monospace;display:flex;flex-direction:column;align-items:center}
img{border:1px solid #333;image-rendering:pixelated}
#info{margin:10px;color:#d4a574}</style></head>
<body>
<h2>Patina Engine Live Preview</h2>
<div id="info">Loading...</div>
<img id="frame" />
<script>
setInterval(()=>{
  document.getElementById('frame').src='/frame.bmp?t='+Date.now();
  fetch('/status').then(r=>r.json()).then(d=>{
    document.getElementById('info').textContent=
      'Frame: '+d.frame_count+' | FPS: '+d.fps.toFixed(1)+' | '+d.width+'x'+d.height;
  });
}, 100);
document.addEventListener('keydown', e=>{
  fetch('/input',{method:'POST',body:JSON.stringify({type:'key',key:e.key,pressed:true})});
});
document.addEventListener('keyup', e=>{
  fetch('/input',{method:'POST',body:JSON.stringify({type:'key',key:e.key,pressed:false})});
});
</script></body></html>"#;

fn serve_html(stream: &mut TcpStream) {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
        HTML_PAGE.len(),
        HTML_PAGE
    );
    let _ = stream.write_all(response.as_bytes());
}

fn serve_frame(state: &SharedState, stream: &mut TcpStream) {
    let frame = state.frame.lock().unwrap().clone();
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: image/bmp\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        frame.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(&frame);
}

fn serve_status(state: &SharedState, stream: &mut TcpStream) {
    let status = state.status.lock().unwrap().clone();
    let json = format!(
        r#"{{"frame_count":{},"fps":{},"width":{},"height":{}}}"#,
        status.frame_count, status.fps, status.width, status.height
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
        json.len(),
        json
    );
    let _ = stream.write_all(response.as_bytes());
}

fn handle_input(state: &SharedState, request: &str, stream: &mut TcpStream) {
    // Find the body after the blank line.
    if let Some(body) = request.split("\r\n\r\n").nth(1) {
        if let Some(event) = parse_input_json(body) {
            state.input_queue.lock().unwrap().push(event);
        }
    }
    let response = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n";
    let _ = stream.write_all(response.as_bytes());
}

fn serve_cors_preflight(stream: &mut TcpStream) {
    let response = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n\r\n";
    let _ = stream.write_all(response.as_bytes());
}

fn serve_404(stream: &mut TcpStream) {
    let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let _ = stream.write_all(response.as_bytes());
}

// ---------------------------------------------------------------------------
// Input JSON parsing (minimal, no serde)
// ---------------------------------------------------------------------------

/// Parses a minimal JSON input event from the browser.
///
/// Expected format: `{"type":"key","key":"ArrowLeft","pressed":true}`
fn parse_input_json(json: &str) -> Option<BrowserInputEvent> {
    let json = json.trim();

    // Extract a value for a given JSON key. Searches for `"key":` to avoid
    // ambiguity (e.g. `"type":"key"` vs the field named `"key"`).
    let get_value = |field: &str| -> Option<&str> {
        let pattern = format!("\"{field}\":");
        // Use rfind for "key" to skip `"type":"key"` which contains the substring.
        let idx = if field == "key" {
            json.rfind(&pattern)?
        } else {
            json.find(&pattern)?
        };
        let after = json[idx + pattern.len()..].trim_start();
        if after.starts_with('"') {
            let start = 1;
            let end = after[start..].find('"')?;
            Some(&after[start..start + end])
        } else {
            let end = after.find([',', '}', ' '])?;
            Some(&after[..end])
        }
    };

    let event_type = get_value("type")?;
    match event_type {
        "key" => {
            let key_name = get_value("key")?;
            let pressed = get_value("pressed")? == "true";
            let key = map_browser_key(key_name)?;
            Some(BrowserInputEvent::Key { key, pressed })
        }
        _ => None,
    }
}

/// Maps browser `KeyboardEvent.key` names to [`BrowserKey`].
pub fn map_browser_key(name: &str) -> Option<BrowserKey> {
    match name {
        "ArrowUp" => Some(BrowserKey::Up),
        "ArrowDown" => Some(BrowserKey::Down),
        "ArrowLeft" => Some(BrowserKey::Left),
        "ArrowRight" => Some(BrowserKey::Right),
        " " | "Space" => Some(BrowserKey::Space),
        "Enter" => Some(BrowserKey::Enter),
        "Escape" => Some(BrowserKey::Escape),
        "Tab" => Some(BrowserKey::Tab),
        "Shift" => Some(BrowserKey::Shift),
        "Control" => Some(BrowserKey::Ctrl),
        "Alt" => Some(BrowserKey::Alt),
        "Backspace" => Some(BrowserKey::Backspace),
        "Delete" => Some(BrowserKey::Delete),
        "Insert" => Some(BrowserKey::Insert),
        "Home" => Some(BrowserKey::Home),
        "End" => Some(BrowserKey::End),
        "PageUp" => Some(BrowserKey::PageUp),
        "PageDown" => Some(BrowserKey::PageDown),
        "CapsLock" => Some(BrowserKey::CapsLock),
        "F1" => Some(BrowserKey::F1),
        "F2" => Some(BrowserKey::F2),
        "F3" => Some(BrowserKey::F3),
        "F4" => Some(BrowserKey::F4),
        "F5" => Some(BrowserKey::F5),
        "F6" => Some(BrowserKey::F6),
        "F7" => Some(BrowserKey::F7),
        "F8" => Some(BrowserKey::F8),
        "F9" => Some(BrowserKey::F9),
        "F10" => Some(BrowserKey::F10),
        "F11" => Some(BrowserKey::F11),
        "F12" => Some(BrowserKey::F12),
        "," => Some(BrowserKey::Comma),
        "." => Some(BrowserKey::Period),
        "/" => Some(BrowserKey::Slash),
        ";" => Some(BrowserKey::Semicolon),
        "'" => Some(BrowserKey::Quote),
        "[" => Some(BrowserKey::BracketLeft),
        "]" => Some(BrowserKey::BracketRight),
        "\\" => Some(BrowserKey::Backslash),
        "-" => Some(BrowserKey::Minus),
        "=" => Some(BrowserKey::Equal),
        "0" => Some(BrowserKey::Num0),
        "1" => Some(BrowserKey::Num1),
        "2" => Some(BrowserKey::Num2),
        "3" => Some(BrowserKey::Num3),
        "4" => Some(BrowserKey::Num4),
        "5" => Some(BrowserKey::Num5),
        "6" => Some(BrowserKey::Num6),
        "7" => Some(BrowserKey::Num7),
        "8" => Some(BrowserKey::Num8),
        "9" => Some(BrowserKey::Num9),
        "a" | "A" => Some(BrowserKey::A),
        "b" | "B" => Some(BrowserKey::B),
        "c" | "C" => Some(BrowserKey::C),
        "d" | "D" => Some(BrowserKey::D),
        "e" | "E" => Some(BrowserKey::E),
        "f" | "F" => Some(BrowserKey::F),
        "g" | "G" => Some(BrowserKey::G),
        "h" | "H" => Some(BrowserKey::H),
        "i" | "I" => Some(BrowserKey::I),
        "j" | "J" => Some(BrowserKey::J),
        "k" | "K" => Some(BrowserKey::K),
        "l" | "L" => Some(BrowserKey::L),
        "m" | "M" => Some(BrowserKey::M),
        "n" | "N" => Some(BrowserKey::N),
        "o" | "O" => Some(BrowserKey::O),
        "p" | "P" => Some(BrowserKey::P),
        "q" | "Q" => Some(BrowserKey::Q),
        "r" | "R" => Some(BrowserKey::R),
        "s" | "S" => Some(BrowserKey::S),
        "t" | "T" => Some(BrowserKey::T),
        "u" | "U" => Some(BrowserKey::U),
        "v" | "V" => Some(BrowserKey::V),
        "w" | "W" => Some(BrowserKey::W),
        "x" | "X" => Some(BrowserKey::X),
        "y" | "Y" => Some(BrowserKey::Y),
        "z" | "Z" => Some(BrowserKey::Z),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Color;
    use std::net::TcpStream;

    fn free_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    fn connect_with_retry(port: u16) -> TcpStream {
        for attempt in 0..20 {
            match TcpStream::connect(format!("127.0.0.1:{port}")) {
                Ok(stream) => return stream,
                Err(_) if attempt < 19 => thread::sleep(Duration::from_millis(50)),
                Err(e) => panic!("failed to connect after retries: {e}"),
            }
        }
        unreachable!()
    }

    fn http_request(port: u16, request: &str) -> String {
        let mut stream = connect_with_retry(port);
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = Vec::new();
        let _ = stream.read_to_end(&mut response);
        String::from_utf8_lossy(&response).to_string()
    }

    fn http_request_raw(port: u16, request: &str) -> Vec<u8> {
        let mut stream = connect_with_retry(port);
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = Vec::new();
        let _ = stream.read_to_end(&mut response);
        response
    }

    #[test]
    fn server_starts_and_stops() {
        let port = free_port();
        let handle = start(port);
        assert!(handle.state.running.load(Ordering::SeqCst));
        handle.stop();
    }

    #[test]
    fn serves_html_on_root() {
        let port = free_port();
        let handle = start(port);
        thread::sleep(Duration::from_millis(150));

        let resp = http_request(port, "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("text/html"));
        assert!(resp.contains("Patina Engine Live Preview"));
        assert!(resp.contains("Patina Engine Viewer"));

        handle.stop();
    }

    #[test]
    fn serves_bmp_on_frame() {
        let port = free_port();
        let handle = start(port);

        let fb = FrameBuffer::new(4, 4, Color::rgb(1.0, 0.0, 0.0));
        handle.update_frame(&fb);

        thread::sleep(Duration::from_millis(150));

        let resp = http_request_raw(port, "GET /frame.bmp HTTP/1.1\r\nHost: localhost\r\n\r\n");
        let resp_str = String::from_utf8_lossy(&resp);
        assert!(resp_str.contains("HTTP/1.1 200 OK"));
        assert!(resp_str.contains("image/bmp"));

        let bm_pos = resp.windows(2).position(|w| w == b"BM");
        assert!(bm_pos.is_some(), "BMP data should contain BM header");

        handle.stop();
    }

    #[test]
    fn serves_json_on_status() {
        let port = free_port();
        let handle = start(port);

        handle.update_status(42, 59.9);
        {
            let mut status = handle.state.status.lock().unwrap();
            status.width = 320;
            status.height = 240;
        }

        thread::sleep(Duration::from_millis(150));

        let resp = http_request(port, "GET /status HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert!(resp.contains("HTTP/1.1 200 OK"));
        assert!(resp.contains("application/json"));
        assert!(resp.contains("\"frame_count\":42"));
        assert!(resp.contains("\"width\":320"));
        assert!(resp.contains("\"height\":240"));

        handle.stop();
    }

    #[test]
    fn input_queue_receives_key_event() {
        let port = free_port();
        let handle = start(port);
        thread::sleep(Duration::from_millis(200));

        let body = r#"{"type":"key","key":"ArrowLeft","pressed":true}"#;
        let req = format!(
            "POST /input HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let resp = http_request(port, &req);
        assert!(resp.contains("200 OK"));

        // Give server time to process.
        thread::sleep(Duration::from_millis(50));

        let events = handle.drain_input();
        assert_eq!(events.len(), 1);
        match &events[0] {
            BrowserInputEvent::Key { key, pressed } => {
                assert_eq!(*key, BrowserKey::Left);
                assert!(*pressed);
            }
        }

        handle.stop();
    }

    #[test]
    fn drain_input_clears_queue() {
        let port = free_port();
        let handle = start(port);
        thread::sleep(Duration::from_millis(200));

        let body = r#"{"type":"key","key":"a","pressed":true}"#;
        let req = format!(
            "POST /input HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = http_request(port, &req);
        // Allow time for server to process on slower CI (macOS)
        let mut events = Vec::new();
        for _ in 0..5 {
            thread::sleep(Duration::from_millis(100));
            events = handle.drain_input();
            if !events.is_empty() {
                break;
            }
        }
        assert_eq!(events.len(), 1);

        let events2 = handle.drain_input();
        assert!(events2.is_empty());

        handle.stop();
    }

    #[test]
    fn unknown_path_returns_404() {
        let port = free_port();
        let handle = start(port);
        thread::sleep(Duration::from_millis(150));

        let resp = http_request(port, "GET /nonexistent HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert!(resp.contains("404 Not Found"));

        handle.stop();
    }

    #[test]
    fn cors_headers_present() {
        let port = free_port();
        let handle = start(port);
        thread::sleep(Duration::from_millis(150));

        let resp = http_request(port, "GET /status HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert!(resp.contains("Access-Control-Allow-Origin: *"));

        handle.stop();
    }

    #[test]
    fn options_preflight_returns_204() {
        let port = free_port();
        let handle = start(port);
        thread::sleep(Duration::from_millis(150));

        let resp = http_request(port, "OPTIONS /input HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert!(resp.contains("204 No Content"));
        assert!(resp.contains("Access-Control-Allow-Origin: *"));
        assert!(resp.contains("Access-Control-Allow-Methods"));

        handle.stop();
    }

    #[test]
    fn bmp_encoding_correct_header() {
        let fb = FrameBuffer::new(2, 2, Color::BLACK);
        let bmp = encode_bmp(&fb);

        assert_eq!(&bmp[0..2], b"BM");
        // 32bpp BGRA, 2x2: pixel_data = 2*2*4 = 16, file_size = 54 + 16 = 70
        let file_size = u32::from_le_bytes([bmp[2], bmp[3], bmp[4], bmp[5]]);
        assert_eq!(file_size, 70);
        let pixel_offset = u32::from_le_bytes([bmp[10], bmp[11], bmp[12], bmp[13]]);
        assert_eq!(pixel_offset, 54);
        let width = i32::from_le_bytes([bmp[18], bmp[19], bmp[20], bmp[21]]);
        assert_eq!(width, 2);
        let height = i32::from_le_bytes([bmp[22], bmp[23], bmp[24], bmp[25]]);
        assert_eq!(height, 2);
        let bpp = u16::from_le_bytes([bmp[28], bmp[29]]);
        assert_eq!(bpp, 32);
    }

    #[test]
    fn bmp_encoding_red_pixel() {
        let fb = FrameBuffer::new(1, 1, Color::rgb(1.0, 0.0, 0.0));
        let bmp = encode_bmp(&fb);

        // Pixel data at offset 54, BGRA order.
        assert_eq!(bmp[54], 0); // B
        assert_eq!(bmp[55], 0); // G
        assert_eq!(bmp[56], 255); // R
        assert_eq!(bmp[57], 255); // A
    }

    #[test]
    fn map_browser_key_arrows() {
        assert_eq!(map_browser_key("ArrowUp"), Some(BrowserKey::Up));
        assert_eq!(map_browser_key("ArrowDown"), Some(BrowserKey::Down));
        assert_eq!(map_browser_key("ArrowLeft"), Some(BrowserKey::Left));
        assert_eq!(map_browser_key("ArrowRight"), Some(BrowserKey::Right));
    }

    #[test]
    fn map_browser_key_space_variants() {
        assert_eq!(map_browser_key(" "), Some(BrowserKey::Space));
        assert_eq!(map_browser_key("Space"), Some(BrowserKey::Space));
    }

    #[test]
    fn map_browser_key_letters() {
        assert_eq!(map_browser_key("a"), Some(BrowserKey::A));
        assert_eq!(map_browser_key("A"), Some(BrowserKey::A));
        assert_eq!(map_browser_key("z"), Some(BrowserKey::Z));
        assert_eq!(map_browser_key("Z"), Some(BrowserKey::Z));
    }

    #[test]
    fn parse_input_json_valid() {
        let json = r#"{"type":"key","key":"w","pressed":true}"#;
        let event = parse_input_json(json).unwrap();
        match event {
            BrowserInputEvent::Key { key, pressed } => {
                assert_eq!(key, BrowserKey::W);
                assert!(pressed);
            }
        }
    }

    #[test]
    fn parse_input_json_release() {
        let json = r#"{"type":"key","key":"Escape","pressed":false}"#;
        let event = parse_input_json(json).unwrap();
        match event {
            BrowserInputEvent::Key { key, pressed } => {
                assert_eq!(key, BrowserKey::Escape);
                assert!(!pressed);
            }
        }
    }

    #[test]
    fn parse_input_json_unknown_type_returns_none() {
        let json = r#"{"type":"mouse","x":10,"y":20}"#;
        assert!(parse_input_json(json).is_none());
    }

    #[test]
    fn update_status_sets_values() {
        let port = free_port();
        let handle = start(port);

        handle.update_status(100, 60.0);
        {
            let status = handle.state.status.lock().unwrap();
            assert_eq!(status.frame_count, 100);
            assert_eq!(status.fps, 60.0);
        }

        handle.stop();
    }
}

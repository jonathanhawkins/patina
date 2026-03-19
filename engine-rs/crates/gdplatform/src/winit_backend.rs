// pat-oa3: WinitPlatform implements PlatformBackend. run_with_main_loop()
// creates a WinitPlatform and calls MainLoop::run_frame() from inside the
// winit callback. The old run_with_winit() is preserved for compatibility.
// See PLATFORM_ROADMAP.md.

//! Winit + softbuffer windowing backend.
//!
//! Gated behind the `windowed` feature. Provides a real OS window via
//! [`winit`] and CPU-blitted framebuffer presentation via [`softbuffer`].
//!
//! The primary entry point is [`run_with_main_loop`], which creates a
//! [`WinitPlatform`] (implementing [`PlatformBackend`]) and drives a
//! [`MainLoop`] from the winit event callback.

use crate::backend::PlatformBackend;
use crate::input::{InputState, Key};
use crate::window::{WindowConfig, WindowEvent, WindowId, WindowManager};
use gdcore::math::Color;
use gdrender2d::FrameBuffer;

use std::collections::VecDeque;
use std::num::NonZeroU32;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent as WinitWindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::Window;

// ---------------------------------------------------------------------------
// Key mapping
// ---------------------------------------------------------------------------

/// Maps a winit [`KeyCode`] to our engine [`Key`] enum.
///
/// Returns `None` for unmapped keys.
pub fn map_winit_key(code: KeyCode) -> Option<Key> {
    Some(match code {
        KeyCode::KeyA => Key::A,
        KeyCode::KeyB => Key::B,
        KeyCode::KeyC => Key::C,
        KeyCode::KeyD => Key::D,
        KeyCode::KeyE => Key::E,
        KeyCode::KeyF => Key::F,
        KeyCode::KeyG => Key::G,
        KeyCode::KeyH => Key::H,
        KeyCode::KeyI => Key::I,
        KeyCode::KeyJ => Key::J,
        KeyCode::KeyK => Key::K,
        KeyCode::KeyL => Key::L,
        KeyCode::KeyM => Key::M,
        KeyCode::KeyN => Key::N,
        KeyCode::KeyO => Key::O,
        KeyCode::KeyP => Key::P,
        KeyCode::KeyQ => Key::Q,
        KeyCode::KeyR => Key::R,
        KeyCode::KeyS => Key::S,
        KeyCode::KeyT => Key::T,
        KeyCode::KeyU => Key::U,
        KeyCode::KeyV => Key::V,
        KeyCode::KeyW => Key::W,
        KeyCode::KeyX => Key::X,
        KeyCode::KeyY => Key::Y,
        KeyCode::KeyZ => Key::Z,
        KeyCode::Digit0 => Key::Num0,
        KeyCode::Digit1 => Key::Num1,
        KeyCode::Digit2 => Key::Num2,
        KeyCode::Digit3 => Key::Num3,
        KeyCode::Digit4 => Key::Num4,
        KeyCode::Digit5 => Key::Num5,
        KeyCode::Digit6 => Key::Num6,
        KeyCode::Digit7 => Key::Num7,
        KeyCode::Digit8 => Key::Num8,
        KeyCode::Digit9 => Key::Num9,
        KeyCode::Space => Key::Space,
        KeyCode::Enter => Key::Enter,
        KeyCode::Escape => Key::Escape,
        KeyCode::Tab => Key::Tab,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => Key::Shift,
        KeyCode::ControlLeft | KeyCode::ControlRight => Key::Ctrl,
        KeyCode::AltLeft | KeyCode::AltRight => Key::Alt,
        KeyCode::ArrowUp => Key::Up,
        KeyCode::ArrowDown => Key::Down,
        KeyCode::ArrowLeft => Key::Left,
        KeyCode::ArrowRight => Key::Right,
        KeyCode::F1 => Key::F1,
        KeyCode::F2 => Key::F2,
        KeyCode::F3 => Key::F3,
        KeyCode::F4 => Key::F4,
        KeyCode::F5 => Key::F5,
        KeyCode::F6 => Key::F6,
        KeyCode::F7 => Key::F7,
        KeyCode::F8 => Key::F8,
        KeyCode::F9 => Key::F9,
        KeyCode::F10 => Key::F10,
        KeyCode::F11 => Key::F11,
        KeyCode::F12 => Key::F12,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Pixel conversion
// ---------------------------------------------------------------------------

/// Converts an engine [`Color`] (f32 RGBA, 0.0–1.0) to a `u32` in
/// `0xAARRGGBB` format suitable for softbuffer.
///
/// Components are clamped to `[0.0, 1.0]` before conversion.
pub fn color_to_u32(color: Color) -> u32 {
    let r = (color.r.clamp(0.0, 1.0) * 255.0) as u32;
    let g = (color.g.clamp(0.0, 1.0) * 255.0) as u32;
    let b = (color.b.clamp(0.0, 1.0) * 255.0) as u32;
    let a = (color.a.clamp(0.0, 1.0) * 255.0) as u32;
    (a << 24) | (r << 16) | (g << 8) | b
}

/// Writes all pixels from a [`FrameBuffer`] into a softbuffer [`softbuffer::Surface`] buffer.
pub fn blit_framebuffer(
    surface: &mut softbuffer::Surface<Arc<Window>, Arc<Window>>,
    fb: &FrameBuffer,
) {
    let width = NonZeroU32::new(fb.width).expect("framebuffer width must be > 0");
    let height = NonZeroU32::new(fb.height).expect("framebuffer height must be > 0");

    surface
        .resize(width, height)
        .expect("failed to resize softbuffer surface");

    let mut buf = surface.buffer_mut().expect("failed to get surface buffer");
    for (i, pixel) in fb.pixels.iter().enumerate() {
        buf[i] = color_to_u32(*pixel);
    }
    buf.present().expect("failed to present buffer");
}

// ---------------------------------------------------------------------------
// WinitWindow
// ---------------------------------------------------------------------------

/// A real OS window backend using winit + softbuffer.
pub struct WinitWindow {
    window: Arc<Window>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    event_queue: VecDeque<WindowEvent>,
    open: bool,
}

impl WinitWindow {
    /// Creates a new `WinitWindow` from an already-created winit window.
    pub fn new(window: Arc<Window>) -> Self {
        let context =
            softbuffer::Context::new(window.clone()).expect("failed to create softbuffer context");
        let surface = softbuffer::Surface::new(&context, window.clone())
            .expect("failed to create softbuffer surface");

        Self {
            window,
            surface,
            event_queue: VecDeque::new(),
            open: true,
        }
    }

    /// Returns a reference to the underlying winit window.
    pub fn window(&self) -> &Window {
        &self.window
    }

    /// Returns a mutable reference to the softbuffer surface.
    pub fn surface_mut(&mut self) -> &mut softbuffer::Surface<Arc<Window>, Arc<Window>> {
        &mut self.surface
    }

    /// Pushes a converted window event into the internal queue.
    pub fn push_event(&mut self, event: WindowEvent) {
        self.event_queue.push_back(event);
    }
}

impl WindowManager for WinitWindow {
    fn create_window(&mut self, config: &WindowConfig) -> WindowId {
        // Window already exists; just apply the config.
        self.set_title(WindowId(0), &config.title);
        self.set_size(WindowId(0), config.width, config.height);
        self.set_fullscreen(WindowId(0), config.fullscreen);
        WindowId(0)
    }

    fn set_title(&mut self, _id: WindowId, title: &str) {
        self.window.set_title(title);
    }

    fn set_size(&mut self, _id: WindowId, width: u32, height: u32) {
        let _ = self
            .window
            .request_inner_size(winit::dpi::LogicalSize::new(width, height));
    }

    fn set_fullscreen(&mut self, _id: WindowId, fullscreen: bool) {
        if fullscreen {
            self.window
                .set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
        } else {
            self.window.set_fullscreen(None);
        }
    }

    fn close(&mut self, _id: WindowId) {
        self.open = false;
    }

    fn poll_events(&mut self) -> Vec<WindowEvent> {
        self.event_queue.drain(..).collect()
    }

    fn is_open(&self, _id: WindowId) -> bool {
        self.open
    }
}

// ---------------------------------------------------------------------------
// WinitPlatform — PlatformBackend implementation
// ---------------------------------------------------------------------------

/// Winit-backed platform that implements [`PlatformBackend`].
///
/// Created internally by [`run_with_main_loop`]. Buffers window events
/// between winit callbacks so that [`MainLoop::run_frame`] can drain them
/// each frame via [`poll_events`](PlatformBackend::poll_events).
pub struct WinitPlatform {
    winit_window: WinitWindow,
    quit: bool,
    last_frame: std::time::Instant,
}

impl WinitPlatform {
    /// Creates a new `WinitPlatform` wrapping an existing `WinitWindow`.
    pub fn new(winit_window: WinitWindow) -> Self {
        Self {
            winit_window,
            quit: false,
            last_frame: std::time::Instant::now(),
        }
    }

    /// Returns a reference to the underlying [`WinitWindow`].
    pub fn winit_window(&self) -> &WinitWindow {
        &self.winit_window
    }

    /// Returns a mutable reference to the underlying [`WinitWindow`].
    pub fn winit_window_mut(&mut self) -> &mut WinitWindow {
        &mut self.winit_window
    }

    /// Pushes a window event into the buffer (called from the winit callback).
    pub fn push_event(&mut self, event: WindowEvent) {
        self.winit_window.push_event(event);
    }

    /// Signals that the platform should quit.
    pub fn request_quit(&mut self) {
        self.quit = true;
        self.winit_window.open = false;
    }

    /// Returns the delta time since the last frame and resets the timer.
    pub fn take_delta(&mut self) -> f64 {
        let now = std::time::Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f64();
        self.last_frame = now;
        dt
    }

    /// Presents a framebuffer to the window surface.
    pub fn present(&mut self, fb: &FrameBuffer) {
        blit_framebuffer(&mut self.winit_window.surface, fb);
    }
}

impl PlatformBackend for WinitPlatform {
    fn poll_events(&mut self) -> Vec<WindowEvent> {
        self.winit_window.event_queue.drain(..).collect()
    }

    fn should_quit(&self) -> bool {
        self.quit || !self.winit_window.open
    }

    fn window_size(&self) -> (u32, u32) {
        let size = self.winit_window.window.inner_size();
        (size.width, size.height)
    }

    fn end_frame(&mut self) {
        self.winit_window.window.request_redraw();
    }
}

// ---------------------------------------------------------------------------
// run_with_winit (legacy)
// ---------------------------------------------------------------------------

/// Application handler that drives the game loop.
struct WinitApp<F>
where
    F: FnMut(&mut InputState, f64) -> Option<FrameBuffer>,
{
    config: WindowConfig,
    game_loop: F,
    input_state: InputState,
    winit_window: Option<WinitWindow>,
    last_frame: std::time::Instant,
}

impl<F> ApplicationHandler for WinitApp<F>
where
    F: FnMut(&mut InputState, f64) -> Option<FrameBuffer>,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.winit_window.is_some() {
            return;
        }

        let attrs = Window::default_attributes()
            .with_title(self.config.title.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.config.width,
                self.config.height,
            ))
            .with_resizable(self.config.resizable);

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );
        self.winit_window = Some(WinitWindow::new(window));
        self.last_frame = std::time::Instant::now();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WinitWindowEvent,
    ) {
        let Some(ww) = self.winit_window.as_mut() else {
            return;
        };

        match &event {
            WinitWindowEvent::CloseRequested => {
                ww.push_event(WindowEvent::CloseRequested);
                ww.open = false;
                event_loop.exit();
                return;
            }
            WinitWindowEvent::Resized(size) => {
                ww.push_event(WindowEvent::Resized {
                    width: size.width,
                    height: size.height,
                });
            }
            WinitWindowEvent::Focused(focused) => {
                if *focused {
                    ww.push_event(WindowEvent::FocusGained);
                } else {
                    ww.push_event(WindowEvent::FocusLost);
                }
            }
            WinitWindowEvent::KeyboardInput { event: ke, .. } => {
                if let PhysicalKey::Code(code) = ke.physical_key {
                    if let Some(key) = map_winit_key(code) {
                        let pressed = ke.state == ElementState::Pressed;
                        ww.push_event(WindowEvent::KeyInput {
                            key,
                            pressed,
                            shift: false, // simplified — full modifier tracking is future work
                            ctrl: false,
                            alt: false,
                        });
                    }
                }
            }
            WinitWindowEvent::RedrawRequested => {
                let now = std::time::Instant::now();
                let dt = now.duration_since(self.last_frame).as_secs_f64();
                self.last_frame = now;

                // Drain window events into InputState.
                let events = ww.poll_events();
                for we in &events {
                    if let Some(ie) = we.to_input_event() {
                        self.input_state.process_event(ie);
                    }
                }

                // Run user game loop.
                if let Some(fb) = (self.game_loop)(&mut self.input_state, dt) {
                    blit_framebuffer(&mut ww.surface, &fb);
                }

                self.input_state.flush_frame();
                ww.window.request_redraw();
            }
            _ => {}
        }
    }
}

/// Creates a winit event loop, opens a window, and runs the provided game loop each frame.
///
/// The `game_loop` closure receives the current [`InputState`] and delta time (seconds),
/// and optionally returns a [`FrameBuffer`] to present.
pub fn run_with_winit(
    config: WindowConfig,
    game_loop: impl FnMut(&mut InputState, f64) -> Option<FrameBuffer>,
) {
    let event_loop = EventLoop::new().expect("failed to create event loop");
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    let mut app = WinitApp {
        config,
        game_loop,
        input_state: InputState::new(),
        winit_window: None,
        last_frame: std::time::Instant::now(),
    };

    event_loop
        .run_app(&mut app)
        .expect("event loop exited with error");
}

// ---------------------------------------------------------------------------
// Tests — key mapping and pixel conversion only (no window creation)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Color;

    // --- Key mapping: letters ---

    #[test]
    fn key_map_letters_a_to_z() {
        let pairs = [
            (KeyCode::KeyA, Key::A),
            (KeyCode::KeyB, Key::B),
            (KeyCode::KeyC, Key::C),
            (KeyCode::KeyD, Key::D),
            (KeyCode::KeyE, Key::E),
            (KeyCode::KeyF, Key::F),
            (KeyCode::KeyG, Key::G),
            (KeyCode::KeyH, Key::H),
            (KeyCode::KeyI, Key::I),
            (KeyCode::KeyJ, Key::J),
            (KeyCode::KeyK, Key::K),
            (KeyCode::KeyL, Key::L),
            (KeyCode::KeyM, Key::M),
            (KeyCode::KeyN, Key::N),
            (KeyCode::KeyO, Key::O),
            (KeyCode::KeyP, Key::P),
            (KeyCode::KeyQ, Key::Q),
            (KeyCode::KeyR, Key::R),
            (KeyCode::KeyS, Key::S),
            (KeyCode::KeyT, Key::T),
            (KeyCode::KeyU, Key::U),
            (KeyCode::KeyV, Key::V),
            (KeyCode::KeyW, Key::W),
            (KeyCode::KeyX, Key::X),
            (KeyCode::KeyY, Key::Y),
            (KeyCode::KeyZ, Key::Z),
        ];
        for (winit_key, expected) in pairs {
            assert_eq!(
                map_winit_key(winit_key),
                Some(expected),
                "failed for {:?}",
                winit_key
            );
        }
    }

    // --- Key mapping: digits ---

    #[test]
    fn key_map_digits_0_to_9() {
        let pairs = [
            (KeyCode::Digit0, Key::Num0),
            (KeyCode::Digit1, Key::Num1),
            (KeyCode::Digit2, Key::Num2),
            (KeyCode::Digit3, Key::Num3),
            (KeyCode::Digit4, Key::Num4),
            (KeyCode::Digit5, Key::Num5),
            (KeyCode::Digit6, Key::Num6),
            (KeyCode::Digit7, Key::Num7),
            (KeyCode::Digit8, Key::Num8),
            (KeyCode::Digit9, Key::Num9),
        ];
        for (winit_key, expected) in pairs {
            assert_eq!(
                map_winit_key(winit_key),
                Some(expected),
                "failed for {:?}",
                winit_key
            );
        }
    }

    // --- Key mapping: special keys ---

    #[test]
    fn key_map_special_keys() {
        assert_eq!(map_winit_key(KeyCode::Space), Some(Key::Space));
        assert_eq!(map_winit_key(KeyCode::Enter), Some(Key::Enter));
        assert_eq!(map_winit_key(KeyCode::Escape), Some(Key::Escape));
        assert_eq!(map_winit_key(KeyCode::Tab), Some(Key::Tab));
    }

    // --- Key mapping: modifiers (left and right variants) ---

    #[test]
    fn key_map_modifiers() {
        assert_eq!(map_winit_key(KeyCode::ShiftLeft), Some(Key::Shift));
        assert_eq!(map_winit_key(KeyCode::ShiftRight), Some(Key::Shift));
        assert_eq!(map_winit_key(KeyCode::ControlLeft), Some(Key::Ctrl));
        assert_eq!(map_winit_key(KeyCode::ControlRight), Some(Key::Ctrl));
        assert_eq!(map_winit_key(KeyCode::AltLeft), Some(Key::Alt));
        assert_eq!(map_winit_key(KeyCode::AltRight), Some(Key::Alt));
    }

    // --- Key mapping: arrows ---

    #[test]
    fn key_map_arrows() {
        assert_eq!(map_winit_key(KeyCode::ArrowUp), Some(Key::Up));
        assert_eq!(map_winit_key(KeyCode::ArrowDown), Some(Key::Down));
        assert_eq!(map_winit_key(KeyCode::ArrowLeft), Some(Key::Left));
        assert_eq!(map_winit_key(KeyCode::ArrowRight), Some(Key::Right));
    }

    // --- Key mapping: function keys ---

    #[test]
    fn key_map_function_keys_f1_to_f12() {
        let pairs = [
            (KeyCode::F1, Key::F1),
            (KeyCode::F2, Key::F2),
            (KeyCode::F3, Key::F3),
            (KeyCode::F4, Key::F4),
            (KeyCode::F5, Key::F5),
            (KeyCode::F6, Key::F6),
            (KeyCode::F7, Key::F7),
            (KeyCode::F8, Key::F8),
            (KeyCode::F9, Key::F9),
            (KeyCode::F10, Key::F10),
            (KeyCode::F11, Key::F11),
            (KeyCode::F12, Key::F12),
        ];
        for (winit_key, expected) in pairs {
            assert_eq!(
                map_winit_key(winit_key),
                Some(expected),
                "failed for {:?}",
                winit_key
            );
        }
    }

    // --- Key mapping: unmapped key returns None ---

    #[test]
    fn key_map_unmapped_returns_none() {
        assert_eq!(map_winit_key(KeyCode::Pause), None);
        assert_eq!(map_winit_key(KeyCode::ScrollLock), None);
    }

    // --- Pixel conversion ---

    #[test]
    fn color_to_u32_white() {
        let c = Color::WHITE;
        assert_eq!(color_to_u32(c), 0xFFFFFFFF);
    }

    #[test]
    fn color_to_u32_black() {
        let c = Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        assert_eq!(color_to_u32(c), 0xFF000000);
    }

    #[test]
    fn color_to_u32_red() {
        let c = Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        assert_eq!(color_to_u32(c), 0xFFFF0000);
    }

    #[test]
    fn color_to_u32_transparent() {
        let c = Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };
        assert_eq!(color_to_u32(c), 0x00000000);
    }

    #[test]
    fn color_to_u32_green() {
        let c = Color {
            r: 0.0,
            g: 1.0,
            b: 0.0,
            a: 1.0,
        };
        assert_eq!(color_to_u32(c), 0xFF00FF00);
    }

    #[test]
    fn color_to_u32_blue() {
        let c = Color {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        };
        assert_eq!(color_to_u32(c), 0xFF0000FF);
    }

    #[test]
    fn color_to_u32_clamps_above_one() {
        let c = Color {
            r: 1.5,
            g: 2.0,
            b: -0.5,
            a: 1.0,
        };
        // r clamped to 1.0 → 255, g clamped to 1.0 → 255, b clamped to 0.0 → 0
        assert_eq!(color_to_u32(c), 0xFFFFFF00);
    }

    #[test]
    fn color_to_u32_half_alpha() {
        let c = Color {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.5,
        };
        // a ≈ 127 → 0x7F
        let result = color_to_u32(c);
        let alpha = (result >> 24) & 0xFF;
        // 0.5 * 255 = 127.5 → truncates to 127
        assert_eq!(alpha, 127);
        assert_eq!(result & 0x00FFFFFF, 0x00FFFFFF);
    }

    #[test]
    fn color_to_u32_clamps_negative() {
        let c = Color {
            r: -1.0,
            g: -0.1,
            b: 0.0,
            a: 1.0,
        };
        assert_eq!(color_to_u32(c), 0xFF000000);
    }
}

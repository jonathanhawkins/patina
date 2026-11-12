//! pat-z35r2.13: Windows desktop runtime smoke (window + input + audio).
//!
//! Source of truth: `docs/WINDOWS_SMOKE.md`, `COMPAT_MATRIX.md`
//! Classification: Measured (headless Windows runtime composition)
//!
//! CI environments (including `windows-latest`) cannot reliably boot a
//! windowed GUI or an audio device, so this smoke exercises the three
//! subsystems a Windows desktop runtime must compose — **window**, **input**,
//! and **audio** — through their headless/device-independent paths. Because
//! the paths are OS-agnostic, the exact same test provides real coverage when
//! it runs on the `windows-latest` CI runner (see `.github/workflows/ci.yml`).
//!
//! What each phase proves:
//! - Window: `WindowsPlatformLayer` creates a sized, foregrounded, DPI-aware
//!   window and drives a frame loop to a clean quit.
//! - Input: `WindowEvent`s from the window backend convert to `InputEvent`s
//!   and drive `InputState` (key + mouse button + motion).
//! - Audio: a WAV is synthesized, decoded, and pushed through an output stream
//!   (the `NullAudioOutput` stands in for the WASAPI/cpal device present on a
//!   real Windows machine — see `docs/WINDOWS_SMOKE.md`).

use gdcore::math::Vector2;
use gdplatform::input::{InputState, Key, MouseButton};
use gdplatform::os::Platform;
use gdplatform::platform_targets::targets_for_platform;
use gdplatform::window::{WindowConfig, WindowEvent};
use gdplatform::windows::{DpiAwarenessMode, WindowsPlatformLayer, WindowsTheme};

use gdaudio::sample::AudioOutputStream;
use gdaudio::wav::build_wav_bytes;
use gdaudio::{decode_wav, NullAudioOutput};

/// Builds a short 16-bit PCM stereo WAV (a quiet sine burst) as raw bytes,
/// mirroring an asset a Windows game would ship and play at startup.
fn synth_sine_wav(sample_rate: u32, frames: u32, freq_hz: f32) -> Vec<u8> {
    let channels: u16 = 2;
    let bits: u16 = 16;
    let mut pcm = Vec::with_capacity(frames as usize * channels as usize * 2);
    for n in 0..frames {
        let t = n as f32 / sample_rate as f32;
        // Deterministic waveform — no Date/rand needed.
        let s = (2.0 * std::f32::consts::PI * freq_hz * t).sin() * 0.5;
        let sample = (s * i16::MAX as f32) as i16;
        // Interleaved L/R, both channels identical.
        pcm.extend_from_slice(&sample.to_le_bytes());
        pcm.extend_from_slice(&sample.to_le_bytes());
    }
    // audio_format 1 = PCM.
    build_wav_bytes(sample_rate, channels, bits, 1, &pcm)
}

// ===========================================================================
// 1. Full window + input + audio smoke (the bead's acceptance test)
// ===========================================================================

#[test]
fn windows_runtime_smoke_window_input_audio() {
    // ---- Phase 1: Window creation -----------------------------------------
    let config = WindowConfig::new()
        .with_size(1280, 720)
        .with_title("Patina Windows Smoke")
        .with_vsync(true);
    let mut layer = WindowsPlatformLayer::new("PatinaWindowsSmoke", &config);

    // A modern Win32 app declares per-monitor-v2 DPI awareness before showing
    // its window; verify the layer honors that request.
    layer.set_dpi_awareness(DpiAwarenessMode::recommended());
    assert_eq!(layer.dpi_awareness(), DpiAwarenessMode::PerMonitorV2);
    assert!(
        layer.is_per_monitor_dpi(),
        "modern app is per-monitor DPI aware"
    );

    assert_eq!(layer.app_name(), "PatinaWindowsSmoke");
    assert_eq!(
        layer.window_size(),
        (1280, 720),
        "window created at requested size"
    );
    assert!(layer.is_foreground(), "new window starts in the foreground");
    assert!(
        !layer.should_quit(),
        "runtime must not want to quit before it starts"
    );

    // Windows respects the OS light/dark theme; the default is Light and the
    // runtime must be able to observe a switch (affects title-bar rendering).
    assert_eq!(layer.system_theme(), WindowsTheme::Light);
    layer.set_system_theme(WindowsTheme::Dark);
    assert_eq!(layer.system_theme(), WindowsTheme::Dark);

    // ---- Phase 2: Input -----------------------------------------------------
    let mut input = InputState::new();

    // Queue a realistic burst: press Space, click the left mouse button, and
    // move the cursor. These arrive from the Win32 message pump in a real app.
    layer.backend_mut().push_events(vec![
        WindowEvent::FocusGained,
        WindowEvent::KeyInput {
            key: Key::Space,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        },
        WindowEvent::MouseInput {
            button: MouseButton::Left,
            pressed: true,
            position: Vector2::new(640.0, 360.0),
        },
        WindowEvent::MouseMotion {
            position: Vector2::new(700.0, 400.0),
            relative: Vector2::new(60.0, 40.0),
        },
    ]);

    // Drive one frame of the loop: poll → translate → feed input → end frame.
    let events = layer.poll_window_events();
    assert!(
        events.iter().any(|e| matches!(e, WindowEvent::FocusGained)),
        "focus event surfaced from the window backend"
    );
    for event in &events {
        if let Some(input_event) = event.to_input_event() {
            input.process_event(input_event);
        }
    }
    layer.end_frame();

    assert!(
        input.is_key_pressed(Key::Space),
        "Space registered as pressed"
    );
    assert!(
        input.is_mouse_button_pressed(MouseButton::Left),
        "left mouse button registered as pressed"
    );

    // A key that was never pressed stays up — proves we route state, not noise.
    assert!(!input.is_key_pressed(Key::Escape));

    // ---- Phase 3: Audio -----------------------------------------------------
    let sample_rate = 44_100;
    let frames = 4_410; // 0.1s
    let wav = synth_sine_wav(sample_rate, frames, 440.0);
    let decoded = decode_wav(&wav).expect("decode synthesized WAV");
    assert_eq!(decoded.sample_rate, sample_rate);
    assert_eq!(decoded.channels, 2);
    assert_eq!(decoded.frame_count(), frames as usize, "all frames decoded");

    // Push the decoded buffer to an output stream. On a real Windows box this
    // is WASAPI via cpal; here NullAudioOutput proves the decode→output path
    // is wired without depending on a device being present in CI.
    let mut output = NullAudioOutput::new(sample_rate, 2);
    let written = output.write_samples(&decoded);
    assert_eq!(
        written, frames as usize,
        "output accepted every decoded frame"
    );
    assert_eq!(output.frames_written, frames as usize);

    // ---- Phase 4: Clean shutdown -------------------------------------------
    // The user closes the window; the runtime must observe the quit request.
    layer.backend_mut().push_event(WindowEvent::CloseRequested);
    let _ = layer.poll_window_events();
    assert!(
        layer.should_quit(),
        "CloseRequested drives the runtime to quit"
    );
}

// ===========================================================================
// 2. The Windows desktop target the smoke stands in for actually exists
// ===========================================================================

#[test]
fn windows_msvc_target_is_registered() {
    let targets = targets_for_platform(Platform::Windows);
    assert!(
        !targets.is_empty(),
        "Windows must have registered desktop targets"
    );
    assert!(
        targets
            .iter()
            .any(|t| t.rust_triple == "x86_64-pc-windows-msvc"),
        "the x86_64 MSVC triple the smoke targets must be registered"
    );
}

// ===========================================================================
// 3. A window resize during the loop is reflected (frame-buffer sizing)
// ===========================================================================

#[test]
fn windows_runtime_handles_resize_midloop() {
    let config = WindowConfig::new().with_size(800, 600);
    let mut layer = WindowsPlatformLayer::new("PatinaWindowsResize", &config);
    assert_eq!(layer.window_size(), (800, 600));

    layer.backend_mut().push_event(WindowEvent::Resized {
        width: 1920,
        height: 1080,
    });
    let _ = layer.poll_window_events();
    assert_eq!(
        layer.window_size(),
        (1920, 1080),
        "runtime tracks the new client size after a resize"
    );
    assert!(!layer.should_quit(), "a resize must not quit the runtime");
}

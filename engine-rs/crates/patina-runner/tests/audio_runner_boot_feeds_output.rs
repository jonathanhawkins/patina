//! Regression test for pat-nzgdu.1: the mixer output must reach a real sink
//! through patina-runner's Boot flow.
//!
//! Godot's runner opens an audio driver at startup and pulls mixed frames into
//! it every callback. Patina mixed each frame but discarded it — `attach_output`
//! on the [`AudioServerBridge`] was never called by the boot flow, and no
//! backend was ever opened, so the runner was silent.
//!
//! This test boots a project headlessly with a capture sink injected via
//! [`EngineBootstrap::set_audio_output`], adds an autoplaying
//! `AudioStreamPlayer` to the booted scene, steps the [`MainLoop`], and asserts
//! that non-silent PCM arrived at the attached sink with the expected format.

use std::sync::{Arc, Mutex};

use gdaudio::{AudioBuffer, AudioOutputStream, AudioSampleBuffer};
use gdscene::Node;
use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// Shared record of what a [`CaptureSink`] received, readable after the sink
/// has been moved into the boot flow's audio bridge.
#[derive(Default)]
struct CaptureData {
    frames: usize,
    nonsilent_samples: usize,
    sample_rate: u32,
    channels: u16,
}

#[derive(Clone, Default)]
struct CaptureHandle(Arc<Mutex<CaptureData>>);

/// A mock [`AudioOutputStream`] that records every frame written to it.
struct CaptureSink(CaptureHandle);

impl AudioOutputStream for CaptureSink {
    fn write_samples(&mut self, buffer: &AudioSampleBuffer) -> usize {
        let frames = buffer.frame_count();
        let mut data = self.0 .0.lock().unwrap();
        data.frames += frames;
        data.nonsilent_samples += buffer.samples.iter().filter(|s| s.abs() > 0.01).count();
        data.sample_rate = buffer.sample_rate;
        data.channels = buffer.channels;
        frames
    }

    fn sample_rate(&self) -> u32 {
        44_100
    }

    fn channels(&self) -> u16 {
        2
    }
}

/// A one-second stereo 440 Hz sine at half amplitude — a clearly non-silent
/// stream to hand to the autoplaying player.
fn sine_buffer() -> AudioBuffer {
    let sample_rate = 44_100u32;
    let mut samples = Vec::with_capacity(sample_rate as usize * 2);
    for i in 0..sample_rate as usize {
        let t = i as f32 / sample_rate as f32;
        let v = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
        samples.push(v); // left
        samples.push(v); // right
    }
    AudioBuffer {
        samples,
        sample_rate,
        channels: 2,
    }
}

#[test]
fn audio_runner_boot_feeds_output() {
    // Inject a capture sink and keep a handle so we can read what it received
    // after it has been moved into the boot flow's audio bridge.
    let handle = CaptureHandle::default();
    let mut boot = EngineBootstrap::new(BootConfig::headless());
    boot.set_audio_output(Box::new(CaptureSink(handle.clone())));

    // Run the full Boot flow (Core -> Servers -> ... -> Running). The Running
    // phase creates the MainLoop and, with our wiring, attaches the sink to its
    // audio bridge.
    boot.run_all().expect("boot flow should complete");

    let main_loop = boot.main_loop_mut().expect("Running phase creates a MainLoop");

    // Add an autoplaying AudioStreamPlayer to the booted scene and give it a
    // non-silent stream, mirroring a project that plays audio on start.
    let root = main_loop.tree().root_id();
    let mut player = Node::new("Sfx", "AudioStreamPlayer");
    player.set_property("autoplay", Variant::Bool(true));
    let player_id = main_loop
        .tree_mut()
        .add_child(root, player)
        .expect("add AudioStreamPlayer");
    main_loop
        .audio_bridge_mut()
        .set_stream_buffer(player_id, sine_buffer());

    // Step the loop. MainLoop::step drives audio_bridge.process, which mixes a
    // frame's worth of PCM and writes it to the attached sink.
    for _ in 0..4 {
        main_loop.step(1.0 / 60.0);
    }

    let data = handle.0.lock().unwrap();
    assert!(
        data.frames > 0,
        "capture sink should receive mixed frames through the boot flow",
    );
    assert!(
        data.nonsilent_samples > 0,
        "the autoplayed sine should arrive at the sink as non-silent PCM",
    );
    // Format (sample rate / channels) is reported through the mixed buffer.
    assert_eq!(data.sample_rate, 44_100, "sink should see the mix sample rate");
    assert_eq!(data.channels, 2, "sink should see the mix channel count");
}

/// Without an injected sink, a headless boot attaches no hardware output (like
/// Godot's headless dummy driver) — boot must still succeed and the loop runs.
#[test]
fn audio_runner_headless_boot_without_sink_is_ok() {
    let mut boot = EngineBootstrap::new(BootConfig::headless());
    boot.run_all().expect("headless boot without a sink should complete");
    let main_loop = boot.main_loop_mut().expect("Running phase creates a MainLoop");
    // Stepping with no attached output must not panic (mix falls back to the
    // bridge's last_mix buffer).
    main_loop.step(1.0 / 60.0);
}

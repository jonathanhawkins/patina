//! Attaching an audio device makes the mixer produce audio in THAT device's
//! format. Found playing Dodge the Creeps in the editor: the server kept the
//! project's 44.1 kHz while the Mac's device ran at 48 kHz, so the device drained
//! ~9% faster than the game filled it — the queue kept running dry (crackle on
//! the music) and every stream played ~9% sharp. Godot's mix rate is the rate
//! the driver actually opened (`AudioDriver::get_mix_rate`).

use gdaudio::CaptureAudioOutput;
use gdscene::audio_server_bridge::AudioServerBridge;

#[test]
fn attaching_a_48k_stereo_device_switches_the_mix_to_48k_stereo() {
    let mut bridge = AudioServerBridge::new();
    bridge.server_mut().set_output_sample_rate(44_100);
    bridge.server_mut().set_output_channels(1);

    bridge.attach_output(Box::new(CaptureAudioOutput::new(48_000, 2)));

    assert_eq!(bridge.server().output_sample_rate(), 48_000);
    assert_eq!(bridge.server().output_channels(), 2);
}

/// A device that reports its queue, recording how much it was fed.
struct QueueDevice {
    queued: std::rc::Rc<std::cell::Cell<usize>>,
    fed: std::rc::Rc<std::cell::Cell<usize>>,
}

impl gdaudio::AudioOutputStream for QueueDevice {
    fn write_samples(&mut self, buffer: &gdaudio::AudioSampleBuffer) -> usize {
        let frames = buffer.frame_count();
        self.fed.set(self.fed.get() + frames);
        self.queued.set(self.queued.get() + frames);
        frames
    }
    fn sample_rate(&self) -> u32 {
        48_000
    }
    fn channels(&self) -> u16 {
        2
    }
    fn queued_frames(&self) -> Option<usize> {
        Some(self.queued.get())
    }
}

/// The device's clock decides how much is mixed, not the game's frame delta.
/// Found measuring Dodge the Creeps' music on the real device: mixing a fixed
/// `delta * rate` per frame let the queue run dry 25 times in 12 s (each a
/// ~50 ms hole). The bridge now tops the queue up to its target every frame.
#[test]
fn the_bridge_tops_a_real_device_queue_up_to_its_target() {
    use gdscene::audio_server_bridge::DEVICE_QUEUE_TARGET_SECS;
    let queued = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let fed = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let mut bridge = AudioServerBridge::new();
    bridge.attach_output(Box::new(QueueDevice { queued: queued.clone(), fed: fed.clone() }));
    let mut tree = gdscene::scene_tree::SceneTree::new();
    let target = (DEVICE_QUEUE_TARGET_SECS * 48_000.0) as usize;

    // An empty device queue is filled to the target in one frame, whatever
    // the frame's own delta (here a 1 ms frame).
    bridge.process(&mut tree, 0.001);
    assert_eq!(fed.get(), target, "an empty queue is topped up to the target");

    // A full queue gets nothing more, however long the frame was.
    bridge.process(&mut tree, 0.1);
    assert_eq!(fed.get(), target, "a full queue is not over-fed");

    // The device plays 800 frames; the next frame replaces exactly those.
    queued.set(queued.get() - 800);
    bridge.process(&mut tree, 1.0 / 60.0);
    assert_eq!(fed.get(), target + 800, "consumption is replaced exactly");
}

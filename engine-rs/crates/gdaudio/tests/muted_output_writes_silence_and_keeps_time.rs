//! The editor's Game-view mute silences what reaches the device and nothing
//! else: the mix (every voice's playhead, the bus meters) runs on exactly as if
//! unmuted, so unmuting mid-song resumes where the song really is. The mute
//! button used to be stored and reported and read by nothing.

use gdaudio::{mix_and_write_gain, AudioBuffer, AudioServer, CaptureAudioOutput};

fn tone() -> AudioBuffer {
    let rate = 44_100u32;
    let samples = (0..rate)
        .map(|n| (n as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin() * 0.5)
        .collect();
    AudioBuffer {
        samples,
        sample_rate: rate,
        channels: 1,
    }
}

#[test]
fn a_zero_gain_write_is_silent_but_the_mix_still_advances() {
    let mut loud = AudioServer::new();
    let mut quiet = AudioServer::new();
    loud.play(tone());
    quiet.play(tone());
    let (mut a, mut b) = (CaptureAudioOutput::new(44_100, 2), CaptureAudioOutput::new(44_100, 2));

    for _ in 0..4 {
        mix_and_write_gain(&mut loud, &mut a, 735, 1.0);
        mix_and_write_gain(&mut quiet, &mut b, 735, 0.0);
    }
    assert!(a.samples.iter().any(|s| s.abs() > 0.1), "the unmuted run is audible");
    assert!(b.samples.iter().all(|s| *s == 0.0), "the muted run writes silence");
    assert_eq!(a.frames_written, b.frames_written, "the device is fed at the same rate");

    // Unmute: the next block is the SAME audio the unmuted run is playing —
    // the playhead kept moving while muted.
    let (mut a2, mut b2) = (CaptureAudioOutput::new(44_100, 2), CaptureAudioOutput::new(44_100, 2));
    mix_and_write_gain(&mut loud, &mut a2, 735, 1.0);
    mix_and_write_gain(&mut quiet, &mut b2, 735, 1.0);
    assert_eq!(a2.samples, b2.samples, "muting did not pause the song");
}

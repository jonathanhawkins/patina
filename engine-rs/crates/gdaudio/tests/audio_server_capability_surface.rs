//! pat-0q2kx.159 — AudioServer capability surface: per-bus peak meters,
//! `playback_speed_scale`, and project `audio/driver/mix_rate`.
//!
//! Godot's AudioServer meters every bus each mix (`get_bus_peak_volume_left_db`
//! drives the editor VU strips), scales all stream advancement by
//! `playback_speed_scale`, and takes its rate from `audio/driver/mix_rate`
//! (`audio_server.cpp`, default 44100).
//!
//! Patina had none of it: `mix()` computed bus buffers but recorded no peaks,
//! there was no speed knob, and `AudioServer::new()` hardcoded 44100 while
//! `project.godot` was parsed but its audio section never applied — so a 48 kHz
//! project mixed at the wrong rate and every stream came out at the wrong pitch.

use gdaudio::decode::AudioBuffer;
use gdaudio::server::AudioServer;
use gdcore::project_settings::ProjectSettings;

const SR: u32 = 44_100;

/// A constant-amplitude stereo buffer — a DC "tone" whose peak is exactly `amp`.
fn dc_buffer(amp: f32, frames: usize) -> AudioBuffer {
    AudioBuffer {
        samples: vec![amp; frames * 2],
        sample_rate: SR,
        channels: 2,
    }
}

/// **The acceptance test.**
#[test]
fn audio_server_capability_surface() {
    // -- a 0.5-amplitude voice meters at about -6 dB -------------------------
    let mut server = AudioServer::new();
    let bus = server.mixer_mut().add_bus("Music");

    let id = server.play_on_bus(dc_buffer(0.5, 4096), "Music");
    assert!(server.is_playing(id), "the buffer must start playing");

    let _ = server.mix(512);

    let left = server.get_bus_peak_volume_left_db(bus, 0);
    let right = server.get_bus_peak_volume_right_db(bus, 0);
    // 20*log10(0.5) = -6.02 dB.
    assert!(
        (left - (-6.02)).abs() < 0.5,
        "a 0.5-amplitude voice must meter near -6 dB, got {left}",
    );
    assert!(
        (right - (-6.02)).abs() < 0.5,
        "both channels must meter, got right={right}",
    );

    // -- an idle bus decays toward the floor ---------------------------------
    //
    // Stop everything, then mix silence repeatedly: the meter must fall, and
    // keep falling, rather than latching its last peak.
    server.stop(id);
    let _ = server.mix(512);
    let after_one = server.get_bus_peak_volume_left_db(bus, 0);
    for _ in 0..40 {
        let _ = server.mix(512);
    }
    let after_many = server.get_bus_peak_volume_left_db(bus, 0);

    assert!(
        after_one < left,
        "the meter must start falling once the voice stops: {left} -> {after_one}",
    );
    assert!(
        after_many < after_one - 10.0,
        "an idle bus must keep decaying toward the floor, stalled at {after_many}",
    );

    // -- playback_speed_scale advances a stream twice as fast ----------------
    //
    // Compare playback POSITION after an identical mix: at 2x the same block of
    // output consumes twice as much source.
    let mut slow = AudioServer::new();
    let a = slow.play(dc_buffer(0.25, 44_100));
    let _ = slow.mix(4410);
    let pos_1x = slow.stream_position(a).expect("position is tracked");

    let mut fast = AudioServer::new();
    assert_eq!(fast.playback_speed_scale(), 1.0, "default speed is unity");
    fast.set_playback_speed_scale(2.0);
    assert_eq!(fast.playback_speed_scale(), 2.0);
    let b = fast.play(dc_buffer(0.25, 44_100));
    let _ = fast.mix(4410);
    let pos_2x = fast.stream_position(b).expect("position is tracked");

    assert!(
        (pos_2x - pos_1x * 2.0).abs() < pos_1x * 0.05,
        "2x speed must advance twice as far: 1x={pos_1x}, 2x={pos_2x}",
    );

    // Negative speeds clamp to zero rather than playing backwards.
    fast.set_playback_speed_scale(-3.0);
    assert_eq!(fast.playback_speed_scale(), 0.0);

    // -- project audio/driver/mix_rate reaches the server --------------------
    let settings = ProjectSettings::from_godot_format(
        "[application]\n\nconfig/name=\"RateTest\"\n\n[audio]\n\ndriver/mix_rate=48000\n",
    )
    .expect("project.godot parses");

    let from_project = AudioServer::from_project_settings(&settings);
    assert_eq!(
        from_project.output_sample_rate(),
        48_000,
        "audio/driver/mix_rate must reach the server",
    );

    // A project that says nothing about audio keeps Godot's 44100 default.
    let bare = ProjectSettings::from_godot_format("[application]\n\nconfig/name=\"Bare\"\n")
        .expect("parses");
    assert_eq!(
        AudioServer::from_project_settings(&bare).output_sample_rate(),
        44_100,
        "an absent mix_rate falls back to Godot's default",
    );
}

/// An unknown bus or channel pair reports the floor instead of panicking —
/// the editor polls these every frame while buses are being added and removed.
#[test]
fn peak_queries_are_total() {
    let server = AudioServer::new();
    assert_eq!(server.get_bus_peak_volume_left_db(999, 0), -200.0);
    assert_eq!(server.get_bus_peak_volume_right_db(999, 0), -200.0);
    // Stereo output has exactly one channel pair, so pair 1 does not exist.
    assert_eq!(server.get_bus_peak_volume_left_db(0, 1), -200.0);
}

/// A malformed or absurd rate must not produce a degenerate server — `dt` and
/// the resample step both divide by the output rate.
#[test]
fn mix_rate_is_clamped() {
    assert!(AudioServer::with_sample_rate(0).output_sample_rate() >= 8_000);
    assert!(AudioServer::with_sample_rate(10_000_000).output_sample_rate() <= 384_000);

    let junk = ProjectSettings::from_godot_format("[audio]\n\ndriver/mix_rate=\"not a number\"\n")
        .expect("parses");
    assert_eq!(
        AudioServer::from_project_settings(&junk).output_sample_rate(),
        44_100,
        "a non-numeric mix_rate falls back rather than failing",
    );
}

//! # gdaudio
//!
//! Audio runtime, stream plumbing, and basic mixer
//! for the Patina Engine runtime.

pub mod bus;
pub mod mixer;
pub mod stream;

pub use bus::AudioBus;
pub use mixer::AudioMixer;
pub use stream::{AudioStreamPlayback, LoopMode, PlaybackState};

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Bus tests ----

    #[test]
    fn bus_create_defaults() {
        let bus = AudioBus::new("SFX");
        assert_eq!(bus.name(), "SFX");
        assert_eq!(bus.volume_db(), 0.0);
        assert!(!bus.is_mute());
        assert!(!bus.is_solo());
    }

    #[test]
    fn bus_volume_db_to_linear_unity() {
        let bus = AudioBus::new("Test");
        let lin = bus.volume_linear();
        assert!(
            (lin - 1.0).abs() < 1e-6,
            "0 dB should be linear 1.0, got {lin}"
        );
    }

    #[test]
    fn bus_volume_db_to_linear_minus20() {
        let mut bus = AudioBus::new("Test");
        bus.set_volume_db(-20.0);
        let lin = bus.volume_linear();
        assert!((lin - 0.1).abs() < 1e-6, "-20 dB should be ~0.1, got {lin}");
    }

    #[test]
    fn bus_mute_and_solo() {
        let mut bus = AudioBus::new("Test");
        bus.set_mute(true);
        assert!(bus.is_mute());
        bus.set_solo(true);
        assert!(bus.is_solo());
    }

    // ---- Mixer tests ----

    #[test]
    fn mixer_master_bus_exists() {
        let mixer = AudioMixer::new();
        assert_eq!(mixer.bus_count(), 1);
        assert_eq!(mixer.get_bus(0).unwrap().name(), "Master");
    }

    #[test]
    fn mixer_add_buses() {
        let mut mixer = AudioMixer::new();
        let idx = mixer.add_bus("SFX");
        assert_eq!(idx, 1);
        let idx2 = mixer.add_bus("Music");
        assert_eq!(idx2, 2);
        assert_eq!(mixer.bus_count(), 3);
    }

    #[test]
    fn mixer_remove_bus() {
        let mut mixer = AudioMixer::new();
        mixer.add_bus("SFX");
        mixer.add_bus("Music");
        mixer.remove_bus(1);
        assert_eq!(mixer.bus_count(), 2);
        assert_eq!(mixer.get_bus(1).unwrap().name(), "Music");
    }

    #[test]
    #[should_panic(expected = "cannot remove the master bus")]
    fn mixer_cannot_remove_master() {
        let mut mixer = AudioMixer::new();
        mixer.remove_bus(0);
    }

    #[test]
    fn mixer_get_by_name() {
        let mut mixer = AudioMixer::new();
        mixer.add_bus("SFX");
        mixer.add_bus("Music");
        assert_eq!(mixer.get_bus_by_name("Music"), Some(2));
        assert_eq!(mixer.get_bus_by_name("Master"), Some(0));
        assert_eq!(mixer.get_bus_by_name("Nonexistent"), None);
    }

    #[test]
    fn mixer_move_bus() {
        let mut mixer = AudioMixer::new();
        mixer.add_bus("SFX");
        mixer.add_bus("Music");
        mixer.add_bus("Voice");
        // Buses: [Master, SFX, Music, Voice]
        mixer.move_bus(3, 1); // Move Voice to index 1
                              // Buses: [Master, Voice, SFX, Music]
        assert_eq!(mixer.get_bus(1).unwrap().name(), "Voice");
        assert_eq!(mixer.get_bus(2).unwrap().name(), "SFX");
    }

    // ---- Playback tests ----

    #[test]
    fn playback_state_transitions() {
        let mut pb = AudioStreamPlayback::new(5.0);
        assert_eq!(pb.state(), PlaybackState::Stopped);

        pb.play();
        assert_eq!(pb.state(), PlaybackState::Playing);
        assert!(pb.is_playing());

        pb.pause();
        assert_eq!(pb.state(), PlaybackState::Paused);
        assert!(!pb.is_playing());

        pb.play();
        pb.stop();
        assert_eq!(pb.state(), PlaybackState::Stopped);
        assert_eq!(pb.get_playback_position(), 0.0);
    }

    #[test]
    fn playback_advance_moves_position() {
        let mut pb = AudioStreamPlayback::new(10.0);
        pb.play();
        pb.advance(1.5);
        assert!((pb.get_playback_position() - 1.5).abs() < 1e-6);
        pb.advance(2.0);
        assert!((pb.get_playback_position() - 3.5).abs() < 1e-6);
    }

    #[test]
    fn playback_loop_wraps_around() {
        let mut pb = AudioStreamPlayback::new(4.0);
        pb.set_loop_mode(LoopMode::Forward);
        pb.play();
        pb.advance(3.0);
        pb.advance(2.0); // 5.0 total, wraps to 1.0
        assert!((pb.get_playback_position() - 1.0).abs() < 1e-6);
        assert!(pb.is_playing());
    }

    #[test]
    fn playback_no_loop_stops_at_end() {
        let mut pb = AudioStreamPlayback::new(3.0);
        pb.play();
        pb.advance(4.0); // past end
        assert_eq!(pb.state(), PlaybackState::Stopped);
        assert_eq!(pb.get_playback_position(), 0.0);
    }

    #[test]
    fn playback_seek_clamps() {
        let mut pb = AudioStreamPlayback::new(5.0);
        pb.seek(3.0);
        assert!((pb.get_playback_position() - 3.0).abs() < 1e-6);
        pb.seek(100.0);
        assert!((pb.get_playback_position() - 5.0).abs() < 1e-6);
        pb.seek(-1.0);
        assert_eq!(pb.get_playback_position(), 0.0);
    }

    #[test]
    fn playback_volume_and_bus() {
        let mut pb = AudioStreamPlayback::new(5.0);
        assert_eq!(pb.volume_db(), 0.0);
        pb.set_volume_db(-6.0);
        assert_eq!(pb.volume_db(), -6.0);

        assert_eq!(pb.get_bus(), "Master");
        pb.set_bus("SFX");
        assert_eq!(pb.get_bus(), "SFX");
    }

    // ---- Integration test ----

    #[test]
    fn integration_playback_on_mixer_bus() {
        let mut mixer = AudioMixer::new();
        let sfx_idx = mixer.add_bus("SFX");

        let mut pb = AudioStreamPlayback::new(10.0);
        pb.set_bus("SFX");
        pb.play();
        pb.advance(2.0);

        // Verify the bus exists in the mixer and playback is routed to it
        let bus_idx = mixer.get_bus_by_name(pb.get_bus()).unwrap();
        assert_eq!(bus_idx, sfx_idx);

        let bus = mixer.get_bus_mut(bus_idx).unwrap();
        bus.set_volume_db(-10.0);
        let effective_linear = bus.volume_linear();
        assert!((effective_linear - 0.316_227_76).abs() < 1e-4);
        assert!(pb.is_playing());
        assert!((pb.get_playback_position() - 2.0).abs() < 1e-6);
    }
}

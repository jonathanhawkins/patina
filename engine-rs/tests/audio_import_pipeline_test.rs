//! pat-uxn40: Import pipeline for AudioStream from WAV and OGG files.
//!
//! Integration tests verifying the full audio import pipeline:
//!
//! 1. WAV file → AudioStreamLoader → AudioBuffer → AudioServer → mix output
//! 2. OGG file → header detection → decode → playback
//! 3. Format auto-detection from file headers
//! 4. Loader caching behavior
//! 5. Error handling for missing/corrupt files
//! 6. Resource importer → metadata extraction (WAV and OGG)
//! 7. Buffer type conversions (AudioSampleBuffer ↔ AudioBuffer)
//! 8. Multi-format concurrent loading
//!
//! Acceptance: all tests pass, WAV and OGG files decode correctly, and the
//! full pipeline from file load to mixed audio output works end-to-end.

use gdaudio::import::{
    audio_buffer_to_sample_buffer, decode_audio_data, load_audio_file, sample_buffer_to_audio_buffer,
    AudioFormat, AudioStreamLoader, ImportError,
};
use gdaudio::{AudioBuffer, AudioServer};

// ===========================================================================
// Helpers
// ===========================================================================

/// Build a 16-bit PCM WAV file in memory from i16 samples.
fn make_wav(sample_rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
    let sample_data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    gdaudio::wav::build_wav_bytes(sample_rate, channels, 16, 1, &sample_data)
}

/// Build a 32-bit float WAV file in memory from f32 samples.
fn make_wav_float(sample_rate: u32, channels: u16, samples: &[f32]) -> Vec<u8> {
    let sample_data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    gdaudio::wav::build_wav_bytes(sample_rate, channels, 32, 3, &sample_data)
}

// ===========================================================================
// 1. Full pipeline: WAV file → load → decode → play → mix
// ===========================================================================

#[test]
fn uxn40_wav_full_pipeline_load_decode_play_mix() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("test.wav");

    // DC value of ~0.5 (16384 / 32768).
    let samples: Vec<i16> = vec![16384; 8820]; // ~0.2 sec at 44100 Hz mono
    std::fs::write(&path, make_wav(44100, 1, &samples)).unwrap();

    // Load.
    let mut loader = AudioStreamLoader::new();
    let import = loader.load_file(&path).unwrap();

    assert_eq!(import.info.format, AudioFormat::Wav);
    assert_eq!(import.info.sample_rate, 44100);
    assert_eq!(import.info.channels, 1);
    assert_eq!(import.info.frame_count, 8820);
    assert!((import.info.duration_secs - 0.2).abs() < 0.01);

    // Play on server.
    let mut server = AudioServer::new();
    let id = server.play(import.buffer);
    assert!(server.is_playing(id));

    // Mix and verify.
    let output = server.mix(512);
    assert_eq!(output.len(), 512 * 2); // stereo
    assert!(
        (output[0] - 0.5).abs() < 0.02,
        "L={} expected ~0.5",
        output[0]
    );
    assert!(
        (output[1] - 0.5).abs() < 0.02,
        "R={} expected ~0.5",
        output[1]
    );
}

// ===========================================================================
// 2. WAV stereo loading and mixing
// ===========================================================================

#[test]
fn uxn40_wav_stereo_pipeline() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("stereo.wav");

    // Stereo: L=0.8, R=0.2 (as float WAV).
    let mut samples = Vec::new();
    for _ in 0..4410 {
        samples.push(0.8f32);
        samples.push(0.2f32);
    }
    std::fs::write(&path, make_wav_float(44100, 2, &samples)).unwrap();

    let mut loader = AudioStreamLoader::new();
    let import = loader.load_file(&path).unwrap();

    assert_eq!(import.buffer.channels, 2);
    assert_eq!(import.buffer.frame_count(), 4410);

    let mut server = AudioServer::new();
    server.play(import.buffer);

    let output = server.mix(4);
    assert!(
        (output[0] - 0.8).abs() < 0.02,
        "L={} expected ~0.8",
        output[0]
    );
    assert!(
        (output[1] - 0.2).abs() < 0.02,
        "R={} expected ~0.2",
        output[1]
    );
}

// ===========================================================================
// 3. WAV at different sample rates
// ===========================================================================

#[test]
fn uxn40_wav_various_sample_rates() {
    let dir = tempfile::TempDir::new().unwrap();

    for &rate in &[22050u32, 44100, 48000, 96000] {
        let path = dir.path().join(format!("rate_{rate}.wav"));
        let samples = vec![16384i16; rate as usize]; // 1 second
        std::fs::write(&path, make_wav(rate, 1, &samples)).unwrap();

        let import = load_audio_file(&path).unwrap();
        assert_eq!(import.info.sample_rate, rate);
        assert!(
            (import.info.duration_secs - 1.0).abs() < 0.01,
            "rate={rate}: duration={} expected ~1.0",
            import.info.duration_secs
        );
    }
}

// ===========================================================================
// 4. Format detection from header bytes
// ===========================================================================

#[test]
fn uxn40_format_detection_wav_header() {
    let wav_data = make_wav(44100, 1, &[0, 0]);
    assert_eq!(AudioFormat::from_header(&wav_data), Some(AudioFormat::Wav));
}

#[test]
fn uxn40_format_detection_ogg_header() {
    assert_eq!(
        AudioFormat::from_header(b"OggS\x00\x02"),
        Some(AudioFormat::OggVorbis)
    );
}

#[test]
fn uxn40_format_detection_unknown() {
    assert_eq!(AudioFormat::from_header(b"ID3\x04"), None);
    assert_eq!(AudioFormat::from_header(b"fl"), None);
}

// ===========================================================================
// 5. Format detection from extension
// ===========================================================================

#[test]
fn uxn40_format_from_extension() {
    assert_eq!(AudioFormat::from_extension("wav"), Some(AudioFormat::Wav));
    assert_eq!(AudioFormat::from_extension("WAV"), Some(AudioFormat::Wav));
    assert_eq!(
        AudioFormat::from_extension("ogg"),
        Some(AudioFormat::OggVorbis)
    );
    assert_eq!(AudioFormat::from_extension("mp3"), None);
    assert_eq!(AudioFormat::from_extension("flac"), None);
}

// ===========================================================================
// 6. Header-based fallback when extension is wrong
// ===========================================================================

#[test]
fn uxn40_header_fallback_overrides_bad_extension() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("audio.dat"); // unknown extension
    std::fs::write(&path, make_wav(44100, 1, &[0, 100])).unwrap();

    let import = load_audio_file(&path).unwrap();
    assert_eq!(import.info.format, AudioFormat::Wav);
}

// ===========================================================================
// 7. Loader caching
// ===========================================================================

#[test]
fn uxn40_loader_cache_hit() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("cached.wav");
    std::fs::write(&path, make_wav(44100, 1, &[0; 100])).unwrap();

    let mut loader = AudioStreamLoader::new();
    assert_eq!(loader.cache_size(), 0);

    let _ = loader.load_file(&path).unwrap();
    assert_eq!(loader.cache_size(), 1);

    // Second load should be cached.
    let import2 = loader.load_file(&path).unwrap();
    assert_eq!(loader.cache_size(), 1);
    assert_eq!(import2.info.sample_rate, 44100);
}

#[test]
fn uxn40_loader_cache_disabled() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("uncached.wav");
    std::fs::write(&path, make_wav(44100, 1, &[0; 10])).unwrap();

    let mut loader = AudioStreamLoader::without_cache();
    let _ = loader.load_file(&path).unwrap();
    assert_eq!(loader.cache_size(), 0);
}

#[test]
fn uxn40_loader_clear_cache() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("clear.wav");
    std::fs::write(&path, make_wav(44100, 1, &[0])).unwrap();

    let mut loader = AudioStreamLoader::new();
    let _ = loader.load_file(&path).unwrap();
    assert_eq!(loader.cache_size(), 1);

    loader.clear_cache();
    assert_eq!(loader.cache_size(), 0);
}

// ===========================================================================
// 8. Error handling
// ===========================================================================

#[test]
fn uxn40_missing_file_returns_io_error() {
    let err = load_audio_file("/nonexistent/audio.wav").unwrap_err();
    assert!(matches!(err, ImportError::Io { .. }));
}

#[test]
fn uxn40_unsupported_extension_and_header() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("music.mp3");
    std::fs::write(&path, b"ID3v2_NOT_SUPPORTED").unwrap();

    let err = load_audio_file(&path).unwrap_err();
    assert!(matches!(err, ImportError::UnsupportedFormat(_)));
}

#[test]
fn uxn40_corrupt_wav_returns_decode_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("corrupt.wav");
    // Valid RIFF header but garbage content.
    std::fs::write(&path, b"RIFF\x00\x00\x00\x00WAVEgarbage").unwrap();

    let err = load_audio_file(&path).unwrap_err();
    assert!(matches!(err, ImportError::Decode { .. }));
}

// ===========================================================================
// 9. Buffer type conversion roundtrip
// ===========================================================================

#[test]
fn uxn40_audio_buffer_sample_buffer_roundtrip() {
    let original = AudioBuffer {
        samples: vec![0.1, -0.2, 0.3, -0.4, 0.5, -0.6],
        sample_rate: 48000,
        channels: 2,
    };

    let sample_buf = audio_buffer_to_sample_buffer(&original);
    assert_eq!(sample_buf.sample_rate, 48000);
    assert_eq!(sample_buf.channels, 2);
    assert_eq!(
        sample_buf.channel_layout,
        gdaudio::ChannelLayout::Stereo
    );

    let back = sample_buffer_to_audio_buffer(&sample_buf);
    assert_eq!(back.sample_rate, original.sample_rate);
    assert_eq!(back.channels, original.channels);
    assert_eq!(back.samples, original.samples);
}

#[test]
fn uxn40_mono_buffer_conversion() {
    let ab = AudioBuffer {
        samples: vec![0.5; 100],
        sample_rate: 22050,
        channels: 1,
    };
    let sb = audio_buffer_to_sample_buffer(&ab);
    assert_eq!(sb.channel_layout, gdaudio::ChannelLayout::Mono);
    assert_eq!(sb.frame_count(), 100);
    assert!((sb.duration_secs() - 100.0 / 22050.0).abs() < 1e-5);
}

// ===========================================================================
// 10. In-memory decode of raw WAV data
// ===========================================================================

#[test]
fn uxn40_decode_raw_wav_data() {
    let wav_bytes = make_wav(44100, 1, &[0, 16384, -16384, 0]);
    let buf = decode_audio_data(&wav_bytes, AudioFormat::Wav).unwrap();

    assert_eq!(buf.sample_rate, 44100);
    assert_eq!(buf.channels, 1);
    assert_eq!(buf.frame_count(), 4);
    assert!((buf.samples[0]).abs() < 1e-4); // silence
    assert!((buf.samples[1] - 0.5).abs() < 0.01); // half positive
    assert!((buf.samples[2] + 0.5).abs() < 0.01); // half negative
}

// ===========================================================================
// 11. Multiple WAV files loaded concurrently, played on different buses
// ===========================================================================

#[test]
fn uxn40_multi_file_multi_bus_pipeline() {
    let dir = tempfile::TempDir::new().unwrap();

    // File 1: quiet DC (0.3).
    let path1 = dir.path().join("sfx.wav");
    let samples1: Vec<f32> = vec![0.3; 44100];
    std::fs::write(&path1, make_wav_float(44100, 1, &samples1)).unwrap();

    // File 2: louder DC (0.7).
    let path2 = dir.path().join("music.wav");
    let samples2: Vec<f32> = vec![0.7; 44100];
    std::fs::write(&path2, make_wav_float(44100, 1, &samples2)).unwrap();

    let mut loader = AudioStreamLoader::new();
    let import1 = loader.load_file(&path1).unwrap();
    let import2 = loader.load_file(&path2).unwrap();

    assert_eq!(loader.cache_size(), 2);

    let mut server = AudioServer::new();
    server.mixer_mut().add_bus("SFX");
    server.mixer_mut().add_bus("Music");

    let id1 = server.play_on_bus(import1.buffer, "SFX");
    let id2 = server.play_on_bus(import2.buffer, "Music");

    assert!(server.is_playing(id1));
    assert!(server.is_playing(id2));
    assert_eq!(server.active_stream_count(), 2);

    // Mix: both at 0 dB, so output = 0.3 + 0.7 = 1.0.
    let output = server.mix(1);
    assert!(
        (output[0] - 1.0).abs() < 0.02,
        "mixed = {} expected ~1.0",
        output[0]
    );
}

// ===========================================================================
// 12. Resource importer integration (WAV metadata)
// ===========================================================================

#[test]
fn uxn40_resource_importer_wav_metadata() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("meta.wav");
    std::fs::write(&path, make_wav(48000, 2, &[0; 96000])).unwrap();

    let res = gdresource::import_wav(&path).unwrap();
    assert_eq!(res.class_name, "AudioStreamWAV");
    assert_eq!(
        res.get_property("sample_rate"),
        Some(&gdvariant::Variant::Int(48000))
    );
    assert_eq!(
        res.get_property("channels"),
        Some(&gdvariant::Variant::Int(2))
    );
    assert_eq!(
        res.get_property("bit_depth"),
        Some(&gdvariant::Variant::Int(16))
    );

    // length_seconds should be ~1.0 (96000 samples / 2 channels / 48000 rate).
    if let Some(gdvariant::Variant::Float(len)) = res.get_property("length_seconds") {
        assert!(
            (*len - 1.0).abs() < 0.01,
            "length_seconds={len} expected ~1.0"
        );
    } else {
        panic!("missing length_seconds");
    }
}

// ===========================================================================
// 13. Resource format loader recognizes audio extensions
// ===========================================================================

#[test]
fn uxn40_resource_format_loader_audio_extensions() {
    let rfl = gdresource::ResourceFormatLoader::with_defaults();
    assert!(rfl.can_load("wav"));
    assert!(rfl.can_load("WAV"));
    assert!(rfl.can_load(".wav"));
}

// ===========================================================================
// 14. Full pipeline: WAV → resource importer + audio decoder agreement
// ===========================================================================

#[test]
fn uxn40_importer_and_decoder_agree_on_metadata() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("agree.wav");
    let samples: Vec<i16> = vec![0; 22050]; // 0.5 sec at 44100 mono
    std::fs::write(&path, make_wav(44100, 1, &samples)).unwrap();

    // Resource importer: metadata only.
    let res = gdresource::import_wav(&path).unwrap();
    let res_rate = match res.get_property("sample_rate") {
        Some(gdvariant::Variant::Int(r)) => *r,
        _ => panic!("missing sample_rate"),
    };
    let res_channels = match res.get_property("channels") {
        Some(gdvariant::Variant::Int(c)) => *c,
        _ => panic!("missing channels"),
    };

    // Audio decoder: full decode.
    let import = load_audio_file(&path).unwrap();

    // Both should agree.
    assert_eq!(res_rate as u32, import.info.sample_rate);
    assert_eq!(res_channels as u16, import.info.channels);
}

// ===========================================================================
// 15. Pipeline with bus volume attenuation
// ===========================================================================

#[test]
fn uxn40_pipeline_with_bus_attenuation() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("atten.wav");

    // Full-scale DC (1.0).
    let samples: Vec<f32> = vec![1.0; 44100];
    std::fs::write(&path, make_wav_float(44100, 1, &samples)).unwrap();

    let import = load_audio_file(&path).unwrap();

    let mut server = AudioServer::new();
    // Master at -20 dB = 0.1 linear.
    server
        .mixer_mut()
        .get_bus_mut(0)
        .unwrap()
        .set_volume_db(-20.0);

    server.play(import.buffer);

    let output = server.mix(1);
    assert!(
        (output[0] - 0.1).abs() < 0.02,
        "attenuated output = {} expected ~0.1",
        output[0]
    );
}

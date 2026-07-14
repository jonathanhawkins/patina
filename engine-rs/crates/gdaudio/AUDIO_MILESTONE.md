# Minimal Audio Milestone

This document defines the smallest acceptable audio contract for the current Patina Engine port stage.

## Implemented (Stub-Level)

### Bus Routing (`bus.rs`)
- `AudioBus`: named bus with volume (dB), mute, and solo controls
- dB-to-linear conversion (`10^(dB/20)`)
- Bus creation with unity gain defaults

### Mixer (`mixer.rs`)
- `AudioMixer`: ordered collection of `AudioBus` instances
- Master bus always at index 0 (cannot be removed)
- Add, remove, move, and lookup buses by name or index

### Playback State Machine (`stream.rs`)
- `AudioStreamPlayback`: controls playback of a single stream
- States: `Stopped` → `Playing` → `Paused` → `Playing` → `Stopped`
- `play()`, `pause()`, `stop()`, `seek()`, `advance(delta)`
- Loop modes: `None` (stop at end), `Forward` (wrap around)
- Volume (dB) and bus routing by name
- Position tracking with clamp and wrap semantics

### WAV Decode (`wav.rs`)
- Parses RIFF/WAV files (fmt + data chunks) into `AudioSampleBuffer` (f32 PCM)
- Supports 8-bit unsigned, 16-bit signed, 24-bit signed, 32-bit float
- Mono and stereo layouts; normalizes all formats to `f32`
- **Note**: Decode-to-buffer only — no real audio output or streaming playback

## Platform output (pat-p0s82.1)

- **`PlatformAudioOutput`** (`output.rs`): real device output via `cpal`
  (CoreAudio on macOS, WASAPI on Windows, ALSA/Pulse on Linux).
- **`mix_and_write`**: pumps `AudioServer::mix` into any `AudioOutputStream`.
- **`CaptureAudioOutput`**: records PCM for tests / offline capture.
- **`NullAudioOutput`**: still available for headless CI.

## Deferred (Not in Current Scope)

- **OGG/MP3 decoding**: OGG is decoded; MP3 remains later
- **Effects processing**: No reverb, EQ, compression, or bus effects chain (P0 bead pat-p0s82.2)
- **Audio streaming**: No background file streaming or buffer management
- **Bus send routing**: No bus-to-bus send topology (only flat bus list)
- **PingPong loop mode**: Only `None` and `Forward` loops implemented
- **AudioServer singleton**: No global AudioServer — mixer is standalone

## Test Coverage

Smoke tests guard the stub contract in `engine-rs/crates/gdaudio/src/lib.rs`:

| Test | What it covers |
|------|---------------|
| `bus_create_defaults` | Bus initialization: name, volume, mute, solo |
| `bus_volume_db_to_linear_unity` | 0 dB → 1.0 linear conversion |
| `bus_volume_db_to_linear_minus20` | -20 dB → 0.1 linear conversion |
| `bus_mute_and_solo` | Mute/solo flag toggling |
| `mixer_master_bus_exists` | Master bus always at index 0 |
| `mixer_add_buses` | Adding named buses |
| `mixer_remove_bus` | Removing non-master buses |
| `mixer_cannot_remove_master` | Master bus protection (panics) |
| `mixer_get_by_name` | Name-based bus lookup |
| `mixer_move_bus` | Bus reordering |
| `playback_state_transitions` | Stopped→Playing→Paused→Playing→Stopped |
| `playback_advance_moves_position` | Time advancement |
| `playback_loop_wraps_around` | Forward loop wrap semantics |
| `playback_no_loop_stops_at_end` | Stop at end without loop |
| `playback_seek_clamps` | Seek clamping to [0, length] |
| `playback_volume_and_bus` | Volume dB set/get, bus routing |
| `integration_playback_on_mixer_bus` | End-to-end playback through mixer bus |

## Graduation Criteria

Audio graduates from stub to real implementation when:
1. Runtime oracle parity reaches 98%+ across all supported scenes
2. A platform audio backend is selected and prototyped
3. At least one fixture scene requires audible output for behavioral verification

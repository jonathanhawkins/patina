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

## Deferred (Not in Current Scope)

The following are explicitly out of scope until runtime parity exits are met:

- **Actual audio output**: No platform audio backend (WASAPI, CoreAudio, ALSA/PulseAudio)
- **Audio decoding**: No WAV/OGG/MP3 decoding — streams are time-based stubs
- **Effects processing**: No reverb, EQ, compression, or bus effects chain
- **Spatial audio**: No 2D/3D positional audio or distance attenuation
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

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
- **Note**: WAV decodes to a PCM buffer; playback and device output are handled
  by the mixer + platform output described below. (This line previously read
  "no real audio output", which is no longer accurate — see Platform output.)

## Platform output (pat-p0s82.1)

- **`PlatformAudioOutput`** (`output.rs`): real device output via `cpal`
  (CoreAudio on macOS, WASAPI on Windows, ALSA/Pulse on Linux).
- **`mix_and_write`**: pumps `AudioServer::mix` into any `AudioOutputStream`.
- **`CaptureAudioOutput`**: records PCM for tests / offline capture.
- **`NullAudioOutput`**: still available for headless CI.

## Implemented beyond the minimal milestone

The subsystem has moved well past the "stub" contract above. Verified in-tree
(2026-07-16 reconciliation, pat-z35r2.20):

- **Decoding**: WAV, OGG, and **MP3** (`decode_mp3` via minimp3, pat-p0s82.13)
  all decode to `f32` PCM buffers (`decode.rs`); `import.rs` covers the
  resource-import path.
- **Effects chain** (`effects.rs`, pat-p0s82.2 / pat-u3l8c.107): `AudioEffectFilter`
  (RBJ biquad low/high/band/notch/peak/shelf), sidechain `AudioEffectCompressor`,
  `AudioEffectLimiter`/`AudioEffectHardLimiter`, `AudioEffectSpectrumAnalyzer`,
  `AudioEffectAmplify` — applied per-bus in the mixer.
- **Bus-to-bus send routing**: `AudioServer::mix` (`server.rs`) runs the full bus
  send graph — per-bus volume, effect chain, mute/solo audibility gate, and
  `send`/`send_db` routing into a target bus, plus effect `bypass`
  (pat-nzgdu.26/.27).
- **Bus-layout load**: `AudioMixer::load_bus_layout_str` / `load_bus_layout`
  parse a Godot `[audio_bus_layout]` / `default_bus_layout.tres` section into
  named buses (name/volume_db/send/send_db/mute/solo/bypass_fx).
- **Real device output**: `PlatformAudioOutput` (`output.rs`, cpal) + `mix_and_write`,
  and `gdscene::audio_server_bridge` pumps `AudioServer::mix` — so mixed output
  reaches a device. The earlier "no real audio output / nothing pumps `mix()`"
  claim is no longer accurate.
- **Procedural audio**: `AudioStreamGenerator` (`decode.rs`) exposes the
  `push_frame` buffer API.

## Remaining gaps (NOT 1:1 complete)

Precise deltas versus Godot 4.6.1 — see the
[1:1 scorecard](../../../prd/PORT_1TO1_SCORECARD_2026-07-11.md):

- **Script-facing `AudioServer` singleton**: the Rust `AudioServer` exists and
  the mixer honors bus layout/send/solo/effects, but GDScript access via the
  interpreter singleton (`AudioServer.set_bus_volume_db`/`set_bus_mute`/
  `set_bus_solo`/`add_bus_effect`) is not yet wired (pat-nzgdu.26 delivered the
  mixer half; the interpreter dispatch remains).
- **Boot-time bus-layout auto-load**: the mixer *can* parse a bus layout, but
  auto-loading `res://default_bus_layout.tres` at project boot is not wired.
- **Streaming**: decoding is whole-buffer; there is no incremental background
  file streaming / buffer management yet.
- **Loop modes**: only `None` and `Forward`; `PingPong` is not implemented.
- **Godot oracle parity**: no automated Godot-vs-Patina audio parity comparison
  yet, and no fixture scene gates audible output.

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

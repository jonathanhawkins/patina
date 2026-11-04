---
name: audio-debug
description: Measure what the audio DEVICE actually played while a game runs in the Patina web editor — underruns (dropouts), clicks with timestamps, gaps, clipping, level — and save it as a WAV, instead of guessing from code. Use whenever someone says the audio crackles, pops, stutters, cuts out, sounds distorted, plays at the wrong pitch, or is "not perfect", and after touching anything in gdaudio, the audio bridge, or the device output. Trigger on: "crackle", "crackling", "pops", "audio stutters", "sound cuts out", "distorted", "music sounds wrong", "audio glitch".
argument-hint: [project-dir] [--port 9410]
---

# Audio Debug

You cannot hear. The device can be measured. Every audible defect leaves a mark
in the samples the device was handed:

| Heard as | Measured as | Usual cause |
|---|---|---|
| crackle / stutter / dropouts | `underruns > 0`, `gaps` ~50 ms | producer not keeping the device queue fed (game-clock pacing, a stalled frame) |
| harsh distortion on loud parts | `clipped > 0`, `peak ≥ 1` | mix exceeds full scale |
| sharp pops with no underruns | clicks **> 200x** local curvature | mix discontinuity: block seam, loop seam, cursor snap |
| gritty, worse the longer it plays | rises with play time | f32 playhead precision (fixed: f64 cursor) |
| slightly sharp, periodic gaps | device rate ≠ mix rate | output format not adopted (fixed: `attach_output`) |

⚠️ **Soft clicks (8-45x) are usually the music itself.** Dodge the Creeps'
source OGG shows 13-15 per 8 s (drum hits). Always compare against the source:
`gdaudio::diagnostics::analyze(&decoded.samples, rate, ch)` on the decoded file.
A real cut measured **36,000x**.

## Steps

1. Release editor (debug renders slowly and starves audio):
   `scripts/editor-bg.sh start --release --build --port 9410 <project> <scene>`
   (pass `--port` to `stop` too — bare `stop` kills :8080).
2. Play it the way a person does, with real audio, and measure the last N s
   the device played (music starts only once the game starts, so press its
   start key first):
   ```
   node scripts/browser-playtest.mjs --url http://localhost:9410/editor --audio 8 \
     --keys "wait:1500,Enter:100,wait:8000"
   ```
   The page's Play button opens the real device (`audio: true`); this plays
   through the user's speakers for the run.
3. Read `audio device:` in the report: the counters, the clicks/gaps with
   timestamps, the `finding:` lines (they also fail the run), and `audio.wav`
   in the report directory.
4. While a human plays, the same numbers come from MCP `audio`
   (`{"secs": 8, "save": "/tmp/x.wav"}`) or `GET /api/runtime/audio?secs=8`
   (`&wav=1` for the WAV).
5. Fix the cause the measurement names, add a regression test that measures
   the same quantity (see `gdaudio/tests/long_streams_stay_clean.rs`,
   `gdscene/tests/audio_output_adopts_the_device_format.rs`), re-run step 2,
   and only then ask the human to listen.

## Where things are

- Device queue, counters, 10 s history: `gdaudio::output::SharedRing`
  (`live_device_stats`, `live_device_capture`).
- Analyser: `gdaudio::diagnostics::analyze`.
- Feeding policy: `AudioServerBridge::process` tops the device queue up to
  `DEVICE_QUEUE_TARGET_SECS` (the device clock decides how much is mixed).
- Resampler: `AudioServer::mix` — Godot's 4-point cubic Hermite, neighbours
  wrapping through the loop window (`servers/audio/audio_stream.cpp:207`).

# Windows desktop runtime smoke (pat-z35r2.13)

Environment in CI may not boot a windowed GUI or expose an audio device.
Acceptance for this bead:

1. `gdplatform` target metadata includes `x86_64-pc-windows-msvc`.
2. Audio uses cpal (WASAPI on Windows when a device is present).
3. A dedicated **window + input + audio** smoke runs headlessly on every OS,
   including the `windows-latest` CI runner.
4. Manual smoke (developer machine): build `patina-runner`, open a window,
   press a key, play a WAV.

## Automated proof

`engine-rs/tests/platform_suite/windows_runtime_smoke_test.rs` composes the
three subsystems a Windows desktop runtime must drive, through their
device-independent paths so the same test provides real coverage on the
`windows-latest` CI runner:

- **Window** — `WindowsPlatformLayer` creates a sized, foregrounded,
  per-monitor-v2 DPI-aware window, tracks a mid-loop resize, observes the
  light/dark theme, and drives the frame loop to a clean quit on
  `CloseRequested`.
- **Input** — `WindowEvent`s (key press, mouse click, motion) from the window
  backend translate to `InputEvent`s and update `InputState`.
- **Audio** — a 16-bit stereo WAV is synthesized, decoded via `decode_wav`,
  and pushed through an `AudioOutputStream`. `NullAudioOutput` stands in for
  the WASAPI/cpal device present on a real Windows machine.

Run it:

```
cargo nextest run --test platform_suite -E 'test(/windows_runtime_smoke/)'
```

CI runs the full `platform_suite` on `ubuntu-latest`, `macos-latest`, and
`windows-latest` (see `.github/workflows/ci.yml`), so the smoke executes on
Windows on every push.

Existing supporting coverage: platform target validation
(`platform_targets_validation_test.rs`) and the headless stable-layer
composition test (`platform_first_stable_layer_test.rs`).

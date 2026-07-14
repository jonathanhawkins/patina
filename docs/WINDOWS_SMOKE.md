# Windows desktop runtime smoke (pat-z35r2.13)

Environment in CI may not boot a windowed GUI. Acceptance for this bead:

1. `gdplatform` target metadata includes `x86_64-pc-windows-msvc`.
2. Audio uses cpal (WASAPI on Windows when device present).
3. Manual smoke (developer machine): build `patina-runner`, open window, press key, play WAV.

Automated proof: platform target validation tests + this document.

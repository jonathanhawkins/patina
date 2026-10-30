# Web export: one command to a playable folder (pat-u3l8c.114)

```sh
scripts/web-export.sh --project fixtures/project --app "My Game"
python3 -m http.server 8080 --directory engine-rs/export/web
# open http://localhost:8080/
```

That is the whole flow. `--release` for an optimized build; `scripts/web-export.sh --help`
lists the rest.

## What lands in the folder

| File | Where it comes from | Why it is needed |
| --- | --- | --- |
| `index.html` | generated from `WebExportConfig` | hosts the canvas and boots the bundle |
| `patina_web.js` | `wasm-bindgen --target web` | instantiates the wasm module |
| `patina_web_bg.wasm` | the wasm build | the engine |
| `<App_Name>.pck` | the project directory | everything `res://` resolves out of |

With `--pwa`, three more: `manifest.webmanifest`, `sw.js`, `offline.html`. The
service worker precaches exactly the four files above — a worker that guessed
would eventually pair a stale `.pck` with a fresh `.wasm`, which surfaces as a
corrupt archive rather than as a cache miss.

The artifact names are **bindgen's**, not the app's. `wasm-bindgen` writes the
`.wasm` filename into the JS it generates, so renaming either would break an
`import` inside a file the export does not author.

## The orderings that matter

The generated shell does these in order, and none of the failures are loud:

1. **`user://` is hydrated from IndexedDB before the first frame.** A script
   that reads `user://settings.cfg` on frame one before hydration finishes sees
   an empty store and writes defaults over it. The data is in IndexedDB; it just
   arrived late — indistinguishable from "saving is broken".
2. **The `.pck` is mounted before anything loads a scene.** A `res://` read that
   misses the pack falls through to a filesystem the browser does not have.
3. **The audio worklet is registered from the handle `startWebAudio` returns.**
   That call builds the `AudioContext` and the mixer and stops there — it hands
   the `AudioWorkletProcessor` back as a source string (`audio.workletGlue`) for
   the page to import and register. Drop the handle and the mixer is never
   pulled from: the page runs, logs nothing, and is silent. The gesture unlock
   is the same failure one step later — a context created before any user input
   starts suspended.

The first two go through `patina-web`'s `WebVfs` (pat-u3l8c.113), the third
through its `web_audio` bindings. A custom `--shell` must do all three; copy the
sequence out of `gdplatform::web_export::DEFAULT_SHELL`.

## What is excluded from the pack

`.godot/` (editor cache, regenerated), `*.import` sidecars (importer metadata,
not runtime content), dot-files and dot-directories (`.git`, `.DS_Store`), and
**the export's own output directory** — otherwise exporting into the project
packs the previous export into the next one and doubles its size every run.

The export refuses to write a pack with zero files rather than produce a page
that loads and then cannot find its main scene.

## A custom HTML shell

`--shell my_shell.html` is used when the file contains at least one
`{{PATINA_…}}` placeholder; otherwise the built-in shell is used instead. Both
go through the same substitution, so a custom shell cannot silently miss a value
the default one fills in:

`{{PATINA_APP_NAME}}` · `{{PATINA_JS}}` · `{{PATINA_WASM}}` · `{{PATINA_PCK}}` ·
`{{PATINA_HEAD_INCLUDE}}` · `{{PATINA_INITIAL_MEMORY_MB}}` ·
`{{PATINA_MAX_MEMORY_MB}}` · `{{PATINA_PRELOADER_DISPLAY}}` ·
`{{PATINA_FOCUS_CANVAS}}` · `{{PATINA_REQUIRE_ISOLATION}}` ·
`{{PATINA_REQUIRE_SIMD}}`

`head_include` is injected **unescaped** — it exists so a project can add a font
link or an analytics snippet. The app name is escaped, since it ends up in the
title and a visible label.

## Memory and features are build flags, not page settings

`initial_memory_mb`, `max_memory_mb`, `simd_enabled` and `threads_enabled` are
link-time: they become `--initial-memory` / `--max-memory` link args and
`-C target-feature=…`. No HTML shell can apply them, which is why
`gdplatform::web_export::rustflags` is their consumer and the script asks for
them *before* building. `max_memory_mb = 0` means unlimited growth, so no cap is
emitted — passing 0 through would cap the heap at zero and fail the first
allocation.

## ⚠️ `--threads` cannot be served by `python3 -m http.server`

`SharedArrayBuffer` is only exposed to a cross-origin-isolated page, which needs
response headers a static file server does not send:

```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

A threaded export behind `http.server` loads and then refuses to start. The
generated shell checks `crossOriginIsolated` and says so, rather than failing
deep inside wasm instantiation, and `serve_hint` prints the headers instead of
the `http.server` line. `--threads` also needs a nightly toolchain, because std
must be rebuilt with atomics (`-Z build-std`); the script checks and stops
rather than producing a build that cannot boot.

## Where the logic lives

| Piece | Location | Tested by |
| --- | --- | --- |
| names, HTML, build flags, pack exclusions | `gdplatform::web_export` | `gdplatform/tests/web_export_produces_a_servable_folder.rs` |
| filesystem walk, `.pck` writing, folder assembly | `engine-rs/examples/web_export.rs` | — (IO driver) |
| build + bindgen + the above | `scripts/web-export.sh` | — (one command) |

The split is deliberate: the parts that can be wrong about something (which
files ship, what the shell says, which flags a config implies) are pure
functions a test can reach. `gdplatform` cannot see `gdresource`, where
`PckPacker` lives, which is why the packing itself is in the driver.

## Related

- `scripts/web.sh` — runs the editor, or the `patina-web` harness, for
  development. This document is about shipping a project.
- `engine-rs/crates/patina-web/build-web.sh` — the wasm build + bindgen steps on
  their own, without packing or a shell.
- [EXPORT_SCOPE.md](EXPORT_SCOPE.md) — why Patina packaging is not bit-compatible
  with Godot's export templates.

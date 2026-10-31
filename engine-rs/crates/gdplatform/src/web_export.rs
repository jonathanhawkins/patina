//! The web export: turning a built wasm bundle into a folder a static server
//! can serve as a playable game (pat-u3l8c.114).
//!
//! Godot's web export writes four files next to each other — `index.html`, the
//! bindgen JS, the `.wasm`, and the project `.pck` — plus the optional PWA set,
//! and substitutes the project's values into an HTML shell
//! (`platform/web/export/export_plugin.cpp`, `platform/web/js/engine/`). This
//! module is that step for Patina.
//!
//! ## What was here before
//!
//! [`crate::export`] could name an output file (`output_filename()` mapped the
//! `"web"` platform to a bare `.wasm` extension) and emit a text manifest, and
//! [`WebExportConfig`] carried nine fields — `html_shell`, `initial_memory_mb`,
//! `threads_enabled`, `pwa_enabled` and the rest — that **nothing read**. A
//! `.wasm` on its own is not servable: a browser needs the bindgen glue to
//! instantiate it, an HTML document to host it, and the pack to read `res://`
//! out of. So the export described a deployment it could not produce.
//!
//! ## Layering
//!
//! Everything here is pure: names, text, and the `res://` mapping. Nothing
//! walks a filesystem, builds wasm, or writes a `.pck` — `gdplatform` cannot
//! even see `gdresource`, where [`PckPacker`] lives. The driver that does the
//! IO is `engine-rs/examples/web_export.rs`, and `scripts/web-export.sh` is the
//! one command the bead asks for. That split is deliberate: the parts with bugs
//! in them (which files belong in the pack, what the shell says) are the parts
//! a test can reach.
//!
//! [`PckPacker`]: https://docs.rs/gdresource
//!
//! ## ⚠️ Threads and `python -m http.server`
//!
//! `threads_enabled` needs `SharedArrayBuffer`, which a browser only exposes to
//! a cross-origin-isolated page — that means `Cross-Origin-Opener-Policy:
//! same-origin` and `Cross-Origin-Embedder-Policy: require-corp` response
//! headers. `python -m http.server` cannot send them, so a threaded export is
//! **not** servable by the command in this bead's acceptance. [`serve_hint`]
//! says so in the text it hands the user rather than letting them discover a
//! blank page, and the generated shell checks `crossOriginIsolated` and reports
//! it instead of failing deep inside the wasm instantiation.

use crate::web::WebExportConfig;
use std::path::{Component, Path};

/// The files a web export writes, in the order they are produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebExportLayout {
    /// The HTML document the browser loads — always `index.html`, because a
    /// static server serves it for the bare directory URL.
    pub index_html: String,
    /// The `wasm-bindgen` JS glue, which the document imports.
    pub js: String,
    /// The wasm module itself.
    pub wasm: String,
    /// The project pack every `res://` read resolves out of.
    pub pck: String,
    /// PWA extras, present only when [`WebExportConfig::pwa_enabled`] is set.
    pub extra: Vec<String>,
}

impl WebExportLayout {
    /// Every file in the export, `index.html` first.
    pub fn all(&self) -> Vec<String> {
        let mut files = vec![
            self.index_html.clone(),
            self.js.clone(),
            self.wasm.clone(),
            self.pck.clone(),
        ];
        files.extend(self.extra.iter().cloned());
        files
    }
}

/// The wasm crate's name, and so the stem `wasm-bindgen` gives its output.
///
/// Fixed rather than derived from the app name: `wasm-bindgen` names its files
/// after the *crate*, and the export copies them as they are instead of
/// renaming — a rename would break the `import` inside the generated JS, which
/// is written by bindgen and refers to its own `.wasm` by name.
pub const WASM_CRATE_STEM: &str = "patina_web";

/// The files `app_name` exports to under `config`.
pub fn layout(app_name: &str, config: &WebExportConfig) -> WebExportLayout {
    let pck_stem = pck_stem(app_name);
    WebExportLayout {
        index_html: "index.html".to_string(),
        js: format!("{WASM_CRATE_STEM}.js"),
        wasm: format!("{WASM_CRATE_STEM}_bg.wasm"),
        pck: format!("{pck_stem}.pck"),
        extra: if config.pwa_enabled {
            vec![
                "manifest.webmanifest".to_string(),
                "sw.js".to_string(),
                "offline.html".to_string(),
            ]
        } else {
            Vec::new()
        },
    }
}

/// A filesystem-safe stem for the pack, derived from the app name.
///
/// Spaces and separators become `_` so the fetch URL needs no escaping, which
/// is one less thing to get wrong in a hand-edited shell.
fn pck_stem(app_name: &str) -> String {
    let stem: String = app_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if stem.is_empty() {
        "game".to_string()
    } else {
        stem
    }
}

/// The `res://` path `file` takes inside the pack, or `None` when it must not
/// be packed.
///
/// Exclusions, each for a reason that has bitten a Godot export:
/// - anything under `.godot/` — the editor's own cache, regenerated on open;
/// - `*.import` sidecars — metadata for the importer, not content the runtime
///   reads;
/// - dot-files and dot-directories — `.git`, `.DS_Store`, editor state;
/// - anything under the export's own `output_dir`, so exporting into the
///   project directory does not pack the previous export into the next one,
///   doubling in size every time.
pub fn res_path_for(project_root: &Path, file: &Path, config: &WebExportConfig) -> Option<String> {
    let relative = file.strip_prefix(project_root).ok()?;
    let output_dir = Path::new(&config.output_dir);

    if !config.output_dir.is_empty() && relative.starts_with(output_dir) {
        return None;
    }

    let mut parts = Vec::new();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            // `..` or a root escaping the project: refuse rather than guess.
            return None;
        };
        let name = name.to_str()?;
        if name.starts_with('.') {
            return None;
        }
        parts.push(name);
    }

    let name = parts.last()?;
    if name.ends_with(".import") {
        return None;
    }

    Some(format!("res://{}", parts.join("/")))
}

/// The HTML document for this export.
///
/// `config.html_shell` is used as the template when it names a file whose
/// contents the caller has already read *and* that text carries at least one
/// `{{PATINA_…}}` placeholder; otherwise the built-in shell is used. Both go
/// through the same substitution, so a custom shell cannot silently miss a
/// value the default one would have filled in.
///
/// Placeholders:
/// - `{{PATINA_APP_NAME}}`
/// - `{{PATINA_JS}}`, `{{PATINA_WASM}}`, `{{PATINA_PCK}}`
/// - `{{PATINA_HEAD_INCLUDE}}`
/// - `{{PATINA_INITIAL_MEMORY_MB}}`, `{{PATINA_MAX_MEMORY_MB}}`
/// - `{{PATINA_PRELOADER_DISPLAY}}` — `block` or `none`
/// - `{{PATINA_FOCUS_CANVAS}}` — `true` or `false`
/// - `{{PATINA_REQUIRE_ISOLATION}}` — `true` when threads are on
/// - `{{PATINA_REQUIRE_SIMD}}` — `true` when SIMD is on
///
/// ⚠️ Substitution is all a custom shell inherits. It does **not** inherit the
/// startup *obligations* the built-in shell discharges — hydrating `user://`
/// before the first frame, mounting the pack before a scene is asked for, and
/// registering the audio worklet off the handle `startWebAudio` returns. Each
/// of those fails silently when omitted: the page loads, runs, and is wrong
/// with nothing in the console. A custom shell that drops one gets no warning
/// from here, so copy the sequence out of [`DEFAULT_SHELL`] rather than writing
/// a template from scratch.
pub fn render_html_shell(
    app_name: &str,
    config: &WebExportConfig,
    layout: &WebExportLayout,
    custom_shell: Option<&str>,
) -> String {
    let template = match custom_shell {
        Some(text) if text.contains("{{PATINA_") => text,
        _ => DEFAULT_SHELL,
    };

    template
        .replace("{{PATINA_APP_NAME}}", &escape_html(app_name))
        .replace("{{PATINA_JS}}", &layout.js)
        .replace("{{PATINA_WASM}}", &layout.wasm)
        .replace("{{PATINA_PCK}}", &layout.pck)
        // Not escaped: the field exists so a project can inject analytics or a
        // font link, and escaping it would make that impossible. It is the
        // project's own text, same trust level as its shell.
        .replace("{{PATINA_HEAD_INCLUDE}}", &config.head_include)
        .replace(
            "{{PATINA_INITIAL_MEMORY_MB}}",
            &config.initial_memory_mb.to_string(),
        )
        .replace(
            "{{PATINA_MAX_MEMORY_MB}}",
            &config.max_memory_mb.to_string(),
        )
        .replace(
            "{{PATINA_PRELOADER_DISPLAY}}",
            if config.show_preloader {
                "block"
            } else {
                "none"
            },
        )
        .replace("{{PATINA_FOCUS_CANVAS}}", &config.focus_canvas.to_string())
        // Gates the service-worker registration in the shell. Registering
        // unconditionally would 404 on `sw.js` for a non-PWA export, which
        // shows up in the console of every ordinary build.
        .replace("{{PATINA_PWA_ENABLED}}", &config.pwa_enabled.to_string())
        .replace(
            "{{PATINA_REQUIRE_ISOLATION}}",
            &config.threads_enabled.to_string(),
        )
        .replace("{{PATINA_REQUIRE_SIMD}}", &config.simd_enabled.to_string())
}

/// The PWA files for this export, as `(filename, contents)`.
///
/// Empty unless [`WebExportConfig::pwa_enabled`] is set. The service worker
/// precaches exactly the files in `layout` — a worker that guessed would serve
/// a stale `.pck` against a fresh `.wasm`, which fails as a corrupt-looking
/// archive rather than as a cache miss.
pub fn pwa_files(
    app_name: &str,
    config: &WebExportConfig,
    layout: &WebExportLayout,
) -> Vec<(String, String)> {
    if !config.pwa_enabled {
        return Vec::new();
    }

    let precache: Vec<String> = layout
        .all()
        .into_iter()
        .filter(|f| f != "sw.js")
        .map(|f| format!("    \"./{f}\""))
        .collect();

    let manifest = format!(
        r##"{{
  "name": "{name}",
  "short_name": "{name}",
  "start_url": "./index.html",
  "display": "fullscreen",
  "orientation": "landscape",
  "background_color": "#000000",
  "theme_color": "#000000"
}}
"##,
        name = escape_json(app_name)
    );

    let service_worker = format!(
        r#"// Patina web export service worker (pat-u3l8c.114).
// Precaches exactly the files the export produced: a worker that cached by
// guesswork would eventually serve a stale .pck against a fresh .wasm, which
// surfaces as a corrupt archive rather than as a cache miss.
const CACHE = "patina-{stem}-v1";
const PRECACHE = [
{precache}
];

self.addEventListener("install", (event) => {{
  event.waitUntil(caches.open(CACHE).then((cache) => cache.addAll(PRECACHE)));
}});

self.addEventListener("activate", (event) => {{
  event.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k)))
    )
  );
}});

// ⚠️ Without this, `JavaScriptBridge.pwa_update()` is a no-op that LOOKS
// like it works: the shell posts skip-waiting, nothing listens, the waiting
// worker stays waiting, and the reload serves the very same version again
// (pat-mgixk.41).
self.addEventListener("message", (event) => {{
  if (event.data && event.data.type === "SKIP_WAITING") self.skipWaiting();
}});

self.addEventListener("fetch", (event) => {{
  event.respondWith(
    caches.match(event.request).then(
      (hit) => hit || fetch(event.request).catch(() => caches.match("./offline.html"))
    )
  );
}});
"#,
        stem = pck_stem(app_name),
        precache = precache.join(",\n")
    );

    let offline = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="utf-8"><title>{name} — offline</title></head>
<body style="background:#000;color:#eee;font:14px system-ui;text-align:center;padding:4rem">
<h1>{name}</h1>
<p>You are offline and this build is not cached yet. Reconnect and reload.</p>
</body>
</html>
"#,
        name = escape_html(app_name)
    );

    vec![
        ("manifest.webmanifest".to_string(), manifest),
        ("sw.js".to_string(), service_worker),
        ("offline.html".to_string(), offline),
    ]
}

/// The `RUSTFLAGS` a build needs to honour this config.
///
/// ⚠️ The memory and feature settings are **link-time**, not document-time:
/// `initial_memory_mb` / `max_memory_mb` become `--initial-memory` /
/// `--max-memory` link args, and SIMD/threads become `-C target-feature`. A
/// generated HTML shell cannot apply any of them, which is why this is the
/// consumer of those fields rather than [`render_html_shell`] — the fields are
/// a *build* setting that a previous reading of this config treated as page
/// configuration.
///
/// Both sizes are expressed in MB and wasm pages are 64 KiB, so every value is
/// page-aligned by construction. `max_memory_mb == 0` documents "unlimited
/// growth", so no `--max-memory` is emitted at all — passing 0 would cap the
/// heap at zero and fail the first allocation.
///
/// ⚠️ `threads_enabled` also needs a **nightly** toolchain with
/// `-Z build-std=std,panic_abort`, because the standard library has to be
/// rebuilt with atomics; the flags here are necessary but not sufficient.
/// `scripts/web-export.sh` says so when it refuses the build.
pub fn rustflags(config: &WebExportConfig) -> String {
    const BYTES_PER_MB: u64 = 1024 * 1024;
    let mut flags = vec![format!(
        "-C link-arg=--initial-memory={}",
        u64::from(config.initial_memory_mb) * BYTES_PER_MB
    )];
    if config.max_memory_mb > 0 {
        flags.push(format!(
            "-C link-arg=--max-memory={}",
            u64::from(config.max_memory_mb) * BYTES_PER_MB
        ));
    }

    let mut features: Vec<&str> = Vec::new();
    if config.simd_enabled {
        features.push("+simd128");
    }
    if config.threads_enabled {
        // All three are required together: atomics alone does not give you a
        // shared memory, and bulk-memory/mutable-globals are what the threaded
        // std expects.
        features.extend(["+atomics", "+bulk-memory", "+mutable-globals"]);
    }
    if !features.is_empty() {
        flags.push(format!("-C target-feature={}", features.join(",")));
    }

    flags.join(" ")
}

/// How to serve the folder this export produced, as text for the user.
///
/// ⚠️ Says plainly when `python -m http.server` is **not** enough: a threaded
/// export needs COOP/COEP headers that it cannot send, so the page would load
/// and then refuse to start. Better to read that here than to debug a blank
/// canvas.
pub fn serve_hint(config: &WebExportConfig) -> String {
    let dir = if config.output_dir.is_empty() {
        "export/web"
    } else {
        &config.output_dir
    };
    if config.threads_enabled {
        format!(
            "threads are enabled, so this export needs a cross-origin-isolated \
             origin:\n  Cross-Origin-Opener-Policy: same-origin\n  \
             Cross-Origin-Embedder-Policy: require-corp\n`python3 -m http.server` \
             sends neither, so use a server that can add headers (or disable \
             threads_enabled) and serve {dir}"
        )
    } else {
        format!("python3 -m http.server 8080 --directory {dir}")
    }
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn escape_json(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The built-in HTML shell.
///
/// It boots the bundle the way `crates/patina-web/web/index.html` does — the
/// hand-written harness that proved the sequence — and adds the two steps a
/// shipped export needs and the harness has no reason to: mounting the fetched
/// `.pck` over `res://`, and hydrating `user://` out of IndexedDB *before the
/// first frame*.
///
/// ⚠️ Both orderings matter and neither fails loudly if you get them wrong: a
/// script that reads `user://settings.cfg` on frame one before hydration
/// completes sees an empty store and overwrites it with defaults (the data is in
/// IndexedDB, it just arrived late — indistinguishable from "saving is
/// broken"), and a `res://` load before the pack is mounted falls through to a
/// filesystem the browser does not have.
pub const DEFAULT_SHELL: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{{PATINA_APP_NAME}}</title>
{{PATINA_HEAD_INCLUDE}}
<style>
  html, body { margin: 0; height: 100%; background: #000; color: #eee;
               font: 14px system-ui, sans-serif; overflow: hidden; }
  #patina-canvas { display: block; width: 100vw; height: 100vh; }
  #patina-preloader { position: fixed; inset: 0; display: {{PATINA_PRELOADER_DISPLAY}};
                      place-content: center; text-align: center; align-content: center; }
  #patina-error { position: fixed; inset: 0; display: none; padding: 2rem;
                  background: #200; color: #fbb; white-space: pre-wrap; }
</style>
</head>
<body>
<canvas id="patina-canvas" tabindex="0"></canvas>
<div id="patina-preloader"><div>{{PATINA_APP_NAME}}<br><small id="patina-status">loading…</small></div></div>
<div id="patina-error"></div>
<script type="module">
  import init, { WebVfs, startCanvasRenderer, startEngine, startWebAudio } from "./{{PATINA_JS}}";

  const status = (text) => {
    const el = document.getElementById("patina-status");
    if (el) el.textContent = text;
  };
  const fail = (err) => {
    const el = document.getElementById("patina-error");
    el.style.display = "block";
    el.textContent = String(err && err.stack ? err.stack : err);
    document.getElementById("patina-preloader").style.display = "none";
  };

  // A glue string is turned into a Blob URL and imported like any other ES
  // module — the same trick the audio worklet uses, so the wasm module needs no
  // JS import of its own.
  const importSource = (source) =>
    import(URL.createObjectURL(new Blob([source], { type: "text/javascript" })));

  async function main() {
    if ({{PATINA_REQUIRE_ISOLATION}} && !self.crossOriginIsolated) {
      throw new Error(
        "this build needs SharedArrayBuffer, so the page must be cross-origin isolated: " +
        "serve it with Cross-Origin-Opener-Policy: same-origin and " +
        "Cross-Origin-Embedder-Policy: require-corp"
      );
    }
    if ({{PATINA_REQUIRE_SIMD}} && typeof WebAssembly.validate === "function") {
      // 0x0b 0xfd 0x0c = v128.const, the shortest SIMD-only body.
      const simd = WebAssembly.validate(new Uint8Array([
        0, 97, 115, 109, 1, 0, 0, 0, 1, 4, 1, 96, 0, 0, 3, 2, 1, 0, 10, 10, 1,
        8, 0, 253, 12, 0, 0, 0, 0, 0, 11,
      ]));
      if (!simd) throw new Error("this build uses wasm SIMD, which this browser does not support");
    }

    status("loading engine…");
    await init();

    // user:// first: hydration has to finish before the first frame, or a
    // script reading its settings on frame one overwrites them with defaults.
    status("loading saved data…");
    const vfs = new WebVfs();
    const store = await importSource(vfs.userStoreGlue);
    const db = await store.openUserStore();
    await store.hydrateUserStore(db, vfs);

    // then res://: mounted before anything can ask for a scene.
    status("loading assets…");
    const packed = await fetch("./{{PATINA_PCK}}");
    if (!packed.ok) throw new Error(`fetching {{PATINA_PCK}} failed: ${packed.status} ${packed.statusText}`);
    vfs.mountPack("game", new Uint8Array(await packed.arrayBuffer()), 0);

    status("starting…");
    const renderer = await startCanvasRenderer("patina-canvas");
    const engine = startEngine("patina-canvas");
    // Audio is two halves. startWebAudio only builds the AudioContext and the
    // mixer; the AudioWorkletProcessor comes back as a source string that has
    // to be imported and registered, or the mixer is never pulled from and the
    // page is silent. The gesture unlock is the other half of that: a context
    // created before any user input starts suspended.
    //
    // Both callbacks below reach &mut methods on the same handle (markUnlocked,
    // pullBlock), so wasm-bindgen would panic with a BorrowMutError if either
    // were still borrowed when the other ran. They cannot be: each body is
    // synchronous and runs to completion as its own task on a single-threaded
    // event loop, and `pullBlock(renderQuantum)` reads the quantum through an
    // &self getter that has already returned before the &mut call borrows.
    // ⚠️ Keep them synchronous — an `await` inside either one re-introduces the
    // overlap, and it surfaces as audio that works until suddenly it does not.
    try {
      const audio = startWebAudio(44100);
      const audioGlue = await importSource(audio.workletGlue);
      audioGlue.patinaInstallGestureUnlock(audio.context, () => audio.markUnlocked());
      await audioGlue.patinaRegisterWorklet(audio.context, () =>
        audio.pullBlock(audio.renderQuantum),
      );
    } catch (err) { console.warn("audio unavailable:", err); }

    document.getElementById("patina-preloader").style.display = "none";
    if ({{PATINA_FOCUS_CANVAS}}) document.getElementById("patina-canvas").focus();

    // Persistence runs on the same cadence as the frame loop: the engine's
    // writes are already in memory, this is only the IndexedDB catch-up.
    setInterval(() => { store.drainPendingWrites(db, vfs).catch(console.warn); }, 1000);

    // -- JavaScriptBridge shell hooks (pat-mgixk.41) --
    //
    // `gdplatform::js_bridge` looks these up on `globalThis` BY NAME:
    // `__patina_fs_sync` and `__patina_pwa_update` as zero-argument functions,
    // `__patina_pwa_update_pending` as a boolean. Nothing defined them, so
    // `JavaScriptBridge.force_fs_sync()`, `.pwa_needs_update()` and
    // `.pwa_update()` were all silent no-ops in an exported game.
    //
    // Installed here, inside main(), because the fs-sync hook needs the `store`,
    // `db` and `vfs` locals created above.

    // Godot's `force_fs_sync` flushes IDBFS; the equivalent here is to run the
    // write-back now instead of waiting for the 1s interval above.
    globalThis.__patina_fs_sync = () => {
      store.drainPendingWrites(db, vfs).catch(console.warn);
    };

    // Explicitly false rather than left undefined: `pwa_needs_update()` treats
    // absent as "no update", so this only documents the starting state — but it
    // also means a non-PWA export answers the same as Godot does.
    globalThis.__patina_pwa_update_pending = false;

    if ({{PATINA_PWA_ENABLED}} && "serviceWorker" in navigator) {
      try {
        const reg = await navigator.serviceWorker.register("./sw.js");
        const markPending = () => { globalThis.__patina_pwa_update_pending = true; };

        // ⚠️ An ALREADY-waiting worker counts as an available update.
        // `updatefound` fired before this page loaded and will not fire again,
        // so an implementation that only listened for the event would miss the
        // most common real case: reloading after the worker updated in the
        // background.
        if (reg.waiting) markPending();

        reg.addEventListener("updatefound", () => {
          const installing = reg.installing;
          if (!installing) { markPending(); return; }
          // ⚠️ Only once it is actually installed. Flagging on `updatefound`
          // alone would leave the flag stuck true for an update that FAILED to
          // install, and `pwa_update()` would then reload into the same
          // version on every call. `controller` distinguishes an update from
          // this page's very first install, which is not an "update".
          installing.addEventListener("statechange", () => {
            if (installing.state === "installed" && navigator.serviceWorker.controller) {
              markPending();
            }
          });
        });

        globalThis.__patina_pwa_update = () => {
          // The worker only steps aside if it is listening for this; the
          // generated sw.js has a `message` handler calling skipWaiting().
          if (reg.waiting) reg.waiting.postMessage({ type: "SKIP_WAITING" });
          globalThis.__patina_pwa_update_pending = false;
          location.reload();
        };
      } catch (err) { console.warn("service worker unavailable:", err); }
    }

    void renderer;
    void engine;
  }

  main().catch(fail);
</script>
</body>
</html>
"#;

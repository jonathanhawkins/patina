//! The web export produces a folder a static server can serve (pat-u3l8c.114).
//!
//! The bead's acceptance is "one command produces a folder that
//! `python -m http.server` serves as a playable game; documented flow". The
//! command and the IO are `scripts/web-export.sh` and
//! `engine-rs/examples/web_export.rs`; everything a test can be wrong about —
//! which files exist, what the shell says, and which project files go into the
//! pack — is in `gdplatform::web_export`, and this is where it is pinned.
//!
//! What these tests are really guarding is that
//! [`gdplatform::web::WebExportConfig`] has **consumers**. Before this it had
//! nine fields that nothing read: a project could set `threads_enabled`,
//! `pwa_enabled` or a custom `html_shell` and the export would ignore all
//! three. Every assertion below ties one of those fields to something
//! observable in the output.

use gdplatform::web::WebExportConfig;
use gdplatform::web_export::{
    layout, pwa_files, render_html_shell, res_path_for, rustflags, serve_hint, DEFAULT_SHELL,
};
use std::path::Path;

fn config() -> WebExportConfig {
    WebExportConfig::default()
}

// ---------------------------------------------------------------------------
// The folder
// ---------------------------------------------------------------------------

/// A servable export is four files, and the document has to reference the other
/// three — a `.wasm` on its own is what the export used to produce, and a
/// browser cannot do anything with it.
#[test]
fn the_export_is_a_document_that_references_the_bundle_and_the_pack() {
    let config = config();
    let plan = layout("My Game", &config);

    assert_eq!(
        plan.index_html, "index.html",
        "a static server serves index.html for the directory URL"
    );
    assert_eq!(
        plan.pck, "My_Game.pck",
        "spaces must not survive into a fetch URL"
    );
    assert!(plan.js.ends_with(".js"));
    assert!(plan.wasm.ends_with(".wasm"));
    assert_eq!(
        plan.all().len(),
        4,
        "no PWA files unless asked: {:?}",
        plan.all()
    );

    let html = render_html_shell("My Game", &config, &plan, None);
    for referenced in [&plan.js, &plan.pck] {
        assert!(
            html.contains(referenced.as_str()),
            "the shell must reference `{referenced}`, or the file it ships beside is dead weight"
        );
    }
    // The `.wasm` is deliberately NOT referenced here: bindgen's own JS loads
    // it by the name bindgen chose, which is why the export does not rename it.
}

/// The bindgen stem is fixed, not derived from the app name: bindgen writes the
/// `.wasm` name into the JS it generates, so renaming the artifacts would break
/// the import inside a file the export does not author.
#[test]
fn the_bundle_keeps_the_name_bindgen_gave_it() {
    let plan = layout("Totally Different Title", &config());
    assert_eq!(plan.js, "patina_web.js");
    assert_eq!(plan.wasm, "patina_web_bg.wasm");
}

/// An app name with nothing usable in it still produces a fetchable pack.
#[test]
fn an_unusable_app_name_falls_back_to_a_safe_pack_name() {
    assert_eq!(layout("", &config()).pck, "game.pck");
    assert_eq!(layout("///", &config()).pck, "___.pck");
}

// ---------------------------------------------------------------------------
// Every WebExportConfig field has a consumer
// ---------------------------------------------------------------------------

/// `initial_memory_mb` / `max_memory_mb` are link-time settings, so their
/// consumer is the build flags, not the document. 64 MB and 512 MB in wasm
/// pages of 64 KiB.
#[test]
fn the_memory_budget_reaches_the_build_flags() {
    let mut config = config();
    config.initial_memory_mb = 64;
    config.max_memory_mb = 512;

    let flags = rustflags(&config);
    assert!(
        flags.contains("--initial-memory=67108864"),
        "initial_memory_mb must become a link arg, got {flags:?}"
    );
    assert!(
        flags.contains("--max-memory=536870912"),
        "max_memory_mb must become a link arg, got {flags:?}"
    );
}

/// `max_memory_mb == 0` documents unlimited growth, so no cap may be emitted —
/// passing 0 through would cap the heap at zero and fail the first allocation.
#[test]
fn unlimited_growth_emits_no_max_memory() {
    let mut config = config();
    config.max_memory_mb = 0;
    let flags = rustflags(&config);
    assert!(flags.contains("--initial-memory="));
    assert!(
        !flags.contains("--max-memory"),
        "0 means unlimited, not a zero cap, got {flags:?}"
    );
}

/// SIMD and threads are target features, and threads needs all three of the
/// features the threaded std expects — atomics alone does not give a shared
/// memory.
#[test]
fn simd_and_threads_become_target_features() {
    let mut config = config();
    config.simd_enabled = true;
    config.threads_enabled = false;
    let simd_only = rustflags(&config);
    assert!(simd_only.contains("-C target-feature=+simd128"));
    assert!(!simd_only.contains("+atomics"));

    config.threads_enabled = true;
    let threaded = rustflags(&config);
    for feature in ["+simd128", "+atomics", "+bulk-memory", "+mutable-globals"] {
        assert!(
            threaded.contains(feature),
            "a threaded build needs {feature}, got {threaded:?}"
        );
    }

    config.simd_enabled = false;
    let no_simd = rustflags(&config);
    assert!(
        !no_simd.contains("simd128"),
        "simd_enabled=false must drop the feature"
    );
}

/// `show_preloader` and `focus_canvas` are the two cosmetic switches, and both
/// are observable.
#[test]
fn the_preloader_and_focus_switches_are_observable() {
    let mut config = config();
    config.show_preloader = true;
    config.focus_canvas = true;
    let plan = layout("Game", &config);
    let shown = render_html_shell("Game", &config, &plan, None);
    assert!(
        shown.contains("display: block"),
        "the preloader must be visible"
    );
    assert!(
        shown.contains("if (true) document.getElementById(\"patina-canvas\").focus()"),
        "focus_canvas must reach the boot script"
    );

    config.show_preloader = false;
    config.focus_canvas = false;
    let hidden = render_html_shell("Game", &config, &plan, None);
    assert!(
        hidden.contains("display: none"),
        "the preloader must be hidden"
    );
    assert!(
        hidden.contains("if (false) document.getElementById(\"patina-canvas\").focus()"),
        "focus_canvas=false must reach the boot script too"
    );
}

/// `head_include` is injected verbatim — it exists so a project can add a font
/// link or an analytics snippet, which escaping would defeat.
#[test]
fn head_include_is_injected_verbatim() {
    let mut config = config();
    config.head_include = "<link rel=\"stylesheet\" href=\"theme.css\">".to_string();
    let plan = layout("Game", &config);
    let html = render_html_shell("Game", &config, &plan, None);

    assert!(
        html.contains("<link rel=\"stylesheet\" href=\"theme.css\">"),
        "head_include must survive unescaped"
    );
}

/// The app name, by contrast, IS escaped: it is a project setting that ends up
/// in a title and a visible label.
#[test]
fn the_app_name_is_escaped_in_the_document() {
    let config = config();
    let plan = layout("Spiky <Game>", &config);
    let html = render_html_shell("Spiky <Game>", &config, &plan, None);

    assert!(
        html.contains("Spiky &lt;Game&gt;"),
        "the title must be escaped"
    );
    assert!(
        !html.contains("Spiky <Game>"),
        "an unescaped app name would let a project setting inject markup"
    );
}

/// `threads_enabled` is the one setting that changes how the export must be
/// *served*, and it shows up in both the shell and the instructions.
#[test]
fn threads_require_isolation_in_the_shell_and_say_so_in_the_hint() {
    let mut config = config();
    config.threads_enabled = true;
    let plan = layout("Game", &config);
    let html = render_html_shell("Game", &config, &plan, None);

    assert!(
        html.contains("crossOriginIsolated"),
        "a threaded build must check isolation itself rather than failing inside wasm"
    );
    assert!(
        html.contains("if (true && !self.crossOriginIsolated)"),
        "the isolation check must be switched on by threads_enabled, got:\n{}",
        html.lines()
            .find(|l| l.contains("crossOriginIsolated"))
            .unwrap_or("<missing>")
    );

    let hint = serve_hint(&config);
    assert!(
        hint.contains("Cross-Origin-Opener-Policy")
            && hint.contains("Cross-Origin-Embedder-Policy"),
        "the hint must name both headers, got {hint:?}"
    );
    assert!(
        hint.contains("sends neither"),
        "the hint must say python's server cannot serve a threaded export, got {hint:?}"
    );
}

/// Without threads, the documented flow is exactly the bead's acceptance
/// command and nothing more.
#[test]
fn the_default_flow_is_the_one_line_the_acceptance_asks_for() {
    let hint = serve_hint(&config());
    assert!(
        hint.starts_with("python3 -m http.server"),
        "the default hint must be the plain static-server command, got {hint:?}"
    );
    assert!(
        hint.contains("export/web"),
        "it must name the output directory"
    );
}

/// `simd_enabled` gates a feature check, so an unsupported browser gets a
/// sentence instead of a wasm validation error.
#[test]
fn simd_is_feature_detected_when_enabled() {
    let mut config = config();
    config.simd_enabled = true;
    let plan = layout("Game", &config);
    assert!(
        render_html_shell("Game", &config, &plan, None).contains("WebAssembly.validate"),
        "a SIMD build must feature-detect SIMD"
    );
}

/// `pwa_enabled` adds exactly three files, and the worker precaches the real
/// filenames — a worker that guessed would eventually pair a stale `.pck` with
/// a fresh `.wasm`, which reads as a corrupt archive rather than a cache miss.
#[test]
fn the_pwa_set_precaches_the_files_that_were_actually_produced() {
    let mut config = config();
    assert!(pwa_files("Game", &config, &layout("Game", &config)).is_empty());

    config.pwa_enabled = true;
    let plan = layout("Game", &config);
    assert_eq!(
        plan.extra,
        vec![
            "manifest.webmanifest".to_string(),
            "sw.js".to_string(),
            "offline.html".to_string()
        ]
    );

    let files = pwa_files("Game", &config, &plan);
    let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, vec!["manifest.webmanifest", "sw.js", "offline.html"]);

    let worker = &files[1].1;
    for precached in [&plan.index_html, &plan.js, &plan.wasm, &plan.pck] {
        assert!(
            worker.contains(&format!("\"./{precached}\"")),
            "the service worker must precache `{precached}`"
        );
    }
    assert!(
        !worker.contains("\"./sw.js\""),
        "a worker that precaches itself pins its own old version"
    );
    assert!(
        worker.contains("offline.html"),
        "the fetch fallback must be the offline page"
    );
}

// ---------------------------------------------------------------------------
// A custom shell
// ---------------------------------------------------------------------------

/// A project-supplied shell goes through the same substitution as the built-in
/// one, so it cannot silently miss a value.
#[test]
fn a_custom_shell_gets_the_same_substitutions() {
    let config = config();
    let plan = layout("Game", &config);
    let custom = "<html><body><h1>{{PATINA_APP_NAME}}</h1>\
                  <script type=\"module\">import init from \"./{{PATINA_JS}}\";\
                  fetch(\"./{{PATINA_PCK}}\");</script></body></html>";

    let html = render_html_shell("Game", &config, &plan, Some(custom));

    assert!(html.contains("<h1>Game</h1>"));
    assert!(html.contains("./patina_web.js"));
    assert!(html.contains("./Game.pck"));
    assert!(
        !html.contains("{{PATINA_"),
        "no placeholder may survive into the served document: {html}"
    );
}

/// Text that is not a shell template is ignored rather than served. The field
/// defaults to the string `"default_shell.html"`, a *filename*, so treating any
/// non-empty value as a template would ship that literal text as the page.
#[test]
fn a_shell_without_placeholders_is_not_used() {
    let config = config();
    let plan = layout("Game", &config);
    let html = render_html_shell("Game", &config, &plan, Some("default_shell.html"));

    assert!(
        html.contains("patina-canvas"),
        "the built-in shell must be used instead"
    );
    assert!(!html.contains("default_shell.html"));
}

/// Nothing may reach the browser with a placeholder still in it, including the
/// built-in shell — the template and the substitution list are edited in
/// different places and drift silently.
#[test]
fn the_builtin_shell_has_no_unsubstituted_placeholders() {
    let mut config = config();
    config.pwa_enabled = true;
    config.threads_enabled = true;
    let plan = layout("Game", &config);
    let html = render_html_shell("Game", &config, &plan, None);

    assert!(
        !html.contains("{{PATINA_"),
        "unsubstituted placeholder in the built-in shell: {:?}",
        html.lines().find(|l| l.contains("{{PATINA_"))
    );
    assert!(
        DEFAULT_SHELL.contains("{{PATINA_"),
        "the template itself must still be a template"
    );
}

/// The shell must drive the web VFS, which is what makes the folder *playable*
/// rather than merely served: mount the pack before any `res://` load, and
/// hydrate `user://` before the first frame.
#[test]
fn the_shell_drives_the_vfs_in_the_order_that_matters() {
    let config = config();
    let plan = layout("Game", &config);
    let html = render_html_shell("Game", &config, &plan, None);

    for needle in [
        "new WebVfs()",
        "vfs.userStoreGlue",
        "openUserStore()",
        "hydrateUserStore(db, vfs)",
        "vfs.mountPack(",
        "drainPendingWrites(db, vfs)",
    ] {
        assert!(html.contains(needle), "the shell must call `{needle}`");
    }

    let hydrate = html.find("hydrateUserStore(db, vfs)").expect("hydrate");
    let mount = html.find("vfs.mountPack(").expect("mount");
    let engine = html.find("startEngine(").expect("engine start");
    assert!(
        hydrate < engine,
        "user:// must be hydrated before the first frame, or a script reading its \
         settings on frame one overwrites them with defaults"
    );
    assert!(
        mount < engine,
        "the pack must be mounted before the engine can ask for a scene"
    );
}

/// The shell must do both halves of audio startup.
///
/// Regression guard (reported by ScarletRidge, msg 88246): the shell shipped as
/// `try { startWebAudio(44100); } catch ...`, discarding the handle. That call
/// only builds the `AudioContext` and the mixer — `start_web_audio` deliberately
/// does not load the processor, it hands the source back as `workletGlue` for
/// the page to import and register. With the handle dropped, nothing ever pulls
/// from the mixer and the exported page is silent, while every assertion in this
/// file still passed: they covered layout and naming only.
///
/// The names here are the live bindings in `patina-web::web_audio` and the glue
/// in `gdaudio::web_output`, which is why this is an exact-string check — a
/// shell that calls a plausible-but-absent method fails at runtime, in a page,
/// with no test able to see it.
#[test]
fn the_shell_registers_the_audio_worklet_and_not_just_the_context() {
    let config = config();
    let plan = layout("Game", &config);
    let html = render_html_shell("Game", &config, &plan, None);

    for needle in [
        "startWebAudio(44100)",
        "audio.workletGlue",
        "patinaRegisterWorklet(",
        "patinaInstallGestureUnlock(",
        "audio.pullBlock(audio.renderQuantum)",
    ] {
        assert!(
            html.contains(needle),
            "the shell must call `{needle}`, or the export has no audio"
        );
    }

    let start = html.find("startWebAudio(44100)").expect("audio start");
    let register = html
        .find("patinaRegisterWorklet(")
        .expect("worklet registration");
    assert!(
        start < register,
        "the worklet source comes off the handle startWebAudio returns, so the \
         registration cannot precede the call"
    );

    // The handle has to be *bound*. The original defect was the bare statement
    // `try { startWebAudio(44100); }`, which reads as working code; an absence
    // check on that spelling is useless here, because the correct form contains
    // it as a substring. The binding is the discriminating evidence.
    assert!(
        html.contains("= startWebAudio(44100)"),
        "the shell discards the audio handle — `workletGlue` is unreachable from \
         a dropped return value, and the page goes silent with no error"
    );

    // A context that was never resumed is suspended, which is silence that
    // looks like success: the worklet registers and the graph runs.
    assert!(
        html.contains("markUnlocked()"),
        "the gesture unlock must mark the context unlocked, or audio stays \
         suspended until a gesture that nothing is listening for"
    );
}

/// A fetch that 404s has to say so. The common failure of a static deployment
/// is a missing `.pck`, and an unchecked `arrayBuffer()` of an error page
/// surfaces as "not a pck" from deep inside the mount instead.
#[test]
fn a_missing_pack_is_reported_at_the_fetch() {
    let config = config();
    let plan = layout("Game", &config);
    let html = render_html_shell("Game", &config, &plan, None);
    assert!(
        html.contains("packed.ok"),
        "the pack fetch must check its status"
    );
}

// ---------------------------------------------------------------------------
// What goes into the pack
// ---------------------------------------------------------------------------

/// Project files map to `res://` paths, with separators normalized.
#[test]
fn project_files_map_to_res_paths() {
    let config = config();
    let root = Path::new("/proj");

    assert_eq!(
        res_path_for(root, Path::new("/proj/main.tscn"), &config).as_deref(),
        Some("res://main.tscn")
    );
    assert_eq!(
        res_path_for(root, Path::new("/proj/art/sprites/hero.png"), &config).as_deref(),
        Some("res://art/sprites/hero.png")
    );
}

/// The editor's cache, importer sidecars and dot-files stay out — and so does
/// the export's own output directory, because exporting into the project would
/// otherwise pack the previous export into the next one and double its size
/// every run.
#[test]
fn the_pack_excludes_what_must_not_ship() {
    let mut config = config();
    config.output_dir = "export/web".to_string();
    let root = Path::new("/proj");

    for excluded in [
        "/proj/.godot/uid_cache.bin",
        "/proj/art/hero.png.import",
        "/proj/.gitignore",
        "/proj/.DS_Store",
        "/proj/export/web/index.html",
        "/proj/export/web/Game.pck",
    ] {
        assert_eq!(
            res_path_for(root, Path::new(excluded), &config),
            None,
            "`{excluded}` must not be packed"
        );
    }

    // A path outside the project is refused rather than guessed at.
    assert_eq!(
        res_path_for(root, Path::new("/elsewhere/x.tscn"), &config),
        None
    );
}

/// With no output directory configured the exclusion must not swallow the whole
/// project — an empty prefix matches every path.
#[test]
fn an_empty_output_dir_excludes_nothing() {
    let mut config = config();
    config.output_dir = String::new();
    assert_eq!(
        res_path_for(Path::new("/proj"), Path::new("/proj/main.tscn"), &config).as_deref(),
        Some("res://main.tscn"),
        "an empty output_dir must not be treated as a prefix of everything"
    );
}

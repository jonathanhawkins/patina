//! Boot a patina-runner project from a `project.godot` file (pat-nzgdu.17).
//!
//! Godot boots a project by parsing `project.godot`: it resolves
//! `application/run/main_scene`, registers `[autoload]` scene/script singletons
//! under `/root`, installs the `[input]` action map, and populates
//! [`ProjectSettings`] (physics/rendering/audio) that the runtime honors.
//!
//! Patina's runner previously took a raw `.tscn` path and ignored
//! `project.godot` entirely; the parsers existed but had no runtime caller and
//! `gdcore::set_path_roots` was never invoked, so `res://` / `user://` could not
//! resolve. This module implements that boot path:
//!
//! 1. parse `project.godot` ([`ProjectLoader`] for scene/autoload/input,
//!    [`ProjectSettings`] for config),
//! 2. call [`gdcore::set_path_roots`] so `FileAccess` `res://` works,
//! 3. instantiate `[autoload]` singletons under `/root` (before the main scene),
//! 4. resolve and load `run/main_scene`,
//! 5. build the `[input]` action map.

use std::path::{Path, PathBuf};

use gdcore::file_access::FileAccess;
use gdcore::{set_path_roots, ProjectSettings};
use gdplatform::InputMap;
use gdresource::{AutoloadEntry, ProjectLoader};
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::{add_packed_scene_to_tree, Node, NodeId, PackedScene, SceneTree};
use gdvariant::Variant;

/// A project booted from `project.godot`: the populated scene tree plus the
/// [`ProjectSettings`] and [`InputMap`] the runtime should honor.
pub struct BootedProject {
    /// The scene tree with autoloads (under `/root`) and the main scene instanced.
    pub tree: SceneTree,
    /// Settings parsed from `project.godot` (physics/rendering/audio/...).
    pub settings: ProjectSettings,
    /// The `InputMap` actions declared in the `[input]` section.
    pub input_map: InputMap,
    /// Root node of the instanced main scene, if `run/main_scene` was set.
    pub main_scene_root: Option<NodeId>,
    /// `(autoload_name, node_id)` for each instanced autoload, in declared order.
    pub autoload_roots: Vec<(String, NodeId)>,
}

/// Boots the project rooted at `project_dir` (which must contain `project.godot`).
pub fn boot_project(project_dir: &Path) -> Result<BootedProject, String> {
    boot_project_with_user_root(project_dir, None)
}

/// Boots an exported game from a Godot `.pck`, the way Godot boots a shipped
/// build (pat-0q2kx.134).
///
/// The pack is mounted over `res://` before anything is parsed, so
/// `project.godot`, the main scene, the autoloads and every texture they
/// reference all resolve out of the archive — the loose project directory does
/// not need to exist. `res://` still points at the pack's own directory, so a
/// file deliberately shipped beside the pack keeps resolving.
///
/// `user_dir` maps `user://` exactly as in [`boot_project_with_user_root`].
pub fn boot_project_from_pck(
    pck_path: &Path,
    user_dir: Option<PathBuf>,
) -> Result<BootedProject, String> {
    let bytes = std::fs::read(pck_path)
        .map_err(|e| format!("failed to read pack '{}': {e}", pck_path.display()))?;
    let mount = gdresource::pck::PckMount::from_bytes(bytes)
        .map_err(|e| format!("'{}' is not a valid .pck: {e}", pck_path.display()))?;

    // `res://` falls back to the pack's directory for anything not packed.
    let fallback_root = pck_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    set_path_roots(Some(fallback_root.clone()), user_dir.clone());

    let label = pck_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "game".to_string());
    gdcore::file_access::mount_pack(label, std::sync::Arc::new(mount), 0);

    // `res://project.binary` FIRST, `project.godot` only as a fallback
    // (pat-mgixk.56).
    //
    // 🔑 A real Godot export does not ship `project.godot` at all — it writes
    // the binary form instead (`editor_export_platform.cpp:1676`), and the
    // engine prefers it (`project_settings.cpp:993-1002`,
    // `_load_settings_text_or_binary`). Requiring the text file meant only a
    // pack assembled by hand from a loose project could boot; a genuinely
    // exported `.pck` failed here before any scene was touched.
    //
    // The decoded settings are rendered back to project.godot text and handed to
    // the same `boot_from_project_text` the loose-directory path uses, so the
    // main scene, autoloads, input map and ProjectSettings all keep coming from
    // one code path rather than growing a second boot sequence that drifts.
    let content = match FileAccess::read_file_bytes("res://project.binary") {
        Ok(bytes) => {
            let decoded = gdresource::ecfg::parse_project_binary(&bytes).map_err(|e| {
                format!(
                    "pack '{}' has a res://project.binary that will not decode: {e}",
                    pck_path.display()
                )
            })?;
            // Named, not swallowed: `SettingsValue` has no Dictionary or
            // PackedStringArray, so the input map and `application/config/features`
            // land here. Silence would surface much later as a missing action.
            if !decoded.skipped.is_empty() {
                tracing::warn!(
                    count = decoded.skipped.len(),
                    keys = ?decoded.skipped,
                    "project.binary: entries with no ProjectSettings representation"
                );
            }
            decoded.settings.to_godot_format()
        }
        Err(binary_err) => FileAccess::read_file_string("res://project.godot").map_err(|e| {
            format!(
                "pack '{}' has neither a readable res://project.binary ({binary_err}) nor \
                 res://project.godot ({e}) — an exported pack ships the former, a \
                 hand-packed project directory the latter",
                pck_path.display()
            )
        })?,
    };
    boot_from_project_text(&fallback_root, &content, user_dir)
}

/// Like [`boot_project`], but with an explicit `user://` root (e.g. the platform
/// application-data directory). Passing `None` leaves `user://` unmapped.
pub fn boot_project_with_user_root(
    project_dir: &Path,
    user_dir: Option<PathBuf>,
) -> Result<BootedProject, String> {
    let project_root = project_dir.to_path_buf();
    let content = std::fs::read_to_string(project_root.join("project.godot")).map_err(|e| {
        format!(
            "failed to load project.godot from {}: {e}",
            project_root.display()
        )
    })?;
    // `res://` must resolve before the shared body loads scenes.
    set_path_roots(Some(project_root.clone()), user_dir.clone());
    boot_from_project_text(&project_root, &content, user_dir)
}

/// Splits a `.tscn`-style string list into its paths.
///
/// `ProjectSettings` has no array kind, so `translations=PackedStringArray("a",
/// "b")` reaches us as the **raw literal text, wrapper and all**. Splitting that
/// on commas directly would yield `PackedStringArray("a"` and ` "b")` — paths
/// that resolve to nothing — so the constructor wrapper is stripped first.
/// `Array[String](…)` and a bare `[…]` are accepted for the same reason.
fn parse_res_path_list(raw: &str) -> Vec<String> {
    let mut s = raw.trim();
    for prefix in ["PackedStringArray(", "Array[String](", "["] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest
                .trim_end()
                .strip_suffix(if prefix == "[" { "]" } else { ")" })
                .unwrap_or(rest);
            break;
        }
    }
    // A nested `Array[String]([…])` leaves one more bracket layer behind.
    let s = s
        .trim()
        .strip_prefix('[')
        .and_then(|r| r.strip_suffix(']'))
        .unwrap_or(s);

    s.split(',')
        .map(|p| p.trim().trim_matches('"').trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// Loads `internationalization/locale/translations` into the engine-wide
/// [`TranslationServer`](gdcore::translation::TranslationServer) (pat-0q2kx.147).
///
/// Godot's setting is a `PackedStringArray` of `res://` paths to `.po` files or
/// CSV-imported `.translation` resources. A `.po` names exactly one locale; a
/// `.csv` carries one column per locale and so yields several catalogs.
///
/// The locale comes from `internationalization/locale/test` when set (Godot
/// uses it to force a locale for testing), else `.../locale/fallback`.
/// Unreadable or unparseable entries are skipped rather than aborting the boot:
/// Godot logs and continues, and a missing translation degrades to the key
/// itself rather than to no game.
/// Builds a catalog from a compressed `.translation` Resource (pat-s3rrv.213).
///
/// Property names transcribed from `OptimizedTranslation::_set`
/// (`upstream/godot/core/string/optimized_translation.cpp`): `hash_table` and
/// `bucket_table` are `PACKED_INT32_ARRAY`, `strings` is `PACKED_BYTE_ARRAY`.
///
/// ⚠️ `load_from` also appears in `_get_property_list`, but it is
/// `PROPERTY_USAGE_EDITOR` and write-only — it triggers `generate()` and is
/// never persisted, so there is nothing to read there.
fn optimized_translation_from_resource(
    res: &gdresource::resource::Resource,
    rel_path: &str,
) -> Option<gdcore::translation::Translation> {
    // ⚠️ Accept the Godot 3 spelling too: `renames_map_3_to_4.cpp:1557` maps
    // `PHashTranslation` -> `OptimizedTranslation`, so a project upgraded from
    // 3.x can still carry the old class name in its imported artifact.
    if res.class_name != "OptimizedTranslation" && res.class_name != "PHashTranslation" {
        return None;
    }

    // 🔑 `as u32`, NOT a checked conversion. Godot declares these as
    // `Vector<int>` and reinterprets the buffer as `uint32_t*`, so the
    // empty-slot sentinel `0xFFFFFFFF` arrives here as `-1`. A `try_from` would
    // reject exactly the sentinel and turn every empty hash slot into a load
    // failure.
    let ints = |key: &str| match res.get_property(key) {
        Some(gdvariant::Variant::PackedInt32Array(v)) => {
            Some(v.iter().map(|i| *i as u32).collect::<Vec<u32>>())
        }
        _ => None,
    };

    let hash_table = ints("hash_table")?;
    let bucket_table = ints("bucket_table")?;
    let strings = match res.get_property("strings") {
        Some(gdvariant::Variant::PackedByteArray(b)) => b.clone(),
        _ => return None,
    };
    if hash_table.is_empty() {
        // Nothing can be looked up; treat as unusable rather than as an empty
        // catalog that would look like a successful load.
        return None;
    }

    let locale = match res.get_property("locale") {
        Some(gdvariant::Variant::String(s)) if !s.is_empty() => s.clone(),
        _ => locale_from_artifact_name(rel_path),
    };

    Some(gdcore::translation::Translation::from_optimized(
        locale,
        gdcore::optimized_translation::OptimizedTranslation {
            locale: String::new(),
            hash_table,
            bucket_table,
            strings,
        },
    ))
}

/// Builds a catalog from a loaded `.translation` Resource (pat-s3rrv.139).
///
/// Godot's `Translation` stores its catalog in a **Dictionary** property named
/// `messages` (`core/string/translation.cpp:74`, `:276`) and its locale in a
/// `locale` String (`:277`).
///
/// ⚠️ Only the plain-`String` keys are read — those are Godot's own "old
/// version, no context or plural support" case (`translation.cpp:79-85`). A key
/// of any other type encodes a msgctxt or plural form, and guessing at its
/// shape would put wrong strings on screen; those entries are skipped rather
/// than approximated.
///
/// ⚠️ `OptimizedTranslation` is a different layout entirely — compressed hash
/// and bucket tables, no `messages` — so it yields `None` here rather than an
/// empty catalog that looks like a successful load.
fn translation_from_resource(
    res: &gdresource::resource::Resource,
    rel_path: &str,
) -> Option<gdcore::translation::Translation> {
    let messages = match res.get_property("messages") {
        Some(gdvariant::Variant::Dictionary(d)) => d,
        _ => return None,
    };

    // The resource's own locale wins; Godot's importer also encodes it in the
    // artifact name (`x.en.translation`), which is the fallback.
    let locale = match res.get_property("locale") {
        Some(gdvariant::Variant::String(s)) if !s.is_empty() => s.clone(),
        _ => locale_from_artifact_name(rel_path),
    };

    let mut t = gdcore::translation::Translation::new(locale);
    for (key, value) in messages.to_dict().iter() {
        if let (gdvariant::Variant::String(k), gdvariant::Variant::String(v)) = (key, value) {
            t.add_message(k.clone(), v.clone());
        }
    }
    Some(t)
}

/// `"ui/strings.en.translation"` -> `"en"`.
///
/// ⚠️ The locale is the segment BEFORE the extension, not the file stem: the
/// stem of `strings.en.translation` is `strings.en`, and using it would register
/// every catalog under a locale no lookup ever asks for.
fn locale_from_artifact_name(rel_path: &str) -> String {
    std::path::Path::new(rel_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|stem| stem.rsplit_once('.').map(|(_, loc)| loc.to_string()))
        .unwrap_or_else(|| "en".to_string())
}

fn load_project_translations(project_root: &Path, settings: &ProjectSettings) {
    let paths: Vec<String> = settings
        .get("internationalization/locale/translations")
        .and_then(|v| v.as_string())
        .map(parse_res_path_list)
        .unwrap_or_default();

    gdcore::translation::with_translation_server_mut(|server| {
        for res_path in &paths {
            let rel = res_path.strip_prefix("res://").unwrap_or(res_path);
            let full = project_root.join(rel);
            // 🔑 `project.godot` lists the IMPORTED artifacts, not the sources:
            // Godot's CSV importer emits `res://x.en.translation`, a BINARY
            // Resource. Reading that as text either fails on invalid UTF-8 or
            // yields bytes that the `.po` branch below parses into nothing —
            // so a real Godot project loaded ZERO catalogs and said nothing
            // about it (pat-s3rrv.139).
            if rel.ends_with(".translation") {
                match std::fs::read(&full)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| {
                        gdresource::res_loader::load_res_binary(&bytes, res_path)
                            .map_err(|e| e.to_string())
                    }) {
                    // Plain `Translation` first, then the compressed
                    // `OptimizedTranslation` the importer emits with
                    // `compress` on (pat-s3rrv.213).
                    Ok(res) => match translation_from_resource(&res, rel)
                        .or_else(|| optimized_translation_from_resource(&res, rel))
                    {
                        Some(t) => server.add_translation(t),
                        None => tracing::warn!(
                            "translation `{res_path}` is a `{}` with no readable \
                             `messages` or hash tables; skipping",
                            res.class_name
                        ),
                    },
                    Err(e) => {
                        tracing::warn!("translation `{res_path}` could not be loaded: {e}")
                    }
                }
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&full) else {
                tracing::warn!("translation `{res_path}` could not be read; skipping");
                continue;
            };
            if rel.ends_with(".csv") {
                for t in gdcore::translation::parse_csv(&text) {
                    server.add_translation(t);
                }
            } else {
                // A `.po`'s locale is its file stem (`fr.po` -> `fr`), which is
                // the convention Godot's importer uses for the output name.
                let locale = std::path::Path::new(rel)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("en");
                server.add_translation(gdcore::translation::parse_po(locale, &text));
            }
        }

        if let Some(fallback) = settings
            .get("internationalization/locale/fallback")
            .and_then(|v| v.as_string())
        {
            if !fallback.is_empty() {
                server.set_fallback(fallback);
            }
        }
        // `locale/test` forces a locale for testing and wins over the fallback,
        // matching Godot.
        if let Some(test_locale) = settings
            .get("internationalization/locale/test")
            .and_then(|v| v.as_string())
        {
            if !test_locale.is_empty() {
                server.set_locale(test_locale);
            }
        }
    });
}

/// The boot body shared by the loose-directory and `.pck` entry points.
///
/// Everything from here on reads through `res://`, so it does not care whether
/// the bytes come from disk or from a mounted pack. `project_root` is only the
/// `res://` fallback and the path reported in messages; both callers have already
/// installed the path roots.
fn boot_from_project_text(
    project_root: &Path,
    content: &str,
    user_dir: Option<PathBuf>,
) -> Result<BootedProject, String> {
    let project_root = project_root.to_path_buf();

    // (0) Populate ClassDB before ANYTHING loads a scene (pat-s3rrv.7).
    //
    // Godot fills ClassDB in `register_core_types()`/`register_scene_types()`
    // before the MainLoop exists, so every class default, method and signal is
    // queryable from frame 0. Patina only ever registered from the editor, so a
    // booted game had `Object -> Node -> Viewport -> Window` and nothing else.
    //
    // This must run ahead of the scene load below, not beside it: the loader
    // seeds each node's defaults from `class_db::get_property_list()`
    // (gdscene `packed_scene::node_with_class_defaults` / `SceneTree::create_node`),
    // and that read happens once, as the node is built. Registering afterwards
    // populates the database but leaves every already-loaded node bare — the
    // failure would look like a ClassDB bug rather than an ordering bug.
    //
    // Every boot entry point (`boot_project`, `boot_project_with_user_root`,
    // `boot_project_from_pck`) funnels through here, so this is the one call
    // site that covers all of them. Idempotent, so a host that already
    // registered (the editor) pays nothing.
    gdobject::class_db::register_all_classes();

    // (1) Parse project.godot for scene/autoloads/input and for ProjectSettings.
    let loader = ProjectLoader::from_text(&project_root, content)
        .map_err(|e| format!("failed to parse project.godot: {e}"))?;
    let settings = ProjectSettings::from_godot_format(content)
        .map_err(|e| format!("failed to load ProjectSettings: {e}"))?;

    // (1b) Seed the global shader parameter store from `[shader_globals]`
    // (pat-0q2kx.225). A shader declaring `global uniform vec4 tint;` reads
    // this store, so without seeding here the parsing and the store both exist
    // and the shader still renders with nothing — the same shape as the
    // `input_map` gap below, where boot parsed a value nobody ever installed.
    //
    // Values are authored as GDScript literals (`Color(1, 0, 0, 1)`), so they
    // go through the same `parse_variant_value` the `.tres` loader uses. An
    // unparseable value is skipped rather than defaulted: a global bound to a
    // wrong-typed value would misrender silently, which is worse than one that
    // is visibly absent.
    for global in loader.project_file().shader_globals() {
        match gdresource::parse_variant_value(&global.value_literal) {
            Ok(value) => {
                gdvariant::shader_globals::add(&global.name, &global.type_name, value);
            }
            Err(e) => {
                tracing::warn!(
                    name = %global.name,
                    literal = %global.value_literal,
                    error = %e,
                    "skipping [shader_globals] entry with an unparseable value"
                );
            }
        }
    }

    // (1a) Name the `user://` directory after the project, the way Godot does
    // (`<data dir>/Godot/app_userdata/<application/config/name>`). Without this
    // every Patina project shared one `user://` (pat-0q2kx.123). The declared
    // feature tags go in at the same time so `OS.has_feature("my_tag")` answers
    // for project-defined tags, not just the built-in ones.
    gdplatform::os::set_project_name(settings.project_name());
    gdplatform::os::clear_custom_features();
    if let Some(features) = settings.get("application/config/features") {
        if let Some(list) = features.as_string() {
            for tag in list.split(',').map(str::trim).filter(|t| !t.is_empty()) {
                gdplatform::os::add_custom_feature(tag);
            }
        }
    }

    // (1a1) Load the project's translation catalogs (pat-0q2kx.147).
    //
    // Godot boots `TranslationServer` from
    // `internationalization/locale/translations` plus the locale/fallback
    // settings (`core/string/translation_server.cpp`). Patina had a correct
    // catalog in `gdcore::translation` that **nothing called**, while the
    // interpreter read a private thread-local store seeded only by a scripted
    // `add_message` — so a project's `.po` files never reached `tr()`.
    load_project_translations(&project_root, &settings);

    // (1a2) Publish the parsed settings to the script-visible `ProjectSettings`
    // singleton (pat-0q2kx.135). The interpreter's store was seeded by nothing,
    // so `ProjectSettings.get_setting("physics/2d/default_gravity")` answered
    // Nil inside a project that plainly declares it. Feature tags come from the
    // same OS layer that answers `OS.has_feature`, so `setting.web` resolves
    // against the same truth, and the source path is remembered so
    // `ProjectSettings.save()` writes back where it read from.
    //
    // 🔑 TWO seeds, defaults first. Godot's `GLOBAL_DEF` registers every default
    // in `ProjectSettings::props`, so `get_setting("physics/2d/default_gravity")`
    // answers 980.0 in a project that never overrides it — and `project.godot`
    // never writes a key it has not changed. Seeding only `settings.iter()`
    // (declared keys; see `ProjectSettings::iter`) left every default `Nil`,
    // which broke the classic `CharacterBody2D` idiom (pat-mgixk.42).
    //
    // The halves stay apart rather than merging into one iterator: the defaults
    // are not marked project-declared, so `ProjectSettings.save()` still writes
    // back only overrides instead of stamping ~230 engine defaults into the
    // user's `project.godot`.
    // Delegated to the shared helper so the runner and the editor's Play path
    // cannot drift apart: the editor applied NONE of this, which is the whole of
    // pat-mgixk.44. The defaults/declared split, the feature tags and the
    // settings path all live there now, with the reasoning.
    gdscene::project_runtime::seed_script_settings_for_this_thread(&settings, &project_root);

    // (1b) Install the DisplayServer backend now that the project's configured
    // viewport is known. Without this a script calling
    // `DisplayServer.window_get_size()` got a hardcoded 1920x1080 from inside
    // the interpreter, unrelated to `display/window/size/viewport_*` and to the
    // window it was actually running in (pat-0q2kx.38).
    gdcore::display_backend::install_display_backend(std::sync::Arc::new(
        gdcore::display_backend::HeadlessDisplayBackend::new(
            settings.viewport_width(),
            settings.viewport_height(),
        ),
    ));

    // (2) Re-assert res:// / user:// roots. Both entry points set them before
    // calling in (the `.pck` path must, to read project.godot at all); doing it
    // again here keeps `user://` correct now that the project name is known.
    set_path_roots(Some(project_root.clone()), user_dir);

    // (2b) Populate the ResourceUID registry BEFORE anything resolves a path
    // (pat-mgixk.55). Godot's `ResourceUID::load_from_cache` runs at startup for
    // exactly this reason: the 4.6 editor writes `run/main_scene` as a
    // `uid://…` (`editor/editor_node.cpp:2627`), and autoloads and `load()`
    // calls may be uids too, so the mapping has to exist before the first
    // resolution rather than being filled in lazily by whoever asks first.
    //
    // ⚠️ After `set_path_roots`, because the cache is read through `res://`.
    load_uid_registry(&project_root);

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();

    // (3) Instantiate autoloads BEFORE the main scene, each under /root and
    // renamed to its autoload name (Godot exposes them as `/root/<Name>`).
    let mut autoload_roots = Vec::new();
    for entry in loader.get_autoloads() {
        let node_id = instantiate_autoload(&mut tree, root_id, &entry)?;
        autoload_roots.push((entry.name, node_id));
    }

    // (4) Resolve and instance the declared main scene.
    let main_scene = loader.config().main_scene.clone();
    let main_scene_root = if main_scene.is_empty() {
        None
    } else {
        Some(load_scene_into_tree(&mut tree, root_id, &main_scene)?)
    };

    // (4a) Record it as the RUNNING scene (pat-mgixk.62). `current_scene_id` was
    // assigned only by the change-scene functions, so a freshly booted project
    // had a null `get_tree().current_scene` and `reload_current_scene()` failed
    // with "No packed scene source" — on the scene the game was visibly running.
    //
    // 🔑 It also decides what a later scene change FREES: the change path now
    // removes only the current scene instead of every child of `/root`, so
    // leaving this unset would make the first `change_scene_to_file` free
    // nothing and stack the new scene on top of the old one.
    if let Some(root) = main_scene_root {
        // The packed source comes along so `reload_current_scene()` works from
        // the first frame; re-reading the `.tscn` here goes through the same
        // pack-aware `FileAccess` path `load_scene_into_tree` used, so a packed
        // project behaves identically to a loose one.
        // ⚠️ Through `resolve_uid_path`, the SAME helper `load_scene_into_tree`
        // used — not a second copy of the uid-plus-fallback logic, which would
        // be free to drift from it and resolve a different file than the one
        // actually instanced.
        let packed = FileAccess::read_file_string(&resolve_uid_path(&main_scene))
            .ok()
            .and_then(|text| PackedScene::from_tscn(&text).ok());
        tree.set_current_scene(root, packed);
    }

    // (4b) Attach the scenes' GDScripts.
    //
    // ⚠️ This is the step the project-directory boot path never had
    // (pat-mgixk.414). `boot_project` instanced the main scene and the
    // autoloads, but only script AUTOLOADS ever got a script, so every node in
    // the main scene ran with no script at all: no `_ready`, no `_process`, no
    // prints. The conformance gate measured it as 0 of 4 projects passing with
    // ZERO `[T]` lines from any of them.
    //
    // It happens HERE, not in the callers, because three call paths booted a
    // project and only one of them (the single-`.tscn` path in `main.rs`)
    // remembered to attach. Doing it inside the boot means a path cannot forget.
    //
    // Ordering matters: after the main scene is instanced (there is nothing to
    // attach to before that) and before any caller runs `enter_tree`, because
    // `_ready` has to find a script already on the node.
    attach_scripts(&mut tree, &project_root);

    // (5) Seed the runtime InputMap from `[input]`. Godot's
    // InputMap::load_from_project_settings installs the builtin `ui_*` actions
    // and then registers each project action with its real deadzone and bound
    // events (keycodes), so `Input.is_action_pressed` fires for project actions
    // after a matching key press. `ProjectLoader::get_input_map` keeps only
    // lossy action names (no deadzone, no bound events), so the raw
    // project.godot text goes to the full parser instead. That text is already
    // in hand, which also removes the old re-read-from-disk fallback and is what
    // lets this work for a packed project (pat-0q2kx.134).
    let input_map = InputMap::load_from_project_godot(content);

    Ok(BootedProject {
        tree,
        settings,
        input_map,
        main_scene_root,
        autoload_roots,
    })
}

/// Attaches each node's `_script_path` GDScript, returning how many attached.
///
/// This is the ONE implementation. It used to exist three times — in
/// `main.rs::attach_scripts`, in `Bootstrap::init_scripts` and nowhere on the
/// project-directory boot path, which is exactly how that path shipped without
/// it (pat-mgixk.414). Both of the others now call this.
///
/// ⚠️ Nodes that already carry a script are SKIPPED. Autoloads are
/// instantiated with their script before the main scene is loaded, and
/// re-attaching would re-parse the source and clobber state their `_ready` has
/// already produced.
///
/// A script that fails to read or parse is logged and skipped rather than
/// failing the boot: Godot loads the rest of the scene when one script is
/// broken, and a project with one bad file should still start.
pub fn attach_scripts(tree: &mut SceneTree, project_dir: &Path) -> usize {
    // Collected first: attaching borrows the tree mutably, and the walk needs it
    // immutably.
    let mut scripts_to_load: Vec<(NodeId, PathBuf)> = Vec::new();
    for node_id in tree.all_nodes_in_tree_order() {
        if tree.get_script(node_id).is_some() {
            continue;
        }
        if let Some(node) = tree.get_node(node_id) {
            if let Variant::String(res_path) = node.get_property("_script_path") {
                scripts_to_load.push((node_id, resolve_res_path(project_dir, &res_path)));
            }
        }
    }

    let mut attached = 0usize;
    for (node_id, path) in scripts_to_load {
        match std::fs::read_to_string(&path) {
            Ok(source) => match GDScriptNodeInstance::from_source(&source, node_id) {
                Ok(instance) => {
                    tree.attach_script(node_id, Box::new(instance));
                    attached += 1;
                    tracing::info!(path = %path.display(), "attached GDScript to node");
                }
                Err(e) => tracing::warn!(
                    path = %path.display(),
                    error = format!("{e:?}"),
                    "failed to parse GDScript"
                ),
            },
            Err(e) => tracing::warn!(
                path = %path.display(),
                error = %e,
                "failed to read GDScript file"
            ),
        }
    }
    attached
}

/// Resolves a `res://` path against the project directory.
///
/// Walks ancestors of `project_dir` and then of the cwd, because a fixture
/// project is often nested below the directory the runner was pointed at.
/// Falls back to `project_dir.join(relative)` so the caller gets a path to
/// report even when nothing exists.
pub fn resolve_res_path(project_dir: &Path, res_path: &str) -> PathBuf {
    let relative = res_path.strip_prefix("res://").unwrap_or(res_path);
    for ancestor in project_dir.ancestors() {
        let candidate = ancestor.join(relative);
        if candidate.exists() {
            return candidate;
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        for ancestor in cwd.ancestors() {
            let candidate = ancestor.join(relative);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    project_dir.join(relative)
}

/// Resolves a `uid://…` to the `res://` path it names, leaving anything else
/// untouched.
///
/// ⚠️ Falls back to the INPUT when the uid is unregistered.
/// `uid::ensure_path` returns an EMPTY string for an unknown uid (it wraps
/// `get_id_path`, which Godot fails to `String()`), and an empty path would
/// surface downstream as `failed to read scene ''` — an error naming nothing.
/// Echoing the uid back keeps the eventual message pointing at what was asked
/// for.
fn resolve_uid_path(path_or_uid: &str) -> String {
    let resolved = gdresource::uid::ensure_path(path_or_uid);
    if resolved.is_empty() {
        return path_or_uid.to_string();
    }
    resolved
}

/// Populates `uid::global_registry()` from the project's UID sources, as Godot's
/// `ResourceUID::load_from_cache` does at startup (`core/io/resource_uid.cpp`).
///
/// Two sources, both of which a real 4.4+ project has:
///
/// * `res://.godot/uid_cache.bin` — read through [`FileAccess`] so a mounted
///   `.pck` serves it exactly as a loose project does.
/// * `*.uid` sidecars next to scripts and shaders, which the editor writes
///   per-asset and which are not in the cache for a freshly-cloned project.
///
/// Every failure is ignored on purpose: a project with no UIDs at all is
/// completely normal, and booting must not depend on the cache existing.
fn load_uid_registry(project_root: &Path) {
    if let Ok(bytes) = FileAccess::read_file_bytes("res://.godot/uid_cache.bin") {
        if let Ok(mut registry) = gdresource::uid::global_registry().lock() {
            let _ = registry.load_from_cache_bytes(&bytes);
        }
    }
    register_uid_sidecars(project_root);
}

/// Registers every `*.uid` sidecar under `project_root` against its source
/// asset's `res://` path.
///
/// 🔑 Registers a `res://` path, NOT a filesystem path. `uid.rs`'s own
/// `load_uid_sidecar_file` strips `.uid` from the path it was handed, so feeding
/// it a filesystem path registers `/tmp/xyz/icon.svg` — which no consumer ever
/// looks up, because every consumer asks with `res://icon.svg`. The mapping has
/// to be keyed the way it will be queried.
fn register_uid_sidecars(project_root: &Path) {
    // Depth-bounded: a project tree is shallow, and a bound means a symlink
    // cycle cannot hang boot.
    fn walk(dir: &Path, root: &Path, depth: usize) {
        if depth > 12 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // `.godot` holds the cache itself and the import artifacts, not
                // source assets; skipping it also skips the biggest directory.
                if path.file_name().is_some_and(|n| n == ".godot") {
                    continue;
                }
                walk(&path, root, depth + 1);
                continue;
            }
            if path.extension().is_some_and(|e| e == "uid") {
                let Ok(content) = std::fs::read_to_string(&path) else {
                    continue;
                };
                // `foo.gd.uid` describes `foo.gd`.
                let Some(source) = path.to_str().and_then(|s| s.strip_suffix(".uid")) else {
                    continue;
                };
                let Ok(relative) = Path::new(source).strip_prefix(root) else {
                    continue;
                };
                let res_path = format!("res://{}", relative.to_string_lossy());
                if let Ok(mut registry) = gdresource::uid::global_registry().lock() {
                    registry.register_uid_sidecar(res_path, &content);
                }
            }
        }
    }
    walk(project_root, project_root, 0);
}

/// Instantiates one autoload under `parent`, returning its node id. Scene
/// autoloads (`.tscn`/`.scn`) are instanced; other (script) autoloads become a
/// bare `Node` carrying the script path. Either way the node is renamed to the
/// autoload's name so it is reachable as `/root/<Name>`.
fn instantiate_autoload(
    tree: &mut SceneTree,
    parent: NodeId,
    entry: &AutoloadEntry,
) -> Result<NodeId, String> {
    // 🔑 Resolved BEFORE the extension test, not inside `load_scene_into_tree`
    // (pat-mgixk.55). A `uid://…` has no `.tscn` suffix, so testing the raw
    // string would classify a uid SCENE autoload as a script autoload and
    // silently add a bare `Node` carrying the uid as its script path — a wrong
    // tree rather than an error. Resolving one hop earlier is what makes both
    // autoload kinds work from a uid.
    let autoload_path = resolve_uid_path(&entry.path);
    let node_id = if autoload_path.ends_with(".tscn") || autoload_path.ends_with(".scn") {
        load_scene_into_tree(tree, parent, &autoload_path)?
    } else {
        let mut node = Node::new(entry.name.clone(), "Node");
        node.set_property("script", Variant::String(autoload_path.clone()));
        tree.add_child(parent, node)
            .map_err(|e| format!("failed to add autoload '{}': {e}", entry.name))?
    };
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_name(entry.name.clone());
    }
    Ok(node_id)
}

/// Resolves a `res://` scene path to disk, parses the `.tscn`, and instances it
/// under `parent`, returning the instanced root node id.
fn load_scene_into_tree(
    tree: &mut SceneTree,
    parent: NodeId,
    res_path: &str,
) -> Result<NodeId, String> {
    // `run/main_scene` is a `uid://…` in any project the Godot 4.6 editor saved
    // (pat-mgixk.55), so resolve before reading. A plain `res://` path passes
    // through untouched.
    let res_path = &resolve_uid_path(res_path);
    // Then follow a `<path>.remap` redirect (pat-mgixk.56).
    //
    // 🔑 An export with `convert_text_resources_to_binary` (the default) leaves
    // only `res://main.tscn.remap` in the pack, pointing at the converted file
    // under `res://.godot/exported/`. So a shipped game's `run/main_scene` names
    // a path the pack does not contain, and the read below fails outright
    // without this hop.
    //
    // AFTER the uid resolution, matching `_path_remap`, which calls
    // `ResourceUID::ensure_path` and only then tests for `.remap`
    // (`resource_loader.cpp:1286-1287`). The other order would look for
    // `uid://abc.remap`, which never exists.
    //
    // Scene autoloads reach this through `instantiate_autoload`, so they follow
    // the same redirect.
    let res_path = &gdresource::importers::remap_path(res_path);
    // Pack-aware: `FileAccess::read_file_string` serves a mounted `.pck` before
    // disk, so a packed and a loose project take the identical load path
    // (pat-0q2kx.134). The `res://` path is used in errors rather than a
    // filesystem path, because a packed read has none.
    let source = FileAccess::read_file_string(res_path)
        .map_err(|e| format!("failed to read scene '{res_path}': {e}"))?;
    let packed = PackedScene::from_tscn(&source)
        .map_err(|e| format!("failed to parse scene '{res_path}': {e}"))?;
    add_packed_scene_to_tree(tree, parent, &packed)
        .map_err(|e| format!("failed to instance scene '{res_path}': {e}"))
}

#[cfg(test)]
mod tests {
    //! pat-nzgdu.3: the runner's project boot path must instantiate every
    //! `[autoload]` entry under `/root` before the main scene, so scene scripts
    //! can reference `/root/<Name>` in `_ready`. The shared `gdscene::autoload`
    //! helper is unit-tested on its own; these tests cover the patina-runner
    //! wiring (`boot_project`) end to end at the runner boot site.

    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use gdresource::resource::Resource;
    use gdvariant::Dictionary;

    /// A `Translation` resource as Godot's CSV importer writes one.
    fn translation_res(locale: Option<&str>) -> Resource {
        let mut msgs = Dictionary::new();
        msgs.insert(
            Variant::String("GREETING".into()),
            Variant::String("Bonjour".into()),
        );
        msgs.insert(
            Variant::String("FAREWELL".into()),
            Variant::String("Au revoir".into()),
        );
        // ⚠️ A non-String key is Godot's context/plural form. It must be SKIPPED,
        // not guessed at — a wrong guess puts the wrong string on screen.
        msgs.insert(
            Variant::Int(7),
            Variant::String("not a plain message".into()),
        );

        let mut res = Resource::new("Translation");
        res.set_property("messages", Variant::Dictionary(msgs.into()));
        if let Some(l) = locale {
            res.set_property("locale", Variant::String(l.into()));
        }
        res
    }

    /// **The core**: a `.translation` artifact yields a real catalog.
    #[test]
    fn a_binary_translation_resource_becomes_a_catalog() {
        let t =
            translation_from_resource(&translation_res(Some("fr")), "ui/strings.fr.translation")
                .expect("a Translation resource must convert");
        assert_eq!(t.locale(), "fr");
        assert_eq!(t.get_message("GREETING").as_deref(), Some("Bonjour"));
        assert_eq!(t.get_message("FAREWELL").as_deref(), Some("Au revoir"));
    }

    /// ⚠️ Only plain-String keys are read. The `Int` key above is Godot's
    /// context/plural encoding; it must not appear as a message.
    #[test]
    fn non_string_keys_are_skipped_not_guessed() {
        let t =
            translation_from_resource(&translation_res(Some("fr")), "x.fr.translation").unwrap();
        assert_eq!(t.len(), 2, "only the two plain messages are read");
    }

    /// 🔑 The locale is the segment BEFORE the extension, not the file stem.
    ///
    /// ⚠️ `Path::file_stem("strings.en.translation")` is `"strings.en"` — using
    /// it registers every catalog under a locale no lookup ever asks for, so
    /// `tr()` silently returns its key forever.
    #[test]
    fn the_locale_comes_from_the_segment_before_the_extension() {
        assert_eq!(locale_from_artifact_name("ui/strings.en.translation"), "en");
        assert_eq!(
            locale_from_artifact_name("a/b/menu.pt_BR.translation"),
            "pt_BR"
        );
        // No locale segment at all — fall back rather than inventing one.
        assert_eq!(locale_from_artifact_name("strings.translation"), "en");
    }

    /// The resource's own `locale` is authoritative over the filename.
    #[test]
    fn the_resources_locale_property_wins_over_the_filename() {
        let t =
            translation_from_resource(&translation_res(Some("de")), "ui/strings.fr.translation")
                .unwrap();
        assert_eq!(t.locale(), "de");
        // And with no property, the name is used.
        let t2 =
            translation_from_resource(&translation_res(None), "ui/strings.fr.translation").unwrap();
        assert_eq!(t2.locale(), "fr");
    }

    /// ⚠️ `OptimizedTranslation` has no `messages`, so the PLAIN reader must
    /// still refuse it. Its intent is unchanged from when it was written: an
    /// empty catalog would look like a successful load and leave every key
    /// untranslated with no warning.
    ///
    /// What changed (pat-s3rrv.213) is that refusing it is no longer the end of
    /// the story — `load_project_translations` now falls through to
    /// `optimized_translation_from_resource`. So this asserts the plain reader
    /// declines, and the test below asserts the compressed reader picks it up.
    #[test]
    fn an_optimized_translation_is_refused_by_the_plain_reader() {
        let res = Resource::new("OptimizedTranslation");
        assert!(translation_from_resource(&res, "x.fr.translation").is_none());
    }

    /// A compressed resource with no payload is refused rather than accepted as
    /// an EMPTY catalog — the silent failure the plain reader also guards.
    #[test]
    fn an_optimized_translation_without_tables_is_refused() {
        let res = Resource::new("OptimizedTranslation");
        assert!(optimized_translation_from_resource(&res, "x.fr.translation").is_none());
    }

    /// 🔑 A compressed `.translation` becomes a catalog whose messages resolve.
    ///
    /// The bucket layout is written by hand exactly as `generate()` emits it:
    /// `[size, func, (key, str_offset, comp_size, uncomp_size)]`, with the
    /// string arena holding the NUL terminator that `uncomp_size` counts.
    ///
    /// ⚠️ `hash_table` / `bucket_table` are authored as **i32** here, the way
    /// the resource actually stores them, so the `-1` sentinel round-trips.
    #[test]
    fn an_optimized_translation_becomes_a_working_catalog() {
        use gdcore::optimized_translation::hash;

        let func = 0x41u32;
        let key = "GREETING";
        let stored: &[u8] = b"Bonjour\0"; // 8 bytes: 7 + terminator

        let mut res = Resource::new("OptimizedTranslation");
        res.set_property("locale", gdvariant::Variant::String("fr".to_string()));
        res.set_property("hash_table", gdvariant::Variant::PackedInt32Array(vec![0]));
        res.set_property(
            "bucket_table",
            gdvariant::Variant::PackedInt32Array(vec![
                1,
                func as i32,
                hash(func, key) as i32,
                0,
                stored.len() as i32,
                stored.len() as i32,
            ]),
        );
        res.set_property(
            "strings",
            gdvariant::Variant::PackedByteArray(stored.to_vec()),
        );

        let t = optimized_translation_from_resource(&res, "ui/strings.fr.translation")
            .expect("a compressed resource with tables must load");
        assert_eq!(t.locale(), "fr");
        assert_eq!(
            t.get_message(key).as_deref(),
            Some("Bonjour"),
            "the compressed catalog must answer lookups — this is what makes \
             tr() return the translation instead of the key"
        );
        assert_eq!(t.get_message("ABSENT").as_deref(), None);
    }

    /// The Godot 3 class name still loads.
    ///
    /// ⚠️ `renames_map_3_to_4.cpp:1557` maps `PHashTranslation` ->
    /// `OptimizedTranslation`; an upgraded project can still ship the old name.
    #[test]
    fn the_godot_3_phash_translation_name_is_accepted() {
        let mut res = Resource::new("PHashTranslation");
        res.set_property("hash_table", gdvariant::Variant::PackedInt32Array(vec![-1]));
        res.set_property("bucket_table", gdvariant::Variant::PackedInt32Array(vec![]));
        res.set_property("strings", gdvariant::Variant::PackedByteArray(vec![]));

        let t = optimized_translation_from_resource(&res, "x.de.translation")
            .expect("the 3.x class name must still be recognised");
        assert_eq!(t.locale(), "de");
        // 🔑 The -1 sentinel survived the i32 -> u32 round trip. Had it been
        // rejected by a checked conversion, this would have returned None above.
        assert_eq!(t.get_message("ANY").as_deref(), None);
    }

    // `boot_project` mutates the process-global `res://` path roots via
    // `set_path_roots`, so serialize these tests to stay robust under
    // `cargo test` (nextest already isolates each test in its own process).
    static BOOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn unique_temp_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "patina_nzgdu3_{}_{}_{}",
            tag,
            std::process::id(),
            n
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// pat-nzgdu.69: booting seeds the runtime InputMap from the project's
    /// `[input]` section — with the project's own deadzone and bound key events,
    /// not a hardcoded placeholder — plus the builtin `ui_*` action set, so
    /// `Input.is_action_pressed` can fire for a project-defined action after the
    /// matching key press. Proven end-to-end through `boot_project`, not by
    /// populating the map by hand.
    #[test]
    fn project_input_map_honored() {
        let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = unique_temp_dir("inputmap");
        std::fs::write(
            dir.join("project.godot"),
            "[application]\nconfig/name=\"InputMapBootTest\"\n\n\
             [input]\n\n\
             jump={\n\
             \"deadzone\": 0.35,\n\
             \"events\": [Object(InputEventKey,\"resource_local_to_scene\":false,\"device\":-1,\"keycode\":32,\"physical_keycode\":32,\"pressed\":false,\"script\":null)]\n\
             }\n",
        )
        .unwrap();

        let booted = boot_project(&dir).expect("boot project.godot with [input]");

        // The project action exists with the project's deadzone (0.35) — not the
        // old hardcoded 0.5.
        let actions: Vec<&str> = booted.input_map.actions().map(|s| s.as_str()).collect();
        assert!(
            actions.contains(&"jump"),
            "project action 'jump' registered at boot, got {actions:?}"
        );
        assert!(
            (booted.input_map.get_deadzone("jump") - 0.35).abs() < 1e-6,
            "action deadzone matches the project value, got {}",
            booted.input_map.get_deadzone("jump")
        );

        // The project's Space key event is bound to the action, so a Space press
        // triggers it (the runtime input path consults this map).
        let space = gdplatform::InputEvent::Key {
            key: gdplatform::Key::Space,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        };
        assert!(
            booted.input_map.event_matches_action(&space, "jump"),
            "a Space press triggers the project-defined 'jump' action"
        );

        // The builtin `ui_*` actions are also present (installed from the default
        // table, not per-demo).
        assert!(
            booted
                .input_map
                .actions()
                .any(|a| a.as_str() == "ui_accept"),
            "builtin ui_* actions are registered at boot"
        );
    }

    #[test]
    fn boot_project_instantiates_script_autoloads_under_root() {
        let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = unique_temp_dir("script");
        std::fs::write(
            dir.join("project.godot"),
            "[application]\nconfig/name=\"AutoloadBootTest\"\n\n\
             [autoload]\nGlobal=\"*res://global.gd\"\nMusic=\"*res://music.gd\"\n",
        )
        .unwrap();
        // Script autoloads: `boot_project` stores the script path on a bare Node
        // and does not read the file, so the `.gd` files need not exist.

        let booted = boot_project(&dir).expect("boot project.godot with autoloads");

        // Both autoloads instantiated and tracked.
        assert_eq!(booted.autoload_roots.len(), 2, "two autoloads instantiated");
        let names: Vec<&str> = booted
            .autoload_roots
            .iter()
            .map(|(n, _)| n.as_str())
            .collect();
        assert!(names.contains(&"Global"), "Global autoload present");
        assert!(names.contains(&"Music"), "Music autoload present");

        // Reachable as `/root/<Name>` — Godot's autoload access path.
        let global = booted
            .tree
            .get_node_by_path("/root/Global")
            .expect("/root/Global exists");
        assert_eq!(booted.tree.get_node(global).unwrap().name(), "Global");
        assert!(
            booted.tree.get_node_by_path("/root/Music").is_some(),
            "/root/Music exists"
        );

        // No main scene declared → none instanced; autoloads still present.
        assert!(booted.main_scene_root.is_none(), "no main scene declared");
    }

    #[test]
    fn boot_project_autoloads_coexist_with_main_scene() {
        let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = unique_temp_dir("mainscene");
        std::fs::write(
            dir.join("project.godot"),
            "[application]\nrun/main_scene=\"res://main.tscn\"\n\n\
             [autoload]\nGlobal=\"*res://global.gd\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("main.tscn"),
            "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node2D\"]\n",
        )
        .unwrap();

        let booted = boot_project(&dir).expect("boot autoload + main scene");

        // The main scene instanced under `/root` ...
        let main_root = booted.main_scene_root.expect("main scene instanced");
        assert_eq!(booted.tree.get_node(main_root).unwrap().name(), "Main");

        // ... and the autoload constructed before it still lives under `/root`
        // (Godot builds autoloads first so the main scene's `_ready` sees them).
        assert_eq!(booted.autoload_roots.len(), 1, "autoload instantiated");
        let global = booted
            .tree
            .get_node_by_path("/root/Global")
            .expect("/root/Global survives main-scene instancing");
        assert_eq!(booted.tree.get_node(global).unwrap().name(), "Global");
    }
}

//! Applying a parsed `project.godot` to a running engine.
//!
//! Godot's Play boots the game exactly as an export would: `Main::setup` /
//! `Main::start` read every `GLOBAL_GET`, so physics tick rate, gravity,
//! stretch, translations and the script-visible `ProjectSettings` all come from
//! the project file (`main/main.cpp`).
//!
//! Patina grew two entry points instead. `patina_runner` applied the settings
//! inline during boot, and the editor's Play path built a `MainLoop` with only
//! the input map and the two pointer-emulation flags — so a project's tick rate
//! and gravity were simply ignored in the editor, and
//! `ProjectSettings.get_setting` answered `Nil` inside a project that plainly
//! declares the value. The editor is where a developer tests the game, which
//! makes it the worst of the two places to get this wrong (pat-mgixk.44).
//!
//! This module is the one place that knows how a parsed [`ProjectSettings`]
//! becomes running-engine state, so the two entry points cannot drift again.
//!
//! # Threading
//!
//! ⚠️ The script-visible settings store in `gdscript_interop` is a
//! **thread-local**, so the two halves below are deliberately separate:
//!
//! * [`apply_project_settings`] touches shared state — the `MainLoop` — and can
//!   be called from anywhere, once per run.
//! * [`seed_script_settings_for_this_thread`] touches the thread-local store and
//!   must be called **on the thread that runs frames**. The editor serves each
//!   HTTP connection on its own thread and advances the runtime there, so
//!   seeding at play time would be invisible to the thread that later runs
//!   `_ready`.
//!
//! Collapsing the two would produce a settings surface that works in the runner
//! (single-threaded) and silently answers `Nil` in the editor.

use std::path::Path;

use gdcore::{ProjectSettings, SettingsValue};
use gdvariant::Variant;

use crate::MainLoop;

/// Applies the project's settings to `main_loop`.
///
/// This is the half that lives on shared state: the physics tick rate,
/// interpolation flag and 2D/3D gravity all live on the `MainLoop`, so one call
/// per run is enough however many threads later advance it.
///
/// `os::set_project_name` is included because it is process-global and answers
/// `OS.get_name`-adjacent queries plus the window title; it is idempotent.
///
/// ⚠️ This does NOT seed the script-visible `ProjectSettings` singleton — see
/// [`seed_script_settings_for_this_thread`] and this module's threading note.
pub fn apply_project_settings(main_loop: &mut MainLoop, settings: &ProjectSettings) {
    gdplatform::os::set_project_name(settings.project_name());
    main_loop.configure_from_project_settings(settings);
}

/// Publishes the settings a script can observe, for a host that has NOT built
/// its `MainLoop` yet.
///
/// 🔑 This exists because of an ORDERING bug, not a missing value
/// (pat-mgixk.44). [`apply_project_settings`] needs a `&mut MainLoop`, so it can
/// only run after the loop is constructed — but both hosts dispatch `_ready`
/// while building the tree, BEFORE that point. A script reading
/// `Engine.physics_ticks_per_second` or
/// `ProjectSettings.get_setting("application/config/name")` in `_ready` was
/// therefore answered from the ENGINE defaults: 60 ticks for a project
/// declaring 30. Godot has no such window — `Main::setup` reads every
/// `GLOBAL_GET` before the scene is instantiated.
///
/// Call this BEFORE the tree is built; call [`apply_project_settings`] after the
/// loop exists. The two overlap deliberately: this one seeds the script-visible
/// globals, that one configures the loop's own physics/gravity fields.
/// Re-publishing the same tick rate is harmless — `publish_*` does not bump the
/// script-write generation counter, so the loop cannot mistake it for a script
/// assigning the value.
pub fn seed_script_visible_settings(settings: &ProjectSettings, project_root: &Path) {
    gdplatform::os::set_project_name(settings.project_name());
    seed_script_settings_for_this_thread(settings, project_root);
    // The tick rate specifically, because `Engine.physics_ticks_per_second`
    // reads a process-wide atomic that defaults to 60 rather than consulting
    // the settings store `seed_script_settings_for_this_thread` just filled.
    gdscript_interop::publish_engine_physics_ticks_per_second(
        settings.physics_ticks_per_second(),
    );
}

/// Publishes `settings` to the script-visible `ProjectSettings` singleton for
/// the CURRENT thread, once.
///
/// Godot's `ProjectSettings` is a process-wide singleton; Patina's store is a
/// thread-local (`gdscript_interop::interpreter`, `thread_local! {
/// PROJECT_SETTINGS }`). A host that runs frames on more than one thread — the
/// editor does — therefore has to seed each of them.
///
/// ⚠️ Guarded so it seeds a given thread exactly once. Re-seeding on every frame
/// would look harmless and would quietly undo a script's own
/// `ProjectSettings.set_setting`, handing back the project's value on the next
/// frame.
pub fn seed_script_settings_for_this_thread(settings: &ProjectSettings, project_root: &Path) {
    use std::cell::Cell;
    thread_local! {
        static SEEDED: Cell<bool> = const { Cell::new(false) };
    }
    if SEEDED.with(Cell::get) {
        return;
    }
    SEEDED.with(|s| s.set(true));

    // Engine DEFAULTS first, then the project's own values.
    //
    // Seeding only the declared keys leaves every default `Nil`, which is how
    // `ProjectSettings.get_setting("physics/2d/default_gravity")` answered
    // nothing in a project that simply never overrode it — `project.godot`
    // never writes a key it has not changed (pat-mgixk.42).
    //
    // ⚠️ The two halves stay apart rather than merging into one iterator: the
    // defaults are deliberately NOT marked project-declared, so
    // `ProjectSettings.save()` still writes back only the overrides instead of
    // stamping ~230 engine defaults into the user's `project.godot`.
    gdscript_interop::interpreter::seed_project_settings_defaults(
        settings
            .iter_defaults()
            .filter_map(|(k, v)| settings_value_to_variant(v).map(|var| (k.to_string(), var))),
    );
    gdscript_interop::interpreter::seed_project_settings(
        settings
            .iter()
            .filter_map(|(k, v)| settings_value_to_variant(v).map(|var| (k.to_string(), var))),
    );
    // Feature tags come from the same OS layer that answers `OS.has_feature`, so
    // a script's `OS.has_feature("web")` and a `setting.web` override cannot
    // disagree.
    gdscript_interop::interpreter::set_project_feature_tags(active_feature_tags());
    // Remembered so `ProjectSettings.save()` writes back where it was read from.
    gdscript_interop::interpreter::set_project_settings_path(
        project_root
            .join("project.godot")
            .to_string_lossy()
            .to_string(),
    );
}

/// Maps a `project.godot` value to the `Variant` a script sees.
///
/// Every [`SettingsValue`] has a Variant counterpart, so this is total — there
/// is no silently dropped setting.
fn settings_value_to_variant(v: &SettingsValue) -> Option<Variant> {
    use SettingsValue as S;
    Some(match v {
        S::Bool(b) => Variant::Bool(*b),
        S::Int(i) => Variant::Int(*i),
        S::Float(f) => Variant::Float(*f),
        S::String(s) => Variant::String(s.clone()),
        S::Vector2(x, y) => Variant::Vector2(gdcore::math::Vector2::new(*x as f32, *y as f32)),
        S::Vector3(x, y, z) => {
            Variant::Vector3(gdcore::math::Vector3::new(*x as f32, *y as f32, *z as f32))
        }
        S::Color(r, g, b, a) => Variant::Color(gdcore::math::Color::new(
            *r as f32, *g as f32, *b as f32, *a as f32,
        )),
    })
}

/// The feature tags active for this run, in the order
/// `get_setting_with_override` should try them.
fn active_feature_tags() -> Vec<String> {
    [
        "web", "editor", "template", "debug", "release", "pc", "desktop", "mobile", "linuxbsd",
        "macos", "windows", "linux", "android", "ios", "x86_64", "arm64",
    ]
    .into_iter()
    .filter(|tag| gdplatform::os::has_feature(tag))
    .map(str::to_string)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A project's tick rate and gravity reach the `MainLoop`, which is what the
    /// editor's Play path never did.
    #[test]
    fn apply_project_settings_carries_tick_rate_and_gravity() {
        let mut settings = ProjectSettings::new();
        settings.set(
            "physics/common/physics_ticks_per_second",
            SettingsValue::Int(30),
        );
        settings.set("physics/2d/default_gravity", SettingsValue::Float(490.0));

        let mut main_loop = MainLoop::new(crate::SceneTree::new());
        apply_project_settings(&mut main_loop, &settings);

        assert_eq!(
            main_loop.physics_ticks_per_second(),
            30,
            "the project's tick rate must reach the loop"
        );
        let g = main_loop.physics_server().gravity();
        assert_eq!(g.x, 0.0, "the default 2D direction is straight down");
        assert!(
            (g.y - 490.0).abs() < 1e-4,
            "the project's 2D gravity must reach the solver; got y = {}",
            g.y
        );
    }

    /// Every `SettingsValue` shape converts, so no setting is silently dropped
    /// on its way to a script.
    #[test]
    fn every_settings_value_shape_converts() {
        for v in [
            SettingsValue::Bool(true),
            SettingsValue::Int(7),
            SettingsValue::Float(1.5),
            SettingsValue::String("x".into()),
            SettingsValue::Vector2(1.0, 2.0),
            SettingsValue::Vector3(1.0, 2.0, 3.0),
            SettingsValue::Color(1.0, 0.0, 0.0, 1.0),
        ] {
            assert!(
                settings_value_to_variant(&v).is_some(),
                "{v:?} must have a Variant counterpart"
            );
        }
    }
}

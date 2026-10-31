//! The [`Environment3D`] a **running game** renders with (pat-mgixk.51 part 2).
//!
//! Godot resolves a frame's environment in one order, in `Camera3D::get_camera()`
//! and `Viewport::_propagate_enter_world`: the active `Camera3D`'s own
//! `environment`, else the `WorldEnvironment` node in the scene, else
//! `rendering/environment/defaults/default_environment`. With none of those, the
//! viewport clears to `rendering/environment/defaults/default_clear_color`
//! (`scene/3d/world_environment.cpp`, `scene/3d/camera_3d.cpp`).
//!
//! Patina had all three inputs and read none of them on the game path:
//! `world_environment_from_scene` lived in the **editor server** and ran only on
//! `api_load_scene`, for the editor viewport; `RenderServer3DAdapter::build_viewport`
//! set `environment: None`; `default_clear_color` and `default_environment` had no
//! reader at all. So a scene that authored a `WorldEnvironment` played with the
//! editor's *preview* sky no matter what it asked for, and the runner adapter
//! played with no environment whatsoever — two paths, neither of them the scene's.
//!
//! # Why this resolves off the `SceneTree`, not the `PackedScene`
//!
//! 🔑 The editor's resolver needs the `PackedScene` because it reads
//! `[sub_resource]` sections — an inline `environment = SubResource("Env_1")`. The
//! runtime has no `PackedScene`: by the time a game is rendering, the scene has
//! been instanced and dropped. What survives is the node's properties, and
//! `packed_scene` stores an `ExtResource` reference as the literal string plus a
//! companion `__extres__<prop>` holding the resolved `res://` path
//! (`packed_scene.rs:1551-1583`) — the same mechanism the theme chain reads
//! (`canvas_render.rs:2115`).
//!
//! So this module handles the **`.tres` file** form, which is what a shipped
//! project uses, and deliberately does not handle the inline sub-resource form.
//! An inline environment resolves to `None` here and the caller keeps its own
//! default, which is the editor preview in the editor and no environment in the
//! adapter — exactly today's behaviour, so nothing regresses.

use gdcore::file_access::FileAccess;
use gdserver3d::environment::{BackgroundMode, Environment3D};
use gdserver3d::sky::{Sky, SkyMaterial};
use gdvariant::Variant;

use crate::node::{Node, NodeId};
use crate::SceneTree;

/// The `res://` path a resource-valued property names, or empty.
///
/// Reads the `__extres__<prop>` companion first: `prop` itself still holds the
/// literal `ExtResource("1_e")`, and asking the filesystem for that string would
/// look for a file of that name.
fn resource_path(node: &Node, prop: &str) -> String {
    match node.get_property(&format!("__extres__{prop}")) {
        Variant::String(s) if !s.is_empty() => return s,
        _ => {}
    }
    match node.get_property(prop) {
        Variant::String(s) if s.starts_with("res://") || s.starts_with("user://") => s,
        _ => String::new(),
    }
}

/// The environment resource path this scene asks for, in Godot's resolution
/// order, or empty when it asks for none.
fn scene_environment_path(tree: &SceneTree, camera: Option<NodeId>) -> String {
    // 1. The active camera's own `environment` wins over everything.
    if let Some(node) = camera.and_then(|id| tree.get_node(id)) {
        let path = resource_path(node, "environment");
        if !path.is_empty() {
            return path;
        }
    }

    // 2. The first `WorldEnvironment` in tree order, as Godot takes the first to
    //    enter the world.
    for id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        if !node.is_class("WorldEnvironment") {
            continue;
        }
        let path = resource_path(node, "environment");
        if !path.is_empty() {
            return path;
        }
    }

    // 3. The project default.
    project_setting_string("rendering/environment/defaults/default_environment")
}

/// A `String` project setting, or empty.
///
/// ⚠️ Reads through [`FileAccess`] rather than a cached `ProjectSettings`, so it
/// resolves against whatever `res://` root the caller installed and a mounted
/// `.pck` serves it — the idiom `project_base_theme` uses (`canvas_render.rs:2154`).
///
/// 🔑 The non-empty filter is load-bearing, not defensive.
/// `default_environment` has a **registered default of the empty string**
/// (`project_settings.rs:1307`), and `ProjectSettings::get` falls back to the
/// defaults table (`:205-209`), so the key resolves for every project whether or
/// not it was authored. Without the filter every project would name an
/// environment at `""`.
fn project_setting_string(key: &str) -> String {
    let Ok(text) = FileAccess::read_file_string("res://project.godot") else {
        return String::new();
    };
    let Ok(settings) = gdcore::project_settings::ProjectSettings::from_godot_format(&text) else {
        return String::new();
    };
    settings
        .get(key)
        .and_then(|v| v.as_string())
        .filter(|s| !s.is_empty())
        // `as_string` lends a `&str` out of `settings`, which is local to this
        // function, so the path has to be owned to leave it.
        .map(str::to_string)
        .unwrap_or_default()
}

/// Parses an `Environment` `.tres`, resolving its nested `Sky` sub-resource.
///
/// A sky's panorama / cover textures are **not** loaded here: that needs the
/// ext-resource path table and an image decoder, and `Sky` renders its
/// procedural layers without them — the same miss the editor's resolver tolerates
/// when it has no project root.
pub fn environment_from_tres(text: &str, path: &str) -> Option<Environment3D> {
    let resource = gdresource::loader::TresLoader::new()
        .parse_str(text, path)
        .ok()?;

    let mut env =
        Environment3D::from_properties(resource.properties().map(|(k, v)| (k.as_str(), v)));

    // `sky = SubResource("Sky_x")` → the `[sub_resource type="Sky"]` section,
    // whose `sky_material` names the material section in turn.
    if let Some(sky_res) = resource.resolve_subresource("sky") {
        let material = match sky_res.resolve_subresource("sky_material") {
            Some(mat) => {
                let props = mat.properties().map(|(k, v)| (k.as_str(), v));
                match mat.class_name.as_str() {
                    "PhysicalSkyMaterial" => SkyMaterial::Physical(
                        gdserver3d::sky::PhysicalSkyMaterial::from_properties(props),
                    ),
                    "PanoramaSkyMaterial" => SkyMaterial::Panoramic(
                        gdserver3d::sky::PanoramicSkyMaterial::from_properties(props),
                    ),
                    // Procedural is Godot's default sky material and the
                    // fallback for anything this loader does not model (a
                    // `ShaderMaterial` sky, say), which renders a plain gradient
                    // rather than black.
                    _ => SkyMaterial::Procedural(
                        gdserver3d::sky::ProceduralSkyMaterial::from_properties(props),
                    ),
                }
            }
            None => SkyMaterial::Procedural(gdserver3d::sky::ProceduralSkyMaterial::default()),
        };
        env.sky = Some(Sky {
            material,
            ..Sky::default()
        });
    }

    Some(env)
}

/// The environment a running game renders this scene with, or `None` when there
/// is nothing to say — no authored environment and no project to take a clear
/// colour from.
///
/// `camera` should be the camera the frame renders through, so its own
/// `environment` can win; pass `None` to skip that step.
///
/// # The `None` case is deliberate, and it is what keeps this additive
///
/// Returning an all-default `Environment3D` instead would be *more* Godot-exact —
/// Godot clears to grey and lights nothing when a scene authors neither an
/// environment nor a light. But Patina's Play view falls back to the editor's
/// preview sky and preview sun, and nine fixtures across `gdeditor/tests` build
/// their scene in Rust with no `res://` root, so they have no project and no
/// authored environment. Answering `None` there leaves every one of them
/// rendering exactly as before: the behaviour changes only for a scene (or
/// project) that actually asked for an environment, which is the gap this bead
/// names. Removing the preview sun unconditionally is a separate change with
/// those nine fixtures in its blast radius.
pub fn runtime_environment_3d(tree: &SceneTree, camera: Option<NodeId>) -> Option<Environment3D> {
    let path = scene_environment_path(tree, camera);
    if !path.is_empty() {
        match FileAccess::read_file_string(&path) {
            Ok(text) => {
                if let Some(env) = environment_from_tres(&text, &path) {
                    return Some(env);
                }
                tracing::warn!(
                    environment = %path,
                    "the scene names an Environment resource that could not be parsed; \
                     falling back to the project clear colour"
                );
            }
            Err(e) => tracing::warn!(
                environment = %path,
                error = %e,
                "the scene names an Environment resource that cannot be read; \
                 falling back to the project clear colour"
            ),
        }
    }

    // No authored environment: Godot clears to the project's
    // `default_clear_color`. Gated on the project being READABLE, so a scene
    // assembled in Rust with no `res://` root is unaffected — see the note above.
    let text = FileAccess::read_file_string("res://project.godot").ok()?;
    let settings = gdcore::project_settings::ProjectSettings::from_godot_format(&text).ok()?;
    Some(Environment3D {
        // `CustomColor`, not `ClearColor`: the renderer's `ClearColor` arm is
        // hard black (`gdrender3d/src/renderer.rs:1700-1710`, pinned by
        // `clear_color_mode_is_black`), because at that layer there is no
        // project to ask. Carrying the colour as a custom background is how the
        // project setting reaches a renderer that does not know about projects.
        background_mode: BackgroundMode::CustomColor,
        background_color: settings.default_clear_color(),
        ..Environment3D::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Color;

    const RED_ENV: &str = r#"[gd_resource type="Environment" load_steps=2 format=3]

[resource]
background_mode = 1
background_color = Color(1, 0, 0, 1)
"#;

    #[test]
    fn a_tres_background_colour_is_read() {
        let env = environment_from_tres(RED_ENV, "res://env.tres").expect("parses");
        assert_eq!(env.background_mode, BackgroundMode::CustomColor);
        assert_eq!(env.background_color, Color::new(1.0, 0.0, 0.0, 1.0));
    }

    #[test]
    fn a_tres_sky_sub_resource_is_resolved() {
        let text = r#"[gd_resource type="Environment" load_steps=3 format=3]

[sub_resource type="ProceduralSkyMaterial" id="ProceduralSkyMaterial_1"]

[sub_resource type="Sky" id="Sky_1"]
sky_material = SubResource("ProceduralSkyMaterial_1")

[resource]
background_mode = 2
sky = SubResource("Sky_1")
"#;
        let env = environment_from_tres(text, "res://sky_env.tres").expect("parses");
        assert_eq!(env.background_mode, BackgroundMode::Sky);
        let sky = env.sky.expect("the nested Sky resolves");
        assert!(matches!(sky.material, SkyMaterial::Procedural(_)));
    }

    /// The companion key is what carries the resolved path; the property itself
    /// still holds the unresolved reference.
    #[test]
    fn an_extresource_reference_resolves_through_the_companion_key() {
        let mut node = Node::new("World", "WorldEnvironment");
        node.set_property("environment", Variant::String("ExtResource:1_e".into()));
        assert_eq!(
            resource_path(&node, "environment"),
            "",
            "an unresolved ExtResource reference is not a path"
        );
        node.set_property(
            "__extres__environment",
            Variant::String("res://env.tres".into()),
        );
        assert_eq!(resource_path(&node, "environment"), "res://env.tres");
    }
}

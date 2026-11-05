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
        let mut material = match sky_res.resolve_subresource("sky_material") {
            Some(mat) => {
                let props: std::collections::HashMap<String, Variant> = mat
                    .properties()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let ext = |id: &str| resource.ext_resources.get(id).map(|e| e.path.clone());
                sky_material_from_props(&mat.class_name, &props, &ext, &|_| None)
            }
            None => SkyMaterial::Procedural(gdserver3d::sky::ProceduralSkyMaterial::default()),
        };
        let ext = |id: &str| resource.ext_resources.get(id).map(|e| e.path.clone());
        load_sky_textures(&mut material, &ext);
        env.sky = Some(Sky {
            material,
            ..Sky::default()
        });
    }

    Some(env)
}

/// Builds a `Sky.sky_material` from its class and property bag.
///
/// `ext` resolves an `ExtResource` id to its `res://` path and `sub` a
/// `SubResource` id to that sub-resource's properties (an inline `Shader`).
///
/// 🔑 A `ShaderMaterial` sky (pat-cezo4) becomes [`SkyMaterial::Shader`] with
/// its shader source -- read from the `.gdshader` file an `ExtResource` shader
/// names -- and its `shader_parameter/*` values, any resource reference among
/// them turned into a path. It used to fall to the default procedural sky.
fn sky_material_from_props(
    class: &str,
    props: &std::collections::HashMap<String, Variant>,
    ext: &dyn Fn(&str) -> Option<String>,
    sub: &dyn Fn(&str) -> Option<std::collections::HashMap<String, Variant>>,
) -> SkyMaterial {
    let pairs = || props.iter().map(|(k, v)| (k.as_str(), v));
    match class {
        "PhysicalSkyMaterial" => {
            SkyMaterial::Physical(gdserver3d::sky::PhysicalSkyMaterial::from_properties(pairs()))
        }
        "PanoramaSkyMaterial" => {
            SkyMaterial::Panoramic(gdserver3d::sky::PanoramicSkyMaterial::from_properties(pairs()))
        }
        "ShaderMaterial" => {
            let code = match props.get("shader") {
                Some(Variant::String(r)) => {
                    if let Some(id) = r.strip_prefix("ExtResource:") {
                        ext(id).and_then(|path| FileAccess::read_file_string(&path).ok())
                    } else if let Some(id) = r.strip_prefix("SubResource:") {
                        sub(id).and_then(|p| match p.get("code") {
                            Some(Variant::String(c)) => Some(c.clone()),
                            _ => None,
                        })
                    } else {
                        None
                    }
                }
                _ => None,
            }
            .unwrap_or_default();
            let mut m = gdserver3d::shader::ShaderMaterial3D::new();
            m.shader = Some(gdserver3d::shader::Shader3D::new(
                gdserver3d::shader::ShaderType3D::Sky,
                &code,
            ));
            for (k, v) in props {
                let Some(name) = k.strip_prefix("shader_parameter/") else {
                    continue;
                };
                let v = match v {
                    Variant::String(r) if r.starts_with("ExtResource:") => {
                        match ext(&r["ExtResource:".len()..]) {
                            Some(path) => Variant::String(path),
                            None => continue,
                        }
                    }
                    Variant::Nil => continue,
                    other => other.clone(),
                };
                m.set_shader_parameter(name, v);
            }
            SkyMaterial::Shader(gdserver3d::sky::ShaderSkyMaterial::new(m))
        }
        // Procedural is Godot's default sky material and the fallback for
        // anything this loader does not model, which renders a plain gradient
        // rather than black.
        _ => SkyMaterial::Procedural(gdserver3d::sky::ProceduralSkyMaterial::from_properties(
            pairs(),
        )),
    }
}

/// Decodes the textures a sky material references (panorama, sky cover,
/// night sky, a shader sky's cubemaps), and bakes a cubemap shader sky into
/// the equirect panorama the GPU sky pass samples (pat-cezo4).
fn load_sky_textures(material: &mut SkyMaterial, ext: &dyn Fn(&str) -> Option<String>) {
    gdserver3d::sky::resolve_sky_material_textures(material, |reference| {
        let path = match reference.strip_prefix("ExtResource:") {
            Some(id) => ext(id)?,
            None => reference.to_string(),
        };
        let resolved = if path.starts_with("res://") || path.starts_with("user://") {
            gdcore::resolve_path(&path).ok()?
        } else {
            std::path::PathBuf::from(&path)
        };
        let (width, height, pixels) =
            gdresource::importers::load_color_texture_linear(&resolved).ok()?;
        Some(gdserver3d::sky::PanoramaTexture {
            width,
            height,
            pixels,
        })
    });
    if let SkyMaterial::Shader(m) = material {
        if !m.cubemaps.is_empty() {
            m.bake_panorama(SKY_BAKE_WIDTH, SKY_BAKE_WIDTH / 2);
        }
    }
}

/// Width of the equirect panorama a cubemap shader sky is baked into. A
/// 512-texel cube face spans a quarter turn, so 1024 keeps about half a texel per
/// face texel at the horizon.
const SKY_BAKE_WIDTH: u32 = 1024;

thread_local! {
    /// Resolved inline environments by (scene file, sub-resource id). Parsing
    /// the scene and baking its sky every frame would dominate the frame; the
    /// `Arc`'d sky bake is also what lets the GPU keep its upload.
    static INLINE_ENV_CACHE: std::cell::RefCell<
        std::collections::HashMap<(String, String), Option<InlineEnvBase>>,
    > = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// The scene file a node was authored in: the `_instance_path` of the node
/// that owns it (an instanced sub-scene's root), else the project's main scene.
/// The node property holding the `res://` file a script-instantiated scene's
/// root was built from (Godot's `Node.scene_file_path`; pat-i4jgd).
pub const SCENE_FILE_PATH_KEY: &str = "_scene_file_path";

fn authoring_scene_path(tree: &SceneTree, id: NodeId) -> Option<String> {
    let node = tree.get_node(id)?;
    let owner = node.owner().and_then(|o| tree.get_node(o)).unwrap_or(node);
    // `_instance_path` marks the root of an `instance=` sub-scene;
    // `_scene_file_path` (Godot's `Node.scene_file_path`) the root of a scene a
    // SCRIPT instantiated -- truck_town's `preload("town_scene.tscn")
    // .instantiate()`, which carries none of the former (pat-i4jgd). Without it
    // the lookup fell back to the main scene, car_select.tscn, which has no
    // such Environment, and the town played with the project clear colour.
    let file = |key: &str| match owner.get_property(key) {
        Variant::String(s) if !s.is_empty() => Some(s),
        _ => None,
    };
    match file("_instance_path").or_else(|| file(SCENE_FILE_PATH_KEY)) {
        Some(s) => Some(s),
        None => {
            let main = project_setting_string("application/run/main_scene");
            (!main.is_empty()).then_some(main)
        }
    }
}

/// The environment an inline `environment = SubResource("Environment_x")`
/// describes, on the camera or the first `WorldEnvironment` (pat-cezo4).
///
/// 🔑 By render time the `PackedScene` is gone; the node keeps only
/// `__resref__environment = "Environment::<id>"`. So the scene the node was
/// authored in is read again (once, cached) to resolve the Environment, its
/// Sky and the sky material. Before this a scene with an inline environment
/// -- the official 3D platformer's `stage.tscn` -- played with the project
/// clear colour: no sky, no fog, no AgX, no ambient colour.
fn inline_scene_environment(tree: &SceneTree, camera: Option<NodeId>) -> Option<Environment3D> {
    let candidates = camera.into_iter().chain(
        tree.all_nodes_in_tree_order()
            .into_iter()
            .filter(|id| tree.get_node(*id).is_some_and(|n| n.is_class("WorldEnvironment"))),
    );
    for id in candidates {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        let sub_id = match node.get_property("__resref__environment") {
            Variant::String(s) => match s.rsplit_once("::") {
                Some((_, sid)) if !sid.is_empty() => sid.to_string(),
                _ => continue,
            },
            _ => continue,
        };
        let Some(scene_path) = authoring_scene_path(tree, id) else {
            continue;
        };
        let key = (scene_path.clone(), sub_id.clone());
        let cached = match INLINE_ENV_CACHE.with(|c| c.borrow().get(&key).cloned()) {
            Some(hit) => hit,
            None => {
                let base = environment_from_scene_file(&scene_path, &sub_id);
                INLINE_ENV_CACHE.with(|c| c.borrow_mut().insert(key, base.clone()));
                base
            }
        };
        let (mut props, sky) = cached?;
        // 🔑 A Godot resource is SHARED: `$WorldEnvironment.environment.
        // fog_light_color = c` (truck_town's `set_mood`) changes what renders.
        // Patina lands such writes on the node's `__subres__environment__*`
        // companions (the interpreter's sub-resource member write), which also
        // hold the file's body verbatim -- so they are the live values and
        // win over the file read (pat-i4jgd).
        let prefix = "__subres__environment__";
        for (k, v) in node.properties() {
            if let Some(field) = k.strip_prefix(prefix) {
                if field != "sky" {
                    props.insert(field.to_string(), v.clone());
                }
            }
        }
        let mut env = Environment3D::from_properties(props.iter().map(|(k, v)| (k.as_str(), v)));
        env.sky = sky;
        return Some(env);
    }
    None
}

/// An inline Environment as its scene file declares it: the property bag and
/// the resolved (texture-loaded, baked) sky.
type InlineEnvBase = (std::collections::HashMap<String, Variant>, Option<Sky>);

/// A sky material saved as its own `.tres` (`sky_material = ExtResource(..)`,
/// truck_town's `sky_day.tres`), read with that file's own ext-resource table.
fn sky_material_from_tres(path: &str) -> Option<SkyMaterial> {
    let text = FileAccess::read_file_string(path).ok()?;
    let res = gdresource::loader::TresLoader::new().parse_str(&text, path).ok()?;
    let props: std::collections::HashMap<String, Variant> =
        res.properties().map(|(k, v)| (k.clone(), v.clone())).collect();
    let ext = |id: &str| res.ext_resources.get(id).map(|e| e.path.clone());
    let mut material = sky_material_from_props(&res.class_name, &props, &ext, &|_| None);
    load_sky_textures(&mut material, &ext);
    Some(material)
}

/// Builds the Environment sub-resource `env_id` of the scene file at `path`.
fn environment_from_scene_file(path: &str, env_id: &str) -> Option<InlineEnvBase> {
    let text = crate::binary_scene::read_scene_source(path)?;
    let scene = crate::packed_scene::PackedScene::from_tscn(&text).ok()?;
    let props = scene.sub_resource_props(env_id)?;
    let mut env_sky = None;
    let sub_id = |v: Option<&Variant>| match v {
        Some(Variant::String(s)) => s.strip_prefix("SubResource:").map(str::to_string),
        _ => None,
    };
    let ext = |id: &str| scene.ext_resources().get(id).map(|e| e.path.clone());
    let sub = |id: &str| scene.sub_resource_props(id).cloned();
    if let Some(sky_props) = sub_id(props.get("sky")).and_then(|id| scene.sub_resource_props(&id))
    {
        // `sky_material = ExtResource(..)` names a material `.tres`
        // (pat-i4jgd). It used to fall to the DEFAULT procedural sky, which
        // drew truck_town's blue day sky as Godot's grey default.
        let ext_material = match sky_props.get("sky_material") {
            Some(Variant::String(r)) => r
                .strip_prefix("ExtResource:")
                .and_then(|id| ext(id))
                .and_then(|p| sky_material_from_tres(&p)),
            _ => None,
        };
        let material = match ext_material {
            Some(m) => m,
            None => {
                let mut material = match sub_id(sky_props.get("sky_material")) {
                    Some(mid) => {
                        match (scene.sub_resource_type(&mid), scene.sub_resource_props(&mid)) {
                            (Some(class), Some(mp)) => {
                                sky_material_from_props(class, mp, &ext, &sub)
                            }
                            _ => SkyMaterial::Procedural(
                                gdserver3d::sky::ProceduralSkyMaterial::default(),
                            ),
                        }
                    }
                    None => {
                        SkyMaterial::Procedural(gdserver3d::sky::ProceduralSkyMaterial::default())
                    }
                };
                load_sky_textures(&mut material, &ext);
                material
            }
        };
        env_sky = Some(Sky {
            material,
            ..Sky::default()
        });
    }
    Some((props.clone(), env_sky))
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

    // An inline `environment = SubResource(..)` on a WorldEnvironment
    // (pat-cezo4) -- the form the official 3D platformer ships.
    if let Some(env) = inline_scene_environment(tree, camera) {
        return Some(env);
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

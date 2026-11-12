//! pat-i4jgd: truck_town's town must render lit in editor Play, as in Godot.
//!
//! Picking a car in `car_select.tscn` instantiates `town/town_scene.tscn` from a
//! script. From then on Patina's Play frame (Godot reference, `playtest-gate`
//! f258) was ~99% wrong, mean delta ~221: one dark surface with the car
//! silhouette. Four independent causes, each asserted here on the render
//! pipeline's own inputs (`viewport_3d_render::game_3d_render_inputs` and the
//! environment `prepare_play_3d_frame` installs), not on pixels:
//!
//! 1. Every node of the multi-mesh `town_model.gltf` drew the WHOLE file: the
//!    editor's mesh loader ignored `gltf_mesh_index`. The copy placed at
//!    `HugeTireMesh` enclosed the camera.
//! 2. `town/tree/scene.gltf.import` sets `nodes/root_scale=0.01` (Sketchfab,
//!    centimetres), which was ignored: every tree was 100x too big and its
//!    blended canopy hid the town.
//! 3. The WorldEnvironment's inline `Environment` was never found: the town's
//!    root, instantiated by `preload(..).instantiate()`, carried no scene file
//!    path, so the resolver re-read car_select.tscn, and the Sky's
//!    `sky_material = ExtResource(sky_day.tres)` fell to the default material.
//!    Play cleared to the project grey with linear tonemapping and no fog.
//! 4. `surface_material_override/0 = ExtResource("grass.tres")` kept the
//!    `.tres`'s own `ExtResource("1_5bi3i")` id for its albedo texture, and the
//!    tree's extracted materials (`materials/extract=1`) were not read: the
//!    ground and leaves drew untextured.
//!
//! Drives the unmodified `games/truck_town` through the editor binary's launch
//! and Play path, exactly like `truck_town_loads_the_town_in_editor_play_test`
//! (which is why this file holds ONE test).

use gdeditor::editor_server::{
    apply_project_path_roots, build_play_tree, load_launch_scene, EditorState,
};
use gdeditor::viewport_3d_render as v3r;
use gdscene::MainLoop;
use gdserver3d::environment::{BackgroundMode, ToneMapper};
use gdserver3d::material::{CullMode, Transparency};
use gdserver3d::sky::SkyMaterial;
use gdvariant::Variant;
use std::path::Path;

const TOWN: &str = "/root/TownScene";

#[test]
fn the_town_renders_with_its_meshes_trees_environment_and_textures() {
    let game = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../games/truck_town")
        .canonicalize()
        .unwrap();
    let root = game.to_string_lossy().to_string();
    apply_project_path_roots(&root);
    let scene = game.join("car_select/car_select.tscn");
    let (tree, _r, _s) = load_launch_scene(&scene.to_string_lossy()).expect("opens");
    let mut state = EditorState::new(tree);
    state.scene_file = Some(scene.to_string_lossy().to_string());
    state.texture_cache = gdscene::texture_cache::TextureCache::new(&root);
    state.project_root = Some(root.clone());
    let play = build_play_tree(&mut state, None);
    let mut ml = MainLoop::new(play);
    ml.step(1.0 / 60.0);
    let car_select = ml.tree().get_node_by_path("/root/CarSelect").unwrap();
    ml.tree_mut()
        .call_script_method(car_select, "_on_mini_van_pressed", &[]);
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
    }
    let tree = ml.tree();
    assert!(tree.get_node_by_path(TOWN).is_some(), "the town must load");
    let node = |p: &str| {
        tree.get_node_by_path(&format!("{TOWN}/{p}"))
            .unwrap_or_else(|| panic!("{TOWN}/{p}"))
    };

    // The Play frame, built the way `prepare_play_3d_frame` builds it.
    let cam = v3r::game_camera_for_play(tree, false).expect("a current Camera3D");
    assert_eq!(
        tree.node_path(cam).as_deref(),
        Some("/root/TownScene/InstancePos/car/Body/CameraBase/Camera3D")
    );
    let mut vp = v3r::viewport_for_scene_camera(tree, cam, 1152, 648);
    assert!(v3r::apply_runtime_environment_3d(tree, cam, &mut vp));
    let inputs = v3r::game_3d_render_inputs(tree, cam, &vp);
    let item = |p: &str| {
        let id = node(p).raw();
        inputs
            .items
            .iter()
            .find(|i| i.node_id == id)
            .unwrap_or_else(|| panic!("{p} is not in the render list"))
    };
    let path_of = |p: &str| match &item(p).kind {
        v3r::Render3DKind::Mesh { path } => path.clone(),
        other => panic!("{p} draws {other:?}, not its glTF mesh"),
    };

    // 1. Each glTF node draws ITS mesh (town_model.gltf has 35).
    let verts = |p: &str| v3r::mesh_for_primitive(&path_of(p)).vertex_count();
    let gltf = game.join("town/model/town_model.gltf");
    let doc = gdresource::import_gltf(&gltf).unwrap();
    // The geometry of the node's OWN glTF mesh, straight from the document.
    let own = |p: &str| {
        let index = match tree
            .get_node(node(p))
            .unwrap()
            .get_property("gltf_mesh_index")
        {
            Variant::Int(i) => i,
            other => panic!("{p} has no gltf_mesh_index: {other:?}"),
        };
        gdscene::render_server_3d::mesh3d_from_gltf_resource_for_mesh(&doc, Some(index))
            .unwrap()
            .vertex_count()
    };
    let tire = verts("TownModel/Racetrack/HugeTire/HugeTireMesh");
    let grass = verts("TownModel/Terrain/GrassMesh");
    let road = verts("TownModel/Terrain/RoadMesh");
    assert_eq!(
        tire,
        own("TownModel/Racetrack/HugeTire/HugeTireMesh"),
        "HugeTireMesh must draw the huge tire only"
    );
    assert_eq!(grass, own("TownModel/Terrain/GrassMesh"));
    assert_eq!(road, own("TownModel/Terrain/RoadMesh"));
    assert_ne!(
        tire, grass,
        "HugeTireMesh and GrassMesh drew the same geometry"
    );

    // 2. root_scale=0.01: a tree's mesh sits at the authored instance scale
    //    (Tree: 0.375) times the file's own x100 `tree` node times 0.01.
    let leaves = "TownModel/Trees/Tree/Sketchfab_model/tree0_1_fbx/RootNode/tree/leaves/leaves_Leav material_0";
    let basis = item(leaves).global_transform.basis;
    let scale = basis.x.length().max(basis.y.length()).max(basis.z.length());
    assert!(
        (scale - 0.375).abs() < 1e-3,
        "the tree's leaves draw at scale {scale}, Godot's is 0.375 (root_scale 0.01)"
    );

    // 3. The inline Environment, its Sky and the sky_day.tres material.
    let env = &vp.environment.environment;
    assert_eq!(env.background_mode, BackgroundMode::Sky);
    assert_eq!(env.tone_mapper, ToneMapper::Agx, "tonemap_mode = 4");
    assert!(env.fog_enabled, "fog_enabled = true");
    assert!((env.fog_density - 0.0015).abs() < 1e-6);
    assert!((env.glow_intensity - 0.5).abs() < 1e-6);
    let sky = env.sky.as_ref().expect("the Environment's Sky");
    match &sky.material {
        SkyMaterial::Procedural(m) => {
            // sky_day.tres: sky_top_color = Color(7.123351e-06, 0.6897872, 0.89266914, 1)
            assert!(
                (m.sky_top_color.g - 0.6897872).abs() < 1e-5,
                "{:?}",
                m.sky_top_color
            );
            assert!((m.sky_top_color.b - 0.89266914).abs() < 1e-5);
        }
        other => panic!("sky_day.tres is a ProceduralSkyMaterial, got {other:?}"),
    }
    // The sun, as the scene authors it (Godot's -Z of the row-major basis).
    let sun = &inputs.scene_lights[0];
    assert!((sun.energy - 1.45).abs() < 1e-6);
    let want = gdcore::math::Vector3::new(0.496732, -0.819152, 0.286788);
    assert!(
        (sun.direction - want).length() < 1e-4,
        "{:?}",
        sun.direction
    );
    assert!(sun.shadow_enabled);

    // 4. Textures on the draw items.
    let tex = |p: &str| {
        item(p)
            .material
            .as_ref()
            .and_then(|m| m.albedo_texture.as_ref())
            .map(|t| (t.width, t.height))
    };
    assert_eq!(
        tex("TownModel/Terrain/GrassMesh"),
        Some((1024, 1024)),
        "grass.tres -> grass.webp"
    );
    assert_eq!(
        tex("TownModel/Terrain/RoadMesh"),
        Some((1024, 1024)),
        "cement.tres -> cement.webp"
    );
    // Leav_material.tres (extracted): texture, transparency = 4, cull_mode = 2.
    let leaf = item(leaves).material.as_ref().expect("leaves material");
    assert_eq!(
        tex(leaves),
        Some((512, 1024)),
        "Leav_material_baseColor.png"
    );
    assert_ne!(leaf.transparency, Transparency::Disabled);
    assert_eq!(leaf.cull_mode, CullMode::Disabled);
    // A script write into the shared Environment reaches the frame.
    let we = node("WorldEnvironment");
    let probe = gdcore::math::Color::new(0.25, 0.5, 0.75, 1.0);
    ml.tree_mut().get_node_mut(we).unwrap().set_property(
        "__subres__environment__fog_light_color",
        Variant::Color(probe),
    );
    let tree = ml.tree();
    let env2 = gdscene::world_environment::runtime_environment_3d(tree, Some(cam)).unwrap();
    assert_eq!(
        env2.fog_light_color, probe,
        "`environment.fog_light_color = c` must render"
    );
}

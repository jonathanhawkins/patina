//! pat-zwvgw / pat-cezo4: the Play frame of the official 3D platformer demo
//! (`games/platformer_3d`, unmodified) carries the materials, environment and
//! effects Godot draws it with.
//!
//! Driven exactly the way the editor binary runs Play: the project is opened
//! through the editor server, `game.tscn` is played, and the frame is prepared
//! with `prepare_play_3d_frame` from the running tree. The assertions sit on the
//! render-pipeline structs (the draw items and the resolved environment), not on
//! sampled pixels.
//!
//! `PATINA_PLATFORMER_FRAME_OUT=<file.png>` additionally renders the frame and
//! writes it, for a whole-frame comparison against a real Godot capture of the
//! same camera (see the bead notes).

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::play_3d::{Play3DScene, PreparedPlay3DFrame};
use gdeditor::viewport_3d_render::{Render3DItem, Render3DKind};
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

fn request(port: u16, method: &str, path: &str, body: &str) -> String {
    let mut s = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(120))).ok();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out.split("\r\n\r\n").nth(1).unwrap_or("").to_string()
}

fn project() -> String {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/platformer_3d")
        .canonicalize()
        .expect("games/platformer_3d fixture")
        .to_string_lossy()
        .into_owned()
}

/// Plays `game.tscn` and prepares one Play frame at `w`x`h` after ~`frames`
/// game frames.
/// The prepared frame, and the scene path of every draw item's node.
fn play_frame(w: u32, h: u32) -> (PreparedPlay3DFrame, std::collections::HashMap<u64, String>) {
    gdobject::class_db::register_all_classes();
    let project = project();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let handle = EditorServerHandle::start(port, EditorState::new(SceneTree::new()));
    std::thread::sleep(Duration::from_millis(100));
    request(
        port,
        "POST",
        "/api/project/open",
        &format!(r#"{{"path":"{project}"}}"#),
    );
    let load = request(
        port,
        "POST",
        "/api/scene/load",
        &format!(r#"{{"path":"{project}/game.tscn"}}"#),
    );
    assert!(load.contains("\"ok\""), "load: {load}");
    let r = request(port, "POST", "/api/runtime/play", "{}");
    assert!(r.contains("\"ok\":true"), "play: {r}");
    // Let the player land and the follow camera settle.
    std::thread::sleep(Duration::from_millis(1500));
    let (prepared, paths) = {
        let state = handle.state();
        let mut st = state.lock().unwrap();
        // `PATINA_PLATFORMER_CAMERA="ex ey ez tx ty tz"` moves the game camera to
        // the pose the Godot reference capture used, so the two frames are
        // comparable whole (the follow camera settles differently in each
        // engine). Moved in the TREE, before the frame is prepared, so
        // billboards face it too.
        if let Ok(pose) = std::env::var("PATINA_PLATFORMER_CAMERA") {
            let v: Vec<f32> = pose.split_whitespace().map(|x| x.parse().unwrap()).collect();
            let eye = gdcore::math::Vector3::new(v[0], v[1], v[2]);
            let target = gdcore::math::Vector3::new(v[3], v[4], v[5]);
            let basis = gdcore::math3d::Basis::looking_at(
                target - eye,
                gdcore::math::Vector3::new(0.0, 1.0, 0.0),
            );
            let ml = st.run_main_loop.as_mut().expect("running");
            let cam = gdeditor::viewport_3d_render::game_camera_for_play(ml.tree(), false)
                .expect("camera");
            gdscene::node3d::set_global_transform(
                ml.tree_mut(),
                cam,
                gdcore::math3d::Transform3D { basis, origin: eye },
            );
        }
        let tree = st
            .run_main_loop
            .as_ref()
            .map(|ml| ml.tree())
            .unwrap_or(&st.scene_tree);
        let clear = st
            .run_project_settings
            .as_ref()
            .unwrap_or(&st.project_settings)
            .default_clear_color();
        let prepared = gdeditor::play_3d::prepare_play_3d_frame(
            tree,
            &st.editor_viewport_3d,
            st.runtime_camera_override,
            clear,
            w,
            h,
        );
        let mut paths = std::collections::HashMap::new();
        if let Play3DScene::Camera { inputs, .. } = &prepared.scene {
            for it in &inputs.items {
                let nid = gdscene::node::NodeId::from_object_id(gdcore::id::ObjectId::from_raw(
                    it.node_id,
                ));
                if let Some(path) = tree.node_path(nid) {
                    paths.insert(it.node_id, path);
                }
            }
        }
        (prepared, paths)
    };
    request(port, "POST", "/api/runtime/stop", "");
    handle.stop();
    (prepared, paths)
}

fn mesh_items<'a>(items: &'a [Render3DItem], file: &str) -> Vec<&'a Render3DItem> {
    items
        .iter()
        .filter(|i| matches!(&i.kind, Render3DKind::Mesh { path } if path.ends_with(file)))
        .collect()
}

fn maybe_dump(prepared: &PreparedPlay3DFrame) {
    let Ok(out) = std::env::var("PATINA_PLATFORMER_FRAME_OUT") else {
        return;
    };
    let img = prepared.render();
    let mut fb = gdrender2d::renderer::FrameBuffer::new(
        img.width,
        img.height,
        gdcore::math::Color::BLACK,
    );
    for (i, px) in img.data.chunks_exact(4).enumerate() {
        fb.pixels[i] = gdcore::math::Color::new(
            px[2] as f32 / 255.0,
            px[1] as f32 / 255.0,
            px[0] as f32 / 255.0,
            1.0,
        );
    }
    fb.save_png(&out).expect("write frame");
    if let Play3DScene::Camera { vp, .. } = &prepared.scene {
        eprintln!("camera: {:?}", vp.camera.override_transform);
    }
}

/// 🔑 pat-zwvgw: every level tile draws with the material its ArrayMesh
/// surfaces carry — `tile_material.tres`, i.e. `tiles_albedo.webp` as the
/// albedo map and `tiles_rough.webp` as the roughness map.
///
/// Before the fix the `_surfaces[i].material` reference was decoded and never
/// read, so every GridMap cell and both `ExtraFloor` MeshInstance3Ds reached
/// the renderer with `material: None` and drew flat grey.
#[test]
fn level_tiles_carry_their_array_mesh_surface_material() {
    let (prepared, _) = play_frame(1280, 720);
    maybe_dump(&prepared);
    let Play3DScene::Camera { inputs, .. } = &prepared.scene else {
        panic!("game.tscn has a current Camera3D");
    };
    let items = &inputs.items;

    // floor.res is both a GridMap tile and the ExtraFloor1/2 mesh.
    let floors = mesh_items(items, "stage/meshes/floor.res");
    assert!(
        floors.len() > 100,
        "the level's floor cells must be in the render list ({} found)",
        floors.len()
    );
    let tile_meshes: Vec<&Render3DItem> = items
        .iter()
        .filter(|i| matches!(&i.kind, Render3DKind::Mesh { path } if path.contains("stage/meshes/")))
        .collect();
    let untextured: Vec<String> = tile_meshes
        .iter()
        .filter(|i| {
            i.material
                .as_ref()
                .and_then(|m| m.albedo_texture.as_ref())
                .is_none()
        })
        .map(|i| format!("{:?}", i.kind))
        .collect();
    assert!(
        untextured.is_empty(),
        "{} of {} tile draws have no albedo texture, e.g. {:?}",
        untextured.len(),
        tile_meshes.len(),
        untextured.first()
    );

    // The albedo map is tiles_albedo.webp (256x256), and the roughness map is
    // loaded too — both come from tile_material.tres, nowhere else.
    let mat = floors[0].material.as_ref().unwrap();
    let albedo = mat.albedo_texture.as_ref().unwrap();
    assert_eq!((albedo.width, albedo.height), (256, 256));
    assert!(
        mat.roughness_texture.is_some(),
        "tile_material.tres's roughness_texture must load"
    );
    // tile_material.tres authors no albedo_color / roughness: BaseMaterial3D's
    // defaults, white and 1.0.
    assert_eq!(mat.albedo, gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0));
    assert_eq!(mat.roughness, 1.0);
}

/// 🔑 pat-cezo4: Play renders with `stage.tscn`'s INLINE WorldEnvironment —
/// a sky background whose ShaderMaterial samples the `skybox.webp` cubemap,
/// AgX tonemapping, fog and a flat ambient colour.
///
/// Before the fix the runtime resolver read only an `ExtResource` `.tres`
/// environment; an inline `environment = SubResource(..)` fell through to the
/// project clear colour, so the game played on flat grey with none of the
/// authored environment, and a ShaderMaterial sky became the default
/// procedural one even where it was resolved.
#[test]
fn play_uses_the_inline_world_environment_and_its_cubemap_sky() {
    use gdserver3d::environment::{AmbientSource, BackgroundMode, ToneMapper};
    use gdserver3d::sky::SkyMaterial;
    let (prepared, _) = play_frame(320, 180);
    let Play3DScene::Camera { vp, .. } = &prepared.scene else {
        panic!("game.tscn has a current Camera3D");
    };
    let env = &vp.environment.environment;
    assert!(vp.environment.authored, "the scene's environment, not the preview");
    assert_eq!(env.background_mode, BackgroundMode::Sky);
    assert_eq!(env.tone_mapper, ToneMapper::Agx, "tonemap_mode = 4");
    assert!(env.fog_enabled);
    assert!((env.fog_density - 0.0015).abs() < 1e-6);
    assert_eq!(env.fog_sky_affect, 0.0);
    assert_eq!(env.ambient_source, AmbientSource::Color);
    assert!((env.ambient_color.r - 0.6).abs() < 1e-6);

    let sky = env.sky.as_ref().expect("Environment.sky");
    let SkyMaterial::Shader(mat) = &sky.material else {
        panic!("sky_material is a ShaderMaterial");
    };
    // The cubemap is skybox.webp (1024x1536, 2x3 faces of 512).
    let cube = mat
        .cubemaps
        .get("source_panorama")
        .expect("source_panorama samplerCube bound to skybox.webp");
    assert_eq!(cube.size, 512);
    // The shader samples it: straight up reads the +Y face's centre texel.
    let up = mat
        .eval_program(gdcore::math::Vector3::new(0.0, 1.0, 0.0))
        .expect("sky() runs");
    let face_centre = cube.faces[2][256 * 512 + 256];
    assert!(
        (up.r - face_centre.r).abs() < 0.02 && (up.b - face_centre.b).abs() < 0.02,
        "sky() up = {up:?}, +Y face centre = {face_centre:?}"
    );
    assert!(up.b > 0.05 && up.b > up.r, "the zenith of this sky is blue, got {up:?}");
    // The GPU sky pass samples the CPU bake.
    let baked = mat.baked.as_ref().expect("baked panorama for the GPU sky pass");
    assert_eq!((baked.width, baked.height), (1024, 512));
}

fn items_at<'a>(
    items: &'a [Render3DItem],
    paths: &std::collections::HashMap<u64, String>,
    path: &str,
) -> Vec<&'a Render3DItem> {
    items
        .iter()
        .filter(|i| paths.get(&i.node_id).is_some_and(|p| p == path))
        .collect()
}

/// 🔑 pat-cezo4: the coin's `GlowSprite` (a QuadMesh with an additive,
/// unshaded, alpha-blended, billboarded StandardMaterial3D whose albedo is an
/// inline radial `GradientTexture2D`) reaches the renderer as exactly that.
///
/// Before the fix the gradient sub-resource had no loader (no texture: a flat
/// tan quad), the GPU had only alpha blending (no `BLEND_MODE_ADD`), and
/// `billboard_mode` was never read, so the quad kept the coin's spinning basis.
#[test]
fn the_coin_glow_is_an_additive_billboard_with_its_gradient_texture() {
    use gdserver3d::material::{BlendMode, ShadingMode, Transparency};
    let (prepared, paths) = play_frame(320, 180);
    let Play3DScene::Camera { vp, inputs } = &prepared.scene else {
        panic!("game.tscn has a current Camera3D");
    };
    let glow = items_at(&inputs.items, &paths, "/root/Game/Coins/Coin1/GlowSprite");
    assert_eq!(glow.len(), 1, "one GlowSprite draw");
    let glow = glow[0];
    let mat = glow.material.as_ref().expect("surface_material_override/0");
    assert_eq!(mat.blend_mode, BlendMode::Add);
    assert_eq!(mat.transparency, Transparency::Alpha);
    assert_eq!(mat.shading_mode, ShadingMode::Unlit);
    let tex = mat.albedo_texture.as_ref().expect("GradientTexture2D albedo");
    assert_eq!((tex.width, tex.height), (64, 64));
    let at = |x: u32, y: u32| tex.pixels[(y * tex.width + x) as usize];
    assert!(at(32, 32).a > 0.95, "opaque centre: {:?}", at(32, 32));
    assert!(at(0, 0).a < 0.01, "clear corner: {:?}", at(0, 0));

    // Billboard: the draw's basis is the camera's, not the coin's.
    let cam = vp.camera.transform().basis;
    let b = glow.global_transform.basis;
    for (got, want) in [(b.x, cam.x), (b.y, cam.y), (b.z, cam.z)] {
        assert!(
            (got - want).length() < 1e-4,
            "billboard axis {got:?} must equal the camera's {want:?}"
        );
    }

    // The one-shot CPUParticles3D burst is not emitting: Godot draws nothing.
    assert!(
        items_at(&inputs.items, &paths, "/root/Game/Coins/Coin1/CPUParticles3D").is_empty(),
        "a non-emitting particle system must not draw its draw-pass mesh"
    );
}

/// 🔑 pat-zwvgw (glb half): the enemy's skinned `Body` draws with the glb's own
/// three materials — `enemy_red`, `enemy_black`, `enemy_glow` — one per
/// primitive, sRGB-encoded as Godot's glTF importer stores them.
///
/// Before the fix a glb mesh had no material at all on this path (the default
/// white), and the GPU drew only its first surface.
#[test]
fn the_enemy_glb_draws_with_its_gltf_materials_per_surface() {
    let (prepared, paths) = play_frame(320, 180);
    let Play3DScene::Camera { inputs, .. } = &prepared.scene else {
        panic!("game.tscn has a current Camera3D");
    };
    let body = items_at(
        &inputs.items,
        &paths,
        "/root/Game/Enemies/Enemy1/Enemy/Skeleton/Skeleton3D/Body",
    );
    assert_eq!(body.len(), 1);
    let body = body[0];
    let red = body.material.as_ref().expect("surface 0: enemy_red");
    assert!(
        (red.albedo.r - 1.0).abs() < 1e-4 && red.albedo.g == 0.0 && red.albedo.b == 0.0,
        "{:?}",
        red.albedo
    );
    assert!((red.roughness - 0.3).abs() < 1e-4);
    assert_eq!(red.metallic, 0.0);
    assert_eq!(body.surface_materials.len(), 2, "surfaces 1 and 2");
    let black = body.surface_materials[0].as_ref().expect("enemy_black");
    // baseColorFactor 0.1 linear -> sRGB 0.349.
    assert!((black.albedo.r - 0.349).abs() < 0.002, "{:?}", black.albedo);
    let glow = body.surface_materials[1].as_ref().expect("enemy_glow");
    // emissiveFactor (1, 0.5, 0.5) linear -> sRGB (1, 0.735, 0.735).
    assert!((glow.emission.r - 1.0).abs() < 1e-3 && (glow.emission.g - 0.735).abs() < 0.003);
}

/// pat-zwvgw: `player.glb.import` maps the glb's `player_gray` / `player_glow`
/// materials to the external `player_gray.tres` / `player_glow.tres`, and the
/// robot draws with those — as Godot's scene importer does — not with the
/// glb's embedded colours.
#[test]
fn the_robot_draws_with_the_external_materials_its_import_names() {
    let (prepared, paths) = play_frame(320, 180);
    let Play3DScene::Camera { inputs, .. } = &prepared.scene else {
        panic!("game.tscn has a current Camera3D");
    };
    let robot = items_at(
        &inputs.items,
        &paths,
        "/root/Game/Player/Player/Skeleton/Skeleton3D/Robot",
    );
    assert_eq!(robot.len(), 1);
    let gray = robot[0].material.as_ref().expect("surface 0: player_gray");
    // player_gray.tres: albedo 0.8, metallic 1, roughness 0.4 (the glb's own
    // player_gray is 0.4 linear with roughness 0.8).
    assert!((gray.albedo.r - 0.8).abs() < 1e-4, "{:?}", gray.albedo);
    assert_eq!(gray.metallic, 1.0);
    assert!((gray.roughness - 0.4).abs() < 1e-4);
    let glow = robot[0].surface_materials[0]
        .as_ref()
        .expect("surface 1: player_glow");
    assert!((glow.albedo.b - 1.75).abs() < 1e-4, "{:?}", glow.albedo);
}

/// 🔑 pat-3u1wu: `tile_material.tres` authors `texture_filter = 4`
/// (NEAREST_WITH_MIPMAPS_ANISOTROPIC) and `texture_repeat = false`, and the
/// tile draws reach the renderer with both — and the GPU sampler built for
/// them is nearest/nearest with a linear mip filter, clamped, as Godot's
/// `material_storage.cpp` configures it. Before the fix neither key was read
/// and the GPU sampled every material bilinear, so the pixel-art tiles were
/// blurred against Godot's crisp ones.
#[test]
fn level_tiles_sample_nearest_and_clamped_as_their_material_authors() {
    use gdserver3d::material::TextureFilter;
    let (prepared, _) = play_frame(320, 180);
    let Play3DScene::Camera { inputs, .. } = &prepared.scene else {
        panic!("game.tscn has a current Camera3D");
    };
    let floors = mesh_items(&inputs.items, "stage/meshes/floor.res");
    assert!(!floors.is_empty(), "floor tiles in the render list");
    let mat = floors[0].material.as_ref().expect("tile material");
    assert_eq!(mat.texture_filter, TextureFilter::NearestWithMipmapsAnisotropic);
    assert!(!mat.texture_repeat, "texture_repeat = false");

    #[cfg(feature = "gpu-render")]
    {
        use gdrender3d::wgpu::{AddressMode, FilterMode, MipmapFilterMode};
        use gdrender3d::wgpu_pipeline::{material_sampler_descriptor, MaterialSamplerKey};
        let d = material_sampler_descriptor(MaterialSamplerKey::of(mat));
        assert_eq!((d.mag_filter, d.min_filter), (FilterMode::Nearest, FilterMode::Nearest));
        assert_eq!(d.mipmap_filter, MipmapFilterMode::Linear);
        assert_eq!(d.address_mode_u, AddressMode::ClampToEdge);
        // CONTROL: a material that authors nothing keeps Godot's default
        // LINEAR_WITH_MIPMAPS + repeat, so the tile result is the key, not a
        // sampler that is nearest for everything.
        let def = material_sampler_descriptor(MaterialSamplerKey::of(
            &gdserver3d::material::Material3D::default(),
        ));
        assert_eq!((def.mag_filter, def.min_filter), (FilterMode::Linear, FilterMode::Linear));
        assert_eq!(def.address_mode_u, AddressMode::Repeat);
    }
}

/// 🔑 pat-wyei2: the player's coin counter (`CoinCount`, a Label3D with
/// `no_depth_test`, default `alpha_cut` and `outline_size` 12 at 48 pt)
/// reaches the renderer ALPHA-BLENDED, with a text texture that is transparent
/// around a THIN outline — Godot draws bare yellow outlined text.
///
/// Before the fix the label material was opaque (`Transparency::Disabled`), so
/// every uncovered texel drew as a dark box, and `outline_size` was read as the
/// ring's RADIUS where Godot's stroker uses a quarter of it, so the ring
/// closed over the glyph's counters into a solid slab.
#[test]
fn the_coin_counter_label_is_alpha_blended_text_with_a_thin_outline() {
    use gdserver3d::material::Transparency;
    let (prepared, paths) = play_frame(320, 180);
    let Play3DScene::Camera { inputs, .. } = &prepared.scene else {
        panic!("game.tscn has a current Camera3D");
    };
    let label = items_at(&inputs.items, &paths, "/root/Game/Player/Player/Skeleton/CoinCount");
    assert_eq!(label.len(), 1, "CoinCount in the render list");
    let mat = label[0].material.as_ref().expect("label material");
    assert_eq!(mat.transparency, Transparency::Alpha, "alpha_cut DISABLED = alpha blending");
    assert!(mat.no_depth_test, "player.tscn sets no_depth_test = true");

    let tex = mat.albedo_texture.as_ref().expect("label text texture");
    let clear = tex.pixels.iter().filter(|c| c.a == 0.0).count();
    let outline = tex
        .pixels
        .iter()
        .filter(|c| c.a > 0.0 && c.r + c.g + c.b < 0.05)
        .count();
    let glyph = tex.pixels.iter().filter(|c| c.a > 0.0 && c.r > 0.5).count();
    assert!(glyph > 0, "the digit draws in its modulate colour");
    // Godot: stroke radius = outline_size / 4 font px = 3 px at 48 pt, i.e.
    // 3 * 42/48 = 2.6 -> 3 texels of this 42-texel-tall glyph.
    assert_eq!(gdeditor::sprite3d_render::label3d_outline_texels(12, 48, 6), 3);
    // The built-in 5x7 "0" fills most of its cell, so only the corners and
    // the counters stay clear (276 of 1728 texels at the fixed radius).
    assert!(
        clear * 10 > tex.pixels.len(),
        "the label quad must stay transparent around the text: \
         {clear} clear of {} texels ({outline} outline, {glyph} glyph)",
        tex.pixels.len()
    );
    assert!(
        outline < glyph * 2,
        "the outline is a thin ring, not a slab: {outline} outline texels vs {glyph} glyph"
    );
}

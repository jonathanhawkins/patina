//! pat-p3p1m: the editor / Play color path uploads each mesh and each material
//! map ONCE, not on every frame.
//!
//! `render_editor_color_bgra` used to build a vertex buffer, an index buffer,
//! five textures and a sampler for every draw of every frame; with a textured
//! scene that was the largest CPU cost of a 3D Play frame after the post pass.
//! The editor rebuilds its scene every frame (`reset_scene` + re-add), so the
//! caches must survive that, keyed by content (meshes) and `Arc` identity
//! (maps).

#![cfg(feature = "gpu")]

use std::sync::Arc;

use gdcore::math::{Color, Vector3};
use gdcore::math3d::Transform3D;
use gdrender3d::wgpu_pipeline::GpuRenderer3D;
use gdserver3d::material::{Material3D, ShadingMode, TextureImage};
use gdserver3d::mesh::Mesh3D;
use gdserver3d::server::RenderingServer3D;
use gdserver3d::viewport::Viewport3D;

const W: u32 = 64;
const H: u32 = 64;

fn checker() -> Arc<TextureImage> {
    let red = Color::new(1.0, 0.0, 0.0, 1.0);
    let blue = Color::new(0.0, 0.0, 1.0, 1.0);
    Arc::new(TextureImage::new(2, 2, vec![red, blue, blue, red]))
}

/// What the editor does every frame: forget the scene and add it again.
fn rebuild(r: &mut GpuRenderer3D, texture: &Arc<TextureImage>) {
    r.reset_scene();
    for x in [-0.6f32, 0.6] {
        let inst = r.create_instance();
        r.set_mesh(inst, Mesh3D::cube(0.5));
        r.set_material(
            inst,
            Material3D {
                albedo: Color::WHITE,
                shading_mode: ShadingMode::Unlit,
                albedo_texture: Some(texture.clone()),
                ..Default::default()
            },
        );
        r.set_transform(inst, Transform3D::IDENTITY.translated(Vector3::new(x, 0.0, -2.5)));
        r.set_visible(inst, true);
    }
}

#[test]
fn a_static_scene_uploads_its_meshes_and_maps_once() {
    let Some(mut r) = GpuRenderer3D::new() else {
        eprintln!("SKIP: no wgpu adapter; this is a SKIP, not a pass");
        return;
    };
    let vp = Viewport3D::new(W, H);
    let clear = Color::new(0.0, 0.0, 0.0, 1.0);
    let texture = checker();

    rebuild(&mut r, &texture);
    let (_, _, first) = r.render_editor_color_bgra(&vp, clear);
    let after_first = r.editor_upload_counts();
    // Two cubes share one mesh and one map: one upload each.
    assert_eq!(after_first, (1, 1), "first frame uploads (meshes, maps)");

    for _ in 0..3 {
        rebuild(&mut r, &texture);
        let (_, _, again) = r.render_editor_color_bgra(&vp, clear);
        assert_eq!(again, first, "a cached frame must render the same bytes");
    }
    assert_eq!(
        r.editor_upload_counts(),
        after_first,
        "re-rendering an unchanged scene must not upload any mesh or map again"
    );

    // A NEW map (a different Arc) is uploaded; the mesh is still cached.
    let other = checker();
    rebuild(&mut r, &other);
    let (_, _, _) = r.render_editor_color_bgra(&vp, clear);
    assert_eq!(r.editor_upload_counts(), (1, 2));
}

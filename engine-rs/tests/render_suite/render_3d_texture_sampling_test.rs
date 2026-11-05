//! End-to-end framebuffer tests for `StandardMaterial3D` texture sampling in the
//! software render path (pat-u3l8c.32).
//!
//! Each test drives the real [`SoftwareRenderer3D`] over a camera-facing quad
//! and inspects the rendered pixels, exercising the four acceptance criteria:
//!   * a textured mesh renders its texture (albedo map sampled at UV),
//!   * perspective-correct UV (foreshortened texel spacing on a depth-tilted quad),
//!   * a normal map perturbs lighting,
//!   * roughness / metallic / emission maps modulate their channels.

use std::sync::Arc;

use gdcore::math::{Color, Vector3};
use gdrender3d::renderer::FrameBuffer3D;
use gdrender3d::test_adapter::capture_frame_3d;
use gdrender3d::SoftwareRenderer3D;
use gdserver3d::light::{Light3D, Light3DId};
use gdserver3d::material::{Material3D, ShadingMode, TextureImage};
use gdserver3d::mesh::{Mesh3D, PrimitiveType};
use gdserver3d::server::RenderingServer3D;
use gdserver3d::viewport::Viewport3D;

const W: u32 = 64;
const H: u32 = 64;

/// A unit quad in the XY plane at z = -2 facing +Z (toward the default camera at
/// the origin looking down -Z). It over-covers the whole viewport, so every
/// pixel lands on the quad. UV origin is top-left (v grows downward), matching
/// Godot: world +Y (screen top) → v = 0.
fn facing_quad() -> Mesh3D {
    let mut m = Mesh3D::new(PrimitiveType::Triangles);
    m.vertices = vec![
        Vector3::new(-1.0, -1.0, -2.0), // bottom-left
        Vector3::new(1.0, -1.0, -2.0),  // bottom-right
        Vector3::new(1.0, 1.0, -2.0),   // top-right
        Vector3::new(-1.0, 1.0, -2.0),  // top-left
    ];
    let n = Vector3::new(0.0, 0.0, 1.0);
    m.normals = vec![n; 4];
    m.uvs = vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    // Clockwise = front-facing in Godot (`doc/classes/ArrayMesh.xml:49`); the
    // renderer now culls counter-clockwise triangles, as Godot does.
    m.indices = vec![0, 2, 1, 0, 3, 2];
    m
}

/// A quad whose top edge recedes into the distance (z = -5) while its bottom
/// edge stays near (z = -2). Used to test perspective-correct UV: the near half
/// foreshortens to occupy more screen rows than the far half.
fn tilted_quad() -> Mesh3D {
    let mut m = Mesh3D::new(PrimitiveType::Triangles);
    m.vertices = vec![
        Vector3::new(-2.0, -0.8, -2.0), // near bottom-left  (v = 1)
        Vector3::new(2.0, -0.8, -2.0),  // near bottom-right (v = 1)
        Vector3::new(2.0, 2.0, -5.0),   // far  top-right    (v = 0)
        Vector3::new(-2.0, 2.0, -5.0),  // far  top-left     (v = 0)
    ];
    let n = Vector3::new(0.0, 0.0, 1.0);
    m.normals = vec![n; 4];
    m.uvs = vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    // Clockwise = front-facing in Godot (`doc/classes/ArrayMesh.xml:49`); the
    // renderer now culls counter-clockwise triangles, as Godot does.
    m.indices = vec![0, 2, 1, 0, 3, 2];
    m
}

/// Renders `mesh` with `mat`, optionally adding one directional light whose
/// *travel* direction is `light_travel` (the light arrives from the opposite
/// direction).
fn render(mesh: Mesh3D, mat: Material3D, light_travel: Option<Vector3>) -> FrameBuffer3D {
    let mut r = SoftwareRenderer3D::new();
    let id = r.create_instance();
    r.set_mesh(id, mesh);
    r.set_material(id, mat);
    if let Some(dir) = light_travel {
        let lid = Light3DId(1);
        r.add_light(lid);
        let mut l = Light3D::directional(lid);
        l.direction = dir;
        r.update_light(&l);
    }
    let vp = Viewport3D::new(W, H);
    capture_frame_3d(&mut r, &vp)
}

/// Mean per-channel brightness over the central quarter of the frame (always on
/// the quad, never background).
fn center_luma(fb: &FrameBuffer3D) -> f32 {
    let mut sum = 0.0;
    let mut n = 0u32;
    for y in (H / 4)..(3 * H / 4) {
        for x in (W / 4)..(3 * W / 4) {
            let c = fb.get_pixel(x, y);
            sum += c.r + c.g + c.b;
            n += 1;
        }
    }
    sum / (n as f32 * 3.0)
}

/// Sum of absolute per-channel differences over the central quarter of the frame.
fn center_diff(a: &FrameBuffer3D, b: &FrameBuffer3D) -> f32 {
    let mut sum = 0.0;
    for y in (H / 4)..(3 * H / 4) {
        for x in (W / 4)..(3 * W / 4) {
            let p = a.get_pixel(x, y);
            let q = b.get_pixel(x, y);
            sum += (p.r - q.r).abs() + (p.g - q.g).abs() + (p.b - q.b).abs();
        }
    }
    sum
}

fn approx(c: Color, r: f32, g: f32, b: f32, tol: f32) -> bool {
    (c.r - r).abs() <= tol && (c.g - g).abs() <= tol && (c.b - b).abs() <= tol
}

fn arc_tex(w: u32, h: u32, pixels: Vec<Color>) -> Arc<TextureImage> {
    Arc::new(TextureImage::new(w, h, pixels))
}

fn red() -> Color {
    Color::new(1.0, 0.0, 0.0, 1.0)
}
fn blue() -> Color {
    Color::new(0.0, 0.0, 1.0, 1.0)
}

// ── Albedo ──────────────────────────────────────────────────────────────

#[test]
fn albedo_solid_texture_multiplies_flat_color() {
    // White base albedo × solid-green texture → the mesh renders green, and
    // differs from the untextured (white) render.
    let mut mat = Material3D {
        shading_mode: ShadingMode::Unlit,
        albedo: Color::WHITE,
        ..Default::default()
    };
    mat.albedo_texture = Some(Arc::new(TextureImage::solid(Color::new(0.0, 1.0, 0.0, 1.0))));
    let fb = render(facing_quad(), mat, None);

    let c = fb.get_pixel(W / 2, H / 2);
    assert!(
        approx(c, 0.0, 1.0, 0.0, 0.08),
        "textured quad centre should be green, got {c:?}"
    );

    let flat = render(
        facing_quad(),
        Material3D {
            shading_mode: ShadingMode::Unlit,
            albedo: Color::WHITE,
            ..Default::default()
        },
        None,
    );
    assert!(
        center_diff(&fb, &flat) > 10.0,
        "albedo texture must change the render vs a flat white material"
    );
}

#[test]
fn albedo_texture_varies_across_uv() {
    // 8-wide texture: left half red, right half blue. The left screen region
    // samples the red half and the right region the blue half (or vice-versa,
    // depending on projection orientation) — either way both colours appear.
    let mut pixels = vec![red(); 4];
    pixels.extend(vec![blue(); 4]);
    let mut mat = Material3D {
        shading_mode: ShadingMode::Unlit,
        albedo: Color::WHITE,
        ..Default::default()
    };
    mat.albedo_texture = Some(arc_tex(8, 1, pixels));
    let fb = render(facing_quad(), mat, None);

    let left = fb.get_pixel(W / 4, H / 2);
    let right = fb.get_pixel(3 * W / 4, H / 2);
    let left_red = approx(left, 1.0, 0.0, 0.0, 0.15);
    let left_blue = approx(left, 0.0, 0.0, 1.0, 0.15);
    let right_red = approx(right, 1.0, 0.0, 0.0, 0.15);
    let right_blue = approx(right, 0.0, 0.0, 1.0, 0.15);
    assert!(
        (left_red && right_blue) || (left_blue && right_red),
        "the two screen halves must sample the two texture halves; \
         left={left:?} right={right:?}"
    );
}

// ── Perspective-correct UV ──────────────────────────────────────────────

#[test]
fn perspective_correct_uv_foreshortens_near_half() {
    // Vertical split texture: top half (v<0.5) red, bottom half (v>0.5) blue.
    // On the depth-tilted quad the bottom edge is near and the top edge is far,
    // so the near (blue) half foreshortens to MORE screen rows than the far
    // (red) half. With affine (perspective-incorrect) UV the two halves would
    // occupy equal screen rows.
    let mut pixels = vec![red(); 4];
    pixels.extend(vec![blue(); 4]);
    let mut mat = Material3D {
        shading_mode: ShadingMode::Unlit,
        albedo: Color::WHITE,
        ..Default::default()
    };
    mat.albedo_texture = Some(arc_tex(1, 8, pixels)); // 1 wide × 8 tall
    let fb = render(tilted_quad(), mat, None);

    let x = W / 2;
    let mut red_rows = 0u32;
    let mut blue_rows = 0u32;
    for y in 0..H {
        let c = fb.get_pixel(x, y);
        if approx(c, 1.0, 0.0, 0.0, 0.25) {
            red_rows += 1;
        } else if approx(c, 0.0, 0.0, 1.0, 0.25) {
            blue_rows += 1;
        }
    }
    assert!(
        red_rows > 3 && blue_rows > 3,
        "both texture halves must be visible (red={red_rows}, blue={blue_rows})"
    );
    assert!(
        blue_rows > red_rows + 6,
        "near (blue) half must foreshorten to more rows than the far (red) half: \
         blue={blue_rows} red={red_rows}"
    );
}

// ── Normal map ──────────────────────────────────────────────────────────

#[test]
fn normal_map_perturbs_lighting() {
    // Light arrives from +X (travel −X). A flat quad's geometric normal is +Z,
    // so n·l = 0 (ambient only). A normal map tilting the normal toward +X
    // raises n·l and brightens the surface. A flat normal map must match the
    // untextured result.
    let base = || Material3D {
        shading_mode: ShadingMode::Lambert,
        albedo: Color::WHITE,
        ..Default::default()
    };
    let light = Some(Vector3::new(-1.0, 0.0, 0.0));

    let flat_map = {
        let mut m = base();
        m.normal_texture = Some(arc_tex(1, 1, vec![Color::new(0.5, 0.5, 1.0, 1.0)]));
        m.normal_scale = 1.0;
        render(facing_quad(), m, light)
    };
    let tilted_map = {
        let mut m = base();
        m.normal_texture = Some(arc_tex(1, 1, vec![Color::new(1.0, 0.5, 1.0, 1.0)]));
        m.normal_scale = 1.0;
        render(facing_quad(), m, light)
    };
    let no_map = render(facing_quad(), base(), light);

    assert!(
        center_luma(&tilted_map) > center_luma(&flat_map) + 0.15,
        "tilted normal map must brighten the lit surface: tilted={:.3} flat={:.3}",
        center_luma(&tilted_map),
        center_luma(&flat_map)
    );
    assert!(
        center_diff(&flat_map, &no_map) < 1.0,
        "a flat (0.5,0.5,1.0) normal map must equal no normal map"
    );
}

// ── Roughness / metallic / emission maps ────────────────────────────────

#[test]
fn roughness_map_modulates_specular() {
    // Front-right light so the quad receives a specular highlight. Base
    // roughness is fully rough (weak highlight); a roughness map driving
    // roughness toward 0 sharpens/brightens the highlight.
    let light = Some(Vector3::new(-0.3, 0.0, -1.0));
    let base = || Material3D {
        shading_mode: ShadingMode::Phong,
        // ⚠️ Not white: since pat-9gjpr applied Godot's xPI light energy, a white
        // surface lit head-on clips to 1.0 with or without the highlight, and
        // the comparison below could never see the roughness map.
        albedo: Color::new(0.25, 0.25, 0.25, 1.0),
        roughness: 1.0,
        metallic: 0.0,
        ..Default::default()
    };
    let no_map = render(facing_quad(), base(), light);
    let with_map = {
        let mut m = base();
        // Green channel (0.05) drives roughness → nearly mirror-smooth.
        m.roughness_texture = Some(arc_tex(1, 1, vec![Color::new(0.9, 0.05, 0.9, 1.0)]));
        render(facing_quad(), m, light)
    };
    assert!(
        center_diff(&with_map, &no_map) > 1.0,
        "roughness map must change the specular response"
    );
    // ⚠️ Lower roughness CONCENTRATES the GGX lobe: the brightest pixel gets
    // brighter while off-peak pixels dim, so the AVERAGE over the centre can
    // fall (it did: 0.669 vs 0.671). Compare the peak, which is what "sharper
    // highlight" means.
    assert!(
        peak_luma(&with_map) > peak_luma(&no_map),
        "lower roughness must sharpen the highlight peak: with={:.3} without={:.3}",
        peak_luma(&with_map),
        peak_luma(&no_map)
    );
}

fn peak_luma(fb: &FrameBuffer3D) -> f32 {
    let mut best = 0.0f32;
    for y in 0..H {
        for x in 0..W {
            let c = fb.get_pixel(x, y);
            best = best.max(c.r + c.g + c.b);
        }
    }
    best
}

#[test]
fn metallic_map_modulates_response() {
    let light = Some(Vector3::new(-0.3, 0.0, -1.0));
    // Godot: `METALLIC = metallic_tex * metallic` — the map MULTIPLIES the
    // scalar, so on a metallic = 0 base no map can change anything. Start
    // fully metallic and let the map (B = 0 in the renderer's internal
    // packing) drive it to a dielectric.
    let base = || Material3D {
        shading_mode: ShadingMode::Phong,
        albedo: Color::new(0.8, 0.3, 0.2, 1.0),
        roughness: 0.4,
        metallic: 1.0,
        ..Default::default()
    };
    let no_map = render(facing_quad(), base(), light);
    let with_map = {
        let mut m = base();
        m.metallic_texture = Some(arc_tex(1, 1, vec![Color::new(0.9, 0.9, 0.0, 1.0)]));
        render(facing_quad(), m, light)
    };
    assert!(
        center_diff(&with_map, &no_map) > 1.0,
        "metallic map must change the shading response"
    );
}

#[test]
fn emission_map_modulates_emission() {
    // Black albedo, white emission base, unlit: output = emission × texel.
    // 8-wide map: left half full emission, right half none.
    let mut pixels = vec![Color::WHITE; 4];
    pixels.extend(vec![Color::BLACK; 4]);
    let mut mat = Material3D {
        shading_mode: ShadingMode::Unlit,
        albedo: Color::BLACK,
        emission: Color::WHITE,
        ..Default::default()
    };
    mat.emission_texture = Some(arc_tex(8, 1, pixels));
    let fb = render(facing_quad(), mat, None);

    let left = fb.get_pixel(W / 4, H / 2);
    let right = fb.get_pixel(3 * W / 4, H / 2);
    // One screen half glows (≈white) and the other is dark (≈black).
    let left_bright = left.r + left.g + left.b > 2.0;
    let right_bright = right.r + right.g + right.b > 2.0;
    assert!(
        left_bright != right_bright,
        "emission map must light one half and not the other: left={left:?} right={right:?}"
    );

    let uniform = render(
        facing_quad(),
        Material3D {
            shading_mode: ShadingMode::Unlit,
            albedo: Color::BLACK,
            emission: Color::WHITE,
            ..Default::default()
        },
        None,
    );
    assert!(
        center_diff(&fb, &uniform) > 10.0,
        "emission map must differ from uniform emission"
    );
}

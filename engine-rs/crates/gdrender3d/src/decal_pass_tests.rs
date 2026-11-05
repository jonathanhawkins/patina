//! pat-414k3: `apply_decal_pass` skips pixels no decal can reach (a projected
//! screen rectangle, a world AABB and the box test with a once-per-decal
//! inverse) before it reconstructs or samples anything. These tests hold it to
//! the old per-pixel walk — every pixel, every decal, `Decal3D::sample` — bit
//! for bit, including the cases the rejections are most likely to get wrong:
//! a box around the camera, a box crossing the camera plane, a box behind the
//! camera, and rotated, scaled and overlapping decals.

use super::*;
use gdcore::math3d::Basis;

/// The pass as it was before pat-414k3: no rejection at all.
fn reference_decal_pass(
    pixels: &mut [Color],
    depth: &[f32],
    w: u32,
    h: u32,
    decals: &[Decal3D],
    inv_proj: &[[f32; 4]; 4],
    camera_transform: &Transform3D,
) {
    let camera_pos = camera_transform.origin;
    for y in 0..h {
        let ndc_y = 1.0 - 2.0 * (y as f32 + 0.5) / h as f32;
        for x in 0..w {
            let ndc_x = 2.0 * (x as f32 + 0.5) / w as f32 - 1.0;
            let i = (y * w + x) as usize;
            let d = depth[i];
            if d >= 1.0 {
                continue;
            }
            let view_pos = pixel_view_position(ndc_x, ndc_y, d, inv_proj);
            let world_pos = camera_transform.xform(view_pos);
            for decal in decals {
                let dist = (decal.global_transform.origin - camera_pos).length() as f64;
                let sample = match decal.sample(world_pos, dist) {
                    Some(s) if s.fade > 0.0 => s,
                    _ => continue,
                };
                let alpha = sample.albedo_alpha as f32;
                if alpha > 0.0 {
                    pixels[i] = Color::new(
                        lerp(pixels[i].r, sample.albedo.r, alpha),
                        lerp(pixels[i].g, sample.albedo.g, alpha),
                        lerp(pixels[i].b, sample.albedo.b, alpha),
                        pixels[i].a,
                    );
                }
                if let Some(e) = sample.emission {
                    let k = (sample.fade * decal.emission_energy) as f32;
                    if k > 0.0 {
                        pixels[i] = Color::new(
                            (pixels[i].r + e.r * k).min(1.0),
                            (pixels[i].g + e.g * k).min(1.0),
                            (pixels[i].b + e.b * k).min(1.0),
                            pixels[i].a,
                        );
                    }
                }
            }
        }
    }
}

const W: u32 = 160;
const H: u32 = 120;

/// The depth buffer of a camera looking at the ground plane `y = 0`, with a
/// pillar of nearer depth in the middle and sky above the horizon.
fn ground_depth(camera: &Transform3D, proj: &[[f32; 4]; 4], inv_proj: &[[f32; 4]; 4]) -> Vec<f32> {
    let view_of_world = camera.inverse();
    let mut depth = vec![1.0f32; (W * H) as usize];
    for y in 0..H {
        let ndc_y = 1.0 - 2.0 * (y as f32 + 0.5) / H as f32;
        for x in 0..W {
            let ndc_x = 2.0 * (x as f32 + 0.5) / W as f32 - 1.0;
            let far = pixel_view_position(ndc_x, ndc_y, 1.0, inv_proj);
            let dir = camera.basis.xform(far).normalized();
            if dir.y >= -1e-4 {
                continue;
            }
            let t = -camera.origin.y / dir.y;
            let hit = camera.origin + dir * t;
            let v = view_of_world.xform(hit);
            let clip = |r: usize| proj[0][r] * v.x + proj[1][r] * v.y + proj[2][r] * v.z + proj[3][r];
            let mut d = clip(2) / clip(3);
            // A nearer slab down the left quarter of the screen.
            if x < W / 4 && y > H / 3 {
                d *= 0.97;
            }
            depth[(y * W + x) as usize] = d.clamp(0.0, 0.999_999);
        }
    }
    depth
}

fn frame(fill: Color) -> Vec<Color> {
    (0..W * H)
        .map(|i| {
            let k = (i % 97) as f32 / 97.0;
            Color::new(fill.r * k, fill.g, fill.b * (1.0 - k), 1.0)
        })
        .collect()
}

fn decal_at(origin: Vector3, size: Vector3, basis: Basis, tint: Color) -> Decal3D {
    let mut d = Decal3D::new();
    d.size = size;
    d.global_transform = Transform3D { basis, origin };
    d.modulate = tint;
    d.albedo_mix = 0.8;
    d
}

fn check(camera: Transform3D, decals: &[Decal3D]) -> usize {
    let proj = perspective_projection_matrix(std::f32::consts::FRAC_PI_3, W as f32 / H as f32, 0.05, 400.0);
    let inv_proj = invert_projection(&proj);
    let depth = ground_depth(&camera, &proj, &inv_proj);
    let mut fast = frame(Color::new(0.6, 0.5, 0.4, 1.0));
    let mut slow = fast.clone();
    SoftwareRenderer3D::apply_decal_pass(&mut fast, &depth, W, H, decals, &inv_proj, &camera);
    reference_decal_pass(&mut slow, &depth, W, H, decals, &inv_proj, &camera);
    let differing = fast
        .iter()
        .zip(&slow)
        .filter(|(a, b)| {
            a.r.to_bits() != b.r.to_bits()
                || a.g.to_bits() != b.g.to_bits()
                || a.b.to_bits() != b.b.to_bits()
                || a.a.to_bits() != b.a.to_bits()
        })
        .count();
    assert_eq!(differing, 0, "{differing} pixels differ from the per-pixel walk");
    // How many pixels the decals touched — a test whose decals hit nothing
    // proves nothing.
    let base = frame(Color::new(0.6, 0.5, 0.4, 1.0));
    fast.iter().zip(&base).filter(|(a, b)| a != b).count()
}

/// A camera at `origin` whose -Z (Godot's forward) points at `target`.
fn camera_at(origin: Vector3, target: Vector3) -> Transform3D {
    Transform3D { basis: Basis::looking_at(target - origin, Vector3::UP), origin }
}

#[test]
fn a_blob_shadow_column_in_front_of_the_camera_matches_the_per_pixel_walk() {
    let cam = camera_at(Vector3::new(0.0, 4.0, 8.0), Vector3::ZERO);
    let decals = [decal_at(
        Vector3::new(0.3, 0.0, 0.2),
        Vector3::new(1.6, 12.0, 1.6),
        Basis::IDENTITY,
        Color::new(0.0, 0.0, 0.0, 1.0),
    )];
    assert!(check(cam, &decals) > 50);
}

#[test]
fn a_column_around_and_across_the_camera_plane_matches_the_per_pixel_walk() {
    // The platformer's follow camera sits inside the player's 12 m blob-shadow
    // column more often than not.
    let cam = camera_at(Vector3::new(0.2, 3.0, 0.5), Vector3::new(0.0, 0.0, -3.0));
    let inside = [decal_at(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.6, 12.0, 1.6),
        Basis::IDENTITY,
        Color::new(0.0, 0.0, 0.0, 1.0),
    )];
    assert!(check(cam, &inside) > 50);
    // Wide and shallow, crossing the camera plane without containing the eye.
    let across = [decal_at(
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(8.0, 1.0, 6.0),
        Basis::IDENTITY,
        Color::new(1.0, 0.0, 0.0, 1.0),
    )];
    assert!(check(cam, &across) > 50);
}

#[test]
fn decals_behind_or_beside_the_view_draw_nothing_and_still_match() {
    let cam = camera_at(Vector3::new(0.0, 4.0, 8.0), Vector3::ZERO);
    let decals = [
        decal_at(Vector3::new(0.0, 0.0, 20.0), Vector3::new(2.0, 2.0, 2.0), Basis::IDENTITY, Color::WHITE),
        decal_at(Vector3::new(-90.0, 0.0, 0.0), Vector3::new(2.0, 2.0, 2.0), Basis::IDENTITY, Color::WHITE),
    ];
    assert_eq!(check(cam, &decals), 0);
}

#[test]
fn rotated_scaled_and_overlapping_decals_match_the_per_pixel_walk() {
    let cam = camera_at(Vector3::new(3.0, 5.0, 6.0), Vector3::new(0.0, 0.0, -1.0));
    let turn = Basis::IDENTITY.rotated(Vector3::UP, 0.7);
    let tilt = Basis::IDENTITY.rotated(Vector3::new(1.0, 0.0, 0.3).normalized(), 0.4);
    let scaled = Basis {
        x: turn.x * 1.5,
        y: turn.y * 0.5,
        z: turn.z * 2.0,
    };
    let mut glow = decal_at(Vector3::new(0.5, 0.0, 0.0), Vector3::new(2.0, 3.0, 2.0), tilt, Color::new(0.2, 0.9, 0.3, 1.0));
    glow.emission_image = Some(std::sync::Arc::new(gdserver3d::material::TextureImage::new(
        1,
        1,
        vec![Color::new(0.5, 0.4, 0.1, 1.0)],
    )));
    let decals = [
        decal_at(Vector3::new(0.0, 0.0, -1.0), Vector3::new(3.0, 2.0, 3.0), scaled, Color::new(0.1, 0.1, 0.8, 1.0)),
        glow,
        decal_at(Vector3::new(-1.0, 0.0, -2.0), Vector3::new(4.0, 4.0, 1.0), turn, Color::new(1.0, 1.0, 0.0, 0.5)),
    ];
    assert!(check(cam, &decals) > 200);
}

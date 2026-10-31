//! pat-mgixk.52, normal-map half: the **wgpu** pipeline perturbs the shading
//! normal with `BaseMaterial3D.normal_texture`.
//!
//! `fs_main` shaded from the interpolated geometric normal only
//! (`let normal = normalize(in.world_normal);`), so a normal-mapped material
//! rendered identically to a flat one on the GPU while the software path
//! perturbed it (`shader.rs:252` → `apply_normal_map`).
//!
//! # The frame is derived from the NORMAL, not from screen derivatives
//!
//! ⚠️ The bead asks for "screen-derivative TBN like the software path", but the
//! software path uses no screen derivatives at all: `tangent_frame`
//! (`shader.rs:273`) is Duff et al.'s branchless orthonormal basis built from
//! the normal alone, never touching `dpdx`/`dpdy` or the UVs. The WGSL mirrors
//! the code, not the bead text — the acceptance compares the two backends, so a
//! different (even defensible) frame would be a visible mismatch.
//!
//! # Why `normal_scale = 0.0` is the load-bearing control
//!
//! "The two frames differ" alone would also be satisfied by a wrong tangent
//! frame, a mis-decoded texel, or a stray binding. Zeroing the scale makes
//! `apply_normal_map` reduce **analytically** to the geometric normal: `tx` and
//! `ty` become 0, so `perturbed = n * tz` and `normalize` gives back `n`
//! exactly. So the scaled render must differ from flat AND the zero-scale
//! render must match flat — a pair only the correct math satisfies.
//!
//! ⚠️ Lambert, not unlit: the unlit branch returns before `normal` is ever
//! read, so an unlit fixture could not observe a normal map at all.

#![cfg(feature = "gpu")]

use gdcore::math::{Color, Vector3};
use gdcore::math3d::Transform3D;
use gdrender3d::wgpu_pipeline::GpuRenderer3D;
use gdserver3d::light::{Light3D, Light3DId};
use gdserver3d::material::{Material3D, TextureImage};
use gdserver3d::mesh::Mesh3D;
use gdserver3d::server::{FrameData3D, RenderingServer3D};
use gdserver3d::viewport::Viewport3D;

const W: u32 = 96;
const H: u32 = 96;

/// `None` means no adapter, which is a skip and not a pass.
fn gpu_renderer() -> Option<GpuRenderer3D> {
    match GpuRenderer3D::new() {
        Some(g) => Some(g),
        None => {
            eprintln!(
                "SKIP: no wgpu adapter available; the normal-map half of \
                 pat-mgixk.52 cannot be exercised on this machine. This is a \
                 SKIP, not a pass."
            );
            None
        }
    }
}

/// A uniform tangent-space tilt of roughly 45 degrees toward +tangent.
///
/// Channel values are chosen to be **exact in 8 bits** where it matters:
/// `r = 1.0` and `b = 1.0` store as 255 and decode back to exactly 1.0, giving
/// `tx = 1` and `tz = 1` so the perturbed normal is `(tangent + n)` normalized.
/// `g = 0.5` is deliberately the only inexact channel (128/255 decodes to
/// 0.50196, i.e. `ty = 0.00392`) because a tiny Y term changes nothing the
/// assertions depend on — whereas picking 0.5 for `b` would have driven `tz` to
/// the `max(.., 1e-4)` floor and made the tilt degenerate.
fn tilt_map() -> TextureImage {
    TextureImage::new(1, 1, vec![Color::new(1.0, 0.5, 1.0, 1.0)])
}

/// A lit cube whose material optionally carries a normal map at `scale`.
fn build_scene<R: RenderingServer3D>(r: &mut R, normal: Option<TextureImage>, scale: f32) {
    let mut light = Light3D::directional(Light3DId(1));
    // Head-on, so the camera-facing quad starts at full `n·l` = 1.0 and a tilt
    // can only reduce it. A grazing light would make the sign of the change
    // depend on the tilt direction.
    light.direction = Vector3::new(0.0, 0.0, -1.0);
    light.energy = 1.0;
    r.add_light(light.id);
    r.update_light(&light);

    let inst = r.create_instance();
    r.set_mesh(inst, Mesh3D::cube(0.8));
    r.set_material(
        inst,
        Material3D {
            albedo: Color::new(1.0, 1.0, 1.0, 1.0),
            // Lambert (the default) reads `normal`; unlit returns before it.
            normal_texture: normal.map(std::sync::Arc::new),
            normal_scale: scale,
            ..Default::default()
        },
    );
    r.set_transform(
        inst,
        Transform3D::IDENTITY.translated(Vector3::new(0.0, 0.0, -2.5)),
    );
    r.set_visible(inst, true);
}

fn render(normal: Option<TextureImage>, scale: f32) -> Option<FrameData3D> {
    let mut gpu = gpu_renderer()?;
    build_scene(&mut gpu, normal, scale);
    Some(gpu.render_frame(&Viewport3D::new(W, H)))
}

fn lit_count(f: &FrameData3D) -> usize {
    f.pixels.iter().filter(|c| c.r + c.g + c.b > 0.1).count()
}

/// Pixels differing by more than `tol` in any channel.
fn differing(a: &FrameData3D, b: &FrameData3D, tol: f32) -> usize {
    a.pixels
        .iter()
        .zip(b.pixels.iter())
        .filter(|(x, y)| {
            (x.r - y.r).abs() > tol || (x.g - y.g).abs() > tol || (x.b - y.b).abs() > tol
        })
        .count()
}

#[test]
fn gpu_normal_map_changes_the_shading() {
    let Some(flat) = render(None, 1.0) else {
        return;
    };
    let mapped = render(Some(tilt_map()), 1.0).expect("adapter was available a moment ago");

    let lit = lit_count(&flat);
    assert!(
        lit > 200,
        "fixture precondition: the cube must rasterize and be LIT before a \
         normal map can change its shading; only {lit} lit pixels"
    );

    // The defect: before this change the GPU shaded from the interpolated
    // normal only, so these two frames were bit-identical.
    let changed = differing(&flat, &mapped, 1e-3);
    assert!(
        changed > 100,
        "a normal map must change the shading: only {changed} of {} pixels \
         differ from the flat render. Zero means the wgpu pipeline still \
         ignores normal_texture.",
        flat.pixels.len()
    );

    // Direction: the light is head-on, so the flat quad is at full n·l and any
    // tilt can only reduce it. A normal map that BRIGHTENED a head-on surface
    // would mean the frame or the decode is inverted.
    let mut brighter = 0usize;
    for (f, m) in flat.pixels.iter().zip(mapped.pixels.iter()) {
        if f.r + f.g + f.b > 0.1 && (m.r + m.g + m.b) > (f.r + f.g + f.b) + 1e-3 {
            brighter += 1;
        }
    }
    assert_eq!(
        brighter, 0,
        "tilting the normal away from a head-on light must not brighten the \
         surface; {brighter} pixels got brighter, which means the tangent frame \
         or the texel decode is inverted"
    );
}

/// The control that pins the TBN math rather than merely its presence.
///
/// With `normal_scale = 0.0`, `apply_normal_map` reduces analytically to the
/// geometric normal, so the render must come back the same as the flat one. A
/// wrong tangent frame, a mis-decoded texel or a stray binding would all still
/// satisfy "the frames differ" above, and all three fail here.
#[test]
fn a_zero_scale_normal_map_reduces_to_the_geometric_normal() {
    let Some(flat) = render(None, 1.0) else {
        return;
    };
    let zeroed = render(Some(tilt_map()), 0.0).expect("adapter was available a moment ago");

    let lit = lit_count(&flat);
    assert!(lit > 200, "fixture precondition: {lit} lit pixels");

    let changed = differing(&flat, &zeroed, 1e-3);
    assert_eq!(
        changed, 0,
        "normal_scale = 0.0 zeroes the tangent and bitangent terms, so the \
         perturbed normal is the geometric normal and the frame must be \
         unchanged; {changed} pixels differ"
    );
}

/// A material with no normal map must render exactly as before this change.
///
/// This is why the shader uses an explicit `has_normal_map` flag instead of a
/// "flat" (0.5, 0.5, 1.0) dummy texel: 0.5 is not representable in 8 bits
/// (128/255 = 0.50196 → a 0.00392 tilt), so a dummy would perturb every normal
/// slightly and this assertion could never hold.
#[test]
fn an_unmapped_material_is_bit_identical() {
    let Some(a) = render(None, 1.0) else {
        return;
    };
    let b = render(None, 1.0).expect("adapter was available a moment ago");
    assert!(lit_count(&a) > 200, "fixture precondition");
    assert_eq!(
        differing(&a, &b, 0.0),
        0,
        "two renders of an unmapped material must be bit-identical"
    );
}

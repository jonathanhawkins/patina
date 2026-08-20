//! pat-s3rrv.204 — a `Decal`'s albedo/emission textures are **sampled** during
//! projection, so the art on the decal reaches the frame.
//!
//! Godot ground truth: `upstream/godot/scene/3d/decal.cpp` and
//! `doc/classes/Decal.xml`.
//!
//! # The defect this pins
//!
//! ⚠️ The bead describes the textures as "decoded, transported and discarded".
//! That is not quite what was happening, and the difference matters for anyone
//! reading this later: they were never **decoded at all**. `pat-0q2kx.195` read
//! the four `texture_*` names off the node (see `probe_decal.rs`, which pins
//! exactly that much and says in its own header that sampling them was *not*
//! delivered), and they travelled to the renderer as `Option<String>` **paths**.
//! Nothing ever resolved a path to texels.
//!
//! The projection side had the matching hole. `Decal3D::project_point` returns
//! `(u, v, w)`, and the only consumer was `sample_alpha`, which opened with
//!
//! ```ignore
//! let (_, _, w) = self.project_point(world_point)?;
//! ```
//!
//! — discarding the exact UV a texture lookup needs and keeping only the
//! vertical coordinate. So `apply_decal_pass` blended a flat `modulate` colour
//! over the footprint, and a decal with a two-colour checker on it rendered as
//! one flat colour. Every test below fails on that build.
//!
//! # Why the assertions are shaped this way
//!
//! ⚠️ `apply_decal_pass` is **not** the last thing to touch these pixels — the
//! fog pass, local fog volumes, SSAO and `apply_camera_post` all run after it
//! (`renderer.rs:526-556`), and the first three are depth-dependent. So an
//! exact-colour assertion ("the footprint is byte-identical everywhere") would
//! be measuring the post chain, not the decal. Everything here is asserted with
//! **hue dominance** (`r > g` vs `g > r`) instead: tonemapping and fog can
//! lighten, darken or wash a pixel, but none of them turns a red pixel green.

use gdcore::math::Color;
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::{editor_3d_render_inputs, render_editor_3d_frame};
use gdrender2d::renderer::FrameBuffer;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};

const W: u32 = 160;
const H: u32 = 160;

const RED: Color = Color::new(1.0, 0.0, 0.0, 1.0);
const GREEN: Color = Color::new(0.0, 1.0, 0.0, 1.0);
const BLACK: Color = Color::new(0.0, 0.0, 0.0, 1.0);
const WHITE: Color = Color::new(1.0, 1.0, 1.0, 1.0);

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Writes a real PNG of `w`x`h` from `texels` and returns its absolute path.
///
/// A genuine encoded file, not a stub: `MaterialTextureCache::load` runs the
/// real `gdresource::importers::load_color_texture_linear` decoder, and a bare
/// filesystem path is passed through without needing a project root
/// (`render_server_3d.rs:2760`).
///
/// ⚠️ Every file gets a unique name. The decoder cache behind
/// `decal_texture_image` is a **thread-local keyed by path that memoizes
/// failures as well as successes**, so reusing one name across two fixtures
/// with different contents would silently serve the first decode to both.
fn texture_file(tag: &str, w: u32, h: u32, texels: &[Color]) -> String {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);

    assert_eq!(texels.len(), (w * h) as usize, "fixture texel count");

    let dir = std::env::temp_dir().join(format!("patina_s3rrv204_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create fixture dir");

    let mut fb = FrameBuffer::new(w, h, BLACK);
    for (i, c) in texels.iter().enumerate() {
        fb.set_pixel(i as u32 % w, i as u32 / w, *c);
    }

    let path = dir.join(format!("{tag}_{n}.png"));
    std::fs::write(&path, gdrender2d::export::encode_png(&fb)).expect("write fixture png");
    path.to_string_lossy().into_owned()
}

/// A floor plane in front of the camera, with `__DECAL__` substituted for the
/// decal node under test (or nothing, for the no-decal baseline).
///
/// The camera is copied verbatim from `probe_decal.rs`, which already proves
/// this floor renders shaded pixels from this viewpoint.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 6)
current = true

[node name="Floor" type="MeshInstance3D" parent="."]
mesh = "primitive:PlaneMesh"

__DECAL__
"#;

/// The decal node header.
///
/// ⚠️ The size is deliberately **smaller than the floor**. `PlaneMesh` defaults
/// to 2x2, so the floor spans x/z in `[-1, 1]`; half-extents of 0.8 leave an
/// undecaled border, which is what gives the acceptance its "and one outside"
/// clause something on the *floor* to point at rather than only backdrop.
///
/// The y extent stays large so the floor sits at the box's mid-plane: `w = 0.5`
/// there, where `vertical_fade` is 1.0 for the default 0.3 upper/lower fades.
/// A floor at the box edge would fade to zero and every test here would go
/// green-by-vacuum with no decal contribution at all.
const DECAL_HEAD: &str = r#"[node name="Splat" type="Decal" parent="."]
size = Vector3(1.6, 3, 1.6)
"#;

fn scene(decal: &str) -> SceneTree {
    let tscn = SCENE.replace("__DECAL__", decal);
    let packed = PackedScene::from_tscn(&tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed).expect("instance scene");
    tree
}

fn viewport() -> Viewport3D {
    let mut vp = Viewport3D::default();
    vp.width = W;
    vp.height = H;
    vp
}

fn render(tree: &SceneTree) -> FrameBuffer {
    render_editor_3d_frame(&viewport(), tree)
}

// ---------------------------------------------------------------------------
// Frame helpers
// ---------------------------------------------------------------------------

fn differs(a: Color, b: Color) -> bool {
    (a.r - b.r).abs() > 1e-4 || (a.g - b.g).abs() > 1e-4 || (a.b - b.b).abs() > 1e-4
}

/// Indices where `frame` differs from the same scene rendered with no decal —
/// i.e. the decal's footprint, discovered rather than assumed from screen
/// coordinates.
fn footprint(frame: &FrameBuffer, baseline: &FrameBuffer) -> Vec<usize> {
    assert_eq!(
        frame.pixels.len(),
        baseline.pixels.len(),
        "both frames are the same size"
    );
    (0..frame.pixels.len())
        .filter(|&i| differs(frame.pixels[i], baseline.pixels[i]))
        .collect()
}

/// Hue dominance, with a margin comfortably above the shading noise floor.
fn red_dominant(c: Color) -> bool {
    c.r > c.g + 0.05
}

fn green_dominant(c: Color) -> bool {
    c.g > c.r + 0.05
}

fn distinct_colors(frame: &FrameBuffer, idx: &[usize]) -> usize {
    idx.iter()
        .map(|&i| {
            let c = frame.pixels[i];
            (
                (c.r.clamp(0.0, 1.0) * 255.0) as u8,
                (c.g.clamp(0.0, 1.0) * 255.0) as u8,
                (c.b.clamp(0.0, 1.0) * 255.0) as u8,
            )
        })
        .collect::<HashSet<_>>()
        .len()
}

/// Mean screen x and y of a set of pixel indices.
fn centroid(idx: &[usize]) -> (f64, f64) {
    assert!(!idx.is_empty(), "centroid of an empty set");
    let (sx, sy) = idx.iter().fold((0.0, 0.0), |(sx, sy), &i| {
        (sx + (i as u32 % W) as f64, sy + (i as u32 / W) as f64)
    });
    (sx / idx.len() as f64, sy / idx.len() as f64)
}

// ---------------------------------------------------------------------------
// The acceptance
// ---------------------------------------------------------------------------

/// 🔑 **THE ACCEPTANCE.** A two-colour checker `texture_albedo` projected onto
/// the floor renders as two colours inside the footprint, where before this
/// change the footprint was a single flat `modulate`.
#[test]
fn decal_texture_projects_pixels() {
    let checker = texture_file("albedo_uv", 2, 1, &[RED, GREEN]);
    let tree = scene(&format!("{DECAL_HEAD}texture_albedo = \"{checker}\"\n"));

    // -- (1) the path is read AND decoded -----------------------------------
    let inputs = editor_3d_render_inputs(&viewport(), &tree);
    assert_eq!(
        inputs.env_effects.decals.len(),
        1,
        "the `.tscn` Decal must reach the render inputs"
    );
    let decal = &inputs.env_effects.decals[0];
    assert!(
        decal.texture_albedo.is_some(),
        "the texture path must survive the `.tscn` — this half already worked"
    );
    let image = decal
        .albedo_image
        .as_ref()
        .expect("the path must be DECODED to texels; carrying it as a String is the whole defect");
    assert_eq!(
        (image.width, image.height),
        (2, 1),
        "the decoded image is the 2x1 checker that was written to disk, so the \
         assertions below are reading real texels"
    );

    // -- (2) …and the texels reach the frame --------------------------------
    let with_decal = render(&tree);
    let baseline = render(&scene(""));
    let foot = footprint(&with_decal, &baseline);
    assert!(
        !foot.is_empty(),
        "a decal over the floor must change pixels at all"
    );

    let reds = foot
        .iter()
        .filter(|&&i| red_dominant(with_decal.pixels[i]))
        .count();
    let greens = foot
        .iter()
        .filter(|&&i| green_dominant(with_decal.pixels[i]))
        .count();
    assert!(
        reds > 0 && greens > 0,
        "both checker texels must appear inside the footprint; got {reds} red and \
         {greens} green of {} footprint pixels. A build that discards the UV from \
         `project_point` stamps one flat colour and fails here",
        foot.len()
    );
    assert!(
        distinct_colors(&with_decal, &foot) >= 2,
        "the acceptance's own wording: at least two distinct colours inside the \
         decal footprint"
    );

    // -- (3) …and only inside it --------------------------------------------
    let outside: Vec<usize> = (0..with_decal.pixels.len())
        .filter(|i| !foot.contains(i))
        .collect();
    assert!(
        outside.len() > foot.len(),
        "the decal covers a patch of the floor, not the frame: {} inside vs {} \
         outside",
        foot.len(),
        outside.len()
    );
    // The top-left pixel is backdrop — no decal footprint can reach it, and it
    // is the "one colour outside" the acceptance asks for.
    assert!(
        !differs(with_decal.pixels[0], baseline.pixels[0]),
        "the backdrop outside the footprint must be untouched"
    );
    assert!(
        !red_dominant(with_decal.pixels[0]) && !green_dominant(with_decal.pixels[0]),
        "and it must not be either checker colour"
    );
}

// ---------------------------------------------------------------------------
// Controls
// ---------------------------------------------------------------------------

/// ⚠️ The control that makes the acceptance mean something. Without it, the
/// test above would also pass on a build that painted *any* two colours over
/// the footprint — including one that varied the stamp with depth and never
/// read a texel.
///
/// A decal with **no** texture must still stamp a single flat `modulate`, which
/// is both the old behaviour and the correct behaviour for an untextured decal.
/// Asserted by hue rather than by an exact colour count, because the fog and
/// tonemap passes downstream of `apply_decal_pass` legitimately vary the
/// footprint's exact values with depth.
#[test]
fn an_untextured_decal_still_stamps_one_flat_modulate() {
    let flat = scene(&format!("{DECAL_HEAD}modulate = Color(1, 0, 0, 1)\n"));
    let with_decal = render(&flat);
    let foot = footprint(&with_decal, &render(&scene("")));

    assert!(
        !foot.is_empty(),
        "an untextured decal still stamps modulate"
    );

    let reds = foot
        .iter()
        .filter(|&&i| red_dominant(with_decal.pixels[i]))
        .count();
    let greens = foot
        .iter()
        .filter(|&&i| green_dominant(with_decal.pixels[i]))
        .count();
    assert!(
        reds > 0,
        "a red `modulate` must tint the footprint red — the fallback path for a \
         decal with no texture must not regress to nothing"
    );
    assert_eq!(
        greens, 0,
        "and NOTHING in the footprint may be green: with no texture there is only \
         one colour to stamp"
    );
}

/// 🔑 The transpose guard. Two distinct colours in the footprint would also
/// appear if `u` and `v` were swapped, so this pins each axis to the screen
/// direction it actually drives.
///
/// ⚠️ `project_point` maps decal-local **x → u** and decal-local **z → v**. With
/// the camera on the +z axis looking down -z, x runs across the screen and z
/// runs into it. So a texture that varies only in u must split the footprint
/// left-to-right, and one that varies only in v must split it near-to-far —
/// and each must leave the *other* axis alone. A build that swapped the two
/// would pass every other test in this file.
#[test]
fn the_projection_uses_both_uv_axes() {
    let baseline = render(&scene(""));

    // Varies in u only (2 wide, 1 tall).
    let u_tex = texture_file("split_u", 2, 1, &[RED, GREEN]);
    let u_frame = render(&scene(&format!(
        "{DECAL_HEAD}texture_albedo = \"{u_tex}\"\n"
    )));
    let u_foot = footprint(&u_frame, &baseline);

    // Varies in v only (1 wide, 2 tall).
    let v_tex = texture_file("split_v", 1, 2, &[RED, GREEN]);
    let v_frame = render(&scene(&format!(
        "{DECAL_HEAD}texture_albedo = \"{v_tex}\"\n"
    )));
    let v_foot = footprint(&v_frame, &baseline);

    let split = |frame: &FrameBuffer, foot: &[usize]| {
        let reds: Vec<usize> = foot
            .iter()
            .copied()
            .filter(|&i| red_dominant(frame.pixels[i]))
            .collect();
        let greens: Vec<usize> = foot
            .iter()
            .copied()
            .filter(|&i| green_dominant(frame.pixels[i]))
            .collect();
        assert!(
            !reds.is_empty() && !greens.is_empty(),
            "both texels must show: {} red, {} green",
            reds.len(),
            greens.len()
        );
        (centroid(&reds), centroid(&greens))
    };

    let ((urx, ury), (ugx, ugy)) = split(&u_frame, &u_foot[..]);
    assert!(
        (urx - ugx).abs() > 3.0,
        "a texture varying in u must split the footprint ACROSS the screen; the \
         two colours' centroids are at x={urx:.1} and x={ugx:.1}"
    );
    assert!(
        (ury - ugy).abs() < 3.0,
        "…and must NOT split it top-to-bottom — y={ury:.1} vs y={ugy:.1} means u \
         is driving the v axis (the two are transposed)"
    );

    let ((vrx, vry), (vgx, vgy)) = split(&v_frame, &v_foot[..]);
    assert!(
        (vry - vgy).abs() > 3.0,
        "a texture varying in v must split the footprint near-to-far, i.e. \
         top-to-bottom on screen; centroids at y={vry:.1} and y={vgy:.1}"
    );
    assert!(
        (vrx - vgx).abs() < 3.0,
        "…and must NOT split it left-to-right — x={vrx:.1} vs x={vgx:.1} means v \
         is driving the u axis"
    );
}

/// The bead's second clause: "an emission texture brightens only its lit
/// texels".
///
/// ⚠️ The albedo is deliberately black. Emission **adds** to the pixel and
/// clamps at 1.0, so over a lit white floor it would be invisible — the test
/// would report "emission does nothing" on a build where emission works
/// perfectly. A black stamp gives it the headroom to show.
#[test]
fn an_emission_texture_brightens_only_its_lit_texels() {
    let dark = format!("{DECAL_HEAD}modulate = Color(0, 0, 0, 1)\n");
    let emission = texture_file("emission_half", 2, 1, &[BLACK, WHITE]);

    let off = render(&scene(&dark));
    let on = render(&scene(&format!(
        "{dark}texture_emission = \"{emission}\"\n"
    )));

    // Both scenes carry the same decal, so the footprint is identical and any
    // difference between these two frames is the emission texture alone.
    let foot = footprint(&off, &render(&scene("")));
    assert!(!foot.is_empty(), "the dark decal stamps a footprint");

    let lum = |c: Color| c.r + c.g + c.b;
    let brighter: Vec<usize> = foot
        .iter()
        .copied()
        .filter(|&i| lum(on.pixels[i]) > lum(off.pixels[i]) + 1e-3)
        .collect();
    let darker = foot
        .iter()
        .filter(|&&i| lum(on.pixels[i]) < lum(off.pixels[i]) - 1e-3)
        .count();

    assert!(
        !brighter.is_empty(),
        "the white half of the emission texture must brighten its texels"
    );
    assert_eq!(
        darker, 0,
        "emission only ever adds — nothing in the footprint may get darker"
    );

    // 🔑 The UV half of the clause: "only its lit texels". A build that sampled
    // one texel and applied it everywhere would brighten the whole footprint.
    let unlit: Vec<usize> = foot
        .iter()
        .copied()
        .filter(|i| !brighter.contains(i))
        .collect();
    assert!(
        !unlit.is_empty(),
        "the BLACK half of the emission texture must leave its texels alone; all \
         {} footprint pixels brightened, which is what ignoring the UV looks like",
        foot.len()
    );

    // …and the two halves are on opposite sides, not interleaved noise.
    let (bx, _) = centroid(&brighter);
    let (ux, _) = centroid(&unlit);
    assert!(
        (bx - ux).abs() > 3.0,
        "the lit and unlit texels must occupy different halves of the footprint; \
         centroids at x={bx:.1} and x={ux:.1}"
    );

    // Nothing outside the footprint moved.
    let leaked = (0..on.pixels.len())
        .filter(|i| !foot.contains(i) && differs(on.pixels[*i], off.pixels[*i]))
        .count();
    assert_eq!(
        leaked, 0,
        "emission must stay inside the decal's box — {leaked} pixels outside the \
         footprint changed"
    );
}

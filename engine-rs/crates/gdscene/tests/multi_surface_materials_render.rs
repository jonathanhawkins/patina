//! pat-s3rrv.187: a mesh's ADDITIONAL surfaces are drawn, each with its own
//! `surface_material_override`.
//!
//! # What was wrong
//!
//! `gdrender3d` never read `Mesh3D::surfaces`. Both backends iterated only the
//! top-level `vertices`/`indices` — the primary surface — so a two-surface mesh
//! silently drew half of itself and a `surface_material_override/1` could never
//! reach a pixel. The load path already flattened it: `packed_scene` writes
//! `__surfmat<N>__*` companions onto the node, and `scene_saver` writes them
//! back out, but **nothing in between ever read them**. The data arrived and
//! stopped.
//!
//! # ⚠️ The index is off by one against Godot's property names
//!
//! `Mesh3D::surfaces` holds the surfaces *beyond* the primary one, so
//! `surfaces[i]` is Godot's `surface_material_override/{i + 1}`. Override `0` is
//! the primary surface and travels as the instance material, exactly as before.
//! `surface_materials_for_node` skips companion `0` for that reason, and this
//! file asserts the pairing rather than trusting it.

use gdcore::math::{Color, Vector3};
use gdcore::math3d::{Basis, Transform3D};
use gdrender3d::renderer::FrameBuffer3D;
use gdrender3d::test_adapter::capture_frame_3d;
use gdrender3d::SoftwareRenderer3D;
use gdscene::node::Node;
use gdscene::render_server_3d::{surface_materials_for_node, MaterialTextureCache};
use gdscene::scene_tree::SceneTree;
use gdserver3d::material::{Material3D, ShadingMode};
use gdserver3d::mesh::{Mesh3D, PrimitiveType, Surface3D};
use gdserver3d::server::RenderingServer3D;
use gdserver3d::viewport::Viewport3D;
use gdvariant::Variant;

const W: u32 = 64;
const H: u32 = 64;

/// A camera-facing quad spanning `x0..x1`, wound the way `Mesh3D::cube` winds
/// its `+Z` face so it survives back-face culling.
fn quad(x0: f32, x1: f32) -> (Vec<Vector3>, Vec<Vector3>, Vec<[f32; 2]>, Vec<u32>) {
    (
        vec![
            Vector3::new(x0, -0.9, 0.0),
            Vector3::new(x1, -0.9, 0.0),
            Vector3::new(x1, 0.9, 0.0),
            Vector3::new(x0, 0.9, 0.0),
        ],
        vec![Vector3::new(0.0, 0.0, 1.0); 4],
        vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        vec![0, 1, 2, 0, 2, 3],
    )
}

/// Primary surface on the LEFT; `extra` additional surfaces on the RIGHT.
///
/// The two are disjoint on screen so a pixel's colour names which surface drew
/// it — overlapping quads would let the depth test decide and turn a rendering
/// question into a z-fighting one.
fn two_surface_mesh(extra: bool) -> Mesh3D {
    let mut mesh = Mesh3D::new(PrimitiveType::Triangles);
    let (v, n, uv, i) = quad(-1.2, -0.2);
    mesh.vertices = v;
    mesh.normals = n;
    mesh.uvs = uv;
    mesh.indices = i;

    if extra {
        let (v, n, uv, i) = quad(0.2, 1.2);
        // ⚠️ Every field spelled out: `Surface3D` derives no `Default`, so there
        // is no `..Default::default()` to lean on here.
        mesh.surfaces.push(Surface3D {
            vertices: v,
            normals: n,
            uvs: uv,
            indices: i,
            primitive_type: PrimitiveType::Triangles,
            lods: Vec::new(),
        });
    }
    mesh
}

fn unlit(r: f32, g: f32, b: f32) -> Material3D {
    Material3D {
        albedo: Color::new(r, g, b, 1.0),
        // Unlit on purpose: with no light in the scene a shaded surface is
        // black, and every colour claim below would be vacuous.
        shading_mode: ShadingMode::Unlit,
        ..Material3D::default()
    }
}

fn render(extra: bool, overrides: Option<Vec<Option<Material3D>>>) -> FrameBuffer3D {
    let mut renderer = SoftwareRenderer3D::new();
    let id = renderer.create_instance();
    renderer.set_mesh(id, two_surface_mesh(extra));
    renderer.set_material(id, unlit(1.0, 0.0, 0.0));
    if let Some(mats) = overrides {
        renderer.set_surface_materials(id, mats);
    }
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -4.0),
        },
    );
    capture_frame_3d(&mut renderer, &Viewport3D::new(W, H))
}

/// Counts pixels where one channel clearly dominates.
///
/// Hue dominance rather than an exact colour: it survives any tone mapping or
/// exposure the post chain applies, which an equality assertion would not.
fn dominant(fb: &FrameBuffer3D, want_red: bool) -> usize {
    let mut count = 0usize;
    for y in 0..fb.height {
        for x in 0..fb.width {
            let c = fb.get_pixel(x, y);
            let hit = if want_red {
                c.r > c.b + 0.2 && c.r > 0.2
            } else {
                c.b > c.r + 0.2 && c.b > 0.2
            };
            if hit {
                count += 1;
            }
        }
    }
    count
}

/// **The regression.** Both surfaces reach the framebuffer, each wearing its own
/// material.
#[test]
fn a_second_surface_renders_with_its_own_override() {
    let fb = render(true, Some(vec![Some(unlit(0.0, 0.0, 1.0))]));

    let red = dominant(&fb, true);
    let blue = dominant(&fb, false);

    assert!(
        red > 20,
        "the PRIMARY surface must still draw ({red} red px) — if this is the \
         assertion that failed, the change broke the ordinary single-surface \
         path rather than adding to it"
    );
    assert!(
        blue > 20,
        "the ADDITIONAL surface never reached the framebuffer ({blue} blue px). \
         Before pat-s3rrv.187 `gdrender3d` did not read `Mesh3D::surfaces` at \
         all, so a two-surface mesh drew only its primary surface"
    );
}

/// ⚠️ The vacuity guard: the extra surface must add GEOMETRY, not just recolour
/// what was already drawn.
///
/// Without it, a bug that drew the primary surface twice — once per material —
/// would satisfy the test above perfectly.
#[test]
fn the_extra_surface_adds_geometry_of_its_own() {
    let one = render(false, None);
    let two = render(true, None);

    let one_red = dominant(&one, true);
    let two_red = dominant(&two, true);

    assert!(
        one_red > 20,
        "the single-surface control must draw something"
    );
    assert!(
        two_red > one_red + 20,
        "adding a surface must add lit pixels: one surface {one_red}, two \
         surfaces {two_red}. Equal counts mean the second surface was skipped \
         and the override merely repainted the first"
    );
}

/// With no override the extra surface falls back to the instance material,
/// which is what Godot draws for a surface that has none.
#[test]
fn an_unoverridden_surface_uses_the_instance_material() {
    let fb = render(true, None);

    assert_eq!(
        dominant(&fb, false),
        0,
        "nothing blue was authored, so nothing may be blue — a non-zero count \
         means an override leaked in from somewhere"
    );
    assert!(
        dominant(&fb, true) > 20,
        "both surfaces should be red, falling back to the instance material"
    );
}

/// The sync half: `__surfmat<N>__*` companions become a list parallel to
/// `Mesh3D::surfaces`.
///
/// ⚠️ Authored at companion index **2**, leaving 1 absent, so the +1 mapping and
/// the gap handling are both exercised. An implementation that collapses gaps
/// would put this material at index 0 and pass a denser fixture.
#[test]
fn surfmat_companions_map_to_surfaces_with_the_offset() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree
        .add_child(root, Node::new("Mesh", "MeshInstance3D"))
        .expect("child adds");
    let n = tree.get_node_mut(node).expect("node exists");
    n.set_property(
        "__surfmat2__albedo_color",
        Variant::Color(Color::new(0.0, 0.0, 1.0, 1.0)),
    );

    let mut textures = MaterialTextureCache::new();
    let mats = surface_materials_for_node(&tree, node, &mut textures);

    assert_eq!(
        mats.len(),
        2,
        "companion 2 is `surfaces[1]`, so the list must be long enough to hold \
         it: {mats:?}"
    );
    assert!(
        mats[0].is_none(),
        "surface 0 had no override and must stay empty rather than absorbing \
         surface 1's material — collapsing the gap shifts every later material"
    );
    let second = mats[1].as_ref().expect("companion 2 maps to surfaces[1]");
    assert!(
        second.albedo.b > second.albedo.r,
        "the authored blue must survive into the material, got {:?}",
        second.albedo
    );
}

/// Companion `0` belongs to the primary surface and must NOT appear in the
/// additional-surface list, or every override would be off by one.
#[test]
fn companion_zero_is_not_treated_as_an_extra_surface() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree
        .add_child(root, Node::new("Mesh", "MeshInstance3D"))
        .expect("child adds");
    let n = tree.get_node_mut(node).expect("node exists");
    n.set_property(
        "__surfmat0__albedo_color",
        Variant::Color(Color::new(0.0, 1.0, 0.0, 1.0)),
    );

    let mut textures = MaterialTextureCache::new();
    assert!(
        surface_materials_for_node(&tree, node, &mut textures).is_empty(),
        "override 0 is the primary surface's and arrives as the instance \
         material; treating it as an extra surface would paint the wrong one"
    );
}

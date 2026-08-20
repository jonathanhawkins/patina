//! pat-s3rrv.205: per-surface LOD index selection.
//!
//! Godot stores coarser index buffers per surface and draws the **coarsest**
//! one whose screen-space error still fits `Viewport.mesh_lod_threshold`,
//! scaled by the instance's `lod_bias`.
//!
//! ⚠️ The error is a screen-space quantity in **pixels**, not a world distance.
//! Selecting on camera distance instead looks right at one field of view and is
//! wrong at every other — the same class of mistake as applying a curve
//! tolerance in the wrong space.

use gdserver3d::mesh::{PrimitiveType, Surface3D, SurfaceLod};

fn surface_with_lods() -> Surface3D {
    Surface3D {
        vertices: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        // Full detail: 4 triangles.
        indices: (0..12).collect(),
        primitive_type: PrimitiveType::Triangles,
        lods: vec![
            SurfaceLod {
                error: 1.0,
                indices: (0..9).collect(), // 3 triangles
            },
            SurfaceLod {
                error: 4.0,
                indices: (0..6).collect(), // 2 triangles
            },
            SurfaceLod {
                error: 16.0,
                indices: (0..3).collect(), // 1 triangle
            },
        ],
    }
}

/// ⚠️ The COARSEST level that fits wins, not the first or the finest.
///
/// Picking the first match would return the 1.0-error level for every budget
/// above 1.0 — the mesh would never actually simplify, and the bug is invisible
/// unless a test uses a budget that admits more than one level.
#[test]
fn the_coarsest_level_within_budget_is_selected() {
    let s = surface_with_lods();
    assert_eq!(
        s.select_lod(4.0).len(),
        6,
        "budget 4 admits errors 1 and 4; take 4"
    );
    assert_eq!(
        s.select_lod(100.0).len(),
        3,
        "a generous budget takes the coarsest"
    );
    assert_eq!(
        s.select_lod(1.0).len(),
        9,
        "budget 1 admits only the finest level"
    );
}

/// A budget below every level falls back to full detail.
#[test]
fn a_budget_under_every_level_draws_full_detail() {
    let s = surface_with_lods();
    assert_eq!(
        s.select_lod(0.5).len(),
        12,
        "no level qualifies, so the original index buffer is drawn"
    );
}

/// ⚠️ Zero or negative means "never degrade", not "degrade maximally".
///
/// A caller that has not set a viewport threshold passes 0. That must look like
/// today's behaviour — full detail — rather than silently selecting the
/// coarsest level everywhere.
#[test]
fn a_zero_threshold_never_degrades() {
    let s = surface_with_lods();
    assert_eq!(s.select_lod(0.0).len(), 12);
    assert_eq!(s.select_lod(-1.0).len(), 12);
}

/// ⚠️ A surface with no LOD levels must draw, not vanish.
///
/// Every surface in the engine predates LOD support and has an empty `lods`.
/// Returning an empty slice for those would make the entire existing scene
/// disappear — a far worse failure than not switching.
#[test]
fn a_surface_without_lods_draws_its_own_indices() {
    let s = Surface3D {
        vertices: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        indices: (0..6).collect(),
        primitive_type: PrimitiveType::Triangles,
        lods: Vec::new(),
    };
    assert_eq!(s.select_lod(1000.0).len(), 6, "no levels means full detail");
    assert_eq!(s.select_lod(0.0).len(), 6);
}

/// An empty index buffer on a level is ignored rather than selected.
///
/// Selecting it would draw nothing, which reads as a culling bug rather than a
/// malformed LOD.
#[test]
fn a_level_with_no_indices_is_skipped() {
    let mut s = surface_with_lods();
    s.lods.push(SurfaceLod {
        error: 32.0,
        indices: Vec::new(),
    });
    assert_eq!(
        s.select_lod(1000.0).len(),
        3,
        "the empty level must not win despite having the largest error"
    );
}

/// Levels are selected by error, not by their order in the vector.
#[test]
fn ordering_in_the_vector_does_not_decide() {
    let mut s = surface_with_lods();
    s.lods.reverse();
    assert_eq!(
        s.select_lod(4.0).len(),
        6,
        "same answer with the levels reversed"
    );
    assert_eq!(s.select_lod(100.0).len(), 3);
}

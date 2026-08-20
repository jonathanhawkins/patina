//! pat-s3rrv.130 clause (d) — there is exactly ONE 3D particle implementation.
//!
//! `gdserver3d::particles3d` was an 819-line second 3D particle system
//! (`Particle3D`, `ParticleProcessMaterial3D`, `EmissionShape3D`, `DrawMode3D`,
//! `GPUParticles3D`) re-exported from this crate's root with **no consumers
//! anywhere in the workspace**. The live simulator — the one `MainLoop` steps
//! every frame and publishes positions from — is `gdscene::particle3d`. Two
//! divergent implementations of the same feature is how a fix lands in the half
//! nothing runs.
//!
//! ⚠️ **Both modules define the same type NAMES**, so a bare
//! `grep -rn 'Particle3D'` cannot tell them apart and reads as "plenty of
//! consumers" for a module that had none. The sweep that actually settled it
//! was path-qualified: of 502 `gdserver3d::` references in workspace source,
//! zero mentioned particles, and nothing imports this crate with a glob.
//!
//! ⚠️ **What proves the removal was safe is the build, not this file.** Any
//! surviving consumer would now fail to resolve, so a green workspace is the
//! real evidence. This test is a REINTRODUCTION guard: it fails if the module
//! or its re-export comes back, which is the likely way the duplicate returns —
//! someone implements 3D particle rendering, greps the name, finds this crate's
//! copy, and revives it instead of `gdscene::particle3d`.

/// The crate root, read at compile time.
const LIB_RS: &str = include_str!("../src/lib.rs");

/// 🔑 The module declaration must not come back.
#[test]
fn gdserver3d_does_not_declare_a_particles3d_module() {
    assert!(
        !LIB_RS.contains("pub mod particles3d;"),
        "🔑 `gdserver3d::particles3d` is back. It is a DUPLICATE of the live \
         simulator in `gdscene::particle3d`, which is the one MainLoop steps. \
         3D particle work belongs there; reviving this module recreates the \
         two-implementations split pat-s3rrv.130 removed"
    );
}

/// …and neither must the re-export that put its types on the crate root.
///
/// ⚠️ Asserted separately from the module declaration. These are two different
/// mistakes: a `pub mod` with no re-export still compiles the dead code, and a
/// re-export without the module does not compile at all — so a single combined
/// assertion would report the wrong one.
#[test]
fn gdserver3d_does_not_reexport_particle_types() {
    assert!(
        !LIB_RS.contains("particles3d::"),
        "`gdserver3d` must not re-export particle types from a `particles3d` \
         module; `gdscene::particle3d` owns 3D particles"
    );
    for ty in ["ParticleProcessMaterial3D", "DrawMode3D"] {
        assert!(
            !LIB_RS.contains(ty),
            "`{ty}` is a `gdserver3d::particles3d` type and must not reappear \
             on this crate's root"
        );
    }
}

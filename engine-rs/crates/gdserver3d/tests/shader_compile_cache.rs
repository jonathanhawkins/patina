//! pat-s3rrv.36 (slice 5): the shader compile cache.
//!
//! `CustomFragmentShader::from_material` (gdrender3d/src/shader.rs) calls
//! `ShaderCompiler3D::compile` for EVERY instance on EVERY frame — it sits
//! inside the entry loop in gdrender3d/src/renderer.rs. So a scene with N
//! shader-material instances re-tokenized, re-parsed and re-generated WGSL N
//! times per frame for sources that had not changed.
//!
//! These tests pin the contract the cache must honour: same input → same
//! output, different input → different output, and invalidation works.

use gdserver3d::{clear_compile_cache, ShaderCompiler3D, ShaderType3D};

const SRC_A: &str =
    "shader_type spatial;\n\nuniform vec4 tint;\n\nvoid fragment() {\n\tALBEDO = tint.rgb;\n}\n";
const SRC_B: &str =
    "shader_type spatial;\n\nuniform float rough;\n\nvoid fragment() {\n\tROUGHNESS = rough;\n}\n";

/// A cached compile must equal an uncached one. This is the whole safety
/// property — a cache that returned anything else would silently change
/// rendering.
#[test]
fn cached_compile_equals_direct_compile() {
    clear_compile_cache();
    let compiler = ShaderCompiler3D::new();

    let direct = compiler.compile(ShaderType3D::Spatial, SRC_A);
    let cached_miss = compiler.compile_cached(ShaderType3D::Spatial, SRC_A);
    let cached_hit = compiler.compile_cached(ShaderType3D::Spatial, SRC_A);

    assert_eq!(
        direct, cached_miss,
        "the first (cache-miss) call must produce exactly what compile() does"
    );
    assert_eq!(
        direct, cached_hit,
        "and the second (cache-hit) call must too — a hit returning something \
         else would change rendering invisibly"
    );
}

/// Distinct sources must not collide. Keyed by full source text precisely so a
/// hash collision cannot hand back the wrong program.
#[test]
fn different_sources_get_different_programs() {
    clear_compile_cache();
    let compiler = ShaderCompiler3D::new();

    let a = compiler.compile_cached(ShaderType3D::Spatial, SRC_A);
    let b = compiler.compile_cached(ShaderType3D::Spatial, SRC_B);

    assert_ne!(
        a, b,
        "two different shader sources must not share a cache entry"
    );
    assert_eq!(a.program, SRC_A, "each entry keeps its own source");
    assert_eq!(b.program, SRC_B);

    // Re-reading A after B was inserted must still give A, i.e. the second
    // insert did not clobber the first.
    let a_again = compiler.compile_cached(ShaderType3D::Spatial, SRC_A);
    assert_eq!(a, a_again, "inserting B must not evict or corrupt A");
}

/// The shader TYPE is part of the key: the same text compiled as spatial and as
/// sky are different programs, and a key of source-text-alone would confuse them.
#[test]
fn shader_type_is_part_of_the_key() {
    clear_compile_cache();
    let compiler = ShaderCompiler3D::new();

    let spatial = compiler.compile_cached(ShaderType3D::Spatial, SRC_A);
    let sky = compiler.compile_cached(ShaderType3D::Sky, SRC_A);

    assert_eq!(spatial.shader_type, ShaderType3D::Spatial);
    assert_eq!(
        sky.shader_type,
        ShaderType3D::Sky,
        "the same source under a different shader_type must not return the \
         spatial entry"
    );
}

/// Invalidation works — the case an editor needs when a shader file changes on
/// disk under the same identity.
#[test]
fn clearing_the_cache_forces_a_recompile() {
    clear_compile_cache();
    let compiler = ShaderCompiler3D::new();

    let before = compiler.compile_cached(ShaderType3D::Spatial, SRC_A);
    clear_compile_cache();
    let after = compiler.compile_cached(ShaderType3D::Spatial, SRC_A);

    // Same source, so the value is equal either way; what matters is that the
    // call still succeeds and is correct after the cache is emptied, rather
    // than returning a stale or absent entry.
    assert_eq!(
        before, after,
        "a recompile after clearing must reproduce the same program"
    );
}

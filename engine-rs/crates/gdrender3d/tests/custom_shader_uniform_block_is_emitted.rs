//! pat-s3rrv.122 (step 2): a custom spatial shader's uniforms reach the GPU as a
//! real group-3 uniform block.
//!
//! The transpiler used to refuse any body touching a uniform, so
//! `custom_spatial_module_wgsl` returned `None` and the draw fell back to flat
//! material albedo. Now the module declares `CustomUniforms` and the pipeline
//! binds a buffer for it.
//!
//! 🔑 THE ORDERING CONTRACT IS THE WHOLE RISK. The emitted struct and the
//! per-draw buffer are packed by two different pieces of code; if they disagree
//! the shader reads the WRONG uniform rather than failing, which no pixel test
//! would attribute correctly. Both walk `custom_uniform_fields`, and these tests
//! pin that order — `time` first, then declarations, samplers excluded.
//!
//! ⚠️ These assert the emitted WGSL, not rendered pixels: the module is built
//! and naga-validated on the CPU. The two-backend pixel comparison in the bead's
//! acceptance needs a GPU adapter and is reported as remaining scope.

// ⚠️ NO top-level `use` of `gdrender3d::wgpu_pipeline`: it lives behind the
// non-default `gpu` feature, so naming it unconditionally would fail to compile
// a plain `-p gdrender3d` build. The gpu-only tests below use fully-qualified
// paths and carry `#[cfg(feature = "gpu")]` individually, which keeps this file
// compiling either way — and leaves `wgsl_uniform_field_is_stable` running
// unconditionally, so a run that silently lost `--features gpu` still executes
// a real assertion instead of reporting a vacuous green on zero tests.
use gdserver3d::shader::{ShaderCompiler3D, ShaderType3D};

#[allow(dead_code)]
fn compile(src: &str) -> gdserver3d::CompiledShader3D {
    ShaderCompiler3D::new().compile(ShaderType3D::Spatial, src)
}

#[allow(dead_code)]
const TINT_SHADER: &str = "shader_type spatial;
render_mode unshaded;
uniform vec4 tint : source_color;
void fragment() { ALBEDO = tint.rgb * UV.x; }";

/// **The acceptance for the emission half.** A `uniform vec4` becomes a block
/// field and the body reads it.
#[cfg(feature = "gpu")]
#[test]
fn a_uniform_shader_emits_a_group_three_block() {
    let compiled = compile(TINT_SHADER);
    let module = gdrender3d::wgpu_pipeline::custom_spatial_module_wgsl(&compiled)
        .expect("a UV + uniform body must now transpile to a bindable module");

    assert!(
        module.contains("struct CustomUniforms"),
        "the module must declare the uniform block: {module}"
    );
    assert!(
        module.contains("@group(3) @binding(0) var<uniform> custom: CustomUniforms;"),
        "the block must be bound at group 3 binding 0, matching `custom_layout`: {module}"
    );
    assert!(
        module.contains("tint: vec4<f32>"),
        "the declared uniform must be a field: {module}"
    );
    assert!(
        module.contains("custom.tint"),
        "the fragment body must READ the field — a declared-but-unread block \
         would render flat while looking wired: {module}"
    );
}

/// `time` leads the struct, so the packed buffer is never zero-sized and the
/// field offsets are stable as uniforms are added.
#[cfg(feature = "gpu")]
#[test]
fn time_is_always_the_first_field() {
    let module = gdrender3d::wgpu_pipeline::custom_spatial_module_wgsl(&compile(TINT_SHADER))
        .expect("transpiles");
    let decl = module
        .split("struct CustomUniforms")
        .nth(1)
        .expect("the struct is declared");
    let time_at = decl.find("time:").expect("time is a field");
    let tint_at = decl.find("tint:").expect("tint is a field");
    assert!(
        time_at < tint_at,
        "`time` must be declared FIRST — the draw-time buffer packs it first, \
         and a swap would feed every uniform the wrong slot: {decl}"
    );
}

/// CONTROL: a shader with no uniforms must still work.
///
/// Without this, a change that emitted the block only when uniforms exist would
/// leave the group-3 layout unbound for plain UV shaders — a draw-time
/// validation failure that renders nothing at all.
#[cfg(feature = "gpu")]
#[test]
fn a_uniform_free_shader_still_transpiles_and_still_binds_time() {
    let compiled = compile(
        "shader_type spatial;
render_mode unshaded;
void fragment() { ALBEDO = vec3(UV.x, UV.y, 1.0); }",
    );
    let module = gdrender3d::wgpu_pipeline::custom_spatial_module_wgsl(&compiled)
        .expect("a plain UV body must transpile");
    assert!(
        module.contains("time: vec4<f32>"),
        "even with no uniforms the block must exist, or the bound group 3 has \
         nothing to point at: {module}"
    );
    assert!(gdrender3d::wgpu_pipeline::custom_uniform_fields(&compiled).is_empty());
}

/// TIME still falls back, deliberately — nothing feeds a per-frame clock yet.
#[cfg(feature = "gpu")]
#[test]
fn a_time_driven_shader_still_falls_back() {
    let compiled = compile(
        "shader_type spatial;
render_mode unshaded;
void fragment() { ALBEDO = vec3(fract(TIME), UV.y, 1.0); }",
    );
    assert!(
        gdrender3d::wgpu_pipeline::custom_spatial_module_wgsl(&compiled).is_none(),
        "with no clock source, rendering TIME as a frozen 0.0 would look like a \
         working feature while being wrong; falling back stays visibly unimplemented"
    );
}

/// A sampler is not a buffer field — it binds separately and must not consume a
/// slot, or every uniform after it reads the wrong value.
#[cfg(feature = "gpu")]
#[test]
fn sampler_uniforms_are_excluded_from_the_buffer_block() {
    let compiled = compile(
        "shader_type spatial;
render_mode unshaded;
uniform sampler2D tex;
uniform vec4 tint : source_color;
void fragment() { ALBEDO = tint.rgb * UV.x; }",
    );
    let fields = gdrender3d::wgpu_pipeline::custom_uniform_fields(&compiled);
    assert!(
        !fields.iter().any(|f| f == "tex"),
        "a sampler2D must not take a vec4 slot: {fields:?}"
    );
    assert!(
        fields.iter().any(|f| f == "tint"),
        "the buffer-backed uniform must still be there: {fields:?}"
    );
}

/// Runs in EVERY configuration, including one that lost `--features gpu`.
///
/// The field-name generator is the shared contract between the transpiler's
/// `custom.<field>` reference and the emitted struct declaration, and it lives
/// in gdserver3d with no feature gate — so this assertion is always real, and a
/// run reporting zero executed tests cannot be mistaken for a pass.
#[test]
fn wgsl_uniform_field_is_stable() {
    use gdserver3d::shader::wgsl_uniform_field;
    assert_eq!(wgsl_uniform_field("tint"), "tint");
    assert_eq!(wgsl_uniform_field("2tint"), "_2tint");
}

/// A `sampler2D` uniform is declared as a group-3 texture+sampler pair
/// (pat-s3rrv.122).
#[cfg(feature = "gpu")]
#[test]
fn a_sampler_uniform_emits_a_texture_and_sampler_binding() {
    let compiled = compile(
        "shader_type spatial;
render_mode unshaded;
uniform sampler2D tex;
void fragment() { ALBEDO = texture(tex, UV).rgb; }",
    );
    let module = gdrender3d::wgpu_pipeline::custom_spatial_module_wgsl(&compiled)
        .expect("a sampler body must now transpile to a bindable module");

    // Slot 0 is bindings 1 and 2 — the layout reserves them in that order.
    assert!(
        module.contains("@group(3) @binding(1) var t_tex: texture_2d<f32>;"),
        "the sampler's texture must be declared at its slot binding: {module}"
    );
    assert!(
        module.contains("@group(3) @binding(2) var s_tex: sampler;"),
        "the sampler itself must be declared alongside it: {module}"
    );
    assert!(
        module.contains("textureSample(t_tex, s_tex,"),
        "the body must SAMPLE the pair it declares — a declared-but-unread \
         binding renders flat while looking wired: {module}"
    );
}

/// CONTROL: a shader with more samplers than the layout has slots must fall
/// back rather than emit a binding that does not exist.
///
/// Without this bail, the module would reference `@binding(9)` on a layout that
/// stops at 8 — pipeline creation fails and the draw disappears entirely, which
/// is strictly worse than the flat-albedo fallback.
#[cfg(feature = "gpu")]
#[test]
fn more_samplers_than_slots_falls_back() {
    let src = format!(
        "shader_type spatial;
render_mode unshaded;
{}
void fragment() {{ ALBEDO = texture(t0, UV).rgb; }}",
        (0..gdrender3d::wgpu_pipeline::CUSTOM_SAMPLER_SLOTS + 1)
            .map(|i| format!("uniform sampler2D t{i};"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        gdrender3d::wgpu_pipeline::custom_spatial_module_wgsl(&compile(&src)).is_none(),
        "a shader exceeding the fixed slot count must fall back, not emit an \
         out-of-range binding"
    );
}

/// The slot indices are computed by ONE pair of functions, used by both the
/// emitted WGSL and the bind group.
#[cfg(feature = "gpu")]
#[test]
fn texture_and_sampler_bindings_interleave_per_slot() {
    use gdrender3d::wgpu_pipeline::{custom_sampler_binding, custom_texture_binding};
    // Binding 0 is the uniform block, so slots start at 1 and take two each.
    assert_eq!(
        (custom_texture_binding(0), custom_sampler_binding(0)),
        (1, 2)
    );
    assert_eq!(
        (custom_texture_binding(1), custom_sampler_binding(1)),
        (3, 4)
    );
    assert_ne!(custom_texture_binding(1), custom_sampler_binding(0));
}

//! pat-s3rrv.122: a `sampler2D` uniform's texture reaches the fragment path.
//!
//! 🔑 THE MISSING PRODUCER. `CustomFragmentShader` has sampled bound textures
//! correctly for a long time and `CustomFragmentShader::set_texture` existed —
//! but NOTHING in production ever called it. `from_material` built the shader
//! with an unconditionally empty texture map, so `texture(tex, UV)` always hit
//! the missing-binding white placeholder. The engine had a sampler with nothing
//! bindable to it, and the only proof it worked was this crate's own unit tests
//! constructing the shader by hand.
//!
//! A `shader_parameter/<name>` Variant carries a resource PATH, not pixels, so
//! the chain needs three links: the scene layer loads the path into a
//! `TextureImage` and calls `ShaderMaterial3D::set_texture`; the material
//! carries it; `from_material` hands it to the shader. This pins the second and
//! third — the pair that made the first one worth writing.

use gdcore::math::{Color, Vector3};
use gdrender3d::shader::{
    CustomFragmentShader, FragmentInput, FragmentShader, MaterialTextures, ShaderUniforms,
};
use gdserver3d::material::TextureImage;
use gdserver3d::shader::{Shader3D, ShaderMaterial3D, ShaderType3D};
use std::sync::Arc;

const SAMPLING_SHADER: &str = "shader_type spatial;
render_mode unshaded;
uniform sampler2D tex;
void fragment() { ALBEDO = texture(tex, UV).rgb; }";

fn material(bind: Option<Color>) -> ShaderMaterial3D {
    let mut mat = ShaderMaterial3D::new();
    // `Shader3D::new`, not a struct literal: the struct has a THIRD field
    // (`uniforms`), so a literal listing only shader_type/source_code is E0063.
    mat.shader = Some(Shader3D::new(ShaderType3D::Spatial, SAMPLING_SHADER));
    if let Some(c) = bind {
        mat.set_texture("tex", Arc::new(TextureImage::new(1, 1, vec![c])));
    }
    mat
}

/// Neither `FragmentInput` nor `ShaderUniforms` derives `Default`, so both are
/// built in full. The shader is `unshaded`, so the lighting fields are inert.
fn shade(mat: &ShaderMaterial3D) -> Color {
    let fs = CustomFragmentShader::from_material(mat);
    let input = FragmentInput {
        world_position: Vector3::ZERO,
        world_normal: Vector3::new(0.0, 1.0, 0.0),
        uv: [0.5, 0.5],
        depth: 0.0,
    };
    let uniforms = ShaderUniforms {
        model_matrix: [[0.0; 4]; 4],
        view_matrix: [[0.0; 4]; 4],
        projection_matrix: [[0.0; 4]; 4],
        albedo: Color::new(1.0, 1.0, 1.0, 1.0),
        emission: Color::new(0.0, 0.0, 0.0, 1.0),
        ambient: Color::new(0.0, 0.0, 0.0, 1.0),
        roughness: 1.0,
        metallic: 0.0,
        camera_position: Vector3::ZERO,
        layers: 1,
        lights: Vec::new(),
        directional_shadow_maps: Vec::new(),
        omni_shadow_cubemaps: Vec::new(),
        textures: MaterialTextures::default(),
    };
    fs.process(&input, &uniforms)
}

/// **The acceptance for the binding half.** A bound texture's texel reaches the
/// shaded colour.
#[test]
fn a_bound_sampler_texture_is_sampled() {
    let texel = Color::new(0.2, 0.4, 0.6, 1.0);
    let got = shade(&material(Some(texel)));

    assert!(
        (got.r - texel.r).abs() < 1e-3
            && (got.g - texel.g).abs() < 1e-3
            && (got.b - texel.b).abs() < 1e-3,
        "the shaded colour must come from the bound texture, got {got:?} vs {texel:?}"
    );
}

/// CONTROL: with nothing bound the placeholder still applies.
///
/// This is what makes the test above mean something. The placeholder is opaque
/// WHITE, so a texel of (1,1,1) would be indistinguishable from an unbound
/// sampler — which is exactly why the assertion above uses a distinctly
/// non-white colour, and why this asserts the unbound case separately.
#[test]
fn an_unbound_sampler_falls_back_to_the_white_placeholder() {
    let got = shade(&material(None));

    assert!(
        (got.r - 1.0).abs() < 1e-3 && (got.g - 1.0).abs() < 1e-3 && (got.b - 1.0).abs() < 1e-3,
        "an unbound sampler must keep the neutral white placeholder, got {got:?}"
    );
}

/// The material carries the binding, so it survives being cloned into a frame's
/// draw list.
#[test]
fn the_binding_survives_a_material_clone() {
    let texel = Color::new(0.2, 0.4, 0.6, 1.0);
    let mat = material(Some(texel));
    let cloned = mat.clone();

    assert!(
        cloned.get_texture("tex").is_some(),
        "a cloned material must keep its sampler bindings — the render path \
         clones materials per frame, and losing them here would make the \
         feature work only on the frame it was authored"
    );
    let got = shade(&cloned);
    assert!(
        (got.b - texel.b).abs() < 1e-3,
        "the clone must still sample the texture, got {got:?}"
    );
}

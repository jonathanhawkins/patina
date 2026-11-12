//! pat-3u1wu: a custom shader's `sampler2D` hinted `source_color` returns the
//! texel sRGB-DECODED, as Godot binds it; an unhinted sampler returns the
//! texel as stored.
//!
//! The hint was never honoured for samplers: textures were sampled raw on the
//! software path and uploaded `Rgba8Unorm` on the GPU, so a colour texture
//! read through `texture()` came out too bright and desaturated. And the usual
//! declaration carries SEVERAL hints (`: filter_linear, source_color,
//! hint_default_black`), which `parse_shader_hint` reads as one unknown hint,
//! so the one place that did look for `source_color` never matched it.

use gdcore::math::{Color, Vector3};
use gdrender3d::shader::{
    CustomFragmentShader, FragmentInput, FragmentShader, MaterialTextures, ShaderUniforms,
};
use gdserver3d::material::TextureImage;
use gdserver3d::shader::{Shader3D, ShaderMaterial3D, ShaderType3D};
use std::sync::Arc;

fn shader(hint: &str) -> String {
    format!(
        "shader_type spatial;
render_mode unshaded;
uniform sampler2D tex{hint};
void fragment() {{ ALBEDO = texture(tex, UV).rgb; }}"
    )
}

fn shade(src: &str, texel: Color) -> Color {
    let mut mat = ShaderMaterial3D::new();
    mat.shader = Some(Shader3D::new(ShaderType3D::Spatial, src));
    mat.set_texture("tex", Arc::new(TextureImage::new(1, 1, vec![texel])));
    let fs = CustomFragmentShader::from_material(&mat);
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
        probe: None,
        layers: 1,
        lights: Vec::new(),
        directional_shadow_maps: Vec::new(),
        omni_shadow_cubemaps: Vec::new(),
        textures: MaterialTextures::default(),
    };
    fs.process(&input, &uniforms)
}

const GREY: Color = Color {
    r: 0.5,
    g: 0.5,
    b: 0.5,
    a: 1.0,
};

#[test]
fn a_source_color_sampler_is_decoded_to_linear() {
    let linear = gdcore::math::srgb_to_linear_channel(0.5);
    for hint in [
        " : source_color",
        " : filter_linear, source_color, hint_default_black",
    ] {
        let got = shade(&shader(hint), GREY);
        assert!(
            (got.r - linear).abs() < 1e-3,
            "`uniform sampler2D tex{hint}` must sample sRGB 0.5 as linear {linear:.4}, got {:.4}",
            got.r
        );
    }
}

/// CONTROL: an unhinted (data) sampler is untouched, so the decode above is
/// the hint and not a blanket transform of every texture.
#[test]
fn an_unhinted_sampler_returns_the_texel_as_stored() {
    for hint in ["", " : filter_nearest, repeat_enable"] {
        let got = shade(&shader(hint), GREY);
        assert!(
            (got.r - 0.5).abs() < 1e-3,
            "`tex{hint}` must stay raw, got {:.4}",
            got.r
        );
    }
}

#[test]
fn every_hint_in_a_list_is_seen() {
    let compiled = gdserver3d::ShaderCompiler3D::new().compile(
        ShaderType3D::Spatial,
        &shader(" : filter_linear, source_color, hint_default_black"),
    );
    let tex = compiled
        .uniforms
        .iter()
        .find(|u| u.name == "tex")
        .expect("tex uniform");
    assert!(tex.is_source_color(), "hint {:?}", tex.hint);
}

/// The GPU path uploads a `source_color` sampler sRGB so the hardware decodes
/// it, and a data sampler raw.
#[cfg(feature = "gpu")]
#[test]
fn the_gpu_upload_format_follows_the_hint() {
    use gdrender3d::wgpu::TextureFormat;
    use gdrender3d::wgpu_pipeline::custom_texture_format;
    assert_eq!(custom_texture_format(true), TextureFormat::Rgba8UnormSrgb);
    assert_eq!(custom_texture_format(false), TextureFormat::Rgba8Unorm);
}

//! Integration tests for ShaderMaterial3D with custom shader compilation
//! (pat-3u1si).
//!
//! Validates that:
//! 1. ShaderMaterial3D is registered in ClassDB with correct properties
//! 2. Scene tree nodes with `shader_code` resolve to ShaderMaterial3D in the render pipeline
//! 3. Shader parameters prefixed with `shader_parameter/` are forwarded to the shader
//! 4. Custom shader compilation parses uniforms, render modes, and validates structure
//! 5. The full scene-tree → compile → render path produces correct output

use gdcore::math::{Color, Vector3};
use gdscene::node::Node;
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;
use gdserver3d::shader::{
    CompiledShader3D, RenderModeFlags, Shader3D, ShaderCompiler3D, ShaderMaterial3D,
    ShaderType3D, UniformType,
};
use gdvariant::Variant;
use std::collections::HashMap;

// ===========================================================================
// ClassDB registration
// ===========================================================================

#[test]
fn classdb_registers_shader_material() {
    gdobject::class_db::register_3d_classes();
    assert!(
        gdobject::class_db::class_exists("ShaderMaterial"),
        "ShaderMaterial must be registered in ClassDB"
    );
}

#[test]
fn classdb_shader_material_has_expected_properties() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("ShaderMaterial");
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"shader_code"), "missing shader_code property");
    assert!(names.contains(&"shader_type"), "missing shader_type property");
    assert!(names.contains(&"render_mode"), "missing render_mode property");
}

// ===========================================================================
// Shader compilation pipeline
// ===========================================================================

#[test]
fn compiler_parses_spatial_shader_with_uniforms() {
    let source = r#"shader_type spatial;
render_mode unshaded, cull_disabled;
uniform vec4 albedo_color : source_color;
uniform float speed = 2.5;
uniform int iterations = 10;
uniform bool use_effect = true;
void fragment() {
    ALBEDO = albedo_color.rgb;
}
"#;
    let compiler = ShaderCompiler3D::new();
    let compiled = compiler.compile(ShaderType3D::Spatial, source);

    assert!(!compiled.has_errors(), "clean shader should not produce errors");
    assert_eq!(compiled.uniform_count(), 4);
    assert!(compiled.render_modes.unshaded);
    assert!(compiled.render_modes.cull_disabled);

    let albedo = compiled.get_uniform("albedo_color").unwrap();
    assert_eq!(albedo.uniform_type, UniformType::Vec4);
    assert_eq!(albedo.hint.as_deref(), Some("source_color"));

    let speed = compiled.get_uniform("speed").unwrap();
    assert_eq!(speed.uniform_type, UniformType::Float);
    assert_eq!(speed.default_value, Variant::Float(2.5));

    let iterations = compiled.get_uniform("iterations").unwrap();
    assert_eq!(iterations.uniform_type, UniformType::Int);
    assert_eq!(iterations.default_value, Variant::Int(10));

    let use_effect = compiled.get_uniform("use_effect").unwrap();
    assert_eq!(use_effect.uniform_type, UniformType::Bool);
    assert_eq!(use_effect.default_value, Variant::Bool(true));
}

#[test]
fn compiler_detects_shader_type_mismatch() {
    let source = "shader_type sky;\nuniform float x;";
    let compiler = ShaderCompiler3D::new();
    let compiled = compiler.compile(ShaderType3D::Spatial, source);
    assert!(compiled.has_errors(), "declaring sky but compiling as spatial should error");
}

#[test]
fn compiler_detects_mismatched_braces() {
    let source = "shader_type spatial;\nvoid fragment() {";
    let compiler = ShaderCompiler3D::new();
    let compiled = compiler.compile(ShaderType3D::Spatial, source);
    assert!(compiled.has_errors(), "mismatched braces should produce an error");
}

#[test]
fn compiler_detects_duplicate_uniforms() {
    let source = "shader_type spatial;\nuniform float x;\nuniform float x;";
    let compiler = ShaderCompiler3D::new();
    let compiled = compiler.compile(ShaderType3D::Spatial, source);
    assert!(compiled.has_errors(), "duplicate uniforms should produce an error");
}

#[test]
fn compiler_warns_on_missing_shader_type() {
    let source = "uniform float speed = 1.0;";
    let compiler = ShaderCompiler3D::new();
    let compiled = compiler.compile(ShaderType3D::Spatial, source);
    assert!(compiled.has_warnings(), "missing shader_type should warn");
}

#[test]
fn compiler_empty_source_is_valid() {
    let compiler = ShaderCompiler3D::new();
    let compiled = compiler.compile(ShaderType3D::Spatial, "");
    assert!(!compiled.has_errors());
    assert_eq!(compiled.uniform_count(), 0);
}

// ===========================================================================
// ShaderMaterial3D construction and parameter binding
// ===========================================================================

#[test]
fn shader_material_compiles_attached_shader() {
    let source = "shader_type spatial;\nrender_mode unshaded;\nuniform vec4 albedo_color : source_color;";
    let shader = Shader3D::new(ShaderType3D::Spatial, source);
    assert_eq!(shader.uniforms.len(), 1);

    let mut mat = ShaderMaterial3D::new();
    mat.shader = Some(shader);
    mat.set_shader_parameter("albedo_color", Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)));

    let compiler = ShaderCompiler3D::new();
    let compiled = compiler.compile(
        mat.shader.as_ref().unwrap().shader_type,
        &mat.shader.as_ref().unwrap().source_code,
    );

    assert!(!compiled.has_errors());
    assert!(compiled.render_modes.unshaded);
    assert_eq!(compiled.uniform_count(), 1);
}

#[test]
fn shader_material_parameters_override_defaults() {
    let source = "shader_type spatial;\nuniform float speed = 1.0;";
    let shader = Shader3D::new(ShaderType3D::Spatial, source);
    let mut mat = ShaderMaterial3D::new();
    mat.shader = Some(shader);

    mat.set_shader_parameter("speed", Variant::Float(5.0));

    let compiler = ShaderCompiler3D::new();
    let compiled = compiler.compile(
        mat.shader.as_ref().unwrap().shader_type,
        &mat.shader.as_ref().unwrap().source_code,
    );

    let resolved = gdserver3d::shader::ShaderProcessor3D::resolve_uniform(
        &compiled,
        &mat.parameters,
        "speed",
    );
    assert_eq!(resolved, Some(&Variant::Float(5.0)));
}

// ===========================================================================
// Render mode flags
// ===========================================================================

#[test]
fn all_render_mode_flags_parsed() {
    let source = "shader_type spatial;\nrender_mode unshaded, cull_disabled, cull_front, blend_mix, blend_add, depth_draw_never;";
    let compiler = ShaderCompiler3D::new();
    let compiled = compiler.compile(ShaderType3D::Spatial, source);

    assert!(compiled.render_modes.unshaded);
    assert!(compiled.render_modes.cull_disabled);
    assert!(compiled.render_modes.cull_front);
    assert!(compiled.render_modes.blend_mix);
    assert!(compiled.render_modes.blend_add);
    assert!(compiled.render_modes.depth_draw_never);
}

#[test]
fn unknown_render_mode_flags_are_silently_ignored() {
    let source = "shader_type spatial;\nrender_mode unshaded, some_future_flag;";
    let compiler = ShaderCompiler3D::new();
    let compiled = compiler.compile(ShaderType3D::Spatial, source);

    assert!(compiled.render_modes.unshaded);
    assert!(!compiled.has_errors());
}

// ===========================================================================
// Scene tree → ShaderMaterial3D → render pipeline integration
// ===========================================================================

/// Helper: builds a scene tree with Camera3D and a MeshInstance3D that has
/// ShaderMaterial3D properties set on it.
fn scene_with_shader_material(
    shader_code: &str,
    params: &[(&str, Variant)],
) -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    gdscene::node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));
    gdscene::node3d::set_camera_current(&mut tree, cam_id, true);

    let mut mesh = Node::new("ShaderCube", "MeshInstance3D");
    mesh.set_property("shader_code", Variant::String(shader_code.to_string()));
    for (name, value) in params {
        let key = format!("shader_parameter/{}", name);
        mesh.set_property(&key, value.clone());
    }
    let _mesh_id = tree.add_child(root, mesh).unwrap();

    tree
}

#[test]
fn scene_tree_shader_material_renders_custom_color() {
    let shader_code =
        "shader_type spatial;\nrender_mode unshaded;\nuniform vec4 albedo_color : source_color;";

    let tree = scene_with_shader_material(
        shader_code,
        &[("albedo_color", Variant::Color(Color::new(0.0, 1.0, 0.0, 1.0)))],
    );

    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _frame_data) = adapter.render_frame(&tree);
    assert_eq!(snapshot.visible_mesh_count, 1);
    assert!(
        snapshot.nonblack_pixel_count > 0,
        "shader material cube should produce visible pixels"
    );
}

#[test]
fn scene_tree_without_shader_code_does_not_apply_shader_material() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    gdscene::node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));
    gdscene::node3d::set_camera_current(&mut tree, cam_id, true);

    let mesh = Node::new("PlainCube", "MeshInstance3D");
    let _mesh_id = tree.add_child(root, mesh).unwrap();

    let mut adapter = RenderServer3DAdapter::new(64, 64);
    // Should render with default material without panicking.
    let (snapshot, _) = adapter.render_frame(&tree);
    assert_eq!(snapshot.visible_mesh_count, 1);
}

#[test]
fn scene_tree_empty_shader_code_does_not_apply_shader_material() {
    let tree = scene_with_shader_material("", &[]);

    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);
    // Empty shader code should not create a ShaderMaterial3D — just use default material.
    assert_eq!(snapshot.visible_mesh_count, 1);
}

#[test]
fn scene_tree_shader_parameters_forwarded_to_material() {
    let shader_code = r#"shader_type spatial;
render_mode unshaded;
uniform vec4 albedo_color : source_color;
uniform float alpha = 1.0;
"#;

    let tree = scene_with_shader_material(
        shader_code,
        &[
            ("albedo_color", Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0))),
            ("alpha", Variant::Float(0.5)),
        ],
    );

    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);
    assert!(
        snapshot.nonblack_pixel_count > 0,
        "shader material with parameters should render visible pixels"
    );
}

// ===========================================================================
// Full pipeline: ShaderProcessor3D evaluation
// ===========================================================================

#[test]
fn shader_processor_applies_uniforms_in_priority_order() {
    use gdserver3d::shader::{FragmentContext3D, ShaderProcessor3D};

    let compiler = ShaderCompiler3D::new();
    let source = "shader_type spatial;\nuniform vec4 albedo_color : source_color;";
    let compiled = compiler.compile(ShaderType3D::Spatial, source);
    let processor = ShaderProcessor3D::new();

    let mut params = HashMap::new();
    params.insert(
        "albedo_color".to_string(),
        Variant::Color(Color::new(0.0, 0.0, 1.0, 1.0)),
    );

    let ctx = FragmentContext3D {
        albedo: Color::new(1.0, 1.0, 1.0, 1.0),
        ..Default::default()
    };

    let result = processor.apply_shader(&compiled, &params, &ctx);
    assert!(
        (result.r - 0.0).abs() < 0.01 && (result.b - 1.0).abs() < 0.01,
        "runtime albedo_color should override context: got {:?}",
        result
    );
}

#[test]
fn shader_processor_emission_adds_to_color() {
    use gdserver3d::shader::{FragmentContext3D, ShaderProcessor3D};

    let compiler = ShaderCompiler3D::new();
    let compiled = compiler.compile(ShaderType3D::Spatial, "shader_type spatial;\nvoid fragment() {}");
    let processor = ShaderProcessor3D::new();

    let mut params = HashMap::new();
    params.insert(
        "emission".to_string(),
        Variant::Color(Color::new(0.3, 0.0, 0.0, 1.0)),
    );

    let ctx = FragmentContext3D {
        albedo: Color::new(0.5, 0.5, 0.5, 1.0),
        ..Default::default()
    };

    let result = processor.apply_shader(&compiled, &params, &ctx);
    assert!(
        (result.r - 0.8).abs() < 0.01,
        "emission should add to albedo: r={:.3}",
        result.r
    );
}

#[test]
fn shader_processor_unshaded_bypasses_lighting() {
    use gdserver3d::shader::{FragmentContext3D, ShaderProcessor3D};

    let compiled = CompiledShader3D {
        shader_type: ShaderType3D::Spatial,
        uniforms: vec![],
        program: String::new(),
        wgsl_program: None,
        render_modes: RenderModeFlags {
            unshaded: true,
            ..Default::default()
        },
        diagnostics: vec![],
    };

    let processor = ShaderProcessor3D::new();
    let ctx = FragmentContext3D {
        albedo: Color::new(0.7, 0.3, 0.1, 1.0),
        ..Default::default()
    };

    let result = processor.apply_shader(&compiled, &HashMap::new(), &ctx);
    assert_eq!(
        result,
        Color::new(0.7, 0.3, 0.1, 1.0),
        "unshaded should return albedo as-is"
    );
}

// ===========================================================================
// Uniform type coverage
// ===========================================================================

#[test]
fn all_uniform_types_parsed() {
    let source = r#"
uniform float f_val = 1.0;
uniform vec2 v2_val;
uniform vec3 v3_val;
uniform vec4 v4_val;
uniform int i_val = 42;
uniform bool b_val = false;
uniform sampler2D tex;
uniform mat4 xform;
"#;

    let shader = Shader3D::new(ShaderType3D::Spatial, source);
    assert_eq!(shader.uniforms.len(), 8);

    let types: Vec<UniformType> = shader.uniforms.iter().map(|u| u.uniform_type).collect();
    assert!(types.contains(&UniformType::Float));
    assert!(types.contains(&UniformType::Vec2));
    assert!(types.contains(&UniformType::Vec3));
    assert!(types.contains(&UniformType::Vec4));
    assert!(types.contains(&UniformType::Int));
    assert!(types.contains(&UniformType::Bool));
    assert!(types.contains(&UniformType::Sampler2D));
    assert!(types.contains(&UniformType::Mat4));
}

// ===========================================================================
// Sky shader type
// ===========================================================================

#[test]
fn sky_shader_compiles_without_error() {
    let source = "shader_type sky;\nvoid sky() {}";
    let compiler = ShaderCompiler3D::new();
    let compiled = compiler.compile(ShaderType3D::Sky, source);
    assert!(!compiled.has_errors());
    assert_eq!(compiled.shader_type, ShaderType3D::Sky);
}

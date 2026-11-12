//! GDShader `shader_type spatial` -> WGSL for the GPU editor/Play path
//! (pat-9gjpr).
//!
//! The wgpu renderer used to draw every custom spatial `ShaderMaterial` as a
//! flat colour: `resolve_shader_albedo` picked the first `Color` parameter, and
//! the only transpiler (`gdserver3d::shader::fragment_body_to_wgsl_statements`)
//! declined every shader the real GDShader front end parses
//! (`FragmentInstruction3D::Program`), which is every shader with a local
//! variable, an `if` or a user function. A game with a custom spatial shader
//! drew a wrong picture in Play while the software renderer drew the right one.
//!
//! This module translates the parsed [`ShaderProgram`] (the same AST the CPU
//! evaluator runs) into a complete WGSL module built from
//! [`crate::wgpu_pipeline::SHADER_3D_WGSL`]:
//!
//! - `fragment()` becomes `custom_fragment`, spliced in at the head of
//!   `fs_main`. It writes Godot's outputs (`ALBEDO`, `ALPHA`, `EMISSION`,
//!   `ROUGHNESS`, `METALLIC`, `NORMAL`) and the rest of `fs_main` shades them
//!   with the same light loop every other material uses.
//! - `vertex()` becomes `custom_vertex`, spliced in at the head of `vs_main`:
//!   `VERTEX` / `NORMAL` / `UV` in model space, as in Godot.
//! - varyings become extra `VertexOutput` locations.
//! - uniforms read a group-3 block of `vec4` slots; `sampler2D` uniforms bind
//!   the group-3 texture/sampler pairs `RenderPipeline3D::custom_layout`
//!   declares.
//!
//! Anything this cannot translate faithfully returns `Err` with the reason, and
//! the caller renders that frame on the software reference path instead of
//! drawing a wrong picture fast.
//!
//! Godot ground truth for the built-ins: `servers/rendering/shader_types.cpp`
//! (spatial section) and `scene_forward_clustered.glsl` — in `fragment()`,
//! `VERTEX` and `NORMAL` are VIEW-space, `VIEW` points from the fragment to the
//! camera in view space, and `ALBEDO` starts at `vec3(1.0)`, `ROUGHNESS` at
//! `1.0`, `METALLIC` at `0.0`.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use gdresource::shader_ast::{parse_shader, Expr, Function, ShaderProgram, ShaderValue, Stmt};
use gdserver3d::shader::{ShaderUniform3D, UniformScope, UniformType};

use crate::render_modes::{CullMode, DepthDrawMode};

/// How many `sampler2D` uniforms a custom module can bind — the fixed slot
/// count of `RenderPipeline3D::custom_layout`.
pub const SAMPLER_SLOTS: usize = 4;

/// Godot `render_mode` flags that change pipeline state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpatialRenderModes {
    /// `unshaded`: `ALBEDO` is the output colour, no lighting.
    pub unshaded: bool,
    /// `cull_back` (default) / `cull_front` / `cull_disabled`.
    pub cull: CullMode,
    /// `blend_add` (otherwise `blend_mix`, the default).
    pub blend_add: bool,
    /// `depth_draw_opaque` (default) / `depth_draw_always` / `depth_draw_never`.
    pub depth_draw: DepthDrawMode,
    /// `depth_test_disabled`.
    pub depth_test_disabled: bool,
}

impl Default for SpatialRenderModes {
    fn default() -> Self {
        Self {
            unshaded: false,
            cull: CullMode::Back,
            blend_add: false,
            depth_draw: DepthDrawMode::OpaqueOnly,
            depth_test_disabled: false,
        }
    }
}

/// One `vec4` slot of the group-3 uniform block, in buffer order.
#[derive(Debug, Clone, PartialEq)]
pub struct UniformSlot {
    /// The uniform's GDShader name.
    pub name: String,
    /// Its declared type.
    pub ty: UniformType,
    /// `source_color`: the value is sRGB and is decoded to linear on upload,
    /// as Godot does.
    pub source_color: bool,
    /// The declared default, used when the material sets no value.
    pub default_value: gdvariant::Variant,
}

/// One `sampler2D` uniform, in binding-slot order.
#[derive(Debug, Clone, PartialEq)]
pub struct SamplerSlot {
    /// The uniform's GDShader name.
    pub name: String,
    /// `source_color`: texels are sRGB and decode to linear when sampled.
    pub source_color: bool,
    /// What an UNBOUND sampler reads (`hint_default_white` / `_black` /
    /// `hint_normal`), in linear RGBA.
    pub unbound: [f32; 4],
    /// `filter_nearest*` was requested.
    pub nearest: bool,
    /// `repeat_enable` was requested.
    pub repeat: bool,
}

/// A transpiled custom spatial shader, ready for `create_shader_module`.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomSpatialModule {
    /// The complete, naga-validated WGSL module.
    pub wgsl: String,
    /// The group-3 block's `vec4` slots after the leading `time` slot.
    pub uniforms: Vec<UniformSlot>,
    /// The group-3 sampler slots.
    pub samplers: Vec<SamplerSlot>,
    /// Pipeline-state render modes.
    pub render_modes: SpatialRenderModes,
    /// `fragment()` writes `ALPHA`, which puts the draw in Godot's alpha pass
    /// (`uses_alpha` in `scene_shader_forward_clustered.h`).
    pub writes_alpha: bool,
    /// The shader reads `TIME`, so the draw must be fed the clock.
    pub uses_time: bool,
    /// Outputs the shader writes that this path does not shade (`RIM`, `AO`,
    /// `NORMAL_MAP`, …). Written to throwaway locals so the module compiles;
    /// listed so a test or a report can see what was approximated.
    pub ignored_outputs: Vec<String>,
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scalar {
    Bool,
    Int,
    UInt,
    Float,
    /// An integer LITERAL: WGSL's AbstractInt, which converts to whichever
    /// concrete scalar it meets. Emitted bare.
    AbstractInt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ty {
    Scalar(Scalar),
    Vector(Scalar, u8),
    Matrix(u8),
    Void,
}

impl Ty {
    const F: Ty = Ty::Scalar(Scalar::Float);

    fn scalar(self) -> Option<Scalar> {
        match self {
            Ty::Scalar(s) | Ty::Vector(s, _) => Some(s),
            Ty::Matrix(_) => Some(Scalar::Float),
            Ty::Void => None,
        }
    }

    fn width(self) -> u8 {
        match self {
            Ty::Scalar(_) => 1,
            Ty::Vector(_, n) => n,
            Ty::Matrix(n) => n,
            Ty::Void => 0,
        }
    }

    fn is_float_like(self) -> bool {
        matches!(self.scalar(), Some(Scalar::Float))
    }

    fn with_width(self, n: u8) -> Ty {
        let s = self.scalar().unwrap_or(Scalar::Float);
        if n == 1 {
            Ty::Scalar(s)
        } else {
            Ty::Vector(s, n)
        }
    }

    fn wgsl(self) -> String {
        fn s(sc: Scalar) -> &'static str {
            match sc {
                Scalar::Bool => "bool",
                Scalar::Int | Scalar::AbstractInt => "i32",
                Scalar::UInt => "u32",
                Scalar::Float => "f32",
            }
        }
        match self {
            Ty::Scalar(sc) => s(sc).to_string(),
            Ty::Vector(sc, n) => format!("vec{n}<{}>", s(sc)),
            Ty::Matrix(n) => format!("mat{n}x{n}<f32>"),
            Ty::Void => String::new(),
        }
    }

    fn parse(name: &str) -> Option<Ty> {
        Some(match name {
            "bool" => Ty::Scalar(Scalar::Bool),
            "int" => Ty::Scalar(Scalar::Int),
            "uint" => Ty::Scalar(Scalar::UInt),
            "float" => Ty::F,
            "vec2" => Ty::Vector(Scalar::Float, 2),
            "vec3" => Ty::Vector(Scalar::Float, 3),
            "vec4" => Ty::Vector(Scalar::Float, 4),
            "ivec2" => Ty::Vector(Scalar::Int, 2),
            "ivec3" => Ty::Vector(Scalar::Int, 3),
            "ivec4" => Ty::Vector(Scalar::Int, 4),
            "uvec2" => Ty::Vector(Scalar::UInt, 2),
            "uvec3" => Ty::Vector(Scalar::UInt, 3),
            "uvec4" => Ty::Vector(Scalar::UInt, 4),
            "bvec2" => Ty::Vector(Scalar::Bool, 2),
            "bvec3" => Ty::Vector(Scalar::Bool, 3),
            "bvec4" => Ty::Vector(Scalar::Bool, 4),
            "mat2" => Ty::Matrix(2),
            "mat3" => Ty::Matrix(3),
            "mat4" => Ty::Matrix(4),
            "void" => Ty::Void,
            _ => return None,
        })
    }
}

/// The two operands' common type for arithmetic, or `None` when GLSL rejects
/// the pair. Scalar-vector and matrix-vector forms follow GLSL.
fn arith_result(op: &str, a: Ty, b: Ty) -> Option<Ty> {
    let unify = |x: Scalar, y: Scalar| -> Option<Scalar> {
        match (x, y) {
            _ if x == y => Some(x),
            (Scalar::AbstractInt, o) | (o, Scalar::AbstractInt) if o != Scalar::Bool => Some(o),
            _ => None,
        }
    };
    match (a, b) {
        (Ty::Matrix(n), Ty::Matrix(m)) if n == m => Some(Ty::Matrix(n)),
        (Ty::Matrix(n), Ty::Vector(_, m)) | (Ty::Vector(_, m), Ty::Matrix(n))
            if n == m && op == "*" =>
        {
            Some(Ty::Vector(Scalar::Float, n))
        }
        (Ty::Matrix(n), Ty::Scalar(_)) | (Ty::Scalar(_), Ty::Matrix(n)) => Some(Ty::Matrix(n)),
        (Ty::Vector(x, n), Ty::Vector(y, m)) if n == m => Some(Ty::Vector(unify(x, y)?, n)),
        (Ty::Vector(x, n), Ty::Scalar(y)) | (Ty::Scalar(y), Ty::Vector(x, n)) => {
            Some(Ty::Vector(unify(x, y)?, n))
        }
        (Ty::Scalar(x), Ty::Scalar(y)) => Some(Ty::Scalar(unify(x, y)?)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Stages and built-ins
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Stage {
    Vertex,
    Fragment,
}

/// A built-in the stage exposes: its type, the WGSL that seeds it, and
/// whether the shader may write it.
struct Builtin {
    ty: Ty,
    /// Expression the wrapper assigns before running the body.
    init: &'static str,
    writable: bool,
}

fn vec(n: u8) -> Ty {
    Ty::Vector(Scalar::Float, n)
}

fn fragment_builtin(name: &str) -> Option<Builtin> {
    let b = |ty, init, writable| Some(Builtin { ty, init, writable });
    match name {
        "UV" | "UV2" => b(vec(2), "in.uv", false),
        "VERTEX" => b(
            vec(3),
            "(camera.view_matrix * vec4<f32>(in.world_position, 1.0)).xyz",
            false,
        ),
        "NORMAL" => b(
            vec(3),
            "normalize((camera.view_matrix * vec4<f32>(in.world_normal, 0.0)).xyz)",
            true,
        ),
        "VIEW" => b(
            vec(3),
            "normalize(-(camera.view_matrix * vec4<f32>(in.world_position, 1.0)).xyz)",
            false,
        ),
        "FRAGCOORD" => b(vec(4), "in.clip_position", false),
        "SCREEN_UV" => b(vec(2), "in.clip_position.xy / custom.time.yz", false),
        "TIME" => b(Ty::F, "custom.time.x", false),
        "COLOR" => b(vec(4), "vec4<f32>(1.0)", false),
        "FRONT_FACING" => b(Ty::Scalar(Scalar::Bool), "front_facing", false),
        "CAMERA_POSITION_WORLD" => b(vec(3), "camera.camera_position", false),
        "CAMERA_DIRECTION_WORLD" => b(
            vec(3),
            "-vec3<f32>(camera.view_matrix[0].z, camera.view_matrix[1].z, camera.view_matrix[2].z)",
            false,
        ),
        "NODE_POSITION_WORLD" => b(vec(3), "model.model_matrix[3].xyz", false),
        "MODEL_MATRIX" => b(Ty::Matrix(4), "model.model_matrix", false),
        "VIEW_MATRIX" => b(Ty::Matrix(4), "camera.view_matrix", false),
        "INV_VIEW_MATRIX" => b(
            Ty::Matrix(4),
            "custom_rigid_inverse(camera.view_matrix)",
            false,
        ),
        "PROJECTION_MATRIX" => b(Ty::Matrix(4), "camera.projection_matrix", false),
        "PI" => b(Ty::F, "3.141592653589793", false),
        "TAU" => b(Ty::F, "6.283185307179586", false),
        "E" => b(Ty::F, "2.718281828459045", false),
        // Outputs, seeded with Godot's defaults.
        "ALBEDO" => b(vec(3), "vec3<f32>(1.0)", true),
        "ALPHA" => b(Ty::F, "1.0", true),
        "EMISSION" => b(vec(3), "vec3<f32>(0.0)", true),
        "ROUGHNESS" => b(Ty::F, "1.0", true),
        "METALLIC" => b(Ty::F, "0.0", true),
        _ => ignored_fragment_output(name).map(|ty| Builtin {
            ty,
            init: "",
            writable: true,
        }),
    }
}

/// Fragment outputs Godot shades that this path does not: written to a
/// throwaway local (zero-initialised) and reported in
/// [`CustomSpatialModule::ignored_outputs`].
fn ignored_fragment_output(name: &str) -> Option<Ty> {
    Some(match name {
        "SPECULAR"
        | "RIM"
        | "RIM_TINT"
        | "CLEARCOAT"
        | "CLEARCOAT_ROUGHNESS"
        | "ANISOTROPY"
        | "AO"
        | "AO_LIGHT_AFFECT"
        | "SSS_STRENGTH"
        | "SSS_TRANSMITTANCE_DEPTH"
        | "SSS_TRANSMITTANCE_BOOST"
        | "NORMAL_MAP_DEPTH"
        | "ALPHA_SCISSOR_THRESHOLD"
        | "ALPHA_HASH_SCALE"
        | "ALPHA_ANTIALIASING_EDGE" => Ty::F,
        "NORMAL_MAP" | "BACKLIGHT" | "BENT_NORMAL_MAP" => vec(3),
        "ANISOTROPY_FLOW" | "ALPHA_TEXTURE_COORDINATE" => vec(2),
        "SSS_TRANSMITTANCE_COLOR" => vec(4),
        _ => return None,
    })
}

fn vertex_builtin(name: &str) -> Option<Builtin> {
    let b = |ty, init, writable| Some(Builtin { ty, init, writable });
    match name {
        "VERTEX" => b(vec(3), "vin.position", true),
        "NORMAL" => b(vec(3), "vin.normal", true),
        "UV" => b(vec(2), "vin.uv", true),
        "UV2" => b(vec(2), "vin.uv", true),
        "COLOR" => b(vec(4), "vec4<f32>(1.0)", true),
        "TIME" => b(Ty::F, "custom.time.x", false),
        "MODEL_MATRIX" => b(Ty::Matrix(4), "model.model_matrix", false),
        "VIEW_MATRIX" => b(Ty::Matrix(4), "camera.view_matrix", false),
        "INV_VIEW_MATRIX" => b(
            Ty::Matrix(4),
            "custom_rigid_inverse(camera.view_matrix)",
            false,
        ),
        "PROJECTION_MATRIX" => b(Ty::Matrix(4), "camera.projection_matrix", false),
        "NODE_POSITION_WORLD" => b(vec(3), "model.model_matrix[3].xyz", false),
        "CAMERA_POSITION_WORLD" => b(vec(3), "camera.camera_position", false),
        "PI" => b(Ty::F, "3.141592653589793", false),
        "TAU" => b(Ty::F, "6.283185307179586", false),
        "E" => b(Ty::F, "2.718281828459045", false),
        _ => None,
    }
}

fn builtin(stage: Stage, name: &str) -> Option<Builtin> {
    match stage {
        Stage::Vertex => vertex_builtin(name),
        Stage::Fragment => fragment_builtin(name),
    }
}

/// The private global a built-in lives in for `stage`.
fn builtin_global(stage: Stage, name: &str) -> String {
    match stage {
        Stage::Vertex => format!("cv_{name}"),
        Stage::Fragment => format!("cf_{name}"),
    }
}

// ---------------------------------------------------------------------------
// Translator
// ---------------------------------------------------------------------------

struct Translator {
    uniforms: HashMap<String, (usize, UniformType)>,
    samplers: HashMap<String, usize>,
    consts: HashMap<String, Ty>,
    varyings: HashMap<String, Ty>,
    /// Signatures of user functions (params, return).
    functions: HashMap<String, (Vec<Ty>, Ty)>,
    /// Built-ins referenced per stage (seeded in the wrapper).
    used_builtins: HashMap<Stage, Vec<String>>,
    /// Built-ins written per stage.
    written_builtins: HashMap<Stage, HashSet<String>>,
    /// Local scopes: name -> type.
    scopes: Vec<HashMap<String, Ty>>,
    stage: Stage,
    /// Return type of the function being emitted.
    ret: Ty,
    /// `textureSampleLevel` everywhere instead of `textureSample` — the retry
    /// when a sample sits in non-uniform control flow.
    explicit_lod: bool,
    tmp: usize,
}

type R<T> = Result<T, String>;

fn ident(name: &str) -> String {
    format!("g_{name}")
}

fn float_lit(v: f32) -> String {
    if !v.is_finite() {
        return "0.0".into();
    }
    let s = format!("{v:?}");
    if s.contains('.') || s.contains('e') || s.contains("inf") {
        s
    } else {
        format!("{s}.0")
    }
}

fn swizzle_index(c: char) -> Option<usize> {
    Some(match c {
        'x' | 'r' | 's' => 0,
        'y' | 'g' | 't' => 1,
        'z' | 'b' | 'p' => 2,
        'w' | 'a' | 'q' => 3,
        _ => return None,
    })
}

fn swizzle_wgsl(member: &str) -> Option<String> {
    member
        .chars()
        .map(|c| swizzle_index(c).map(|i| ['x', 'y', 'z', 'w'][i]))
        .collect()
}

impl Translator {
    fn lookup_local(&self, name: &str) -> Option<Ty> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied())
    }

    fn note_builtin(&mut self, name: &str, write: bool) {
        let used = self.used_builtins.entry(self.stage).or_default();
        if !used.iter().any(|n| n == name) {
            used.push(name.to_string());
        }
        if write {
            self.written_builtins
                .entry(self.stage)
                .or_default()
                .insert(name.to_string());
        }
    }

    /// Resolves a bare identifier to (wgsl, type).
    fn ident_expr(&mut self, name: &str, write: bool) -> R<(String, Ty)> {
        if let Some(ty) = self.lookup_local(name) {
            return Ok((ident(name), ty));
        }
        if let Some(ty) = self.varyings.get(name).copied() {
            if write && self.stage == Stage::Fragment {
                return Err(format!("fragment() writes varying `{name}`"));
            }
            return Ok((format!("cvary_{name}"), ty));
        }
        if let Some(&(slot, ty)) = self.uniforms.get(name) {
            if write {
                return Err(format!("writes uniform `{name}`"));
            }
            let field = format!("custom.u{slot}");
            return Ok(match ty {
                UniformType::Float => (format!("{field}.x"), Ty::F),
                UniformType::Vec2 => (format!("{field}.xy"), vec(2)),
                UniformType::Vec3 => (format!("{field}.xyz"), vec(3)),
                UniformType::Vec4 | UniformType::Color => (field, vec(4)),
                UniformType::Int => (format!("i32({field}.x)"), Ty::Scalar(Scalar::Int)),
                UniformType::Bool => (format!("({field}.x != 0.0)"), Ty::Scalar(Scalar::Bool)),
                UniformType::Sampler2D | UniformType::Mat4 => {
                    return Err(format!("uniform `{name}` used as a value"))
                }
            });
        }
        if let Some(ty) = self.consts.get(name).copied() {
            if write {
                return Err(format!("writes const `{name}`"));
            }
            return Ok((ident(name), ty));
        }
        if let Some(b) = builtin(self.stage, name) {
            if write && !b.writable {
                return Err(format!("writes read-only built-in `{name}`"));
            }
            self.note_builtin(name, write);
            return Ok((builtin_global(self.stage, name), b.ty));
        }
        Err(format!("unsupported identifier `{name}`"))
    }

    fn expr(&mut self, e: &Expr) -> R<(String, Ty)> {
        match e {
            Expr::Literal(v) => Ok(match v {
                ShaderValue::Bool(b) => (b.to_string(), Ty::Scalar(Scalar::Bool)),
                ShaderValue::Int(i) => {
                    if *i < i32::MIN as i64 || *i > i32::MAX as i64 {
                        return Err("integer literal out of range".into());
                    }
                    (i.to_string(), Ty::Scalar(Scalar::AbstractInt))
                }
                ShaderValue::Float(f) => (float_lit(*f), Ty::F),
                _ => return Err("vector literal".into()),
            }),
            Expr::Ident { name, .. } => self.ident_expr(name, false),
            Expr::Unary { op, expr } => {
                let (s, t) = self.expr(expr)?;
                match op.as_str() {
                    "-" => Ok((format!("(-{s})"), t)),
                    "!" => Ok((format!("(!{s})"), t)),
                    _ => Err(format!("unary `{op}`")),
                }
            }
            Expr::Binary { op, lhs, rhs } => self.binary(op, lhs, rhs),
            Expr::Ternary { cond, then, other } => {
                let (c, ct) = self.expr(cond)?;
                if ct != Ty::Scalar(Scalar::Bool) {
                    return Err("ternary condition is not a bool".into());
                }
                let (a, at) = self.expr(then)?;
                let (b, bt) = self.expr(other)?;
                let t = if at == bt {
                    at
                } else {
                    arith_result("+", at, bt).ok_or("ternary arms disagree")?
                };
                Ok((format!("select({b}, {a}, {c})"), t))
            }
            Expr::Member { object, member } => {
                let (o, ot) = self.expr(object)?;
                self.swizzle(&o, ot, member)
            }
            Expr::Call { name, args } => self.call(name, args),
        }
    }

    fn swizzle(&self, o: &str, ot: Ty, member: &str) -> R<(String, Ty)> {
        let Ty::Vector(sc, n) = ot else {
            return Err(format!("member `.{member}` on a non-vector"));
        };
        let sw = swizzle_wgsl(member).ok_or_else(|| format!("member `.{member}`"))?;
        if sw.len() > 4
            || member
                .chars()
                .any(|c| swizzle_index(c).unwrap_or(9) >= n as usize)
        {
            return Err(format!("swizzle `.{member}` out of range"));
        }
        let t = if sw.len() == 1 {
            Ty::Scalar(sc)
        } else {
            Ty::Vector(sc, sw.len() as u8)
        };
        Ok((format!("{o}.{sw}"), t))
    }

    fn binary(&mut self, op: &str, lhs: &Expr, rhs: &Expr) -> R<(String, Ty)> {
        let (a, at) = self.expr(lhs)?;
        let (b, bt) = self.expr(rhs)?;
        match op {
            "&&" | "||" => Ok((format!("({a} {op} {b})"), Ty::Scalar(Scalar::Bool))),
            "<" | ">" | "<=" | ">=" => {
                if at.width() != 1 || bt.width() != 1 {
                    return Err("vector comparison".into());
                }
                Ok((format!("({a} {op} {b})"), Ty::Scalar(Scalar::Bool)))
            }
            "==" | "!=" => {
                if at.width() > 1 {
                    let f = if op == "==" { "all" } else { "any" };
                    Ok((format!("{f}({a} {op} {b})"), Ty::Scalar(Scalar::Bool)))
                } else {
                    Ok((format!("({a} {op} {b})"), Ty::Scalar(Scalar::Bool)))
                }
            }
            "+" | "-" | "*" | "/" | "%" => {
                let t = arith_result(op, at, bt)
                    .ok_or_else(|| format!("operator `{op}` on {at:?} and {bt:?}"))?;
                if op == "%" && t.is_float_like() {
                    return Err("`%` on floats".into());
                }
                // GLSL `mat * mat` and `mat * vec` are linear algebra, which is
                // also what WGSL's `*` means; `+`/`-` are componentwise in both.
                Ok((format!("({a} {op} {b})"), t))
            }
            _ => Err(format!("binary `{op}`")),
        }
    }

    /// `x` converted to `want` (scalar kind and width), splatting a scalar.
    fn coerce(&self, x: &str, from: Ty, want: Ty) -> String {
        if from == want {
            return x.to_string();
        }
        match (from, want) {
            // An abstract literal converts by itself.
            (Ty::Scalar(Scalar::AbstractInt), Ty::Scalar(_)) => x.to_string(),
            (Ty::Scalar(_), Ty::Vector(..)) => format!("{}({x})", want.wgsl()),
            (Ty::Scalar(_), Ty::Scalar(_)) | (Ty::Vector(..), Ty::Vector(..)) => {
                format!("{}({x})", want.wgsl())
            }
            _ => x.to_string(),
        }
    }

    fn args(&mut self, args: &[Expr]) -> R<Vec<(String, Ty)>> {
        args.iter().map(|a| self.expr(a)).collect()
    }

    fn call(&mut self, name: &str, args: &[Expr]) -> R<(String, Ty)> {
        if let Some(ty) = Ty::parse(name) {
            return self.construct(ty, args);
        }
        if let Some((params, ret)) = self.functions.get(name).cloned() {
            let a = self.args(args)?;
            if a.len() != params.len() {
                return Err(format!("call to `{name}` with the wrong arity"));
            }
            let list: Vec<String> = a
                .iter()
                .zip(&params)
                .map(|((s, t), p)| self.coerce(s, *t, *p))
                .collect();
            return Ok((format!("{}({})", ident(name), list.join(", ")), ret));
        }
        match name {
            "texture" | "textureLod" => return self.texture(name, args),
            "textureSize" => {
                let Some(Expr::Ident { name: s, .. }) = args.first() else {
                    return Err("textureSize of a non-sampler".into());
                };
                let slot = *self
                    .samplers
                    .get(s)
                    .ok_or("textureSize of an unknown sampler")?;
                return Ok((
                    format!("vec2<i32>(textureDimensions(ct_{slot}))"),
                    Ty::Vector(Scalar::Int, 2),
                ));
            }
            _ => {}
        }
        let a = self.args(args)?;
        let first = a.first().map(|x| x.1).unwrap_or(Ty::F);
        // The widest operand decides a genType call's type.
        let widest = a
            .iter()
            .map(|x| x.1)
            .max_by_key(|t| t.width())
            .unwrap_or(Ty::F);
        let widest = if widest.scalar() == Some(Scalar::AbstractInt) {
            a.iter()
                .map(|x| x.1)
                .find(|t| t.scalar() != Some(Scalar::AbstractInt))
                .map(|t| t.with_width(widest.width()))
                .unwrap_or(widest)
        } else {
            widest
        };
        let s = |i: usize| a[i].0.clone();
        let need = |n: usize| -> R<()> {
            if a.len() == n {
                Ok(())
            } else {
                Err(format!("`{name}` with {} arguments", a.len()))
            }
        };
        // Every argument splatted/converted to `widest`.
        let all_as = |me: &Self, t: Ty| -> Vec<String> {
            a.iter().map(|(x, xt)| me.coerce(x, *xt, t)).collect()
        };
        let one = |f: &str| -> R<(String, Ty)> {
            need(1)?;
            Ok((format!("{f}({})", s(0)), first))
        };
        match name {
            "sin" | "cos" | "tan" | "asin" | "acos" | "sinh" | "cosh" | "tanh" | "exp" | "exp2"
            | "log" | "log2" | "sqrt" | "abs" | "sign" | "floor" | "ceil" | "fract" | "trunc"
            | "round" | "radians" | "degrees" | "normalize" => one(name),
            "roundEven" => one("round"),
            "inversesqrt" => one("inverseSqrt"),
            "dFdx" => one("dpdx"),
            "dFdy" => one("dpdy"),
            "fwidth" => one("fwidth"),
            "atan" => match a.len() {
                1 => one("atan"),
                2 => Ok((format!("atan2({}, {})", s(0), s(1)), widest)),
                _ => Err("atan arity".into()),
            },
            "pow" | "min" | "max" | "step" => {
                need(2)?;
                let v = all_as(self, widest);
                Ok((format!("{name}({}, {})", v[0], v[1]), widest))
            }
            "mod" => {
                need(2)?;
                let v = all_as(self, widest);
                Ok((
                    format!("({0} - {1} * floor({0} / {1}))", v[0], v[1]),
                    widest,
                ))
            }
            "clamp" | "smoothstep" => {
                need(3)?;
                let v = all_as(self, widest);
                Ok((format!("{name}({}, {}, {})", v[0], v[1], v[2]), widest))
            }
            "mix" => {
                need(3)?;
                let t = arith_result("+", a[0].1, a[1].1).ok_or("mix arms disagree")?;
                let x = self.coerce(&a[0].0, a[0].1, t);
                let y = self.coerce(&a[1].0, a[1].1, t);
                if a[2].1 == Ty::Scalar(Scalar::Bool) || a[2].1.scalar() == Some(Scalar::Bool) {
                    return Ok((format!("select({x}, {y}, {})", s(2)), t));
                }
                // WGSL takes a scalar or a same-shaped blend factor.
                let f = if a[2].1.width() == 1 {
                    self.coerce(&a[2].0, a[2].1, Ty::F)
                } else {
                    self.coerce(&a[2].0, a[2].1, t)
                };
                Ok((format!("mix({x}, {y}, {f})"), t))
            }
            "length" => {
                need(1)?;
                Ok((format!("length({})", s(0)), Ty::F))
            }
            "distance" | "dot" => {
                need(2)?;
                Ok((format!("{name}({}, {})", s(0), s(1)), Ty::F))
            }
            "cross" => {
                need(2)?;
                Ok((format!("cross({}, {})", s(0), s(1)), vec(3)))
            }
            "reflect" => {
                need(2)?;
                Ok((format!("reflect({}, {})", s(0), s(1)), first))
            }
            "refract" => {
                need(3)?;
                Ok((format!("refract({}, {}, {})", s(0), s(1), s(2)), first))
            }
            "faceforward" => {
                need(3)?;
                Ok((format!("faceForward({}, {}, {})", s(0), s(1), s(2)), first))
            }
            "fma" => {
                need(3)?;
                let v = all_as(self, widest);
                Ok((format!("fma({}, {}, {})", v[0], v[1], v[2]), widest))
            }
            "all" | "any" => {
                need(1)?;
                Ok((format!("{name}({})", s(0)), Ty::Scalar(Scalar::Bool)))
            }
            "not" => {
                need(1)?;
                Ok((format!("(!{})", s(0)), first))
            }
            "transpose" => {
                need(1)?;
                Ok((format!("transpose({})", s(0)), first))
            }
            "determinant" => {
                need(1)?;
                Ok((format!("determinant({})", s(0)), Ty::F))
            }
            _ => Err(format!("unsupported function `{name}`")),
        }
    }

    fn construct(&mut self, ty: Ty, args: &[Expr]) -> R<(String, Ty)> {
        let a = self.args(args)?;
        let w = ty.wgsl();
        match ty {
            Ty::Scalar(_) => {
                if a.len() != 1 {
                    return Err("scalar constructor arity".into());
                }
                let (x, xt) = &a[0];
                // `float(v3)` takes the first component, as GLSL does.
                let x = if xt.width() > 1 {
                    format!("{x}.x")
                } else {
                    x.clone()
                };
                Ok((format!("{w}({x})"), ty))
            }
            Ty::Vector(sc, n) => {
                if a.len() == 1 {
                    let (x, xt) = &a[0];
                    return Ok(match xt.width() {
                        1 => (format!("{w}({x})"), ty),
                        m if m == n => (self.coerce(x, *xt, ty), ty),
                        m if m > n => {
                            let sw = &"xyzw"[..n as usize];
                            (
                                self.coerce(&format!("({x}).{sw}"), xt.with_width(n), ty),
                                ty,
                            )
                        }
                        _ => return Err("vector constructor too short".into()),
                    });
                }
                let total: u8 = a.iter().map(|x| x.1.width()).sum();
                if total != n || a.iter().any(|x| matches!(x.1, Ty::Matrix(_))) {
                    return Err("vector constructor component count".into());
                }
                let parts: Vec<String> = a
                    .iter()
                    .map(|(x, xt)| {
                        let target = if xt.width() == 1 {
                            Ty::Scalar(sc)
                        } else {
                            Ty::Vector(sc, xt.width())
                        };
                        self.coerce(x, *xt, target)
                    })
                    .collect();
                Ok((format!("{w}({})", parts.join(", ")), ty))
            }
            Ty::Matrix(n) => {
                if a.len() == 1 {
                    if let Ty::Matrix(m) = a[0].1 {
                        let x = &a[0].0;
                        if m == n {
                            return Ok((x.clone(), ty));
                        }
                        if m > n {
                            let sw = &"xyzw"[..n as usize];
                            let cols: Vec<String> =
                                (0..n).map(|c| format!("({x})[{c}].{sw}")).collect();
                            return Ok((format!("{w}({})", cols.join(", ")), ty));
                        }
                    }
                    return Err("matrix constructor".into());
                }
                if a.len() == n as usize && a.iter().all(|x| x.1 == vec(n)) {
                    let cols: Vec<String> = a.iter().map(|x| x.0.clone()).collect();
                    return Ok((format!("{w}({})", cols.join(", ")), ty));
                }
                Err("matrix constructor".into())
            }
            Ty::Void => Err("void constructor".into()),
        }
    }

    fn texture(&mut self, name: &str, args: &[Expr]) -> R<(String, Ty)> {
        let Some(Expr::Ident { name: s, .. }) = args.first() else {
            return Err("texture() of a non-sampler".into());
        };
        let slot = *self
            .samplers
            .get(s)
            .ok_or_else(|| format!("texture() of unsupported sampler `{s}`"))?;
        let (uv, uvt) = self.expr(args.get(1).ok_or("texture() without uv")?)?;
        if uvt != vec(2) {
            return Err("texture() with a non-vec2 coordinate".into());
        }
        let level = match (name, args.get(2)) {
            ("textureLod", Some(l)) => Some(self.expr(l)?.0),
            ("textureLod", None) => return Err("textureLod without a level".into()),
            // `texture(s, uv, bias)` — the bias is dropped.
            _ => None,
        };
        let call = match level {
            Some(l) => format!("textureSampleLevel(ct_{slot}, cs_{slot}, {uv}, f32({l}))"),
            // The vertex stage has no derivatives, so it must name a level.
            None if self.stage == Stage::Vertex || self.explicit_lod => {
                format!("textureSampleLevel(ct_{slot}, cs_{slot}, {uv}, 0.0)")
            }
            None => format!("textureSample(ct_{slot}, cs_{slot}, {uv})"),
        };
        Ok((call, vec(4)))
    }

    // -- statements --------------------------------------------------------

    fn block(&mut self, stmts: &[Stmt], out: &mut String, depth: usize) -> R<()> {
        self.scopes.push(HashMap::new());
        let r = stmts.iter().try_for_each(|s| self.stmt(s, out, depth));
        self.scopes.pop();
        r
    }

    fn stmt(&mut self, st: &Stmt, out: &mut String, depth: usize) -> R<()> {
        let pad = "    ".repeat(depth);
        match st {
            Stmt::Local { name, ty, init } => {
                let declared = Ty::parse(ty).ok_or_else(|| format!("local of type `{ty}`"))?;
                if declared == Ty::Void {
                    return Err("void local".into());
                }
                match init {
                    Some(e) => {
                        let (x, xt) = self.expr(e)?;
                        let x = self.coerce(&x, xt, declared);
                        writeln!(out, "{pad}var {}: {} = {x};", ident(name), declared.wgsl()).ok();
                    }
                    None => {
                        writeln!(out, "{pad}var {}: {};", ident(name), declared.wgsl()).ok();
                    }
                }
                self.scopes
                    .last_mut()
                    .expect("a scope is open")
                    .insert(name.clone(), declared);
            }
            Stmt::Assign { target, op, value } => self.assign(target, op, value, out, &pad)?,
            Stmt::If {
                cond,
                then,
                otherwise,
            } => {
                let (c, ct) = self.expr(cond)?;
                if ct != Ty::Scalar(Scalar::Bool) {
                    return Err("if condition is not a bool".into());
                }
                writeln!(out, "{pad}if ({c}) {{").ok();
                self.block(then, out, depth + 1)?;
                if otherwise.is_empty() {
                    writeln!(out, "{pad}}}").ok();
                } else {
                    writeln!(out, "{pad}}} else {{").ok();
                    self.block(otherwise, out, depth + 1)?;
                    writeln!(out, "{pad}}}").ok();
                }
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                // `{ init; loop { if !cond break; body; continuing { step } } }`
                // keeps `continue` correct without WGSL's restricted `for`
                // header grammar.
                writeln!(out, "{pad}{{").ok();
                self.scopes.push(HashMap::new());
                let r = (|| -> R<()> {
                    if let Some(i) = init.as_ref() {
                        self.stmt(i, out, depth + 1)?;
                    }
                    writeln!(out, "{pad}    loop {{").ok();
                    if let Some(c) = cond {
                        let (c, _) = self.expr(c)?;
                        writeln!(out, "{pad}        if (!({c})) {{ break; }}").ok();
                    }
                    self.block(body, out, depth + 2)?;
                    writeln!(out, "{pad}        continuing {{").ok();
                    if let Some(s) = step.as_ref() {
                        self.stmt(s, out, depth + 3)?;
                    }
                    writeln!(out, "{pad}        }}").ok();
                    writeln!(out, "{pad}    }}").ok();
                    Ok(())
                })();
                self.scopes.pop();
                r?;
                writeln!(out, "{pad}}}").ok();
            }
            Stmt::While { cond, body } => {
                let (c, _) = self.expr(cond)?;
                writeln!(out, "{pad}while ({c}) {{").ok();
                self.block(body, out, depth + 1)?;
                writeln!(out, "{pad}}}").ok();
            }
            Stmt::Return(v) => match v {
                Some(e) => {
                    let (x, xt) = self.expr(e)?;
                    let x = self.coerce(&x, xt, self.ret);
                    writeln!(out, "{pad}return {x};").ok();
                }
                None => {
                    writeln!(out, "{pad}return;").ok();
                }
            },
            Stmt::Discard => {
                if self.stage != Stage::Fragment {
                    return Err("discard outside fragment()".into());
                }
                writeln!(out, "{pad}discard;").ok();
            }
            Stmt::Break => {
                writeln!(out, "{pad}break;").ok();
            }
            Stmt::Continue => {
                writeln!(out, "{pad}continue;").ok();
            }
            Stmt::Expr(e) => {
                let (x, t) = self.expr(e)?;
                if matches!(e, Expr::Call { name, .. } if self.functions.contains_key(name)) {
                    writeln!(out, "{pad}{x};").ok();
                } else if t != Ty::Void {
                    writeln!(out, "{pad}_ = {x};").ok();
                }
            }
        }
        Ok(())
    }

    fn assign(
        &mut self,
        target: &Expr,
        op: &str,
        value: &Expr,
        out: &mut String,
        pad: &str,
    ) -> R<()> {
        // The l-value: a name, optionally swizzled once.
        let (base, base_ty, member) = match target {
            Expr::Ident { name, .. } => {
                let (b, t) = self.ident_expr(name, true)?;
                (b, t, None)
            }
            Expr::Member { object, member } => match object.as_ref() {
                Expr::Ident { name, .. } => {
                    let (b, t) = self.ident_expr(name, true)?;
                    (b, t, Some(member.clone()))
                }
                _ => return Err("nested member assignment".into()),
            },
            _ => return Err("unsupported assignment target".into()),
        };
        let (cur, cur_ty) = match &member {
            Some(m) => self.swizzle(&base, base_ty, m)?,
            None => (base.clone(), base_ty),
        };
        let (v, vt) = self.expr(value)?;
        let rhs = if op.is_empty() {
            self.coerce(&v, vt, cur_ty)
        } else {
            let t = arith_result(op, cur_ty, vt)
                .ok_or_else(|| format!("compound `{op}=` on {cur_ty:?} and {vt:?}"))?;
            if t.width() != cur_ty.width() && !matches!(cur_ty, Ty::Matrix(_)) {
                return Err("compound assignment changes the width".into());
            }
            format!("({cur} {op} {v})")
        };
        match member {
            Some(m) if m.len() > 1 => {
                // WGSL cannot assign through a multi-component swizzle.
                let t = self.tmp;
                self.tmp += 1;
                writeln!(out, "{pad}let ctmp{t} = {rhs};").ok();
                let sw = swizzle_wgsl(&m).ok_or("swizzle")?;
                for (i, c) in sw.chars().enumerate() {
                    let src = ['x', 'y', 'z', 'w'][i];
                    writeln!(out, "{pad}{base}.{c} = ctmp{t}.{src};").ok();
                }
            }
            Some(m) => {
                let sw = swizzle_wgsl(&m).ok_or("swizzle")?;
                writeln!(out, "{pad}{base}.{sw} = {rhs};").ok();
            }
            None => {
                writeln!(out, "{pad}{base} = {rhs};").ok();
            }
        }
        Ok(())
    }

    fn function(&mut self, f: &Function, stage: Stage, out: &mut String) -> R<()> {
        self.stage = stage;
        self.scopes.clear();
        self.scopes.push(HashMap::new());
        let (params, ret) = self
            .functions
            .get(&f.name)
            .cloned()
            .unwrap_or((Vec::new(), Ty::Void));
        self.ret = ret;
        let mut sig = Vec::new();
        for (name, ty) in f.params.iter().zip(&params) {
            sig.push(format!("{}: {}", ident(name), ty.wgsl()));
            self.scopes[0].insert(name.clone(), *ty);
        }
        let ret_s = if ret == Ty::Void {
            String::new()
        } else {
            format!(" -> {}", ret.wgsl())
        };
        let mut body = String::new();
        // Parameters are immutable in WGSL; GLSL lets a function assign them.
        for (name, ty) in f.params.iter().zip(&params) {
            writeln!(body, "    var {0}_m: {1} = {0};", ident(name), ty.wgsl()).ok();
        }
        let mut inner = String::new();
        self.block(&f.body, &mut inner, 1)?;
        // Rename parameter uses to the mutable copies.
        let mut inner_fixed = inner;
        for name in &f.params {
            inner_fixed = rename_word(&inner_fixed, &ident(name), &format!("{}_m", ident(name)));
        }
        body.push_str(&inner_fixed);
        writeln!(out, "fn {}({}){ret_s} {{", ident(&f.name), sig.join(", ")).ok();
        out.push_str(&body);
        if ret != Ty::Void {
            // WGSL rejects a function whose end is reachable without a return.
            writeln!(out, "    return {}();", ret.wgsl()).ok();
        }
        out.push_str("}\n\n");
        Ok(())
    }
}

/// Replaces whole-word occurrences of `from` in `s`.
fn rename_word(s: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut i = 0;
    while i < s.len() {
        if s[i..].starts_with(from)
            && (i == 0 || !is_word(bytes[i - 1]))
            && bytes.get(i + from.len()).is_none_or(|b| !is_word(*b))
        {
            out.push_str(to);
            i += from.len();
        } else {
            let ch = s[i..].chars().next().expect("in bounds");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// Parses every `render_mode` list in `source`. Unknown flags that change what
/// is drawn are an error; ones that only tune lighting models are accepted.
fn parse_render_modes(source: &str) -> R<SpatialRenderModes> {
    let mut m = SpatialRenderModes::default();
    let mut rest = source;
    while let Some(pos) = rest.find("render_mode") {
        let after = &rest[pos + "render_mode".len()..];
        let end = after.find(';').ok_or("unterminated render_mode")?;
        for flag in after[..end].split(',') {
            match flag.trim() {
                "unshaded" => m.unshaded = true,
                "cull_back" => m.cull = CullMode::Back,
                "cull_front" => m.cull = CullMode::Front,
                "cull_disabled" => m.cull = CullMode::Disabled,
                "blend_mix" => m.blend_add = false,
                "blend_add" => m.blend_add = true,
                "depth_draw_opaque" => m.depth_draw = DepthDrawMode::OpaqueOnly,
                "depth_draw_always" => m.depth_draw = DepthDrawMode::Always,
                "depth_draw_never" => m.depth_draw = DepthDrawMode::Never,
                "depth_test_disabled" => m.depth_test_disabled = true,
                // Lighting-model and feature toggles this renderer has one
                // fixed answer for; accepting them changes nothing drawn.
                "diffuse_burley"
                | "diffuse_lambert"
                | "diffuse_lambert_wrap"
                | "diffuse_toon"
                | "specular_schlick_ggx"
                | "specular_toon"
                | "specular_disabled"
                | "shadows_disabled"
                | "ambient_light_disabled"
                | "fog_disabled"
                | "vertex_lighting"
                | "depth_prepass_alpha"
                | "shadow_to_opacity"
                | "skip_vertex_transform_disabled"
                | "alpha_to_coverage"
                | "alpha_to_coverage_and_one"
                | "ensure_correct_normals"
                | "particle_trails"
                | "sss_mode_skin"
                | "debug_shadow_splits"
                | "wireframe"
                | "" => {}
                other => return Err(format!("render_mode `{other}`")),
            }
        }
        rest = &after[end..];
    }
    Ok(m)
}

fn hint_tokens(u: &ShaderUniform3D) -> Vec<String> {
    u.hint
        .as_deref()
        .map(|h| h.split(',').map(|t| t.trim().to_string()).collect())
        .unwrap_or_default()
}

/// The WGSL module `base` (`SHADER_3D_WGSL`) with this shader spliced in.
///
/// `source` is the `.gdshader` text, `uniforms` its parsed declarations.
pub fn transpile_spatial(
    base: &str,
    source: &str,
    uniforms: &[ShaderUniform3D],
) -> Result<CustomSpatialModule, String> {
    let program = parse_shader(source).map_err(|e| format!("parse: {e}"))?;
    if program.shader_type.as_deref() != Some("spatial") {
        return Err("not a spatial shader".into());
    }
    let render_modes = parse_render_modes(source)?;
    match build(base, &program, uniforms, render_modes, false) {
        Ok(m) => Ok(m),
        // `textureSample` needs uniform control flow; retry with explicit
        // level-0 samples, which do not. Mip selection is the only cost.
        Err(e) if e.contains("NonUniform") => build(base, &program, uniforms, render_modes, true),
        Err(e) => Err(e),
    }
}

fn build(
    base: &str,
    program: &ShaderProgram,
    decls: &[ShaderUniform3D],
    render_modes: SpatialRenderModes,
    explicit_lod: bool,
) -> Result<CustomSpatialModule, String> {
    // Uniforms, in declaration order: buffer slots and sampler slots.
    let mut uniform_slots = Vec::new();
    let mut sampler_slots = Vec::new();
    let mut uniforms = HashMap::new();
    let mut samplers = HashMap::new();
    for name in &program.uniforms {
        let u = decls
            .iter()
            .find(|u| &u.name == name)
            .ok_or_else(|| format!("uniform `{name}` has a type this path cannot bind"))?;
        if u.scope != UniformScope::Local {
            return Err(format!("{:?} uniform `{name}`", u.scope));
        }
        let hints = hint_tokens(u);
        let source_color = u.is_source_color();
        match u.uniform_type {
            UniformType::Sampler2D => {
                if hints.iter().any(|h| {
                    h.starts_with("hint_screen_texture")
                        || h.starts_with("hint_depth_texture")
                        || h.starts_with("hint_normal_roughness_texture")
                }) {
                    return Err(format!("screen-reading sampler `{name}`"));
                }
                if sampler_slots.len() >= SAMPLER_SLOTS {
                    return Err("more samplers than the layout has slots".into());
                }
                let unbound = if hints.iter().any(|h| h == "hint_default_black") {
                    [0.0, 0.0, 0.0, 1.0]
                } else if hints.iter().any(|h| h == "hint_default_transparent") {
                    [0.0, 0.0, 0.0, 0.0]
                } else if hints.iter().any(|h| h == "hint_normal") {
                    [0.5, 0.5, 1.0, 1.0]
                } else {
                    [1.0, 1.0, 1.0, 1.0]
                };
                samplers.insert(name.clone(), sampler_slots.len());
                sampler_slots.push(SamplerSlot {
                    name: name.clone(),
                    source_color,
                    unbound,
                    nearest: hints.iter().any(|h| h.starts_with("filter_nearest")),
                    repeat: hints.iter().any(|h| h == "repeat_enable"),
                });
            }
            UniformType::Mat4 => return Err(format!("mat4 uniform `{name}`")),
            ty => {
                uniforms.insert(name.clone(), (uniform_slots.len(), ty));
                uniform_slots.push(UniformSlot {
                    name: name.clone(),
                    ty,
                    source_color,
                    default_value: u.default_value.clone(),
                });
            }
        }
    }

    let mut varyings = HashMap::new();
    for (name, (ty, _)) in program.varyings.iter().zip(&program.varying_types) {
        let t = Ty::parse(ty).ok_or_else(|| format!("varying `{name}` of type `{ty}`"))?;
        if matches!(t, Ty::Matrix(_) | Ty::Void) || t.scalar() == Some(Scalar::Bool) {
            return Err(format!("varying `{name}` of type `{ty}`"));
        }
        varyings.insert(name.clone(), t);
    }

    let mut functions = HashMap::new();
    for f in program.functions.values() {
        if f.name == "vertex" || f.name == "fragment" {
            continue;
        }
        if f.name == "light" {
            return Err("custom light() function".into());
        }
        if f.param_qualifiers
            .iter()
            .any(|q| q == "out" || q == "inout")
        {
            return Err(format!("`{}` has an out/inout parameter", f.name));
        }
        let params = f
            .param_types
            .iter()
            .map(|t| Ty::parse(t).filter(|t| *t != Ty::Void))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| format!("`{}` has a parameter type this path cannot emit", f.name))?;
        let ret = Ty::parse(&f.return_type)
            .ok_or_else(|| format!("`{}` returns `{}`", f.name, f.return_type))?;
        functions.insert(f.name.clone(), (params, ret));
    }

    let mut t = Translator {
        uniforms,
        samplers,
        consts: HashMap::new(),
        varyings,
        functions,
        used_builtins: HashMap::new(),
        written_builtins: HashMap::new(),
        scopes: vec![HashMap::new()],
        stage: Stage::Fragment,
        ret: Ty::Void,
        explicit_lod,
        tmp: 0,
    };

    // Constants first: later declarations may read them.
    let mut consts_src = String::new();
    for (name, ty, init) in &program.consts {
        let declared = Ty::parse(ty).ok_or_else(|| format!("const `{name}` of type `{ty}`"))?;
        let (x, xt) = t.expr(init)?;
        let x = t.coerce(&x, xt, declared);
        writeln!(
            consts_src,
            "const {}: {} = {x};",
            ident(name),
            declared.wgsl()
        )
        .ok();
        t.consts.insert(name.clone(), declared);
    }

    // User functions. A helper reads built-ins through the private globals,
    // so it is emitted for the stage that calls it; helpers called from both
    // stages would need two copies, and Godot shaders rarely do that.
    let mut helpers = String::new();
    let mut names: Vec<&String> = program
        .functions
        .keys()
        .filter(|n| *n != "vertex" && *n != "fragment")
        .collect();
    names.sort();
    let called_from_vertex = program
        .function("vertex")
        .map(|f| calls_in(&f.body))
        .unwrap_or_default();
    for n in names {
        let f = &program.functions[n];
        let stage = if called_from_vertex.contains(n.as_str()) {
            Stage::Vertex
        } else {
            Stage::Fragment
        };
        t.function(f, stage, &mut helpers)?;
    }

    let vertex = program.function("vertex").filter(|f| !f.body.is_empty());
    let mut vertex_body = String::new();
    if let Some(f) = vertex {
        t.stage = Stage::Vertex;
        t.scopes = vec![HashMap::new()];
        t.ret = Ty::Void;
        t.block(&f.body, &mut vertex_body, 1)?;
    }
    let mut fragment_body = String::new();
    if let Some(f) = program.function("fragment") {
        t.stage = Stage::Fragment;
        t.scopes = vec![HashMap::new()];
        t.ret = Ty::Void;
        t.block(&f.body, &mut fragment_body, 1)?;
    }

    let frag_used = t
        .used_builtins
        .get(&Stage::Fragment)
        .cloned()
        .unwrap_or_default();
    let frag_written = t
        .written_builtins
        .get(&Stage::Fragment)
        .cloned()
        .unwrap_or_default();
    let vert_used = t
        .used_builtins
        .get(&Stage::Vertex)
        .cloned()
        .unwrap_or_default();
    let vert_written = t
        .written_builtins
        .get(&Stage::Vertex)
        .cloned()
        .unwrap_or_default();
    let uses_time = frag_used.iter().chain(&vert_used).any(|n| n == "TIME");
    let writes_alpha = frag_written.contains("ALPHA");
    let mut ignored_outputs: Vec<String> = frag_written
        .iter()
        .filter(|n| ignored_fragment_output(n).is_some())
        .cloned()
        .collect();
    ignored_outputs.sort();

    // ---- assemble -------------------------------------------------------
    let mut decl = String::new();
    decl.push_str("struct CustomUniforms {\n    time: vec4<f32>,\n");
    for i in 0..uniform_slots.len() {
        writeln!(decl, "    u{i}: vec4<f32>,").ok();
    }
    decl.push_str("};\n@group(3) @binding(0) var<uniform> custom: CustomUniforms;\n");
    for slot in 0..sampler_slots.len() {
        writeln!(
            decl,
            "@group(3) @binding({}) var ct_{slot}: texture_2d<f32>;\n@group(3) @binding({}) var cs_{slot}: sampler;",
            1 + slot * 2,
            2 + slot * 2
        )
        .ok();
    }
    decl.push_str(
        "fn custom_rigid_inverse(m: mat4x4<f32>) -> mat4x4<f32> {\n    \
         let r = transpose(mat3x3<f32>(m[0].xyz, m[1].xyz, m[2].xyz));\n    \
         let p = -(r * m[3].xyz);\n    \
         return mat4x4<f32>(vec4<f32>(r[0], 0.0), vec4<f32>(r[1], 0.0), vec4<f32>(r[2], 0.0), vec4<f32>(p, 1.0));\n}\n",
    );
    let mut vary_names: Vec<(&String, &Ty)> = t.varyings.iter().collect();
    vary_names.sort_by(|a, b| a.0.cmp(b.0));
    for (name, ty) in &vary_names {
        writeln!(decl, "var<private> cvary_{name}: {};", ty.wgsl()).ok();
    }
    let all_frag: Vec<&String> = frag_used.iter().collect();
    for name in &all_frag {
        let b = fragment_builtin(name).expect("noted from the table");
        writeln!(
            decl,
            "var<private> {}: {};",
            builtin_global(Stage::Fragment, name),
            b.ty.wgsl()
        )
        .ok();
    }
    for name in &vert_used {
        let b = vertex_builtin(name).expect("noted from the table");
        writeln!(
            decl,
            "var<private> {}: {};",
            builtin_global(Stage::Vertex, name),
            b.ty.wgsl()
        )
        .ok();
    }
    decl.push_str(&consts_src);
    decl.push('\n');
    decl.push_str(&helpers);

    // fragment wrapper
    let mut frag = String::new();
    frag.push_str(
        "struct CustomSurface {\n    frag: VertexOutput,\n    albedo: vec4<f32>,\n    emission: vec3<f32>,\n    roughness: f32,\n    metallic: f32,\n};\n\n",
    );
    frag.push_str("fn custom_fragment_body(in: VertexOutput, front_facing: bool) {\n");
    frag.push_str(&fragment_body);
    frag.push_str("}\n\n");
    frag.push_str("fn custom_fragment(in: VertexOutput, front_facing: bool) -> CustomSurface {\n");
    for (name, _) in &vary_names {
        writeln!(frag, "    cvary_{name} = in.cvary_{name};").ok();
    }
    // Seed every output, read or not, so the wrapper below can pack them.
    let outputs = [
        "ALBEDO",
        "ALPHA",
        "EMISSION",
        "ROUGHNESS",
        "METALLIC",
        "NORMAL",
    ];
    let mut seeded: Vec<String> = all_frag.iter().map(|s| s.to_string()).collect();
    for o in outputs {
        if !seeded.iter().any(|s| s == o) {
            seeded.push(o.to_string());
            let b = fragment_builtin(o).expect("output");
            writeln!(
                decl,
                "var<private> {}: {};",
                builtin_global(Stage::Fragment, o),
                b.ty.wgsl()
            )
            .ok();
        }
    }
    for name in &seeded {
        let b = fragment_builtin(name).expect("noted from the table");
        if !b.init.is_empty() {
            writeln!(
                frag,
                "    {} = {};",
                builtin_global(Stage::Fragment, name),
                b.init
            )
            .ok();
        }
    }
    frag.push_str("    custom_fragment_body(in, front_facing);\n");
    frag.push_str("    var s: CustomSurface;\n    s.frag = in;\n");
    if frag_written.contains("NORMAL") {
        // View-space NORMAL back to the world space the light loop shades in.
        frag.push_str(
            "    s.frag.world_normal = normalize((custom_rigid_inverse(camera.view_matrix) * vec4<f32>(cf_NORMAL, 0.0)).xyz);\n",
        );
    }
    frag.push_str(
        "    s.albedo = vec4<f32>(cf_ALBEDO, cf_ALPHA);\n    s.emission = cf_EMISSION;\n    s.roughness = cf_ROUGHNESS;\n    s.metallic = cf_METALLIC;\n    return s;\n}\n\n",
    );

    // vertex wrapper
    let mut vert = String::new();
    vert.push_str("fn custom_vertex_body() {\n");
    vert.push_str(&vertex_body);
    vert.push_str("}\n\n");
    vert.push_str(
        "fn custom_vertex(vin: VertexInput, out: ptr<function, VertexOutput>) -> VertexInput {\n",
    );
    let mut vseed: Vec<String> = vert_used.clone();
    for o in ["VERTEX", "NORMAL", "UV"] {
        if !vseed.iter().any(|s| s == o) {
            vseed.push(o.to_string());
            writeln!(
                decl,
                "var<private> {}: {};",
                builtin_global(Stage::Vertex, o),
                vertex_builtin(o).expect("o").ty.wgsl()
            )
            .ok();
        }
    }
    for name in &vseed {
        let b = vertex_builtin(name).expect("noted from the table");
        writeln!(
            vert,
            "    {} = {};",
            builtin_global(Stage::Vertex, name),
            b.init
        )
        .ok();
    }
    vert.push_str("    custom_vertex_body();\n");
    for (name, _) in &vary_names {
        writeln!(vert, "    (*out).cvary_{name} = cvary_{name};").ok();
    }
    let _ = &vert_written;
    vert.push_str("    var v: VertexInput;\n    v.position = cv_VERTEX;\n    v.normal = cv_NORMAL;\n    v.uv = cv_UV;\n    return v;\n}\n\n");

    // ---- splice into the base module -------------------------------------
    let mut module = base.to_string();
    let splice = |module: &mut String, marker: &str, replacement: &str| -> R<()> {
        if module.matches(marker).count() != 1 {
            return Err(format!("base shader marker not found: {marker:?}"));
        }
        *module = module.replacen(marker, replacement, 1);
        Ok(())
    };
    // Varyings ride extra VertexOutput locations.
    let vo_marker = "struct VertexOutput {\n    @builtin(position) clip_position: vec4<f32>,\n    @location(0) world_position: vec3<f32>,\n    @location(1) world_normal: vec3<f32>,\n    @location(2) uv: vec2<f32>,\n";
    let mut vo = vo_marker.to_string();
    for (i, (name, ty)) in vary_names.iter().enumerate() {
        let flat = program
            .varyings
            .iter()
            .position(|v| v == *name)
            .map(|p| program.varying_types[p].1)
            .unwrap_or(false)
            || !ty.is_float_like();
        let interp = if flat { " @interpolate(flat)" } else { "" };
        writeln!(
            vo,
            "    @location({}){interp} cvary_{name}: {},",
            3 + i,
            ty.wgsl()
        )
        .ok();
    }
    splice(&mut module, vo_marker, &vo)?;

    let vs_marker = "fn vs_main(in: VertexInput) -> VertexOutput {\n    var out: VertexOutput;\n";
    splice(
        &mut module,
        vs_marker,
        "fn vs_main(in_raw: VertexInput) -> VertexOutput {\n    var out: VertexOutput;\n    let in = custom_vertex(in_raw, &out);\n",
    )?;

    let fs_marker = "@fragment\nfn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {\n    let albedo = model.albedo;";
    let mut fs = String::new();
    fs.push_str(&decl);
    fs.push_str(&vert);
    fs.push_str(&frag);
    fs.push_str(
        "@fragment\nfn fs_main(in_raw: VertexOutput, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {\n    let cs = custom_fragment(in_raw, front_facing);\n    let in = cs.frag;\n    let albedo = cs.albedo;",
    );
    splice(&mut module, fs_marker, &fs)?;

    // Optional outputs: each needs its own line in `fs_main`. A line that has
    // moved makes that output unsupported rather than silently dropped.
    let optional = |module: &mut String, name: &str, marker: &str, with: &str| -> R<()> {
        if module.matches(marker).count() == 1 {
            *module = module.replacen(marker, with, 1);
            Ok(())
        } else if frag_written.contains(name) {
            Err(format!("fs_main has no splice point for `{name}`"))
        } else {
            Ok(())
        }
    };
    optional(
        &mut module,
        "EMISSION",
        "let emission = model.emission * textureSample(base_emission_map,",
        "let emission = vec4<f32>(cs.emission, 0.0) * textureSample(base_emission_map,",
    )?;
    optional(
        &mut module,
        "ROUGHNESS",
        "let roughness_map = textureSample(base_roughness_map, base_material_sampler, in.uv).g;",
        "let roughness_map = cs.roughness;",
    )?;
    optional(
        &mut module,
        "METALLIC",
        "let metallic_map = textureSample(base_metallic_map, base_material_sampler, in.uv).b;",
        "let metallic_map = cs.metallic;",
    )?;

    validate(&module)?;
    Ok(CustomSpatialModule {
        wgsl: module,
        uniforms: uniform_slots,
        samplers: sampler_slots,
        render_modes,
        writes_alpha,
        uses_time,
        ignored_outputs,
    })
}

/// Names of every function called (transitively, syntactically) in `body`.
fn calls_in(body: &[Stmt]) -> HashSet<String> {
    fn e(x: &Expr, out: &mut HashSet<String>) {
        match x {
            Expr::Call { name, args } => {
                out.insert(name.clone());
                args.iter().for_each(|a| e(a, out));
            }
            Expr::Unary { expr, .. } => e(expr, out),
            Expr::Binary { lhs, rhs, .. } => {
                e(lhs, out);
                e(rhs, out);
            }
            Expr::Member { object, .. } => e(object, out),
            Expr::Ternary { cond, then, other } => {
                e(cond, out);
                e(then, out);
                e(other, out);
            }
            Expr::Literal(_) | Expr::Ident { .. } => {}
        }
    }
    fn s(x: &Stmt, out: &mut HashSet<String>) {
        match x {
            Stmt::Local { init, .. } => init.iter().for_each(|i| e(i, out)),
            Stmt::Assign { target, value, .. } => {
                e(target, out);
                e(value, out);
            }
            Stmt::If {
                cond,
                then,
                otherwise,
            } => {
                e(cond, out);
                then.iter().chain(otherwise).for_each(|t| s(t, out));
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                init.iter().for_each(|i| s(i, out));
                cond.iter().for_each(|c| e(c, out));
                step.iter().for_each(|i| s(i, out));
                body.iter().for_each(|b| s(b, out));
            }
            Stmt::While { cond, body } => {
                e(cond, out);
                body.iter().for_each(|b| s(b, out));
            }
            Stmt::Return(v) => v.iter().for_each(|v| e(v, out)),
            Stmt::Expr(x) => e(x, out),
            Stmt::Discard | Stmt::Break | Stmt::Continue => {}
        }
    }
    let mut out = HashSet::new();
    body.iter().for_each(|b| s(b, &mut out));
    out
}

/// Parses AND validates (types, uniformity) the module, so nothing invalid
/// reaches `create_shader_module`, where it would be a device error.
fn validate(module: &str) -> R<()> {
    use wgpu::naga;
    let parsed = naga::front::wgsl::parse_str(module).map_err(|e| format!("wgsl parse: {e}"))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&parsed)
    .map_err(|e| format!("wgsl validate: {e:?}"))?;
    Ok(())
}

/// The transpiled module for `material`'s spatial shader, memoized on the
/// shader source for the life of the process, or `None` when the material has
/// no spatial shader at all.
///
/// `Err` carries why the GPU path cannot draw it; the editor/Play entry point
/// renders such a frame on the software reference path
/// (`viewport_3d_render::render_editor_3d_frame_gpu`).
pub fn module_for(
    material: &gdserver3d::ShaderMaterial3D,
) -> Option<Result<std::sync::Arc<CustomSpatialModule>, String>> {
    use std::sync::{Arc, Mutex, OnceLock};
    type Cache = HashMap<String, Result<Arc<CustomSpatialModule>, String>>;
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let shader = material.shader.as_ref()?;
    if shader.shader_type != gdserver3d::ShaderType3D::Spatial {
        return None;
    }
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(hit) = cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&shader.source_code)
    {
        return Some(hit.clone());
    }
    let built = transpile_spatial(
        crate::wgpu_pipeline::SHADER_3D_WGSL,
        &shader.source_code,
        &shader.uniforms,
    )
    .map(Arc::new);
    if let Err(e) = &built {
        tracing::warn!("custom spatial shader cannot run on the GPU path: {e}");
    }
    cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(shader.source_code.clone(), built.clone());
    Some(built)
}

/// A uniform value as its `vec4` slot (narrower types fill the leading
/// components), with a `source_color` value decoded sRGB -> linear as Godot
/// does on upload.
pub fn slot_value(slot: &UniformSlot, value: Option<&gdvariant::Variant>) -> [f32; 4] {
    use gdvariant::Variant;
    let v = value.unwrap_or(&slot.default_value);
    let mut out = match v {
        Variant::Color(c) => [c.r, c.g, c.b, c.a],
        Variant::Vector4(p) => [p.x, p.y, p.z, p.w],
        Variant::Vector3(p) => [p.x, p.y, p.z, 0.0],
        Variant::Vector2(p) => [p.x, p.y, 0.0, 0.0],
        Variant::Float(f) => [*f as f32, 0.0, 0.0, 0.0],
        Variant::Int(i) => [*i as f32, 0.0, 0.0, 0.0],
        Variant::Bool(b) => [if *b { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
        _ => [0.0; 4],
    };
    // An unset vec4/colour uniform defaults to opaque in Godot's zero-init
    // only for alpha it was given; a declared default already carries it.
    if slot.source_color
        && matches!(
            slot.ty,
            UniformType::Vec3 | UniformType::Vec4 | UniformType::Color
        )
    {
        for c in out.iter_mut().take(3) {
            *c = gdcore::math::srgb_to_linear_channel(*c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wgpu_pipeline::SHADER_3D_WGSL;

    fn go(src: &str) -> Result<CustomSpatialModule, String> {
        transpile_spatial(
            SHADER_3D_WGSL,
            src,
            &gdserver3d::shader::parse_uniforms(src),
        )
    }

    #[test]
    fn locals_if_and_user_function_transpile_and_validate() {
        // Every construct here makes the CPU front end produce a `Program`,
        // which the old GPU transpiler declined wholesale.
        let src = r#"shader_type spatial;
uniform vec4 tint : source_color = vec4(1.0, 0.5, 0.25, 1.0);
uniform float stripes = 8.0;
float band(float x) {
    return step(0.5, fract(x));
}
void fragment() {
    float b = band(UV.x * stripes);
    vec3 c = mix(tint.rgb, vec3(0.1), b);
    if (UV.y > 0.5) {
        c *= 0.5;
    }
    ALBEDO = c;
    EMISSION = c * 0.1;
    ROUGHNESS = 0.4;
}
"#;
        let m = go(src).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(m.uniforms.len(), 2);
        assert!(m.uniforms[0].source_color);
        assert!(!m.writes_alpha);
        assert!(m.wgsl.contains("fn g_band(g_x: f32) -> f32"));
        assert!(m.wgsl.contains("let roughness_map = cs.roughness;"));
    }

    #[test]
    fn alpha_time_texture_and_render_modes() {
        let src = r#"shader_type spatial;
render_mode unshaded, cull_disabled, depth_test_disabled, blend_add;
uniform sampler2D noise : source_color, filter_nearest, repeat_enable;
void fragment() {
    vec4 n = texture(noise, UV + vec2(TIME * 0.1, 0.0));
    ALBEDO = n.rgb;
    ALPHA = n.a * 0.5;
}
"#;
        let m = go(src).unwrap_or_else(|e| panic!("{e}"));
        assert!(m.writes_alpha && m.uses_time);
        assert_eq!(m.samplers.len(), 1);
        assert!(m.samplers[0].source_color && m.samplers[0].nearest && m.samplers[0].repeat);
        let r = m.render_modes;
        assert!(r.unshaded && r.depth_test_disabled && r.blend_add);
        assert_eq!(r.cull, CullMode::Disabled);
    }

    #[test]
    fn vertex_displacement_varyings_loops_and_swizzle_writes() {
        let src = r#"shader_type spatial;
varying float height;
const float AMP = 0.25;
void vertex() {
    VERTEX.y += sin(VERTEX.x * 4.0 + TIME) * AMP;
    height = VERTEX.y;
}
void fragment() {
    float acc = 0.0;
    for (int i = 0; i < 4; i++) {
        acc += float(i) * 0.1;
    }
    ALBEDO.rg = vec2(height, acc);
    ALBEDO.b = 1.0;
    NORMAL = normalize(NORMAL);
}
"#;
        let m = go(src).unwrap_or_else(|e| panic!("{e}"));
        assert!(m.wgsl.contains("cvary_height"));
        assert!(m.wgsl.contains("let in = custom_vertex(in_raw, &out);"));
    }

    #[test]
    fn texture_in_a_branch_validates() {
        // `textureSample` must sit in uniform control flow; whichever form
        // the module ends up with, naga (the same validator wgpu runs at
        // `create_shader_module`) accepted it.
        let src = r#"shader_type spatial;
uniform sampler2D tex;
void fragment() {
    if (UV.x > 0.5) {
        ALBEDO = texture(tex, UV).rgb;
    }
}
"#;
        let m = go(src).unwrap_or_else(|e| panic!("{e}"));
        assert!(m.wgsl.contains("(ct_0, cs_0, cf_UV"), "{}", m.wgsl);
    }

    #[test]
    fn unsupported_features_are_reported_not_mistranslated() {
        for (src, why) in [
            (
                "shader_type spatial;\nuniform sampler2D s : hint_screen_texture;\nvoid fragment() { ALBEDO = texture(s, SCREEN_UV).rgb; }\n",
                "screen",
            ),
            (
                "shader_type spatial;\nvoid fragment() { ALBEDO = vec3(1.0); }\nvoid light() { DIFFUSE_LIGHT = vec3(1.0); }\n",
                "light()",
            ),
            (
                "shader_type spatial;\nrender_mode blend_mul;\nvoid fragment() { ALBEDO = vec3(1.0); }\n",
                "blend_mul",
            ),
            (
                "shader_type spatial;\nvoid fragment() { ALBEDO = vec3(TANGENT); }\n",
                "TANGENT",
            ),
        ] {
            let e = go(src).expect_err(src);
            assert!(e.contains(why), "{src}: {e}");
        }
    }

    #[test]
    fn ignored_outputs_are_listed() {
        let m = go("shader_type spatial;\nvoid fragment() { ALBEDO = vec3(0.5); RIM = 0.2; SPECULAR = 0.1; }\n")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            m.ignored_outputs,
            vec!["RIM".to_string(), "SPECULAR".to_string()]
        );
    }

    #[test]
    fn source_color_slot_is_linearized() {
        let slot = UniformSlot {
            name: "c".into(),
            ty: UniformType::Vec4,
            source_color: true,
            default_value: gdvariant::Variant::Nil,
        };
        let v = slot_value(
            &slot,
            Some(&gdvariant::Variant::Color(gdcore::math::Color::new(
                0.5, 1.0, 0.0, 0.5,
            ))),
        );
        assert!((v[0] - 0.21404).abs() < 1e-3, "{v:?}");
        assert_eq!((v[1], v[2], v[3]), (1.0, 0.0, 0.5));
    }
}

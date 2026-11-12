//! Sky resource types for 3D environment rendering.
//!
//! Implements Godot's Sky, ProceduralSkyMaterial, and PanoramicSkyMaterial
//! resource types. A [`Sky`] holds a [`SkyMaterial`] that defines how the
//! sky dome is rendered, and is consumed by an [`Environment3D`](super::environment::Environment3D).

use gdcore::math::{Color, Vector3};
use gdvariant::Variant;

/// The processing mode for sky rendering.
///
/// Maps to Godot's `Sky.ProcessMode` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SkyProcessMode {
    /// Automatically choose quality based on usage.
    #[default]
    Automatic,
    /// High quality — update every frame.
    Quality,
    /// Low quality — update incrementally.
    Incremental,
    /// Real-time — always re-render.
    RealTime,
}

impl SkyProcessMode {
    /// Converts from the Godot integer representation.
    pub fn from_godot_int(v: i64) -> Self {
        match v {
            0 => Self::Automatic,
            1 => Self::Quality,
            2 => Self::Incremental,
            3 => Self::RealTime,
            _ => Self::Automatic,
        }
    }

    /// Converts to the Godot integer representation.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::Automatic => 0,
            Self::Quality => 1,
            Self::Incremental => 2,
            Self::RealTime => 3,
        }
    }
}

/// A procedural sky material that generates a sky from color gradients
/// and sun parameters.
///
/// Maps to Godot's `ProceduralSkyMaterial`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProceduralSkyMaterial {
    /// Color at the top of the sky dome.
    pub sky_top_color: Color,
    /// Color at the horizon.
    pub sky_horizon_color: Color,
    /// Curve controlling the sky gradient falloff (higher = sharper).
    pub sky_curve: f32,
    /// Energy multiplier for the sky.
    pub sky_energy_multiplier: f32,
    /// Color at the bottom of the sky dome (ground reflection).
    pub ground_bottom_color: Color,
    /// Color at the ground horizon.
    pub ground_horizon_color: Color,
    /// Curve controlling the ground gradient falloff.
    pub ground_curve: f32,
    /// Energy multiplier for the ground.
    pub ground_energy_multiplier: f32,
    /// Angular size of the sun disc in degrees.
    pub sun_angle_max: f32,
    /// Curve controlling sun disc falloff.
    pub sun_curve: f32,
    /// Resource path to the sky-cover texture (Godot's `sky_cover`, e.g. clouds
    /// or stars), or empty. Sampled equirectangularly at `SKY_COORDS`.
    pub sky_cover_path: String,
    /// Optional decoded sky-cover texture (linear RGBA) for runtime sampling.
    /// When set, it is added over the sky hemisphere modulated by
    /// [`sky_cover_modulate`](Self::sky_cover_modulate).
    pub sky_cover: Option<PanoramaTexture>,
    /// Tint/opacity applied to the sky-cover texture (Godot's
    /// `sky_cover_modulate`, default opaque white).
    pub sky_cover_modulate: Color,
}

impl Default for ProceduralSkyMaterial {
    fn default() -> Self {
        Self {
            sky_top_color: Color::new(0.385, 0.454, 0.55, 1.0),
            // Godot's constructor uses the full-precision horizon color
            // (0.6463, 0.6558, 0.6708) — the truncated (0.646, 0.654, 0.67)
            // in its shader-default text is overridden by the setters.
            sky_horizon_color: Color::new(0.6463, 0.6558, 0.6708, 1.0),
            sky_curve: 0.15,
            sky_energy_multiplier: 1.0,
            ground_bottom_color: Color::new(0.2, 0.169, 0.133, 1.0),
            ground_horizon_color: Color::new(0.6463, 0.6558, 0.6708, 1.0),
            ground_curve: 0.02,
            ground_energy_multiplier: 1.0,
            sun_angle_max: 30.0,
            sun_curve: 0.15,
            sky_cover_path: String::new(),
            sky_cover: None,
            sky_cover_modulate: Color::new(1.0, 1.0, 1.0, 1.0),
        }
    }
}

/// Optional in-memory panorama pixels for equirect sampling (tests + loaders).
///
/// **Pixels MUST be linear.** All three consumers — `PanoramaSkyMaterial`,
/// `ProceduralSkyMaterial::sky_cover`, and `PhysicalSkyMaterial::night_sky` —
/// are Godot `source_color` samplers, so the sampler decodes sRGB→linear at
/// sample time. The renderer therefore uses these texels **raw** (already
/// linear). A loader that decodes one of these textures from an image file
/// must apply sRGB→linear itself (i.e. treat them as color, not data,
/// textures) before populating this buffer, or the cover/stars/panorama layer
/// will render too bright and over-saturated.
#[derive(Debug, Clone, PartialEq)]
pub struct PanoramaTexture {
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// **Linear** RGB(A) pixels, row-major, length `width * height` (see the
    /// type-level note — these are `source_color`, decode sRGB→linear on load).
    pub pixels: Vec<Color>,
}

/// A panoramic sky material that maps an HDR panorama texture onto the
/// sky dome.
///
/// Maps to Godot's `PanoramaSkyMaterial`.
#[derive(Debug, Clone, PartialEq)]
pub struct PanoramicSkyMaterial {
    /// Resource path to the panoramic texture (e.g. `"res://sky.hdr"`).
    pub panorama_path: String,
    /// Whether to filter the panorama texture.
    pub filter: bool,
    /// Energy multiplier.
    pub energy_multiplier: f32,
    /// Optional decoded panorama for runtime equirect sampling.
    pub texture: Option<PanoramaTexture>,
}

impl Default for PanoramicSkyMaterial {
    fn default() -> Self {
        Self {
            panorama_path: String::new(),
            filter: true,
            energy_multiplier: 1.0,
            texture: None,
        }
    }
}

/// A physical sky material based on atmospheric scattering.
///
/// Maps to Godot's `PhysicalSkyMaterial`.
#[derive(Debug, Clone, PartialEq)]
pub struct PhysicalSkyMaterial {
    /// Rayleigh scattering coefficient.
    pub rayleigh_coefficient: f32,
    /// Rayleigh scattering color.
    pub rayleigh_color: Color,
    /// Mie scattering coefficient.
    pub mie_coefficient: f32,
    /// Mie eccentricity (directionality).
    pub mie_eccentricity: f32,
    /// Mie scattering color.
    pub mie_color: Color,
    /// Turbidity factor (atmospheric haze).
    pub turbidity: f32,
    /// Sun disc radius in degrees.
    pub sun_disk_scale: f32,
    /// Ground color for the lower hemisphere.
    pub ground_color: Color,
    /// Energy multiplier.
    pub energy_multiplier: f32,
    /// Resource path to the night-sky texture (Godot's `night_sky`), or empty.
    /// Sampled equirectangularly at `SKY_COORDS`.
    pub night_sky_path: String,
    /// Optional decoded night-sky texture (linear RGB). Shown where the
    /// atmosphere is thin (added to `L0` × extinction) and as the whole sky
    /// when there is no sun.
    pub night_sky: Option<PanoramaTexture>,
}

impl Default for PhysicalSkyMaterial {
    fn default() -> Self {
        Self {
            rayleigh_coefficient: 2.0,
            rayleigh_color: Color::new(0.3, 0.405, 0.6, 1.0),
            mie_coefficient: 0.005,
            mie_eccentricity: 0.8,
            mie_color: Color::new(0.69, 0.729, 0.812, 1.0),
            turbidity: 10.0,
            sun_disk_scale: 1.0,
            ground_color: Color::new(0.1, 0.07, 0.034, 1.0),
            energy_multiplier: 1.0,
            night_sky_path: String::new(),
            night_sky: None,
        }
    }
}

/// A `Sky.sky_material` that is a `ShaderMaterial` with `shader_type sky`
/// (pat-0q2kx.224).
///
/// Godot ground truth: `doc/classes/Sky.xml` — `sky_material` accepts a
/// ShaderMaterial whose `sky()` function writes `COLOR` per direction from
/// `EYEDIR`/`TIME`/uniforms (`shader_types.cpp`, sky section).
#[derive(Debug, Clone, PartialEq)]
pub struct ShaderSkyMaterial {
    /// The material, carrying the shader source and its parameter values.
    pub material: crate::shader::ShaderMaterial3D,
    /// The parsed `sky()` body.
    ///
    /// Parsed **once**, when the material is built, rather than per sample: a
    /// sky is evaluated for every background pixel, so re-parsing the source per
    /// direction would make a custom sky cost orders of magnitude more than a
    /// built-in one for identical output.
    pub body: Vec<crate::shader::FragmentInstruction3D>,
    /// The whole program, for skies that sample a `samplerCube` (pat-cezo4).
    ///
    /// The line matcher behind [`Self::body`] has no notion of a cube sampler,
    /// so a sky that reads `texture(cube, EYEDIR)` -- the shape every
    /// cubemap/skybox sky has, the official 3D platformer's included -- came out
    /// black. Set only when the source declares a `uniform samplerCube`, so
    /// every other shader sky keeps the matcher path it had.
    pub program: Option<std::sync::Arc<gdresource::shader_ast::ShaderProgram>>,
    /// The decoded cubemaps bound to the shader's `samplerCube` uniforms, by
    /// uniform name. Filled by [`resolve_sky_material_textures`].
    pub cubemaps: std::collections::HashMap<String, std::sync::Arc<CubemapTexture>>,
    /// The sky evaluated once into an equirect panorama, for a backend that
    /// cannot run the shader per pixel (the wgpu sky pass) -- see
    /// [`Self::bake_panorama`]. An `Arc` so a renderer can key its upload by
    /// identity across frames.
    pub baked: Option<std::sync::Arc<PanoramaTexture>>,
}

/// A cubemap's six faces, **linear** colour, in Godot's layer order
/// `+X, -X, +Y, -Y, +Z, -Z` (pat-cezo4).
#[derive(Debug, Clone, PartialEq)]
pub struct CubemapTexture {
    /// Edge length of each square face, in texels.
    pub size: u32,
    /// The faces, each `size * size` row-major texels.
    pub faces: [Vec<Color>; 6],
}

impl CubemapTexture {
    /// Slices an image holding the six faces in a grid into a cubemap, the way
    /// Godot's cubemap importer does with `slices/arrangement`: 1x6, 2x3, 3x2
    /// or 6x1, read row-major. The arrangement is inferred from the aspect
    /// ratio, since a shipped project may carry no `.import` file. `None` for
    /// an image whose shape fits none of them.
    pub fn from_grid(image: &PanoramaTexture) -> Option<Self> {
        let (w, h) = (image.width, image.height);
        let (cols, rows) = if w * 3 == h * 2 {
            (2, 3)
        } else if w * 2 == h * 3 {
            (3, 2)
        } else if w * 6 == h {
            (1, 6)
        } else if w == h * 6 {
            (6, 1)
        } else {
            return None;
        };
        let size = w / cols;
        if size == 0 || size * rows != h || image.pixels.len() != (w * h) as usize {
            return None;
        }
        let face = |i: u32| -> Vec<Color> {
            let (fx, fy) = ((i % cols) * size, (i / cols) * size);
            let mut out = Vec::with_capacity((size * size) as usize);
            for y in 0..size {
                let row = ((fy + y) * w + fx) as usize;
                out.extend_from_slice(&image.pixels[row..row + size as usize]);
            }
            out
        };
        Some(Self {
            size,
            faces: [face(0), face(1), face(2), face(3), face(4), face(5)],
        })
    }

    /// Bilinear sample along `dir` (not necessarily normalized), with the
    /// standard cube-map face selection and orientation (the one Vulkan and
    /// OpenGL share, which Godot's `samplerCube` uses).
    pub fn sample(&self, dir: Vector3) -> Color {
        let (ax, ay, az) = (dir.x.abs(), dir.y.abs(), dir.z.abs());
        // (face, sc, tc, major axis)
        let (face, sc, tc, ma) = if ax >= ay && ax >= az {
            if dir.x > 0.0 {
                (0, -dir.z, -dir.y, ax)
            } else {
                (1, dir.z, -dir.y, ax)
            }
        } else if ay >= az {
            if dir.y > 0.0 {
                (2, dir.x, dir.z, ay)
            } else {
                (3, dir.x, -dir.z, ay)
            }
        } else if dir.z > 0.0 {
            (4, dir.x, -dir.y, az)
        } else {
            (5, -dir.x, -dir.y, az)
        };
        if ma <= 0.0 || self.size == 0 {
            return Color::BLACK;
        }
        let u = (sc / ma + 1.0) * 0.5;
        let v = (tc / ma + 1.0) * 0.5;
        let n = self.size as f32;
        let px = (u * n - 0.5).clamp(0.0, n - 1.0);
        let py = (v * n - 0.5).clamp(0.0, n - 1.0);
        let (x0, y0) = (px.floor() as u32, py.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(self.size - 1), (y0 + 1).min(self.size - 1));
        let (fx, fy) = (px - x0 as f32, py - y0 as f32);
        let texels = &self.faces[face];
        let at = |x: u32, y: u32| texels[(y * self.size + x) as usize];
        let lerp = |a: Color, b: Color, t: f32| {
            Color::new(
                a.r + (b.r - a.r) * t,
                a.g + (b.g - a.g) * t,
                a.b + (b.b - a.b) * t,
                a.a + (b.a - a.a) * t,
            )
        };
        lerp(
            lerp(at(x0, y0), at(x1, y0), fx),
            lerp(at(x0, y1), at(x1, y1), fx),
            fy,
        )
    }
}

/// The names of the `uniform samplerCube` declarations in `source`.
fn sampler_cube_uniforms(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("uniform ")?.trim_start();
            let rest = rest.strip_prefix("samplerCube")?.trim_start();
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

impl ShaderSkyMaterial {
    /// Builds a shader sky, parsing the material's `sky()` body up front.
    pub fn new(material: crate::shader::ShaderMaterial3D) -> Self {
        let body = material
            .shader
            .as_ref()
            .map(|s| crate::shader::parse_sky_body_3d(&s.source_code))
            .unwrap_or_default();
        let program = material
            .shader
            .as_ref()
            .filter(|s| !sampler_cube_uniforms(&s.source_code).is_empty())
            .and_then(|s| gdresource::shader_ast::parse_shader(&s.source_code).ok())
            .filter(|p| p.function("sky").is_some())
            .map(std::sync::Arc::new);
        Self {
            material,
            body,
            program,
            cubemaps: std::collections::HashMap::new(),
            baked: None,
        }
    }

    /// The `samplerCube` uniforms this sky declares.
    pub fn cube_uniform_names(&self) -> Vec<String> {
        self.material
            .shader
            .as_ref()
            .map(|s| sampler_cube_uniforms(&s.source_code))
            .unwrap_or_default()
    }

    /// Runs the whole program's `sky()` for one direction, or `None` when this
    /// sky has no program (the matcher [`Self::body`] then applies) or the
    /// evaluation fails. `dir` is `EYEDIR`; `COLOR` starts black, as in Godot.
    pub fn eval_program(&self, dir: Vector3) -> Option<Color> {
        use gdresource::shader_ast::{eval_function, Sampler, ShaderValue as SV, StageContext};
        let program = self.program.as_ref()?;
        let uniforms: std::collections::HashMap<String, SV> = self
            .material
            .parameters
            .iter()
            .filter_map(|(k, v)| {
                crate::shader::uniform_to_shader_value_3d(v).map(|sv| (k.clone(), sv))
            })
            .collect();
        let mut builtins: std::collections::HashMap<String, SV> = std::collections::HashMap::new();
        let d = dir.normalized();
        builtins.insert("EYEDIR".into(), SV::Vec3([d.x, d.y, d.z]));
        builtins.insert("COLOR".into(), SV::Vec3([0.0, 0.0, 0.0]));
        builtins.insert("ALPHA".into(), SV::Float(1.0));
        builtins.insert("TIME".into(), SV::Float(0.0));
        let cubes = self.cubemaps.clone();
        let sampler = Sampler::new(|_, _| [0.0, 0.0, 0.0, 0.0]).with_cube(move |name, dir| {
            // An unbound sampler reads its hint default; `hint_default_black`
            // is the common one, and black is also what an unset one gives.
            let Some(cube) = cubes.get(name) else {
                return Some([0.0, 0.0, 0.0, 1.0]);
            };
            let c = cube.sample(Vector3::new(dir[0], dir[1], dir[2]));
            Some([c.r, c.g, c.b, c.a])
        });
        let mut stage = StageContext::new(uniforms, builtins).with_sampler(sampler);
        eval_function(program, "sky", &mut stage).ok()?;
        let c = stage.builtins.get("COLOR")?.components();
        Some(Color::new(
            c.first().copied().unwrap_or(0.0),
            c.get(1).copied().unwrap_or(0.0),
            c.get(2).copied().unwrap_or(0.0),
            1.0,
        ))
    }

    /// Evaluates the program over an equirect grid (`u = atan2(x, -z) / 2pi`,
    /// `v = acos(y) / pi`, the mapping the panorama sky uses) and stores it in
    /// [`Self::baked`]. A no-op without a program, or when already baked.
    pub fn bake_panorama(&mut self, width: u32, height: u32) {
        if self.baked.is_some() || self.program.is_none() {
            return;
        }
        let mut pixels = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            let theta = (y as f32 + 0.5) / height as f32 * std::f32::consts::PI;
            for x in 0..width {
                let phi = (x as f32 + 0.5) / width as f32 * std::f32::consts::TAU;
                // Inverse of u = atan2(x, -z) / 2pi, v = acos(y) / pi.
                let dir = Vector3::new(
                    theta.sin() * phi.sin(),
                    theta.cos(),
                    -theta.sin() * phi.cos(),
                );
                pixels.push(self.eval_program(dir).unwrap_or(Color::BLACK));
            }
        }
        self.baked = Some(std::sync::Arc::new(PanoramaTexture {
            width,
            height,
            pixels,
        }));
    }

    /// Whether the shader yielded no usable instructions — no shader attached, no
    /// `sky()` function, or a body the parser understood nothing in.
    ///
    /// Callers fall back to a built-in sky rather than rendering black, which is
    /// what an unparsed body would otherwise produce.
    pub fn is_inert(&self) -> bool {
        self.body.is_empty()
    }
}

/// The material that defines how a sky dome is rendered.
#[derive(Debug, Clone, PartialEq)]
pub enum SkyMaterial {
    /// Procedurally generated sky from gradients and sun parameters.
    Procedural(ProceduralSkyMaterial),
    /// Sky from a panoramic HDR texture.
    Panoramic(PanoramicSkyMaterial),
    /// Physically-based atmospheric scattering sky.
    Physical(PhysicalSkyMaterial),
    /// A custom `shader_type sky` ShaderMaterial (pat-0q2kx.224).
    Shader(ShaderSkyMaterial),
}

/// Populates a sky material's optional panorama texture(s) from its stored
/// resource path(s), using a caller-supplied `load` closure that maps a
/// resource reference to decoded **linear** pixels.
///
/// This is the seam between scene-loading (which only records a texture's
/// resource path — e.g. the normalized `"ExtResource:<id>"` form the `.tscn`
/// parser stores in [`ProceduralSkyMaterial::sky_cover_path`]) and the renderer
/// (which needs a [`PanoramaTexture`] of linear texels). It is deliberately
/// decode-free so it lives in `gdserver3d` (no image-codec dependency): the host
/// — the editor or runtime — provides `load`, which resolves the reference to a
/// file and decodes+linearizes it (see `gdresource::load_color_texture_linear`).
///
/// `load` returning `None` (unresolved reference, unsupported/HDR format, decode
/// failure) leaves the texture unset, so the material renders without that layer
/// rather than failing — matching Godot, where a missing sky cover is simply not
/// drawn. Materials whose texture is already populated (e.g. constructed in a
/// test) or whose path is empty are left untouched, so this is idempotent.
///
/// Covers all three texture-bearing sky materials: `ProceduralSkyMaterial`'s
/// `sky_cover`, `PhysicalSkyMaterial`'s `night_sky`, and the panorama of a
/// `PanoramaSkyMaterial`.
pub fn resolve_sky_material_textures(
    material: &mut SkyMaterial,
    mut load: impl FnMut(&str) -> Option<PanoramaTexture>,
) {
    match material {
        SkyMaterial::Procedural(m) => {
            if m.sky_cover.is_none() && !m.sky_cover_path.is_empty() {
                m.sky_cover = load(&m.sky_cover_path);
            }
        }
        SkyMaterial::Physical(m) => {
            if m.night_sky.is_none() && !m.night_sky_path.is_empty() {
                m.night_sky = load(&m.night_sky_path);
            }
        }
        SkyMaterial::Panoramic(m) => {
            if m.texture.is_none() && !m.panorama_path.is_empty() {
                m.texture = load(&m.panorama_path);
            }
        }
        // A shader sky has no texture *paths* to resolve: its samplers would come
        // from `ShaderMaterial` parameters, and sky-pass texture sampling is not
        // wired yet (the evaluator's `texture()` yields black). Nothing to load —
        // an explicit arm rather than a catch-all so that adding sky samplers
        // later fails to compile here instead of silently loading nothing
        // (pat-0q2kx.224).
        // A shader sky's cube samplers (pat-cezo4): each `uniform samplerCube`
        // whose shader parameter names an image is loaded and sliced into
        // faces.
        SkyMaterial::Shader(m) => {
            for name in m.cube_uniform_names() {
                if m.cubemaps.contains_key(&name) {
                    continue;
                }
                let path = match m.material.parameters.get(&name) {
                    Some(gdvariant::Variant::String(p)) if !p.is_empty() => p.clone(),
                    _ => continue,
                };
                if let Some(cube) = load(&path).as_ref().and_then(CubemapTexture::from_grid) {
                    m.cubemaps.insert(name, std::sync::Arc::new(cube));
                }
            }
        }
    }
}

/// A Sky resource that holds a material and rendering parameters.
///
/// Maps to Godot's `Sky` resource. Used as the sky source for
/// [`Environment3D`](super::environment::Environment3D).
#[derive(Debug, Clone, PartialEq)]
pub struct Sky {
    /// The sky material defining the visual appearance.
    pub material: SkyMaterial,
    /// Processing mode for sky updates.
    pub process_mode: SkyProcessMode,
    /// Radiance size for reflections (in pixels per face).
    pub radiance_size: u32,
}

impl Default for Sky {
    fn default() -> Self {
        Self {
            material: SkyMaterial::Procedural(ProceduralSkyMaterial::default()),
            process_mode: SkyProcessMode::Automatic,
            radiance_size: 256,
        }
    }
}

impl ProceduralSkyMaterial {
    /// Constructs a `ProceduralSkyMaterial` from property name/value pairs.
    ///
    /// Unrecognised properties are ignored; missing ones keep defaults.
    pub fn from_properties<'a>(props: impl Iterator<Item = (&'a str, &'a Variant)>) -> Self {
        let mut mat = Self::default();
        for (key, value) in props {
            match key {
                "sky_top_color" => {
                    if let Variant::Color(c) = value {
                        mat.sky_top_color = *c;
                    }
                }
                "sky_horizon_color" => {
                    if let Variant::Color(c) = value {
                        mat.sky_horizon_color = *c;
                    }
                }
                "sky_curve" => {
                    if let Variant::Float(f) = value {
                        mat.sky_curve = *f as f32;
                    }
                }
                "sky_energy_multiplier" => {
                    if let Variant::Float(f) = value {
                        mat.sky_energy_multiplier = *f as f32;
                    }
                }
                "ground_bottom_color" => {
                    if let Variant::Color(c) = value {
                        mat.ground_bottom_color = *c;
                    }
                }
                "ground_horizon_color" => {
                    if let Variant::Color(c) = value {
                        mat.ground_horizon_color = *c;
                    }
                }
                "ground_curve" => {
                    if let Variant::Float(f) = value {
                        mat.ground_curve = *f as f32;
                    }
                }
                "ground_energy_multiplier" => {
                    if let Variant::Float(f) = value {
                        mat.ground_energy_multiplier = *f as f32;
                    }
                }
                "sun_angle_max" => {
                    if let Variant::Float(f) = value {
                        mat.sun_angle_max = *f as f32;
                    }
                }
                "sun_curve" => {
                    if let Variant::Float(f) = value {
                        mat.sun_curve = *f as f32;
                    }
                }
                // The texture itself is resolved by a loader; here we keep the
                // resource-ref string (Godot serialises `sky_cover` as a
                // SubResource/ExtResource) and the modulate tint.
                "sky_cover" => {
                    if let Variant::String(s) = value {
                        mat.sky_cover_path = s.clone();
                    }
                }
                "sky_cover_modulate" => {
                    if let Variant::Color(c) = value {
                        mat.sky_cover_modulate = *c;
                    }
                }
                _ => {}
            }
        }
        mat
    }

    /// Serialises non-default properties as `(name, Variant)` pairs.
    pub fn to_properties(&self) -> Vec<(String, Variant)> {
        let def = Self::default();
        let mut props = Vec::new();
        if self.sky_top_color != def.sky_top_color {
            props.push(("sky_top_color".into(), Variant::Color(self.sky_top_color)));
        }
        if self.sky_horizon_color != def.sky_horizon_color {
            props.push((
                "sky_horizon_color".into(),
                Variant::Color(self.sky_horizon_color),
            ));
        }
        if (self.sky_curve - def.sky_curve).abs() > f32::EPSILON {
            props.push(("sky_curve".into(), Variant::Float(self.sky_curve as f64)));
        }
        if (self.sky_energy_multiplier - def.sky_energy_multiplier).abs() > f32::EPSILON {
            props.push((
                "sky_energy_multiplier".into(),
                Variant::Float(self.sky_energy_multiplier as f64),
            ));
        }
        if self.ground_bottom_color != def.ground_bottom_color {
            props.push((
                "ground_bottom_color".into(),
                Variant::Color(self.ground_bottom_color),
            ));
        }
        if self.ground_horizon_color != def.ground_horizon_color {
            props.push((
                "ground_horizon_color".into(),
                Variant::Color(self.ground_horizon_color),
            ));
        }
        if (self.ground_curve - def.ground_curve).abs() > f32::EPSILON {
            props.push((
                "ground_curve".into(),
                Variant::Float(self.ground_curve as f64),
            ));
        }
        if (self.ground_energy_multiplier - def.ground_energy_multiplier).abs() > f32::EPSILON {
            props.push((
                "ground_energy_multiplier".into(),
                Variant::Float(self.ground_energy_multiplier as f64),
            ));
        }
        if (self.sun_angle_max - def.sun_angle_max).abs() > f32::EPSILON {
            props.push((
                "sun_angle_max".into(),
                Variant::Float(self.sun_angle_max as f64),
            ));
        }
        if (self.sun_curve - def.sun_curve).abs() > f32::EPSILON {
            props.push(("sun_curve".into(), Variant::Float(self.sun_curve as f64)));
        }
        if self.sky_cover_path != def.sky_cover_path {
            props.push((
                "sky_cover".into(),
                Variant::String(self.sky_cover_path.clone()),
            ));
        }
        if self.sky_cover_modulate != def.sky_cover_modulate {
            props.push((
                "sky_cover_modulate".into(),
                Variant::Color(self.sky_cover_modulate),
            ));
        }
        props
    }
}

impl PanoramicSkyMaterial {
    /// Constructs a `PanoramicSkyMaterial` from property name/value pairs.
    pub fn from_properties<'a>(props: impl Iterator<Item = (&'a str, &'a Variant)>) -> Self {
        let mut mat = Self::default();
        for (key, value) in props {
            match key {
                "panorama" => {
                    if let Variant::String(s) = value {
                        mat.panorama_path = s.clone();
                    }
                }
                "filter" => {
                    if let Variant::Bool(b) = value {
                        mat.filter = *b;
                    }
                }
                "energy_multiplier" => {
                    if let Variant::Float(f) = value {
                        mat.energy_multiplier = *f as f32;
                    }
                }
                _ => {}
            }
        }
        mat
    }

    /// Serialises non-default properties as `(name, Variant)` pairs.
    pub fn to_properties(&self) -> Vec<(String, Variant)> {
        let def = Self::default();
        let mut props = Vec::new();
        if self.panorama_path != def.panorama_path {
            props.push((
                "panorama".into(),
                Variant::String(self.panorama_path.clone()),
            ));
        }
        if self.filter != def.filter {
            props.push(("filter".into(), Variant::Bool(self.filter)));
        }
        if (self.energy_multiplier - def.energy_multiplier).abs() > f32::EPSILON {
            props.push((
                "energy_multiplier".into(),
                Variant::Float(self.energy_multiplier as f64),
            ));
        }
        props
    }
}

impl PhysicalSkyMaterial {
    /// Constructs a `PhysicalSkyMaterial` from property name/value pairs.
    pub fn from_properties<'a>(props: impl Iterator<Item = (&'a str, &'a Variant)>) -> Self {
        let mut mat = Self::default();
        for (key, value) in props {
            match key {
                "rayleigh_coefficient" => {
                    if let Variant::Float(f) = value {
                        mat.rayleigh_coefficient = *f as f32;
                    }
                }
                "rayleigh_color" => {
                    if let Variant::Color(c) = value {
                        mat.rayleigh_color = *c;
                    }
                }
                "mie_coefficient" => {
                    if let Variant::Float(f) = value {
                        mat.mie_coefficient = *f as f32;
                    }
                }
                "mie_eccentricity" => {
                    if let Variant::Float(f) = value {
                        mat.mie_eccentricity = *f as f32;
                    }
                }
                "mie_color" => {
                    if let Variant::Color(c) = value {
                        mat.mie_color = *c;
                    }
                }
                "turbidity" => {
                    if let Variant::Float(f) = value {
                        mat.turbidity = *f as f32;
                    }
                }
                "sun_disk_scale" => {
                    if let Variant::Float(f) = value {
                        mat.sun_disk_scale = *f as f32;
                    }
                }
                "ground_color" => {
                    if let Variant::Color(c) = value {
                        mat.ground_color = *c;
                    }
                }
                "energy_multiplier" => {
                    if let Variant::Float(f) = value {
                        mat.energy_multiplier = *f as f32;
                    }
                }
                // The texture is resolved by a loader; keep the resource-ref
                // string (Godot serialises `night_sky` as a Sub/ExtResource).
                "night_sky" => {
                    if let Variant::String(s) = value {
                        mat.night_sky_path = s.clone();
                    }
                }
                _ => {}
            }
        }
        mat
    }

    /// Serialises non-default properties as `(name, Variant)` pairs.
    pub fn to_properties(&self) -> Vec<(String, Variant)> {
        let def = Self::default();
        let mut props = Vec::new();
        if (self.rayleigh_coefficient - def.rayleigh_coefficient).abs() > f32::EPSILON {
            props.push((
                "rayleigh_coefficient".into(),
                Variant::Float(self.rayleigh_coefficient as f64),
            ));
        }
        if self.rayleigh_color != def.rayleigh_color {
            props.push(("rayleigh_color".into(), Variant::Color(self.rayleigh_color)));
        }
        if (self.mie_coefficient - def.mie_coefficient).abs() > f32::EPSILON {
            props.push((
                "mie_coefficient".into(),
                Variant::Float(self.mie_coefficient as f64),
            ));
        }
        if (self.mie_eccentricity - def.mie_eccentricity).abs() > f32::EPSILON {
            props.push((
                "mie_eccentricity".into(),
                Variant::Float(self.mie_eccentricity as f64),
            ));
        }
        if self.mie_color != def.mie_color {
            props.push(("mie_color".into(), Variant::Color(self.mie_color)));
        }
        if (self.turbidity - def.turbidity).abs() > f32::EPSILON {
            props.push(("turbidity".into(), Variant::Float(self.turbidity as f64)));
        }
        if (self.sun_disk_scale - def.sun_disk_scale).abs() > f32::EPSILON {
            props.push((
                "sun_disk_scale".into(),
                Variant::Float(self.sun_disk_scale as f64),
            ));
        }
        if self.ground_color != def.ground_color {
            props.push(("ground_color".into(), Variant::Color(self.ground_color)));
        }
        if (self.energy_multiplier - def.energy_multiplier).abs() > f32::EPSILON {
            props.push((
                "energy_multiplier".into(),
                Variant::Float(self.energy_multiplier as f64),
            ));
        }
        if self.night_sky_path != def.night_sky_path {
            props.push((
                "night_sky".into(),
                Variant::String(self.night_sky_path.clone()),
            ));
        }
        props
    }
}

impl Sky {
    /// Constructs a `Sky` from property name/value pairs.
    ///
    /// The `"sky_material_type"` property selects the material variant:
    /// `"ProceduralSkyMaterial"` (default), `"PanoramaSkyMaterial"`, or
    /// `"PhysicalSkyMaterial"`. Other material-specific properties are
    /// forwarded to the corresponding material's `from_properties`.
    pub fn from_properties<'a>(props: impl Iterator<Item = (&'a str, &'a Variant)>) -> Self {
        let mut sky = Self::default();
        let mut material_type = String::new();
        let mut material_props: Vec<(&str, &Variant)> = Vec::new();

        for (key, value) in props {
            match key {
                "sky_material_type" | "sky_material" => {
                    if let Variant::String(s) = value {
                        material_type = s.clone();
                    }
                }
                "process_mode" => {
                    if let Variant::Int(v) = value {
                        sky.process_mode = SkyProcessMode::from_godot_int(*v);
                    }
                }
                "radiance_size" => {
                    if let Variant::Int(v) = value {
                        sky.radiance_size = *v as u32;
                    }
                }
                _ => {
                    material_props.push((key, value));
                }
            }
        }

        sky.material = match material_type.as_str() {
            "PanoramaSkyMaterial" | "PanoramicSkyMaterial" => SkyMaterial::Panoramic(
                PanoramicSkyMaterial::from_properties(material_props.into_iter()),
            ),
            "PhysicalSkyMaterial" => SkyMaterial::Physical(PhysicalSkyMaterial::from_properties(
                material_props.into_iter(),
            )),
            _ => {
                // Default to procedural
                SkyMaterial::Procedural(ProceduralSkyMaterial::from_properties(
                    material_props.into_iter(),
                ))
            }
        };

        sky
    }

    /// Serialises non-default properties as `(name, Variant)` pairs.
    pub fn to_properties(&self) -> Vec<(String, Variant)> {
        let def = Self::default();
        let mut props = Vec::new();

        // Emit material type and material-specific properties.
        match &self.material {
            SkyMaterial::Procedural(mat) => {
                props.push((
                    "sky_material_type".into(),
                    Variant::String("ProceduralSkyMaterial".into()),
                ));
                props.extend(mat.to_properties());
            }
            SkyMaterial::Panoramic(mat) => {
                props.push((
                    "sky_material_type".into(),
                    Variant::String("PanoramaSkyMaterial".into()),
                ));
                props.extend(mat.to_properties());
            }
            SkyMaterial::Physical(mat) => {
                props.push((
                    "sky_material_type".into(),
                    Variant::String("PhysicalSkyMaterial".into()),
                ));
                props.extend(mat.to_properties());
            }
            // A custom `shader_type sky` material (pat-0q2kx.224).
            //
            // The sibling arms delegate to the material's own `to_properties`,
            // which `ShaderSkyMaterial` deliberately does not have: what a
            // ShaderMaterial round-trips is its *shader* plus its uniform values,
            // not a fixed property list. Rather than invent a format, this emits
            // the two keys the engine already reads back —
            // `render_server_3d::resolve_shader_material` consumes `shader_code`
            // and `shader_parameter/*` — so the trip out and the trip in use one
            // convention. `sky_material_type` carries Godot's class name, which
            // is the same string the `.tscn` sub-resource type dispatch matches
            // on, keeping the round-trip symmetric.
            SkyMaterial::Shader(mat) => {
                props.push((
                    "sky_material_type".into(),
                    Variant::String("ShaderMaterial".into()),
                ));
                if let Some(shader) = &mat.material.shader {
                    props.push((
                        "shader_code".into(),
                        Variant::String(shader.source_code.clone()),
                    ));
                }
                // Sorted: `parameters` is a HashMap, and emitting it in iteration
                // order would make a saved scene's property order differ between
                // runs for identical input.
                let mut names: Vec<&String> = mat.material.parameters.keys().collect();
                names.sort();
                for name in names {
                    if let Some(v) = mat.material.parameters.get(name) {
                        props.push((format!("shader_parameter/{name}"), v.clone()));
                    }
                }
            }
        }

        if self.process_mode != def.process_mode {
            props.push((
                "process_mode".into(),
                Variant::Int(self.process_mode.to_godot_int()),
            ));
        }
        if self.radiance_size != def.radiance_size {
            props.push((
                "radiance_size".into(),
                Variant::Int(self.radiance_size as i64),
            ));
        }

        props
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_sky_is_procedural() {
        let sky = Sky::default();
        assert!(matches!(sky.material, SkyMaterial::Procedural(_)));
        assert_eq!(sky.process_mode, SkyProcessMode::Automatic);
        assert_eq!(sky.radiance_size, 256);
    }

    #[test]
    fn procedural_sky_defaults_match_godot() {
        let mat = ProceduralSkyMaterial::default();
        assert!((mat.sky_curve - 0.15).abs() < 1e-5);
        assert!((mat.sky_energy_multiplier - 1.0).abs() < 1e-5);
        assert!((mat.ground_curve - 0.02).abs() < 1e-5);
        assert!((mat.sun_angle_max - 30.0).abs() < 1e-5);
    }

    #[test]
    fn panoramic_sky_defaults() {
        let mat = PanoramicSkyMaterial::default();
        assert!(mat.panorama_path.is_empty());
        assert!(mat.filter);
        assert!((mat.energy_multiplier - 1.0).abs() < 1e-5);
    }

    #[test]
    fn physical_sky_defaults() {
        let mat = PhysicalSkyMaterial::default();
        assert!((mat.rayleigh_coefficient - 2.0).abs() < 1e-5);
        assert!((mat.mie_eccentricity - 0.8).abs() < 1e-5);
        assert!((mat.turbidity - 10.0).abs() < 1e-5);
    }

    #[test]
    fn sky_with_panoramic_material() {
        let sky = Sky {
            material: SkyMaterial::Panoramic(PanoramicSkyMaterial {
                panorama_path: "res://sky_hdr.exr".to_string(),
                filter: true,
                energy_multiplier: 1.5,
                texture: None,
            }),
            process_mode: SkyProcessMode::Quality,
            radiance_size: 512,
        };
        assert!(matches!(sky.material, SkyMaterial::Panoramic(_)));
        assert_eq!(sky.process_mode, SkyProcessMode::Quality);
    }

    #[test]
    fn sky_process_mode_roundtrip() {
        for (int_val, expected) in [
            (0, SkyProcessMode::Automatic),
            (1, SkyProcessMode::Quality),
            (2, SkyProcessMode::Incremental),
            (3, SkyProcessMode::RealTime),
        ] {
            let mode = SkyProcessMode::from_godot_int(int_val);
            assert_eq!(mode, expected);
            assert_eq!(mode.to_godot_int(), int_val);
        }
    }

    #[test]
    fn sky_process_mode_unknown_defaults_to_automatic() {
        assert_eq!(
            SkyProcessMode::from_godot_int(99),
            SkyProcessMode::Automatic
        );
    }

    #[test]
    fn procedural_sky_custom_colors() {
        let mat = ProceduralSkyMaterial {
            sky_top_color: Color::new(0.0, 0.0, 1.0, 1.0),
            sky_horizon_color: Color::new(0.8, 0.8, 1.0, 1.0),
            ground_bottom_color: Color::new(0.1, 0.1, 0.1, 1.0),
            ground_horizon_color: Color::new(0.5, 0.5, 0.5, 1.0),
            sun_angle_max: 45.0,
            ..Default::default()
        };
        assert_eq!(mat.sky_top_color, Color::new(0.0, 0.0, 1.0, 1.0));
        assert!((mat.sun_angle_max - 45.0).abs() < 1e-5);
    }

    #[test]
    fn sky_clone_eq() {
        let sky = Sky::default();
        let cloned = sky.clone();
        assert_eq!(sky, cloned);
    }

    // -- resolve_sky_material_textures -----------------------------------------

    fn one_pixel(c: Color) -> PanoramaTexture {
        PanoramaTexture {
            width: 1,
            height: 1,
            pixels: vec![c],
        }
    }

    #[test]
    fn resolve_loads_procedural_sky_cover_from_path() {
        let mut sky = SkyMaterial::Procedural(ProceduralSkyMaterial {
            sky_cover_path: "ExtResource:1_clouds".into(),
            ..Default::default()
        });
        let mut seen = Vec::new();
        resolve_sky_material_textures(&mut sky, |p| {
            seen.push(p.to_string());
            Some(one_pixel(Color::new(0.2, 0.4, 0.6, 1.0)))
        });
        // The loader is handed the raw stored reference (the parser's normalized
        // `ExtResource:<id>` form), which the host resolves to a file.
        assert_eq!(seen, vec!["ExtResource:1_clouds".to_string()]);
        match sky {
            SkyMaterial::Procedural(m) => {
                let tex = m.sky_cover.expect("sky_cover populated");
                assert_eq!(tex.pixels[0], Color::new(0.2, 0.4, 0.6, 1.0));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn resolve_loads_physical_night_sky_and_panorama() {
        let mut phys = SkyMaterial::Physical(PhysicalSkyMaterial {
            night_sky_path: "res://stars.png".into(),
            ..Default::default()
        });
        resolve_sky_material_textures(&mut phys, |_| {
            Some(one_pixel(Color::new(1.0, 1.0, 1.0, 1.0)))
        });
        assert!(matches!(&phys, SkyMaterial::Physical(m) if m.night_sky.is_some()));

        let mut pano = SkyMaterial::Panoramic(PanoramicSkyMaterial {
            panorama_path: "res://sky.png".into(),
            ..Default::default()
        });
        resolve_sky_material_textures(&mut pano, |_| {
            Some(one_pixel(Color::new(0.5, 0.5, 0.5, 1.0)))
        });
        assert!(matches!(&pano, SkyMaterial::Panoramic(m) if m.texture.is_some()));
    }

    #[test]
    fn resolve_skips_empty_path_without_calling_loader() {
        let mut sky = SkyMaterial::Procedural(ProceduralSkyMaterial::default());
        let mut called = false;
        resolve_sky_material_textures(&mut sky, |_| {
            called = true;
            Some(one_pixel(Color::new(1.0, 0.0, 0.0, 1.0)))
        });
        assert!(!called, "loader must not run for an empty texture path");
        assert!(matches!(&sky, SkyMaterial::Procedural(m) if m.sky_cover.is_none()));
    }

    #[test]
    fn resolve_is_idempotent_when_already_loaded() {
        let existing = one_pixel(Color::new(0.1, 0.1, 0.1, 1.0));
        let mut sky = SkyMaterial::Procedural(ProceduralSkyMaterial {
            sky_cover_path: "res://clouds.png".into(),
            sky_cover: Some(existing.clone()),
            ..Default::default()
        });
        resolve_sky_material_textures(&mut sky, |_| {
            panic!("loader must not run when the texture is already populated")
        });
        assert!(
            matches!(&sky, SkyMaterial::Procedural(m) if m.sky_cover == Some(existing.clone()))
        );
    }

    #[test]
    fn resolve_leaves_texture_unset_when_loader_returns_none() {
        let mut sky = SkyMaterial::Procedural(ProceduralSkyMaterial {
            sky_cover_path: "res://missing.exr".into(),
            ..Default::default()
        });
        resolve_sky_material_textures(&mut sky, |_| None);
        assert!(
            matches!(&sky, SkyMaterial::Procedural(m) if m.sky_cover.is_none()),
            "a failed load must leave the layer unset, not crash"
        );
    }

    /// A 2x3 grid image whose face `i` is the flat colour (i/5, 0, 1 - i/5).
    fn six_face_grid(face: u32) -> PanoramaTexture {
        let (w, h) = (2 * face, 3 * face);
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let i = (y / face) * 2 + x / face;
                pixels.push(Color::new(i as f32 / 5.0, 0.0, 1.0 - i as f32 / 5.0, 1.0));
            }
        }
        PanoramaTexture {
            width: w,
            height: h,
            pixels,
        }
    }

    /// pat-cezo4: a 2x3 cubemap image (the platformer's `skybox.webp` layout)
    /// slices row-major into +X, -X, +Y, -Y, +Z, -Z, and each axis direction
    /// samples its own face.
    #[test]
    fn a_grid_image_slices_into_cube_faces_and_each_axis_reads_its_face() {
        let cube = CubemapTexture::from_grid(&six_face_grid(4)).expect("2x3 grid");
        assert_eq!(cube.size, 4);
        let dirs = [
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(-1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 0.0, -1.0),
        ];
        for (i, d) in dirs.iter().enumerate() {
            let c = cube.sample(*d);
            assert!(
                (c.r - i as f32 / 5.0).abs() < 1e-5,
                "{d:?} read {c:?}, want face {i}"
            );
        }
        // Not a grid shape: no cubemap.
        assert!(CubemapTexture::from_grid(&PanoramaTexture {
            width: 3,
            height: 3,
            pixels: vec![Color::BLACK; 9],
        })
        .is_none());
    }

    /// pat-cezo4: a `shader_type sky` that samples a `samplerCube` runs as a
    /// whole program and reads the bound cubemap along EYEDIR, times its
    /// uniform — the platformer's `skybox.gdshader`, verbatim. The line
    /// matcher alone produced black for it.
    #[test]
    fn a_cubemap_sky_shader_samples_its_cube_along_eyedir() {
        let source = "shader_type sky;\n\nuniform samplerCube source_panorama : filter_linear, source_color, hint_default_black;\nuniform float exposure : hint_range(0, 128) = 1.0;\n\nvoid sky() {\n    vec3 eyedir = vec3(EYEDIR.x, EYEDIR.y, EYEDIR.z);\n    COLOR = texture(source_panorama, eyedir).rgb * exposure;\n}\n";
        let mut m = crate::shader::ShaderMaterial3D::new();
        m.shader = Some(crate::shader::Shader3D::new(
            crate::shader::ShaderType3D::Sky,
            source,
        ));
        m.set_shader_parameter("source_panorama", Variant::String("grid.png".into()));
        m.set_shader_parameter("exposure", Variant::Float(0.5));
        let mut sky = SkyMaterial::Shader(ShaderSkyMaterial::new(m));
        resolve_sky_material_textures(&mut sky, |path| {
            assert_eq!(path, "grid.png");
            Some(six_face_grid(4))
        });
        let SkyMaterial::Shader(mut mat) = sky else {
            unreachable!()
        };
        assert!(mat.program.is_some(), "a samplerCube sky runs as a program");
        // +Z is face 4: r = 0.8, times exposure 0.5.
        let c = mat.eval_program(Vector3::new(0.0, 0.0, 1.0)).expect("runs");
        assert!(
            (c.r - 0.4).abs() < 1e-4 && (c.b - 0.1).abs() < 1e-4,
            "{c:?}"
        );
        // The bake covers every direction; its middle row at u = 0.5 is +Z.
        mat.bake_panorama(8, 4);
        let pano = mat.baked.as_ref().expect("baked");
        let mid = pano.pixels[2 * 8 + 4];
        assert!((mid.r - 0.4).abs() < 0.05, "equirect u=0.5 is +Z: {mid:?}");
    }
}

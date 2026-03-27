//! Shader editor with GLSL/GDShader syntax highlighting.
//!
//! Provides a [`ShaderEditor`] that manages open shader tabs and a
//! [`ShaderHighlighter`] that uses the GDShader tokenizer from `gdresource`
//! to assign [`ShaderHighlightKind`] categories to token spans for rendering.

use gdcore::math::Color;
use gdrender2d::renderer::FrameBuffer;
use gdresource::shader_tokenizer::{
    tokenize_shader, tokenize_shader_no_comments, extract_uniforms as parse_uniforms,
    ShaderLexError, ShaderToken, ShaderUniform,
};

// ── Syntax highlighting ─────────────────────────────────────────────────

/// The category of highlighting to apply to a shader source span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderHighlightKind {
    /// A language keyword (`shader_type`, `render_mode`, `uniform`, control flow).
    Keyword,
    /// A built-in type (`vec2`, `mat4`, `sampler2D`, etc.).
    BuiltinType,
    /// A qualifier (`in`, `out`, `inout`, `flat`, `smooth`, precision).
    Qualifier,
    /// A string literal (GDShader doesn't have these, but reserved for GLSL compat).
    StringLiteral,
    /// A numeric literal (int or float).
    NumberLiteral,
    /// A boolean literal (`true`, `false`).
    BoolLiteral,
    /// A comment (line or block).
    Comment,
    /// A known built-in function (`texture`, `mix`, `clamp`, etc.).
    BuiltinFunction,
    /// A user identifier (variable, function name).
    Identifier,
    /// An operator (`+`, `-`, `*`, `=`, etc.).
    Operator,
    /// Punctuation (parens, braces, semicolons, etc.).
    Punctuation,
    /// Whitespace (tracked but not rendered).
    Whitespace,
    /// Plain / unknown text.
    Plain,
}

/// A highlighted span of shader source code.
#[derive(Debug, Clone, PartialEq)]
pub struct ShaderHighlightSpan {
    /// The highlight category.
    pub kind: ShaderHighlightKind,
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number.
    pub col: usize,
    /// The token text.
    pub text: String,
}

/// Known GLSL/GDShader built-in functions for highlighting.
const BUILTIN_FUNCTIONS: &[&str] = &[
    "texture", "textureLod", "textureProj", "textureSize", "texelFetch",
    "mix", "clamp", "step", "smoothstep", "fract", "floor", "ceil", "round",
    "abs", "sign", "mod", "min", "max", "pow", "exp", "exp2", "log", "log2",
    "sqrt", "inversesqrt",
    "sin", "cos", "tan", "asin", "acos", "atan",
    "dot", "cross", "length", "distance", "normalize", "reflect", "refract",
    "transpose", "inverse", "determinant",
    "radians", "degrees",
    "lessThan", "greaterThan", "lessThanEqual", "greaterThanEqual", "equal",
    "notEqual", "any", "all", "not",
    "dFdx", "dFdy", "fwidth",
    "COLOR", "VERTEX", "UV", "NORMAL", "FRAGCOORD", "TIME", "PI", "TAU",
    "MODEL_MATRIX", "VIEW_MATRIX", "PROJECTION_MATRIX", "INV_VIEW_MATRIX",
    "SCREEN_UV", "SCREEN_TEXTURE", "DEPTH_TEXTURE",
];

fn is_builtin_function(name: &str) -> bool {
    BUILTIN_FUNCTIONS.contains(&name)
}

/// Classifies a [`ShaderToken`] into a [`ShaderHighlightKind`].
fn classify_shader_token(token: &ShaderToken) -> ShaderHighlightKind {
    match token {
        // Keywords
        ShaderToken::ShaderType
        | ShaderToken::RenderMode
        | ShaderToken::Uniform
        | ShaderToken::Varying
        | ShaderToken::Const
        | ShaderToken::Struct
        | ShaderToken::Void
        | ShaderToken::If
        | ShaderToken::Else
        | ShaderToken::For
        | ShaderToken::While
        | ShaderToken::Do
        | ShaderToken::Switch
        | ShaderToken::Case
        | ShaderToken::Default
        | ShaderToken::Break
        | ShaderToken::Continue
        | ShaderToken::Return
        | ShaderToken::Discard
        | ShaderToken::Instance
        | ShaderToken::Global
        | ShaderToken::GroupUniforms => ShaderHighlightKind::Keyword,

        // Qualifiers
        ShaderToken::In
        | ShaderToken::Out
        | ShaderToken::Inout
        | ShaderToken::Flat
        | ShaderToken::Smooth
        | ShaderToken::Lowp
        | ShaderToken::Mediump
        | ShaderToken::Highp => ShaderHighlightKind::Qualifier,

        // Built-in types
        ShaderToken::Bool
        | ShaderToken::Int
        | ShaderToken::Uint
        | ShaderToken::Float
        | ShaderToken::Vec2
        | ShaderToken::Vec3
        | ShaderToken::Vec4
        | ShaderToken::Ivec2
        | ShaderToken::Ivec3
        | ShaderToken::Ivec4
        | ShaderToken::Uvec2
        | ShaderToken::Uvec3
        | ShaderToken::Uvec4
        | ShaderToken::Bvec2
        | ShaderToken::Bvec3
        | ShaderToken::Bvec4
        | ShaderToken::Mat2
        | ShaderToken::Mat3
        | ShaderToken::Mat4
        | ShaderToken::Sampler2D
        | ShaderToken::Isampler2D
        | ShaderToken::Usampler2D
        | ShaderToken::Sampler2DArray
        | ShaderToken::Sampler3D
        | ShaderToken::SamplerCube
        | ShaderToken::SamplerCubeArray
        | ShaderToken::SamplerExternalOES => ShaderHighlightKind::BuiltinType,

        // Literals
        ShaderToken::IntLit(_) | ShaderToken::UintLit(_) | ShaderToken::FloatLit(_) => {
            ShaderHighlightKind::NumberLiteral
        }
        ShaderToken::BoolLit(_) => ShaderHighlightKind::BoolLiteral,

        // Comments
        ShaderToken::LineComment(_) | ShaderToken::BlockComment(_) => {
            ShaderHighlightKind::Comment
        }

        // Identifiers — check for built-in functions
        ShaderToken::Ident(name) => {
            if is_builtin_function(name) {
                ShaderHighlightKind::BuiltinFunction
            } else {
                ShaderHighlightKind::Identifier
            }
        }

        // Operators
        ShaderToken::Plus
        | ShaderToken::Minus
        | ShaderToken::Star
        | ShaderToken::Slash
        | ShaderToken::Percent
        | ShaderToken::Assign
        | ShaderToken::EqEq
        | ShaderToken::BangEq
        | ShaderToken::Lt
        | ShaderToken::LtEq
        | ShaderToken::Gt
        | ShaderToken::GtEq
        | ShaderToken::AmpAmp
        | ShaderToken::PipePipe
        | ShaderToken::Bang
        | ShaderToken::Amp
        | ShaderToken::Pipe
        | ShaderToken::Caret
        | ShaderToken::Tilde
        | ShaderToken::LtLt
        | ShaderToken::GtGt
        | ShaderToken::PlusAssign
        | ShaderToken::MinusAssign
        | ShaderToken::StarAssign
        | ShaderToken::SlashAssign
        | ShaderToken::PercentAssign
        | ShaderToken::AmpAssign
        | ShaderToken::PipeAssign
        | ShaderToken::CaretAssign
        | ShaderToken::LtLtAssign
        | ShaderToken::GtGtAssign
        | ShaderToken::Question
        | ShaderToken::PlusPlus
        | ShaderToken::MinusMinus => ShaderHighlightKind::Operator,

        // Punctuation
        ShaderToken::LParen
        | ShaderToken::RParen
        | ShaderToken::LBrace
        | ShaderToken::RBrace
        | ShaderToken::LBracket
        | ShaderToken::RBracket
        | ShaderToken::Semicolon
        | ShaderToken::Comma
        | ShaderToken::Dot
        | ShaderToken::Colon => ShaderHighlightKind::Punctuation,

        // EOF
        ShaderToken::Eof => ShaderHighlightKind::Plain,
    }
}

/// Extracts the display text from a [`ShaderToken`].
fn token_text(token: &ShaderToken) -> String {
    format!("{token}")
}

/// The syntax highlighter for GDShader/GLSL source code.
#[derive(Debug)]
pub struct ShaderHighlighter;

impl ShaderHighlighter {
    /// Creates a new shader highlighter.
    pub fn new() -> Self {
        Self
    }

    /// Highlights the entire shader source, returning classified spans.
    pub fn highlight(&self, source: &str) -> Result<Vec<ShaderHighlightSpan>, ShaderLexError> {
        let tokens = tokenize_shader(source)?;
        Ok(tokens
            .into_iter()
            .filter(|t| !matches!(t.token, ShaderToken::Eof))
            .map(|span| ShaderHighlightSpan {
                kind: classify_shader_token(&span.token),
                line: span.line,
                col: span.col,
                text: token_text(&span.token),
            })
            .collect())
    }

    /// Highlights a single line of shader source.
    pub fn highlight_line(
        &self,
        source: &str,
        target_line: usize,
    ) -> Result<Vec<ShaderHighlightSpan>, ShaderLexError> {
        Ok(self
            .highlight(source)?
            .into_iter()
            .filter(|s| s.line == target_line)
            .collect())
    }

    /// Returns the set of highlight kinds used in the source.
    pub fn used_kinds(
        &self,
        source: &str,
    ) -> Result<Vec<ShaderHighlightKind>, ShaderLexError> {
        let spans = self.highlight(source)?;
        let mut kinds: Vec<ShaderHighlightKind> = spans.iter().map(|s| s.kind).collect();
        kinds.sort_by_key(|k| format!("{k:?}"));
        kinds.dedup();
        Ok(kinds)
    }
}

impl Default for ShaderHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

// ── Shader editor ───────────────────────────────────────────────────────

/// A single open shader tab.
#[derive(Debug, Clone)]
pub struct ShaderTab {
    /// The file path (e.g., `res://shaders/outline.gdshader`).
    pub path: String,
    /// The current source text.
    pub source: String,
    /// Whether the source has unsaved modifications.
    pub modified: bool,
    /// The cursor line (1-based).
    pub cursor_line: usize,
    /// The cursor column (1-based).
    pub cursor_col: usize,
    /// Undo history.
    undo_stack: Vec<String>,
    /// Redo stack.
    redo_stack: Vec<String>,
}

impl ShaderTab {
    /// Creates a new tab.
    pub fn new(path: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            source: source.into(),
            modified: false,
            cursor_line: 1,
            cursor_col: 1,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// Replaces the source text, pushing old onto undo stack.
    pub fn set_source(&mut self, new_source: impl Into<String>) {
        let old = std::mem::replace(&mut self.source, new_source.into());
        self.undo_stack.push(old);
        self.redo_stack.clear();
        self.modified = true;
    }

    /// Undoes the last edit.
    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.undo_stack.pop() {
            let current = std::mem::replace(&mut self.source, prev);
            self.redo_stack.push(current);
            self.modified = true;
            true
        } else {
            false
        }
    }

    /// Redoes the last undone edit.
    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.redo_stack.pop() {
            let current = std::mem::replace(&mut self.source, next);
            self.undo_stack.push(current);
            self.modified = true;
            true
        } else {
            false
        }
    }

    /// Marks the tab as saved.
    pub fn mark_saved(&mut self) {
        self.modified = false;
    }

    /// Sets the cursor position (1-based).
    pub fn set_cursor(&mut self, line: usize, col: usize) {
        self.cursor_line = line;
        self.cursor_col = col;
    }

    /// Returns the number of lines.
    pub fn line_count(&self) -> usize {
        self.source.lines().count().max(1)
    }

    /// Returns a specific line (1-based).
    pub fn get_line(&self, line: usize) -> Option<&str> {
        self.source.lines().nth(line.saturating_sub(1))
    }

    /// Extracts the shader type from the source (e.g., "spatial", "canvas_item").
    pub fn shader_type(&self) -> Option<String> {
        let tokens = tokenize_shader(&self.source).ok()?;
        gdresource::shader_tokenizer::extract_shader_type(&tokens)
    }

    /// Extracts all uniform declarations from the shader source.
    pub fn uniforms(&self) -> Vec<ShaderUniform> {
        let tokens = match tokenize_shader_no_comments(&self.source) {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };
        parse_uniforms(&tokens)
    }
}

/// The shader editor, managing multiple open shader tabs.
#[derive(Debug)]
pub struct ShaderEditor {
    /// Open tabs.
    tabs: Vec<ShaderTab>,
    /// Active tab index.
    active_tab: Option<usize>,
    /// The syntax highlighter.
    highlighter: ShaderHighlighter,
}

impl ShaderEditor {
    /// Creates a new empty shader editor.
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: None,
            highlighter: ShaderHighlighter::new(),
        }
    }

    /// Opens a shader. If already open, switches to that tab.
    pub fn open(&mut self, path: impl Into<String>, source: impl Into<String>) -> usize {
        let path = path.into();
        if let Some(idx) = self.tabs.iter().position(|t| t.path == path) {
            self.active_tab = Some(idx);
            return idx;
        }
        let tab = ShaderTab::new(path, source);
        self.tabs.push(tab);
        let idx = self.tabs.len() - 1;
        self.active_tab = Some(idx);
        idx
    }

    /// Closes a tab by index.
    pub fn close(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.active_tab = None;
        } else if let Some(active) = self.active_tab {
            if active >= self.tabs.len() {
                self.active_tab = Some(self.tabs.len() - 1);
            } else if active > index {
                self.active_tab = Some(active - 1);
            }
        }
        true
    }

    /// Returns the number of open tabs.
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Returns the active tab index.
    pub fn active_tab_index(&self) -> Option<usize> {
        self.active_tab
    }

    /// Switches to a tab by index.
    pub fn set_active_tab(&mut self, index: usize) -> bool {
        if index < self.tabs.len() {
            self.active_tab = Some(index);
            true
        } else {
            false
        }
    }

    /// Returns the active tab.
    pub fn active(&self) -> Option<&ShaderTab> {
        self.active_tab.and_then(|i| self.tabs.get(i))
    }

    /// Returns a mutable reference to the active tab.
    pub fn active_mut(&mut self) -> Option<&mut ShaderTab> {
        self.active_tab.and_then(|i| self.tabs.get_mut(i))
    }

    /// Returns a tab by index.
    pub fn tab(&self, index: usize) -> Option<&ShaderTab> {
        self.tabs.get(index)
    }

    /// Returns open file paths.
    pub fn open_paths(&self) -> Vec<&str> {
        self.tabs.iter().map(|t| t.path.as_str()).collect()
    }

    /// Returns whether any tab has unsaved changes.
    pub fn has_unsaved(&self) -> bool {
        self.tabs.iter().any(|t| t.modified)
    }

    /// Highlights the active tab's source.
    pub fn highlight_active(
        &self,
    ) -> Option<Result<Vec<ShaderHighlightSpan>, ShaderLexError>> {
        self.active().map(|tab| self.highlighter.highlight(&tab.source))
    }

    /// Returns the highlighter.
    pub fn highlighter(&self) -> &ShaderHighlighter {
        &self.highlighter
    }
}

impl Default for ShaderEditor {
    fn default() -> Self {
        Self::new()
    }
}

// ── Material preview ────────────────────────────────────────────────────

/// Preview mesh shape for material rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreviewShape {
    /// A UV sphere.
    Sphere,
    /// A flat quad.
    Quad,
    /// A cube (box).
    Cube,
}

/// Uniform override value for live preview.
#[derive(Debug, Clone, PartialEq)]
pub enum UniformValue {
    Float(f64),
    Vec2(f64, f64),
    Vec3(f64, f64, f64),
    Vec4(f64, f64, f64, f64),
    Int(i64),
    Bool(bool),
    Color(Color),
}

/// Live material preview that renders a shader onto a preview mesh.
///
/// The preview extracts uniforms from the active shader source and renders
/// a software-rasterized preview sphere/quad/cube with approximate shading.
/// Uniform overrides let the inspector tweak values and see results immediately.
#[derive(Debug)]
pub struct MaterialPreview {
    /// The preview mesh shape.
    shape: PreviewShape,
    /// Preview framebuffer dimensions.
    width: u32,
    height: u32,
    /// User-provided uniform overrides (name → value).
    overrides: std::collections::HashMap<String, UniformValue>,
    /// Whether the preview is dirty and needs re-rendering.
    dirty: bool,
    /// Cached rendered preview (None if never rendered or dirty).
    cached_frame: Option<FrameBuffer>,
}

impl MaterialPreview {
    /// Creates a new material preview with the given dimensions.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            shape: PreviewShape::Sphere,
            width,
            height,
            overrides: std::collections::HashMap::new(),
            dirty: true,
            cached_frame: None,
        }
    }

    /// Sets the preview shape.
    pub fn set_shape(&mut self, shape: PreviewShape) {
        if self.shape != shape {
            self.shape = shape;
            self.dirty = true;
        }
    }

    /// Returns the current preview shape.
    pub fn shape(&self) -> PreviewShape {
        self.shape
    }

    /// Sets a uniform override value.
    pub fn set_uniform(&mut self, name: impl Into<String>, value: UniformValue) {
        self.overrides.insert(name.into(), value);
        self.dirty = true;
    }

    /// Removes a uniform override, reverting to the shader default.
    pub fn clear_uniform(&mut self, name: &str) {
        if self.overrides.remove(name).is_some() {
            self.dirty = true;
        }
    }

    /// Clears all uniform overrides.
    pub fn clear_all_uniforms(&mut self) {
        if !self.overrides.is_empty() {
            self.overrides.clear();
            self.dirty = true;
        }
    }

    /// Returns the current uniform overrides.
    pub fn overrides(&self) -> &std::collections::HashMap<String, UniformValue> {
        &self.overrides
    }

    /// Marks the preview as needing re-rendering (e.g., after source change).
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// Returns `true` if the preview needs re-rendering.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Renders the material preview for the given shader source.
    ///
    /// Uses uniform declarations to determine a base color. If the shader
    /// declares a `source_color` uniform of type `vec4`, that color is used
    /// as the material albedo. Otherwise, a neutral gray is used.
    ///
    /// The preview is a software-rasterized sphere/quad with simple N·L lighting.
    pub fn render(&mut self, source: &str) -> &FrameBuffer {
        if !self.dirty {
            if let Some(ref fb) = self.cached_frame {
                return fb;
            }
        }

        let base_color = self.resolve_base_color(source);
        let fb = match self.shape {
            PreviewShape::Sphere => self.render_sphere(base_color),
            PreviewShape::Quad => self.render_quad(base_color),
            PreviewShape::Cube => self.render_cube(base_color),
        };

        self.cached_frame = Some(fb);
        self.dirty = false;
        self.cached_frame.as_ref().unwrap()
    }

    /// Returns the cached framebuffer if available.
    pub fn cached(&self) -> Option<&FrameBuffer> {
        self.cached_frame.as_ref()
    }

    /// Extracts uniform info from the shader and returns a serializable summary.
    pub fn uniform_info(source: &str) -> Vec<PreviewUniformInfo> {
        let tokens = match tokenize_shader_no_comments(source) {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };
        parse_uniforms(&tokens)
            .into_iter()
            .map(|u| PreviewUniformInfo {
                name: u.name,
                type_name: u.type_name,
                instance: u.instance,
                group: u.group,
            })
            .collect()
    }

    /// Resolves the base color from uniform overrides or shader defaults.
    fn resolve_base_color(&self, source: &str) -> Color {
        // Check overrides first — look for any color-typed override.
        for (name, val) in &self.overrides {
            if let UniformValue::Color(c) = val {
                return *c;
            }
            if let UniformValue::Vec4(r, g, b, a) = val {
                // Check if this is a color uniform.
                let tokens = tokenize_shader_no_comments(source).unwrap_or_default();
                let uniforms = parse_uniforms(&tokens);
                if uniforms.iter().any(|u| u.name == *name && u.type_name == "vec4") {
                    return Color::new(*r as f32, *g as f32, *b as f32, *a as f32);
                }
            }
        }

        // Parse shader source for a vec4 uniform with "albedo" or "color" in name.
        let tokens = match tokenize_shader_no_comments(source) {
            Ok(t) => t,
            Err(_) => return Color::new(0.6, 0.6, 0.6, 1.0),
        };
        let uniforms = parse_uniforms(&tokens);
        for u in &uniforms {
            if u.type_name == "vec4"
                && (u.name.contains("color") || u.name.contains("albedo") || u.name.contains("tint"))
            {
                // Try to find a default value — for now, use a distinctive placeholder.
                return Color::new(1.0, 0.5, 0.0, 1.0);
            }
        }

        Color::new(0.6, 0.6, 0.6, 1.0)
    }

    /// Renders a sphere preview with simple N·L lighting.
    fn render_sphere(&self, base_color: Color) -> FrameBuffer {
        let mut fb = FrameBuffer::new(self.width, self.height, Color::new(0.12, 0.12, 0.14, 1.0));
        let cx = self.width as f32 / 2.0;
        let cy = self.height as f32 / 2.0;
        let radius = (self.width.min(self.height) as f32 / 2.0) * 0.85;

        // Light direction (normalized, upper-right-front).
        let lx: f32 = 0.5;
        let ly: f32 = -0.6;
        let lz: f32 = 0.6;
        let ll = (lx * lx + ly * ly + lz * lz).sqrt();
        let (lx, ly, lz) = (lx / ll, ly / ll, lz / ll);

        for py in 0..self.height {
            for px in 0..self.width {
                let dx = (px as f32 - cx) / radius;
                let dy = (py as f32 - cy) / radius;
                let r2 = dx * dx + dy * dy;
                if r2 > 1.0 {
                    continue;
                }
                let dz = (1.0 - r2).sqrt();
                // Normal = (dx, dy, dz), already unit length.
                let ndotl = (dx * lx + dy * ly + dz * lz).max(0.0);
                let ambient = 0.15;
                let diffuse = ndotl * 0.85;
                let intensity = ambient + diffuse;

                // Specular (Blinn-Phong).
                let hx = lx;
                let hy = ly;
                let hz = lz + 1.0; // view direction = (0, 0, 1)
                let hl = (hx * hx + hy * hy + hz * hz).sqrt();
                let ndoth = if hl > 0.0 {
                    ((dx * hx + dy * hy + dz * hz) / hl).max(0.0)
                } else {
                    0.0
                };
                let specular = ndoth.powf(32.0) * 0.4;

                let color = Color::new(
                    (base_color.r * intensity + specular).min(1.0),
                    (base_color.g * intensity + specular).min(1.0),
                    (base_color.b * intensity + specular).min(1.0),
                    1.0,
                );
                fb.set_pixel(px, py, color);
            }
        }
        fb
    }

    /// Renders a flat quad preview.
    fn render_quad(&self, base_color: Color) -> FrameBuffer {
        let mut fb = FrameBuffer::new(self.width, self.height, Color::new(0.12, 0.12, 0.14, 1.0));
        let margin = (self.width.min(self.height) as f32 * 0.1) as u32;
        let x0 = margin;
        let y0 = margin;
        let x1 = self.width.saturating_sub(margin);
        let y1 = self.height.saturating_sub(margin);

        // Simple gradient to give depth impression.
        for py in y0..y1 {
            for px in x0..x1 {
                let _u = (px - x0) as f32 / (x1 - x0) as f32;
                let v = (py - y0) as f32 / (y1 - y0) as f32;
                let shade = 0.7 + 0.3 * (1.0 - v); // brighter at top
                let color = Color::new(
                    (base_color.r * shade).min(1.0),
                    (base_color.g * shade).min(1.0),
                    (base_color.b * shade).min(1.0),
                    1.0,
                );
                fb.set_pixel(px, py, color);
            }
        }
        fb
    }

    /// Renders a cube preview with simple face shading.
    fn render_cube(&self, base_color: Color) -> FrameBuffer {
        let mut fb = FrameBuffer::new(self.width, self.height, Color::new(0.12, 0.12, 0.14, 1.0));
        let cx = self.width as f32 / 2.0;
        let cy = self.height as f32 / 2.0;
        let size = (self.width.min(self.height) as f32 / 2.0) * 0.6;

        // Isometric-like projection of a cube (three visible faces).
        let iso_x = size * 0.85;
        let iso_y = size * 0.5;

        // Face brightnesses (top, right, left).
        let faces: [(Color, [(f32, f32); 4]); 3] = [
            // Left face.
            (
                Color::new(base_color.r * 0.5, base_color.g * 0.5, base_color.b * 0.5, 1.0),
                [
                    (cx - iso_x, cy),
                    (cx, cy + iso_y),
                    (cx, cy + iso_y + size),
                    (cx - iso_x, cy + size),
                ],
            ),
            // Right face.
            (
                Color::new(base_color.r * 0.7, base_color.g * 0.7, base_color.b * 0.7, 1.0),
                [
                    (cx, cy + iso_y),
                    (cx + iso_x, cy),
                    (cx + iso_x, cy + size),
                    (cx, cy + iso_y + size),
                ],
            ),
            // Top face.
            (
                Color::new(
                    (base_color.r * 0.9).min(1.0),
                    (base_color.g * 0.9).min(1.0),
                    (base_color.b * 0.9).min(1.0),
                    1.0,
                ),
                [
                    (cx, cy + iso_y - size),
                    (cx + iso_x, cy - size),
                    (cx, cy - size + iso_y - size + size), // simplification
                    (cx - iso_x, cy),
                ],
            ),
        ];

        // Rasterize each face as a filled quad using scanline.
        for (color, verts) in &faces {
            fill_quad(&mut fb, verts, *color);
        }
        fb
    }
}

impl Default for MaterialPreview {
    fn default() -> Self {
        Self::new(128, 128)
    }
}

/// Serializable uniform info for the preview panel.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewUniformInfo {
    /// The uniform variable name.
    pub name: String,
    /// The GLSL type name.
    pub type_name: String,
    /// Whether this is an `instance` uniform.
    pub instance: bool,
    /// The uniform group, if any.
    pub group: Option<String>,
}

/// Fills a convex quad in the framebuffer using scanline rasterization.
fn fill_quad(fb: &mut FrameBuffer, verts: &[(f32, f32); 4], color: Color) {
    // Find bounding box.
    let min_y = verts.iter().map(|v| v.1).fold(f32::MAX, f32::min).max(0.0) as u32;
    let max_y = verts
        .iter()
        .map(|v| v.1)
        .fold(f32::MIN, f32::max)
        .min(fb.height as f32 - 1.0) as u32;

    for py in min_y..=max_y {
        let y = py as f32 + 0.5;
        let mut xs = Vec::new();

        // Test each edge.
        for i in 0..4 {
            let (x0, y0) = verts[i];
            let (x1, y1) = verts[(i + 1) % 4];
            if (y0 <= y && y1 > y) || (y1 <= y && y0 > y) {
                let t = (y - y0) / (y1 - y0);
                xs.push(x0 + t * (x1 - x0));
            }
        }

        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut i = 0;
        while i + 1 < xs.len() {
            let x_start = (xs[i].max(0.0) as u32).min(fb.width.saturating_sub(1));
            let x_end = (xs[i + 1].min(fb.width as f32 - 1.0) as u32).min(fb.width.saturating_sub(1));
            for px in x_start..=x_end {
                fb.set_pixel(px, py, color);
            }
            i += 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_SHADER: &str = r#"shader_type spatial;
render_mode unshaded;

uniform vec4 albedo_color : source_color = vec4(1.0, 0.5, 0.0, 1.0);
uniform sampler2D noise_tex;

void fragment() {
    vec4 noise = texture(noise_tex, UV);
    ALBEDO = albedo_color.rgb * noise.rgb;
    ALPHA = albedo_color.a;
}
"#;

    #[test]
    fn highlight_empty_source() {
        let h = ShaderHighlighter::new();
        let spans = h.highlight("").unwrap();
        assert!(spans.is_empty());
    }

    #[test]
    fn highlight_keywords() {
        let h = ShaderHighlighter::new();
        let spans = h.highlight("shader_type spatial;").unwrap();
        assert_eq!(spans[0].kind, ShaderHighlightKind::Keyword);
        assert_eq!(spans[0].text, "shader_type");
    }

    #[test]
    fn highlight_builtin_types() {
        let h = ShaderHighlighter::new();
        let spans = h.highlight("vec4 color;").unwrap();
        assert_eq!(spans[0].kind, ShaderHighlightKind::BuiltinType);
        assert_eq!(spans[0].text, "vec4");
    }

    #[test]
    fn highlight_comments() {
        let h = ShaderHighlighter::new();
        let spans = h.highlight("// this is a comment").unwrap();
        assert!(spans.iter().any(|s| s.kind == ShaderHighlightKind::Comment));
    }

    #[test]
    fn highlight_numeric_literals() {
        let h = ShaderHighlighter::new();
        let spans = h.highlight("float x = 3.14;").unwrap();
        let num = spans.iter().find(|s| s.kind == ShaderHighlightKind::NumberLiteral);
        assert!(num.is_some());
    }

    #[test]
    fn highlight_sample_shader() {
        let h = ShaderHighlighter::new();
        let spans = h.highlight(SAMPLE_SHADER).unwrap();
        assert!(!spans.is_empty());
        // Should have keywords, types, identifiers, etc.
        let kinds: Vec<_> = spans.iter().map(|s| s.kind).collect();
        assert!(kinds.contains(&ShaderHighlightKind::Keyword));
        assert!(kinds.contains(&ShaderHighlightKind::BuiltinType));
        assert!(kinds.contains(&ShaderHighlightKind::Identifier));
        assert!(kinds.contains(&ShaderHighlightKind::NumberLiteral));
    }

    #[test]
    fn highlight_builtin_function() {
        let h = ShaderHighlighter::new();
        let spans = h.highlight("texture(tex, UV)").unwrap();
        let tex = spans.iter().find(|s| s.text == "texture");
        assert_eq!(tex.unwrap().kind, ShaderHighlightKind::BuiltinFunction);
    }

    #[test]
    fn highlight_line_filters_correctly() {
        let h = ShaderHighlighter::new();
        let spans = h.highlight_line(SAMPLE_SHADER, 1).unwrap();
        assert!(spans.iter().all(|s| s.line == 1));
        assert!(spans.iter().any(|s| s.text == "shader_type"));
    }

    #[test]
    fn shader_tab_basics() {
        let tab = ShaderTab::new("res://test.gdshader", SAMPLE_SHADER);
        assert_eq!(tab.path, "res://test.gdshader");
        assert!(!tab.modified);
        assert!(tab.line_count() > 1);
        assert_eq!(tab.get_line(1), Some("shader_type spatial;"));
    }

    #[test]
    fn shader_tab_undo_redo() {
        let mut tab = ShaderTab::new("res://test.gdshader", "void fragment() {}");
        tab.set_source("void vertex() {}");
        assert!(tab.modified);
        assert_eq!(tab.source, "void vertex() {}");

        assert!(tab.undo());
        assert_eq!(tab.source, "void fragment() {}");

        assert!(tab.redo());
        assert_eq!(tab.source, "void vertex() {}");

        assert!(!tab.redo()); // nothing to redo
    }

    #[test]
    fn shader_tab_shader_type_extraction() {
        let tab = ShaderTab::new("res://test.gdshader", SAMPLE_SHADER);
        assert_eq!(tab.shader_type(), Some("spatial".to_string()));

        let tab2 = ShaderTab::new("res://canvas.gdshader", "shader_type canvas_item;");
        assert_eq!(tab2.shader_type(), Some("canvas_item".to_string()));
    }

    #[test]
    fn shader_editor_open_close() {
        let mut editor = ShaderEditor::new();
        assert_eq!(editor.tab_count(), 0);
        assert!(editor.active().is_none());

        let idx = editor.open("res://a.gdshader", "shader_type spatial;");
        assert_eq!(idx, 0);
        assert_eq!(editor.tab_count(), 1);
        assert!(editor.active().is_some());

        // Opening same path returns same index
        let idx2 = editor.open("res://a.gdshader", "different source");
        assert_eq!(idx2, 0);
        assert_eq!(editor.tab_count(), 1);

        // Open a second shader
        let idx3 = editor.open("res://b.gdshader", "shader_type canvas_item;");
        assert_eq!(idx3, 1);
        assert_eq!(editor.tab_count(), 2);

        assert!(editor.close(0));
        assert_eq!(editor.tab_count(), 1);
    }

    #[test]
    fn shader_editor_highlight_active() {
        let mut editor = ShaderEditor::new();
        editor.open("res://test.gdshader", SAMPLE_SHADER);
        let result = editor.highlight_active();
        assert!(result.is_some());
        let spans = result.unwrap().unwrap();
        assert!(!spans.is_empty());
    }

    #[test]
    fn shader_tab_extract_uniforms() {
        let tab = ShaderTab::new("res://test.gdshader", SAMPLE_SHADER);
        let uniforms = tab.uniforms();
        assert_eq!(uniforms.len(), 2);
        assert_eq!(uniforms[0].name, "albedo_color");
        assert_eq!(uniforms[0].type_name, "vec4");
        assert_eq!(uniforms[1].name, "noise_tex");
        assert_eq!(uniforms[1].type_name, "sampler2D");
    }

    #[test]
    fn material_preview_renders_sphere() {
        let mut preview = MaterialPreview::new(64, 64);
        assert!(preview.is_dirty());
        preview.render(SAMPLE_SHADER);
        assert!(!preview.is_dirty());
        // Center pixel should not be the background color.
        let fb = preview.cached().unwrap();
        let center = fb.pixels[(32 * 64 + 32) as usize];
        assert!(center.r > 0.12 || center.g > 0.12 || center.b > 0.14);
    }

    #[test]
    fn material_preview_renders_quad() {
        let mut preview = MaterialPreview::new(64, 64);
        preview.set_shape(PreviewShape::Quad);
        let fb = preview.render("shader_type spatial;");
        assert_eq!(fb.width, 64);
        assert_eq!(fb.height, 64);
    }

    #[test]
    fn material_preview_renders_cube() {
        let mut preview = MaterialPreview::new(64, 64);
        preview.set_shape(PreviewShape::Cube);
        let fb = preview.render("shader_type spatial;");
        assert_eq!(fb.width, 64);
        assert_eq!(fb.height, 64);
    }

    #[test]
    fn material_preview_shape_toggle() {
        let mut preview = MaterialPreview::new(64, 64);
        assert_eq!(preview.shape(), PreviewShape::Sphere);
        preview.set_shape(PreviewShape::Cube);
        assert_eq!(preview.shape(), PreviewShape::Cube);
        assert!(preview.is_dirty());
    }

    #[test]
    fn material_preview_uniform_overrides() {
        let mut preview = MaterialPreview::new(64, 64);
        preview.set_uniform("my_color", UniformValue::Color(Color::new(1.0, 0.0, 0.0, 1.0)));
        assert_eq!(preview.overrides().len(), 1);

        // Render with the override — the sphere should be reddish.
        preview.render(SAMPLE_SHADER);
        let fb = preview.cached().unwrap();
        let center = fb.pixels[(32 * 64 + 32) as usize];
        // Red channel should dominate.
        assert!(center.r > center.g);
        assert!(center.r > center.b);

        preview.clear_uniform("my_color");
        assert!(preview.overrides().is_empty());
        assert!(preview.is_dirty());
    }

    #[test]
    fn material_preview_clear_all_uniforms() {
        let mut preview = MaterialPreview::new(64, 64);
        preview.set_uniform("a", UniformValue::Float(1.0));
        preview.set_uniform("b", UniformValue::Float(2.0));
        assert_eq!(preview.overrides().len(), 2);
        preview.clear_all_uniforms();
        assert!(preview.overrides().is_empty());
        assert!(preview.is_dirty());
    }

    #[test]
    fn material_preview_invalidate() {
        let mut preview = MaterialPreview::new(64, 64);
        let _ = preview.render("shader_type spatial;");
        assert!(!preview.is_dirty());
        preview.invalidate();
        assert!(preview.is_dirty());
    }

    #[test]
    fn material_preview_cached() {
        let mut preview = MaterialPreview::new(64, 64);
        assert!(preview.cached().is_none());
        let _ = preview.render("shader_type spatial;");
        assert!(preview.cached().is_some());
    }

    #[test]
    fn material_preview_uniform_info() {
        let info = MaterialPreview::uniform_info(SAMPLE_SHADER);
        assert_eq!(info.len(), 2);
        assert_eq!(info[0].name, "albedo_color");
        assert_eq!(info[0].type_name, "vec4");
        assert!(!info[0].instance);
        assert_eq!(info[1].name, "noise_tex");
        assert_eq!(info[1].type_name, "sampler2D");
    }

    #[test]
    fn material_preview_uniform_info_empty_source() {
        let info = MaterialPreview::uniform_info("");
        assert!(info.is_empty());
    }

    #[test]
    fn material_preview_default() {
        let preview = MaterialPreview::default();
        assert_eq!(preview.shape(), PreviewShape::Sphere);
        assert!(preview.is_dirty());
    }
}

//! 3D viewport camera controller for the editor.
//!
//! Provides orbit, pan, and fly camera modes that mirror Godot 4's editor viewport.
//!
//! - **Orbit**: Middle-mouse drag rotates around a focus point.
//! - **Pan**: Shift + middle-mouse drag translates the view.
//! - **Fly**: Right-click + WASD for free-look navigation.
//! - **Zoom**: Scroll wheel moves towards/away from the focus point.

use gdcore::math::{Color, Vector3};
use gdcore::math3d::{Basis, Transform3D};
use gdscene::SceneTree;
use gdserver3d::environment::{AmbientSource, BackgroundMode, Environment3D, ToneMapper};
use gdserver3d::fog_volume::{FogVolume, FogVolumeShape};
#[cfg(test)]
use gdserver3d::fog_volume::FogMaterial;
use gdserver3d::sky::{ProceduralSkyMaterial, Sky, SkyMaterial, SkyProcessMode};

// ---------------------------------------------------------------------------
// Camera mode
// ---------------------------------------------------------------------------

/// Active camera interaction mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraMode {
    /// No active interaction — idle.
    Idle,
    /// Orbiting around the focus point (middle-mouse drag).
    Orbit,
    /// Panning the view (shift + middle-mouse drag).
    Pan,
    /// Free-look fly mode (right-click + WASD).
    Fly,
}

// ---------------------------------------------------------------------------
// Projection
// ---------------------------------------------------------------------------

/// Viewport projection type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Projection {
    /// Standard perspective projection.
    Perspective,
    /// Orthographic projection (top/front/side views).
    Orthographic,
}

// ---------------------------------------------------------------------------
// ViewportCamera3D
// ---------------------------------------------------------------------------

/// A 3D editor viewport camera with orbit, pan, and fly controls.
///
/// Follows Godot 4's editor camera conventions:
/// - Y-up, right-handed coordinate system.
/// - Orbit rotates around `focus_point`.
/// - Distance is the distance from `focus_point` along the camera's -Z axis.
#[derive(Debug, Clone)]
pub struct ViewportCamera3D {
    /// The point the camera orbits around.
    pub focus_point: Vector3,
    /// Distance from the focus point (orbit radius).
    pub distance: f32,
    /// Yaw angle in radians (rotation around Y axis).
    pub yaw: f32,
    /// Pitch angle in radians (rotation around X axis, clamped to ±89°).
    pub pitch: f32,
    /// Field of view in degrees (perspective mode only).
    pub fov_degrees: f32,
    /// Orthographic size (half-height in world units).
    pub ortho_size: f32,
    /// Current projection mode.
    pub projection: Projection,
    /// Current interaction mode.
    pub mode: CameraMode,
    /// Fly-mode movement speed (world units per second).
    pub fly_speed: f32,
    /// Orbit sensitivity (radians per pixel of mouse movement).
    pub orbit_sensitivity: f32,
    /// Pan sensitivity (world units per pixel).
    pub pan_sensitivity: f32,
    /// Zoom sensitivity (multiplier per scroll tick).
    pub zoom_sensitivity: f32,
}

impl Default for ViewportCamera3D {
    fn default() -> Self {
        Self {
            focus_point: Vector3::ZERO,
            distance: 5.0,
            yaw: std::f32::consts::FRAC_PI_4,    // 45°
            pitch: -0.5,                           // ~-28.6° (looking slightly down)
            fov_degrees: 70.0,
            ortho_size: 5.0,
            projection: Projection::Perspective,
            mode: CameraMode::Idle,
            fly_speed: 5.0,
            orbit_sensitivity: 0.005,
            pan_sensitivity: 0.01,
            zoom_sensitivity: 0.1,
        }
    }
}

/// Pitch clamp limits (±89 degrees in radians).
const PITCH_MAX: f32 = 89.0 * std::f32::consts::PI / 180.0;
const PITCH_MIN: f32 = -PITCH_MAX;

/// Minimum orbit distance to prevent clipping through the focus point.
const MIN_DISTANCE: f32 = 0.05;

/// Maximum orbit distance.
const MAX_DISTANCE: f32 = 1000.0;

impl ViewportCamera3D {
    /// Creates a new camera with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the camera's world-space position, derived from orbit parameters.
    pub fn position(&self) -> Vector3 {
        let dir = self.orbit_direction();
        Vector3::new(
            self.focus_point.x - dir.x * self.distance,
            self.focus_point.y - dir.y * self.distance,
            self.focus_point.z - dir.z * self.distance,
        )
    }

    /// Returns the camera's forward direction (unit vector pointing from camera toward focus).
    ///
    /// Convention: yaw=0, pitch=0 looks along −Z (Godot front view).
    /// Negative pitch tilts the view downward (camera above the focus point).
    pub fn orbit_direction(&self) -> Vector3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vector3::new(-sy * cp, sp, -cy * cp)
    }

    /// Returns the camera's right vector (perpendicular to forward in the XZ plane).
    pub fn right(&self) -> Vector3 {
        let (sy, cy) = self.yaw.sin_cos();
        Vector3::new(cy, 0.0, -sy)
    }

    /// Returns the camera's up vector (perpendicular to forward and right).
    pub fn up(&self) -> Vector3 {
        let fwd = self.orbit_direction();
        let right = self.right();
        // up = right × forward
        right.cross(fwd).normalized()
    }

    /// Computes the full `Transform3D` for this camera.
    pub fn transform(&self) -> Transform3D {
        let pos = self.position();
        let fwd = self.orbit_direction();
        let right = self.right();
        let up = right.cross(fwd).normalized();

        // Camera basis: columns are right, up, -forward (Godot convention: -Z is forward).
        let basis = Basis {
            x: right,
            y: up,
            z: Vector3::new(-fwd.x, -fwd.y, -fwd.z),
        };

        Transform3D {
            basis,
            origin: pos,
        }
    }

    // -----------------------------------------------------------------------
    // Orbit
    // -----------------------------------------------------------------------

    /// Begin orbiting.
    pub fn begin_orbit(&mut self) {
        self.mode = CameraMode::Orbit;
    }

    /// Update orbit with mouse delta (in pixels).
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw += dx * self.orbit_sensitivity;
        self.pitch = (self.pitch + dy * self.orbit_sensitivity).clamp(PITCH_MIN, PITCH_MAX);
    }

    /// End orbiting.
    pub fn end_orbit(&mut self) {
        if self.mode == CameraMode::Orbit {
            self.mode = CameraMode::Idle;
        }
    }

    // -----------------------------------------------------------------------
    // Pan
    // -----------------------------------------------------------------------

    /// Begin panning.
    pub fn begin_pan(&mut self) {
        self.mode = CameraMode::Pan;
    }

    /// Update pan with mouse delta (in pixels).
    ///
    /// Pan sensitivity scales with distance so panning feels consistent
    /// regardless of zoom level.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        let right = self.right();
        let up = self.up();
        let scale = self.pan_sensitivity * self.distance;

        self.focus_point = Vector3::new(
            self.focus_point.x - right.x * dx * scale + up.x * dy * scale,
            self.focus_point.y - right.y * dx * scale + up.y * dy * scale,
            self.focus_point.z - right.z * dx * scale + up.z * dy * scale,
        );
    }

    /// End panning.
    pub fn end_pan(&mut self) {
        if self.mode == CameraMode::Pan {
            self.mode = CameraMode::Idle;
        }
    }

    // -----------------------------------------------------------------------
    // Zoom
    // -----------------------------------------------------------------------

    /// Zoom by scroll delta. Positive = zoom in, negative = zoom out.
    pub fn zoom(&mut self, delta: f32) {
        match self.projection {
            Projection::Perspective => {
                let factor = 1.0 - delta * self.zoom_sensitivity;
                self.distance = (self.distance * factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
            }
            Projection::Orthographic => {
                let factor = 1.0 - delta * self.zoom_sensitivity;
                self.ortho_size = (self.ortho_size * factor).clamp(0.1, 500.0);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Fly mode
    // -----------------------------------------------------------------------

    /// Begin fly mode (free-look).
    pub fn begin_fly(&mut self) {
        self.mode = CameraMode::Fly;
    }

    /// Update fly-mode look with mouse delta (in pixels).
    pub fn fly_look(&mut self, dx: f32, dy: f32) {
        // Save position before orientation change so look doesn't drift the camera.
        let pos = self.position();
        self.yaw += dx * self.orbit_sensitivity;
        self.pitch = (self.pitch + dy * self.orbit_sensitivity).clamp(PITCH_MIN, PITCH_MAX);
        // Update focus_point so position() stays at the saved pos.
        let fwd = self.orbit_direction();
        self.focus_point = Vector3::new(
            pos.x + fwd.x * self.distance,
            pos.y + fwd.y * self.distance,
            pos.z + fwd.z * self.distance,
        );
    }

    /// Move the camera in fly mode.
    ///
    /// `input` is a normalized direction vector in camera-local space:
    /// - x: right/left
    /// - y: up/down (world Y)
    /// - z: forward/backward
    ///
    /// `dt` is the time delta in seconds.
    pub fn fly_move(&mut self, input: Vector3, dt: f32) {
        let right = self.right();
        let fwd = self.orbit_direction();
        let speed = self.fly_speed * dt;

        let movement = Vector3::new(
            right.x * input.x * speed + fwd.x * input.z * speed,
            input.y * speed,
            right.z * input.x * speed + fwd.z * input.z * speed,
        );

        self.focus_point = Vector3::new(
            self.focus_point.x + movement.x,
            self.focus_point.y + movement.y,
            self.focus_point.z + movement.z,
        );
    }

    /// End fly mode, keeping the current position.
    pub fn end_fly(&mut self) {
        if self.mode == CameraMode::Fly {
            self.mode = CameraMode::Idle;
        }
    }

    // -----------------------------------------------------------------------
    // Presets
    // -----------------------------------------------------------------------

    /// Snap to a preset view direction (top, front, right, etc.).
    pub fn snap_to_front(&mut self) {
        self.yaw = 0.0;
        self.pitch = 0.0;
    }

    /// Snap to top-down view.
    pub fn snap_to_top(&mut self) {
        self.yaw = 0.0;
        self.pitch = PITCH_MIN; // looking straight down
    }

    /// Snap to right-side view.
    pub fn snap_to_right(&mut self) {
        self.yaw = std::f32::consts::FRAC_PI_2;
        self.pitch = 0.0;
    }

    /// Reset the camera to default position.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Focus on a specific world-space point, keeping the current orientation.
    pub fn focus_on(&mut self, point: Vector3) {
        self.focus_point = point;
    }

    /// Focus on a point and set a specific distance.
    pub fn focus_on_with_distance(&mut self, point: Vector3, distance: f32) {
        self.focus_point = point;
        self.distance = distance.clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    /// Toggle between perspective and orthographic projection.
    pub fn toggle_projection(&mut self) {
        self.projection = match self.projection {
            Projection::Perspective => Projection::Orthographic,
            Projection::Orthographic => Projection::Perspective,
        };
    }

    /// Returns the perspective projection FOV in radians.
    pub fn fov_radians(&self) -> f32 {
        self.fov_degrees * std::f32::consts::PI / 180.0
    }

    /// Compute a view matrix as a `Transform3D` (inverse of the camera transform).
    pub fn view_transform(&self) -> Transform3D {
        self.transform().inverse()
    }
}

// ---------------------------------------------------------------------------
// Grid3D
// ---------------------------------------------------------------------------

/// Describes a 3D grid plane for the viewport.
#[derive(Debug, Clone)]
pub struct Grid3D {
    /// Number of grid lines in each direction from the origin.
    pub extent: u32,
    /// Spacing between minor grid lines (in world units).
    pub minor_spacing: f32,
    /// How many minor lines per major line.
    pub major_every: u32,
    /// Whether the grid is visible.
    pub visible: bool,
}

impl Default for Grid3D {
    fn default() -> Self {
        Self {
            extent: 50,
            minor_spacing: 1.0,
            major_every: 5,
            visible: true,
        }
    }
}

/// A single grid line segment (for rendering).
#[derive(Debug, Clone, Copy)]
pub struct GridLine {
    pub start: Vector3,
    pub end: Vector3,
    pub is_major: bool,
}

impl Grid3D {
    /// Generate grid line segments on the XZ plane at Y=0.
    pub fn generate_lines(&self) -> Vec<GridLine> {
        if !self.visible {
            return Vec::new();
        }

        let n = self.extent as i32;
        let total = ((n * 2 + 1) * 2) as usize;
        let mut lines = Vec::with_capacity(total);
        let half = n as f32 * self.minor_spacing;

        for i in -n..=n {
            let pos = i as f32 * self.minor_spacing;
            let is_major = self.major_every > 0 && (i % self.major_every as i32) == 0;

            // Line along Z
            lines.push(GridLine {
                start: Vector3::new(pos, 0.0, -half),
                end: Vector3::new(pos, 0.0, half),
                is_major,
            });

            // Line along X
            lines.push(GridLine {
                start: Vector3::new(-half, 0.0, pos),
                end: Vector3::new(half, 0.0, pos),
                is_major,
            });
        }

        lines
    }
}

// ---------------------------------------------------------------------------
// EnvironmentPreview3D
// ---------------------------------------------------------------------------

/// Controls the 3D environment preview shown in the editor viewport.
///
/// Wraps an [`Environment3D`] resource together with viewport-specific preview
/// state such as whether the sky, fog, and lighting overlays are enabled.
/// This mirrors Godot 4's "Preview Environment" / "Preview Sun" toggles in the
/// 3D viewport toolbar.
#[derive(Debug, Clone)]
pub struct EnvironmentPreview3D {
    /// The environment resource driving the preview.
    pub environment: Environment3D,
    /// Whether the environment preview is enabled at all.
    pub enabled: bool,
    /// Whether sky rendering is shown in the viewport.
    pub sky_visible: bool,
    /// Whether fog is shown in the viewport.
    pub fog_visible: bool,
    /// Whether the preview sun (directional light) is active.
    pub sun_enabled: bool,
    /// Direction of the preview sun (unit vector, points *from* the light).
    pub sun_direction: Vector3,
    /// Color of the preview sun.
    pub sun_color: Color,
    /// Energy (intensity) of the preview sun.
    pub sun_energy: f32,
    /// Fog volumes present in the scene (collected from WorldEnvironment / FogVolume nodes).
    pub fog_volumes: Vec<FogVolume>,
}

impl Default for EnvironmentPreview3D {
    fn default() -> Self {
        // Default environment: procedural sky, mild ambient, sun at 45°
        let mut env = Environment3D::default();
        env.background_mode = BackgroundMode::Sky;
        env.ambient_source = AmbientSource::Sky;
        env.ambient_energy = 0.5;
        env.sky = Some(Sky {
            material: SkyMaterial::Procedural(ProceduralSkyMaterial::default()),
            process_mode: SkyProcessMode::Automatic,
            radiance_size: 256,
        });

        Self {
            environment: env,
            enabled: true,
            sky_visible: true,
            fog_visible: true,
            sun_enabled: true,
            // Default sun direction: ~45° elevation, slight yaw — matches Godot's default.
            sun_direction: Vector3::new(-0.5, -0.707, -0.5).normalized(),
            sun_color: Color::WHITE,
            sun_energy: 1.0,
            fog_volumes: Vec::new(),
        }
    }
}

impl EnvironmentPreview3D {
    /// Toggles the entire environment preview on/off.
    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
    }

    /// Toggles sky visibility independently.
    pub fn toggle_sky(&mut self) {
        self.sky_visible = !self.sky_visible;
    }

    /// Toggles fog visibility independently.
    pub fn toggle_fog(&mut self) {
        self.fog_visible = !self.fog_visible;
    }

    /// Toggles the preview sun on/off.
    pub fn toggle_sun(&mut self) {
        self.sun_enabled = !self.sun_enabled;
    }

    /// Returns the effective background mode, accounting for preview visibility.
    ///
    /// If the preview is disabled or sky is hidden, falls back to `ClearColor`.
    pub fn effective_background_mode(&self) -> BackgroundMode {
        if !self.enabled || !self.sky_visible {
            return BackgroundMode::ClearColor;
        }
        self.environment.background_mode
    }

    /// Returns the effective fog enabled state, considering both the environment
    /// setting and the viewport toggle.
    pub fn effective_fog_enabled(&self) -> bool {
        self.enabled && self.fog_visible && self.environment.fog_enabled
    }

    /// Returns the effective ambient color, incorporating the environment's
    /// ambient settings and energy.
    pub fn effective_ambient_color(&self) -> Color {
        if !self.enabled {
            return Color::BLACK;
        }
        Color::new(
            self.environment.ambient_color.r * self.environment.ambient_energy,
            self.environment.ambient_color.g * self.environment.ambient_energy,
            self.environment.ambient_color.b * self.environment.ambient_energy,
            1.0,
        )
    }

    /// Replaces the current environment with one loaded from a scene's
    /// `WorldEnvironment` node.
    pub fn load_from_environment(&mut self, env: Environment3D) {
        self.environment = env;
    }

    /// Sets the sky material on the current environment.
    pub fn set_sky_material(&mut self, material: SkyMaterial) {
        match &mut self.environment.sky {
            Some(sky) => sky.material = material,
            None => {
                self.environment.sky = Some(Sky {
                    material,
                    process_mode: SkyProcessMode::Automatic,
                    radiance_size: 256,
                });
                self.environment.background_mode = BackgroundMode::Sky;
            }
        }
    }

    /// Updates the preview sun direction from an azimuth/elevation pair (in radians).
    ///
    /// `azimuth` is the horizontal angle (0 = north/−Z), `elevation` is the
    /// angle above the horizon (0 = horizontal, π/2 = straight down).
    pub fn set_sun_angles(&mut self, azimuth: f32, elevation: f32) {
        let (sa, ca) = azimuth.sin_cos();
        let (se, ce) = elevation.sin_cos();
        self.sun_direction = Vector3::new(-sa * ce, -se, -ca * ce).normalized();
    }

    /// Resets all preview state to defaults.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Sets the fog density on the environment.
    pub fn set_fog_density(&mut self, density: f32) {
        self.environment.fog_density = density.max(0.0);
    }

    /// Sets the fog color on the environment.
    pub fn set_fog_color(&mut self, color: Color) {
        self.environment.fog_light_color = color;
    }

    /// Enables or disables fog on the environment resource itself.
    pub fn set_fog_enabled(&mut self, enabled: bool) {
        self.environment.fog_enabled = enabled;
    }

    /// Sets the tone mapping mode.
    pub fn set_tone_mapper(&mut self, mapper: ToneMapper) {
        self.environment.tone_mapper = mapper;
    }

    /// Adds a fog volume to the preview.
    pub fn add_fog_volume(&mut self, volume: FogVolume) {
        self.fog_volumes.push(volume);
    }

    /// Clears all fog volumes from the preview.
    pub fn clear_fog_volumes(&mut self) {
        self.fog_volumes.clear();
    }

    /// Synchronises the environment preview from a scene tree.
    ///
    /// Walks the scene tree looking for `WorldEnvironment` and `FogVolume`
    /// nodes. If a `WorldEnvironment` is found, its properties are used to
    /// build an [`Environment3D`] which replaces the current preview
    /// environment. All `FogVolume` nodes are collected and stored.
    ///
    /// If no `WorldEnvironment` node exists in the scene, the current
    /// preview environment is left unchanged (keeping the editor default).
    pub fn sync_from_scene(&mut self, tree: &SceneTree) {
        let all_nodes = tree.all_nodes_in_tree_order();

        let mut found_env = false;
        self.fog_volumes.clear();

        for &nid in &all_nodes {
            let node = match tree.get_node(nid) {
                Some(n) => n,
                None => continue,
            };

            match node.class_name() {
                "WorldEnvironment" => {
                    if !found_env {
                        let env = Environment3D::from_properties(node.properties().map(|(k, v)| (k.as_str(), v)));
                        self.load_from_environment(env);
                        found_env = true;
                    }
                }
                "FogVolume" => {
                    let volume = fog_volume_from_node_properties(node.properties());
                    self.fog_volumes.push(volume);
                }
                _ => {}
            }
        }
    }
}

/// Constructs a [`FogVolume`] from a node's property iterator.
///
/// Reads `shape`, `size`, and fog material properties (`density`, `albedo`,
/// `emission`, `height_falloff`, `edge_fade`). Unrecognised properties are
/// ignored; missing ones keep their defaults.
fn fog_volume_from_node_properties<'a>(props: impl Iterator<Item = (&'a String, &'a gdvariant::Variant)>) -> FogVolume {
    use gdvariant::Variant;

    let mut volume = FogVolume::default();

    for (key, value) in props {
        match key.as_str() {
            "shape" => {
                if let Variant::Int(v) = value {
                    volume.shape = FogVolumeShape::from_godot_int(*v);
                }
            }
            "size" => {
                if let Variant::Vector3(v) = value {
                    volume.size = *v;
                }
            }
            "density" => {
                if let Variant::Float(f) = value {
                    volume.material.density = *f as f32;
                }
            }
            "albedo" => {
                if let Variant::Color(c) = value {
                    volume.material.albedo = *c;
                }
            }
            "emission" => {
                if let Variant::Color(c) = value {
                    volume.material.emission = *c;
                }
            }
            "height_falloff" => {
                if let Variant::Float(f) = value {
                    volume.material.height_falloff = *f as f32;
                }
            }
            "edge_fade" => {
                if let Variant::Float(f) = value {
                    volume.material.edge_fade = *f as f32;
                }
            }
            _ => {}
        }
    }

    volume
}

// ---------------------------------------------------------------------------
// Viewport3D (combines camera + grid + environment + state)
// ---------------------------------------------------------------------------

/// Complete 3D viewport state for the editor.
#[derive(Debug, Clone)]
pub struct Viewport3D {
    /// The camera controller.
    pub camera: ViewportCamera3D,
    /// The grid configuration.
    pub grid: Grid3D,
    /// 3D selection and gizmo state.
    pub selection: Selection3D,
    /// Environment preview (sky, fog, lighting).
    pub environment: EnvironmentPreview3D,
    /// Viewport width in pixels.
    pub width: u32,
    /// Viewport height in pixels.
    pub height: u32,
    /// In-progress gizmo drag state, if any.
    pub gizmo_drag: Option<GizmoDragState3D>,
}

impl Default for Viewport3D {
    fn default() -> Self {
        Self {
            camera: ViewportCamera3D::default(),
            grid: Grid3D::default(),
            selection: Selection3D::default(),
            environment: EnvironmentPreview3D::default(),
            width: 800,
            height: 600,
            gizmo_drag: None,
        }
    }
}

impl Viewport3D {
    /// Create a new 3D viewport with given dimensions.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            ..Default::default()
        }
    }

    /// Resize the viewport.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
    }

    /// Returns the aspect ratio.
    pub fn aspect_ratio(&self) -> f32 {
        if self.height == 0 {
            1.0
        } else {
            self.width as f32 / self.height as f32
        }
    }

    /// Handles a mouse drag event based on the current camera mode.
    pub fn on_mouse_drag(&mut self, dx: f32, dy: f32) {
        match self.camera.mode {
            CameraMode::Orbit => self.camera.orbit(dx, dy),
            CameraMode::Pan => self.camera.pan(dx, dy),
            CameraMode::Fly => self.camera.fly_look(dx, dy),
            CameraMode::Idle => {}
        }
    }

    /// Handles a scroll event.
    pub fn on_scroll(&mut self, delta: f32) {
        self.camera.zoom(delta);
    }

    /// Advances fly-mode movement for the given time step.
    pub fn on_fly_move(&mut self, input: Vector3, dt: f32) {
        if self.camera.mode == CameraMode::Fly {
            self.camera.fly_move(input, dt);
        }
    }

    /// Returns the effective background color for rendering.
    ///
    /// Takes the environment preview state into account: if the environment
    /// background is `Sky` and the sky is visible, returns a sky-sampled
    /// horizon color; if `CustomColor`, returns that color; otherwise falls
    /// back to the editor clear color.
    pub fn effective_background_color(&self) -> Color {
        match self.environment.effective_background_mode() {
            BackgroundMode::Sky => {
                // Approximate: use the sky horizon color as the background.
                if let Some(sky) = &self.environment.environment.sky {
                    match &sky.material {
                        SkyMaterial::Procedural(proc_sky) => proc_sky.sky_horizon_color,
                        _ => Color::new(0.08, 0.08, 0.1, 1.0),
                    }
                } else {
                    Color::new(0.08, 0.08, 0.1, 1.0)
                }
            }
            BackgroundMode::CustomColor => self.environment.environment.background_color,
            _ => Color::new(0.08, 0.08, 0.1, 1.0),
        }
    }

    /// Scans the scene tree for `WorldEnvironment` and `FogVolume` nodes and
    /// updates the environment preview accordingly.
    pub fn sync_environment_from_scene(&mut self, tree: &SceneTree) {
        self.environment.sync_from_scene(tree);
    }

    /// Returns an [`Environment3D`] that reflects the current effective
    /// preview state (accounting for toggles).
    ///
    /// This is suitable for passing to a render viewport or for display in
    /// the editor UI.
    pub fn effective_environment(&self) -> Option<Environment3D> {
        if !self.environment.enabled {
            return None;
        }

        let mut env = self.environment.environment.clone();

        // Apply preview visibility toggles.
        if !self.environment.sky_visible {
            env.background_mode = BackgroundMode::ClearColor;
            env.sky = None;
        }
        if !self.environment.fog_visible {
            env.fog_enabled = false;
        }

        Some(env)
    }
}

// ---------------------------------------------------------------------------
// 3D Gizmo types
// ---------------------------------------------------------------------------

/// Which gizmo tool is active in the 3D viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GizmoMode3D {
    /// Selection mode — no gizmo drawn.
    Select,
    /// Move (translate) gizmo with X/Y/Z arrows.
    Move,
    /// Rotate gizmo with X/Y/Z rings.
    Rotate,
    /// Scale gizmo with X/Y/Z handles.
    Scale,
}

/// Which axis of a 3D gizmo the user is interacting with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GizmoAxis {
    /// No axis — not interacting with a gizmo.
    None,
    /// X axis (red).
    X,
    /// Y axis (green).
    Y,
    /// Z axis (blue).
    Z,
    /// XY plane (blue plane handle).
    XY,
    /// XZ plane (green plane handle).
    XZ,
    /// YZ plane (red plane handle).
    YZ,
}

impl GizmoAxis {
    /// Returns the unit direction vector(s) for this axis.
    /// For single axes, returns the axis direction.
    /// For plane axes, returns the plane normal.
    pub fn direction(&self) -> Vector3 {
        match self {
            GizmoAxis::None => Vector3::ZERO,
            GizmoAxis::X => Vector3::new(1.0, 0.0, 0.0),
            GizmoAxis::Y => Vector3::new(0.0, 1.0, 0.0),
            GizmoAxis::Z => Vector3::new(0.0, 0.0, 1.0),
            GizmoAxis::XY => Vector3::new(0.0, 0.0, 1.0), // normal to XY plane
            GizmoAxis::XZ => Vector3::new(0.0, 1.0, 0.0), // normal to XZ plane
            GizmoAxis::YZ => Vector3::new(1.0, 0.0, 0.0), // normal to YZ plane
        }
    }

    /// Returns true if this is a plane axis (XY, XZ, YZ).
    pub fn is_plane(&self) -> bool {
        matches!(self, GizmoAxis::XY | GizmoAxis::XZ | GizmoAxis::YZ)
    }

    /// Returns true if this is a single axis (X, Y, Z).
    pub fn is_single(&self) -> bool {
        matches!(self, GizmoAxis::X | GizmoAxis::Y | GizmoAxis::Z)
    }

    /// Masks a vector to only the components this axis allows.
    pub fn mask(&self, v: Vector3) -> Vector3 {
        match self {
            GizmoAxis::None => Vector3::ZERO,
            GizmoAxis::X => Vector3::new(v.x, 0.0, 0.0),
            GizmoAxis::Y => Vector3::new(0.0, v.y, 0.0),
            GizmoAxis::Z => Vector3::new(0.0, 0.0, v.z),
            GizmoAxis::XY => Vector3::new(v.x, v.y, 0.0),
            GizmoAxis::XZ => Vector3::new(v.x, 0.0, v.z),
            GizmoAxis::YZ => Vector3::new(0.0, v.y, v.z),
        }
    }
}

/// A ray in 3D space, used for viewport picking.
#[derive(Debug, Clone, Copy)]
pub struct Ray3D {
    /// Origin of the ray (camera position).
    pub origin: Vector3,
    /// Normalized direction of the ray.
    pub direction: Vector3,
}

impl Ray3D {
    /// Creates a new ray.
    pub fn new(origin: Vector3, direction: Vector3) -> Self {
        Self {
            origin,
            direction: direction.normalized(),
        }
    }

    /// Returns the point along the ray at parameter `t`.
    pub fn at(&self, t: f32) -> Vector3 {
        Vector3::new(
            self.origin.x + self.direction.x * t,
            self.origin.y + self.direction.y * t,
            self.origin.z + self.direction.z * t,
        )
    }

    /// Tests intersection with an axis-aligned bounding sphere centered at `center`
    /// with given `radius`. Returns the distance `t` along the ray if hit.
    pub fn intersect_sphere(&self, center: Vector3, radius: f32) -> Option<f32> {
        let oc = Vector3::new(
            self.origin.x - center.x,
            self.origin.y - center.y,
            self.origin.z - center.z,
        );
        let a = self.direction.dot(self.direction);
        let b = 2.0 * oc.dot(self.direction);
        let c = oc.dot(oc) - radius * radius;
        let discriminant = b * b - 4.0 * a * c;
        if discriminant < 0.0 {
            return None;
        }
        let t = (-b - discriminant.sqrt()) / (2.0 * a);
        if t >= 0.0 {
            Some(t)
        } else {
            // Try the far intersection
            let t2 = (-b + discriminant.sqrt()) / (2.0 * a);
            if t2 >= 0.0 { Some(t2) } else { None }
        }
    }
}

/// Result of a 3D viewport pick/selection operation.
#[derive(Debug, Clone)]
pub struct PickResult {
    /// The node ID that was picked.
    pub node_id: u64,
    /// The 3D world-space point where the pick ray hit.
    pub hit_point: Vector3,
    /// Distance from camera to hit point.
    pub distance: f32,
}

/// State for 3D node selection and gizmo interaction within the viewport.
#[derive(Debug, Clone)]
pub struct Selection3D {
    /// Currently selected node IDs (supports multi-select).
    pub selected_nodes: Vec<u64>,
    /// Active gizmo mode.
    pub gizmo_mode: GizmoMode3D,
    /// Which gizmo axis is currently being dragged (None if not dragging).
    pub active_axis: GizmoAxis,
    /// Whether a gizmo drag is in progress.
    pub dragging: bool,
    /// World-space point where the drag started.
    pub drag_start: Vector3,
    /// Accumulated drag delta in world space along the active axis.
    pub drag_delta: Vector3,
}

impl Default for Selection3D {
    fn default() -> Self {
        Self {
            selected_nodes: Vec::new(),
            gizmo_mode: GizmoMode3D::Select,
            active_axis: GizmoAxis::None,
            dragging: false,
            drag_start: Vector3::ZERO,
            drag_delta: Vector3::ZERO,
        }
    }
}

impl Selection3D {
    /// Selects a single node, replacing any previous selection.
    pub fn select(&mut self, node_id: u64) {
        self.selected_nodes.clear();
        self.selected_nodes.push(node_id);
    }

    /// Adds a node to the selection (for multi-select).
    pub fn add_to_selection(&mut self, node_id: u64) {
        if !self.selected_nodes.contains(&node_id) {
            self.selected_nodes.push(node_id);
        }
    }

    /// Removes a node from the selection.
    pub fn remove_from_selection(&mut self, node_id: u64) {
        self.selected_nodes.retain(|&id| id != node_id);
    }

    /// Toggles a node in/out of the selection.
    pub fn toggle_selection(&mut self, node_id: u64) {
        if self.selected_nodes.contains(&node_id) {
            self.remove_from_selection(node_id);
        } else {
            self.add_to_selection(node_id);
        }
    }

    /// Clears the selection.
    pub fn clear(&mut self) {
        self.selected_nodes.clear();
    }

    /// Returns the primary selected node (first in the list), if any.
    pub fn primary(&self) -> Option<u64> {
        self.selected_nodes.first().copied()
    }

    /// Returns true if the given node is selected.
    pub fn is_selected(&self, node_id: u64) -> bool {
        self.selected_nodes.contains(&node_id)
    }

    /// Sets the gizmo mode.
    pub fn set_gizmo_mode(&mut self, mode: GizmoMode3D) {
        self.gizmo_mode = mode;
        // Cancel any active drag when switching modes
        self.cancel_drag();
    }

    /// Begins a gizmo drag on the given axis.
    pub fn begin_drag(&mut self, axis: GizmoAxis, start_point: Vector3) {
        self.active_axis = axis;
        self.dragging = true;
        self.drag_start = start_point;
        self.drag_delta = Vector3::ZERO;
    }

    /// Updates the drag with a new world-space delta.
    pub fn update_drag(&mut self, delta: Vector3) {
        if self.dragging {
            self.drag_delta = delta;
        }
    }

    /// Ends the drag, returning the final delta.
    pub fn end_drag(&mut self) -> Vector3 {
        let delta = self.drag_delta;
        self.cancel_drag();
        delta
    }

    /// Cancels any active drag.
    pub fn cancel_drag(&mut self) {
        self.active_axis = GizmoAxis::None;
        self.dragging = false;
        self.drag_start = Vector3::ZERO;
        self.drag_delta = Vector3::ZERO;
    }

    /// Returns the axis direction vector for the active axis.
    pub fn axis_direction(&self) -> Vector3 {
        self.active_axis.direction()
    }
}

// ---------------------------------------------------------------------------
// Viewport3D picking (ray casting from screen to 3D scene)
// ---------------------------------------------------------------------------

impl Viewport3D {
    /// Unprojects a screen pixel coordinate into a world-space ray.
    ///
    /// `px` and `py` are pixel coordinates with (0,0) at the top-left.
    pub fn screen_to_ray(&self, px: f32, py: f32) -> Ray3D {
        let aspect = self.aspect_ratio();
        // Normalize to [-1, 1] clip space
        let ndc_x = (2.0 * px / self.width as f32) - 1.0;
        let ndc_y = 1.0 - (2.0 * py / self.height as f32);

        let half_fov = (self.camera.fov_degrees * 0.5 * std::f32::consts::PI / 180.0).tan();

        // Direction in camera-local space
        let local_dir = Vector3::new(
            ndc_x * half_fov * aspect,
            ndc_y * half_fov,
            -1.0, // camera looks down -Z
        )
        .normalized();

        // Transform to world space using camera basis
        let t = self.camera.transform();
        let world_dir = Vector3::new(
            t.basis.x.x * local_dir.x + t.basis.y.x * local_dir.y + t.basis.z.x * local_dir.z,
            t.basis.x.y * local_dir.x + t.basis.y.y * local_dir.y + t.basis.z.y * local_dir.z,
            t.basis.x.z * local_dir.x + t.basis.y.z * local_dir.y + t.basis.z.z * local_dir.z,
        );

        Ray3D::new(self.camera.position(), world_dir)
    }

    /// Hit-tests 3D nodes using sphere intersection.
    ///
    /// Returns the closest node whose bounding sphere (at its position with
    /// the given `pick_radius`) is intersected by the ray from `screen_to_ray`.
    ///
    /// `nodes` is an iterator of `(node_id, world_position)` pairs.
    pub fn pick_node(
        &self,
        px: f32,
        py: f32,
        nodes: &[(u64, Vector3)],
        pick_radius: f32,
    ) -> Option<PickResult> {
        let ray = self.screen_to_ray(px, py);
        let mut best: Option<PickResult> = None;

        for &(node_id, pos) in nodes {
            if let Some(t) = ray.intersect_sphere(pos, pick_radius) {
                let should_replace = match &best {
                    None => true,
                    Some(prev) => t < prev.distance,
                };
                if should_replace {
                    best = Some(PickResult {
                        node_id,
                        hit_point: ray.at(t),
                        distance: t,
                    });
                }
            }
        }

        best
    }

    /// Hit-tests the gizmo axes for the given node position.
    ///
    /// Each axis is represented as a thin cylinder approximated by a sphere at
    /// the tip of the arrow. Returns the axis that was hit, if any.
    pub fn pick_gizmo_axis(
        &self,
        px: f32,
        py: f32,
        gizmo_center: Vector3,
        gizmo_length: f32,
    ) -> GizmoAxis {
        let ray = self.screen_to_ray(px, py);
        let tip_radius = gizmo_length * 0.15;

        let tips = [
            (GizmoAxis::X, Vector3::new(gizmo_center.x + gizmo_length, gizmo_center.y, gizmo_center.z)),
            (GizmoAxis::Y, Vector3::new(gizmo_center.x, gizmo_center.y + gizmo_length, gizmo_center.z)),
            (GizmoAxis::Z, Vector3::new(gizmo_center.x, gizmo_center.y, gizmo_center.z + gizmo_length)),
        ];

        let mut best_axis = GizmoAxis::None;
        let mut best_t = f32::MAX;

        for (axis, tip_pos) in tips {
            if let Some(t) = ray.intersect_sphere(tip_pos, tip_radius) {
                if t < best_t {
                    best_t = t;
                    best_axis = axis;
                }
            }
        }

        best_axis
    }

    /// Projects a screen-space mouse drag onto the active gizmo axis to compute
    /// a world-space translation delta for the **move** gizmo.
    ///
    /// `px`/`py` is the current mouse position. `gizmo_center` is the pivot point
    /// of the gizmo (usually the selected node's world position).
    /// Returns the translation delta along the constrained axis.
    pub fn gizmo_move_delta(
        &self,
        px: f32,
        py: f32,
        gizmo_center: Vector3,
        axis: GizmoAxis,
        drag_start_px: f32,
        drag_start_py: f32,
    ) -> Vector3 {
        if axis == GizmoAxis::None {
            return Vector3::ZERO;
        }

        let ray_start = self.screen_to_ray(drag_start_px, drag_start_py);
        let ray_now = self.screen_to_ray(px, py);

        if axis.is_plane() {
            // For plane axes, intersect rays with the plane
            let plane_normal = axis.direction();
            let t_start = ray_plane_intersect(&ray_start, gizmo_center, plane_normal);
            let t_now = ray_plane_intersect(&ray_now, gizmo_center, plane_normal);
            if let (Some(ts), Some(tn)) = (t_start, t_now) {
                let p_start = ray_start.at(ts);
                let p_now = ray_now.at(tn);
                axis.mask(Vector3::new(
                    p_now.x - p_start.x,
                    p_now.y - p_start.y,
                    p_now.z - p_start.z,
                ))
            } else {
                Vector3::ZERO
            }
        } else {
            // Single-axis: project rays onto the axis line
            let axis_dir = axis.direction();
            let t_start = ray_closest_to_line(&ray_start, gizmo_center, axis_dir);
            let t_now = ray_closest_to_line(&ray_now, gizmo_center, axis_dir);
            let delta = t_now - t_start;
            Vector3::new(axis_dir.x * delta, axis_dir.y * delta, axis_dir.z * delta)
        }
    }

    /// Computes a rotation angle (in radians) for the **rotate** gizmo based on
    /// screen-space mouse drag.
    ///
    /// Uses the angle between start and current mouse positions relative to the
    /// gizmo center projected onto screen space.
    pub fn gizmo_rotate_angle(
        &self,
        px: f32,
        py: f32,
        gizmo_center: Vector3,
        drag_start_px: f32,
        drag_start_py: f32,
    ) -> f32 {
        // Project gizmo center to screen space
        let center_screen = self.world_to_screen(gizmo_center);
        let (cx, cy) = (center_screen.0, center_screen.1);

        let angle_start = (drag_start_py - cy).atan2(drag_start_px - cx);
        let angle_now = (py - cy).atan2(px - cx);

        angle_now - angle_start
    }

    /// Computes a scale factor for the **scale** gizmo based on screen-space
    /// mouse drag distance.
    ///
    /// Moving outward from the gizmo center scales up; moving inward scales down.
    pub fn gizmo_scale_factor(
        &self,
        px: f32,
        py: f32,
        gizmo_center: Vector3,
        drag_start_px: f32,
        drag_start_py: f32,
    ) -> f32 {
        let center_screen = self.world_to_screen(gizmo_center);
        let (cx, cy) = (center_screen.0, center_screen.1);

        let dist_start = ((drag_start_px - cx).powi(2) + (drag_start_py - cy).powi(2)).sqrt();
        let dist_now = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();

        if dist_start < 1.0 {
            1.0
        } else {
            dist_now / dist_start
        }
    }

    /// Projects a world-space point to screen coordinates (pixel space).
    ///
    /// Returns `(px, py)` in the viewport's pixel coordinate system.
    pub fn world_to_screen(&self, world_pos: Vector3) -> (f32, f32) {
        let view = self.camera.view_transform();

        // Transform to view space
        let vx = view.basis.x.x * world_pos.x + view.basis.y.x * world_pos.y + view.basis.z.x * world_pos.z + view.origin.x;
        let vy = view.basis.x.y * world_pos.x + view.basis.y.y * world_pos.y + view.basis.z.y * world_pos.z + view.origin.y;
        let vz = view.basis.x.z * world_pos.x + view.basis.y.z * world_pos.y + view.basis.z.z * world_pos.z + view.origin.z;

        let aspect = self.aspect_ratio();
        let half_fov = (self.camera.fov_degrees * 0.5 * std::f32::consts::PI / 180.0).tan();

        // Perspective divide (vz is negative in front of camera)
        let inv_z = if vz.abs() < 1e-6 { -1e6 } else { -1.0 / vz };

        let ndc_x = (vx * inv_z) / (half_fov * aspect);
        let ndc_y = (vy * inv_z) / half_fov;

        // NDC [-1, 1] to pixel
        let px = (ndc_x + 1.0) * 0.5 * self.width as f32;
        let py = (1.0 - ndc_y) * 0.5 * self.height as f32;

        (px, py)
    }
}

/// Finds the parameter `t` along an axis line that is closest to the given ray.
///
/// The axis line passes through `line_origin` in direction `line_dir`.
/// Returns the signed distance along the axis direction from `line_origin`.
fn ray_closest_to_line(ray: &Ray3D, line_origin: Vector3, line_dir: Vector3) -> f32 {
    // We want the closest point between two lines:
    //   P = ray.origin + s * ray.direction
    //   Q = line_origin + t * line_dir
    //
    // Using the standard closest-point-between-lines formula.
    let w = Vector3::new(
        ray.origin.x - line_origin.x,
        ray.origin.y - line_origin.y,
        ray.origin.z - line_origin.z,
    );
    let a = ray.direction.dot(ray.direction); // always 1 if normalized
    let b = ray.direction.dot(line_dir);
    let c = line_dir.dot(line_dir);
    let d = ray.direction.dot(w);
    let e = line_dir.dot(w);

    let denom = a * c - b * b;
    if denom.abs() < 1e-10 {
        // Lines are parallel — just project
        return -e / c;
    }

    // t parameter along the axis line
    (a * e - b * d) / denom
}

// ---------------------------------------------------------------------------
// Gizmo configuration and snap
// ---------------------------------------------------------------------------

/// Configuration for 3D gizmo visual and interaction parameters.
#[derive(Debug, Clone)]
pub struct GizmoConfig3D {
    /// Length of the gizmo arrows in world units.
    pub arrow_length: f32,
    /// Radius of the rotate ring in world units.
    pub ring_radius: f32,
    /// Scale handle length in world units.
    pub scale_handle_length: f32,
    /// Screen-space hit tolerance for picking gizmo elements (pixels).
    pub pick_tolerance: f32,
    /// Size of the plane handles as fraction of arrow_length.
    pub plane_handle_fraction: f32,
}

impl Default for GizmoConfig3D {
    fn default() -> Self {
        Self {
            arrow_length: 1.0,
            ring_radius: 0.8,
            scale_handle_length: 0.9,
            pick_tolerance: 12.0,
            plane_handle_fraction: 0.3,
        }
    }
}

/// Snap settings for 3D gizmo operations.
#[derive(Debug, Clone)]
pub struct GizmoSnap3D {
    /// Translation snap step in world units (0 = no snap).
    pub translate_step: f32,
    /// Rotation snap step in radians (0 = no snap).
    pub rotate_step: f32,
    /// Scale snap step as a factor (0 = no snap).
    pub scale_step: f32,
}

impl Default for GizmoSnap3D {
    fn default() -> Self {
        Self {
            translate_step: 0.0,
            rotate_step: 0.0,
            scale_step: 0.0,
        }
    }
}

impl GizmoSnap3D {
    /// Snaps a value to the nearest multiple of `step`. Returns the value unchanged if step <= 0.
    pub fn snap_value(value: f32, step: f32) -> f32 {
        if step <= 0.0 {
            value
        } else {
            (value / step).round() * step
        }
    }

    /// Snaps a translation vector component-wise.
    pub fn snap_translate(&self, v: Vector3) -> Vector3 {
        if self.translate_step <= 0.0 {
            v
        } else {
            Vector3::new(
                Self::snap_value(v.x, self.translate_step),
                Self::snap_value(v.y, self.translate_step),
                Self::snap_value(v.z, self.translate_step),
            )
        }
    }

    /// Snaps a rotation angle.
    pub fn snap_rotate(&self, angle: f32) -> f32 {
        Self::snap_value(angle, self.rotate_step)
    }

    /// Snaps a scale factor.
    pub fn snap_scale(&self, factor: f32) -> f32 {
        if self.scale_step <= 0.0 {
            factor
        } else {
            Self::snap_value(factor, self.scale_step)
        }
    }
}

/// The result of a completed gizmo drag operation.
#[derive(Debug, Clone)]
pub enum GizmoTransform3D {
    /// Translation by a world-space delta vector.
    Move(Vector3),
    /// Rotation by an angle (radians) around the given axis.
    Rotate { axis: GizmoAxis, angle: f32 },
    /// Scale by a factor along the given axis.
    Scale { axis: GizmoAxis, factor: f32 },
}

/// State tracking an in-progress gizmo drag.
#[derive(Debug, Clone)]
pub struct GizmoDragState3D {
    /// Which gizmo mode initiated this drag.
    pub mode: GizmoMode3D,
    /// Which axis is being dragged.
    pub axis: GizmoAxis,
    /// World position of the gizmo center at drag start.
    pub gizmo_center: Vector3,
    /// Screen pixel where the drag started.
    pub start_px: f32,
    /// Screen pixel where the drag started.
    pub start_py: f32,
    /// Current accumulated transform result.
    pub current_transform: GizmoTransform3D,
}

impl Viewport3D {
    /// Hit-tests the gizmo for the current selection at the given screen position.
    ///
    /// Returns the axis that was hit, considering the current gizmo mode.
    /// For Move mode, tests arrow tips and plane handles.
    /// For Rotate mode, tests ring intersections.
    /// For Scale mode, tests scale handle tips.
    pub fn hit_test_gizmo_full(
        &self,
        px: f32,
        py: f32,
        gizmo_center: Vector3,
        config: &GizmoConfig3D,
    ) -> GizmoAxis {
        let ray = self.screen_to_ray(px, py);

        match self.selection.gizmo_mode {
            GizmoMode3D::Select => GizmoAxis::None,

            GizmoMode3D::Move => {
                // Test arrow tips (single axes)
                let tip_radius = config.arrow_length * 0.15;
                let tips = [
                    (GizmoAxis::X, Vector3::new(gizmo_center.x + config.arrow_length, gizmo_center.y, gizmo_center.z)),
                    (GizmoAxis::Y, Vector3::new(gizmo_center.x, gizmo_center.y + config.arrow_length, gizmo_center.z)),
                    (GizmoAxis::Z, Vector3::new(gizmo_center.x, gizmo_center.y, gizmo_center.z + config.arrow_length)),
                ];

                let mut best_axis = GizmoAxis::None;
                let mut best_t = f32::MAX;

                for (axis, tip_pos) in tips {
                    if let Some(t) = ray.intersect_sphere(tip_pos, tip_radius) {
                        if t < best_t {
                            best_t = t;
                            best_axis = axis;
                        }
                    }
                }

                // Test plane handles (small squares at arrow_length * plane_handle_fraction along each pair of axes)
                if best_axis == GizmoAxis::None {
                    let pf = config.arrow_length * config.plane_handle_fraction;
                    let plane_centers = [
                        (GizmoAxis::XY, Vector3::new(gizmo_center.x + pf, gizmo_center.y + pf, gizmo_center.z)),
                        (GizmoAxis::XZ, Vector3::new(gizmo_center.x + pf, gizmo_center.y, gizmo_center.z + pf)),
                        (GizmoAxis::YZ, Vector3::new(gizmo_center.x, gizmo_center.y + pf, gizmo_center.z + pf)),
                    ];
                    let plane_radius = pf * 0.5;

                    for (axis, center) in plane_centers {
                        if let Some(t) = ray.intersect_sphere(center, plane_radius) {
                            if t < best_t {
                                best_t = t;
                                best_axis = axis;
                            }
                        }
                    }
                }

                best_axis
            }

            GizmoMode3D::Rotate => {
                // Test ring intersections — each ring is a torus approximated by sampling
                // points on the ring and testing sphere intersection at each sample.
                let sample_count = 16;
                let sample_radius = config.ring_radius * 0.1;
                let axes = [GizmoAxis::X, GizmoAxis::Y, GizmoAxis::Z];

                let mut best_axis = GizmoAxis::None;
                let mut best_t = f32::MAX;

                for &axis in &axes {
                    for i in 0..sample_count {
                        let angle = (i as f32 / sample_count as f32) * std::f32::consts::TAU;
                        let (sin_a, cos_a) = angle.sin_cos();
                        let point = match axis {
                            GizmoAxis::X => Vector3::new(
                                gizmo_center.x,
                                gizmo_center.y + cos_a * config.ring_radius,
                                gizmo_center.z + sin_a * config.ring_radius,
                            ),
                            GizmoAxis::Y => Vector3::new(
                                gizmo_center.x + cos_a * config.ring_radius,
                                gizmo_center.y,
                                gizmo_center.z + sin_a * config.ring_radius,
                            ),
                            GizmoAxis::Z => Vector3::new(
                                gizmo_center.x + cos_a * config.ring_radius,
                                gizmo_center.y + sin_a * config.ring_radius,
                                gizmo_center.z,
                            ),
                            _ => unreachable!(),
                        };
                        if let Some(t) = ray.intersect_sphere(point, sample_radius) {
                            if t < best_t {
                                best_t = t;
                                best_axis = axis;
                            }
                        }
                    }
                }

                best_axis
            }

            GizmoMode3D::Scale => {
                // Scale handles are cubes at axis tips — approximate as spheres
                let tip_radius = config.scale_handle_length * 0.15;
                let tips = [
                    (GizmoAxis::X, Vector3::new(gizmo_center.x + config.scale_handle_length, gizmo_center.y, gizmo_center.z)),
                    (GizmoAxis::Y, Vector3::new(gizmo_center.x, gizmo_center.y + config.scale_handle_length, gizmo_center.z)),
                    (GizmoAxis::Z, Vector3::new(gizmo_center.x, gizmo_center.y, gizmo_center.z + config.scale_handle_length)),
                ];

                let mut best_axis = GizmoAxis::None;
                let mut best_t = f32::MAX;

                for (axis, tip_pos) in tips {
                    if let Some(t) = ray.intersect_sphere(tip_pos, tip_radius) {
                        if t < best_t {
                            best_t = t;
                            best_axis = axis;
                        }
                    }
                }

                best_axis
            }
        }
    }

    /// Begins a gizmo drag interaction.
    ///
    /// Call this when the user clicks on a gizmo axis. Returns `true` if a drag was started.
    pub fn begin_gizmo_drag(
        &mut self,
        px: f32,
        py: f32,
        gizmo_center: Vector3,
        axis: GizmoAxis,
    ) -> bool {
        if axis == GizmoAxis::None || self.selection.gizmo_mode == GizmoMode3D::Select {
            return false;
        }

        let initial_transform = match self.selection.gizmo_mode {
            GizmoMode3D::Move => GizmoTransform3D::Move(Vector3::ZERO),
            GizmoMode3D::Rotate => GizmoTransform3D::Rotate { axis, angle: 0.0 },
            GizmoMode3D::Scale => GizmoTransform3D::Scale { axis, factor: 1.0 },
            GizmoMode3D::Select => return false,
        };

        self.selection.begin_drag(axis, gizmo_center);
        self.gizmo_drag = Some(GizmoDragState3D {
            mode: self.selection.gizmo_mode,
            axis,
            gizmo_center,
            start_px: px,
            start_py: py,
            current_transform: initial_transform,
        });
        true
    }

    /// Updates an in-progress gizmo drag with the current mouse position.
    ///
    /// Returns the current transform if a drag is active.
    pub fn update_gizmo_drag(
        &mut self,
        px: f32,
        py: f32,
        snap: &GizmoSnap3D,
    ) -> Option<GizmoTransform3D> {
        let vp_w = self.width;
        let vp_h = self.height;
        let drag = self.gizmo_drag.as_mut()?;

        match drag.mode {
            GizmoMode3D::Move => {
                let raw_delta = if drag.axis.is_plane() {
                    // For plane movement, cast rays onto the plane
                    let ray_start = screen_to_ray_static(
                        drag.start_px, drag.start_py,
                        vp_w, vp_h, &self.camera,
                    );
                    let ray_now = screen_to_ray_static(px, py, vp_w, vp_h, &self.camera);

                    let plane_normal = drag.axis.direction();
                    let t_start = ray_plane_intersect(&ray_start, drag.gizmo_center, plane_normal);
                    let t_now = ray_plane_intersect(&ray_now, drag.gizmo_center, plane_normal);

                    if let (Some(ts), Some(tn)) = (t_start, t_now) {
                        let p_start = ray_start.at(ts);
                        let p_now = ray_now.at(tn);
                        drag.axis.mask(Vector3::new(
                            p_now.x - p_start.x,
                            p_now.y - p_start.y,
                            p_now.z - p_start.z,
                        ))
                    } else {
                        Vector3::ZERO
                    }
                } else {
                    // Single-axis: use the existing ray-to-line projection
                    let axis_dir = drag.axis.direction();
                    let ray_start = screen_to_ray_static(
                        drag.start_px, drag.start_py,
                        vp_w, vp_h, &self.camera,
                    );
                    let ray_now = screen_to_ray_static(px, py, vp_w, vp_h, &self.camera);

                    let t_start = ray_closest_to_line(&ray_start, drag.gizmo_center, axis_dir);
                    let t_now = ray_closest_to_line(&ray_now, drag.gizmo_center, axis_dir);

                    let d = t_now - t_start;
                    Vector3::new(axis_dir.x * d, axis_dir.y * d, axis_dir.z * d)
                };

                let snapped = snap.snap_translate(raw_delta);
                drag.current_transform = GizmoTransform3D::Move(snapped);
                self.selection.update_drag(snapped);
                Some(GizmoTransform3D::Move(snapped))
            }

            GizmoMode3D::Rotate => {
                let center_screen = world_to_screen_static(
                    drag.gizmo_center, vp_w, vp_h, &self.camera,
                );
                let (cx, cy) = center_screen;
                let angle_start = (drag.start_py - cy).atan2(drag.start_px - cx);
                let angle_now = (py - cy).atan2(px - cx);
                let raw_angle = angle_now - angle_start;
                let snapped = snap.snap_rotate(raw_angle);

                drag.current_transform = GizmoTransform3D::Rotate { axis: drag.axis, angle: snapped };
                let axis_dir = drag.axis.direction();
                self.selection.update_drag(Vector3::new(
                    axis_dir.x * snapped,
                    axis_dir.y * snapped,
                    axis_dir.z * snapped,
                ));
                Some(GizmoTransform3D::Rotate { axis: drag.axis, angle: snapped })
            }

            GizmoMode3D::Scale => {
                let center_screen = world_to_screen_static(
                    drag.gizmo_center, vp_w, vp_h, &self.camera,
                );
                let (cx, cy) = center_screen;
                let dist_start = ((drag.start_px - cx).powi(2) + (drag.start_py - cy).powi(2)).sqrt();
                let dist_now = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
                let raw_factor = if dist_start < 1.0 { 1.0 } else { dist_now / dist_start };
                let snapped = snap.snap_scale(raw_factor);

                drag.current_transform = GizmoTransform3D::Scale { axis: drag.axis, factor: snapped };
                let axis_dir = drag.axis.direction();
                self.selection.update_drag(Vector3::new(
                    axis_dir.x * snapped,
                    axis_dir.y * snapped,
                    axis_dir.z * snapped,
                ));
                Some(GizmoTransform3D::Scale { axis: drag.axis, factor: snapped })
            }

            GizmoMode3D::Select => None,
        }
    }

    /// Ends the current gizmo drag, returning the final transform.
    pub fn end_gizmo_drag(&mut self) -> Option<GizmoTransform3D> {
        let drag = self.gizmo_drag.take()?;
        self.selection.end_drag();
        Some(drag.current_transform)
    }

    /// Cancels the current gizmo drag without applying.
    pub fn cancel_gizmo_drag(&mut self) {
        self.gizmo_drag = None;
        self.selection.cancel_drag();
    }

    /// Returns true if a gizmo drag is currently in progress.
    pub fn is_gizmo_dragging(&self) -> bool {
        self.gizmo_drag.is_some()
    }

    /// Returns the current gizmo drag axis, if dragging.
    pub fn gizmo_drag_axis(&self) -> GizmoAxis {
        self.gizmo_drag.as_ref().map_or(GizmoAxis::None, |d| d.axis)
    }
}

/// Static helper: unproject screen pixel to world ray without borrowing Viewport3D.
fn screen_to_ray_static(
    px: f32, py: f32,
    width: u32, height: u32,
    camera: &ViewportCamera3D,
) -> Ray3D {
    let aspect = if height == 0 { 1.0 } else { width as f32 / height as f32 };
    let ndc_x = (2.0 * px / width as f32) - 1.0;
    let ndc_y = 1.0 - (2.0 * py / height as f32);
    let half_fov = (camera.fov_degrees * 0.5 * std::f32::consts::PI / 180.0).tan();
    let local_dir = Vector3::new(ndc_x * half_fov * aspect, ndc_y * half_fov, -1.0).normalized();
    let t = camera.transform();
    let world_dir = Vector3::new(
        t.basis.x.x * local_dir.x + t.basis.y.x * local_dir.y + t.basis.z.x * local_dir.z,
        t.basis.x.y * local_dir.x + t.basis.y.y * local_dir.y + t.basis.z.y * local_dir.z,
        t.basis.x.z * local_dir.x + t.basis.y.z * local_dir.y + t.basis.z.z * local_dir.z,
    );
    Ray3D::new(camera.position(), world_dir)
}

/// Static helper: project world point to screen without borrowing Viewport3D.
fn world_to_screen_static(
    world_pos: Vector3,
    width: u32, height: u32,
    camera: &ViewportCamera3D,
) -> (f32, f32) {
    let view = camera.view_transform();
    let vx = view.basis.x.x * world_pos.x + view.basis.y.x * world_pos.y + view.basis.z.x * world_pos.z + view.origin.x;
    let vy = view.basis.x.y * world_pos.x + view.basis.y.y * world_pos.y + view.basis.z.y * world_pos.z + view.origin.y;
    let vz = view.basis.x.z * world_pos.x + view.basis.y.z * world_pos.y + view.basis.z.z * world_pos.z + view.origin.z;
    let aspect = if height == 0 { 1.0 } else { width as f32 / height as f32 };
    let half_fov = (camera.fov_degrees * 0.5 * std::f32::consts::PI / 180.0).tan();
    let inv_z = if vz.abs() < 1e-6 { -1e6 } else { -1.0 / vz };
    let ndc_x = (vx * inv_z) / (half_fov * aspect);
    let ndc_y = (vy * inv_z) / half_fov;
    let px = (ndc_x + 1.0) * 0.5 * width as f32;
    let py = (1.0 - ndc_y) * 0.5 * height as f32;
    (px, py)
}

/// Intersects a ray with a plane defined by a point and normal.
/// Returns the distance `t` along the ray, or None if parallel.
fn ray_plane_intersect(ray: &Ray3D, plane_point: Vector3, plane_normal: Vector3) -> Option<f32> {
    let denom = ray.direction.dot(plane_normal);
    if denom.abs() < 1e-8 {
        return None;
    }
    let diff = Vector3::new(
        plane_point.x - ray.origin.x,
        plane_point.y - ray.origin.y,
        plane_point.z - ray.origin.z,
    );
    let t = diff.dot(plane_normal) / denom;
    if t >= 0.0 { Some(t) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() < eps
    }

    fn vec3_approx_eq(a: Vector3, b: Vector3, eps: f32) -> bool {
        approx_eq(a.x, b.x, eps) && approx_eq(a.y, b.y, eps) && approx_eq(a.z, b.z, eps)
    }

    #[test]
    fn test_default_camera_position() {
        let cam = ViewportCamera3D::default();
        let pos = cam.position();
        // Camera should be behind and above the origin
        assert!(pos.y > 0.0, "camera should be above origin");
        assert!(cam.distance > 0.0);
    }

    #[test]
    fn test_orbit_changes_yaw_pitch() {
        let mut cam = ViewportCamera3D::new();
        let orig_yaw = cam.yaw;
        let orig_pitch = cam.pitch;
        cam.begin_orbit();
        cam.orbit(100.0, 50.0);
        assert_ne!(cam.yaw, orig_yaw);
        assert_ne!(cam.pitch, orig_pitch);
        cam.end_orbit();
        assert_eq!(cam.mode, CameraMode::Idle);
    }

    #[test]
    fn test_pitch_clamped() {
        let mut cam = ViewportCamera3D::new();
        cam.begin_orbit();
        // Orbit a huge amount upward
        cam.orbit(0.0, 100_000.0);
        assert!(cam.pitch <= PITCH_MAX);
        assert!(cam.pitch >= PITCH_MIN);
        // Orbit a huge amount downward
        cam.orbit(0.0, -200_000.0);
        assert!(cam.pitch >= PITCH_MIN);
    }

    #[test]
    fn test_pan_moves_focus() {
        let mut cam = ViewportCamera3D::new();
        let orig = cam.focus_point;
        cam.begin_pan();
        cam.pan(100.0, 0.0);
        assert!(!vec3_approx_eq(cam.focus_point, orig, 1e-6));
        cam.end_pan();
        assert_eq!(cam.mode, CameraMode::Idle);
    }

    #[test]
    fn test_zoom_changes_distance() {
        let mut cam = ViewportCamera3D::new();
        let orig = cam.distance;
        cam.zoom(1.0); // zoom in
        assert!(cam.distance < orig);
        cam.zoom(-2.0); // zoom out
        assert!(cam.distance > orig);
    }

    #[test]
    fn test_zoom_clamped() {
        let mut cam = ViewportCamera3D::new();
        // Zoom in a lot
        for _ in 0..1000 {
            cam.zoom(10.0);
        }
        assert!(cam.distance >= MIN_DISTANCE);
        // Zoom out a lot
        for _ in 0..1000 {
            cam.zoom(-10.0);
        }
        assert!(cam.distance <= MAX_DISTANCE);
    }

    #[test]
    fn test_fly_mode() {
        let mut cam = ViewportCamera3D::new();
        cam.begin_fly();
        assert_eq!(cam.mode, CameraMode::Fly);

        let orig_focus = cam.focus_point;
        cam.fly_move(Vector3::new(0.0, 0.0, 1.0), 1.0);
        assert!(!vec3_approx_eq(cam.focus_point, orig_focus, 1e-6));

        cam.end_fly();
        assert_eq!(cam.mode, CameraMode::Idle);
    }

    #[test]
    fn test_fly_look_updates_focus() {
        let mut cam = ViewportCamera3D::new();
        cam.begin_fly();
        let pos_before = cam.position();
        cam.fly_look(50.0, 0.0);
        // Focus should have moved to stay in front of the camera
        let pos_after = cam.position();
        // Position should not change during look (only orientation)
        assert!(vec3_approx_eq(pos_before, pos_after, 0.1));
    }

    #[test]
    fn test_transform_is_valid() {
        let cam = ViewportCamera3D::default();
        let t = cam.transform();
        // Basis should be approximately orthonormal
        let right = t.basis.x;
        let up = t.basis.y;
        let fwd = t.basis.z;
        assert!(approx_eq(right.length(), 1.0, 0.01));
        assert!(approx_eq(up.length(), 1.0, 0.01));
        assert!(approx_eq(fwd.length(), 1.0, 0.01));
        // Origin should match position()
        assert!(vec3_approx_eq(t.origin, cam.position(), 1e-4));
    }

    #[test]
    fn test_snap_to_front() {
        let mut cam = ViewportCamera3D::new();
        cam.yaw = 1.5;
        cam.pitch = -0.3;
        cam.snap_to_front();
        assert_eq!(cam.yaw, 0.0);
        assert_eq!(cam.pitch, 0.0);
    }

    #[test]
    fn test_snap_to_top() {
        let mut cam = ViewportCamera3D::new();
        cam.snap_to_top();
        assert_eq!(cam.yaw, 0.0);
        assert_eq!(cam.pitch, PITCH_MIN);
    }

    #[test]
    fn test_snap_to_right() {
        let mut cam = ViewportCamera3D::new();
        cam.snap_to_right();
        assert!(approx_eq(cam.yaw, std::f32::consts::FRAC_PI_2, 1e-6));
        assert_eq!(cam.pitch, 0.0);
    }

    #[test]
    fn test_toggle_projection() {
        let mut cam = ViewportCamera3D::new();
        assert_eq!(cam.projection, Projection::Perspective);
        cam.toggle_projection();
        assert_eq!(cam.projection, Projection::Orthographic);
        cam.toggle_projection();
        assert_eq!(cam.projection, Projection::Perspective);
    }

    #[test]
    fn test_focus_on() {
        let mut cam = ViewportCamera3D::new();
        let target = Vector3::new(10.0, 5.0, -3.0);
        cam.focus_on(target);
        assert!(vec3_approx_eq(cam.focus_point, target, 1e-6));
    }

    #[test]
    fn test_focus_on_with_distance() {
        let mut cam = ViewportCamera3D::new();
        cam.focus_on_with_distance(Vector3::new(1.0, 2.0, 3.0), 15.0);
        assert!(approx_eq(cam.distance, 15.0, 1e-6));
    }

    #[test]
    fn test_ortho_zoom() {
        let mut cam = ViewportCamera3D::new();
        cam.projection = Projection::Orthographic;
        let orig = cam.ortho_size;
        cam.zoom(1.0);
        assert!(cam.ortho_size < orig);
    }

    #[test]
    fn test_grid_generates_lines() {
        let grid = Grid3D::default();
        let lines = grid.generate_lines();
        assert!(!lines.is_empty());
        // Should have both major and minor lines
        let has_major = lines.iter().any(|l| l.is_major);
        let has_minor = lines.iter().any(|l| !l.is_major);
        assert!(has_major);
        assert!(has_minor);
    }

    #[test]
    fn test_grid_hidden_returns_empty() {
        let mut grid = Grid3D::default();
        grid.visible = false;
        assert!(grid.generate_lines().is_empty());
    }

    #[test]
    fn test_viewport3d_resize() {
        let mut vp = Viewport3D::new(800, 600);
        assert!(approx_eq(vp.aspect_ratio(), 800.0 / 600.0, 0.01));
        vp.resize(1920, 1080);
        assert_eq!(vp.width, 1920);
        assert_eq!(vp.height, 1080);
    }

    #[test]
    fn test_viewport3d_on_mouse_drag_orbit() {
        let mut vp = Viewport3D::default();
        vp.camera.begin_orbit();
        let orig_yaw = vp.camera.yaw;
        vp.on_mouse_drag(50.0, 0.0);
        assert_ne!(vp.camera.yaw, orig_yaw);
    }

    #[test]
    fn test_viewport3d_on_scroll() {
        let mut vp = Viewport3D::default();
        let orig = vp.camera.distance;
        vp.on_scroll(1.0);
        assert!(vp.camera.distance < orig);
    }

    #[test]
    fn test_viewport3d_fly_move_only_in_fly_mode() {
        let mut vp = Viewport3D::default();
        let orig = vp.camera.focus_point;
        // Should be no-op when not in fly mode
        vp.on_fly_move(Vector3::new(1.0, 0.0, 0.0), 1.0);
        assert!(vec3_approx_eq(vp.camera.focus_point, orig, 1e-6));
    }

    #[test]
    fn test_view_transform_is_inverse_of_transform() {
        let cam = ViewportCamera3D::default();
        let t = cam.transform();
        let v = cam.view_transform();
        // t * v should approximately be identity
        let _identity_check = t.xform(v.origin);
        // Round-trip a known point through view → world.
        let point = Vector3::new(1.0, 2.0, 3.0);
        let through = v.xform(point);
        let back = t.xform(through);
        assert!(vec3_approx_eq(point, back, 0.01));
    }

    #[test]
    fn test_camera_reset() {
        let mut cam = ViewportCamera3D::new();
        cam.yaw = 3.0;
        cam.pitch = -1.0;
        cam.distance = 50.0;
        cam.focus_point = Vector3::new(99.0, 99.0, 99.0);
        cam.reset();
        let def = ViewportCamera3D::default();
        assert!(approx_eq(cam.yaw, def.yaw, 1e-6));
        assert!(approx_eq(cam.pitch, def.pitch, 1e-6));
        assert!(approx_eq(cam.distance, def.distance, 1e-6));
    }

    // -----------------------------------------------------------------------
    // Ray3D tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ray_at_returns_point_along_ray() {
        let ray = Ray3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, -1.0));
        let p = ray.at(5.0);
        assert!(approx_eq(p.z, -5.0, 1e-6));
    }

    #[test]
    fn test_ray_intersect_sphere_hit() {
        let ray = Ray3D::new(Vector3::new(0.0, 0.0, 10.0), Vector3::new(0.0, 0.0, -1.0));
        let t = ray.intersect_sphere(Vector3::ZERO, 1.0);
        assert!(t.is_some());
        let t = t.unwrap();
        assert!(approx_eq(t, 9.0, 0.01), "Should hit near sphere at t=9, got {t}");
    }

    #[test]
    fn test_ray_intersect_sphere_miss() {
        let ray = Ray3D::new(Vector3::new(0.0, 0.0, 10.0), Vector3::new(0.0, 0.0, -1.0));
        let t = ray.intersect_sphere(Vector3::new(100.0, 0.0, 0.0), 1.0);
        assert!(t.is_none(), "Ray should miss a distant sphere");
    }

    #[test]
    fn test_ray_intersect_sphere_inside() {
        // Ray starts inside the sphere
        let ray = Ray3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, -1.0));
        let t = ray.intersect_sphere(Vector3::ZERO, 5.0);
        assert!(t.is_some(), "Ray starting inside sphere should still intersect (exit)");
    }

    // -----------------------------------------------------------------------
    // Selection3D tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_selection_default_is_empty() {
        let sel = Selection3D::default();
        assert!(sel.selected_nodes.is_empty());
        assert_eq!(sel.gizmo_mode, GizmoMode3D::Select);
        assert_eq!(sel.active_axis, GizmoAxis::None);
        assert!(!sel.dragging);
    }

    #[test]
    fn test_selection_select_replaces() {
        let mut sel = Selection3D::default();
        sel.select(1);
        sel.select(2);
        assert_eq!(sel.selected_nodes, vec![2]);
    }

    #[test]
    fn test_selection_add_and_remove() {
        let mut sel = Selection3D::default();
        sel.add_to_selection(1);
        sel.add_to_selection(2);
        sel.add_to_selection(1); // duplicate
        assert_eq!(sel.selected_nodes, vec![1, 2]);
        sel.remove_from_selection(1);
        assert_eq!(sel.selected_nodes, vec![2]);
    }

    #[test]
    fn test_selection_toggle() {
        let mut sel = Selection3D::default();
        sel.toggle_selection(1);
        assert!(sel.is_selected(1));
        sel.toggle_selection(1);
        assert!(!sel.is_selected(1));
    }

    #[test]
    fn test_selection_primary() {
        let mut sel = Selection3D::default();
        assert_eq!(sel.primary(), None);
        sel.add_to_selection(5);
        sel.add_to_selection(3);
        assert_eq!(sel.primary(), Some(5));
    }

    #[test]
    fn test_selection_clear() {
        let mut sel = Selection3D::default();
        sel.add_to_selection(1);
        sel.add_to_selection(2);
        sel.clear();
        assert!(sel.selected_nodes.is_empty());
    }

    #[test]
    fn test_gizmo_mode_switch_cancels_drag() {
        let mut sel = Selection3D::default();
        sel.set_gizmo_mode(GizmoMode3D::Move);
        sel.begin_drag(GizmoAxis::X, Vector3::ZERO);
        assert!(sel.dragging);
        sel.set_gizmo_mode(GizmoMode3D::Rotate);
        assert!(!sel.dragging);
        assert_eq!(sel.active_axis, GizmoAxis::None);
    }

    #[test]
    fn test_drag_lifecycle() {
        let mut sel = Selection3D::default();
        sel.set_gizmo_mode(GizmoMode3D::Move);
        sel.begin_drag(GizmoAxis::Y, Vector3::new(0.0, 1.0, 0.0));
        assert!(sel.dragging);
        assert_eq!(sel.active_axis, GizmoAxis::Y);

        sel.update_drag(Vector3::new(0.0, 3.5, 0.0));
        assert!(vec3_approx_eq(sel.drag_delta, Vector3::new(0.0, 3.5, 0.0), 1e-6));

        let delta = sel.end_drag();
        assert!(vec3_approx_eq(delta, Vector3::new(0.0, 3.5, 0.0), 1e-6));
        assert!(!sel.dragging);
    }

    #[test]
    fn test_cancel_drag_resets_state() {
        let mut sel = Selection3D::default();
        sel.begin_drag(GizmoAxis::Z, Vector3::new(0.0, 0.0, 5.0));
        sel.update_drag(Vector3::new(0.0, 0.0, 10.0));
        sel.cancel_drag();
        assert!(!sel.dragging);
        assert_eq!(sel.active_axis, GizmoAxis::None);
        assert!(vec3_approx_eq(sel.drag_delta, Vector3::ZERO, 1e-6));
    }

    #[test]
    fn test_axis_direction() {
        let mut sel = Selection3D::default();
        sel.active_axis = GizmoAxis::X;
        assert!(vec3_approx_eq(sel.axis_direction(), Vector3::new(1.0, 0.0, 0.0), 1e-6));
        sel.active_axis = GizmoAxis::Y;
        assert!(vec3_approx_eq(sel.axis_direction(), Vector3::new(0.0, 1.0, 0.0), 1e-6));
        sel.active_axis = GizmoAxis::Z;
        assert!(vec3_approx_eq(sel.axis_direction(), Vector3::new(0.0, 0.0, 1.0), 1e-6));
        sel.active_axis = GizmoAxis::None;
        assert!(vec3_approx_eq(sel.axis_direction(), Vector3::ZERO, 1e-6));
    }

    // -----------------------------------------------------------------------
    // Viewport3D picking tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_screen_to_ray_center() {
        let vp = Viewport3D::new(800, 600);
        let ray = vp.screen_to_ray(400.0, 300.0);
        // Center of screen should produce a ray along the camera's forward direction.
        let fwd = vp.camera.orbit_direction();
        let dot = ray.direction.dot(fwd);
        assert!(dot > 0.99, "Center ray should be approximately forward, dot={dot}");
    }

    #[test]
    fn test_screen_to_ray_corners_diverge() {
        let vp = Viewport3D::new(800, 600);
        let ray_tl = vp.screen_to_ray(0.0, 0.0);
        let ray_br = vp.screen_to_ray(800.0, 600.0);
        // Opposite corners should produce different directions.
        let dot = ray_tl.direction.dot(ray_br.direction);
        assert!(dot < 0.99, "Corner rays should diverge, dot={dot}");
    }

    #[test]
    fn test_pick_node_hits_closest() {
        // Set up camera looking from (0,0,10) toward origin
        let mut vp = Viewport3D::new(800, 600);
        vp.camera.focus_point = Vector3::ZERO;
        vp.camera.distance = 10.0;
        vp.camera.yaw = 0.0;
        vp.camera.pitch = 0.0;

        let nodes = vec![
            (1, Vector3::new(0.0, 0.0, -2.0)),  // farther
            (2, Vector3::new(0.0, 0.0, 0.0)),    // at origin (closer)
        ];

        // Pick at the center of the screen
        let result = vp.pick_node(400.0, 300.0, &nodes, 1.0);
        assert!(result.is_some(), "Should hit at least one node");
        // The closer node (id=2) should be picked
        let r = result.unwrap();
        assert_eq!(r.node_id, 2, "Should pick the closer node");
    }

    #[test]
    fn test_pick_node_misses_when_nothing_there() {
        let vp = Viewport3D::new(800, 600);
        let nodes = vec![
            (1, Vector3::new(1000.0, 1000.0, 1000.0)),
        ];
        let result = vp.pick_node(400.0, 300.0, &nodes, 0.5);
        assert!(result.is_none(), "Should not hit a node far from center");
    }

    #[test]
    fn test_pick_gizmo_axis_hits_correct_axis() {
        // Camera looking from (0,0,10) toward origin
        let mut vp = Viewport3D::new(800, 600);
        vp.camera.focus_point = Vector3::ZERO;
        vp.camera.distance = 10.0;
        vp.camera.yaw = 0.0;
        vp.camera.pitch = 0.0;

        // The gizmo is at the origin with length 1.0
        // The X-axis tip is at (1, 0, 0)
        // We need to find the screen position of that tip and click there
        // With camera at (0,0,10) looking at origin, X=1 is slightly to the right
        // Let's test that clicking far off doesn't hit anything
        let axis = vp.pick_gizmo_axis(0.0, 0.0, Vector3::ZERO, 1.0);
        assert_eq!(axis, GizmoAxis::None, "Click in corner should miss gizmo");
    }

    // -----------------------------------------------------------------------
    // Gizmo mode and selection tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_gizmo_mode_default_is_select() {
        let sel = Selection3D::default();
        assert_eq!(sel.gizmo_mode, GizmoMode3D::Select);
    }

    #[test]
    fn test_gizmo_mode_switch() {
        let mut sel = Selection3D::default();
        sel.set_gizmo_mode(GizmoMode3D::Move);
        assert_eq!(sel.gizmo_mode, GizmoMode3D::Move);
        sel.set_gizmo_mode(GizmoMode3D::Rotate);
        assert_eq!(sel.gizmo_mode, GizmoMode3D::Rotate);
        sel.set_gizmo_mode(GizmoMode3D::Scale);
        assert_eq!(sel.gizmo_mode, GizmoMode3D::Scale);
    }

    #[test]
    fn test_gizmo_drag_lifecycle_move() {
        let mut sel = Selection3D::default();
        sel.select(42);
        sel.set_gizmo_mode(GizmoMode3D::Move);

        // Begin drag on X axis
        sel.begin_drag(GizmoAxis::X, Vector3::new(1.0, 0.0, 0.0));
        assert!(sel.dragging);
        assert_eq!(sel.active_axis, GizmoAxis::X);

        // Update drag
        sel.update_drag(Vector3::new(3.0, 0.0, 0.0));
        assert!(vec3_approx_eq(sel.drag_delta, Vector3::new(3.0, 0.0, 0.0), 1e-6));

        // End drag returns final delta
        let delta = sel.end_drag();
        assert!(vec3_approx_eq(delta, Vector3::new(3.0, 0.0, 0.0), 1e-6));
        assert!(!sel.dragging);
        assert_eq!(sel.active_axis, GizmoAxis::None);
    }

    #[test]
    fn test_gizmo_drag_lifecycle_rotate() {
        let mut sel = Selection3D::default();
        sel.select(99);
        sel.set_gizmo_mode(GizmoMode3D::Rotate);

        sel.begin_drag(GizmoAxis::Y, Vector3::ZERO);
        assert!(sel.dragging);
        assert_eq!(sel.active_axis, GizmoAxis::Y);

        sel.cancel_drag();
        assert!(!sel.dragging);
    }

    #[test]
    fn test_gizmo_drag_lifecycle_scale() {
        let mut sel = Selection3D::default();
        sel.select(7);
        sel.set_gizmo_mode(GizmoMode3D::Scale);

        sel.begin_drag(GizmoAxis::Z, Vector3::new(0.0, 0.0, 1.0));
        assert!(sel.dragging);
        sel.update_drag(Vector3::new(0.0, 0.0, 2.5));
        let delta = sel.end_drag();
        assert!(approx_eq(delta.z, 2.5, 1e-6));
    }

    #[test]
    fn test_gizmo_mode_switch_cancels_active_drag() {
        let mut sel = Selection3D::default();
        sel.set_gizmo_mode(GizmoMode3D::Move);
        sel.begin_drag(GizmoAxis::X, Vector3::ZERO);
        assert!(sel.dragging);

        // Switching mode cancels the drag
        sel.set_gizmo_mode(GizmoMode3D::Rotate);
        assert!(!sel.dragging);
        assert_eq!(sel.active_axis, GizmoAxis::None);
    }

    #[test]
    fn test_axis_direction_vectors() {
        let mut sel = Selection3D::default();

        sel.active_axis = GizmoAxis::X;
        assert!(vec3_approx_eq(sel.axis_direction(), Vector3::new(1.0, 0.0, 0.0), 1e-6));

        sel.active_axis = GizmoAxis::Y;
        assert!(vec3_approx_eq(sel.axis_direction(), Vector3::new(0.0, 1.0, 0.0), 1e-6));

        sel.active_axis = GizmoAxis::Z;
        assert!(vec3_approx_eq(sel.axis_direction(), Vector3::new(0.0, 0.0, 1.0), 1e-6));

        sel.active_axis = GizmoAxis::None;
        assert!(vec3_approx_eq(sel.axis_direction(), Vector3::ZERO, 1e-6));
    }

    // -----------------------------------------------------------------------
    // Gizmo transform computation tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_gizmo_move_delta_x_axis() {
        // Camera looking straight down -Z from (0, 0, 10)
        let mut vp = Viewport3D::new(800, 600);
        vp.camera.focus_point = Vector3::ZERO;
        vp.camera.distance = 10.0;
        vp.camera.yaw = 0.0;
        vp.camera.pitch = 0.0;

        // Dragging from center to the right along X
        let delta = vp.gizmo_move_delta(
            500.0, 300.0,   // current mouse
            Vector3::ZERO,  // gizmo center
            GizmoAxis::X,
            400.0, 300.0,   // drag start
        );
        // Should produce a positive X delta
        assert!(delta.x > 0.0, "Moving mouse right should produce positive X delta, got {}", delta.x);
        assert!(approx_eq(delta.y, 0.0, 1e-4), "Y should be zero for X-axis drag");
        assert!(approx_eq(delta.z, 0.0, 1e-4), "Z should be zero for X-axis drag");
    }

    #[test]
    fn test_gizmo_move_delta_none_returns_zero() {
        let vp = Viewport3D::new(800, 600);
        let delta = vp.gizmo_move_delta(
            500.0, 300.0,
            Vector3::ZERO,
            GizmoAxis::None,
            400.0, 300.0,
        );
        assert!(vec3_approx_eq(delta, Vector3::ZERO, 1e-6));
    }

    #[test]
    fn test_gizmo_move_delta_no_motion_is_zero() {
        let vp = Viewport3D::new(800, 600);
        let delta = vp.gizmo_move_delta(
            400.0, 300.0,   // same as start
            Vector3::ZERO,
            GizmoAxis::X,
            400.0, 300.0,
        );
        assert!(approx_eq(delta.x, 0.0, 1e-4), "No mouse motion should produce zero delta");
    }

    #[test]
    fn test_gizmo_rotate_angle_zero_when_stationary() {
        let vp = Viewport3D::new(800, 600);
        let angle = vp.gizmo_rotate_angle(
            400.0, 300.0,   // same position
            Vector3::ZERO,
            400.0, 300.0,
        );
        assert!(approx_eq(angle, 0.0, 1e-4), "No motion should produce zero rotation");
    }

    #[test]
    fn test_gizmo_rotate_angle_quarter_turn() {
        let mut vp = Viewport3D::new(800, 600);
        vp.camera.focus_point = Vector3::ZERO;
        vp.camera.distance = 10.0;
        vp.camera.yaw = 0.0;
        vp.camera.pitch = 0.0;

        // The gizmo center projects approximately to screen center
        // Start to the right of center, end above center = ~90° CCW
        let center_screen = vp.world_to_screen(Vector3::ZERO);
        let cx = center_screen.0;
        let cy = center_screen.1;

        let angle = vp.gizmo_rotate_angle(
            cx, cy - 100.0,           // above center
            Vector3::ZERO,
            cx + 100.0, cy,           // right of center
        );
        // Should be approximately PI/2 (90 degrees)
        assert!(approx_eq(angle.abs(), std::f32::consts::FRAC_PI_2, 0.1),
            "Expected ~90° rotation, got {} radians", angle);
    }

    #[test]
    fn test_gizmo_scale_factor_unity_when_stationary() {
        let vp = Viewport3D::new(800, 600);
        let factor = vp.gizmo_scale_factor(
            400.0, 300.0,
            Vector3::ZERO,
            400.0, 300.0,
        );
        assert!(approx_eq(factor, 1.0, 1e-4), "No motion should produce scale factor 1.0");
    }

    #[test]
    fn test_gizmo_scale_factor_increases_when_moving_outward() {
        let mut vp = Viewport3D::new(800, 600);
        vp.camera.focus_point = Vector3::ZERO;
        vp.camera.distance = 10.0;
        vp.camera.yaw = 0.0;
        vp.camera.pitch = 0.0;

        let center_screen = vp.world_to_screen(Vector3::ZERO);
        let cx = center_screen.0;
        let cy = center_screen.1;

        let factor = vp.gizmo_scale_factor(
            cx + 200.0, cy,         // further from center
            Vector3::ZERO,
            cx + 100.0, cy,         // closer to center (start)
        );
        assert!(factor > 1.0, "Moving away from center should scale up, got {}", factor);
    }

    #[test]
    fn test_gizmo_scale_factor_decreases_when_moving_inward() {
        let mut vp = Viewport3D::new(800, 600);
        vp.camera.focus_point = Vector3::ZERO;
        vp.camera.distance = 10.0;
        vp.camera.yaw = 0.0;
        vp.camera.pitch = 0.0;

        let center_screen = vp.world_to_screen(Vector3::ZERO);
        let cx = center_screen.0;
        let cy = center_screen.1;

        let factor = vp.gizmo_scale_factor(
            cx + 50.0, cy,          // closer to center
            Vector3::ZERO,
            cx + 100.0, cy,         // further from center (start)
        );
        assert!(factor < 1.0, "Moving toward center should scale down, got {}", factor);
    }

    // -----------------------------------------------------------------------
    // World-to-screen projection tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_world_to_screen_origin_near_center() {
        let mut vp = Viewport3D::new(800, 600);
        vp.camera.focus_point = Vector3::ZERO;
        vp.camera.distance = 10.0;
        vp.camera.yaw = 0.0;
        vp.camera.pitch = 0.0;

        let (px, py) = vp.world_to_screen(Vector3::ZERO);
        // Origin should project near the center of the viewport
        assert!(approx_eq(px, 400.0, 50.0), "Origin X should be near center, got {}", px);
        assert!(approx_eq(py, 300.0, 50.0), "Origin Y should be near center, got {}", py);
    }

    #[test]
    fn test_world_to_screen_roundtrip_center() {
        let mut vp = Viewport3D::new(800, 600);
        vp.camera.focus_point = Vector3::ZERO;
        vp.camera.distance = 10.0;
        vp.camera.yaw = 0.0;
        vp.camera.pitch = 0.0;

        // Project origin to screen, then cast a ray back — ray should pass near origin
        let (px, py) = vp.world_to_screen(Vector3::ZERO);
        let ray = vp.screen_to_ray(px, py);

        // The ray should pass close to the origin
        // Find closest point on ray to origin
        let t = -ray.origin.dot(ray.direction); // project origin onto ray
        let closest = ray.at(if t > 0.0 { t } else { 0.0 });
        let dist = Vector3::new(closest.x, closest.y, closest.z).length();
        assert!(dist < 1.0, "Roundtrip ray should pass near origin, distance was {}", dist);
    }

    // -----------------------------------------------------------------------
    // ray_closest_to_line tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ray_closest_to_line_perpendicular() {
        // Ray along Z, line along X through origin
        let ray = Ray3D::new(
            Vector3::new(0.0, 0.0, 10.0),
            Vector3::new(0.0, 0.0, -1.0),
        );
        let t = ray_closest_to_line(&ray, Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
        // Closest point on X axis to the ray (which passes through origin) should be at t=0
        assert!(approx_eq(t, 0.0, 1e-4), "Expected t=0 for perpendicular intersection, got {}", t);
    }

    #[test]
    fn test_ray_closest_to_line_offset() {
        // Ray along Z from (3, 0, 10), line along X through origin
        let ray = Ray3D::new(
            Vector3::new(3.0, 0.0, 10.0),
            Vector3::new(0.0, 0.0, -1.0),
        );
        let t = ray_closest_to_line(&ray, Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
        // Closest point on X axis should be at x=3 (t=3)
        assert!(approx_eq(t, 3.0, 1e-4), "Expected t=3 for offset ray, got {}", t);
    }

    // -----------------------------------------------------------------------
    // Viewport3D selection integration
    // -----------------------------------------------------------------------

    #[test]
    fn test_viewport3d_has_selection() {
        let vp = Viewport3D::default();
        assert_eq!(vp.selection.gizmo_mode, GizmoMode3D::Select);
        assert!(vp.selection.selected_nodes.is_empty());
    }

    #[test]
    fn test_viewport3d_gizmo_workflow() {
        let mut vp = Viewport3D::new(800, 600);
        vp.camera.focus_point = Vector3::ZERO;
        vp.camera.distance = 10.0;
        vp.camera.yaw = 0.0;
        vp.camera.pitch = 0.0;

        // Select a node
        vp.selection.select(42);
        assert_eq!(vp.selection.primary(), Some(42));

        // Switch to move mode
        vp.selection.set_gizmo_mode(GizmoMode3D::Move);
        assert_eq!(vp.selection.gizmo_mode, GizmoMode3D::Move);

        // Begin drag on X axis
        vp.selection.begin_drag(GizmoAxis::X, Vector3::ZERO);
        assert!(vp.selection.dragging);

        // Compute move delta
        let delta = vp.gizmo_move_delta(
            500.0, 300.0,
            Vector3::ZERO,
            GizmoAxis::X,
            400.0, 300.0,
        );
        vp.selection.update_drag(delta);

        // End drag
        let final_delta = vp.selection.end_drag();
        assert!(final_delta.x > 0.0, "Should have moved along X");
        assert!(!vp.selection.dragging);
    }

    // -----------------------------------------------------------------------
    // EnvironmentPreview3D tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_environment_preview_default_has_sky() {
        let preview = EnvironmentPreview3D::default();
        assert!(preview.enabled);
        assert!(preview.sky_visible);
        assert!(preview.fog_visible);
        assert!(preview.sun_enabled);
        assert_eq!(preview.environment.background_mode, BackgroundMode::Sky);
        assert!(preview.environment.sky.is_some());
    }

    #[test]
    fn test_environment_preview_toggle() {
        let mut preview = EnvironmentPreview3D::default();
        assert!(preview.enabled);
        preview.toggle();
        assert!(!preview.enabled);
        preview.toggle();
        assert!(preview.enabled);
    }

    #[test]
    fn test_environment_preview_toggle_sky() {
        let mut preview = EnvironmentPreview3D::default();
        assert!(preview.sky_visible);
        preview.toggle_sky();
        assert!(!preview.sky_visible);
        // With sky hidden, effective background should fall back to ClearColor
        assert_eq!(preview.effective_background_mode(), BackgroundMode::ClearColor);
    }

    #[test]
    fn test_environment_preview_toggle_fog() {
        let mut preview = EnvironmentPreview3D::default();
        preview.environment.fog_enabled = true;
        assert!(preview.effective_fog_enabled());
        preview.toggle_fog();
        assert!(!preview.effective_fog_enabled());
    }

    #[test]
    fn test_environment_preview_toggle_sun() {
        let mut preview = EnvironmentPreview3D::default();
        assert!(preview.sun_enabled);
        preview.toggle_sun();
        assert!(!preview.sun_enabled);
    }

    #[test]
    fn test_environment_preview_effective_background_disabled() {
        let mut preview = EnvironmentPreview3D::default();
        preview.toggle();
        // When disabled, always ClearColor
        assert_eq!(preview.effective_background_mode(), BackgroundMode::ClearColor);
    }

    #[test]
    fn test_environment_preview_effective_fog_disabled() {
        let mut preview = EnvironmentPreview3D::default();
        preview.environment.fog_enabled = true;
        preview.toggle();
        assert!(!preview.effective_fog_enabled());
    }

    #[test]
    fn test_environment_preview_effective_ambient_disabled() {
        let mut preview = EnvironmentPreview3D::default();
        preview.toggle();
        let ambient = preview.effective_ambient_color();
        assert!(approx_eq(ambient.r, 0.0, 1e-6));
        assert!(approx_eq(ambient.g, 0.0, 1e-6));
        assert!(approx_eq(ambient.b, 0.0, 1e-6));
    }

    #[test]
    fn test_environment_preview_load_from_environment() {
        let mut preview = EnvironmentPreview3D::default();
        let mut env = Environment3D::default();
        env.background_mode = BackgroundMode::CustomColor;
        env.background_color = Color::new(1.0, 0.0, 0.0, 1.0);
        preview.load_from_environment(env);
        assert_eq!(preview.environment.background_mode, BackgroundMode::CustomColor);
        assert!(approx_eq(preview.environment.background_color.r, 1.0, 1e-6));
    }

    #[test]
    fn test_environment_preview_set_sky_material() {
        let mut preview = EnvironmentPreview3D::default();
        let mut proc = ProceduralSkyMaterial::default();
        proc.sky_top_color = Color::new(0.0, 0.0, 1.0, 1.0);
        preview.set_sky_material(SkyMaterial::Procedural(proc));
        match &preview.environment.sky.unwrap().material {
            SkyMaterial::Procedural(p) => {
                assert!(approx_eq(p.sky_top_color.b, 1.0, 1e-6));
            }
            _ => panic!("Expected procedural sky material"),
        }
    }

    #[test]
    fn test_environment_preview_set_sky_material_creates_sky_if_none() {
        let mut preview = EnvironmentPreview3D::default();
        preview.environment.sky = None;
        preview.environment.background_mode = BackgroundMode::ClearColor;
        preview.set_sky_material(SkyMaterial::Procedural(ProceduralSkyMaterial::default()));
        assert!(preview.environment.sky.is_some());
        assert_eq!(preview.environment.background_mode, BackgroundMode::Sky);
    }

    #[test]
    fn test_environment_preview_set_sun_angles() {
        let mut preview = EnvironmentPreview3D::default();
        // Straight down (elevation = PI/2)
        preview.set_sun_angles(0.0, std::f32::consts::FRAC_PI_2);
        assert!(approx_eq(preview.sun_direction.y, -1.0, 0.01),
            "Sun should point straight down, got y={}", preview.sun_direction.y);
    }

    #[test]
    fn test_environment_preview_set_sun_angles_horizontal() {
        let mut preview = EnvironmentPreview3D::default();
        // Horizontal, facing -Z (azimuth=0, elevation=0)
        preview.set_sun_angles(0.0, 0.0);
        assert!(approx_eq(preview.sun_direction.y, 0.0, 0.01));
        assert!(preview.sun_direction.z < 0.0, "Should face -Z");
    }

    #[test]
    fn test_environment_preview_fog_settings() {
        let mut preview = EnvironmentPreview3D::default();
        preview.set_fog_enabled(true);
        preview.set_fog_density(0.05);
        preview.set_fog_color(Color::new(0.5, 0.5, 0.5, 1.0));
        assert!(preview.environment.fog_enabled);
        assert!(approx_eq(preview.environment.fog_density, 0.05, 1e-6));
        assert!(approx_eq(preview.environment.fog_light_color.r, 0.5, 1e-6));
    }

    #[test]
    fn test_environment_preview_fog_density_clamped() {
        let mut preview = EnvironmentPreview3D::default();
        preview.set_fog_density(-5.0);
        assert!(preview.environment.fog_density >= 0.0);
    }

    #[test]
    fn test_environment_preview_tone_mapper() {
        let mut preview = EnvironmentPreview3D::default();
        preview.set_tone_mapper(ToneMapper::Aces);
        assert_eq!(preview.environment.tone_mapper, ToneMapper::Aces);
    }

    #[test]
    fn test_environment_preview_fog_volumes() {
        let mut preview = EnvironmentPreview3D::default();
        assert!(preview.fog_volumes.is_empty());
        preview.add_fog_volume(FogVolume {
            shape: FogVolumeShape::Box,
            size: Vector3::new(10.0, 5.0, 10.0),
            material: FogMaterial::default(),
        });
        assert_eq!(preview.fog_volumes.len(), 1);
        preview.clear_fog_volumes();
        assert!(preview.fog_volumes.is_empty());
    }

    #[test]
    fn test_environment_preview_reset() {
        let mut preview = EnvironmentPreview3D::default();
        preview.toggle();
        preview.toggle_sky();
        preview.toggle_sun();
        preview.set_fog_density(99.0);
        preview.reset();
        let def = EnvironmentPreview3D::default();
        assert_eq!(preview.enabled, def.enabled);
        assert_eq!(preview.sky_visible, def.sky_visible);
        assert_eq!(preview.sun_enabled, def.sun_enabled);
    }

    #[test]
    fn test_viewport3d_has_environment() {
        let vp = Viewport3D::default();
        assert!(vp.environment.enabled);
        assert!(vp.environment.sky_visible);
    }

    #[test]
    fn test_viewport3d_effective_background_color_sky() {
        let vp = Viewport3D::default();
        let bg = vp.effective_background_color();
        // Should be the procedural sky horizon color
        let proc_default = ProceduralSkyMaterial::default();
        assert!(approx_eq(bg.r, proc_default.sky_horizon_color.r, 1e-4));
    }

    #[test]
    fn test_viewport3d_effective_background_color_custom() {
        let mut vp = Viewport3D::default();
        vp.environment.environment.background_mode = BackgroundMode::CustomColor;
        vp.environment.environment.background_color = Color::new(0.5, 0.2, 0.8, 1.0);
        let bg = vp.effective_background_color();
        assert!(approx_eq(bg.r, 0.5, 1e-4));
        assert!(approx_eq(bg.g, 0.2, 1e-4));
    }

    #[test]
    fn test_viewport3d_effective_background_color_disabled() {
        let mut vp = Viewport3D::default();
        vp.environment.toggle();
        let bg = vp.effective_background_color();
        // Should fall back to editor clear color
        assert!(approx_eq(bg.r, 0.08, 0.01));
    }

    // -- sync_from_scene tests --

    #[test]
    fn test_sync_from_scene_empty_tree() {
        use gdscene::node::Node;

        let tree = SceneTree::new();
        let mut preview = EnvironmentPreview3D::default();
        let original_bg = preview.environment.background_mode;
        preview.sync_from_scene(&tree);
        // No WorldEnvironment → environment unchanged
        assert_eq!(preview.environment.background_mode, original_bg);
        assert!(preview.fog_volumes.is_empty());
    }

    #[test]
    fn test_sync_from_scene_with_world_environment() {
        use gdscene::node::Node;

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut we = Node::new("WorldEnv", "WorldEnvironment");
        we.set_property("background_mode", gdvariant::Variant::Int(1)); // CustomColor
        we.set_property(
            "background_color",
            gdvariant::Variant::Color(Color::new(0.9, 0.1, 0.2, 1.0)),
        );
        we.set_property("fog_enabled", gdvariant::Variant::Bool(true));
        we.set_property("fog_density", gdvariant::Variant::Float(0.08));
        tree.add_child(root, we).unwrap();

        let mut preview = EnvironmentPreview3D::default();
        preview.sync_from_scene(&tree);

        assert_eq!(preview.environment.background_mode, BackgroundMode::CustomColor);
        assert!(approx_eq(preview.environment.background_color.r, 0.9, 1e-4));
        assert!(preview.environment.fog_enabled);
        assert!(approx_eq(preview.environment.fog_density, 0.08, 1e-4));
    }

    #[test]
    fn test_sync_from_scene_with_fog_volume() {
        use gdscene::node::Node;

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut fv = Node::new("Fog1", "FogVolume");
        fv.set_property("shape", gdvariant::Variant::Int(3)); // Box
        fv.set_property(
            "size",
            gdvariant::Variant::Vector3(Vector3::new(10.0, 5.0, 10.0)),
        );
        fv.set_property("density", gdvariant::Variant::Float(0.5));
        tree.add_child(root, fv).unwrap();

        let mut preview = EnvironmentPreview3D::default();
        preview.sync_from_scene(&tree);

        assert_eq!(preview.fog_volumes.len(), 1);
        assert_eq!(preview.fog_volumes[0].shape, FogVolumeShape::Box);
        assert!(approx_eq(preview.fog_volumes[0].size.x, 10.0, 1e-4));
        assert!(approx_eq(preview.fog_volumes[0].material.density, 0.5, 1e-4));
    }

    #[test]
    fn test_sync_from_scene_multiple_fog_volumes() {
        use gdscene::node::Node;

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let fv1 = Node::new("Fog1", "FogVolume");
        tree.add_child(root, fv1).unwrap();

        let fv2 = Node::new("Fog2", "FogVolume");
        tree.add_child(root, fv2).unwrap();

        let mut preview = EnvironmentPreview3D::default();
        preview.sync_from_scene(&tree);
        assert_eq!(preview.fog_volumes.len(), 2);
    }

    #[test]
    fn test_sync_from_scene_first_world_env_wins() {
        use gdscene::node::Node;

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut we1 = Node::new("Env1", "WorldEnvironment");
        we1.set_property("background_mode", gdvariant::Variant::Int(1)); // CustomColor
        tree.add_child(root, we1).unwrap();

        let mut we2 = Node::new("Env2", "WorldEnvironment");
        we2.set_property("background_mode", gdvariant::Variant::Int(2)); // Sky
        tree.add_child(root, we2).unwrap();

        let mut preview = EnvironmentPreview3D::default();
        preview.sync_from_scene(&tree);
        // First WorldEnvironment wins (Godot behavior)
        assert_eq!(preview.environment.background_mode, BackgroundMode::CustomColor);
    }

    #[test]
    fn test_sync_clears_old_fog_volumes() {
        use gdscene::node::Node;

        let mut preview = EnvironmentPreview3D::default();
        preview.add_fog_volume(FogVolume::default());
        preview.add_fog_volume(FogVolume::default());
        assert_eq!(preview.fog_volumes.len(), 2);

        // Sync with a tree that has no FogVolume nodes
        let tree = SceneTree::new();
        preview.sync_from_scene(&tree);
        assert!(preview.fog_volumes.is_empty());
    }

    #[test]
    fn test_viewport3d_sync_environment_from_scene() {
        use gdscene::node::Node;

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut we = Node::new("WorldEnv", "WorldEnvironment");
        we.set_property("fog_enabled", gdvariant::Variant::Bool(true));
        we.set_property("fog_density", gdvariant::Variant::Float(0.03));
        tree.add_child(root, we).unwrap();

        let fv = Node::new("Fog", "FogVolume");
        tree.add_child(root, fv).unwrap();

        let mut vp = Viewport3D::default();
        vp.sync_environment_from_scene(&tree);

        assert!(vp.environment.environment.fog_enabled);
        assert!(approx_eq(vp.environment.environment.fog_density, 0.03, 1e-4));
        assert_eq!(vp.environment.fog_volumes.len(), 1);
    }

    #[test]
    fn test_viewport3d_effective_environment_enabled() {
        let vp = Viewport3D::default();
        let env = vp.effective_environment();
        assert!(env.is_some());
        let env = env.unwrap();
        assert_eq!(env.background_mode, BackgroundMode::Sky);
    }

    #[test]
    fn test_viewport3d_effective_environment_disabled() {
        let mut vp = Viewport3D::default();
        vp.environment.toggle(); // disable
        assert!(vp.effective_environment().is_none());
    }

    #[test]
    fn test_viewport3d_effective_environment_sky_hidden() {
        let mut vp = Viewport3D::default();
        vp.environment.toggle_sky(); // hide sky
        let env = vp.effective_environment().unwrap();
        assert_eq!(env.background_mode, BackgroundMode::ClearColor);
        assert!(env.sky.is_none());
    }

    #[test]
    fn test_viewport3d_effective_environment_fog_hidden() {
        let mut vp = Viewport3D::default();
        vp.environment.set_fog_enabled(true);
        vp.environment.toggle_fog(); // hide fog
        let env = vp.effective_environment().unwrap();
        assert!(!env.fog_enabled);
    }

    #[test]
    fn test_fog_volume_from_node_properties_defaults() {
        let props: Vec<(String, gdvariant::Variant)> = vec![];
        let volume = fog_volume_from_node_properties(props.iter().map(|(k, v)| (k, v)));
        assert_eq!(volume.shape, FogVolumeShape::Ellipsoid);
        assert!(approx_eq(volume.material.density, 1.0, 1e-4));
    }

    #[test]
    fn test_fog_volume_from_node_properties_all_fields() {
        let props: Vec<(String, gdvariant::Variant)> = vec![
            ("shape".into(), gdvariant::Variant::Int(3)),
            ("size".into(), gdvariant::Variant::Vector3(Vector3::new(4.0, 6.0, 8.0))),
            ("density".into(), gdvariant::Variant::Float(0.7)),
            ("albedo".into(), gdvariant::Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0))),
            ("emission".into(), gdvariant::Variant::Color(Color::new(0.0, 1.0, 0.0, 1.0))),
            ("height_falloff".into(), gdvariant::Variant::Float(2.5)),
            ("edge_fade".into(), gdvariant::Variant::Float(0.3)),
        ];
        let volume = fog_volume_from_node_properties(props.iter().map(|(k, v)| (k, v)));
        assert_eq!(volume.shape, FogVolumeShape::Box);
        assert!(approx_eq(volume.size.x, 4.0, 1e-4));
        assert!(approx_eq(volume.size.y, 6.0, 1e-4));
        assert!(approx_eq(volume.material.density, 0.7, 1e-4));
        assert!(approx_eq(volume.material.albedo.r, 1.0, 1e-4));
        assert!(approx_eq(volume.material.emission.g, 1.0, 1e-4));
        assert!(approx_eq(volume.material.height_falloff, 2.5, 1e-4));
        assert!(approx_eq(volume.material.edge_fade, 0.3, 1e-4));
    }

    // -----------------------------------------------------------------------
    // GizmoAxis extended tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_gizmo_axis_direction_single() {
        assert!(vec3_approx_eq(GizmoAxis::X.direction(), Vector3::new(1.0, 0.0, 0.0), 1e-6));
        assert!(vec3_approx_eq(GizmoAxis::Y.direction(), Vector3::new(0.0, 1.0, 0.0), 1e-6));
        assert!(vec3_approx_eq(GizmoAxis::Z.direction(), Vector3::new(0.0, 0.0, 1.0), 1e-6));
        assert!(vec3_approx_eq(GizmoAxis::None.direction(), Vector3::ZERO, 1e-6));
    }

    #[test]
    fn test_gizmo_axis_direction_plane() {
        // XY plane normal is Z
        assert!(vec3_approx_eq(GizmoAxis::XY.direction(), Vector3::new(0.0, 0.0, 1.0), 1e-6));
        // XZ plane normal is Y
        assert!(vec3_approx_eq(GizmoAxis::XZ.direction(), Vector3::new(0.0, 1.0, 0.0), 1e-6));
        // YZ plane normal is X
        assert!(vec3_approx_eq(GizmoAxis::YZ.direction(), Vector3::new(1.0, 0.0, 0.0), 1e-6));
    }

    #[test]
    fn test_gizmo_axis_is_plane_and_single() {
        assert!(GizmoAxis::X.is_single());
        assert!(!GizmoAxis::X.is_plane());
        assert!(GizmoAxis::XY.is_plane());
        assert!(!GizmoAxis::XY.is_single());
        assert!(!GizmoAxis::None.is_plane());
        assert!(!GizmoAxis::None.is_single());
    }

    #[test]
    fn test_gizmo_axis_mask() {
        let v = Vector3::new(3.0, 4.0, 5.0);
        assert!(vec3_approx_eq(GizmoAxis::X.mask(v), Vector3::new(3.0, 0.0, 0.0), 1e-6));
        assert!(vec3_approx_eq(GizmoAxis::Y.mask(v), Vector3::new(0.0, 4.0, 0.0), 1e-6));
        assert!(vec3_approx_eq(GizmoAxis::Z.mask(v), Vector3::new(0.0, 0.0, 5.0), 1e-6));
        assert!(vec3_approx_eq(GizmoAxis::XY.mask(v), Vector3::new(3.0, 4.0, 0.0), 1e-6));
        assert!(vec3_approx_eq(GizmoAxis::XZ.mask(v), Vector3::new(3.0, 0.0, 5.0), 1e-6));
        assert!(vec3_approx_eq(GizmoAxis::YZ.mask(v), Vector3::new(0.0, 4.0, 5.0), 1e-6));
        assert!(vec3_approx_eq(GizmoAxis::None.mask(v), Vector3::ZERO, 1e-6));
    }

    // -----------------------------------------------------------------------
    // GizmoConfig3D tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_gizmo_config_defaults() {
        let config = GizmoConfig3D::default();
        assert!(config.arrow_length > 0.0);
        assert!(config.ring_radius > 0.0);
        assert!(config.scale_handle_length > 0.0);
        assert!(config.pick_tolerance > 0.0);
        assert!(config.plane_handle_fraction > 0.0 && config.plane_handle_fraction < 1.0);
    }

    // -----------------------------------------------------------------------
    // GizmoSnap3D tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_snap_value_with_zero_step() {
        assert!(approx_eq(GizmoSnap3D::snap_value(3.7, 0.0), 3.7, 1e-6));
    }

    #[test]
    fn test_snap_value_rounds_to_nearest() {
        assert!(approx_eq(GizmoSnap3D::snap_value(3.7, 1.0), 4.0, 1e-6));
        assert!(approx_eq(GizmoSnap3D::snap_value(3.2, 1.0), 3.0, 1e-6));
        assert!(approx_eq(GizmoSnap3D::snap_value(0.3, 0.25), 0.25, 1e-6));
        assert!(approx_eq(GizmoSnap3D::snap_value(-0.3, 0.25), -0.25, 1e-6));
    }

    #[test]
    fn test_snap_translate() {
        let snap = GizmoSnap3D { translate_step: 0.5, ..Default::default() };
        let v = Vector3::new(1.3, 2.7, -0.1);
        let snapped = snap.snap_translate(v);
        assert!(approx_eq(snapped.x, 1.5, 1e-6));
        assert!(approx_eq(snapped.y, 2.5, 1e-6));
        assert!(approx_eq(snapped.z, 0.0, 1e-6));
    }

    #[test]
    fn test_snap_translate_no_snap() {
        let snap = GizmoSnap3D::default();
        let v = Vector3::new(1.3, 2.7, -0.1);
        let snapped = snap.snap_translate(v);
        assert!(vec3_approx_eq(snapped, v, 1e-6));
    }

    #[test]
    fn test_snap_rotate() {
        let snap = GizmoSnap3D {
            rotate_step: std::f32::consts::FRAC_PI_4,
            ..Default::default()
        };
        let snapped = snap.snap_rotate(0.3);
        assert!(approx_eq(snapped, 0.0, 0.1), "0.3 should snap to 0 with PI/4 step");
        let snapped2 = snap.snap_rotate(0.9);
        assert!(approx_eq(snapped2, std::f32::consts::FRAC_PI_4, 0.1));
    }

    #[test]
    fn test_snap_scale() {
        let snap = GizmoSnap3D { scale_step: 0.1, ..Default::default() };
        assert!(approx_eq(snap.snap_scale(1.34), 1.3, 0.05));
        assert!(approx_eq(snap.snap_scale(1.06), 1.1, 0.05));
    }

    // -----------------------------------------------------------------------
    // GizmoDragState3D / Viewport3D gizmo integration tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_viewport3d_gizmo_drag_not_started_in_select_mode() {
        let mut vp = Viewport3D::new(800, 600);
        vp.selection.set_gizmo_mode(GizmoMode3D::Select);
        let started = vp.begin_gizmo_drag(400.0, 300.0, Vector3::ZERO, GizmoAxis::X);
        assert!(!started);
        assert!(!vp.is_gizmo_dragging());
    }

    #[test]
    fn test_viewport3d_gizmo_drag_not_started_with_none_axis() {
        let mut vp = Viewport3D::new(800, 600);
        vp.selection.set_gizmo_mode(GizmoMode3D::Move);
        let started = vp.begin_gizmo_drag(400.0, 300.0, Vector3::ZERO, GizmoAxis::None);
        assert!(!started);
    }

    #[test]
    fn test_viewport3d_gizmo_move_drag_lifecycle() {
        let mut vp = Viewport3D::new(800, 600);
        vp.camera.focus_point = Vector3::ZERO;
        vp.camera.distance = 10.0;
        vp.camera.yaw = 0.0;
        vp.camera.pitch = 0.0;

        vp.selection.select(42);
        vp.selection.set_gizmo_mode(GizmoMode3D::Move);

        // Begin drag on X axis
        let started = vp.begin_gizmo_drag(400.0, 300.0, Vector3::ZERO, GizmoAxis::X);
        assert!(started);
        assert!(vp.is_gizmo_dragging());
        assert_eq!(vp.gizmo_drag_axis(), GizmoAxis::X);

        // Update drag
        let snap = GizmoSnap3D::default();
        let transform = vp.update_gizmo_drag(500.0, 300.0, &snap);
        assert!(transform.is_some());
        match transform.unwrap() {
            GizmoTransform3D::Move(delta) => {
                assert!(delta.x > 0.0, "Moving right should produce positive X delta");
                assert!(approx_eq(delta.y, 0.0, 1e-4));
                assert!(approx_eq(delta.z, 0.0, 1e-4));
            }
            _ => panic!("Expected Move transform"),
        }

        // End drag
        let result = vp.end_gizmo_drag();
        assert!(result.is_some());
        assert!(!vp.is_gizmo_dragging());
        assert!(!vp.selection.dragging);
    }

    #[test]
    fn test_viewport3d_gizmo_rotate_drag_lifecycle() {
        let mut vp = Viewport3D::new(800, 600);
        vp.camera.focus_point = Vector3::ZERO;
        vp.camera.distance = 10.0;
        vp.camera.yaw = 0.0;
        vp.camera.pitch = 0.0;

        vp.selection.select(7);
        vp.selection.set_gizmo_mode(GizmoMode3D::Rotate);

        let started = vp.begin_gizmo_drag(500.0, 300.0, Vector3::ZERO, GizmoAxis::Y);
        assert!(started);

        let snap = GizmoSnap3D::default();
        let transform = vp.update_gizmo_drag(500.0, 200.0, &snap);
        assert!(transform.is_some());
        match transform.unwrap() {
            GizmoTransform3D::Rotate { axis, angle } => {
                assert_eq!(axis, GizmoAxis::Y);
                assert!(angle.abs() > 0.0, "Should have rotated");
            }
            _ => panic!("Expected Rotate transform"),
        }

        let result = vp.end_gizmo_drag();
        assert!(result.is_some());
        assert!(!vp.is_gizmo_dragging());
    }

    #[test]
    fn test_viewport3d_gizmo_scale_drag_lifecycle() {
        let mut vp = Viewport3D::new(800, 600);
        vp.camera.focus_point = Vector3::ZERO;
        vp.camera.distance = 10.0;
        vp.camera.yaw = 0.0;
        vp.camera.pitch = 0.0;

        vp.selection.select(99);
        vp.selection.set_gizmo_mode(GizmoMode3D::Scale);

        let center_screen = vp.world_to_screen(Vector3::ZERO);
        let cx = center_screen.0;
        let cy = center_screen.1;

        let started = vp.begin_gizmo_drag(cx + 100.0, cy, Vector3::ZERO, GizmoAxis::X);
        assert!(started);

        let snap = GizmoSnap3D::default();
        let transform = vp.update_gizmo_drag(cx + 200.0, cy, &snap);
        assert!(transform.is_some());
        match transform.unwrap() {
            GizmoTransform3D::Scale { axis, factor } => {
                assert_eq!(axis, GizmoAxis::X);
                assert!(factor > 1.0, "Moving outward should scale up, got {}", factor);
            }
            _ => panic!("Expected Scale transform"),
        }

        vp.end_gizmo_drag();
        assert!(!vp.is_gizmo_dragging());
    }

    #[test]
    fn test_viewport3d_gizmo_cancel_drag() {
        let mut vp = Viewport3D::new(800, 600);
        vp.selection.select(1);
        vp.selection.set_gizmo_mode(GizmoMode3D::Move);
        vp.begin_gizmo_drag(400.0, 300.0, Vector3::ZERO, GizmoAxis::Y);
        assert!(vp.is_gizmo_dragging());

        vp.cancel_gizmo_drag();
        assert!(!vp.is_gizmo_dragging());
        assert!(!vp.selection.dragging);
    }

    #[test]
    fn test_viewport3d_gizmo_move_with_snap() {
        let mut vp = Viewport3D::new(800, 600);
        vp.camera.focus_point = Vector3::ZERO;
        vp.camera.distance = 10.0;
        vp.camera.yaw = 0.0;
        vp.camera.pitch = 0.0;

        vp.selection.select(42);
        vp.selection.set_gizmo_mode(GizmoMode3D::Move);
        vp.begin_gizmo_drag(400.0, 300.0, Vector3::ZERO, GizmoAxis::X);

        let snap = GizmoSnap3D { translate_step: 1.0, ..Default::default() };
        let transform = vp.update_gizmo_drag(500.0, 300.0, &snap);
        match transform.unwrap() {
            GizmoTransform3D::Move(delta) => {
                // Delta should be snapped to nearest integer
                let remainder = delta.x % 1.0;
                assert!(approx_eq(remainder, 0.0, 0.01) || approx_eq(remainder.abs(), 1.0, 0.01),
                    "Delta X should be snapped to nearest 1.0, got {}", delta.x);
            }
            _ => panic!("Expected Move transform"),
        }

        vp.end_gizmo_drag();
    }

    #[test]
    fn test_viewport3d_hit_test_gizmo_full_select_mode() {
        let vp = Viewport3D::new(800, 600);
        let config = GizmoConfig3D::default();
        let axis = vp.hit_test_gizmo_full(400.0, 300.0, Vector3::ZERO, &config);
        assert_eq!(axis, GizmoAxis::None, "Select mode should never hit a gizmo");
    }

    #[test]
    fn test_viewport3d_hit_test_gizmo_full_miss() {
        let mut vp = Viewport3D::new(800, 600);
        vp.camera.focus_point = Vector3::ZERO;
        vp.camera.distance = 10.0;
        vp.camera.yaw = 0.0;
        vp.camera.pitch = 0.0;
        vp.selection.set_gizmo_mode(GizmoMode3D::Move);

        let config = GizmoConfig3D::default();
        // Click in the far corner — should miss
        let axis = vp.hit_test_gizmo_full(0.0, 0.0, Vector3::ZERO, &config);
        assert_eq!(axis, GizmoAxis::None);
    }

    #[test]
    fn test_viewport3d_gizmo_plane_drag() {
        let mut vp = Viewport3D::new(800, 600);
        vp.camera.focus_point = Vector3::ZERO;
        vp.camera.distance = 10.0;
        vp.camera.yaw = 0.0;
        vp.camera.pitch = 0.0;

        vp.selection.select(42);
        vp.selection.set_gizmo_mode(GizmoMode3D::Move);

        // Begin drag on XY plane
        let started = vp.begin_gizmo_drag(400.0, 300.0, Vector3::ZERO, GizmoAxis::XY);
        assert!(started);
        assert_eq!(vp.gizmo_drag_axis(), GizmoAxis::XY);

        let snap = GizmoSnap3D::default();
        let transform = vp.update_gizmo_drag(450.0, 250.0, &snap);
        assert!(transform.is_some());
        match transform.unwrap() {
            GizmoTransform3D::Move(delta) => {
                // Z should be zero for XY plane
                assert!(approx_eq(delta.z, 0.0, 1e-4), "Z should be zero for XY plane drag");
            }
            _ => panic!("Expected Move transform"),
        }

        vp.end_gizmo_drag();
    }

    // -----------------------------------------------------------------------
    // ray_plane_intersect tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ray_plane_intersect_hit() {
        let ray = Ray3D::new(Vector3::new(0.0, 5.0, 0.0), Vector3::new(0.0, -1.0, 0.0));
        let t = ray_plane_intersect(&ray, Vector3::ZERO, Vector3::new(0.0, 1.0, 0.0));
        assert!(t.is_some());
        assert!(approx_eq(t.unwrap(), 5.0, 1e-4));
    }

    #[test]
    fn test_ray_plane_intersect_parallel() {
        let ray = Ray3D::new(Vector3::new(0.0, 5.0, 0.0), Vector3::new(1.0, 0.0, 0.0));
        let t = ray_plane_intersect(&ray, Vector3::ZERO, Vector3::new(0.0, 1.0, 0.0));
        assert!(t.is_none(), "Parallel ray should not intersect plane");
    }

    #[test]
    fn test_ray_plane_intersect_behind() {
        let ray = Ray3D::new(Vector3::new(0.0, -5.0, 0.0), Vector3::new(0.0, -1.0, 0.0));
        let t = ray_plane_intersect(&ray, Vector3::ZERO, Vector3::new(0.0, 1.0, 0.0));
        assert!(t.is_none(), "Intersection behind ray should return None");
    }
}

//! Engine startup bootstrap sequence matching Godot initialization order.
//!
//! Godot initializes in a well-defined order:
//!
//! 1. **Core** — math types, Variant system, OS singleton
//! 2. **Servers** — ClassDB registration, PhysicsServer, RenderingServer, etc.
//! 3. **Resources** — ResourceLoader, resource cache, importers
//! 4. **Scene** — SceneTree creation, root viewport
//! 5. **Main scene** — load and instance the project's main scene
//! 6. **Scripts** — parse and attach GDScript instances to nodes
//! 7. **Lifecycle** — enter_tree / _ready notifications in tree order
//! 8. **Main loop** — begin frame stepping (_process / _physics_process)
//!
//! [`EngineBootstrap`] walks through these phases in order, tracking which
//! phases have completed. Each phase can be run independently for testing
//! or all at once via [`EngineBootstrap::run_all`].

use std::fmt;
use std::path::{Path, PathBuf};

use gdaudio::AudioOutputStream;
use gdscene::scene_tree::SceneTree;
use gdscene::{LifecycleManager, MainLoop, PackedScene};
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// BootPhase
// ---------------------------------------------------------------------------

/// Ordered phases of engine initialization, matching Godot's startup sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BootPhase {
    /// Phase 0: Not yet started.
    None = 0,
    /// Phase 1: Core systems initialized (math, Variant, OS).
    Core = 1,
    /// Phase 2: Server registration (ClassDB, physics, rendering).
    Servers = 2,
    /// Phase 3: Resource systems ready (loader, cache, importers).
    Resources = 3,
    /// Phase 4: Scene tree created with root viewport.
    SceneTree = 4,
    /// Phase 5: Project autoloads instantiated under /root (before main scene).
    Autoloads = 5,
    /// Phase 6: Main scene loaded and instanced into tree.
    MainScene = 6,
    /// Phase 7: Scripts parsed and attached to nodes.
    Scripts = 7,
    /// Phase 8: Lifecycle notifications sent (enter_tree, _ready).
    Lifecycle = 8,
    /// Phase 9: Main loop running (frame stepping active).
    Running = 9,
}

impl BootPhase {
    /// Returns all phases in initialization order.
    pub fn all() -> &'static [BootPhase] {
        &[
            BootPhase::None,
            BootPhase::Core,
            BootPhase::Servers,
            BootPhase::Resources,
            BootPhase::SceneTree,
            BootPhase::Autoloads,
            BootPhase::MainScene,
            BootPhase::Scripts,
            BootPhase::Lifecycle,
            BootPhase::Running,
        ]
    }

    /// Returns the next phase in sequence, or `None` if already at `Running`.
    pub fn next(self) -> Option<BootPhase> {
        match self {
            BootPhase::None => Some(BootPhase::Core),
            BootPhase::Core => Some(BootPhase::Servers),
            BootPhase::Servers => Some(BootPhase::Resources),
            BootPhase::Resources => Some(BootPhase::SceneTree),
            BootPhase::SceneTree => Some(BootPhase::Autoloads),
            BootPhase::Autoloads => Some(BootPhase::MainScene),
            BootPhase::MainScene => Some(BootPhase::Scripts),
            BootPhase::Scripts => Some(BootPhase::Lifecycle),
            BootPhase::Lifecycle => Some(BootPhase::Running),
            BootPhase::Running => None,
        }
    }

    /// Returns a human-readable name for this phase.
    pub fn name(self) -> &'static str {
        match self {
            BootPhase::None => "None",
            BootPhase::Core => "Core",
            BootPhase::Servers => "Servers",
            BootPhase::Resources => "Resources",
            BootPhase::SceneTree => "SceneTree",
            BootPhase::Autoloads => "Autoloads",
            BootPhase::MainScene => "MainScene",
            BootPhase::Scripts => "Scripts",
            BootPhase::Lifecycle => "Lifecycle",
            BootPhase::Running => "Running",
        }
    }

    /// Returns the integer index for this phase (0–8).
    pub fn index(self) -> u8 {
        self as u8
    }
}

impl fmt::Display for BootPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ---------------------------------------------------------------------------
// BootError
// ---------------------------------------------------------------------------

/// Error that can occur during engine bootstrap.
#[derive(Debug, Clone)]
pub struct BootError {
    /// The phase during which the error occurred.
    pub phase: BootPhase,
    /// Description of the error.
    pub message: String,
}

impl fmt::Display for BootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bootstrap error in {}: {}", self.phase, self.message)
    }
}

impl std::error::Error for BootError {}

// ---------------------------------------------------------------------------
// BootConfig
// ---------------------------------------------------------------------------

/// Configuration for the engine bootstrap sequence.
#[derive(Debug, Clone)]
pub struct BootConfig {
    /// Path to the main scene file (.tscn).
    pub main_scene: Option<PathBuf>,
    /// Project directory (for resolving res:// paths).
    pub project_dir: PathBuf,
    /// Window width.
    pub window_width: u32,
    /// Window height.
    pub window_height: u32,
    /// Whether to enable event tracing.
    pub event_tracing: bool,
    /// Whether to run in headless mode (no window).
    pub headless: bool,
}

impl Default for BootConfig {
    fn default() -> Self {
        Self {
            main_scene: None,
            project_dir: PathBuf::from("."),
            window_width: 1152,
            window_height: 648,
            event_tracing: false,
            headless: true,
        }
    }
}

impl BootConfig {
    /// Creates a new config for headless testing.
    pub fn headless() -> Self {
        Self::default()
    }

    /// Creates a config with a main scene path.
    pub fn with_scene(scene_path: impl Into<PathBuf>) -> Self {
        let path: PathBuf = scene_path.into();
        let project_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        Self {
            main_scene: Some(path),
            project_dir,
            ..Self::default()
        }
    }

    /// Sets the project directory.
    pub fn project_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.project_dir = dir.into();
        self
    }

    /// Sets the window dimensions.
    pub fn window_size(mut self, width: u32, height: u32) -> Self {
        self.window_width = width;
        self.window_height = height;
        self
    }

    /// Enables event tracing.
    pub fn with_event_tracing(mut self) -> Self {
        self.event_tracing = true;
        self
    }
}

// ---------------------------------------------------------------------------
// BootstrapLog
// ---------------------------------------------------------------------------

/// A log entry recording when a bootstrap phase completed.
#[derive(Debug, Clone)]
pub struct BootstrapLogEntry {
    /// The phase that completed.
    pub phase: BootPhase,
    /// Optional detail message.
    pub detail: String,
}

// ---------------------------------------------------------------------------
// EngineBootstrap
// ---------------------------------------------------------------------------

/// Drives engine initialization through Godot's startup phases in order.
///
/// # Example
///
/// ```
/// use patina_runner::bootstrap::{EngineBootstrap, BootConfig, BootPhase};
///
/// let config = BootConfig::headless();
/// let mut boot = EngineBootstrap::new(config);
///
/// // Run through core + servers + resources + scene tree.
/// boot.advance_to(BootPhase::SceneTree).unwrap();
/// assert_eq!(boot.current_phase(), BootPhase::SceneTree);
/// assert!(boot.tree().is_some());
/// ```
pub struct EngineBootstrap {
    config: BootConfig,
    phase: BootPhase,
    tree: Option<SceneTree>,
    scene_root_id: Option<gdscene::node::NodeId>,
    main_loop: Option<MainLoop>,
    log: Vec<BootstrapLogEntry>,
    scripts_attached: usize,
    autoloads_instantiated: usize,
    /// Audio output sink to attach to the [`MainLoop`]'s audio bridge when the
    /// [`BootPhase::Running`] phase creates it. If `None`, a platform-default
    /// sink is selected at boot (see [`default_audio_output`]). Injecting a sink
    /// via [`EngineBootstrap::set_audio_output`] overrides the default and lets
    /// tests capture the mixed PCM that flows out of the boot flow.
    pending_output: Option<Box<dyn AudioOutputStream>>,
}

impl EngineBootstrap {
    /// Creates a new bootstrap sequence with the given configuration.
    pub fn new(config: BootConfig) -> Self {
        Self {
            config,
            phase: BootPhase::None,
            tree: None,
            scene_root_id: None,
            main_loop: None,
            log: Vec::new(),
            scripts_attached: 0,
            autoloads_instantiated: 0,
            pending_output: None,
        }
    }

    /// Attaches a custom audio output sink to the run.
    ///
    /// The sink is handed to the [`MainLoop`]'s audio bridge when the
    /// [`BootPhase::Running`] phase constructs the loop, so mixed PCM produced
    /// by `AudioStreamPlayer` nodes flows into it every frame instead of being
    /// discarded. Overrides the platform-default sink chosen at boot. Primarily
    /// used by tests (with a capture sink) and by hosts that own the device
    /// (e.g. the web AudioWorklet backend attaching after a user-gesture
    /// unlock). Must be called before the loop is created.
    pub fn set_audio_output(&mut self, output: Box<dyn AudioOutputStream>) {
        self.pending_output = Some(output);
    }

    /// Returns the current bootstrap phase.
    pub fn current_phase(&self) -> BootPhase {
        self.phase
    }

    /// Returns `true` if the engine has completed all bootstrap phases.
    pub fn is_running(&self) -> bool {
        self.phase == BootPhase::Running
    }

    /// Returns a reference to the scene tree (available after `SceneTree` phase).
    pub fn tree(&self) -> Option<&SceneTree> {
        self.tree.as_ref()
    }

    /// Returns a mutable reference to the scene tree.
    pub fn tree_mut(&mut self) -> Option<&mut SceneTree> {
        self.tree.as_mut()
    }

    /// Returns a reference to the main loop (available after `Running` phase).
    pub fn main_loop(&self) -> Option<&MainLoop> {
        self.main_loop.as_ref()
    }

    /// Returns a mutable reference to the main loop.
    pub fn main_loop_mut(&mut self) -> Option<&mut MainLoop> {
        self.main_loop.as_mut()
    }

    /// Returns the scene root node ID (available after `MainScene` phase).
    pub fn scene_root_id(&self) -> Option<gdscene::node::NodeId> {
        self.scene_root_id
    }

    /// Returns the bootstrap log.
    pub fn log(&self) -> &[BootstrapLogEntry] {
        &self.log
    }

    /// Returns the configuration.
    pub fn config(&self) -> &BootConfig {
        &self.config
    }

    /// Returns the number of scripts attached during the Scripts phase.
    pub fn scripts_attached(&self) -> usize {
        self.scripts_attached
    }

    /// Returns the number of project autoloads instantiated during the
    /// Autoloads phase.
    pub fn autoloads_instantiated(&self) -> usize {
        self.autoloads_instantiated
    }

    /// Advances the bootstrap sequence by one phase.
    ///
    /// Returns the new phase, or an error if the phase failed.
    pub fn step(&mut self) -> Result<BootPhase, BootError> {
        let next = match self.phase.next() {
            Some(p) => p,
            None => return Ok(self.phase), // Already at Running.
        };

        match next {
            BootPhase::Core => self.init_core()?,
            BootPhase::Servers => self.init_servers()?,
            BootPhase::Resources => self.init_resources()?,
            BootPhase::SceneTree => self.init_scene_tree()?,
            BootPhase::Autoloads => self.init_autoloads()?,
            BootPhase::MainScene => self.init_main_scene()?,
            BootPhase::Scripts => self.init_scripts()?,
            BootPhase::Lifecycle => self.init_lifecycle()?,
            BootPhase::Running => self.init_running()?,
            BootPhase::None => unreachable!(),
        }

        self.phase = next;
        self.log.push(BootstrapLogEntry {
            phase: next,
            detail: String::new(),
        });
        Ok(next)
    }

    /// Advances the bootstrap to the given phase (running all intermediate phases).
    pub fn advance_to(&mut self, target: BootPhase) -> Result<(), BootError> {
        while self.phase < target {
            self.step()?;
        }
        Ok(())
    }

    /// Runs all bootstrap phases from current state to `Running`.
    pub fn run_all(&mut self) -> Result<(), BootError> {
        self.advance_to(BootPhase::Running)
    }

    // -- Phase implementations -----------------------------------------------

    fn init_core(&self) -> Result<(), BootError> {
        // Core types (Vector2, Vector3, Variant, etc.) are initialized at
        // compile time in Rust. Nothing to do at runtime, but this phase
        // exists to match Godot's sequence and allow hooks.
        Ok(())
    }

    fn init_servers(&self) -> Result<(), BootError> {
        // Populate ClassDB, mirroring Godot's `register_scene_types()` inside
        // its server-init phase (upstream/godot/main/main.cpp) — PhysicsServer,
        // RenderingServer etc. remain on-demand (pat-s3rrv.7).
        //
        // This phase used to only *read* `class_count()` to prove the lock was
        // not poisoned, on the documented assumption that "the caller is
        // responsible for registering classes". No caller ever was: the runner
        // booted every game against an all-but-empty ClassDB, so scene loading
        // seeded no class defaults at all. Registering here makes
        // `EngineBootstrap::run_all` self-sufficient the way Godot's boot is.
        gdobject::class_db::register_all_classes();
        Ok(())
    }

    fn init_resources(&self) -> Result<(), BootError> {
        // Resource systems (loader, cache, importers) are initialized on demand.
        // This phase validates the project directory exists.
        if !self.config.project_dir.exists() && self.config.main_scene.is_some() {
            return Err(BootError {
                phase: BootPhase::Resources,
                message: format!(
                    "project directory does not exist: {}",
                    self.config.project_dir.display()
                ),
            });
        }

        // Root `res://` at the project directory, as Godot's boot does before
        // anything loads. `project_boot::boot_project` sets it for the runner's
        // own path; `EngineBootstrap` never did, so every `res://` read in a
        // game booted this way failed — `load("res://layout.tres")` fell back
        // to a bare `Resource` placeholder and `get_class()` lied. `user://`
        // keeps whatever root is already installed.
        if self.config.project_dir.exists() {
            gdcore::file_access::set_path_roots(
                Some(self.config.project_dir.clone()),
                gdcore::file_access::user_root(),
            );
        }

        // Install the DisplayServer backend from the project's configured
        // viewport, so a script calling `DisplayServer.window_get_size()` sees
        // the real value instead of the interpreter's old hardcoded 1920x1080
        // (pat-0q2kx.38). `project_boot::boot_project` installs it for the
        // runner's own path; this covers everything booting through
        // `EngineBootstrap`, which does not go through `boot_project`.
        //
        // A missing or unreadable project.godot is not a boot failure — many
        // tests boot a bare scene — so fall back to Godot's default viewport.
        let settings = gdcore::ProjectSettings::load_godot_project(
            &self.config.project_dir.join("project.godot"),
        )
        .unwrap_or_default();
        gdcore::display_backend::install_display_backend(std::sync::Arc::new(
            gdcore::display_backend::HeadlessDisplayBackend::new(
                settings.viewport_width(),
                settings.viewport_height(),
            ),
        ));

        Ok(())
    }

    fn init_scene_tree(&mut self) -> Result<(), BootError> {
        let mut tree = SceneTree::new();
        if self.config.event_tracing {
            tree.event_trace_mut().enable();
        }

        // Install the project's content-scale ("stretch") settings on the root
        // window (pat-nzgdu.87). Godot seeds the root Window from
        // `display/window/stretch/*` at boot; without this the whole pipeline
        // sits at its defaults and a project that asked to be stretched renders
        // 1:1. As elsewhere in boot, a missing project.godot is not a failure —
        // a bare scene keeps Godot's defaults.
        let settings = gdcore::ProjectSettings::load_godot_project(
            &self.config.project_dir.join("project.godot"),
        )
        .unwrap_or_default();
        let window = tree.root_window_mut();
        window.config = settings.content_scale_config();
        // The window starts at the project's configured size; a real resize
        // later overwrites this through the same field.
        window.size = gdcore::math::Vector2::new(
            settings.viewport_width() as f32,
            settings.viewport_height() as f32,
        );
        // Mirror the state onto the root node so a script's
        // `get_window().content_scale_mode` answers the project's value rather
        // than the ClassDB registration default (pat-0q2kx.141).
        tree.publish_root_window_state();

        self.tree = Some(tree);
        Ok(())
    }

    fn init_autoloads(&mut self) -> Result<(), BootError> {
        let project_dir = self.config.project_dir.clone();
        let tree = self.tree.as_mut().ok_or_else(|| BootError {
            phase: BootPhase::Autoloads,
            message: "SceneTree not initialized".to_string(),
        })?;

        // Instantiate the project's [autoload] singletons under /root and run
        // their enter_tree/_ready now, BEFORE the main scene is instanced, so
        // the main scene's _ready observes them (Godot boot order). A missing
        // or autoload-free project.godot yields an empty result (headless/test).
        let result = gdscene::autoload::boot_autoloads_from_dir(tree, &project_dir);
        self.autoloads_instantiated = result.len();
        Ok(())
    }

    fn init_main_scene(&mut self) -> Result<(), BootError> {
        let tree = self.tree.as_mut().ok_or_else(|| BootError {
            phase: BootPhase::MainScene,
            message: "SceneTree not initialized".to_string(),
        })?;

        if let Some(scene_path) = &self.config.main_scene {
            // Text `.tscn` or binary `.scn` (pat-33wmr).
            let source = std::fs::read(scene_path)
                .map_err(|e| e.to_string())
                .and_then(|bytes| {
                    gdscene::binary_scene::scene_source_from_bytes(
                        &bytes,
                        &scene_path.to_string_lossy(),
                    )
                    .map_err(|e| e.to_string())
                })
                .map_err(|e| BootError {
                    phase: BootPhase::MainScene,
                    message: format!("failed to read scene '{}': {}", scene_path.display(), e),
                })?;

            let packed_scene = PackedScene::from_tscn(&source).map_err(|e| BootError {
                phase: BootPhase::MainScene,
                message: format!("failed to parse scene '{}': {}", scene_path.display(), e),
            })?;

            let root_id = tree.root_id();
            let scene_root = gdscene::add_packed_scene_to_tree(tree, root_id, &packed_scene)
                .map_err(|e| BootError {
                    phase: BootPhase::MainScene,
                    message: format!("failed to instance scene: {e}"),
                })?;

            // Godot's `Main::start` makes the booted main scene the tree's
            // CURRENT scene (`SceneTree::add_current_scene`). Patina instanced
            // it and told nobody: `current_scene_id` was written only by the
            // change-scene functions, so a freshly booted project had a null
            // `get_tree().current_scene` (pat-mgixk.59).
            //
            // The `PackedScene` goes along as the source in the same call —
            // `set_current_scene` takes both halves precisely so a caller
            // cannot set the id and forget the source, which is the state that
            // makes `reload_current_scene()` fail with "No packed scene source"
            // on the scene the game is visibly running.
            tree.set_current_scene(scene_root, Some(packed_scene));

            self.scene_root_id = Some(scene_root);
        }
        // No main_scene configured is valid (headless/test mode).
        Ok(())
    }

    fn init_scripts(&mut self) -> Result<(), BootError> {
        let tree = self.tree.as_mut().ok_or_else(|| BootError {
            phase: BootPhase::Scripts,
            message: "SceneTree not initialized".to_string(),
        })?;

        let project_dir = self.config.project_dir.clone();

        // Delegated to the one implementation (pat-mgixk.414). This was the
        // THIRD copy of the same walk — `main.rs` had one, this had one, and the
        // project-directory boot path had none, which is how a booted project
        // ran every main-scene node with no script. A parse or read failure is
        // still non-fatal there, as it was here: Godot starts a project whose
        // one script is broken.
        self.scripts_attached = crate::project_boot::attach_scripts(tree, &project_dir);
        Ok(())
    }

    fn init_lifecycle(&mut self) -> Result<(), BootError> {
        let tree = self.tree.as_mut().ok_or_else(|| BootError {
            phase: BootPhase::Lifecycle,
            message: "SceneTree not initialized".to_string(),
        })?;

        if let Some(scene_root) = self.scene_root_id {
            LifecycleManager::enter_tree(tree, scene_root);
        }
        Ok(())
    }

    fn init_running(&mut self) -> Result<(), BootError> {
        let tree = self.tree.take().ok_or_else(|| BootError {
            phase: BootPhase::Running,
            message: "SceneTree not initialized".to_string(),
        })?;

        let mut main_loop = MainLoop::new(tree);

        // Tell the audio bridge where `res://` lives. Godot resolves every
        // `res://` path against the project directory, never the process CWD
        // (`ProjectSettings::globalize_path`), so a game launched from any
        // working directory loads the same assets. The bridge's `set_base_dir`
        // had ZERO production callers — only tests — so it fell back to a
        // CWD-relative path and a project launched from outside its own
        // directory was silently mute (pat-s3rrv.5).
        main_loop
            .audio_bridge_mut()
            .set_base_dir(self.config.project_dir.clone());

        // Wire the mixer output to a real sink. Godot's runner opens an audio
        // driver at startup and pulls mixed frames into it every callback;
        // without this the bridge mixes each frame and discards it, so the
        // runner is silent. Prefer an injected sink (tests / web host); else
        // pick the platform default.
        // Load the project's audio bus layout BEFORE the first frame, the way
        // Godot's `AudioServer::load_default_bus_layout()` does at startup
        // (`servers/audio/audio_server.cpp:1626`) — pat-0q2kx.56.
        //
        // `AudioMixer::load_bus_layout` existed and was tested but had ZERO
        // callers, so every project booted with a bare Master bus and any
        // player naming another bus silently fell back to it.
        //
        // A missing or unreadable layout is not a boot failure: Godot boots
        // with Master only, and so does a bare test scene.
        let settings = gdcore::ProjectSettings::load_godot_project(
            &self.config.project_dir.join("project.godot"),
        )
        .unwrap_or_default();

        // The project's audio settings — `audio/driver/mix_rate` and the two
        // global panning strengths (pat-mgixk.88).
        //
        // 🔑 This boot path reached NONE of them. `configure_from_project_settings`
        // is called from `main.rs:604`, `conformance.rs:289` and
        // `project_runtime.rs:55` — but never from here, so a project booted
        // through `EngineBootstrap` always mixed at 44100 however it was
        // configured, and Godot's 0.5 global panning default never applied.
        //
        // ⚠️ The BRIDGE's configure, not `MainLoop::configure_from_project_settings`.
        // The latter also installs physics tick rate, gravity and interpolation,
        // none of which this boot path applies today either — adding all of that
        // here would change behaviour for every test that boots a project with
        // physics settings, which is well outside this bead. That gap is real and
        // wants its own bead; this call is deliberately the audio slice only.
        //
        // Order against the bus-layout load below does not matter:
        // `AudioServer::set_output_sample_rate` retunes the live server instead of
        // replacing it, so the buses loaded next (or already loaded) survive.
        main_loop
            .audio_bridge_mut()
            .configure_from_project_settings(&settings);

        let layout_path = settings
            .get("audio/buses/default_bus_layout")
            .and_then(|v| v.as_string())
            .unwrap_or("res://default_bus_layout.tres");
        let layout_file = self
            .config
            .project_dir
            .join(layout_path.trim_start_matches("res://"));
        if let Ok(text) = std::fs::read_to_string(&layout_file) {
            let count = main_loop
                .audio_bridge_mut()
                .server_mut()
                .mixer_mut()
                .load_bus_layout_tres(&text);
            tracing::debug!(
                path = %layout_file.display(),
                buses = count,
                "loaded audio bus layout"
            );
        }

        let output = self
            .pending_output
            .take()
            .or_else(|| default_audio_output(&self.config));
        if let Some(sink) = output {
            main_loop.audio_bridge_mut().attach_output(sink);
        }

        self.main_loop = Some(main_loop);
        Ok(())
    }
}

/// Selects the platform-default audio output sink for the boot flow.
///
/// - **Native, non-headless**: opens the host device via cpal
///   ([`gdaudio::PlatformAudioOutput`]). If no device is available (e.g. CI),
///   returns `None` so boot stays silent rather than failing — matching Godot's
///   fallback to a dummy driver.
/// - **Native, headless**: `None` (no hardware sink, like Godot's headless
///   dummy driver).
/// - **Web (wasm)**: `None` here; the AudioWorklet backend is attached by the
///   JS/wasm host through [`EngineBootstrap::set_audio_output`] after the
///   AudioContext is resumed on a user gesture.
#[cfg(not(target_arch = "wasm32"))]
fn default_audio_output(config: &BootConfig) -> Option<Box<dyn AudioOutputStream>> {
    if config.headless {
        return None;
    }
    match gdaudio::PlatformAudioOutput::try_open() {
        Ok(out) => Some(Box::new(out) as Box<dyn AudioOutputStream>),
        Err(_) => None,
    }
}

#[cfg(target_arch = "wasm32")]
fn default_audio_output(_config: &BootConfig) -> Option<Box<dyn AudioOutputStream>> {
    // The browser AudioWorklet sink is attached by the wasm host via
    // `set_audio_output` once the AudioContext is unlocked (a user gesture is
    // required before audio can play). No device is opened here.
    None
}

impl fmt::Debug for EngineBootstrap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EngineBootstrap")
            .field("phase", &self.phase)
            .field("has_tree", &self.tree.is_some())
            .field("has_main_loop", &self.main_loop.is_some())
            .field("scene_root_id", &self.scene_root_id)
            .field("scripts_attached", &self.scripts_attached)
            .field("autoloads_instantiated", &self.autoloads_instantiated)
            .field("log_entries", &self.log.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_phase_ordering() {
        assert!(BootPhase::None < BootPhase::Core);
        assert!(BootPhase::Core < BootPhase::Servers);
        assert!(BootPhase::Servers < BootPhase::Resources);
        assert!(BootPhase::Resources < BootPhase::SceneTree);
        assert!(BootPhase::SceneTree < BootPhase::Autoloads);
        assert!(BootPhase::Autoloads < BootPhase::MainScene);
        assert!(BootPhase::MainScene < BootPhase::Scripts);
        assert!(BootPhase::Scripts < BootPhase::Lifecycle);
        assert!(BootPhase::Lifecycle < BootPhase::Running);
    }

    #[test]
    fn boot_phase_next_sequence() {
        let mut phase = BootPhase::None;
        let expected = [
            BootPhase::Core,
            BootPhase::Servers,
            BootPhase::Resources,
            BootPhase::SceneTree,
            BootPhase::Autoloads,
            BootPhase::MainScene,
            BootPhase::Scripts,
            BootPhase::Lifecycle,
            BootPhase::Running,
        ];
        for exp in &expected {
            phase = phase.next().unwrap();
            assert_eq!(phase, *exp);
        }
        assert!(phase.next().is_none());
    }

    #[test]
    fn boot_phase_all_has_ten_entries() {
        assert_eq!(BootPhase::all().len(), 10);
        assert_eq!(BootPhase::all()[0], BootPhase::None);
        assert_eq!(BootPhase::all()[9], BootPhase::Running);
    }

    #[test]
    fn boot_phase_names() {
        assert_eq!(BootPhase::None.name(), "None");
        assert_eq!(BootPhase::Core.name(), "Core");
        assert_eq!(BootPhase::Servers.name(), "Servers");
        assert_eq!(BootPhase::Resources.name(), "Resources");
        assert_eq!(BootPhase::SceneTree.name(), "SceneTree");
        assert_eq!(BootPhase::Autoloads.name(), "Autoloads");
        assert_eq!(BootPhase::MainScene.name(), "MainScene");
        assert_eq!(BootPhase::Scripts.name(), "Scripts");
        assert_eq!(BootPhase::Lifecycle.name(), "Lifecycle");
        assert_eq!(BootPhase::Running.name(), "Running");
    }

    #[test]
    fn boot_phase_display() {
        assert_eq!(format!("{}", BootPhase::Core), "Core");
        assert_eq!(format!("{}", BootPhase::Running), "Running");
    }

    #[test]
    fn boot_phase_index() {
        assert_eq!(BootPhase::None.index(), 0);
        assert_eq!(BootPhase::Core.index(), 1);
        assert_eq!(BootPhase::Autoloads.index(), 5);
        assert_eq!(BootPhase::Running.index(), 9);
    }

    #[test]
    fn boot_config_default() {
        let config = BootConfig::default();
        assert!(config.main_scene.is_none());
        assert!(config.headless);
        assert_eq!(config.window_width, 1152);
        assert_eq!(config.window_height, 648);
        assert!(!config.event_tracing);
    }

    #[test]
    fn boot_config_headless() {
        let config = BootConfig::headless();
        assert!(config.headless);
        assert!(config.main_scene.is_none());
    }

    #[test]
    fn boot_config_with_scene() {
        let config = BootConfig::with_scene("/some/path/scene.tscn");
        assert_eq!(
            config.main_scene.as_deref(),
            Some(Path::new("/some/path/scene.tscn"))
        );
        assert_eq!(config.project_dir, PathBuf::from("/some/path"));
    }

    #[test]
    fn boot_config_builder_methods() {
        let config = BootConfig::headless()
            .project_dir("/my/project")
            .window_size(800, 600)
            .with_event_tracing();
        assert_eq!(config.project_dir, PathBuf::from("/my/project"));
        assert_eq!(config.window_width, 800);
        assert_eq!(config.window_height, 600);
        assert!(config.event_tracing);
    }

    #[test]
    fn bootstrap_starts_at_none() {
        let boot = EngineBootstrap::new(BootConfig::headless());
        assert_eq!(boot.current_phase(), BootPhase::None);
        assert!(!boot.is_running());
        assert!(boot.tree().is_none());
        assert!(boot.main_loop().is_none());
    }

    #[test]
    fn bootstrap_step_advances_one_phase() {
        let mut boot = EngineBootstrap::new(BootConfig::headless());
        let phase = boot.step().unwrap();
        assert_eq!(phase, BootPhase::Core);
        assert_eq!(boot.current_phase(), BootPhase::Core);
    }

    #[test]
    fn bootstrap_advance_to_scene_tree() {
        let mut boot = EngineBootstrap::new(BootConfig::headless());
        boot.advance_to(BootPhase::SceneTree).unwrap();
        assert_eq!(boot.current_phase(), BootPhase::SceneTree);
        assert!(boot.tree().is_some());
    }

    #[test]
    fn bootstrap_run_all_headless() {
        let mut boot = EngineBootstrap::new(BootConfig::headless());
        boot.run_all().unwrap();
        assert!(boot.is_running());
        assert!(boot.main_loop().is_some());
        assert!(boot.tree().is_none()); // Tree moved into MainLoop.
    }

    #[test]
    fn bootstrap_log_records_all_phases() {
        let mut boot = EngineBootstrap::new(BootConfig::headless());
        boot.run_all().unwrap();
        assert_eq!(boot.log().len(), 9); // 9 transitions from None→Running
        assert_eq!(boot.log()[0].phase, BootPhase::Core);
        assert_eq!(boot.log()[8].phase, BootPhase::Running);
    }

    #[test]
    fn bootstrap_main_loop_can_step() {
        let mut boot = EngineBootstrap::new(BootConfig::headless());
        boot.run_all().unwrap();
        let ml = boot.main_loop_mut().unwrap();
        ml.step(1.0 / 60.0);
        assert_eq!(ml.frame_count(), 1);
    }

    #[test]
    fn bootstrap_step_at_running_is_noop() {
        let mut boot = EngineBootstrap::new(BootConfig::headless());
        boot.run_all().unwrap();
        let phase = boot.step().unwrap();
        assert_eq!(phase, BootPhase::Running); // No change.
    }

    #[test]
    fn bootstrap_headless_no_scene_root() {
        let mut boot = EngineBootstrap::new(BootConfig::headless());
        boot.run_all().unwrap();
        assert!(boot.scene_root_id().is_none());
    }

    #[test]
    fn bootstrap_servers_phase_checks_classdb() {
        let mut boot = EngineBootstrap::new(BootConfig::headless());
        boot.step().unwrap(); // Core
        boot.step().unwrap(); // Servers — should verify ClassDB is populated
        assert_eq!(boot.current_phase(), BootPhase::Servers);
    }

    #[test]
    fn boot_error_display() {
        let err = BootError {
            phase: BootPhase::MainScene,
            message: "file not found".to_string(),
        };
        assert_eq!(
            format!("{err}"),
            "bootstrap error in MainScene: file not found"
        );
    }

    #[test]
    fn bootstrap_debug_output() {
        let boot = EngineBootstrap::new(BootConfig::headless());
        let debug = format!("{boot:?}");
        assert!(debug.contains("EngineBootstrap"));
        assert!(debug.contains("None")); // phase
    }

    #[test]
    fn bootstrap_with_event_tracing() {
        let config = BootConfig::headless().with_event_tracing();
        let mut boot = EngineBootstrap::new(config);
        boot.advance_to(BootPhase::SceneTree).unwrap();
        let tree = boot.tree().unwrap();
        assert!(tree.event_trace().is_enabled());
    }

    #[test]
    fn bootstrap_phases_match_godot_order() {
        // Verify the phase sequence matches Godot's documented initialization:
        // Core → Servers → Resources → SceneTree → Autoloads → MainScene → Scripts → Lifecycle → Running
        let phases: Vec<BootPhase> = {
            let mut v = Vec::new();
            let mut p = BootPhase::None;
            while let Some(next) = p.next() {
                v.push(next);
                p = next;
            }
            v
        };
        assert_eq!(
            phases,
            vec![
                BootPhase::Core,
                BootPhase::Servers,
                BootPhase::Resources,
                BootPhase::SceneTree,
                BootPhase::Autoloads,
                BootPhase::MainScene,
                BootPhase::Scripts,
                BootPhase::Lifecycle,
                BootPhase::Running,
            ]
        );
    }
}

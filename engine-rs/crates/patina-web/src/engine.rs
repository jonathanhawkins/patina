//! The web runtime's engine handle: a `MainLoop` driven by animation-frame
//! timestamps (pat-s3rrv.40).
//!
//! `gdscene::frame_driver::FrameDriver` already implements everything a browser
//! frame loop needs — coalescing callbacks that arrive inside the frame budget,
//! carrying the leftover delta rather than dropping it, clamping a long stall so
//! a backgrounded tab does not simulate minutes at once, and throttling while
//! hidden. It had **zero production callers**: the only references outside its
//! own module were in its own test, so nothing in the shipped web runtime
//! advanced a frame at all.
//!
//! This handle is deliberately **wasm-independent** — it takes a timestamp in
//! milliseconds and nothing else. `requestAnimationFrame` hands its callback
//! exactly that, so the browser entry is a one-line adapter, and the whole frame
//! pipeline stays testable on the host with synthetic timestamps instead of only
//! in a browser.

use gdaudio::server::AudioServer;
use gdaudio::web_output::SharedWebAudioOutput;
use gdscene::frame_driver::FrameDriver;
use gdscene::main_loop::{FrameOutput, MainLoop};
use gdscene::scene_tree::SceneTree;

use crate::audio::SceneAudioRing;

/// The 3D render server held across frames, plus the size it was built for.
///
/// 🔑 The size is stored so the adapter can be REUSED. Godot keeps one
/// `RenderingServer` for the process and draws from it in every
/// `Main::iteration`; rebuilding it per frame would discard its mesh and texture
/// caches each time, so the per-frame cost would grow with the scene instead of
/// staying flat. A canvas resize is the only thing that forces a new one,
/// because the adapter's framebuffer is fixed at construction.
struct Renderer3D {
    width: u32,
    height: u32,
    adapter: gdscene::render_server_3d::RenderServer3DAdapter,
}

/// A running scene plus the driver that advances it.
pub struct WebEngine {
    main_loop: MainLoop,
    driver: FrameDriver,
    /// Built on the first 3D frame, not at construction: a 2D-only project must
    /// not pay for a renderer it never draws through.
    renderer_3d: Option<Renderer3D>,
}

impl WebEngine {
    /// Wraps an already-populated scene tree.
    ///
    /// 🔑 Populates the process-global ClassDB first (pat-mgixk.74). This is the
    /// web runtime's equivalent of the boot step every other entry point
    /// performs — `patina-runner`'s `project_boot::boot_from_project_text` and
    /// `EngineBootstrap::init_servers` both call this, mirroring Godot's
    /// `register_scene_types()` during server init.
    ///
    /// ⚠️ Without it the browser renders a BLANK canvas no matter what else is
    /// correct, and silently: `canvas_render::collect_canvas_items` keeps a node
    /// only `if node.is_class("CanvasItem")`, that answer comes from the global
    /// ClassDB, and `ClassDB::new()` starts empty — so the render list is empty
    /// and every frame is the clear colour. The gate is spelled negatively, so
    /// an unpopulated DB drops the whole scene rather than degrading.
    ///
    /// Safe to call on every construction: registration writes into a
    /// name-keyed map, so repeats overwrite rather than accumulate, which is why
    /// the runner's two boot paths can both call it in one process.
    pub fn new(tree: SceneTree) -> Self {
        gdobject::class_db::register_all_classes();
        Self {
            main_loop: MainLoop::new(tree),
            driver: FrameDriver::new(),
            renderer_3d: None,
        }
    }

    /// Boots the project out of the mounted `.pck` and returns an engine
    /// driving its main scene (pat-mgixk.73).
    ///
    /// Godot's web export mounts the pack and then runs the ordinary
    /// `Main::setup`/`Main::start`, so the project's main scene, autoloads and
    /// `[input]` map are live on the first frame
    /// (`upstream/godot/platform/web/web_main.cpp`). Patina's `start_engine`
    /// built a bare `Node2D` tree and never read the pack the export shell had
    /// just mounted, so an exported game ran an empty scene — no `_ready`, no
    /// `_process`, no input map.
    ///
    /// # Ordering
    ///
    /// ⚠️ The lifecycle runs HERE, not inside
    /// [`gdscene::res_boot::boot_from_res`], and autoloads enter before the main
    /// scene. Godot guarantees an autoload is in the tree before the main
    /// scene's `_ready` can reach it, so entering the main scene first would
    /// break `get_node("/root/MyAutoload")` in `_ready` — and it would break it
    /// only for projects that use autoloads, which is the worst kind of
    /// intermittent.
    ///
    /// ⚠️ `configure_from_project_settings` and `set_input_map` run AFTER the
    /// tree is assembled but BEFORE any frame, matching
    /// `run_booted_project` in the native runner. Installing the input map
    /// later would drop the first frame's input; configuring the tick rate
    /// later would run frame 0 at the engine default.
    ///
    /// The error is a `String` naming the stage that failed, so the JS binding
    /// can surface "no project.godot in the mounted pack" instead of a blank
    /// canvas.
    pub fn boot_mounted_project() -> Result<Self, String> {
        let booted = gdscene::res_boot::boot_from_res()?;

        // `WebEngine::new` populates the ClassDB, which `boot_from_res` has
        // already done — it is idempotent, and going through `new` keeps this
        // path from drifting out of step with however `new` is configured.
        let mut engine = Self::new(booted.tree);

        // 🔑 CONFIGURE BEFORE ENTERING THE TREE. `LifecycleManager::enter_tree`
        // dispatches `_ready` (it calls `process_script_ready` in its READY
        // phase, lifecycle.rs:119), so anything a script reads there must
        // already be installed.
        //
        // Godot's order is exactly this: `Main::setup()` applies the project's
        // settings — `Engine::set_physics_ticks_per_second` among them — and
        // only then does `Main::start()` instance and enter the main scene
        // (`platform/web/web_main.cpp`). Configuring afterwards meant a script
        // reading `Engine.physics_ticks_per_second` in `_ready` saw the engine
        // default 60 instead of the project's value, and
        // `Input.is_action_pressed` in `_ready` resolved against an empty map.
        // `MainLoop::new` is a pure constructor and dispatches nothing, so
        // building it first costs nothing.
        //
        // ⚠️ `register_physics_bodies` stays AFTER the tree is entered: it
        // registers the bodies that are in the tree, so hoisting it with the
        // rest would find none.
        engine
            .main_loop
            .configure_from_project_settings(&booted.settings);
        engine.main_loop.set_input_map(booted.input_map);

        // Autoloads first, then the main scene — Godot boots them in that
        // order, which is what lets a main scene's `_ready` reach an autoload.
        for (_, node_id) in &booted.autoload_roots {
            gdscene::lifecycle::LifecycleManager::enter_tree(engine.main_loop.tree_mut(), *node_id);
        }
        if let Some(main_root) = booted.main_scene_root {
            gdscene::lifecycle::LifecycleManager::enter_tree(
                engine.main_loop.tree_mut(),
                main_root,
            );
        }

        engine.main_loop.register_physics_bodies();
        Ok(engine)
    }

    /// Advances the engine for an animation-frame callback at `now_ms`.
    ///
    /// Returns the frame's output, or `None` when this callback produced no
    /// step — the first callback (which only establishes the time origin), or
    /// one that arrived inside the frame budget. A `None` is not lost time: the
    /// delta is carried into the next step.
    ///
    /// `now_ms` is the browser's `DOMHighResTimeStamp`, so this is exactly the
    /// value `requestAnimationFrame` passes its callback.
    pub fn frame(&mut self, now_ms: f64) -> Option<FrameOutput> {
        self.driver.tick(now_ms, &mut self.main_loop)
    }

    /// Marks the tab hidden or visible.
    ///
    /// A hidden tab is throttled rather than stopped, which is what keeps a
    /// backgrounded game's timers advancing at a reduced rate instead of
    /// freezing and then lurching forward on return.
    pub fn set_hidden(&mut self, hidden: bool) {
        self.driver.set_hidden(hidden);
    }

    /// Whether the tab is currently marked hidden.
    pub fn is_hidden(&self) -> bool {
        self.driver.is_hidden()
    }

    /// Caps the frame rate (Godot's `Engine.max_fps`).
    pub fn set_max_fps(&mut self, fps: u32) {
        self.driver.set_max_fps(fps);
    }

    /// Total simulated time in seconds, after clamping and throttling.
    pub fn elapsed(&self) -> f64 {
        self.driver.elapsed()
    }

    /// Drops banked time and forgets the time origin — for re-entering the loop
    /// after a long pause that should not be simulated.
    pub fn reset_timing(&mut self) {
        self.driver.reset();
    }

    /// Feeds a translated browser event into the engine (pat-s3rrv.41).
    ///
    /// Goes through `MainLoop::push_event`, which both queues the event for
    /// `_input` dispatch on the next step AND updates the pollable state behind
    /// `Input.is_action_pressed`. Feeding `InputState` directly would give a
    /// script the action but never the event.
    pub fn push_input(&mut self, event: gdplatform::input::InputEvent) {
        self.main_loop.push_event(event);
    }

    /// Feeds several events in order — e.g. the press/release pair a wheel
    /// notch produces.
    pub fn push_inputs(&mut self, events: impl IntoIterator<Item = gdplatform::input::InputEvent>) {
        for e in events {
            self.main_loop.push_event(e);
        }
    }

    /// The scene being driven.
    pub fn tree(&self) -> &SceneTree {
        self.main_loop.tree()
    }

    /// Renders the current 3D scene into an RGBA8 buffer of `width` x `height`,
    /// row-major, four bytes a pixel (pat-mgixk.49).
    ///
    /// 🔑 This is the 3D counterpart of [`Self::render_frame_rgba`], and the web
    /// half of the same "one renderer, two runtime drivers" split: it drives the
    /// same `RenderServer3DAdapter::render_frame` the native runner's
    /// `--render-to` does, from the scene's own `Camera3D`. Before it, this
    /// crate's entire presentation surface was `WebRenderer::present_clear` — a
    /// flat colour — so a 3D game in the browser produced no scene pixel at all,
    /// while the 2D path had had `render_frame_rgba` since pat-mgixk.74.
    ///
    /// Call it from the animation-frame tick, after [`Self::frame`], so the
    /// pixels are the frame that was just stepped.
    ///
    /// ⚠️ Returns PIXELS, and does not present them, exactly as the 2D frame
    /// does. The host decides: a 2D canvas context takes these straight through
    /// `putImageData`, while the WebGPU surface in `crate::canvas` would need a
    /// texture upload and a blit pipeline it does not have. Handing back the
    /// buffer keeps that choice out of the engine rather than guessing at it.
    ///
    /// ⚠️ Deliberately NOT folded into `render_frame_rgba`. Compositing 2D over
    /// 3D needs the canvas rasterizer to report which pixels it actually covered
    /// — its clear is opaque, so "equals the clear colour" cannot tell an
    /// untouched pixel from a black sprite. Returning the two frames separately
    /// lets the host stack them, and leaves every existing 2D caller's output
    /// byte-for-byte unchanged.
    pub fn render_3d_rgba(&mut self, width: u32, height: u32) -> Vec<u8> {
        let width = width.max(1);
        let height = height.max(1);
        // `take` rather than `as_mut`: moving the renderer out for the duration
        // of the render keeps the adapter borrow and the `self.main_loop` borrow
        // plainly disjoint instead of relying on field-splitting.
        let mut renderer = self
            .renderer_3d
            .take()
            .filter(|r| r.width == width && r.height == height)
            .unwrap_or_else(|| Renderer3D {
                width,
                height,
                adapter: gdscene::render_server_3d::RenderServer3DAdapter::new(width, height),
            });
        let (_snapshot, frame) = renderer.adapter.render_frame(self.main_loop.tree());
        let mut out = Vec::with_capacity((width as usize) * (height as usize) * 4);
        for px in &frame.pixels {
            // `putImageData` wants straight (non-premultiplied) sRGB bytes in
            // RGBA order, which is the order `Color` already stores — the same
            // conversion the 2D frames below use.
            let to_u8 = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
            out.extend_from_slice(&[to_u8(px.r), to_u8(px.g), to_u8(px.b), to_u8(px.a)]);
        }
        self.renderer_3d = Some(renderer);
        out
    }

    /// Rasterizes the current 2D scene into an RGBA8 buffer of `width` x
    /// `height`, row-major, four bytes a pixel (pat-mgixk.28).
    ///
    /// 🔑 This is the web half of "one canvas builder, two runtime drivers".
    /// The node-to-draw-command producer used to live only in
    /// `gdeditor::viewport_2d_render`, and this crate depended on neither
    /// `gdeditor` nor `gdrender2d` — `WebRenderer::present_clear` was its only
    /// presentation call, so a 2D game in the browser drew a flat colour and
    /// nothing else. The builder now lives in `gdscene::canvas_render` and this
    /// drives exactly the same four calls `patina-runner --frame-out` does.
    ///
    /// ⚠️ Returns PIXELS, and does not present them. The host decides: a 2D
    /// canvas context can take these straight through `putImageData`, while the
    /// WebGPU surface in `crate::canvas` would need a texture upload and a blit
    /// pipeline it does not have yet. Handing back the buffer keeps that choice
    /// out of the engine instead of guessing at it — and the guess would be the
    /// expensive one to undo.
    ///
    /// ⚠️ `res://` texture paths resolve against `project_root`. An empty root
    /// means `resolve_res_path` falls back to a CWD-relative path, which in a
    /// browser resolves nothing — so a caller that wants textures must pass the
    /// mounted project directory.
    pub fn render_2d_rgba(&mut self, width: u32, height: u32, project_root: &str) -> Vec<u8> {
        use gdcore::math::{Color, Vector2};

        let tree = self.main_loop.tree();
        let mut items = gdscene::canvas_render::collect_canvas_items(tree);
        if let Some(view) = gdscene::canvas_render::camera2d_view_transform(
            tree,
            Vector2::new(width as f32, height as f32),
            None,
            0.0,
        ) {
            gdscene::canvas_render::apply_camera2d_view(&mut items, view);
        }
        let mut textures = gdscene::texture_cache::TextureCache::new(project_root.to_string());
        let frame = gdscene::canvas_render::rasterize_render_list(
            &items,
            width,
            height,
            Color::new(0.0, 0.0, 0.0, 1.0),
            Some(&mut textures),
        );
        let mut out = Vec::with_capacity((width as usize) * (height as usize) * 4);
        for px in &frame.pixels {
            // `putImageData` wants straight (non-premultiplied) sRGB bytes in
            // RGBA order, which is the order `Color` already stores.
            let to_u8 = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
            out.extend_from_slice(&[to_u8(px.r), to_u8(px.g), to_u8(px.b), to_u8(px.a)]);
        }
        out
    }

    /// Renders the running scene into a `canvas_w` x `canvas_h` RGBA buffer,
    /// honouring the project's `display/window/stretch/*` settings
    /// (pat-mgixk.74).
    ///
    /// 🔑 This is [`Self::render_2d_rgba`]'s stretch-aware replacement. That one
    /// renders the scene AT the size it is handed, so on a 640x480 canvas a
    /// 320x180 project drew its nodes at project coordinates in the top-left
    /// corner at 1:1 and left the rest blank — the canvas was bigger, not the
    /// content. Nothing scaled and nothing was letterboxed.
    ///
    /// The transform comes from `RootWindow::stretch_transform()`, whose
    /// counterpart `window_to_canvas()` already describes itself as "the inverse
    /// of the transform the renderer applies". The renderer simply never applied
    /// one; both halves existed and were never joined.
    pub fn render_frame_rgba(
        &mut self,
        canvas_w: u32,
        canvas_h: u32,
        project_root: &str,
    ) -> Vec<u8> {
        use gdcore::math::{Color, Vector2};

        // The window has to know the real backing size first: every value below
        // is derived from it, so computing the stretch against a stale size
        // would letterbox for the wrong canvas.
        let canvas_size = Vector2::new(canvas_w as f32, canvas_h as f32);
        self.main_loop.tree_mut().root_window_mut().size = canvas_size;
        let stretch = self.main_loop.tree().root_window().stretch_transform();
        // ⚠️ The camera is resolved against the PROJECT space (`visible_size`),
        // not the canvas. Passing the canvas size here would centre the camera
        // on the stretched viewport and then stretch it again.
        let visible = self.main_loop.tree().root_window().visible_size();

        let tree = self.main_loop.tree();
        let mut items = gdscene::canvas_render::collect_canvas_items(tree);
        if let Some(view) =
            gdscene::canvas_render::camera2d_view_transform(tree, visible, None, 0.0)
        {
            gdscene::canvas_render::apply_camera2d_view(&mut items, view);
        }
        // ⚠️ Prepended UNCONDITIONALLY, which is why this is a plain loop and
        // not a second `apply_camera2d_view` call. That helper deliberately
        // exempts `CanvasFollow::Fixed` items so a HUD layer does not pan with
        // the camera — correct for a camera, wrong for the stretch: a HUD that
        // skipped the stretch would draw at 1:1 over scaled content.
        for item in items.iter_mut() {
            item.global_transform = stretch * item.global_transform;
        }

        let mut textures = gdscene::texture_cache::TextureCache::new(project_root.to_string());
        // Rasterized at the CANVAS size with an opaque black clear, so the
        // letterbox bars need no special case: they are simply the region no
        // stretched item covers. Godot's bars are black too.
        let frame = gdscene::canvas_render::rasterize_render_list(
            &items,
            canvas_w,
            canvas_h,
            Color::new(0.0, 0.0, 0.0, 1.0),
            Some(&mut textures),
        );
        let mut out = Vec::with_capacity((canvas_w as usize) * (canvas_h as usize) * 4);
        for px in &frame.pixels {
            let to_u8 = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
            out.extend_from_slice(&[to_u8(px.r), to_u8(px.g), to_u8(px.b), to_u8(px.a)]);
        }
        out
    }

    /// Canvas (backing) pixels -> project coordinates, for mapping pointer
    /// events under stretch (pat-mgixk.74).
    ///
    /// The exact inverse of what [`Self::render_frame_rgba`] applies, taken from
    /// the same `RootWindow`, so a click always lands on the node drawn under
    /// the cursor. ⚠️ Call it only after a frame has been rendered at the
    /// current canvas size, since that is what sets the window size the
    /// transform is derived from.
    pub fn canvas_to_project(&self, canvas_point: gdcore::math::Vector2) -> gdcore::math::Vector2 {
        self.main_loop
            .tree()
            .root_window()
            .window_to_canvas(canvas_point)
    }

    /// Mutable access to the scene, for wiring a scene up after construction.
    pub fn tree_mut(&mut self) -> &mut SceneTree {
        self.main_loop.tree_mut()
    }

    /// The underlying loop, for callers needing physics counters or settings.
    pub fn main_loop(&self) -> &MainLoop {
        &self.main_loop
    }

    /// Mutable access to the loop, e.g. to set `physics_ticks_per_second`.
    pub fn main_loop_mut(&mut self) -> &mut MainLoop {
        &mut self.main_loop
    }

    /// Routes this scene's audio into a ring an `AudioWorkletProcessor` can
    /// drain, and hands back the consumer end (pat-mgixk.4).
    ///
    /// Godot's web export runs exactly ONE `AudioServer` and lets the worklet
    /// pull mixed frames from it (`platform/web/audio_driver_web.cpp`), so every
    /// `AudioStreamPlayer` in a running scene is audible once a gesture has
    /// unlocked the context. Patina's web runtime attached *nothing*:
    /// `AudioServerBridge::process` mixed the whole scene into `last_mix` and
    /// discarded it, while the page's `pullBlock` drained a separate demo mixer
    /// that no scene node ever reached. A web game was therefore silent. This
    /// call is the missing `attach_output`.
    ///
    /// `context_rate` is the rate the browser's `AudioContext` reported; `0`
    /// (a context that has not started yet) falls back to 48 kHz rather than
    /// making the resample ratio degenerate. `mix_rate` is the project's
    /// `audio/driver/mix_rate`; pass `0` to keep whatever rate the bridge's
    /// server already runs at.
    ///
    /// ⚠️ Call this **before the first frame**. Honouring a mix rate the
    /// bridge was not built with means replacing its `AudioServer`, which drops
    /// any voice already playing — unobservable at startup, an audible cut
    /// mid-game.
    pub fn attach_audio_output(&mut self, context_rate: u32, mix_rate: u32) -> SceneAudioRing {
        // A degenerate ratio would resample the whole mix to nothing.
        let context_rate = if context_rate == 0 {
            48_000
        } else {
            context_rate
        };
        if mix_rate != 0 && self.main_loop.audio_bridge().server().output_sample_rate() != mix_rate
        {
            *self.main_loop.audio_bridge_mut().server_mut() =
                AudioServer::with_sample_rate(mix_rate);
        }
        // Both read back off the server rather than assumed:
        // `with_sample_rate` clamps what it is given, and the channel count is
        // the mixer's alone. De-interleaving the ring at a different width than
        // it was written with is heard as a garbled stereo image rather than as
        // a failure, so it must come from the one place that decides it.
        let effective_mix_rate = self.main_loop.audio_bridge().server().output_sample_rate();
        let channels = self
            .main_loop
            .audio_bridge()
            .server()
            .output_channels()
            .max(1);
        let ring = SharedWebAudioOutput::new(context_rate, effective_mix_rate, channels);
        self.main_loop
            .audio_bridge_mut()
            .attach_output(Box::new(ring.clone()));
        SceneAudioRing::new(ring)
    }
}

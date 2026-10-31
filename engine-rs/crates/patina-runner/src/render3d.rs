//! The runner's per-frame 3D scene render (pat-s3rrv.32).
//!
//! # Why this module exists
//!
//! `patina-runner` documented itself as a headless runner that "dumps the final
//! tree state as JSON to stdout", and it meant it: `grep gdrender3d` across the
//! crate returned nothing and its `Cargo.toml` named no render crate. A project
//! run through the runner produced no pixels at all. patina-web was no better —
//! its whole render surface was `present_clear(r, g, b)`, a solid-colour fill
//! with no scene submission, camera, or instance list.
//!
//! The renderer was not missing, only unreachable from here.
//! `RenderServer3DAdapter` (gdscene) already walks a `SceneTree`, builds the
//! instance list and drives `SoftwareRenderer3D` — and it had **no production
//! callers at all**, only tests. Since `patina-runner` already depends on
//! `gdscene`, which already depends on `gdrender3d`, wiring it needs no new
//! dependency: the pieces were one call apart.
//!
//! # Shared with the web canvas
//!
//! [`render_scene_frame`] is deliberately the single entry point, taking a
//! `SceneTree` and returning pixels, so patina-web's canvas render and the
//! native runner produce frames through the same code rather than growing two
//! render paths that drift.

use gdcore::math::Color;
use gdrender2d::FrameBuffer;
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;

/// A rendered frame: `width * height` pixels in row-major, top-down order.
#[derive(Debug, Clone, PartialEq)]
pub struct RunnerFrame3D {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Row-major, top-down linear colours.
    pub pixels: Vec<Color>,
}

impl RunnerFrame3D {
    /// Number of pixels differing from `background` by more than a quantisation
    /// step.
    ///
    /// This is the runner's answer to "did the scene actually draw anything",
    /// which is otherwise invisible in a headless process. A tolerance rather
    /// than equality because the renderer works in linear HDR and the
    /// post-process tonemaps, so a cleared pixel is not bit-identical to the
    /// requested clear colour.
    pub fn coverage(&self, background: Color) -> usize {
        self.pixels
            .iter()
            .filter(|c| {
                (c.r - background.r).abs() > 1.0 / 255.0
                    || (c.g - background.g).abs() > 1.0 / 255.0
                    || (c.b - background.b).abs() > 1.0 / 255.0
            })
            .count()
    }

    /// The colour at `(x, y)`, or black outside the frame.
    ///
    /// Exists so a caller can ask about one pixel — "is the centre of the frame
    /// the colour of the box I put in front of the camera" — without indexing
    /// `pixels` and getting the stride arithmetic wrong.
    pub fn pixel(&self, x: u32, y: u32) -> Color {
        if x >= self.width || y >= self.height {
            return Color::new(0.0, 0.0, 0.0, 1.0);
        }
        self.pixels[(y * self.width + x) as usize]
    }

    /// The frame as a [`FrameBuffer`], the representation every image encoder in
    /// `gdrender2d` takes.
    pub fn to_frame_buffer(&self) -> FrameBuffer {
        let mut fb = FrameBuffer::new(
            self.width.max(1),
            self.height.max(1),
            Color::new(0.0, 0.0, 0.0, 1.0),
        );
        let n = fb.pixels.len().min(self.pixels.len());
        fb.pixels[..n].copy_from_slice(&self.pixels[..n]);
        fb
    }

    /// Writes the frame to `path` as a PNG.
    ///
    /// 🔑 This is what makes a headless run observable at all. The runner
    /// previously dumped the tree as JSON and nothing else, so "did the scene
    /// render" had no answer outside the editor process (pat-mgixk.49).
    pub fn save_png(&self, path: &str) -> std::io::Result<()> {
        gdrender2d::save_png(&self.to_frame_buffer(), path)
    }
}

/// One 3D renderer for a whole run.
///
/// 🔑 The point of the type is that the [`RenderServer3DAdapter`] is built ONCE
/// and reused. [`render_scene_frame`] constructs a fresh adapter per call, which
/// is fine for a one-shot screenshot and wrong for a frame loop: every frame
/// would throw away the adapter's mesh and texture caches and re-derive them, so
/// the per-frame cost grows with the scene instead of staying flat. Godot keeps
/// one `RenderingServer` for the process and draws from it every iteration
/// (`Main::iteration`).
///
/// Holding the adapter is also what lets a caller ask for [`last_frame`] after
/// the loop ends — the runner writes its image from that rather than rendering a
/// second, separate frame that nothing on screen ever corresponded to.
///
/// [`last_frame`]: Self::last_frame
pub struct SceneRenderer3D {
    adapter: RenderServer3DAdapter,
    width: u32,
    height: u32,
    last: Option<RunnerFrame3D>,
}

impl SceneRenderer3D {
    /// A renderer producing `width` x `height` frames for the rest of the run.
    pub fn new(width: u32, height: u32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            adapter: RenderServer3DAdapter::new(width, height),
            width,
            height,
            last: None,
        }
    }

    /// Frame width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Frame height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Renders `tree` and keeps the result as the run's latest frame.
    pub fn render(&mut self, tree: &SceneTree) -> &RunnerFrame3D {
        let (_snapshot, frame) = self.adapter.render_frame(tree);
        self.last = Some(RunnerFrame3D {
            width: frame.width,
            height: frame.height,
            pixels: frame.pixels,
        });
        // Just assigned, so the expect cannot fire.
        self.last.as_ref().expect("frame was just stored")
    }

    /// The most recently rendered frame, or `None` before the first render.
    pub fn last_frame(&self) -> Option<&RunnerFrame3D> {
        self.last.as_ref()
    }
}

/// Renders one frame of `tree`'s 3D scene at `width` x `height`.
///
/// The camera, lights, meshes and materials come from the scene itself, via the
/// same `RenderServer3DAdapter` walk the editor's server-side renderer uses — so
/// a scene renders the same way whether it is running under the runner or being
/// previewed.
pub fn render_scene_frame(tree: &SceneTree, width: u32, height: u32) -> RunnerFrame3D {
    let mut adapter = RenderServer3DAdapter::new(width, height);
    let (_snapshot, frame) = adapter.render_frame(tree);
    RunnerFrame3D {
        width: frame.width,
        height: frame.height,
        pixels: frame.pixels,
    }
}

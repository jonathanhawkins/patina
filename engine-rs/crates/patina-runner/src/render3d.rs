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

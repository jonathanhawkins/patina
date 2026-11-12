//! The Play (game) view of a running 3D scene (pat-p3p1m).
//!
//! The editor binary's render loop used to build this inline, which left the
//! decision "what does Play draw for a 3D scene" untestable — and it was wrong
//! in the one case nobody had a fixture for: a scene with **no current
//! Camera3D** fell back to a clone of the *editor* viewport and its editor
//! render inputs, so the Game tab showed the editor's grid, origin axes,
//! collision wireframes and selection outlines, framed by the editor's orbit
//! camera. The official 3D platformer demo hit exactly that while its Player
//! (which carries the camera) failed to instance.
//!
//! Godot draws no World3D at all without a camera: the viewport is cleared to
//! `rendering/environment/defaults/default_clear_color` and the canvas layers
//! are composited over it. That is what [`Play3DScene::NoCamera`] renders.
//!
//! The split mirrors the 2D `prepare_play_frame` / `PreparedPlayFrame`: only
//! [`prepare_play_3d_frame`] reads the tree (run it under the editor-state
//! lock); [`PreparedPlay3DFrame::render`] needs nothing but the snapshot (run it
//! off the lock), including the HUD canvas raster.

use gdcore::math::{Color, Vector2};
use gdrender2d::renderer::{Bgra8Image, FrameBuffer};
use gdscene::canvas_render::Render2DItem;
use gdscene::SceneTree;

use crate::viewport_3d::Viewport3D;
use crate::viewport_3d_render::{self, Editor3DRenderInputs};

/// What the 3D half of a Play frame draws.
pub enum Play3DScene {
    /// Through the scene's current `Camera3D`, with the game's environment and
    /// no editor furniture.
    Camera {
        /// The viewport pinned to the camera.
        vp: Viewport3D,
        /// The game render inputs (editor-only markers and cull-masked items
        /// already stripped).
        inputs: Editor3DRenderInputs,
    },
    /// The editor's own camera, because the user turned on the runtime camera
    /// override (Godot's "Camera Override" button) to fly around a running game.
    EditorCamera {
        /// A clone of the editor viewport sized to the Play view.
        vp: Viewport3D,
        /// The editor render inputs.
        inputs: Editor3DRenderInputs,
    },
    /// No current `Camera3D`: Godot renders nothing of the 3D world and the
    /// viewport shows the clear colour.
    NoCamera {
        /// The project's `default_clear_color` (display sRGB).
        clear: Color,
    },
}

/// Everything a Play frame of a 3D scene needs, snapshotted from the tree.
pub struct PreparedPlay3DFrame {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// The 3D half.
    pub scene: Play3DScene,
    /// The canvas (HUD) items to composite over the 3D frame, already camera-
    /// and stretch-transformed. Rasterized in [`Self::render`], off the lock.
    pub canvas_items: Vec<Render2DItem>,
}

/// Builds the Play frame for a running 3D scene. The only step that reads the
/// tree.
///
/// `editor_vp` is the editor's 3D viewport (used only under the camera
/// override); `clear_color` is the project's `default_clear_color`.
pub fn prepare_play_3d_frame(
    tree: &mut SceneTree,
    editor_vp: &Viewport3D,
    camera_override: bool,
    clear_color: Color,
    width: u32,
    height: u32,
) -> PreparedPlay3DFrame {
    let width = width.max(1);
    let height = height.max(1);
    let scene = if camera_override {
        let mut vp = editor_vp.clone();
        vp.width = width;
        vp.height = height;
        let inputs = viewport_3d_render::editor_3d_render_inputs(&vp, tree);
        Play3DScene::EditorCamera { vp, inputs }
    } else {
        match viewport_3d_render::game_camera_for_play(tree, false) {
            Some(camera) => {
                let mut vp =
                    viewport_3d_render::viewport_for_scene_camera(tree, camera, width, height);
                // Editor furniture off. `viewport_for_scene_camera` starts from
                // an empty selection, so no outline can leak in either.
                vp.grid.visible = false;
                vp.show_origin = false;
                // The scene's own environment in place of the editor preview
                // sky and sun (pat-mgixk.51).
                viewport_3d_render::apply_runtime_environment_3d(tree, camera, &mut vp);
                let inputs = viewport_3d_render::game_3d_render_inputs(tree, camera, &vp);
                Play3DScene::Camera { vp, inputs }
            }
            None => Play3DScene::NoCamera { clear: clear_color },
        }
    };

    // The HUD (pat-mgixk.36): same item order as
    // `scene_renderer::render_runtime_frame` (list -> Camera2D -> window
    // stretch) so both Play paths frame it alike.
    let mut window = *tree.root_window();
    window.size = Vector2::new(width as f32, height as f32);
    // 🔑 The Control pass FIRST, exactly as the 2D `prepare_play_frame` runs it
    // (pat-9ww3e). Without it a 3D game's UI was drawn only by the generic
    // per-node `render_kind_for` walk: no container layout (truck_town's HUD
    // piled up at the window's top-left with the Back button missing), no
    // CheckBox/HSlider/TextureButton theme drawing, and labels at the walk's own
    // metrics. Its layout also writes each Control's rect back onto the node,
    // which the runtime list then reads.
    let root = tree.root_id();
    let controls = crate::viewport_2d_render::build_control_render_items_sourced(
        tree,
        root,
        gdscene::canvas_render::canvas_size_of(&window),
    );
    let runtime = crate::viewport_2d_render::build_runtime_render_list(tree);
    let mut canvas_items = crate::viewport_2d_render::merge_control_items(tree, runtime, controls);
    if let Some(view) = crate::viewport_2d_render::camera2d_view_transform(
        tree,
        gdscene::canvas_render::canvas_size_of(&window),
        None,
        1.0 / 60.0,
    ) {
        crate::viewport_2d_render::apply_camera2d_view(&mut canvas_items, view);
    }
    crate::viewport_2d_render::apply_stretch_transform(
        &mut canvas_items,
        window.stretch_transform(),
    );

    PreparedPlay3DFrame {
        width,
        height,
        scene,
        canvas_items,
    }
}

impl PreparedPlay3DFrame {
    /// `true` when this frame draws the World3D (through any camera).
    pub fn draws_world(&self) -> bool {
        !matches!(self.scene, Play3DScene::NoCamera { .. })
    }

    /// Renders the 3D half, without the canvas.
    pub fn render_world(&self) -> Bgra8Image {
        match &self.scene {
            Play3DScene::Camera { vp, inputs } | Play3DScene::EditorCamera { vp, inputs } => {
                viewport_3d_render::render_editor_3d_frame_gpu(vp, inputs)
            }
            Play3DScene::NoCamera { clear } => solid_bgra(self.width, self.height, *clear),
        }
    }

    /// Rasterizes the HUD canvas with a transparent clear, or `None` when the
    /// scene has no canvas items at all.
    pub fn render_canvas(&self) -> Option<FrameBuffer> {
        if self.canvas_items.is_empty() {
            return None;
        }
        // 🔑 A texture cache, as the 2D Play raster has (pat-9ww3e). With
        // `None` every textured HUD item drew the rasterizer's 32px placeholder
        // square instead: truck_town's turbometer was a white box. One cache per
        // render thread (the editor renders Play on one persistent thread), reset
        // whenever the mounted `res://` root changes so a second project never
        // reads the first one's textures.
        thread_local! {
            static HUD_TEXTURES: std::cell::RefCell<(Option<std::path::PathBuf>, gdscene::texture_cache::TextureCache)> =
                std::cell::RefCell::new((None, gdscene::texture_cache::TextureCache::new(String::new())));
        }
        let root = gdcore::file_access::project_root();
        Some(HUD_TEXTURES.with(|cell| {
            let mut cell = cell.borrow_mut();
            if cell.0 != root {
                let dir = root
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                cell.1 = gdscene::texture_cache::TextureCache::new(dir);
                cell.0 = root.clone();
            }
            crate::viewport_2d_render::rasterize_render_list(
                &self.canvas_items,
                self.width,
                self.height,
                crate::scene_renderer::CANVAS_OVERLAY_CLEAR,
                Some(&mut cell.1),
            )
        }))
    }

    /// The whole Play frame: the 3D half with the HUD composited over it. Needs
    /// no tree, so the caller runs it off the editor-state lock.
    pub fn render(&self) -> Bgra8Image {
        let mut img = self.render_world();
        if let Some(canvas) = self.render_canvas() {
            crate::scene_renderer::composite_canvas_over_bgra(&mut img, &canvas);
        }
        img
    }
}

/// A `width`x`height` BGRA image of one opaque colour.
fn solid_bgra(width: u32, height: u32, color: Color) -> Bgra8Image {
    let px = [
        (color.b.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.r.clamp(0.0, 1.0) * 255.0).round() as u8,
        255,
    ];
    let data: Vec<u8> = px
        .iter()
        .copied()
        .cycle()
        .take(width as usize * height as usize * 4)
        .collect();
    Bgra8Image::from_bgra(width, height, data)
}

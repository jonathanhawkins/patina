//! Binds an HTML `<canvas>` to the engine's wgpu render surface (pat-u3l8c.110).
//!
//! This is the browser end of [`gdrender3d::wgpu_surface`]: it looks up the
//! canvas in the DOM, sizes its backing store for the device pixel ratio, and
//! hands it to [`GpuSurface::new`] as a `SurfaceTarget::Canvas`. Adapter and
//! device acquisition are `await`ed — never blocked on — because the browser's
//! JS thread cannot be parked while the WebGPU promise resolves.
//!
//! The whole module is `cfg(target_arch = "wasm32")`: `web_sys::window()` and
//! the canvas surface target only exist there. The platform-independent part
//! (the DPR sizing math, [`crate::canvas_backing_size`]) lives in the crate
//! root so it is unit-tested on the host too.
//!
//! Build it with:
//! `cargo check -p patina-web --target wasm32-unknown-unknown`

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use gdrender3d::wgpu::SurfaceTarget;
use gdrender3d::wgpu_pipeline::GpuContext3D;
use gdrender3d::wgpu_surface::GpuSurface;

use crate::canvas_backing_size;

/// Looks up a `<canvas>` element by its `id`.
///
/// Every failure is reported as a message rather than a bare `None` so the JS
/// caller learns *which* step failed (no DOM / missing id / wrong element
/// type) instead of a generic "could not start".
pub fn canvas_by_id(id: &str) -> Result<web_sys::HtmlCanvasElement, String> {
    let window = web_sys::window().ok_or("no `window`: not running on a browser main thread")?;
    let document = window.document().ok_or("`window` has no `document`")?;
    let element = document
        .get_element_by_id(id)
        .ok_or_else(|| format!("no element with id `{id}`"))?;
    element
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| format!("element `{id}` is not a <canvas>"))
}

/// The device pixel ratio of the current window, defaulting to 1.0 when there
/// is no window (worker) or the browser reports a nonsensical value.
pub fn device_pixel_ratio() -> f64 {
    match web_sys::window() {
        Some(window) => {
            let dpr = window.device_pixel_ratio();
            if dpr.is_finite() && dpr > 0.0 {
                dpr
            } else {
                1.0
            }
        }
        None => 1.0,
    }
}

/// Sizes the canvas' backing store for the current device pixel ratio and
/// returns the pixel size the swapchain should use.
///
/// The CSS box (`client_width`/`client_height`) drives layout; the backing
/// store (`width`/`height`) is what wgpu renders into. Keeping the latter at
/// `css * dpr` is what stops a HiDPI canvas from looking soft.
pub fn fit_canvas_to_display(canvas: &web_sys::HtmlCanvasElement) -> (u32, u32) {
    let css_width = canvas.client_width().max(0) as u32;
    let css_height = canvas.client_height().max(0) as u32;
    // A canvas that has not been laid out yet reports a 0-sized CSS box; fall
    // back to whatever backing store the markup declared.
    let (css_width, css_height) = if css_width == 0 || css_height == 0 {
        (canvas.width(), canvas.height())
    } else {
        (css_width, css_height)
    };

    let (width, height) = canvas_backing_size(css_width, css_height, device_pixel_ratio());
    if canvas.width() != width {
        canvas.set_width(width);
    }
    if canvas.height() != height {
        canvas.set_height(height);
    }
    (width, height)
}

/// A live canvas renderer: the GPU device/queue plus the configured swapchain
/// for one `<canvas>`.
///
/// Held by the JS host across frames — creating it is the expensive,
/// asynchronous part, and everything after it is synchronous.
#[wasm_bindgen]
pub struct WebRenderer {
    ctx: GpuContext3D,
    surface: GpuSurface,
    canvas: web_sys::HtmlCanvasElement,
}

#[wasm_bindgen]
impl WebRenderer {
    /// Name of the GPU adapter that was actually selected.
    #[wasm_bindgen(getter)]
    pub fn adapter(&self) -> String {
        self.ctx.adapter_name()
    }

    /// Which backend the surface ended up on — `BrowserWebGpu` when WebGPU is
    /// available, `Gl` when it fell back to WebGL2. This is the observable
    /// proof of the fallback path.
    #[wasm_bindgen(getter)]
    pub fn backend(&self) -> String {
        format!("{:?}", self.ctx.adapter.get_info().backend)
    }

    /// The negotiated swapchain texture format.
    #[wasm_bindgen(getter)]
    pub fn format(&self) -> String {
        format!("{:?}", self.surface.format())
    }

    /// Current swapchain width in physical pixels.
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.surface.width()
    }

    /// Current swapchain height in physical pixels.
    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.surface.height()
    }

    /// Re-fits the canvas backing store to its CSS box and reconfigures the
    /// swapchain when the size actually changed. Cheap enough to call every
    /// frame; returns whether a reconfigure happened.
    #[wasm_bindgen(js_name = syncSize)]
    pub fn sync_size(&mut self) -> bool {
        let (width, height) = fit_canvas_to_display(&self.canvas);
        self.surface.resize(&self.ctx.device, width, height)
    }

    /// Clears the canvas to a linear RGB color and presents the frame.
    ///
    /// The minimal end-to-end check that surface creation, the async device,
    /// swapchain configuration, submission and `present` all work in this
    /// browser. Returns `false` if no frame could be acquired.
    #[wasm_bindgen(js_name = presentClear)]
    pub fn present_clear(&self, r: f64, g: f64, b: f64) -> bool {
        self.surface.present_clear(&self.ctx, [r, g, b, 1.0])
    }
}

/// Starts a renderer on the `<canvas id="...">` named by `canvas_id`.
///
/// Async by necessity: WebGPU hands back adapters and devices as promises.
/// Rejects with a message naming the failing stage.
#[wasm_bindgen(js_name = startCanvasRenderer)]
pub async fn start_canvas_renderer(canvas_id: String) -> Result<WebRenderer, JsValue> {
    let canvas = canvas_by_id(&canvas_id).map_err(JsValue::from)?;
    let (width, height) = fit_canvas_to_display(&canvas);
    let (ctx, surface) = GpuSurface::new(SurfaceTarget::Canvas(canvas.clone()), width, height)
        .await
        .map_err(|e| JsValue::from(e.to_string()))?;
    Ok(WebRenderer {
        ctx,
        surface,
        canvas,
    })
}

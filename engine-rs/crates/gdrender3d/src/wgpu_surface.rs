//! Presentable wgpu surfaces (canvas / window) with **async** adapter+device
//! acquisition — the piece the browser runtime needs that
//! [`crate::wgpu_pipeline`] does not provide (pat-u3l8c.110).
//!
//! [`GpuContext3D::new`](crate::wgpu_pipeline::GpuContext3D::new) is headless
//! *and* synchronous: it has no [`wgpu::Surface`] at all, and it drives
//! `request_adapter` / `request_device` through `pollster::block_on`. Blocking
//! is forbidden on `wasm32-unknown-unknown` — the browser's single JS thread
//! cannot be parked while the WebGPU promise resolves, so `block_on` there
//! either panics or deadlocks the tab. This module supplies the two missing
//! halves:
//!
//! 1. [`GpuSurface::new`] — an `async fn` that creates a surface from any
//!    [`wgpu::SurfaceTarget`] (on the web that is
//!    `SurfaceTarget::Canvas(HtmlCanvasElement)`), then `.await`s the adapter
//!    and device instead of blocking. It builds the very same
//!    [`GpuContext3D`] the existing pipeline code consumes, so the two paths
//!    share one context type.
//! 2. Swapchain configuration policy ([`surface_configuration`] and the
//!    `choose_*` helpers) — the format/present-mode/alpha-mode negotiation a
//!    presentable surface needs and a headless renderer never did.
//!
//! **Backends.** On the web [`browser_backends`] asks for
//! `BROWSER_WEBGPU | GL`, i.e. WebGPU first with a **WebGL2 fallback** for
//! browsers that have not shipped (or have disabled) WebGPU; wgpu picks the
//! first backend that can actually produce an adapter for the surface. The
//! device is requested with [`surface_limits`], which starts from
//! `Limits::downlevel_webgl2_defaults()` so the same code runs unmodified on
//! the WebGL2 fallback path.
//!
//! Everything except the target type is platform-independent, so this module
//! compiles and is unit-tested natively as well as on `wasm32`.

use crate::wgpu_pipeline::GpuContext3D;

// ── Errors ──────────────────────────────────────────────────────────

/// Why a presentable GPU surface could not be brought up.
///
/// The wgpu error types are flattened to their `Display` text so callers (and
/// the JS side of the web runtime) can surface a message without depending on
/// wgpu's error enums.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SurfaceInitError {
    /// `Instance::create_surface` rejected the target (e.g. the browser has no
    /// WebGL2 and no WebGPU, or the element was not a `<canvas>`).
    #[error("could not create a wgpu surface for the render target: {0}")]
    CreateSurface(String),
    /// No adapter is compatible with the surface on any enabled backend.
    #[error("no wgpu adapter is compatible with the render surface")]
    NoAdapter,
    /// An adapter was found but refused to hand out a device.
    #[error("the wgpu adapter did not provide a device: {0}")]
    NoDevice(String),
}

// ── Backend / limit policy ──────────────────────────────────────────

/// Backends to request in the browser: WebGPU preferred, **WebGL2 as the
/// fallback**.
///
/// wgpu enumerates adapters across every enabled backend, so including `GL`
/// means a browser without WebGPU still yields an adapter instead of failing
/// surface creation outright.
pub fn browser_backends() -> wgpu::Backends {
    wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL
}

/// Backends for the current target: [`browser_backends`] on wasm, everything
/// available natively (where the desktop backends are the point).
pub fn default_backends() -> wgpu::Backends {
    if cfg!(target_arch = "wasm32") {
        browser_backends()
    } else {
        wgpu::Backends::all()
    }
}

/// Device limits for a presentable surface: the WebGL2 downlevel set (so the
/// fallback backend can satisfy them) widened to the adapter's real texture
/// resolution (so a 4K canvas is not clamped to 2048).
///
/// Mirrors the limits [`GpuContext3D::new`] already requests, keeping the
/// headless and presentable paths on one shader/pipeline budget.
pub fn surface_limits(adapter_limits: wgpu::Limits) -> wgpu::Limits {
    wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter_limits)
}

// ── Swapchain configuration policy ──────────────────────────────────

/// Picks the swapchain color format: the first sRGB format the surface
/// advertises, else its preferred (first) format, else the format wgpu
/// guarantees everywhere.
///
/// sRGB is preferred because the engine's shaders write linear color and rely
/// on the target doing the sRGB encode — the same reason
/// [`crate::wgpu_pipeline::GpuRenderer3D`] uses `Bgra8UnormSrgb` for its
/// offscreen trait path.
pub fn choose_surface_format(formats: &[wgpu::TextureFormat]) -> wgpu::TextureFormat {
    formats
        .iter()
        .copied()
        .find(|f| f.is_srgb())
        .or_else(|| formats.first().copied())
        .unwrap_or(wgpu::TextureFormat::Bgra8UnormSrgb)
}

/// Picks the present mode: `Fifo` (vsync) whenever supported — it is the only
/// mode guaranteed by the spec and the only one WebGPU/WebGL2 offer.
pub fn choose_present_mode(modes: &[wgpu::PresentMode]) -> wgpu::PresentMode {
    if modes.contains(&wgpu::PresentMode::Fifo) {
        wgpu::PresentMode::Fifo
    } else {
        modes.first().copied().unwrap_or(wgpu::PresentMode::Fifo)
    }
}

/// Picks the composite alpha mode: `Opaque` when supported, so the canvas does
/// not blend the page background through the rendered frame.
pub fn choose_alpha_mode(modes: &[wgpu::CompositeAlphaMode]) -> wgpu::CompositeAlphaMode {
    if modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
        wgpu::CompositeAlphaMode::Opaque
    } else {
        modes
            .first()
            .copied()
            .unwrap_or(wgpu::CompositeAlphaMode::Auto)
    }
}

/// Builds a swapchain configuration for `caps` at `width` x `height`.
///
/// Both dimensions are clamped to at least 1: `wgpu` requires a nonzero
/// swapchain size, and a hidden or zero-height `<canvas>` reports 0 — which
/// would otherwise panic on `configure`.
pub fn surface_configuration(
    caps: &wgpu::SurfaceCapabilities,
    width: u32,
    height: u32,
) -> wgpu::SurfaceConfiguration {
    wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: choose_surface_format(&caps.formats),
        width: width.max(1),
        height: height.max(1),
        present_mode: choose_present_mode(&caps.present_modes),
        alpha_mode: choose_alpha_mode(&caps.alpha_modes),
        view_formats: Vec::new(),
        desired_maximum_frame_latency: 2,
    }
}

// ── Presentable surface ─────────────────────────────────────────────

/// A configured, presentable wgpu surface plus the swapchain settings it was
/// configured with.
///
/// Created together with its [`GpuContext3D`] by [`GpuSurface::new`]; the
/// context owns the device/queue and outlives individual frames.
pub struct GpuSurface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

impl std::fmt::Debug for GpuSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuSurface")
            .field("format", &self.config.format)
            .field("width", &self.config.width)
            .field("height", &self.config.height)
            .field("present_mode", &self.config.present_mode)
            .finish()
    }
}

impl GpuSurface {
    /// Creates a presentable surface for `target` and, **without blocking**,
    /// acquires an adapter and device for it.
    ///
    /// `target` is anything wgpu accepts: a window handle natively, or
    /// `wgpu::SurfaceTarget::Canvas(canvas)` in the browser. The returned
    /// [`GpuContext3D`] is the same type the offscreen pipeline uses, so
    /// device/queue-taking code is shared between the two.
    ///
    /// The surface is already `configure`d when this returns; call
    /// [`GpuSurface::acquire`] to get a frame.
    pub async fn new(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> Result<(GpuContext3D, Self), SurfaceInitError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: default_backends(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let surface = instance
            .create_surface(target)
            .map_err(|e| SurfaceInitError::CreateSurface(e.to_string()))?;

        // `compatible_surface` is what makes the WebGPU-then-WebGL2 fallback
        // work: an adapter that cannot present to this canvas is rejected here
        // rather than failing later at `configure`.
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .map_err(|_| SurfaceInitError::NoAdapter)?;

        let required_limits = surface_limits(adapter.limits());
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("patina-surface-device"),
                required_features: wgpu::Features::empty(),
                required_limits,
                ..Default::default()
            })
            .await
            .map_err(|e| SurfaceInitError::NoDevice(e.to_string()))?;

        let caps = surface.get_capabilities(&adapter);
        let config = surface_configuration(&caps, width, height);
        surface.configure(&device, &config);

        let ctx = GpuContext3D {
            instance,
            adapter,
            device,
            queue,
        };
        Ok((ctx, Self { surface, config }))
    }

    /// The swapchain color format — what a render pipeline targeting this
    /// surface must be built against.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    /// Current swapchain width in pixels.
    pub fn width(&self) -> u32 {
        self.config.width
    }

    /// Current swapchain height in pixels.
    pub fn height(&self) -> u32 {
        self.config.height
    }

    /// The full swapchain configuration.
    pub fn config(&self) -> &wgpu::SurfaceConfiguration {
        &self.config
    }

    /// Re-applies the current configuration (after a `Lost`/`Outdated` frame).
    pub fn reconfigure(&self, device: &wgpu::Device) {
        self.surface.configure(device, &self.config);
    }

    /// Resizes the swapchain, e.g. when the canvas' CSS box changed.
    ///
    /// A no-op when the clamped size is unchanged, so a per-frame resize check
    /// is cheap. Returns whether the surface was actually reconfigured.
    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) -> bool {
        let w = width.max(1);
        let h = height.max(1);
        if w == self.config.width && h == self.config.height {
            return false;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(device, &self.config);
        true
    }

    /// Acquires the next frame to render into.
    ///
    /// A `Suboptimal` frame is still usable and is returned as-is. `Outdated`
    /// and `Lost` are recovered from by reconfiguring and retrying once — the
    /// two statuses a browser tab hits routinely (canvas resize,
    /// device-pixel-ratio change, GPU process restart). Any other status means
    /// "skip this frame", reported as `None`.
    pub fn acquire(&self, device: &wgpu::Device) -> Option<wgpu::SurfaceTexture> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Some(frame),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(device, &self.config);
                match self.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(frame)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Some(frame),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Clears the canvas to `color` (linear, 0..=1 per channel) and presents it.
    ///
    /// This is the minimal end-to-end proof that the whole chain works in the
    /// browser — surface creation, async device, swapchain configuration,
    /// command submission and `present` — without needing a scene or a
    /// pipeline. Returns `false` when no frame could be acquired.
    pub fn present_clear(&self, ctx: &GpuContext3D, color: [f64; 4]) -> bool {
        let Some(frame) = self.acquire(&ctx.device) else {
            return false;
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("patina-surface-clear"),
            });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("patina-surface-clear-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: color[0],
                            g: color[1],
                            b: color[2],
                            a: color[3],
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
        }
        ctx.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        true
    }
}

// ── Tests ───────────────────────────────────────────────────────────
//
// All of these run on the host without a GPU: they exercise the swapchain
// negotiation policy against synthetic `SurfaceCapabilities`, which is where
// the platform-specific mistakes (non-sRGB target, unsupported present mode,
// zero-sized canvas) actually live.

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(
        formats: Vec<wgpu::TextureFormat>,
        present_modes: Vec<wgpu::PresentMode>,
        alpha_modes: Vec<wgpu::CompositeAlphaMode>,
    ) -> wgpu::SurfaceCapabilities {
        wgpu::SurfaceCapabilities {
            formats,
            present_modes,
            alpha_modes,
            usages: wgpu::TextureUsages::RENDER_ATTACHMENT,
        }
    }

    #[test]
    fn browser_backends_are_webgpu_plus_webgl2_fallback() {
        let backends = browser_backends();
        assert!(
            backends.contains(wgpu::Backends::BROWSER_WEBGPU),
            "WebGPU must be requested first"
        );
        assert!(
            backends.contains(wgpu::Backends::GL),
            "WebGL2 must remain as the fallback backend"
        );
    }

    #[test]
    fn srgb_format_is_preferred_over_a_linear_one() {
        let formats = vec![
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Bgra8UnormSrgb,
        ];
        assert_eq!(
            choose_surface_format(&formats),
            wgpu::TextureFormat::Bgra8UnormSrgb
        );
    }

    #[test]
    fn preferred_format_is_used_when_none_are_srgb() {
        let formats = vec![
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureFormat::Bgra8Unorm,
        ];
        assert_eq!(
            choose_surface_format(&formats),
            wgpu::TextureFormat::Rgba16Float,
            "the first (preferred) format wins when there is no sRGB option"
        );
    }

    #[test]
    fn empty_format_list_falls_back_to_the_guaranteed_format() {
        assert_eq!(
            choose_surface_format(&[]),
            wgpu::TextureFormat::Bgra8UnormSrgb
        );
    }

    #[test]
    fn fifo_present_mode_is_preferred() {
        let modes = vec![wgpu::PresentMode::Immediate, wgpu::PresentMode::Fifo];
        assert_eq!(choose_present_mode(&modes), wgpu::PresentMode::Fifo);
    }

    #[test]
    fn present_mode_falls_back_to_the_first_supported() {
        assert_eq!(
            choose_present_mode(&[wgpu::PresentMode::Mailbox]),
            wgpu::PresentMode::Mailbox
        );
        assert_eq!(choose_present_mode(&[]), wgpu::PresentMode::Fifo);
    }

    #[test]
    fn opaque_alpha_mode_is_preferred() {
        let modes = vec![
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::Opaque,
        ];
        assert_eq!(choose_alpha_mode(&modes), wgpu::CompositeAlphaMode::Opaque);
    }

    #[test]
    fn alpha_mode_falls_back_to_the_first_supported() {
        assert_eq!(
            choose_alpha_mode(&[wgpu::CompositeAlphaMode::Inherit]),
            wgpu::CompositeAlphaMode::Inherit
        );
        assert_eq!(choose_alpha_mode(&[]), wgpu::CompositeAlphaMode::Auto);
    }

    #[test]
    fn configuration_negotiates_format_present_and_alpha_together() {
        let c = caps(
            vec![
                wgpu::TextureFormat::Bgra8Unorm,
                wgpu::TextureFormat::Rgba8UnormSrgb,
            ],
            vec![wgpu::PresentMode::Fifo],
            vec![wgpu::CompositeAlphaMode::Opaque],
        );
        let config = surface_configuration(&c, 800, 600);
        assert_eq!(config.format, wgpu::TextureFormat::Rgba8UnormSrgb);
        assert_eq!(config.present_mode, wgpu::PresentMode::Fifo);
        assert_eq!(config.alpha_mode, wgpu::CompositeAlphaMode::Opaque);
        assert_eq!(config.width, 800);
        assert_eq!(config.height, 600);
        assert!(config.view_formats.is_empty());
    }

    #[test]
    fn configuration_clamps_a_zero_sized_canvas_to_one_pixel() {
        // A display:none / not-yet-laid-out <canvas> reports 0x0, and wgpu
        // panics on a zero-sized swapchain.
        let c = caps(
            vec![wgpu::TextureFormat::Bgra8UnormSrgb],
            vec![wgpu::PresentMode::Fifo],
            vec![wgpu::CompositeAlphaMode::Opaque],
        );
        let config = surface_configuration(&c, 0, 0);
        assert_eq!(config.width, 1);
        assert_eq!(config.height, 1);
    }

    #[test]
    fn configuration_requests_render_attachment_usage() {
        let c = caps(
            vec![wgpu::TextureFormat::Bgra8UnormSrgb],
            vec![wgpu::PresentMode::Fifo],
            vec![wgpu::CompositeAlphaMode::Opaque],
        );
        let config = surface_configuration(&c, 320, 240);
        assert!(config
            .usage
            .contains(wgpu::TextureUsages::RENDER_ATTACHMENT));
    }

    #[test]
    fn surface_limits_stay_within_webgl2_downlevel_but_keep_adapter_resolution() {
        let adapter = wgpu::Limits::default();
        let limits = surface_limits(adapter.clone());
        // Resolution comes from the adapter so a large canvas is not clamped…
        assert_eq!(
            limits.max_texture_dimension_2d,
            adapter.max_texture_dimension_2d
        );
        // …but everything else stays at the WebGL2 downlevel budget, which is
        // what makes the fallback backend able to satisfy the request.
        let webgl2 = wgpu::Limits::downlevel_webgl2_defaults();
        assert_eq!(
            limits.max_storage_buffers_per_shader_stage,
            webgl2.max_storage_buffers_per_shader_stage
        );
        assert_eq!(
            limits.max_uniform_buffers_per_shader_stage,
            webgl2.max_uniform_buffers_per_shader_stage
        );
        assert_eq!(
            limits.max_vertex_buffer_array_stride,
            webgl2.max_vertex_buffer_array_stride
        );
    }

    #[test]
    fn surface_init_error_messages_name_the_failing_stage() {
        assert!(SurfaceInitError::NoAdapter.to_string().contains("adapter"));
        assert!(SurfaceInitError::CreateSurface("no webgl2".into())
            .to_string()
            .contains("no webgl2"));
        assert!(SurfaceInitError::NoDevice("limits".into())
            .to_string()
            .contains("limits"));
    }
}

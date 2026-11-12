//! Window content-scale (the "stretch" pipeline) — pat-nzgdu.87.
//!
//! Godot's root `Window` maps a project's *base* resolution
//! (`display/window/size/viewport_width` × `viewport_height`) onto the actual
//! window, adding black bars or stretching as the project's
//! `display/window/stretch/{mode,aspect,scale}` settings direct. Every
//! `CanvasItem` is then drawn through the resulting transform, which is what
//! makes a 640×360 game fill a 1920×1080 window without the game code knowing.
//!
//! [`compute_content_scale`] is a direct transcription of
//! `Window::_update_viewport_size()` (`upstream/godot/scene/main/window.cpp`),
//! including its `floor`/`round` placement — the rounding is load-bearing, since
//! off-by-one margins are exactly what produces a one-pixel seam at the edge of
//! a letterboxed frame.

use crate::math::{Transform2D, Vector2};

/// `Window.ContentScaleMode` — how the base resolution is mapped to the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContentScaleMode {
    /// `CONTENT_SCALE_MODE_DISABLED` — no stretching; the viewport is the
    /// window (Godot's default).
    #[default]
    Disabled,
    /// `CONTENT_SCALE_MODE_CANVAS_ITEMS` — render at window resolution and
    /// scale the *canvas coordinate system*, so text and sprites stay crisp.
    CanvasItems,
    /// `CONTENT_SCALE_MODE_VIEWPORT` — render at the base resolution and scale
    /// the finished image up, giving deliberate low-res pixelation.
    Viewport,
}

impl ContentScaleMode {
    /// Parses the `display/window/stretch/mode` project setting.
    ///
    /// Unknown values fall back to `Disabled`, matching Godot's behaviour for
    /// an unrecognised stretch mode: draw at window resolution rather than
    /// silently letterboxing.
    pub fn from_setting(value: &str) -> Self {
        match value {
            "canvas_items" => Self::CanvasItems,
            "viewport" => Self::Viewport,
            _ => Self::Disabled,
        }
    }
}

/// `Window.ContentScaleAspect` — what happens when the window's aspect ratio
/// differs from the base resolution's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContentScaleAspect {
    /// `CONTENT_SCALE_ASPECT_IGNORE` — stretch both axes independently,
    /// distorting the image (Godot's default).
    #[default]
    Ignore,
    /// `CONTENT_SCALE_ASPECT_KEEP` — preserve aspect with black bars on
    /// whichever axis has spare room.
    Keep,
    /// `CONTENT_SCALE_ASPECT_KEEP_WIDTH` — preserve aspect, showing more/less
    /// vertically (bars top and bottom only when needed).
    KeepWidth,
    /// `CONTENT_SCALE_ASPECT_KEEP_HEIGHT` — preserve aspect, showing more/less
    /// horizontally.
    KeepHeight,
    /// `CONTENT_SCALE_ASPECT_EXPAND` — preserve aspect and expand the visible
    /// area on the longer axis, never adding bars.
    Expand,
}

impl ContentScaleAspect {
    /// Parses the `display/window/stretch/aspect` project setting.
    pub fn from_setting(value: &str) -> Self {
        match value {
            "keep" => Self::Keep,
            "keep_width" => Self::KeepWidth,
            "keep_height" => Self::KeepHeight,
            "expand" => Self::Expand,
            _ => Self::Ignore,
        }
    }
}

/// `Window.ContentScaleStretch` — whether the scale factor may be fractional.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContentScaleStretch {
    /// `CONTENT_SCALE_STRETCH_FRACTIONAL` — any scale (Godot's default).
    #[default]
    Fractional,
    /// `CONTENT_SCALE_STRETCH_INTEGER` — whole-number scaling only, which is
    /// what keeps pixel art from wobbling.
    Integer,
}

impl ContentScaleStretch {
    /// Parses the `display/window/stretch/scale_mode` project setting.
    pub fn from_setting(value: &str) -> Self {
        match value {
            "integer" => Self::Integer,
            _ => Self::Fractional,
        }
    }
}

/// The root window's content-scale configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContentScaleConfig {
    /// How to map the base resolution onto the window.
    pub mode: ContentScaleMode,
    /// How to handle an aspect-ratio mismatch.
    pub aspect: ContentScaleAspect,
    /// Whether the scale must be a whole number.
    pub stretch: ContentScaleStretch,
    /// Extra user scale on top of the computed one
    /// (`display/window/stretch/scale`, Godot default `1.0`).
    pub factor: f32,
    /// The project's base resolution (`content_scale_size` — Godot seeds this
    /// from `display/window/size/viewport_width`/`_height`).
    pub base_size: Vector2,
}

impl Default for ContentScaleConfig {
    fn default() -> Self {
        Self {
            mode: ContentScaleMode::Disabled,
            aspect: ContentScaleAspect::Ignore,
            stretch: ContentScaleStretch::Fractional,
            factor: 1.0,
            base_size: Vector2::ZERO,
        }
    }
}

/// The result of laying the base resolution out inside a window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContentScaleResult {
    /// The size the viewport actually renders at, in pixels.
    ///
    /// Equal to the window size under `Disabled`/`CanvasItems`, and to the
    /// (factor-divided) base resolution under `Viewport` — that difference *is*
    /// the difference between the two modes.
    pub final_size: Vector2,
    /// The canvas coordinate space nodes position themselves in — Godot's
    /// `final_size_override` / `Viewport.get_visible_rect().size`.
    pub size_override: Vector2,
    /// The on-window rectangle the viewport is blitted into.
    pub screen_size: Vector2,
    /// Black-bar thickness on each side, in window pixels. Non-zero only when
    /// the aspect setting calls for letter/pillarboxing.
    pub margin: Vector2,
    /// Canvas space → window pixels, Godot's `Window::window_transform`.
    ///
    /// This is the transform the 2D draw path must prepend to every
    /// `CanvasItem`, and the one a pointer position must be run through
    /// *backwards* to hit-test the canvas.
    pub window_transform: Transform2D,
}

impl ContentScaleResult {
    /// The identity result for a window with no stretching: the viewport is
    /// the window, one canvas unit is one pixel, no bars.
    pub fn unscaled(window_size: Vector2) -> Self {
        Self {
            final_size: window_size,
            size_override: window_size,
            screen_size: window_size,
            margin: Vector2::ZERO,
            window_transform: Transform2D::IDENTITY,
        }
    }
}

/// Aspect ratio (`width / height`), `0.0` for a degenerate height.
fn aspect_of(size: Vector2) -> f32 {
    if size.y == 0.0 {
        0.0
    } else {
        size.x / size.y
    }
}

fn floor_size(size: Vector2) -> Vector2 {
    Vector2::new(size.x.floor(), size.y.floor())
}

/// Lays the project's base resolution out inside `window_size`, following
/// `Window::_update_viewport_size()`.
///
/// ⚠️ A zero-area base size disables stretching entirely, exactly as Godot does
/// (`content_scale_size.x == 0 || content_scale_size.y == 0`) — a project that
/// never set a base resolution must render 1:1, not divide by zero.
pub fn compute_content_scale(window_size: Vector2, cfg: &ContentScaleConfig) -> ContentScaleResult {
    // Godot clamps the factor to a whole number >= 1 before anything else when
    // integer stretching is on, so the rest of the math sees the real value.
    let mut factor = cfg.factor;
    if cfg.stretch == ContentScaleStretch::Integer {
        factor = factor.floor();
        if factor < 1.0 {
            factor = 1.0;
        }
    }
    // A zero or negative factor would divide the canvas out of existence.
    if factor <= 0.0 {
        factor = 1.0;
    }

    if cfg.mode == ContentScaleMode::Disabled
        || cfg.base_size.x == 0.0
        || cfg.base_size.y == 0.0
        || window_size.x <= 0.0
        || window_size.y <= 0.0
    {
        return ContentScaleResult {
            final_size: window_size,
            size_override: Vector2::new(window_size.x / factor, window_size.y / factor),
            screen_size: window_size,
            margin: Vector2::ZERO,
            window_transform: Transform2D::IDENTITY,
        };
    }

    let video_mode = window_size;
    let desired_res = cfg.base_size;

    let viewport_aspect = aspect_of(desired_res);
    let video_mode_aspect = aspect_of(video_mode);

    let (mut viewport_size, mut screen_size) = if cfg.aspect == ContentScaleAspect::Ignore
        || (viewport_aspect - video_mode_aspect).abs() < 1e-5
    {
        // Same aspect, or the aspect is deliberately ignored.
        (desired_res, video_mode)
    } else if viewport_aspect < video_mode_aspect {
        // The window is relatively wider than the base resolution.
        if matches!(
            cfg.aspect,
            ContentScaleAspect::KeepHeight | ContentScaleAspect::Expand
        ) {
            // Show more of the world horizontally.
            (
                Vector2::new(desired_res.y * video_mode_aspect, desired_res.y),
                video_mode,
            )
        } else {
            // Pillarbox.
            (
                desired_res,
                Vector2::new(video_mode.y * viewport_aspect, video_mode.y),
            )
        }
    } else {
        // The window is relatively taller than the base resolution.
        if matches!(
            cfg.aspect,
            ContentScaleAspect::KeepWidth | ContentScaleAspect::Expand
        ) {
            // Show more of the world vertically.
            (
                Vector2::new(desired_res.x, desired_res.x / video_mode_aspect),
                video_mode,
            )
        } else {
            // Letterbox.
            (
                desired_res,
                Vector2::new(video_mode.x, video_mode.x / viewport_aspect),
            )
        }
    };

    screen_size = floor_size(screen_size);
    viewport_size = floor_size(viewport_size);

    if cfg.stretch == ContentScaleStretch::Integer && viewport_size.x > 0.0 && viewport_size.y > 0.0
    {
        let scale_x = (screen_size.x / viewport_size.x).floor();
        let scale_y = (screen_size.y / viewport_size.y).floor();
        let scale_factor = scale_x.min(scale_y).max(1.0);
        screen_size = Vector2::new(
            viewport_size.x * scale_factor,
            viewport_size.y * scale_factor,
        );
    }

    let mut margin = Vector2::ZERO;
    if screen_size.x < video_mode.x && screen_size.y != 0.0 {
        margin.x = ((video_mode.x - screen_size.x) / 2.0).round();
    }
    if screen_size.y < video_mode.y && screen_size.x != 0.0 {
        margin.y = ((video_mode.y - screen_size.y) / 2.0).round();
    }

    match cfg.mode {
        ContentScaleMode::Disabled => unreachable!("handled by the early return above"),
        ContentScaleMode::CanvasItems => {
            let size_override = Vector2::new(viewport_size.x / factor, viewport_size.y / factor);
            // The canvas is authored in `size_override` units and lands on a
            // `screen_size` rectangle offset by the bars.
            let scale = Vector2::new(
                if size_override.x != 0.0 {
                    screen_size.x / size_override.x
                } else {
                    1.0
                },
                if size_override.y != 0.0 {
                    screen_size.y / size_override.y
                } else {
                    1.0
                },
            );
            let mut window_transform = Transform2D::scaled(scale);
            window_transform.origin = margin;
            ContentScaleResult {
                final_size: screen_size,
                size_override,
                screen_size,
                margin,
                window_transform,
            }
        }
        ContentScaleMode::Viewport => {
            // The frame is rendered small, then blown up to `screen_size`.
            let final_size = floor_size(Vector2::new(
                viewport_size.x / factor,
                viewport_size.y / factor,
            ));
            let scale = Vector2::new(
                if final_size.x != 0.0 {
                    screen_size.x / final_size.x
                } else {
                    1.0
                },
                if final_size.y != 0.0 {
                    screen_size.y / final_size.y
                } else {
                    1.0
                },
            );
            let mut window_transform = Transform2D::scaled(scale);
            window_transform.origin = margin;
            ContentScaleResult {
                final_size,
                size_override: final_size,
                screen_size,
                margin,
                window_transform,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: Vector2 = Vector2::new(640.0, 360.0);

    fn cfg(mode: ContentScaleMode, aspect: ContentScaleAspect) -> ContentScaleConfig {
        ContentScaleConfig {
            mode,
            aspect,
            base_size: BASE,
            ..ContentScaleConfig::default()
        }
    }

    /// Godot's default project stretches nothing: the viewport IS the window.
    #[test]
    fn disabled_mode_is_one_to_one() {
        let r = compute_content_scale(
            Vector2::new(1920.0, 1080.0),
            &cfg(ContentScaleMode::Disabled, ContentScaleAspect::Ignore),
        );
        assert_eq!(r.final_size, Vector2::new(1920.0, 1080.0));
        assert_eq!(r.window_transform, Transform2D::IDENTITY);
        assert_eq!(r.margin, Vector2::ZERO);
    }

    /// ⚠️ A project that never set a base resolution must render 1:1 rather
    /// than divide by a zero-area viewport.
    #[test]
    fn zero_base_size_disables_stretching() {
        let c = ContentScaleConfig {
            mode: ContentScaleMode::CanvasItems,
            base_size: Vector2::ZERO,
            ..ContentScaleConfig::default()
        };
        let r = compute_content_scale(Vector2::new(800.0, 600.0), &c);
        assert_eq!(r.final_size, Vector2::new(800.0, 600.0));
        assert_eq!(r.window_transform, Transform2D::IDENTITY);
    }

    /// canvas_items at an exact 3× multiple: no bars, uniform 3× scale, and the
    /// canvas still measures 640×360 to the game's code.
    #[test]
    fn canvas_items_scales_the_canvas_to_fill_the_window() {
        let r = compute_content_scale(
            Vector2::new(1920.0, 1080.0),
            &cfg(ContentScaleMode::CanvasItems, ContentScaleAspect::Keep),
        );
        assert_eq!(r.margin, Vector2::ZERO, "16:9 into 16:9 needs no bars");
        assert_eq!(r.size_override, BASE, "the canvas still measures 640x360");
        assert_eq!(r.final_size, Vector2::new(1920.0, 1080.0));
        // A point at the canvas's bottom-right maps to the window's.
        let corner = r.window_transform.xform(Vector2::new(640.0, 360.0));
        assert!((corner.x - 1920.0).abs() < 0.01, "{corner:?}");
        assert!((corner.y - 1080.0).abs() < 0.01, "{corner:?}");
    }

    /// `keep` on a window wider than the base ratio pillarboxes: bars left and
    /// right, and the canvas is pushed right by the bar width.
    #[test]
    fn keep_aspect_pillarboxes_a_too_wide_window() {
        // 16:9 base into a 2:1 window (1440x720): height fills, width bars.
        let r = compute_content_scale(
            Vector2::new(1440.0, 720.0),
            &cfg(ContentScaleMode::CanvasItems, ContentScaleAspect::Keep),
        );
        assert_eq!(r.screen_size, Vector2::new(1280.0, 720.0));
        assert_eq!(r.margin, Vector2::new(80.0, 0.0), "(1440-1280)/2 = 80");
        assert_eq!(
            r.window_transform.xform(Vector2::ZERO),
            Vector2::new(80.0, 0.0),
            "the canvas origin sits after the left bar"
        );
    }

    /// `keep` on a window taller than the base ratio letterboxes instead.
    #[test]
    fn keep_aspect_letterboxes_a_too_tall_window() {
        // 16:9 base into a 4:3 window (640x480): width fills, height bars.
        let r = compute_content_scale(
            Vector2::new(640.0, 480.0),
            &cfg(ContentScaleMode::CanvasItems, ContentScaleAspect::Keep),
        );
        assert_eq!(r.screen_size, Vector2::new(640.0, 360.0));
        assert_eq!(r.margin, Vector2::new(0.0, 60.0), "(480-360)/2 = 60");
    }

    /// `ignore` distorts rather than bars — the whole window is covered and the
    /// two axes get different scales.
    #[test]
    fn ignore_aspect_stretches_both_axes_independently() {
        let r = compute_content_scale(
            Vector2::new(640.0, 480.0),
            &cfg(ContentScaleMode::CanvasItems, ContentScaleAspect::Ignore),
        );
        assert_eq!(r.margin, Vector2::ZERO, "ignore never adds bars");
        assert_eq!(r.screen_size, Vector2::new(640.0, 480.0));
        let corner = r.window_transform.xform(Vector2::new(640.0, 360.0));
        assert!(
            (corner.y - 480.0).abs() < 0.01,
            "y is stretched: {corner:?}"
        );
        assert!((corner.x - 640.0).abs() < 0.01, "x is 1:1: {corner:?}");
    }

    /// `keep_height` shows MORE of the world horizontally instead of adding
    /// bars — the canvas gets wider than the authored base resolution.
    #[test]
    fn keep_height_widens_the_visible_canvas() {
        let r = compute_content_scale(
            Vector2::new(1440.0, 720.0),
            &cfg(
                ContentScaleMode::CanvasItems,
                ContentScaleAspect::KeepHeight,
            ),
        );
        assert_eq!(
            r.margin,
            Vector2::ZERO,
            "no bars — the world widens instead"
        );
        assert!(
            r.size_override.x > BASE.x,
            "more world is visible horizontally: {:?}",
            r.size_override
        );
        assert_eq!(r.size_override.y, BASE.y, "height is unchanged");
    }

    /// `keep_width` is the mirror image: more world vertically.
    #[test]
    fn keep_width_heightens_the_visible_canvas() {
        let r = compute_content_scale(
            Vector2::new(640.0, 480.0),
            &cfg(ContentScaleMode::CanvasItems, ContentScaleAspect::KeepWidth),
        );
        assert_eq!(r.margin, Vector2::ZERO);
        assert_eq!(r.size_override.x, BASE.x);
        assert!(
            r.size_override.y > BASE.y,
            "more world is visible vertically: {:?}",
            r.size_override
        );
    }

    /// `viewport` mode renders at the BASE resolution and scales the finished
    /// image up — that smaller `final_size` is the entire difference from
    /// `canvas_items`, which renders at window resolution.
    #[test]
    fn viewport_mode_renders_at_the_base_resolution() {
        let window = Vector2::new(1920.0, 1080.0);
        let canvas = compute_content_scale(
            window,
            &cfg(ContentScaleMode::CanvasItems, ContentScaleAspect::Keep),
        );
        let viewport = compute_content_scale(
            window,
            &cfg(ContentScaleMode::Viewport, ContentScaleAspect::Keep),
        );

        assert_eq!(canvas.final_size, window, "canvas_items draws at 1920x1080");
        assert_eq!(viewport.final_size, BASE, "viewport draws at 640x360");
        assert_eq!(
            viewport.screen_size, window,
            "…and is then blown up to fill the window"
        );
    }

    /// `stretch/scale` divides the canvas coordinate space: a factor of 2 makes
    /// every canvas unit twice as big on screen.
    #[test]
    fn scale_factor_shrinks_the_canvas_coordinate_space() {
        let c = ContentScaleConfig {
            factor: 2.0,
            ..cfg(ContentScaleMode::CanvasItems, ContentScaleAspect::Keep)
        };
        let r = compute_content_scale(Vector2::new(1920.0, 1080.0), &c);
        assert_eq!(
            r.size_override,
            Vector2::new(320.0, 180.0),
            "the canvas halves, so everything drawn in it doubles on screen"
        );
    }

    /// Integer stretching only ever scales by a whole number, which is what
    /// stops pixel art from wobbling.
    #[test]
    fn integer_stretch_rounds_the_scale_down_to_a_whole_number() {
        let c = ContentScaleConfig {
            stretch: ContentScaleStretch::Integer,
            ..cfg(ContentScaleMode::CanvasItems, ContentScaleAspect::Keep)
        };
        // 640x360 into 1500x800: the fractional fit would be 2.22x, so integer
        // stretching drops to 2x and bars out the remainder.
        let r = compute_content_scale(Vector2::new(1500.0, 800.0), &c);
        assert_eq!(r.screen_size, Vector2::new(1280.0, 720.0), "exactly 2x");
        assert_eq!(r.margin, Vector2::new(110.0, 40.0));
    }

    /// ⚠️ Integer stretching must never land on a 0x scale — a window smaller
    /// than the base resolution still has to show something.
    #[test]
    fn integer_stretch_never_scales_below_one() {
        let c = ContentScaleConfig {
            stretch: ContentScaleStretch::Integer,
            ..cfg(ContentScaleMode::CanvasItems, ContentScaleAspect::Keep)
        };
        let r = compute_content_scale(Vector2::new(320.0, 180.0), &c);
        assert_eq!(r.screen_size, BASE, "clamped to 1x, not 0x");
    }

    /// A degenerate window (minimised, or a headless boot before the first
    /// resize) must not produce NaN transforms.
    #[test]
    fn zero_window_size_falls_back_to_identity() {
        let r = compute_content_scale(
            Vector2::ZERO,
            &cfg(ContentScaleMode::CanvasItems, ContentScaleAspect::Keep),
        );
        assert_eq!(r.window_transform, Transform2D::IDENTITY);
        assert!(r.final_size.x.is_finite() && r.final_size.y.is_finite());
    }

    // -- setting parsers ----------------------------------------------------

    #[test]
    fn stretch_settings_parse_godot_spellings() {
        assert_eq!(
            ContentScaleMode::from_setting("canvas_items"),
            ContentScaleMode::CanvasItems
        );
        assert_eq!(
            ContentScaleMode::from_setting("viewport"),
            ContentScaleMode::Viewport
        );
        assert_eq!(
            ContentScaleMode::from_setting("disabled"),
            ContentScaleMode::Disabled
        );
        // ⚠️ An unknown mode must not letterbox a project that never asked for
        // it — fall back to drawing at window resolution.
        assert_eq!(
            ContentScaleMode::from_setting("2d"),
            ContentScaleMode::Disabled
        );

        assert_eq!(
            ContentScaleAspect::from_setting("keep"),
            ContentScaleAspect::Keep
        );
        assert_eq!(
            ContentScaleAspect::from_setting("keep_width"),
            ContentScaleAspect::KeepWidth
        );
        assert_eq!(
            ContentScaleAspect::from_setting("keep_height"),
            ContentScaleAspect::KeepHeight
        );
        assert_eq!(
            ContentScaleAspect::from_setting("expand"),
            ContentScaleAspect::Expand
        );
        assert_eq!(
            ContentScaleAspect::from_setting("ignore"),
            ContentScaleAspect::Ignore
        );

        assert_eq!(
            ContentScaleStretch::from_setting("integer"),
            ContentScaleStretch::Integer
        );
        assert_eq!(
            ContentScaleStretch::from_setting("fractional"),
            ContentScaleStretch::Fractional
        );
    }

    /// The stretch settings come off `ProjectSettings` under Godot's exact
    /// keys, with Godot's exact defaults.
    #[test]
    fn project_settings_supply_the_content_scale_config() {
        use crate::project_settings::{ProjectSettings, SettingsValue};

        // An empty project must land on Godot's documented defaults: no
        // stretching, `keep` aspect, fractional, 1.0, 1152x648 base.
        let empty = ProjectSettings::default();
        let d = empty.content_scale_config();
        assert_eq!(d.mode, ContentScaleMode::Disabled);
        assert_eq!(d.aspect, ContentScaleAspect::Keep);
        assert_eq!(d.stretch, ContentScaleStretch::Fractional);
        assert_eq!(d.factor, 1.0);

        let mut ps = ProjectSettings::default();
        ps.set(
            "display/window/size/viewport_width",
            SettingsValue::Int(640),
        );
        ps.set(
            "display/window/size/viewport_height",
            SettingsValue::Int(360),
        );
        ps.set(
            "display/window/stretch/mode",
            SettingsValue::String("canvas_items".into()),
        );
        ps.set(
            "display/window/stretch/aspect",
            SettingsValue::String("keep_height".into()),
        );
        // ⚠️ `scale_mode`, not `stretch_mode` — reading the wrong key would
        // leave every project on fractional scaling.
        ps.set(
            "display/window/stretch/scale_mode",
            SettingsValue::String("integer".into()),
        );
        ps.set("display/window/stretch/scale", SettingsValue::Float(2.0));

        let c = ps.content_scale_config();
        assert_eq!(c.mode, ContentScaleMode::CanvasItems);
        assert_eq!(c.aspect, ContentScaleAspect::KeepHeight);
        assert_eq!(c.stretch, ContentScaleStretch::Integer);
        assert_eq!(c.factor, 2.0);
        assert_eq!(
            c.base_size,
            Vector2::new(640.0, 360.0),
            "the base size comes from the project's viewport size"
        );
    }

    /// A zero or negative `stretch/scale` would collapse the canvas to nothing.
    #[test]
    fn non_positive_scale_setting_falls_back_to_one() {
        use crate::project_settings::{ProjectSettings, SettingsValue};
        let mut ps = ProjectSettings::default();
        ps.set("display/window/stretch/scale", SettingsValue::Float(0.0));
        assert_eq!(ps.content_scale_config().factor, 1.0);
    }

    /// The unscaled helper is what a viewport with no window behind it uses.
    #[test]
    fn unscaled_result_is_the_window_itself() {
        let r = ContentScaleResult::unscaled(Vector2::new(800.0, 600.0));
        assert_eq!(r.final_size, Vector2::new(800.0, 600.0));
        assert_eq!(r.size_override, Vector2::new(800.0, 600.0));
        assert_eq!(r.window_transform, Transform2D::IDENTITY);
    }
}

//! `ExternalTexture` — a Texture2D backed by a platform external buffer.
//!
//! Mirrors Godot's `scene/resources/external_texture.{h,cpp}`. Per its own
//! header, an external texture is one "as defined by `OES_EGL_image_external`
//! (GLES) or `VK_ANDROID_external_memory_android_hardware_buffer` (Vulkan)":
//! the caller hands over an **opaque `uint64` buffer handle** owned by the
//! platform, and the renderer imports it.
//!
//! ⚠️ It is NOT a video or camera source. Godot's ExternalTexture contains no
//! reference to either; those are [`CameraFeed`]/[`CameraServer`] (camera) and
//! `VideoStreamPlayer` (video). See pat-nzgdu.209's notes.
//!
//! **What this port covers, and why that is parity rather than a stub:** the
//! class's own logic — the `set_size` guards, the buffer-id handling, and the
//! size/format reporting — is renderer-independent and ported faithfully here.
//! The external-memory *import* is a driver feature; Patina's 2D renderer is a
//! CPU rasteriser with no such extension, which is exactly the situation
//! Godot's own headless driver is in. `RendererDummy` answers
//! `texture_get_native_handle() { return 0; }` and implements
//! `texture_external_update() {}` as a no-op
//! (`servers/rendering/dummy/storage/texture_storage.h:96,132`), so this
//! backend matching that is agreeing with upstream, not faking it.
//!
//! [`CameraFeed`]: https://docs.godotengine.org/en/stable/classes/class_camerafeed.html
//! [`CameraServer`]: https://docs.godotengine.org/en/stable/classes/class_cameraserver.html

use gdcore::math::Vector2;

/// A texture whose pixels live in a platform-owned external buffer.
///
/// Construct with [`ExternalTexture::default`]; the defaults mirror Godot's
/// member initialisers (`external_texture.h`).
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalTexture {
    size: Vector2,
    external_buffer: u64,
}

impl Default for ExternalTexture {
    fn default() -> Self {
        Self {
            // ⚠️ 256x256, not zero. `Size2 size = Size2(256, 256)` is a member
            // initialiser and the constructor body is EMPTY, so reading only
            // `ExternalTexture::ExternalTexture() {}` suggests a zero default.
            size: Vector2::new(256.0, 256.0),
            external_buffer: 0,
        }
    }
}

impl ExternalTexture {
    /// The texture's size. Godot: `get_size` (overrides `Texture2D`).
    pub fn size(&self) -> Vector2 {
        self.size
    }

    /// Sets the size, returning whether it actually changed.
    ///
    /// ⚠️ Godot **ignores** a non-positive size and a size equal to the current
    /// one — `if (p_size.width > 0 && p_size.height > 0 && p_size != size)`
    /// (external_texture.cpp:49). A zero/negative size is therefore not an
    /// error and not a clamp: it is silently dropped, and the texture keeps its
    /// previous size.
    ///
    /// The bool is Godot's own predicate for emitting `changed`: it emits
    /// exactly when the assignment happens, so a redundant set is signal-free.
    pub fn set_size(&mut self, size: Vector2) -> bool {
        if size.x > 0.0 && size.y > 0.0 && size != self.size {
            self.size = size;
            // Godot then calls `texture_external_update(texture, w, h, buffer)`,
            // which the headless driver implements as a no-op, and emits
            // `changed`.
            return true;
        }
        false
    }

    /// The bound external buffer handle. 0 means "none bound".
    pub fn external_buffer_id(&self) -> u64 {
        self.external_buffer
    }

    /// Binds an external buffer handle, returning whether it changed.
    ///
    /// ⚠️ Unlike [`set_size`](Self::set_size) this accepts **any** value,
    /// including 0 — the only guard is `if (p_external_buffer != external_buffer)`
    /// (external_texture.cpp:62). 0 is a legitimate value: it is the initial
    /// state, and re-binding it detaches the buffer.
    ///
    /// ⚠️ It also does NOT emit `changed`, where `set_size` does. The asymmetry
    /// is Godot's, not a transcription slip: the buffer swap does not alter the
    /// texture's reported dimensions, so nothing observing the resource needs to
    /// re-read it.
    pub fn set_external_buffer_id(&mut self, external_buffer: u64) -> bool {
        if external_buffer != self.external_buffer {
            self.external_buffer = external_buffer;
            return true;
        }
        false
    }

    /// The renderer's native handle for this texture. Godot:
    /// `get_external_texture_id`.
    ///
    /// ⚠️ Always 0 on this backend, and that is upstream's answer, not a
    /// placeholder: Godot returns `texture_get_native_handle(texture)`, which
    /// `RendererDummy` defines as `{ return 0; }`
    /// (dummy/storage/texture_storage.h:132). A CPU rasteriser has no native
    /// GPU handle to report. Note this is NOT the value passed to
    /// [`set_external_buffer_id`](Self::set_external_buffer_id) — that is the
    /// buffer you hand *in*; this is the texture handle the driver hands *out*.
    pub fn external_texture_id(&self) -> u64 {
        0
    }

    /// Godot: `get_width` — reads straight off `size`.
    pub fn width(&self) -> i32 {
        self.size.x as i32
    }

    /// Godot: `get_height` — reads straight off `size`.
    pub fn height(&self) -> i32 {
        self.size.y as i32
    }

    /// ⚠️ Always `false` in Godot — a hardcoded `return false`
    /// (external_texture.cpp:77), not a property of the bound buffer. An
    /// external buffer may well have an alpha channel; the class does not
    /// report it.
    pub fn has_alpha(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-4;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    /// ⚠️ 256x256, not zero — the ctor body is empty and the default lives in a
    /// member initialiser.
    #[test]
    fn default_matches_godots_member_initialisers() {
        let tex = ExternalTexture::default();
        assert!(approx(tex.size().x, 256.0), "size.x defaults to 256");
        assert!(approx(tex.size().y, 256.0), "size.y defaults to 256");
        assert_eq!(tex.width(), 256);
        assert_eq!(tex.height(), 256);
        assert_eq!(tex.external_buffer_id(), 0, "no buffer bound initially");
    }

    #[test]
    fn set_size_reports_the_new_size() {
        let mut tex = ExternalTexture::default();
        assert!(tex.set_size(Vector2::new(640.0, 480.0)), "a real change");
        assert!(approx(tex.size().x, 640.0));
        assert!(approx(tex.size().y, 480.0));
        assert_eq!(tex.width(), 640);
        assert_eq!(tex.height(), 480);
    }

    /// ⚠️ A non-positive size is SILENTLY DROPPED — not clamped, not an error.
    /// The texture keeps its previous size.
    #[test]
    fn set_size_ignores_non_positive_dimensions() {
        let mut tex = ExternalTexture::default();
        tex.set_size(Vector2::new(640.0, 480.0));

        for bad in [
            Vector2::new(0.0, 0.0),
            Vector2::new(0.0, 480.0),
            Vector2::new(640.0, 0.0),
            Vector2::new(-1.0, 480.0),
            Vector2::new(640.0, -1.0),
        ] {
            assert!(
                !tex.set_size(bad),
                "{bad:?} must be rejected, not applied or clamped"
            );
            assert!(approx(tex.size().x, 640.0), "size survives {bad:?}");
            assert!(approx(tex.size().y, 480.0), "size survives {bad:?}");
        }
    }

    /// The bool mirrors Godot's `emit_changed` predicate: a redundant set is
    /// signal-free.
    #[test]
    fn set_size_to_the_same_value_is_not_a_change() {
        let mut tex = ExternalTexture::default();
        assert!(tex.set_size(Vector2::new(640.0, 480.0)));
        assert!(
            !tex.set_size(Vector2::new(640.0, 480.0)),
            "re-setting the same size must not report a change"
        );
        assert!(approx(tex.size().x, 640.0));
    }

    /// ⚠️ Unlike `set_size`, ANY buffer id is accepted — including 0, which
    /// detaches.
    #[test]
    fn external_buffer_id_round_trips_and_zero_is_legitimate() {
        let mut tex = ExternalTexture::default();

        assert!(tex.set_external_buffer_id(0xDEAD_BEEF));
        assert_eq!(tex.external_buffer_id(), 0xDEAD_BEEF);
        assert!(
            !tex.set_external_buffer_id(0xDEAD_BEEF),
            "re-binding the same handle is not a change"
        );

        // 0 is a value, not "unset" — the guard is only `!=`, so this applies.
        assert!(tex.set_external_buffer_id(0), "0 detaches the buffer");
        assert_eq!(tex.external_buffer_id(), 0);

        // The full u64 range survives: the handle is opaque platform data and
        // must not be truncated.
        assert!(tex.set_external_buffer_id(u64::MAX));
        assert_eq!(tex.external_buffer_id(), u64::MAX);
    }

    /// ⚠️ The handle you bind IN is not the handle reported OUT.
    #[test]
    fn external_texture_id_is_the_drivers_handle_not_the_bound_buffer() {
        let mut tex = ExternalTexture::default();
        tex.set_external_buffer_id(0xABCD);

        assert_eq!(
            tex.external_texture_id(),
            0,
            "a CPU backend has no native handle — RendererDummy returns 0 too"
        );
        assert_ne!(
            tex.external_texture_id(),
            tex.external_buffer_id(),
            "get_external_texture_id must not echo back the bound buffer id"
        );
    }

    /// ⚠️ Hardcoded `false` in Godot (external_texture.cpp:77), regardless of
    /// the bound buffer — an external buffer may well carry alpha; the class
    /// simply does not report it.
    ///
    /// This is a **parity lock, not a behavioural test**: `has_alpha` ignores
    /// `&self` entirely, so no state can make it fail. It exists to make a
    /// later "fix" that inspects the buffer show up as a deliberate divergence.
    #[test]
    fn has_alpha_is_always_false() {
        assert!(!ExternalTexture::default().has_alpha());
    }

    /// Size and buffer are independent: neither setter disturbs the other.
    #[test]
    fn size_and_buffer_are_independent() {
        let mut tex = ExternalTexture::default();
        tex.set_size(Vector2::new(1920.0, 1080.0));
        tex.set_external_buffer_id(0x42);

        assert_eq!(tex.external_buffer_id(), 0x42);
        assert!(approx(tex.size().x, 1920.0));

        tex.set_external_buffer_id(0x43);
        assert!(
            approx(tex.size().x, 1920.0) && approx(tex.size().y, 1080.0),
            "rebinding the buffer must not resize"
        );

        tex.set_size(Vector2::new(64.0, 64.0));
        assert_eq!(
            tex.external_buffer_id(),
            0x43,
            "resizing must not detach the buffer"
        );
    }
}

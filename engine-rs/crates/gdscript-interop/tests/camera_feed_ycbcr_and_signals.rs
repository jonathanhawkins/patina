//! pat-s3rrv.261 — `CameraFeed` YCbCr planes, format selection, and the
//! per-feed `frame_changed` / `format_changed` signals.
//!
//! **Godot ground truth:** `servers/camera/camera_feed.cpp` — `set_rgb_image`,
//! `set_ycbcr_image` and `set_ycbcr_images` each install pixels, set the
//! matching `FeedDataType`, and emit `frame_changed`; `set_format` selects from
//! `get_formats` and emits `format_changed`.
//!
//! ## ⚠️ One clause of this bead's acceptance is NOT implemented, on purpose
//!
//! The acceptance asks that `CameraServer.add_feed(feed)` emit
//! **`camera_feeds_updated`**. It does not, and must not.
//!
//! The signal is real — declared at `camera_server.h:62`, bound at
//! `camera_server.cpp:57`, documented in `CameraServer.xml` — but
//! `camera_server.cpp` has exactly **two** `emit_signal` sites, `:126`
//! (`camera_feed_added`) and `:140` (`camera_feed_removed`). The three places
//! that raise `feeds_updated_signal_name` are all platform **drivers**
//! re-enumerating the device list: `modules/camera/camera_macos.mm:400`,
//! `camera_android.cpp:816`, `camera_linux.cpp:83`. A script calling `add_feed`
//! is a different event, and Patina has no camera driver on any platform, so
//! there is no correct trigger for it here.
//!
//! `camera_feed.rs::camera_feeds_updated_is_never_emitted` (pat-0q2kx.262)
//! already pins that silence, and this file reinforces it rather than
//! contradicting it — because a test asserting the opposite would make one of
//! the two permanently red.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}"))
        .return_value
        .expect("script returned a value")
}

fn arr(v: &Variant) -> &[Variant] {
    match v {
        Variant::Array(a) => a.as_slice(),
        other => panic!("expected an Array, got {other:?}"),
    }
}

fn int_at(v: &Variant, i: usize) -> i64 {
    match &arr(v)[i] {
        Variant::Int(n) => *n,
        other => panic!("element {i} is not an Int: {other:?}"),
    }
}

/// **The acceptance.** `frame_changed` fires per image, and the datatype tracks
/// which flavour of image arrived.
#[test]
fn camera_feed_ycbcr_and_signals() {
    // FORMAT_RGBA8 is 5. The planes are separate Images so a mix-up between
    // them is visible rather than aliased.
    let v = run_val(
        "var log = []\n\
         var f = CameraFeed.new()\n\
         f.connect(\"frame_changed\", func(): log.append(1))\n\
         var rgb = Image.create(2, 2, false, 5)\n\
         var y = Image.create(4, 4, false, 5)\n\
         var cbcr = Image.create(2, 2, false, 5)\n\
         var before = log.size()\n\
         f.set_rgb_image(rgb)\n\
         var after_rgb = log.size()\n\
         var type_rgb = f.get_datatype()\n\
         f.set_ycbcr_images(y, cbcr)\n\
         var after_sep = log.size()\n\
         var type_sep = f.get_datatype()\n\
         f.set_ycbcr_image(y)\n\
         var type_interleaved = f.get_datatype()\n\
         return [before, after_rgb, after_sep, type_rgb, type_sep, \
                 type_interleaved, log.size(), f.is_connected(\"frame_changed\")]\n",
    );

    // Guard the guard: without a registered handler a zero count proves nothing.
    assert_eq!(
        arr(&v)[7],
        Variant::Bool(true),
        "the frame_changed handler must actually be connected"
    );

    assert_eq!(int_at(&v, 0), 0, "nothing fires before an image is pushed");
    assert_eq!(
        int_at(&v, 1),
        1,
        "set_rgb_image emits frame_changed exactly once"
    );
    assert_eq!(
        int_at(&v, 2),
        2,
        "set_ycbcr_images emits it again — camera_feed.cpp emits from EVERY \
         set_*_image, not only the RGB one"
    );
    assert_eq!(int_at(&v, 6), 3, "and set_ycbcr_image makes three");

    // FeedDataType: NOIMAGE 0, RGB 1, YCBCR 2, YCBCR_SEP 3, EXTERNAL 4.
    assert_eq!(int_at(&v, 3), 1, "set_rgb_image is FEED_RGB (1)");
    assert_eq!(
        int_at(&v, 4),
        3,
        "set_ycbcr_imageS — two separate planes — is FEED_YCBCR_SEP (3)"
    );
    // ⚠️ The pair most easily collapsed into one implementation. If both
    // reported 2 or both reported 3, a shader sampling the chroma plane would
    // read the luma one.
    assert_eq!(
        int_at(&v, 5),
        2,
        "set_ycbcr_image — ONE interleaved plane — is FEED_YCBCR (2), which is \
         a different datatype from the two-plane form"
    );
}

/// `set_format` selects a real format, and refuses one that does not exist.
#[test]
fn camera_feed_ycbcr_and_signals_set_format_selects_and_reports() {
    let v = run_val(
        "var log = []\n\
         var f = CameraFeed.new()\n\
         f.connect(\"format_changed\", func(): log.append(1))\n\
         var formats = f.get_formats()\n\
         var ok = f.set_format(0, {})\n\
         var after_ok = log.size()\n\
         var chosen = f.get_selected_format()\n\
         var bad = f.set_format(7, {})\n\
         var after_bad = log.size()\n\
         var still = f.get_selected_format()\n\
         var neg = f.set_format(-1, {})\n\
         return [formats.size(), ok, after_ok, chosen, bad, after_bad, still, neg]\n",
    );

    assert!(
        int_at(&v, 0) >= 1,
        "get_formats must report at least one format, or every index is out of \
         range and set_format can never succeed"
    );
    assert_eq!(arr(&v)[1], Variant::Bool(true), "index 0 is selectable");
    assert_eq!(
        int_at(&v, 2),
        1,
        "a successful set_format emits format_changed exactly once"
    );
    assert_eq!(int_at(&v, 3), 0, "and records the selection");

    assert_eq!(
        arr(&v)[4],
        Variant::Bool(false),
        "an out-of-range index is refused"
    );
    // ⚠️ The half that matters: a refused selection must emit NOTHING. An
    // implementation that emitted unconditionally would still return false here
    // and look correct.
    assert_eq!(
        int_at(&v, 5),
        1,
        "a REFUSED set_format must not emit format_changed — the count is still 1"
    );
    assert_eq!(
        int_at(&v, 6),
        0,
        "and must not disturb the previous selection"
    );
    assert_eq!(
        arr(&v)[7],
        Variant::Bool(false),
        "a negative index is refused too, not wrapped"
    );
}

/// Connections are per feed, and `disconnect` works.
///
/// A single global registry would deliver one feed's frames to another feed's
/// listener — invisible in any test that only ever makes one feed.
#[test]
fn camera_feed_ycbcr_and_signals_connections_are_per_feed() {
    let v = run_val(
        "var log_a = []\n\
         var log_b = []\n\
         var a = CameraFeed.new()\n\
         var b = CameraFeed.new()\n\
         a.connect(\"frame_changed\", func(): log_a.append(1))\n\
         b.connect(\"frame_changed\", func(): log_b.append(1))\n\
         var img = Image.create(1, 1, false, 5)\n\
         a.set_rgb_image(img)\n\
         var a1 = log_a.size()\n\
         var b1 = log_b.size()\n\
         a.disconnect(\"frame_changed\")\n\
         a.set_rgb_image(img)\n\
         return [a1, b1, log_a.size(), log_b.size(), a.is_connected(\"frame_changed\")]\n",
    );

    assert_eq!(int_at(&v, 0), 1, "feed a's listener fired");
    assert_eq!(
        int_at(&v, 1),
        0,
        "feed b's listener must NOT fire for feed a's frame — connections are \
         per feed, not global"
    );
    assert_eq!(int_at(&v, 2), 1, "after disconnect, a's listener stops firing");
    assert_eq!(int_at(&v, 3), 0, "b still untouched");
    assert_eq!(arr(&v)[4], Variant::Bool(false), "and reports disconnected");
}

/// `add_feed` still does NOT emit `camera_feeds_updated`.
///
/// This bead's acceptance asks for it; Godot does not do it (see the module
/// note). Pinning it here as well means the deviation is deliberate and
/// recorded at the place someone would look to change it back.
#[test]
fn camera_feed_ycbcr_and_signals_add_feed_does_not_emit_feeds_updated() {
    let v = run_val(
        "var updated = []\n\
         var added = []\n\
         CameraServer.connect(\"camera_feeds_updated\", func(): updated.append(1))\n\
         CameraServer.connect(\"camera_feed_added\", func(id): added.append(id))\n\
         var f = CameraFeed.new()\n\
         CameraServer.add_feed(f)\n\
         CameraServer.remove_feed(f)\n\
         return [added.size(), updated.size()]\n",
    );
    assert_eq!(
        int_at(&v, 0),
        1,
        "camera_feed_added still fires — the control proving the registry works"
    );
    assert_eq!(
        int_at(&v, 1),
        0,
        "camera_feeds_updated is raised by the platform DRIVERS on device \
         re-enumeration (camera_macos.mm:400, camera_android.cpp:816, \
         camera_linux.cpp:83), never by camera_server.cpp's add_feed. Patina \
         has no driver, so nothing should raise it."
    );
}

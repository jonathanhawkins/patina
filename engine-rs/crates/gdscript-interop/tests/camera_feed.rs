//! pat-0q2kx.262 — `CameraFeed` image path, feed transform, and `CameraServer`
//! signals, all reachable from GDScript.
//!
//! **Godot ground truth:** `servers/camera/camera_feed.{h,cpp}` and
//! `servers/camera/camera_server.cpp`. `servers/camera/` is CORE and ships in
//! the web export — only the OS driver module `modules/camera` is native-gated
//! — so the script-facing surface is in scope even with no camera hardware.
//!
//! Three things here are easy to get plausibly wrong, and each has a test whose
//! whole job is to pin the non-obvious answer:
//!
//! 1. A fresh feed's transform is a **Y-flip**, not the identity
//!    (`camera_feed.cpp:160` builds `Transform2D(1, 0, 0, -1, 0, 1)`).
//! 2. `set_external` reports `FEED_EXTERNAL` (**4**), not `FEED_RGB` (1) — it
//!    declares an externally-owned texture and supplies no pixels.
//! 3. `camera_feeds_updated` is **never emitted**. The bead text asks for it,
//!    but `camera_server.cpp` has exactly two `emit_signal` sites, :126 and
//!    :140, for `camera_feed_added`/`camera_feed_removed`. Emitting a third
//!    signal Godot does not emit would be a parity *defect*, so
//!    `camera_feeds_updated_is_never_emitted` pins the silence.
//!
//! These are integration tests on purpose: `-p gdscript-interop --lib` cannot
//! pass (a pre-existing `fuzz_deeply_nested_parens` stack overflow cancels the
//! run), so unit tests here would be unverifiable.

use gdcore::math::Color;
use gdscript_interop::interpreter::{camera_feed_rgb_image, Interpreter};
use gdvariant::Variant;

/// Runs `src` and returns its `return` value.
fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}"))
        .return_value
        .expect("script returned a value")
}

/// Runs `src` expecting a runtime error.
fn run_err(src: &str) -> gdscript_interop::interpreter::RuntimeError {
    let mut interp = Interpreter::new();
    interp.run(src).expect_err("script should have failed")
}

/// The elements of a returned `Array`, by reference — `Variant` is not `Copy`,
/// so the obvious `let [a, b] = v` does not compile.
fn arr(v: &Variant) -> &[Variant] {
    match v {
        Variant::Array(items) => items,
        other => panic!("expected an Array, got {other:?}"),
    }
}

fn int_at(v: &Variant, i: usize) -> i64 {
    match &arr(v)[i] {
        Variant::Int(n) => *n,
        other => panic!("element {i} should be an Int, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Image — the prerequisite that did not exist
// ---------------------------------------------------------------------------

/// Before this bead `Image` appeared **zero** times in the interpreter, so
/// `set_rgb_image(Ref<Image>)` had no argument any script could construct. This
/// is the type that unblocks it: create, write a pixel, read it back.
#[test]
fn image_is_script_constructible_and_holds_pixels() {
    let v = run_val(
        "var img = Image.create(3, 2, false, 5)\n\
         img.set_pixel(2, 1, Color(0.25, 0.5, 0.75, 1))\n\
         var c = img.get_pixel(2, 1)\n\
         return [img.get_width(), img.get_height(), img.get_format(), c]\n",
    );

    assert_eq!(int_at(&v, 0), 3, "width is the first create() argument");
    assert_eq!(int_at(&v, 1), 2, "height is the second");
    assert_eq!(
        int_at(&v, 2),
        5,
        "FORMAT_RGBA8 is 5 in Godot's Image.Format enum"
    );
    match &arr(&v)[3] {
        Variant::Color(c) => {
            assert!(
                (c.r - 0.25).abs() < 1e-4
                    && (c.g - 0.5).abs() < 1e-4
                    && (c.b - 0.75).abs() < 1e-4
                    && (c.a - 1.0).abs() < 1e-4,
                "get_pixel must return exactly what set_pixel wrote, got {c:?}"
            );
        }
        other => panic!("get_pixel should return a Color, got {other:?}"),
    }
}

/// `format` defaults to `FORMAT_RGBA8` when the argument is omitted, and a
/// zero-area image is `is_empty()`. The zero case matters because it is the one
/// that would otherwise try a negative-length allocation.
#[test]
fn image_defaults_to_rgba8_and_zero_area_is_empty() {
    let v = run_val(
        "var a = Image.create(1, 1)\n\
         var b = Image.create(0, 4, false, 5)\n\
         return [a.get_format(), b.is_empty(), a.is_empty()]\n",
    );
    assert_eq!(int_at(&v, 0), 5, "the default format is FORMAT_RGBA8 (5)");
    assert_eq!(arr(&v)[1], Variant::Bool(true), "a 0x4 image has no pixels");
    assert_eq!(
        arr(&v)[2],
        Variant::Bool(false),
        "a 1x1 image is NOT empty — is_empty asks about the buffer, not content"
    );
}

/// Out-of-range access is a no-op / unset read, matching Godot's
/// `ERR_FAIL_INDEX`. It must not panic and must not grow the buffer — a `Vec`
/// index would do the former and a naive `resize` the latter.
#[test]
fn image_out_of_range_access_is_survivable() {
    let v = run_val(
        "var img = Image.create(2, 2, false, 5)\n\
         img.set_pixel(9, 9, Color(1, 0, 0, 1))\n\
         img.set_pixel(-1, 0, Color(1, 0, 0, 1))\n\
         var c = img.get_pixel(9, 9)\n\
         return [img.get_width(), img.get_height(), c.a]\n",
    );
    assert_eq!(int_at(&v, 0), 2);
    assert_eq!(int_at(&v, 1), 2, "an out-of-range write must not resize");
    match &arr(&v)[2] {
        Variant::Float(a) => assert!(*a < 1e-6, "an out-of-range read is unset"),
        other => panic!("alpha should be a Float, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// CameraFeed — datatype transitions and the image slot
// ---------------------------------------------------------------------------

/// THE ACCEPTANCE's datatype clause: `get_datatype` reports `FEED_RGB` after
/// `set_rgb_image`, and `FEED_NOIMAGE` before it. It used to be hardcoded to 0.
#[test]
fn set_rgb_image_moves_the_feed_to_feed_rgb() {
    let v = run_val(
        "var feed = CameraFeed.new()\n\
         var before = feed.get_datatype()\n\
         var img = Image.create(2, 2, false, 5)\n\
         img.set_pixel(0, 0, Color(1, 0, 0, 1))\n\
         feed.set_rgb_image(img)\n\
         return [before, feed.get_datatype()]\n",
    );
    assert_eq!(
        int_at(&v, 0),
        0,
        "a fresh feed is FEED_NOIMAGE (0): there is no driver supplying frames"
    );
    assert_eq!(
        int_at(&v, 1),
        1,
        "after set_rgb_image the feed is FEED_RGB (1)"
    );
}

/// The feed stores the pixels **by value**. Godot's feed keeps its own
/// `Ref<Image>` copy, so a later `set_pixel` on the caller's image must not
/// retroactively change what the feed is showing. Holding the handle instead
/// would make the feed a live view of someone else's buffer — a real
/// aliasing bug, and completely invisible without this test.
#[test]
fn the_feed_copies_the_image_rather_than_aliasing_it() {
    let v = run_val(
        "var feed = CameraFeed.new()\n\
         var img = Image.create(1, 1, false, 5)\n\
         img.set_pixel(0, 0, Color(1, 0, 0, 1))\n\
         feed.set_rgb_image(img)\n\
         img.set_pixel(0, 0, Color(0, 1, 0, 1))\n\
         return [feed, img.get_pixel(0, 0)]\n",
    );

    // The script's own image was repainted green...
    match &arr(&v)[1] {
        Variant::Color(c) => assert!(
            c.g > 0.9 && c.r < 0.1,
            "the caller's image should now be green, got {c:?}"
        ),
        other => panic!("expected a Color, got {other:?}"),
    }

    // ...while the feed still holds the red it was handed.
    let raw = match &arr(&v)[0] {
        Variant::ObjectId(oid) => oid.raw(),
        other => panic!("CameraFeed.new() should yield an ObjectId, got {other:?}"),
    };
    let (w, h, pixels) = camera_feed_rgb_image(raw).expect("the feed holds an RGB image");
    assert_eq!((w, h), (1, 1));
    assert_eq!(pixels.len(), 1);
    assert!(
        pixels[0].r > 0.9 && pixels[0].g < 0.1,
        "the feed must still show the RED it was given, not the caller's later \
         green — got {:?}. Equality here means the feed aliased the handle.",
        pixels[0]
    );
}

/// `set_external(width, height)` declares an externally-managed texture. It
/// reports `FEED_EXTERNAL` (**4**), NOT `FEED_RGB` (1): the feed carries no RGB
/// pixels of its own, so answering 1 would promise data that is not there.
#[test]
fn set_external_reports_feed_external_not_feed_rgb() {
    let v = run_val(
        "var feed = CameraFeed.new()\n\
         feed.set_external(640, 480)\n\
         var s = feed.get_external_size()\n\
         return [feed.get_datatype(), s.x, s.y]\n",
    );
    assert_eq!(
        int_at(&v, 0),
        4,
        "FEED_EXTERNAL is 4 (camera_feed.h:47-51); 1 would be FEED_RGB and wrong"
    );
    match (&arr(&v)[1], &arr(&v)[2]) {
        (Variant::Float(x), Variant::Float(y)) => {
            assert!((*x - 640.0).abs() < 1e-3 && (*y - 480.0).abs() < 1e-3);
        }
        other => panic!("external size should be floats, got {other:?}"),
    }

    // No pixels were supplied, so there is nothing for a renderer to read.
    let raw = match run_val("var feed = CameraFeed.new()\nfeed.set_external(4, 4)\nreturn feed\n") {
        Variant::ObjectId(oid) => oid.raw(),
        other => panic!("expected an ObjectId, got {other:?}"),
    };
    assert!(
        camera_feed_rgb_image(raw).is_none(),
        "an EXTERNAL feed has no RGB image — reporting one would be a fiction"
    );
}

/// ⚠️ A fresh feed's transform is the **Y-flip**, not the identity:
/// `camera_feed.cpp:160` constructs `Transform2D(1, 0, 0, -1, 0, 1)` because
/// camera images arrive with the opposite vertical origin. Defaulting to the
/// identity renders every feed upside-down, and nothing else in the API would
/// reveal it.
#[test]
fn a_fresh_feeds_transform_is_the_y_flip_not_the_identity() {
    let v = run_val("var feed = CameraFeed.new()\nreturn feed.get_transform()\n");
    let t = match v {
        Variant::Transform2D(t) => t,
        other => panic!("get_transform should return a Transform2D, got {other:?}"),
    };
    assert!(
        (t.x.x - 1.0).abs() < 1e-6 && t.x.y.abs() < 1e-6,
        "x basis is (1, 0)"
    );
    assert!(
        t.y.x.abs() < 1e-6 && (t.y.y + 1.0).abs() < 1e-6,
        "y basis must be (0, -1) — the FLIP. Got {:?}; (0, 1) is the identity \
         and would show every camera frame upside-down.",
        t.y
    );
    assert!(
        t.origin.x.abs() < 1e-6 && (t.origin.y - 1.0).abs() < 1e-6,
        "origin must be (0, 1) so the flipped image stays in the unit square, \
         got {:?}",
        t.origin
    );
}

/// `set_transform` round-trips, so a script can correct for a rotated sensor.
#[test]
fn set_transform_round_trips() {
    let v = run_val(
        "var feed = CameraFeed.new()\n\
         feed.set_transform(Transform2D(Vector2(2, 0), Vector2(0, 3), Vector2(4, 5)))\n\
         var t = feed.get_transform()\n\
         return [t.x.x, t.y.y, t.origin.x, t.origin.y]\n",
    );
    let f = |i: usize| match &arr(&v)[i] {
        Variant::Float(n) => *n,
        other => panic!("element {i} should be a Float, got {other:?}"),
    };
    assert!((f(0) - 2.0).abs() < 1e-4);
    assert!((f(1) - 3.0).abs() < 1e-4);
    assert!((f(2) - 4.0).abs() < 1e-4);
    assert!((f(3) - 5.0).abs() < 1e-4);
}

/// `set_rgb_image` must reject a non-Image rather than silently doing nothing —
/// a silent no-op would leave the feed at FEED_NOIMAGE with no clue why.
#[test]
fn set_rgb_image_rejects_a_non_image() {
    let e = run_err("var feed = CameraFeed.new()\nfeed.set_rgb_image(7)\nreturn 1\n");
    let msg = format!("{e:?}");
    assert!(
        msg.contains("set_rgb_image"),
        "the error should name the method, got {msg}"
    );
}

// ---------------------------------------------------------------------------
// CameraServer signals
// ---------------------------------------------------------------------------
//
// ⚠️ Every handler below accumulates into an **Array**. GDScript lambdas
// capture by VALUE, so `func(id): got = id` never leaves the lambda and the
// assertion would fail against a perfectly correct implementation.

/// THE ACCEPTANCE's signal clause: `add_feed` emits `camera_feed_added` with
/// the feed's id to a connected script (`camera_server.cpp:126`).
#[test]
fn add_feed_emits_camera_feed_added_with_the_feed_id() {
    let v = run_val(
        "var log = []\n\
         CameraServer.connect(\"camera_feed_added\", func(id): log.append(id))\n\
         var feed = CameraFeed.new()\n\
         feed.set_name(\"front\")\n\
         var before = log.size()\n\
         CameraServer.add_feed(feed)\n\
         return [before, log.size(), feed.get_id(), log]\n",
    );
    assert_eq!(int_at(&v, 0), 0, "nothing fires before add_feed");
    assert_eq!(int_at(&v, 1), 1, "add_feed emits exactly once");

    let feed_id = int_at(&v, 2);
    let log = arr(&arr(&v)[3]);
    assert_eq!(
        log[0],
        Variant::Int(feed_id),
        "the signal must carry the id of the feed that was added, not its index"
    );
}

/// `remove_feed` emits ONLY for a feed that was actually registered. Upstream
/// emits from inside the search loop and returns (`camera_server.cpp:140`), so
/// removing a stranger is silent — an unconditional emit would announce
/// removals that never happened.
#[test]
fn remove_feed_emits_only_for_a_registered_feed() {
    let v = run_val(
        "var log = []\n\
         CameraServer.connect(\"camera_feed_removed\", func(id): log.append(id))\n\
         var a = CameraFeed.new()\n\
         var stranger = CameraFeed.new()\n\
         CameraServer.add_feed(a)\n\
         CameraServer.remove_feed(stranger)\n\
         var after_stranger = log.size()\n\
         CameraServer.remove_feed(a)\n\
         return [after_stranger, log.size(), a.get_id(), log]\n",
    );
    assert_eq!(
        int_at(&v, 0),
        0,
        "removing a feed that was never added must emit nothing"
    );
    assert_eq!(int_at(&v, 1), 1, "removing a registered feed emits once");
    let a_id = int_at(&v, 2);
    let log = arr(&arr(&v)[3]);
    assert_eq!(log[0], Variant::Int(a_id));
}

/// `disconnect` stops delivery, and `is_connected` reports the registry state.
#[test]
fn disconnect_stops_delivery() {
    let v = run_val(
        "var log = []\n\
         CameraServer.connect(\"camera_feed_added\", func(id): log.append(id))\n\
         var connected = CameraServer.is_connected(\"camera_feed_added\")\n\
         CameraServer.disconnect(\"camera_feed_added\")\n\
         var after = CameraServer.is_connected(\"camera_feed_added\")\n\
         CameraServer.add_feed(CameraFeed.new())\n\
         return [connected, after, log.size()]\n",
    );
    assert_eq!(arr(&v)[0], Variant::Bool(true));
    assert_eq!(arr(&v)[1], Variant::Bool(false));
    assert_eq!(
        arr(&v)[2],
        Variant::Int(0),
        "a disconnected handler must not fire"
    );
}

/// ⚠️ **`camera_feeds_updated` must NEVER fire.** The bead text asks for it,
/// but `camera_server.cpp` contains exactly two `emit_signal` calls — :126 and
/// :140 — and neither is this one. Adding it would be a parity defect that
/// looks like extra coverage, so this test exists to keep it out.
///
/// Both a 0-arg and a 1-arg handler are connected on purpose: a wrong
/// implementation might emit with either arity, and connecting only one would
/// let the other slip through as a swallowed arity error.
#[test]
fn camera_feeds_updated_is_never_emitted() {
    let v = run_val(
        "var log = []\n\
         CameraServer.connect(\"camera_feeds_updated\", func(): log.append(0))\n\
         CameraServer.connect(\"camera_feeds_updated\", func(id): log.append(1))\n\
         var f = CameraFeed.new()\n\
         CameraServer.add_feed(f)\n\
         CameraServer.remove_feed(f)\n\
         CameraServer.set_monitoring_feeds(true)\n\
         return [log.size(), CameraServer.is_connected(\"camera_feeds_updated\")]\n",
    );
    assert_eq!(
        arr(&v)[1],
        Variant::Bool(true),
        "guard the guard: the handlers must really be registered, or a zero \
         count below proves nothing"
    );
    assert_eq!(
        int_at(&v, 0),
        0,
        "Godot never emits camera_feeds_updated — zero emit_signal sites for it \
         in camera_server.cpp. Firing it here is a parity defect, not a feature."
    );
}

/// A raising handler must not abort the emitter. Godot's `emit_signal` does not
/// propagate a callee error back to the caller, so `add_feed` still registers
/// the feed and the *later* handlers still run. The bad handler is connected
/// FIRST on purpose — that is the ordering where a propagated error would
/// swallow the good one.
///
/// `no_such_local` is an unbound name, which raises `UndefinedVariable` while
/// evaluating the `append` argument.
#[test]
fn a_raising_handler_does_not_abort_add_feed() {
    let v = run_val(
        "var log = []\n\
         CameraServer.connect(\"camera_feed_added\", func(id): log.append(no_such_local))\n\
         CameraServer.connect(\"camera_feed_added\", func(id): log.append(id))\n\
         var f = CameraFeed.new()\n\
         CameraServer.add_feed(f)\n\
         return [CameraServer.get_feed_count(), log.size()]\n",
    );
    assert_eq!(
        int_at(&v, 0),
        1,
        "the feed must still be registered — the emit happens after the state \
         change and a handler error must not undo it"
    );
    assert_eq!(
        int_at(&v, 1),
        1,
        "exactly one append: the raising handler contributed nothing and the \
         good handler that follows it still ran"
    );
}

/// Rust-side accessor used by whatever renders a `CameraTexture`: an unknown
/// handle answers `None` rather than panicking or inventing a black frame.
#[test]
fn camera_feed_rgb_image_is_none_for_an_unknown_handle() {
    assert!(camera_feed_rgb_image(0).is_none());
    assert!(camera_feed_rgb_image(u64::MAX).is_none());
}

/// The pixels a script fed in are readable from Rust in row-major order, which
/// is the contract the render binding will consume. A transposed or
/// bottom-up buffer would still be 4 pixels and would still "look wired".
#[test]
fn feed_pixels_are_row_major_for_the_renderer() {
    let v = run_val(
        "var feed = CameraFeed.new()\n\
         var img = Image.create(2, 2, false, 5)\n\
         img.set_pixel(1, 0, Color(1, 0, 0, 1))\n\
         img.set_pixel(0, 1, Color(0, 0, 1, 1))\n\
         feed.set_rgb_image(img)\n\
         return feed\n",
    );
    let raw = match v {
        Variant::ObjectId(oid) => oid.raw(),
        other => panic!("expected an ObjectId, got {other:?}"),
    };
    let (w, h, px) = camera_feed_rgb_image(raw).expect("the feed holds an RGB image");
    assert_eq!((w, h, px.len()), (2, 2, 4));

    let is = |c: Color, r: f32, g: f32, b: f32| {
        (c.r - r).abs() < 1e-4 && (c.g - g).abs() < 1e-4 && (c.b - b).abs() < 1e-4
    };
    assert!(
        is(px[0], 0.0, 0.0, 0.0) && px[0].a < 1e-4,
        "(0,0) untouched"
    );
    assert!(
        is(px[1], 1.0, 0.0, 0.0),
        "index 1 must be (x=1, y=0) — red. Blue here means the buffer is \
         column-major; got {:?}",
        px[1]
    );
    assert!(
        is(px[2], 0.0, 0.0, 1.0),
        "index 2 must be (x=0, y=1) — blue, got {:?}",
        px[2]
    );
}

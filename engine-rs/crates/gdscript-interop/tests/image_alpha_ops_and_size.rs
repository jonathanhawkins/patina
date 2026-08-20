//! pat-s3rrv.118 — `Image.get_size()`, `premultiply_alpha()` and
//! `detect_alpha()` on the script surface.
//!
//! The script-side `Image` answered 7 statics and a handful of instance methods
//! out of Godot's 76; every other name raised `UndefinedFunction`. These three
//! are the ones that need no decoder and no new pixel format, so they are exact
//! rather than approximate.
//!
//! 🔑 Both alpha operations are **8-bit integer** algorithms in Godot, and
//! porting them as the obvious float arithmetic gives different answers. The
//! fixtures below are picked so the float version fails.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// `Image.FORMAT_RGBA8`. `premultiply_alpha` is defined only for this one.
const RGBA8: &str = "5";
/// `Image.FORMAT_RGB8` — a format `premultiply_alpha` must leave alone.
const RGB8: &str = "4";

fn script() -> String {
    format!(
        "extends Node\n\
         \n\
         func size():\n\
         \x20   return Image.create(4, 3, false, {RGBA8}).get_size()\n\
         \n\
         func premultiplied():\n\
         \x20   var img = Image.create(1, 1, false, {RGBA8})\n\
         \x20   img.set_pixel(0, 0, Color(1.0, 0.5, 0.0, 0.5))\n\
         \x20   img.premultiply_alpha()\n\
         \x20   return img.get_pixel(0, 0)\n\
         \n\
         func premultiplied_rgb8():\n\
         \x20   var img = Image.create(1, 1, false, {RGB8})\n\
         \x20   img.set_pixel(0, 0, Color(1.0, 0.5, 0.0, 0.5))\n\
         \x20   img.premultiply_alpha()\n\
         \x20   return img.get_pixel(0, 0)\n\
         \n\
         func alpha_for(a):\n\
         \x20   var img = Image.create(1, 1, false, {RGBA8})\n\
         \x20   img.set_pixel(0, 0, Color(1.0, 0.0, 0.0, a))\n\
         \x20   return img.detect_alpha()\n"
    )
}

fn call(method: &str, args: &[Variant]) -> Variant {
    let src = script();
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(&src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, method, args)
        .unwrap_or_else(|e| panic!("Image.{method} raised: {e:?}"))
}

fn detect_alpha(a: f64) -> i64 {
    match call("alpha_for", &[Variant::Float(a)]) {
        Variant::Int(v) => v,
        other => panic!("detect_alpha must return an int, got {other:?}"),
    }
}

#[test]
fn get_size_returns_the_dimensions_as_a_vector2i() {
    match call("size", &[]) {
        // ⚠️ Vector2i, not Vector2 — `Image.xml` types it as an integer vector,
        // and a script doing `img.get_size().x` on a float would still read 4,
        // so only the Variant type distinguishes them.
        Variant::Vector2i(v) => assert_eq!((v.x, v.y), (4, 3)),
        other => panic!("get_size() must return Vector2i, got {other:?}"),
    }
}

#[test]
fn premultiply_alpha_uses_godots_integer_rounding() {
    let Variant::Color(c) = call("premultiplied", &[]) else {
        panic!("get_pixel must return a Color");
    };

    // 🔑 Godot computes `(c * a + 255) >> 8` on 8-bit channels
    // (`core/io/image.cpp:4235`). The `+ 255` is a round-UP bias, so the result
    // is NOT `c * a`:
    //
    //   r: (255*128 + 255) >> 8 == 128  ->  128/255 == 0.50196
    //   g: (128*128 + 255) >> 8 ==  64  ->   64/255 == 0.25098
    //
    // ⚠️ The float version gives 0.5 and 0.25 exactly. The gap is ~0.002, so
    // the tolerance below has to be tight enough to see it — 1e-4 is two orders
    // inside the gap and far outside f32 noise. The doc's stated
    // `(color * alpha) / 256` is a third answer again; the source is the oracle.
    let tol = 1e-4;
    assert!(
        (c.r - 128.0 / 255.0).abs() < tol,
        "r: expected {} (integer form), got {} — {} would be the float version",
        128.0 / 255.0,
        c.r,
        0.5
    );
    assert!(
        (c.g - 64.0 / 255.0).abs() < tol,
        "g: expected {}, got {}",
        64.0 / 255.0,
        c.g
    );
    assert!(c.b.abs() < tol, "b: zero stays zero, got {}", c.b);
    // Alpha itself is untouched — only the colour channels are scaled.
    assert!((c.a - 0.5).abs() < tol, "alpha must survive, got {}", c.a);

    // The fixture is discriminating: the two candidate answers really differ.
    assert!(
        (128.0f32 / 255.0 - 0.5).abs() > tol,
        "fixture is not discriminating"
    );
}

#[test]
fn premultiply_alpha_is_a_no_op_off_rgba8() {
    // ⚠️ Godot returns early unless the format is RGBA8 (`image.cpp:4225`), so
    // this is a no-op rather than an approximation on other formats. Applying
    // it regardless is the natural implementation and silently darkens every
    // RGB8 image it touches.
    let Variant::Color(c) = call("premultiplied_rgb8", &[]) else {
        panic!("get_pixel must return a Color");
    };
    assert!(
        (c.r - 1.0).abs() < 1e-4 && (c.g - 0.5).abs() < 1e-4,
        "an RGB8 image must be left alone, got {c:?}"
    );
}

#[test]
fn detect_alpha_thresholds_match_godots() {
    const NONE: i64 = 0;
    const BIT: i64 = 1;
    const BLEND: i64 = 2;

    assert_eq!(detect_alpha(1.0), NONE, "fully opaque");
    assert_eq!(detect_alpha(0.0), BIT, "fully transparent is a cutout");
    assert_eq!(detect_alpha(0.5), BLEND, "partial alpha blends");

    // 🔑 The thresholds are 2 and 254, not 0 and 255 (`image.cpp:2600-2601`).
    // These four cases straddle them, and every one of them is reported wrong by
    // the intuitive `a == 255 -> NONE / a == 0 -> BIT / else BLEND` version.
    assert_eq!(
        detect_alpha(254.0 / 255.0),
        NONE,
        "alpha 254 counts as opaque, so a slightly-off encoder does not flip a \
         whole image into the expensive BLEND path"
    );
    assert_eq!(detect_alpha(253.0 / 255.0), BLEND, "alpha 253 does blend");
    assert_eq!(detect_alpha(1.0 / 255.0), BIT, "alpha 1 is still a cutout");
    assert_eq!(detect_alpha(2.0 / 255.0), BLEND, "alpha 2 blends");
}

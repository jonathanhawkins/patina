//! pat-s3rrv.137 — `ResourceImporterDynamicFont`'s options reach the imported
//! `FontFile`.
//!
//! Godot ground truth: `editor/import/resource_importer_dynamic_font.cpp`
//! `get_import_options`, which declares 22 options. Godot bakes them into the
//! artifact at import time; Patina parsed the `.import` sidecar and then
//! dropped every font option on the floor — `import_font` took no parameters at
//! all, so `multichannel_signed_distance_field=true` in a project was inert.
//!
//! ⚠️ **The option keys are FLAT.** `hinting`, not `rendering/hinting` — unlike
//! the texture importer's `process/*` keys next door. A group prefix invented
//! by analogy matches nothing and fails silently, which is why every assertion
//! here drives the real sidecar rather than calling `apply_font_params` with
//! hand-built params.
//!
//! ⚠️ **Scope: the options are RECORDED, not acted on.** There is no atlas
//! builder to hand an MSDF flag to. This pins that the value survives the
//! import and reaches the resource, which is what lets the text layer read it
//! later — not that anything renders differently.

use gdresource::importers::ResourceFormatLoader;
use gdvariant::Variant;

const FONT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/test_assets/Cantarell-Regular.ttf"
));

/// Writes the fixture font and an optional sidecar, then loads it the way a
/// project does — through the registry, which is what looks the sidecar up.
fn load_with_sidecar(params_body: Option<&str>) -> std::sync::Arc<gdresource::Resource> {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("Cantarell-Regular.ttf");
    std::fs::write(&path, FONT).unwrap();
    if let Some(body) = params_body {
        std::fs::write(
            dir.path().join("Cantarell-Regular.ttf.import"),
            format!("[params]\n{body}"),
        )
        .unwrap();
    }
    ResourceFormatLoader::with_defaults()
        .load_resource(&path)
        .expect("the font loads")
}

/// **The acceptance clause.** A sidecar setting MSDF on produces a FontFile
/// reporting both the mode and its pixel range.
#[test]
fn msdf_options_from_the_sidecar_reach_the_font_resource() {
    let res = load_with_sidecar(Some(
        "multichannel_signed_distance_field=true\nmsdf_pixel_range=8\nmsdf_size=48\n",
    ));

    // ⚠️ Asserted FIRST: if the font itself failed to parse, every option
    // assertion below would be about a resource nobody would ever use.
    assert_eq!(
        res.get_property("parsed"),
        Some(&Variant::Bool(true)),
        "the fixture font must still parse — options must not replace the import"
    );
    assert_eq!(
        res.get_property("multichannel_signed_distance_field"),
        Some(&Variant::Bool(true)),
        "the MSDF mode must survive the import"
    );
    assert_eq!(
        res.get_property("msdf_pixel_range"),
        Some(&Variant::Int(8)),
        "…and so must its pixel range"
    );
    assert_eq!(res.get_property("msdf_size"), Some(&Variant::Int(48)));
}

/// 🔑 The control: with no sidecar, none of those keys appears.
///
/// ⚠️ Without this the test above passes against an importer that hardcodes
/// `msdf_pixel_range = 8` — which is Godot's default, so the hardcoded value
/// and the honoured one are the SAME NUMBER. Absence is the only reading that
/// distinguishes them.
#[test]
fn a_font_with_no_sidecar_carries_no_option_keys() {
    let res = load_with_sidecar(None);
    assert_eq!(
        res.get_property("parsed"),
        Some(&Variant::Bool(true)),
        "the plain path must still import the font"
    );
    for key in [
        "multichannel_signed_distance_field",
        "msdf_pixel_range",
        "msdf_size",
        "hinting",
        "antialiasing",
        "oversampling",
    ] {
        assert!(
            res.get_property(key).is_none(),
            "🔑 `{key}` must be absent when no sidecar sets it — a value here \
             is hardcoded, and for msdf_pixel_range the hardcoded number would \
             match Godot's default and hide the bug"
        );
    }
}

/// 🔑 The options whose Godot defaults are NOT the obvious ones.
///
/// ⚠️ Four dynamic-font bools default to **true** in Godot
/// (`disable_embedded_bitmaps`, `allow_system_fallback`,
/// `keep_rounding_remainders`, `compress`) and three ints default non-zero
/// (`hinting` 1, `antialiasing` 1, `subpixel_positioning` 4). Each is written
/// here to the value OPPOSITE its default, so an implementation that quietly
/// substituted a default instead of reading the sidecar fails on every line.
#[test]
fn non_obvious_defaults_are_read_from_the_sidecar_not_assumed() {
    let res = load_with_sidecar(Some(
        "disable_embedded_bitmaps=false\n\
         allow_system_fallback=false\n\
         keep_rounding_remainders=false\n\
         compress=false\n\
         hinting=0\n\
         antialiasing=0\n\
         subpixel_positioning=0\n\
         force_autohinter=true\n\
         modulate_color_glyphs=true\n\
         generate_mipmaps=true\n",
    ));
    for key in [
        "disable_embedded_bitmaps",
        "allow_system_fallback",
        "keep_rounding_remainders",
        "compress",
    ] {
        assert_eq!(
            res.get_property(key),
            Some(&Variant::Bool(false)),
            "`{key}` defaults to TRUE in Godot, so `false` here can only come \
             from the sidecar actually being read"
        );
    }
    for key in ["hinting", "antialiasing", "subpixel_positioning"] {
        assert_eq!(
            res.get_property(key),
            Some(&Variant::Int(0)),
            "`{key}` has a non-zero Godot default, so 0 proves the sidecar won"
        );
    }
    for key in [
        "force_autohinter",
        "modulate_color_glyphs",
        "generate_mipmaps",
    ] {
        assert_eq!(res.get_property(key), Some(&Variant::Bool(true)));
    }
}

/// `oversampling` is a float, and `0` is a meaningful value rather than "unset".
///
/// ⚠️ Godot documents `0` as "use the viewport scale", not "no oversampling",
/// so an implementation that treated zero as absent would drop an explicit
/// author choice. Written as `0.0` precisely to pin that it is still recorded.
#[test]
fn oversampling_is_a_float_and_zero_is_recorded() {
    let res = load_with_sidecar(Some("oversampling=0.0\n"));
    assert_eq!(
        res.get_property("oversampling"),
        Some(&Variant::Float(0.0)),
        "an explicit 0 must be stored as a Float, not skipped as unset"
    );

    let res = load_with_sidecar(Some("oversampling=2.5\n"));
    assert_eq!(
        res.get_property("oversampling"),
        Some(&Variant::Float(2.5)),
        "a fractional oversampling must not be truncated to an Int"
    );
}

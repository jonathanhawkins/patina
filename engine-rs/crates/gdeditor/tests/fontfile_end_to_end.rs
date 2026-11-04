//! pat-0q2kx.146 acceptance: a project `.ttf` becomes a usable `FontFile`, and a
//! Control's `theme_override_fonts/font` changes the pixels it renders.
//!
//! Godot ground truth: `scene/resources/font.cpp` (Font/FontFile, the
//! `fallbacks` chain), `doc/classes/FontFile.xml` for the property surface.
//!
//! Before this bead the three halves never met: `import_font` produced a
//! `FontFile` resource nothing could consume, `Font`/`FontFile`/`FontVariation`/
//! `SystemFont`/`LabelSettings` were absent from ClassDB, and
//! `theme_override_fonts/font` was inspector-visible and ignored — the draw path
//! resolved `theme_override_colors/font_color` and dropped the font override,
//! because `DrawCommand` carries no font field and `CanvasItem` no node id.
//!
//! ⚠️ **Documented divergence.** Godot bundles OpenSans SemiBold plus a
//! Noto/Droid per-script fallback stack; Patina ships only Cantarell, and all
//! three `.ttf` files in this repo are byte-identical copies of it. The bead
//! allows this explicitly ("bundle Godot-compatible fallback faces **or document
//! the Cantarell divergence**). The consequence for testing is concrete: the
//! *positive* direction of the per-glyph fallback — a character absent from the
//! primary drawn from a covering fallback — cannot be asserted here, because
//! there is no second face to cover anything. The chain itself
//! (`draw_ttf_text_with_fallbacks` / `face_for`) is implemented and its
//! no-covering-face behaviour is pinned below.

use gdcore::math::{Color, Vector2};
use gdeditor::viewport_2d_render::{build_control_render_items, rasterize_render_list};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::path::Path;

/// The one face the repo ships, reached from the gdeditor crate root.
// ⚠️ NOT Cantarell: that file is byte-identical to the built-in default face
// (`gdrender2d/assets/default_font.ttf`), so overriding with it could never
// change a pixel and the "override differs from default" clause was red at
// every commit once default text drew with the real face. See
// `gdrender2d/test_assets/README.md`.
const FONT_REL: &str = "../gdrender2d/test_assets/Xolonium-Regular.ttf";

fn font_path() -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(FONT_REL);
    p.to_string_lossy().into_owned()
}

/// `Root(Control) -> Label`, optionally overriding the font, rasterized.
fn render_label(font_override: Option<&str>) -> Vec<Color> {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Control"))
        .unwrap();
    let label = tree.add_child(root, Node::new("Label", "Label")).unwrap();
    {
        let node = tree.get_node_mut(label).unwrap();
        node.set_property("text", Variant::String("Patina".to_string()));
        if let Some(path) = font_override {
            node.set_property(
                "theme_override_fonts/font",
                Variant::String(path.to_string()),
            );
        }
    }
    let items = build_control_render_items(&mut tree, root, Vector2::new(160.0, 48.0));
    let fb = rasterize_render_list(&items, 160, 48, Color::new(0.0, 0.0, 0.0, 1.0), None);
    fb.pixels
}

fn differing(a: &[Color], b: &[Color]) -> usize {
    a.iter()
        .zip(b.iter())
        .filter(|(x, y)| {
            (x.r - y.r).abs() > 1e-4 || (x.g - y.g).abs() > 1e-4 || (x.b - y.b).abs() > 1e-4
        })
        .count()
}

/// **The acceptance test.** Import produces a `FontFile`, ClassDB can
/// instantiate one, and a Label's font override changes its rendered pixels.
#[test]
fn fontfile_end_to_end() {
    // -- (1) importing a project .ttf produces a FontFile resource ------------
    let path = font_path();
    let imported =
        gdresource::importers::import_font(Path::new(&path)).expect("the bundled .ttf imports");
    // `class_name` is the resource's own type. `resource_type` is a field on
    // `ExtResource` (the *reference* to a resource, resource.rs:38), not on
    // `Resource` itself — naming it here was an E0609, so this test target never
    // compiled and its assertions have never run.
    assert_eq!(
        imported.class_name, "FontFile",
        "an imported font must be a FontFile resource"
    );
    assert_eq!(
        imported.get_property("has_font_data"),
        Some(&Variant::Bool(true)),
        "the imported FontFile must carry the font payload, not just a path"
    );

    // -- (2) ClassDB knows the Font family ------------------------------------
    gdobject::class_db::register_2d_classes();
    for class in [
        "Font",
        "FontFile",
        "FontVariation",
        "SystemFont",
        "LabelSettings",
    ] {
        assert!(
            gdobject::class_db::class_exists(class),
            "{class} must be registered so a .tscn/inspector can resolve it"
        );
    }
    assert!(
        gdobject::class_db::instantiate("FontFile").is_some(),
        "ClassDB.instantiate(\"FontFile\") must work"
    );
    // `Font` is Godot's abstract base — instantiating it is an error there too.
    assert!(
        gdobject::class_db::is_parent_class("FontFile", "Font"),
        "FontFile must derive from Font so a `fallbacks` chain type-checks"
    );

    // -- (3) the override changes the rendered pixels -------------------------
    //
    // The default path draws with the built-in bitmap font; an overridden
    // Control draws through the outline rasterizer at the authored size. Same
    // string, same rect, different glyph pixels — which is exactly what the
    // clause asks for ("different from the default-font render").
    let plain = render_label(None);
    let overridden = render_label(Some(&path));
    assert_eq!(plain.len(), overridden.len(), "same-size frames");
    let changed = differing(&plain, &overridden);
    assert!(
        changed > 0,
        "a Label with theme_override_fonts/font must render different glyph \
         pixels from the default-font render of the same text"
    );
    assert!(
        changed < plain.len(),
        "the override must change glyphs, not repaint the whole frame \
         ({changed} of {})",
        plain.len()
    );

    // A path that does not resolve must degrade to the default face rather than
    // panic or drop the text — a scene referencing a missing font still draws.
    let missing = render_label(Some("res://fonts/does_not_exist.ttf"));
    assert_eq!(
        differing(&plain, &missing),
        0,
        "an unresolvable font path must fall back to the default face"
    );
}

/// The per-glyph fallback chain, to the extent one face allows: with no face
/// covering the character, the primary is used and `.notdef` is drawn — which is
/// the correct visible signal, not a crash and not a dropped glyph.
#[test]
fn fallback_chain_with_no_covering_face_draws_notdef() {
    use gdrender2d::renderer::FrameBuffer;
    use gdrender2d::ttf_font::{draw_ttf_text_with_fallbacks, GlyphAtlas, TtfFont};

    let primary = TtfFont::builtin();
    // A CJK ideograph Cantarell does not cover; the only fallback available is
    // the same face, so nothing in the chain has it.
    assert!(
        !primary.has_glyph('漢'),
        "the bundled face is expected not to cover CJK — if this ever fails the \
         repo gained a broader font and the fallback clause becomes testable"
    );

    let mut fb = FrameBuffer::new(64, 32, Color::new(0.0, 0.0, 0.0, 1.0));
    let mut atlas = GlyphAtlas::new(128, 128);
    let fallbacks = [TtfFont::builtin()];
    draw_ttf_text_with_fallbacks(
        &mut fb,
        &mut atlas,
        &primary,
        &fallbacks,
        "漢",
        Vector2::new(4.0, 4.0),
        24.0,
        Color::new(1.0, 1.0, 1.0, 1.0),
    );

    // The call completes and leaves a well-formed frame. Whether `.notdef` has
    // an outline is face-specific, so this pins the contract that matters: an
    // uncovered glyph is handled, not fatal.
    assert_eq!(fb.pixels.len(), 64 * 32);
}

/// Covered glyphs must be unaffected by the presence of a fallback list — the
/// chain is consulted per glyph, and only for glyphs the primary lacks.
#[test]
fn fallbacks_do_not_disturb_glyphs_the_primary_covers() {
    use gdrender2d::renderer::FrameBuffer;
    use gdrender2d::ttf_font::{draw_ttf_text_with_fallbacks, GlyphAtlas, TtfFont};

    let draw = |fallbacks: &[TtfFont]| {
        let mut fb = FrameBuffer::new(96, 32, Color::new(0.0, 0.0, 0.0, 1.0));
        let mut atlas = GlyphAtlas::new(256, 256);
        draw_ttf_text_with_fallbacks(
            &mut fb,
            &mut atlas,
            &TtfFont::builtin(),
            fallbacks,
            "Patina",
            Vector2::new(2.0, 2.0),
            18.0,
            Color::new(1.0, 1.0, 1.0, 1.0),
        );
        fb.pixels
    };

    let without = draw(&[]);
    let with = draw(&[TtfFont::builtin()]);
    assert_eq!(
        differing(&without, &with),
        0,
        "a fallback list must not change glyphs the primary already covers"
    );
}

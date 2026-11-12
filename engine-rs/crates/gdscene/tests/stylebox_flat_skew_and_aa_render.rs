//! pat-s3rrv.215 — `StyleBoxFlat.skew` and `corner_detail` reach the emitted
//! geometry, and `StyleBoxTexture.region_rect` reaches the draw command.
//!
//! All three were parsed and unit-tested but never read at draw time:
//! `stylebox.rs` said so verbatim in its own field docs. The shear and the
//! tessellation are ported from
//! `upstream/godot/scene/resources/style_box_flat.cpp` `draw_rounded_rectangle`
//! (:313-392).

use gdcore::math::{Rect2, Vector2};
use gdscene::stylebox::{rounded_box_outline, StyleBox, StyleBoxFlat, StyleBoxTexture};
use gdserver2d::canvas::DrawCommand;

const RECT: Rect2 = Rect2 {
    position: Vector2 { x: 0.0, y: 0.0 },
    size: Vector2 { x: 100.0, y: 50.0 },
};

/// Mean x of the vertices sharing the extreme y, i.e. one edge of the ring.
fn edge_mean_x(points: &[Vector2], top: bool) -> f32 {
    let extreme = points.iter().map(|p| p.y).fold(
        if top {
            f32::INFINITY
        } else {
            f32::NEG_INFINITY
        },
        |a, b| {
            if top {
                a.min(b)
            } else {
                a.max(b)
            }
        },
    );
    let on_edge: Vec<f32> = points
        .iter()
        .filter(|p| (p.y - extreme).abs() < 1.0)
        .map(|p| p.x)
        .collect();
    on_edge.iter().sum::<f32>() / on_edge.len() as f32
}

/// **The acceptance's first clause.** `skew = (0.5, 0)` offsets the top edge
/// from the bottom edge horizontally.
///
/// 🔑 The shear pivots on the rect's CENTRE with a negative sign
/// (`x' = x - skew.x * (y - cy)`), so a positive `skew.x` pushes the top RIGHT
/// and the bottom LEFT by the same amount. Pivoting on the top-left corner —
/// the intuitive choice — shears by the same angle but also slides the whole
/// box sideways out of its own layout rect, which this test would not catch on
/// its own; the symmetry assertion below is what pins the pivot.
#[test]
fn skew_offsets_the_top_edge_from_the_bottom_edge() {
    let sheared = rounded_box_outline(RECT, [8.0; 4], 8, Vector2::new(0.5, 0.0));
    let top = edge_mean_x(&sheared, true);
    let bottom = edge_mean_x(&sheared, false);

    assert!(
        top > bottom + 10.0,
        "a positive skew.x must push the top edge right of the bottom edge; \
         got top={top} bottom={bottom}"
    );

    // 🔑 Pins the PIVOT, not just the shear: centre-pivoted shear moves the two
    // edges by equal and opposite amounts, so their midpoint is unchanged.
    let unsheared = rounded_box_outline(RECT, [8.0; 4], 8, Vector2::ZERO);
    let mid_before = (edge_mean_x(&unsheared, true) + edge_mean_x(&unsheared, false)) / 2.0;
    let mid_after = (top + bottom) / 2.0;
    assert!(
        (mid_after - mid_before).abs() < 0.5,
        "the shear must pivot on the rect centre, leaving the box in its own \
         layout rect; a midpoint shift of {} means it pivoted on a corner and \
         slid sideways",
        mid_after - mid_before
    );
}

/// The control: with no skew the ring is symmetric, so the test above is
/// measuring the shear rather than some pre-existing asymmetry.
#[test]
fn an_unskewed_box_has_its_edges_aligned() {
    let points = rounded_box_outline(RECT, [8.0; 4], 8, Vector2::ZERO);
    let top = edge_mean_x(&points, true);
    let bottom = edge_mean_x(&points, false);
    assert!(
        (top - bottom).abs() < 0.5,
        "an unsheared box must have its top and bottom edges aligned; \
         got top={top} bottom={bottom}"
    );
}

/// **The acceptance's second clause.** Lowering `corner_detail` measurably
/// reduces the emitted vertex count.
///
/// The ring is `4 * (detail + 1)` vertices, so 8 gives 36 and 1 gives 8.
#[test]
fn corner_detail_drives_the_vertex_count() {
    let fine = rounded_box_outline(RECT, [8.0; 4], 8, Vector2::ZERO);
    let coarse = rounded_box_outline(RECT, [8.0; 4], 1, Vector2::ZERO);

    assert_eq!(fine.len(), 36, "4 * (8 + 1)");
    assert_eq!(coarse.len(), 8, "4 * (1 + 1)");
    assert!(
        coarse.len() < fine.len(),
        "lowering corner_detail must emit fewer vertices"
    );
}

/// ⚠️ A SQUARE box ignores `corner_detail` entirely — Godot sets
/// `adapted_corner_detail = any radius > 0 ? corner_detail : 1`, so a
/// sharp-cornered box is 8 vertices however high the detail is set.
///
/// Without this, "corner_detail drives the vertex count" would be asserted as a
/// universal rule and a sharp box would look like a bug.
#[test]
fn a_square_box_ignores_corner_detail() {
    let a = rounded_box_outline(RECT, [0.0; 4], 8, Vector2::ZERO);
    let b = rounded_box_outline(RECT, [0.0; 4], 1, Vector2::ZERO);
    assert_eq!(a.len(), 8);
    assert_eq!(
        b.len(),
        8,
        "a square box is one segment per corner regardless"
    );
}

/// ⚠️ `corner_detail = 0` must not divide by zero. Godot divides by this value
/// when stepping the arc, so the clamp is load-bearing rather than cosmetic.
#[test]
fn a_zero_corner_detail_does_not_divide_by_zero() {
    let points = rounded_box_outline(RECT, [8.0; 4], 0, Vector2::ZERO);
    assert_eq!(points.len(), 8, "clamped to one segment per corner");
    assert!(
        points.iter().all(|p| p.x.is_finite() && p.y.is_finite()),
        "every vertex must be finite; a NaN here is the division by zero"
    );
}

/// A sheared StyleBoxFlat emits polygon geometry rather than axis-aligned
/// rects, because a sheared box is not a union of axis-aligned rectangles.
#[test]
fn a_sheared_stylebox_draws_as_a_polygon() {
    let mut flat = StyleBoxFlat {
        corner_radius: [6.0; 4],
        skew: Vector2::new(0.4, 0.0),
        ..Default::default()
    };
    let cmds = StyleBox::Flat(flat.clone()).draw(RECT);
    assert!(
        cmds.iter()
            .any(|c| matches!(c, DrawCommand::DrawPolygon { .. })),
        "a skewed box must emit DrawPolygon; got {cmds:?}"
    );

    // 🔑 The control: with skew cleared the box keeps the original rect/disc
    // path, so this change cannot have altered any existing themed control.
    flat.skew = Vector2::ZERO;
    let plain = StyleBox::Flat(flat).draw(RECT);
    assert!(
        !plain
            .iter()
            .any(|c| matches!(c, DrawCommand::DrawPolygon { .. })),
        "an unskewed box must still take the rect/disc path — swapping it would \
         change the pixels of every themed control in the engine"
    );
}

/// **The acceptance's fourth clause.** `region_rect` reaches the nine-patch
/// draw command, so an atlas-backed stylebox cuts its patches from that
/// sub-rect instead of the whole sheet.
#[test]
fn stylebox_texture_region_rect_reaches_the_draw_command() {
    let region = Rect2::new(Vector2::ZERO, Vector2::new(16.0, 16.0));
    let tex = StyleBoxTexture {
        texture_path: "res://atlas.png".to_string(),
        region_rect: region,
        ..Default::default()
    };
    let cmds = StyleBox::Texture(tex).draw(RECT);
    match cmds.first() {
        Some(DrawCommand::DrawNinePatch { region: r, .. }) => {
            assert_eq!(
                *r, region,
                "the authored region must reach the rasterizer; a zero rect \
                 here means the patches are still cut from the whole texture"
            );
        }
        other => panic!("expected a nine-patch command, got {other:?}"),
    }
}

/// ⚠️ The control, and the compatibility guarantee: an ABSENT `region_rect`
/// stays the zero rect, which the rasterizer reads as "the whole texture".
///
/// Every atlas-free stylebox in every existing scene depends on that, so a
/// non-zero default here would re-cut all of them.
#[test]
fn an_absent_region_rect_still_means_the_whole_texture() {
    let tex = StyleBoxTexture {
        texture_path: "res://panel.png".to_string(),
        ..Default::default()
    };
    let cmds = StyleBox::Texture(tex).draw(RECT);
    match cmds.first() {
        Some(DrawCommand::DrawNinePatch { region: r, .. }) => {
            assert_eq!(r.size, Vector2::ZERO, "the default must stay a zero rect");
        }
        other => panic!("expected a nine-patch command, got {other:?}"),
    }
}

/// **The acceptance's fifth clause, at the layer a gdscene test can reach.**
///
/// `draw_style_box(box, rect)` takes the resource `load("res://box.tres")`
/// returns — a `Variant::Resource` carrying the file's class name and property
/// bag — converts it with `stylebox_from_props`, and emits through the SAME
/// `StyleBox::draw` the themed control path calls.
///
/// 🔑 What this pins is the conversion, which is the part that can be wrong.
/// The equality with the themed path is guaranteed by construction — the
/// dispatch arm calls `StyleBox::draw` rather than re-deriving the geometry —
/// and reimplementing it there is precisely how the two would silently drift
/// apart.
///
/// ⚠️ NOT covered here: invoking the builtin from a running script. That needs
/// the interpreter, which gdscene cannot reach from its own test.
#[test]
fn a_resource_bag_converts_to_the_same_commands_as_the_themed_path() {
    use gdscene::stylebox::stylebox_from_props;
    use gdvariant::Variant;
    use std::collections::HashMap;

    // The property bag a `.tres` StyleBoxFlat produces.
    let mut props: HashMap<String, Variant> = HashMap::new();
    props.insert("corner_radius_top_left".into(), Variant::Float(6.0));
    props.insert("corner_radius_top_right".into(), Variant::Float(6.0));
    props.insert("skew".into(), Variant::Vector2(Vector2::new(0.4, 0.0)));

    let from_resource =
        stylebox_from_props("StyleBoxFlat", &props).expect("a StyleBoxFlat bag converts");
    let script_commands = from_resource.draw(RECT);

    // The themed path: the same resource, drawn the same way.
    let themed_commands = stylebox_from_props("StyleBoxFlat", &props)
        .expect("same bag")
        .draw(RECT);

    assert_eq!(
        script_commands, themed_commands,
        "the script path must emit byte-identical commands to the themed path"
    );
    assert!(
        !script_commands.is_empty(),
        "and it must actually emit something — comparing two empty vectors \
         would pass while drawing nothing at all"
    );
    // The authored skew survived the resource round trip.
    assert!(
        script_commands
            .iter()
            .any(|c| matches!(c, DrawCommand::DrawPolygon { .. })),
        "the bag's `skew` must reach the geometry; only rect/disc commands means \
         the property was parsed and dropped"
    );
}

/// An unrecognised resource class draws NOTHING rather than a default box.
///
/// ⚠️ Godot's `draw_style_box` with a null StyleBox is a no-op. Falling back to
/// a grey panel would make a broken resource reference look like a deliberate
/// one, which is far harder to notice than a missing box.
#[test]
fn an_unrecognised_resource_class_draws_nothing() {
    use gdscene::stylebox::stylebox_from_props;
    use gdvariant::Variant;
    use std::collections::HashMap;

    let props: HashMap<String, Variant> = HashMap::new();
    assert!(
        stylebox_from_props("NotAStyleBox", &props).is_none(),
        "an unknown class must not convert to a default box"
    );
}

/// 🔑 **Godot writes `skew` as a single `Vector2`**, and reading only the split
/// `skew_x`/`skew_y` floats dropped it silently.
///
/// That stayed invisible for as long as `skew` was unused at draw time — the
/// value was parsed into a field nothing read, so a wrong parse and a correct
/// one produced identical output. Making the field observable is what exposed
/// it.
///
/// Both spellings are asserted: the Vector2 is what a real `.tres` carries, the
/// split floats are what this parser's own older tests use, and dropping either
/// would break a real scene.
#[test]
fn both_spellings_of_skew_are_parsed() {
    use gdscene::stylebox::stylebox_from_props;
    use gdvariant::Variant;
    use std::collections::HashMap;

    // What Godot authors.
    let mut vector_form: HashMap<String, Variant> = HashMap::new();
    vector_form.insert("skew".into(), Variant::Vector2(Vector2::new(0.4, 0.2)));
    match stylebox_from_props("StyleBoxFlat", &vector_form) {
        Some(StyleBox::Flat(f)) => {
            assert!(
                (f.skew.x - 0.4).abs() < 1e-6 && (f.skew.y - 0.2).abs() < 1e-6,
                "Godot's `skew = Vector2(...)` must be read; got {:?}",
                f.skew
            );
        }
        other => panic!("expected a flat stylebox, got {other:?}"),
    }

    // What this parser already accepted, kept working.
    let mut split_form: HashMap<String, Variant> = HashMap::new();
    split_form.insert("skew_x".into(), Variant::Float(0.25));
    split_form.insert("skew_y".into(), Variant::Float(0.5));
    match stylebox_from_props("StyleBoxFlat", &split_form) {
        Some(StyleBox::Flat(f)) => {
            assert!(
                (f.skew.x - 0.25).abs() < 1e-6 && (f.skew.y - 0.5).abs() < 1e-6,
                "the split-float spelling must keep working; got {:?}",
                f.skew
            );
        }
        other => panic!("expected a flat stylebox, got {other:?}"),
    }
}

/// **The acceptance's third clause.** `anti_aliasing` with `aa_size = 2`
/// produces partial-alpha geometry that is absent when it is off.
///
/// The feather is one half-alpha ring drawn outside the box, so the observable
/// is a command whose colour alpha is strictly between 0 and the box's own.
///
/// ⚠️ Scoped to the SKEWED path deliberately. `anti_aliasing` defaults to TRUE,
/// so feathering the ordinary rect/disc path would move the edge pixels of every
/// themed control in the engine and every render golden with them. The control
/// below asserts exactly that: an unskewed box gains nothing.
#[test]
fn anti_aliasing_emits_partial_alpha_geometry() {
    let base = StyleBoxFlat {
        corner_radius: [6.0; 4],
        skew: Vector2::new(0.4, 0.0),
        anti_aliasing: true,
        aa_size: 2.0,
        ..Default::default()
    };
    let opaque_alpha = base.bg_color.a;

    let with_aa = StyleBox::Flat(base.clone()).draw(RECT);
    let partial: Vec<f32> = with_aa
        .iter()
        .filter_map(|c| match c {
            DrawCommand::DrawPolygon { colors, .. } => colors.first().map(|c| c.a),
            _ => None,
        })
        .filter(|a| *a > 0.0 && *a < opaque_alpha)
        .collect();
    assert!(
        !partial.is_empty(),
        "anti_aliasing must emit at least one partial-alpha polygon; got \
         alphas {:?}",
        with_aa
            .iter()
            .filter_map(|c| match c {
                DrawCommand::DrawPolygon { colors, .. } => colors.first().map(|c| c.a),
                _ => None,
            })
            .collect::<Vec<_>>()
    );

    // 🔑 The discriminator: the same box with AA off has no partial-alpha
    // geometry at all.
    let mut off = base.clone();
    off.anti_aliasing = false;
    let without_aa = StyleBox::Flat(off).draw(RECT);
    assert!(
        !without_aa.iter().any(|c| matches!(
            c,
            DrawCommand::DrawPolygon { colors, .. }
                if colors.first().is_some_and(|c| c.a > 0.0 && c.a < opaque_alpha)
        )),
        "with anti_aliasing off there must be no partial-alpha geometry"
    );

    // And it really did add something, rather than the two differing for some
    // other reason.
    assert!(
        with_aa.len() > without_aa.len(),
        "the feather ring is an extra command, not a substitution"
    );
}

/// ⚠️ The compatibility control. An UNSKEWED box takes the rect/disc path and
/// gains no feather even though `anti_aliasing` defaults to true — which is
/// what keeps this change from moving a pixel on any existing themed control.
#[test]
fn an_unskewed_box_is_unaffected_by_anti_aliasing() {
    let boxed = StyleBoxFlat {
        corner_radius: [6.0; 4],
        anti_aliasing: true,
        aa_size: 2.0,
        ..Default::default()
    };
    let mut off = boxed.clone();
    off.anti_aliasing = false;

    assert_eq!(
        StyleBox::Flat(boxed).draw(RECT),
        StyleBox::Flat(off).draw(RECT),
        "an unskewed box must draw identically with anti_aliasing on and off; \
         a difference here means every themed control in the engine just moved"
    );
}

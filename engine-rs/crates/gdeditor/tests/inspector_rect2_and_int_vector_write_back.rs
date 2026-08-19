//! pat-s3rrv.64 — the inspector's Rect2 editor writes back.
//!
//! `editor_ui.rs` rendered a Rect2 as four `<input type=number>` elements and
//! **never attached a change listener**. Contrast the Vector2 and Vector3
//! branches a few lines above, which both call `setProperty`. So every edit to
//! a `Sprite2D.region_rect`, a `Camera2D` limit or a `NinePatchRect.region_rect`
//! was silently discarded: the field accepted the keystroke, showed the new
//! number, and threw it away on blur. Nothing errored, and the value snapped
//! back on the next inspector refresh.
//!
//! Integer vectors had no branch at all and fell to the read-only
//! `insp-readonly` span, so a TileMap cell coordinate could be read and never
//! changed.
//!
//! ⚠️ These assertions slice the SPECIFIC branch out of the page source rather
//! than grepping the whole file. A file-wide search for `addEventListener`
//! passes trivially — the page is full of them — and would have passed against
//! the bug this test exists to catch.

/// Returns the source of the `if/else if` branch that starts with `marker`, up
/// to the next `} else if` / `} else {`.
fn branch_after<'a>(src: &'a str, marker: &str) -> &'a str {
    let start = src
        .find(marker)
        .unwrap_or_else(|| panic!("no branch matching {marker:?} in EDITOR_HTML"));
    let rest = &src[start + marker.len()..];
    let end = rest
        .find("} else")
        .unwrap_or_else(|| panic!("branch {marker:?} never closes"));
    &rest[..end]
}

/// **The acceptance test**: the Rect2 branch commits its edits.
#[test]
fn inspector_rect2_and_int_vector_write_back() {
    let html = gdeditor::editor_ui::EDITOR_HTML;

    let rect2 = branch_after(html, "type === 'Rect2'");
    assert!(
        rect2.contains("addEventListener('change'"),
        "the Rect2 editor must attach a change listener — without one every \
         edit is silently discarded, which is the entire bug",
    );
    assert!(
        rect2.contains("setProperty(nodeId, prop.name, { type: 'Rect2'"),
        "…and the listener must actually write the property back as a Rect2",
    );
    // All four components go together: a Rect2 is written as a whole value, so
    // the handler has to re-read its siblings rather than send one number.
    assert!(
        rect2.contains("querySelectorAll('.rect2-input')"),
        "the change handler must re-read every component, or editing W would \
         send a rect whose X/Y are whatever the handler captured",
    );

    // -- integer vectors now have a branch at all --------------------------
    let veci = branch_after(html, "type === 'Vector2i' || type === 'Vector3i'");
    assert!(
        veci.contains("addEventListener('change'"),
        "the integer-vector editor must commit its edits too",
    );
    assert!(
        veci.contains("parseInt("),
        "integer vectors must parse as INTEGERS — parseFloat would let a \
         fractional cell coordinate through",
    );
    assert!(
        veci.contains("ni.step = '1'"),
        "…and step by whole numbers, or the spinner offers values the type \
         cannot hold",
    );
}

/// An editor exists exactly for the types the engine can round-trip
/// (pat-s3rrv.64).
///
/// An editor for a type the server cannot decode LOOKS like it works and
/// silently 400s on save — the same class of bug as the Rect2 one: the user
/// sees their edit accepted and loses it.
///
/// ⚠️ UPDATED, not deleted. `Rect2i` / `Vector4` / `Vector4i` used to be the
/// undecodable ones and this test asserted they had NO editor. They now have
/// both a `Variant` kind and a `from_json` arm, so the assertion flipped while
/// the rule stayed the same. Check the DECODER, not the bead, before moving a
/// type across this line.
#[test]
fn no_editor_is_offered_for_a_type_the_server_cannot_decode() {
    let html = gdeditor::editor_ui::EDITOR_HTML;

    // ⚠️ THIS TEST WAS INVERTED, not deleted. It used to assert Rect2i /
    // Vector4 / Vector4i had NO editor, because `Variant` had no such kinds and
    // `from_json` would reject the write — an editor for an undecodable type
    // looks like it works and silently 400s.
    //
    // Those kinds now exist AND have `from_json` arms (serialize.rs: "Vector4"
    // decodes a flat 4-array, "Vector4i" the same as ints, "Rect2i" an object
    // of `{position:[2], size:[2]}`), so the writes round-trip and the editors
    // are correct to offer. The rule the test encodes is unchanged: an editor
    // may exist only for a type the server can decode.
    for present in [
        "type === 'Rect2i'",
        "type === 'Vector4'",
        "type === 'Vector4i'",
    ] {
        assert!(
            html.contains(present),
            "{present} must have an inspector branch now that from_json decodes \
             it — leaving it read-only is the gap this bead exists to close",
        );
    }

    // ⚠️ RID stays read-only for a DIFFERENT reason, and the distinction
    // matters: `from_json` *can* decode it (`"RID" => Variant::Rid(...)`), so
    // "undecodable" would be a false justification. A RID is an opaque runtime
    // server handle — Godot's inspector exposes no editor for one either, and
    // typing a raw handle number is not a meaningful edit.
    assert!(
        !html.contains("type === 'RID'"),
        "RID must stay read-only: it is an opaque runtime handle, not a value \
         a user can meaningfully type",
    );
}

/// Rect2i commits as INTEGERS in the object-of-arrays shape its decoder wants.
///
/// It rides in the compound branch rather than with the flat integer vectors
/// because `from_json` reads `{position:[2], size:[2]}` for it, not a flat list.
#[test]
fn rect2i_commits_as_an_integer_object_of_arrays() {
    let html = gdeditor::editor_ui::EDITOR_HTML;
    let compound = branch_after(
        html,
        "type === 'AABB' || type === 'Transform2D' || type === 'Basis' || type === 'Rect2i'",
    );
    assert!(
        compound.contains("{ key: 'position', n: 2 }, { key: 'size', n: 2 }"),
        "Rect2i must emit position/size groups of two:\n{compound}"
    );
    assert!(
        compound.contains("var groupsAreInt = (type === 'Rect2i');"),
        "Rect2i's components are integers, not floats:\n{compound}"
    );
    assert!(
        compound.contains("groupsAreInt ? (parseInt(inp.value, 10) || 0)"),
        "…so the commit must parse them as integers:\n{compound}"
    );
}

/// Projection commits the FLAT 16-element array its decoder requires
/// (pat-s3rrv.64).
///
/// `serialize.rs` rejects any other length outright (`arr.len() != 16`) and
/// fills `m[i / 4][i % 4]`, so the grid is ROW MAJOR and the commit must send
/// one flat list — not four rows, and not an object of named axes like the
/// Transform2D/Basis family next door.
#[test]
fn projection_commits_as_a_flat_sixteen_element_array() {
    let html = gdeditor::editor_ui::EDITOR_HTML;
    let proj = branch_after(html, "type === 'Projection'");

    assert!(
        proj.contains("addEventListener('change', commitProjection)"),
        "the Projection editor must commit:\n{proj}"
    );
    assert!(
        proj.contains("setProperty(nodeId, prop.name, { type: 'Projection', value: nums })"),
        "…sending a FLAT array, which is the only shape from_json accepts:\n{proj}"
    );
    // 4x4, read back in DOM order so the flat list stays row major.
    assert!(
        proj.contains("projVals[row * 4 + pc]"),
        "the grid must be indexed row-major to match m[i / 4][i % 4]:\n{proj}"
    );
    assert!(
        proj.contains("new Array(16).fill(0)"),
        "a Projection is fixed at 16 cells, so a malformed value must still \
         render a full grid rather than an empty editor:\n{proj}"
    );
}

/// The Packed* family gets a resizable indexed list, as EditorPropertyArray
/// does (`editor_properties_array_dict.cpp`) — pat-s3rrv.64.
#[test]
fn packed_arrays_get_a_resizable_indexed_editor() {
    let html = gdeditor::editor_ui::EDITOR_HTML;

    // Every Packed type `from_json` can decode needs a spec. One without falls
    // to the read-only span and cannot be edited at all — the gap this closes.
    for ty in [
        "PackedByteArray",
        "PackedInt32Array",
        "PackedInt64Array",
        "PackedFloat32Array",
        "PackedFloat64Array",
        "PackedStringArray",
        "PackedVector2Array",
        "PackedVector3Array",
        "PackedVector4Array",
        "PackedColorArray",
    ] {
        assert!(
            html.contains(&format!("{ty}:")),
            "{ty} decodes in serialize.rs but has no inspector spec, so it stays \
             read-only"
        );
    }

    // ⚠️ Stride is what makes the flat wire format correct: the decoder chunks
    // a FLAT list (pairs for Vector2, triples for Vector3) and rejects a length
    // that is not a whole multiple. A wrong stride here produces an array the
    // server refuses outright.
    for (ty, stride) in [
        ("PackedVector2Array", 2),
        ("PackedVector3Array", 3),
        ("PackedVector4Array", 4),
        ("PackedColorArray", 4),
        ("PackedByteArray", 1),
    ] {
        assert!(
            html.contains(&format!("{ty}: {{ stride: {stride}")),
            "{ty} must have stride {stride} to match how from_json chunks it"
        );
    }

    let branch = branch_after(html, "PACKED_ARRAY_SPECS[type]) {");
    assert!(
        branch.contains("addEventListener('change'"),
        "editing an element must commit:\n{branch}"
    );
    assert!(
        branch.contains("array-add") && branch.contains("array-remove"),
        "a Packed array is variable-length, so the editor must offer add AND \
         remove — that is what makes it EditorPropertyArray rather than a fixed \
         component grid:\n{branch}"
    );
    assert!(
        branch.contains("next.splice(idx * pspec.stride, pspec.stride);"),
        "remove must drop a whole ELEMENT: taking one slot out of a Vector2 \
         array leaves an odd length that the decoder rejects outright:\n{branch}"
    );
    // Add/remove must read the live inputs, not the values captured at render,
    // or edits typed since the row was drawn are silently dropped on resize.
    assert!(
        branch.contains("var next = packedValues();"),
        "add/remove must start from the CURRENT input values:\n{branch}"
    );
}

/// `PackedByteArray` is the one clamped element type.
///
/// Its decoder is `u8::try_from(x.as_u64()?)`, so a negative or >255 entry makes
/// the conversion fail and **the whole array is rejected**, not just the bad
/// element. Clamping in the editor is therefore load-bearing, not cosmetic.
#[test]
fn packed_byte_elements_are_clamped_to_the_range_u8_can_hold() {
    let html = gdeditor::editor_ui::EDITOR_HTML;
    assert!(
        html.contains("min: 0, max: 255"),
        "PackedByteArray's spec must bound its elements to 0..=255",
    );
    assert!(
        html.contains("if (spec.min != null && n < spec.min) n = spec.min;")
            && html.contains("if (spec.max != null && n > spec.max) n = spec.max;"),
        "…and the coercion must actually clamp to it, since one out-of-range \
         element makes from_json reject the entire write",
    );
}

/// Quaternion, AABB and Transform2D commit — each in the SHAPE its decoder
/// requires (pat-s3rrv.64).
///
/// This is the assertion that matters most for these three: `serialize.rs`
/// decodes a Quaternion from a flat `[x,y,z,w]` but an AABB from
/// `{position:[3], size:[3]}` and a Transform2D from `{x:[2], y:[2],
/// origin:[2]}`. Sending a flat array for the latter two is rejected by
/// `from_json`, so the edit vanishes at the server instead of the browser —
/// the same silent discard, one layer deeper.
#[test]
fn compound_types_commit_in_the_shape_their_decoder_requires() {
    let html = gdeditor::editor_ui::EDITOR_HTML;

    let quat = branch_after(html, "type === 'Quaternion'");
    assert!(
        quat.contains("addEventListener('change'"),
        "the Quaternion editor must commit",
    );
    assert!(
        quat.contains("setProperty(nodeId, prop.name, { type: 'Quaternion', value: vals })"),
        "…sending a FLAT array, which is what the Quaternion decoder accepts",
    );

    let compound = branch_after(
        html,
        "type === 'AABB' || type === 'Transform2D' || type === 'Basis'",
    );
    assert!(
        compound.contains("addEventListener('change', commitCompound)"),
        "the AABB/Transform2D editor must commit",
    );
    // The decoders read named keys off an object; a flat array would be
    // rejected outright.
    assert!(
        compound.contains("out[g.key] = nums") && compound.contains("value: out"),
        "…sending an OBJECT of named arrays, not a flat list — from_json reads \
         `position`/`size` for AABB and `x`/`y`/`origin` for Transform2D",
    );
    assert!(
        compound.contains("{ key: 'position', n: 3 }")
            && compound.contains("{ key: 'origin', n: 2 }"),
        "…with the component counts the decoders require: AABB position/size \
         are 3 each and Transform2D's columns are 2 each, and from_json returns \
         None on any other length",
    );
}

/// `Basis` reuses the object-of-arrays emitter, and `Transform3D` nests
/// (pat-s3rrv.64).
///
/// `parse_vec3` accepts EITHER `{x,y,z}` or a 3-element array, which is why
/// Basis needs no emitter of its own. `Transform3D` is the one shape that
/// genuinely nests — `{basis: {x:[3],y:[3],z:[3]}, origin:[3]}` — and
/// flattening it, or emitting `basis` as an array, is rejected by `from_json`.
#[test]
fn basis_reuses_the_array_emitter_and_transform3d_nests() {
    let html = gdeditor::editor_ui::EDITOR_HTML;

    let compound = branch_after(
        html,
        "type === 'AABB' || type === 'Transform2D' || type === 'Basis'",
    );
    assert!(
        compound.contains("{ key: 'x', n: 3 }") && compound.contains("{ key: 'z', n: 3 }"),
        "Basis must emit three 3-component columns through the shared emitter",
    );

    let t3d = branch_after(html, "type === 'Transform3D'");
    assert!(
        t3d.contains("value: { basis: basis, origin: origin }"),
        "Transform3D must nest its basis under a `basis` key — a flat object is \
         rejected by from_json and the edit vanishes at the server",
    );
    assert!(
        t3d.contains("{ key: 'basis.x', n: 3 }") && t3d.contains("{ key: 'origin', n: 3 }"),
        "…with three 3-component basis columns plus a 3-component origin",
    );
    assert!(
        t3d.contains("addEventListener('change', commitTransform3D)"),
        "…and it must actually commit",
    );
}

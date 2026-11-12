//! pat-s3rrv.82 acceptance: `patina-runner --debug-navigation` emits the
//! navmesh, not just the flag.
//!
//! The bead asks that the runner "reports the flag enabled and emits navmesh
//! draw items". The flag half was already covered; this is the geometry half,
//! through the REAL binary rather than the library function, because the two
//! output sites that carry it live in `main.rs` and a `--lib` test never
//! compiles them.
//!
//! ⚠️ The fixture authors `__navmesh_vertices__` and NO `__navmesh_polygons__`,
//! which exercises the documented implicit-face fallback: vertices with no
//! polygon list are one face over all of them. That is the behaviour two
//! independent copies of the parser used to implement separately, so pinning it
//! end to end is worth more than a hand-written index list.

use std::process::Command;

fn fixture_path() -> String {
    format!(
        "{}/../../fixtures/scenes/navigation_baked_region.tscn",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn run(args: &[&str]) -> serde_json::Value {
    let bin = env!("CARGO_BIN_EXE_patina-runner");
    let output = Command::new(bin)
        .arg(fixture_path())
        .arg("--frames")
        .arg("1")
        .args(args)
        .output()
        .expect("failed to execute patina-runner");
    assert!(
        output.status.success(),
        "patina-runner exited with error:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    serde_json::from_str(&String::from_utf8(output.stdout).expect("non-UTF-8 stdout"))
        .expect("stdout is not valid JSON")
}

/// 🔑 `--debug-navigation` EMITS THE BAKED GEOMETRY, AND ONLY WHEN ASKED.
///
/// ⚠️ Both halves against the same scene. Asserting only that the key appears
/// would pass for a build that emitted it unconditionally, which would change
/// the output of every existing run — the exact thing the flag-reporting code
/// beside it is careful to avoid.
#[test]
fn debug_navigation_emits_navmesh_geometry() {
    let without = run(&[]);
    assert!(
        without.get("navigation_draw").is_none(),
        "a run without the flag must not carry navigation_draw: {without}"
    );

    let with = run(&["--debug-navigation"]);
    assert_eq!(
        with["debug_navigation"], true,
        "the flag itself must still be reported"
    );

    let regions = with["navigation_draw"]
        .as_array()
        .expect("navigation_draw must be an array");
    assert_eq!(
        regions.len(),
        1,
        "only the region with baked vertices is emitted; the unbaked sibling \
         is omitted entirely: {}",
        with["navigation_draw"]
    );
    assert_eq!(regions[0]["node"], "Baked");
    assert_eq!(
        regions[0]["polygons"],
        serde_json::json!([[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0]]]),
        "three vertices with no polygon list are ONE implicit face, in \
         node-local space"
    );
    assert_eq!(
        regions[0]["position"],
        serde_json::json!([12.0, 8.0]),
        "the region's position travels beside the polygons, not composed into \
         them — at the origin the two are indistinguishable, which is why the \
         fixture offsets this region"
    );
}

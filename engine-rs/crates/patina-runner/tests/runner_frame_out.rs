//! pat-mgixk.28 acceptance — a headless run draws 2D pixels, with no
//! `gdeditor` in the path.
//!
//! The only node-to-draw-command producer in the engine lived in
//! `gdeditor::viewport_2d_render`: `render_kind_for`, `collect_canvas_items`,
//! the camera and parallax transforms, the rasterizer and the light pass. Its
//! only callers were the editor binary and `gdeditor::scene_renderer`.
//! `patina-runner` emitted tree JSON and nothing else; `patina-web` depended on
//! neither `gdeditor` nor `gdrender2d` and only cleared its canvas. So a 2D game
//! run anywhere but the editor's Play tab drew no `Sprite2D`, `TileMapLayer` or
//! `Line2D` pixels at all.
//!
//! The builder now lives in `gdscene::canvas_render`, and `gdeditor` re-exports
//! it so every editor call site and test keeps working. This drives it through
//! the REAL binary, because the flag and the writer live in `main.rs` and a
//! `--lib` test never compiles them.

use std::process::Command;

/// Framebuffer size for the dump. Small on purpose: a 160x120 PPM is 57 KB and
/// every assertion below is positional, so a bigger buffer would only slow the
/// test down.
const W: u32 = 160;
const H: u32 = 120;

/// `sprite_scene.tscn` puts the camera at (40, 30) and the sprite at (60, 50)
/// with `centered = false`, so the sprite's own 8x8 occupies world
/// `(60..68, 50..58)` — centre `(64, 54)`.
const WORLD_CENTRE: (f32, f32) = (64.0, 54.0);

/// A `Camera2D` centres its target in the viewport, so the view translates by
/// `(W/2 - 40, H/2 - 30)` = `(40, 30)` and the sprite's centre lands at
/// `(104, 84)`.
const CAMERA_CENTRE: (f32, f32) = (104.0, 84.0);

/// The fixture paints ONE cell at `(0, 0)` with a 16x16 tile size, so the tile
/// occupies world `(0..16, 0..16)` — centre `(8, 8)` — and the same `(40, 30)`
/// view puts its drawn centre at `(48, 38)`.
const TILE_CENTRE: (f32, f32) = (48.0, 38.0);

fn fixture_path() -> String {
    format!(
        "{}/../../fixtures/scenes/sprite_scene.tscn",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// A binary PPM (P6) decoded to `(width, height, rgb triples)`.
fn read_ppm(path: &std::path::Path) -> (u32, u32, Vec<[u8; 3]>) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("no PPM at {}: {e}", path.display()));
    assert_eq!(&bytes[..2], b"P6", "not a binary PPM: {:?}", &bytes[..8]);
    // Header is three whitespace-separated ASCII fields after the magic.
    let mut fields = Vec::new();
    let mut idx = 2;
    while fields.len() < 3 {
        while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        let start = idx;
        while idx < bytes.len() && !bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        fields.push(
            std::str::from_utf8(&bytes[start..idx])
                .expect("ASCII header")
                .parse::<u32>()
                .expect("numeric header field"),
        );
    }
    idx += 1; // the single whitespace byte before the pixel data
    let (w, h) = (fields[0], fields[1]);
    let pixels: Vec<[u8; 3]> = bytes[idx..]
        .chunks_exact(3)
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    assert_eq!(
        pixels.len() as u32,
        w * h,
        "PPM body is {} px for a {w}x{h} header",
        pixels.len()
    );
    (w, h, pixels)
}

/// The centroid of the pixels matching `pick`, and how many there were.
///
/// Thresholds rather than exact equality, so a filtering or blend change does
/// not fail these for the wrong reason — but both are nowhere near the black
/// clear colour, and neither matches the other's channel.
fn centroid(w: u32, pixels: &[[u8; 3]], pick: impl Fn(&[u8; 3]) -> bool) -> (usize, f32, f32) {
    let mut n = 0usize;
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    for (i, p) in pixels.iter().enumerate() {
        if pick(p) {
            n += 1;
            sx += (i as u32 % w) as f32;
            sy += (i as u32 / w) as f32;
        }
    }
    if n == 0 {
        return (0, f32::NAN, f32::NAN);
    }
    (n, sx / n as f32, sy / n as f32)
}

/// The Sprite2D's texture: opaque red.
fn is_red(p: &[u8; 3]) -> bool {
    p[0] > 120 && p[1] < 80 && p[2] < 80
}

/// The painted tile, which samples the atlas's green half.
fn is_green(p: &[u8; 3]) -> bool {
    p[1] > 120 && p[0] < 80 && p[2] < 80
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// **The acceptance test.**
#[test]
fn runner_2d_frame_output_test() {
    let out_dir = std::env::temp_dir().join(format!("patina-frame-out-{}", std::process::id()));
    std::fs::create_dir_all(&out_dir).expect("temp dir");
    let out = out_dir.join("out.ppm");
    let _ = std::fs::remove_file(&out);

    let bin = env!("CARGO_BIN_EXE_patina-runner");
    let output = Command::new(bin)
        .arg(fixture_path())
        .arg("--frames")
        .arg("2")
        .arg("--frame-out")
        .arg(&out)
        .arg("--frame-size")
        .arg(format!("{W}x{H}"))
        .output()
        .expect("failed to execute patina-runner");
    assert!(
        output.status.success(),
        "patina-runner exited with error:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let (w, h, pixels) = read_ppm(&out);
    assert_eq!((w, h), (W, H), "the PPM must be the requested size");

    let (count, cx, cy) = centroid(w, &pixels, is_red);
    assert!(
        count >= 32,
        "the Sprite2D's texture must reach the framebuffer: found {count} red \
         pixels in a {W}x{H} frame, for an 8x8 opaque texture. Zero is the old \
         behaviour — the runner had no 2D draw path at all, so every pixel was \
         the clear colour."
    );

    // 🔑 The camera has to have been honoured. Asserted as a DISTANCE from the
    // world position rather than only as a match against the expected one, so
    // the test still fails loudly if the centring convention differs from the
    // one computed here, instead of passing because both happen to be wrong.
    let from_world = dist((cx, cy), WORLD_CENTRE);
    assert!(
        from_world > 20.0,
        "the sprite must be drawn at its CAMERA-relative position, not its \
         world position: centroid ({cx:.1}, {cy:.1}) is only {from_world:.1} px \
         from the un-transformed world centre {WORLD_CENTRE:?}. The camera is \
         offset (40, 30) from the sprite, so honouring it moves the sprite \
         50 px."
    );
    let from_camera = dist((cx, cy), CAMERA_CENTRE);
    assert!(
        from_camera <= 12.0,
        "the sprite's drawn centroid ({cx:.1}, {cy:.1}) should be near \
         {CAMERA_CENTRE:?} — the camera at (40, 30) centred in a {W}x{H} \
         viewport translates the world by (40, 30). It is {from_camera:.1} px \
         away, which means the view transform differs from the one this test \
         computes; the measured centroid is the number to reason from."
    );

    // -- the TileMapLayer cell ----------------------------------------------
    //
    // A different colour AND a different place, so neither assertion can be
    // satisfied by the other node's pixels.
    let (tiles, tx, ty) = centroid(w, &pixels, is_green);
    assert!(
        tiles >= 100,
        "the painted TileMapLayer cell must reach the framebuffer: found \
         {tiles} green pixels for one 16x16 tile (256 px). Zero means the \
         tilemap arm of the shared builder never ran — `tilemap_quads` and the \
         `TileSet` sub-resource it reads were editor-only before this."
    );
    let tile_off = dist((tx, ty), TILE_CENTRE);
    assert!(
        tile_off <= 12.0,
        "the tile's drawn centroid ({tx:.1}, {ty:.1}) should be near \
         {TILE_CENTRE:?} — cell (0, 0) at a 16x16 tile size is world \
         (0..16, 0..16), which the same (40, 30) view moves to (40..56, \
         30..46). It is {tile_off:.1} px away."
    );
    // ...and the two did not land on top of each other, which is the one way
    // both position assertions could pass on a builder that ignored node
    // transforms entirely.
    assert!(
        dist((cx, cy), (tx, ty)) > 30.0,
        "the sprite ({cx:.1}, {cy:.1}) and the tile ({tx:.1}, {ty:.1}) must be \
         drawn at DIFFERENT positions; overlapping centroids would mean the \
         builder placed everything at one spot"
    );

    let _ = std::fs::remove_file(&out);
}

/// The web runtime must link the shared builder ON the wasm target, and must
/// not link `gdeditor` either.
///
/// 🔑 The acceptance's web clause is "patina-web compiles for wasm32 with the
/// shared builder linked". A `cargo check --target wasm32-unknown-unknown` is
/// the only thing that proves the *compile*, and it cannot run inside a test —
/// but the regression that clause exists to catch is a DEPENDENCY one, and that
/// is checkable here. Nothing else in the suite looks at this manifest.
///
/// ⚠️ Presence alone is not enough: a dep declared under
/// `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` is absent from
/// exactly the build this clause is about, while still reading as "linked" to a
/// plain `contains` grep. So this tracks which section each line falls in and
/// only counts the ones a wasm32 build actually sees.
#[test]
fn patina_web_links_the_shared_canvas_builder_for_wasm() {
    let manifest = std::fs::read_to_string(format!(
        "{}/../patina-web/Cargo.toml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("patina-web Cargo.toml");

    let mut section = String::new();
    let mut visible_to_wasm: Vec<&str> = Vec::new();
    let mut offending: Vec<&str> = Vec::new();
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            section = trimmed.to_string();
            continue;
        }
        // A comment is not a dependency. The absence of `gdeditor` is actually
        // DOCUMENTED in a comment in this file, so a naive grep for it here
        // reports the opposite of the truth.
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        let applies_to_wasm = section == "[dependencies]"
            || (section.contains("target.") && !section.contains("not(target_arch"));
        if applies_to_wasm {
            visible_to_wasm.push(trimmed);
        }
        if trimmed.contains("gdeditor") {
            offending.push(trimmed);
        }
    }

    for crate_name in ["gdscene", "gdrender2d"] {
        assert!(
            visible_to_wasm
                .iter()
                .any(|l| l.starts_with(&format!("{crate_name} "))
                    || l.starts_with(&format!("{crate_name}="))),
            "patina-web must link `{crate_name}` on wasm32 — `gdscene::canvas_render` \
             is the shared scene-to-draw-list builder and `gdrender2d` holds the \
             `FrameBuffer` it fills. Without both in a section a wasm32 build sees, \
             the browser runtime is back to clearing its canvas and drawing no 2D \
             scene at all. Sections visible to wasm32 declare: {visible_to_wasm:?}"
        );
    }
    assert!(
        offending.is_empty(),
        "patina-web must not depend on gdeditor; found {offending:?}"
    );
}

/// The runner must not depend on `gdeditor` — the entire point of moving the
/// canvas builder into `gdscene`.
///
/// ⚠️ Asserted against the manifest, not against behaviour. A headless binary
/// that happens to work today would still be wrong if it linked the editor:
/// the dependency is the regression, and nothing else in the suite would catch
/// someone re-adding it.
#[test]
fn runner_does_not_depend_on_gdeditor() {
    let manifest = std::fs::read_to_string(format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR")))
        .expect("patina-runner Cargo.toml");
    let offending: Vec<&str> = manifest
        .lines()
        .filter(|l| {
            let l = l.trim();
            !l.starts_with('#') && l.contains("gdeditor")
        })
        .collect();
    assert!(
        offending.is_empty(),
        "patina-runner must not depend on gdeditor; found {offending:?}"
    );
}

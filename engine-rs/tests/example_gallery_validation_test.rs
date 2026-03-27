//! Validation tests for the example project gallery (pat-tyi1j).
//!
//! Ensures the gallery document exists, references real example files,
//! and covers at least 5 demo projects with required metadata.

use std::fs;
use std::path::Path;

fn repo_root() -> &'static Path {
    let manifest = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest).parent().unwrap()
}

fn examples_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples")
}

fn read_gallery() -> String {
    let path = repo_root().join("docs/EXAMPLE_GALLERY.md");
    fs::read_to_string(&path)
        .expect("docs/EXAMPLE_GALLERY.md must exist")
}

fn read_readme() -> String {
    let path = examples_dir().join("README.md");
    fs::read_to_string(&path)
        .expect("examples/README.md must exist")
}

// ===========================================================================
// 1. Documents exist
// ===========================================================================

#[test]
fn gallery_doc_exists() {
    assert!(
        repo_root().join("docs/EXAMPLE_GALLERY.md").exists(),
        "docs/EXAMPLE_GALLERY.md must exist"
    );
}

#[test]
fn examples_readme_exists() {
    assert!(
        examples_dir().join("README.md").exists(),
        "examples/README.md must exist"
    );
}

// ===========================================================================
// 2. Gallery has at least 5 demo entries
// ===========================================================================

#[test]
fn gallery_has_at_least_5_demos() {
    let gallery = read_gallery();
    // Count numbered section headers (## 1. ... ## 2. ... etc.)
    let demo_count = gallery.lines()
        .filter(|l| l.starts_with("## ") && l.chars().nth(3).map_or(false, |c| c.is_ascii_digit()))
        .count();
    assert!(
        demo_count >= 5,
        "gallery must have at least 5 demo entries (got {demo_count})"
    );
}

// ===========================================================================
// 3. Each referenced example file exists on disk
// ===========================================================================

#[test]
fn gallery_example_files_exist() {
    let examples = [
        "space_shooter.rs",
        "space_shooter_live.rs",
        "platformer_demo.rs",
        "demo_2d.rs",
        "hello_gdscript.rs",
        "run_project.rs",
        "editor.rs",
        "benchmarks.rs",
    ];

    for name in &examples {
        let path = examples_dir().join(name);
        assert!(
            path.exists(),
            "example file '{name}' must exist at {path:?}"
        );
    }
}

// ===========================================================================
// 4. Gallery content quality
// ===========================================================================

#[test]
fn gallery_has_title() {
    let gallery = read_gallery();
    assert!(
        gallery.starts_with("# Example Project Gallery"),
        "gallery must have correct title"
    );
}

#[test]
fn gallery_has_run_instructions() {
    let gallery = read_gallery();
    assert!(
        gallery.contains("cargo run --example"),
        "gallery must show how to run examples"
    );
}

#[test]
fn gallery_has_feature_coverage_matrix() {
    let gallery = read_gallery();
    assert!(
        gallery.contains("Feature Coverage Matrix"),
        "gallery must have a feature coverage matrix"
    );
}

#[test]
fn gallery_has_starter_template() {
    let gallery = read_gallery();
    assert!(
        gallery.contains("Minimal Starter Template"),
        "gallery must include a starter template"
    );
    assert!(
        gallery.contains("```rust"),
        "starter template must include Rust code"
    );
}

#[test]
fn gallery_documents_subsystems_per_demo() {
    let gallery = read_gallery();
    assert!(
        gallery.contains("Subsystems demonstrated"),
        "each demo must list subsystems demonstrated"
    );
}

// ===========================================================================
// 5. Key game demos are documented
// ===========================================================================

#[test]
fn gallery_has_space_shooter() {
    let gallery = read_gallery();
    assert!(gallery.contains("Space Shooter"), "must have Space Shooter demo");
    assert!(gallery.contains("space_shooter.rs"), "must reference the file");
}

#[test]
fn gallery_has_platformer() {
    let gallery = read_gallery();
    assert!(gallery.contains("Platformer Demo"), "must have Platformer Demo");
    assert!(gallery.contains("platformer_demo.rs"), "must reference the file");
}

#[test]
fn gallery_has_gdscript_example() {
    let gallery = read_gallery();
    assert!(gallery.contains("GDScript"), "must have GDScript example");
    assert!(gallery.contains("hello_gdscript.rs"), "must reference the file");
}

#[test]
fn gallery_has_project_loader() {
    let gallery = read_gallery();
    assert!(gallery.contains("Project Loader"), "must have Project Loader");
    assert!(gallery.contains("run_project.rs"), "must reference the file");
}

#[test]
fn gallery_has_editor() {
    let gallery = read_gallery();
    assert!(gallery.contains("Editor"), "must have Editor demo");
    assert!(gallery.contains("editor.rs"), "must reference the file");
}

// ===========================================================================
// 6. README references gallery
// ===========================================================================

#[test]
fn readme_links_to_gallery() {
    let readme = read_readme();
    assert!(
        readme.contains("EXAMPLE_GALLERY.md"),
        "examples README must link to the gallery document"
    );
}

#[test]
fn readme_documents_all_examples() {
    let readme = read_readme();

    let examples = [
        "space_shooter",
        "space_shooter_live",
        "platformer_demo",
        "demo_2d",
        "hello_gdscript",
        "run_project",
        "editor",
        "benchmarks",
    ];

    for ex in &examples {
        assert!(
            readme.contains(ex),
            "README must document example: {ex}"
        );
    }
}

// ===========================================================================
// 7. Gallery has cross-references
// ===========================================================================

#[test]
fn gallery_has_see_also() {
    let gallery = read_gallery();
    assert!(gallery.contains("## See Also"), "must have See Also section");
    assert!(gallery.contains("migration-guide.md"), "must link to migration guide");
    assert!(gallery.contains("GDSCRIPT_COMPATIBILITY.md"), "must link to GDScript compat");
}

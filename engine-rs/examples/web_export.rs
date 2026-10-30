//! Writes a deployable web export folder (pat-u3l8c.114).
//!
//! The IO driver for [`gdplatform::web_export`]: that module decides the file
//! names, the HTML, the build flags and which project files belong in the pack;
//! this walks the filesystem, builds the `.pck` and writes the folder.
//! `scripts/web-export.sh` is the one command that runs the wasm build, the
//! bindgen step and then this.
//!
//! ```text
//! cargo run --example web_export -- \
//!     --project ../fixtures/project --out dist --app "My Game" --bundle crates/patina-web/web
//! ```
//!
//! Flags:
//! - `--project <dir>`  project root whose files become the `.pck` (required,
//!   unless `--print-rustflags`)
//! - `--out <dir>`      folder to write (default: the config's `output_dir`)
//! - `--app <name>`     application name (default: `Patina Game`)
//! - `--bundle <dir>`   where the `wasm-bindgen` output already sits; its JS and
//!   wasm are copied into `--out`
//! - `--shell <file>`   custom HTML shell template (must contain `{{PATINA_…}}`
//!   placeholders, or it is ignored in favour of the built-in one)
//! - `--pwa`, `--threads`, `--no-simd`, `--no-preloader`
//! - `--print-rustflags` print the `RUSTFLAGS` this config needs and exit,
//!   which is how the script configures the build *before* anything is compiled
//!
//! ⚠️ Deliberately does **not** shell out to the compiler. Mixing "decide the
//! flags" and "run the build" into one program is what made the old export
//! untestable: the flags are a pure function of the config
//! ([`gdplatform::web_export::rustflags`]) and the script that needs them runs
//! before this binary could exist.

use gdplatform::web::WebExportConfig;
use gdplatform::web_export::{
    layout, pwa_files, render_html_shell, res_path_for, rustflags, serve_hint,
};
use gdresource::pck::PckPacker;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut config = WebExportConfig::default();
    let mut project: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut bundle: Option<PathBuf> = None;
    let mut shell_path: Option<PathBuf> = None;
    let mut app_name = "Patina Game".to_string();
    let mut print_rustflags = false;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--project" => {
                project = Some(PathBuf::from(value(&args, i, arg)?));
                i += 2;
            }
            "--out" => {
                out = Some(PathBuf::from(value(&args, i, arg)?));
                i += 2;
            }
            "--bundle" => {
                bundle = Some(PathBuf::from(value(&args, i, arg)?));
                i += 2;
            }
            "--shell" => {
                shell_path = Some(PathBuf::from(value(&args, i, arg)?));
                i += 2;
            }
            "--app" => {
                app_name = value(&args, i, arg)?;
                i += 2;
            }
            "--pwa" => {
                config.pwa_enabled = true;
                i += 1;
            }
            "--threads" => {
                config.threads_enabled = true;
                i += 1;
            }
            "--no-simd" => {
                config.simd_enabled = false;
                i += 1;
            }
            "--no-preloader" => {
                config.show_preloader = false;
                i += 1;
            }
            "--print-rustflags" => {
                print_rustflags = true;
                i += 1;
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    // Printing the flags has to work before anything is built, so it is handled
    // before the arguments a write would need are validated.
    if print_rustflags {
        println!("{}", rustflags(&config));
        return Ok(());
    }

    let project = project.ok_or("--project is required")?;
    if !project.is_dir() {
        return Err(format!(
            "--project is not a directory: {}",
            project.display()
        ));
    }
    let out = out.unwrap_or_else(|| PathBuf::from(&config.output_dir));
    config.output_dir = out.to_string_lossy().into_owned();

    let plan = layout(&app_name, &config);
    fs::create_dir_all(&out).map_err(|e| format!("creating {}: {e}", out.display()))?;

    // --- the pack -----------------------------------------------------------
    let mut packer = PckPacker::new();
    let mut packed = 0usize;
    for file in walk(&project)? {
        let Some(res_path) = res_path_for(&project, &file, &config) else {
            continue;
        };
        let bytes = fs::read(&file).map_err(|e| format!("reading {}: {e}", file.display()))?;
        packer.add_file(res_path, bytes);
        packed += 1;
    }
    if packed == 0 {
        // A pack with nothing in it produces a page that loads and then cannot
        // find its main scene — worth refusing now rather than debugging in a
        // browser.
        return Err(format!(
            "no packable files under {} — every file was excluded (.godot/, \
             *.import, dot-files, or the output directory)",
            project.display()
        ));
    }
    let pck_bytes = packer.pack().map_err(|e| format!("packing: {e}"))?;
    write(&out.join(&plan.pck), &pck_bytes)?;

    // --- the document -------------------------------------------------------
    let custom_shell = match &shell_path {
        Some(path) => {
            Some(fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?)
        }
        None => None,
    };
    let html = render_html_shell(&app_name, &config, &plan, custom_shell.as_deref());
    write(&out.join(&plan.index_html), html.as_bytes())?;

    // --- the bundle ---------------------------------------------------------
    if let Some(bundle) = &bundle {
        for name in [&plan.js, &plan.wasm] {
            let from = bundle.join(name);
            if !from.is_file() {
                return Err(format!(
                    "missing bundle artifact {} — run crates/patina-web/build-web.sh first",
                    from.display()
                ));
            }
            let bytes = fs::read(&from).map_err(|e| format!("reading {}: {e}", from.display()))?;
            write(&out.join(name), &bytes)?;
        }
    } else {
        eprintln!(
            "warning: no --bundle given, so {} and {} were not copied; the page will not boot \
             without them",
            plan.js, plan.wasm
        );
    }

    // --- PWA ----------------------------------------------------------------
    for (name, contents) in pwa_files(&app_name, &config, &plan) {
        write(&out.join(&name), contents.as_bytes())?;
    }

    println!(
        "export layout: {} file(s) in {}",
        plan.all().len(),
        out.display()
    );
    for name in plan.all() {
        println!("  {name}");
    }
    println!("{} file(s) packed into {}", packed, plan.pck);
    println!("serve it with:\n  {}", serve_hint(&config));
    Ok(())
}

/// The value following `--flag`, or a message naming the flag that lacks one.
///
/// A free function rather than a closure: a closure capturing `i` would hold a
/// borrow across the match arms that assign to `i`.
fn value(args: &[String], i: usize, arg: &str) -> Result<String, String> {
    args.get(i + 1)
        .cloned()
        .ok_or_else(|| format!("{arg} needs a value"))
}

/// Every file under `root`, recursively.
///
/// Directories are filtered by [`res_path_for`] rather than here, so the
/// exclusion rules live in one tested place — except that descending into
/// `.godot/` or a previous export would mean reading thousands of files only to
/// discard them, so a directory whose name starts with `.` is skipped outright.
fn walk(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("reading {}: {e}", dir.display()))?;
            let path = entry.path();
            let is_dir = entry
                .file_type()
                .map_err(|e| format!("stat {}: {e}", path.display()))?
                .is_dir();
            if is_dir {
                let hidden = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with('.'));
                if !hidden {
                    stack.push(path);
                }
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(path, bytes).map_err(|e| format!("writing {}: {e}", path.display()))
}

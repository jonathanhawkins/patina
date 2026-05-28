//! DISABLED (references gdcore::crash_handler module which does not exist yet)
//! Re-enable by removing the cfg gate below once the missing
//! items land. Tracked as part of editor parity work.
#![cfg(any())]

//! pat-onl3x: Crash report generation on panic.
//!
//! Validates that the `crash_handler` module:
//! - Produces structured JSON crash reports with all required fields
//! - Writes reports to the configured directory (creating it if needed)
//! - Captures the engine state provider snapshot when registered
//! - Generates deterministic, parseable file names
//! - Actually fires on a real panic via `install_panic_hook` (subprocess test)

use gdcore::crash_handler::{
    install_panic_hook, set_engine_state_provider, write_crash_report, CrashReport,
};
use serde_json::{json, Value};
use std::path::PathBuf;

// ===========================================================================
// Helpers
// ===========================================================================

fn sample_report() -> CrashReport {
    CrashReport {
        unix_ts_secs: 1_700_000_000,
        unix_ts_nanos: 123_456_789,
        pid: 42,
        thread_name: "main".to_string(),
        panic_message: "index out of bounds".to_string(),
        location: Some("src/scene_tree.rs:42:5".to_string()),
        backtrace: "<backtrace>".to_string(),
        engine_state: Value::Null,
    }
}

// ===========================================================================
// 1. Structured JSON includes all fields
// ===========================================================================

#[test]
fn crash_report_json_has_all_required_fields() {
    let report = sample_report();
    let v = report.to_json();

    assert_eq!(v["unix_ts_secs"], json!(1_700_000_000u64));
    assert_eq!(v["unix_ts_nanos"], json!(123_456_789u64));
    assert_eq!(v["pid"], json!(42u64));
    assert_eq!(v["thread"], json!("main"));
    assert_eq!(v["panic_message"], json!("index out of bounds"));
    assert_eq!(v["location"], json!("src/scene_tree.rs:42:5"));
    assert_eq!(v["backtrace"], json!("<backtrace>"));
    assert_eq!(v["engine_state"], Value::Null);
}

#[test]
fn crash_report_json_location_null_when_absent() {
    let mut report = sample_report();
    report.location = None;
    let v = report.to_json();
    assert!(v["location"].is_null());
}

// ===========================================================================
// 2. File name generation
// ===========================================================================

#[test]
fn file_name_contains_timestamp_and_pid() {
    let report = sample_report();
    let name = report.file_name();
    assert_eq!(name, "crash-1700000000-123456789-42.json");
}

#[test]
fn file_name_zero_pads_small_nanos() {
    let mut report = sample_report();
    report.unix_ts_nanos = 7;
    assert_eq!(report.file_name(), "crash-1700000000-000000007-42.json");
}

#[test]
fn file_name_is_valid_path_component() {
    let report = sample_report();
    let name = report.file_name();
    assert!(!name.contains('/'));
    assert!(!name.contains('\\'));
    assert!(name.ends_with(".json"));
}

// ===========================================================================
// 3. Write crash report creates directory and valid JSON
// ===========================================================================

#[test]
fn write_creates_nested_dir_and_valid_json_file() {
    let tmp = tempfile::tempdir().unwrap();
    let nested = tmp.path().join("crashes/reports/nested");
    let report = sample_report();

    let path = write_crash_report(&nested, &report).unwrap();
    assert!(path.exists());
    assert_eq!(path.file_name().unwrap().to_str().unwrap(), report.file_name());

    let body = std::fs::read_to_string(&path).unwrap();
    let v: Value = serde_json::from_str(&body).expect("report must be valid JSON");
    assert_eq!(v["panic_message"], json!("index out of bounds"));
    assert_eq!(v["pid"], json!(42u64));
}

#[test]
fn write_overwrites_existing_report_same_name() {
    let tmp = tempfile::tempdir().unwrap();
    let report = sample_report();

    write_crash_report(tmp.path(), &report).unwrap();
    // Write again — should overwrite, not error.
    let path = write_crash_report(tmp.path(), &report).unwrap();
    assert!(path.exists());
}

// ===========================================================================
// 4. Engine state provider integration
// ===========================================================================

#[test]
fn engine_state_provider_captured_in_report() {
    let mut report = sample_report();
    report.engine_state = json!({
        "phase": "running",
        "fps": 60,
        "scene_count": 3
    });
    let v = report.to_json();
    assert_eq!(v["engine_state"]["phase"], json!("running"));
    assert_eq!(v["engine_state"]["fps"], json!(60));
    assert_eq!(v["engine_state"]["scene_count"], json!(3));
}

#[test]
fn engine_state_null_when_no_provider() {
    let report = sample_report();
    assert_eq!(report.engine_state, Value::Null);
    let v = report.to_json();
    assert!(v["engine_state"].is_null());
}

// ===========================================================================
// 5. Subprocess panic test — actually triggers install_panic_hook
// ===========================================================================

#[test]
fn install_panic_hook_writes_report_on_real_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let crash_dir = tmp.path().join("crash-reports");

    // Spawn a child process that installs the hook, sets an engine state
    // provider, and panics. The parent then checks for the report file.
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--ignored")
        .arg("--exact")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .arg("__panic_subprocess_helper")
        .env("PATINA_CRASH_DIR", crash_dir.to_str().unwrap())
        .env("RUST_BACKTRACE", "1")
        .output()
        .expect("failed to spawn subprocess");

    // The child should have exited non-zero (panic).
    assert!(
        !output.status.success(),
        "subprocess should panic and exit non-zero"
    );

    // The stderr should contain the crash report path.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("crash report written to"),
        "expected crash report confirmation in stderr, got: {stderr}"
    );

    // Exactly one JSON file should be in the crash dir.
    let entries: Vec<_> = std::fs::read_dir(&crash_dir)
        .expect("crash dir should exist")
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(entries.len(), 1, "expected exactly one crash report");

    let report_path = entries[0].path();
    assert!(report_path.extension().is_some_and(|e| e == "json"));

    let body = std::fs::read_to_string(&report_path).unwrap();
    let v: Value = serde_json::from_str(&body).expect("crash report must be valid JSON");

    // Validate the report fields.
    assert_eq!(v["panic_message"], json!("intentional test panic"));
    assert!(v["unix_ts_secs"].as_u64().unwrap() > 0);
    assert!(v["pid"].as_u64().unwrap() > 0);
    assert!(v["backtrace"].as_str().is_some());
    assert_eq!(v["engine_state"]["test_mode"], json!(true));
}

/// Hidden test entry point used by `install_panic_hook_writes_report_on_real_panic`.
/// Runs only when invoked with `--ignored`.
#[test]
#[ignore]
fn __panic_subprocess_helper() {
    let crash_dir = std::env::var("PATINA_CRASH_DIR").expect("PATINA_CRASH_DIR must be set");
    install_panic_hook(PathBuf::from(crash_dir));
    set_engine_state_provider(Some(Box::new(|| json!({"test_mode": true}))));
    panic!("intentional test panic");
}

// ===========================================================================
// 6. Edge cases
// ===========================================================================

#[test]
fn report_with_empty_message_roundtrips() {
    let mut report = sample_report();
    report.panic_message = String::new();
    let v = report.to_json();
    assert_eq!(v["panic_message"], json!(""));

    let tmp = tempfile::tempdir().unwrap();
    let path = write_crash_report(tmp.path(), &report).unwrap();
    let body = std::fs::read_to_string(&path).unwrap();
    let parsed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["panic_message"], json!(""));
}

#[test]
fn report_with_unicode_message() {
    let mut report = sample_report();
    report.panic_message = "panic: 💥 ñoño 日本語".to_string();
    let v = report.to_json();
    assert_eq!(v["panic_message"], json!("panic: 💥 ñoño 日本語"));

    let tmp = tempfile::tempdir().unwrap();
    let path = write_crash_report(tmp.path(), &report).unwrap();
    let body = std::fs::read_to_string(&path).unwrap();
    let parsed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["panic_message"], json!("panic: 💥 ñoño 日本語"));
}

#[test]
fn report_with_large_backtrace() {
    let mut report = sample_report();
    report.backtrace = "frame\n".repeat(1000);

    let tmp = tempfile::tempdir().unwrap();
    let path = write_crash_report(tmp.path(), &report).unwrap();
    let body = std::fs::read_to_string(&path).unwrap();
    let parsed: Value = serde_json::from_str(&body).unwrap();
    assert!(parsed["backtrace"].as_str().unwrap().contains("frame"));
}

#[test]
fn report_with_complex_engine_state() {
    let mut report = sample_report();
    report.engine_state = json!({
        "scene_tree": {
            "root": "Main",
            "children": ["Player", "Enemy", "UI"],
            "count": 3
        },
        "physics": {
            "bodies": 42,
            "contacts": 7
        },
        "resources_loaded": 128
    });

    let tmp = tempfile::tempdir().unwrap();
    let path = write_crash_report(tmp.path(), &report).unwrap();
    let body = std::fs::read_to_string(&path).unwrap();
    let parsed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["engine_state"]["scene_tree"]["count"], json!(3));
    assert_eq!(parsed["engine_state"]["physics"]["bodies"], json!(42));
}

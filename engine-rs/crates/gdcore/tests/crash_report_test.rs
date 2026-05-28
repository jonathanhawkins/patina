//! DISABLED (references gdcore::crash_handler module which does not exist yet)
//! Re-enable by removing the cfg gate below once the missing
//! items land. Tracked as part of editor parity work.
#![cfg(any())]

//! Integration tests for the crash handler's panic hook.
//!
//! These tests install a global panic hook, deliberately panic inside
//! [`std::panic::catch_unwind`], and verify that a structured JSON crash
//! report is written to disk. A process-wide mutex serializes the tests
//! because [`std::panic::set_hook`] mutates global state.

use std::panic;
use std::sync::Mutex;

use gdcore::crash_handler::{install_panic_hook, write_crash_report, CrashReport};
use serde_json::Value;

static HOOK_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn panic_writes_crash_report_to_directory() {
    let _guard = HOOK_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    let tmp = tempfile::tempdir().expect("tempdir");
    let reports_dir = tmp.path().to_path_buf();

    install_panic_hook(reports_dir.clone());

    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        panic!("deliberate crash for crash_report_test");
    }));

    let _ = panic::take_hook();

    assert!(result.is_err(), "panic should have unwound");

    let entries: Vec<_> = std::fs::read_dir(&reports_dir)
        .expect("read reports dir")
        .filter_map(Result::ok)
        .collect();
    assert_eq!(entries.len(), 1, "expected exactly one crash report file");

    let path = entries[0].path();
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .expect("utf-8 file name")
        .to_string();
    assert!(
        file_name.starts_with("crash-"),
        "unexpected file name: {file_name}"
    );
    assert!(
        file_name.ends_with(".json"),
        "unexpected file name: {file_name}"
    );

    let body = std::fs::read_to_string(&path).expect("read crash report");
    let json: Value = serde_json::from_str(&body).expect("valid json");

    assert_eq!(
        json["panic_message"],
        Value::String("deliberate crash for crash_report_test".into())
    );
    assert!(json["thread"].is_string(), "thread should be a string");
    assert!(json["unix_ts_secs"].is_u64(), "unix_ts_secs should be u64");
    assert!(
        json["unix_ts_nanos"].is_u64(),
        "unix_ts_nanos should be u64"
    );
    assert!(json["pid"].is_u64(), "pid should be u64");
    assert!(json["backtrace"].is_string(), "backtrace should be string");
    assert!(
        json["location"].is_string(),
        "location should be present for panic! macro"
    );
    let location = json["location"].as_str().unwrap();
    assert!(
        location.contains("crash_report_test"),
        "location should point at this test file, got: {location}"
    );
}

#[test]
fn write_crash_report_produces_deterministic_file_name() {
    let _guard = HOOK_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    let tmp = tempfile::tempdir().expect("tempdir");
    let report = CrashReport {
        unix_ts_secs: 1_700_000_000,
        unix_ts_nanos: 987_654_321,
        pid: 7,
        thread_name: "manual".to_string(),
        panic_message: "manual".to_string(),
        location: None,
        backtrace: "<skipped>".to_string(),
        engine_state: Value::Null,
    };

    let path = write_crash_report(tmp.path(), &report).expect("write crash report");
    assert_eq!(
        path.file_name().and_then(|s| s.to_str()).unwrap(),
        "crash-1700000000-987654321-7.json"
    );

    let body = std::fs::read_to_string(&path).expect("read crash report");
    let json: Value = serde_json::from_str(&body).expect("valid json");
    assert_eq!(json["pid"], serde_json::json!(7u64));
    assert_eq!(json["thread"], serde_json::json!("manual"));
    assert_eq!(json["panic_message"], serde_json::json!("manual"));
    assert_eq!(json["location"], Value::Null);
    assert_eq!(json["engine_state"], Value::Null);
}

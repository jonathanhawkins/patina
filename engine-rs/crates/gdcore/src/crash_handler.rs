//! Structured panic / crash handler.
//!
//! Installs a global [`std::panic::set_hook`] that captures panic information
//! — thread name, source location, payload message, backtrace, and an optional
//! engine state snapshot — and writes the result as a JSON file into a
//! configurable reports directory.
//!
//! # Example
//!
//! ```no_run
//! use std::path::PathBuf;
//!
//! gdcore::crash_handler::install_panic_hook(PathBuf::from("/tmp/patina-crashes"));
//! ```
//!
//! A process may also register a callback that produces a richer engine state
//! value at panic time via [`set_engine_state_provider`].

use std::backtrace::Backtrace;
use std::fs;
use std::io;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

/// Callback used to snapshot engine state at panic time.
///
/// The closure runs inside the panic hook, so implementations must be cheap,
/// allocation-light, and must not themselves panic (that would abort the
/// process via double-panic).
pub type EngineStateProvider = Box<dyn Fn() -> Value + Send + Sync + 'static>;

fn engine_state_provider_slot() -> &'static Mutex<Option<EngineStateProvider>> {
    static SLOT: OnceLock<Mutex<Option<EngineStateProvider>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Register (or clear, with `None`) the global engine state provider.
pub fn set_engine_state_provider(provider: Option<EngineStateProvider>) {
    if let Ok(mut slot) = engine_state_provider_slot().lock() {
        *slot = provider;
    }
}

fn snapshot_engine_state() -> Value {
    if let Ok(slot) = engine_state_provider_slot().lock() {
        if let Some(provider) = slot.as_ref() {
            return provider();
        }
    }
    Value::Null
}

/// Structured crash report captured at panic time.
#[derive(Debug, Clone)]
pub struct CrashReport {
    /// Whole-second component of the UNIX timestamp at panic time.
    pub unix_ts_secs: u64,
    /// Sub-second component in nanoseconds.
    pub unix_ts_nanos: u32,
    /// Process ID of the crashing process.
    pub pid: u32,
    /// Name of the thread that panicked (`"unknown"` if none was provided).
    pub thread_name: String,
    /// Panic payload, downcast to `&str` or `String` where possible.
    pub panic_message: String,
    /// `file:line:column` location of the `panic!` site, when available.
    pub location: Option<String>,
    /// Captured backtrace as produced by [`Backtrace::force_capture`].
    pub backtrace: String,
    /// Engine state snapshot; [`Value::Null`] when no provider is registered.
    pub engine_state: Value,
}

impl CrashReport {
    /// Serialize this report into a [`serde_json::Value`].
    pub fn to_json(&self) -> Value {
        json!({
            "unix_ts_secs": self.unix_ts_secs,
            "unix_ts_nanos": self.unix_ts_nanos,
            "pid": self.pid,
            "thread": self.thread_name,
            "panic_message": self.panic_message,
            "location": self.location,
            "backtrace": self.backtrace,
            "engine_state": self.engine_state,
        })
    }

    /// Deterministic file name used by [`write_crash_report`].
    pub fn file_name(&self) -> String {
        format!(
            "crash-{}-{:09}-{}.json",
            self.unix_ts_secs, self.unix_ts_nanos, self.pid
        )
    }
}

fn panic_message_of(info: &PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

fn build_crash_report(info: &PanicHookInfo<'_>) -> CrashReport {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let thread_name = thread::current().name().unwrap_or("unknown").to_string();
    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
    let backtrace = Backtrace::force_capture().to_string();

    CrashReport {
        unix_ts_secs: now.as_secs(),
        unix_ts_nanos: now.subsec_nanos(),
        pid: process::id(),
        thread_name,
        panic_message: panic_message_of(info),
        location,
        backtrace,
        engine_state: snapshot_engine_state(),
    }
}

/// Write a crash report to `<reports_dir>/<report.file_name()>`.
///
/// Creates `reports_dir` (and any missing parents) if it does not already
/// exist. Returns the full path written on success.
pub fn write_crash_report(reports_dir: &Path, report: &CrashReport) -> io::Result<PathBuf> {
    fs::create_dir_all(reports_dir)?;
    let path = reports_dir.join(report.file_name());
    let body = serde_json::to_string_pretty(&report.to_json()).map_err(io::Error::other)?;
    fs::write(&path, body)?;
    Ok(path)
}

/// Install a global panic hook that writes structured crash reports under
/// `reports_dir`.
///
/// Replaces any previously installed hook. Report-write failures are logged
/// via `eprintln!` but never re-panic — the hook is best-effort, and a panic
/// inside the hook would double-panic and abort the process.
pub fn install_panic_hook(reports_dir: PathBuf) {
    std::panic::set_hook(Box::new(move |info| {
        let report = build_crash_report(info);
        match write_crash_report(&reports_dir, &report) {
            Ok(path) => eprintln!("patina: crash report written to {}", path.display()),
            Err(err) => eprintln!(
                "patina: failed to write crash report to {}: {err}",
                reports_dir.display()
            ),
        }
        eprintln!(
            "patina: panic in thread '{}': {}",
            report.thread_name, report.panic_message
        );
        if let Some(loc) = &report.location {
            eprintln!("patina: at {loc}");
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_report() -> CrashReport {
        CrashReport {
            unix_ts_secs: 1_700_000_000,
            unix_ts_nanos: 123_456_789,
            pid: 42,
            thread_name: "test-thread".to_string(),
            panic_message: "boom".to_string(),
            location: Some("file.rs:10:5".to_string()),
            backtrace: "<disabled>".to_string(),
            engine_state: Value::Null,
        }
    }

    #[test]
    fn to_json_includes_all_fields() {
        let v = sample_report().to_json();
        assert_eq!(v["unix_ts_secs"], json!(1_700_000_000u64));
        assert_eq!(v["unix_ts_nanos"], json!(123_456_789u64));
        assert_eq!(v["pid"], json!(42u64));
        assert_eq!(v["thread"], json!("test-thread"));
        assert_eq!(v["panic_message"], json!("boom"));
        assert_eq!(v["location"], json!("file.rs:10:5"));
        assert_eq!(v["backtrace"], json!("<disabled>"));
        assert_eq!(v["engine_state"], json!(null));
    }

    #[test]
    fn file_name_pads_nanos_to_nine_digits() {
        let mut report = sample_report();
        report.unix_ts_nanos = 42;
        assert_eq!(report.file_name(), "crash-1700000000-000000042-42.json");
    }

    #[test]
    fn file_name_uses_full_nanos_when_large() {
        assert_eq!(
            sample_report().file_name(),
            "crash-1700000000-123456789-42.json"
        );
    }

    #[test]
    fn write_crash_report_creates_directory_and_file() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("does/not/exist/yet");
        let path = write_crash_report(&nested, &sample_report()).unwrap();
        assert!(path.exists());
        let body = std::fs::read_to_string(&path).unwrap();
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["panic_message"], json!("boom"));
        assert_eq!(v["pid"], json!(42u64));
    }

    #[test]
    fn engine_state_provider_roundtrip() {
        set_engine_state_provider(Some(Box::new(|| json!({"phase": "running"}))));
        assert_eq!(snapshot_engine_state(), json!({"phase": "running"}));
        set_engine_state_provider(None);
        assert_eq!(snapshot_engine_state(), Value::Null);
    }
}

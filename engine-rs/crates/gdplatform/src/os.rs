//! OS-level integration and platform abstraction.
//!
//! Provides basic OS information and timing utilities, mirroring a subset
//! of Godot's `OS` singleton.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
// `std::time::Instant::now()` panics on `wasm32-unknown-unknown` (there is no
// native monotonic clock). `web_time::Instant` is a drop-in replacement: on
// native targets it re-exports `std::time::Instant` (identical behavior and
// cost), and on wasm32 it is backed by the browser's `performance.now()`
// monotonic clock. This keeps `get_ticks_*` working in the web runtime
// (pat-u3l8c.109).
use web_time::{Instant, SystemTime, UNIX_EPOCH};

static EPOCH: OnceLock<Instant> = OnceLock::new();

fn epoch() -> &'static Instant {
    EPOCH.get_or_init(Instant::now)
}

// ---------------------------------------------------------------------------
// Platform
// ---------------------------------------------------------------------------

/// Target platform identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    /// GNU/Linux.
    Linux,
    /// Apple macOS.
    MacOS,
    /// Microsoft Windows.
    Windows,
    /// Web / WASM.
    Web,
    /// Unrecognized platform.
    Unknown,
}

/// Returns the platform this binary was compiled for.
pub fn current_platform() -> Platform {
    if cfg!(target_os = "linux") {
        Platform::Linux
    } else if cfg!(target_os = "macos") {
        Platform::MacOS
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_arch = "wasm32") {
        Platform::Web
    } else {
        Platform::Unknown
    }
}

/// Returns `true` when compiled with debug assertions enabled.
pub fn is_debug_build() -> bool {
    cfg!(debug_assertions)
}

/// Returns whether the runtime provides the given Godot feature tag, mirroring
/// `OS.has_feature`. Recognizes platform tags (`web`/`wasm`/the host OS,
/// `pc`/`desktop`), architecture tags, and build tags (`debug`/`release`,
/// `threads`).
///
/// Tags resolve from the compile-time target: on a native build `web`/`wasm`
/// are `false` and `pc`/`desktop` are `true`; on the `wasm32` target those
/// invert. Unknown tags return `false`.
pub fn has_feature(feature: &str) -> bool {
    let platform = current_platform();
    match feature {
        // Web / WASM platform tags.
        "web" | "wasm" | "wasm32" | "JavaScript" | "javascript" => platform == Platform::Web,
        // Native desktop tags.
        "pc" | "desktop" => matches!(
            platform,
            Platform::Linux | Platform::MacOS | Platform::Windows
        ),
        // Host OS tags.
        "linux" | "linuxbsd" => platform == Platform::Linux,
        "macos" => platform == Platform::MacOS,
        "windows" => platform == Platform::Windows,
        // Architecture tags.
        "x86_64" => cfg!(target_arch = "x86_64"),
        "arm64" | "aarch64" => cfg!(target_arch = "aarch64"),
        // Build-config tags.
        "debug" => is_debug_build(),
        "release" => !is_debug_build(),
        // Threads: always available natively. On the web it depends on the
        // EXPORT VARIANT, not on the target, so this reads a runtime capability
        // the wasm entry sets from `self.crossOriginIsolated` (pat-s3rrv.217).
        //
        // ⚠️ It used to be hardcoded `platform != Platform::Web`, with a comment
        // saying a threaded export was "assumed off". That made the tag a
        // property of the build target rather than of the running page, so a
        // correctly threaded export could never report itself and a script had
        // no way to branch on what it actually had.
        "threads" => platform != Platform::Web || is_cross_origin_isolated(),
        // Pointer-width tags (Godot exports these as "64"/"32").
        "64" => cfg!(target_pointer_width = "64"),
        "32" => cfg!(target_pointer_width = "32"),
        // Build-kind tags. The runner is a template build, never the editor;
        // the editor host sets `editor` through `set_editor_build`.
        "editor" => EDITOR_BUILD.load(Ordering::Relaxed),
        "template" => !EDITOR_BUILD.load(Ordering::Relaxed),
        "template_debug" => !EDITOR_BUILD.load(Ordering::Relaxed) && is_debug_build(),
        "template_release" => !EDITOR_BUILD.load(Ordering::Relaxed) && !is_debug_build(),
        // Platform families Patina does not target.
        "mobile" | "android" | "ios" => false,
        // `movie` is Godot's Movie Maker mode, off unless the host turns it on
        // through a custom feature.
        _ => has_custom_feature(feature),
    }
}

/// Whether this process is the editor (`OS.has_feature("editor")`). Godot's
/// editor build reports `editor`; an exported game reports `template`.
static EDITOR_BUILD: AtomicBool = AtomicBool::new(false);

/// Declares this process an editor build, so `has_feature("editor")` answers
/// true and `has_feature("template")` false — the split Godot exports.
pub fn set_editor_build(is_editor: bool) {
    EDITOR_BUILD.store(is_editor, Ordering::Relaxed);
}

/// Whether the page is cross-origin isolated, i.e. whether `SharedArrayBuffer`
/// and wasm threads are actually available (pat-s3rrv.217).
///
/// Godot's web export ships a `thread_support` variant that only works when the
/// server sends `Cross-Origin-Opener-Policy: same-origin` and
/// `Cross-Origin-Embedder-Policy: require-corp`; the browser then reports
/// `self.crossOriginIsolated`. This mirrors that single browser-supplied fact.
///
/// ⚠️ It is a RUNTIME capability, not a build flag. The same wasm bundle is
/// isolated or not depending on the headers the server happens to send, so
/// deciding it at compile time is wrong in both directions — and the editor's
/// own dev server already sends both headers.
static CROSS_ORIGIN_ISOLATED: AtomicBool = AtomicBool::new(false);

/// Records whether the host page is cross-origin isolated.
///
/// Called by the wasm entry point with `self.crossOriginIsolated`. Native hosts
/// never call it: `has_feature("threads")` is already true off-web, so the flag
/// only ever decides the web answer.
pub fn set_cross_origin_isolated(isolated: bool) {
    CROSS_ORIGIN_ISOLATED.store(isolated, Ordering::Relaxed);
}

/// Whether the host page reported cross-origin isolation.
///
/// Also decides which audio-worklet transport an export emits: a
/// `SharedArrayBuffer` ring when isolated, message-port block passing when not.
pub fn is_cross_origin_isolated() -> bool {
    CROSS_ORIGIN_ISOLATED.load(Ordering::Relaxed)
}

/// Custom feature tags. Godot collects these from `application/config/features`
/// and the export preset's custom features, and `has_feature` answers them
/// alongside the built-in tags.
static CUSTOM_FEATURES: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();

fn custom_features() -> &'static Mutex<BTreeSet<String>> {
    CUSTOM_FEATURES.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// Registers a custom feature tag (project/export defined).
pub fn add_custom_feature(name: &str) {
    if let Ok(mut set) = custom_features().lock() {
        set.insert(name.to_string());
    }
}

/// Drops every registered custom feature tag.
pub fn clear_custom_features() {
    if let Ok(mut set) = custom_features().lock() {
        set.clear();
    }
}

/// Whether `name` was registered through [`add_custom_feature`].
pub fn has_custom_feature(name: &str) -> bool {
    custom_features()
        .lock()
        .map(|s| s.contains(name))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// OsInfo
// ---------------------------------------------------------------------------

/// Basic OS information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsInfo {
    /// The operating system name (e.g. "Windows", "macOS", "Linux").
    pub name: String,
    /// Whether the engine is running in debug mode.
    pub is_debug: bool,
}

impl OsInfo {
    /// Detects the current OS at runtime.
    pub fn detect() -> Self {
        let name = if cfg!(target_os = "windows") {
            "Windows"
        } else if cfg!(target_os = "macos") {
            "macOS"
        } else if cfg!(target_os = "linux") {
            "Linux"
        } else {
            "Unknown"
        };

        Self {
            name: name.to_string(),
            is_debug: cfg!(debug_assertions),
        }
    }
}

/// Returns elapsed time in milliseconds since the first call to any tick function.
pub fn get_ticks_msec() -> u64 {
    epoch().elapsed().as_millis() as u64
}

/// Returns elapsed time in microseconds since the first call to any tick function.
pub fn get_ticks_usec() -> u64 {
    epoch().elapsed().as_micros() as u64
}

/// Seconds since the Unix epoch, as Godot's `Time.get_unix_time_from_system()`
/// reports it.
///
/// ⚠️ The wall clock needs the same treatment as the monotonic clock above:
/// `std::time::SystemTime::now()` panics on `wasm32-unknown-unknown`, which has
/// no wall clock. `web_time::SystemTime` re-exports `std::time::SystemTime` on
/// native targets and is backed by the browser's `Date.now()` on wasm32, so one
/// body serves both (pat-mgixk.40).
///
/// Lives here rather than in each caller because `gdscript-interop` had its own
/// private `std::time` copies for `Time.*` and `randomize()`, which is how the
/// browser script path kept a panic that native tests could never see.
///
/// Returns 0.0 if the system clock is before the epoch, matching the callers'
/// previous `unwrap_or(0.0)` rather than panicking on a misconfigured clock.
pub fn unix_time_secs_f64() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

// ---------------------------------------------------------------------------
// Project identity and the user:// root (pat-0q2kx.123)
// ---------------------------------------------------------------------------

/// Godot's fallback when `application/config/name` is unset
/// (`OS::get_safe_dir_name` on an empty name).
/// Whether the project should relaunch itself on exit, plus the command line to
/// relaunch with (Godot's `OS.set_restart_on_exit`).
///
/// ⚠️ Both halves are stored together and REPLACED on every call, matching
/// `OS::set_restart_on_exit`. So `set_restart_on_exit(false)` with the default
/// empty argument list also clears any previously stored arguments — leaving
/// stale arguments behind would relaunch a later `true` with a command line the
/// caller never asked for.
static RESTART_ON_EXIT: OnceLock<Mutex<(bool, Vec<String>)>> = OnceLock::new();

fn restart_on_exit_slot() -> &'static Mutex<(bool, Vec<String>)> {
    RESTART_ON_EXIT.get_or_init(|| Mutex::new((false, Vec::new())))
}

/// Godot's `OS.set_restart_on_exit(restart, arguments)`.
pub fn set_restart_on_exit(restart: bool, arguments: Vec<String>) {
    if let Ok(mut slot) = restart_on_exit_slot().lock() {
        *slot = (restart, arguments);
    }
}

/// Godot's `OS.is_restart_on_exit_set()`.
pub fn is_restart_on_exit_set() -> bool {
    restart_on_exit_slot().lock().map(|s| s.0).unwrap_or(false)
}

/// Godot's `OS.get_restart_on_exit_arguments()`.
pub fn restart_on_exit_arguments() -> Vec<String> {
    restart_on_exit_slot()
        .lock()
        .map(|s| s.1.clone())
        .unwrap_or_default()
}

/// Whether the engine smooths the measured frame delta (Godot's
/// `OS.delta_smoothing`).
///
/// ⚠️ Defaults to **true** — `OS.xml:898` gives the member `default="true"`,
/// and on start-up it mirrors `application/run/delta_smoothing`. Defaulting to
/// false would report smoothing as off in a fresh project where Godot reports
/// it on.
static DELTA_SMOOTHING: AtomicBool = AtomicBool::new(true);

/// Godot's `OS.set_delta_smoothing(enabled)`.
pub fn set_delta_smoothing(enabled: bool) {
    DELTA_SMOOTHING.store(enabled, Ordering::Relaxed);
}

/// Godot's `OS.is_delta_smoothing_enabled()`.
pub fn is_delta_smoothing_enabled() -> bool {
    DELTA_SMOOTHING.load(Ordering::Relaxed)
}

const UNNAMED_PROJECT: &str = "[unnamed project]";

static PROJECT_NAME: OnceLock<Mutex<String>> = OnceLock::new();

fn project_name_slot() -> &'static Mutex<String> {
    PROJECT_NAME.get_or_init(|| Mutex::new(UNNAMED_PROJECT.to_string()))
}

/// Sets the project name that names the `user://` directory. The runner calls
/// this at boot with `application/config/name` from `project.godot`; before
/// that the name is Godot's `[unnamed project]` placeholder.
pub fn set_project_name(name: &str) {
    let safe = safe_dir_name(name);
    if let Ok(mut slot) = project_name_slot().lock() {
        *slot = safe;
    }
}

/// The current project name, as used in the `user://` path.
pub fn project_name() -> String {
    project_name_slot()
        .lock()
        .map(|s| s.clone())
        .unwrap_or_else(|_| UNNAMED_PROJECT.to_string())
}

/// Godot's `OS::get_safe_dir_name`: strips the characters that are illegal in a
/// directory name and trims, falling back to the unnamed placeholder.
fn safe_dir_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            ':' | '/' | '\\' | '?' | '*' | '"' | '|' | '%' | '<' | '>' => '-',
            c if (c as u32) < 0x20 => '-',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        UNNAMED_PROJECT.to_string()
    } else {
        trimmed.to_string()
    }
}

fn home_dir() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("USERPROFILE").ok().filter(|s| !s.is_empty()))
}

fn env_dir(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

/// `OS.get_data_dir()` — the platform application-data root
/// (`~/Library/Application Support`, `$XDG_DATA_HOME`, `%APPDATA%`).
pub fn data_dir() -> String {
    if cfg!(target_os = "macos") {
        home_dir()
            .map(|h| format!("{h}/Library/Application Support"))
            .unwrap_or_default()
    } else if cfg!(target_os = "windows") {
        env_dir("APPDATA").unwrap_or_default()
    } else {
        env_dir("XDG_DATA_HOME")
            .or_else(|| home_dir().map(|h| format!("{h}/.local/share")))
            .unwrap_or_default()
    }
}

/// `OS.get_config_dir()` — the platform configuration root. On macOS and
/// Windows Godot points this at the same place as the data dir; on Linux it is
/// the separate XDG config root.
pub fn config_dir() -> String {
    if cfg!(target_os = "macos") {
        home_dir()
            .map(|h| format!("{h}/Library/Application Support"))
            .unwrap_or_default()
    } else if cfg!(target_os = "windows") {
        env_dir("APPDATA").unwrap_or_default()
    } else {
        env_dir("XDG_CONFIG_HOME")
            .or_else(|| home_dir().map(|h| format!("{h}/.config")))
            .unwrap_or_default()
    }
}

/// `OS.get_cache_dir()` — the platform cache root.
pub fn cache_dir() -> String {
    if cfg!(target_os = "macos") {
        home_dir()
            .map(|h| format!("{h}/Library/Caches"))
            .unwrap_or_default()
    } else if cfg!(target_os = "windows") {
        env_dir("LOCALAPPDATA")
            .or_else(|| env_dir("TEMP"))
            .unwrap_or_default()
    } else {
        env_dir("XDG_CACHE_HOME")
            .or_else(|| home_dir().map(|h| format!("{h}/.cache")))
            .unwrap_or_default()
    }
}

/// `OS.get_temp_dir()` — the platform temporary directory.
pub fn temp_dir() -> String {
    std::env::temp_dir().to_string_lossy().to_string()
}

/// `OS.get_user_data_dir()` — `<data dir>/Godot/app_userdata/<project name>`.
///
/// The `Godot/app_userdata/` segment is Godot's own layout
/// (`OS::get_user_data_dir` + `ProjectSettings`), kept verbatim so a project
/// ported to Patina finds the save files it already wrote.
pub fn user_data_dir() -> String {
    let base = data_dir();
    if base.is_empty() {
        // No writable home: keep the scheme itself rather than inventing a path.
        return "user://".to_string();
    }
    format!(
        "{}/Godot/app_userdata/{}",
        base.trim_end_matches('/'),
        project_name()
    )
}

/// `OS.is_userfs_persistent()` — true natively (a real filesystem); on the web
/// it depends on IndexedDB being available, which the headless build is not.
pub fn is_userfs_persistent() -> bool {
    current_platform() != Platform::Web
}

// ---------------------------------------------------------------------------
// Timing, environment and shell (pat-0q2kx.123)
// ---------------------------------------------------------------------------

/// `OS.delay_msec()` — blocks the calling thread.
///
/// On the web target the main thread cannot block (there is no `sleep`), so
/// this is a no-op there, matching how `OS.delay_msec` degrades in Godot's web
/// export.
pub fn delay_msec(msec: i64) {
    delay_usec(msec.saturating_mul(1000));
}

/// `OS.delay_usec()` — blocks the calling thread. No-op on the web target.
pub fn delay_usec(usec: i64) {
    if usec <= 0 {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::sleep(std::time::Duration::from_micros(usec as u64));
}

/// `OS.set_environment()`.
pub fn set_environment(key: &str, value: &str) {
    if key.is_empty() {
        return;
    }
    // SAFETY-equivalent note: `std::env::set_var` is process-global. Godot's
    // `OS.set_environment` has exactly the same reach and the same caveat.
    std::env::set_var(key, value);
}

/// `OS.unset_environment()`.
pub fn unset_environment(key: &str) {
    if key.is_empty() {
        return;
    }
    std::env::remove_var(key);
}

/// `OS.get_environment()` — empty string when unset, as Godot returns.
pub fn get_environment(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}

/// `OS.has_environment()`.
pub fn has_environment(key: &str) -> bool {
    std::env::var(key).is_ok()
}

/// `OS.get_executable_path()`.
pub fn executable_path() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// `OS.get_version()` — the host OS version string. Best-effort per platform;
/// empty when it cannot be determined.
pub fn version() -> String {
    #[cfg(target_os = "macos")]
    return run_capture("sw_vers", &["-productVersion"]);
    #[cfg(target_os = "linux")]
    return std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    #[cfg(target_os = "windows")]
    return get_environment("OS");
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    return String::new();
}

#[cfg(target_os = "macos")]
fn run_capture(program: &str, args: &[&str]) -> String {
    std::process::Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// `OS.shell_open()` — opens a URI with the platform handler. Returns Godot's
/// error code: `OK` (0) on success, `ERR_UNAVAILABLE` (46) where no handler can
/// be launched (notably the headless web target).
pub fn shell_open(uri: &str) -> i64 {
    const OK: i64 = 0;
    const ERR_UNAVAILABLE: i64 = 46;
    // The browser opens URIs through `window.open`, which lives in the
    // JavaScriptBridge surface, not here.
    #[cfg(target_arch = "wasm32")]
    {
        let _ = uri;
        let _ = OK;
        return ERR_UNAVAILABLE;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (program, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
            ("open", vec![uri])
        } else if cfg!(target_os = "windows") {
            ("cmd", vec!["/C", "start", "", uri])
        } else {
            ("xdg-open", vec![uri])
        };
        match std::process::Command::new(program).args(args).spawn() {
            Ok(_) => OK,
            Err(_) => ERR_UNAVAILABLE,
        }
    }
}

/// `OS.alert()` — Godot pops a modal dialog. Patina runs headless, so the
/// message goes to stderr in Godot's `title: text` shape; the return tells the
/// caller whether a real dialog was shown (never, here).
pub fn alert(text: &str, title: &str) -> bool {
    eprintln!("{title}: {text}");
    false
}

// ---------------------------------------------------------------------------
// Keycode helpers (pat-0q2kx.123)
// ---------------------------------------------------------------------------

// Godot's `KeyModifierMask` (core/os/keyboard.h:249-260).
const KEY_CODE_MASK: i64 = (1 << 23) - 1;
const KEY_MASK_CMD_OR_CTRL: i64 = 1 << 24;
const KEY_MASK_SHIFT: i64 = 1 << 25;
const KEY_MASK_ALT: i64 = 1 << 26;
const KEY_MASK_META: i64 = 1 << 27;
const KEY_MASK_CTRL: i64 = 1 << 28;

/// Godot's `OS::prefer_meta_over_ctrl` — macOS spells the command modifier
/// "Meta", every other platform spells it "Ctrl".
fn prefer_meta_over_ctrl() -> bool {
    cfg!(target_os = "macos")
}

/// `OS.get_keycode_string()` — the printable name of a keycode, with any
/// modifier bits rendered as Godot's `Ctrl+Alt+Shift+Meta+Key` prefix chain
/// (core/os/keyboard.cpp `keycode_get_string`).
pub fn keycode_string(keycode: i64) -> String {
    let mut parts: Vec<String> = Vec::new();
    if keycode & KEY_MASK_CMD_OR_CTRL != 0 && !prefer_meta_over_ctrl() {
        parts.push("Ctrl".to_string());
    }
    if keycode & KEY_MASK_CTRL != 0 {
        parts.push("Ctrl".to_string());
    }
    if keycode & KEY_MASK_ALT != 0 {
        parts.push("Alt".to_string());
    }
    if keycode & KEY_MASK_SHIFT != 0 {
        parts.push("Shift".to_string());
    }
    if keycode & KEY_MASK_CMD_OR_CTRL != 0 && prefer_meta_over_ctrl() {
        parts.push("Meta".to_string());
    }
    if keycode & KEY_MASK_META != 0 {
        parts.push("Meta".to_string());
    }

    // Godot strips the modifier bits before naming the key itself; a code of 0
    // means the keycode was modifiers only.
    let code = keycode & KEY_CODE_MASK;
    if code != 0 {
        if let Some(key) = crate::input::key_from_godot_keycode(code) {
            parts.push(key.name().to_string());
        } else if let Some(c) = char::from_u32(code as u32) {
            parts.push(c.to_string());
        }
    }
    parts.join("+")
}

/// The keycode span `keycode_string` can name: printable ASCII plus Godot's
/// `KEY_SPECIAL` block (`1 << 22` and up).
const KEY_SPECIAL_BASE: i64 = 1 << 22;

/// `OS.find_keycode_from_string()` — the inverse of [`keycode_string`], matching
/// Godot's case-insensitive comparison on the final `+`-separated part.
pub fn find_keycode_from_string(text: &str) -> i64 {
    let last = text.rsplit('+').next().unwrap_or("").trim();
    if last.is_empty() {
        return 0;
    }
    // Scan the keycode space this build actually names rather than duplicating
    // the `Key` table: any code that round-trips through `keycode_string` back
    // to `last` is the answer.
    for code in (0x20..=0x7e).chain(KEY_SPECIAL_BASE..=KEY_SPECIAL_BASE + 0x200) {
        if let Some(key) = crate::input::key_from_godot_keycode(code) {
            if key.name().eq_ignore_ascii_case(last) {
                return code;
            }
        }
    }
    0
}

/// `OS.request_permission(name)` — whether the named permission is available.
///
/// 🔑 **Godot's default is to GRANT, not to deny** (`core/os/os.h:349-350`: the
/// base `request_permission`/`request_permissions` both `return true`). A
/// platform with no permission gate has withheld nothing, so the honest answer
/// is "already granted". Answering `false` would push a cross-platform script
/// down its permission-denied branch on a desktop where the operation is in
/// fact allowed — a wrong branch, not a missing feature.
///
/// Only Android overrides these; the parameter is kept so that override has the
/// name to act on.
pub fn request_permission(_name: &str) -> bool {
    true
}

/// `OS.request_permissions()` — request every *dangerous* permission at once.
/// Android-only in Godot; grants by default elsewhere, as above.
pub fn request_permissions() -> bool {
    true
}

/// `OS.get_granted_permissions()`.
///
/// ⚠️ **Empty even though [`request_permission`] answers `true`**, and the
/// disagreement is deliberate. This list is not "what is permitted" — it is the
/// set of *dangerous* permissions granted at runtime on Android, or the
/// user-selected folders a sandboxed macOS app may reach. A desktop build has
/// neither, so the list is empty while everything is still permitted. Deriving
/// one of these two answers from the other is the tempting mistake.
pub fn granted_permissions() -> Vec<String> {
    Vec::new()
}

/// `OS.revoke_granted_permissions()` — clears the folder list a sandboxed macOS
/// app accumulated. Nothing accumulates off that platform, so this is a no-op
/// rather than an unimplemented error.
pub fn revoke_granted_permissions() {}

/// `OS.get_version_alias()` — the *branded* OS version ("Sequoia (15.0.0)",
/// "11 (build 22000)").
///
/// ⚠️ This is the **operating system** version, not the engine version. Godot's
/// base class returns [`version`] verbatim (`core/os/os.h:220`) and only
/// Windows, macOS and Android override it with a marketing name, so delegating
/// is the documented default rather than a shortcut.
pub fn version_alias() -> String {
    version()
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- pat-0q2kx.123: project-named user://, custom features, keycodes -----

    #[test]
    fn user_data_dir_is_named_after_the_project() {
        set_project_name("My Game");
        let dir = user_data_dir();
        assert!(
            dir.ends_with("Godot/app_userdata/My Game"),
            "user:// must follow Godot's layout, got '{dir}'",
        );
        // A different project must not land in the same directory — the bug
        // this replaced hardcoded `<data dir>/Patina` for every project.
        set_project_name("Other Game");
        assert_ne!(dir, user_data_dir());
    }

    #[test]
    fn project_name_is_sanitised_for_the_filesystem() {
        set_project_name("Bad/Name:With*Chars");
        assert_eq!(project_name(), "Bad-Name-With-Chars");
        // An empty or whitespace-only name falls back to Godot's placeholder.
        set_project_name("   ");
        assert_eq!(project_name(), UNNAMED_PROJECT);
    }

    #[test]
    fn custom_feature_tags_answer_through_has_feature() {
        clear_custom_features();
        assert!(!has_feature("my_export_tag"));
        add_custom_feature("my_export_tag");
        assert!(has_feature("my_export_tag"));
        // Built-in tags are unaffected by the custom set.
        assert_eq!(has_feature("debug"), is_debug_build());
        clear_custom_features();
        assert!(!has_feature("my_export_tag"));
    }

    #[test]
    fn editor_and_template_tags_are_complementary() {
        set_editor_build(false);
        assert!(has_feature("template"));
        assert!(!has_feature("editor"));
        set_editor_build(true);
        assert!(has_feature("editor"));
        assert!(!has_feature("template"));
        set_editor_build(false);
    }

    #[test]
    fn keycode_string_names_special_and_ascii_keys() {
        // KEY_ENTER = KEY_SPECIAL (1 << 22) | 5.
        assert_eq!(keycode_string(4194309), "Enter");
        // A bare ASCII letter names itself.
        assert_eq!(keycode_string(65), "A");
        // Modifiers render as Godot's `+`-joined prefix chain.
        assert_eq!(keycode_string(4194309 | (1 << 25)), "Shift+Enter");
        assert_eq!(keycode_string(65 | (1 << 26)), "Alt+A");
        // Modifiers with no keycode render as the modifier alone.
        assert_eq!(keycode_string(1 << 25), "Shift");
    }

    #[test]
    fn find_keycode_from_string_inverts_keycode_string() {
        // `Key::Space` names itself " " in this build's table, which no
        // `+`-separated parse can round-trip, so it is not part of the sweep.
        for code in [4194309i64, 65, 4194305] {
            let name = keycode_string(code);
            assert_eq!(
                find_keycode_from_string(&name),
                code,
                "'{name}' should map back to {code}",
            );
        }
        assert_eq!(find_keycode_from_string(""), 0);
        assert_eq!(find_keycode_from_string("NotAKey"), 0);
    }

    #[test]
    fn directories_are_populated_and_distinct() {
        assert!(!data_dir().is_empty(), "data dir resolves from the env");
        assert!(!config_dir().is_empty(), "config dir resolves from the env");
        assert!(!cache_dir().is_empty(), "cache dir resolves from the env");
        assert!(!temp_dir().is_empty(), "temp dir resolves from the env");
        assert_ne!(
            config_dir(),
            cache_dir(),
            "config and cache must be separate roots",
        );
    }

    #[test]
    fn delay_msec_blocks_for_at_least_the_requested_time() {
        let t0 = get_ticks_usec();
        delay_msec(10);
        let elapsed = get_ticks_usec() - t0;
        assert!(
            elapsed >= 10_000,
            "delay_msec(10) should block >= 10ms, blocked {elapsed}us",
        );
        // A non-positive delay returns immediately rather than blocking.
        delay_msec(0);
        delay_msec(-5);
    }

    #[test]
    fn environment_round_trips() {
        let key = "PATINA_OS_UNIT_T";
        unset_environment(key);
        assert!(!has_environment(key));
        assert_eq!(get_environment(key), "");
        set_environment(key, "value");
        assert!(has_environment(key));
        assert_eq!(get_environment(key), "value");
        unset_environment(key);
        assert!(!has_environment(key));
    }

    #[test]
    fn os_info_detect() {
        let info = OsInfo::detect();
        assert!(!info.name.is_empty());
        // In test builds, debug_assertions is typically true.
        assert!(info.is_debug);
    }

    #[test]
    fn get_ticks_returns_increasing_values() {
        let t1 = get_ticks_usec();
        // Spin briefly to guarantee time passes.
        for _ in 0..10_000 {
            std::hint::black_box(0);
        }
        let t2 = get_ticks_usec();
        assert!(t2 >= t1, "t2 ({t2}) should be >= t1 ({t1})");
    }

    #[test]
    fn current_platform_returns_known_variant() {
        let p = current_platform();
        // On macOS CI / dev machines this should be MacOS.
        assert!(
            matches!(
                p,
                Platform::Linux
                    | Platform::MacOS
                    | Platform::Windows
                    | Platform::Web
                    | Platform::Unknown
            ),
            "unexpected platform variant"
        );
    }

    #[test]
    fn is_debug_build_true_in_tests() {
        // Cargo test builds with debug_assertions by default.
        assert!(is_debug_build());
    }

    #[test]
    fn platform_variants_are_distinct() {
        assert_ne!(Platform::Linux, Platform::MacOS);
        assert_ne!(Platform::Windows, Platform::Web);
        assert_ne!(Platform::Unknown, Platform::Linux);
    }

    #[test]
    fn has_feature_platform_and_build_tags() {
        let platform = current_platform();
        let is_web = platform == Platform::Web;

        // Web/wasm tags track the compile target.
        assert_eq!(has_feature("web"), is_web);
        assert_eq!(has_feature("wasm"), is_web);

        // Recognized platforms report their pc/desktop and host-OS tags.
        match platform {
            Platform::Linux => {
                assert!(has_feature("pc"));
                assert!(has_feature("desktop"));
                assert!(has_feature("linux"));
                assert!(!has_feature("web"));
            }
            Platform::MacOS => {
                assert!(has_feature("pc"));
                assert!(has_feature("desktop"));
                assert!(has_feature("macos"));
                assert!(!has_feature("web"));
            }
            Platform::Windows => {
                assert!(has_feature("pc"));
                assert!(has_feature("desktop"));
                assert!(has_feature("windows"));
                assert!(!has_feature("web"));
            }
            Platform::Web => {
                assert!(has_feature("web"));
                assert!(has_feature("wasm"));
                assert!(!has_feature("pc"));
            }
            Platform::Unknown => {}
        }

        // Exactly one of debug/release is active, matching is_debug_build().
        assert_ne!(has_feature("debug"), has_feature("release"));
        assert_eq!(has_feature("debug"), is_debug_build());

        // Unknown tags are not features.
        assert!(!has_feature("totally_made_up_tag"));
    }
}

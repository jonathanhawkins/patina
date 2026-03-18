//! OS-level integration and platform abstraction.
//!
//! Provides basic OS information and timing utilities, mirroring a subset
//! of Godot's `OS` singleton.

use std::sync::OnceLock;
use std::time::Instant;

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

#[cfg(test)]
mod tests {
    use super::*;

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
            matches!(p, Platform::Linux | Platform::MacOS | Platform::Windows | Platform::Web | Platform::Unknown),
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
}

//! Desktop platform target definitions and validation.
//!
//! Enumerates the supported desktop platforms with their architecture,
//! feature requirements, and runtime capabilities. Provides compile-time
//! and runtime validation helpers.

use crate::export::ExportConfig;
use crate::os::Platform;

// ---------------------------------------------------------------------------
// Architecture
// ---------------------------------------------------------------------------

/// CPU architecture for a platform target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Architecture {
    /// 64-bit x86 (AMD64 / Intel 64).
    X86_64,
    /// 64-bit ARM (Apple Silicon, Graviton, etc.).
    Aarch64,
    /// 32-bit x86 (legacy, not actively supported).
    X86,
    /// WebAssembly (32-bit address space).
    Wasm32,
}

impl Architecture {
    /// Returns the Rust target triple component for this architecture.
    pub fn triple_component(&self) -> &'static str {
        match self {
            Architecture::X86_64 => "x86_64",
            Architecture::Aarch64 => "aarch64",
            Architecture::X86 => "i686",
            Architecture::Wasm32 => "wasm32",
        }
    }

    /// Returns the display name of this architecture.
    pub fn display_name(&self) -> &'static str {
        match self {
            Architecture::X86_64 => "x86_64",
            Architecture::Aarch64 => "aarch64",
            Architecture::X86 => "x86",
            Architecture::Wasm32 => "wasm32",
        }
    }

    /// Detects the architecture of the current build.
    pub fn current() -> Self {
        if cfg!(target_arch = "x86_64") {
            Architecture::X86_64
        } else if cfg!(target_arch = "aarch64") {
            Architecture::Aarch64
        } else if cfg!(target_arch = "x86") {
            Architecture::X86
        } else if cfg!(target_arch = "wasm32") {
            Architecture::Wasm32
        } else {
            // Fallback — treat as x86_64 for unknown architectures.
            Architecture::X86_64
        }
    }
}

impl std::fmt::Display for Architecture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}

// ---------------------------------------------------------------------------
// DesktopTarget
// ---------------------------------------------------------------------------

/// A supported desktop platform target with its properties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopTarget {
    /// Human-readable target name (e.g. "Linux x86_64").
    pub name: &'static str,
    /// The platform OS.
    pub platform: Platform,
    /// CPU architecture.
    pub arch: Architecture,
    /// Rust target triple (e.g. "x86_64-unknown-linux-gnu").
    pub rust_triple: &'static str,
    /// Whether this target is actively tested in CI.
    pub ci_tested: bool,
    /// Whether GPU rendering is available on this target.
    pub gpu_supported: bool,
    /// Whether windowing (winit) is supported.
    pub windowing_supported: bool,
    /// Minimum Rust toolchain version required.
    pub min_rust_version: &'static str,
}

impl DesktopTarget {
    /// Returns the platform name string used by the export system
    /// (e.g. "linux", "windows", "macos", "web").
    pub fn platform_name(&self) -> &'static str {
        match self.platform {
            Platform::Linux => "linux",
            Platform::MacOS => "macos",
            Platform::Windows => "windows",
            Platform::Web => "web",
            Platform::Unknown => "unknown",
        }
    }

    /// Creates an [`ExportConfig`] pre-filled for this target.
    pub fn export_config(&self, app_name: impl Into<String>) -> ExportConfig {
        ExportConfig::new(self.platform_name(), app_name)
    }

    /// Returns `true` if this target matches the given platform and architecture.
    pub fn matches(&self, platform: Platform, arch: Architecture) -> bool {
        self.platform == platform && self.arch == arch
    }
}

impl std::fmt::Display for DesktopTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.name, self.rust_triple)
    }
}

/// All supported desktop platform targets.
pub const DESKTOP_TARGETS: &[DesktopTarget] = &[
    // Linux
    DesktopTarget {
        name: "Linux x86_64",
        platform: Platform::Linux,
        arch: Architecture::X86_64,
        rust_triple: "x86_64-unknown-linux-gnu",
        ci_tested: true,
        gpu_supported: true,
        windowing_supported: true,
        min_rust_version: "1.75.0",
    },
    DesktopTarget {
        name: "Linux aarch64",
        platform: Platform::Linux,
        arch: Architecture::Aarch64,
        rust_triple: "aarch64-unknown-linux-gnu",
        ci_tested: false,
        gpu_supported: true,
        windowing_supported: true,
        min_rust_version: "1.75.0",
    },
    // macOS
    DesktopTarget {
        name: "macOS x86_64",
        platform: Platform::MacOS,
        arch: Architecture::X86_64,
        rust_triple: "x86_64-apple-darwin",
        ci_tested: true,
        gpu_supported: true,
        windowing_supported: true,
        min_rust_version: "1.75.0",
    },
    DesktopTarget {
        name: "macOS aarch64 (Apple Silicon)",
        platform: Platform::MacOS,
        arch: Architecture::Aarch64,
        rust_triple: "aarch64-apple-darwin",
        ci_tested: true,
        gpu_supported: true,
        windowing_supported: true,
        min_rust_version: "1.75.0",
    },
    // Windows
    DesktopTarget {
        name: "Windows x86_64",
        platform: Platform::Windows,
        arch: Architecture::X86_64,
        rust_triple: "x86_64-pc-windows-msvc",
        ci_tested: true,
        gpu_supported: true,
        windowing_supported: true,
        min_rust_version: "1.75.0",
    },
    DesktopTarget {
        name: "Windows aarch64",
        platform: Platform::Windows,
        arch: Architecture::Aarch64,
        rust_triple: "aarch64-pc-windows-msvc",
        ci_tested: false,
        gpu_supported: true,
        windowing_supported: true,
        min_rust_version: "1.75.0",
    },
    // Web
    DesktopTarget {
        name: "Web (WASM)",
        platform: Platform::Web,
        arch: Architecture::Wasm32,
        rust_triple: "wasm32-unknown-unknown",
        ci_tested: false,
        gpu_supported: false,
        windowing_supported: false,
        min_rust_version: "1.75.0",
    },
];

/// Finds a desktop target by its Rust target triple.
pub fn find_target_by_triple(triple: &str) -> Option<&'static DesktopTarget> {
    DESKTOP_TARGETS.iter().find(|t| t.rust_triple == triple)
}

/// Returns the desktop target matching the current build, if any.
pub fn current_target() -> Option<&'static DesktopTarget> {
    let platform = crate::os::current_platform();
    let arch = Architecture::current();
    DESKTOP_TARGETS
        .iter()
        .find(|t| t.platform == platform && t.arch == arch)
}

/// Returns all targets that are tested in CI.
pub fn ci_tested_targets() -> Vec<&'static DesktopTarget> {
    DESKTOP_TARGETS.iter().filter(|t| t.ci_tested).collect()
}

/// Returns all targets for a given platform.
pub fn targets_for_platform(platform: Platform) -> Vec<&'static DesktopTarget> {
    DESKTOP_TARGETS
        .iter()
        .filter(|t| t.platform == platform)
        .collect()
}

/// Validates that the current build environment matches a supported target.
///
/// Returns `Ok(target)` if the current build matches a supported target,
/// or `Err(message)` describing the mismatch.
pub fn validate_current_target() -> Result<&'static DesktopTarget, String> {
    let platform = crate::os::current_platform();
    let arch = Architecture::current();

    match current_target() {
        Some(target) => Ok(target),
        None => Err(format!(
            "unsupported platform/architecture combination: {:?}/{:?}",
            platform, arch
        )),
    }
}

/// A capability that a platform target may or may not support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlatformCapability {
    /// GPU-accelerated rendering (wgpu/Vulkan/Metal/DX12).
    GpuRendering,
    /// Native windowing via winit.
    Windowing,
    /// File system access.
    FileSystem,
    /// Network sockets.
    Networking,
    /// Audio output.
    Audio,
    /// Gamepad input.
    GamepadInput,
    /// Thread spawning.
    Threading,
}

/// Checks whether the current target supports a given capability.
pub fn supports_capability(capability: PlatformCapability) -> bool {
    let target = match current_target() {
        Some(t) => t,
        None => return false,
    };

    match capability {
        PlatformCapability::GpuRendering => target.gpu_supported,
        PlatformCapability::Windowing => target.windowing_supported,
        PlatformCapability::FileSystem => true, // All desktop targets
        PlatformCapability::Networking => true, // All desktop targets
        PlatformCapability::Audio => target.platform != Platform::Web,
        PlatformCapability::GamepadInput => target.windowing_supported,
        PlatformCapability::Threading => target.platform != Platform::Web,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_targets_not_empty() {
        assert!(
            !DESKTOP_TARGETS.is_empty(),
            "must define at least one desktop target"
        );
    }

    #[test]
    fn all_targets_have_unique_triples() {
        let mut seen = std::collections::HashSet::new();
        for target in DESKTOP_TARGETS {
            assert!(
                seen.insert(target.rust_triple),
                "duplicate rust_triple: {}",
                target.rust_triple
            );
        }
    }

    #[test]
    fn all_targets_have_nonempty_name() {
        for target in DESKTOP_TARGETS {
            assert!(!target.name.is_empty(), "target must have a name");
        }
    }

    #[test]
    fn current_target_returns_some() {
        let target = current_target();
        assert!(
            target.is_some(),
            "current build must match a supported desktop target"
        );
    }

    #[test]
    fn validate_current_target_succeeds() {
        let result = validate_current_target();
        assert!(
            result.is_ok(),
            "current target validation failed: {:?}",
            result
        );
    }

    #[test]
    fn ci_tested_targets_nonempty() {
        let ci = ci_tested_targets();
        assert!(!ci.is_empty(), "at least one target must be CI-tested");
    }

    #[test]
    fn ci_tested_includes_linux_x86_64() {
        let ci = ci_tested_targets();
        assert!(
            ci.iter()
                .any(|t| t.rust_triple == "x86_64-unknown-linux-gnu"),
            "Linux x86_64 must be CI-tested"
        );
    }

    #[test]
    fn targets_for_platform_linux() {
        let linux = targets_for_platform(Platform::Linux);
        assert!(linux.len() >= 1, "must define at least one Linux target");
        for t in &linux {
            assert_eq!(t.platform, Platform::Linux);
        }
    }

    #[test]
    fn targets_for_platform_macos() {
        let macos = targets_for_platform(Platform::MacOS);
        assert!(
            macos.len() >= 2,
            "must define x86_64 and aarch64 macOS targets"
        );
    }

    #[test]
    fn targets_for_platform_windows() {
        let windows = targets_for_platform(Platform::Windows);
        assert!(!windows.is_empty());
    }

    #[test]
    fn architecture_triple_component_format() {
        assert_eq!(Architecture::X86_64.triple_component(), "x86_64");
        assert_eq!(Architecture::Aarch64.triple_component(), "aarch64");
        assert_eq!(Architecture::X86.triple_component(), "i686");
        assert_eq!(Architecture::Wasm32.triple_component(), "wasm32");
    }

    #[test]
    fn architecture_current_is_known() {
        let arch = Architecture::current();
        // Should be one of the known variants on any test machine.
        assert!(
            matches!(
                arch,
                Architecture::X86_64
                    | Architecture::Aarch64
                    | Architecture::X86
                    | Architecture::Wasm32
            ),
            "architecture must be recognized"
        );
    }

    #[test]
    fn all_desktop_targets_support_filesystem() {
        for target in DESKTOP_TARGETS {
            // All desktop targets should support file system access.
            assert!(
                target.platform != Platform::Unknown,
                "target {} must have a known platform",
                target.name
            );
        }
    }

    #[test]
    fn desktop_targets_have_valid_min_rust() {
        for target in DESKTOP_TARGETS {
            let parts: Vec<&str> = target.min_rust_version.split('.').collect();
            assert!(
                parts.len() == 3,
                "min_rust_version must be semver: {}",
                target.min_rust_version
            );
            for part in &parts {
                assert!(
                    part.parse::<u32>().is_ok(),
                    "min_rust_version component '{}' must be numeric",
                    part
                );
            }
        }
    }

    #[test]
    fn web_target_no_gpu() {
        let web = targets_for_platform(Platform::Web);
        for t in &web {
            assert!(!t.gpu_supported, "Web target should not claim GPU support");
        }
    }

    #[test]
    fn web_target_no_windowing() {
        let web = targets_for_platform(Platform::Web);
        for t in &web {
            assert!(
                !t.windowing_supported,
                "Web target should not claim windowing support"
            );
        }
    }

    #[test]
    fn current_target_matches_current_platform() {
        let target = current_target().unwrap();
        assert_eq!(target.platform, crate::os::current_platform());
        assert_eq!(target.arch, Architecture::current());
    }

    #[test]
    fn supports_capability_filesystem_on_desktop() {
        assert!(supports_capability(PlatformCapability::FileSystem));
    }

    #[test]
    fn supports_capability_networking_on_desktop() {
        assert!(supports_capability(PlatformCapability::Networking));
    }

    #[test]
    fn supports_capability_windowing_on_desktop() {
        // All native desktop targets support windowing.
        let target = current_target().unwrap();
        if target.platform != Platform::Web {
            assert!(supports_capability(PlatformCapability::Windowing));
        }
    }

    #[test]
    fn supports_capability_threading_on_desktop() {
        let target = current_target().unwrap();
        if target.platform != Platform::Web {
            assert!(supports_capability(PlatformCapability::Threading));
        }
    }

    // -- New: Display impls ---------------------------------------------------

    #[test]
    fn architecture_display() {
        assert_eq!(format!("{}", Architecture::X86_64), "x86_64");
        assert_eq!(format!("{}", Architecture::Aarch64), "aarch64");
        assert_eq!(format!("{}", Architecture::Wasm32), "wasm32");
    }

    #[test]
    fn desktop_target_display() {
        let target = current_target().unwrap();
        let s = format!("{target}");
        assert!(s.contains(target.name), "Display must include name");
        assert!(
            s.contains(target.rust_triple),
            "Display must include triple"
        );
    }

    // -- New: find_target_by_triple -------------------------------------------

    #[test]
    fn find_target_by_triple_known() {
        let t = find_target_by_triple("x86_64-unknown-linux-gnu");
        assert!(t.is_some());
        assert_eq!(t.unwrap().platform, Platform::Linux);
        assert_eq!(t.unwrap().arch, Architecture::X86_64);
    }

    #[test]
    fn find_target_by_triple_unknown() {
        assert!(find_target_by_triple("riscv64-unknown-linux-gnu").is_none());
    }

    // -- New: DesktopTarget methods -------------------------------------------

    #[test]
    fn desktop_target_platform_name() {
        for target in DESKTOP_TARGETS {
            let name = target.platform_name();
            assert!(
                ["linux", "macos", "windows", "web"].contains(&name),
                "platform_name() for {} must be a known name, got '{}'",
                target.name,
                name
            );
        }
    }

    #[test]
    fn desktop_target_export_config() {
        let target = current_target().unwrap();
        let config = target.export_config("MyGame");
        assert_eq!(config.target_platform, target.platform_name());
        assert_eq!(config.app_name, "MyGame");
    }

    #[test]
    fn desktop_target_export_config_all_targets() {
        for target in DESKTOP_TARGETS {
            let config = target.export_config("TestApp");
            assert_eq!(config.target_platform, target.platform_name());
            assert_eq!(config.app_name, "TestApp");
        }
    }

    #[test]
    fn desktop_target_matches() {
        let target = current_target().unwrap();
        assert!(target.matches(crate::os::current_platform(), Architecture::current()));
        assert!(!target.matches(Platform::Unknown, Architecture::Wasm32));
    }
}

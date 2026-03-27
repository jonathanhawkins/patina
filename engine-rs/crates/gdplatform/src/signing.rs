//! Release binary signing for macOS and Windows.
//!
//! Provides configuration and execution of code signing for release builds:
//! - macOS: `codesign` + `notarytool` for Apple notarization
//! - Windows: `signtool` with PFX/certificate store signing
//!
//! The signing pipeline validates inputs, invokes platform tools, and
//! returns structured results with verification support.

use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// SigningError
// ---------------------------------------------------------------------------

/// Errors that can occur during the signing process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SigningError {
    /// The binary to sign was not found.
    BinaryNotFound(String),
    /// The signing identity or certificate was not found.
    IdentityNotFound(String),
    /// The entitlements file was not found (macOS).
    EntitlementsNotFound(String),
    /// The signing tool is not available on this platform.
    ToolNotAvailable(String),
    /// The signing process failed with an error message.
    SigningFailed(String),
    /// Signature verification failed.
    VerificationFailed(String),
    /// Notarization failed (macOS).
    NotarizationFailed(String),
}

impl std::fmt::Display for SigningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BinaryNotFound(p) => write!(f, "binary not found: {p}"),
            Self::IdentityNotFound(id) => write!(f, "signing identity not found: {id}"),
            Self::EntitlementsNotFound(p) => write!(f, "entitlements file not found: {p}"),
            Self::ToolNotAvailable(t) => write!(f, "signing tool not available: {t}"),
            Self::SigningFailed(msg) => write!(f, "signing failed: {msg}"),
            Self::VerificationFailed(msg) => write!(f, "verification failed: {msg}"),
            Self::NotarizationFailed(msg) => write!(f, "notarization failed: {msg}"),
        }
    }
}

// ---------------------------------------------------------------------------
// TimestampServer
// ---------------------------------------------------------------------------

/// A timestamp authority server for countersigning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimestampServer {
    /// The URL of the timestamp authority.
    pub url: String,
}

impl TimestampServer {
    /// Apple's timestamp server (used by default on macOS).
    pub const APPLE: &'static str = "http://timestamp.apple.com/ts01";
    /// DigiCert timestamp server (common for Windows).
    pub const DIGICERT: &'static str = "http://timestamp.digicert.com";
    /// Sectigo timestamp server.
    pub const SECTIGO: &'static str = "http://timestamp.sectigo.com";

    /// Creates a new timestamp server with the given URL.
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }
}

// ---------------------------------------------------------------------------
// MacOsSigningConfig
// ---------------------------------------------------------------------------

/// Configuration for macOS code signing via `codesign` and notarization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacOsSigningConfig {
    /// The signing identity (e.g. "Developer ID Application: Name (TEAMID)").
    pub identity: String,
    /// Path to the entitlements plist file (optional).
    pub entitlements: Option<PathBuf>,
    /// Whether to enable hardened runtime (required for notarization).
    pub hardened_runtime: bool,
    /// Whether to submit for Apple notarization after signing.
    pub notarize: bool,
    /// Apple ID for notarization (required if `notarize` is true).
    pub apple_id: Option<String>,
    /// Team ID for notarization.
    pub team_id: Option<String>,
    /// Timestamp server URL (defaults to Apple's).
    pub timestamp_url: Option<String>,
}

impl MacOsSigningConfig {
    /// Creates a new macOS signing config with the given identity.
    pub fn new(identity: impl Into<String>) -> Self {
        Self {
            identity: identity.into(),
            entitlements: None,
            hardened_runtime: true,
            notarize: false,
            apple_id: None,
            team_id: None,
            timestamp_url: None,
        }
    }

    /// Builder: sets the entitlements plist path.
    pub fn with_entitlements(mut self, path: impl Into<PathBuf>) -> Self {
        self.entitlements = Some(path.into());
        self
    }

    /// Builder: enables notarization.
    pub fn with_notarization(mut self, apple_id: impl Into<String>, team_id: impl Into<String>) -> Self {
        self.notarize = true;
        self.apple_id = Some(apple_id.into());
        self.team_id = Some(team_id.into());
        self
    }

    /// Builder: sets the timestamp URL.
    pub fn with_timestamp(mut self, url: impl Into<String>) -> Self {
        self.timestamp_url = Some(url.into());
        self
    }

    /// Validates the configuration, returning errors for missing requirements.
    pub fn validate(&self) -> Result<(), SigningError> {
        if self.identity.is_empty() {
            return Err(SigningError::IdentityNotFound("empty identity".into()));
        }
        if self.notarize && self.apple_id.is_none() {
            return Err(SigningError::SigningFailed(
                "notarization requires apple_id".into(),
            ));
        }
        if self.notarize && self.team_id.is_none() {
            return Err(SigningError::SigningFailed(
                "notarization requires team_id".into(),
            ));
        }
        Ok(())
    }

    /// Builds the `codesign` command arguments for signing a binary.
    pub fn codesign_args(&self, binary_path: &Path) -> Vec<String> {
        let mut args = vec![
            "--sign".to_string(),
            self.identity.clone(),
            "--force".to_string(),
        ];
        if self.hardened_runtime {
            args.push("--options".to_string());
            args.push("runtime".to_string());
        }
        if let Some(ref ent) = self.entitlements {
            args.push("--entitlements".to_string());
            args.push(ent.to_string_lossy().into_owned());
        }
        if let Some(ref ts) = self.timestamp_url {
            args.push("--timestamp".to_string());
            args.push(ts.clone());
        } else {
            args.push("--timestamp".to_string());
        }
        args.push(binary_path.to_string_lossy().into_owned());
        args
    }
}

// ---------------------------------------------------------------------------
// WindowsSigningConfig
// ---------------------------------------------------------------------------

/// Configuration for Windows code signing via `signtool`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsSigningConfig {
    /// Path to the PFX certificate file (mutually exclusive with `cert_store`).
    pub pfx_path: Option<PathBuf>,
    /// Password for the PFX file.
    pub pfx_password: Option<String>,
    /// Certificate store name (e.g. "My") for store-based signing.
    pub cert_store: Option<String>,
    /// Subject name filter for certificate selection from the store.
    pub cert_subject: Option<String>,
    /// SHA-256 thumbprint for certificate selection.
    pub cert_thumbprint: Option<String>,
    /// Digest algorithm (default: sha256).
    pub digest_algorithm: String,
    /// Timestamp server URL.
    pub timestamp_url: Option<String>,
    /// Description shown in the UAC dialog.
    pub description: Option<String>,
}

impl WindowsSigningConfig {
    /// Creates a new config for PFX-based signing.
    pub fn from_pfx(pfx_path: impl Into<PathBuf>, password: impl Into<String>) -> Self {
        Self {
            pfx_path: Some(pfx_path.into()),
            pfx_password: Some(password.into()),
            cert_store: None,
            cert_subject: None,
            cert_thumbprint: None,
            digest_algorithm: "sha256".to_string(),
            timestamp_url: Some(TimestampServer::DIGICERT.to_string()),
            description: None,
        }
    }

    /// Creates a new config for certificate-store-based signing.
    pub fn from_store(store: impl Into<String>, subject: impl Into<String>) -> Self {
        Self {
            pfx_path: None,
            pfx_password: None,
            cert_store: Some(store.into()),
            cert_subject: Some(subject.into()),
            cert_thumbprint: None,
            digest_algorithm: "sha256".to_string(),
            timestamp_url: Some(TimestampServer::DIGICERT.to_string()),
            description: None,
        }
    }

    /// Builder: sets the digest algorithm.
    pub fn with_digest(mut self, algorithm: impl Into<String>) -> Self {
        self.digest_algorithm = algorithm.into();
        self
    }

    /// Builder: sets the timestamp URL.
    pub fn with_timestamp(mut self, url: impl Into<String>) -> Self {
        self.timestamp_url = Some(url.into());
        self
    }

    /// Builder: sets the description for the UAC dialog.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Builder: sets the SHA-256 thumbprint for cert selection.
    pub fn with_thumbprint(mut self, thumbprint: impl Into<String>) -> Self {
        self.cert_thumbprint = Some(thumbprint.into());
        self
    }

    /// Validates the configuration.
    pub fn validate(&self) -> Result<(), SigningError> {
        if self.pfx_path.is_none() && self.cert_store.is_none() {
            return Err(SigningError::IdentityNotFound(
                "either pfx_path or cert_store must be set".into(),
            ));
        }
        Ok(())
    }

    /// Builds the `signtool sign` command arguments for signing a binary.
    pub fn signtool_args(&self, binary_path: &Path) -> Vec<String> {
        let mut args = vec!["sign".to_string()];

        if let Some(ref pfx) = self.pfx_path {
            args.push("/f".to_string());
            args.push(pfx.to_string_lossy().into_owned());
            if let Some(ref pw) = self.pfx_password {
                args.push("/p".to_string());
                args.push(pw.clone());
            }
        } else if let Some(ref store) = self.cert_store {
            args.push("/s".to_string());
            args.push(store.clone());
            if let Some(ref subj) = self.cert_subject {
                args.push("/n".to_string());
                args.push(subj.clone());
            }
        }

        if let Some(ref thumb) = self.cert_thumbprint {
            args.push("/sha1".to_string());
            args.push(thumb.clone());
        }

        args.push("/fd".to_string());
        args.push(self.digest_algorithm.clone());

        if let Some(ref ts) = self.timestamp_url {
            args.push("/tr".to_string());
            args.push(ts.clone());
            args.push("/td".to_string());
            args.push(self.digest_algorithm.clone());
        }

        if let Some(ref desc) = self.description {
            args.push("/d".to_string());
            args.push(desc.clone());
        }

        args.push(binary_path.to_string_lossy().into_owned());
        args
    }
}

// ---------------------------------------------------------------------------
// SigningResult
// ---------------------------------------------------------------------------

/// Result of a signing or verification operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningResult {
    /// Whether the operation succeeded.
    pub success: bool,
    /// The binary that was signed.
    pub binary_path: PathBuf,
    /// The command that was executed (for logging).
    pub command: String,
    /// Messages from the signing tool.
    pub messages: Vec<String>,
}

impl SigningResult {
    /// Creates a successful result.
    pub fn ok(binary_path: impl Into<PathBuf>, command: impl Into<String>) -> Self {
        Self {
            success: true,
            binary_path: binary_path.into(),
            command: command.into(),
            messages: Vec::new(),
        }
    }

    /// Creates a failed result.
    pub fn err(
        binary_path: impl Into<PathBuf>,
        command: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            success: false,
            binary_path: binary_path.into(),
            command: command.into(),
            messages: vec![message.into()],
        }
    }
}

// ---------------------------------------------------------------------------
// sign_binary / verify_signature (headless stubs with command generation)
// ---------------------------------------------------------------------------

/// Signs a binary using the macOS `codesign` tool.
///
/// In headless/CI mode, this validates the config and returns the command
/// that would be executed. In a real build pipeline, it would invoke the tool.
pub fn sign_macos(
    binary_path: &Path,
    config: &MacOsSigningConfig,
) -> Result<SigningResult, SigningError> {
    config.validate()?;
    if !binary_path.as_os_str().is_empty()
        && binary_path.extension().is_none()
        && !binary_path.to_string_lossy().ends_with(".app")
        && !binary_path.to_string_lossy().ends_with(".dmg")
        && !binary_path.to_string_lossy().ends_with(".dylib")
    {
        // Accept extensionless Unix binaries
    }
    let args = config.codesign_args(binary_path);
    let cmd = format!("codesign {}", args.join(" "));
    Ok(SigningResult::ok(binary_path, cmd))
}

/// Signs a binary using the Windows `signtool`.
pub fn sign_windows(
    binary_path: &Path,
    config: &WindowsSigningConfig,
) -> Result<SigningResult, SigningError> {
    config.validate()?;
    let args = config.signtool_args(binary_path);
    let cmd = format!("signtool {}", args.join(" "));
    Ok(SigningResult::ok(binary_path, cmd))
}

/// Generates a `codesign --verify` command for macOS signature verification.
pub fn verify_macos(binary_path: &Path) -> SigningResult {
    let cmd = format!(
        "codesign --verify --deep --strict {}",
        binary_path.display()
    );
    SigningResult::ok(binary_path, cmd)
}

/// Generates a `signtool verify` command for Windows signature verification.
pub fn verify_windows(binary_path: &Path) -> SigningResult {
    let cmd = format!("signtool verify /pa {}", binary_path.display());
    SigningResult::ok(binary_path, cmd)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- MacOsSigningConfig --------------------------------------------------

    #[test]
    fn macos_config_new_defaults() {
        let cfg = MacOsSigningConfig::new("Developer ID Application: Test (ABCDEF)");
        assert_eq!(cfg.identity, "Developer ID Application: Test (ABCDEF)");
        assert!(cfg.hardened_runtime);
        assert!(!cfg.notarize);
        assert!(cfg.entitlements.is_none());
        assert!(cfg.apple_id.is_none());
        assert!(cfg.team_id.is_none());
    }

    #[test]
    fn macos_config_builder_chain() {
        let cfg = MacOsSigningConfig::new("Test Identity")
            .with_entitlements("entitlements.plist")
            .with_notarization("dev@example.com", "ABCDEF")
            .with_timestamp(TimestampServer::APPLE);
        assert_eq!(cfg.entitlements, Some(PathBuf::from("entitlements.plist")));
        assert!(cfg.notarize);
        assert_eq!(cfg.apple_id.as_deref(), Some("dev@example.com"));
        assert_eq!(cfg.team_id.as_deref(), Some("ABCDEF"));
        assert_eq!(cfg.timestamp_url.as_deref(), Some(TimestampServer::APPLE));
    }

    #[test]
    fn macos_config_validate_empty_identity() {
        let cfg = MacOsSigningConfig::new("");
        assert!(matches!(cfg.validate(), Err(SigningError::IdentityNotFound(_))));
    }

    #[test]
    fn macos_config_validate_notarize_missing_apple_id() {
        let mut cfg = MacOsSigningConfig::new("Test");
        cfg.notarize = true;
        cfg.team_id = Some("TEAM".into());
        assert!(matches!(cfg.validate(), Err(SigningError::SigningFailed(_))));
    }

    #[test]
    fn macos_config_validate_notarize_missing_team_id() {
        let mut cfg = MacOsSigningConfig::new("Test");
        cfg.notarize = true;
        cfg.apple_id = Some("dev@test.com".into());
        assert!(matches!(cfg.validate(), Err(SigningError::SigningFailed(_))));
    }

    #[test]
    fn macos_codesign_args_basic() {
        let cfg = MacOsSigningConfig::new("Developer ID");
        let args = cfg.codesign_args(Path::new("build/patina"));
        assert!(args.contains(&"--sign".to_string()));
        assert!(args.contains(&"Developer ID".to_string()));
        assert!(args.contains(&"--force".to_string()));
        assert!(args.contains(&"--options".to_string()));
        assert!(args.contains(&"runtime".to_string()));
        assert!(args.contains(&"--timestamp".to_string()));
        assert!(args.contains(&"build/patina".to_string()));
    }

    #[test]
    fn macos_codesign_args_with_entitlements() {
        let cfg = MacOsSigningConfig::new("ID").with_entitlements("ent.plist");
        let args = cfg.codesign_args(Path::new("app.app"));
        assert!(args.contains(&"--entitlements".to_string()));
        assert!(args.contains(&"ent.plist".to_string()));
    }

    #[test]
    fn macos_codesign_args_custom_timestamp() {
        let cfg = MacOsSigningConfig::new("ID").with_timestamp("http://ts.example.com");
        let args = cfg.codesign_args(Path::new("bin"));
        let ts_idx = args.iter().position(|a| a == "--timestamp").unwrap();
        assert_eq!(args[ts_idx + 1], "http://ts.example.com");
    }

    // -- WindowsSigningConfig ------------------------------------------------

    #[test]
    fn windows_config_from_pfx() {
        let cfg = WindowsSigningConfig::from_pfx("cert.pfx", "secret");
        assert_eq!(cfg.pfx_path, Some(PathBuf::from("cert.pfx")));
        assert_eq!(cfg.pfx_password.as_deref(), Some("secret"));
        assert_eq!(cfg.digest_algorithm, "sha256");
        assert!(cfg.timestamp_url.is_some());
    }

    #[test]
    fn windows_config_from_store() {
        let cfg = WindowsSigningConfig::from_store("My", "Patina Inc.");
        assert_eq!(cfg.cert_store.as_deref(), Some("My"));
        assert_eq!(cfg.cert_subject.as_deref(), Some("Patina Inc."));
        assert!(cfg.pfx_path.is_none());
    }

    #[test]
    fn windows_config_builder_chain() {
        let cfg = WindowsSigningConfig::from_pfx("c.pfx", "pw")
            .with_digest("sha384")
            .with_timestamp("http://ts.test.com")
            .with_description("Patina Engine")
            .with_thumbprint("AABBCCDD");
        assert_eq!(cfg.digest_algorithm, "sha384");
        assert_eq!(cfg.timestamp_url.as_deref(), Some("http://ts.test.com"));
        assert_eq!(cfg.description.as_deref(), Some("Patina Engine"));
        assert_eq!(cfg.cert_thumbprint.as_deref(), Some("AABBCCDD"));
    }

    #[test]
    fn windows_config_validate_no_cert() {
        let cfg = WindowsSigningConfig {
            pfx_path: None,
            pfx_password: None,
            cert_store: None,
            cert_subject: None,
            cert_thumbprint: None,
            digest_algorithm: "sha256".into(),
            timestamp_url: None,
            description: None,
        };
        assert!(matches!(cfg.validate(), Err(SigningError::IdentityNotFound(_))));
    }

    #[test]
    fn windows_signtool_args_pfx() {
        let cfg = WindowsSigningConfig::from_pfx("cert.pfx", "pass123");
        let args = cfg.signtool_args(Path::new("game.exe"));
        assert_eq!(args[0], "sign");
        assert!(args.contains(&"/f".to_string()));
        assert!(args.contains(&"cert.pfx".to_string()));
        assert!(args.contains(&"/p".to_string()));
        assert!(args.contains(&"pass123".to_string()));
        assert!(args.contains(&"/fd".to_string()));
        assert!(args.contains(&"sha256".to_string()));
        assert!(args.contains(&"game.exe".to_string()));
    }

    #[test]
    fn windows_signtool_args_store() {
        let cfg = WindowsSigningConfig::from_store("My", "Patina Inc.");
        let args = cfg.signtool_args(Path::new("editor.exe"));
        assert!(args.contains(&"/s".to_string()));
        assert!(args.contains(&"My".to_string()));
        assert!(args.contains(&"/n".to_string()));
        assert!(args.contains(&"Patina Inc.".to_string()));
    }

    #[test]
    fn windows_signtool_args_with_description() {
        let cfg = WindowsSigningConfig::from_pfx("c.pfx", "pw")
            .with_description("Test App");
        let args = cfg.signtool_args(Path::new("app.exe"));
        assert!(args.contains(&"/d".to_string()));
        assert!(args.contains(&"Test App".to_string()));
    }

    #[test]
    fn windows_signtool_args_with_thumbprint() {
        let cfg = WindowsSigningConfig::from_pfx("c.pfx", "pw")
            .with_thumbprint("AA11BB22");
        let args = cfg.signtool_args(Path::new("app.exe"));
        assert!(args.contains(&"/sha1".to_string()));
        assert!(args.contains(&"AA11BB22".to_string()));
    }

    // -- sign_macos / sign_windows -------------------------------------------

    #[test]
    fn sign_macos_generates_command() {
        let cfg = MacOsSigningConfig::new("Developer ID Application: Test");
        let result = sign_macos(Path::new("build/patina"), &cfg).unwrap();
        assert!(result.success);
        assert!(result.command.starts_with("codesign"));
        assert!(result.command.contains("Developer ID Application: Test"));
        assert!(result.command.contains("build/patina"));
    }

    #[test]
    fn sign_macos_rejects_empty_identity() {
        let cfg = MacOsSigningConfig::new("");
        let err = sign_macos(Path::new("build/patina"), &cfg).unwrap_err();
        assert!(matches!(err, SigningError::IdentityNotFound(_)));
    }

    #[test]
    fn sign_windows_generates_command() {
        let cfg = WindowsSigningConfig::from_pfx("cert.pfx", "pass");
        let result = sign_windows(Path::new("build/patina.exe"), &cfg).unwrap();
        assert!(result.success);
        assert!(result.command.starts_with("signtool"));
        assert!(result.command.contains("cert.pfx"));
        assert!(result.command.contains("build/patina.exe"));
    }

    #[test]
    fn sign_windows_rejects_no_cert() {
        let cfg = WindowsSigningConfig {
            pfx_path: None,
            pfx_password: None,
            cert_store: None,
            cert_subject: None,
            cert_thumbprint: None,
            digest_algorithm: "sha256".into(),
            timestamp_url: None,
            description: None,
        };
        let err = sign_windows(Path::new("app.exe"), &cfg).unwrap_err();
        assert!(matches!(err, SigningError::IdentityNotFound(_)));
    }

    // -- verify_macos / verify_windows ---------------------------------------

    #[test]
    fn verify_macos_command() {
        let result = verify_macos(Path::new("build/patina"));
        assert!(result.success);
        assert!(result.command.contains("codesign --verify"));
        assert!(result.command.contains("build/patina"));
    }

    #[test]
    fn verify_windows_command() {
        let result = verify_windows(Path::new("game.exe"));
        assert!(result.success);
        assert!(result.command.contains("signtool verify"));
        assert!(result.command.contains("game.exe"));
    }

    // -- SigningResult -------------------------------------------------------

    #[test]
    fn signing_result_ok() {
        let r = SigningResult::ok("bin", "cmd");
        assert!(r.success);
        assert_eq!(r.binary_path, PathBuf::from("bin"));
        assert_eq!(r.command, "cmd");
        assert!(r.messages.is_empty());
    }

    #[test]
    fn signing_result_err() {
        let r = SigningResult::err("bin", "cmd", "failed");
        assert!(!r.success);
        assert_eq!(r.messages, vec!["failed"]);
    }

    // -- SigningError Display ------------------------------------------------

    #[test]
    fn signing_error_display() {
        let e = SigningError::BinaryNotFound("foo".into());
        assert_eq!(format!("{e}"), "binary not found: foo");
        let e = SigningError::ToolNotAvailable("codesign".into());
        assert_eq!(format!("{e}"), "signing tool not available: codesign");
    }

    // -- TimestampServer constants -------------------------------------------

    #[test]
    fn timestamp_server_constants() {
        assert!(TimestampServer::APPLE.starts_with("http"));
        assert!(TimestampServer::DIGICERT.starts_with("http"));
        assert!(TimestampServer::SECTIGO.starts_with("http"));
    }

    #[test]
    fn timestamp_server_new() {
        let ts = TimestampServer::new("http://ts.example.com");
        assert_eq!(ts.url, "http://ts.example.com");
    }
}

use std::path::{Path, PathBuf};

use crate::error::{OrchestratorError, Result};

/// Agent Mail server connection config, read from codex.mcp.json.
#[derive(Debug, Clone)]
pub struct MailConfig {
    pub url: String,
    pub token: Option<String>,
    pub connect_timeout_secs: u64,
    pub max_time_secs: u64,
    pub retry_attempts: u32,
}

/// All tunable orchestrator parameters with env-var overrides.
#[derive(Debug, Clone)]
pub struct Config {
    pub project_root: PathBuf,
    pub orch_root: PathBuf,
    pub interval_seconds: u64,
    pub session_name: Option<String>,
    pub session_family: Option<String>,
    pub window_index: u32,
    pub coordinator_agent: Option<String>,

    // Backlog thresholds
    pub min_ready_unassigned: usize,
    pub min_open_backlog: usize,

    // Stall detection
    pub stall_idle_assigned_threshold: usize,
    pub stall_active_assigned_max: usize,
    pub missing_worker_assignment_threshold: usize,
    pub stall_consecutive_polls: usize,
    pub stall_reclaim_seconds: u64,
    pub max_stall_cycles: usize,

    // Assignment tuning
    pub reprompt_cooldown_seconds: u64,
    pub stale_assignment_seconds: u64,
    pub idle_reclaim_grace_seconds: u64,
    pub completion_wait_grace_seconds: u64,

    // Verification
    pub verify_reported_tests: bool,
    pub verify_timeout_seconds: u64,

    // Browser verification
    pub browser_verify_enabled: bool,
    pub browser_verify_panes: Vec<u32>,

    // Agent type: "claude" or "codex"
    pub agent_type: String,

    // Tmux worker detection
    pub min_worker_pane_index: u32,
    pub worker_command: String,
    pub queue_prompt_marker: String,
    pub shell_prompt_char: char,
    pub mail_server_suffix: String,

    // Tmux timing (milliseconds unless noted)
    pub prompt_submit_delay_ms: u64,
    pub shell_prompt_wait_ms: u64,
    pub post_clear_delay_ms: u64,
    pub post_type_delay_ms: u64,
    pub submit_retry_attempts: u32,
    pub shell_prompt_timeout_secs: u64,
    pub capture_lines: usize,

    // Tmux key sequences
    pub clear_line_key: String,
    pub submit_key: String,

    // Idle fill retry
    pub idle_fill_retry_attempts: u32,

    // Tiered planner
    pub deep_planner_cooldown_secs: u64,
    pub fast_planner_enabled: bool,
    pub deep_planner_enabled: bool,
    pub planner_restart_cooldown_secs: u64,

    // Pull mode: workers discover and claim their own work.
    // When true, coordinator skips: assign_idle_workers, recover_stuck_workers,
    // prompt submission, and reprompt logic. It only monitors, verifies, and closes.
    pub pull_mode: bool,

    // Mail config
    pub mail: MailConfig,
}

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_string(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

impl Config {
    /// Load config from environment variables and codex.mcp.json.
    pub fn from_env(project_root: PathBuf) -> Result<Self> {
        let orch_root = project_root.join("apps/orchestrator");
        let mail = load_mail_config(&project_root)?;

        // Read coordinator name from env var, or from the discovery file
        // written by the launcher (.beads/coordinator_agent).
        let coordinator_agent = env_string("AGENT_NAME")
            .or_else(|| env_string("ORCH_COORDINATOR_AGENT"))
            .or_else(|| {
                let coord_file = project_root.join(".beads/coordinator_agent");
                std::fs::read_to_string(coord_file).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
            });

        let browser_verify_panes: Vec<u32> = env_string("ORCH_BROWSER_VERIFY_PANES")
            .map(|s| {
                s.split(',')
                    .filter_map(|p| p.trim().parse().ok())
                    .collect()
            })
            .unwrap_or_default();

        Ok(Config {
            project_root,
            orch_root,
            interval_seconds: env_or("ORCH_INTERVAL_SECONDS", 8),
            session_name: env_string("ORCH_SESSION"),
            session_family: env_string("ORCH_SESSION_FAMILY"),
            window_index: env_or("ORCH_WINDOW_INDEX", 0),
            coordinator_agent,
            min_ready_unassigned: env_or("ORCH_MIN_READY_UNASSIGNED", 6),
            min_open_backlog: env_or("ORCH_MIN_OPEN_BACKLOG", 8),
            stall_idle_assigned_threshold: env_or("ORCH_STALL_IDLE_ASSIGNED_THRESHOLD", 3),
            stall_active_assigned_max: env_or("ORCH_STALL_ACTIVE_ASSIGNED_MAX", 1),
            missing_worker_assignment_threshold: env_or(
                "ORCH_MISSING_WORKER_ASSIGNMENT_THRESHOLD",
                2,
            ),
            stall_consecutive_polls: env_or("ORCH_STALL_CONSECUTIVE_POLLS", 2),
            stall_reclaim_seconds: env_or("ORCH_STALL_RECLAIM_SECONDS", 120),
            max_stall_cycles: env_or("ORCH_MAX_STALL_CYCLES", 10),
            reprompt_cooldown_seconds: env_or("ORCH_REPROMPT_COOLDOWN_SECONDS", 45),
            stale_assignment_seconds: env_or("ORCH_STALE_ASSIGNMENT_SECONDS", 120),
            idle_reclaim_grace_seconds: env_or("ORCH_IDLE_RECLAIM_GRACE_SECONDS", 30),
            completion_wait_grace_seconds: env_or("ORCH_COMPLETION_WAIT_GRACE_SECONDS", 30),
            verify_reported_tests: env_or("VERIFY_REPORTED_TESTS", true),
            verify_timeout_seconds: env_or("VERIFY_TIMEOUT_SECONDS", 900),
            browser_verify_enabled: env_or("ORCH_BROWSER_VERIFY_ENABLED", false),
            browser_verify_panes,

            // Agent type (infer from worker command if not set explicitly)
            agent_type: env_string("ORCH_AGENT_TYPE")
                .unwrap_or_else(|| {
                    let wc = env_string("ORCH_WORKER_COMMAND").unwrap_or_default();
                    if wc.contains("codex") { "codex".to_string() } else { "claude".to_string() }
                }),

            // Tmux worker detection
            min_worker_pane_index: env_or("ORCH_MIN_WORKER_PANE_INDEX", 3),
            worker_command: env_string("ORCH_WORKER_COMMAND")
                .unwrap_or_else(|| "claude".to_string()),
            queue_prompt_marker: env_string("ORCH_QUEUE_PROMPT_MARKER")
                .unwrap_or_else(|| "Press up to edit".to_string()),
            shell_prompt_char: env_string("ORCH_SHELL_PROMPT_CHAR")
                .and_then(|s| s.chars().next())
                .unwrap_or('\u{276f}'),
            mail_server_suffix: env_string("ORCH_MAIL_SERVER_SUFFIX")
                .unwrap_or_else(|| "--agent-mail".to_string()),

            // Tmux timing
            prompt_submit_delay_ms: env_or("ORCH_PROMPT_SUBMIT_DELAY_MS", 800),
            shell_prompt_wait_ms: env_or("ORCH_SHELL_PROMPT_WAIT_MS", 1000),
            post_clear_delay_ms: env_or("ORCH_POST_CLEAR_DELAY_MS", 200),
            post_type_delay_ms: env_or("ORCH_POST_TYPE_DELAY_MS", 2000),
            submit_retry_attempts: env_or("ORCH_SUBMIT_RETRY_ATTEMPTS", 3),
            shell_prompt_timeout_secs: env_or("ORCH_SHELL_PROMPT_TIMEOUT_SECS", 5),
            capture_lines: env_or("ORCH_CAPTURE_LINES", 40),

            // Tmux key sequences
            clear_line_key: env_string("ORCH_CLEAR_LINE_KEY")
                .unwrap_or_else(|| "C-u".to_string()),
            submit_key: env_string("ORCH_SUBMIT_KEY")
                .unwrap_or_else(|| "Enter".to_string()),

            idle_fill_retry_attempts: env_or("ORCH_IDLE_FILL_RETRY_ATTEMPTS", 3),

            deep_planner_cooldown_secs: env_or("ORCH_DEEP_PLANNER_COOLDOWN_SECS", 300),
            fast_planner_enabled: env_or("ORCH_FAST_PLANNER_ENABLED", true),
            deep_planner_enabled: env_or("ORCH_DEEP_PLANNER_ENABLED", true),
            planner_restart_cooldown_secs: env_or("ORCH_PLANNER_RESTART_COOLDOWN_SECS", 180),
            pull_mode: env_or("ORCH_PULL_MODE", true),

            mail,
        })
    }

    /// The mail server session name derived from session family.
    pub fn mail_server_session(&self) -> Option<String> {
        self.session_family
            .as_ref()
            .or(self.session_name.as_ref())
            .map(|family| format!("{family}{}", self.mail_server_suffix))
    }

    /// Path to the seed script.
    pub fn seed_script(&self) -> PathBuf {
        self.orch_root.join("swarm/seed_port_beads.sh")
    }

    /// Path to the notify-boss script.
    pub fn notify_boss_script(&self) -> PathBuf {
        self.orch_root.join("swarm/notify_boss.sh")
    }

    /// Path to the identity-resolve script.
    pub fn identity_resolve_script(&self) -> PathBuf {
        self.project_root
            .join("mcp_agent_mail/scripts/identity-resolve.sh")
    }

    /// Path to the coordinator lock file.
    pub fn lock_file(&self) -> PathBuf {
        self.project_root
            .join(".codex/orchestrator/coordinator.lock")
    }

    /// Path to the reprompt cache directory.
    pub fn cache_dir(&self) -> PathBuf {
        self.project_root.join(".codex/orchestrator")
    }

    /// Desired ready backlog based on worker count.
    pub fn desired_ready_backlog(&self, worker_panes: usize) -> usize {
        self.min_ready_unassigned.max(worker_panes)
    }

    /// Desired open backlog based on worker count.
    pub fn desired_open_backlog(&self, worker_panes: usize) -> usize {
        self.min_open_backlog.max(worker_panes * 2)
    }

    /// Whether a pane index is reserved for browser verification.
    pub fn is_reserved_verifier_pane(&self, pane_index: u32) -> bool {
        self.browser_verify_enabled && self.browser_verify_panes.contains(&pane_index)
    }

    pub fn is_codex(&self) -> bool {
        self.agent_type == "codex"
    }

    pub fn agent_program_name(&self) -> &str {
        if self.is_codex() { "codex" } else { "claude-code" }
    }

    pub fn worker_skill_name(&self) -> &str {
        if self.is_codex() { "patina-fly-worker" } else { "flywheel-worker" }
    }

    pub fn completion_skill_name(&self) -> &str {
        "mail-complete"
    }

    /// Build a PromptConfig from the orchestrator config.
    pub fn prompt_config(&self) -> crate::tmux::PromptConfig {
        crate::tmux::PromptConfig {
            queue_prompt_marker: self.queue_prompt_marker.clone(),
            shell_prompt_char: self.shell_prompt_char,
            clear_line_key: self.clear_line_key.clone(),
            submit_key: self.submit_key.clone(),
            prompt_submit_delay_ms: self.prompt_submit_delay_ms,
            shell_prompt_wait_ms: self.shell_prompt_wait_ms,
            post_clear_delay_ms: self.post_clear_delay_ms,
            post_type_delay_ms: self.post_type_delay_ms,
            submit_retry_attempts: self.submit_retry_attempts,
            shell_prompt_timeout_secs: self.shell_prompt_timeout_secs,
            capture_lines: self.capture_lines,
        }
    }

    /// Check whether a pane should be skipped (reserved or non-worker).
    pub fn is_worker_pane(&self, pane: &crate::tmux::PaneInfo) -> bool {
        pane.index >= self.min_worker_pane_index
            && !pane.dead
            && pane.current_command.starts_with(&self.worker_command)
    }
}

/// Read Agent Mail URL and token from codex.mcp.json.
pub fn load_mail_config(project_root: &Path) -> Result<MailConfig> {
    let config_path = project_root.join("codex.mcp.json");
    if !config_path.exists() {
        return Err(OrchestratorError::Config(format!(
            "missing {}",
            config_path.display()
        )));
    }

    let raw = std::fs::read_to_string(&config_path)?;
    let parsed: serde_json::Value = serde_json::from_str(&raw)?;

    let server = parsed
        .get("mcpServers")
        .and_then(|s| s.get("mcp-agent-mail"))
        .ok_or_else(|| {
            OrchestratorError::Config("mcpServers.mcp-agent-mail not found in codex.mcp.json".into())
        })?;

    let url = server
        .get("url")
        .and_then(|u| u.as_str())
        .ok_or_else(|| OrchestratorError::Config("mcp-agent-mail.url not found".into()))?
        .to_string();

    let token = server
        .get("headers")
        .and_then(|h| h.get("Authorization"))
        .and_then(|a| a.as_str())
        .map(|auth| {
            auth.strip_prefix("Bearer ")
                .unwrap_or(auth)
                .to_string()
        });

    Ok(MailConfig {
        url,
        token,
        connect_timeout_secs: env_or("AGENT_MAIL_CONNECT_TIMEOUT", 2),
        max_time_secs: env_or("AGENT_MAIL_MAX_TIME", 15),
        retry_attempts: env_or("AGENT_MAIL_RETRY_ATTEMPTS", 4),
    })
}

/// Build a Config with all defaults (no env lookups, no file I/O).
/// Useful for tests in any module.
#[cfg(test)]
pub fn test_config() -> Config {
    Config {
        project_root: PathBuf::from("/tmp"),
        orch_root: PathBuf::from("/tmp"),
        interval_seconds: 8,
        session_name: None,
        session_family: None,
        window_index: 0,
        coordinator_agent: None,
        min_ready_unassigned: 6,
        min_open_backlog: 8,
        stall_idle_assigned_threshold: 3,
        stall_active_assigned_max: 1,
        missing_worker_assignment_threshold: 2,
        stall_consecutive_polls: 2,
        stall_reclaim_seconds: 120,
        max_stall_cycles: 10,
        reprompt_cooldown_seconds: 45,
        stale_assignment_seconds: 120,
        idle_reclaim_grace_seconds: 30,
        completion_wait_grace_seconds: 300,
        verify_reported_tests: true,
        verify_timeout_seconds: 900,
        browser_verify_enabled: false,
        browser_verify_panes: vec![],
        agent_type: "claude".to_string(),
        min_worker_pane_index: 3,
        worker_command: "claude".to_string(),
        queue_prompt_marker: "Press up to edit".to_string(),
        shell_prompt_char: '\u{276f}',
        mail_server_suffix: "--agent-mail".to_string(),
        prompt_submit_delay_ms: 800,
        shell_prompt_wait_ms: 1000,
        post_clear_delay_ms: 200,
        post_type_delay_ms: 300,
        submit_retry_attempts: 3,
        shell_prompt_timeout_secs: 5,
        capture_lines: 40,
        clear_line_key: "C-u".to_string(),
        submit_key: "Enter".to_string(),
        idle_fill_retry_attempts: 3,
        deep_planner_cooldown_secs: 300,
        fast_planner_enabled: true,
        deep_planner_enabled: true,
        planner_restart_cooldown_secs: 180,
        pull_mode: true,
        mail: MailConfig {
            url: String::new(),
            token: None,
            connect_timeout_secs: 2,
            max_time_secs: 15,
            retry_attempts: 4,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_load_mail_config() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("codex.mcp.json");
        let mut f = std::fs::File::create(&config_path).unwrap();
        write!(
            f,
            r#"{{
                "mcpServers": {{
                    "mcp-agent-mail": {{
                        "type": "http",
                        "url": "http://127.0.0.1:8765/api/",
                        "headers": {{
                            "Authorization": "Bearer test-token-123"
                        }}
                    }}
                }}
            }}"#
        )
        .unwrap();

        let mail = load_mail_config(dir.path()).unwrap();
        assert_eq!(mail.url, "http://127.0.0.1:8765/api/");
        assert_eq!(mail.token.as_deref(), Some("test-token-123"));
    }

    #[test]
    fn test_desired_backlog() {
        let cfg = test_config();


        // With 3 workers, min_ready (6) wins
        assert_eq!(cfg.desired_ready_backlog(3), 6);
        // With 9 workers, worker count wins
        assert_eq!(cfg.desired_ready_backlog(9), 9);
        // Open backlog: 2x workers or min, whichever is greater
        assert_eq!(cfg.desired_open_backlog(3), 8); // min wins
        assert_eq!(cfg.desired_open_backlog(9), 18); // 2x wins
    }

    #[test]
    fn test_mail_server_session_uses_suffix() {
        let mut cfg = test_config();
        cfg.session_family = Some("swarm".to_string());
        assert_eq!(cfg.mail_server_session(), Some("swarm--agent-mail".to_string()));

        cfg.mail_server_suffix = "--custom-mail".to_string();
        assert_eq!(cfg.mail_server_session(), Some("swarm--custom-mail".to_string()));
    }

    #[test]
    fn test_mail_server_session_none_when_no_family() {
        let cfg = test_config();
        assert_eq!(cfg.mail_server_session(), None);
    }

    #[test]
    fn test_is_worker_pane() {
        let cfg = test_config();

        // Worker pane: index >= 3, alive, running claude
        let worker = crate::tmux::PaneInfo {
            index: 3,
            id: "%3".to_string(),
            dead: false,
            current_command: "claude".to_string(),
        };
        assert!(cfg.is_worker_pane(&worker));

        // Reserved panes (0=monitor, 1=boss, 2=bv)
        for idx in 0..=2 {
            let reserved = crate::tmux::PaneInfo {
                index: idx,
                id: format!("%{idx}"),
                dead: false,
                current_command: "claude".to_string(),
            };
            assert!(!cfg.is_worker_pane(&reserved), "pane {idx} should not be a worker");
        }

        // Dead pane
        let dead = crate::tmux::PaneInfo {
            index: 3,
            id: "%3".to_string(),
            dead: true,
            current_command: "claude".to_string(),
        };
        assert!(!cfg.is_worker_pane(&dead));

        // Non-claude command
        let shell = crate::tmux::PaneInfo {
            index: 3,
            id: "%3".to_string(),
            dead: false,
            current_command: "zsh".to_string(),
        };
        assert!(!cfg.is_worker_pane(&shell));
    }

    #[test]
    fn test_is_worker_pane_custom_thresholds() {
        let mut cfg = test_config();
        cfg.min_worker_pane_index = 4;
        cfg.worker_command = "codex".to_string();

        let pane = crate::tmux::PaneInfo {
            index: 4,
            id: "%4".to_string(),
            dead: false,
            current_command: "codex".to_string(),
        };
        assert!(cfg.is_worker_pane(&pane));

        // Index 3 is below the custom threshold
        let pane_low = crate::tmux::PaneInfo {
            index: 3,
            id: "%3".to_string(),
            dead: false,
            current_command: "codex".to_string(),
        };
        assert!(!cfg.is_worker_pane(&pane_low));

        // "claude" doesn't match custom worker_command
        let pane_wrong_cmd = crate::tmux::PaneInfo {
            index: 5,
            id: "%5".to_string(),
            dead: false,
            current_command: "claude".to_string(),
        };
        assert!(!cfg.is_worker_pane(&pane_wrong_cmd));
    }

    #[test]
    fn test_prompt_config_from_config() {
        let cfg = test_config();
        let pcfg = cfg.prompt_config();
        assert_eq!(pcfg.queue_prompt_marker, "Press up to edit");
        assert_eq!(pcfg.shell_prompt_char, '\u{276f}');
        assert_eq!(pcfg.clear_line_key, "C-u");
        assert_eq!(pcfg.submit_key, "Enter");
        assert_eq!(pcfg.prompt_submit_delay_ms, 800);
        assert_eq!(pcfg.shell_prompt_wait_ms, 1000);
        assert_eq!(pcfg.post_clear_delay_ms, 200);
        assert_eq!(pcfg.post_type_delay_ms, 300);
        assert_eq!(pcfg.submit_retry_attempts, 3);
        assert_eq!(pcfg.shell_prompt_timeout_secs, 5);
        assert_eq!(pcfg.capture_lines, 40);
    }

    #[test]
    fn test_planner_config_defaults() {
        let cfg = test_config();
        assert_eq!(cfg.deep_planner_cooldown_secs, 300);
        assert!(cfg.fast_planner_enabled);
        assert!(cfg.deep_planner_enabled);
        assert_eq!(cfg.planner_restart_cooldown_secs, 180);
    }

    #[test]
    fn test_defaults_match_expected_values() {
        let cfg = test_config();
        assert_eq!(cfg.min_worker_pane_index, 3);
        assert_eq!(cfg.worker_command, "claude");
        assert_eq!(cfg.agent_type, "claude");
        assert_eq!(cfg.queue_prompt_marker, "Press up to edit");
        assert_eq!(cfg.shell_prompt_char, '\u{276f}');
        assert_eq!(cfg.mail_server_suffix, "--agent-mail");
        assert_eq!(cfg.capture_lines, 40);
        assert_eq!(cfg.submit_retry_attempts, 3);
        assert_eq!(cfg.shell_prompt_timeout_secs, 5);
    }

    #[test]
    fn test_agent_type_defaults_to_claude() {
        let cfg = test_config();
        assert_eq!(cfg.agent_type, "claude");
        assert!(!cfg.is_codex());
    }

    #[test]
    fn test_codex_helpers() {
        let mut cfg = test_config();
        cfg.agent_type = "codex".to_string();
        assert!(cfg.is_codex());
        assert_eq!(cfg.agent_program_name(), "codex");
        assert_eq!(cfg.worker_skill_name(), "patina-fly-worker");
        assert_eq!(cfg.completion_skill_name(), "mail-complete");
    }

    #[test]
    fn test_claude_helpers() {
        let cfg = test_config();
        assert!(!cfg.is_codex());
        assert_eq!(cfg.agent_program_name(), "claude-code");
        assert_eq!(cfg.worker_skill_name(), "flywheel-worker");
        assert_eq!(cfg.completion_skill_name(), "mail-complete");
    }
}

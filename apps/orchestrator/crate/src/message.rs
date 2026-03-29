use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

use crate::mail::InboxMessage;

#[derive(Debug, Clone)]
pub struct ParsedCompletion {
    pub msg_id: Option<i64>,
    pub bead_id: String,
    pub worker: String,
    pub subject: String,
    pub ack_required: bool,
    pub files_changed: String,
    pub tests_run: String,
    pub files_ok: bool,
    pub tests_ok: bool,
    pub structured: bool,
}

#[derive(Debug)]
pub struct CompletionSet {
    pub completions: Vec<ParsedCompletion>,
    pub stale_ids: Vec<i64>,
}

pub struct StructuredPayload {
    pub bead_id: String,
    pub files_changed: Vec<String>,
    pub tests_run: Vec<String>,
}

// ---------- regex singletons ----------

fn re_subject_completion() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\[(pat-[a-z0-9]+)\]\s+(complete|already complete)\b").unwrap()
    })
}

fn re_thread_bead() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^pat-[a-z0-9]+$").unwrap())
}

fn re_bead_id_from_subject() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[(pat-[a-z0-9]+)\]|(pat-[a-z0-9]+)").unwrap())
}

fn re_fenced_block() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)```(?:json)?\s*(.*?)```").unwrap())
}

// ---------- public helpers ----------

/// Return false for placeholder / empty text.
pub fn is_meaningful(value: &str) -> bool {
    let raw = value.trim();
    if raw.is_empty() {
        return false;
    }
    let lowered = raw
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();

    const WEAK: &[&str] = &[
        "...", "n/a", "na", "none", "not run", "not ran", "unknown", "tbd", "todo", "pending",
        "-", "--",
    ];
    if WEAK.contains(&lowered.as_str()) {
        return false;
    }
    if lowered.starts_with("...") {
        return false;
    }
    if lowered == "files changed: ..." || lowered == "tests run: ..." {
        return false;
    }
    true
}

/// Extract content after "Label: content" or "## Label\ncontent" patterns.
pub fn extract_section(body: &str, label: &str) -> String {
    let escaped = regex::escape(label);

    // Try "Label: content" first — content runs until next "; Key:" or end
    let inline_pat = format!(
        r"(?is){}\s*:\s*(.+?)(?:\s*;\s*[A-Za-z][^:]*:\s|$)",
        escaped
    );
    if let Ok(re) = Regex::new(&inline_pat) {
        if let Some(caps) = re.captures(body) {
            if let Some(cap) = caps.get(1) {
                let v = cap.as_str().trim();
                if !v.is_empty() {
                    return v.to_string();
                }
            }
        }
    }

    // Try markdown heading "## Label\ncontent"
    // Rust regex doesn't support lookahead, so we match up to next heading or end.
    let heading_pat = format!(
        r"(?is)(?:^|\n)\s*#+\s*{}\s*\n(.+?)(?:\n\s*#+\s|$)",
        escaped
    );
    if let Ok(re) = Regex::new(&heading_pat) {
        if let Some(m) = re.captures(body) {
            if let Some(cap) = m.get(1) {
                let v = cap.as_str().trim();
                if !v.is_empty() {
                    return v.to_string();
                }
            }
        }
    }

    String::new()
}

/// Find fenced ```json blocks containing `test_commands` or `files_changed`.
pub fn parse_structured_payload(body: &str, fallback_bead_id: &str) -> Option<StructuredPayload> {
    let re = re_fenced_block();
    for cap in re.captures_iter(body) {
        let candidate = cap.get(1)?.as_str().trim();
        if !candidate.starts_with('{') {
            continue;
        }
        let payload: serde_json::Value = match serde_json::from_str(candidate) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let obj = payload.as_object()?;
        if !obj.contains_key("test_commands") && !obj.contains_key("files_changed") {
            continue;
        }

        let bead_id = payload
            .get("bead_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(fallback_bead_id)
            .to_string();

        let files_changed = json_string_or_array(&payload, "files_changed");
        let tests_run = json_string_or_array(&payload, "test_commands");

        return Some(StructuredPayload {
            bead_id,
            files_changed,
            tests_run,
        });
    }
    None
}

/// Filter inbox to completion messages, parse, and deduplicate.
pub fn parse_completions(inbox: &[InboxMessage]) -> CompletionSet {
    let re_subj = re_subject_completion();
    let re_thread = re_thread_bead();
    let re_bead = re_bead_id_from_subject();

    let mut raw: Vec<ParsedCompletion> = Vec::new();

    for msg in inbox {
        let topic = msg.topic.as_deref().unwrap_or("");
        let subject = msg.subject.as_deref().unwrap_or("");
        let thread_id = msg.thread_id.as_deref().unwrap_or("");

        let subject_is_completion = re_subj.is_match(subject);
        let thread_is_bead = re_thread.is_match(thread_id);
        let completion_like =
            topic == "bead-complete" || subject_is_completion || thread_is_bead;

        if !completion_like || msg.is_acknowledged() {
            continue;
        }

        // Extract bead ID
        let mut bead_id = String::new();
        if let Some(caps) = re_bead.captures(subject) {
            bead_id = caps
                .get(1)
                .or_else(|| caps.get(2))
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
        }
        if bead_id.is_empty() && thread_is_bead {
            bead_id = thread_id.to_string();
        }

        let body = msg.body_text();

        let (files_changed, tests_run, structured) =
            if let Some(sp) = parse_structured_payload(body, &bead_id) {
                if !sp.bead_id.is_empty() {
                    bead_id = sp.bead_id;
                }
                (sp.files_changed.join("\n"), sp.tests_run.join("\n"), true)
            } else {
                let fc = extract_section(body, "Files changed");
                let tr = extract_section(body, "Tests run");
                (fc, tr, false)
            };

        raw.push(ParsedCompletion {
            msg_id: msg.id,
            bead_id,
            worker: msg.sender().to_string(),
            subject: subject.to_string(),
            ack_required: msg.ack_required.unwrap_or(false),
            files_ok: is_meaningful(&files_changed),
            tests_ok: is_meaningful(&tests_run),
            files_changed,
            tests_run,
            structured,
        });
    }

    // Deduplicate by (bead_id, worker) keeping highest-scored
    let mut latest: HashMap<(String, String), ParsedCompletion> = HashMap::new();
    let mut stale_ids: Vec<i64> = Vec::new();

    for item in raw {
        let key = (item.bead_id.clone(), item.worker.clone());
        let item_score = score(&item);

        if let Some(current) = latest.get(&key) {
            let current_score = score(current);
            if item_score >= current_score {
                if let Some(id) = current.msg_id {
                    stale_ids.push(id);
                }
                latest.insert(key, item);
            } else {
                if let Some(id) = item.msg_id {
                    stale_ids.push(id);
                }
            }
        } else {
            latest.insert(key, item);
        }
    }

    let mut completions: Vec<ParsedCompletion> = latest.into_values().collect();
    completions.sort_by(|a, b| {
        let a_id = a.msg_id.unwrap_or(0);
        let b_id = b.msg_id.unwrap_or(0);
        b_id.cmp(&a_id)
    });

    CompletionSet {
        completions,
        stale_ids,
    }
}

// ---------- private helpers ----------

fn score(c: &ParsedCompletion) -> (u8, u8, u8, i64) {
    (
        c.tests_ok as u8,
        c.files_ok as u8,
        c.structured as u8,
        c.msg_id.unwrap_or(0),
    )
}

fn json_string_or_array(val: &serde_json::Value, key: &str) -> Vec<String> {
    match val.get(key) {
        None => Vec::new(),
        Some(v) if v.is_string() => {
            let s = v.as_str().unwrap().trim().to_string();
            if s.is_empty() {
                Vec::new()
            } else {
                vec![s]
            }
        }
        Some(v) if v.is_array() => v
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| {
                let s = match item {
                    serde_json::Value::String(s) => s.trim().to_string(),
                    other => {
                        let s = other.to_string().trim().to_string();
                        if s.is_empty() {
                            return None;
                        }
                        s
                    }
                };
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_msg(overrides: impl FnOnce(&mut InboxMessage)) -> InboxMessage {
        let mut msg = InboxMessage {
            id: Some(1),
            topic: None,
            subject: None,
            sender_name: None,
            from_name: None,
            body_md: None,
            body: None,
            thread_id: None,
            ack_required: None,
            read_at: None,
            acknowledged_at: None,
            acked_at: None,
        };
        overrides(&mut msg);
        msg
    }

    #[test]
    fn test_is_meaningful() {
        // True for real content
        assert!(is_meaningful("src/main.rs"));
        assert!(is_meaningful("cargo test --workspace"));
        assert!(is_meaningful("engine-rs/crates/gdcore/src/lib.rs\nengine-rs/Cargo.toml"));

        // False for placeholders
        assert!(!is_meaningful(""));
        assert!(!is_meaningful("   "));
        assert!(!is_meaningful("..."));
        assert!(!is_meaningful("n/a"));
        assert!(!is_meaningful("N/A"));
        assert!(!is_meaningful("na"));
        assert!(!is_meaningful("none"));
        assert!(!is_meaningful("None"));
        assert!(!is_meaningful("not run"));
        assert!(!is_meaningful("Not Run"));
        assert!(!is_meaningful("not ran"));
        assert!(!is_meaningful("unknown"));
        assert!(!is_meaningful("tbd"));
        assert!(!is_meaningful("TBD"));
        assert!(!is_meaningful("todo"));
        assert!(!is_meaningful("pending"));
        assert!(!is_meaningful("-"));
        assert!(!is_meaningful("--"));
        assert!(!is_meaningful("... some trailing"));
        assert!(!is_meaningful("Files changed: ..."));
        assert!(!is_meaningful("Tests run: ..."));
    }

    #[test]
    fn test_extract_section() {
        let body = "Files changed: foo.rs\nbar.rs; Tests run: cargo test";
        assert_eq!(extract_section(body, "Files changed"), "foo.rs\nbar.rs");
        assert_eq!(extract_section(body, "Tests run"), "cargo test");

        // Markdown heading style
        let body2 = "## Files changed\nfoo.rs\nbar.rs\n## Tests run\ncargo test --workspace";
        assert_eq!(extract_section(body2, "Files changed"), "foo.rs\nbar.rs");
        assert_eq!(extract_section(body2, "Tests run"), "cargo test --workspace");

        // Missing section
        assert_eq!(extract_section(body, "Summary"), "");
        assert_eq!(extract_section("", "Files changed"), "");
    }

    #[test]
    fn test_parse_structured_payload() {
        let body = r#"Here is the result:
```json
{
    "bead_id": "pat-abc123",
    "files_changed": ["src/main.rs", "Cargo.toml"],
    "test_commands": ["cargo test --workspace"]
}
```
"#;
        let sp = parse_structured_payload(body, "pat-fallback").unwrap();
        assert_eq!(sp.bead_id, "pat-abc123");
        assert_eq!(sp.files_changed, vec!["src/main.rs", "Cargo.toml"]);
        assert_eq!(sp.tests_run, vec!["cargo test --workspace"]);

        // Fallback bead ID when not in payload
        let body2 = r#"```json
{"files_changed": ["a.rs"], "test_commands": ["cargo test"]}
```"#;
        let sp2 = parse_structured_payload(body2, "pat-fallback").unwrap();
        assert_eq!(sp2.bead_id, "pat-fallback");

        // No structured payload
        assert!(parse_structured_payload("just plain text", "pat-x").is_none());

        // JSON block without required fields
        let body3 = r#"```json
{"status": "ok"}
```"#;
        assert!(parse_structured_payload(body3, "pat-x").is_none());

        // String values (not arrays)
        let body4 = r#"```json
{"files_changed": "single.rs", "test_commands": "cargo test"}
```"#;
        let sp4 = parse_structured_payload(body4, "pat-x").unwrap();
        assert_eq!(sp4.files_changed, vec!["single.rs"]);
        assert_eq!(sp4.tests_run, vec!["cargo test"]);
    }

    #[test]
    fn test_parse_completions_dedup() {
        let msgs = vec![
            make_msg(|m| {
                m.id = Some(10);
                m.topic = Some("bead-complete".into());
                m.subject = Some("[pat-abc] complete".into());
                m.sender_name = Some("Worker1".into());
                m.body_md = Some("Files changed: ...\nTests run: ...".into());
            }),
            make_msg(|m| {
                m.id = Some(20);
                m.topic = Some("bead-complete".into());
                m.subject = Some("[pat-abc] complete".into());
                m.sender_name = Some("Worker1".into());
                m.body_md = Some(
                    r#"```json
{"files_changed": ["src/lib.rs"], "test_commands": ["cargo test"]}
```"#
                    .into(),
                );
            }),
        ];

        let result = parse_completions(&msgs);
        assert_eq!(result.completions.len(), 1);
        assert_eq!(result.completions[0].msg_id, Some(20));
        assert!(result.completions[0].structured);
        assert!(result.completions[0].files_ok);
        assert!(result.completions[0].tests_ok);
        assert_eq!(result.stale_ids, vec![10]);
    }

    #[test]
    fn test_bead_id_from_subject() {
        // [pat-xxx] style
        let msgs = vec![make_msg(|m| {
            m.id = Some(1);
            m.topic = Some("bead-complete".into());
            m.subject = Some("[pat-abc123] complete".into());
            m.sender_name = Some("W".into());
        })];
        let result = parse_completions(&msgs);
        assert_eq!(result.completions[0].bead_id, "pat-abc123");

        // thread_id fallback
        let msgs2 = vec![make_msg(|m| {
            m.id = Some(2);
            m.thread_id = Some("pat-def456".into());
            m.sender_name = Some("W".into());
        })];
        let result2 = parse_completions(&msgs2);
        assert_eq!(result2.completions[0].bead_id, "pat-def456");
    }

    #[test]
    fn test_empty_inbox() {
        let result = parse_completions(&[]);
        assert!(result.completions.is_empty());
        assert!(result.stale_ids.is_empty());
    }

    #[test]
    fn test_acknowledged_messages_skipped() {
        let msgs = vec![make_msg(|m| {
            m.id = Some(1);
            m.topic = Some("bead-complete".into());
            m.subject = Some("[pat-abc] complete".into());
            m.acknowledged_at = Some("2026-01-01T00:00:00Z".into());
        })];
        let result = parse_completions(&msgs);
        assert!(result.completions.is_empty());
    }

    #[test]
    fn test_non_completion_messages_skipped() {
        let msgs = vec![make_msg(|m| {
            m.id = Some(1);
            m.topic = Some("general".into());
            m.subject = Some("Hello world".into());
        })];
        let result = parse_completions(&msgs);
        assert!(result.completions.is_empty());
    }
}

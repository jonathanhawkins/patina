//! Park review — the coordinator's periodic revisit of every bead that is
//! excluded from assignment: `status='blocked'` beads and beads carrying the
//! `needs-attention` / `env-blocked` labels.
//!
//! All three are one-way doors without this pass: the ready query only looks
//! at `status='open'` beads without parking labels, and nothing else in the
//! system ever flips them back. This pass makes parking self-healing where it
//! can be mechanical, and keeps a human-visible ledger where it can't:
//!
//! - **Stale block** (blocking deps exist, all blockers closed): the park
//!   reason is satisfied — auto-reopen with a comment.
//! - **No-reason park** (`blocked`, zero blocking deps, zero comments): a park
//!   indistinguishable from an accident — reopen ONCE ever (state-tracked, and
//!   the reopen comment itself makes any future re-park a commented one), ask
//!   for a comment if it was deliberate. Bounded, so it cannot restart the
//!   reassign livelock deliberate parks exist to prevent.
//! - **Deliberate park** (`blocked`, zero deps, has comments): never touched —
//!   surfaced in the operator digest with age and the recorded reason.
//! - **Fail-streak park** (`needs-attention`): retried after a cooldown by
//!   clearing the label, a bounded number of times; the verifier re-parks it
//!   if it still fails. Exhausted retries land in the operator digest.
//! - **Env-blocked**: needs the environment/operator — digest only.
//! - **Live block** (open blockers remain): healthy — the ready query would
//!   still exclude it if it were open; counted in the digest.
//!
//! The digest is logged every pass but boss-notified only when its *stable
//! signature* changes (bead set / classes / retry counts — ages excluded), so
//! a static park population doesn't ping the operator every interval.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::br;
use crate::coordinator::Coordinator;
use crate::db::{self, ParkSource, ParkedBead};

/// What kind of park this bead is in, derived purely from its row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParkClass {
    /// `blocked` status, ≥1 blocking dep, every blocker closed/deleted.
    StaleBlock { total: i64 },
    /// `blocked` status with blockers still open — genuinely blocked.
    LiveBlock { open: i64, total: i64 },
    /// `blocked` status, zero blocking deps — a worker parked it on purpose
    /// (`has_comment`) or without recording why (`!has_comment`).
    DeliberatePark { has_comment: bool },
    /// `needs-attention` fail-streak park.
    FailStreak,
    /// `env-blocked` label.
    EnvBlocked,
}

/// What the pass should do about one parked bead this cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParkAction {
    /// Flip status back to open (clearing assignee) with the given reason.
    Reopen { reason: String },
    /// Remove the needs-attention label so the bead re-enters the queue.
    ClearNeedsAttention { retry: u32, max: u32 },
    /// Nothing mechanical to do — report in the digest.
    DigestOnly,
}

pub fn classify(bead: &ParkedBead) -> ParkClass {
    match bead.source {
        ParkSource::NeedsAttention => ParkClass::FailStreak,
        ParkSource::EnvBlocked => ParkClass::EnvBlocked,
        ParkSource::BlockedStatus => {
            if bead.blocking_deps_total == 0 {
                ParkClass::DeliberatePark {
                    has_comment: bead.comment_count > 0,
                }
            } else if bead.blocking_deps_open == 0 {
                ParkClass::StaleBlock {
                    total: bead.blocking_deps_total,
                }
            } else {
                ParkClass::LiveBlock {
                    open: bead.blocking_deps_open,
                    total: bead.blocking_deps_total,
                }
            }
        }
    }
}

/// Per-bead memory across passes (persisted JSON).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BeadReviewState {
    /// Epoch seconds when the pass first saw this bead parked.
    pub first_seen: i64,
    /// needs-attention retries performed so far.
    pub retries: u32,
    /// Epoch seconds of the last needs-attention retry.
    pub last_retry: Option<i64>,
    /// How many times a no-reason `blocked` was reopened (capped at 1).
    pub noreason_reopens: u32,
    /// Epoch seconds when the pass last saw this bead parked (for pruning).
    pub last_seen: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewState {
    pub beads: BTreeMap<String, BeadReviewState>,
    /// Stable signature of the last digest that was boss-notified.
    pub last_signature: Option<String>,
}

/// Entries unseen for this long are dropped from the state file.
const STATE_PRUNE_SECS: i64 = 30 * 24 * 3600;

pub fn state_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("park_review.json")
}

pub fn load_state(path: &Path) -> ReviewState {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_state(path: &Path, state: &ReviewState) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(path, json);
    }
}

/// Pure decision: given the class and this bead's history, what happens now.
pub fn decide(
    class: &ParkClass,
    state: &BeadReviewState,
    now_epoch: i64,
    retry_cooldown_secs: u64,
    retry_max: u32,
) -> ParkAction {
    match class {
        ParkClass::StaleBlock { total } => ParkAction::Reopen {
            reason: format!(
                "all {total} blocking dependencies are closed; the blocked status is \
                 never recomputed, so this bead would have stayed invisible forever"
            ),
        },
        ParkClass::DeliberatePark { has_comment: false } if state.noreason_reopens == 0 => {
            ParkAction::Reopen {
                reason: "blocked with zero blocking dependencies and zero comments — \
                         no recorded park reason. Reopening once; if this park was \
                         deliberate, set --status blocked again WITH a comment naming why"
                    .to_string(),
            }
        }
        ParkClass::FailStreak => {
            if retry_max == 0 || state.retries >= retry_max {
                return ParkAction::DigestOnly;
            }
            let anchor = state.last_retry.unwrap_or(state.first_seen).max(state.first_seen);
            if now_epoch.saturating_sub(anchor) >= retry_cooldown_secs as i64 {
                ParkAction::ClearNeedsAttention {
                    retry: state.retries + 1,
                    max: retry_max,
                }
            } else {
                ParkAction::DigestOnly
            }
        }
        _ => ParkAction::DigestOnly,
    }
}

/// One reviewed bead: what it is and what we did.
#[derive(Debug, Clone)]
pub struct ReviewedBead {
    pub bead: ParkedBead,
    pub class: ParkClass,
    pub action: ParkAction,
    /// Whether the action's br calls all succeeded (true for DigestOnly).
    pub applied: bool,
    /// FailStreak only: retries are used up, so it needs an operator now.
    pub exhausted: bool,
}

#[derive(Debug, Default)]
pub struct ReviewOutcome {
    pub reviewed: Vec<ReviewedBead>,
    pub digest: String,
    pub notified: bool,
}

/// Stable identity of the parked population — bead ids, classes, and retry
/// counts, but NOT ages — so the boss is re-notified only on real change.
pub fn signature(reviewed: &[ReviewedBead], state: &ReviewState) -> String {
    let mut parts: Vec<String> = reviewed
        .iter()
        .map(|r| {
            let retries = state.beads.get(&r.bead.id).map(|s| s.retries).unwrap_or(0);
            format!("{}:{}:{}", r.bead.id, discriminant_tag(&r.class), retries)
        })
        .collect();
    parts.sort();
    parts.join(",")
}

fn discriminant_tag(class: &ParkClass) -> &'static str {
    match class {
        ParkClass::StaleBlock { .. } => "stale",
        ParkClass::LiveBlock { .. } => "live",
        ParkClass::DeliberatePark { has_comment: true } => "park",
        ParkClass::DeliberatePark { has_comment: false } => "park-noreason",
        ParkClass::FailStreak => "failstreak",
        ParkClass::EnvBlocked => "env",
    }
}

fn first_line(text: &str, max_chars: usize) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let mut s: String = line.trim().chars().take(max_chars).collect();
    if line.trim().chars().count() > max_chars {
        s.push('…');
    }
    s
}

/// Render the operator digest. Counts on top, then the beads a human must
/// decide about (deliberate parks + exhausted fail-streaks), highest priority
/// first, capped so the boss message stays readable.
pub fn render_digest(reviewed: &[ReviewedBead]) -> String {
    let mut stale = 0usize;
    let mut live = 0usize;
    let mut deliberate = 0usize;
    let mut noreason = 0usize;
    let mut fail_retried = 0usize;
    let mut fail_waiting = 0usize;
    let mut fail_exhausted = 0usize;
    let mut env = 0usize;
    let mut operator_items: Vec<&ReviewedBead> = Vec::new();

    for r in reviewed {
        match (&r.class, &r.action) {
            (ParkClass::StaleBlock { .. }, _) => stale += 1,
            (ParkClass::LiveBlock { .. }, _) => live += 1,
            (ParkClass::DeliberatePark { has_comment: false }, ParkAction::Reopen { .. }) => {
                noreason += 1
            }
            (ParkClass::DeliberatePark { .. }, _) => {
                deliberate += 1;
                operator_items.push(r);
            }
            (ParkClass::FailStreak, ParkAction::ClearNeedsAttention { .. }) => fail_retried += 1,
            (ParkClass::FailStreak, _) => {
                // Exhausted vs still cooling down: exhausted ones need a human.
                if r.exhausted {
                    fail_exhausted += 1;
                    operator_items.push(r);
                } else {
                    fail_waiting += 1;
                }
            }
            (ParkClass::EnvBlocked, _) => env += 1,
        }
    }

    let mut out = format!(
        "park review: {} blocked ({stale} stale→reopened, {live} live, {deliberate} deliberate, \
         {noreason} no-reason→reopened), {} needs-attention ({fail_retried} retried, \
         {fail_waiting} cooling down, {fail_exhausted} retries exhausted), {env} env-blocked",
        stale + live + deliberate + noreason,
        fail_retried + fail_waiting + fail_exhausted,
    );

    if !operator_items.is_empty() {
        operator_items.sort_by_key(|r| (r.bead.priority, r.bead.id.clone()));
        out.push_str("\noperator decision needed:");
        for r in operator_items.iter().take(10) {
            let reason = r
                .bead
                .last_comment
                .as_deref()
                .map(|c| first_line(c, 90))
                .unwrap_or_else(|| "<no comment>".to_string());
            out.push_str(&format!(
                "\n  {} P{} {:.0}d [{}] — {}",
                r.bead.id,
                r.bead.priority,
                r.bead.age_days,
                discriminant_tag(&r.class),
                reason
            ));
        }
        if operator_items.len() > 10 {
            out.push_str(&format!("\n  … and {} more", operator_items.len() - 10));
        }
    }
    out
}

/// Run one park-review pass: query, classify, act (via br CLI only), digest,
/// notify on change, persist state. Returns None if the DB can't be read.
pub fn run_park_review(coord: &Coordinator) -> Option<ReviewOutcome> {
    let now_epoch = chrono::Utc::now().timestamp();
    let cooldown = coord.config.park_retry_cooldown_seconds;
    let retry_max = coord.config.park_retry_max;
    let path = state_path(&coord.config.cache_dir());
    let mut state = load_state(&path);

    // Collect rows with a short-lived read connection, dropped BEFORE any br
    // subprocess writes (a held reader deadlocks the child writer).
    let parked: Vec<ParkedBead> = {
        let conn = db::open(&coord.config.project_root).ok()?;
        let mut all = db::parked_blocked_beads(&conn).ok()?;
        all.extend(db::parked_labeled_beads(&conn).ok()?);
        all
    };

    let mut outcome = ReviewOutcome::default();
    for bead in parked {
        let entry = state.beads.entry(bead.id.clone()).or_insert_with(|| BeadReviewState {
            first_seen: now_epoch,
            last_seen: now_epoch,
            ..Default::default()
        });
        entry.last_seen = now_epoch;
        let class = classify(&bead);
        let action = decide(&class, entry, now_epoch, cooldown, retry_max);
        let exhausted = matches!(class, ParkClass::FailStreak)
            && (retry_max == 0 || entry.retries >= retry_max);

        let mut applied = true;
        if !coord.is_dry_run() {
            match &action {
                ParkAction::Reopen { reason } => {
                    let comment = format!("park-review (coordinator): reopening — {reason}");
                    let ok = br::update_bead(&bead.id, Some(""), Some("open")).is_ok();
                    if ok {
                        let _ = br::comment_add(&bead.id, &comment);
                        if matches!(class, ParkClass::DeliberatePark { has_comment: false }) {
                            entry.noreason_reopens += 1;
                        }
                        tracing::info!(bead = %bead.id, reason = %reason, "park review: reopened");
                    } else {
                        applied = false;
                        tracing::warn!(bead = %bead.id, "park review: reopen failed; will retry next pass");
                    }
                }
                ParkAction::ClearNeedsAttention { retry, max } => {
                    let ok = br::label_remove(&bead.id, db::NEEDS_ATTENTION_LABEL).is_ok();
                    if ok {
                        entry.retries = *retry;
                        entry.last_retry = Some(now_epoch);
                        let _ = br::comment_add(
                            &bead.id,
                            &format!(
                                "park-review (coordinator): cooldown elapsed — clearing \
                                 needs-attention for retry {retry}/{max}. The verifier \
                                 re-parks it if it keeps failing."
                            ),
                        );
                        tracing::info!(bead = %bead.id, retry, max, "park review: needs-attention cleared for retry");
                    } else {
                        applied = false;
                        tracing::warn!(bead = %bead.id, "park review: label remove failed; will retry next pass");
                    }
                }
                ParkAction::DigestOnly => {}
            }
        }

        outcome.reviewed.push(ReviewedBead {
            bead,
            class,
            action,
            applied,
            exhausted,
        });
    }

    // Prune long-gone entries so the state file doesn't grow forever.
    state
        .beads
        .retain(|_, s| now_epoch.saturating_sub(s.last_seen) < STATE_PRUNE_SECS);

    outcome.digest = render_digest(&outcome.reviewed);
    let sig = signature(&outcome.reviewed, &state);
    let acted = outcome
        .reviewed
        .iter()
        .any(|r| r.applied && !matches!(r.action, ParkAction::DigestOnly));
    let changed = state.last_signature.as_deref() != Some(sig.as_str());
    if (acted || changed) && !outcome.reviewed.is_empty() {
        coord.notify_boss(&outcome.digest);
        outcome.notified = true;
    }
    state.last_signature = Some(sig);
    save_state(&path, &state);

    println!("{}", outcome.digest);
    Some(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bead(id: &str, source: ParkSource) -> ParkedBead {
        ParkedBead {
            id: id.to_string(),
            title: format!("title {id}"),
            priority: 1,
            source,
            blocking_deps_total: 0,
            blocking_deps_open: 0,
            age_days: 10.0,
            comment_count: 0,
            last_comment: None,
        }
    }

    fn blocked(id: &str, total: i64, open: i64, comments: i64) -> ParkedBead {
        let mut b = bead(id, ParkSource::BlockedStatus);
        b.blocking_deps_total = total;
        b.blocking_deps_open = open;
        b.comment_count = comments;
        if comments > 0 {
            b.last_comment = Some("BLOCKED: parked on purpose\nsecond line".to_string());
        }
        b
    }

    // --- classify ---

    #[test]
    fn classify_stale_block_when_all_blockers_closed() {
        assert_eq!(
            classify(&blocked("pat-1", 2, 0, 0)),
            ParkClass::StaleBlock { total: 2 }
        );
    }

    #[test]
    fn classify_live_block_when_blockers_open() {
        assert_eq!(
            classify(&blocked("pat-1", 3, 1, 5)),
            ParkClass::LiveBlock { open: 1, total: 3 }
        );
    }

    #[test]
    fn classify_deliberate_park_by_comment_presence() {
        assert_eq!(
            classify(&blocked("pat-1", 0, 0, 2)),
            ParkClass::DeliberatePark { has_comment: true }
        );
        assert_eq!(
            classify(&blocked("pat-1", 0, 0, 0)),
            ParkClass::DeliberatePark { has_comment: false }
        );
    }

    #[test]
    fn classify_labels() {
        assert_eq!(
            classify(&bead("pat-1", ParkSource::NeedsAttention)),
            ParkClass::FailStreak
        );
        assert_eq!(
            classify(&bead("pat-1", ParkSource::EnvBlocked)),
            ParkClass::EnvBlocked
        );
    }

    // --- decide ---

    const COOLDOWN: u64 = 7200;
    const MAX: u32 = 3;

    fn st(first_seen: i64) -> BeadReviewState {
        BeadReviewState {
            first_seen,
            last_seen: first_seen,
            ..Default::default()
        }
    }

    #[test]
    fn decide_stale_block_always_reopens() {
        let a = decide(&ParkClass::StaleBlock { total: 2 }, &st(0), 1, COOLDOWN, MAX);
        assert!(matches!(a, ParkAction::Reopen { .. }));
    }

    #[test]
    fn decide_live_block_and_env_are_digest_only() {
        for class in [
            ParkClass::LiveBlock { open: 1, total: 2 },
            ParkClass::EnvBlocked,
        ] {
            assert_eq!(
                decide(&class, &st(0), 1_000_000, COOLDOWN, MAX),
                ParkAction::DigestOnly
            );
        }
    }

    #[test]
    fn decide_no_reason_park_reopens_exactly_once() {
        let class = ParkClass::DeliberatePark { has_comment: false };
        let fresh = st(0);
        assert!(matches!(
            decide(&class, &fresh, 1, COOLDOWN, MAX),
            ParkAction::Reopen { .. }
        ));
        // After one reopen it becomes hands-off forever (livelock guard).
        let mut used = st(0);
        used.noreason_reopens = 1;
        assert_eq!(decide(&class, &used, 1, COOLDOWN, MAX), ParkAction::DigestOnly);
    }

    #[test]
    fn decide_commented_park_is_never_touched() {
        let class = ParkClass::DeliberatePark { has_comment: true };
        assert_eq!(
            decide(&class, &st(0), i64::MAX, COOLDOWN, MAX),
            ParkAction::DigestOnly
        );
    }

    #[test]
    fn decide_fail_streak_waits_out_cooldown_from_first_seen() {
        let state = st(1000);
        // 1s before the cooldown boundary: wait.
        assert_eq!(
            decide(
                &ParkClass::FailStreak,
                &state,
                1000 + COOLDOWN as i64 - 1,
                COOLDOWN,
                MAX
            ),
            ParkAction::DigestOnly
        );
        // At the boundary: retry 1.
        assert_eq!(
            decide(
                &ParkClass::FailStreak,
                &state,
                1000 + COOLDOWN as i64,
                COOLDOWN,
                MAX
            ),
            ParkAction::ClearNeedsAttention { retry: 1, max: MAX }
        );
    }

    #[test]
    fn decide_fail_streak_cooldown_anchors_on_last_retry() {
        let mut state = st(1000);
        state.retries = 1;
        state.last_retry = Some(50_000);
        // Cooldown counts from the retry, not first_seen.
        assert_eq!(
            decide(
                &ParkClass::FailStreak,
                &state,
                50_000 + COOLDOWN as i64 - 1,
                COOLDOWN,
                MAX
            ),
            ParkAction::DigestOnly
        );
        assert_eq!(
            decide(
                &ParkClass::FailStreak,
                &state,
                50_000 + COOLDOWN as i64,
                COOLDOWN,
                MAX
            ),
            ParkAction::ClearNeedsAttention { retry: 2, max: MAX }
        );
    }

    #[test]
    fn decide_fail_streak_respects_retry_cap_and_disable() {
        let mut exhausted = st(0);
        exhausted.retries = MAX;
        assert_eq!(
            decide(&ParkClass::FailStreak, &exhausted, i64::MAX, COOLDOWN, MAX),
            ParkAction::DigestOnly
        );
        // retry_max = 0 disables retries entirely.
        assert_eq!(
            decide(&ParkClass::FailStreak, &st(0), i64::MAX, COOLDOWN, 0),
            ParkAction::DigestOnly
        );
    }

    // --- state persistence ---

    #[test]
    fn state_roundtrip_and_corrupt_file_fallback() {
        let dir = std::env::temp_dir().join(format!("park-review-test-{}", std::process::id()));
        let path = state_path(&dir);

        let mut state = ReviewState::default();
        state.beads.insert(
            "pat-1".to_string(),
            BeadReviewState {
                first_seen: 100,
                retries: 2,
                last_retry: Some(200),
                noreason_reopens: 1,
                last_seen: 300,
            },
        );
        state.last_signature = Some("sig".to_string());
        save_state(&path, &state);

        let loaded = load_state(&path);
        assert_eq!(loaded.beads.get("pat-1"), state.beads.get("pat-1"));
        assert_eq!(loaded.last_signature.as_deref(), Some("sig"));

        // Corrupt file → clean default, no panic.
        std::fs::write(&path, "{not json").unwrap();
        let recovered = load_state(&path);
        assert!(recovered.beads.is_empty());
        assert!(recovered.last_signature.is_none());

        // Missing file → default.
        let missing = load_state(&dir.join("nope.json"));
        assert!(missing.beads.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- digest + signature ---

    fn reviewed(bead: ParkedBead, class: ParkClass, action: ParkAction, exhausted: bool) -> ReviewedBead {
        ReviewedBead {
            bead,
            class,
            action,
            applied: true,
            exhausted,
        }
    }

    #[test]
    fn digest_counts_and_operator_items() {
        let items = vec![
            reviewed(
                blocked("pat-a", 2, 0, 0),
                ParkClass::StaleBlock { total: 2 },
                ParkAction::Reopen {
                    reason: "x".into(),
                },
                false,
            ),
            reviewed(
                blocked("pat-b", 0, 0, 1),
                ParkClass::DeliberatePark { has_comment: true },
                ParkAction::DigestOnly,
                false,
            ),
            reviewed(
                bead("pat-c", ParkSource::NeedsAttention),
                ParkClass::FailStreak,
                ParkAction::DigestOnly,
                true, // exhausted → operator item
            ),
            reviewed(
                bead("pat-d", ParkSource::NeedsAttention),
                ParkClass::FailStreak,
                ParkAction::ClearNeedsAttention { retry: 1, max: 3 },
                false,
            ),
            reviewed(
                bead("pat-e", ParkSource::EnvBlocked),
                ParkClass::EnvBlocked,
                ParkAction::DigestOnly,
                false,
            ),
        ];
        let digest = render_digest(&items);
        assert!(
            digest.contains("2 blocked (1 stale→reopened, 0 live, 1 deliberate, 0 no-reason→reopened)"),
            "digest was: {digest}"
        );
        assert!(
            digest.contains("2 needs-attention (1 retried, 0 cooling down, 1 retries exhausted)"),
            "digest was: {digest}"
        );
        assert!(digest.contains("1 env-blocked"), "digest was: {digest}");
        // Operator list carries the park reason's first line, truncated.
        assert!(digest.contains("operator decision needed:"));
        assert!(digest.contains("pat-b"));
        assert!(digest.contains("BLOCKED: parked on purpose"));
        assert!(!digest.contains("second line"));
        assert!(digest.contains("pat-c"));
        // Non-operator items are not listed.
        assert!(!digest.contains("pat-d P"));
    }

    #[test]
    fn digest_empty_population() {
        let digest = render_digest(&[]);
        assert!(digest.contains("0 blocked"));
        assert!(!digest.contains("operator decision needed"));
    }

    #[test]
    fn signature_stable_and_age_independent() {
        let state = ReviewState::default();
        let mut a = blocked("pat-a", 0, 0, 1);
        a.age_days = 1.0;
        let items = vec![reviewed(
            a.clone(),
            ParkClass::DeliberatePark { has_comment: true },
            ParkAction::DigestOnly,
            false,
        )];
        let sig1 = signature(&items, &state);

        // Same population, different age → same signature (no boss re-ping).
        let mut b = a.clone();
        b.age_days = 25.0;
        let items2 = vec![reviewed(
            b,
            ParkClass::DeliberatePark { has_comment: true },
            ParkAction::DigestOnly,
            false,
        )];
        assert_eq!(sig1, signature(&items2, &state));

        // Retry-count change → different signature.
        let mut state2 = ReviewState::default();
        state2.beads.insert(
            "pat-a".to_string(),
            BeadReviewState {
                retries: 1,
                ..Default::default()
            },
        );
        assert_ne!(sig1, signature(&items, &state2));
    }

    #[test]
    fn first_line_skips_blanks_and_truncates() {
        assert_eq!(first_line("\n\n  real reason here\nmore", 90), "real reason here");
        let long = "x".repeat(120);
        let cut = first_line(&long, 90);
        assert_eq!(cut.chars().count(), 91); // 90 + ellipsis
        assert!(cut.ends_with('…'));
        assert_eq!(first_line("", 90), "");
    }
}

//! The sharded integration-test run in `.github/workflows/ci.yml`.
//!
//! CI used to run named "slice" jobs (headless / 2D / 3D / fuzz / CI meta),
//! each a `cargo test --workspace -- <name filters>`. Every one of them built
//! ALL ~1,250 integration-test binaries (~100 MB each), which overflowed the
//! runner's disk, so they were replaced by `rust-tests`: 12 shards, each running
//! `scripts/ci/nextest-shard.py <index> <count>` over its slice of the targets.
//!
//! What can silently go wrong now is coverage, not filters: if the matrix and
//! the script disagree on the shard count, whole shards of tests never run and
//! CI stays green. These tests pin the invariants that prevent that.

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn ci_yaml() -> String {
    std::fs::read_to_string(repo_root().join(".github/workflows/ci.yml"))
        .expect("ci.yml must exist")
}

/// The text of one top-level job, from its `  <name>:` line to the next job.
fn job(yaml: &str, name: &str) -> String {
    let header = format!("\n  {name}:\n");
    let start = yaml
        .find(&header)
        .unwrap_or_else(|| panic!("ci.yml must define job `{name}`"));
    let body = &yaml[start + header.len()..];
    let end = body
        .match_indices("\n  ")
        .find(|(i, _)| {
            let rest = &body[i + 3..];
            // A sibling job: two-space indent, then a name, then `:` + newline.
            !rest.starts_with(' ')
                && !rest.starts_with('#')
                && rest.split('\n').next().is_some_and(|l| l.ends_with(':'))
        })
        .map(|(i, _)| i)
        .unwrap_or(body.len());
    body[..end].to_string()
}

/// `(matrix shard list, count passed to the script)` for the shard job.
fn shard_config() -> (Vec<u32>, u32) {
    let shards = job(&ci_yaml(), "rust-tests");
    let list = shards
        .lines()
        .find_map(|l| l.trim().strip_prefix("shard: ["))
        .expect("rust-tests must have a `shard: [...]` matrix")
        .trim_end_matches(']')
        .split(',')
        .map(|n| n.trim().parse::<u32>().expect("shard numbers are integers"))
        .collect();
    let count = shards
        .lines()
        .find(|l| l.contains("nextest-shard.py"))
        .expect("rust-tests must run scripts/ci/nextest-shard.py")
        .split_whitespace()
        .last()
        .unwrap()
        .parse::<u32>()
        .expect("the script's last argument is the shard count");
    (list, count)
}

#[test]
fn shard_matrix_and_script_agree_on_the_shard_count() {
    let (list, count) = shard_config();
    assert!(
        count >= 2,
        "a single shard is the disk overflow this job exists to avoid"
    );
    assert_eq!(
        list,
        (1..=count).collect::<Vec<_>>(),
        "the matrix must list shards 1..={count} exactly once each, matching the \
         count passed to nextest-shard.py — otherwise some shards' tests never run"
    );
    assert!(
        job(&ci_yaml(), "rust-tests").contains("${{ matrix.shard }} - 1"),
        "the script takes a 0-based index: the job must pass `matrix.shard - 1`"
    );
}

#[test]
fn every_integration_target_lands_in_exactly_one_shard() {
    let (_, count) = shard_config();
    let script = repo_root().join("scripts/ci/nextest-shard.py");
    assert!(script.exists(), "{script:?} must exist");
    let output = match Command::new("python3")
        .arg(&script)
        .args(["--check", &count.to_string()])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
    {
        Ok(output) => output,
        Err(e) => {
            eprintln!("SKIP: python3 is not available here ({e}); CI runs this check");
            return;
        }
    };
    assert!(
        output.status.success(),
        "nextest-shard.py --check {count} failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let targets: usize = stdout
        .split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .expect("--check prints the target count first");
    assert!(
        targets > 1000,
        "only {targets} integration-test targets found — cargo metadata is not \
         seeing the workspace (expected ~1,250)"
    );
}

#[test]
fn shards_build_the_runner_binary_the_trace_tests_execute() {
    let shards = job(&ci_yaml(), "rust-tests");
    let build = shards.find("cargo build -p patina-runner").expect(
        "trace/oracle/headless tests exec target/debug/patina-runner; the shard must build it",
    );
    let run = shards.find("nextest-shard.py").unwrap();
    assert!(
        build < run,
        "patina-runner must be built BEFORE the shard's tests run"
    );
}

#[test]
fn the_required_rust_check_fails_when_any_shard_fails() {
    let rust = job(&ci_yaml(), "rust");
    assert!(
        rust.contains("needs: [rust-fmt, rust-tests]"),
        "`Rust (ubuntu-latest)` is the required check; it must wait for the shards"
    );
    assert!(
        rust.contains("always()"),
        "without always() a failed shard SKIPS the rust job, and branch \
         protection counts a skipped required check as passing"
    );
    assert!(
        rust.contains(r#"test "${{ needs.rust-tests.result }}" = "success""#),
        "the rust job must fail unless every shard succeeded"
    );
}

#[test]
fn the_removed_slice_jobs_stay_removed() {
    // Each would rebuild every test binary on one runner and overflow its disk;
    // the shards already run every test they selected.
    let yaml = ci_yaml();
    for gone in [
        "rust-compat-headless:",
        "rust-compat-2d:",
        "rust-compat-3d:",
        "rust-compat-fuzz:",
        "rust-compat-ci-meta:",
        "cargo test --workspace -- ",
        "cargo test $WS",
    ] {
        assert!(
            !yaml.contains(gone),
            "ci.yml must not contain `{gone}`: a workspace-wide test build overflows the runner disk"
        );
    }
}

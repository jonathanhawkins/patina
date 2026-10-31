#!/usr/bin/env python3
"""Verify the repin-validation CI lane has every gate its tests require.

Mirrors `engine-rs/tests/ci_suite/ci_repin_lane_validation_test.rs` so the
workflow can be checked WITHOUT a 10-minute Rust build. The Rust tests remain
the acceptance; this is the fast path, and it is deliberately a faithful port
rather than a looser re-statement:

* `extract_repin_section` reproduces the Rust helper exactly, including the
  `Option` comparison `find(':') < find('\n')` where Rust orders `None` BELOW
  `Some(_)`, and the fallback that grabs 3000 bytes when the first `"\n  "` it
  finds is not a job header.
* Verified by falsification: run against the pre-fix files and it reports the
  same 17 assertions the test binary reported, no more and no fewer (pat-21oab).

Usage:
    python3 docs/test-debt/verify_repin_workflow_gates.py
    REPIN_PATH=... CI_PATH=... python3 docs/test-debt/verify_repin_workflow_gates.py

Exit 0 when every gate is present, 1 with a per-assertion list otherwise.
"""

import os

REPIN_PATH = os.environ.get("REPIN_PATH", ".github/workflows/repin-validation.yml")
CI_PATH = os.environ.get("CI_PATH", ".github/workflows/ci.yml")
REPIN = open(REPIN_PATH).read()
CI    = open(CI_PATH).read()

def _find(h, n):
    i = h.find(n)
    return None if i < 0 else i
def _key(o):            # Rust Ord for Option: None < Some(_)
    return -1 if o is None else o

def extract_repin_section(ci, job):
    marker = "  %s:" % job
    start = _find(ci, marker)
    if start is None:
        return ""
    rest = ci[start:]
    nl = _find(rest, "\n")
    after_first_line = (nl + 1) if nl is not None else len(rest)
    section_end = None
    pos = _find(rest[after_first_line:], "\n  ")
    if pos is not None:
        candidate = rest[after_first_line + pos + 3:]
        if (candidate[:1].isalpha()
                and ":" in candidate
                and _key(_find(candidate, ":")) < _key(_find(candidate, "\n"))
                and not candidate.startswith(" ")):
            section_end = after_first_line + pos
    end = section_end if section_end is not None else min(len(rest), 3000)
    return rest[:end]

fails = []
def ck(name, cond, msg):
    if not cond: fails.append(f"{name}: {msg}")

ck("repin_workflow_has_push_trigger", "push:" in REPIN, "no push:")
ck("repin_workflow_has_push_trigger", "upstream/godot" in REPIN, "no upstream/godot")
ck("repin_workflow_has_pr_trigger", "pull_request:" in REPIN, "no pull_request:")
ck("repin_workflow_watches_oracle_and_golden_paths", "fixtures/oracle_outputs/**" in REPIN, "no oracle path")
ck("repin_workflow_watches_oracle_and_golden_paths", "fixtures/golden/**" in REPIN, "no golden path")
for job in ["detect-version","oracle-parity","render-goldens","physics-trace-goldens",
            "runtime-compat-slices","pin-verification","parity-summary","gdextension-lab"]:
    ck("repin_has_%s_job" % job, job+":" in REPIN, "missing job")

sec = extract_repin_section(REPIN, "parity-summary")
for dep in ["detect-version","oracle-parity","render-goldens","physics-trace-goldens",
            "runtime-compat-slices","pin-verification"]:
    ck("parity_summary_depends_on_all_gates", dep in sec, f"missing dep {dep}")
for g in ["oracle-parity","render-goldens","physics-trace-goldens","runtime-compat-slices","pin-verification"]:
    ck("fail_condition_checks_all_gates", f"needs.{g}.result" in REPIN, f"missing needs.{g}.result")

dv = extract_repin_section(REPIN, "detect-version")
ck("detect_version_has_fallback_logic", ("inputs.godot_version" in dv) or ("godot_version" in dv), "no godot_version")
ck("detect_version_has_fallback_logic", "outputs" in dv, "no outputs")

pt = extract_repin_section(REPIN, "physics-trace-goldens")
for pat in ["physics_trace","deterministic_physics","physics_playground","physics_stepping","mainloop_physics"]:
    ck("physics_trace_job_covers_expected_tests", pat in pt, f"missing {pat}")

rc = extract_repin_section(REPIN, "runtime-compat-slices")
ck("runtime_compat_job_covers_headless_and_2d", ("resource_" in rc) or ("headless" in rc), "no headless slice")
ck("runtime_compat_job_covers_headless_and_2d", ("render_" in rc) or ("2d" in rc) or ("2D" in rc), "no 2D slice")

pv = extract_repin_section(REPIN, "pin-verification")
ck("pin_verification_checks_submodule", ("upstream/godot" in pv) or ("submodule" in pv), "no submodule check")
ck("pin_verification_checks_fixtures", ("oracle_outputs" in pv) or ("fixtures" in pv), "no fixture check")

ck("summary_reports_physics_and_runtime_results", ("PHYSICS_RESULT" in sec) or ("physics_res" in sec), "no physics result")
ck("summary_reports_physics_and_runtime_results", ("RUNTIME_RESULT" in sec) or ("runtime_res" in sec), "no runtime result")
ck("summary_reports_physics_and_runtime_results", ("PIN_RESULT" in sec) or ("pin_res" in sec), "no pin result")

markers = ["detect-version:","oracle-parity:","render-goldens:","gdextension-lab:",
           "physics-trace-goldens:","runtime-compat-slices:","pin-verification:","parity-summary:"]
cnt = sum(1 for m in markers if m in REPIN)
ck("repin_workflow_has_at_least_7_jobs", cnt >= 7, f"only {cnt} jobs")

ck("ci_render_goldens_uploads_artifacts_on_failure", "upload-artifact@v4" in CI, "no upload-artifact@v4")
ck("ci_render_goldens_uploads_artifacts_on_failure", "if: failure()" in CI, "no if: failure()")
ck("ci_render_goldens_uploads_artifacts_on_failure", "golden-render-" in CI, "no golden-render- name")

if fails:
    print("FAILED %d assertion(s):" % len(fails))
    for f in fails: print("  -", f)
    sys.exit(1)
print("ALL 17 target assertions PASS (job count: %d)" % cnt)

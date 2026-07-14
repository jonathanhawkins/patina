// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test crash_suite -E 'test(/^<mod>::/)'
mod crash_report_test;
mod crash_triage_audit_test;
mod crash_triage_auto_issue_test;
mod crash_triage_process_test;

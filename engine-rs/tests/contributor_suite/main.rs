// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test contributor_suite -E 'test(/^<mod>::/)'
mod contributor_onboarding_docs_test;
mod contributor_onboarding_first_issue_test;
mod contributor_onboarding_validation_test;

//! pat-s3rrv.78 — `HTTPRequest`'s timeout and redirect settings reach the wire.
//!
//! The node built its transport config with `..Default::default()`, so `timeout`
//! and `max_redirects` never left the node at all: a `.tscn` could author them,
//! a script could read them back, and every request still went out with the
//! transport's defaults.
//!
//! ⚠️ The settings are read from the **node properties**, not from fields on
//! `HttpRequestNode`. ClassDB already registers `timeout` and `max_redirects` on
//! HTTPRequest with Godot's defaults, so the `.tscn` writes them there — and
//! Godot treats `r.timeout = 5.0` and `r.set_timeout(5.0)` as the same
//! operation. A second copy on the node struct would let those two disagree,
//! with whichever the transport happened to read winning silently. These tests
//! cover the pure pieces; `http_request_script_settings_and_cancel.rs` covers
//! the scene route.
//!
//! Ground truth: `doc/classes/HTTPRequest.xml:252-257` for the defaults and
//! `scene/main/http_request.cpp:255,591,616` for the three behaviours below.

use gdplatform::http_request::HttpMethod;
use gdscene::http_request_node::{
    accepted_timeout, redirects_for_transport, HttpRequestNode, RequestSettings,
    DEFAULT_MAX_REDIRECTS, DEFAULT_TIMEOUT,
};

fn config(settings: RequestSettings, url: &str) -> gdplatform::http_request::HttpRequestConfig {
    HttpRequestNode::new().config_for(url, HttpMethod::Get, Vec::new(), settings)
}

/// Godot's documented defaults.
#[test]
fn the_defaults_are_godots() {
    assert_eq!(DEFAULT_TIMEOUT, 0.0, "timeout defaults to 0.0 = never");
    assert_eq!(DEFAULT_MAX_REDIRECTS, 8, "max_redirects defaults to 8");

    let c = config(RequestSettings::default(), "http://example.test/x");
    assert_eq!(c.timeout_seconds, 0.0);
    assert_eq!(c.max_redirects, 8);
}

/// **The regression this pins**: the settings must reach the config.
///
/// ⚠️ The assertions read the CONFIG. A test that only round-tripped the
/// settings through getters passes against the old code, which stored each
/// value faithfully and then discarded it when building the request.
#[test]
fn the_settings_reach_the_transport_config() {
    let c = config(
        RequestSettings {
            timeout: 2.5,
            max_redirects: 3,
        },
        "http://example.test/x",
    );
    assert_eq!(
        c.timeout_seconds, 2.5,
        "the request must carry the node's timeout, not the transport default",
    );
    assert_eq!(
        c.max_redirects, 3,
        "the request must carry the node's max_redirects, not the default 8",
    );
}

/// A negative `max_redirects` means UNLIMITED, not "none".
///
/// ⚠️ Godot's check is `max_redirects >= 0 && redirections >= max_redirects`
/// (`http_request.cpp:255`), so a negative disables the limit entirely, and the
/// property's hint range is `-1,64` — `-1` is the documented spelling. Clamping
/// it to `0` inverts the meaning into "refuse every redirect", which surfaces as
/// a server or URL problem rather than as a settings one.
#[test]
fn a_negative_redirect_limit_means_unlimited_not_none() {
    assert_eq!(redirects_for_transport(-1), u32::MAX);
    assert_eq!(redirects_for_transport(0), 0, "0 really does forbid them");
    assert_eq!(redirects_for_transport(8), 8);
    // A value past the transport's counter saturates rather than wrapping.
    assert_eq!(redirects_for_transport(i64::MAX), u32::MAX);

    assert_eq!(
        config(
            RequestSettings {
                max_redirects: -1,
                ..Default::default()
            },
            "http://example.test/x",
        )
        .max_redirects,
        u32::MAX,
        "-1 must reach the transport as unlimited",
    );
}

/// A negative timeout is rejected, leaving the previous value in place.
///
/// ⚠️ Not clamped to zero: `ERR_FAIL_COND(p_timeout < 0)`
/// (`http_request.cpp:616`) returns before assigning. Clamping would turn a bad
/// argument into "never time out" — the one setting a caller asking for a
/// timeout definitely did not want.
#[test]
fn a_negative_timeout_is_rejected_rather_than_clamped() {
    assert_eq!(accepted_timeout(4.0, -1.0), 4.0, "the old value survives");
    assert_eq!(
        accepted_timeout(4.0, 0.0),
        0.0,
        "zero is legal — it means never"
    );
    assert_eq!(accepted_timeout(4.0, 9.5), 9.5);
}

/// The scheme still decides TLS, and the refactor must not have lost it.
#[test]
fn the_scheme_still_selects_ssl() {
    let s = RequestSettings::default();
    assert!(config(s, "https://example.test/x").use_ssl);
    assert!(!config(s, "http://example.test/x").use_ssl);
}

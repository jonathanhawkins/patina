//! pat-s3rrv.78 — `HTTPRequest.set_http_proxy` / `set_https_proxy`, script side.
//!
//! Godot ground truth: `doc/classes/HTTPRequest.xml:209-225` declares both as
//! `(host: String, port: int) -> void`, and states the unset rule in words —
//! *"The proxy server is unset if [param host] is empty or [param port] is -1."*
//! `core/io/http_client_tcp.cpp:770-788` spells the same test in code.
//!
//! 🔑 **Setters only.** Neither has a getter, and neither is a `<member>` — the
//! class XML has six `proxy` hits and not one of them is a property. Every other
//! configurable pair on this node (`timeout`, `max_redirects`,
//! `body_size_limit`, `accept_gzip`, `download_file`) has a `get_*`/`is_*` twin,
//! so the symmetric thing to do here is to invent API upstream does not have.
//! These tests read the result back through `proxy_settings`, the way the
//! request path does, rather than through a getter that should not exist.

use gdplatform::http_request::HttpMethod;
use gdscene::http_request_node::{proxy_settings, HttpRequestNode, ProxySettings, RequestSettings};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// A bare HTTPRequest node with whatever proxy properties a test authors.
fn node_with(props: &[(&str, Variant)]) -> Node {
    let mut node = Node::new("Http", "HTTPRequest");
    for (key, value) in props {
        node.set_property(*key, value.clone());
    }
    node
}

fn s(v: &str) -> Variant {
    Variant::String(v.to_string())
}

fn pair(host: &str, port: i32) -> Option<(String, i32)> {
    Some((host.to_string(), port))
}

// ------------------------------------------------------------ the read rule

/// The positive control: an untouched node is direct on both schemes.
///
/// ⚠️ Without this, every "must be None" assertion below would also pass if
/// `proxy_settings` returned `None` unconditionally.
#[test]
fn an_untouched_node_has_no_proxies() {
    let settings = proxy_settings(&node_with(&[]));
    assert_eq!(settings, ProxySettings::default());
    assert_eq!(settings.http, None);
    assert_eq!(settings.https, None);
}

/// …and the control's opposite: a fully authored node reports both.
#[test]
fn an_authored_node_reports_both_proxies() {
    let settings = proxy_settings(&node_with(&[
        ("http_proxy_host", s("plain.test")),
        ("http_proxy_port", Variant::Int(3128)),
        ("https_proxy_host", s("tls.test")),
        ("https_proxy_port", Variant::Int(8443)),
    ]));
    assert_eq!(settings.http, pair("plain.test", 3128));
    assert_eq!(settings.https, pair("tls.test", 8443));
}

/// Godot's documented unset rule, both halves of the `or`.
#[test]
fn an_empty_host_or_a_minus_one_port_reads_as_unset() {
    let empty_host = proxy_settings(&node_with(&[
        ("http_proxy_host", s("")),
        ("http_proxy_port", Variant::Int(3128)),
    ]));
    assert_eq!(empty_host.http, None, "an empty host is unset");

    let no_port = proxy_settings(&node_with(&[
        ("http_proxy_host", s("plain.test")),
        ("http_proxy_port", Variant::Int(-1)),
    ]));
    assert_eq!(no_port.http, None, "a port of -1 is unset");
}

/// 🔑 The coincidence point: port `0`.
///
/// ⚠️ The rule is `port == -1` exactly (`HTTPRequest.xml:215`,
/// `http_client_tcp.cpp:771`). Writing it as `port <= 0` reads as a tidier
/// spelling of the same thing and silently discards a caller that asked for
/// port 0 — the only value at which the two spellings disagree.
#[test]
fn a_zero_port_is_a_real_proxy_not_an_unset_one() {
    let settings = proxy_settings(&node_with(&[
        ("http_proxy_host", s("plain.test")),
        ("http_proxy_port", Variant::Int(0)),
    ]));
    assert_eq!(settings.http, pair("plain.test", 0));
}

/// 🔑 An absent port property means UNSET, not port 0.
///
/// ⚠️ This is the same defect as above reached from the other side, and it is
/// the one a default-initialised reader walks into: `Variant::Int` missing →
/// `i64::default()` → `0` → a node that never called the setter ends up holding
/// a live proxy on port 0. The fallback has to be `-1`.
#[test]
fn a_host_with_no_port_property_is_unset_not_port_zero() {
    let settings = proxy_settings(&node_with(&[("http_proxy_host", s("plain.test"))]));
    assert_eq!(
        settings.http, None,
        "an absent port must default to -1 (unset), never to 0"
    );
}

/// The two schemes never leak into each other.
#[test]
fn the_two_proxies_are_read_independently() {
    let only_https = proxy_settings(&node_with(&[
        ("https_proxy_host", s("tls.test")),
        ("https_proxy_port", Variant::Int(8443)),
    ]));
    assert_eq!(only_https.https, pair("tls.test", 8443));
    assert_eq!(
        only_https.http, None,
        "authoring the TLS proxy must not produce a plaintext one"
    );
}

// --------------------------------------------------- reaching the transport

#[test]
fn the_proxy_pair_reaches_the_request_config() {
    let node = HttpRequestNode::new();
    let config = node.config_for_with_proxies(
        "http://example.test/x",
        HttpMethod::Get,
        Vec::new(),
        &[],
        RequestSettings::default(),
        &ProxySettings {
            http: pair("plain.test", 3128),
            https: pair("tls.test", 8443),
        },
    );
    assert_eq!(config.http_proxy, pair("plain.test", 3128));
    assert_eq!(config.https_proxy, pair("tls.test", 8443));
}

/// The regression guard for the delegation: the five existing call sites of
/// `config_for` / `config_for_with_headers` were not touched, and must still
/// produce a direct request.
#[test]
fn the_existing_config_entry_points_still_default_to_direct() {
    let node = HttpRequestNode::new();
    let config = node.config_for(
        "http://example.test/x",
        HttpMethod::Get,
        Vec::new(),
        RequestSettings::default(),
    );
    assert_eq!(config.http_proxy, None);
    assert_eq!(config.https_proxy, None);
}

// -------------------------------------------------------------- script side

/// Runs `src` against a tree holding one HTTPRequest child named `Http`, and
/// returns that node's proxy settings afterwards.
///
/// ⚠️ The script source must be a single-line literal with `\n` escapes — the
/// parser here does not take a raw multi-line string.
fn run_script(src: &str) -> ProxySettings {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let http = tree
        .add_child(root, Node::new("Http", "HTTPRequest"))
        .expect("add http node");
    tree.attach_script(
        root,
        Box::new(gdscene::GDScriptNodeInstance::from_source(src, root).expect("script parses")),
    );
    assert!(
        tree.call_script_method(root, "run", &[]).is_some(),
        "run() raised — the proxy arm did not resolve"
    );
    proxy_settings(tree.get_node(http).expect("http node still in the tree"))
}

#[test]
fn the_script_set_http_proxy_arm_reaches_the_reader() {
    let settings = run_script(
        "extends Node\nfunc run():\n    var r = get_node(\"Http\")\n    r.set_http_proxy(\"plain.test\", 3128)\n",
    );
    assert_eq!(
        settings.http,
        pair("plain.test", 3128),
        "the setter must write the properties `proxy_settings` reads"
    );
    assert_eq!(settings.https, None, "and must not touch the TLS proxy");
}

#[test]
fn the_script_set_https_proxy_arm_reaches_the_reader() {
    let settings = run_script(
        "extends Node\nfunc run():\n    var r = get_node(\"Http\")\n    r.set_https_proxy(\"tls.test\", 8443)\n",
    );
    assert_eq!(settings.https, pair("tls.test", 8443));
    assert_eq!(
        settings.http, None,
        "the two arms must write different properties"
    );
}

/// The same invariant in the opposite direction: what a script sets, a script
/// can clear.
#[test]
fn a_script_can_clear_a_proxy_it_set() {
    let settings = run_script(
        "extends Node\nfunc run():\n    var r = get_node(\"Http\")\n    r.set_http_proxy(\"plain.test\", 3128)\n    r.set_http_proxy(\"\", -1)\n",
    );
    assert_eq!(
        settings.http, None,
        "clearing must actually clear, not leave the previous pair behind"
    );
}

/// A script's port 0 survives the whole path, script arm included.
#[test]
fn a_script_authored_zero_port_survives_to_the_reader() {
    let settings = run_script(
        "extends Node\nfunc run():\n    var r = get_node(\"Http\")\n    r.set_http_proxy(\"plain.test\", 0)\n",
    );
    assert_eq!(settings.http, pair("plain.test", 0));
}

/// End to end: a script sets a proxy, and the config the transport receives
/// carries it.
///
/// 🔑 This is the assertion the increment exists for. Everything above tests one
/// hop; this one pins script → node property → `ProxySettings` →
/// `HttpRequestConfig` as a single chain, which is where a break would actually
/// hurt.
#[test]
fn a_scripted_proxy_reaches_the_request_config_end_to_end() {
    let settings = run_script(
        "extends Node\nfunc run():\n    var r = get_node(\"Http\")\n    r.set_http_proxy(\"plain.test\", 3128)\n",
    );
    let config = HttpRequestNode::new().config_for_with_proxies(
        "http://example.test/x",
        HttpMethod::Get,
        Vec::new(),
        &[],
        RequestSettings::default(),
        &settings,
    );
    assert_eq!(config.http_proxy, pair("plain.test", 3128));
}

//! pat-s3rrv.78 — `HTTPRequest.accept_gzip`, both halves.
//!
//! Godot ground truth: the node adds `Accept-Encoding: gzip, deflate` on the way
//! out (`http_request.cpp:136-139`) and decodes on the way back, choosing the
//! codec from `Content-Encoding` (`:308-316`). Both halves are node-level; the
//! transport is never involved, which is why this needs no transport change.
//!
//! 🔑 The trap this file exists to pin: **HTTP `deflate` means ZLIB-WRAPPED
//! deflate, not raw deflate.** Godot's decompressor takes
//! `window_bits = p_is_deflate ? 15 : (15 + 16)` — 15 is zlib, 15+16 is gzip, and
//! raw would be negative. Reading the name at face value produces an
//! implementation that fails against every real server, so
//! `a_deflate_body_is_zlib_wrapped_not_raw` asserts a RAW body is REJECTED. A
//! test that only checked "zlib decodes" would pass against a both-ways decoder
//! that is wrong about what the header means.
//!
//! ⚠️ `accept_gzip` defaults to **true**, so these paths are the normal ones.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gdplatform::http_request::{HttpMethod, RequestCompleted};
use gdscene::http_request_node::{
    deliver, DeliverySettings, HttpRequestNode, RequestSettings, RESULT_BODY_DECOMPRESS_FAILED,
    RESULT_BODY_SIZE_LIMIT_EXCEEDED,
};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const PLAIN: &[u8] = b"the quick brown fox jumps over the lazy dog, repeatedly and at length";

/// Builds an RFC 1952 gzip member around `plain`.
///
/// `flg` and `optional` let a test exercise the variable-length header fields;
/// `optional` must be exactly what `flg` announces.
///
/// ⚠️ The CRC32 slot is left zero on purpose: this decoder checks the ISIZE
/// length but not the CRC (recorded as a known deviation in `decompress_body`).
/// A fixture with a real CRC would hide that, making the gap invisible here.
fn gzip_member(plain: &[u8], flg: u8, optional: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1f, 0x8b, 0x08, flg, 0, 0, 0, 0, 0, 0xff];
    out.extend_from_slice(optional);
    out.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(plain, 6));
    out.extend_from_slice(&[0, 0, 0, 0]); // CRC32, not verified
    out.extend_from_slice(&(plain.len() as u32).to_le_bytes()); // ISIZE
    out
}

fn gzip(plain: &[u8]) -> Vec<u8> {
    gzip_member(plain, 0, &[])
}

fn encoded_payload(body: Vec<u8>, encoding: &str) -> RequestCompleted {
    RequestCompleted {
        result: 0,
        response_code: 200,
        headers: vec![
            "Content-Type: text/plain".to_string(),
            format!("Content-Encoding: {encoding}"),
        ],
        body,
    }
}

fn gzip_on() -> DeliverySettings {
    DeliverySettings::default()
}

// ------------------------------------------------------------ response side

#[test]
fn a_gzip_body_is_decoded() {
    let got = deliver(encoded_payload(gzip(PLAIN), "gzip"), &gzip_on());

    assert_eq!(got.result, 0);
    assert_eq!(
        got.body, PLAIN,
        "the gzip member must decode to the original"
    );
}

/// 🔑 `deflate` is zlib-wrapped. A raw-deflate body must be REFUSED.
#[test]
fn a_deflate_body_is_zlib_wrapped_not_raw() {
    let zlib = miniz_oxide::deflate::compress_to_vec_zlib(PLAIN, 6);
    let got = deliver(encoded_payload(zlib, "deflate"), &gzip_on());
    assert_eq!(got.result, 0, "a zlib-wrapped body is what `deflate` means");
    assert_eq!(got.body, PLAIN);

    // ⚠️ The half that catches the misreading: raw DEFLATE is NOT `deflate`.
    let raw = miniz_oxide::deflate::compress_to_vec(PLAIN, 6);
    let got = deliver(encoded_payload(raw, "deflate"), &gzip_on());
    assert_eq!(
        got.result, RESULT_BODY_DECOMPRESS_FAILED,
        "a raw DEFLATE body must be refused — Godot asks zlib for window bits \
         15, which requires the zlib wrapper. Accepting it here would mean this \
         decoder disagrees with the one Godot uses."
    );
}

/// The optional gzip header fields are skipped, in the RFC's order.
///
/// ⚠️ Real servers set FNAME routinely. Assuming a flat 10-byte header decodes
/// nothing but the simplest fixture — and the simplest fixture is exactly what a
/// hand-written test uses, so this is easy to ship broken.
#[test]
fn gzip_optional_header_fields_are_skipped() {
    // FEXTRA | FNAME | FCOMMENT | FHCRC, in the order RFC 1952 lays them out.
    let flg = 0x04 | 0x08 | 0x10 | 0x02;
    let mut optional = Vec::new();
    optional.extend_from_slice(&3u16.to_le_bytes()); // FEXTRA length
    optional.extend_from_slice(b"abc"); // FEXTRA payload
    optional.extend_from_slice(b"a-file-name\0"); // FNAME
    optional.extend_from_slice(b"a comment\0"); // FCOMMENT
    optional.extend_from_slice(&[0, 0]); // FHCRC

    let got = deliver(
        encoded_payload(gzip_member(PLAIN, flg, &optional), "gzip"),
        &gzip_on(),
    );
    assert_eq!(got.result, 0, "optional header fields must be skipped");
    assert_eq!(got.body, PLAIN);
}

#[test]
fn a_body_that_is_not_gzip_at_all_reports_decompress_failed() {
    let got = deliver(
        encoded_payload(b"not compressed".to_vec(), "gzip"),
        &gzip_on(),
    );
    assert_eq!(got.result, RESULT_BODY_DECOMPRESS_FAILED, "result 8");
    assert!(got.body.is_empty());
    assert_eq!(got.response_code, 200, "the server still answered 200");
}

/// A truncated member is caught by the ISIZE trailer.
#[test]
fn a_gzip_member_whose_length_disagrees_is_refused() {
    let mut member = gzip(PLAIN);
    let n = member.len();
    // Claim one byte more than was compressed.
    member[n - 4..].copy_from_slice(&(PLAIN.len() as u32 + 1).to_le_bytes());

    let got = deliver(encoded_payload(member, "gzip"), &gzip_on());
    assert_eq!(
        got.result, RESULT_BODY_DECOMPRESS_FAILED,
        "the ISIZE trailer must be checked against what was actually decoded"
    );
}

/// ⚠️ With the property off, a compressed body is passed through UNTOUCHED.
///
/// Upstream reads `Content-Encoding` only `if (accept_gzip)`. Decoding anyway
/// would be "helpful" and wrong: a script that turned the property off did so to
/// get the bytes as sent.
#[test]
fn accept_gzip_false_leaves_the_body_encoded() {
    let member = gzip(PLAIN);
    let settings = DeliverySettings {
        accept_gzip: false,
        ..Default::default()
    };
    let got = deliver(encoded_payload(member.clone(), "gzip"), &settings);

    assert_eq!(got.result, 0);
    assert_eq!(got.body, member, "the bytes must arrive exactly as sent");
    assert_ne!(got.body, PLAIN, "and must NOT have been decoded");
}

/// An encoding this decoder has no codec for is left alone rather than failed.
#[test]
fn an_unknown_encoding_is_passed_through() {
    let got = deliver(encoded_payload(PLAIN.to_vec(), "br"), &gzip_on());
    assert_eq!(got.result, 0, "brotli is not an error, just not decoded");
    assert_eq!(got.body, PLAIN);
}

/// 🔑 The zip-bomb guard: the limit is enforced **by** the decoder.
///
/// ⚠️ The compressed body here is far UNDER the limit, so the first size check
/// (against the received length) passes. Only a limit applied during decoding
/// stops it. An implementation that decodes fully and measures afterwards gives
/// the same result code for this input and still allocates the whole output —
/// so this test is about the code path, and the comment is the record of why.
#[test]
fn a_body_that_expands_past_the_limit_is_refused_as_too_large() {
    let big = vec![b'a'; 200_000];
    let member = gzip(&big);
    assert!(
        member.len() < 1_000,
        "fixture precondition: the COMPRESSED body must be tiny ({} bytes), or \
         the first check would catch it and the decoder bound would go untested",
        member.len()
    );

    let settings = DeliverySettings {
        body_size_limit: 1_000,
        ..Default::default()
    };
    let got = deliver(encoded_payload(member, "gzip"), &settings);

    assert_eq!(
        got.result, RESULT_BODY_SIZE_LIMIT_EXCEEDED,
        "an over-limit EXPANSION is a size refusal (7), not a decode failure (8)"
    );
    assert!(got.body.is_empty());
}

/// The boundary again, now on the decoded size: exactly the limit is delivered.
///
/// ⚠️ The fixture must be genuinely COMPRESSIBLE. The first check runs against
/// the body as received, which for a short string is LARGER than the decoded
/// form once the 18 bytes of gzip framing are counted — so a small fixture is
/// refused by check (1) and never reaches the decoder bound this test is about.
/// (Godot refuses it too: its `:408` check reads Content-Length, which for a
/// compressed response is the compressed length. Faithful, and easy to mistake
/// for a bug in the boundary.)
#[test]
fn a_decoded_body_of_exactly_the_limit_is_delivered() {
    let plain = vec![b'a'; 5_000];
    let member = gzip(&plain);
    assert!(
        member.len() < plain.len(),
        "fixture precondition: the compressed form must be smaller ({} vs {})",
        member.len(),
        plain.len()
    );

    let settings = DeliverySettings {
        body_size_limit: plain.len() as i64,
        ..Default::default()
    };
    let got = deliver(encoded_payload(member, "gzip"), &settings);

    assert_eq!(
        got.result, 0,
        "the decoder's cap is limit + 1, so a body of exactly the limit must \
         still decode — an off-by-one here refuses valid responses"
    );
    assert_eq!(got.body, plain);
}

// ------------------------------------------------------------- request side

#[test]
fn accept_encoding_is_sent_by_default() {
    let node = HttpRequestNode::new();
    let config = node.config_for(
        "http://example.test/x",
        HttpMethod::Get,
        Vec::new(),
        RequestSettings::default(),
    );
    assert_eq!(
        config.headers.get("Accept-Encoding").map(String::as_str),
        Some("gzip, deflate"),
        "accept_gzip defaults to true, so the header goes out by default"
    );
}

#[test]
fn accept_gzip_false_sends_no_accept_encoding() {
    let node = HttpRequestNode::new();
    let config = node.config_for(
        "http://example.test/x",
        HttpMethod::Get,
        Vec::new(),
        RequestSettings {
            accept_gzip: false,
            ..Default::default()
        },
    );
    assert!(
        !config
            .headers
            .keys()
            .any(|k| k.eq_ignore_ascii_case("Accept-Encoding")),
        "with the property off no encoding may be advertised, got {:?}",
        config.headers
    );
}

/// ⚠️ A caller's own `Accept-Encoding` wins (`http_request.cpp:137-139`).
#[test]
fn a_caller_supplied_accept_encoding_is_not_overwritten() {
    let node = HttpRequestNode::new();
    let config = node.config_for_with_headers(
        "http://example.test/x",
        HttpMethod::Get,
        Vec::new(),
        // Deliberately lower-cased: HTTP header names are case-insensitive, and
        // a case-sensitive check would silently add a second, contradictory one.
        &["accept-encoding: identity".to_string()],
        RequestSettings::default(),
    );
    let sent: Vec<_> = config
        .headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("Accept-Encoding"))
        .collect();
    assert_eq!(sent.len(), 1, "exactly one, got {sent:?}");
    assert_eq!(
        sent[0].1.as_str(),
        "identity",
        "the caller's value must survive"
    );
}

#[test]
fn custom_headers_reach_the_transport_config() {
    let node = HttpRequestNode::new();
    let config = node.config_for_with_headers(
        "http://example.test/x",
        HttpMethod::Get,
        Vec::new(),
        &[
            "X-Token: abc123".to_string(),
            // A value containing a colon: splitting from the right would keep
            // only the fragment after the last one.
            "X-When: 10:30:00".to_string(),
        ],
        RequestSettings::default(),
    );
    assert_eq!(
        config.headers.get("X-Token").map(String::as_str),
        Some("abc123")
    );
    assert_eq!(
        config.headers.get("X-When").map(String::as_str),
        Some("10:30:00"),
        "a header value may contain colons; only the FIRST separates name from value"
    );
}

// ------------------------------------------------------- on the wire

/// Serves one request and hands back exactly what arrived.
fn capture_one_request() -> (u16, Arc<Mutex<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(String::new()));
    let sink = seen.clone();
    std::thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            let mut buf = [0u8; 4096];
            if let Ok(n) = sock.read(&mut buf) {
                *sink.lock().unwrap() = String::from_utf8_lossy(&buf[..n]).into_owned();
            }
            let _ = sock.write_all(
                b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            let _ = sock.flush();
        }
    });
    (port, seen)
}

/// Issues one request and returns its `Accept-Encoding` header lines.
fn accept_encoding_lines_on_the_wire(settings: RequestSettings) -> Vec<String> {
    let (port, seen) = capture_one_request();
    let node = HttpRequestNode::new();
    node.request(
        &format!("http://127.0.0.1:{port}/x"),
        HttpMethod::Get,
        Vec::new(),
        settings,
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let head = seen.lock().unwrap().clone();
        if !head.is_empty() {
            return head
                .lines()
                .filter(|l| l.to_ascii_lowercase().starts_with("accept-encoding:"))
                .map(|l| l.trim().to_string())
                .collect();
        }
        assert!(
            Instant::now() < deadline,
            "the request never reached the server"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 🔑 Exactly ONE `Accept-Encoding` reaches the wire.
///
/// ⚠️ The transport hardcoded `Accept-Encoding: identity` before every caller
/// header, so advertising gzip produced TWO conflicting lines — `identity`
/// first, `gzip, deflate` second. RFC 7230 lets a server honour either, so
/// `accept_gzip` would have worked against some servers and not others, with
/// nothing in Patina to point at. Asserting the VALUE alone would not catch it;
/// the count is the assertion that matters.
#[test]
fn exactly_one_accept_encoding_reaches_the_wire() {
    let lines = accept_encoding_lines_on_the_wire(RequestSettings::default());
    assert_eq!(
        lines.len(),
        1,
        "exactly one Accept-Encoding may be sent, got {lines:?}"
    );
    assert_eq!(lines[0], "Accept-Encoding: gzip, deflate");
}

/// ⚠️ And with the property off the transport's own default still applies —
/// the guard must not delete the header for callers who never set one.
#[test]
fn accept_gzip_false_still_sends_the_transport_default() {
    let lines = accept_encoding_lines_on_the_wire(RequestSettings {
        accept_gzip: false,
        ..Default::default()
    });
    assert_eq!(lines.len(), 1, "still exactly one, got {lines:?}");
    assert_eq!(
        lines[0], "Accept-Encoding: identity",
        "suppressing the hardcoded default for everyone would be a regression \\
         for every caller that sends no Accept-Encoding of its own"
    );
}

// ------------------------------------------------------------- script side

/// `set_accept_gzip` / `is_accepting_gzip` reach the node property both readers use.
#[test]
fn the_script_accept_gzip_arms_round_trip() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let http = tree
        .add_child(root, Node::new("Http", "HTTPRequest"))
        .expect("add http node");

    let src = "extends Node\nvar before = true\nvar after = true\nfunc run():\n    var r = get_node(\"Http\")\n    self.before = r.is_accepting_gzip()\n    r.set_accept_gzip(false)\n    self.after = r.is_accepting_gzip()\n";
    tree.attach_script(
        root,
        Box::new(gdscene::GDScriptNodeInstance::from_source(src, root).expect("script parses")),
    );
    assert!(
        tree.call_script_method(root, "run", &[]).is_some(),
        "run() raised — set_accept_gzip or is_accepting_gzip did not resolve"
    );

    let var = |name: &str| {
        tree.get_script(root)
            .expect("script attached")
            .get_property(name)
            .unwrap_or(Variant::Nil)
    };
    assert_eq!(
        var("before"),
        Variant::Bool(true),
        "an untouched node reports the Godot default, which is true"
    );
    assert_eq!(var("after"), Variant::Bool(false));

    // And the emit path sees the same value the script wrote.
    assert!(
        !gdscene::http_request_node::delivery_settings(tree.get_node(http).unwrap()).accept_gzip,
        "the setter must write the property `delivery_settings` reads"
    );
}

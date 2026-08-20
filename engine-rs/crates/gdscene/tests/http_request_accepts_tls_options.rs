//! pat-s3rrv.237 — `HTTPRequest.set_tls_options` accepts a `TLSOptions`.
//!
//! # Why this is a real gap and not a formality
//!
//! `TLSOptions` is the configuration object Godot threads through
//! `HTTPClient.connect_to_host`, `HTTPRequest.set_tls_options` and
//! `WebSocketPeer.connect_to_url`. Before this bead the class did not exist at
//! all, so a script that merely built one to hand to an HTTP request failed at
//! the class level — long before any question of TLS support arose. That is why
//! `TLSOptions` had to genuinely work while `StreamPeerTLS` correctly does not:
//! on web the browser performs the handshake through fetch, so the options are
//! configuration the engine carries, not a transport it must implement.
//!
//! ⚠️ **There is deliberately no `get_tls_options`.** Godot's `HTTPRequest`
//! declares `set_tls_options` and no getter — no method, and no `tls_options`
//! member. Adding one would invent API in a parity port, so the stored value is
//! read here through the node property rather than through a script call.

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// **The acceptance case.** A script builds a `TLSOptions` and hands it to an
/// `HTTPRequest`; the call resolves, and the object it stored is the one the
/// script built.
#[test]
fn http_request_accepts_tls_options() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let http = tree
        .add_child(root, Node::new("Http", "HTTPRequest"))
        .expect("add http node");

    // `sent` records the id the script built so the assertion below can compare
    // identities rather than merely checking that *something* was stored.
    let src = "extends Node\n\
               var sent = 0\n\
               var ok = false\n\
               func run():\n\
               \x20   var r = get_node(\"Http\")\n\
               \x20   var o = TLSOptions.client(null, \"example.org\")\n\
               \x20   self.sent = o.get_instance_id()\n\
               \x20   r.set_tls_options(o)\n\
               \x20   self.ok = true\n";
    tree.attach_script(
        root,
        Box::new(gdscene::GDScriptNodeInstance::from_source(src, root).expect("script parses")),
    );

    assert!(
        tree.call_script_method(root, "run", &[]).is_some(),
        "run() raised — either TLSOptions.client() did not resolve or \
         set_tls_options did not resolve on the node"
    );

    let script_var = |name: &str| {
        tree.get_script(root)
            .expect("script attached")
            .get_property(name)
            .unwrap_or(Variant::Nil)
    };

    // Reaching the last line means neither call raised. Asserted explicitly
    // because `call_script_method` returning Some only says the method ran.
    assert_eq!(
        script_var("ok"),
        Variant::Bool(true),
        "the script must run to completion with no error"
    );

    let sent = match script_var("sent") {
        Variant::Int(id) => id,
        other => panic!("the script never recorded an instance id: {other:?}"),
    };
    assert_ne!(sent, 0, "TLSOptions.client() must produce a real object");

    // The node stored THAT object, not a placeholder. An arm that accepted the
    // call and dropped the argument — the easy way to make this "work" — leaves
    // the property Nil and fails here.
    let stored = tree
        .get_node(http)
        .expect("http node")
        .get_property("tls_options");
    match stored {
        Variant::ObjectId(id) => assert_eq!(
            id.raw() as i64,
            sent,
            "the stored TLSOptions must be the object the script built"
        ),
        other => panic!("set_tls_options stored {other:?} rather than the object"),
    }
}

/// The control: an `HTTPRequest` that was never configured has no options.
///
/// Without this, the assertion above could pass on an implementation that
/// stored something into `tls_options` regardless of the call.
#[test]
fn http_request_accepts_tls_options_absent_until_set() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let http = tree
        .add_child(root, Node::new("Http", "HTTPRequest"))
        .expect("add http node");

    assert!(
        matches!(
            tree.get_node(http)
                .expect("http node")
                .get_property("tls_options"),
            Variant::Nil
        ),
        "a fresh HTTPRequest must carry no TLS options"
    );
}

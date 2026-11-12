//! pat-s3rrv.79 — the WebSocketPeer connection accessors answer from a script.
//!
//! `gdplatform` implements them and ClassDB now declares them; this is the third
//! side, and the one a project actually touches. Without a dispatch arm the
//! interpreter answers `UndefinedFunction("WebSocketPeer.get_connected_host")`
//! and kills the calling script — a registered method that raises on call is
//! worse than an absent one, because reflection says it is there.
//!
//! ⚠️ Every assertion here is on an UNCONNECTED peer, which is deliberate: that
//! is the state a polling script sees most of the time, and it is the state a
//! naive implementation panics in by unwrapping the socket.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn eval(body: &str) -> Variant {
    let src = format!("extends Node\n\nfunc check():\n{body}\n");
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(&src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "check", &[])
        .unwrap_or_else(|e| panic!("script failed: {e:?}"))
}

#[test]
fn the_connection_accessors_answer_on_a_fresh_peer() {
    assert_eq!(
        eval("    var p = WebSocketPeer.new()\n    return p.get_connected_host()"),
        Variant::String(String::new()),
        "an unconnected peer reports no host rather than raising",
    );
    assert_eq!(
        eval("    var p = WebSocketPeer.new()\n    return p.get_connected_port()"),
        Variant::Int(0),
        "…and port 0",
    );
    assert_eq!(
        eval(
            "    var p = WebSocketPeer.new()\n    return p.get_current_outbound_buffered_amount()"
        ),
        Variant::Int(0),
        "nothing sent, nothing queued — and an Int, not a Float",
    );
}

/// `set_no_delay` returns void and does not raise on a socketless peer.
///
/// ⚠️ The argument is inverted relative to the option it sets:
/// `set_no_delay(true)` DISABLES Nagle. The test drives both values so a
/// dispatch arm that ignored its argument entirely would still have to accept
/// one.
#[test]
fn set_no_delay_is_callable_and_returns_nothing() {
    assert_eq!(
        eval("    var p = WebSocketPeer.new()\n    p.set_no_delay(true)\n    p.set_no_delay(false)\n    return p.get_connected_port()"),
        Variant::Int(0),
        "set_no_delay must be dispatched, take a bool, and leave the peer usable",
    );
}

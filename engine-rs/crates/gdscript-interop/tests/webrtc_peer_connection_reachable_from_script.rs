//! pat-s3rrv.80 — the WebRTC stack is reachable and drivable from GDScript.
//!
//! The types landed in `gdplatform/src/webrtc_peer.rs` in the previous pass, but
//! a script could not touch them: `WebRTCPeerConnection` and `WebRTCDataChannel`
//! had no ClassDB entry and no handle path in the interpreter, so
//! `WebRTCPeerConnection.new()` raised. This is the reachability half.
//!
//! ⚠️ **`poll()` is the only place events reach a script.** Godot's browser glue
//! flushes its queued `RTCPeerConnection` callbacks into signals inside
//! `WebRTCPeerConnection.poll()`, so this transport does the same. That is not a
//! detail of the test harness — it is the contract, and
//! [`an_unpolled_connection_emits_nothing`] pins it, because an implementation
//! that emitted eagerly would pass the happy-path test while behaving nothing
//! like Godot in a real frame loop.
//!
//! ⚠️ A handle is not a scene node, so neither of the interpreter's `connect`
//! paths could bind these signals; they go through a handle-side registry, the
//! same shape `WebXRInterface` uses.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn call_check(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "check", &[])
        .unwrap_or_else(|e| panic!("check() failed: {e:?}\n--- source ---\n{src}"))
}

fn array_of(value: &Variant) -> &Vec<Variant> {
    match value {
        Variant::Array(v) => v,
        other => panic!("check() should return an Array, got {other:?}"),
    }
}

/// 🔑 The class can be CONSTRUCTED at all — this is what used to raise.
#[test]
fn webrtc_peer_connection_can_be_constructed_from_script() {
    let src = "\
extends Node

func check():
    var c = WebRTCPeerConnection.new()
    return [c.get_connection_state(), c.get_gathering_state(), c.get_signaling_state()]
";
    let result = call_check(src);
    let v = array_of(&result);
    // STATE_NEW / GATHERING_STATE_NEW / SIGNALING_STATE_STABLE, Godot's values.
    assert_eq!(v[0], Variant::Int(0), "a fresh connection is STATE_NEW");
    assert_eq!(v[1], Variant::Int(0), "gathering has not started");
    assert_eq!(v[2], Variant::Int(0), "signaling is STABLE");
}

/// **The acceptance test.** A script drives the whole handshake.
///
/// Two connections, a signalling loop written in GDScript exactly as an
/// application would write it: connect to the signals, relay each description
/// and candidate to the other side, then send a packet over the data channel.
#[test]
fn webrtc_peer_connection_offer_answer_and_data_channel_from_script() {
    let src = "\
extends Node

var a_type = \"\"
var a_sdp = \"\"
var b_type = \"\"
var b_sdp = \"\"
var a_media = \"\"
var a_index = 0
var a_name = \"\"
var b_media = \"\"
var b_index = 0
var b_name = \"\"

func _a_desc(type, sdp):
    a_type = type
    a_sdp = sdp

func _b_desc(type, sdp):
    b_type = type
    b_sdp = sdp

func _a_ice(media, index, name):
    a_media = media
    a_index = index
    a_name = name

func _b_ice(media, index, name):
    b_media = media
    b_index = index
    b_name = name

func check():
    var a = WebRTCPeerConnection.new()
    var b = WebRTCPeerConnection.new()
    a.connect(\"session_description_created\", _a_desc)
    a.connect(\"ice_candidate_created\", _a_ice)
    b.connect(\"session_description_created\", _b_desc)
    b.connect(\"ice_candidate_created\", _b_ice)

    var ch_a = a.create_data_channel(\"game\", {})
    var ch_b = b.create_data_channel(\"game\", {})

    a.create_offer()
    a.poll()
    a.set_local_description(a_type, a_sdp)
    a.poll()

    b.set_remote_description(a_type, a_sdp)
    b.poll()
    b.set_local_description(b_type, b_sdp)
    a.set_remote_description(b_type, b_sdp)

    a.add_ice_candidate(b_media, b_index, b_name)
    b.add_ice_candidate(a_media, a_index, a_name)
    a.poll()
    b.poll()

    ch_a.put_packet(PackedByteArray([7, 7]))
    return [a.get_connection_state(), b.get_connection_state(), a_type, b_type, ch_b.get_available_packet_count(), ch_b.get_packet(), ch_b.get_ready_state()]
";
    let result = call_check(src);
    let v = array_of(&result);

    assert_eq!(
        v[2],
        Variant::String("offer".to_string()),
        "the first description a script sees must be the offer it asked for",
    );
    assert_eq!(
        v[3],
        Variant::String("answer".to_string()),
        "setting a remote OFFER generates the answer, as the browser glue does \
         by chaining createAnswer onto setRemoteDescription — a script never \
         has to know an answer is owed",
    );
    // STATE_CONNECTED == 2.
    assert_eq!(v[0], Variant::Int(2), "A must reach STATE_CONNECTED");
    assert_eq!(v[1], Variant::Int(2), "and so must B");
    // ChannelState::Open == 1.
    assert_eq!(v[6], Variant::Int(1), "the channel must be Open");
    assert_eq!(v[4], Variant::Int(1), "one packet should be waiting");
    assert_eq!(
        v[5],
        Variant::PackedByteArray(vec![7, 7]),
        "the bytes must survive the trip",
    );
}

/// 🔑 The control: no `poll()`, no events.
///
/// ⚠️ Emitting eagerly from `create_offer` would pass the acceptance test above
/// and still be wrong — Godot delivers these only when the application polls,
/// and a script written against an eager implementation would break the moment
/// it ran on a real frame loop.
#[test]
fn an_unpolled_connection_emits_nothing() {
    let src = "\
extends Node

var seen = 0

func _desc(type, sdp):
    seen += 1

func check():
    var c = WebRTCPeerConnection.new()
    c.connect(\"session_description_created\", _desc)
    c.create_offer()
    var before = seen
    c.poll()
    return [before, seen]
";
    let result = call_check(src);
    let v = array_of(&result);
    assert_eq!(
        v[0],
        Variant::Int(0),
        "create_offer() alone must not fire the signal — the description is \
         queued until the application polls",
    );
    assert_eq!(
        v[1],
        Variant::Int(1),
        "and poll() must then deliver exactly one",
    );
}

/// The channel's SCTP configuration survives the dictionary Godot takes.
#[test]
fn data_channel_configuration_reaches_the_channel() {
    let src = "\
extends Node

func check():
    var c = WebRTCPeerConnection.new()
    var ch = c.create_data_channel(\"cfg\", {\"ordered\": false, \"negotiated\": true, \"id\": 3, \"maxRetransmits\": 2, \"protocol\": \"chat\"})
    return [ch.get_label(), ch.is_ordered(), ch.is_negotiated(), ch.get_id(), ch.get_max_retransmits(), ch.get_protocol(), ch.get_max_packet_life_time()]
";
    let result = call_check(src);
    let v = array_of(&result);
    assert_eq!(v[0], Variant::String("cfg".to_string()));
    assert_eq!(v[1], Variant::Bool(false), "ordered:false must survive");
    assert_eq!(v[2], Variant::Bool(true), "negotiated:true must survive");
    assert_eq!(v[3], Variant::Int(3));
    assert_eq!(v[4], Variant::Int(2));
    assert_eq!(v[5], Variant::String("chat".to_string()));
    // ⚠️ -1, not 0: these SCTP settings are optional, and a script must be able
    // to tell "unset" from a real 0, which would mean "retransmit zero times".
    assert_eq!(
        v[6],
        Variant::Int(-1),
        "an unset maxPacketLifetime must read as -1, not 0",
    );
}

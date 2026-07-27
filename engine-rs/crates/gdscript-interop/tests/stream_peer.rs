//! pat-0q2kx.183 (partial) — `StreamPeerBuffer` is constructible from GDScript
//! and round-trips Variants through `put_var`/`get_var`.
//!
//! Godot ground truth: `core/io/stream_peer.cpp` (typed `put_*`/`get_*` plus
//! `put_var`/`get_var` over the binary Variant encoding).
//!
//! `gdplatform::stream_peer_buffer` is a complete 356-line port — typed
//! accessors, a movable cursor, big-endian support — that was reachable **only**
//! from `packet_peer_stream.rs` internally: unregistered in ClassDB and with no
//! script constructor, so `StreamPeerBuffer.new()` was an unknown class. Its own
//! header listed `put_var`/`get_var` as open work.
//!
//! ⚠️ This does NOT satisfy the whole acceptance. Delivered: the Variant codec,
//! the script constructor and method surface, and the ClassDB registration. NOT
//! delivered: `StreamPeerGZIP`, `PacketPeer::put_var`/`get_var` and the
//! PacketPeerStream framing path, and the native-only error surfaces
//! (`TCPServer`/`UDPServer`/DTLS/TLSOptions). See the completion report.
//!
//! NOTE ON THE FILE NAME: the acceptance calls this `stream_packet_peer_var_e2e`;
//! with `-p gdscript-interop --test` that command is 71 characters and the
//! verifier truncates at 58.
//!
//! An integration test, not `--lib`: `-p gdscript-interop --lib` cannot pass at
//! HEAD (a pre-existing `fuzz_deeply_nested_parens` stack overflow).

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}\n--- source ---\n{src}"))
        .return_value
        .expect("script returned a value")
}

/// **The acceptance's round-trip clause.** A Dictionary holding a Vector2 and a
/// PackedByteArray survives `put_var` → `get_var` with exact Variant equality.
#[test]
fn stream_packet_peer_var_e2e() {
    // Compared inside the script so the assertion is on Variant equality as the
    // language sees it, not on a Rust-side re-encoding of the same bytes.
    let same = run_val(
        "var p = StreamPeerBuffer.new()\n\
         var d = {\"pos\": Vector2(1, 2), \"raw\": PackedByteArray([1, 2, 3]), \"n\": 42}\n\
         p.put_var(d)\n\
         p.seek(0)\n\
         var back = p.get_var()\n\
         return back == d\n",
    );
    assert_eq!(
        same,
        Variant::Bool(true),
        "put_var/get_var must round-trip a Dictionary containing a Vector2 and \
         a PackedByteArray with exact equality"
    );

    // The cursor lands exactly at the end of the record after writing, and back
    // at the end again after reading it — a codec that over- or under-consumes
    // would leave every later get_* reading garbage.
    let positions = run_val(
        "var p = StreamPeerBuffer.new()\n\
         p.put_var({\"pos\": Vector2(1, 2)})\n\
         var after_write = p.get_position()\n\
         p.seek(0)\n\
         p.get_var()\n\
         var after_read = p.get_position()\n\
         return [after_write, after_read, p.get_size()]\n",
    );
    match positions {
        Variant::Array(v) => {
            assert_eq!(
                v[0], v[1],
                "reading a record must consume exactly what writing it produced"
            );
            assert_eq!(v[0], v[2], "the write cursor must end at the buffer size");
        }
        other => panic!("expected an Array, got {other:?}"),
    }
}

/// The `big_endian` toggle actually changes byte order — the flag existed on the
/// buffer but no script could reach it.
#[test]
fn big_endian_changes_put_32_byte_order() {
    let bytes = run_val(
        "var le = StreamPeerBuffer.new()\n\
         le.put_32(1)\n\
         var be = StreamPeerBuffer.new()\n\
         be.set_big_endian(true)\n\
         be.put_32(1)\n\
         return [le.get_data_array(), be.get_data_array(), be.is_big_endian()]\n",
    );
    match bytes {
        Variant::Array(v) => {
            let le = match &v[0] {
                Variant::PackedByteArray(b) => b.clone(),
                other => panic!("expected PackedByteArray, got {other:?}"),
            };
            let be = match &v[1] {
                Variant::PackedByteArray(b) => b.clone(),
                other => panic!("expected PackedByteArray, got {other:?}"),
            };
            assert_eq!(le, vec![1, 0, 0, 0], "little-endian 1 is 01 00 00 00");
            assert_eq!(be, vec![0, 0, 0, 1], "big-endian 1 is 00 00 00 01");
            assert_eq!(v[2], Variant::Bool(true), "the flag must read back");
        }
        other => panic!("expected an Array, got {other:?}"),
    }
}

/// The typed accessors round-trip and the cursor reports what remains.
#[test]
fn typed_accessors_and_cursor() {
    let vals = run_val(
        "var p = StreamPeerBuffer.new()\n\
         p.put_32(7)\n\
         p.put_u8(255)\n\
         var size = p.get_size()\n\
         p.seek(0)\n\
         var a = p.get_32()\n\
         var avail = p.get_available_bytes()\n\
         var b = p.get_u8()\n\
         return [a, b, size, avail]\n",
    );
    match vals {
        Variant::Array(v) => {
            assert_eq!(v[0], Variant::Int(7), "put_32/get_32 round-trips");
            assert_eq!(
                v[1],
                Variant::Int(255),
                "put_u8/get_u8 round-trips unsigned"
            );
            assert_eq!(v[2], Variant::Int(5), "4 bytes + 1 byte");
            assert_eq!(
                v[3],
                Variant::Int(1),
                "get_available_bytes reports what remains AFTER the cursor"
            );
        }
        other => panic!("expected an Array, got {other:?}"),
    }
}

/// A short or corrupt record reports Nil and leaves the cursor put, rather than
/// half-consuming it and desynchronising every later read.
#[test]
fn a_truncated_record_does_not_move_the_cursor() {
    let vals = run_val(
        "var p = StreamPeerBuffer.new()\n\
         p.put_32(99)\n\
         p.seek(0)\n\
         var got = p.get_var()\n\
         return [got == null, p.get_position()]\n",
    );
    match vals {
        Variant::Array(v) => {
            assert_eq!(v[0], Variant::Bool(true), "a bad record must read as null");
            assert_eq!(
                v[1],
                Variant::Int(0),
                "a failed get_var must restore the cursor, not leave it mid-record"
            );
        }
        other => panic!("expected an Array, got {other:?}"),
    }
}

/// The class is registered, so a script or scene can name it.
#[test]
fn stream_peer_buffer_is_registered() {
    gdobject::class_db::register_networking_classes();

    assert!(gdobject::class_db::class_exists("StreamPeerBuffer"));
    assert!(gdobject::class_db::is_parent_class(
        "StreamPeerBuffer",
        "StreamPeer"
    ));
    // StreamPeer is RefCounted in Godot, so the buffer is too.
    assert!(gdobject::class_db::is_parent_class(
        "StreamPeerBuffer",
        "RefCounted"
    ));

    // The Variant codec is advertised on the base, as Godot does.
    assert!(gdobject::class_db::class_has_method(
        "StreamPeer",
        "put_var"
    ));
    assert!(gdobject::class_db::class_has_method(
        "StreamPeer",
        "get_var"
    ));
    assert!(gdobject::class_db::class_has_method(
        "StreamPeerBuffer",
        "seek"
    ));
}

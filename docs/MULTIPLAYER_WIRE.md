# Patina multiplayer wire format (pat-z35r2.4)

## Decision: **Patina-native wire**, not bit-compatible with stock Godot ENet clients

Patina multiplayer is intentional **Patina↔Patina** (and test harnesses). Interop with
upstream Godot `ENetMultiplayerPeer` / `SceneMultiplayer` packets is **out of scope**
until a dedicated interop spike lands.

## UDP peer framing (`ENetMultiplayerPeer` in `gdplatform`)

Magic: `PEN1` (4 bytes)  
Then: version u8, flags u8, channel u8, sender u32 BE, seq u32 BE, payload_len u16 BE, payload.

| Flag / mode | Meaning |
|-------------|---------|
| DATA + Reliable (0) | Retransmit until ACK |
| DATA + Unreliable (1) | Fire-and-forget |
| DATA + Ordered (2) | Reliable + in-order delivery buffer |
| HELLO / WELCOME | Client handshake; server assigns peer id |
| ACK | Clears pending reliable |

## Packet framing (Godot `NETWORK_COMMAND`)

Every multiplayer packet starts with Godot's command byte
(`upstream/godot/modules/multiplayer/scene_multiplayer.h`, `enum NetworkCommands`),
exposed as `gdplatform::network::NetworkCommand`:

| Byte | Command | Status |
|------|---------|--------|
| 0 | `REMOTE_CALL` | implemented |
| 1 | `SIMPLIFY_PATH` | reserved — path cache not yet implemented |
| 2 | `CONFIRM_PATH` | reserved — path cache not yet implemented |
| 3 | `RAW` | implemented (`send_bytes`) |
| 4 | `SPAWN` | reserved |
| 5 | `DESPAWN` | reserved |
| 6 | `SYNC` | reserved |
| 7 | `SYS` | reserved |

A `REMOTE_CALL` packet is: command byte + node_path u64 + method utf8 + argc +
args encoded with `gdvariant::binary::encode_variant` — the same marshalls port
Godot uses, so `Vector2` / `Array` / `Dictionary` round-trip exactly. The
previous `PRPC` / `PRAW` magics and the private 5-type codec (which stringified
anything outside Nil/Bool/Int/Float/String) are gone (pat-0q2kx.95).

Object arguments are refused unless the caller opts in via
`decode_rpc_packet_with(.., ObjectDecoding::Allow)`, mirroring Godot's
`allow_object_decoding`.

**Not yet implemented:** the path-id cache (`SIMPLIFY_PATH`/`CONFIRM_PATH`), so
every call still carries the u64 path hash and the receiver still resolves it
with an O(n) node scan.

See `encode_rpc_packet` / `decode_rpc_packet` in `gdplatform::network`.

## Compatibility matrix claim

Mark multiplayer as **Claimed / Partial** for Godot peer interop; **Measured** only for
Patina loopback UDP + MockNetwork tests.

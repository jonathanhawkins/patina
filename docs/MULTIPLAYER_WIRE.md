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

## RPC framing (`PRPC`)

Magic `PRPC` + node_path u64 + method utf8 + argc + Variant subset (Nil/Bool/Int/Float/String).

See `encode_rpc_packet` / `decode_rpc_packet` in `gdplatform::network`.

## Compatibility matrix claim

Mark multiplayer as **Claimed / Partial** for Godot peer interop; **Measured** only for
Patina loopback UDP + MockNetwork tests.

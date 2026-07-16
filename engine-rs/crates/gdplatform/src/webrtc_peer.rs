//! WebRTCMultiplayerPeer stub with offer/answer state + a mock in-memory data
//! channel for CI (pat-p0s82.22).

use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebRtcState {
    New,
    LocalOffer,
    RemoteAnswer,
    Connected,
    Closed,
}

#[derive(Debug)]
pub struct WebRTCMultiplayerPeer {
    pub state: WebRtcState,
    pub local_description: Option<String>,
    pub remote_description: Option<String>,
    /// Data-channel packets received from the remote; drained by `get_packet`.
    incoming: VecDeque<Vec<u8>>,
    /// Data-channel packets queued by `put_packet`, awaiting delivery.
    outgoing: VecDeque<Vec<u8>>,
}

impl Default for WebRTCMultiplayerPeer {
    fn default() -> Self {
        Self::new()
    }
}

impl WebRTCMultiplayerPeer {
    pub fn new() -> Self {
        Self {
            state: WebRtcState::New,
            local_description: None,
            remote_description: None,
            incoming: VecDeque::new(),
            outgoing: VecDeque::new(),
        }
    }
    pub fn create_offer(&mut self) -> String {
        let sdp = "v=0\r\no=- 0 0 IN IP4 127.0.0.1\r\ns=Patina\r\n".to_string();
        self.local_description = Some(sdp.clone());
        self.state = WebRtcState::LocalOffer;
        sdp
    }
    pub fn set_remote_description(&mut self, sdp: impl Into<String>) {
        self.remote_description = Some(sdp.into());
        if self.state == WebRtcState::LocalOffer {
            self.state = WebRtcState::RemoteAnswer;
        }
    }
    pub fn poll_connected(&mut self) {
        if self.state == WebRtcState::RemoteAnswer {
            self.state = WebRtcState::Connected;
        }
    }

    /// Whether the data channel is open (the peer has reached `Connected`).
    pub fn is_connected(&self) -> bool {
        self.state == WebRtcState::Connected
    }

    /// Queues a packet on the outgoing data channel, mirroring Godot's
    /// `PacketPeer.put_packet`. It is not visible to the remote until the mock
    /// transport delivers it via [`deliver_to`](Self::deliver_to).
    pub fn put_packet(&mut self, bytes: Vec<u8>) {
        self.outgoing.push_back(bytes);
    }

    /// Pops the next received data-channel packet, if any
    /// (`PacketPeer.get_packet`).
    pub fn get_packet(&mut self) -> Option<Vec<u8>> {
        self.incoming.pop_front()
    }

    /// Number of buffered incoming packets ready to read
    /// (`PacketPeer.get_available_packet_count`).
    pub fn available_packet_count(&self) -> usize {
        self.incoming.len()
    }

    /// Mock signaling/transport: once both peers are `Connected`, flush this
    /// peer's queued outgoing packets into `remote`'s incoming buffer. Real
    /// WebRTC carries these over the network's data channel; the mock delivers
    /// them in-memory so the peer is testable in CI. No-op until connected.
    pub fn deliver_to(&mut self, remote: &mut WebRTCMultiplayerPeer) {
        if self.is_connected() && remote.is_connected() {
            while let Some(pkt) = self.outgoing.pop_front() {
                remote.incoming.push_back(pkt);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offer_answer_flow() {
        let mut a = WebRTCMultiplayerPeer::new();
        let offer = a.create_offer();
        assert_eq!(a.state, WebRtcState::LocalOffer);
        a.set_remote_description("answer");
        a.poll_connected();
        assert_eq!(a.state, WebRtcState::Connected);
        assert!(offer.contains("Patina"));
    }

    /// Mock signaling handshake + data-channel round-trip (pat-p0s82.22): two
    /// peers exchange SDP, reach Connected, then send packets both ways through
    /// the in-memory mock transport.
    #[test]
    fn mock_signaling_data_channel_roundtrip() {
        let mut a = WebRTCMultiplayerPeer::new();
        let mut b = WebRTCMultiplayerPeer::new();

        // Mock signaling: each peer offers, takes the other's SDP as its remote
        // description, then polls to Connected.
        let a_sdp = a.create_offer();
        let b_sdp = b.create_offer();
        a.set_remote_description(b_sdp);
        b.set_remote_description(a_sdp);
        a.poll_connected();
        b.poll_connected();
        assert!(a.is_connected() && b.is_connected());

        // A sends a packet; it only reaches B once the mock transport runs.
        a.put_packet(vec![1, 2, 3]);
        assert_eq!(b.available_packet_count(), 0);
        a.deliver_to(&mut b);
        assert_eq!(b.available_packet_count(), 1);
        assert_eq!(b.get_packet(), Some(vec![1, 2, 3]));
        assert_eq!(b.get_packet(), None);

        // The reverse direction works too.
        b.put_packet(vec![9, 9]);
        b.deliver_to(&mut a);
        assert_eq!(a.get_packet(), Some(vec![9, 9]));

        // Packets are not delivered before both peers connect.
        let mut c = WebRTCMultiplayerPeer::new();
        let mut d = WebRTCMultiplayerPeer::new();
        c.put_packet(vec![7]);
        c.deliver_to(&mut d); // neither connected → no-op
        assert_eq!(d.available_packet_count(), 0);
    }
}

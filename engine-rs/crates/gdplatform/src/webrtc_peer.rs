//! WebRTCMultiplayerPeer stub with offer/answer state (pat-p0s82.22).

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
}

//! VideoStreamPlayer API stub with frame counter (pat-p0s82.28).

#[derive(Debug, Clone)]
pub struct VideoStreamPlayer {
    pub playing: bool,
    pub position_secs: f64,
    pub length_secs: f64,
    pub stream_path: String,
}

impl Default for VideoStreamPlayer {
    fn default() -> Self {
        Self {
            playing: false,
            position_secs: 0.0,
            length_secs: 0.0,
            stream_path: String::new(),
        }
    }
}

impl VideoStreamPlayer {
    pub fn play(&mut self) {
        self.playing = true;
    }
    pub fn stop(&mut self) {
        self.playing = false;
        self.position_secs = 0.0;
    }
    pub fn advance(&mut self, dt: f64) {
        if self.playing {
            self.position_secs = (self.position_secs + dt).min(self.length_secs.max(0.0));
            if self.length_secs > 0.0 && self.position_secs >= self.length_secs {
                self.playing = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn play_advances() {
        let mut v = VideoStreamPlayer {
            length_secs: 1.0,
            ..Default::default()
        };
        v.play();
        v.advance(0.5);
        assert!((v.position_secs - 0.5).abs() < 1e-9);
        assert!(v.playing);
    }
}

//! XRServer / OpenXR mock (pat-p0s82.23).

use gdcore::math::Vector3;

#[derive(Debug, Clone, Copy)]
pub struct XrPose {
    pub position: Vector3,
    pub yaw: f32,
}

#[derive(Debug, Default)]
pub struct XrServer {
    pub session_active: bool,
    pub head: XrPose,
}

impl Default for XrPose {
    fn default() -> Self {
        Self {
            position: Vector3::ZERO,
            yaw: 0.0,
        }
    }
}

impl XrServer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn start_mock_session(&mut self) {
        self.session_active = true;
        self.head = XrPose {
            position: Vector3::new(0.0, 1.6, 0.0),
            yaw: 0.0,
        };
    }
    pub fn get_head_pose(&self) -> Option<XrPose> {
        if self.session_active {
            Some(self.head)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mock_session_pose() {
        let mut s = XrServer::new();
        assert!(s.get_head_pose().is_none());
        s.start_mock_session();
        let p = s.get_head_pose().unwrap();
        assert!((p.position.y - 1.6).abs() < 1e-5);
    }
}

//! Depth buffer for 3D rendering.

/// A depth buffer (Z-buffer) for hidden surface removal.
#[derive(Debug, Clone)]
pub struct DepthBuffer {
    width: u32,
    height: u32,
    data: Vec<f32>,
}

impl DepthBuffer {
    /// Creates a new depth buffer initialized to max depth (far plane).
    pub fn new(width: u32, height: u32) -> Self {
        let count = (width * height) as usize;
        Self {
            width,
            height,
            data: vec![f32::MAX; count],
        }
    }

    /// Clears the buffer to the given depth value.
    pub fn clear(&mut self, value: f32) {
        self.data.fill(value);
    }

    /// Returns the depth at the given pixel coordinate.
    pub fn get(&self, x: u32, y: u32) -> f32 {
        if x < self.width && y < self.height {
            self.data[(y * self.width + x) as usize]
        } else {
            f32::MAX
        }
    }

    /// Tests and sets the depth at (x, y). Returns `true` if the new depth
    /// is closer than the existing value (meaning the pixel should be drawn).
    pub fn test_and_set(&mut self, x: u32, y: u32, depth: f32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let idx = (y * self.width + x) as usize;
        if depth < self.data[idx] {
            self.data[idx] = depth;
            true
        } else {
            false
        }
    }

    /// Returns the buffer dimensions.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Consumes the buffer and returns normalized depth values.
    ///
    /// Pixels that were never written (`f32::MAX`) are mapped to `1.0`.
    /// All other values are clamped to `[0.0, 1.0]`.
    pub fn into_normalized(self) -> Vec<f32> {
        self.data
            .into_iter()
            .map(|d| {
                if d >= f32::MAX {
                    1.0
                } else {
                    d.clamp(0.0, 1.0)
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_buffer_initialized_to_max() {
        let buf = DepthBuffer::new(4, 4);
        assert_eq!(buf.get(0, 0), f32::MAX);
        assert_eq!(buf.get(3, 3), f32::MAX);
    }

    #[test]
    fn test_and_set_closer_wins() {
        let mut buf = DepthBuffer::new(4, 4);
        assert!(buf.test_and_set(1, 1, 10.0));
        assert_eq!(buf.get(1, 1), 10.0);
        assert!(buf.test_and_set(1, 1, 5.0));
        assert_eq!(buf.get(1, 1), 5.0);
        assert!(!buf.test_and_set(1, 1, 8.0));
        assert_eq!(buf.get(1, 1), 5.0);
    }

    #[test]
    fn out_of_bounds_returns_max() {
        let buf = DepthBuffer::new(4, 4);
        assert_eq!(buf.get(10, 10), f32::MAX);
    }

    #[test]
    fn out_of_bounds_test_and_set_returns_false() {
        let mut buf = DepthBuffer::new(4, 4);
        assert!(!buf.test_and_set(10, 10, 1.0));
    }

    #[test]
    fn clear_resets_all() {
        let mut buf = DepthBuffer::new(4, 4);
        buf.test_and_set(0, 0, 5.0);
        buf.clear(100.0);
        assert_eq!(buf.get(0, 0), 100.0);
    }

    #[test]
    fn dimensions() {
        let buf = DepthBuffer::new(16, 8);
        assert_eq!(buf.dimensions(), (16, 8));
    }

    #[test]
    fn into_normalized_unwritten_is_one() {
        let buf = DepthBuffer::new(2, 2);
        let data = buf.into_normalized();
        assert_eq!(data.len(), 4);
        assert!(data.iter().all(|&d| d == 1.0));
    }

    #[test]
    fn into_normalized_written_values_preserved() {
        let mut buf = DepthBuffer::new(2, 2);
        buf.test_and_set(0, 0, 0.5);
        buf.test_and_set(1, 1, 0.25);
        let data = buf.into_normalized();
        assert_eq!(data[0], 0.5);
        assert_eq!(data[3], 0.25);
        assert_eq!(data[1], 1.0); // unwritten
    }
}

//! Projection matrix utilities.

/// Computes a perspective projection matrix.
///
/// Returns a column-major 4x4 matrix matching Godot's projection conventions.
pub fn perspective_projection_matrix(fov: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    let half_fov_tan = (fov * 0.5).tan();
    let f = 1.0 / half_fov_tan;
    let range_inv = 1.0 / (near - far);

    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, (far + near) * range_inv, -1.0],
        [0.0, 0.0, 2.0 * far * near * range_inv, 0.0],
    ]
}

/// Computes an orthographic projection matrix.
///
/// Returns a column-major 4x4 matrix.
pub fn orthographic_projection_matrix(
    left: f32,
    right: f32,
    bottom: f32,
    top: f32,
    near: f32,
    far: f32,
) -> [[f32; 4]; 4] {
    let rml = right - left;
    let tmb = top - bottom;
    let fmn = far - near;

    [
        [2.0 / rml, 0.0, 0.0, 0.0],
        [0.0, 2.0 / tmb, 0.0, 0.0],
        [0.0, 0.0, -2.0 / fmn, 0.0],
        [
            -(right + left) / rml,
            -(top + bottom) / tmb,
            -(far + near) / fmn,
            1.0,
        ],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-5;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    #[test]
    fn perspective_90_deg_unit_aspect() {
        let m = perspective_projection_matrix(std::f32::consts::FRAC_PI_2, 1.0, 0.1, 100.0);
        assert!(approx_eq(m[0][0], 1.0));
        assert!(approx_eq(m[1][1], 1.0));
        assert!(approx_eq(m[2][3], -1.0));
        assert!(approx_eq(m[3][3], 0.0));
    }

    #[test]
    fn perspective_aspect_scaling() {
        let m = perspective_projection_matrix(std::f32::consts::FRAC_PI_2, 2.0, 0.1, 100.0);
        assert!(approx_eq(m[0][0], 0.5));
        assert!(approx_eq(m[1][1], 1.0));
    }

    #[test]
    fn orthographic_symmetric() {
        let m = orthographic_projection_matrix(-1.0, 1.0, -1.0, 1.0, 0.0, 1.0);
        assert!(approx_eq(m[0][0], 1.0));
        assert!(approx_eq(m[1][1], 1.0));
        assert!(approx_eq(m[3][0], 0.0)); // centered
        assert!(approx_eq(m[3][1], 0.0)); // centered
        assert!(approx_eq(m[3][3], 1.0));
    }

    #[test]
    fn orthographic_asymmetric() {
        let m = orthographic_projection_matrix(0.0, 100.0, 0.0, 50.0, 0.1, 100.0);
        assert!(approx_eq(m[0][0], 2.0 / 100.0));
        assert!(approx_eq(m[1][1], 2.0 / 50.0));
    }
}

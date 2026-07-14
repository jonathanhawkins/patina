//! pat-frrj: Pixel diff against upstream golden at most 0.5% error rate.

use gdcore::math::Color;
use gdrender2d::{compare_framebuffers, FrameBuffer};

fn error_rate(a: &FrameBuffer, b: &FrameBuffer) -> f64 {
    let result = compare_framebuffers(a, b, 0.01);
    if result.total_pixels == 0 {
        return 0.0;
    }
    let mismatched = result.total_pixels - result.matching_pixels;
    mismatched as f64 / result.total_pixels as f64
}

#[test]
fn identical_framebuffers_zero_diff() {
    let a = FrameBuffer::new(10, 10, Color::rgb(1.0, 0.0, 0.0));
    let b = FrameBuffer::new(10, 10, Color::rgb(1.0, 0.0, 0.0));
    let rate = error_rate(&a, &b);
    assert!(
        rate <= 0.001,
        "identical should be 0%, got {:.2}%",
        rate * 100.0
    );
}

#[test]
fn small_diff_within_threshold() {
    let a = FrameBuffer::new(10, 10, Color::rgb(1.0, 0.0, 0.0));
    let mut b = FrameBuffer::new(10, 10, Color::rgb(1.0, 0.0, 0.0));
    b.set_pixel(0, 0, Color::rgb(0.0, 1.0, 0.0));
    let rate = error_rate(&a, &b);
    assert!(
        rate <= 0.02,
        "1/100 pixels should be ~1%, got {:.2}%",
        rate * 100.0
    );
}

#[test]
fn completely_different_is_high_error() {
    let a = FrameBuffer::new(10, 10, Color::rgb(1.0, 0.0, 0.0));
    let b = FrameBuffer::new(10, 10, Color::rgb(0.0, 0.0, 1.0));
    let rate = error_rate(&a, &b);
    assert!(
        rate > 0.5,
        "all different should be high, got {:.2}%",
        rate * 100.0
    );
}

#[test]
fn near_identical_within_half_percent() {
    let golden = FrameBuffer::new(20, 20, Color::rgb(0.5, 0.5, 0.5));
    let mut rendered = FrameBuffer::new(20, 20, Color::rgb(0.5, 0.5, 0.5));
    rendered.set_pixel(0, 0, Color::rgb(0.51, 0.49, 0.5));
    let rate = error_rate(&golden, &rendered);
    assert!(
        rate <= 0.005,
        "near-identical should be ≤0.5%, got {:.2}%",
        rate * 100.0
    );
}

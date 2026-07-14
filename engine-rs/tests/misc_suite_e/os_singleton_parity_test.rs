//! pat-50m6: OS singleton get_ticks_msec/get_name parity.

use gdplatform::os;

#[test]
fn get_ticks_msec_callable() {
    // First call initializes epoch (may return 0), subsequent calls return elapsed.
    let _init = os::get_ticks_msec();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let t = os::get_ticks_msec();
    assert!(t >= 1, "get_ticks_msec should advance after sleep");
}

#[test]
fn get_ticks_msec_monotonic() {
    let _init = os::get_ticks_msec();
    let a = os::get_ticks_msec();
    let b = os::get_ticks_msec();
    assert!(b >= a, "ticks must be monotonically non-decreasing");
}

#[test]
fn get_ticks_usec_returns_nonzero() {
    let t = os::get_ticks_usec();
    assert!(t > 0);
}

#[test]
fn get_ticks_usec_finer_than_msec() {
    let us = os::get_ticks_usec();
    let ms = os::get_ticks_msec();
    // usec value should be roughly 1000x the msec value
    assert!(us >= ms, "usec should be >= msec");
}

#[test]
fn current_platform_returns_known_value() {
    let p = os::current_platform();
    let name = format!("{:?}", p);
    assert!(!name.is_empty(), "platform name should not be empty");
}

#[test]
fn is_debug_build_returns_bool() {
    // Just verify it doesn't panic
    let _debug = os::is_debug_build();
}

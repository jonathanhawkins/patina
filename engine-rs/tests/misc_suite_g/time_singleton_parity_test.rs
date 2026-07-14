//! pat-aaaj: Time singleton get_ticks_usec parity.

use gdplatform::os;

#[test]
fn get_ticks_usec_callable() {
    let _init = os::get_ticks_usec();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let t = os::get_ticks_usec();
    assert!(
        t >= 1000,
        "should advance at least 1000us after 2ms sleep, got {t}"
    );
}

#[test]
fn get_ticks_usec_monotonic() {
    let _init = os::get_ticks_usec();
    let a = os::get_ticks_usec();
    let b = os::get_ticks_usec();
    assert!(b >= a, "must be monotonic");
}

#[test]
fn get_ticks_usec_microsecond_resolution() {
    let _init = os::get_ticks_usec();
    std::thread::sleep(std::time::Duration::from_micros(100));
    let a = os::get_ticks_usec();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let b = os::get_ticks_usec();
    let diff = b - a;
    assert!(
        diff >= 1000,
        "5ms sleep should show >= 1000us difference, got {diff}"
    );
}

#[test]
fn get_ticks_msec_consistent_with_usec() {
    let _init_us = os::get_ticks_usec();
    let _init_ms = os::get_ticks_msec();
    std::thread::sleep(std::time::Duration::from_millis(10));
    let us = os::get_ticks_usec();
    let ms = os::get_ticks_msec();
    // usec / 1000 should be roughly equal to msec (within 50ms tolerance)
    let us_as_ms = us / 1000;
    let diff = if us_as_ms > ms {
        us_as_ms - ms
    } else {
        ms - us_as_ms
    };
    assert!(
        diff < 50,
        "usec/1000 and msec should be within 50ms, diff={diff}"
    );
}

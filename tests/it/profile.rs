//! The test profile optimises our code and keeps the checks that catch
//! mistakes.

use std::hint::black_box;
use std::panic::catch_unwind;

#[test]
fn the_test_profile_keeps_debug_assertions() {
    assert!(catch_unwind(|| debug_assert!(black_box(false))).is_err());
}

#[test]
fn the_test_profile_keeps_overflow_checks() {
    let one = black_box(1_u8);
    assert!(catch_unwind(|| black_box(u8::MAX) + one).is_err());
}

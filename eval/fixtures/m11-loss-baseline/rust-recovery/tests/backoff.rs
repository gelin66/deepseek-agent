use cw_rust_recovery::retry_delay_ms;

#[test]
fn attempt_is_zero_based_and_capped() {
    assert_eq!(retry_delay_ms(0, 100, 5_000), Ok(100));
    assert_eq!(retry_delay_ms(1, 100, 5_000), Ok(200));
    assert_eq!(retry_delay_ms(12, 100, 5_000), Ok(5_000));
}

#[test]
fn large_attempt_does_not_overflow() {
    assert_eq!(retry_delay_ms(200, 250, 4_000), Ok(4_000));
}

#[test]
fn invalid_bounds_are_rejected() {
    assert!(retry_delay_ms(0, 0, 100).is_err());
    assert!(retry_delay_ms(0, 200, 100).is_err());
}

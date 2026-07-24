use fixture::retry::backoff::bounded_retry_delay;

#[test]
fn retry_delay_is_bounded() {
    assert_eq!(bounded_retry_delay(10, 500), 500);
}

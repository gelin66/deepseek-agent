use m18_event_id::identity::normalize_event_id;

#[test]
fn keeps_simple_event_id() {
    assert_eq!(normalize_event_id("dse.run.started").unwrap(), "dse.run.started");
}

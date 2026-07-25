use m15_line_framer::LineFramer;

#[test]
fn emits_complete_lines() {
    let mut framer = LineFramer::default();
    assert_eq!(framer.push(b"alpha\nbeta\n").unwrap(), ["alpha", "beta"]);
}

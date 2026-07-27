use journal_reopen::{append_record, reopen_records};

#[test]
fn complete_records_reopen() {
    let mut bytes = Vec::new();
    append_record(&mut bytes, "reef");
    assert_eq!(reopen_records(&bytes).unwrap(), vec!["reef"]);
}

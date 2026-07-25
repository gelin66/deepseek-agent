use m18_netstring::NetstringDecoder;

#[test]
fn decodes_one_complete_record() {
    let mut decoder = NetstringDecoder::default();
    assert_eq!(decoder.push(b"2:ok,").unwrap(), ["ok"]);
    assert_eq!(decoder.finish(), Ok(()));
}

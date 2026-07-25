use m18_netstring::NetstringDecoder;

fn main() {
    let mut decoder = NetstringDecoder::default();
    println!("{:?}", decoder.push(b"5:hel"));
    println!("{:?}", decoder.push(b"lo,5:ca"));
    println!("{:?}", decoder.push(b"f\xc3\xa9,0:,"));
    println!("{:?}", decoder.finish());

    let mut invalid_length = NetstringDecoder::default();
    println!("{:?}", invalid_length.push(b"02:ok,"));

    let mut invalid_utf8 = NetstringDecoder::default();
    println!("{:?}", invalid_utf8.push(b"1:\xff,"));

    let mut incomplete = NetstringDecoder::default();
    println!("{:?}", incomplete.push(b"4:abc"));
    println!("{:?}", incomplete.finish());
}

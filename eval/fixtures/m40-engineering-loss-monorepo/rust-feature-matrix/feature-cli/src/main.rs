fn main() {
    let value = std::env::args().nth(1).unwrap_or_default();
    println!("{}", feature_core::encode_record(&value));
}

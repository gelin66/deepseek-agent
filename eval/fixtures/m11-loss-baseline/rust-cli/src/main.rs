use cw_rust_cli::parse_bind_address;

fn main() {
    let mut arguments = std::env::args().skip(1);
    let Some(value) = arguments.next() else {
        eprintln!("usage: cw-rust-cli <host:port>");
        std::process::exit(2);
    };
    if arguments.next().is_some() {
        eprintln!("expected exactly one address");
        std::process::exit(2);
    }
    match parse_bind_address(&value) {
        Ok((host, port)) => println!("{host}\t{port}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}

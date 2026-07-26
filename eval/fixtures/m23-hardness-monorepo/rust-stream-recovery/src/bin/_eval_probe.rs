use m15_line_framer::LineFramer;

fn main() {
    let mut framer = LineFramer::default();
    println!("{:?}", framer.push(b"alpha\r\nbe"));
    println!("{:?}", framer.push(b"ta\ncaf\xc3"));
    println!("{:?}", framer.push(b"\xa9\nlast"));
    println!("{:?}", framer.finish());

    let mut invalid = LineFramer::default();
    println!("{:?}", invalid.push(b"bad\xff\n"));
}

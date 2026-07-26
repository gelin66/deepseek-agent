use m15_wire_version::wire::normalize_wire_version;

fn main() {
    for value in [" V4_Pro ", "api.2", "release-2026"] {
        println!("{:?}", normalize_wire_version(value));
    }
    for value in ["-v4", "v4..pro", "v4_", "v 4", "版本4", "v4/pro"] {
        println!("{:?}", normalize_wire_version(value));
    }
}

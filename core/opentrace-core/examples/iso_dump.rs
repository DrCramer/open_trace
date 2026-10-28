use opentrace_core::project_store::iso_has_tz;
use std::io::{BufRead, BufReader};
fn main() {
    let f = std::fs::File::open(std::env::args().nth(1).unwrap()).unwrap();
    for (i, l) in BufReader::new(f).lines().enumerate() {
        let s: String = serde_json::from_str(&l.unwrap()).unwrap();
        let r = match iso_has_tz(&s.replace('Z', "+00:00")) {
            Err(()) => "ERR",
            Ok(false) => "NOTZ",
            Ok(true) => "TZ",
        };
        println!("ISO {} {}", i, r);
    }
}

use opentrace_core::project_store::iso_parse;
use std::io::{BufRead, BufReader};
fn main() {
    let f = std::fs::File::open(std::env::args().nth(1).unwrap()).unwrap();
    for (i, l) in BufReader::new(f).lines().enumerate() {
        let s: String = serde_json::from_str(&l.unwrap()).unwrap();
        let r = match iso_parse(&s.replace('Z', "+00:00")) {
            Err(()) => "ERR".to_string(),
            Ok(None) => "NOTZ".to_string(),
            Ok(Some(m)) => format!("TZ {}", m),
        };
        println!("ISO {} {}", i, r);
    }
}

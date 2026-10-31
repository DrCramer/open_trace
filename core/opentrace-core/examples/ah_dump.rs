//! Autoheight over the same sample channels as scratchpad/ah_check.py (values come from the Rust ABIF reader).
use opentrace_core::abif::{AbifFile, Num};
use opentrace_core::autoheight::estimate_standard_autoheight;
use std::io::{BufRead, BufReader};

fn hb(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

fn main() {
    let list = std::env::args().nth(1).unwrap();
    let names: Vec<String> = BufReader::new(std::fs::File::open(list).unwrap()).lines().map(|l| l.unwrap()).collect();
    for path in names.iter().flat_map(|l| l.split_whitespace()) {
        let data = std::fs::read(path).unwrap();
        let fname = path.rsplit('/').next().unwrap();
        let chans = AbifFile::parse(&data, fname).unwrap().channels().unwrap();
        for tag in ["DATA105", "DATA1", "DATA2", "DATA3", "DATA4", "DATA106"] {
            let Some(ch) = chans.iter().find(|c| c.tag == tag) else { continue };
            if ch.values.len() < 250 {
                continue;
            }
            // Python: values must be exact ints; a float channel would be rejected by the reference.
            let raw: Vec<i64> = ch.values.iter().map(|v| if let Num::Int(i) = v { *i } else { 0 }).collect();
            let baseline = opentrace_core::preprocessing::estimate_baseline(&raw, 25, 2, 0);
            let corrected = opentrace_core::preprocessing::correct(&raw, &baseline);
            let ints: Option<Vec<i64>> = Some(opentrace_core::preprocessing::smooth_trace(&corrected, 2));
            let Some(ints) = ints else {
                println!("AH {} {} ERR float values", fname, tag);
                continue;
            };
            match estimate_standard_autoheight(&ints) {
                Ok(r) => println!("AH {} {} {} {} {}", fname, tag, hb(r.threshold), hb(r.median_control), r.control.iter().sum::<i64>()),
                Err(e) => println!("AH {} {} ERR {}", fname, tag, e),
            }
        }
    }
}

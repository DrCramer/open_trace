//! Dump `AbifFile.channels()` in the same text format as scratchpad/abif_dump.py (bit-exact comparison).
use opentrace_core::abif::{AbifFile, Num};
use std::fs;

fn main() {
    for path in std::env::args().skip(1) {
        let data = fs::read(&path).expect("read");
        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
        match AbifFile::parse(&data, &name).and_then(|f| f.channels()) {
            Err(e) => println!("{}|ERROR|{}", name, e),
            Ok(chs) => {
                for c in chs {
                    let dye_index = c.dye_index.map(|v| v.to_string()).unwrap_or("-".into());
                    let wl = c.wavelength_nm.map(|v| v.to_string()).unwrap_or("-".into());
                    let tr = c.truncated_to.map(|v| v.to_string()).unwrap_or("-".into());
                    let mut line = format!("{}|{}|{}|{}|{}|{}|{}|{}|", name, c.tag, c.group, dye_index, c.dye, wl, tr, c.values.len());
                    for v in &c.values {
                        match v {
                            Num::Int(i) => line.push_str(&format!("i{} ", i)),
                            Num::Float(f) => line.push_str(&format!("f{:016x} ", f.to_bits())),
                        }
                    }
                    println!("{}", line.trim_end());
                }
            }
        }
    }
}

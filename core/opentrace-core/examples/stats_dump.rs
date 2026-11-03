use opentrace_core::pystats;
use std::io::BufRead;
fn main() {
    for line in std::io::BufReader::new(std::fs::File::open(std::env::args().nth(1).unwrap()).unwrap()).lines() {
        let vals: Vec<f64> = line.unwrap().split_whitespace().map(|h| f64::from_bits(u64::from_str_radix(h, 16).unwrap())).collect();
        let f = |r: Result<f64, String>| match r { Ok(x) => format!("{:016x}", x.to_bits()), Err(e) => format!("ERR {}", e) };
        println!("{} {}", f(pystats::mean(&vals)), f(pystats::stdev(&vals)));
    }
}

//! Rust counterpart of scratchpad/sgw_check.py: general SG derivative weights, bit-exact hex output.
use opentrace_core::sg_weights::sg_derivative_weights;

fn degrees(_w: usize) -> Vec<usize> {
    (1..100).collect()
}

fn main() {
    for w in (5..=101).step_by(2) {
        for d in degrees(w) {
            match sg_derivative_weights(w, d) {
                Ok(ws) => {
                    let hex: Vec<String> = ws.iter().map(|x| format!("{:016x}", x.to_bits())).collect();
                    println!("W {} {} {}", w, d, hex.join(" "));
                }
                Err(e) => println!("W {} {} ERR {}", w, d, e),
            }
        }
    }
}

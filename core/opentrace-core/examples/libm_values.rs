//! Prints exp/log/pow/expm1/sin results (bit patterns) for a deterministic argument list, one per line, so two
//! platforms' outputs can be diffed: `cargo run --release --example libm_values -- 200000 > values.txt`.
fn main() {
    let n: usize = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(100_000);
    let mut s: u64 = 0xD1B5_4A32_D192_ED03;
    let mut u = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f64 / (1u64 << 53) as f64
    };
    for i in 0..n {
        let e = match i % 3 {
            0 => -745.0 * u(),
            1 => -1e-3 * u(),
            _ => -50.0 * u(),
        };
        let x = match i % 3 {
            0 => std::f64::consts::PI * (2.0 * u()),
            1 => (u() - 0.5) * 2e-6,
            _ => (u() - 0.5) * 2000.0,
        };
        println!("expm1 {:016x} {:016x}", e.to_bits(), opentrace_core::cmath::expm1(e).to_bits());
        println!("sin {:016x} {:016x}", x.to_bits(), opentrace_core::cmath::sin(x).to_bits());
    }
}

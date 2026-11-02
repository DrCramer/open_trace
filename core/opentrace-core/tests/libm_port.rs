//! `libm_port` (Rust port of glibc exp/log/pow, FMA variant) against the system glibc, and a cross-platform
//! fingerprint of its results.
use opentrace_core::libm_port;
use sha2::{Digest, Sha256};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn u01(&mut self) -> f64 {
        (self.next() >> 11) as f64 * (1.0 / 9007199254740992.0)
    }
    /// Any finite double with a uniformly random exponent in [lo, hi].
    fn wide(&mut self, lo: i32, hi: i32) -> f64 {
        let e = lo + (self.next() % (hi - lo + 1) as u64) as i32;
        let m = self.next() & 0x000f_ffff_ffff_ffff;
        f64::from_bits((((e + 1023) as u64) << 52) | m)
    }
}

/// Deterministic argument mix covering the analysis' use (Gaussians, gamma/log-likelihoods, powers) and the
/// special/edge ranges (subnormal, near 1, overflow/underflow boundaries, negative bases with integer exponents).
fn cases(n: usize, seed: u64) -> Vec<(f64, f64, f64, f64)> {
    let mut r = Rng(seed);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let ex = match i % 5 {
            0 => -60.0 + 70.0 * r.u01(),
            1 => -745.5 + 1455.5 * r.u01(),
            2 => (r.u01() - 0.5) * 1e-6,
            3 => if r.next() & 1 == 1 { r.wide(-60, 9) } else { -r.wide(-60, 9) },
            _ => {
                let t = (r.u01() - 0.5) * 40.0;
                -0.5 * (t * t)
            }
        };
        let lx = match i % 4 {
            0 => r.wide(-1022, 1023),
            1 => 0.9 + 0.2 * r.u01(),
            2 => f64::from_bits(r.next() & 0x000f_ffff_ffff_ffff | 1), // subnormal
            _ => 1e-3 + 1e6 * r.u01(),
        };
        let (px, py) = match i % 6 {
            0 => (1e-3 + 2000.0 * r.u01(), -4.0 + 8.0 * r.u01()),
            1 => (r.wide(-200, 200), -5.0 + 10.0 * r.u01()),
            2 => (-(1.0 + 50.0 * r.u01()), (r.next() % 21) as f64 - 10.0),
            3 => (0.5 + r.u01(), -800.0 + 1600.0 * r.u01()),
            4 => (1e-3 + 100.0 * r.u01(), [2.0, 0.5, -1.0, 3.0, 1.5, -2.0][(r.next() % 6) as usize]),
            _ => (r.wide(-1022, 1023), (r.u01() - 0.5) * 1e-3),
        };
        out.push((ex, lx, px, py));
    }
    out
}

fn fingerprint(n: usize) -> String {
    let mut h = Sha256::new();
    for (ex, lx, px, py) in cases(n, 0x9E37_79B9_7F4A_7C15) {
        h.update(libm_port::exp(ex).to_bits().to_le_bytes());
        h.update(libm_port::log(lx).to_bits().to_le_bytes());
        h.update(libm_port::pow(px, py).to_bits().to_le_bytes());
    }
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

/// Recorded on Linux x86-64 where the port equals glibc 2.39's FMA variant bit for bit (see the test below). Every
/// platform must reproduce it: this is what makes Windows results identical to the Linux server.
const FINGERPRINT_1M: &str = "3498acae80c38bc9fc32c7129bcc18df07e1f02586f64260d036b29e11adb7b9";

#[test]
fn identical_results_on_every_platform() {
    let got = fingerprint(1_000_000);
    if FINGERPRINT_1M.starts_with("FILLED") {
        panic!("record this fingerprint: {}", got);
    }
    assert_eq!(got, FINGERPRINT_1M);
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn bit_identical_to_glibc_fma_variant() {
    extern "C" {
        fn exp(x: f64) -> f64;
        fn log(x: f64) -> f64;
        fn pow(x: f64, y: f64) -> f64;
    }
    if !std::arch::is_x86_feature_detected!("fma") {
        eprintln!("CPU without FMA: glibc runs its non-FMA variant here; nothing to compare against");
        return;
    }
    let n: usize = std::env::var("LIBM_PORT_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(3_000_000);
    let (mut de, mut dl, mut dp) = (0usize, 0usize, 0usize);
    let mut shown = 0;
    for (ex, lx, px, py) in cases(n, 0x1234_5678_9abc_def1) {
        let (a, b) = (libm_port::exp(ex), unsafe { exp(std::hint::black_box(ex)) });
        if a.to_bits() != b.to_bits() && !(a.is_nan() && b.is_nan()) {
            de += 1;
            if shown < 5 {
                eprintln!("exp({:e}) port {:e} glibc {:e}", ex, a, b);
                shown += 1;
            }
        }
        let (a, b) = (libm_port::log(lx), unsafe { log(std::hint::black_box(lx)) });
        if a.to_bits() != b.to_bits() && !(a.is_nan() && b.is_nan()) {
            dl += 1;
            if shown < 5 {
                eprintln!("log({:e}) port {:e} glibc {:e}", lx, a, b);
                shown += 1;
            }
        }
        let (a, b) = (libm_port::pow(px, py), unsafe { pow(std::hint::black_box(px), std::hint::black_box(py)) });
        if a.to_bits() != b.to_bits() && !(a.is_nan() && b.is_nan()) {
            dp += 1;
            if shown < 5 {
                eprintln!("pow({:e}, {:e}) port {:e} glibc {:e}", px, py, a, b);
                shown += 1;
            }
        }
    }
    assert_eq!((de, dl, dp), (0, 0, 0), "differences in {} cases each (exp, log, pow)", n);
}

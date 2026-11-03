//! Run `wrap_standard_peaks` over the same CASE/P input as the prefilter harness (see scratchpad/wrap_check.py).
use opentrace_core::prefilters::PeakIn;
use opentrace_core::wrapper::wrap_standard_peaks;
use std::io::{BufRead, BufReader};

fn bits(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}
fn hb(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

fn main() {
    let path = std::env::args().nth(1).expect("input path");
    let reader = BufReader::new(std::fs::File::open(path).unwrap());
    let lines: Vec<String> = reader.lines().map(|l| l.unwrap()).collect();
    let mut signal_len = 0usize;
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        if let Some(rest) = line.strip_prefix("SIG ") {
            signal_len = rest.split(' ').count();
        } else if line.starts_with("CASE ") {
            let n: usize = line.split(' ').nth(3).unwrap().parse().unwrap();
            let mut peaks = Vec::with_capacity(n);
            for k in 0..n {
                let v: Vec<&str> = lines[i + 1 + k][2..].split(' ').collect();
                peaks.push(PeakIn {
                    scan: bits(v[0]),
                    begin: bits(v[1]),
                    end: bits(v[2]),
                    height: bits(v[3]),
                    begin_height: bits(v[4]),
                    end_height: bits(v[5]),
                    fwhm_scan: bits(v[6]),
                    offscaled: v[7] == "1",
                });
            }
            i += n;
            for off in [0i64, 7] {
                for ivs in [vec![], vec![(1000.0, 1010.0), (2000.5, 2100.0)]] {
                    match wrap_standard_peaks(&peaks, signal_len, off, &ivs) {
                        Err(e) => println!("WRAP ERR {:?}", e),
                        Ok((res, d)) => {
                            let items: Vec<String> = res
                                .iter()
                                .map(|p| {
                                    format!(
                                        "{}:{}:{}:{}:{}:{}:{}",
                                        hb(p.scan),
                                        hb(p.begin),
                                        hb(p.end),
                                        p.offscaled as u8,
                                        hb(p.source_local.0),
                                        hb(p.source_local.1),
                                        hb(p.source_local.2)
                                    )
                                })
                                .collect();
                            println!("WRAP {}", items.join(" "));
                            println!(
                                "DIAG {} {} {} {} {} {} {}",
                                d.input_count,
                                d.output_count,
                                d.fractional_coordinates_truncated,
                                d.new_offscaled_peaks,
                                d.duplicate_projected_scans,
                                d.offscale_interval_count,
                                off
                            );
                        }
                    }
                }
            }
        }
        i += 1;
    }
}

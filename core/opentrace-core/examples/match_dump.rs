//! Filter + ratio match over the CASE/P harness input (see scratchpad/match_check.py); prints the same report.
use opentrace_core::prefilters::{filter_standard_peaks, PeakIn};
use opentrace_core::ratio::{match_ratio_standard, RatioCandidate};
use std::io::{BufRead, BufReader};

fn bits(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}
fn hb(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

fn main() {
    let path = std::env::args().nth(1).expect("input path");
    let lines: Vec<String> = BufReader::new(std::fs::File::open(path).unwrap()).lines().map(|l| l.unwrap()).collect();
    let mut signal: Vec<i64> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        if let Some(rest) = line.strip_prefix("SIG ") {
            signal = rest.split(' ').map(|v| v.parse().unwrap()).collect();
        } else if line.starts_with("CASE ") {
            let f: Vec<&str> = line.split(' ').collect();
            let exp: usize = f[1].parse().unwrap();
            let gap = bits(f[2]);
            let n: usize = f[3].parse().unwrap();
            let mut peaks = Vec::new();
            for k in 0..n {
                let v: Vec<&str> = lines[i + 1 + k][2..].split(' ').collect();
                peaks.push(PeakIn {
                    scan: bits(v[0]), begin: bits(v[1]), end: bits(v[2]), height: bits(v[3]),
                    begin_height: bits(v[4]), end_height: bits(v[5]), fwhm_scan: bits(v[6]), offscaled: v[7] == "1",
                });
            }
            i += n;
            let filt = filter_standard_peaks(&peaks, exp, &signal, gap);
            if !filt.passed {
                println!("FILTER rejected");
                i += 1;
                continue;
            }
            let cands: Vec<RatioCandidate> = filt.kept.iter().map(|&k| RatioCandidate { scan: peaks[k].scan, height: peaks[k].height }).collect();
            let sub_mode = std::env::args().nth(2).map(|a| a == "sub").unwrap_or(false);
            let bp: Vec<f64> = if sub_mode {
                let step = 1 + (i % 3);
                let sub: Vec<&RatioCandidate> = cands.iter().step_by(step).take(exp).collect();
                if sub.len() < 3 {
                    i += 1;
                    continue;
                }
                sub.iter().enumerate().map(|(k, c)| 0.2 * c.scan + 60.0 + 0.37 * k as f64).collect()
            } else {
                (0..exp).map(|k| 70.0 + 25.0 * k as f64).collect()
            };
            for allow in [true, false] {
                for maxb in [Some(0.25f64), None] {
                    let r = match_ratio_standard(&cands, &bp, 25, 0.8, 0.75, 0.01, allow, maxb);
                    println!("M {} {}", if r.accepted { "accepted" } else { "rejected" }, r.reasons.join(";"));
                    if let Some(sel) = &r.selection {
                        println!("SEL {} {} {} {} {} [{}]", sel.input_count, sel.selected_count, sel.expected_count, sel.extra_peaks, hb(sel.height_clip),
                            sel.candidate_indices.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","));
                    }
                    println!("ATT {}", r.attempts.iter().map(|(l, b)| format!("{}:{}", l, hb(*b))).collect::<Vec<_>>().join(" "));
                    println!("MATCH {}", r.matched.iter().map(|m| format!("{}:{}:{}:{}", m.candidate_index, hb(m.scan), hb(m.height), hb(m.expected_bp))).collect::<Vec<_>>().join(" "));
                    let d = &r.diagnostics;
                    let o = |v: Option<f64>| v.map(hb).unwrap_or("-".into());
                    println!("DIAG {} {} {} {} {} {}", d.transitions_checked, o(d.best_score), o(d.second_best_score), o(d.ambiguity_gap), d.complete as u8, o(d.global_bp_per_scan));
                    match &r.calibration {
                        None => println!("CAL -"),
                        Some(c) => println!("CAL {}", c.anchors.iter().map(|(x, y)| format!("{}:{}", hb(*x), hb(*y))).collect::<Vec<_>>().join(" ")),
                    }
                }
            }
        }
        i += 1;
    }
}

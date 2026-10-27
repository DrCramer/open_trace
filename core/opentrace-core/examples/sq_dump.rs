//! Filter -> ratio match -> sizing quality over the CASE/P harness input (see scratchpad/sq_check.py).
use opentrace_core::prefilters::{filter_standard_peaks, PeakIn};
use opentrace_core::ratio::{match_ratio_standard, RatioCandidate};
use opentrace_core::sizing_quality::{evaluate_sizing_quality, SqPeak};
use std::io::{BufRead, BufReader};

fn bits(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let lines: Vec<String> = BufReader::new(std::fs::File::open(&args[1]).unwrap()).lines().map(|l| l.unwrap()).collect();
    let sub_mode = args.get(2).map(|a| a == "sub").unwrap_or(false);
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
                i += 1;
                continue;
            }
            let kept: Vec<PeakIn> = filt.kept.iter().map(|&k| peaks[k].clone()).collect();
            let cands: Vec<RatioCandidate> = kept.iter().map(|p| RatioCandidate { scan: p.scan, height: p.height }).collect();
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
                    if r.matched.is_empty() {
                        continue;
                    }
                    let sqin: Vec<SqPeak> = kept
                        .iter()
                        .enumerate()
                        .map(|(j, p)| SqPeak {
                            scan: p.scan,
                            height: p.height,
                            begin_height: p.begin_height,
                            end_height: p.end_height,
                            matched_bp: r.matched.iter().find(|m| m.candidate_index == j).map(|m| m.expected_bp),
                        })
                        .collect();
                    match evaluate_sizing_quality(&sqin, &bp) {
                        Err(e) => println!("SQ ERR {}", e),
                        Ok(q) => {
                            let ev: Vec<String> = q.events.iter().map(|e| format!("{}:{}:{:?}", e.kind, e.peak_index, e.factor)).collect();
                            let o = |v: Option<f64>| v.map(|x| format!("{:?}", x)).unwrap_or("None".into());
                            println!(
                                "SQ {} {} {} {} {} {} {} {} {} {}",
                                if q.evaluated { "evaluated" } else { "rejected" },
                                o(q.score),
                                o(q.raw_score),
                                q.reasons.join(";"),
                                ev.join(";"),
                                o(q.chi_square_statistic),
                                q.degrees_of_freedom.map(|d| d.to_string()).unwrap_or("None".into()),
                                o(q.chi_square_survival),
                                format!("{:?}", q.penalty_factor),
                                o(q.global_bp_per_scan)
                            );
                        }
                    }
                }
            }
        }
        i += 1;
    }
}

//! Size-standard pipeline over the SIG traces of scratchpad/prefilter_py.txt (see scratchpad/pipeline_check.py).
use opentrace_core::sizing_pipeline::{analyze_size_standard, CoordinateMode, HeightMode, SizeParams};
use std::io::{BufRead, BufReader};

fn hb(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

fn bits(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let lines: Vec<String> = BufReader::new(std::fs::File::open(path).unwrap()).lines().map(|l| l.unwrap()).collect();
    // Each SIG trace with the scans of its first CASE block (Python: pipeline_check2.py).
    let mut signals: Vec<(Vec<i64>, Option<Vec<f64>>)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let ln = &lines[i];
        if let Some(rest) = ln.strip_prefix("SIG ") {
            let sig: Vec<i64> = rest.split(' ').map(|v| v.parse::<i64>().unwrap()).collect();
            signals.push((sig, None));
        } else if ln.starts_with("CASE ") {
            if let Some(entry) = signals.last_mut().filter(|e| e.1.is_none()) {
                let n: usize = ln.split(' ').nth(3).unwrap().parse().unwrap();
                let scans: Vec<f64> = (0..n)
                    .map(|k| bits(lines[i + 1 + k][2..].split(' ').next().unwrap()))
                    .collect();
                entry.1 = Some(scans);
            }
        }
        i += 1;
    }
    for (si, (sig, scans)) in signals.iter().enumerate() {
        for n in [6usize, 8, 10] {
            for bpkind in ["lin", "alt", "sub"] {
                let bp: Vec<f64> = if bpkind == "lin" {
                    (0..n).map(|k| 70.0 + 25.0 * k as f64).collect()
                } else if bpkind == "alt" {
                    (0..n).map(|k| 50.0 + 22.0 * k as f64 + 0.5 * (k % 3) as f64).collect()
                } else {
                    let Some(scans) = scans else { continue };
                    let sc: Vec<f64> = scans.iter().step_by(2).cloned().collect();
                    if sc.len() < n {
                        continue;
                    }
                    (0..n).map(|k| 0.2 * sc[k] + 60.0 + 0.37 * k as f64).collect()
                };
                for hm in ["explicit", "basic_standard_auto"] {
                    for cm in ["ap_fractional", "standard_wrapper"] {
                        let mut p = SizeParams::new(25, 0.75);
                        p.height_mode = if hm == "explicit" { HeightMode::Explicit } else { HeightMode::BasicStandardAuto };
                        p.coordinate_mode = if cm == "ap_fractional" { CoordinateMode::ApFractional } else { CoordinateMode::StandardWrapper };
                        let r = analyze_size_standard(sig, &bp, &p);
                        let anchors: Vec<String> = r.anchors.iter().map(|(s, b)| format!("{},{}", hb(*s), hb(*b))).collect();
                        let anchors = if anchors.is_empty() { "-".to_string() } else { anchors.join(";") };
                        let score = r.base_score.map(hb).unwrap_or("None".into());
                        println!(
                            "PIPE {} {} {} {} {} {} {} {} {} {} {} {}",
                            si,
                            n,
                            bpkind,
                            hm,
                            cm,
                            if r.accepted { "accepted" } else { "rejected" },
                            r.stage,
                            r.reasons.join("|"),
                            anchors,
                            score,
                            r.selected_count,
                            r.matched_count,
                        );
                    }
                }
            }
        }
    }
}

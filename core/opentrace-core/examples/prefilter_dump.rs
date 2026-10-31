//! Read the prefilter harness input (see scratchpad/prefilter_dump.py) and print the same report.
use opentrace_core::prefilters::{filter_standard_peaks, PeakIn};
use std::io::{BufRead, BufReader};

fn bits(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}
fn hb(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}
fn lst(v: &[usize]) -> String {
    format!("[{}]", v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","))
}

fn main() {
    let path = std::env::args().nth(1).expect("input path");
    let reader = BufReader::new(std::fs::File::open(path).unwrap());
    let mut signal: Vec<i64> = Vec::new();
    let mut lines = reader.lines().map(|l| l.unwrap());
    let mut pending: Option<String> = None;
    loop {
        let line = match pending.take().or_else(|| lines.next()) {
            Some(l) => l,
            None => break,
        };
        if let Some(rest) = line.strip_prefix("SIG ") {
            signal = rest.split(' ').map(|v| v.parse::<i64>().unwrap()).collect();
        } else if let Some(rest) = line.strip_prefix("CASE ") {
            let f: Vec<&str> = rest.split(' ').collect();
            let exp: usize = f[0].parse().unwrap();
            let gap = bits(f[1]);
            let n: usize = f[2].parse().unwrap();
            let mut peaks = Vec::with_capacity(n);
            for _ in 0..n {
                let p = lines.next().unwrap();
                let v: Vec<&str> = p.strip_prefix("P ").unwrap().split(' ').collect();
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
            let r = filter_standard_peaks(&peaks, exp, &signal, gap);
            let status = if r.passed { "passed" } else { "rejected" };
            println!("RES {} {}", status, r.reasons.join(";"));
            let kept: Vec<String> = r.kept.iter().map(|&i| hb(peaks[i].scan)).collect();
            println!("KEPT {}", kept.join(" "));
            let d = &r.diagnostics;
            let ev: Vec<String> = d.group_events.iter().map(|e| format!("{}:{}:{}", e.dominant_index, e.removed_index, e.side)).collect();
            let cutoff = d.early_cutoff_scan.map(hb).unwrap_or("-".into());
            let lastlarge = d.last_large_early_scan.map(hb).unwrap_or("-".into());
            let refm = match d.reference_means {
                None => "-".to_string(),
                Some((a, b, c)) => format!("{},{},{}", hb(a), hb(b), hb(c)),
            };
            let conn = match d.connectivity_error {
                None => "-".to_string(),
                Some((s, c, l, f)) => format!("{},{},{},{}", s, c, l, f),
            };
            println!(
                "DIAG applied={} shape={} high={} prefix={} tail={} group={} events=[{}] cutoff={} late={} ref={} lastlarge={} after={} out={} below={} examined={} conn={}",
                d.early_stage_applied as u8,
                lst(&d.early_shape_removed_indices),
                lst(&d.early_high_removed_indices),
                lst(&d.early_prefix_removed_indices),
                lst(&d.connected_tail_removed_indices),
                lst(&d.group_removed_indices),
                ev.join(";"),
                cutoff,
                d.late_reference_count,
                refm,
                lastlarge,
                d.after_early_count,
                d.output_count,
                d.below_expected_after_filtering as u8,
                d.group_examined_count,
                conn
            );
        }
    }
}

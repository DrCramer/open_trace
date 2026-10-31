//! Port of `genemapper_open/export.py`: plain CSV exports (csv.writer excel dialect, `\r\n` terminators).
use crate::profiles::py_str;
use serde_json::Value;

fn cell(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        other => py_str(other),
    }
}

fn row(cells: &[String], out: &mut String) {
    if cells.len() == 1 && cells[0].is_empty() {
        out.push_str("\"\"");
    } else {
        for (i, c) in cells.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            if c.contains([',', '"', '\r', '\n']) {
                out.push('"');
                out.push_str(&c.replace('"', "\"\""));
                out.push('"');
            } else {
                out.push_str(c);
            }
        }
    }
    out.push_str("\r\n");
}

fn tag(ch: &Value) -> Result<String, String> {
    ch.get("tag").map(cell).ok_or_else(|| "'tag'".to_string())
}

pub fn traces_csv(channels: &[Value]) -> Result<String, String> {
    let mut out = String::new();
    let mut head = vec!["scan".to_string()];
    for ch in channels {
        head.push(tag(ch)?);
    }
    row(&head, &mut out);
    let vals = |ch: &Value| -> Result<Vec<Value>, String> {
        ch.get("values").and_then(Value::as_array).cloned().ok_or_else(|| "'values'".to_string())
    };
    let all: Vec<Vec<Value>> = channels.iter().map(vals).collect::<Result<_, _>>()?;
    let n = all.iter().map(Vec::len).max().unwrap_or(0);
    for scan in 0..n {
        let mut r = vec![scan.to_string()];
        for v in &all {
            r.push(if scan < v.len() { cell(&v[scan]) } else { String::new() });
        }
        row(&r, &mut out);
    }
    Ok(out)
}

pub fn peaks_csv(channels: &[Value]) -> Result<String, String> {
    let mut out = String::new();
    let head: Vec<String> = ["channel", "scan", "height", "raw_height", "area", "width_scans", "size_bp"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    row(&head, &mut out);
    for ch in channels {
        let peaks = match ch.get("peaks") {
            None => continue,
            Some(Value::Array(a)) => a.clone(),
            Some(_) => return Err("bad peaks".into()),
        };
        for p in &peaks {
            let mut r = vec![tag(ch)?];
            for k in ["scan", "height", "raw_height", "area", "width_scans", "size_bp"] {
                r.push(p.get(k).map(cell).unwrap_or_default());
            }
            row(&r, &mut out);
        }
    }
    Ok(out)
}

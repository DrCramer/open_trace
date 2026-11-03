//! Port of `genemapper_open/matrix.py`: dye matrices (MatrixMaker, GeneScan baseliner, MNF/.mtx ABIF files).
//!
//! Known gaps: JSON integers beyond i64 are not modelled; Python exception classes collapse into one error
//! string (messages are kept). Everything else is bit-identical, including Python 3.12+ compensated `sum()`.
use crate::abif::{AbifError, AbifFile, Num, Value as AV};
use crate::numeric::py_sum;
use crate::profiles::py_str;
use serde_json::{json, Map, Value};

pub type Res<T> = Result<T, String>;

const MIN_POINTS: i64 = 200;
const MAX_PAIRS: usize = 1000;
const MIN_PAIRS: usize = 10;
const PEAK_RFU: i64 = 100;
const DYE_LETTERS: [&str; 5] = ["B", "G", "Y", "R", "O"];

/// `MatrixError(code, message, filename)` or any other Python exception (message only).
#[derive(Debug, Clone)]
pub enum MxErr {
    Matrix { code: &'static str, msg: String, filename: Option<String> },
    Other(String),
}
type MR<T> = Result<T, MxErr>;

fn merr<T>(code: &'static str, msg: impl Into<String>) -> MR<T> {
    Err(MxErr::Matrix { code, msg: msg.into(), filename: None })
}
fn merr_f<T>(code: &'static str, msg: impl Into<String>, filename: &str) -> MR<T> {
    Err(MxErr::Matrix { code, msg: msg.into(), filename: Some(filename.to_string()) })
}
fn other<T>(msg: impl Into<String>) -> MR<T> {
    Err(MxErr::Other(msg.into()))
}
impl From<AbifError> for MxErr {
    fn from(e: AbifError) -> Self {
        MxErr::Other(e.0)
    }
}
impl MxErr {
    pub fn message(&self) -> String {
        match self {
            MxErr::Matrix { msg, .. } => msg.clone(),
            MxErr::Other(m) => m.clone(),
        }
    }
}

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

/// Java `(int)` cast of a double.
pub fn d2i(x: f64) -> i64 {
    if x.is_nan() {
        return 0;
    }
    let t = x.trunc();
    t.max(-2147483648.0).min(2147483647.0) as i64
}

/// ai.a(int[],int,int): (-2,3,6,7,6,3,-2)/21 on indices 3..n-4; edges untouched.
pub fn smooth7(values: &[i64]) -> Vec<i64> {
    let n = values.len();
    let mut out = values.to_vec();
    if n > 6 {
        for i in 3..n - 3 {
            let s = -2 * (values[i + 3] + values[i - 3]) + 3 * (values[i + 2] + values[i - 2]) + 6 * (values[i + 1] + values[i - 1]) + 7 * values[i];
            out[i] = s / 21;
        }
    }
    out
}

fn running(arr: &mut Vec<i64>, w: usize, clip: bool) -> MR<()> {
    let n = arr.len();
    if n < w {
        return Ok(());
    }
    if w == 0 {
        return other("integer division or modulo by zero");
    }
    let mut tmp = vec![0i64; n];
    let mut total = 0i64;
    for i in 0..w {
        tmp[i] = arr[i];
        total += arr[i];
    }
    for i in n - w..n {
        tmp[i] = arr[i];
    }
    let h = w / 2;
    tmp[h] = total / w as i64;
    if n > h {
        for i in h + 1..n - h {
            total += arr[i + h] - arr[i - h - 1];
            tmp[i] = total / w as i64;
        }
    }
    if clip {
        for i in 0..n {
            if arr[i] < tmp[i] {
                tmp[i] = arr[i];
            }
        }
    }
    for i in 0..n {
        arr[i] = if tmp[i] >= 0 { tmp[i] } else { 0 };
    }
    Ok(())
}

const SG21: [i64; 11] = [329, 324, 309, 284, 249, 204, 149, 84, 9, -76, -171];

fn smooth21(arr: &mut Vec<i64>) {
    let n = arr.len();
    if n < 21 {
        return;
    }
    let mut out = vec![0i64; n];
    for i in 10..n - 10 {
        let mut s = SG21[0] * arr[i];
        for k in 1..11 {
            s += SG21[k] * (arr[i + k] + arr[i - k]);
        }
        out[i] = s / 3059;
    }
    for i in 10..n - 10 {
        arr[i] = out[i];
    }
}

/// Python list indexing with negative wrap-around.
fn pyidx(n: usize, i: i64) -> MR<usize> {
    let j = if i < 0 { i + n as i64 } else { i };
    if j < 0 || j >= n as i64 {
        return other("list index out of range");
    }
    Ok(j as usize)
}

fn track(arr: &[i64], direction: i64) -> MR<Vec<i64>> {
    let n = arr.len();
    let mut out = vec![0i64; n];
    let (start, step): (i64, i64) = if direction == -1 { (n as i64 - 1, -1) } else { (0, 1) };
    let mut avg = 0.0f64;
    let mut i = start;
    for _ in 0..10 {
        avg += arr[pyidx(n, i)?] as f64;
        i += step;
    }
    avg /= 10.0;
    i = start;
    for _ in 0..n {
        let ix = pyidx(n, i)?;
        let a = arr[ix];
        if a <= d2i(avg) {
            avg = a as f64;
            out[ix] = a;
        } else {
            let d = a as f64 - avg;
            let mut w = d;
            if d > 20.0 {
                w = d * d * d;
            } else if d > 10.0 {
                w = d * d;
            }
            let k = if w >= 1.0 { 1.0 / w } else { 0.0 };
            avg += k * d;
            out[ix] = d2i(avg + 0.5);
        }
        i += step;
    }
    Ok(out)
}

/// GeneScanBaseliner.a(int[]): baseline-subtracted signal (negative values -> 0).
pub fn genescan_baseline(signal: &[i64]) -> MR<Vec<i64>> {
    let mut b = signal.to_vec();
    running(&mut b, 11, false)?;
    let q = b.len() / 4;
    running(&mut b, q, true)?;
    for w in [21usize, 51, 101, 201] {
        running(&mut b, w, true)?;
    }
    smooth21(&mut b);
    running(&mut b, 21, true)?;
    let rev = track(&b, -1)?;
    let fwd = track(&b, 1)?;
    for i in 0..b.len() {
        b[i] = if rev[i] > fwd[i] { rev[i] } else { fwd[i] };
    }
    Ok((0..b.len()).map(|i| (signal[i]).saturating_sub(b[i]).max(0)).collect())
}

fn rofunc(x: &[f64], y: &[f64], b: f64) -> [f64; 3] {
    let n = x.len() - 1;
    let mut arr: Vec<f64> = (0..n).map(|j| y[j] - b * x[j]).collect();
    arr.sort_by(|a, c| a.partial_cmp(c).unwrap_or(std::cmp::Ordering::Equal));
    let lo = (n - 1) / 2;
    let hi = n - 1 - lo;
    let aa = 0.5 * (arr[lo] + arr[hi]);
    let mut total = 0.0;
    let mut abdev = 0.0;
    for j in 0..n {
        let d = y[j] - (b * x[j] + aa);
        abdev += d.abs();
        total += if d > 0.0 { x[j] } else { -x[j] };
    }
    [total, abdev, aa]
}

/// ai.a(double[],double[]): NR medfit -> [a, b, abdev/n].
pub fn medfit(x: &[f64], y: &[f64]) -> MR<[f64; 3]> {
    if x.is_empty() {
        return other("list index out of range");
    }
    let n = x.len() - 1;
    let (mut sx, mut sy, mut sxy, mut sxx) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for j in 0..n {
        sx += x[j];
        sy += y[j];
        sxy += x[j] * y[j];
        sxx += x[j] * x[j];
    }
    let nf = n as f64;
    let dl = nf * sxx - sx * sx;
    if dl == 0.0 {
        return other("float division by zero");
    }
    let mut bb = (nf * sxy - sx * sy) / dl;
    let aa = (sy - bb * sx) / nf;
    let mut chisq = 0.0;
    for j in 0..n {
        let t = y[j] - (aa + bb * x[j]);
        chisq += t * t;
    }
    let q = chisq / dl;
    if q < 0.0 {
        return other("math domain error");
    }
    let mut sigb = q.sqrt();
    if sigb < 1e-5 {
        sigb = 1e-5;
    }
    let mut b1 = bb;
    let mut r = rofunc(x, y, b1);
    let mut f1 = r[0];
    let step = (sigb * 3.0).abs();
    let mut b2 = bb + if f1 > 0.0 { step } else { -step };
    r = rofunc(x, y, b2);
    let mut f2 = r[0];
    let mut it = 0;
    while f1 * f2 > 0.0 {
        bb = 2.0 * b2 - b1;
        b1 = b2;
        f1 = f2;
        b2 = bb;
        r = rofunc(x, y, b2);
        f2 = r[0];
        it += 1;
        if it > 1000 {
            return Ok([0.0, 0.0, 0.0]);
        }
    }
    sigb *= 0.01;
    while (b2 - b1).abs() > sigb {
        bb = 0.5 * (b1 + b2);
        if bb == b1 || bb == b2 {
            break;
        }
        r = rofunc(x, y, bb);
        let f = r[0];
        if f * f1 >= 0.0 {
            f1 = f;
            b1 = bb;
        } else {
            b2 = bb;
        }
    }
    Ok([r[2], bb, r[1] / nf])
}

/// ToolBox.a(double[],double[],1): least-squares straight line -> [a, b].
pub fn linear_fit(x: &[f64], y: &[f64]) -> [f64; 2] {
    let n = x.len();
    let nf = n as f64;
    let sx = py_sum(x.iter().copied());
    let sy = py_sum(y.iter().copied());
    let sxx = py_sum(x.iter().map(|v| v * v));
    let sxy = py_sum((0..n).map(|j| x[j] * y[j]));
    let dl = nf * sxx - sx * sx;
    if dl == 0.0 {
        return [if n > 0 { sy / nf } else { 0.0 }, 0.0];
    }
    let b = (nf * sxy - sx * sy) / dl;
    [(sy - b * sx) / nf, b]
}

/// ToolBox.a(double,4): Math.round(v*1e4)/1e4 (half up).
pub fn round4(v: f64) -> MR<f64> {
    let t = v * 10000.0 + 0.5;
    if t.is_nan() {
        return other("cannot convert float NaN to integer");
    }
    if t.is_infinite() {
        return other("cannot convert float infinity to integer");
    }
    Ok(t.floor() / 10000.0)
}

pub struct Standard {
    pub traces: Vec<Vec<i64>>,
    pub start: i64,
    pub points: i64,
    pub filename: String,
}

pub fn make_matrix(standards: &[Standard], dyes: usize) -> MR<Vec<Vec<f64>>> {
    if dyes != 4 && dyes != 5 {
        return merr("dyes", "Number of dyes must be 4 or 5");
    }
    if standards.len() != dyes {
        return merr("samples", "Please make sure you have chosen required number of samples");
    }
    let mut windows = Vec::new();
    for s in standards {
        let name = &s.filename;
        if s.traces.len() != dyes {
            return merr_f("dimension", "The number of dyes (filters) does not match the matrix dimension", name);
        }
        let start = s.start;
        let length = s.traces[0].len() as i64;
        let end = (start as i128 + s.points as i128 - 1).min(length as i128 - 1) as i64;
        if start < 0 || (end as i128 - start as i128 + 1) < MIN_POINTS as i128 {
            return merr_f("points", format!("{} does not have enough data points over the selected range", name), name);
        }
        windows.push((start, end));
    }
    let mut m = vec![vec![0.0f64; dyes]; dyes];
    for (i, s) in standards.iter().enumerate() {
        let (start, end) = windows[i];
        let n = (end - start + 1) as usize;
        let start = start as usize;
        let mut data = Vec::new();
        for tr in &s.traces {
            let lo = start.min(tr.len());
            let hi = (start + n).min(tr.len());
            data.push(genescan_baseline(&smooth7(&tr[lo..hi]))?);
        }
        let reference = &data[i];
        for j in 0..dyes {
            if j == i {
                m[j][i] = 1.0;
                continue;
            }
            let (mut xs, mut ys): (Vec<f64>, Vec<f64>) = (Vec::new(), Vec::new());
            let mut t = 0usize;
            while xs.len() < MAX_PAIRS && t < data[j].len() {
                if t >= reference.len() {
                    return other("list index out of range");
                }
                if reference[t] >= PEAK_RFU {
                    xs.push(reference[t] as f64);
                    ys.push(data[j][t] as f64);
                }
                t += 1;
            }
            if xs.len() < MIN_PAIRS {
                return merr_f("peaks", format!("{} does not have enough peaks in the selected range", s.filename), &s.filename);
            }
            let s1 = medfit(&xs, &ys)?[1];
            let mut xs2 = vec![0.0];
            xs2.extend_from_slice(&xs[..xs.len() - 1]);
            let mut ys2 = vec![0.0];
            ys2.extend_from_slice(&ys[..ys.len() - 1]);
            let s2 = linear_fit(&xs2, &ys2)[1];
            let v = if s1 >= s2 { s1 } else { s2 };
            m[j][i] = if v >= 0.0 { v } else { 0.0 };
        }
    }
    let mut out = Vec::new();
    for row in &m {
        let mut r = Vec::new();
        for &v in row {
            r.push(round4(v)?);
        }
        out.push(r);
    }
    Ok(out)
}

pub fn invert(m: &[Vec<f64>]) -> MR<Vec<Vec<f64>>> {
    let n = m.len();
    let mut a: Vec<Vec<f64>> = m
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let mut r = row.clone();
            r.extend((0..n).map(|j| if i == j { 1.0 } else { 0.0 }));
            r
        })
        .collect();
    for c in 0..n {
        let mut p = c;
        for r in c + 1..n {
            if a[r][c].abs() > a[p][c].abs() {
                p = r;
            }
        }
        if a[p][c].abs() < 1e-12 {
            return merr("singular", "The matrix cannot be inverted");
        }
        a.swap(c, p);
        let pv = a[c][c];
        for v in a[c].iter_mut() {
            *v /= pv;
        }
        for r in 0..n {
            if r != c && a[r][c] != 0.0 {
                let f = a[r][c];
                let ac = a[c].clone();
                for k in 0..2 * n {
                    a[r][k] -= f * ac[k];
                }
            }
        }
    }
    Ok(a.into_iter().map(|row| row[n..].to_vec()).collect())
}

pub fn validate_matrix(m: &Value) -> MR<Vec<Vec<f64>>> {
    let rows = match m {
        Value::Array(a) if a.len() == 4 || a.len() == 5 => a,
        _ => return merr("shape", "Matrix must be 4 x 4 or 5 x 5"),
    };
    for r in rows {
        match r {
            Value::Array(x) if x.len() == rows.len() => {}
            _ => return merr("shape", "Matrix must be 4 x 4 or 5 x 5"),
        }
    }
    let mut out = Vec::new();
    for r in rows {
        let mut row = Vec::new();
        for v in r.as_array().unwrap() {
            let x = match v {
                Value::Number(n) => n.as_f64().unwrap_or(f64::INFINITY),
                _ => return merr("values", "Please enter numeric values between 0 and 2"),
            };
            if !x.is_finite() || x < 0.0 || x > 2.0 {
                return merr("values", "Please enter numeric values between 0 and 2");
            }
            row.push(x);
        }
        out.push(row);
    }
    invert(&out)?;
    Ok(out)
}

/// MultiComponenter: dye_c[t] = sum_r filter_r[t] * inverse[c][r], truncated (int16 wrap for short data).
pub fn multicomponent(traces: &[Vec<i64>], inverse: &[Vec<f64>], int16: bool) -> MR<Vec<Vec<i64>>> {
    let n = inverse.len();
    if traces.len() != n {
        return merr("dimension", "Matrix has wrong dimension");
    }
    if n == 0 {
        return other("list index out of range");
    }
    let length = traces[0].len();
    let mut out = Vec::new();
    for c in 0..n {
        let w = &inverse[c];
        let mut row = Vec::with_capacity(length);
        for t in 0..length {
            let mut terms = Vec::with_capacity(n);
            for r in 0..n {
                if t >= traces[r].len() {
                    return other("list index out of range");
                }
                terms.push(traces[r][t] as f64 * w[r]);
            }
            let mut v = d2i(py_sum(terms));
            if int16 {
                v = ((v + 32768) & 0xFFFF) - 32768;
            }
            row.push(v);
        }
        out.push(row);
    }
    Ok(out)
}

// ---------- ABIF .mtx files ----------
struct Entry {
    name: [u8; 4],
    number: i32,
    etype: u16,
    esize: u16,
    count: u32,
    payload: Vec<u8>,
}

fn write_abif(entries: &[Entry]) -> Vec<u8> {
    let header_len = 128usize;
    let mut blobs: Vec<u8> = Vec::new();
    let mut dir: Vec<u8> = Vec::new();
    for e in entries {
        let size = e.payload.len();
        dir.extend_from_slice(&e.name);
        dir.extend_from_slice(&e.number.to_be_bytes());
        dir.extend_from_slice(&e.etype.to_be_bytes());
        dir.extend_from_slice(&e.esize.to_be_bytes());
        dir.extend_from_slice(&e.count.to_be_bytes());
        dir.extend_from_slice(&(size as u32).to_be_bytes());
        if size <= 4 {
            let mut field = e.payload.clone();
            field.resize(4, 0);
            dir.extend_from_slice(&field);
            dir.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            let off = header_len + blobs.len();
            blobs.extend_from_slice(&e.payload);
            dir.extend_from_slice(&(off as u32).to_be_bytes());
            dir.extend_from_slice(&0u32.to_be_bytes());
        }
    }
    let dir_off = header_len + blobs.len();
    let mut head: Vec<u8> = b"ABIF".to_vec();
    head.extend_from_slice(&101u16.to_be_bytes());
    head.extend_from_slice(b"tdir");
    head.extend_from_slice(&1i32.to_be_bytes());
    head.extend_from_slice(&1023u16.to_be_bytes());
    head.extend_from_slice(&28u16.to_be_bytes());
    head.extend_from_slice(&((dir.len() / 28) as u32).to_be_bytes());
    head.extend_from_slice(&(dir.len() as u32).to_be_bytes());
    head.extend_from_slice(&(dir_off as u32).to_be_bytes());
    head.extend_from_slice(&0u32.to_be_bytes());
    head.resize(header_len, 0xff);
    head.extend_from_slice(&blobs);
    head.extend_from_slice(&dir);
    head
}

fn name4(s: &str) -> [u8; 4] {
    let b = s.as_bytes();
    [b[0], b[1], b[2], b[3]]
}

pub fn matrix_to_mtx(m: &[Vec<f64>], comment: &str) -> MR<Vec<u8>> {
    let n = m.len();
    let inv = invert(m)?;
    let mut entries = Vec::new();
    if !comment.is_empty() {
        let mut c: Vec<u8> = comment.chars().map(|ch| if ch.is_ascii() { ch as u8 } else { b'?' }).collect();
        c.truncate(255);
        let mut payload = vec![c.len() as u8];
        payload.extend_from_slice(&c);
        entries.push(Entry { name: name4("CMNT"), number: 1, etype: 18, esize: 1, count: c.len() as u32 + 1, payload });
    }
    let mut raw = Vec::new();
    for row in m {
        for &v in row {
            raw.push(round4(v)?);
        }
    }
    let mut payload = Vec::new();
    for v in &raw {
        payload.extend_from_slice(&(*v as f32).to_be_bytes());
    }
    entries.push(Entry { name: name4("MRAW"), number: 1, etype: 7, esize: 4, count: raw.len() as u32, payload });
    entries.push(Entry { name: name4("MTRD"), number: 1, etype: 4, esize: 2, count: 1, payload: (n as i16).to_be_bytes().to_vec() });
    let mut payload = Vec::new();
    let mut count = 0u32;
    for row in &inv {
        for &v in row {
            let x = ((d2i(v * 1000.0) + 32768) & 0xFFFF) - 32768;
            payload.extend_from_slice(&(x as i16).to_be_bytes());
            count += 1;
        }
    }
    entries.push(Entry { name: name4("MTRX"), number: 1, etype: 4, esize: 2, count, payload });
    Ok(write_abif(&entries))
}

fn list_nums(v: &AV) -> Option<Vec<f64>> {
    match v {
        AV::Ints(a) => Some(a.iter().map(|&x| x as f64).collect()),
        AV::Floats(a) => Some(a.clone()),
        _ => None,
    }
}

pub fn mtx_to_matrix(data: &[u8], filename: &str) -> MR<Value> {
    let a = match AbifFile::parse(data, filename) {
        Ok(a) => a,
        Err(e) => return merr("file", e.0),
    };
    let dim = match a.value("MTRD", 1) {
        Some(AV::Int(d)) if d == 4 || d == 5 => d as usize,
        _ => return merr("dimension", "Error reading the dimension"),
    };
    let comment = a.text("CMNT", 1);
    if let Some(raw_v) = a.value("MRAW", 1) {
        if let Some(raw) = list_nums(&raw_v) {
            if raw.len() != dim * dim {
                return merr("values", "The dimension info and the raw value info in the file is not consistent");
            }
            let mut m = Vec::new();
            for r in 0..dim {
                let mut row = Vec::new();
                for c in 0..dim {
                    row.push(fv(round4(raw[r * dim + c])?));
                }
                m.push(Value::Array(row));
            }
            return Ok(json!({"dyes": dim, "matrix": m, "comment": comment, "source": "MRAW"}));
        }
        if matches!(raw_v, AV::Date(_) | AV::Dates(_)) {
            return other("unsupported MRAW element type");
        }
    }
    let mtrx = a.value("MTRX", 1).and_then(|v| list_nums(&v));
    let mtrx = match mtrx {
        Some(m) if m.len() == dim * dim => m,
        _ => return merr("values", "No matrix values are found in the file"),
    };
    let inv: Vec<Vec<f64>> = (0..dim).map(|r| (0..dim).map(|c| mtrx[r * dim + c] / 1000.0).collect()).collect();
    let mut m = Vec::new();
    for row in invert(&inv)? {
        let mut out = Vec::new();
        for v in row {
            out.push(fv(round4(v)?));
        }
        m.push(Value::Array(out));
    }
    Ok(json!({"dyes": dim, "matrix": m, "comment": comment, "source": "MTRX"}))
}

fn bytes_repr(b: &[u8]) -> String {
    let q = if b.contains(&b'\'') && !b.contains(&b'"') { '"' } else { '\'' };
    let mut s = String::from("b");
    s.push(q);
    for &c in b {
        match c {
            b'\\' => s.push_str("\\\\"),
            b'\t' => s.push_str("\\t"),
            b'\n' => s.push_str("\\n"),
            b'\r' => s.push_str("\\r"),
            c if c as char == q => {
                s.push('\\');
                s.push(q);
            }
            0x20..=0x7e => s.push(c as char),
            c => s.push_str(&format!("\\x{:02x}", c)),
        }
    }
    s.push(q);
    s
}

fn abif_str(v: &AV) -> String {
    let list = |a: Vec<String>| format!("[{}]", a.join(", "));
    match v {
        AV::Str(s) => s.clone(),
        AV::Int(i) => i.to_string(),
        AV::Float(f) => crate::kit_definition::py_float_repr(*f),
        AV::Ints(a) => list(a.iter().map(|x| x.to_string()).collect()),
        AV::Floats(a) => list(a.iter().map(|x| crate::kit_definition::py_float_repr(*x)).collect()),
        AV::Date(a) => list(a.iter().map(|x| x.to_string()).collect()),
        AV::Dates(a) => list(a.iter().map(|d| list(d.iter().map(|x| x.to_string()).collect())).collect()),
        AV::Bytes(b) => bytes_repr(b),
    }
}

fn abif_truthy(v: &AV) -> bool {
    match v {
        AV::Str(s) => !s.is_empty(),
        AV::Int(i) => *i != 0,
        AV::Float(f) => *f != 0.0,
        AV::Ints(a) => !a.is_empty(),
        AV::Floats(a) => !a.is_empty(),
        AV::Date(_) => true,
        AV::Dates(a) => !a.is_empty(),
        AV::Bytes(b) => !b.is_empty(),
    }
}

pub fn is_310(a: &AbifFile) -> bool {
    let parts: Vec<String> = ["MODL", "MCHN", "HCFG"]
        .iter()
        .map(|t| match a.value(t, 1) {
            Some(v) if abif_truthy(&v) => abif_str(&v),
            _ => String::new(),
        })
        .collect();
    let text = parts.join(" ");
    text.contains("310") && !text.contains("3100") && !text.contains("3130")
}

pub fn embedded_matrix(a: &AbifFile) -> Option<Vec<Vec<f64>>> {
    let dim = match a.value("MTRD", 1) {
        Some(AV::Int(d)) if d == 4 || d == 5 => d as usize,
        _ => return None,
    };
    let m = a.value("MTRX", 1).and_then(|v| list_nums(&v))?;
    if m.len() != dim * dim {
        return None;
    }
    Some((0..dim).map(|r| (0..dim).map(|c| m[r * dim + c] / 1000.0).collect()).collect())
}

/// Raw filter traces DATA1..DATAn of a sample file (n = `dyes` or every raw trace).
pub fn raw_traces(fsa: &[u8], filename: &str, dyes: Option<i64>) -> MR<Vec<Vec<i64>>> {
    let a = AbifFile::parse(fsa, filename)?;
    let mut raw: std::collections::BTreeMap<i32, Vec<i64>> = std::collections::BTreeMap::new();
    for ch in a.channels()? {
        if ch.group == "raw" {
            if let Some(d) = ch.dye_index {
                if d != 0 {
                    let mut vals = Vec::with_capacity(ch.values.len());
                    for v in &ch.values {
                        vals.push(match v {
                            Num::Int(i) => *i,
                            Num::Float(f) => {
                                if f.is_nan() {
                                    return other("cannot convert float NaN to integer");
                                }
                                if f.is_infinite() {
                                    return other("cannot convert float infinity to integer");
                                }
                                f.trunc() as i64
                            }
                        });
                    }
                    raw.insert(d, vals);
                }
            }
        }
    }
    Ok(raw.into_iter().filter(|(k, _)| dyes.map(|d| (*k as i64) <= d).unwrap_or(true)).map(|(_, v)| v).collect())
}

// ---------- base64 (Python b64decode(validate=True), 3.13) ----------
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64encode(data: &[u8]) -> String {
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn b64decode_strict(s: &str) -> Result<Vec<u8>, String> {
    if !s.is_ascii() {
        return Err("string argument should contain only ASCII characters".into());
    }
    let d = s.as_bytes();
    if !d.is_empty() && d[0] == b'=' {
        return Err("Leading padding not allowed".into());
    }
    let mut out: Vec<u8> = Vec::new();
    let (mut quad, mut left, mut pads, mut started, mut complete) = (0u8, 0u32, 0usize, false, false);
    for &ch in d.iter() {
        if ch == b'=' {
            if complete || quad == 0 {
                return Err("Excess padding not allowed".into());
            }
            if quad == 1 {
                break;
            }
            started = true;
            pads += 1;
            if quad as usize + pads >= 4 {
                complete = true;
            }
            continue;
        }
        let v = match B64.iter().position(|&c| c == ch) {
            Some(p) => p as u32,
            None => return Err("Only base64 data is allowed".into()),
        };
        if complete {
            return Err("Excess data after padding".into());
        }
        if started {
            return Err("Discontinuous padding not allowed".into());
        }
        pads = 0;
        match quad {
            0 => {
                quad = 1;
                left = v;
            }
            1 => {
                quad = 2;
                out.push(((left << 2) | (v >> 4)) as u8);
                left = v & 0xf;
            }
            2 => {
                quad = 3;
                out.push(((left << 4) | (v >> 2)) as u8);
                left = v & 0x3;
            }
            _ => {
                quad = 0;
                out.push(((left << 6) | v) as u8);
                left = 0;
            }
        }
    }
    if complete {
        return Ok(out);
    }
    if quad != 0 {
        if quad == 1 {
            return Err(format!(
                "Invalid base64-encoded string: number of data characters ({}) cannot be 1 more than a multiple of 4",
                (out.len() / 3) * 4 + 1
            ));
        }
        return Err("Incorrect padding".into());
    }
    Ok(out)
}

fn b64(data: &Value, label: &str, limit: usize) -> MR<Vec<u8>> {
    let s = match data {
        Value::String(s) => s,
        _ => return merr("file", format!("{}: file data required", label)),
    };
    let raw = b64decode_strict(s).map_err(MxErr::Other)?;
    if raw.len() > limit {
        return merr("file", format!("{}: file too large", label));
    }
    Ok(raw)
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        Some(x) => crate::pullup_quality::truthy(x),
        None => false,
    }
}

fn plain_int(v: Option<&Value>) -> Option<i64> {
    match v {
        Some(Value::Number(n)) if !n.is_f64() => n.as_i64(),
        _ => None,
    }
}

fn matrix_value(m: &[Vec<f64>]) -> Value {
    Value::Array(m.iter().map(|r| Value::Array(r.iter().map(|&x| fv(x)).collect())).collect())
}

fn matrix_request_inner(request: &Map<String, Value>) -> MR<Option<Value>> {
    let action = request.get("action");
    let is = |s: &str| action == Some(&json!(s));
    if is("make") {
        let dyes_v = request.get("dyes");
        let items = request.get("standards");
        let (dyes, items) = match (plain_int(dyes_v), items) {
            (Some(d), Some(Value::Array(a))) => (d, a),
            _ => return merr("samples", "Please make sure you have chosen required number of samples"),
        };
        let mut standards = Vec::new();
        let mut k: i64 = 0;
        while k < dyes {
            let it = items.get(k as usize);
            let letter = if (k as usize) < DYE_LETTERS.len() { DYE_LETTERS[k as usize].to_string() } else { (k + 1).to_string() };
            let it = match it {
                Some(Value::Object(m)) if truthy(m.get("fsa_b64")) => m,
                _ => return merr("no_file", format!("No File Selected for \"{}\" Data", letter)),
            };
            let name = match it.get("filename") {
                Some(v) if crate::pullup_quality::truthy(v) => py_str(v),
                _ => format!("{}.fsa", letter),
            };
            let raw = b64(&it["fsa_b64"], &name, 64 * 1024 * 1024)?;
            let traces = raw_traces(&raw, &name, Some(dyes))?;
            let start = it.get("start").cloned().unwrap_or(json!(1000));
            let points = it.get("points").cloned().unwrap_or(json!(100000));
            let (s, p) = match (plain_int(Some(&start)), plain_int(Some(&points))) {
                (Some(s), Some(p)) if s >= 0 && p >= 1 => (s, p),
                _ => return merr("points", format!("{}: Start At and Points must be whole numbers", name)),
            };
            standards.push(Standard { traces, start: s, points: p, filename: name });
            k += 1;
        }
        if dyes < 0 || dyes > 5 {
            // make_matrix rejects every count but 4 and 5
            return merr("dyes", "Number of dyes must be 4 or 5");
        }
        let m = make_matrix(&standards, dyes as usize)?;
        let inv = invert(&m)?;
        return Ok(Some(json!({"matrix": matrix_value(&m), "inverse": matrix_value(&inv)})));
    }
    if is("validate") {
        let m = validate_matrix(request.get("matrix").unwrap_or(&Value::Null))?;
        let inv = invert(&m)?;
        return Ok(Some(json!({"matrix": matrix_value(&m), "inverse": matrix_value(&inv)})));
    }
    if is("export") {
        let m = validate_matrix(request.get("matrix").unwrap_or(&Value::Null))?;
        let comment = match request.get("comment") {
            Some(v) if crate::pullup_quality::truthy(v) => py_str(v),
            _ => String::new(),
        };
        let bytes = matrix_to_mtx(&m, &comment)?;
        return Ok(Some(json!({"mtx_b64": b64encode(&bytes)})));
    }
    if is("import") {
        let raw = b64(request.get("mtx_b64").unwrap_or(&Value::Null), "matrix", 1024 * 1024)?;
        let filename = match request.get("filename") {
            Some(v) if crate::pullup_quality::truthy(v) => py_str(v),
            _ => "matrix.mtx".to_string(),
        };
        return Ok(Some(mtx_to_matrix(&raw, &filename)?));
    }
    if is("sniff") {
        let raw = b64(request.get("fsa_b64").unwrap_or(&Value::Null), "sample", 64 * 1024 * 1024)?;
        let filename = match request.get("filename") {
            Some(v) if crate::pullup_quality::truthy(v) => py_str(v),
            _ => "sample.fsa".to_string(),
        };
        let a = AbifFile::parse(&raw, &filename)?;
        return Ok(Some(json!({"is_310": is_310(&a), "embedded": embedded_matrix(&a).is_some(), "model": a.text("MODL", 1)})));
    }
    Ok(None)
}

/// `/api/matrix`: make / validate / export / import / sniff. MatrixError becomes an `{error, code, filename}` object.
pub fn matrix_request(request: &Value) -> Res<Value> {
    let req = match request {
        Value::Object(m) => m,
        _ => return Err("Matrix request must be an object".into()),
    };
    match matrix_request_inner(req) {
        Ok(Some(v)) => Ok(v),
        Ok(None) => Err("action must be make, validate, export, import or sniff".into()),
        Err(MxErr::Matrix { code, msg, filename }) => {
            Ok(json!({"error": msg, "code": code, "filename": filename}))
        }
        Err(MxErr::Other(m)) => Err(m),
    }
}

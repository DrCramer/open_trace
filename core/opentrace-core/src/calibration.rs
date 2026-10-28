//! Port of `genemapper_open/calibration.py`: scan -> bp models (linear, least-squares polynomials, natural cubic
//! spline, local rational triples, global Southern pole fit). Operation order follows the Python source.
//! `float ** int` and `math.exp`/`frexp`/`ldexp` go through libm (as CPython does); `math.hypot` reproduces
//! CPython 3.13's `vector_norm` (compensated sums and differential correction).
use crate::fsum::fsum;
use serde_json::{Map, Value};

pub type Res<T> = Result<T, String>;

pub const MODEL_IDS: [&str; 6] = ["linear", "lsq2", "lsq3", "natural_cubic", "local_rational", "global_southern"];
const MAX_ANCHORS: usize = 32;
const MAX_SCAN: f64 = 100_000_000.0;
const MAX_BP: f64 = 1_000_000.0;
const MIN_NORMALIZED_GAP: f64 = 1e-12;
const QR_RANK_TOLERANCE: f64 = 1e-12;
const MIN_RATIONAL_Q: f64 = 1e-12;
const MAX_RATIONAL_Q: f64 = 1e12;

extern "C" {
    #[link_name = "pow"]
    fn libm_pow(x: f64, y: f64) -> f64;
    #[link_name = "exp"]
    fn libm_exp(x: f64) -> f64;
    #[link_name = "frexp"]
    fn libm_frexp(x: f64, e: *mut i32) -> f64;
    #[link_name = "ldexp"]
    fn libm_ldexp(x: f64, e: i32) -> f64;
}

/// CPython `float ** int` for the small non-negative integer exponents used here.
pub fn py_pow(x: f64, y: f64) -> f64 {
    if y == 0.0 {
        return 1.0;
    }
    unsafe { libm_pow(x, y) }
}

pub fn py_exp(x: f64) -> f64 {
    unsafe { libm_exp(x) }
}

fn dl_mul(x: f64, y: f64) -> (f64, f64) {
    let z = x * y;
    (z, x.mul_add(y, -z))
}

fn dl_fast_sum(a: f64, b: f64) -> (f64, f64) {
    let x = a + b;
    (x, (a - x) + b)
}

/// CPython 3.13 `vector_norm` (used by `math.hypot`); `v` holds absolute values.
fn vector_norm(v: &mut [f64], max: f64) -> f64 {
    if max.is_infinite() {
        return max;
    }
    if max == 0.0 || v.len() <= 1 {
        return max;
    }
    let mut max_e: i32 = 0;
    unsafe { libm_frexp(max, &mut max_e) };
    if max_e < -1023 {
        for x in v.iter_mut() {
            *x /= f64::MIN_POSITIVE;
        }
        return f64::MIN_POSITIVE * vector_norm(v, max / f64::MIN_POSITIVE);
    }
    let scale = unsafe { libm_ldexp(1.0, -max_e) };
    let (mut csum, mut frac1, mut frac2) = (1.0f64, 0.0f64, 0.0f64);
    for &x in v.iter() {
        let x = x * scale;
        let (p_hi, p_lo) = dl_mul(x, x);
        let (s_hi, s_lo) = dl_fast_sum(csum, p_hi);
        csum = s_hi;
        frac1 += p_lo;
        frac2 += s_lo;
    }
    let mut h = (csum - 1.0 + (frac1 + frac2)).sqrt();
    let (p_hi, p_lo) = dl_mul(-h, h);
    let (s_hi, s_lo) = dl_fast_sum(csum, p_hi);
    csum = s_hi;
    frac1 += p_lo;
    frac2 += s_lo;
    let x = csum - 1.0 + (frac1 + frac2);
    h += x / (2.0 * h);
    h / scale
}

/// CPython `math.hypot(*values)`.
pub fn py_hypot(values: &[f64]) -> f64 {
    let mut found_nan = false;
    let mut max = 0.0f64;
    let mut coords = Vec::with_capacity(values.len());
    for &v in values {
        let x = v.abs();
        if x.is_infinite() {
            return f64::INFINITY;
        }
        if x.is_nan() {
            found_nan = true;
        }
        if x > max {
            max = x;
        }
        coords.push(x);
    }
    if found_nan {
        return f64::NAN;
    }
    vector_norm(&mut coords, max)
}

fn number(v: &Value, label: &str) -> Res<f64> {
    if let Value::Number(n) = v {
        if let Some(x) = n.as_f64() {
            if x.is_finite() {
                return Ok(x);
            }
        }
    }
    Err(format!("{} must be a finite number", label))
}

/// Python `_validate_anchors`: returns the normalized (scan, bp) pairs and the model id.
fn validate_anchors(anchors: &Value, model: &Value) -> Res<(Vec<(f64, f64)>, &'static str)> {
    let unknown = || {
        format!("Unknown calibration model; use {}", MODEL_IDS.join(", "))
    };
    let name: &'static str = match model {
        Value::String(m) => match MODEL_IDS.iter().find(|&&id| id == m.as_str()) {
            Some(id) => id,
            None => return Err(unknown()),
        },
        _ => return Err(unknown()),
    };
    let items = match anchors {
        Value::Array(a) => a,
        _ => return Err("Anchors must be an explicit bounded list or tuple".into()),
    };
    let minimum = match name {
        "linear" | "natural_cubic" => 2,
        "lsq2" | "local_rational" => 3,
        _ => 4,
    };
    if items.len() < minimum || items.len() > MAX_ANCHORS {
        return Err(format!("{} requires {}..{} anchors", name, minimum, MAX_ANCHORS));
    }
    let mut result: Vec<(f64, f64)> = Vec::new();
    for (index, pair) in items.iter().enumerate() {
        let p = match pair {
            Value::Array(p) if p.len() == 2 => p,
            _ => return Err(format!("Anchor {} must contain exactly scan and bp", index)),
        };
        let scan = number(&p[0], &format!("Anchor {} scan", index))?;
        let bp = number(&p[1], &format!("Anchor {} bp", index))?;
        if !(0.0 <= scan && scan <= MAX_SCAN) || !(0.0 < bp && bp <= MAX_BP) {
            return Err("Anchors require scan in [0, 100000000] and bp in (0, 1000000]".into());
        }
        result.push((scan, bp));
    }
    if result.windows(2).any(|w| w[1].0 <= w[0].0 || w[1].1 <= w[0].1) {
        return Err("Anchor scans and bp must increase strictly in supplied order".into());
    }
    let s0 = result[0].0;
    let span = result[result.len() - 1].0 - s0;
    let normalized: Vec<f64> = result.iter().map(|&(x, _)| (x - s0) / span).collect();
    if normalized.windows(2).any(|w| w[1] - w[0] < MIN_NORMALIZED_GAP) {
        return Err("Numerically unresolved scan spacing; anchors are too closely clustered".into());
    }
    Ok((result, name))
}

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

fn sv(x: &str) -> Value {
    Value::String(x.to_string())
}

/// Python `min(seq)`: first minimal element wins.
fn first_min(values: &[f64]) -> f64 {
    let mut m = values[0];
    for &x in &values[1..] {
        if x < m {
            m = x;
        }
    }
    m
}

/// Python `sorted(set(values))`: first occurrence kept, ascending order.
fn sorted_unique(values: Vec<f64>) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    for v in values {
        if !out.iter().any(|&o| o == v) {
            out.push(v);
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap());
    out
}

/// Python `_polynomial`: Horner evaluation, plain multiply then add (no fused operations).
fn polynomial(coefficients: &[f64], value: f64) -> f64 {
    let mut result = 0.0f64;
    for &c in coefficients.iter().rev() {
        result = result * value + c;
    }
    result
}

/// Python `_quadratic_minimum`; `a == None` is the Python integer 0 (degree-2 fit).
fn quadratic_minimum(a: Option<f64>, b: f64, c: f64, left: f64, right: f64) -> f64 {
    let mut points = vec![left, right];
    if let Some(av) = a {
        if av > 0.0 {
            let vertex = -b / (2.0 * av);
            if left < vertex && vertex < right {
                points.push(vertex);
            }
        }
    }
    let value = |x: f64| {
        let ax = match a {
            Some(av) => av * x,
            None => 0.0,
        };
        (ax + b) * x + c
    };
    let mut m = value(points[0]);
    for &x in &points[1..] {
        let v = value(x);
        if v < m {
            m = v;
        }
    }
    m
}

/// Python `_quadratic_roots`: finite derivative roots inside [left, right], sorted and unique.
fn quadratic_roots(a: f64, b: f64, c: f64, left: f64, right: f64) -> Vec<f64> {
    let mut roots = if a == 0.0 {
        if b == 0.0 { vec![] } else { vec![-c / b] }
    } else {
        let discriminant = b * b - 4.0 * a * c;
        if discriminant < 0.0 {
            return vec![];
        }
        let root = discriminant.sqrt();
        let q = -0.5 * (b + root.copysign(b));
        if q == 0.0 { vec![-b / (2.0 * a)] } else { vec![q / a, c / q] }
    };
    roots.retain(|x| x.is_finite() && left <= *x && *x <= right);
    sorted_unique(roots)
}

/// Python `_least_squares_qr`: Householder QR with scaled rank checks.
fn least_squares_qr(matrix: &[Vec<f64>], values: &[f64]) -> Res<Vec<f64>> {
    let rows = matrix.len();
    let columns = matrix[0].len();
    let mut a: Vec<Vec<f64>> = matrix.to_vec();
    let mut b: Vec<f64> = values.to_vec();
    let mut diagonal: Vec<f64> = Vec::new();
    for column in 0..columns {
        let mut vector: Vec<f64> = (column..rows).map(|row| a[row][column]).collect();
        let norm = py_hypot(&vector);
        if !norm.is_finite() || norm <= QR_RANK_TOLERANCE {
            return Err("Singular or numerically rank-deficient polynomial fit".into());
        }
        let alpha = -norm.copysign(vector[0]);
        vector[0] -= alpha;
        let vector_norm = py_hypot(&vector);
        let vector: Vec<f64> = vector.iter().map(|value| value / vector_norm).collect();
        for j in column..columns {
            let projection = 2.0 * fsum((0..vector.len()).map(|k| vector[k] * a[column + k][j]));
            for (k, &value) in vector.iter().enumerate() {
                a[column + k][j] -= projection * value;
            }
        }
        let projection = 2.0 * fsum((0..vector.len()).map(|k| vector[k] * b[column + k]));
        for (k, &value) in vector.iter().enumerate() {
            b[column + k] -= projection * value;
        }
        a[column][column] = alpha;
        for row in column + 1..rows {
            a[row][column] = 0.0;
        }
        diagonal.push(alpha.abs());
    }
    let lo = first_min(&diagonal);
    let hi = diagonal.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if lo <= hi * QR_RANK_TOLERANCE {
        return Err("Singular or ill-conditioned polynomial fit".into());
    }
    let mut solution = vec![0.0f64; columns];
    for row in (0..columns).rev() {
        let s = fsum((row + 1..columns).map(|j| a[row][j] * solution[j]));
        solution[row] = (b[row] - s) / a[row][row];
    }
    if !solution.iter().all(|v| v.is_finite()) {
        return Err("Non-finite polynomial coefficients".into());
    }
    Ok(solution)
}

/// Python `_RationalTriple`: normalized [1/1] rational interpolation of three increasing anchors.
#[derive(Clone)]
struct Triple {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    dx: f64,
    dy: f64,
    q: f64,
    margin: f64,
    min_derivative: f64,
}

impl Triple {
    fn new(p0: (f64, f64), p1: (f64, f64), p2: (f64, f64)) -> Res<Triple> {
        let (x0, y0) = p0;
        let (x1, y1) = p1;
        let (x2, y2) = p2;
        let dx = x2 - x0;
        let dy = y2 - y0;
        let s = (x1 - x0) / dx;
        let v = (y1 - y0) / dy;
        if !(0.0 < s && s < 1.0) || !(0.0 < v && v < 1.0) {
            return Err("Numerically unresolved rational middle anchor".into());
        }
        let q = (v / s) * ((1.0 - s) / (1.0 - v));
        if !q.is_finite() || !(MIN_RATIONAL_Q <= q && q <= MAX_RATIONAL_Q) {
            return Err("Numerically unresolved rational denominator or near-pole fit".into());
        }
        let margin = if q < 1.0 { q } else { 1.0 };
        let inverse = 1.0 / q;
        let m = if inverse < q { inverse } else { q };
        let min_derivative = (dy / dx) * m;
        if !min_derivative.is_finite() || min_derivative <= 0.0 {
            return Err("Numerically unresolved positive rational derivative".into());
        }
        Ok(Triple { x0, y0, x1, y1, x2, y2, dx, dy, q, margin, min_derivative })
    }

    fn value(&self, scan: f64) -> Res<f64> {
        if !(self.x0 <= scan && scan <= self.x2) {
            return Err("Rational evaluation outside supporting triple".into());
        }
        if scan == self.x0 {
            return Ok(self.y0);
        }
        if scan == self.x1 {
            return Ok(self.y1);
        }
        if scan == self.x2 {
            return Ok(self.y2);
        }
        let x = (scan - self.x0) / self.dx;
        let denominator = (1.0 - x) + self.q * x;
        if !denominator.is_finite() || denominator < self.margin * (1.0 - 1e-12) {
            return Err("Unresolved rational denominator during evaluation".into());
        }
        Ok(self.y0 + self.dy * ((x * self.q) / denominator))
    }
}

/// Normalized data shared by all models (Python `_Calibration.__init__` fields).
struct Curve {
    anchors: Vec<(f64, f64)>,
    scans: Vec<f64>,
    bp: Vec<f64>,
    span: f64,
    u: Vec<f64>,
    y_center: f64,
    y_scale: f64,
    z: Vec<f64>,
}

enum Kind {
    Linear,
    Poly(Vec<f64>),
    /// Per-segment cubic coefficients in the local variable.
    Natural(Vec<[f64; 4]>),
    Rational(Vec<Triple>),
    Southern { a: f64, b: f64, p: Option<f64> },
}

struct Fit {
    kind: Kind,
    minimum: f64,
    details: Vec<(&'static str, Value)>,
}

fn fit_linear(c: &Curve) -> Res<Fit> {
    let slopes: Vec<f64> = c.anchors.windows(2).map(|w| (w[1].1 - w[0].1) / (w[1].0 - w[0].0)).collect();
    Ok(Fit { kind: Kind::Linear, minimum: first_min(&slopes), details: vec![] })
}

fn fit_polynomial(c: &Curve, degree: usize) -> Res<Fit> {
    let matrix: Vec<Vec<f64>> = c
        .u
        .iter()
        .map(|&u| {
            let t = 2.0 * u - 1.0;
            (0..=degree).map(|p| py_pow(t, p as f64)).collect()
        })
        .collect();
    let coefficients = least_squares_qr(&matrix, &c.z)?;
    let a = if degree == 3 { Some(3.0 * coefficients[3]) } else { None };
    let b = 2.0 * coefficients[2];
    let constant = coefficients[1];
    let minimum = quadratic_minimum(a, b, constant, -1.0, 1.0);
    if !minimum.is_finite() || minimum < 0.0 {
        return Err("Polynomial calibration is nonmonotonic within its domain".into());
    }
    let roots = quadratic_roots(a.unwrap_or(0.0), b, constant, -1.0, 1.0);
    let zero_scans: Vec<f64> = roots.iter().map(|&t| c.scans[0] + (t + 1.0) * c.span / 2.0).collect();
    let details = vec![
        ("degree", Value::from(degree as u64)),
        ("solver", sv("centered_scaled_householder_qr")),
        ("coefficients_scaled", Value::Array(coefficients.iter().map(|&v| fv(v)).collect())),
        ("derivative_zero_scans", Value::Array(zero_scans.iter().map(|&v| fv(v)).collect())),
        (
            "normalization",
            serde_json::json!({
                "x_left": fv(c.scans[0]),
                "x_span": fv(c.span),
                "x_transform": "t = 2 * (scan - x_left) / x_span - 1",
                "y_center": fv(c.y_center),
                "y_scale": fv(c.y_scale),
            }),
        ),
    ];
    Ok(Fit {
        kind: Kind::Poly(coefficients),
        minimum: minimum * c.y_scale * 2.0 / c.span,
        details,
    })
}

fn fit_natural(c: &Curve) -> Res<Fit> {
    let n = c.anchors.len();
    let u = &c.u;
    let z = &c.z;
    let h: Vec<f64> = (0..n - 1).map(|i| u[i + 1] - u[i]).collect();
    let mut second = vec![0.0f64; n];
    if n > 2 {
        let mut diagonal: Vec<f64> = (1..n - 1).map(|i| 2.0 * (h[i - 1] + h[i])).collect();
        let upper: Vec<f64> = (1..n - 2).map(|i| h[i]).collect();
        let lower: Vec<f64> = (1..n - 2).map(|i| h[i]).collect();
        let mut rhs: Vec<f64> = (1..n - 1)
            .map(|i| 6.0 * ((z[i + 1] - z[i]) / h[i] - (z[i] - z[i - 1]) / h[i - 1]))
            .collect();
        for i in 1..n - 2 {
            let factor = lower[i - 1] / diagonal[i - 1];
            diagonal[i] -= factor * upper[i - 1];
            rhs[i] -= factor * rhs[i - 1];
        }
        let mut interior = vec![0.0f64; n - 2];
        for i in (0..=n - 3).rev() {
            if diagonal[i] <= 0.0 || !diagonal[i].is_finite() {
                return Err("Singular natural cubic spline system".into());
            }
            let term = if i < n - 3 { upper[i] * interior[i + 1] } else { 0.0 };
            interior[i] = (rhs[i] - term) / diagonal[i];
        }
        second[1..n - 1].copy_from_slice(&interior);
    }
    let mut segments: Vec<[f64; 4]> = Vec::new();
    let mut minimums: Vec<f64> = Vec::new();
    let mut roots: Vec<f64> = Vec::new();
    for (i, &width) in h.iter().enumerate() {
        let c0 = z[i];
        let c1 = z[i + 1] - z[i] - width * width * (2.0 * second[i] + second[i + 1]) / 6.0;
        let c2 = width * width * second[i] / 2.0;
        let c3 = width * width * (second[i + 1] - second[i]) / 6.0;
        if ![c0, c1, c2, c3].iter().all(|v| v.is_finite()) {
            return Err("Non-finite natural cubic spline coefficients".into());
        }
        segments.push([c0, c1, c2, c3]);
        let (a, b, cc) = (3.0 * c3, 2.0 * c2, c1);
        let local_minimum = quadratic_minimum(Some(a), b, cc, 0.0, 1.0);
        if !local_minimum.is_finite() || local_minimum < 0.0 {
            return Err(format!("Natural cubic calibration is nonmonotonic on segment {}", i));
        }
        minimums.push(local_minimum * c.y_scale / (width * c.span));
        for root in quadratic_roots(a, b, cc, 0.0, 1.0) {
            roots.push(c.scans[i] + root * (c.scans[i + 1] - c.scans[i]));
        }
    }
    let zero_scans = sorted_unique(roots);
    let details = vec![
        ("boundary", sv("natural")),
        ("endpoint_second_derivatives", serde_json::json!([0.0, 0.0])),
        ("derivative_zero_scans", Value::Array(zero_scans.iter().map(|&v| fv(v)).collect())),
        ("segment_count", Value::from(segments.len() as u64)),
        ("monotonicity_check", sv("analytic derivative minima on every cubic segment")),
    ];
    Ok(Fit { kind: Kind::Natural(segments), minimum: first_min(&minimums), details })
}

fn fit_rational(c: &Curve) -> Res<Fit> {
    let n = c.anchors.len();
    let triples: Vec<Triple> = (0..n - 2)
        .map(|i| Triple::new(c.anchors[i], c.anchors[i + 1], c.anchors[i + 2]))
        .collect::<Res<Vec<Triple>>>()?;
    let minimum = first_min(&triples.iter().map(|t| t.min_derivative).collect::<Vec<f64>>());
    let margin = first_min(&triples.iter().map(|t| t.margin).collect::<Vec<f64>>());
    let ratio = triples
        .iter()
        .map(|t| {
            let inverse = 1.0 / t.q;
            if t.q > inverse { t.q } else { inverse }
        })
        .fold(f64::NEG_INFINITY, f64::max);
    let details = vec![
        ("boundary", sv("first/last supporting triple; interior overlap average")),
        ("continuity", sv("C0; C1 is not imposed or claimed")),
        ("rational_triple_count", Value::from(triples.len() as u64)),
        ("minimum_denominator_margin", fv(margin)),
        ("maximum_denominator_ratio", fv(ratio)),
        ("minimum_derivative_bound_kind", sv("conservative lower bound from full-triple analytic minima")),
        ("monotonicity_check", sv("positive pole-free rational derivatives; averages preserve positivity")),
        ("normalized_rational_formula", sv("g(x)=x*q/((1-x)+q*x), q=v*(1-s)/(s*(1-v))")),
    ];
    Ok(Fit { kind: Kind::Rational(triples), minimum, details })
}

fn pole_at(side: i32, s: f64) -> f64 {
    if side < 0 { -py_exp(s) } else { 1.0 + py_exp(s) }
}

fn fit_southern(c: &Curve) -> Res<Fit> {
    let u = &c.u;
    let z = &c.z;
    let nf = u.len() as f64;
    // Least squares of z = a + b * w with w = 1/(u - p); returns (sse, a, b).
    let solve = |p: f64| -> Option<(f64, f64, f64)> {
        let w: Vec<f64> = u.iter().map(|&x| 1.0 / (x - p)).collect();
        let sw = fsum(w.iter().copied());
        let sz = fsum(z.iter().copied());
        let sww = fsum(w.iter().map(|&v| v * v));
        let swz = fsum(w.iter().zip(z.iter()).map(|(&v, &y)| v * y));
        let det = nf * sww - sw * sw;
        if !det.is_finite() || det.abs() < 1e-300 {
            return None;
        }
        let b = (nf * swz - sw * sz) / det;
        let a = (sz - b * sw) / nf;
        let sse = fsum(w.iter().zip(z.iter()).map(|(&v, &y)| py_pow(a + b * v - y, 2.0)));
        Some((sse, a, b))
    };
    let mut best: Option<((f64, f64, f64), f64)> = None;
    for side in [-1i32, 1] {
        let grid: Vec<f64> = (0..=240).map(|i| -12.0 + 24.0 * (i as f64) / 240.0).collect();
        let values: Vec<((f64, f64, f64), f64)> = grid
            .iter()
            .filter_map(|&s| solve(pole_at(side, s)).map(|r| (r, s)))
            .collect();
        if values.is_empty() {
            continue;
        }
        let mut first = values[0];
        for &item in &values[1..] {
            if item.0 .0 < first.0 .0 {
                first = item;
            }
        }
        let (r0, s0) = first;
        let mut lo = (-12.0f64).max(s0 - 0.1);
        let mut hi = (12.0f64).min(s0 + 0.1);
        let g = (5f64.sqrt() - 1.0) / 2.0;
        let mut cc = hi - g * (hi - lo);
        let mut d = lo + g * (hi - lo);
        for _ in 0..80 {
            let rc = solve(pole_at(side, cc));
            let rd = solve(pole_at(side, d));
            let (Some(rc), Some(rd)) = (rc, rd) else {
                break;
            };
            if rc.0 <= rd.0 {
                hi = d;
                d = cc;
                cc = hi - g * (hi - lo);
            } else {
                lo = cc;
                cc = d;
                d = lo + g * (hi - lo);
            }
        }
        let s = (lo + hi) / 2.0;
        let (r, s_used) = match solve(pole_at(side, s)) {
            Some(r) => (r, s),
            None => (r0, s0),
        };
        let better = match &best {
            None => true,
            Some((br, _)) => r.0 < br.0,
        };
        if better {
            best = Some((r, pole_at(side, s_used)));
        }
    }
    // Pole at infinity: the straight-line limit.
    let su = fsum(u.iter().copied());
    let sz = fsum(z.iter().copied());
    let suu = fsum(u.iter().map(|&x| x * x));
    let suz = fsum(u.iter().zip(z.iter()).map(|(&x, &y)| x * y));
    let slope = (nf * suz - su * sz) / (nf * suu - su * su);
    let icpt = (sz - slope * su) / nf;
    let line_sse = fsum(u.iter().zip(z.iter()).map(|(&x, &y)| py_pow(icpt + slope * x - y, 2.0)));
    let use_line = match &best {
        None => true,
        Some(((sse, _, _), _)) => line_sse <= sse * (1.0 + 1e-9) + 1e-24,
    };
    let ys = c.y_scale;
    let span = c.span;
    let (a, b, p, sse, minimum) = if use_line {
        if !(slope > 0.0) {
            return Err("Global Southern calibration is nonmonotonic within its domain".into());
        }
        (icpt, slope, None, line_sse, slope * ys / span)
    } else {
        let ((sse_b, a_b, b_b), p_b) = best.unwrap();
        if !(a_b.is_finite() && b_b.is_finite()) || b_b >= 0.0 {
            return Err("Global Southern calibration is nonmonotonic within its domain".into());
        }
        let far = (0.0 - p_b).abs().max((1.0 - p_b).abs());
        (a_b, b_b, Some(p_b), sse_b, (-b_b / (far * far)) * ys / span)
    };
    let details = vec![
        ("formula", sv("bp = c + m/(scan - s0) (least squares over all fragments)")),
        ("pole_scan", match p { None => Value::Null, Some(p) => fv(c.scans[0] + p * span) }),
        ("c_bp", fv(c.y_center + ys * a)),
        ("m_bp_scan", match p { None => Value::Null, Some(_) => fv(ys * b * span) }),
        ("limit", match p { None => sv("linear (pole at infinity)"), Some(_) => Value::Null }),
        ("residual_sum_squares_bp2", fv(sse * py_pow(ys, 2.0))),
        ("solver", sv("linear least squares in (c, m) with golden-section search of the pole")),
        ("monotonicity_check", sv("derivative -m/(scan-s0)^2 positive on the whole domain")),
    ];
    Ok(Fit { kind: Kind::Southern { a, b, p }, minimum, details })
}

/// Python `bisect_right(sorted, x) - 1`, clipped to `len - 2` (may be -1 outside the domain, as in Python).
fn segment_index(sorted: &[f64], x: f64, len: usize) -> isize {
    let i = sorted.partition_point(|&v| v <= x) as isize - 1;
    i.min(len as isize - 2)
}

/// Python list indexing with negative indices counted from the end.
fn py_index(i: isize, len: usize) -> usize {
    if i < 0 { (i + len as isize) as usize } else { i as usize }
}

pub struct Calibration {
    curve: Curve,
    model: &'static str,
    kind: Kind,
    minimum_derivative: f64,
    diagnostics: Map<String, Value>,
}

impl Calibration {
    fn evaluate(&self, scan: f64) -> Res<f64> {
        let c = &self.curve;
        let len = c.scans.len();
        match &self.kind {
            Kind::Linear => {
                let i = segment_index(&c.scans, scan, len);
                let (x0, y0) = c.anchors[py_index(i, len)];
                let (x1, y1) = c.anchors[py_index(i + 1, len)];
                Ok(y0 + (scan - x0) / (x1 - x0) * (y1 - y0))
            }
            Kind::Rational(triples) => {
                let i = segment_index(&c.scans, scan, len);
                let n = triples.len();
                let li = (i - 1).max(0) as usize;
                let ri = py_index(i.min(n as isize - 1), n);
                let left_value = triples[li].value(scan)?;
                if li == ri {
                    Ok(left_value)
                } else {
                    Ok(left_value + (triples[ri].value(scan)? - left_value) / 2.0)
                }
            }
            Kind::Southern { a, b, p } => {
                let u = (scan - c.scans[0]) / c.span;
                let inner = match p {
                    None => b * u,
                    Some(p) => b / (u - p),
                };
                Ok(c.y_center + c.y_scale * (a + inner))
            }
            Kind::Poly(coefficients) => {
                let u = (scan - c.scans[0]) / c.span;
                Ok(c.y_center + c.y_scale * polynomial(coefficients, 2.0 * u - 1.0))
            }
            Kind::Natural(segments) => {
                let u = (scan - c.scans[0]) / c.span;
                let i = segment_index(&c.u, u, c.u.len());
                let (pi, pj) = (py_index(i, c.u.len()), py_index(i + 1, c.u.len()));
                let local = (u - c.u[pi]) / (c.u[pj] - c.u[pi]);
                Ok(c.y_center + c.y_scale * polynomial(&segments[pi], local))
            }
        }
    }

    /// Python `Calibration.size`: `None` outside the closed anchor domain.
    pub fn size(&self, scan: &Value) -> Res<Option<f64>> {
        if scan.is_null() {
            return Ok(None);
        }
        let s = number(scan, "Query scan")?;
        if s < self.curve.scans[0] || s > self.curve.scans[self.curve.scans.len() - 1] {
            return Ok(None);
        }
        let value = self.evaluate(s)?;
        if !value.is_finite() {
            return Err("Non-finite calibrated fragment size".into());
        }
        Ok(Some(value))
    }

    /// Python `standard_map_size`: unrounded map value, secant extension outside the anchors.
    pub fn standard_map_size(&self, scan: &Value) -> Res<f64> {
        let s = number(scan, "Standard-map query scan")?;
        if !(0.0 <= s && s <= MAX_SCAN) {
            return Err("Standard-map query scan outside supported bounds".into());
        }
        let is_poly = matches!(self.kind, Kind::Poly(_));
        let c = &self.curve;
        let n = c.anchors.len();
        let value = if is_poly || (c.scans[0] <= s && s <= c.scans[n - 1]) {
            self.evaluate(s)?
        } else {
            let (left, right) = if s < c.scans[0] {
                (c.anchors[0], c.anchors[1])
            } else {
                (c.anchors[n - 2], c.anchors[n - 1])
            };
            let slope = (right.1 - left.1) / (right.0 - left.0);
            left.1 + slope * (s - left.0)
        };
        if !value.is_finite() {
            return Err("Non-finite standard-map size".into());
        }
        if !is_poly && value < 0.0 {
            return Ok(-1.0);
        }
        Ok(value)
    }

    pub fn anchors(&self) -> Value {
        Value::Array(self.curve.anchors.iter().map(|&(s, b)| Value::Array(vec![fv(s), fv(b)])).collect())
    }

    pub fn diagnostics(&self) -> Map<String, Value> {
        self.diagnostics.clone()
    }

    pub fn model(&self) -> &'static str {
        self.model
    }
}

/// Python `build_calibration(anchors, model="linear")`.
pub fn build_calibration(anchors: &Value, model: Option<&Value>) -> Res<Calibration> {
    let default = sv("linear");
    let model = model.unwrap_or(&default);
    let (pairs, name) = validate_anchors(anchors, model)?;
    let scans: Vec<f64> = pairs.iter().map(|p| p.0).collect();
    let bp: Vec<f64> = pairs.iter().map(|p| p.1).collect();
    let span = scans[scans.len() - 1] - scans[0];
    let u: Vec<f64> = scans.iter().map(|&s| (s - scans[0]) / span).collect();
    let y_center = (bp[bp.len() - 1] + bp[0]) / 2.0;
    let y_scale = (bp[bp.len() - 1] - bp[0]) / 2.0;
    if y_scale <= 0.0 {
        return Err("Numerically unresolved bp span".into());
    }
    let z: Vec<f64> = bp.iter().map(|&b| (b - y_center) / y_scale).collect();
    let curve = Curve { anchors: pairs, scans, bp, span, u, y_center, y_scale, z };
    let fit = match name {
        "linear" => fit_linear(&curve)?,
        "lsq2" => fit_polynomial(&curve, 2)?,
        "lsq3" => fit_polynomial(&curve, 3)?,
        "natural_cubic" => fit_natural(&curve)?,
        "local_rational" => fit_rational(&curve)?,
        _ => fit_southern(&curve)?,
    };
    if !fit.minimum.is_finite() || fit.minimum < 0.0 {
        return Err("Calibration is nonmonotonic or numerically unstable within its domain".into());
    }
    let mut cal = Calibration {
        curve,
        model: name,
        kind: fit.kind,
        minimum_derivative: fit.minimum,
        diagnostics: Map::new(),
    };
    let predictions: Vec<f64> = cal.curve.scans.clone().iter().map(|&s| cal.evaluate(s)).collect::<Res<Vec<f64>>>()?;
    if predictions.iter().any(|&p| !p.is_finite() || p <= 0.0) {
        return Err("Calibration produces non-finite or nonpositive fragment sizes".into());
    }
    let residuals: Vec<f64> = predictions.iter().zip(cal.curve.bp.iter()).map(|(&p, &o)| p - o).collect();
    let count = residuals.len();
    let rms = (fsum(residuals.iter().map(|e| e * e)) / count as f64).sqrt();
    let max_abs = residuals.iter().fold(f64::NEG_INFINITY, |m, e| if e.abs() > m { e.abs() } else { m });
    let c = &cal.curve;
    let mut d = Map::new();
    d.insert("model".into(), sv(name));
    d.insert("monotonic".into(), Value::Bool(true));
    d.insert("anchor_count".into(), Value::from(count as u64));
    d.insert("rms_anchor_error_bp".into(), fv(rms));
    d.insert("max_abs_anchor_error_bp".into(), fv(max_abs));
    d.insert("anchor_residuals_bp".into(), Value::Array(residuals.iter().map(|&v| fv(v)).collect()));
    d.insert("anchor_predictions_bp".into(), Value::Array(predictions.iter().map(|&v| fv(v)).collect()));
    d.insert("scan_range".into(), serde_json::json!([fv(c.scans[0]), fv(c.scans[count - 1])]));
    d.insert("anchor_bp_range".into(), serde_json::json!([fv(c.bp[0]), fv(c.bp[count - 1])]));
    d.insert("calibrated_bp_range".into(), serde_json::json!([fv(predictions[0]), fv(predictions[count - 1])]));
    d.insert("minimum_derivative_bp_per_scan".into(), fv(cal.minimum_derivative));
    d.insert("monotonicity_check".into(), sv("analytic derivative minima over entire domain"));
    d.insert("extrapolation".into(), Value::Bool(false));
    d.insert("external_validation".into(), Value::Bool(false));
    for (k, v) in fit.details {
        d.insert(k.to_string(), v);
    }
    cal.diagnostics = d;
    Ok(cal)
}

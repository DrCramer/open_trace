//! Port of `genemapper_open/mixture.py` (Mixture Analysis: segregation, 2-contributor extraction, population
//! databases, statistics, likelihood ratio, merge).
//!
//! Known gaps: malformed-type JSON (wrong container types, non-string alleles, ...) produces an error whose text may
//! differ from the Python TypeError/KeyError/AttributeError; well-typed input is bit-identical. JSON integers beyond
//! i64 are not modelled.
use crate::calibration::py_pow;
use crate::gm_kit_import::type_name;
use crate::numeric::py_sum;
use crate::profiles::{csv_rows, py_float_parse, py_repr, py_str, strip};
use crate::pullup_quality::truthy;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

pub type Res<T> = Result<T, String>;

const RESIDUAL_THRESHOLD: f64 = 0.04;
const F: &str = "F";

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}
fn ofv(x: Option<f64>) -> Value {
    x.map(fv).unwrap_or(Value::Null)
}
fn sv(s: &str) -> Value {
    Value::String(s.to_string())
}

fn is_f(a: &str) -> bool {
    a == "F" || (a.chars().count() == 2 && a.starts_with('F') && matches!(a.chars().nth(1), Some('1' | '2' | '3')))
}
fn fnorm(a: &str) -> String {
    if is_f(a) {
        F.to_string()
    } else {
        a.to_string()
    }
}

fn key_err<T>(k: &str) -> Res<T> {
    Err(format!("'{}'", k))
}
fn getk<'a>(m: &'a Map<String, Value>, k: &str) -> Res<&'a Value> {
    match m.get(k) {
        Some(v) => Ok(v),
        None => key_err(k),
    }
}
fn as_dict<'a>(v: &'a Value, attr: &str) -> Res<&'a Map<String, Value>> {
    match v {
        Value::Object(m) => Ok(m),
        other => Err(format!("'{}' object has no attribute '{}'", type_name(other), attr)),
    }
}
fn get_or<'a>(m: &'a Map<String, Value>, k: &str) -> &'a Value {
    m.get(k).unwrap_or(&Value::Null)
}
/// Python `x or default` on an optional JSON value.
fn or_default(v: Option<&Value>, d: Value) -> Value {
    match v {
        Some(x) if truthy(x) => x.clone(),
        _ => d,
    }
}
fn as_strings(v: &Value) -> Res<Vec<String>> {
    match v {
        Value::Array(a) => a
            .iter()
            .map(|x| match x {
                Value::String(s) => Ok(s.clone()),
                other => Err(format!("unsupported allele type {}", type_name(other))),
            })
            .collect(),
        other => Err(format!("'{}' object is not iterable", type_name(other))),
    }
}
fn strs(v: &[String]) -> Value {
    Value::Array(v.iter().map(|s| sv(s)).collect())
}

// ---------- Python int()/float() ----------
fn py_int_str(s: &str) -> Res<i128> {
    let bad = || format!("invalid literal for int() with base 10: {}", py_repr(&sv(s)));
    let t = strip(s);
    let (neg, body) = match t.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if body.is_empty() || body.starts_with('_') || body.ends_with('_') || body.contains("__") || !body.chars().all(|c| c.is_ascii_digit() || c == '_') {
        return Err(bad());
    }
    let mut v: i128 = 0;
    for c in body.chars().filter(|c| *c != '_') {
        v = v.saturating_mul(10).saturating_add(c.to_digit(10).unwrap() as i128);
    }
    Ok(if neg { -v } else { v })
}

fn py_int(v: &Value) -> Res<i128> {
    match v {
        Value::Bool(b) => Ok(*b as i128),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i as i128)
            } else if let Some(u) = n.as_u64() {
                Ok(u as i128)
            } else {
                let f = n.as_f64().unwrap();
                if f.is_nan() {
                    Err("cannot convert float NaN to integer".into())
                } else if f.is_infinite() {
                    Err("cannot convert float infinity to integer".into())
                } else {
                    Ok(f.trunc() as i128)
                }
            }
        }
        Value::String(s) => py_int_str(s),
        other => Err(format!("int() argument must be a string, a bytes-like object or a real number, not '{}'", type_name(other))),
    }
}

fn py_float(v: &Value) -> Res<f64> {
    match v {
        Value::Bool(b) => Ok(*b as i64 as f64),
        Value::Number(n) => Ok(n.as_f64().unwrap_or(f64::INFINITY)),
        Value::String(s) => py_float_parse(s).ok_or_else(|| format!("could not convert string to float: {}", py_repr(v))),
        other => Err(format!("float() argument must be a string or a real number, not '{}'", type_name(other))),
    }
}

fn float_str(s: &str) -> Res<f64> {
    py_float_parse(s).ok_or_else(|| format!("could not convert string to float: {}", py_repr(&sv(s))))
}

// ---------- method ----------
#[derive(Clone)]
pub struct Method {
    mit: i64,
    phr: Vec<(i64, i64, f64)>,
}

fn factory_method() -> Value {
    json!({"name": "Default", "description": "Factory settings (demonstration values, validate before use)",
           "mit": 50, "phr": [[50, 150, 0.4], [151, 300, 0.54], [301, 1000, 0.63], [1001, 99999, 0.75]]})
}

pub fn validate_method(method: &Value) -> Res<(Method, Value)> {
    let m = match method {
        Value::Object(m) => m,
        _ => return Err("Mixture analysis method must be an object".into()),
    };
    let name_raw = match m.get("name") {
        Some(v) => py_str(v),
        None => String::new(),
    };
    let name = strip(&name_raw).to_string();
    if name.is_empty() || name.chars().count() > 120 || name.chars().any(|c| "\\/:*?\"<>|".contains(c)) {
        return Err("Mixture analysis method name: 1..120 characters without \\ / : * ? \" < > |".into());
    }
    let mit_err = || "The MIT value must be a whole number between 1 and 10,000 (exclusive)".to_string();
    let mit = match m.get("mit") {
        Some(Value::Number(n)) => {
            let f = n.as_f64().unwrap_or(f64::INFINITY);
            if f.trunc() != f || !(1.0 < f && f < 10000.0) {
                return Err(mit_err());
            }
            f as i64
        }
        _ => return Err(mit_err()),
    };
    let rows = match m.get("phr") {
        Some(Value::Array(a)) if !a.is_empty() && a.len() <= 50 => a,
        _ => return Err("At least one heterozygote PHR setting is required".into()),
    };
    let mut prev: Option<i128> = None;
    let mut out = Vec::new();
    for row in rows {
        let r = match row {
            Value::Array(r) if r.len() == 3 => r,
            _ => return Err("PHR setting rows are [min, max, ratio]".into()),
        };
        let lo = py_int(&r[0])?;
        let hi = py_int(&r[1])?;
        let ratio = py_float(&r[2])?;
        if !(0 <= lo && lo <= hi && hi <= 99999) || !(0.0 < ratio && ratio <= 1.0) {
            return Err("PHR setting: 0 <= min <= max <= 99999 and 0 < ratio <= 1".into());
        }
        if let Some(p) = prev {
            if lo != p + 1 {
                return Err("Peak height ranges must occur in sequence".into());
            }
        }
        prev = Some(hi);
        out.push((lo as i64, hi as i64, ratio));
    }
    let desc_raw = match m.get("description") {
        Some(v) => py_str(v),
        None => String::new(),
    };
    let desc: String = desc_raw.chars().take(500).collect();
    let value = json!({
        "name": name, "description": desc, "mit": mit,
        "phr": out.iter().map(|(a, b, c)| json!([a, b, fv(*c)])).collect::<Vec<_>>(),
    });
    Ok((Method { mit, phr: out }, value))
}

fn phr_threshold(phr: &[(i64, i64, f64)], height: f64, mit: i64) -> f64 {
    if phr.is_empty() {
        return 0.0;
    }
    if (mit - 1) as f64 <= height && height <= phr[0].1 as f64 {
        return phr[0].2;
    }
    if phr.len() > 2 {
        for &(lo, hi, ratio) in &phr[1..phr.len() - 1] {
            if lo as f64 <= height && height <= hi as f64 {
                return ratio;
            }
        }
    }
    phr[phr.len() - 1].2
}

// ---------- samples ----------
#[derive(Clone)]
struct Allele {
    label: String,
    height: f64,
}
#[derive(Clone)]
struct MarkerS {
    name: String,
    y: bool,
    alleles: Vec<Allele>,
}
struct Sample {
    id: String,
    markers: Vec<MarkerS>,
}

fn sample_of(raw: &Value) -> Res<Sample> {
    let obj = match raw {
        Value::Object(m) => m,
        _ => return Err("Mixture sample needs id and markers".into()),
    };
    let ms = match obj.get("markers") {
        Some(Value::Array(a)) => a,
        _ => return Err("Mixture sample needs id and markers".into()),
    };
    let mut markers = Vec::new();
    for m in ms {
        let mo = as_dict(m, "get")?;
        let empty = Value::Array(vec![]);
        let al = mo.get("alleles").unwrap_or(&empty);
        let items: Vec<Value> = match al {
            Value::Array(a) => a.clone(),
            Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
            Value::Object(o) => o.keys().map(|k| Value::String(k.clone())).collect(),
            other => return Err(format!("'{}' object is not iterable", type_name(other))),
        };
        let mut alleles = Vec::new();
        for a in &items {
            let ao = as_dict(a, "get")?;
            match ao.get("label") {
                None | Some(Value::Null) => continue,
                Some(Value::String(s)) if s.is_empty() => continue,
                _ => {}
            }
            let label = py_str(getk(ao, "label")?);
            let height = py_float(getk(ao, "height")?)?;
            alleles.push(Allele { label, height });
        }
        let name = py_str(getk(mo, "name")?);
        markers.push(MarkerS { name, y: truthy(get_or(mo, "y")), alleles });
    }
    let id = match obj.get("id") {
        Some(v) => py_str(v),
        None => String::new(),
    };
    Ok(Sample { id, markers })
}

pub fn eligible(raw: &Value) -> Res<(bool, Option<&'static str>)> {
    let s = sample_of(raw)?;
    if s.markers.iter().any(|m| m.alleles.iter().any(|a| a.label.to_uppercase() == "OL")) {
        return Ok((false, Some("off_ladder")));
    }
    if !s.markers.iter().any(|m| !m.alleles.is_empty()) {
        return Ok((false, Some("no_genotypes")));
    }
    if s.markers.iter().filter(|m| !m.alleles.is_empty()).all(|m| m.y) {
        return Ok((false, Some("y_panel")));
    }
    Ok((true, None))
}

fn above_mit(sample: &Sample, mit: i64) -> (Vec<MarkerS>, Vec<String>) {
    let mut adbi = Vec::new();
    let mut out = Vec::new();
    for m in &sample.markers {
        let kept: Vec<Allele> = m.alleles.iter().filter(|a| a.height >= mit as f64).cloned().collect();
        if kept.len() != m.alleles.len() {
            adbi.push(m.name.clone());
        }
        out.push(MarkerS { name: m.name.clone(), y: m.y, alleles: kept });
    }
    (out, adbi)
}

pub fn segregate(raw: &Value, method: &Value) -> Res<Value> {
    let (method, _) = validate_method(method)?;
    let sample = sample_of(raw)?;
    let (markers, adbi) = above_mit(&sample, method.mit);
    let counts: Vec<usize> = markers.iter().map(|m| m.alleles.len()).collect();
    let most = counts.iter().copied().max().unwrap_or(0);
    let three = counts.iter().filter(|&&c| c == 3).count();
    let mut reason: Option<String> = None;
    let category;
    if most > 4 {
        category = 3;
        reason = Some("more_than_four_alleles".into());
    } else if most == 4 || three > 1 {
        category = 2;
        reason = Some(if most == 4 { "four_alleles" } else { "several_three_allele_markers" }.into());
    } else {
        let mut c = 1;
        for m in &markers {
            if m.alleles.len() != 2 {
                continue;
            }
            let (h1, h2) = (m.alleles[0].height, m.alleles[1].height);
            let (lo, hi) = (h1.min(h2), h1.max(h2));
            if hi == 0.0 {
                return Err("float division by zero".into());
            }
            if !(lo / hi > phr_threshold(&method.phr, hi, method.mit)) {
                c = 2;
                reason = Some(format!("phr_fail:{}", m.name));
                break;
            }
        }
        category = c;
    }
    Ok(json!({"id": sample.id, "contributors": category, "reason": reason, "adbi_markers": adbi, "max_alleles": most}))
}

// ---------- 2-contributor extraction ----------
fn expected_vector(pid: &str, m: f64) -> [f64; 4] {
    let (p, q) = (m / 2.0, (1.0 - m) / 2.0);
    match pid {
        "1112" => [p + 0.5, q, 0.0, 0.0],
        "1122" => [m, 1.0 - m, 0.0, 0.0],
        "1123" => [m, q, q, 0.0],
        "1211" => [1.0 - p, p, 0.0, 0.0],
        "1212" => [0.5, 0.5, 0.0, 0.0],
        "1213" => [0.5, p, q, 0.0],
        "1222" => [p, 1.0 - p, 0.0, 0.0],
        "1223" => [p, 0.5, q, 0.0],
        "1233" => [p, p, 1.0 - m, 0.0],
        "1234" => [p, p, q, q],
        "1312" => [0.5, q, p, 0.0],
        "1322" => [p, 1.0 - m, p, 0.0],
        "1323" => [p, q, 0.5, 0.0],
        "1324" => [p, q, p, q],
        "1423" => [p, q, q, p],
        "2211" => [1.0 - m, m, 0.0, 0.0],
        "2212" => [q, p + 0.5, 0.0, 0.0],
        "2213" => [q, m, q, 0.0],
        "2311" => [1.0 - m, p, p, 0.0],
        "2312" => [q, 0.5, p, 0.0],
        "2313" => [q, p, 0.5, 0.0],
        "2314" => [q, p, p, q],
        "2413" => [q, p, q, p],
        "3312" => [q, q, m, 0.0],
        "3412" => [q, q, p, p],
        _ => [0.0; 4],
    }
}

#[derive(Clone)]
struct Pat {
    id: String,
    c2: [String; 2],
    c1: [String; 2],
    x: f64,
    phr: &'static str,
    phr2: Option<f64>,
    phr1: Option<f64>,
    observed: Option<[f64; 4]>,
}

struct Gen<'a> {
    phr: &'a [(i64, i64, f64)],
    t: i64,
    names: Vec<String>,
    h: [f64; 4],
    n: usize,
    positive_only: bool,
    r: usize,
}

impl<'a> Gen<'a> {
    fn new(method: &'a Method, names: &[String], heights: &[f64], positive_only: bool) -> Gen<'a> {
        let mut nm: Vec<String> = names.to_vec();
        while nm.len() < 4 {
            nm.push(String::new());
        }
        let mut h = [0.0f64; 4];
        for (i, x) in heights.iter().take(4).enumerate() {
            h[i] = *x;
        }
        let t = method.mit;
        let r = h.iter().filter(|&&x| x >= t as f64).count();
        Gen { phr: &method.phr, t, names: nm, h, n: names.len(), positive_only, r }
    }
    fn q(&self, h: f64) -> f64 {
        phr_threshold(self.phr, h, self.t)
    }
    fn pair(&self, x: f64, y: f64) -> bool {
        let hi = x.max(y);
        if hi <= 0.0 {
            return false;
        }
        x.min(y) / hi >= self.q(hi)
    }
    fn shared(&self, s: f64, u: f64, v: f64) -> bool {
        let lo = (if u > 0.0 { u * self.q(u) } else { 0.0 }) + (if v > 0.0 { v * self.q(v) } else { 0.0 });
        let hu = if u > 0.0 && self.q(u) != 0.0 { u / self.q(u) } else { 0.0 };
        let hv = if v > 0.0 && self.q(v) != 0.0 { v / self.q(v) } else { 0.0 };
        let hi = hu + hv;
        lo <= s && s <= hi
    }
    fn observed(&self) -> [f64; 4] {
        let total = py_sum(self.h.iter().copied());
        if total > 0.0 {
            [self.h[0] / total, self.h[1] / total, self.h[2] / total, self.h[3] / total]
        } else {
            [0.0; 4]
        }
    }
    fn pattern(&self, pid: &str, x: f64, phr: &'static str, ratios: (Option<f64>, Option<f64>)) -> Pat {
        let d: Vec<usize> = pid.chars().map(|c| c.to_digit(10).unwrap() as usize - 1).collect();
        Pat {
            id: pid.to_string(),
            c2: [self.names[d[0]].clone(), self.names[d[1]].clone()],
            c1: [self.names[d[2]].clone(), self.names[d[3]].clone()],
            x,
            phr,
            phr2: ratios.0,
            phr1: ratios.1,
            observed: None,
        }
    }
    fn ratio(&self, i: usize, j: usize) -> Option<f64> {
        let (a, b) = (self.h[i], self.h[j]);
        if a.max(b) > 0.0 {
            Some(a.min(b) / a.max(b))
        } else {
            None
        }
    }
    fn generate(&mut self, amel: bool) -> Vec<Pat> {
        let h = self.h;
        let mut out = Vec::new();
        let none = (None, None);
        let pf = |ok: bool| if ok { "pass" } else { "fail" };
        if amel {
            if self.n == 2 && h[0] >= self.t as f64 && h[1] >= self.t as f64 {
                out.push(self.pattern("1112", 0.0, "?", none));
                out.push(self.pattern("1212", 0.0, pf(self.pair(h[0], h[1])), (self.ratio(0, 1), self.ratio(0, 1))));
                out.push(self.pattern("1211", 0.0, "?", none));
            } else if self.n == 1 {
                self.names[1] = "Y".into();
                self.h[1] = self.t as f64 - 1.0;
                out.push(self.pattern("1111", 0.0, "?", none));
                out.push(self.pattern("1211", 0.0, "?", none));
                out.push(self.pattern("1112", 0.0, "?", none));
                out.push(self.pattern("1212", 0.0, pf(self.pair(self.h[0], self.h[1])), none));
            }
            return out;
        }
        if self.n == 1 {
            let mut p = self.pattern("1111", 0.5, "?", none);
            p.observed = Some([0.0; 4]);
            return vec![p];
        }
        if self.n == 2 {
            let s = h[0] + h[1];
            let v = self.pair(h[0], h[1]);
            for (pid, x) in [("1122", if s != 0.0 { h[0] / s } else { 0.0 }), ("2211", if s != 0.0 { h[1] / s } else { 0.0 })] {
                if !self.positive_only || x > 0.0 {
                    out.push(self.pattern(pid, x, "?", none));
                }
            }
            out.push(self.pattern("1212", 0.0, pf(v), (self.ratio(0, 1), self.ratio(0, 1))));
            for pid in ["1112", "2212", "1211", "1222"] {
                out.push(self.pattern(pid, 0.0, "?", none));
            }
            return out;
        }
        if self.n == 3 {
            let hh = h[0] + h[1] + h[2];
            // (id, numerator, denominator, kind, args, gate)
            enum T {
                P(usize, usize),
                S(usize, usize, usize),
            }
            let rows: Vec<(&str, f64, f64, T, bool)> = vec![
                ("2311", h[1] + h[2], hh, T::P(1, 2), false),
                ("1123", h[0], hh, T::P(1, 2), false),
                ("1322", h[0] + h[2], hh, T::P(0, 2), false),
                ("2213", h[1], hh, T::P(0, 2), false),
                ("1233", h[0] + h[1], hh, T::P(0, 1), true),
                ("3312", h[2], hh, T::P(0, 1), true),
                ("1223", h[0], h[0] + h[2], T::S(1, 0, 2), false),
                ("2312", h[2], h[0] + h[2], T::S(1, 0, 2), false),
                ("1213", h[1], h[1] + h[2], T::S(0, 1, 2), false),
                ("1312", h[2], h[1] + h[2], T::S(0, 1, 2), true),
                ("1323", h[0], h[0] + h[1], T::S(2, 0, 1), true),
                ("2313", h[1], h[0] + h[1], T::S(2, 0, 1), true),
            ];
            for (pid, num, den, test, gate) in rows {
                if gate && !(self.r > 1) {
                    continue;
                }
                let (valid, ratios);
                match test {
                    T::P(a, b) => {
                        valid = self.pair(h[a], h[b]);
                        let rat = self.ratio(a, b);
                        let pb: Vec<char> = pid.chars().collect();
                        ratios = if pb[0] != pb[1] { (rat, None) } else { (None, rat) };
                    }
                    T::S(si, ui, vi) => {
                        let (s, u, v) = (h[si], h[ui], h[vi]);
                        valid = self.shared(s, u, v);
                        let rat = if s.max(u + v) > 0.0 { Some(s.min(u + v) / s.max(u + v)) } else { None };
                        ratios = (rat, rat);
                    }
                }
                let x = if valid && den > 0.0 { num / den } else { 0.0 };
                if self.positive_only && x <= 0.0 {
                    continue;
                }
                out.push(self.pattern(pid, x, pf(valid), ratios));
            }
            return out;
        }
        if self.n == 4 {
            let hh = py_sum(h.iter().copied());
            let rows = [("1234", 0, 1, 2, 3, 0), ("3412", 2, 3, 0, 1, 0), ("1324", 0, 2, 1, 3, 1), ("2413", 1, 3, 0, 2, 1), ("1423", 0, 3, 1, 2, 2), ("2314", 1, 2, 0, 3, 2)];
            for (pid, a, b, c, d, gate) in rows {
                if !(self.r > gate) {
                    continue;
                }
                let valid = self.pair(h[a], h[b]) && self.pair(h[c], h[d]);
                let x = if hh > 0.0 { (h[a] + h[b]) / hh } else { 0.0 };
                if self.positive_only && x <= 0.0 {
                    continue;
                }
                out.push(self.pattern(pid, x, pf(valid), (self.ratio(a, b), self.ratio(c, d))));
            }
            return out;
        }
        out
    }
}

fn min_x(patterns: &[Pat]) -> f64 {
    let mut best = 0.0;
    for p in patterns {
        if best == 0.0 || p.x < best {
            best = p.x;
        }
    }
    best
}

fn iq(res_pass: bool, phr: &str) -> &'static str {
    if res_pass {
        if phr == "pass" || phr == "?" {
            "pass"
        } else {
            "check"
        }
    } else if phr == "fail" {
        "low"
    } else {
        "check"
    }
}

fn is_amel(name: &str) -> bool {
    name.to_uppercase().contains("AMEL")
}

pub fn known_genotype_match(genotype: &[String], known: &[String]) -> bool {
    let g: Vec<String> = genotype.iter().map(|a| fnorm(a)).collect();
    if g.len() != 2 || known.len() != 2 {
        return false;
    }
    let (mut gs, mut ks) = (g.clone(), known.to_vec());
    gs.sort();
    ks.sort();
    if gs == ks {
        return true;
    }
    let (a, b) = (&g[0], &g[1]);
    if a == "F" && b == "F" {
        return true;
    }
    if a == "F" {
        return known.contains(b);
    }
    if b == "F" {
        return known.contains(a);
    }
    false
}

/// {marker: [alleles]} -> homozygote doubled, empty dropped, tri-allelic rejected.
pub fn normalize_known(known: &Value) -> Res<Vec<(String, Vec<String>)>> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let m = match known {
        Value::Null => return Ok(out),
        Value::Object(m) => m,
        v if !truthy(v) => return Ok(out),
        other => return Err(format!("'{}' object has no attribute 'items'", type_name(other))),
    };
    for (marker, alleles) in m {
        let list: Vec<Value> = match alleles {
            v if !truthy(v) => vec![],
            Value::Array(a) => a.clone(),
            other => return Err(format!("'{}' object is not iterable", type_name(other))),
        };
        let al: Vec<String> = list.iter().map(|a| strip(&py_str(a)).to_string()).filter(|s| !s.is_empty()).collect();
        if al.is_empty() {
            continue;
        }
        if al.len() > 2 {
            return Err(format!("Known genotype contains a tri-allelic marker ({}); remove it before importing", marker));
        }
        let val = if al.len() == 1 { vec![al[0].clone(), al[0].clone()] } else { al };
        if let Some(e) = out.iter_mut().find(|(k, _)| k == marker) {
            e.1 = val;
        } else {
            out.push((marker.clone(), val));
        }
    }
    Ok(out)
}

fn known_to_value(k: &[(String, Vec<String>)]) -> Value {
    let mut m = Map::new();
    for (name, al) in k {
        m.insert(name.clone(), strs(al));
    }
    Value::Object(m)
}

pub fn parse_known_genotypes(text: &Value, filename: &Value) -> Res<Value> {
    let fname = if truthy(filename) { py_str(filename) } else { String::new() };
    let base = fname.rsplit('/').next().unwrap().rsplit('\\').next().unwrap();
    let stem: String = if base.contains('.') { base.rsplitn(2, '.').nth(1).unwrap().to_string() } else { base.to_string() };
    if stem.is_empty() || stem.to_lowercase() == "import" || stem.chars().any(|c| "\\/:*?<>|\"".contains(c)) {
        return Err("Invalid known genotype file name".into());
    }
    let text = match text {
        Value::String(s) => s.clone(),
        v if !truthy(v) => String::new(),
        other => return Err(format!("'{}' object has no attribute 'splitlines'", type_name(other))),
    };
    let first = match splitlines_first(&text) {
        Some(l) => l,
        None => return Err("Empty known genotype file".into()),
    };
    let delim = if first.matches('\t').count() >= first.matches(',').count() { '\t' } else { ',' };
    let rows = csv_rows(&text, delim)?;
    let head: Vec<String> = match rows.first() {
        Some(r) => r.iter().map(|h| strip(h).to_string()).collect(),
        None => return Err("list index out of range".into()),
    };
    let low: Vec<String> = head.iter().map(|h| h.to_lowercase()).collect();
    let mi = match low.iter().position(|h| h == "marker") {
        Some(i) => i,
        None => return Err("Known genotype file needs a Marker column".into()),
    };
    let ai: Vec<usize> = low.iter().enumerate().filter(|(_, h)| h.starts_with("allele")).map(|(i, _)| i).collect();
    if ai.is_empty() {
        return Err("Known genotype file needs Allele columns".into());
    }
    let si = low.iter().position(|h| h == "sample name").or_else(|| low.iter().position(|h| h == "sample file"));
    let mut samples: Vec<String> = Vec::new();
    let mut markers: Vec<(String, Vec<String>)> = Vec::new();
    for r in &rows[1..] {
        if !r.iter().any(|x| !strip(x).is_empty()) || r.len() <= mi {
            continue;
        }
        if let Some(si) = si {
            if si < r.len() && !strip(&r[si]).is_empty() {
                let s = strip(&r[si]).to_string();
                if !samples.contains(&s) {
                    samples.push(s);
                }
            }
        }
        let name = strip(&r[mi]).to_string();
        if name.is_empty() {
            continue;
        }
        let al: Vec<String> = ai.iter().filter(|&&i| i < r.len() && !strip(&r[i]).is_empty()).map(|&i| strip(&r[i]).to_string()).collect();
        if let Some(e) = markers.iter_mut().find(|(k, _)| *k == name) {
            if e.1 != al {
                return Err(format!("Marker {} appears twice with different alleles", name));
            }
            e.1 = al;
        } else {
            markers.push((name, al));
        }
    }
    if samples.len() > 1 {
        return Err("The file contains genotypes of more than one sample; export a single-source sample from the Samples Plot".into());
    }
    let mut mv = Map::new();
    for (k, a) in &markers {
        mv.insert(k.clone(), strs(a));
    }
    let profile = normalize_known(&Value::Object(mv))?;
    if profile.is_empty() {
        return Err("The known genotype file contains no alleles".into());
    }
    Ok(json!({"id": stem, "markers": known_to_value(&profile), "sample": samples.first().cloned().unwrap_or_default()}))
}

/// First element of Python `text.splitlines()`.
fn splitlines_first(text: &str) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let end = text
        .char_indices()
        .find(|(_, c)| matches!(c, '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}'))
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    Some(text[..end].to_string())
}

pub fn merge_known(existing: &Value, new: &Value) -> Res<Value> {
    let ex = as_dict(&or_default(Some(existing), json!({})), "get").map(|m| m.clone())?;
    let nw = as_dict(&or_default(Some(new), json!({})), "get").map(|m| m.clone())?;
    let exm = as_dict(getk(&ex, "markers")?, "items")?;
    let nm = as_dict(getk(&nw, "markers")?, "items")?;
    for (m, al) in nm {
        if let Some(e) = exm.get(m) {
            let (mut a, mut b) = (as_strings(e)?, as_strings(al)?);
            a.sort();
            b.sort();
            if a != b {
                return Err(format!("Known genotype {} conflicts with the existing profile at {}; not imported", py_str(getk(&nw, "id")?), m));
            }
        }
    }
    let mut merged = exm.clone();
    for (m, al) in nm {
        if !merged.contains_key(m) {
            merged.insert(m.clone(), al.clone());
        }
    }
    let mut out = Map::new();
    for (k, v) in &ex {
        out.insert(k.clone(), v.clone());
    }
    out.insert("markers".into(), Value::Object(merged));
    Ok(Value::Object(out))
}

struct RowD {
    pat: Pat,
    marker: String,
    residual: f64,
    residual_pass: bool,
    phr_status: &'static str,
    iq: &'static str,
    adbi: &'static str,
    dropout: bool,
    selected: bool,
    known: Option<Vec<String>>,
    known_match: Option<String>,
    known_lean: Option<&'static str>,
}

fn row_value(r: &RowD, with_known: bool) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), sv(&r.pat.id));
    m.insert("c2".into(), strs(&r.pat.c2));
    m.insert("c1".into(), strs(&r.pat.c1));
    m.insert("phr2".into(), ofv(r.pat.phr2));
    m.insert("phr1".into(), ofv(r.pat.phr1));
    m.insert("marker".into(), sv(&r.marker));
    m.insert("residual".into(), fv(r.residual));
    m.insert("residual_status".into(), sv(if r.residual_pass { "pass" } else { "low" }));
    m.insert("phr_status".into(), sv(r.phr_status));
    m.insert("iq".into(), sv(r.iq));
    m.insert("adbi".into(), sv(r.adbi));
    m.insert("dropout".into(), json!(r.dropout));
    m.insert("mx".into(), fv(r.pat.x));
    m.insert("inconclusive".into(), json!({"c1": false, "c2": false}));
    m.insert("selected".into(), json!(r.selected));
    if with_known {
        m.insert("known".into(), strs(r.known.as_deref().unwrap_or(&[])));
        m.insert("known_match".into(), sv(r.known_match.as_deref().unwrap_or("")));
        if let Some(l) = r.known_lean {
            m.insert("known_lean".into(), sv(l));
        }
    }
    Value::Object(m)
}

pub fn extract(raw: &Value, method: &Value, known: &Value) -> Res<Value> {
    let (method, _) = validate_method(method)?;
    let sample = sample_of(raw)?;
    let (markers, adbi) = above_mit(&sample, method.mit);
    let t = method.mit;
    let work: Vec<&MarkerS> = markers.iter().filter(|m| !m.y && !m.alleles.is_empty()).collect();
    // Phase I
    let mut used: Vec<(String, f64, Value)> = Vec::new();
    for m in &work {
        let n = m.alleles.len();
        if (n == 3 || n == 4) && !is_amel(&m.name) {
            let names: Vec<String> = m.alleles.iter().map(|a| a.label.clone()).collect();
            let hs: Vec<f64> = m.alleles.iter().map(|a| a.height).collect();
            let mut g = Gen::new(&method, &names, &hs, true);
            let x = min_x(&g.generate(false));
            if x > 0.0 {
                used.push((m.name.clone(), x, json!(n)));
            }
        }
    }
    let mut fallback = false;
    if used.is_empty() {
        for m in &work {
            if m.alleles.len() == 3 && !is_amel(&m.name) {
                let mut names: Vec<String> = m.alleles.iter().map(|a| a.label.clone()).collect();
                names.push("F1".into());
                let mut hs: Vec<f64> = m.alleles.iter().map(|a| a.height).collect();
                hs.push(t as f64 - 1.0);
                let mut g = Gen::new(&method, &names, &hs, true);
                let x = min_x(&g.generate(false));
                if x > 0.0 {
                    used.push((m.name.clone(), x, json!("3F")));
                }
            }
        }
        fallback = !used.is_empty();
    }
    let avg = if used.is_empty() { 0.0 } else { py_sum(used.iter().map(|u| u.1)) / used.len() as f64 };
    if !(avg > 0.0) {
        return Ok(json!({"id": sample.id, "status": "no_mx", "average_mx": null, "mx_markers": [], "rows": [],
            "residual_threshold": fv(RESIDUAL_THRESHOLD), "adbi_markers": adbi, "missing_markers": [],
            "message": "Average Mx cannot be calculated: no 3- or 4-peak markers"}));
    }
    // Phase II
    let mut rows: Vec<RowD> = Vec::new();
    for m in &work {
        let mut names: Vec<String> = m.alleles.iter().take(4).map(|a| a.label.clone()).collect();
        let mut heights: Vec<f64> = m.alleles.iter().take(4).map(|a| a.height).collect();
        let mut calls = vec![(names.clone(), heights.clone())];
        if !is_amel(&m.name) {
            let mut k = 1;
            while names.len() < 4 && t - 1 > 0 {
                names.push(format!("F{}", k));
                heights.push(t as f64 - 1.0);
                calls.push((names.clone(), heights.clone()));
                k += 1;
            }
        }
        for (cn, ch) in &calls {
            let mut g = Gen::new(&method, cn, ch, false);
            let pats = g.generate(is_amel(&m.name));
            for p in pats {
                let obs = p.observed.unwrap_or_else(|| g.observed());
                let exp = if p.id == "1111" { [0.0; 4] } else { expected_vector(&p.id, avg) };
                let residual = py_sum((0..4).map(|i| py_pow(exp[i] - obs[i], 2.0)));
                let res_pass = residual <= RESIDUAL_THRESHOLD;
                let phr_status = match p.phr {
                    "pass" => "pass",
                    "fail" => "low",
                    _ => "na",
                };
                let iqv = iq(res_pass, p.phr);
                let dropout = p.c1.iter().chain(p.c2.iter()).any(|a| is_f(a));
                rows.push(RowD {
                    marker: m.name.clone(),
                    residual,
                    residual_pass: res_pass,
                    phr_status,
                    iq: iqv,
                    adbi: if adbi.contains(&m.name) { "check" } else { "pass" },
                    dropout,
                    selected: iqv == "pass",
                    known: None,
                    known_match: None,
                    known_lean: None,
                    pat: p,
                });
            }
        }
    }
    let mut known_match: Value = Value::Null;
    let mut known_contributor: Value = Value::Null;
    let mut known_norm: Vec<(String, Vec<String>)> = Vec::new();
    let known_given = truthy(known);
    if known_given {
        known_norm = normalize_known(known)?;
        for r in rows.iter_mut() {
            let k = known_norm.iter().find(|(m, _)| *m == r.marker).map(|(_, a)| a.clone());
            match k {
                Some(k) if !k.is_empty() => {
                    let km = format!(
                        "{}{}",
                        if known_genotype_match(&r.pat.c1, &k) { "C1" } else { "" },
                        if known_genotype_match(&r.pat.c2, &k) { "C2" } else { "" }
                    );
                    r.selected = r.iq == "pass" && !km.is_empty();
                    r.known = Some(k);
                    r.known_match = Some(km);
                }
                _ => {
                    r.known = Some(vec![]);
                    r.known_match = Some(String::new());
                }
            }
        }
        let (mut h1, mut h2, mut counted) = (0usize, 0usize, 0usize);
        let mut keys: Vec<&String> = known_norm.iter().map(|(m, _)| m).filter(|m| rows.iter().any(|r| &r.marker == *m)).collect();
        keys.sort();
        keys.dedup();
        for mk in keys {
            counted += 1;
            let sel: Vec<&RowD> = rows.iter().filter(|r| &r.marker == mk && r.selected).collect();
            if sel.iter().any(|r| r.known_match.as_deref().unwrap_or("").contains("C1")) {
                h1 += 1;
            }
            if sel.iter().any(|r| r.known_match.as_deref().unwrap_or("").contains("C2")) {
                h2 += 1;
            }
        }
        let kc: Option<&str> = if counted > 0 && h1 == counted {
            Some("c1")
        } else if counted > 0 && h2 == counted {
            Some("c2")
        } else {
            None
        };
        if let Some(kc) = kc {
            let key = kc.to_uppercase();
            for r in rows.iter_mut() {
                if r.selected && r.known.as_ref().map(|k| !k.is_empty()).unwrap_or(false) && !r.known_match.as_deref().unwrap_or("").contains(&key) {
                    r.selected = false;
                }
            }
            known_match = sv(if key == "C1" { "C1 (Major)" } else { "C2 (Minor)" });
            known_contributor = sv(kc);
        } else {
            known_match = sv("Undetermined");
            let lean = if h1 >= h2 { "C1" } else { "C2" };
            for r in rows.iter_mut() {
                r.known_lean = Some(lean);
            }
        }
    }
    let present: BTreeSet<String> = rows.iter().map(|r| r.marker.clone()).collect();
    let selected: BTreeSet<String> = rows.iter().filter(|r| r.selected).map(|r| r.marker.clone()).collect();
    let missing: Vec<String> = present.difference(&selected).cloned().collect();
    let known_missing: Vec<String> = if !known_norm.is_empty() {
        let kset: BTreeSet<&String> = known_norm.iter().map(|(m, _)| m).collect();
        present.iter().filter(|m| !kset.contains(m)).cloned().collect()
    } else {
        vec![]
    };
    Ok(json!({
        "id": sample.id, "status": "extracted", "average_mx": fv(avg), "fallback_3f": fallback,
        "mx_markers": used.iter().map(|(k, v, n)| json!({"marker": k, "mx": fv(*v), "peaks": n})).collect::<Vec<_>>(),
        "residual_threshold": fv(RESIDUAL_THRESHOLD), "adbi_markers": adbi,
        "rows": rows.iter().map(|r| row_value(r, known_given)).collect::<Vec<_>>(),
        "missing_markers": missing,
        "known_match": known_match, "known_contributor": known_contributor,
        "known_missing_markers": known_missing,
    }))
}

// ---------- population databases ----------
fn pybool_head(k: &str) -> bool {
    matches!(k, "module" | "version" | "database" | "data source")
}

fn dict_pairs(cells: &[String]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for c in cells {
        if let Some((k, v)) = c.split_once('=') {
            if let Some(e) = out.iter_mut().find(|(ek, _)| ek == k) {
                e.1 = v.to_string();
            } else {
                out.push((k.to_string(), v.to_string()));
            }
        }
    }
    out
}
fn pget<'a>(vals: &'a [(String, String)], k: &str) -> Option<&'a String> {
    vals.iter().find(|(ek, _)| ek == k).map(|(_, v)| v)
}

pub fn parse_population_db(text: &Value) -> Res<Value> {
    let text = match text {
        Value::String(s) if s.chars().count() <= 8 * 1024 * 1024 => s,
        _ => return Err("Population database text required (at most 8 MiB)".into()),
    };
    let mut head: Vec<(String, String)> = Vec::new();
    let mut pops: Vec<(String, f64, Vec<(String, i64, Vec<(String, Option<f64>)>)>)> = Vec::new();
    let mut have_marker = false;
    for raw in crate::gm_kit_import::splitlines_pub(text) {
        let line = strip(strip(raw).trim_end_matches(',')).to_string();
        if line.is_empty() {
            continue;
        }
        let cells: Vec<String> = line.split(',').map(|c| strip(c).to_string()).collect();
        let key = strip(cells[0].split('=').next().unwrap()).to_lowercase();
        if cells[0].contains('=') && pybool_head(&key) {
            let v = strip(line.split_once('=').unwrap().1).to_string();
            if let Some(e) = head.iter_mut().find(|(k, _)| *k == key) {
                e.1 = v;
            } else {
                head.push((key, v));
            }
            continue;
        }
        if key == "population" {
            let vals = dict_pairs(&cells);
            let tv = pget(&vals, "Theta").or_else(|| pget(&vals, "theta")).cloned().unwrap_or_else(|| "0".to_string());
            let theta = if tv.is_empty() { 0.0 } else { float_str(&tv)? };
            if !(0.0 <= theta && theta <= 1.0) {
                return Err("Theta must be between 0 and 1".into());
            }
            pops.push((strip(pget(&vals, "Population").map(|s| s.as_str()).unwrap_or("")).to_string(), theta, Vec::new()));
            have_marker = false;
            continue;
        }
        if key == "marker" {
            if pops.is_empty() {
                return Err("Marker line before any Population line".into());
            }
            let vals = dict_pairs(&cells);
            let nv = pget(&vals, "N").or_else(|| pget(&vals, "n")).cloned().unwrap_or_else(|| "0".to_string());
            let n = py_int_str(&nv)?;
            if !(3 <= n && n <= 99999) {
                return Err(format!("{}: N must be 3..99999", pget(&vals, "Marker").cloned().unwrap_or_else(|| "None".into())));
            }
            let name = match pget(&vals, "Marker") {
                Some(m) => strip(m).to_string(),
                None => return key_err("Marker"),
            };
            let pop = pops.last_mut().unwrap();
            if let Some(e) = pop.2.iter_mut().find(|(k, _, _)| *k == name) {
                *e = (name, n as i64, Vec::new());
            } else {
                pop.2.push((name, n as i64, Vec::new()));
            }
            have_marker = true;
            continue;
        }
        if cells[0].to_lowercase() == "allele" {
            continue;
        }
        if !have_marker || cells.len() < 2 {
            let cut: String = raw.chars().take(60).collect();
            return Err(format!("Unexpected line in population database: {}", py_repr(&sv(&cut))));
        }
        let freq = &cells[1];
        let val = if freq == "*" {
            None
        } else {
            let v = float_str(freq)?;
            if !(0.0 <= v && v < 1.0) {
                return Err(format!("Allele frequency must be >= 0 and < 1: {}={}", cells[0], freq));
            }
            Some(v)
        };
        // the marker is the last one of the current population (Python keeps a reference to that marker dict)
        let marker = pops.last_mut().unwrap().2.last_mut().unwrap();
        if let Some(e) = marker.2.iter_mut().find(|(k, _)| *k == cells[0]) {
            e.1 = val;
        } else {
            marker.2.push((cells[0].clone(), val));
        }
    }
    let hget = |k: &str| head.iter().find(|(hk, _)| hk == k).map(|(_, v)| v.clone());
    if hget("module").is_none() || hget("database").is_none() || pops.is_empty() {
        return Err("Not a GeneMapper ID-X population database (Module, Database and Population lines required)".into());
    }
    let pv: Vec<Value> = pops
        .iter()
        .map(|(name, theta, markers)| {
            let mut mm = Map::new();
            for (mname, n, freq) in markers {
                let mut fm = Map::new();
                for (a, v) in freq {
                    fm.insert(a.clone(), ofv(*v));
                }
                mm.insert(mname.clone(), json!({"n": n, "freq": Value::Object(fm)}));
            }
            json!({"name": name, "theta": fv(*theta), "markers": Value::Object(mm)})
        })
        .collect();
    Ok(json!({"module": hget("module"), "version": hget("version").unwrap_or_else(|| "1.0".into()),
        "database": hget("database"), "source": hget("data source").unwrap_or_default(), "populations": pv}))
}

// ---------- statistics ----------
struct MarkerDb {
    n: f64,
    freq: Map<String, Value>,
}
struct Pop {
    name: Value,
    theta_v: Value,
    theta: f64,
    markers: Map<String, Value>,
}

impl Pop {
    fn from(v: &Value) -> Res<Pop> {
        let m = as_dict(v, "get")?;
        let theta_v = getk(m, "theta")?.clone();
        let theta = py_float(&theta_v)?;
        let markers = match getk(m, "markers")? {
            Value::Object(o) => o.clone(),
            other => return Err(format!("'{}' object has no attribute 'get'", type_name(other))),
        };
        Ok(Pop { name: getk(m, "name")?.clone(), theta_v, theta, markers })
    }
    fn marker(&self, name: &str) -> Res<Option<MarkerDb>> {
        match self.markers.get(name) {
            None => Ok(None),
            Some(Value::Object(m)) => {
                let n = py_float(getk(m, "n")?)?;
                let freq = match getk(m, "freq")? {
                    Value::Object(f) => f.clone(),
                    other => return Err(format!("'{}' object has no attribute 'get'", type_name(other))),
                };
                Ok(Some(MarkerDb { n, freq }))
            }
            Some(other) => Err(format!("'{}' object has no attribute 'get'", type_name(other))),
        }
    }
}

fn allele_frequency(pop: &Pop, marker: &str, allele: &str) -> Res<Option<f64>> {
    let m = match pop.marker(marker)? {
        Some(m) => m,
        None => return Ok(None),
    };
    let minimum = 5.0 / (2.0 * m.n);
    let v = match m.freq.get(allele) {
        None | Some(Value::Null) => None,
        Some(x) => Some(py_float(x)?),
    };
    Ok(Some(match v {
        None => minimum,
        Some(v) if v < minimum => minimum,
        Some(v) => v,
    }))
}

fn stat_marker(name: &str) -> bool {
    let u = name.to_uppercase();
    u != "AMEL" && u != "AMELOGENIN" && !u.starts_with("DYS") && !u.starts_with('Y')
}

fn genotype_frequency(pop: &Pop, marker: &str, a: &str, b: &str) -> Res<Option<f64>> {
    let (a, b) = (fnorm(a), fnorm(b));
    if a == F && b == F {
        return Ok(Some(1.0));
    }
    if b == F || a == F {
        let p = allele_frequency(pop, marker, if b == F { &a } else { &b })?;
        return Ok(p.map(|p| 2.0 * p));
    }
    let p = match allele_frequency(pop, marker, &a)? {
        Some(p) => p,
        None => return Ok(None),
    };
    if a == b {
        return Ok(Some(p * p + p * (1.0 - p) * pop.theta));
    }
    Ok(allele_frequency(pop, marker, &b)?.map(|q| 2.0 * p * q))
}

fn pairs_of(v: &Value) -> Res<Vec<Vec<String>>> {
    match v {
        Value::Array(a) => a.iter().map(as_strings).collect(),
        other => Err(format!("'{}' object is not iterable", type_name(other))),
    }
}

fn combined_frequency(pop: &Pop, marker: &str, genotypes: &[Vec<String>]) -> Res<Option<f64>> {
    let norm: Vec<Vec<String>> = genotypes.iter().map(|g| g.iter().map(|x| fnorm(x)).collect()).collect();
    for g in &norm {
        if g.len() < 2 {
            return Err("list index out of range".into());
        }
    }
    if norm.iter().any(|g| g[0] == F && g[1] == F) {
        return Ok(Some(1.0));
    }
    let mut gts: BTreeSet<(String, String)> = BTreeSet::new();
    for g in &norm {
        let mut s = g.clone();
        s.sort_by(|x, y| (x == F, x).cmp(&(y == F, y)));
        gts.insert((s[0].clone(), s[1].clone()));
    }
    let drop: BTreeSet<&String> = gts.iter().filter(|g| g.1 == F).map(|g| &g.0).collect();
    let mut total = 0.0;
    for g in &gts {
        if g.1 != F && (drop.contains(&g.0) || drop.contains(&g.1)) {
            continue;
        }
        match genotype_frequency(pop, marker, &g.0, &g.1)? {
            None => return Ok(None),
            Some(f) => total += f,
        }
    }
    Ok(Some(total))
}

fn rmp(profile: &Value, pop: &Pop) -> Res<Value> {
    let prof = match profile {
        Value::Object(m) => m,
        other => return Err(format!("'{}' object has no attribute 'items'", type_name(other))),
    };
    let (mut details, mut missing, mut product) = (Vec::new(), Vec::new(), 1.0f64);
    for (marker, item) in prof {
        if !stat_marker(marker) {
            continue;
        }
        let io = as_dict(item, "get")?;
        if truthy(get_or(io, "inconclusive")) || !truthy(get_or(io, "genotypes")) {
            details.push(json!({"marker": marker, "frequency": 1.0, "note": "inconclusive"}));
            continue;
        }
        let gts = pairs_of(&io["genotypes"])?;
        match combined_frequency(pop, marker, &gts)? {
            None => {
                missing.push(sv(marker));
                continue;
            }
            Some(f) => {
                product *= f;
                details.push(json!({"marker": marker, "frequency": fv(f), "genotypes": io["genotypes"]}));
            }
        }
    }
    Ok(json!({"profile_frequency": fv(product), "rmp": if product > 0.0 { fv(1.0 / product) } else { Value::Null },
        "missing_markers": missing, "details": details}))
}

fn sorted_pair(a: &[String]) -> Vec<String> {
    let mut v = a.to_vec();
    v.sort();
    v
}

struct SsItem {
    genotypes: Vec<Vec<String>>,
    inconclusive: bool,
    min: bool,
}

fn single_source_profile(markers: &Map<String, Value>, options: &Value) -> Res<Vec<(String, SsItem)>> {
    let opts: Map<String, Value> = match options {
        Value::Object(o) => o.clone(),
        v if !truthy(v) => Map::new(),
        other => return Err(format!("'{}' object has no attribute 'get'", type_name(other))),
    };
    let mut prof = Vec::new();
    for (marker, alleles_v) in markers {
        let alleles = as_strings(alleles_v)?;
        let opt: String = match opts.get(marker) {
            Some(v) if truthy(v) => py_str(v),
            _ => (if alleles.len() == 3 { "min" } else { "alleles" }).to_string(),
        };
        let item = if opt == "inconclusive" || alleles.is_empty() {
            SsItem { genotypes: vec![], inconclusive: true, min: false }
        } else if alleles.len() == 1 {
            let g = if opt == "2p" { vec![alleles[0].clone(), F.to_string()] } else { vec![alleles[0].clone(), alleles[0].clone()] };
            SsItem { genotypes: vec![g], inconclusive: false, min: false }
        } else if alleles.len() == 2 {
            SsItem { genotypes: vec![sorted_pair(&alleles)], inconclusive: false, min: false }
        } else {
            let mut gts = Vec::new();
            for i in 0..alleles.len() {
                for j in i + 1..alleles.len() {
                    gts.push(sorted_pair(&[alleles[i].clone(), alleles[j].clone()]));
                }
            }
            SsItem { genotypes: gts, inconclusive: false, min: true }
        };
        prof.push((marker.clone(), item));
    }
    Ok(prof)
}

fn gts_value(g: &[Vec<String>]) -> Value {
    Value::Array(g.iter().map(|x| strs(x)).collect())
}

/// Returns the keys details, missing_markers, profile_frequency, rmp as an ordered map.
fn rmp_single(markers: &Value, pop: &Pop, options: &Value) -> Res<Map<String, Value>> {
    let mk = match markers {
        Value::Object(m) => m.clone(),
        v if !truthy(v) => Map::new(),
        other => return Err(format!("'{}' object has no attribute 'items'", type_name(other))),
    };
    let prof = single_source_profile(&mk, options)?;
    let (mut details, mut missing, mut product) = (Vec::new(), Vec::new(), 1.0f64);
    for (marker, item) in &prof {
        if !stat_marker(marker) {
            continue;
        }
        if item.inconclusive {
            details.push(json!({"marker": marker, "frequency": 1.0, "note": "inconclusive"}));
            continue;
        }
        let mut freqs = Vec::new();
        for g in &item.genotypes {
            freqs.push(genotype_frequency(pop, marker, &g[0], &g[1])?);
        }
        if freqs.iter().any(|f| f.is_none()) {
            missing.push(sv(marker));
            continue;
        }
        let fl: Vec<f64> = freqs.iter().map(|f| f.unwrap()).collect();
        let f = if item.min {
            let mut b = fl[0];
            for &x in &fl[1..] {
                if x < b {
                    b = x;
                }
            }
            b
        } else {
            fl[0]
        };
        product *= f;
        details.push(json!({"marker": marker, "frequency": fv(f), "genotypes": gts_value(&item.genotypes)}));
    }
    let mut out = Map::new();
    out.insert("details".into(), Value::Array(details));
    out.insert("missing_markers".into(), Value::Array(missing));
    out.insert("profile_frequency".into(), fv(product));
    out.insert("rmp".into(), if product > 0.0 { fv(1.0 / product) } else { Value::Null });
    Ok(out)
}

fn cpi(markers: &Value, pop: &Pop, dropout: &BTreeSet<String>) -> Res<Map<String, Value>> {
    let mk = match markers {
        Value::Object(m) => m.clone(),
        v if !truthy(v) => Map::new(),
        other => return Err(format!("'{}' object has no attribute 'items'", type_name(other))),
    };
    let (mut details, mut missing, mut product) = (Vec::new(), Vec::new(), 1.0f64);
    for (marker, alleles_v) in &mk {
        if !stat_marker(marker) || !truthy(alleles_v) {
            continue;
        }
        let alleles = match alleles_v {
            Value::Array(a) => a,
            other => return Err(format!("'{}' object is not iterable", type_name(other))),
        };
        if dropout.contains(marker) {
            details.push(json!({"marker": marker, "alleles": alleles_v, "inclusion": 1.0, "combinations": null}));
            continue;
        }
        let mut ps = Vec::new();
        for a in alleles {
            let key = match a {
                Value::String(s) => s.clone(),
                other => return Err(format!("unsupported allele type {}", type_name(other))),
            };
            ps.push(allele_frequency(pop, marker, &key)?);
        }
        if ps.iter().any(|p| p.is_none()) {
            missing.push(sv(marker));
            continue;
        }
        let mif = py_pow(py_sum(ps.iter().map(|p| p.unwrap())), 2.0);
        let n = alleles.len();
        product *= mif;
        details.push(json!({"marker": marker, "alleles": alleles_v, "inclusion": fv(mif), "combinations": n * (n + 1) / 2}));
    }
    let mut out = Map::new();
    out.insert("cpi".into(), fv(product));
    out.insert("one_in".into(), if product > 0.0 { fv(1.0 / product) } else { Value::Null });
    out.insert("cpe".into(), fv(1.0 - product));
    out.insert("missing_markers".into(), Value::Array(missing));
    out.insert("details".into(), Value::Array(details));
    Ok(out)
}

fn get_pops(db_v: &Value, req: &Map<String, Value>) -> Res<(Vec<Value>, Vec<Value>)> {
    // returns (names, population dict values selected)
    let pops_all = match db_v {
        Value::Object(d) => match d.get("populations") {
            Some(Value::Array(a)) => a.clone(),
            _ => vec![],
        },
        _ => vec![],
    };
    let names_v = or_default(req.get("populations"), Value::Null);
    let names: Vec<Value> = if names_v.is_null() {
        let mut n = Vec::new();
        for p in &pops_all {
            n.push(getk(as_dict(p, "get")?, "name")?.clone());
        }
        n
    } else {
        match &names_v {
            Value::Array(a) => a.clone(),
            Value::String(s) => {
                // `p['name'] in "text"` is a substring test; emulate by marking a single string
                vec![Value::String(s.clone()), Value::Bool(true)]
            }
            other => return Err(format!("argument of type '{}' is not iterable", type_name(other))),
        }
    };
    let mut sel = Vec::new();
    for p in &pops_all {
        let name = getk(as_dict(p, "get")?, "name")?;
        let inn = if names.len() == 2 && names[1] == Value::Bool(true) && names[0].is_string() && names_v.is_string() {
            match name {
                Value::String(n) => names[0].as_str().unwrap().contains(n.as_str()),
                other => return Err(format!("'in <string>' requires string as left operand, not {}", type_name(other))),
            }
        } else {
            names.iter().any(|n| crate::pullup_quality::py_eq(n, name))
        };
        if inn {
            sel.push(p.clone());
        }
    }
    Ok((names, sel))
}

pub fn parse_db_arg(v: &Value) -> Res<Value> {
    match v {
        Value::String(_) => parse_population_db(v),
        other => Ok(other.clone()),
    }
}

pub fn statistics(request: &Map<String, Value>) -> Res<Value> {
    let db = parse_db_arg(get_or(request, "population_db"))?;
    let dbm = match &db {
        Value::Object(m) if truthy(get_or(m, "populations")) => m,
        _ => return Err("Select a population database".into()),
    };
    let (_, pops) = get_pops(&db, request)?;
    if pops.is_empty() {
        return Err("Select at least one population".into());
    }
    let kind = get_or(request, "kind");
    let mut out = Vec::new();
    for pv in &pops {
        let pop = Pop::from(pv)?;
        let mut row = Map::new();
        row.insert("population".into(), pop.name.clone());
        row.insert("theta".into(), pop.theta_v.clone());
        match kind.as_str() {
            Some("single") => {
                let mk = or_default(request.get("markers"), json!({}));
                let r = rmp_single(&mk, &pop, get_or(request, "options"))?;
                for (k, v) in r {
                    row.insert(k, v);
                }
            }
            Some("two") => {
                let kc = get_or(request, "known_contributor");
                if !(kc.is_null() || kc == &json!("c1") || kc == &json!("c2")) {
                    return Err("known_contributor must be c1 or c2".into());
                }
                let c1 = if kc == &json!("c1") { Value::Null } else { rmp(&or_default(request.get("c1"), json!({})), &pop)? };
                let c2 = if kc == &json!("c2") { Value::Null } else { rmp(&or_default(request.get("c2"), json!({})), &pop)? };
                row.insert("c1".into(), c1);
                row.insert("c2".into(), c2);
                row.insert("known_contributor".into(), kc.clone());
            }
            Some("cpi") => {
                let mk = or_default(request.get("markers"), json!({}));
                let drops = or_default(request.get("dropout_markers"), json!([]));
                let set: BTreeSet<String> = match &drops {
                    Value::Array(a) => a.iter().map(|x| py_str(x)).collect(),
                    other => return Err(format!("'{}' object is not iterable", type_name(other))),
                };
                let r = cpi(&mk, &pop, &set)?;
                for (k, v) in r {
                    row.insert(k, v);
                }
            }
            _ => return Err("kind must be single, two or cpi".into()),
        }
        out.push(Value::Object(row));
    }
    Ok(json!({"database": getk(dbm, "database")?, "kind": kind, "results": out}))
}

// ---------- likelihood ratio ----------
fn norm_g(g: &[String]) -> Vec<String> {
    let mut v: Vec<String> = g.iter().map(|a| fnorm(a)).collect();
    v.sort();
    v
}

fn ref_fits(refg: &[String], gt: &[String]) -> bool {
    let mut r: Vec<String> = if refg.len() == 1 { vec![refg[0].clone(), refg[0].clone()] } else { refg.to_vec() };
    let gt = norm_g(gt);
    r.sort();
    if !gt.iter().any(|a| a == F) {
        return gt == r;
    }
    let mut rest = r;
    for a in gt.iter().filter(|a| *a != F) {
        match rest.iter().position(|x| x == a) {
            None => return false,
            Some(i) => {
                rest.remove(i);
            }
        }
    }
    true
}

fn hypothesis(h: &str) -> (&'static str, &'static str) {
    match h {
        "single" | "major" => ("C1: R1", "C1: unknown"),
        "h1" => ("R1 + R2", "R1 + unknown"),
        "h2" => ("R1 + R2", "R2 + unknown"),
        "h3" => ("R1 + R2", "two unknowns"),
        "h4" => ("R1 + unknown", "two unknowns"),
        _ => ("R2 + unknown", "two unknowns"),
    }
}

struct EvRow {
    marker: String,
    c1: Vec<String>,
    c2: Vec<String>,
    inc_c1: bool,
    inc_c2: bool,
}

impl EvRow {
    fn pos(&self, k: &str) -> &Vec<String> {
        if k == "c1" {
            &self.c1
        } else {
            &self.c2
        }
    }
}

struct RefP {
    id: String,
    markers: Vec<(String, Vec<String>)>,
}
impl RefP {
    fn get(&self, m: &str) -> Option<&Vec<String>> {
        self.markers.iter().find(|(k, _)| k == m).map(|(_, v)| v)
    }
    fn has(&self, m: &str) -> bool {
        self.get(m).is_some()
    }
}

fn positions(rows: &[EvRow], refp: &RefP, markers: &[String]) -> Vec<&'static str> {
    let mut out = Vec::new();
    for key in ["c1", "c2"] {
        if markers.iter().all(|m| rows.iter().filter(|r| &r.marker == m).any(|r| ref_fits(refp.get(m).unwrap(), r.pos(key)))) {
            out.push(key);
        }
    }
    out
}

fn prob(rows: &[EvRow], m: &str, known: &[(&str, &Vec<String>)], pop: &Pop) -> Res<Option<f64>> {
    let mut seen: BTreeSet<Vec<Vec<String>>> = BTreeSet::new();
    let mut total = 0.0;
    for r in rows {
        if r.marker != m {
            continue;
        }
        if known.iter().any(|(k, rf)| !ref_fits(rf, r.pos(k))) {
            continue;
        }
        let unknown: Vec<&str> = ["c1", "c2"].into_iter().filter(|k| !known.iter().any(|(kk, _)| kk == k)).collect();
        let key: Vec<Vec<String>> = unknown.iter().map(|k| norm_g(r.pos(k))).collect();
        if !seen.insert(key) {
            continue;
        }
        let mut f = 1.0;
        for k in &unknown {
            let g = norm_g(r.pos(k));
            if g.len() < 2 {
                return Err("list index out of range".into());
            }
            match genotype_frequency(pop, m, &g[0], &g[1])? {
                None => return Ok(None),
                Some(x) => f *= x,
            }
        }
        total += f;
    }
    Ok(Some(total))
}

fn bool_get(m: &Map<String, Value>, k: &str) -> bool {
    truthy(get_or(m, k))
}

pub fn likelihood_ratio(request: &Map<String, Value>) -> Res<Value> {
    let db = parse_db_arg(get_or(request, "population_db"))?;
    // `request.get('populations') or [p['name'] for p in db['populations']]` needs db to be a dict with populations
    let dbm = match &db {
        Value::Object(m) => m,
        other => return Err(format!("'{}' object is not subscriptable", type_name(other))),
    };
    if !dbm.contains_key("populations") && !truthy(get_or(request, "populations")) {
        return key_err("populations");
    }
    let (_, pop_vals) = get_pops(&db, request)?;
    if pop_vals.is_empty() {
        return Err("Select at least one population".into());
    }
    let refs_in: Vec<Value> = match or_default(request.get("references"), json!([])) {
        Value::Array(a) => a.into_iter().filter(truthy).collect(),
        other => return Err(format!("'{}' object is not iterable", type_name(&other))),
    };
    if refs_in.is_empty() {
        return Err("Select a reference sample".into());
    }
    let mut refs: Vec<RefP> = Vec::new();
    for r in &refs_in {
        let ro = as_dict(r, "get").map_err(|_| "string indices must be integers, not 'str'".to_string())?;
        let id = py_str(getk(ro, "id")?);
        let mm = as_dict(getk(ro, "markers")?, "items")?;
        let mut markers = Vec::new();
        for (m, v) in mm {
            if !truthy(v) {
                continue;
            }
            let list: Vec<String> = match v {
                Value::Array(a) => a.iter().map(|x| py_str(x)).collect(),
                other => return Err(format!("'{}' object is not iterable", type_name(other))),
            };
            markers.push((m.clone(), list));
        }
        refs.push(RefP { id, markers });
    }
    let kind = get_or(request, "kind");
    if kind == &json!("single") {
        let evidence = or_default(request.get("markers"), json!({}));
        let evm = as_dict(&evidence, "items")?;
        let options = or_default(request.get("options"), json!({}));
        let opts = as_dict(&options, "get")?;
        let r1 = &refs[0];
        let mut markers: Vec<String> = Vec::new();
        for (m, a) in evm {
            if truthy(a) && stat_marker(m) && opts.get(m) != Some(&json!("inconclusive")) {
                markers.push(m.clone());
            }
        }
        let not_in_ref: Vec<String> = markers.iter().filter(|m| !r1.has(m)).cloned().collect();
        let markers: Vec<String> = markers.into_iter().filter(|m| r1.has(m)).collect();
        let mut mismatch: Vec<String> = Vec::new();
        for m in &markers {
            let ev = as_strings(&evm[m])?;
            let rf = r1.get(m).unwrap();
            let evset: BTreeSet<&String> = ev.iter().collect();
            let rset: BTreeSet<&String> = rf.iter().collect();
            let mut ok = if ev.len() <= 2 { evset == rset } else { rset.is_subset(&evset) };
            if ev.len() == 1 && opts.get(m) == Some(&json!("2p")) {
                ok = rf.contains(&ev[0]);
            }
            if !ok {
                mismatch.push(m.clone());
            }
        }
        let status = json!({"R1": if mismatch.is_empty() { "match" } else { "no_match" }, "mismatch": mismatch});
        let hyps: Vec<&str> = if mismatch.is_empty() { vec!["single"] } else { vec![] };
        let mut results = Vec::new();
        for pv in &pop_vals {
            let pop = Pop::from(pv)?;
            for h in &hyps {
                let (mut rows, mut missing, mut num, mut den) = (Vec::new(), Vec::new(), 1.0f64, 1.0f64);
                for m in &markers {
                    let mut one = Map::new();
                    one.insert(m.clone(), evm[m].clone());
                    let f = rmp_single(&Value::Object(one), &pop, &options)?;
                    if !f["missing_markers"].as_array().unwrap().is_empty() {
                        missing.push(sv(m));
                        continue;
                    }
                    let d = f["details"][0]["frequency"].as_f64().unwrap();
                    num *= 1.0;
                    den *= d;
                    rows.push(json!({"marker": m, "hp": 1.0, "hd": fv(d)}));
                }
                let (hp, hd) = hypothesis(h);
                results.push(json!({"population": pop.name, "theta": pop.theta_v, "hypothesis": h, "hp": hp, "hd": hd,
                    "numerator": fv(num), "denominator": fv(den), "lr": if den > 0.0 { fv(num / den) } else { Value::Null },
                    "details": rows, "missing_markers": missing}));
            }
        }
        return Ok(json!({"kind": kind, "references": status, "not_in_reference": not_in_ref, "hypotheses": hyps, "results": results}));
    }
    if kind != &json!("two") {
        return Err("kind must be single or two".into());
    }
    let rows_in: Vec<Value> = match or_default(request.get("rows"), json!([])) {
        Value::Array(a) => a,
        other => return Err(format!("'{}' object is not iterable", type_name(&other))),
    };
    let use_c1 = bool_get(request, "use_c1_only");
    let mut rows: Vec<EvRow> = Vec::new();
    for r in &rows_in {
        let ro = as_dict(r, "get")?;
        if !(ro.get("selected").map(truthy).unwrap_or(true)) {
            continue;
        }
        let inc = |k: &str| -> Res<bool> {
            let v = or_default(ro.get("inconclusive"), json!({}));
            Ok(truthy(get_or(as_dict(&v, "get")?, k)))
        };
        rows.push(EvRow {
            marker: py_str(getk(ro, "marker")?),
            c1: as_strings(getk(ro, "c1")?)?,
            c2: as_strings(getk(ro, "c2")?)?,
            inc_c1: inc("c1")?,
            inc_c2: inc("c2")?,
        });
    }
    let mut markers: Vec<String> = Vec::new();
    for r in &rows {
        if !markers.contains(&r.marker) && stat_marker(&r.marker) {
            markers.push(r.marker.clone());
        }
    }
    let inconc: BTreeSet<&String> = markers.iter().filter(|m| rows.iter().any(|r| &r.marker == *m && r.inc_c1)).collect();
    let inconc2: BTreeSet<&String> = markers.iter().filter(|m| rows.iter().any(|r| &r.marker == *m && r.inc_c2)).collect();
    let not_in_ref: BTreeSet<&String> = markers.iter().filter(|m| refs.iter().any(|rf| !rf.has(m))).collect();
    let usable: Vec<String> = markers.iter().filter(|m| refs.iter().all(|rf| rf.has(m))).cloned().collect();
    let pos: Vec<Vec<&'static str>> = refs.iter().map(|rf| positions(&rows, rf, &usable)).collect();
    let mut status = Map::new();
    for (i, rf) in refs.iter().enumerate() {
        let mut miss: Vec<Vec<String>> = Vec::new();
        for k in ["c1", "c2"] {
            miss.push(
                usable
                    .iter()
                    .filter(|m| !rows.iter().filter(|r| &r.marker == *m).any(|r| ref_fits(rf.get(m).unwrap(), r.pos(k))))
                    .cloned()
                    .collect(),
            );
        }
        status.insert(
            format!("R{}", i + 1),
            json!({"id": rf.id, "positions": pos[i], "match": !pos[i].is_empty(), "mismatch_c1": miss[0], "mismatch_c2": miss[1]}),
        );
    }
    let mut hyps: Vec<&str> = Vec::new();
    let p1 = pos[0].clone();
    let p2: Vec<&str> = if refs.len() > 1 { pos[1].clone() } else { vec![] };
    if use_c1 {
        for m in &usable {
            let set: BTreeSet<Vec<String>> = rows.iter().filter(|r| &r.marker == m).map(|r| norm_g(&r.c1)).collect();
            if set.len() > 1 {
                return Err("Use C1 (Major) Only needs one C1 genotype per marker in the Selected Genotype Combinations table".into());
            }
        }
        if p1.contains(&"c1") {
            hyps = vec!["major"];
        }
    } else {
        if !p1.is_empty() {
            hyps.push("h4");
        }
        if refs.len() > 1 && !p2.is_empty() {
            hyps.push("h5");
        }
        if refs.len() > 1 && !p1.is_empty() && !p2.is_empty() && p1.iter().any(|a| p2.iter().any(|b| a != b)) {
            let mut h = vec!["h1", "h2", "h3"];
            h.extend(hyps.iter());
            hyps = h;
        }
    }
    let a1: Option<&str> = if p1.contains(&"c1") { Some("c1") } else { p1.first().copied() };
    let a2: Option<&str> = if !p2.is_empty() { p2.iter().copied().find(|x| Some(*x) != a1) } else { None };
    let mut results = Vec::new();
    for pv in &pop_vals {
        let pop = Pop::from(pv)?;
        for h in &hyps {
            let (mut num, mut den) = (1.0f64, 1.0f64);
            let (mut details, mut missing) = (Vec::new(), Vec::new());
            for m in &usable {
                if inconc.contains(m) || (inconc2.contains(m) && !use_c1) {
                    details.push(json!({"marker": m, "note": "inconclusive"}));
                    continue;
                }
                let (hp, hd): (Option<f64>, Option<f64>);
                if *h == "major" {
                    hp = Some(1.0);
                    let mut seen: BTreeSet<Vec<String>> = BTreeSet::new();
                    let mut acc: Option<f64> = Some(0.0);
                    for r in rows.iter().filter(|r| &r.marker == m) {
                        let n = norm_g(&r.c1);
                        if seen.contains(&n) {
                            continue;
                        }
                        seen.insert(n.clone());
                        if n.len() < 2 {
                            return Err("list index out of range".into());
                        }
                        let g = genotype_frequency(pop_ref(&pop), m, &n[0], &n[1])?;
                        acc = match (g, acc) {
                            (Some(g), Some(a)) => Some(a + g),
                            _ => None,
                        };
                    }
                    hd = acc;
                } else {
                    let r1 = refs[0].get(m).unwrap();
                    let r2 = if refs.len() > 1 { refs[1].get(m) } else { None };
                    let k1: Vec<(&str, &Vec<String>)> = match a1 {
                        Some(a) => vec![(a, r1)],
                        None => vec![],
                    };
                    let k2: Vec<(&str, &Vec<String>)> = match (a2, r2) {
                        (Some(a), Some(r)) => vec![(a, r)],
                        (Some(_), None) => return Err("list index out of range".into()),
                        _ => vec![],
                    };
                    if *h == "h4" {
                        let key = a1.unwrap_or(p1[0]);
                        hp = prob(&rows, m, &[(key, r1)], &pop)?;
                        hd = prob(&rows, m, &[], &pop)?;
                    } else if *h == "h5" {
                        let pos5 = a2.unwrap_or(p2[0]);
                        hp = prob(&rows, m, &[(pos5, r2.ok_or("list index out of range")?)], &pop)?;
                        hd = prob(&rows, m, &[], &pop)?;
                    } else {
                        let mut both = k1.clone();
                        for (k, v) in &k2 {
                            if let Some(e) = both.iter_mut().find(|(ek, _)| ek == k) {
                                e.1 = v;
                            } else {
                                both.push((k, v));
                            }
                        }
                        hp = prob(&rows, m, &both, &pop)?;
                        hd = if *h == "h1" {
                            prob(&rows, m, &k1, &pop)?
                        } else if *h == "h2" {
                            prob(&rows, m, &k2, &pop)?
                        } else {
                            prob(&rows, m, &[], &pop)?
                        };
                    }
                }
                match (hp, hd) {
                    (Some(hp), Some(hd)) => {
                        num *= hp;
                        den *= hd;
                        details.push(json!({"marker": m, "hp": fv(hp), "hd": fv(hd)}));
                    }
                    _ => {
                        missing.push(sv(m));
                    }
                }
            }
            let (hpl, hdl) = hypothesis(h);
            results.push(json!({"population": pop.name, "theta": pop.theta_v, "hypothesis": h, "hp": hpl, "hd": hdl,
                "numerator": fv(num), "denominator": fv(den), "lr": if den > 0.0 { fv(num / den) } else { Value::Null },
                "details": details, "missing_markers": missing}));
        }
    }
    Ok(json!({"kind": kind, "references": Value::Object(status),
        "assigned": {"R1": a1, "R2": a2},
        "not_in_reference": not_in_ref, "hypotheses": hyps, "results": results}))
}

fn pop_ref(p: &Pop) -> &Pop {
    p
}

// ---------- merge ----------
pub fn merge(request: &Map<String, Value>) -> Res<Value> {
    let sources: Vec<Value> = match or_default(request.get("sources"), json!([])) {
        Value::Array(a) => a,
        other => return Err(format!("'{}' object is not iterable", type_name(&other))),
    };
    if sources.len() < 2 {
        return Err("Merging needs two or more samples with the same name".into());
    }
    let kind = match request.get("kind") {
        Some(v) => v.clone(),
        None => json!("two"),
    };
    let mut order: Vec<String> = Vec::new();
    for s in &sources {
        let so = as_dict(s, "get").map_err(|_| "string indices must be integers, not 'str'".to_string())?;
        let ms = match getk(so, "markers")? {
            Value::Array(a) => a,
            other => return Err(format!("'{}' object is not iterable", type_name(other))),
        };
        for m in ms {
            let name = getk(as_dict(m, "get")?, "name")?;
            let n = match name {
                Value::String(s) => s.clone(),
                other => py_str(other),
            };
            if !order.contains(&n) {
                order.push(n);
            }
        }
    }
    let two = kind == json!("two");
    let mut out = Vec::new();
    for name in &order {
        let mut variants: Vec<(Map<String, Value>, Value)> = Vec::new();
        for s in &sources {
            let so = s.as_object().unwrap();
            let ms = so["markers"].as_array().unwrap();
            let m = match ms.iter().find(|x| x.as_object().map(|o| o.get("name").map(|n| py_str(n)) == Some(name.clone())).unwrap_or(false)) {
                Some(m) => m.as_object().unwrap(),
                None => continue,
            };
            if two {
                let rows = match getk(m, "rows")? {
                    Value::Array(a) => a,
                    other => return Err(format!("'{}' object is not iterable", type_name(other))),
                };
                let mut sel = Vec::new();
                for r in rows {
                    if truthy(get_or(as_dict(r, "get")?, "selected")) {
                        sel.push(r.clone());
                    }
                }
                if !sel.is_empty() {
                    let mut key: Vec<(Vec<String>, Vec<String>)> = Vec::new();
                    for r in &sel {
                        let ro = r.as_object().unwrap();
                        key.push((norm_g(&as_strings(getk(ro, "c1")?)?), norm_g(&as_strings(getk(ro, "c2")?)?)));
                    }
                    key.sort();
                    let mut v = Map::new();
                    v.insert("source".into(), getk(so, "id")?.clone());
                    v.insert("rows".into(), Value::Array(sel));
                    variants.push((v, json!(key.iter().map(|(a, b)| json!([a, b])).collect::<Vec<_>>())));
                }
            } else if truthy(getk(m, "alleles")?) {
                let al = getk(m, "alleles")?;
                let mut keyv = as_strings(al)?;
                keyv.sort();
                let mut v = Map::new();
                v.insert("source".into(), getk(so, "id")?.clone());
                v.insert("alleles".into(), al.clone());
                variants.push((v, json!(keyv)));
            }
        }
        if variants.is_empty() {
            continue;
        }
        let base_key = variants[0].1.clone();
        let (status, source): (&str, Value) = if variants.len() == 1 {
            ("single", variants[0].0["source"].clone())
        } else if variants[1..].iter().all(|v| v.1 == base_key) {
            ("concordant", sv("<Multiple>"))
        } else {
            ("conflict", Value::Null)
        };
        out.push(json!({"marker": name, "status": status, "source": source,
            "chosen": if status == "conflict" { Value::Null } else { json!(0) },
            "variants": variants.into_iter().map(|(v, _)| Value::Object(v)).collect::<Vec<_>>()}));
    }
    Ok(json!({"kind": kind, "markers": out}))
}

pub fn mixture_request(request: &Value) -> Res<Value> {
    let req = match request {
        Value::Object(m) => m,
        _ => return Err("Mixture request must be an object".into()),
    };
    let action = req.get("action").and_then(|a| a.as_str());
    match action {
        Some("factory") => Ok(json!({"method": factory_method(), "residual_threshold": fv(RESIDUAL_THRESHOLD)})),
        Some("validate") => {
            let (_, v) = validate_method(get_or(req, "method"))?;
            Ok(json!({"method": v}))
        }
        Some("segregate") => {
            let method = or_default(req.get("method"), factory_method());
            let samples = match or_default(req.get("samples"), json!([])) {
                Value::Array(a) => a,
                other => return Err(format!("'{}' object is not iterable", type_name(&other))),
            };
            let mut out = Vec::new();
            for s in &samples {
                let (ok, why) = eligible(s)?;
                if ok {
                    out.push(segregate(s, &method)?);
                } else {
                    let so = as_dict(s, "get")?;
                    out.push(json!({"id": so.get("id").cloned().unwrap_or(Value::Null), "contributors": null, "ineligible": why}));
                }
            }
            Ok(json!({"samples": out}))
        }
        Some("extract") => extract(get_or(req, "sample"), &or_default(req.get("method"), factory_method()), get_or(req, "known")),
        Some("parse_known") => parse_known_genotypes(&or_default(req.get("text"), sv("")), &or_default(req.get("filename"), sv(""))),
        Some("merge_known") => merge_known(&or_default(req.get("existing"), json!({})), &or_default(req.get("new"), json!({}))),
        Some("parse_popdb") => parse_population_db(get_or(req, "text")),
        Some("statistics") => statistics(req),
        Some("lr") => likelihood_ratio(req),
        Some("merge") => merge(req),
        _ => Err("action must be factory, validate, segregate, extract, parse_popdb, statistics, lr or merge".into()),
    }
}

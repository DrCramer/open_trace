//! Python-compatible `json.loads` (CPython 3.13 C scanner semantics, error messages included) and the canonical
//! `json.dumps(sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)` used by the project store.
//!
//! Non-finite literals (`NaN`, `Infinity`, `1e999`) cannot live in a `serde_json::Value`; they are parsed into the
//! sentinel string [`NONFINITE`], which [`canonical`] rejects exactly like Python rejects a float that is not finite.
//! Lone surrogate escapes become [`SURROGATE`], which [`canonical`] rejects like Python's UTF-8 encoding error.
//! Known gap: integers beyond 64 bits are read as floats.
use serde_json::{Map, Number, Value};

pub const NONFINITE: &str = "\u{10FFFF}nonfinite";
pub const SURROGATE: char = '\u{10FFFE}';
const RECURSION_LIMIT: usize = 9999;

#[derive(Debug, Clone, PartialEq)]
pub enum LoadErr {
    /// `JSONDecodeError` (a ValueError): the message includes `line .. column .. (char ..)`.
    Decode(String),
    /// `RecursionError`.
    Recursion(String),
    /// Duplicate key found with `reject_dups`.
    Dup,
}

impl LoadErr {
    pub fn message(&self) -> String {
        match self {
            LoadErr::Decode(m) | LoadErr::Recursion(m) => m.clone(),
            LoadErr::Dup => "duplicate key".into(),
        }
    }
}

enum Stop {
    Value(usize),
    Err(LoadErr),
}

struct P<'a> {
    s: &'a [char],
    dups: bool,
}

fn decode_err(s: &[char], msg: &str, pos: usize) -> LoadErr {
    let line = s[..pos.min(s.len())].iter().filter(|&&c| c == '\n').count() + 1;
    let col = match s[..pos.min(s.len())].iter().rposition(|&c| c == '\n') {
        Some(i) => pos - i,
        None => pos + 1,
    };
    LoadErr::Decode(format!("{}: line {} column {} (char {})", msg, line, col, pos))
}

fn ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

impl<'a> P<'a> {
    fn err(&self, msg: &str, pos: usize) -> Stop {
        Stop::Err(decode_err(self.s, msg, pos))
    }
    fn skip(&self, mut i: usize) -> usize {
        while i < self.s.len() && ws(self.s[i]) {
            i += 1;
        }
        i
    }
    fn hex4(&self, from: usize, end: usize) -> Result<u32, Stop> {
        let mut c = 0u32;
        for i in from..from + 4 {
            let d = match self.s[i] {
                ch @ '0'..='9' => ch as u32 - '0' as u32,
                ch @ 'a'..='f' => ch as u32 - 'a' as u32 + 10,
                ch @ 'A'..='F' => ch as u32 - 'A' as u32 + 10,
                _ => return Err(self.err("Invalid \\uXXXX escape", end - 5)),
            };
            c = (c << 4) | d;
        }
        Ok(c)
    }
    fn string(&self, mut end: usize, out: &mut String) -> Result<usize, Stop> {
        let len = self.s.len();
        let begin = end - 1;
        loop {
            let mut next = end;
            let mut c = '\0';
            while next < len {
                let d = self.s[next];
                c = d;
                if d == '"' || d == '\\' {
                    break;
                }
                if (d as u32) <= 0x1f {
                    return Err(self.err("Invalid control character at", next));
                }
                next += 1;
            }
            if next >= len {
                return Err(self.err("Unterminated string starting at", begin));
            }
            out.extend(&self.s[end..next]);
            next += 1;
            if c == '"' {
                return Ok(next);
            }
            if next == len {
                return Err(self.err("Unterminated string starting at", begin));
            }
            let e = self.s[next];
            if e != 'u' {
                end = next + 1;
                let r = match e {
                    '"' => '"',
                    '\\' => '\\',
                    '/' => '/',
                    'b' => '\u{8}',
                    'f' => '\u{c}',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    _ => return Err(self.err("Invalid \\escape", end - 2)),
                };
                out.push(r);
            } else {
                next += 1;
                end = next + 4;
                if end > len {
                    return Err(self.err("Invalid \\uXXXX escape", next - 1));
                }
                let mut c = self.hex4(next, end)?;
                next = end;
                if (0xD800..=0xDBFF).contains(&c) && end + 6 < len && self.s[next] == '\\' && self.s[next + 1] == 'u' {
                    end += 6;
                    let c2 = self.hex4(next + 2, end)?;
                    if (0xDC00..=0xDFFF).contains(&c2) {
                        c = 0x10000 + ((c - 0xD800) << 10) + (c2 - 0xDC00);
                    } else {
                        end -= 6;
                    }
                }
                match char::from_u32(c) {
                    Some(ch) => out.push(ch),
                    None => out.push(SURROGATE),
                }
            }
        }
    }
    fn number(&self, start: usize) -> Result<(Value, usize), Stop> {
        let s = self.s;
        let len = s.len();
        let end_idx = len - 1;
        let mut idx = start;
        if s[idx] == '-' {
            idx += 1;
            if idx > end_idx {
                return Err(Stop::Value(start));
            }
        }
        match s[idx] {
            '1'..='9' => {
                idx += 1;
                while idx <= end_idx && s[idx].is_ascii_digit() {
                    idx += 1;
                }
            }
            '0' => idx += 1,
            _ => return Err(Stop::Value(start)),
        }
        let mut is_float = false;
        if idx < end_idx && s[idx] == '.' && s[idx + 1].is_ascii_digit() {
            is_float = true;
            idx += 1;
            while idx <= end_idx && s[idx].is_ascii_digit() {
                idx += 1;
            }
        }
        if idx < end_idx && (s[idx] == 'e' || s[idx] == 'E') {
            let e_start = idx;
            idx += 1;
            if idx < end_idx && (s[idx] == '-' || s[idx] == '+') {
                idx += 1;
            }
            let ds = idx;
            while idx <= end_idx && s[idx].is_ascii_digit() {
                idx += 1;
            }
            if idx == ds {
                idx = e_start;
            } else {
                is_float = true;
            }
        }
        let text: String = s[start..idx].iter().collect();
        if !is_float {
            if let Ok(i) = text.parse::<i64>() {
                return Ok((Value::from(i), idx));
            }
            if let Ok(u) = text.parse::<u64>() {
                return Ok((Value::from(u), idx));
            }
        }
        let f: f64 = text.parse().unwrap();
        Ok((num_or_sentinel(f), idx))
    }
    fn lit(&self, idx: usize, word: &str) -> bool {
        let w: Vec<char> = word.chars().collect();
        idx + w.len() <= self.s.len() && self.s[idx..idx + w.len()] == w[..]
    }
    fn scan(&self, idx: usize, depth: usize) -> Result<(Value, usize), Stop> {
        let s = self.s;
        if idx >= s.len() {
            return Err(Stop::Value(idx));
        }
        match s[idx] {
            '"' => {
                let mut out = String::new();
                let n = self.string(idx + 1, &mut out)?;
                Ok((Value::String(out), n))
            }
            '{' => {
                if depth + 1 >= RECURSION_LIMIT {
                    return Err(Stop::Err(LoadErr::Recursion(
                        "maximum recursion depth exceeded while decoding a JSON object from a unicode string".into(),
                    )));
                }
                self.object(idx + 1, depth + 1)
            }
            '[' => {
                if depth + 1 >= RECURSION_LIMIT {
                    return Err(Stop::Err(LoadErr::Recursion(
                        "maximum recursion depth exceeded while decoding a JSON array from a unicode string".into(),
                    )));
                }
                self.array(idx + 1, depth + 1)
            }
            'n' if self.lit(idx, "null") => Ok((Value::Null, idx + 4)),
            't' if self.lit(idx, "true") => Ok((Value::Bool(true), idx + 4)),
            'f' if self.lit(idx, "false") => Ok((Value::Bool(false), idx + 5)),
            'N' if self.lit(idx, "NaN") => Ok((Value::String(NONFINITE.into()), idx + 3)),
            'I' if self.lit(idx, "Infinity") => Ok((Value::String(NONFINITE.into()), idx + 8)),
            '-' if self.lit(idx, "-Infinity") => Ok((Value::String(NONFINITE.into()), idx + 9)),
            'n' | 't' | 'f' | 'N' | 'I' => Err(Stop::Value(idx)),
            _ => self.number(idx),
        }
    }
    fn object(&self, mut idx: usize, depth: usize) -> Result<(Value, usize), Stop> {
        let s = self.s;
        let mut pairs: Vec<(String, Value)> = Vec::new();
        idx = self.skip(idx);
        if idx >= s.len() || s[idx] != '}' {
            loop {
                if idx >= s.len() || s[idx] != '"' {
                    return Err(self.err("Expecting property name enclosed in double quotes", idx));
                }
                let mut key = String::new();
                idx = self.string(idx + 1, &mut key)?;
                idx = self.skip(idx);
                if idx >= s.len() || s[idx] != ':' {
                    return Err(self.err("Expecting ':' delimiter", idx));
                }
                idx = self.skip(idx + 1);
                let (v, n) = self.scan(idx, depth)?;
                pairs.push((key, v));
                idx = self.skip(n);
                if idx < s.len() && s[idx] == '}' {
                    break;
                }
                if idx >= s.len() || s[idx] != ',' {
                    return Err(self.err("Expecting ',' delimiter", idx));
                }
                let comma = idx;
                idx = self.skip(idx + 1);
                if idx < s.len() && s[idx] == '}' {
                    return Err(self.err("Illegal trailing comma before end of object", comma));
                }
            }
        }
        let mut m = Map::new();
        for (k, v) in pairs {
            if self.dups && m.contains_key(&k) {
                return Err(Stop::Err(LoadErr::Dup));
            }
            m.insert(k, v);
        }
        Ok((Value::Object(m), idx + 1))
    }
    fn array(&self, mut idx: usize, depth: usize) -> Result<(Value, usize), Stop> {
        let s = self.s;
        let mut items = Vec::new();
        idx = self.skip(idx);
        if idx >= s.len() || s[idx] != ']' {
            loop {
                let (v, n) = self.scan(idx, depth)?;
                items.push(v);
                idx = self.skip(n);
                if idx < s.len() && s[idx] == ']' {
                    break;
                }
                if idx >= s.len() || s[idx] != ',' {
                    return Err(self.err("Expecting ',' delimiter", idx));
                }
                let comma = idx;
                idx = self.skip(idx + 1);
                if idx < s.len() && s[idx] == ']' {
                    return Err(self.err("Illegal trailing comma before end of array", comma));
                }
            }
        }
        Ok((Value::Array(items), idx + 1))
    }
}

pub fn num_or_sentinel(f: f64) -> Value {
    match Number::from_f64(f) {
        Some(n) => Value::Number(n),
        None => Value::String(NONFINITE.into()),
    }
}

fn run(text: &str, dups: bool) -> Result<Value, LoadErr> {
    let s: Vec<char> = text.chars().collect();
    if s.first() == Some(&'\u{feff}') {
        return Err(decode_err(&s, "Unexpected UTF-8 BOM (decode using utf-8-sig)", 0));
    }
    let p = P { s: &s, dups };
    let start = p.skip(0);
    let (v, end) = match p.scan(start, 0) {
        Ok(x) => x,
        Err(Stop::Value(pos)) => return Err(decode_err(&s, "Expecting value", pos)),
        Err(Stop::Err(e)) => return Err(e),
    };
    let end = p.skip(end);
    if end != s.len() {
        return Err(decode_err(&s, "Extra data", end));
    }
    Ok(v)
}

/// Python `json.loads(text)`; with `reject_dups` an `object_pairs_hook` rejecting repeated keys.
pub fn loads(text: &str, reject_dups: bool) -> Result<Value, LoadErr> {
    if text.len() < 2000 {
        return run(text, reject_dups);
    }
    let owned = text.to_string();
    std::thread::Builder::new()
        .stack_size(512 * 1024 * 1024)
        .spawn(move || run(&owned, reject_dups))
        .expect("spawn")
        .join()
        .expect("json thread")
}

fn write_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if n.is_f64() {
                out.push_str(&crate::kit_definition::py_float_repr(n.as_f64().unwrap()));
            } else {
                out.push_str(&n.to_string());
            }
        }
        Value::String(s) => write_str(s, out),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(x, out);
            }
            out.push(']');
        }
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(k, out);
                out.push(':');
                write(&m[k.as_str()], out);
            }
            out.push('}');
        }
    }
}

fn check(v: &Value, depth: usize, limit: usize, surrogate: &mut bool) -> Result<(), String> {
    if depth > limit {
        return Err("JSON exceeds maximum nesting depth".into());
    }
    let bad = || "value must contain only finite JSON values and string keys".to_string();
    match v {
        Value::Null | Value::Bool(_) => Ok(()),
        Value::Number(n) => {
            if n.is_f64() && !n.as_f64().unwrap().is_finite() {
                Err(bad())
            } else {
                Ok(())
            }
        }
        Value::String(s) => {
            if s == NONFINITE {
                return Err(bad());
            }
            *surrogate |= s.contains(SURROGATE);
            Ok(())
        }
        Value::Array(a) => a.iter().try_for_each(|x| check(x, depth + 1, limit, surrogate)),
        Value::Object(m) => {
            for (k, x) in m {
                *surrogate |= k.contains(SURROGATE);
                check(x, depth + 1, limit, surrogate)?;
            }
            Ok(())
        }
    }
}

/// The store's `_canonical(value, limit, depth_limit=...)`; errors are `ProjectStoreError` messages.
pub fn canonical(v: &Value, limit: usize, depth_limit: usize) -> Result<String, String> {
    let mut surrogate = false;
    check(v, 0, depth_limit, &mut surrogate)?;
    if surrogate {
        return Err("invalid JSON value".into());
    }
    let mut out = String::new();
    write(v, &mut out);
    if out.len() > limit {
        return Err(format!("JSON exceeds {} byte limit", limit));
    }
    Ok(out)
}

//! Python `json.dumps` with the options the application uses (`ensure_ascii`, `sort_keys`, `indent`) and a
//! Python-compatible `str()` / `repr()` for JSON values.
use crate::kit_definition::py_float_repr;
use serde_json::Value;

#[derive(Clone, Copy)]
pub struct Opts {
    pub ensure_ascii: bool,
    pub sort_keys: bool,
    pub indent: Option<usize>,
}

pub const PLAIN: Opts = Opts { ensure_ascii: true, sort_keys: false, indent: None };
pub const UTF8: Opts = Opts { ensure_ascii: false, sort_keys: false, indent: None };

fn string(s: &str, ascii: bool, out: &mut String) {
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
            c if ascii && (c as u32) > 0x7e => {
                let mut b = [0u16; 2];
                for u in c.encode_utf16(&mut b) {
                    out.push_str(&format!("\\u{:04x}", u));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write(v: &Value, o: Opts, level: usize, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) if n.is_f64() => out.push_str(&py_float_repr(n.as_f64().unwrap())),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => string(s, o.ensure_ascii, out),
        Value::Array(a) => {
            if a.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                match o.indent {
                    Some(n) => {
                        if i > 0 {
                            out.push(',');
                        }
                        out.push('\n');
                        out.push_str(&" ".repeat(n * (level + 1)));
                    }
                    None => {
                        if i > 0 {
                            out.push_str(", ");
                        }
                    }
                }
                write(x, o, level + 1, out);
            }
            if let Some(n) = o.indent {
                out.push('\n');
                out.push_str(&" ".repeat(n * level));
            }
            out.push(']');
        }
        Value::Object(m) => {
            if m.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            let mut items: Vec<(&String, &Value)> = m.iter().collect();
            if o.sort_keys {
                items.sort_by(|a, b| a.0.cmp(b.0));
            }
            for (i, (k, x)) in items.into_iter().enumerate() {
                match o.indent {
                    Some(n) => {
                        if i > 0 {
                            out.push(',');
                        }
                        out.push('\n');
                        out.push_str(&" ".repeat(n * (level + 1)));
                    }
                    None => {
                        if i > 0 {
                            out.push_str(", ");
                        }
                    }
                }
                string(k, o.ensure_ascii, out);
                out.push_str(": ");
                write(x, o, level + 1, out);
            }
            if let Some(n) = o.indent {
                out.push('\n');
                out.push_str(&" ".repeat(n * level));
            }
            out.push('}');
        }
    }
}

pub fn dumps(v: &Value, o: Opts) -> String {
    let mut out = String::new();
    write(v, o, 0, &mut out);
    out
}

fn printable(c: char) -> bool {
    let u = c as u32;
    !(u < 0x20 || (0x7f..=0xa0).contains(&u) || u == 0xad || (0x2028..=0x2029).contains(&u) || (0x200b..=0x200f).contains(&u) || u == 0xfeff)
}

/// Python `repr(str)`.
pub fn str_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::new();
    out.push(quote);
    for c in s.chars() {
        match c {
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if !printable(c) => {
                let u = c as u32;
                if u < 0x100 {
                    out.push_str(&format!("\\x{:02x}", u));
                } else if u < 0x10000 {
                    out.push_str(&format!("\\u{:04x}", u));
                } else {
                    out.push_str(&format!("\\U{:08x}", u));
                }
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

pub fn py_repr(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) if n.is_f64() => py_float_repr(n.as_f64().unwrap()),
        Value::Number(n) => n.to_string(),
        Value::String(s) => str_repr(s),
        Value::Array(a) => format!("[{}]", a.iter().map(py_repr).collect::<Vec<_>>().join(", ")),
        Value::Object(m) => format!("{{{}}}", m.iter().map(|(k, x)| format!("{}: {}", str_repr(k), py_repr(x))).collect::<Vec<_>>().join(", ")),
    }
}

/// Python `str(value)`.
pub fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => py_repr(other),
    }
}

pub fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

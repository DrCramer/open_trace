//! Counterpart of scratchpad/pj_check.py: each input line is a JSON string literal holding a document.
use opentrace_core::pyjson;
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};

fn show(r: Result<Value, pyjson::LoadErr>, doc: &str) -> String {
    match r {
        Ok(_) if deep(doc) => "DEEPOK".into(),
        Ok(v) => match pyjson::canonical(&v, usize::MAX, 100000) {
            Ok(s) => format!("OK {}", s),
            Err(e) if e.starts_with("value must") => "NONFINITE".into(),
            Err(e) if e == "invalid JSON value" => "SURR".into(),
            Err(e) => format!("CANON {}", e),
        },
        Err(pyjson::LoadErr::Dup) => "DUP".into(),
        Err(pyjson::LoadErr::Decode(m)) => format!("ERR JSONDecodeError {}", m),
        Err(pyjson::LoadErr::Recursion(m)) => format!("ERR RecursionError {}", m),
    }
}
fn deep(doc: &str) -> bool {
    doc.starts_with("[[[[[[[[[[") || doc.starts_with("{\"a\":{\"a\":{\"a\":") || doc.starts_with("[{\"a\":[{\"a\":")
}
fn main() {
    let file = File::open(std::env::args().nth(1).unwrap()).unwrap();
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let doc: String = serde_json::from_str(&line.unwrap()).unwrap();
        println!("PJ {} {} | {}", i, show(pyjson::loads(&doc, false), &doc), show(pyjson::loads(&doc, true), &doc));
    }
}

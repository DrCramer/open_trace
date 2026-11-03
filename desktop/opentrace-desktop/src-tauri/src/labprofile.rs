//! Active laboratory profile of this desktop and the check that an analysis really used it.
//!
//! Activating an approved `lab_profile` version fetches it from the server, re-computes the SHA-256 of the profile
//! and of its analysis method and size standard locally (they must equal the hashes the server pinned), and derives
//! the analysis settings those two standards produce — with the same core function the web UI calls
//! (`/api/analysis-method`, `analyze_settings`). An analysis is attributed to the profile in its provenance only when
//! the settings the UI actually sent are exactly these (same keys, same values; nothing extra such as a manual sizing
//! override or a matrix). Otherwise the provenance is recorded without a profile and the reason is shown.
use crate::remote::Remote;
use serde_json::{json, Map, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct Active {
    pub name: String,
    pub version: i64,
    pub content_sha256: String,
    pub method: String,
    pub size_standard: String,
    /// Settings the profile's method + size standard produce (nulls dropped).
    pub expected: Map<String, Value>,
    /// Approved contents used to check genotyping kits (`check_kit_build`).
    pub method_content: Value,
    pub kit: Value,
    pub bin_set: Value,
    pub stutter: Option<Value>,
}

fn enc(s: &str) -> String {
    crate::workspace::enc(s)
}

fn member<'a>(members: &'a [Value], kind: &str) -> Result<&'a Value, String> {
    members.iter().find(|m| m["kind"] == kind).ok_or_else(|| format!("в профиле нет «{}»", kind))
}

/// One approved standard version, with the locally recomputed hash checked against the pinned one.
fn fetch_pinned(remote: &Remote, ca: Option<&std::path::Path>, m: &Value) -> Result<Value, String> {
    let (kind, name, version) = (m["kind"].as_str().unwrap_or(""), m["name"].as_str().unwrap_or(""), m["version"].as_i64().unwrap_or(0));
    let v = remote
        .get_json(ca, &format!("/api/v1/standards/{}/{}/{}", enc(kind), enc(name), version))
        .map_err(|e| format!("{} «{}» v{}: {}", kind, name, version, e.to_json()["error"].as_str().unwrap_or("ошибка сервера")))?;
    let local = crate::canonical_sha256(&v["content"]);
    if Some(local.as_str()) != m["content_sha256"].as_str() || Some(local.as_str()) != v["content_sha256"].as_str() {
        return Err(format!("{} «{}» v{}: SHA-256 содержимого не совпадает с закреплённым в профиле", kind, name, version));
    }
    Ok(v["content"].clone())
}

pub fn resolve(remote: &Remote, ca: Option<&std::path::Path>, core: &opentrace_core::server::App, name: &str, version: i64) -> Result<Active, String> {
    let p = remote
        .get_json(ca, &format!("/api/v1/standards/lab_profile/{}/{}", enc(name), version))
        .map_err(|e| format!("профиль «{}» v{}: {}", name, version, e.to_json()["error"].as_str().unwrap_or("ошибка сервера")))?;
    if p["status"] != "approved" {
        return Err(format!("профиль «{}» v{} не утверждён (статус {})", name, version, p["status"].as_str().unwrap_or("?")));
    }
    let sha = crate::canonical_sha256(&p["content"]);
    if Some(sha.as_str()) != p["content_sha256"].as_str() {
        return Err("SHA-256 профиля не совпадает с сервером".into());
    }
    let members = p["content"]["members"].as_array().cloned().unwrap_or_default();
    let mm = member(&members, "analysis_method")?;
    let sm = member(&members, "size_standard")?;
    let method = fetch_pinned(remote, ca, mm)?;
    let std = fetch_pinned(remote, ca, sm)?;
    let kit = fetch_pinned(remote, ca, member(&members, "kit")?)?;
    let bin_set = fetch_pinned(remote, ca, member(&members, "bin_set")?)?;
    let stutter = match member(&members, "stutter") {
        Ok(m) => Some(fetch_pinned(remote, ca, m)?),
        Err(_) => None,
    };
    let body = json!({"action": "analyze_settings", "method": method, "standard_bp": std["sizes"], "standard_dye": std["dye"]});
    let req = opentrace_core::server::Request {
        method: "POST".into(),
        target: "/api/analysis-method".into(),
        headers: vec![("host".into(), format!("127.0.0.1:{}", core.bound_port())), ("x-session-token".into(), core.token.clone())],
        body: serde_json::to_vec(&body).unwrap(),
    };
    let r = core.post(&req);
    let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
    if r.code != 200 {
        return Err(format!("настройки метода профиля: {}", v["error"].as_str().unwrap_or("ошибка ядра")));
    }
    let expected: Map<String, Value> = v["settings"].as_object().cloned().unwrap_or_default().into_iter().filter(|(_, x)| !x.is_null()).collect();
    Ok(Active {
        name: name.to_string(),
        version,
        content_sha256: sha,
        method: mm["name"].as_str().unwrap_or("").to_string(),
        size_standard: sm["name"].as_str().unwrap_or("").to_string(),
        expected,
        method_content: method,
        kit,
        bin_set,
        stutter,
    })
}

fn num_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| num_eq(p, q)),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, p)| y.get(k).map(|q| num_eq(p, q)).unwrap_or(false)),
        _ => a == b,
    }
}

/// The UI sends every setting as a string (`JSON.stringify` for objects/arrays, `String(v)` otherwise).
fn same(expected: &Value, sent: &str) -> bool {
    match expected {
        Value::String(s) => s == sent,
        Value::Bool(b) => sent == if *b { "true" } else { "false" },
        Value::Number(n) => sent.parse::<f64>().ok() == n.as_f64(),
        Value::Array(_) | Value::Object(_) => serde_json::from_str::<Value>(sent).map(|g| num_eq(expected, &g)).unwrap_or(false),
        Value::Null => false,
    }
}

/// `Ok` when the sent settings are exactly the profile's; otherwise the first difference, for the user.
pub fn check(active: &Active, sent: &Value) -> Result<(), String> {
    let sent = sent.as_object().cloned().unwrap_or_default();
    for (k, v) in &sent {
        match active.expected.get(k) {
            None => return Err(format!("параметр «{}» не входит в профиль (ручное изменение?)", k)),
            Some(e) if !same(e, v.as_str().unwrap_or("")) => return Err(format!("параметр «{}» отличается от профиля", k)),
            _ => {}
        }
    }
    if let Some(k) = active.expected.keys().find(|k| !sent.contains_key(*k)) {
        return Err(format!("нет параметра профиля «{}»", k));
    }
    Ok(())
}

/// Canonical JSON with every number written from its f64 value, so a structure that went through JavaScript
/// (`100.0` comes back as `100`) hashes the same as the original.
pub fn norm_hash(v: &Value) -> String {
    fn walk(v: &Value, out: &mut String) {
        match v {
            Value::Number(n) => {
                let f = n.as_f64().unwrap_or(f64::NAN);
                if f.fract() == 0.0 && f.abs() < 1e15 {
                    out.push_str(&format!("{}", f as i64));
                } else {
                    out.push_str(&format!("{:e}", f));
                }
            }
            Value::Array(a) => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    walk(x, out);
                }
                out.push(']');
            }
            Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                out.push('{');
                for (i, k) in keys.into_iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(k).unwrap());
                    out.push(':');
                    walk(&m[k], out);
                }
                out.push('}');
            }
            other => out.push_str(&other.to_string()),
        }
    }
    let mut s = String::new();
    walk(v, &mut s);
    crate::provenance::sha256_hex(s.as_bytes())
}

/// A kit build the UI asked for (`/api/import-gm-kit` with a panel: GeneMapper panels/bins/stutter text it generated
/// from its local library, plus the analysis method) against the approved kit, bin set, stutter and method of the
/// profile. The texts are parsed with the core's own parsers and compared structure by structure.
pub fn check_kit_build(active: &Active, req: &Value) -> Result<(), String> {
    use opentrace_core::gm_kit_import::{parse_bins, parse_panels, parse_stutter};
    if norm_hash(&req["analysis_method"]) != norm_hash(&active.method_content) {
        return Err("метод анализа набора не совпадает с методом профиля".into());
    }
    let panels = parse_panels(&req["panels"]).map_err(|e| format!("панели: {}", e))?;
    let kit_name = active.kit["name"].as_str().unwrap_or("");
    let parsed_kit = panels["kits"].as_array().and_then(|k| k.iter().find(|k| k["name"] == kit_name)).ok_or_else(|| format!("набор «{}» не из профиля", kit_name))?;
    let strip = |ps: &Value| -> Vec<Value> {
        ps.as_array().cloned().unwrap_or_default().into_iter().map(|p| json!({"name": p["name"], "markers": p["markers"]})).collect()
    };
    if !num_eq(&Value::Array(strip(&parsed_kit["panels"])), &Value::Array(strip(&active.kit["panels"]))) {
        return Err(format!("панели набора «{}» отличаются от утверждённой версии", kit_name));
    }
    let panel = req["panel"].as_str().unwrap_or("");
    if !active.kit["panels"].as_array().map(|ps| ps.iter().any(|p| p["name"] == panel)).unwrap_or(false) {
        return Err(format!("панель «{}» не входит в набор профиля", panel));
    }
    let bins = parse_bins(&req["bins"]).map_err(|e| format!("бины: {}", e))?;
    if !num_eq(&bins["panels"], &active.bin_set["panels"]) {
        return Err("бины отличаются от утверждённого набора бинов профиля".into());
    }
    match (&active.stutter, req["stutter"].as_str()) {
        (None, None) => {}
        (Some(st), Some(_)) => {
            let parsed = parse_stutter(&req["stutter"]).map_err(|e| format!("статтеры: {}", e))?;
            if !num_eq(&parsed["panels"], &st["stutter"]["panels"]) {
                return Err("статтеры отличаются от утверждённой версии профиля".into());
            }
        }
        (None, Some(_)) => return Err("использованы статтеры, которых нет в профиле".into()),
        (Some(_), None) => return Err("статтеры профиля не использованы".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active() -> Active {
        let expected = json!({"threshold": 50, "standard_bp": [75, 100.5], "channel_thresholds": {"DATA1": 50}, "sizing_mode": "auto", "smooth": true});
        Active {
            name: "P".into(),
            version: 1,
            content_sha256: "x".into(),
            method: "M".into(),
            size_standard: "S".into(),
            expected: expected.as_object().unwrap().clone(),
            method_content: json!({}),
            kit: json!({}),
            bin_set: json!({}),
            stutter: None,
        }
    }

    #[test]
    fn exact_settings_match_as_the_ui_sends_them() {
        let sent = json!({"threshold": "50", "standard_bp": "[75,100.5]", "channel_thresholds": "{\"DATA1\":50.0}", "sizing_mode": "auto", "smooth": "true"});
        assert_eq!(check(&active(), &sent), Ok(()));
    }

    #[test]
    fn any_difference_or_extra_parameter_is_reported() {
        let mut sent = json!({"threshold": "51", "standard_bp": "[75,100.5]", "channel_thresholds": "{\"DATA1\":50}", "sizing_mode": "auto", "smooth": "true"});
        assert!(check(&active(), &sent).unwrap_err().contains("threshold"));
        sent["threshold"] = json!("50");
        sent["sizing_override"] = json!("{}");
        assert!(check(&active(), &sent).unwrap_err().contains("sizing_override"));
        sent.as_object_mut().unwrap().remove("sizing_override");
        sent.as_object_mut().unwrap().remove("smooth");
        assert!(check(&active(), &sent).unwrap_err().contains("smooth"));
    }
}

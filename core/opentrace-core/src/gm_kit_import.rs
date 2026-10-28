//! Port of `genemapper_open/gm_kit_import.py`: GeneMapper Panels/Bins/Stutter text readers and kit builder.
//!
//! Known gaps: `str.casefold` is approximated (lowercase + sharp s), so label matching is exact for ASCII
//! labels only; Python `float()` of non-ASCII digits is not accepted; exception types (KitFormatError /
//! ValueError / TypeError) collapse into one error string.
use crate::analysis_method::{kit_marker_parameters, ladder_quality_definition, load_analysis_method, marker_quality_definition};
use crate::kit_definition::load_kit_definition;
use crate::profiles::{py_float_parse, py_repr, py_isspace_pub, strip};
use serde_json::{json, Map, Value};

pub type Res<T> = Result<T, String>;

const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

fn truthy(v: &Value) -> bool {
    crate::pullup_quality::truthy(v)
}

pub(crate) fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) => {
            if n.is_f64() {
                "float"
            } else {
                "int"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

fn casefold(s: &str) -> String {
    s.to_lowercase().replace('ß', "ss")
}

/// Python `str.splitlines()`.
fn splitlines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0;
    while i < bytes.len() {
        let (pos, c) = bytes[i];
        if matches!(c, '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}') {
            out.push(&text[start..pos]);
            let mut next = pos + c.len_utf8();
            if c == '\r' && i + 1 < bytes.len() && bytes[i + 1].1 == '\n' {
                next += 1;
                i += 1;
            }
            start = next;
        }
        i += 1;
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

type Row = (usize, Vec<String>);

fn lines(text: &Value, what: &str) -> Res<Vec<Row>> {
    let text = match text {
        Value::String(s) => s,
        _ => return Err(format!("{} text required", what)),
    };
    if text.len() > MAX_TEXT_BYTES {
        return Err(format!("{} file exceeds 2 MiB", what));
    }
    let mut rows = Vec::new();
    for (i, line) in splitlines(text).into_iter().enumerate() {
        if strip(line).is_empty() {
            continue;
        }
        rows.push((i + 1, line.split('\t').map(|f| strip(f).to_string()).collect()));
    }
    if rows.is_empty() {
        return Err(format!("{} file is empty", what));
    }
    Ok(rows)
}

fn float(value: &str, wh: &str) -> Res<f64> {
    match py_float_parse(value) {
        None => Err(format!("{}: number expected, got {}", wh, py_repr(&Value::String(value.to_string())))),
        Some(x) if !x.is_finite() => Err(format!("{}: finite number expected", wh)),
        Some(x) => Ok(x),
    }
}

fn strip_quotes_ws(s: &str) -> &str {
    strip(s.trim_matches('"').trim_start_matches(py_isspace_pub).trim_end_matches(py_isspace_pub))
}

fn labels(value: &str) -> Vec<Value> {
    let v = strip(strip(value).trim_matches('"'));
    let lower = v.to_lowercase();
    if matches!(lower.as_str(), "" | "none" | "null") {
        return vec![];
    }
    v.split(',')
        .map(|p| strip_quotes_ws(strip(p)))
        .filter(|p| !p.is_empty())
        .map(|p| Value::String(p.to_string()))
        .collect()
}

fn field<'a>(f: &'a [String], i: usize) -> Res<&'a String> {
    f.get(i).ok_or_else(|| "list index out of range".to_string())
}

fn opt_field(f: &[String], i: usize) -> String {
    f.get(i).cloned().unwrap_or_default()
}

fn is_digits_1_2(s: &str) -> bool {
    (1..=2).contains(&s.chars().count()) && s.chars().all(|c| c.is_ascii_digit())
}

pub fn parse_panels(text: &Value) -> Res<Value> {
    let rows = lines(text, "Panels")?;
    let mut header = Map::new();
    let mut kits: Vec<Map<String, Value>> = Vec::new();
    let mut panel_open = false;
    for (number, f) in rows {
        let key = f[0].as_str();
        match key {
            "Version" => {
                header.insert("version".into(), json!(opt_field(&f, 1)));
            }
            "Kit type:" => {
                let kt = opt_field(&f, 1);
                header.insert("kit_type".into(), json!(kt));
                if kt != "MICROSATELLITE" {
                    return Err(format!("line {}: only MICROSATELLITE kits are supported", number));
                }
            }
            "Chemistry Kit" => {
                let name = field(&f, 1)?.clone();
                let mut k = Map::new();
                k.insert("name".into(), json!(name));
                k.insert("panels".into(), json!([]));
                kits.push(k);
                panel_open = false;
            }
            "Panel" => {
                if kits.is_empty() {
                    return Err(format!("line {}: Panel before Chemistry Kit", number));
                }
                let name = field(&f, 1)?.clone();
                let kit = kits.last_mut().unwrap();
                kit.get_mut("panels").unwrap().as_array_mut().unwrap().push(json!({"name": name, "markers": []}));
                panel_open = true;
            }
            _ => {
                if !panel_open {
                    return Err(format!("line {}: marker row before Panel", number));
                }
                let mut f = f;
                if f.len() > 8 && f[1].is_empty() && !f[2].is_empty() {
                    let mut g = vec![f[0].clone()];
                    g.extend_from_slice(&f[2..]);
                    f = g;
                }
                if f.len() < 8 {
                    return Err(format!("line {}: marker row needs at least 8 fields", number));
                }
                let wh = format!("line {} ({})", number, f[0]);
                let repeat = &f[5];
                if !is_digits_1_2(repeat) {
                    return Err(format!("{}: integer repeat expected", wh));
                }
                let mut flags: Vec<bool> = Vec::new();
                let mut rest: Vec<&String> = f[7..].iter().collect();
                while !rest.is_empty() && flags.len() < 2 {
                    let l = rest[0].to_lowercase();
                    if l != "true" && l != "false" {
                        break;
                    }
                    rest.remove(0);
                    flags.push(l == "true");
                }
                if rest.len() > 1 {
                    return Err(format!("{}: unexpected fields after the marker flags", wh));
                }
                let min_bp = float(&f[2], &wh)?;
                let max_bp = float(&f[3], &wh)?;
                let mut marker = Map::new();
                marker.insert("name".into(), json!(f[0]));
                marker.insert("dye".into(), json!(f[1].to_lowercase()));
                marker.insert("min_bp".into(), fv(min_bp));
                marker.insert("max_bp".into(), fv(max_bp));
                marker.insert("control_alleles".into(), Value::Array(labels(&f[4])));
                marker.insert("repeat".into(), json!(repeat.parse::<i64>().unwrap()));
                marker.insert("option".into(), json!(f[6]));
                marker.insert("y_marker".into(), json!(flags.first().copied().unwrap_or(false)));
                marker.insert("ladder_alleles".into(), Value::Array(rest.first().map(|r| labels(r)).unwrap_or_default()));
                if flags.len() > 1 {
                    marker.insert("internal_qc".into(), json!(flags[1]));
                }
                let kit = kits.last_mut().unwrap();
                let panels = kit.get_mut("panels").unwrap().as_array_mut().unwrap();
                panels.last_mut().unwrap()["markers"].as_array_mut().unwrap().push(Value::Object(marker));
            }
        }
    }
    if !header.contains_key("version") || kits.is_empty() {
        return Err("Panels file needs Version and Chemistry Kit rows".into());
    }
    let mut out = Map::new();
    out.insert("format".into(), json!("genemapper_panels"));
    for (k, v) in header {
        out.insert(k, v);
    }
    out.insert("kits".into(), Value::Array(kits.into_iter().map(Value::Object).collect()));
    Ok(Value::Object(out))
}

fn blocks(
    text: &Value,
    what: &str,
    value_row: &dyn Fn(usize, &[String]) -> Res<Value>,
) -> Res<(Map<String, Value>, Map<String, Value>)> {
    let rows = lines(text, what)?;
    let mut header = Map::new();
    let mut panels: Map<String, Value> = Map::new();
    let mut panel: Option<String> = None;
    let mut marker: Option<String> = None;
    for (number, f) in rows {
        match f[0].as_str() {
            "Version" => {
                header.insert("version".into(), json!(opt_field(&f, 1)));
            }
            "Chemistry Kit" => {
                header.insert("kit".into(), json!(opt_field(&f, 1)));
            }
            "BinSet Name" => {
                header.insert("bin_set".into(), json!(opt_field(&f, 1)));
            }
            "Panel Name" => {
                let name = field(&f, 1)?.clone();
                panels.entry(name.clone()).or_insert_with(|| json!({}));
                panel = Some(name);
                marker = None;
            }
            "Marker Name" => {
                let p = match &panel {
                    Some(p) => p.clone(),
                    None => return Err(format!("line {}: Marker Name before Panel Name", number)),
                };
                let name = field(&f, 1)?.clone();
                let m = panels.get_mut(&p).unwrap().as_object_mut().unwrap();
                m.entry(name.clone()).or_insert_with(|| json!([]));
                marker = Some(name);
            }
            _ => {
                let (p, m) = match (&panel, &marker) {
                    (Some(p), Some(m)) => (p, m),
                    _ => return Err(format!("line {}: value row before Marker Name", number)),
                };
                let row = value_row(number, &f)?;
                panels.get_mut(p).unwrap()[m.as_str()].as_array_mut().unwrap().push(row);
            }
        }
    }
    if !header.contains_key("version") || panels.is_empty() {
        return Err(format!("{} file needs Version and Panel Name rows", what));
    }
    Ok((header, panels))
}

fn assemble(format: &str, header: Map<String, Value>, panels: Map<String, Value>) -> Value {
    let mut out = Map::new();
    out.insert("format".into(), json!(format));
    for (k, v) in header {
        out.insert(k, v);
    }
    out.insert("panels".into(), Value::Object(panels));
    Value::Object(out)
}

pub fn parse_bins(text: &Value) -> Res<Value> {
    let row = |number: usize, f: &[String]| -> Res<Value> {
        if f.len() < 4 {
            return Err(format!("line {}: bin row needs label, center, left, right", number));
        }
        let wh = format!("line {} ({})", number, f[0]);
        let flag = if f.len() > 4 { f[4].to_lowercase() } else { String::new() };
        if flag != "" && flag != "virtual" {
            return Err(format!("{}: unknown bin flag {}", wh, py_repr(&Value::String(f[4].clone()))));
        }
        let center = float(&f[1], &wh)?;
        let left = float(&f[2], &wh)?;
        let right = float(&f[3], &wh)?;
        Ok(json!({"label": f[0], "center_bp": fv(center), "left_bp": fv(left), "right_bp": fv(right), "virtual": flag == "virtual"}))
    };
    let (header, panels) = blocks(text, "Bins", &row)?;
    Ok(assemble("genemapper_bins", header, panels))
}

pub fn parse_stutter(text: &Value) -> Res<Value> {
    let row = |number: usize, f: &[String]| -> Res<Value> {
        if f.len() < 4 {
            return Err(format!("line {}: stutter row needs ratio, from, to, direction", number));
        }
        let wh = format!("line {}", number);
        let direction = strip(&f[3]).to_lowercase();
        if direction != "minus" && direction != "plus" {
            return Err(format!("{}: direction must be Minus or Plus", wh));
        }
        let ratio = float(&f[0], &wh)?;
        let low = float(&f[1], &wh)?;
        let high = float(&f[2], &wh)?;
        if !(0.0 <= ratio && ratio <= 1.0) || low > high {
            return Err(format!("{}: ratio in [0,1] and from <= to required", wh));
        }
        Ok(json!({"ratio": fv(ratio), "min_bp": fv(low), "max_bp": fv(high), "direction": direction}))
    };
    let (header, panels) = blocks(text, "Stutter", &row)?;
    Ok(assemble("genemapper_stutter", header, panels))
}

pub fn list_panels(doc: &Value) -> Value {
    let mut out = Vec::new();
    for kit in doc["kits"].as_array().unwrap() {
        for panel in kit["panels"].as_array().unwrap() {
            out.push(json!({"kit": kit["name"], "panel": panel["name"], "markers": panel["markers"].as_array().unwrap().len()}));
        }
    }
    Value::Array(out)
}

fn default_method() -> Map<String, Value> {
    let v = json!({
        "use_marker_specific_stutter": true,
        "cutoff_ratio": 0.0,
        "ladder_cutoff_ratio": 0.0,
        "minus_a_ratio": 0.0, "minus_a_min_bp": 0.75, "minus_a_max_bp": 1.25,
        "global_minus_stutter": {"ratio": 0.0, "min_bp": 3.25, "max_bp": 4.75},
        "global_plus_stutter": {"ratio": 0.0, "min_bp": 3.25, "max_bp": 4.75},
    });
    match v {
        Value::Object(m) => m,
        _ => unreachable!(),
    }
}

fn method_merge(method: Option<&Value>) -> Res<Map<String, Value>> {
    let mut result = default_method();
    let m = match method {
        None | Some(Value::Null) => return Ok(result),
        Some(v) if !truthy(v) => return Ok(result),
        Some(Value::Object(m)) => m,
        Some(v) => return Err(format!("'{}' object has no attribute 'items'", type_name(v))),
    };
    for (key, value) in m {
        let cur = match result.get(key) {
            Some(c) => c.clone(),
            None => return Err(format!("Unknown method field: {}", key)),
        };
        let new = match value {
            Value::Object(vm) => match &cur {
                Value::Object(cm) => {
                    let mut merged = cm.clone();
                    for (k, x) in vm {
                        merged.insert(k.clone(), x.clone());
                    }
                    Value::Object(merged)
                }
                other => return Err(format!("'{}' object is not a mapping", type_name(other))),
            },
            other => other.clone(),
        };
        result.insert(key.clone(), new);
    }
    Ok(result)
}

/// Python `g[key]` on a method value (global stutter dict that a caller may have replaced).
fn subscript(g: &Value, key: &str) -> Res<Value> {
    match g {
        Value::Object(m) => m.get(key).cloned().ok_or_else(|| format!("'{}'", key)),
        Value::String(_) => Err("string indices must be integers, not 'str'".into()),
        Value::Array(_) => Err("list indices must be integers or slices, not str".into()),
        other => Err(format!("'{}' object is not subscriptable", type_name(other))),
    }
}

/// Python `value > 0` for a JSON value.
fn gt_zero(v: &Value) -> Res<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => Ok(i > 0),
            None => Ok(n.as_f64().unwrap() > 0.0),
        },
        other => Err(format!("'>' not supported between instances of '{}' and 'int'", type_name(other))),
    }
}

fn default_channels() -> Map<String, Value> {
    let mut m = Map::new();
    for (k, v) in [("blue", "DATA1"), ("green", "DATA2"), ("yellow", "DATA3"), ("red", "DATA4"), ("orange", "DATA105"), ("purple", "DATA106")] {
        m.insert(k.into(), json!(v));
    }
    m
}

fn str_of(v: &Value) -> &str {
    v.as_str().unwrap()
}

pub fn build_kit(
    panels_doc: &Value,
    bins_doc: &Value,
    stutter_doc: Option<&Value>,
    panel: Option<&Value>,
    method: Option<&Value>,
    dye_channels: Option<&Value>,
    analysis_method: Option<&Value>,
) -> Res<Value> {
    let method = method.filter(|v| !v.is_null());
    let analysis_method = analysis_method.filter(|v| !v.is_null());
    if analysis_method.is_some() && method.is_some() {
        return Err("Give either method or analysis_method, not both".into());
    }
    let am = match analysis_method {
        Some(a) => Some(load_analysis_method(a)?),
        None => None,
    };
    let mut choices: Vec<(&Value, &Value)> = Vec::new();
    for kit in panels_doc["kits"].as_array().unwrap() {
        for p in kit["panels"].as_array().unwrap() {
            choices.push((kit, p));
        }
    }
    let (kit, chosen) = match panel.filter(|v| !v.is_null()) {
        None => {
            if choices.len() != 1 {
                let names: Vec<&str> = choices.iter().map(|(_, p)| str_of(&p["name"])).collect();
                return Err(format!("Several panels are present; choose one: {}", names.join(", ")));
            }
            choices[0]
        }
        Some(pv) => match choices.iter().find(|(_, p)| p["name"] == *pv) {
            Some(c) => *c,
            None => return Err(format!("Panel {} is not in the Panels file", py_repr(pv))),
        },
    };
    let chosen_name = str_of(&chosen["name"]).to_string();
    let bins = match bins_doc["panels"].get(&chosen_name) {
        Some(b) => b,
        None => return Err(format!("Bins file has no bin set for panel {}", py_repr(&Value::String(chosen_name.clone())))),
    };
    let empty = json!({});
    let stutter = stutter_doc.and_then(|d| d["panels"].get(&chosen_name)).unwrap_or(&empty);
    let mut m = method_merge(method)?;
    let mut channels = default_channels();
    if let Some(dc) = dye_channels {
        if truthy(dc) {
            match dc {
                Value::Object(d) => {
                    for (k, v) in d {
                        channels.insert(k.clone(), v.clone());
                    }
                }
                other => return Err(format!("'{}' object is not a mapping", type_name(other))),
            }
        }
    }
    let mut markers: Vec<Value> = Vec::new();
    let mut notes: Vec<Value> = Vec::new();
    for row in chosen["markers"].as_array().unwrap() {
        let name = str_of(&row["name"]);
        let dye = str_of(&row["dye"]);
        if !channels.contains_key(dye) {
            return Err(format!("{}: no channel for dye {}", name, py_repr(&Value::String(dye.to_string()))));
        }
        let marker_bins = match bins.get(name) {
            Some(Value::Array(a)) if !a.is_empty() => a,
            _ => return Err(format!("{}: no bins in the bin set", name)),
        };
        let low = row["min_bp"].as_f64().unwrap();
        let high = row["max_bp"].as_f64().unwrap();
        let inside = |b: &Value| {
            let c = b["center_bp"].as_f64().unwrap();
            low <= c && c <= high
        };
        let outside: Vec<Value> = marker_bins.iter().filter(|b| !inside(b)).map(|b| b["label"].clone()).collect();
        if !outside.is_empty() {
            notes.push(json!({"code": "bins_outside_marker_range_excluded", "marker": name, "labels": outside}));
        }
        let kit_bins: Vec<Value> = marker_bins
            .iter()
            .filter(|b| inside(b))
            .map(|b| json!({"label": b["label"], "center_bp": b["center_bp"], "left_bp": b["left_bp"], "right_bp": b["right_bp"], "virtual": b["virtual"]}))
            .collect();
        let known: std::collections::HashSet<String> = kit_bins.iter().map(|b| casefold(str_of(&b["label"]))).collect();
        let ladder_alleles = row["ladder_alleles"].as_array().unwrap();
        let ladder: Vec<Value> = ladder_alleles.iter().filter(|l| known.contains(&casefold(str_of(l)))).cloned().collect();
        let missing: Vec<Value> = ladder_alleles.iter().filter(|l| !known.contains(&casefold(str_of(l)))).cloned().collect();
        if !missing.is_empty() {
            notes.push(json!({"code": "ladder_alleles_without_bins", "marker": name, "labels": missing}));
        }
        if let Some(am) = &am {
            let kp = kit_marker_parameters(am, row["repeat"].as_i64().unwrap(), dye);
            let mut nm = kp.as_object().unwrap().clone();
            nm.insert("use_marker_specific_stutter".into(), am["allele"]["use_marker_specific_stutter"].clone());
            m = nm;
        }
        let use_specific = truthy(&m["use_marker_specific_stutter"]);
        let specific: Vec<Value> = if use_specific {
            stutter.get(name).and_then(|v| v.as_array()).cloned().unwrap_or_default()
        } else {
            vec![]
        };
        let pick = |dir: &str| -> Vec<Value> {
            specific
                .iter()
                .filter(|s| s["direction"] == dir)
                .map(|s| json!({"min_bp": s["min_bp"], "max_bp": s["max_bp"], "ratio": s["ratio"]}))
                .collect()
        };
        let mut minus = pick("minus");
        let mut plus = pick("plus");
        if specific.is_empty() {
            let g = &m["global_minus_stutter"];
            minus = vec![json!({"min_bp": subscript(g, "min_bp")?, "max_bp": subscript(g, "max_bp")?, "ratio": subscript(g, "ratio")?})];
            let g = &m["global_plus_stutter"];
            plus = vec![json!({"min_bp": subscript(g, "min_bp")?, "max_bp": subscript(g, "max_bp")?, "ratio": subscript(g, "ratio")?})];
        }
        let mut backward = Vec::new();
        for r in &minus {
            if gt_zero(&r["ratio"])? {
                backward.push(r.clone());
            }
        }
        let mut forward = Vec::new();
        for r in &plus {
            if gt_zero(&r["ratio"])? {
                forward.push(r.clone());
            }
        }
        let mut params = Map::new();
        params.insert("ladder_cutoff_ratio".into(), m["ladder_cutoff_ratio"].clone());
        params.insert("cutoff_ratio".into(), m["cutoff_ratio"].clone());
        params.insert("minus_a_ratio".into(), m["minus_a_ratio"].clone());
        params.insert("minus_a_min_bp".into(), m["minus_a_min_bp"].clone());
        params.insert("minus_a_max_bp".into(), m["minus_a_max_bp"].clone());
        params.insert("backward_stutter_ranges".into(), Value::Array(backward));
        params.insert("forward_stutter_ranges".into(), Value::Array(forward));
        if let Some(cr) = m.get("clear_ranges") {
            if truthy(cr) {
                params.insert("clear_ranges".into(), cr.clone());
            }
        }
        let mut marker = Map::new();
        marker.insert("name".into(), json!(name));
        marker.insert("channel".into(), channels[dye].clone());
        marker.insert("dye".into(), json!(dye));
        marker.insert("min_bp".into(), row["min_bp"].clone());
        marker.insert("max_bp".into(), row["max_bp"].clone());
        marker.insert("repeat".into(), row["repeat"].clone());
        marker.insert("bins".into(), Value::Array(kit_bins));
        marker.insert("ladder_labels".into(), Value::Array(ladder));
        marker.insert("control_alleles".into(), row["control_alleles"].clone());
        marker.insert("y_marker".into(), row["y_marker"].clone());
        if let Some(q) = row.get("internal_qc") {
            marker.insert("internal_qc".into(), q.clone());
        }
        marker.insert("parameters".into(), Value::Object(params));
        markers.push(Value::Object(marker));
    }
    let mut kit_def = load_kit_definition(&json!({"schema_version": 1, "name": chosen_name, "markers": markers}))?;
    let mut diag = kit_def["diagnostics"].as_array().cloned().unwrap_or_default();
    diag.extend(notes);
    let stutter_version = stutter_doc.and_then(|d| d.get("version")).cloned().unwrap_or(Value::Null);
    diag.push(json!({
        "code": "imported_from_genemapper_text",
        "chemistry_kit": kit["name"],
        "panel": chosen_name,
        "bin_set": bins_doc.get("bin_set").cloned().unwrap_or(Value::Null),
        "panels_version": panels_doc.get("version").cloned().unwrap_or(Value::Null),
        "bins_version": bins_doc.get("version").cloned().unwrap_or(Value::Null),
        "stutter_version": stutter_version,
        "dye_channels": Value::Object(channels),
        "method": if am.is_none() { Value::Object(m) } else { Value::Null },
        "analysis_method": am.as_ref().map(|a| a["name"].clone()).unwrap_or(Value::Null),
    }));
    kit_def["diagnostics"] = Value::Array(diag);
    Ok(kit_def)
}

pub fn import_kit(
    panels_text: &Value,
    bins_text: &Value,
    stutter_text: Option<&Value>,
    panel: Option<&Value>,
    method: Option<&Value>,
    dye_channels: Option<&Value>,
    analysis_method: Option<&Value>,
) -> Res<Value> {
    let panels = parse_panels(panels_text)?;
    let bins = parse_bins(bins_text)?;
    let stutter = match stutter_text {
        Some(t) if truthy(t) => Some(parse_stutter(t)?),
        _ => None,
    };
    build_kit(&panels, &bins, stutter.as_ref(), panel, method, dye_channels, analysis_method)
}

pub fn default_ladder_quality_definition(kit: &Value) -> Value {
    let recipe = json!({
        "min_height_ratio": 0.15, "spike_metric": 0, "spike_weight": 0.5, "offscale_metric": 0,
        "offscale_weight": 0.5,
        "spacing": {
            "enabled": true, "max_distance_bp": 49,
            "curve": [{"distance_bp": 0, "fraction": 0.25, "interpolate": false}],
            "exceptions": [], "excluded_alleles": [],
        },
    });
    let mut markers = Map::new();
    for m in kit["markers"].as_array().unwrap() {
        if m["ladder_labels"].as_array().map(|a| !a.is_empty()).unwrap_or(false) {
            markers.insert(str_of(&m["name"]).to_string(), recipe.clone());
        }
    }
    json!({
        "schema_version": 1, "calling_parameters_source": "ladder_matching_calls",
        "ladder_sq": {"source": "base_quality"}, "sq_thresholds": {"good": 0.75, "bad": 0.25},
        "gq_thresholds": {"good": 0.75, "bad": 0.25},
        "markers": Value::Object(markers),
    })
}

pub fn import_request(request: &Value) -> Res<Value> {
    let req = request.as_object().ok_or("Kit import request must be an object")?;
    let get = |k: &str| req.get(k).cloned().unwrap_or(Value::Null);
    let mut out = Map::new();
    for k in ["panels", "bins", "stutter", "panel_list", "kit", "ladder_quality_definition", "marker_quality_definition"] {
        out.insert(k.into(), if k == "panel_list" { json!([]) } else { Value::Null });
    }
    let (tp, tb, ts) = (get("panels"), get("bins"), get("stutter"));
    if truthy(&tp) {
        let p = parse_panels(&tp)?;
        out.insert("panel_list".into(), list_panels(&p));
        out.insert("panels".into(), p);
    }
    if truthy(&tb) {
        out.insert("bins".into(), parse_bins(&tb)?);
    }
    if truthy(&ts) {
        out.insert("stutter".into(), parse_stutter(&ts)?);
    }
    let panel = get("panel");
    if !panel.is_null() {
        if out["panels"].is_null() || out["bins"].is_null() || !truthy(&out["panels"]) || !truthy(&out["bins"]) {
            return Err("Panels and Bins files are both required to build a kit".into());
        }
        let am = get("analysis_method");
        let stutter = if out["stutter"].is_null() { None } else { Some(&out["stutter"]) };
        let kit = build_kit(&out["panels"], &out["bins"], stutter, Some(&panel), req.get("method"), req.get("dye_channels"), Some(&am))?;
        if !am.is_null() {
            let am = load_analysis_method(&am)?;
            out.insert("ladder_quality_definition".into(), ladder_quality_definition(&am, &kit));
            out.insert("marker_quality_definition".into(), marker_quality_definition(&am, &kit));
        } else {
            out.insert("ladder_quality_definition".into(), default_ladder_quality_definition(&kit));
        }
        out.insert("kit".into(), kit);
    }
    Ok(Value::Object(out))
}

pub(crate) fn splitlines_pub(text: &str) -> Vec<&str> {
    splitlines(text)
}

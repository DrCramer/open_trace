//! Port of `genemapper_open/ladder_quality.py`.
use crate::pullup::{py_abs, py_sub};
use crate::quality_control::{boolean, number};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub type Res<T> = Result<T, String>;

fn fv(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

fn object<'a>(value: &'a Value, keys: &[&str], name: &str) -> Res<&'a Map<String, Value>> {
    match value {
        Value::Object(m) if m.len() == keys.len() && keys.iter().all(|k| m.contains_key(*k)) => Ok(m),
        _ => Err(format!("{}: required fields {}", name, keys.join(", "))),
    }
}

fn name_of(value: &Value) -> Res<String> {
    match value {
        Value::String(s) if (1..=128).contains(&s.chars().count()) && s.is_ascii() => Ok(s.clone()),
        _ => Err("Bounded ASCII allele/bin name required".into()),
    }
}

fn trim(s: &str) -> &str {
    s.trim_matches(|c: char| (c as u32) <= 32)
}

fn state(value: &Value) -> Res<()> {
    let v = number(value, "metric state", -2.0, 1.0)?;
    if ![-2.0, -1.0, 0.0, 1.0].contains(&v) {
        return Err("Explicit metric state -2/-1/0/1 required".into());
    }
    Ok(())
}

pub fn validate_ladder_recipe(recipe: &Value) -> Res<Value> {
    let r = object(
        recipe,
        &["min_height_ratio", "spike_metric", "spike_weight", "offscale_metric", "offscale_weight", "spacing"],
        "ladder recipe",
    )?;
    number(&r["min_height_ratio"], "min_height_ratio", 0.0, 1.0)?;
    for key in ["spike", "offscale"] {
        state(&r[&format!("{}_metric", key)])?;
        number(&r[&format!("{}_weight", key)], &format!("{}_weight", key), 0.0, 1.0)?;
    }
    let spacing = object(&r["spacing"], &["enabled", "max_distance_bp", "curve", "exceptions", "excluded_alleles"], "spacing")?;
    boolean(&spacing["enabled"], "spacing.enabled")?;
    number(&spacing["max_distance_bp"], "max_distance_bp", 0.0, 1e6)?;
    let curve = match &spacing["curve"] {
        Value::Array(c) if c.len() <= 128 => c,
        _ => return Err("At most128 explicit curve nodes required".into()),
    };
    let mut last = -1.0;
    for node in curve {
        let n = object(node, &["distance_bp", "fraction", "interpolate"], "curve node")?;
        let x = number(&n["distance_bp"], "distance_bp", 0.0, 1e6)?;
        number(&n["fraction"], "fraction", -1e6, 1e6)?;
        boolean(&n["interpolate"], "interpolate")?;
        if x <= last {
            return Err("Explicit curve distances must strictly increase".into());
        }
        last = x;
    }
    let pairs = match &spacing["exceptions"] {
        Value::Array(p) if p.len() <= 4096 => p,
        _ => return Err("At most4096 explicit pair exceptions required".into()),
    };
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for pair in pairs {
        let p = object(pair, &["first", "second", "tolerance_bp"], "pair exception")?;
        let first = trim(&name_of(&p["first"])?).to_string();
        let second = trim(&name_of(&p["second"])?).to_string();
        if first.is_empty() || second.is_empty() || seen.contains(&(first.clone(), second.clone())) {
            return Err("Nonempty unique directed exception pairs required".into());
        }
        seen.insert((first, second));
        number(&p["tolerance_bp"], "tolerance_bp", -1e6, 1e6)?;
    }
    match &spacing["excluded_alleles"] {
        Value::Array(e) if e.len() <= 128 => {
            for n in e {
                name_of(n)?;
            }
        }
        _ => return Err("At most128 explicitly excluded alleles required".into()),
    }
    Ok(recipe.clone())
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

/// Python `_tolerance`: (tolerance or None, source).
fn tolerance(distance: &Value, first: &str, second: &str, spacing: &Value) -> (Option<f64>, &'static str) {
    let d = f(distance);
    if d > f(&spacing["max_distance_bp"]) {
        return (None, "distance_above_maximum");
    }
    let (first, second) = (trim(first), trim(second));
    let exceptions = spacing["exceptions"].as_array().unwrap();
    let lookup = |a: &str, b: &str| {
        exceptions
            .iter()
            .filter(|p| trim(p["first"].as_str().unwrap()) == a && trim(p["second"].as_str().unwrap()) == b)
            .last()
            .map(|p| f(&p["tolerance_bp"]))
    };
    let explicit = lookup(first, second).or_else(|| lookup(second, first));
    if let Some(e) = explicit {
        if e >= 0.0 {
            return (Some(e), "pair_exception");
        }
    }
    let index = d.trunc();
    let curve = spacing["curve"].as_array().unwrap();
    if curve.is_empty() {
        return (None, "empty_curve");
    }
    let fraction = if index <= f(&curve[0]["distance_bp"]) {
        f(&curve[0]["fraction"])
    } else if index >= f(&curve[curve.len() - 1]["distance_bp"]) {
        f(&curve[curve.len() - 1]["fraction"])
    } else {
        let mut fraction = f64::NAN;
        for w in curve.windows(2) {
            let (left, right) = (&w[0], &w[1]);
            if f(&left["distance_bp"]) <= index && index < f(&right["distance_bp"]) {
                fraction = f(&left["fraction"]);
                if left["interpolate"] == json!(true) {
                    fraction = f(&right["fraction"])
                        + (index - f(&right["distance_bp"])) * (f(&left["fraction"]) - f(&right["fraction"]))
                            / (f(&left["distance_bp"]) - f(&right["distance_bp"]));
                }
                break;
            }
        }
        fraction
    };
    let value = fraction * (if index != 0.0 { index } else { 1.0 });
    if value < 0.0 {
        (None, "negative_lookup_disabled")
    } else {
        (Some(value), "integer_distance_curve")
    }
}

pub fn ladder_spacing_tolerance(distance: &Value, first: &Value, second: &Value, recipe: &Value) -> Res<Value> {
    let recipe = validate_ladder_recipe(recipe)?;
    number(distance, "bin distance", 0.0, 1e6)?;
    let a = name_of(first)?;
    let b = name_of(second)?;
    let (value, source) = tolerance(distance, &a, &b, &recipe["spacing"]);
    Ok(json!({"tolerance_bp": value.map_or(Value::Null, fv), "source": source}))
}

pub fn score_ladder_marker(calls: &Value, expected_bins: &Value, recipe: &Value) -> Res<Value> {
    let recipe = validate_ladder_recipe(recipe)?;
    let expected: Vec<String> = match expected_bins {
        Value::Array(e) if e.len() <= 128 => e.iter().map(name_of).collect::<Res<Vec<_>>>()?,
        _ => return Err("At most128 explicit expected bin names required".into()),
    };
    let list = match calls {
        Value::Array(c) if c.len() <= 4096 => c,
        _ => return Err("At most4096 explicit calls required".into()),
    };
    let mut prepared: Vec<&Map<String, Value>> = Vec::new();
    for call in list {
        let c = object(call, &["name", "size_bp", "peak_bp", "height", "bin", "off_ladder"], "ladder call")?;
        name_of(&c["name"])?;
        number(&c["size_bp"], "allele size", 0.0, 1e6)?;
        number(&c["peak_bp"], "peak bp", 0.0, 1e6)?;
        number(&c["height"], "height", 1e-100, 1e12)?;
        boolean(&c["off_ladder"], "off_ladder")?;
        if !c["bin"].is_null() {
            let b = object(&c["bin"], &["name", "center_bp"], "bin")?;
            name_of(&b["name"])?;
            number(&b["center_bp"], "bin center", 0.0, 1e6)?;
        }
        prepared.push(c);
    }
    prepared.sort_by(|a, b| f(&a["size_bp"]).partial_cmp(&f(&b["size_bp"])).unwrap());
    let mut result = Map::new();
    result.insert("status".into(), json!("evaluated_subset"));
    result.insert("gq".into(), fv(1.0));
    result.insert("ladder_metric".into(), json!(0));
    result.insert("allele_number_metric".into(), json!(-2));
    result.insert("recipe".into(), recipe.clone());
    result.insert("events".into(), json!([]));
    result.insert("spacing_checks".into(), json!([]));
    result.insert("sorted_call_names".into(), Value::Array(prepared.iter().map(|c| c["name"].clone()).collect()));
    result.insert("first_two_height_ratio".into(), Value::Null);
    result.insert("min_max_height_ratio".into(), Value::Null);
    result.insert("missing_expected_bins".into(), json!([]));
    result.insert("scope".into(), json!("fresh_finite_positive_peaks_without_overrides_or_original_rounding"));
    result.insert("external_validation".into(), json!(false));
    result.insert("original_runtime_validated".into(), json!(false));
    let mut events: Vec<Value> = Vec::new();
    let mut failed = false;
    let fail = |events: &mut Vec<Value>, failed: &mut bool, reason: &str, details: Vec<(&str, Value)>| {
        *failed = true;
        let mut e = Map::new();
        e.insert("criterion".into(), json!(reason));
        for (k, v) in details {
            e.insert(k.into(), v);
        }
        events.push(Value::Object(e));
    };
    if prepared.is_empty() {
        fail(&mut events, &mut failed, "no_called_alleles", vec![]);
    }
    for key in ["spike", "offscale"] {
        if f(&recipe[&format!("{}_weight", key)]) > 0.0 && f(&recipe[&format!("{}_metric", key)]) == 1.0 {
            fail(&mut events, &mut failed, &format!("{}_gate", key), vec![]);
        }
    }
    let names: HashSet<&str> = prepared.iter().filter(|c| !c["bin"].is_null()).map(|c| c["bin"]["name"].as_str().unwrap()).collect();
    let missing: Vec<Value> = expected.iter().filter(|n| !names.contains(n.as_str())).map(|n| json!(n)).collect();
    result.insert("missing_expected_bins".into(), Value::Array(missing.clone()));
    if !missing.is_empty() {
        fail(&mut events, &mut failed, "expected_bin_missing", vec![("bins", Value::Array(missing))]);
    }
    if prepared.len() > 1 {
        let ratio = f(&prepared[0]["height"]) / f(&prepared[1]["height"]);
        result.insert("first_two_height_ratio".into(), fv(ratio));
        if ratio <= 0.5 {
            fail(&mut events, &mut failed, "first_two_height_ratio", vec![("observed", fv(ratio)), ("threshold", fv(0.5))]);
        }
        let heights: Vec<f64> = prepared.iter().map(|c| f(&c["height"])).collect();
        let lo = heights.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = heights.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let ratio = lo / hi;
        result.insert("min_max_height_ratio".into(), fv(ratio));
        if ratio <= f(&recipe["min_height_ratio"]) {
            fail(&mut events, &mut failed, "min_max_height_ratio", vec![("observed", fv(ratio)), ("threshold", recipe["min_height_ratio"].clone())]);
        }
    }
    let spacing = &recipe["spacing"];
    let enabled = spacing["enabled"] == json!(true) && !spacing["curve"].as_array().unwrap().is_empty();
    result.insert("spacing_enabled".into(), json!(enabled));
    let excluded: HashSet<String> = spacing["excluded_alleles"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_lowercase()).collect();
    let mut checks: Vec<Value> = Vec::new();
    if enabled {
        for index in 0..prepared.len().saturating_sub(1) {
            let first = prepared[index];
            if first["off_ladder"] == json!(true) || excluded.contains(&first["name"].as_str().unwrap().to_lowercase()) {
                continue;
            }
            let mut second = prepared[index + 1];
            for cand in &prepared[index + 1..] {
                second = cand;
                if cand["off_ladder"] != json!(true) {
                    break;
                }
            }
            let mut obs = Map::new();
            obs.insert("first".into(), first["name"].clone());
            obs.insert("second".into(), second["name"].clone());
            if first["bin"].is_null() || second["bin"].is_null() {
                obs.insert("status".into(), json!("failed"));
                obs.insert("reason".into(), json!("pair_bin_missing"));
                checks.push(Value::Object(obs));
                fail(&mut events, &mut failed, "pair_bin_missing", vec![("first", first["name"].clone()), ("second", second["name"].clone())]);
                break;
            }
            let observed = py_abs(py_sub(&second["peak_bp"], &first["peak_bp"])?);
            let distance = py_abs(py_sub(&second["bin"]["center_bp"], &first["bin"]["center_bp"])?);
            let (tol, source) = tolerance(&distance, first["name"].as_str().unwrap(), second["name"].as_str().unwrap(), spacing);
            let error = py_abs(py_sub(&observed, &distance)?);
            obs.insert("observed_bp".into(), observed);
            obs.insert("bin_distance_bp".into(), distance);
            obs.insert("error_bp".into(), error.clone());
            obs.insert("tolerance_bp".into(), tol.map_or(Value::Null, fv));
            obs.insert("tolerance_source".into(), json!(source));
            obs.insert("status".into(), json!(if tol.is_none() { "disabled" } else { "passed" }));
            let breach = tol.map_or(false, |t| f(&error) > t);
            if breach {
                obs.insert("status".into(), json!("failed"));
                let details: Vec<(&str, Value)> = obs.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
                fail(&mut events, &mut failed, "bin_spacing", details);
            }
            checks.push(Value::Object(obs));
            if breach {
                break;
            }
        }
    }
    result.insert("spacing_checks".into(), Value::Array(checks));
    result.insert("events".into(), Value::Array(events));
    if failed {
        result.insert("ladder_metric".into(), json!(1));
        result.insert("gq".into(), fv(0.0));
    }
    Ok(Value::Object(result))
}

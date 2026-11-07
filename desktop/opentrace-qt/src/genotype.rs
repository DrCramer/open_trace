//! Genotyping of a project (port of the web project window: `groups`, `judgeLadders`, `genotype`, `samplePqv`).
//! The calls themselves come from the core (`/api/ladder-set`, `/api/str-run`, `/api/controls` called in this
//! process); this module only decides the order, the inputs and what is kept, exactly as the web UI does, so both
//! interfaces give the same genotypes for the same files and settings.
use crate::backend::{Inner, Sample, Status};
use opentrace_core::server;
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Condvar, Mutex};

/// Project options that change genotyping (web `options.js`, same defaults).
#[derive(Clone)]
pub struct Options {
    /// Allelic Ladder Analysis Summary: "stop" (ask), "passing", "all"
    pub alas: String,
    /// bin offsets from ladders with green ("green") or green/yellow ("greenYellow") SQ
    pub bin_offsets: String,
    pub pullup_excludes_calls: bool,
    pub homo_min_height: f64,
    pub het_min_height: f64,
}

impl Default for Options {
    fn default() -> Self {
        Options { alas: "stop".into(), bin_offsets: "green".into(), pullup_excludes_calls: false, homo_min_height: 200.0, het_min_height: 100.0 }
    }
}

/// A question from the genotyping thread to the window (Allelic Ladder Analysis Summary) and its answer.
#[derive(Default)]
pub struct Ask {
    state: Mutex<(Option<String>, Option<String>)>,
    cv: Condvar,
}

impl Ask {
    /// Blocks the worker until the window answers (or the analysis is stopped).
    fn ask(&self, question: String, stop: &std::sync::atomic::AtomicBool) -> Option<String> {
        let mut st = self.state.lock().unwrap();
        *st = (Some(question), None);
        loop {
            if let Some(a) = st.1.take() {
                st.0 = None;
                return Some(a);
            }
            if stop.load(Ordering::SeqCst) {
                st.0 = None;
                return None;
            }
            st = self.cv.wait_timeout(st, std::time::Duration::from_millis(200)).unwrap().0;
        }
    }
    pub fn pending(&self) -> Option<String> {
        self.state.lock().unwrap().0.clone()
    }
    pub fn answer(&self, a: &str) {
        let mut st = self.state.lock().unwrap();
        if st.0.is_some() {
            st.1 = Some(a.to_string());
            self.cv.notify_all();
        }
    }
}

pub struct KitEntry {
    pub kit: Value,
    pub lq: Value,
    pub mq: Value,
    pub policy: String,
    pub bin_set: String,
    pub bin_set_substituted: bool,
}

// ------------------------------------------------------------------ sample type (web `sampleTypeFromName`)
fn is_letter(c: char) -> bool {
    c.is_alphabetic()
}

/// sample | ladder | negative | positive from a file or sample name, with the web UI's rules.
pub fn sample_type_from_name(n: &str) -> &'static str {
    let lower_n = n.to_lowercase();
    let base: &str = {
        let l = lower_n.as_str();
        if l.ends_with(".fsa") || l.ends_with(".ab1") || l.ends_with(".hid") { &lower_n[..lower_n.len() - 4] } else { l }
    };
    let base = base.to_string();
    let orig_base: String = {
        let l = n.to_lowercase();
        if l.ends_with(".fsa") || l.ends_with(".ab1") || l.ends_with(".hid") { n[..n.len() - 4].to_string() } else { n.to_string() }
    };
    for w in ["ladder", "allelic", "лестниц", "ладдер"] {
        if base.contains(w) {
            return "ladder";
        }
    }
    // word starting with neg / ntc / blank (not preceded by a Latin letter)
    let starts_word = |w: &str, latin: bool| -> bool {
        let chars: Vec<char> = base.chars().collect();
        let wc: Vec<char> = w.chars().collect();
        for i in 0..chars.len() {
            if chars[i..].starts_with(&wc) {
                let prev_ok = i == 0 || if latin { !chars[i - 1].is_ascii_lowercase() } else { !(('а'..='я').contains(&chars[i - 1]) || chars[i - 1] == 'ё') };
                if prev_ok {
                    return true;
                }
            }
        }
        false
    };
    if starts_word("neg", true) || starts_word("ntc", true) || starts_word("blank", true) || starts_word("отриц", false) || base.contains("нег") {
        return "negative";
    }
    if starts_word("pos", true) || starts_word("control", true) || base.contains("9947") || base.contains("2800m") || starts_word("полож", false) {
        return "positive";
    }
    // tokens and their letter+digit parts
    let toks: Vec<String> = orig_base.split(|c: char| !(c.is_ascii_alphanumeric() || ('А'..='я').contains(&c) || c == 'Ё' || c == 'ё')).filter(|t| !t.is_empty()).map(String::from).collect();
    let mut parts: Vec<String> = Vec::new();
    for t in &toks {
        parts.push(t.clone());
        // letter runs followed by digits ("01PC" -> "PC")
        let cs: Vec<char> = t.chars().collect();
        let mut i = 0;
        while i < cs.len() {
            if is_letter(cs[i]) {
                let mut j = i;
                while j < cs.len() && is_letter(cs[j]) {
                    j += 1;
                }
                while j < cs.len() && cs[j].is_ascii_digit() {
                    j += 1;
                }
                let m: String = cs[i..j].iter().collect();
                if m != *t {
                    parts.push(m);
                }
                i = j;
            } else {
                i += 1;
            }
        }
    }
    // "K+" / "К+" and "+C"
    let ob: Vec<char> = orig_base.chars().collect();
    let sep = |c: char| matches!(c, '_' | '-' | ' ' | '.') || c.is_whitespace();
    for i in 0..ob.len() {
        if matches!(ob[i], 'K' | 'k' | 'К' | 'к') && ob.get(i + 1) == Some(&'+') && (i == 0 || sep(ob[i - 1])) {
            return "positive";
        }
        if ob[i] == '+' && matches!(ob.get(i + 1), Some('C') | Some('c')) && (i == 0 || sep(ob[i - 1])) && (i + 2 >= ob.len() || sep(ob[i + 2])) {
            return "positive";
        }
    }
    if orig_base.starts_with("007") && orig_base.chars().nth(3).map_or(true, |c| !c.is_ascii_digit()) {
        return "positive";
    }
    let strip_digits = |t: &str| -> String { t.trim_end_matches(|c: char| c.is_ascii_digit()).to_lowercase() };
    let ladder_tok = |t: &str| {
        let w = strip_digits(t);
        ["al", "l", "lad", "ladder", "лестница", "ладдер", "ал", "л"].contains(&w.as_str()) || w.starts_with("allelic") || w.starts_with("alleliclad")
    };
    let neg_tok = |t: &str| {
        let w = strip_digits(t);
        ["nc", "ntc", "neg", "negative", "negctrl", "negcontrol", "blank", "нк"].contains(&w.as_str()) || w.starts_with("отр")
    };
    let pos_tok = |t: &str| {
        let w = strip_digits(t);
        ["pc", "pos", "positive", "posctrl", "poscontrol", "ctrl", "control", "ctl", "9947", "9947a", "2800m", "пк"].contains(&w.as_str()) || w.starts_with("пол")
    };
    if parts.iter().any(|t| ladder_tok(t)) {
        return "ladder";
    }
    if parts.iter().any(|t| neg_tok(t)) {
        return "negative";
    }
    if parts.iter().any(|t| pos_tok(t)) {
        return "positive";
    }
    "sample"
}

// ------------------------------------------------------------------ analysis payloads
/// `compactAnalysis`: what the core needs from an analysis for genotyping (pull-up event traces dropped).
pub fn compact_analysis(a: &Value) -> Value {
    let mut out = Map::new();
    for k in ["source_sha256", "analysis_version", "peak_settings", "sizing", "standard_definition_context", "pullup_detection"] {
        if let Some(v) = a.get(k) {
            out.insert(k.into(), v.clone());
        }
    }
    let chans: Vec<Value> = a["channels"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            let peaks: Vec<Value> = c["peaks"].as_array().into_iter().flatten().map(slim_pullup).collect();
            json!({"tag": c["tag"], "group": c["group"], "peaks": peaks})
        })
        .collect();
    out.insert("channels".into(), Value::Array(chans));
    Value::Object(out)
}

fn slim_pullup(p: &Value) -> Value {
    let mut p = p.clone();
    if let Some(pc) = p.get_mut("pullup_classification").and_then(|v| v.as_object_mut()) {
        if let Some(Value::Array(ev)) = pc.remove("events") {
            pc.insert("event_count".into(), json!(ev.len()));
        }
    }
    p
}

/// `slimReport`: pull-up traces of the report peaks dropped, run ladders reduced to identity and quality.
pub fn slim_report(mut r: Value) -> Value {
    if let Some(peaks) = r.get_mut("peaks").and_then(|v| v.as_array_mut()) {
        for p in peaks.iter_mut() {
            *p = slim_pullup(p);
        }
    }
    if let Some(rl) = r.pointer_mut("/quality/run_ladders").and_then(|v| v.as_array_mut()) {
        const KEEP: [&str; 7] = ["ladder_id", "identity", "status", "reasons", "quality_code", "quality_code_source", "provenance"];
        for l in rl.iter_mut() {
            let o: Map<String, Value> = KEEP.iter().filter_map(|k| l.get(*k).map(|v| (k.to_string(), v.clone()))).collect();
            *l = Value::Object(o);
        }
    }
    r
}

/// Allelic ladder report: labels only on the peaks matched to ladder alleles (marker -> scan -> allele name).
fn ladder_report(mut rep: Value, matched: Option<&HashMap<String, HashMap<u64, Value>>>) -> Value {
    let keep = |c: &Value, marker: &str| -> Option<Value> {
        let scan = c["scan"].as_f64()?.to_bits();   // the same number (JavaScript Map key)
        let label = matched?.get(marker)?.get(&scan)?.clone();
        let mut c = c.clone();
        c["label"] = label;
        Some(c)
    };
    if let Some(calls) = rep.get_mut("calls").and_then(|v| v.as_array_mut()) {
        *calls = calls.iter().filter_map(|c| keep(c, c["marker"].as_str().unwrap_or(""))).collect();
    }
    if let Some(ms) = rep.pointer_mut("/allele_calling/markers").and_then(|v| v.as_array_mut()) {
        for m in ms.iter_mut() {
            let name = m["name"].as_str().unwrap_or("").to_string();
            if let Some(calls) = m.get_mut("calls").and_then(|v| v.as_array_mut()) {
                *calls = calls
                    .iter()
                    .filter_map(|c| {
                        let mut c = c.clone();
                        if c.get("marker").map_or(true, |v| v.is_null() || v == "") {
                            c["marker"] = json!(name);
                        }
                        let mk = c["marker"].as_str().unwrap_or("").to_string();
                        keep(&c, &mk)
                    })
                    .collect();
            }
        }
    }
    rep["ladder_labels"] = json!(true);
    rep
}

// ------------------------------------------------------------------ quality (web `samplePqv`)
fn cat(v: Option<f64>) -> &'static str {
    match v {
        None => "na",
        Some(v) if v >= 0.75 => "pass",
        Some(v) if v <= 0.25 => "low",
        Some(_) => "check",
    }
}
pub fn code(c: &Value) -> &'static str {
    match c.as_i64() {
        Some(0) => "pass",
        Some(1) => "check",
        Some(2) => "low",
        _ => "na",
    }
}

#[derive(Clone, Default)]
pub struct Pqv {
    pub sos: &'static str,
    pub sq: &'static str,
    pub sspk: &'static str,
    pub cgq: &'static str,
}

pub fn sample_pqv(s: &Sample) -> Pqv {
    let mut q = Pqv { sos: "na", sq: "na", sspk: "na", cgq: "na" };
    let Some(a) = s.analysis.as_ref() else { return q };
    if s.status != Status::Analyzed {
        return q;
    }
    q.sq = crate::backend::sizing_quality(a);
    if q.sq == "low" {
        return q;
    }
    let peaks: Vec<&Value> = a["channels"].as_array().into_iter().flatten().flat_map(|c| c["peaks"].as_array().into_iter().flatten()).collect();
    q.sos = if peaks.iter().any(|p| p["offscaled"].as_bool().unwrap_or(false)) { "check" } else { "pass" };
    if peaks.iter().any(|p| truthy(&p["spike_classification"])) {
        q.sspk = if peaks.iter().any(|p| truthy(&p["spike"])) { "check" } else { "pass" };
    }
    if s.stype == "ladder" {
        q.cgq = match s.ladder_quality {
            None => "na",
            Some(0) => "pass",
            Some(_) => "low",
        };
    } else if let Some(rep) = &s.report {
        let gq = &rep["quality"]["marker_gq"];
        if !gq.is_null() {
            q.cgq = code(&gq["sample_category"]);
        }
    }
    q
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

fn sq_pass(s: &Sample) -> bool {
    let q = sample_pqv(s).sq;
    q == "pass" || q == "override"
}

// ------------------------------------------------------------------ genotyping
struct Group {
    rf: String,
    key: String,
    members: Vec<usize>,
    ladders: Value,
    off_ladders: Option<Value>,
    pass: bool,
    ladder_set: Option<String>,
    off_set: Option<String>,
    ladder_match: HashMap<String, HashMap<String, HashMap<u64, Value>>>,
    /// ladder summary of the run for the Allelic Ladder Analysis Summary
    ladder_status: String,
}

impl Inner {
    fn api(&self, path: &str, body: &Value) -> Result<Value, String> {
        server::api_json(path, body, &self.resources.join("data").join("shipped_methods.json"))
    }

    fn method(&self, name: &str) -> Option<Value> {
        self.methods.lock().unwrap().iter().find(|m| m["name"] == name).cloned()
    }

    /// `kitFor`: the analysis kit of a panel for a method (built by the core from the Panel Manager store), cached.
    pub fn kit_for(&self, panel_key: &str, method_name: &str) -> Result<Arc<KitEntry>, String> {
        let key = format!("{panel_key}\u{2}{method_name}");
        if let Some(k) = self.kit_cache.lock().unwrap().get(&key) {
            return Ok(k.clone());
        }
        let method = self.method(method_name).ok_or_else(|| format!("Метод анализа не найден: {method_name}"))?;
        let bin_set = method["allele"]["bin_set"].as_str().unwrap_or("").to_string();
        let req = self.kits.lock().unwrap().request(panel_key, &bin_set).ok_or("Панель не готова: в Менеджере панелей нет панели или набора бинов")?;
        let r = self.api(
            "/api/import-gm-kit",
            &json!({"panels": req.panels, "bins": req.bins, "stutter": req.stutter, "panel": req.panel, "analysis_method": method}),
        )?;
        let entry = Arc::new(KitEntry {
            kit: r["kit"].clone(),
            lq: r["ladder_quality_definition"].clone(),
            mq: r["marker_quality_definition"].clone(),
            policy: "forward_fill".into(),
            bin_set: req.bin_set,
            bin_set_substituted: req.bin_set_substituted,
        });
        self.kit_cache.lock().unwrap().insert(key, entry.clone());
        Ok(entry)
    }

    /// Kit of a sample (panel + method), None without a panel or when the kit cannot be built.
    pub fn kit_of(&self, s: &Sample) -> Option<Arc<KitEntry>> {
        if s.panel_key.is_empty() {
            return None;
        }
        self.kit_for(&s.panel_key, &s.method).ok()
    }

    fn register_ladders(&self, ladders: &Value) -> Option<String> {
        self.api("/api/ladder-set", &json!({"ladders": ladders})).ok().and_then(|v| v["ladder_set"].as_str().map(String::from))
    }

    fn str_run(&self, mut body: Map<String, Value>, set: &Option<String>, ladders: &Value) -> Result<Value, String> {
        match set {
            Some(id) => body.insert("ladder_set".into(), json!(id)),
            None => body.insert("ladders".into(), ladders.clone()),
        };
        self.api("/api/str-run", &Value::Object(body))
    }

    fn sample(&self, i: usize) -> Sample {
        self.samples.lock().unwrap()[i].clone()
    }

    fn update<F: FnOnce(&mut Sample)>(&self, i: usize, f: F) {
        if let Some(s) = self.samples.lock().unwrap().get_mut(i) {
            f(s)
        }
    }

    fn ladder_input(s: &Sample) -> Value {
        json!({"ladder_id": s.file, "analysis": compact_analysis(s.analysis.as_deref().unwrap_or(&Value::Null))})
    }

    fn groups(&self) -> Vec<Group> {
        let list = self.samples.lock().unwrap();
        let mut runs: Vec<String> = Vec::new();
        for s in list.iter() {
            if !runs.contains(&s.run_folder) {
                runs.push(s.run_folder.clone());
            }
        }
        let mut out = Vec::new();
        for rf in runs {
            let in_run: Vec<usize> = (0..list.len()).filter(|&i| {
                let s = &list[i];
                s.run_folder == rf && s.analysis.is_some() && s.status == Status::Analyzed && !s.panel_key.is_empty()
            }).collect();
            let mut keys: Vec<String> = Vec::new();
            for &i in &in_run {
                if !keys.contains(&list[i].panel_key) {
                    keys.push(list[i].panel_key.clone());
                }
            }
            for key in keys {
                let members = in_run.iter().copied().filter(|&i| list[i].panel_key == key).collect();
                out.push(Group { rf: rf.clone(), key, members, ladders: json!([]), off_ladders: None, pass: false, ladder_set: None, off_set: None, ladder_match: HashMap::new(), ladder_status: String::new() });
            }
        }
        out
    }

    /// `judgeLadders`: quality of each allelic ladder of the group, whether the run can be genotyped, bin offsets.
    fn judge_ladders(&self, g: &mut Group) {
        let lad: Vec<usize> = g.members.iter().copied().filter(|&i| self.sample(i).stype == "ladder").collect();
        for &i in &lad {
            self.update(i, |s| {
                s.ladder_quality = None;
                s.ladder_used = false;
            });
        }
        let lad_s: Vec<Sample> = lad.iter().map(|&i| self.sample(i)).collect();
        g.ladders = Value::Array(lad_s.iter().map(Self::ladder_input).collect());
        g.pass = false;
        let Some(probe) = lad_s.iter().find(|s| s.analysis.as_ref().is_some_and(|a| a["sizing"]["status"] == "accepted")) else {
            g.ladder_status = if lad_s.is_empty() { "no ladder".into() } else { "no sized ladder".into() };
            return;
        };
        let k = match self.kit_for(&g.key, &probe.method) {
            Ok(k) => k,
            Err(e) => {
                g.ladder_status = e;
                return;
            }
        };
        g.ladder_set = self.register_ladders(&g.ladders);
        g.off_set = g.ladder_set.clone();
        let body = |s: &Sample| -> Map<String, Value> {
            let mut b = Map::new();
            b.insert("sample".into(), compact_analysis(s.analysis.as_deref().unwrap_or(&Value::Null)));
            b.insert("kit".into(), k.kit.clone());
            b.insert("ladder_quality_definition".into(), k.lq.clone());
            b.insert("missing_offset_policy".into(), json!(k.policy));
            b
        };
        let rep = match self.str_run(body(probe), &g.ladder_set, &g.ladders) {
            Ok(r) => r,
            Err(e) => {
                g.ladder_status = e;
                return;
            }
        };
        let used: BTreeSet<String> = rep["ladder"]["markers"]
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(_, m)| m["status"] == "accepted")
            .flat_map(|(_, m)| m["source_ladders"].as_array().cloned().unwrap_or_default())
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        for l in rep["quality"]["run_ladders"].as_array().into_iter().flatten() {
            if let Some(pos) = lad_s.iter().position(|s| l["ladder_id"] == s.file.as_str()) {
                let q = l["quality_code"].as_i64();
                let file = lad_s[pos].file.clone();
                self.update(lad[pos], |s| {
                    s.ladder_quality = q;
                    s.ladder_used = used.contains(&file);
                });
            }
            let mut per_marker = HashMap::new();
            for (m, v) in l["matching"]["markers"].as_object().into_iter().flatten() {
                let mm: HashMap<u64, Value> = v["matched"].as_array().into_iter().flatten().filter_map(|p| Some((p["scan"].as_f64()?.to_bits(), p["label"].clone()))).collect();
                per_marker.insert(m.clone(), mm);
            }
            g.ladder_match.insert(l["ladder_id"].as_str().unwrap_or("").to_string(), per_marker);
        }
        let ok_status = |r: &Value| matches!(r["ladder"]["status"].as_str(), Some("accepted") | Some("partial"));
        let lad_now: Vec<Sample> = lad.iter().map(|&i| self.sample(i)).collect();
        g.pass = ok_status(&rep) && lad_now.iter().any(|s| s.ladder_quality == Some(0) && sq_pass(s));
        g.ladder_status = rep["ladder"]["status"].as_str().unwrap_or("").to_string();
        // ID-X bin offsets: passing ladders with green SQ (Project Options: or green and yellow) contribute.
        let bo = self.options.lock().unwrap().bin_offsets.clone();
        let green: Vec<&Sample> = lad_now.iter().filter(|s| sq_pass(s) || (bo == "greenYellow" && sample_pqv(s).sq == "check")).collect();
        if !green.is_empty() && green.len() < lad_now.len() {
            let off: Value = Value::Array(green.iter().map(|s| Self::ladder_input(s)).collect());
            let gp = green.iter().find(|s| s.analysis.as_ref().is_some_and(|a| a["sizing"]["status"] == "accepted")).map(|s| (*s).clone()).unwrap_or_else(|| probe.clone());
            let set = self.register_ladders(&off);
            match self.str_run(body(&gp), &set, &off) {
                Ok(r) => {
                    g.pass = g.pass && ok_status(&r);
                    g.off_ladders = Some(off);
                    g.off_set = set;
                }
                Err(_) => {
                    g.off_ladders = None;
                    g.off_set = g.ladder_set.clone();
                }
            }
        }
    }

    fn control_definition(s: &Sample, kit: &Value) -> Value {
        let mut markers = Map::new();
        for m in kit["markers"].as_array().into_iter().flatten() {
            let exp = if s.stype == "negative" { json!([]) } else { m.get("control_alleles").cloned().unwrap_or(json!([])) };
            markers.insert(
                m["name"].as_str().unwrap_or("").into(),
                json!({"expected_alleles": exp, "quality_sensor": m["internal_qc"].as_bool().unwrap_or(false), "is_y_marker": m["y_marker"].as_bool().unwrap_or(false)}),
            );
        }
        json!({"schema_version": 1, "sample_type": if s.stype == "negative" { "Negative Control" } else { "Positive Control" }, "markers": markers})
    }

    fn genotype_sample(&self, i: usize, g: &Group, k: &KitEntry, set: &Option<String>) {
        let s = self.sample(i);
        let opts = self.options.lock().unwrap().clone();
        let policy = if g.pass { k.policy.clone() } else { "reference_bins".into() };
        let mut mq = k.mq.clone();
        if !mq.is_null() {
            let acc_w = self.method(&s.method).and_then(|m| m["sq_gq"]["weights"]["acc"].as_f64()).unwrap_or(0.0);
            // Samples: AMEL cross check weight of the method enters GQ (ID-X: "Effect on GQ (samples only)").
            if s.stype == "sample" && truthy(&mq["amel_marker"]) && acc_w > 0.0 {
                if let Some(o) = mq.as_object_mut() {
                    o.insert("schema_version".into(), json!(2));
                    o.insert("sample_type".into(), json!("Sample"));
                    o.insert("control_mode".into(), json!("none"));
                    o.insert("amel_cross_check".into(), json!({"weight": acc_w}));
                }
            }
        }
        let mut body = Map::new();
        body.insert("sample".into(), compact_analysis(s.analysis.as_deref().unwrap_or(&Value::Null)));
        body.insert("kit".into(), k.kit.clone());
        body.insert("ladder_quality_definition".into(), k.lq.clone());
        body.insert("missing_offset_policy".into(), json!(policy));
        body.insert("marker_quality_definition".into(), mq);
        body.insert("slim".into(), json!(true));
        body.insert("pullup_excludes_calls".into(), json!(opts.pullup_excludes_calls));
        body.insert("homozygous_min_height".into(), json!(opts.homo_min_height));
        body.insert("heterozygous_min_height".into(), json!(opts.het_min_height));
        let ladders = g.off_ladders.clone().unwrap_or_else(|| g.ladders.clone());
        match self.str_run(body, set, &ladders) {
            Ok(mut rep) => {
                if !rep["allele_calling"].is_null() && (s.stype == "positive" || s.stype == "negative") {
                    let def = Self::control_definition(&s, &k.kit);
                    match self.api("/api/controls", &json!({"report": rep, "definition": def})) {
                        Ok(r) => rep = r,
                        Err(e) => {
                            if !rep["quality"].is_object() {
                                rep["quality"] = json!({});
                            }
                            rep["quality"]["controls_error"] = json!(e);
                        }
                    }
                }
                let rep = Arc::new(slim_report(rep));
                self.update(i, |s| {
                    s.report = Some(rep);
                    s.genotype_error.clear();
                    s.reference_bins = !g.pass;
                });
            }
            Err(e) => self.update(i, |s| {
                s.report = None;
                s.genotype_error = e;
            }),
        }
    }

    fn genotype_ladder(&self, i: usize, g: &Group, k: &KitEntry, set: &Option<String>) {
        let s = self.sample(i);
        let mut body = Map::new();
        body.insert("sample".into(), compact_analysis(s.analysis.as_deref().unwrap_or(&Value::Null)));
        body.insert("kit".into(), k.kit.clone());
        body.insert("ladder_quality_definition".into(), k.lq.clone());
        body.insert("missing_offset_policy".into(), json!(if g.pass { k.policy.clone() } else { "reference_bins".into() }));
        body.insert("slim".into(), json!(true));
        let ladders = g.off_ladders.clone().unwrap_or_else(|| g.ladders.clone());
        let r = self.str_run(body, set, &ladders);
        let m = g.ladder_match.get(&s.file);
        self.update(i, |s| {
            match r {
                Ok(rep) => {
                    s.report = Some(Arc::new(ladder_report(slim_report(rep), m)));
                    s.genotype_error.clear();
                }
                Err(e) => {
                    s.report = None;
                    s.genotype_error = e;
                }
            }
            s.reference_bins = !g.pass;
        });
    }
}

/// Runs `f` for every item on `n` threads (stops early when the analysis is stopped).
fn parallel<T: Sync, F: Fn(&T) + Sync>(items: &[T], n: usize, stop: &std::sync::atomic::AtomicBool, f: F) {
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|sc| {
        for _ in 0..n.min(items.len()).max(1) {
            sc.spawn(|| loop {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let k = next.fetch_add(1, Ordering::SeqCst);
                let Some(it) = items.get(k) else { break };
                f(it);
            });
        }
    });
}

/// `genotype()`: ladders of every run folder + panel first, the Allelic Ladder Analysis Summary decision, then the
/// samples and the ladders of each group (several requests at a time, like the web UI).
pub fn genotype(inner: &Arc<Inner>) {
    let mut gs = inner.groups();
    if gs.is_empty() {
        return;
    }
    {
        let mut list = inner.samples.lock().unwrap();
        for g in &gs {
            for &i in &g.members {
                list[i].alas_skipped = false;
                list[i].reference_bins = false;
            }
        }
    }
    let n = Inner::concurrency();
    {
        let mut p = inner.progress.lock().unwrap();
        p.phase = "ladders".into();
        p.message = gs.iter().map(|g| g.rf.clone()).collect::<Vec<_>>().join(", ");
    }
    // ladders of independent groups in parallel
    let cells: Vec<Mutex<&mut Group>> = gs.iter_mut().map(Mutex::new).collect();
    parallel(&cells, n, &inner.stop, |c| inner.judge_ladders(&mut c.lock().unwrap()));
    drop(cells);
    if inner.stop.load(Ordering::SeqCst) {
        return;
    }
    let failing: Vec<&Group> = gs.iter().filter(|g| !g.pass).collect();
    let mut mode = "passing".to_string();
    if !failing.is_empty() {
        let o = inner.options.lock().unwrap().alas.clone();
        mode = if o == "stop" {
            let summary: Vec<Value> = gs
                .iter()
                .map(|g| {
                    let lad: Vec<Value> = g.members.iter().map(|&i| inner.sample(i)).filter(|s| s.stype == "ladder").map(|s| json!({"file": s.file, "quality": s.ladder_quality, "sq": sample_pqv(&s).sq})).collect();
                    json!({"run_folder": g.rf, "panel": g.key.split('\u{1}').nth(1).unwrap_or(""), "pass": g.pass, "status": g.ladder_status, "ladders": lad})
                })
                .collect();
            match inner.ask.ask(Value::Array(summary).to_string(), &inner.stop) {
                Some(a) => a,
                None => return,
            }
        } else {
            o
        };
        if mode == "view" {
            let mut list = inner.samples.lock().unwrap();
            for g in &failing {
                for &i in &g.members {
                    if list[i].stype != "ladder" {
                        list[i].report = None;
                    }
                }
            }
            return;
        }
    }
    {
        let mut p = inner.progress.lock().unwrap();
        p.phase = "genotype".into();
    }
    for g in gs.iter_mut() {
        if inner.stop.load(Ordering::SeqCst) {
            return;
        }
        let non_ladders: Vec<usize> = g.members.iter().copied().filter(|&i| inner.sample(i).stype != "ladder").collect();
        if !g.pass && mode != "all" {
            for &i in &non_ladders {
                inner.update(i, |s| {
                    s.report = None;
                    s.alas_skipped = true;
                });
            }
            continue;
        }
        let mut methods: Vec<String> = Vec::new();
        for &i in &non_ladders {
            let m = inner.sample(i).method;
            if !methods.contains(&m) {
                methods.push(m);
            }
        }
        let mut kits: HashMap<String, Arc<KitEntry>> = HashMap::new();
        let mut kit_error = None;
        for m in &methods {
            match inner.kit_for(&g.key, m) {
                Ok(k) => {
                    kits.insert(m.clone(), k);
                }
                Err(e) => kit_error = Some(e),
            }
        }
        if g.off_set.is_none() {
            let l = g.off_ladders.clone().unwrap_or_else(|| g.ladders.clone());
            g.off_set = inner.register_ladders(&l);
        }
        let set = g.off_set.clone();
        inner.progress.lock().unwrap().message = g.rf.clone();
        let gref: &Group = g;
        parallel(&non_ladders, n, &inner.stop, |&i| {
            let m = inner.sample(i).method;
            match kits.get(&m) {
                Some(k) => inner.genotype_sample(i, gref, k, &set),
                None => inner.update(i, |s| {
                    s.report = None;
                    s.genotype_error = kit_error.clone().unwrap_or_else(|| "no kit".into());
                }),
            }
        });
        let ladders: Vec<usize> = g.members.iter().copied().filter(|&i| inner.sample(i).stype == "ladder").collect();
        let lm = ladders.first().map(|&i| inner.sample(i).method).or_else(|| methods.first().cloned());
        if let Some(lk) = lm.and_then(|m| inner.kit_for(&g.key, &m).ok()) {
            parallel(&ladders, n, &inner.stop, |&i| inner.genotype_ladder(i, gref, &lk, &set));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_types_like_the_web_ui() {
        for (n, t) in [
            ("Ladder-2026-10-16_10-20-34-A1-A96.fsa", "ladder"),
            ("AL_01.fsa", "ladder"),
            ("PC-2026-10-16_14-31-7-G11-A96.fsa", "positive"),
            ("NC-2026-10-16_14-31-7-H11-A96.fsa", "negative"),
            ("333-1-2026-10-16_10-20-34-B1-A96.fsa", "sample"),
            ("9947A_x.fsa", "positive"),
            ("blank_3.fsa", "negative"),
            ("K+_2.fsa", "positive"),
            ("007_B02.fsa", "positive"),
            ("x_007_D07.fsa", "sample"),
            ("F-2026-10-16_14-31-7-A12-A96.fsa", "sample"),
            ("01PC.fsa", "positive"),
        ] {
            assert_eq!(sample_type_from_name(n), t, "{n}");
        }
    }
}

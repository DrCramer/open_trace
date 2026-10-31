//! Port of `genemapper_open/codis.py`: CODIS CMF 3.2 / 3.3 XML export.
//!
//! Known gaps: `\d` in the allele/timestamp patterns is ASCII-only (Python accepts any Unicode decimal digit);
//! `str.isdigit()` for CODISMarkers limits is ASCII-only; JSON integers beyond i64.
use crate::gm_kit_import::{splitlines_pub, type_name};
use crate::profiles::{py_repr, py_str, strip};
use crate::pullup_quality::truthy;
use serde_json::{json, Map, Value};

pub type Res<T> = Result<T, String>;

pub const CODIS_MARKERS_DEFAULT: &str = "**#TOTAL_MARKER_FOR_CMF1.0=64
**#TOTAL_MARKER_FOR_CMF3.x=64
**#YMARKER=32
**#STRMARKER=32
Amelogenin,AMEL
DYF387S1
DYS389 I,B_DYS389I
DYS389 II,B_DYS389II
DYS390,B_DYS390
DYS449
DYS456,B_DYS456
DYS460
DYS518
DYS627
CSF1PO
D10S1248
D12S391
D13S317
D16S539
D18S51
D19S433
D1S1656
D21S11
D22S1045
D2S1338
D2S441
D3S1358
D5S818
D7S820
D8S1179
DYS391
FGA
DYS19,G_DYS19
DYS385,G_DYS385
DYS458,G_DYS458
Penta D,Penta_D
Penta E,Penta_E
DYS437,R_DYS437
DYS438,R_DYS438
DYS448,R_DYS448
YGATAH4,R_Y_GATA_H4
SE33
TH01
TPOX
vWA
DYS391,Y_DYS391
DYS392,Y_DYS392
DYS393,Y_DYS393
DYS439,Y_DYS439
DYS635,Y_DYS635
Yindel,Yindel
DYS481
DYS533
DYS549
DYS570
DYS576
DYS643
";

pub const CODIS_KITS_DEFAULT: &str = "Yfiler,Yfiler
AmpFLSTR NGM SElect,NGM_SElect
AmpFLSTR NGM,NGM
AmpFlSTR Yfiler Plus,Yfiler_Plus_Panel
MiniFiler,MiniFiler
Profiler Plus,Profiler_Plus_CODIS
Profiler Plus,Profiler_Plus
COfiler,COfiler_CODIS
COfiler,COfiler
GlobalFiler Express,GlobalFiler_Express
GlobalFiler,GlobalFiler_Panel
Identifiler Direct,Identifiler_Direct
Identifiler Plus,Identifiler_Plus
Identifiler,Identifiler_CODIS
Identifiler,Identifiler
SGM Plus,SGM_Plus
";

const SPECIMEN_CATEGORIES: [&str; 33] = [
    "Alleged Father", "Alleged Mother", "Arrestee", "Biological Child", "Biological Father", "Biological Mother",
    "Biological Sibling", "CO Duplicate", "Convicted Offender", "Deceased", "Deduced Missing Person",
    "Deduced Suspect", "Detainee", "Elimination, Known", "Forensic Mixture", "Forensic Partial",
    "Forensic, Unknown", "Juvenile", "Legal", "Maternal Relative", "Missing Person", "Multi-allelic Offender",
    "Other", "Paternal Relative", "Population", "Proficiency", "Spouse", "Staff", "Suspect, Known",
    "Unidentified Person", "Unreviewed Offender", "Victim, Known", "Volunteer",
];
const DEFAULT_SPECIMEN_TYPES: [&str; 25] = [
    "No Export", "Alleged Father", "Alleged Mother", "Biological Child", "Biological Father", "Biological Mother",
    "Biological Sibling", "Convicted Offender", "Elimination, Known", "Forensic Mixture", "Forensic, Unknown",
    "Maternal Relative", "Missing Person", "Other", "Paternal Relative", "Population", "Proficiency",
    "Suspect, Known", "Unidentified Person", "Victim, Known", "Deduced Missing Person", "Detainee",
    "Forensic Partial", "Multi-allelic Offender", "Unreviewed Offender",
];

fn name_bad(s: &str) -> bool {
    s.chars().any(|c| "\\/:*?\"<>|".contains(c))
}

#[derive(Clone, Debug)]
pub struct Limits {
    cmf1: u64,
    cmf3: u64,
    y: u64,
    str_: u64,
}

impl Limits {
    fn repr(&self) -> String {
        format!(
            "{{'TOTAL_MARKER_FOR_CMF1.0': {}, 'TOTAL_MARKER_FOR_CMF3.x': {}, 'YMARKER': {}, 'STRMARKER': {}}}",
            self.cmf1, self.cmf3, self.y, self.str_
        )
    }
    fn value(&self) -> Value {
        json!({"TOTAL_MARKER_FOR_CMF1.0": self.cmf1, "TOTAL_MARKER_FOR_CMF3.x": self.cmf3, "YMARKER": self.y, "STRMARKER": self.str_})
    }
}

fn isdigit_ascii(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

fn str_digits(s: &str) -> u64 {
    let mut v: u64 = 0;
    for c in s.chars() {
        v = v.saturating_mul(10).saturating_add(c.to_digit(10).unwrap() as u64);
    }
    v
}

/// Returns (ordered mapping panel marker -> CODIS locus, limits).
pub fn parse_codis_markers(text: Option<&Value>) -> Res<(Vec<(String, String)>, Limits)> {
    let text: String = match text {
        None | Some(Value::Null) => CODIS_MARKERS_DEFAULT.to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(_) => return Err("CODISMarkers text required (at most 256 KiB)".into()),
    };
    if text.chars().count() > 262144 {
        return Err("CODISMarkers text required (at most 256 KiB)".into());
    }
    let mut limits = Limits { cmf1: 64, cmf3: 64, y: 32, str_: 32 };
    let mut mapping: Vec<(String, String)> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for raw in splitlines_pub(&text) {
        let line = strip(raw);
        if let Some(rest) = line.strip_prefix("**#") {
            let (key, value) = match rest.split_once('=') {
                Some((k, v)) => (k, v),
                None => (rest, ""),
            };
            let k = strip(key);
            let slot = match k {
                "TOTAL_MARKER_FOR_CMF1.0" => Some(&mut limits.cmf1),
                "TOTAL_MARKER_FOR_CMF3.x" => Some(&mut limits.cmf3),
                "YMARKER" => Some(&mut limits.y),
                "STRMARKER" => Some(&mut limits.str_),
                _ => None,
            };
            if let Some(slot) = slot {
                let v = strip(value);
                if !isdigit_ascii(v) {
                    return Err(format!("CODISMarkers: {} needs a numeric value", k));
                }
                *slot = str_digits(v);
            }
            continue;
        }
        if line.is_empty() || line.starts_with("**") {
            continue;
        }
        let (codis, panel) = match line.split_once(',') {
            Some((c, p)) => (c, p),
            None => (line, ""),
        };
        let codis = strip(codis);
        let panel = if strip(panel).is_empty() { codis } else { strip(panel) };
        if name_bad(panel) || panel.contains(' ') {
            return Err(format!("CODISMarkers: invalid panel marker name {}", py_repr(&Value::String(panel.to_string()))));
        }
        let low = panel.to_lowercase();
        if seen.contains(&low) {
            return Err(format!("CODISMarkers: duplicate marker {}", py_repr(&Value::String(panel.to_string()))));
        }
        seen.push(low);
        mapping.push((panel.to_string(), codis.to_string()));
    }
    Ok((mapping, limits))
}

pub fn parse_codis_kits(text: Option<&Value>) -> Res<Vec<(String, String)>> {
    let text: String = match text {
        None | Some(Value::Null) => CODIS_KITS_DEFAULT.to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => return Err(format!("'{}' object has no attribute 'splitlines'", type_name(other))),
    };
    let mut out = Vec::new();
    for raw in splitlines_pub(&text) {
        let line = strip(raw);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (codis, panel) = match line.split_once(',') {
            Some((c, p)) => (c, p),
            None => (line, ""),
        };
        let panel = if strip(panel).is_empty() { strip(codis) } else { strip(panel) };
        out.push((panel.to_string(), strip(codis).to_string()));
    }
    Ok(out)
}

pub fn codis_kit_name(panel: &str, kits: &[(String, String)]) -> String {
    let mut best: Option<&(String, String)> = None;
    let pl = panel.to_lowercase();
    for k in kits {
        if panel == k.0 || pl.starts_with(&k.0.to_lowercase()) {
            if best.map(|b| k.0.chars().count() > b.0.chars().count()).unwrap_or(true) {
                best = Some(k);
            }
        }
    }
    best.map(|b| b.1.clone()).unwrap_or_else(|| panel.to_string())
}

pub fn is_y_locus(name: &str) -> bool {
    let u = name.to_uppercase();
    u.starts_with("DY") || u.starts_with('Y')
}

fn text_of(value: Option<&Value>, field: &str, limit: usize) -> Res<String> {
    let v = match value {
        None | Some(Value::Null) => String::new(),
        Some(v) => strip(&py_str(v)).to_string(),
    };
    if v.contains('|') || v.contains(';') {
        return Err(format!("{}: text must not contain | or ;", field));
    }
    if v.chars().count() > limit {
        return Err(format!("{}: at most {} characters", field, limit));
    }
    Ok(v)
}

fn bool_of(value: Option<&Value>, field: &str) -> Res<Option<bool>> {
    match value {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(s)) if s.is_empty() => return Ok(None),
        Some(Value::Bool(b)) => return Ok(Some(*b)),
        _ => {}
    }
    let t = strip(&py_str(value.unwrap())).to_lowercase();
    match t.as_str() {
        "true" | "t" | "yes" | "1" => Ok(Some(true)),
        "false" | "f" | "no" | "0" => Ok(Some(false)),
        _ => Err(format!("{}: Partial Profile Indicator must be True or False", field)),
    }
}

fn allele_ok(a: &str) -> bool {
    // ^[<>]?\d{1,3}(\.\d{1,2})?[<>]?$
    let c: Vec<char> = a.chars().collect();
    let mut i = 0;
    if i < c.len() && (c[i] == '<' || c[i] == '>') {
        i += 1;
    }
    let s = i;
    while i < c.len() && c[i].is_ascii_digit() {
        i += 1;
    }
    if !(1..=3).contains(&(i - s)) {
        return false;
    }
    if i < c.len() && c[i] == '.' {
        i += 1;
        let s2 = i;
        while i < c.len() && c[i].is_ascii_digit() {
            i += 1;
        }
        if !(1..=2).contains(&(i - s2)) {
            return false;
        }
    }
    if i < c.len() && (c[i] == '<' || c[i] == '>') {
        i += 1;
    }
    i == c.len()
}

fn stamp_ok(s: &str) -> bool {
    // ^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d$  ($ also matches before one trailing newline)
    let t = s.strip_suffix('\n').unwrap_or(s);
    let pat = "dddd-dd-ddTdd:dd:dd";
    let c: Vec<char> = t.chars().collect();
    if c.len() != pat.len() {
        return false;
    }
    pat.chars().zip(c).all(|(p, ch)| if p == 'd' { ch.is_ascii_digit() } else { p == ch })
}

pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn quoteattr(s: &str) -> String {
    let d = xml_escape(s).replace('\n', "&#10;").replace('\r', "&#13;").replace('\t', "&#9;");
    if d.contains('"') {
        if d.contains('\'') {
            format!("\"{}\"", d.replace('"', "&quot;"))
        } else {
            format!("'{}'", d)
        }
    } else {
        format!("\"{}\"", d)
    }
}

#[repr(C)]
struct Tm {
    tm_sec: i32,
    tm_min: i32,
    tm_hour: i32,
    tm_mday: i32,
    tm_mon: i32,
    tm_year: i32,
    tm_wday: i32,
    tm_yday: i32,
    tm_isdst: i32,
    tm_gmtoff: i64,
    tm_zone: *const i8,
}
extern "C" {
    fn time(t: *mut i64) -> i64;
    fn localtime_r(t: *const i64, r: *mut Tm) -> *mut Tm;
}

fn now_stamp() -> String {
    unsafe {
        let t = time(std::ptr::null_mut());
        let mut tm: Tm = std::mem::zeroed();
        localtime_r(&t, &mut tm);
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
    }
}

#[derive(Clone)]
struct Spec {
    category: String,
    case_id: String,
    comment: String,
    partial: Option<bool>,
    source_id: String,
    ncic: String,
    vicap: String,
}

#[derive(Clone)]
struct Locus {
    name: String,
    alleles: Vec<String>,
    partial: bool,
    kit: String,
    batch: String,
}

struct Specimen {
    spec: Spec,
    loci: Vec<Locus>,
}

pub fn build_cmf(request: &Value, markers_text: Option<&Value>, kits_text: Option<&Value>) -> Res<Value> {
    let req = match request {
        Value::Object(m) => m,
        _ => return Err("CODIS export request must be an object".into()),
    };
    let version = match req.get("version") {
        Some(v) => py_str(v),
        None => "3.2".to_string(),
    };
    if version != "3.2" && version != "3.3" {
        return Err("CMF version must be 3.2 or 3.3".into());
    }
    let comment_limit = if version == "3.2" { 255 } else { 512 };
    let source = text_of(req.get("source_lab"), "Source Lab ID", 64)?;
    let dest = text_of(req.get("destination_ori"), "Destination Lab ID", 64)?;
    let user = text_of(req.get("user"), "User", 64)?;
    if source.is_empty() || dest.is_empty() {
        return Err("Select a Source and a Destination Lab ID".into());
    }
    if user.is_empty() {
        return Err("Enter the CODIS user name (it must match the CODIS user)".into());
    }
    let stamp = match req.get("submit_datetime") {
        Some(v) if truthy(v) => py_str(v),
        _ => now_stamp(),
    };
    if !stamp_ok(&stamp) {
        return Err("submit_datetime must be YYYY-MM-DDThh:mm:ss".into());
    }
    let (mapping, limits) = parse_codis_markers(markers_text)?;
    let kits = parse_codis_kits(kits_text)?;
    let samples = match req.get("samples") {
        Some(Value::Array(a)) if !a.is_empty() && a.len() <= 5000 => a,
        _ => return Err("Select 1..5000 samples for CODIS export".into()),
    };
    let mut skipped: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut specimens: Vec<(String, Specimen, Vec<String>)> = Vec::new();
    let mut exported = 0usize;
    for entry in samples {
        let eo = match entry {
            Value::Object(m) => m,
            _ => return Err("Each CODIS sample must be an object".into()),
        };
        let name = text_of(eo.get("sample_name"), "Sample Name", 255)?;
        let category = match eo.get("category") {
            Some(v) if truthy(v) => py_str(v),
            _ => "No Export".to_string(),
        };
        if category == "No Export" {
            continue;
        }
        if !SPECIMEN_CATEGORIES.contains(&category.as_str()) {
            return Err(format!("{}: {} is not a valid specimen category", name, py_repr(&Value::String(category))));
        }
        let attempt = (|| -> Result<(String, Spec, Vec<Locus>), Result<String, String>> {
            // Err(Ok(msg)) = collected ValueError, Err(Err(msg)) = fatal (non-ValueError) exception
            let ve = |e: String| Err(Ok(e));
            let sid_src = match eo.get("specimen_id") {
                Some(v) if truthy(v) => Some(v.clone()),
                _ => Some(Value::String(name.clone())),
            };
            let sid = match text_of(sid_src.as_ref(), "Specimen ID", 64) {
                Ok(v) => v,
                Err(e) => return ve(e),
            };
            macro_rules! t {
                ($e:expr) => {
                    match $e {
                        Ok(v) => v,
                        Err(e) => return ve(e),
                    }
                };
            }
            let case_id = t!(text_of(eo.get("case_id"), "Case ID", 64));
            let comment = t!(text_of(eo.get("comment"), "Specimen Comment", comment_limit));
            let partial = t!(bool_of(eo.get("partial"), "Partial Profile Indicator"));
            let source_id = t!(text_of(eo.get("source_id"), "Source ID", 64));
            let ncic = if version == "3.3" { t!(text_of(eo.get("ncic"), "NCIC", 64)) } else { String::new() };
            let vicap = if version == "3.3" { t!(text_of(eo.get("vicap"), "ViCAP", 64)) } else { String::new() };
            let spec = Spec { category: category.clone(), case_id, comment, partial, source_id, ncic, vicap };
            if !spec.ncic.is_empty() && spec.ncic.chars().count() != 10 {
                return ve("Specimen NCIC Number (UD7) needs exactly 10 characters".into());
            }
            if !spec.vicap.is_empty() && spec.vicap.chars().count() != 12 {
                return ve("Specimen ViCAP Number (UD8) needs exactly 12 characters".into());
            }
            let panel = t!(text_of(eo.get("panel"), "Panel", 120));
            let kit = codis_kit_name(&panel, &kits);
            let batch = t!(text_of(eo.get("batch_id"), "Batch ID", 64));
            let mut loci = Vec::new();
            let loci_v: Vec<Value> = match eo.get("loci") {
                Some(v) if truthy(v) => match v {
                    Value::Array(a) => a.clone(),
                    Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
                    Value::Object(o) => o.keys().map(|k| Value::String(k.clone())).collect(),
                    other => return Err(Err(format!("'{}' object is not iterable", type_name(other)))),
                },
                _ => vec![],
            };
            for locus in &loci_v {
                let lo = match locus {
                    Value::Object(m) => m,
                    other => return Err(Err(format!("'{}' object has no attribute 'get'", type_name(other)))),
                };
                let marker = match lo.get("marker") {
                    Some(v) => py_str(v),
                    None => String::new(),
                };
                let al_src: Vec<Value> = match lo.get("alleles") {
                    Some(v) if truthy(v) => match v {
                        Value::Array(a) => a.clone(),
                        Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
                        Value::Object(o) => o.keys().map(|k| Value::String(k.clone())).collect(),
                        other => return Err(Err(format!("'{}' object is not iterable", type_name(other)))),
                    },
                    _ => vec![],
                };
                let alleles: Vec<String> = al_src.iter().map(|a| strip(&py_str(a)).to_string()).filter(|s| !s.is_empty()).collect();
                if alleles.is_empty() {
                    continue;
                }
                let codis = match mapping.iter().find(|(k, _)| *k == marker) {
                    Some((_, c)) => c.clone(),
                    None => {
                        if !skipped.contains(&marker) {
                            skipped.push(marker.clone());
                        }
                        continue;
                    }
                };
                for allele in &alleles {
                    let ok = if codis == "Amelogenin" { allele == "X" || allele == "Y" } else { allele_ok(allele) };
                    if !ok {
                        return ve(format!(
                            "The allele name {} for {} is invalid for CODIS export. Assign a valid allele name and try again.",
                            allele, marker
                        ));
                    }
                }
                loci.push(Locus { name: codis, alleles, partial: truthy(lo.get("partial_locus").unwrap_or(&Value::Null)) && version == "3.3", kit: kit.clone(), batch: batch.clone() });
            }
            if loci.is_empty() {
                return ve(format!("Unable to export {}: samples designated for CODIS export must contain at least one allele", name));
            }
            Ok((sid, spec, loci))
        })();
        let (sid, spec, loci) = match attempt {
            Ok(x) => x,
            Err(Ok(msg)) => {
                errors.push(if msg.contains(&name) { msg } else { format!("{}: {}", name, msg) });
                continue;
            }
            Err(Err(msg)) => return Err(msg),
        };
        let idx = specimens.iter().position(|(k, _, _)| *k == sid);
        let idx = match idx {
            None => {
                specimens.push((sid.clone(), Specimen { spec: spec.clone(), loci: Vec::new() }, vec![name.clone()]));
                specimens.len() - 1
            }
            Some(i) => {
                let cur = &mut specimens[i];
                let labels: [(&str, bool); 5] = [
                    ("specimen categories", cur.1.spec.category != spec.category),
                    ("case identifiers (UD3)", cur.1.spec.case_id != spec.case_id),
                    ("specimen comments (UD4)", cur.1.spec.comment != spec.comment),
                    ("partial profile indicators (UD5)", cur.1.spec.partial != spec.partial),
                    ("source identifiers (UD6)", cur.1.spec.source_id != spec.source_id),
                ];
                for (label, diff) in labels {
                    if diff {
                        errors.push(format!("The {} for {} do not match. Assign the same value before exporting to CODIS.", label, sid));
                    }
                }
                cur.2.push(name.clone());
                i
            }
        };
        for locus in loci {
            let cur = &mut specimens[idx].1;
            match cur.loci.iter().find(|l| l.name == locus.name) {
                Some(prior) => {
                    let (mut a, mut b) = (prior.alleles.clone(), locus.alleles.clone());
                    a.sort();
                    b.sort();
                    if a != b {
                        errors.push(format!(
                            "Non-concordant genotypes for {} at {}: genotypes must be reconciled before exporting to CODIS.",
                            sid, locus.name
                        ));
                    }
                }
                None => cur.loci.push(locus),
            }
        }
        exported += 1;
    }
    if !errors.is_empty() {
        let mut seen: Vec<&String> = Vec::new();
        for e in &errors {
            if !seen.contains(&e) {
                seen.push(e);
            }
        }
        return Err(seen.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n"));
    }
    if specimens.is_empty() {
        return Err("All selected samples have specimen type No Export; the export was not performed.".into());
    }
    for (sid, sp, _) in &specimens {
        let n = sp.loci.len() as u64;
        let y = sp.loci.iter().filter(|l| is_y_locus(&l.name)).count() as u64;
        if n > limits.cmf3 || y > limits.y || n - y > limits.str_ {
            return Err(format!("{}: too many loci for CMF ({}; limits {})", sid, n, limits.repr()));
        }
    }
    let mut lines: Vec<String> = vec![
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>".into(),
        "<CODISImportFile xmlns=\"urn:CODISImportFile-schema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">".into(),
        format!("<HEADERVERSION>{}</HEADERVERSION>", version),
        "<MESSAGETYPE>Import</MESSAGETYPE>".into(),
        format!("<DESTINATIONORI>{}</DESTINATIONORI>", xml_escape(&dest)),
        format!("<SOURCELAB>{}</SOURCELAB>", xml_escape(&source)),
        format!("<SUBMITBYUSERID>{}</SUBMITBYUSERID>", xml_escape(&user)),
        format!("<SUBMITDATETIME>{}</SUBMITDATETIME>", stamp),
    ];
    for (sid, sp, _) in &specimens {
        let s = &sp.spec;
        let mut attrs = String::new();
        if !s.source_id.is_empty() {
            attrs += &format!(" SOURCEID={}", quoteattr(&s.source_id));
        }
        if !s.case_id.is_empty() {
            attrs += &format!(" CASEID={}", quoteattr(&s.case_id));
        }
        if !s.ncic.is_empty() {
            attrs += &format!(" NCIC={}", quoteattr(&s.ncic));
        }
        if !s.vicap.is_empty() {
            attrs += &format!(" ViCAP={}", quoteattr(&s.vicap));
        }
        if let Some(p) = s.partial {
            attrs += &format!(" PARTIAL=\"{}\"", if p { "true" } else { "false" });
        }
        lines.push(format!("<SPECIMEN{}>", attrs));
        lines.push(format!("<SPECIMENID>{}</SPECIMENID>", xml_escape(sid)));
        lines.push(format!("<SPECIMENCATEGORY>{}</SPECIMENCATEGORY>", xml_escape(&s.category)));
        if !s.comment.is_empty() {
            lines.push(format!("<SPECIMENCOMMENT>{}</SPECIMENCOMMENT>", xml_escape(&s.comment)));
        }
        for locus in &sp.loci {
            let la = (if locus.batch.is_empty() { String::new() } else { format!(" BATCHID={}", quoteattr(&locus.batch)) })
                + (if locus.partial { " PARTIALLOCUS=\"true\"" } else { "" })
                + &format!(" KIT={}", quoteattr(&locus.kit));
            lines.push(format!("<LOCUS{}>", la));
            lines.push(format!("<LOCUSNAME>{}</LOCUSNAME>", xml_escape(&locus.name)));
            lines.push(format!("<READINGBY>{}</READINGBY>", xml_escape(&user)));
            lines.push(format!("<READINGDATETIME>{}</READINGDATETIME>", stamp));
            for allele in &locus.alleles {
                lines.push(format!("<ALLELE><ALLELEVALUE>{}</ALLELEVALUE></ALLELE>", xml_escape(allele)));
            }
            lines.push("</LOCUS>".into());
        }
        lines.push("</SPECIMEN>".into());
    }
    lines.push("</CODISImportFile>".into());
    skipped.retain(|m| !m.is_empty());
    skipped.sort();
    Ok(json!({"xml": lines.join("\n") + "\n", "version": version, "specimens": specimens.len(), "samples": exported, "skipped_markers": skipped}))
}

pub fn codis_request(request: &Value) -> Res<Value> {
    let action = match request {
        Value::Object(m) => m.get("action").and_then(|a| a.as_str()),
        _ => None,
    };
    let req = request.as_object();
    match (action, req) {
        (Some("config"), Some(r)) => {
            let (mapping, limits) = parse_codis_markers(r.get("markers_text"))?;
            let mut mm = Map::new();
            for (k, v) in &mapping {
                mm.insert(k.clone(), Value::String(v.clone()));
            }
            let kits = parse_codis_kits(r.get("kits_text"))?;
            Ok(json!({"markers": Value::Object(mm), "limits": limits.value(),
                "kits": kits.iter().map(|(a, b)| json!([a, b])).collect::<Vec<_>>(),
                "specimen_categories": SPECIMEN_CATEGORIES.to_vec(), "default_specimen_types": DEFAULT_SPECIMEN_TYPES.to_vec()}))
        }
        (Some("export"), Some(r)) => build_cmf(request, r.get("markers_text"), r.get("kits_text")),
        _ => Err("action must be config or export".into()),
    }
}

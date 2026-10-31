//! Port of `genemapper_open/__main__.py`: the `opentrace` command line.
use crate::abif::AbifFile;
use crate::abif_view::{channel_obj, directory, summary_head};
use crate::project_store::{ProjectStore, Review, MAX_EXPORT_BYTES};
use crate::pydump::{dumps, str_repr, Opts};
use crate::pyjson;
use crate::server::{self, py_float, py_int, Config};
use crate::server_analyze::analyze;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

type Res<T> = Result<T, String>;

const MODELS: [&str; 6] = ["linear", "lsq2", "lsq3", "natural_cubic", "local_rational", "global_southern"];
const ENGINES: [&str; 2] = ["residual", "morphology_ratio"];
const HEIGHT_MODES: [&str; 2] = ["explicit", "basic_standard_auto"];
const ACTIONS: [&str; 8] = ["accept", "exclude", "change-label", "note", "add-allele", "delete-artifact-label", "override-gq", "override-cgq"];
const TARGET_KINDS: [&str; 4] = ["analysis", "call", "peak", "marker"];
const VARIANTS: [&str; 22] = [
    "ordinary", "negative", "amel", "ladder", "ladder-rejected", "spike", "spike-unrequested", "spike-low-initial",
    "spike-low-adjusted", "sq-pass", "sq-check", "sq-fail", "sq-ladder-fail", "ladder-calls", "ladder-sample-filters",
    "morphology", "morphology-unrequested", "true-peak", "true-peak-excluded", "pullup", "pullup-flags-only", "pullup-unrequested",
];
const ANALYZE_KEYS: [&str; 24] = [
    "threshold", "distance", "anchors", "sizing_mode", "standard_channel", "standard_bp", "max_residual_bp",
    "standard_start", "standard_end", "calibration_model", "analysis_engine", "height_mode", "quality_threshold",
    "num_extra_peaks", "spike_definition", "sizing_quality_definition", "pullup_definition", "channel_thresholds",
    "detector", "analysis_range", "sizing_override", "normalization", "matrix", "",
];

#[derive(Clone, Copy, PartialEq)]
enum K {
    Str,
    Int,
    Float,
    Flag,
}
struct O {
    long: &'static str,
    short: Option<&'static str>,
    kind: K,
    required: bool,
    multi: bool,
    choices: &'static [&'static str],
}
fn o(long: &'static str, kind: K) -> O {
    O { long, short: None, kind, required: false, multi: false, choices: &[] }
}
impl O {
    fn req(mut self) -> O {
        self.required = true;
        self
    }
    fn short(mut self, s: &'static str) -> O {
        self.short = Some(s);
        self
    }
    fn ch(mut self, c: &'static [&'static str]) -> O {
        self.choices = c;
        self
    }
    fn multi(mut self) -> O {
        self.multi = true;
        self
    }
}

#[derive(Default)]
struct Args {
    pos: Vec<String>,
    vals: HashMap<String, Vec<String>>,
    flags: HashSet<String>,
}
impl Args {
    fn s(&self, k: &str) -> Option<&str> {
        self.vals.get(k).and_then(|v| v.last()).map(|x| x.as_str())
    }
    fn flag(&self, k: &str) -> bool {
        self.flags.contains(k)
    }
    fn p(&self, i: usize) -> &str {
        &self.pos[i]
    }
    fn path(&self, k: &str) -> Option<PathBuf> {
        self.s(k).map(PathBuf::from)
    }
    /// Typed value: converted number, or the Python default (which argparse leaves unconverted).
    fn num(&self, k: &str, default: Value, float: bool) -> Value {
        match self.s(k) {
            Some(t) if float => py_float(t).unwrap_or(Value::Null),
            Some(t) => py_int(t).unwrap_or(Value::Null),
            None => default,
        }
    }
}

fn usage_error(cmd: &str, msg: &str) -> ! {
    eprintln!("usage: opentrace {} [-h] ...\nopentrace {}: error: {}", cmd, cmd, msg);
    std::process::exit(2)
}

fn parse(cmd: &str, args: &[String], npos: (usize, usize), opts: &[O]) -> Args {
    let mut out = Args::default();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if let Some(rest) = a.strip_prefix("--").filter(|r| !r.is_empty()) {
            let (name, inline) = match rest.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (rest, None),
            };
            let Some(opt) = opts.iter().find(|x| x.long == name) else { usage_error(cmd, &format!("unrecognized arguments: {}", a)) };
            i = take(cmd, opt, &format!("--{}", name), inline, args, i, &mut out);
        } else if a.len() == 2 && a.starts_with('-') && a != "--" && !a[1..].chars().next().unwrap().is_ascii_digit() {
            let Some(opt) = opts.iter().find(|x| x.short == Some(a.as_str())) else { usage_error(cmd, &format!("unrecognized arguments: {}", a)) };
            i = take(cmd, opt, a, None, args, i, &mut out);
        } else {
            out.pos.push(a.clone());
        }
        i += 1;
    }
    if out.pos.len() < npos.0 {
        usage_error(cmd, "the following arguments are required: positional");
    }
    if out.pos.len() > npos.1 {
        usage_error(cmd, &format!("unrecognized arguments: {}", out.pos[npos.1..].join(" ")));
    }
    for opt in opts {
        if opt.required && !out.vals.contains_key(opt.long) {
            usage_error(cmd, &format!("the following arguments are required: --{}", opt.long));
        }
    }
    out
}

fn take(cmd: &str, opt: &O, shown: &str, inline: Option<String>, args: &[String], i: usize, out: &mut Args) -> usize {
    if opt.kind == K::Flag {
        out.flags.insert(opt.long.to_string());
        return i;
    }
    let (val, next) = match inline {
        Some(v) => (v, i),
        None => match args.get(i + 1) {
            Some(v) => (v.clone(), i + 1),
            None => usage_error(cmd, &format!("argument {}: expected one argument", shown)),
        },
    };
    match opt.kind {
        K::Int if py_int(&val).is_err() => usage_error(cmd, &format!("argument {}: invalid int value: {}", shown, str_repr(&val))),
        K::Float if py_float(&val).is_err() => usage_error(cmd, &format!("argument {}: invalid float value: {}", shown, str_repr(&val))),
        _ => {}
    }
    if !opt.choices.is_empty() && !opt.choices.contains(&val.as_str()) {
        let list: Vec<String> = opt.choices.iter().map(|c| str_repr(c)).collect();
        usage_error(cmd, &format!("argument {}: invalid choice: {} (choose from {})", shown, str_repr(&val), list.join(", ")));
    }
    let e = out.vals.entry(opt.long.to_string()).or_default();
    if !opt.multi {
        e.clear();
    }
    e.push(val);
    next
}

// ------------------------------------------------------------------------------------------------ helpers

fn io_err(e: &std::io::Error, path: &Path) -> String {
    let text = e.to_string();
    let base = text.split(" (os error").next().unwrap_or(&text).to_string();
    match e.raw_os_error() {
        Some(n) => format!("[Errno {}] {}: {}", n, base, str_repr(&path.to_string_lossy())),
        None => base,
    }
}

fn read_bytes(path: &Path) -> Res<Vec<u8>> {
    std::fs::read(path).map_err(|e| io_err(&e, path))
}

fn read_json_text(path: &Path, limit: usize) -> Res<String> {
    let meta = std::fs::metadata(path).map_err(|e| io_err(&e, path))?;
    if meta.len() as usize > limit {
        return Err(format!("JSON file exceeds {} byte limit", limit));
    }
    let raw = read_bytes(path)?;
    String::from_utf8(raw).map_err(|e| format!("'utf-8' codec can't decode byte: {}", e))
}

fn loads(text: &str) -> Res<Value> {
    pyjson::loads(text, false).map_err(|e| e.message())
}

fn dump_indent(v: &Value) -> String {
    dumps(v, Opts { ensure_ascii: false, sort_keys: false, indent: Some(2) })
}
fn dump_indent_ascii(v: &Value) -> String {
    dumps(v, Opts { ensure_ascii: true, sort_keys: false, indent: Some(2) })
}

fn write_new(path: &Path, text: &str) -> Res<()> {
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| io_err(&e, path))?;
    f.write_all(text.as_bytes()).map_err(|e| io_err(&e, path))
}
fn write_text(path: &Path, text: &str) -> Res<()> {
    std::fs::write(path, text).map_err(|e| io_err(&e, path))
}
fn resolve(path: &Path) -> String {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().into_owned()
}
fn emit_output(output: Option<PathBuf>, text: &str) -> Res<()> {
    match output {
        Some(p) => {
            write_new(&p, text)?;
            println!("{}", resolve(&p));
        }
        None => {
            print!("{}", text);
        }
    }
    Ok(())
}

fn read_abif(path: &Path) -> Res<AbifFile> {
    let meta = std::fs::metadata(path).map_err(|e| io_err(&e, path))?;
    if meta.len() as usize > crate::abif::MAX_FILE_BYTES {
        return Err("File exceeds 64 MiB limit".into());
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    AbifFile::parse(&read_bytes(path)?, &name).map_err(|e| e.to_string())
}

fn check_settings(settings: &Map<String, Value>) -> Res<()> {
    for k in settings.keys() {
        if k == "raw" || k == "filename" {
            return Err(format!("analyze() got multiple values for argument {}", str_repr(k)));
        }
        if !ANALYZE_KEYS.contains(&k.as_str()) || k.is_empty() {
            return Err(format!("analyze() got an unexpected keyword argument {}", str_repr(k)));
        }
    }
    Ok(())
}

fn analyze_with(bytes: &[u8], name: &str, settings: &Map<String, Value>) -> Res<Value> {
    check_settings(settings)?;
    analyze(bytes, name, settings)
}

fn rejected(v: &Value) -> bool {
    v.get("status").and_then(|s| s.as_str()) == Some("rejected")
}

/// `analyze_str_files`.
fn analyze_str_files(sample: &[u8], ladder: &[u8], kit: &Value, settings: &Value, policy: &str, lq: Option<&Value>) -> Res<Value> {
    let settings = settings.as_object().ok_or("Explicit analysis settings object required")?;
    let s = analyze_with(sample, "sample.fsa", settings)?;
    let l = analyze_with(ladder, "ladder.fsa", settings)?;
    let mut result = crate::str_workflow::run_str_workflow(&s, &l, kit, policy, lq)?;
    result["provenance"]["input_reports_are_caller_supplied"] = json!(false);
    Ok(result)
}

/// `analyze_str_run_files`.
fn analyze_str_run_files(sample: &[u8], ladders: &[(Value, Vec<u8>)], kit: &Value, settings: &Value, lq: &Value, policy: &str) -> Res<Value> {
    let settings = settings.as_object().ok_or("Explicit analysis settings object required")?;
    let s = analyze_with(sample, "sample.fsa", settings)?;
    let mut reports = Vec::new();
    for (id, data) in ladders {
        let name = id.as_str().ok_or("Run ladder_id must be a nonempty string")?;
        reports.push(json!({"ladder_id": id, "analysis": analyze_with(data, name, settings)?}));
    }
    let reports = Value::Array(reports);
    let opts = crate::run_workflow::RunOptions {
        ladder_quality_definition: lq,
        missing_offset_policy: policy,
        marker_gate: false,
        homozygous_min_height: json!(200.0),
        heterozygous_min_height: json!(100.0),
    };
    let mut result = crate::run_workflow::run_str_run_workflow(&s, &reports, kit, &opts)?;
    result["provenance"]["input_reports_are_caller_supplied"] = json!(false);
    Ok(result)
}

fn analyze_options() -> Vec<O> {
    vec![
        o("pullup-definition", K::Str),
        o("spike-definition", K::Str),
        o("sizing-quality-definition", K::Str),
        o("analysis-engine", K::Str).ch(&ENGINES),
        o("height-mode", K::Str).ch(&HEIGHT_MODES),
        o("quality-threshold", K::Float),
        o("num-extra-peaks", K::Int),
        o("standard-channel", K::Str),
        o("standard-bp", K::Str),
        o("max-residual-bp", K::Float),
        o("standard-start", K::Int),
        o("standard-end", K::Int),
        o("calibration-model", K::Str).ch(&MODELS),
    ]
}

// ------------------------------------------------------------------------------------------------ commands

fn cmd_size_export(command: &str, a: &Args) -> Res<i32> {
    let file = PathBuf::from(a.p(0));
    let sample = read_abif(&file)?;
    let standard_channel = a.s("standard-channel").unwrap_or("");
    let standard_bp = a.s("standard-bp").unwrap_or("");
    let model = a.s("calibration-model").unwrap_or("linear");
    let automatic = !standard_channel.is_empty() || !standard_bp.is_empty() || command == "size";
    let peaks = a.flag("peaks");
    if command == "export" && (automatic || model != "linear") && !peaks {
        return Err("Use --peaks to export automatically sized peak candidates".into());
    }
    let mut result = Value::Null;
    let output_text;
    if command == "size" || peaks {
        let def = |k: &str, limit: usize| -> Res<Value> {
            match a.path(k) {
                Some(p) => Ok(json!(read_json_text(&p, limit)?)),
                None => Ok(Value::Null),
            }
        };
        let mut kw = Map::new();
        kw.insert("threshold".into(), a.num("threshold", json!(100), true));
        kw.insert("distance".into(), a.num("distance", json!(8), false));
        kw.insert("anchors".into(), json!(a.s("anchors").unwrap_or("")));
        kw.insert("sizing_mode".into(), json!(if automatic { "auto" } else { "" }));
        kw.insert("standard_channel".into(), json!(standard_channel));
        kw.insert("standard_bp".into(), json!(standard_bp));
        kw.insert("max_residual_bp".into(), a.num("max-residual-bp", json!(5), true));
        kw.insert("standard_start".into(), a.num("standard-start", json!(0), false));
        kw.insert("standard_end".into(), a.num("standard-end", Value::Null, false));
        kw.insert("calibration_model".into(), json!(model));
        kw.insert("analysis_engine".into(), json!(a.s("analysis-engine").unwrap_or("residual")));
        kw.insert("height_mode".into(), json!(a.s("height-mode").unwrap_or("explicit")));
        kw.insert("quality_threshold".into(), a.num("quality-threshold", json!(0.75), true));
        kw.insert("num_extra_peaks".into(), a.num("num-extra-peaks", json!(25), false));
        kw.insert("spike_definition".into(), def("spike-definition", 16384)?);
        kw.insert("pullup_definition".into(), def("pullup-definition", 16384)?);
        kw.insert("sizing_quality_definition".into(), def("sizing-quality-definition", 16384)?);
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        result = analyze(&read_bytes(&file)?, &name, &kw)?;
        if command == "size" {
            let mut report = Map::new();
            for k in ["filename", "source_sha256", "analysis_version", "peak_settings", "sizing", "research_only"] {
                report.insert(k.into(), result.get(k).cloned().ok_or_else(|| format!("{}", str_repr(k)))?);
            }
            output_text = dump_indent(&Value::Object(report)) + "\n";
        } else {
            let channels = result["channels"].as_array().cloned().unwrap_or_default();
            output_text = crate::export::peaks_csv(&channels)?;
        }
    } else {
        let channels: Vec<Value> = sample.channels().map_err(|e| e.to_string())?.iter().map(|c| Value::Object(channel_obj(c))).collect();
        output_text = crate::export::traces_csv(&channels)?;
    }
    match a.path("output") {
        Some(p) => {
            write_text(&p, &output_text)?;
            println!("{}", resolve(&p));
        }
        None => print!("{}", output_text),
    }
    if (command == "size" || peaks) && result["sizing"]["status"] == json!("rejected") {
        let reasons: Vec<String> = result["sizing"]["reasons"].as_array().map(|r| r.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
        eprintln!("Automatic sizing rejected: {}", reasons.join("; "));
        return Ok(3);
    }
    Ok(0)
}

fn cmd_inspect(a: &Args) -> Res<i32> {
    let sample = read_abif(Path::new(a.p(0)))?;
    let mut result = summary_head(&sample);
    let mut channels = Vec::new();
    for c in sample.channels().map_err(|e| e.to_string())? {
        let mut m = channel_obj(&c);
        let n = m.get("values").and_then(|v| v.as_array()).map(|v| v.len()).unwrap_or(0);
        m.shift_remove("values");
        m.insert("points".into(), json!(n));
        channels.push(Value::Object(m));
    }
    result.insert("channels".into(), Value::Array(channels));
    if a.flag("tags") {
        result.insert("directory".into(), directory(&sample));
    }
    println!("{}", dump_indent(&Value::Object(result)));
    Ok(0)
}

fn demo_documents(dir: &Path, docs: &[(&str, &Value)]) -> Res<()> {
    for (name, v) in docs {
        write_text(&dir.join(name), &(dump_indent(v) + "\n"))?;
    }
    Ok(())
}

fn cmd_str_demo(a: &Args) -> Res<i32> {
    let variant = a.s("variant").unwrap_or("ordinary");
    let d = server::build_str_demo(variant).map_err(herr)?;
    let mut result = analyze_str_files(&d.fixture.sample_bytes, &d.fixture.ladder_bytes, &d.fixture.kit, &d.fixture.settings, "reject", d.ladder_quality_definition.as_ref())?;
    if let Some(c) = &d.controls {
        result = crate::control_workflow::attach_control_result(&result, c)?;
    }
    if server::demo_marker_quality_attached(variant) {
        result = crate::marker_quality_workflow::attach_marker_quality(&result, &d.quality_definition)?;
    }
    let dir = PathBuf::from(a.p(0));
    std::fs::create_dir(&dir).map_err(|e| io_err(&e, &dir))?;
    write_new_bytes(&dir.join("sample.fsa"), &d.fixture.sample_bytes)?;
    write_new_bytes(&dir.join("ladder.fsa"), &d.fixture.ladder_bytes)?;
    let spike_demo = crate::demo::make_spike_demo_definition();
    let mut docs: Vec<(&str, &Value)> = vec![
        ("kit.json", &d.fixture.kit),
        ("settings.json", &d.fixture.settings),
        ("report.json", &result),
        ("marker-quality.json", &d.quality_definition),
    ];
    if let Some(c) = &d.controls {
        docs.push(("control.json", c));
    }
    if let Some(q) = &d.ladder_quality_definition {
        docs.push(("ladder-quality.json", q));
    }
    if variant == "spike" || variant == "spike-unrequested" {
        docs.push(("spike.json", &spike_demo));
    }
    if ["spike-low-initial", "spike-low-adjusted", "morphology", "true-peak", "true-peak-excluded"].contains(&variant) {
        docs.push(("spike.json", &d.fixture.settings["spike_definition"]));
    }
    if variant.starts_with("sq-") {
        docs.push(("sizing-quality.json", &d.fixture.settings["sizing_quality_definition"]));
    }
    if d.fixture.settings.get("pullup_definition").is_some() {
        docs.push(("pullup.json", &d.fixture.settings["pullup_definition"]));
    }
    demo_documents(&dir, &docs)?;
    println!("{}", resolve(&dir));
    Ok(if rejected(&result) { 3 } else { 0 })
}

fn write_new_bytes(path: &Path, data: &[u8]) -> Res<()> {
    std::fs::write(path, data).map_err(|e| io_err(&e, path))
}

fn herr(e: server::HErr) -> String {
    match e {
        server::HErr::Bad(m) | server::HErr::Denied(m) | server::HErr::Other(m) => m,
    }
}

fn cmd_str(a: &Args) -> Res<i32> {
    let settings = loads(&read_json_text(&PathBuf::from(a.s("settings").unwrap()), 262144)?)?;
    if !settings.is_object() {
        return Err("Analysis settings must be an explicit JSON object".into());
    }
    let sample = read_bytes(Path::new(a.p(0)))?;
    let ladder = read_bytes(&PathBuf::from(a.s("ladder").unwrap()))?;
    let kit = json!(read_json_text(&PathBuf::from(a.s("kit").unwrap()), 1048576)?);
    let lq = match a.path("ladder-quality") {
        Some(p) => Some(json!(read_json_text(&p, 1048576)?)),
        None => None,
    };
    let result = analyze_str_files(&sample, &ladder, &kit, &settings, a.s("missing-offset-policy").unwrap_or("reject"), lq.as_ref())
        .map_err(|m| if m.starts_with("analyze() got") { format!("Invalid analysis settings: {}", m) } else { m })?;
    emit_output(a.path("output"), &(dump_indent(&result) + "\n"))?;
    Ok(if rejected(&result) { 3 } else { 0 })
}

fn cmd_str_run(a: &Args) -> Res<i32> {
    let manifest_path = PathBuf::from(a.p(0));
    let manifest = loads(&read_json_text(&manifest_path, 262144)?)?;
    let fields = ["schema_version", "sample", "kit", "settings", "ladder_quality", "missing_offset_policy", "ladders"];
    let ok = manifest.as_object().map(|m| m.len() == fields.len() && fields.iter().all(|f| m.contains_key(*f))).unwrap_or(false)
        && manifest["schema_version"].is_i64()
        && manifest["schema_version"] == json!(1);
    if !ok {
        return Err("Explicit version1 run manifest with all fields required".into());
    }
    let root = std::fs::canonicalize(&manifest_path).map_err(|e| io_err(&e, &manifest_path))?.parent().unwrap().to_path_buf();
    let path = |v: &Value| -> Res<PathBuf> {
        match v.as_str() {
            Some(s) if !s.is_empty() => Ok(root.join(s)),
            _ => Err("Nonempty run input path required".into()),
        }
    };
    let data = |v: &Value| -> Res<Vec<u8>> {
        let p = path(v)?;
        let meta = std::fs::metadata(&p).map_err(|e| io_err(&e, &p))?;
        if meta.len() > 64 * 1024 * 1024 {
            return Err("Run FSA exceeds64MiB".into());
        }
        read_bytes(&p)
    };
    let rows = manifest["ladders"].as_array().filter(|r| (1..=128).contains(&r.len())).ok_or("Explicit1..128 run ladders required")?;
    let mut ladders = Vec::new();
    for row in rows {
        let ok = row.as_object().map(|m| m.len() == 2 && m.contains_key("ladder_id") && m.contains_key("file")).unwrap_or(false);
        if !ok {
            return Err("Run ladder needs ladder_id/file".into());
        }
        ladders.push((row["ladder_id"].clone(), data(&row["file"])?));
    }
    let sample = data(&manifest["sample"])?;
    let kit = json!(read_json_text(&path(&manifest["kit"])?, 1048576)?);
    let settings = loads(&read_json_text(&path(&manifest["settings"])?, 262144)?)?;
    let lq = json!(read_json_text(&path(&manifest["ladder_quality"])?, 1048576)?);
    let policy = manifest["missing_offset_policy"].as_str().unwrap_or("\u{0}invalid").to_string();
    let report = analyze_str_run_files(&sample, &ladders, &kit, &settings, &lq, &policy)?;
    emit_output(a.path("output"), &(dump_indent(&report) + "\n"))?;
    Ok(if rejected(&report) { 3 } else { 0 })
}

fn cmd_str_run_demo(a: &Args) -> Res<i32> {
    let fx = crate::demo::make_run_demo();
    let dir = PathBuf::from(a.p(0));
    std::fs::create_dir(&dir).map_err(|e| io_err(&e, &dir))?;
    write_new_bytes(&dir.join("sample.fsa"), &fx.fixture.sample_bytes)?;
    for (id, data) in &fx.ladders {
        write_new_bytes(&dir.join(id), data)?;
    }
    let manifest = json!({"schema_version": 1, "sample": "sample.fsa", "kit": "kit.json", "settings": "settings.json",
        "ladder_quality": "ladder-quality.json", "missing_offset_policy": "reject",
        "ladders": fx.ladders.iter().map(|(id, _)| json!({"ladder_id": id, "file": id})).collect::<Vec<_>>()});
    demo_documents(&dir, &[
        ("kit.json", &fx.fixture.kit),
        ("settings.json", &fx.fixture.settings),
        ("ladder-quality.json", &fx.ladder_quality_definition),
        ("marker-quality.json", &fx.marker_quality_definition),
        ("run.json", &manifest),
    ])?;
    let ladders: Vec<(Value, Vec<u8>)> = fx.ladders.iter().map(|(id, d)| (json!(id), d.clone())).collect();
    let report = analyze_str_run_files(&fx.fixture.sample_bytes, &ladders, &fx.fixture.kit, &fx.fixture.settings, &fx.ladder_quality_definition, "reject")?;
    demo_documents(&dir, &[("report.json", &report)])?;
    println!("{}", resolve(&dir));
    Ok(if rejected(&report) { 3 } else { 0 })
}

fn cmd_annotate(command: &str, a: &Args) -> Res<i32> {
    let report = loads(&read_json_text(Path::new(a.p(0)), MAX_EXPORT_BYTES)?)?;
    let definition = json!(read_json_text(&PathBuf::from(a.s("definition").unwrap()), 1048576)?);
    let result = match command {
        "controls" => crate::control_workflow::attach_control_result(&report, &definition)?,
        "marker-quality" => crate::marker_quality_workflow::attach_marker_quality(&report, &definition)?,
        _ => crate::pullup_quality::attach_pullup_quality(&report, &definition)?,
    };
    emit_output(a.path("output"), &(dump_indent(&result) + "\n"))?;
    Ok(0)
}

fn cmd_import_kit(a: &Args) -> Res<i32> {
    let panels_text = String::from_utf8_lossy(&read_bytes(Path::new(a.p(0)))?).into_owned();
    if a.flag("list") || a.pos.len() < 2 {
        let doc = crate::gm_kit_import::parse_panels(&json!(panels_text))?;
        println!("{}", dump_indent(&crate::gm_kit_import::list_panels(&doc)));
        return Ok(0);
    }
    let bins = String::from_utf8_lossy(&read_bytes(Path::new(a.p(1)))?).into_owned();
    let stutter = match a.path("stutter") {
        Some(p) => json!(String::from_utf8_lossy(&read_bytes(&p)?).into_owned()),
        None => Value::Null,
    };
    let panel = match a.s("panel") {
        Some(p) if !p.is_empty() => json!(p),
        _ => {
            let list = crate::gm_kit_import::list_panels(&crate::gm_kit_import::parse_panels(&json!(panels_text))?);
            let list = list.as_array().unwrap();
            if list.len() == 1 { list[0]["panel"].clone() } else { Value::Null }
        }
    };
    let out = crate::gm_kit_import::import_request(&json!({"panels": panels_text, "bins": bins, "stutter": stutter, "panel": panel}))?;
    if out["kit"].is_null() {
        return Err("Several panels are present; choose one with --panel".into());
    }
    let text = dump_indent(&out["kit"]) + "\n";
    match a.path("output") {
        Some(p) => write_text(&p, &text)?,
        None => print!("{}", text),
    }
    if let Some(p) = a.path("ladder-quality") {
        write_text(&p, &(dump_indent_ascii(&out["ladder_quality_definition"]) + "\n"))?;
    }
    Ok(0)
}

fn cmd_demo(a: &Args) -> Res<i32> {
    if a.flag("sizing") && a.flag("pipeline") {
        usage_error("demo", "argument --pipeline: not allowed with argument --sizing");
    }
    let data = if a.flag("pipeline") {
        crate::demo::pipeline_demo_bytes()
    } else if a.flag("sizing") {
        crate::demo::sizing_demo_bytes()
    } else {
        crate::demo::demo_bytes()
    };
    let p = PathBuf::from(a.p(0));
    write_new_bytes(&p, &data)?;
    println!("{}", resolve(&p));
    Ok(0)
}

/// `opentrace health`: exit status 0 when the local server answers `/api/health` with 200 (container health check).
fn cmd_health(a: &Args) -> Res<i32> {
    use std::io::Read;
    let port = a.num("port", json!(8765), false).as_i64().unwrap_or(8765);
    let host = a.s("host").unwrap_or("127.0.0.1");
    let mut stream = std::net::TcpStream::connect((host, port as u16)).map_err(|e| e.to_string())?;
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(4)));
    let request = format!("GET /api/health HTTP/1.0\r\nHost: 127.0.0.1:{}\r\n\r\n", port);
    stream.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
    let mut data = Vec::new();
    let _ = stream.read_to_end(&mut data);
    let ok = String::from_utf8_lossy(&data).lines().next().map_or(false, |l| l.split_whitespace().nth(1) == Some("200"));
    Ok(if ok { 0 } else { 1 })
}

fn cmd_serve(a: &Args) -> Res<i32> {
    let port = a.num("port", json!(8765), false).as_i64().unwrap_or(8765);
    let host = a.s("host").map(|s| s.to_string()).or_else(|| std::env::var("OPENTRACE_HOST").ok()).unwrap_or_else(|| "127.0.0.1".into());
    let allowed: Vec<String> = match a.vals.get("allowed-host") {
        Some(v) => v.clone(),
        None => std::env::var("OPENTRACE_ALLOWED_HOSTS").unwrap_or_default().split(',').map(|s| s.to_string()).collect(),
    };
    let cfg = Config {
        port: port as u16,
        host,
        open_browser: a.flag("open"),
        projects_dir: a.path("projects-dir"),
        security_dir: a.path("security-dir"),
        allowed_hosts: allowed,
        resources: None,
    };
    server::serve(cfg)?;
    Ok(0)
}

// ---------------------------------------------------------------------------------------------- project

fn compact_project_report(report: &Value, explicit: Option<&str>) -> Res<(Value, Value, Value, Value, String, Value)> {
    let obj = report.as_object().ok_or("Analysis report must be a JSON object")?;
    let source = obj.get("source_sha256").cloned().unwrap_or(Value::Null);
    let version = match obj.get("analysis_version") {
        Some(Value::String(s)) if !crate::profiles::strip(s).is_empty() => s.clone(),
        _ => return Err("Analysis report requires analysis_version".into()),
    };
    let settings = match obj.get("settings").or_else(|| obj.get("peak_settings")) {
        Some(Value::Object(m)) if obj.contains_key("settings") || true => Value::Object(m.clone()),
        _ => return Err("Analysis report requires settings or peak_settings object".into()),
    };
    // `report.get("settings", report.get("peak_settings"))`: a present non-dict `settings` wins over peak_settings
    let settings = match obj.get("settings") {
        Some(v) if !v.is_object() => return Err("Analysis report requires settings or peak_settings object".into()),
        _ => settings,
    };
    let sizing = obj.get("sizing");
    let mut report_status = obj.get("status").cloned().unwrap_or(Value::Null);
    if report_status.is_null() {
        if let Some(Value::Object(s)) = sizing {
            report_status = s.get("status").cloned().unwrap_or(Value::Null);
        }
    }
    let status = match explicit {
        Some(s) => json!(s),
        None => report_status.clone(),
    };
    let status_text = match &status {
        Value::String(s) if !crate::profiles::strip(s).is_empty() => s.clone(),
        _ => return Err("Report must state status, or supply --status explicitly".into()),
    };
    if status_text == "accepted" && report_status != json!("accepted") {
        return Err("accepted status must be explicitly present in the analysis JSON".into());
    }
    if !report_status.is_null() && explicit.is_some() && json!(explicit.unwrap()) != report_status {
        return Err("--status must agree with the report's explicit status".into());
    }
    let mut result = Map::new();
    for k in [
        "schema_version", "source_sha256", "analysis_version", "status", "calls", "peaks", "sizing", "quality", "qc",
        "warnings", "ladder", "allele_calling", "kit_identity", "sample_identity", "ladder_identity", "provenance", "research_only",
    ] {
        if let Some(v) = obj.get(k) {
            result.insert(k.into(), v.clone());
        }
    }
    result.insert("status".into(), json!(status_text));
    let mut metadata = Map::new();
    if let Some(Value::String(f)) = obj.get("filename") {
        metadata.insert("file_name".into(), json!(f));
    }
    Ok((source, json!({"name": "OpenTrace", "version": version}), settings, Value::Object(result), status_text, Value::Object(metadata)))
}

fn se(e: crate::project_store::SErr) -> String {
    e.msg
}

fn project_command(a: &Args, action: &str) -> Res<Value> {
    let db = |i: usize| PathBuf::from(a.p(i));
    if action == "create" {
        let store = ProjectStore::create(&db(0), &json!(a.s("name").unwrap())).map_err(se)?;
        return Ok(json!({"database": store.path.as_ref().map(|p| p.to_string_lossy().into_owned()), "project": store.project().map_err(se)?}));
    }
    if action == "import" {
        let text = read_json_text(Path::new(a.p(0)), MAX_EXPORT_BYTES)?;
        let store = ProjectStore::import_json(&db(1), &json!(text)).map_err(se)?;
        return Ok(json!({"database": store.path.as_ref().map(|p| p.to_string_lossy().into_owned()), "project": store.project().map_err(se)?}));
    }
    let store = ProjectStore::open(&db(0)).map_err(se)?;
    if action == "upgrade" {
        let up = store.upgraded().map_err(se)?;
        let saved = up.save_new(&db(1)).map_err(se)?;
        return Ok(json!({"database": saved.path.as_ref().map(|p| p.to_string_lossy().into_owned()), "project": saved.project().map_err(se)?}));
    }
    let actor = a.s("actor").map(|s| json!(s)).unwrap_or(Value::Null);
    let reason = a.s("reason").map(|s| json!(s)).unwrap_or(Value::Null);
    if action.starts_with("run-") {
        use crate::project_runs::{change_run_ladder, create_project_run, revise_project_run};
        let run_id = if action == "run-create" { Value::Null } else { json!(a.p(1)) };
        let parent = a.s("parent-revision").map(|s| json!(s)).unwrap_or(Value::Null);
        return match action {
            "run-show" => Ok(json!({"run": store.get_run(&run_id).map_err(se)?, "revisions": store.list_run_revisions(&run_id).map_err(se)?})),
            "run-create" => {
                let inputs = json!(read_json_text(Path::new(a.p(1)), 16 * 1024 * 1024)?);
                create_project_run(&store, &json!(a.s("name").unwrap()), &inputs, &actor, &reason).map_err(se)
            }
            "run-revise" => {
                let inputs = json!(read_json_text(Path::new(a.p(2)), 16 * 1024 * 1024)?);
                revise_project_run(&store, &run_id, &parent, &inputs, &actor, &reason).map_err(se)
            }
            _ => {
                let analysis = if action == "run-replace-ladder" { loads(&read_json_text(Path::new(a.p(3)), 16 * 1024 * 1024)?)? } else { Value::Null };
                change_run_ladder(&store, &run_id, &parent, &json!(a.p(2)), &analysis, &json!(action == "run-remove-ladder"), &actor, &reason).map_err(se)
            }
        };
    }
    match action {
        "list" | "show" => {
            if action == "show" {
                if let Some(id) = a.s("analysis-id").filter(|s| !s.is_empty()) {
                    let id = json!(id);
                    return Ok(json!({"analysis": store.get_analysis(&id).map_err(se)?,
                        "effective": store.effective_analysis(&id).map_err(se)?,
                        "review_events": store.review_events(&id).map_err(se)?}));
                }
            }
            let mut analyses = Vec::new();
            for an in store.list_analyses().map_err(se)? {
                let mut m = Map::new();
                for k in ["analysis_id", "sample_id", "input_sha256", "status", "created_at", "algorithm_sha256", "settings_sha256", "result_sha256"] {
                    m.insert(k.into(), an.get(k).cloned().ok_or_else(|| str_repr(k))?);
                }
                analyses.push(Value::Object(m));
            }
            Ok(json!({"project": store.project().map_err(se)?, "samples": store.list_samples().map_err(se)?, "analyses": analyses}))
        }
        "export" => {
            let doc = store.export_json().map_err(se)?;
            let out = db(1);
            write_new(&out, &(doc + "\n"))?;
            Ok(json!({"output": resolve(&out)}))
        }
        "save" => {
            let report_path = db(1);
            let report = loads(&read_json_text(&report_path, MAX_EXPORT_BYTES)?)?;
            let (source, algorithm, settings, result, status, mut metadata) = compact_project_report(&report, a.s("status"))?;
            metadata["report_file_name"] = json!(report_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            let mut sample_id = a.s("sample-id").map(|s| s.to_string());
            if let Some(sid) = &sample_id {
                let samples = store.list_samples().map_err(se)?;
                let matches: Vec<&Value> = samples.iter().filter(|s| s["sample_id"] == json!(sid)).collect();
                if matches.is_empty() || matches[0]["source_sha256"] != source {
                    return Err("--sample-id must identify a sample with the report source SHA-256".into());
                }
            }
            let tmp = std::env::temp_dir().join(format!("opentrace-project-check-{}", crate::project_store::new_uuid()));
            std::fs::create_dir(&tmp).map_err(|e| io_err(&e, &tmp))?;
            let check = (|| -> Res<()> {
                let candidate = ProjectStore::create(&tmp.join("candidate.sqlite"), &json!("Report validation")).map_err(se)?;
                let cs = candidate.add_sample(&source, &metadata, &Value::Null, &Value::Null).map_err(se)?;
                candidate.add_analysis(&json!(cs), &source, &algorithm, &settings, &result, &json!(status), &Value::Null, &Value::Null).map_err(se)?;
                Ok(())
            })();
            let _ = std::fs::remove_dir_all(&tmp);
            check?;
            if sample_id.is_none() {
                sample_id = Some(store.add_sample(&source, &metadata, &Value::Null, &Value::Null).map_err(se)?);
            }
            let sid = json!(sample_id.unwrap());
            let aid = store.add_analysis(&sid, &source, &algorithm, &settings, &result, &json!(status), &Value::Null, &Value::Null).map_err(se)?;
            Ok(json!({"sample_id": sid, "analysis_id": aid, "status": status}))
        }
        "review" | "undo" => {
            let analysis_id = json!(a.p(1));
            let id = if action == "review" {
                let act = json!(a.p(2));
                let target_id = a.s("target-id").map(|s| json!(s)).unwrap_or(Value::Null);
                let value = a.s("value").map(|s| json!(s)).unwrap_or(Value::Null);
                let nul = Value::Null;
                store
                    .append_review(&Review {
                        analysis_id: &analysis_id,
                        action: &act,
                        actor: &actor,
                        reason: &reason,
                        target_kind: json!(a.s("target-kind").unwrap_or("analysis")),
                        target_id: &target_id,
                        value: &value,
                        event_id: &nul,
                        created_at: &nul,
                        reverses_event_id: &nul,
                    })
                    .map_err(se)?
            } else {
                store.undo_review(&analysis_id, &json!(a.p(2)), &actor, &reason).map_err(se)?
            };
            Ok(json!({"event_id": id, "effective": store.effective_analysis(&analysis_id).map_err(se)?}))
        }
        _ => Err("Unknown project action".into()),
    }
}

fn cmd_project(argv: &[String]) -> Res<i32> {
    let Some(action) = argv.first().map(|s| s.as_str()) else { usage_error("project", "the following arguments are required: project_action") };
    let rest = &argv[1..];
    let cmd = format!("project {}", action);
    let mut opts: Vec<O> = vec![];
    let npos: (usize, usize);
    match action {
        "create" => {
            opts.push(o("name", K::Str).req());
            npos = (1, 1);
        }
        "list" => npos = (1, 1),
        "show" => {
            opts.push(o("analysis-id", K::Str));
            npos = (1, 1);
        }
        "save" => {
            opts.push(o("sample-id", K::Str));
            opts.push(o("status", K::Str));
            npos = (2, 2);
        }
        "export" | "import" | "upgrade" => npos = (2, 2),
        "review" => {
            opts.push(o("target-kind", K::Str).ch(&TARGET_KINDS));
            opts.push(o("target-id", K::Str));
            opts.push(o("value", K::Str));
            opts.push(o("actor", K::Str).req());
            opts.push(o("reason", K::Str).req());
            npos = (3, 3);
        }
        "undo" => {
            opts.push(o("actor", K::Str).req());
            opts.push(o("reason", K::Str).req());
            npos = (3, 3);
        }
        "run-create" | "run-revise" | "run-replace-ladder" | "run-remove-ladder" | "run-show" => {
            if action != "run-show" {
                opts.push(o("actor", K::Str).req());
                opts.push(o("reason", K::Str).req());
                if action != "run-create" {
                    opts.push(o("parent-revision", K::Str).req());
                }
            }
            if action == "run-create" {
                opts.push(o("name", K::Str).req());
            }
            npos = match action {
                "run-create" => (2, 2),
                "run-show" => (2, 2),
                "run-revise" => (3, 3),
                "run-remove-ladder" => (3, 3),
                _ => (4, 4),
            };
        }
        other => usage_error("project", &format!("argument project_action: invalid choice: {}", str_repr(other))),
    }
    let mut a = parse(&cmd, rest, npos, &opts);
    if action == "review" {
        // choices for the positional review action
        if !ACTIONS.contains(&a.p(2)) {
            let list: Vec<String> = ACTIONS.iter().map(|c| str_repr(c)).collect();
            usage_error(&cmd, &format!("argument action: invalid choice: {} (choose from {})", str_repr(a.p(2)), list.join(", ")));
        }
    }
    a.flags.clear();
    let result = project_command(&a, action)?;
    println!("{}", dump_indent(&result));
    Ok(0)
}

// ------------------------------------------------------------------------------------------------ main

pub fn main_with(argv: Vec<String>) -> i32 {
    let Some(command) = argv.first().map(|s| s.as_str()) else {
        eprintln!("usage: opentrace [-h] {{serve,demo,inspect,export,size,project,str,str-run,str-run-demo,str-demo,controls,marker-quality,pullup-quality,import-kit}} ...\nopentrace: error: the following arguments are required: command");
        return 2;
    };
    let rest = &argv[1..];
    let mut shared = analyze_options();
    let result: Res<i32> = (|| match command {
        "serve" => {
            let opts = vec![o("port", K::Int), o("open", K::Flag), o("host", K::Str), o("allowed-host", K::Str).multi(), o("projects-dir", K::Str), o("security-dir", K::Str)];
            cmd_serve(&parse(command, rest, (0, 0), &opts))
        }
        "health" => cmd_health(&parse(command, rest, (0, 0), &[o("port", K::Int), o("host", K::Str)])),
        "demo" => cmd_demo(&parse(command, rest, (1, 1), &[o("sizing", K::Flag), o("pipeline", K::Flag)])),
        "inspect" => cmd_inspect(&parse(command, rest, (1, 1), &[o("tags", K::Flag)])),
        "export" => {
            shared.extend([o("output", K::Str).short("-o").req(), o("peaks", K::Flag), o("threshold", K::Float), o("distance", K::Int), o("anchors", K::Str)]);
            cmd_size_export(command, &parse(command, rest, (1, 1), &shared))
        }
        "size" => {
            shared.extend([o("output", K::Str).short("-o"), o("threshold", K::Float), o("distance", K::Int)]);
            cmd_size_export(command, &parse(command, rest, (1, 1), &shared))
        }
        "project" => cmd_project(rest),
        "str" => cmd_str(&parse(command, rest, (1, 1), &[
            o("ladder", K::Str).req(), o("kit", K::Str).req(), o("settings", K::Str).req(),
            o("missing-offset-policy", K::Str).ch(&["reject", "forward_fill"]), o("ladder-quality", K::Str), o("output", K::Str).short("-o"),
        ])),
        "str-run" => cmd_str_run(&parse(command, rest, (1, 1), &[o("output", K::Str).short("-o")])),
        "str-run-demo" => cmd_str_run_demo(&parse(command, rest, (1, 1), &[])),
        "str-demo" => cmd_str_demo(&parse(command, rest, (1, 1), &[o("variant", K::Str).ch(&VARIANTS)])),
        "controls" | "marker-quality" | "pullup-quality" => {
            cmd_annotate(command, &parse(command, rest, (1, 1), &[o("definition", K::Str).req(), o("output", K::Str).short("-o")]))
        }
        "import-kit" => cmd_import_kit(&parse(command, rest, (1, 2), &[o("stutter", K::Str), o("panel", K::Str), o("list", K::Flag), o("ladder-quality", K::Str), o("output", K::Str).short("-o")])),
        other => {
            eprintln!("usage: opentrace [-h] ...\nopentrace: error: argument command: invalid choice: {}", str_repr(other));
            Ok(2)
        }
    })();
    let _ = std::io::stdout().flush();
    match result {
        Ok(code) => code,
        Err(m) => {
            eprintln!("Error: {}", m);
            2
        }
    }
}

#[allow(dead_code)]
fn unused(_: BTreeMap<String, String>) {}

//! Project state and analysis of the native desktop application. The C++/Qt side only displays this state and calls
//! these functions; every number comes from `opentrace-core` in this process (no HTTP, no browser engine).
//!
//! Analysis uses the same entry points as the web UI (`/api/analysis-method` `analyze_settings` and `/api/analyze`),
//! called directly, so a sample analysed here and in the web UI gives the same result.
use opentrace_core::{abif::AbifFile, abif_view, analysis_method, instrument, server};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const SYNTHETIC_STANDARD: &str = "OpenTrace synthetic GS500 (self-test)";

/// Size standards shipped with ID-X 1.6 (published fragment lists; same list as the web UI `sizestd.js`).
pub fn default_size_standards() -> Vec<SizeStandard> {
    let gs600: Vec<f64> = vec![60., 80., 100., 114., 120., 140., 160., 180., 200., 214., 220., 240., 250., 260., 280., 300., 314., 320., 340., 360., 380., 400., 414., 420., 440., 460.];
    let gs500: Vec<f64> = vec![75., 100., 139., 150., 160., 200., 300., 340., 350., 400., 450.];
    let f = |lo: f64, hi: f64| gs600.iter().copied().filter(|&x| x >= lo && x <= hi).collect::<Vec<_>>();
    vec![
        SizeStandard { name: "GS600_LIZ_(60-460)".into(), dye: "orange".into(), sizes: gs600.clone() },
        SizeStandard { name: "GS600_LIZ".into(), dye: "orange".into(), sizes: f(80., 1e9) },
        SizeStandard { name: "GS600_LIZ_(80-400)".into(), dye: "orange".into(), sizes: f(80., 400.) },
        SizeStandard { name: "CE_G5_HID_GS500".into(), dye: "orange".into(), sizes: gs500.clone() },
        SizeStandard { name: "CE_F_HID_GS500 (75-450)".into(), dye: "red".into(), sizes: gs500.clone() },
        SizeStandard { name: "CE_F_HID_GS500 (75-400)".into(), dye: "red".into(), sizes: gs500.iter().copied().filter(|&x| x <= 400.).collect() },
        SizeStandard { name: "GS600_LIZ+Normalization_(60-460)".into(), dye: "orange".into(), sizes: gs600.clone() },
        SizeStandard { name: "GS600_LIZ+Normalization_(80-400)".into(), dye: "orange".into(), sizes: f(80., 400.) },
    ]
}

#[derive(Clone, Debug)]
pub struct SizeStandard {
    pub name: String,
    pub dye: String,
    pub sizes: Vec<f64>,
}

#[derive(Clone, Default)]
pub struct Sample {
    pub path: PathBuf,
    pub file: String,
    pub name: String,
    pub run_folder: String,
    pub method: String,
    pub size_std: String,
    pub status: Status,
    pub error: String,
    pub instrument: String,
    pub well: String,
    pub analysis: Option<Arc<Value>>,
    /// sample | ladder | positive | negative (guessed from the file / sample name like the web UI)
    pub stype: String,
    /// kit + U+0001 + panel ("" = no panel)
    pub panel_key: String,
    pub report: Option<Arc<Value>>,
    pub ladder_quality: Option<i64>,
    pub ladder_used: bool,
    pub reference_bins: bool,
    pub alas_skipped: bool,
    pub genotype_error: String,
    /// label edits (web project format), applied on top of `report` by `edits::effective`
    pub edits: Vec<Value>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Status {
    #[default]
    NotAnalyzed,
    Running,
    Analyzed,
    Failed,
}

#[derive(Default)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub running: bool,
    pub message: String,
    pub phase: String,
}

/// The C++ side owns a `Backend`; worker threads share its state through the `Arc`.
pub struct Backend(pub Arc<Inner>);

impl std::ops::Deref for Backend {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

pub struct Inner {
    pub data_dir: Mutex<Option<PathBuf>>,
    pub resources: PathBuf,
    pub methods: Mutex<Vec<Value>>,
    pub standards: Mutex<Vec<SizeStandard>>,
    pub samples: Arc<Mutex<Vec<Sample>>>,
    pub progress: Arc<Mutex<Progress>>,
    pub stop: Arc<AtomicBool>,
    pub load_problem: String,
    pub last_error: Mutex<String>,
    /// names of the methods imported by the user (saved in library.json; the shipped ones are not)
    pub user_methods: Mutex<std::collections::BTreeSet<String>>,
    pub kits: Mutex<crate::kits::KitDb>,
    pub kit_cache: Mutex<std::collections::HashMap<String, Arc<crate::genotype::KitEntry>>>,
    pub options: Mutex<crate::genotype::Options>,
    pub ask: crate::genotype::Ask,
    pub plot_settings: Mutex<crate::plotsettings::Db>,
}

impl Backend {
    pub fn new() -> Backend {
        Backend(Arc::new(Inner::new()))
    }
}

impl Inner {
    pub fn new() -> Inner {
        let (resources, mut problem) = match server::resource_root(None) {
            Ok(p) => (p, String::new()),
            Err(e) => (PathBuf::from("resources"), e),
        };
        let shipped = resources.join("data").join("shipped_methods.json");
        let mut methods = Vec::new();
        if let Ok(v) = analysis_method::method_request(&json!({"action": "factory"}), &shipped) {
            if let Some(m) = v.get("method") {
                methods.push(m.clone());
            }
        }
        match analysis_method::method_request(&json!({"action": "shipped"}), &shipped) {
            Ok(v) => methods.extend(v["methods"].as_array().cloned().unwrap_or_default()),
            Err(e) => {
                if problem.is_empty() {
                    problem = e;
                }
            }
        }
        Inner {
            kits: Mutex::new(Default::default()),
            kit_cache: Mutex::new(Default::default()),
            options: Mutex::new(Default::default()),
            ask: Default::default(),
            plot_settings: Mutex::new(Default::default()),
            data_dir: Mutex::new(None),
            resources,
            methods: Mutex::new(methods),
            standards: Mutex::new(default_size_standards()),
            samples: Arc::new(Mutex::new(Vec::new())),
            progress: Arc::new(Mutex::new(Progress::default())),
            stop: Arc::new(AtomicBool::new(false)),
            load_problem: problem,
            last_error: Mutex::new(String::new()),
            user_methods: Mutex::new(Default::default()),
        }
    }

    /// User library (imported analysis methods and size standards) of this computer: `<data dir>/library.json`.
    pub fn load_library(&self, dir: &str) {
        let d = PathBuf::from(dir);
        *self.data_dir.lock().unwrap() = Some(d.clone());
        let Ok(b) = std::fs::read(d.join("library.json")) else { return };
        let Ok(v) = serde_json::from_slice::<Value>(&b) else { return };
        let mut ms = self.methods.lock().unwrap();
        for m in v["methods"].as_array().into_iter().flatten() {
            if let Some(name) = m["name"].as_str() {
                ms.retain(|x| x["name"] != name);
                ms.push(m.clone());
                self.user_methods.lock().unwrap().insert(name.to_string());
            }
        }
        *self.kits.lock().unwrap() = crate::kits::KitDb::from_json(&v["kits"]);
        if v["plot_settings"].is_object() {
            *self.plot_settings.lock().unwrap() = crate::plotsettings::Db::from_json(&v["plot_settings"]);
        }
        if let Some(o) = v["options"].as_object() {
            let mut op = self.options.lock().unwrap();
            if let Some(a) = o.get("alas").and_then(|v| v.as_str()).filter(|a| ["stop", "passing", "all"].contains(a)) {
                op.alas = a.into();
            }
            if let Some(a) = o.get("bin_offsets").and_then(|v| v.as_str()).filter(|a| ["green", "greenYellow"].contains(a)) {
                op.bin_offsets = a.into();
            }
            if let Some(b) = o.get("pullup_excludes_calls").and_then(|v| v.as_bool()) {
                op.pullup_excludes_calls = b;
            }
            if let Some(x) = o.get("homo_min_height").and_then(|v| v.as_f64()).filter(|x| *x >= 0.0) {
                op.homo_min_height = x;
            }
            if let Some(x) = o.get("het_min_height").and_then(|v| v.as_f64()).filter(|x| *x >= 0.0) {
                op.het_min_height = x;
            }
        }
        let mut ss = self.standards.lock().unwrap();
        for s in v["size_standards"].as_array().into_iter().flatten() {
            let st = SizeStandard {
                name: s["name"].as_str().unwrap_or("").into(),
                dye: s["dye"].as_str().unwrap_or("orange").into(),
                sizes: s["sizes"].as_array().map(|a| a.iter().filter_map(|x| x.as_f64()).collect()).unwrap_or_default(),
            };
            if !st.name.is_empty() && !st.sizes.is_empty() {
                ss.retain(|x| x.name != st.name);
                ss.push(st);
            }
        }
    }

    pub fn save_library(&self) -> Result<(), String> {
        let Some(d) = self.data_dir.lock().unwrap().clone() else { return Ok(()) };
        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        let defaults: Vec<String> = default_size_standards().into_iter().map(|s| s.name).collect();
        let options = {
            let o = self.options.lock().unwrap();
            json!({"alas": o.alas, "bin_offsets": o.bin_offsets, "pullup_excludes_calls": o.pullup_excludes_calls, "homo_min_height": o.homo_min_height, "het_min_height": o.het_min_height})
        };
        let user_methods: Vec<Value> = {
            let user = self.user_methods.lock().unwrap();
            self.methods.lock().unwrap().iter().filter(|m| m["name"].as_str().is_some_and(|n| user.contains(n))).cloned().collect()
        };
        let v = json!({
            "schema_version": 1,
            "methods": user_methods,
            "size_standards": self.standards.lock().unwrap().iter().filter(|s| !defaults.contains(&s.name)).map(|s| json!({"name": s.name, "dye": s.dye, "sizes": s.sizes})).collect::<Vec<_>>(),
            "kits": self.kits.lock().unwrap().to_json(),
            "options": options,
            "plot_settings": self.plot_settings.lock().unwrap().to_json(),
        });
        let tmp = d.join("library.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&v).unwrap()).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, d.join("library.json")).map_err(|e| e.to_string())
    }

    /// GeneMapper analysis method XML -> core method (validated by the core, `import_xml`); returns its name.
    pub fn import_method_xml(&self, path: &str) -> Result<String, String> {
        let xml = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let shipped = self.resources.join("data").join("shipped_methods.json");
        let r = analysis_method::method_request(&json!({"action": "import_xml", "xml": xml}), &shipped)?;
        let m = r["method"].clone();
        let name = m["name"].as_str().unwrap_or("").to_string();
        if name.is_empty() {
            return Err("method without a name".into());
        }
        self.user_methods.lock().unwrap().insert(name.clone());
        {
            let mut ms = self.methods.lock().unwrap();
            ms.retain(|x| x["name"] != name.as_str());
            ms.push(m);
        }
        self.save_library()?;
        Ok(name)
    }

    /// GeneMapper SizeStandardContainer XML (as the web `sizestd.js` reads it); returns its name.
    pub fn import_size_standard_xml(&self, path: &str) -> Result<String, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        if text.len() > 1 << 20 || text.contains("<!DOCTYPE") || text.contains("<!ENTITY") {
            return Err("not a size standard XML".into());
        }
        let doc = roxmltree::Document::parse(&text).map_err(|e| e.to_string())?;
        let node = doc.descendants().find(|n| n.has_tag_name("xmlSizeStandard")).ok_or("no xmlSizeStandard element")?;
        let one = |tag: &str| node.descendants().find(|n| n.has_tag_name(tag)).and_then(|n| n.text()).map(|t| t.trim().to_string());
        let dyes = ["blue", "green", "yellow", "red", "orange", "purple"];
        let dye_index: usize = one("dyeIndex").and_then(|v| v.parse().ok()).unwrap_or(5);
        let sizes: Vec<f64> = node.descendants().filter(|n| n.has_tag_name("sizeStdDefinition")).filter_map(|n| n.text()?.trim().parse().ok()).collect();
        if sizes.len() < 2 {
            return Err("size standard without fragments".into());
        }
        let st = SizeStandard { name: one("sizeStandardName").unwrap_or_else(|| "Imported".into()), dye: dyes.get(dye_index.wrapping_sub(1)).unwrap_or(&"orange").to_string(), sizes };
        let name = st.name.clone();
        {
            let mut ss = self.standards.lock().unwrap();
            ss.retain(|x| x.name != name);
            ss.push(st);
        }
        self.save_library()?;
        Ok(name)
    }

    /// Invented run files of the core (`demo::synthetic_genotype_fsa`, `synthetic_ladder_fsa`; no laboratory data)
    /// for the scripted self-test; also registers their size standard. Returns the written paths.
    pub fn synthetic_files(&self, dir: &str, n: usize) -> Result<Vec<String>, String> {
        let d = PathBuf::from(dir);
        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        let ladder = d.join("Ladder-synthetic.fsa");
        std::fs::write(&ladder, opentrace_core::demo::synthetic_ladder_fsa()).map_err(|e| e.to_string())?;
        out.push(ladder.display().to_string());
        let mut expected = serde_json::Map::new();
        for k in 0..n {
            let file = format!("Sample-synthetic-{:02}.fsa", k + 1);
            let p = d.join(&file);
            let (bytes, genotype) = opentrace_core::demo::synthetic_genotype_fsa(k as u64 + 1);
            std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
            expected.insert(file, Value::Object(genotype.into_iter().map(|(m, l)| (m, json!(l))).collect()));
            out.push(p.display().to_string());
        }
        // the genotypes the invented files were made with (the self-test compares the calls with them)
        std::fs::write(d.join("expected.json"), serde_json::to_vec_pretty(&expected).unwrap()).map_err(|e| e.to_string())?;
        let (_, _, panels, bins, stutter) = opentrace_core::demo::synthetic_kit_texts();
        let key = self.import_kit_texts(&panels, &bins, &stutter)?;
        *self.last_error.lock().unwrap() = key; // panel key of the synthetic kit for the self-test
        let mut ss = self.standards.lock().unwrap();
        ss.retain(|s| s.name != SYNTHETIC_STANDARD);
        ss.push(SizeStandard { name: SYNTHETIC_STANDARD.into(), dye: "orange".into(), sizes: opentrace_core::demo::SYNTHETIC_RUN_STANDARD_BP.to_vec() });
        Ok(out)
    }

    /// GeneMapper panels / bins / stutter text file into the Panel Manager store (parsed by the core); returns a
    /// description of what was imported. Bins and stutter go to the kit named in the file when it is known, else to
    /// `kit_hint` (case-insensitive), else to the only kit.
    pub fn import_kit_file(&self, path: &str, kind: &str, kit_hint: &str) -> Result<String, String> {
        let text = std::fs::read(path).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&text).into_owned();
        let shipped = self.resources.join("data").join("shipped_methods.json");
        let key = match kind {
            "panels" | "bins" | "stutter" => kind,
            _ => return Err("unknown kit file kind".into()),
        };
        let r = server::api_json("/api/import-gm-kit", &json!({ key: text }), &shipped)?;
        let doc = &r[key];
        if doc.is_null() {
            return Err("the file was not recognised".into());
        }
        let msg = {
            let mut db = self.kits.lock().unwrap();
            let pick = |db: &crate::kits::KitDb, named: &str| -> String {
                let names: Vec<String> = db.kits.iter().filter_map(|k| k["name"].as_str().map(String::from)).collect();
                for cand in [named, kit_hint] {
                    if cand.is_empty() {
                        continue;
                    }
                    if let Some(n) = names.iter().find(|n| n.as_str() == cand) {
                        return n.clone();
                    }
                    if let Some(n) = names.iter().find(|n| n.eq_ignore_ascii_case(cand)) {
                        return n.clone();
                    }
                }
                if names.len() == 1 { names[0].clone() } else { kit_hint.to_string() }
            };
            match kind {
                "panels" => format!("панели: {}", db.add_panels(doc).join(", ")),
                "bins" => {
                    let k = pick(&db, doc["kit"].as_str().unwrap_or(""));
                    let (kit, set) = db.add_bin_set(&k, doc)?;
                    format!("набор бинов {set} → {kit}")
                }
                _ => {
                    let k = pick(&db, doc["kit"].as_str().unwrap_or(""));
                    format!("статтеры → {}", db.set_stutter(&k, doc)?)
                }
            }
        };
        self.kit_cache.lock().unwrap().clear();
        self.save_library()?;
        Ok(msg)
    }

    /// Imports kit texts (panels, bins, stutter) given as strings; returns the key of the first panel.
    pub fn import_kit_texts(&self, panels: &str, bins: &str, stutter: &str) -> Result<String, String> {
        let shipped = self.resources.join("data").join("shipped_methods.json");
        let r = server::api_json("/api/import-gm-kit", &json!({"panels": panels}), &shipped)?;
        let b = server::api_json("/api/import-gm-kit", &json!({"bins": bins}), &shipped)?;
        let st = server::api_json("/api/import-gm-kit", &json!({"stutter": stutter}), &shipped)?;
        let mut db = self.kits.lock().unwrap();
        db.add_panels(&r["panels"]);
        let kit = r["panels"]["kits"][0]["name"].as_str().unwrap_or("").to_string();
        db.add_bin_set(&kit, &b["bins"])?;
        db.set_stutter(&kit, &st["stutter"])?;
        let key = db.choices().into_iter().find(|c| c.kit == kit).map(|c| c.key).ok_or("no panel")?;
        drop(db);
        self.kit_cache.lock().unwrap().clear();
        Ok(key)
    }

    pub fn method_names(&self) -> Vec<String> {
        self.methods.lock().unwrap().iter().filter_map(|m| m["name"].as_str().map(String::from)).collect()
    }

    /// Reads the files (sample name, well, instrument from the ABIF header); unreadable files are reported, not added.
    pub fn add_files(&self, paths: &[String], run_folder: &str, method: &str, size_std: &str, panel_key: &str) -> Vec<String> {
        let mut errors = Vec::new();
        let mut list = self.samples.lock().unwrap();
        for p in paths {
            let path = PathBuf::from(p);
            let file = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| p.clone());
            if list.iter().any(|s| s.path == path) {
                continue;
            }
            let data = match std::fs::read(&path) {
                Ok(d) => d,
                Err(e) => {
                    errors.push(format!("{file}: {e}"));
                    continue;
                }
            };
            let (name, well, inst) = match AbifFile::parse(&data, &file) {
                Ok(a) => {
                    let ri = abif_view::run_info(&a).unwrap_or(Value::Null);
                    let ii = instrument::instrument_info(&a);
                    let inst = [ii["model"].as_str(), ii["serial"].as_str()].iter().flatten().cloned().collect::<Vec<_>>().join(" ");
                    (ri["sample_name"].as_str().unwrap_or("").to_string(), ri["well"].as_str().unwrap_or("").to_string(), inst)
                }
                Err(e) => {
                    errors.push(format!("{file}: {e}"));
                    continue;
                }
            };
            let mut stype = crate::genotype::sample_type_from_name(&file);
            if stype == "sample" && !name.is_empty() {
                stype = crate::genotype::sample_type_from_name(&name);
            }
            list.push(Sample {
                stype: stype.to_string(),
                panel_key: panel_key.to_string(),
                path,
                file,
                name,
                run_folder: run_folder.to_string(),
                method: method.to_string(),
                size_std: size_std.to_string(),
                well,
                instrument: inst,
                ..Default::default()
            });
        }
        errors
    }

    /// `/api/analysis-method` `analyze_settings` -> query parameters of `/api/analyze` (as the web UI sends them).
    pub fn analyze_params(&self, method: &str, size_std: &str) -> Result<Vec<(String, String)>, String> {
        let m = self.methods.lock().unwrap().iter().find(|m| m["name"] == method).cloned().ok_or_else(|| format!("no analysis method {method}"))?;
        let std = self.standards.lock().unwrap().iter().find(|s| s.name == size_std).cloned().ok_or_else(|| format!("no size standard {size_std}"))?;
        let shipped = self.resources.join("data").join("shipped_methods.json");
        let r = analysis_method::method_request(&json!({"action": "analyze_settings", "method": &m, "standard_bp": std.sizes, "standard_dye": std.dye}), &shipped)?;
        let mut q = Vec::new();
        if let Some(o) = r["settings"].as_object() {
            for (k, v) in o {
                let s = match v {
                    Value::Null => continue,
                    Value::String(s) => s.clone(),
                    Value::Object(_) | Value::Array(_) => v.to_string(),
                    other => other.to_string(),
                };
                q.push((k.clone(), s));
            }
        }
        Ok(q)
    }
}

impl Backend {
    /// Analyses the samples (all, or those not analysed yet) on worker threads, then genotypes the project (run folder
    /// + panel groups, allelic ladders first) as the web project window does; `progress` reports the state.
    pub fn run_analysis(&self, all: bool) {
        {
            let mut p = self.progress.lock().unwrap();
            if p.running {
                return;
            }
            *p = Progress { running: true, phase: "analysis".into(), ..Default::default() };
        }
        self.stop.store(false, Ordering::SeqCst);
        let inner = self.0.clone();
        std::thread::spawn(move || {
            inner.analysis_phase(all);
            if !inner.stop.load(Ordering::SeqCst) {
                crate::genotype::genotype(&inner);
            }
            let mut list = inner.samples.lock().unwrap();
            for s in list.iter_mut().filter(|s| s.status == Status::Running) {
                s.status = Status::NotAnalyzed;
            }
            let mut p = inner.progress.lock().unwrap();
            p.running = false;
            p.phase.clear();
            if inner.stop.load(Ordering::SeqCst) {
                p.message = "stopped".into();
            }
        });
    }
}

impl Inner {
    /// Number of parallel jobs (web UI: max(2, min(6, cores))).
    pub fn concurrency() -> usize {
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 6)
    }

    fn analysis_phase(&self, all: bool) {
        let todo: Vec<usize> = {
            let mut list = self.samples.lock().unwrap();
            // ladders first (as the web UI orders them)
            let mut idx: Vec<usize> = list.iter().enumerate().filter(|(_, s)| all || s.status != Status::Analyzed).map(|(i, _)| i).collect();
            idx.sort_by_key(|&i| if list[i].stype == "ladder" { 0 } else { 1 });
            for &i in &idx {
                list[i].status = Status::Running;
                for e in list[i].edits.iter_mut().filter(|e| !e["undone"].as_bool().unwrap_or(false)) {
                    e["undone"] = json!(true);
                    e["undoneBy"] = json!("reanalysis");
                }
            }
            idx
        };
        self.progress.lock().unwrap().total = todo.len();
        let jobs: Vec<(usize, PathBuf, String, Result<Vec<(String, String)>, String>)> = {
            let list = self.samples.lock().unwrap();
            todo.iter().map(|&i| (i, list[i].path.clone(), list[i].file.clone(), self.analyze_params(&list[i].method, &list[i].size_std))).collect()
        };
        let queue = Mutex::new(jobs.into_iter().rev().collect::<Vec<_>>());
        std::thread::scope(|sc| {
            for _ in 0..Self::concurrency() {
                sc.spawn(|| loop {
                    if self.stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let job = queue.lock().unwrap().pop();
                    let Some((i, path, file, params)) = job else { break };
                    self.progress.lock().unwrap().message = file.clone();
                    let result = params.and_then(|mut p| {
                        p.push(("filename".into(), file.clone()));
                        p.push(("slim".into(), "1".into()));
                        let data = std::fs::read(&path).map_err(|e| e.to_string())?;
                        server::analyze_request(&data, &p)
                    });
                    {
                        let mut list = self.samples.lock().unwrap();
                        if let Some(s) = list.get_mut(i) {
                            s.report = None;
                            s.ladder_quality = None;
                            match result {
                                Ok(v) => {
                                    // samples without a name in the file keep the type guessed from the file name
                                    s.analysis = Some(Arc::new(v));
                                    s.status = Status::Analyzed;
                                    s.error.clear();
                                }
                                Err(e) => {
                                    s.analysis = None;
                                    s.status = Status::Failed;
                                    s.error = e;
                                }
                            }
                        }
                    }
                    self.progress.lock().unwrap().done += 1;
                });
            }
        });
    }
}

/// SQ of an analysed sample as the web project window computes it (`samplePqv`): override, else the post-sizing
/// (or base) quality score of an accepted sizing in the ID-X ranges (pass >= 0.75, low <= 0.25, check between),
/// "low" for any other sizing status, "na" without sizing.
pub fn sizing_quality(a: &Value) -> &'static str {
    let sz = &a["sizing"];
    if !sz["sq_override"].is_null() && sz["sq_override"] != json!(false) {
        return "override";
    }
    match sz["status"].as_str() {
        Some("accepted") => {
            let q = if sz["post_sizing_quality"].is_object() { &sz["post_sizing_quality"] } else { &sz["base_quality"] };
            let v = q["score"].as_f64().unwrap_or(1.0);
            if v >= 0.75 {
                "pass"
            } else if v <= 0.25 {
                "low"
            } else {
                "check"
            }
        }
        Some(_) => "low",
        None => "na",
    }
}

/// Calibration curve (scan, bp) sorted by scan, as the web plot window uses it (`curveOf`).
pub fn curve(a: &Value) -> Vec<(f64, f64)> {
    let mut c: Vec<(f64, f64)> = a["sizing"]["anchors"]
        .as_array()
        .map(|v| v.iter().filter_map(|p| Some((p.get(0)?.as_f64()?, p.get(1)?.as_f64()?))).collect())
        .unwrap_or_default();
    c.sort_by(|x, y| x.0.total_cmp(&y.0));
    c
}

/// scan -> bp by linear interpolation between anchors (extrapolated with the end segments), like `scanToBp`.
pub fn scan_to_bp(c: &[(f64, f64)], s: f64) -> f64 {
    if c.len() < 2 {
        return s;
    }
    let mut i = 1;
    while i < c.len() - 1 && s > c[i].0 {
        i += 1;
    }
    let (s0, b0) = c[i - 1];
    let (s1, b1) = c[i];
    b0 + (s - s0) * (b1 - b0) / (s1 - s0)
}

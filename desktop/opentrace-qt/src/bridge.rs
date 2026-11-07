//! Rust <-> C++ interface of the native window code (plain CXX bridge compiled by cxx-qt-build together with the Qt
//! Widgets sources in `cpp/`). The C++ side holds a `Box<Backend>` and reads plain rows and traces from it.
use crate::backend::{self, Backend, Status};

#[cxx::bridge(namespace = "ot")]
pub mod ffi {
    /// One row of the Samples table.
    struct SampleRow {
        file: String,
        name: String,
        run_folder: String,
        status: String,
        sq: String,
        method: String,
        size_std: String,
        instrument: String,
        well: String,
        error: String,
        stype: String,
        panel: String,
        sos: String,
        sspk: String,
        cgq: String,
    }
    /// One row of the Genotypes table (one marker of one sample).
    struct GenoRow {
        sample: usize,
        file: String,
        name: String,
        marker: String,
        dye: String,
        alleles: Vec<String>,
        sizes: Vec<f64>,
        heights: Vec<f64>,
        gq: String,
    }
    struct PlotMarker {
        name: String,
        tag: String,
        min_bp: f64,
        max_bp: f64,
        gq: String,
        window: Vec<f64>,
        /// per bin: low bp, high bp, 1 = virtual (after this run's bin offset)
        bins: Vec<f64>,
    }
    struct PlotLabel {
        tag: String,
        x: f64,
        height: f64,
        text: String,
        flagged: bool,
    }
    struct PanelOption {
        key: String,
        label: String,
    }
    /// One electropherogram channel: x in bp (sized) or data points, y in RFU.
    struct Trace {
        tag: String,
        dye: String,
        xs: Vec<f64>,
        ys: Vec<f64>,
    }
    struct PeakMark {
        tag: String,
        x: f64,
        height: f64,
        offscale: bool,
    }
    struct ProgressInfo {
        done: usize,
        total: usize,
        running: bool,
        message: String,
        phase: String,
    }

    extern "Rust" {
        type Backend;
        fn new_backend() -> Box<Backend>;
        fn load_problem(self: &Backend) -> String;
        fn resources_dir(self: &Backend) -> String;
        fn method_list(self: &Backend) -> Vec<String>;
        fn size_standard_list(self: &Backend) -> Vec<String>;
        fn add_sample_files(self: &Backend, paths: &Vec<String>, run_folder: &str, method: &str, size_std: &str, panel_key: &str) -> Vec<String>;
        fn panel_options(self: &Backend) -> Vec<PanelOption>;
        fn import_kit(self: &Backend, path: &str, kind: &str, kit_hint: &str) -> String;
        fn kit_summary(self: &Backend) -> Vec<String>;
        fn delete_kit(self: &Backend, name: &str);
        fn set_samples_panel(self: &Backend, rows: &Vec<usize>, panel_key: &str);
        fn set_samples_type(self: &Backend, rows: &Vec<usize>, stype: &str);
        fn genotype_rows(self: &Backend) -> Vec<GenoRow>;
        fn ask_pending(self: &Backend) -> String;
        fn ask_answer(self: &Backend, answer: &str);
        fn option_get(self: &Backend, key: &str) -> String;
        fn option_set(self: &Backend, key: &str, value: &str) -> bool;
        fn genotype_digest(self: &Backend) -> String;
        fn plot_markers(self: &Backend, i: usize) -> Vec<PlotMarker>;
        /// "" on success, else the error text
        fn save_project_file(self: &Backend, path: &str) -> String;
        fn open_project_file(self: &Backend, path: &str) -> String;
        fn plot_labels(self: &Backend, i: usize, bp: bool) -> Vec<PlotLabel>;
        fn sample_count(self: &Backend) -> usize;
        fn sample_row(self: &Backend, i: usize) -> SampleRow;
        fn remove_samples(self: &Backend, rows: &Vec<usize>);
        fn start_analysis(self: &Backend, all: bool);
        fn stop_analysis(self: &Backend);
        fn analysis_progress(self: &Backend) -> ProgressInfo;
        fn sample_traces(self: &Backend, i: usize, bp: bool) -> Vec<Trace>;
        fn sample_peaks(self: &Backend, i: usize, bp: bool) -> Vec<PeakMark>;
        fn sized_range(self: &Backend, i: usize) -> Vec<f64>;
        fn sample_info_text(self: &Backend, i: usize) -> String;
        fn core_version() -> String;
        fn set_data_dir(self: &Backend, dir: &str);
        fn import_method(self: &Backend, path: &str) -> String;
        fn import_size_standard(self: &Backend, path: &str) -> String;
        fn last_import_error(self: &Backend) -> String;
        fn autotest_synthetic(self: &Backend, dir: &str, n: usize) -> Vec<String>;
        fn synthetic_standard_name() -> String;
    }

    unsafe extern "C++" {
        include!("opentrace-qt/cpp/app.h");
        fn run_app() -> i32;
    }
}

fn new_backend() -> Box<Backend> {
    Box::new(Backend::new())
}

fn synthetic_standard_name() -> String {
    backend::SYNTHETIC_STANDARD.to_string()
}

fn core_version() -> String {
    opentrace_core::server_analyze::analysis_version()
}

impl Backend {
    fn set_data_dir(&self, dir: &str) {
        self.load_library(dir)
    }
    fn load_problem(&self) -> String {
        self.load_problem.clone()
    }
    fn resources_dir(&self) -> String {
        self.resources.display().to_string()
    }
    fn method_list(&self) -> Vec<String> {
        self.method_names()
    }
    fn size_standard_list(&self) -> Vec<String> {
        self.standards.lock().unwrap().iter().map(|s| s.name.clone()).collect()
    }
    /// Imported name, or "" with the reason in last_import_error().
    fn import_method(&self, path: &str) -> String {
        self.import_result(self.import_method_xml(path))
    }
    fn import_size_standard(&self, path: &str) -> String {
        self.import_result(self.import_size_standard_xml(path))
    }
    fn import_result(&self, r: Result<String, String>) -> String {
        match r {
            Ok(n) => {
                self.last_error.lock().unwrap().clear();
                n
            }
            Err(e) => {
                *self.last_error.lock().unwrap() = e;
                String::new()
            }
        }
    }
    fn autotest_synthetic(&self, dir: &str, n: usize) -> Vec<String> {
        self.synthetic_files(dir, n).unwrap_or_default()
    }
    fn last_import_error(&self) -> String {
        self.last_error.lock().unwrap().clone()
    }
    fn add_sample_files(&self, paths: &Vec<String>, run_folder: &str, method: &str, size_std: &str, panel_key: &str) -> Vec<String> {
        self.add_files(paths, run_folder, method, size_std, panel_key)
    }
    fn panel_options(&self) -> Vec<ffi::PanelOption> {
        self.kits.lock().unwrap().choices().into_iter().map(|c| ffi::PanelOption { label: format!("{} ({})", c.panel, c.kit), key: c.key }).collect()
    }
    fn import_kit(&self, path: &str, kind: &str, kit_hint: &str) -> String {
        self.import_result(self.import_kit_file(path, kind, kit_hint))
    }
    fn kit_summary(&self) -> Vec<String> {
        self.kits.lock().unwrap().summary()
    }
    fn delete_kit(&self, name: &str) {
        self.kits.lock().unwrap().remove_kit(name);
        self.kit_cache.lock().unwrap().clear();
        let _ = self.save_library();
    }
    fn set_samples_panel(&self, rows: &Vec<usize>, panel_key: &str) {
        let mut list = self.samples.lock().unwrap();
        for &i in rows {
            if let Some(s) = list.get_mut(i) {
                if s.panel_key != panel_key {
                    s.panel_key = panel_key.to_string();
                    s.report = None;
                }
            }
        }
    }
    fn set_samples_type(&self, rows: &Vec<usize>, stype: &str) {
        if !["sample", "ladder", "positive", "negative"].contains(&stype) {
            return;
        }
        let mut list = self.samples.lock().unwrap();
        for &i in rows {
            if let Some(s) = list.get_mut(i) {
                s.stype = stype.to_string();
            }
        }
    }
    fn genotype_rows(&self) -> Vec<ffi::GenoRow> {
        let list = self.samples.lock().unwrap();
        let mut out = Vec::new();
        for (i, s) in list.iter().enumerate() {
            let Some(rep) = &s.report else { continue };
            if rep["allele_calling"].is_null() {
                continue;
            }
            let gq: std::collections::HashMap<String, &serde_json::Value> =
                rep["quality"]["marker_gq"]["markers"].as_array().into_iter().flatten().filter_map(|m| Some((m["marker"].as_str()?.to_string(), m))).collect();
            for m in rep["allele_calling"]["markers"].as_array().into_iter().flatten() {
                let name = m["name"].as_str().unwrap_or("");
                let calls: Vec<&serde_json::Value> = rep["calls"].as_array().into_iter().flatten().filter(|c| c["marker"] == name && !c["label"].is_null()).collect();
                out.push(ffi::GenoRow {
                    sample: i,
                    file: s.file.clone(),
                    name: s.name.clone(),
                    marker: name.to_string(),
                    dye: m["channel"].as_str().unwrap_or("").to_string(),
                    alleles: calls.iter().map(|c| crate::kits::js_num(&c["label"])).collect(),
                    sizes: calls.iter().map(|c| c["size_bp"].as_f64().unwrap_or(0.0)).collect(),
                    heights: calls.iter().map(|c| c["height"].as_f64().unwrap_or(0.0)).collect(),
                    gq: if s.stype == "ladder" { "na".into() } else { gq.get(name).map(|g| crate::genotype::code(&g["category"]["code"]).to_string()).unwrap_or_else(|| "na".into()) },
                });
            }
        }
        out
    }
    /// file:marker=allele,... per sample (the same digest the browser tests print) for comparisons with the web UI.
    fn genotype_digest(&self) -> String {
        let list = self.samples.lock().unwrap();
        list.iter()
            .map(|s| {
                let calls: Vec<String> = s.report.as_ref().and_then(|r| r["calls"].as_array().cloned()).unwrap_or_default().iter().filter(|c| !c["label"].is_null()).map(|c| format!("{}={}", c["marker"].as_str().unwrap_or(""), crate::kits::js_num(&c["label"]))).collect();
                format!("{}:{}", s.file, calls.join(","))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn save_project_file(&self, path: &str) -> String {
        self.save_project(path).err().unwrap_or_default()
    }
    fn open_project_file(&self, path: &str) -> String {
        self.open_project(path).err().unwrap_or_default()
    }
    /// Marker ranges, GQ and bins of the sample's kit (bins moved by the bin offsets of its run, as the plot window does).
    fn plot_markers(&self, i: usize) -> Vec<ffi::PlotMarker> {
        let s = match self.samples.lock().unwrap().get(i) {
            Some(s) => s.clone(),
            None => return Vec::new(),
        };
        if s.panel_key.is_empty() {
            return Vec::new();
        }
        // kit of the sample's panel and method (built from the Panel Manager store when the project was just opened)
        let Ok(k) = self.kit_for(&s.panel_key, &s.method) else { return Vec::new() };
        let rep = s.report.clone();
        let calling = |name: &str| -> Option<serde_json::Value> {
            let ms = &rep.as_ref()?["allele_calling"]["markers"];
            match ms {
                serde_json::Value::Array(a) => a.iter().find(|m| m["name"] == name).cloned(),
                serde_json::Value::Object(o) => o.get(name).cloned().or_else(|| o.values().find(|m| m["name"] == name).cloned()),
                _ => None,
            }
        };
        let gq: std::collections::HashMap<String, String> = rep
            .as_ref()
            .and_then(|r| r["quality"]["marker_gq"]["markers"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|m| Some((m["marker"].as_str()?.to_string(), crate::genotype::code(&m["category"]["code"]).to_string())))
            .collect();
        let mut out = Vec::new();
        for m in k.kit["markers"].as_array().into_iter().flatten() {
            let name = m["name"].as_str().unwrap_or("");
            let mc = calling(name);
            let off = mc.as_ref().map(|c| c["offsets"]["by_bin"].clone()).unwrap_or(serde_json::Value::Null);
            let mut bins = Vec::new();
            for b in m["bins"].as_array().into_iter().flatten() {
                let label = crate::kits::js_num(&b["label"]);
                let d = off[label.as_str()]["offset_bp"].as_f64().unwrap_or(0.0);
                let c = b["center_bp"].as_f64().unwrap_or(0.0);
                bins.push(c - d - b["left_bp"].as_f64().unwrap_or(0.5));
                bins.push(c - d + b["right_bp"].as_f64().unwrap_or(0.5));
                bins.push(if b["virtual"].as_bool().unwrap_or(false) { 1.0 } else { 0.0 });
            }
            let window = mc.as_ref().and_then(|c| c["window_bp"].as_array().map(|w| w.iter().filter_map(|x| x.as_f64()).collect::<Vec<_>>())).unwrap_or_default();
            out.push(ffi::PlotMarker {
                name: name.to_string(),
                tag: m["channel"].as_str().unwrap_or("").to_string(),
                min_bp: m["min_bp"].as_f64().unwrap_or(0.0),
                max_bp: m["max_bp"].as_f64().unwrap_or(0.0),
                gq: gq.get(name).cloned().unwrap_or_else(|| "na".into()),
                window,
                bins,
            });
        }
        out
    }
    /// Allele labels of the sample (call label at the call size; ladders: matched ladder alleles).
    fn plot_labels(&self, i: usize, bp: bool) -> Vec<ffi::PlotLabel> {
        let Some(s) = self.samples.lock().unwrap().get(i).cloned() else { return Vec::new() };
        let Some(rep) = s.report else { return Vec::new() };
        let c = s.analysis.as_ref().map(|a| backend::curve(a)).unwrap_or_default();
        let _ = &c;
        rep["calls"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|x| !x["label"].is_null())
            .map(|x| ffi::PlotLabel {
                tag: x["channel"].as_str().unwrap_or("").to_string(),
                x: if bp { x["size_bp"].as_f64().unwrap_or(0.0) } else { x["scan"].as_f64().unwrap_or(0.0) },
                height: x["height"].as_f64().unwrap_or(0.0),
                text: crate::kits::js_num(&x["label"]),
                flagged: x["flags"].as_array().is_some_and(|f| !f.is_empty()),
            })
            .collect()
    }
    fn ask_pending(&self) -> String {
        self.ask.pending().unwrap_or_default()
    }
    fn ask_answer(&self, answer: &str) {
        self.ask.answer(answer)
    }
    fn option_get(&self, key: &str) -> String {
        let o = self.options.lock().unwrap();
        match key {
            "alas" => o.alas.clone(),
            "bin_offsets" => o.bin_offsets.clone(),
            "pullup_excludes_calls" => o.pullup_excludes_calls.to_string(),
            "homo_min_height" => o.homo_min_height.to_string(),
            "het_min_height" => o.het_min_height.to_string(),
            _ => String::new(),
        }
    }
    fn option_set(&self, key: &str, value: &str) -> bool {
        {
            let mut o = self.options.lock().unwrap();
            match key {
                "alas" if ["stop", "passing", "all"].contains(&value) => o.alas = value.into(),
                "bin_offsets" if ["green", "greenYellow"].contains(&value) => o.bin_offsets = value.into(),
                "pullup_excludes_calls" => o.pullup_excludes_calls = value == "true",
                "homo_min_height" => match value.parse::<f64>() { Ok(x) if x >= 0.0 => o.homo_min_height = x, _ => return false },
                "het_min_height" => match value.parse::<f64>() { Ok(x) if x >= 0.0 => o.het_min_height = x, _ => return false },
                _ => return false,
            }
        }
        let _ = self.save_library();
        true
    }
    fn sample_count(&self) -> usize {
        self.samples.lock().unwrap().len()
    }
    fn sample_row(&self, i: usize) -> ffi::SampleRow {
        let list = self.samples.lock().unwrap();
        let Some(s) = list.get(i) else {
            let e = String::new;
            return ffi::SampleRow { file: e(), name: e(), run_folder: e(), status: e(), sq: e(), method: e(), size_std: e(), instrument: e(), well: e(), error: e(), stype: e(), panel: e(), sos: e(), sspk: e(), cgq: e() };
        };
        let pq = crate::genotype::sample_pqv(s);
        ffi::SampleRow {
            file: s.file.clone(),
            name: s.name.clone(),
            run_folder: s.run_folder.clone(),
            status: match s.status {
                Status::NotAnalyzed => "unanalyzed",
                Status::Running => "running",
                Status::Analyzed => "analyzed",
                Status::Failed => "failed",
            }
            .into(),
            sq: pq.sq.into(),
            method: s.method.clone(),
            size_std: s.size_std.clone(),
            instrument: s.instrument.clone(),
            well: s.well.clone(),
            error: if s.error.is_empty() { s.genotype_error.clone() } else { s.error.clone() },
            stype: s.stype.clone(),
            panel: s.panel_key.split('\u{1}').nth(1).unwrap_or("").to_string(),
            sos: pq.sos.into(),
            sspk: pq.sspk.into(),
            cgq: pq.cgq.into(),
        }
    }
    fn remove_samples(&self, rows: &Vec<usize>) {
        if self.progress.lock().unwrap().running {
            return;
        }
        let mut list = self.samples.lock().unwrap();
        let mut r = rows.clone();
        r.sort_unstable();
        r.dedup();
        for i in r.into_iter().rev() {
            if i < list.len() {
                list.remove(i);
            }
        }
    }
    fn start_analysis(&self, all: bool) {
        self.run_analysis(all)
    }
    fn stop_analysis(&self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst)
    }
    fn analysis_progress(&self) -> ffi::ProgressInfo {
        let p = self.progress.lock().unwrap();
        ffi::ProgressInfo { done: p.done, total: p.total, running: p.running, message: p.message.clone(), phase: p.phase.clone() }
    }
    fn analysis_of(&self, i: usize) -> Option<std::sync::Arc<serde_json::Value>> {
        self.samples.lock().unwrap().get(i).and_then(|s| s.analysis.clone())
    }
    fn sample_traces(&self, i: usize, bp: bool) -> Vec<ffi::Trace> {
        let Some(a) = self.analysis_of(i) else { return Vec::new() };
        let c = backend::curve(&a);
        let mut out = Vec::new();
        for ch in a["channels"].as_array().into_iter().flatten() {
            if ch["group"] != "raw" {
                continue;
            }
            let vals = ch.get("analyzed_values").filter(|v| v.is_array()).unwrap_or(&ch["values"]);
            let ys: Vec<f64> = vals.as_array().map(|v| v.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect()).unwrap_or_default();
            let xs: Vec<f64> = (0..ys.len()).map(|k| if bp { backend::scan_to_bp(&c, k as f64) } else { k as f64 }).collect();
            out.push(ffi::Trace { tag: ch["tag"].as_str().unwrap_or("").into(), dye: ch["dye"].as_str().unwrap_or("").into(), xs, ys });
        }
        out
    }
    fn sample_peaks(&self, i: usize, bp: bool) -> Vec<ffi::PeakMark> {
        let Some(a) = self.analysis_of(i) else { return Vec::new() };
        let c = backend::curve(&a);
        let mut out = Vec::new();
        for ch in a["channels"].as_array().into_iter().flatten().filter(|c| c["group"] == "raw") {
            for p in ch["peaks"].as_array().into_iter().flatten() {
                let scan = p["scan"].as_f64().unwrap_or(0.0);
                let x = if bp { p["size_bp"].as_f64().unwrap_or_else(|| backend::scan_to_bp(&c, scan)) } else { scan };
                out.push(ffi::PeakMark { tag: ch["tag"].as_str().unwrap_or("").into(), x, height: p["height"].as_f64().unwrap_or(0.0), offscale: p["offscaled"].as_bool().unwrap_or(false) });
            }
        }
        out
    }
    /// [first, last] bp of the size-standard calibration, empty when the sample is not sized.
    fn sized_range(&self, i: usize) -> Vec<f64> {
        let Some(a) = self.analysis_of(i) else { return Vec::new() };
        let c = backend::curve(&a);
        if c.len() < 2 {
            return Vec::new();
        }
        vec![c[0].1, c[c.len() - 1].1]
    }
    fn sample_info_text(&self, i: usize) -> String {
        let list = self.samples.lock().unwrap();
        let Some(s) = list.get(i) else { return String::new() };
        let mut t = format!("Файл: {}\nОбразец: {}\nЛунка: {}\nПрибор: {}\nМетод: {}\nСтандарт длин: {}\n", s.file, s.name, s.well, s.instrument, s.method, s.size_std);
        if let Some(a) = &s.analysis {
            let sz = &a["sizing"];
            t += &format!("\nКалибровка: {} ({} фрагментов сопоставлено)\nSQ: {}\nВерсия анализа: {}\n", sz["status"].as_str().unwrap_or("—"), sz["matched"].as_array().map(|v| v.len()).unwrap_or(0), backend::sizing_quality(a), a["analysis_version"].as_str().unwrap_or(""));
        }
        if !s.error.is_empty() {
            t += &format!("\nОшибка: {}\n", s.error);
        }
        t
    }
}

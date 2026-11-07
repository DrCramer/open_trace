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
    }

    extern "Rust" {
        type Backend;
        fn new_backend() -> Box<Backend>;
        fn load_problem(self: &Backend) -> String;
        fn resources_dir(self: &Backend) -> String;
        fn method_list(self: &Backend) -> Vec<String>;
        fn size_standard_list(self: &Backend) -> Vec<String>;
        fn add_sample_files(self: &Backend, paths: &Vec<String>, run_folder: &str, method: &str, size_std: &str) -> Vec<String>;
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
    fn add_sample_files(&self, paths: &Vec<String>, run_folder: &str, method: &str, size_std: &str) -> Vec<String> {
        self.add_files(paths, run_folder, method, size_std)
    }
    fn sample_count(&self) -> usize {
        self.samples.lock().unwrap().len()
    }
    fn sample_row(&self, i: usize) -> ffi::SampleRow {
        let list = self.samples.lock().unwrap();
        let Some(s) = list.get(i) else {
            return ffi::SampleRow { file: String::new(), name: String::new(), run_folder: String::new(), status: String::new(), sq: String::new(), method: String::new(), size_std: String::new(), instrument: String::new(), well: String::new(), error: String::new() };
        };
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
            sq: s.analysis.as_ref().map(|a| backend::sizing_quality(a).to_string()).unwrap_or_else(|| "na".into()),
            method: s.method.clone(),
            size_std: s.size_std.clone(),
            instrument: s.instrument.clone(),
            well: s.well.clone(),
            error: s.error.clone(),
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
        ffi::ProgressInfo { done: p.done, total: p.total, running: p.running, message: p.message.clone() }
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

//! Native project file (`.otproj`): gzip-compressed JSON with the samples, their settings, analyses and genotyping
//! reports, so a project reopens without re-analysis. The FSA files themselves are referenced by path (as in ID-X);
//! the analyses keep the SHA-256 of the file they were made from.
use crate::backend::{Inner, Sample, Status};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

pub const FORMAT: &str = "opentrace-native-project";

fn status_text(s: Status) -> &'static str {
    match s {
        Status::Analyzed => "analyzed",
        Status::Failed => "failed",
        _ => "unanalyzed",
    }
}

impl Inner {
    pub fn save_project(&self, path: &str) -> Result<(), String> {
        let samples: Vec<Value> = self
            .samples
            .lock()
            .unwrap()
            .iter()
            .map(|s| {
                json!({
                    "path": s.path.display().to_string(), "file": s.file, "name": s.name, "run_folder": s.run_folder,
                    "method": s.method, "size_std": s.size_std, "status": status_text(s.status), "error": s.error,
                    "instrument": s.instrument, "well": s.well, "type": s.stype, "panel_key": s.panel_key,
                    "analysis": s.analysis.as_deref(), "report": s.report.as_deref(), "ladder_quality": s.ladder_quality,
                    "ladder_used": s.ladder_used, "reference_bins": s.reference_bins, "alas_skipped": s.alas_skipped,
                    "genotype_error": s.genotype_error, "edits": s.edits,
                })
            })
            .collect();
        let doc = json!({"format": FORMAT, "version": 1, "saved_at": now_text(), "samples": samples});
        let tmp = PathBuf::from(format!("{path}.tmp"));
        {
            let f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
            let mut gz = GzEncoder::new(f, Compression::new(6));
            serde_json::to_writer(&mut gz, &doc).map_err(|e| e.to_string())?;
            gz.finish().map_err(|e| e.to_string())?.flush().map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    pub fn open_project(&self, path: &str) -> Result<usize, String> {
        if self.progress.lock().unwrap().running {
            return Err("analysis is running".into());
        }
        let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut text = String::new();
        GzDecoder::new(f).read_to_string(&mut text).map_err(|e| format!("not an OpenTrace project: {e}"))?;
        let doc: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        if doc["format"] != FORMAT {
            return Err("not an OpenTrace project".into());
        }
        if doc["version"].as_i64().unwrap_or(0) > 1 {
            return Err("the project was saved by a newer version".into());
        }
        let s_ = |v: &Value, k: &str| v[k].as_str().unwrap_or("").to_string();
        let mut list = Vec::new();
        for v in doc["samples"].as_array().into_iter().flatten() {
            let analysis = v.get("analysis").filter(|a| a.is_object()).cloned().map(Arc::new);
            list.push(Sample {
                path: PathBuf::from(s_(v, "path")),
                file: s_(v, "file"),
                name: s_(v, "name"),
                run_folder: s_(v, "run_folder"),
                method: s_(v, "method"),
                size_std: s_(v, "size_std"),
                status: match (v["status"].as_str(), analysis.is_some()) {
                    (Some("analyzed"), true) => Status::Analyzed,
                    (Some("failed"), _) => Status::Failed,
                    _ => Status::NotAnalyzed,
                },
                error: s_(v, "error"),
                instrument: s_(v, "instrument"),
                well: s_(v, "well"),
                analysis,
                stype: { let t = s_(v, "type"); if t.is_empty() { "sample".into() } else { t } },
                panel_key: s_(v, "panel_key"),
                report: v.get("report").filter(|r| r.is_object()).cloned().map(Arc::new),
                ladder_quality: v["ladder_quality"].as_i64(),
                ladder_used: v["ladder_used"].as_bool().unwrap_or(false),
                reference_bins: v["reference_bins"].as_bool().unwrap_or(false),
                alas_skipped: v["alas_skipped"].as_bool().unwrap_or(false),
                genotype_error: s_(v, "genotype_error"),
                edits: v["edits"].as_array().cloned().unwrap_or_default(),
            });
        }
        let n = list.len();
        *self.samples.lock().unwrap() = list;
        Ok(n)
    }
}

fn now_text() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{secs}")
}

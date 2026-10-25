//! Python extension `opentrace_core` (PyO3): fast paths for `genemapper_open.preprocessing` and `sg_peaks`.
use otcore::{preprocessing as pp, ratio, sg};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyFloat, PyInt, PyList, PyTuple};

/// True when `values` is a list/tuple of exact Python ints in the signed 32-bit range (reference `_trace` check).
#[pyfunction]
fn is_int32_trace(values: &Bound<'_, PyAny>) -> bool {
    let check = |item: Bound<'_, PyAny>| -> bool {
        item.is_exact_instance_of::<PyInt>()
            && item.extract::<i64>().map(|v| (-(1i64 << 31)..(1i64 << 31)).contains(&v)).unwrap_or(false)
    };
    if let Ok(l) = values.downcast::<PyList>() {
        l.iter().all(check)
    } else if let Ok(t) = values.downcast::<PyTuple>() {
        t.iter().all(check)
    } else {
        false
    }
}

/// True when every item is an int or a finite float (server input check).
#[pyfunction]
fn all_finite(values: &Bound<'_, PyAny>) -> bool {
    let check = |item: Bound<'_, PyAny>| -> bool {
        if item.is_instance_of::<PyInt>() {
            true
        } else if item.is_instance_of::<PyFloat>() {
            item.extract::<f64>().map(|v| v.is_finite()).unwrap_or(false)
        } else {
            item.extract::<f64>().map(|v| v.is_finite()).unwrap_or(false)
        }
    };
    if let Ok(l) = values.downcast::<PyList>() {
        l.iter().all(check)
    } else if let Ok(t) = values.downcast::<PyTuple>() {
        t.iter().all(check)
    } else {
        false
    }
}


/// `ratio_sizing._best_paths` fast path: ([(score, path)], transitions). The caller checks the size limits
/// (`ratio_max_count` / `ratio_max_len`) and otherwise uses the Python reference.
#[pyfunction]
fn best_paths(scans: Vec<f64>, heights: Vec<f64>, bp: Vec<f64>, weight: f64) -> (Vec<(f64, Vec<usize>)>, u64) {
    ratio::best_paths(&scans, &heights, &bp, weight)
}

#[pyfunction]
fn extrema(values: Vec<i64>, radius: usize, maximum: bool) -> Vec<i64> {
    pp::extrema(&values, radius, maximum)
}

#[pyfunction]
fn average(values: Vec<f64>, radius: usize) -> Vec<f64> {
    pp::average(&values, radius)
}

/// (baseline, corrected) for prepare_trace; radii are window // 2.
#[pyfunction]
fn baseline_correct(values: Vec<i64>, broad: usize, narrow: usize, smooth: usize) -> (Vec<f64>, Vec<i64>) {
    let b = pp::estimate_baseline(&values, broad, narrow, smooth);
    let c = pp::correct(&values, &b);
    (b, c)
}

#[pyfunction]
fn estimate_baseline(values: Vec<i64>, broad: usize, narrow: usize, smooth: usize) -> Vec<f64> {
    pp::estimate_baseline(&values, broad, narrow, smooth)
}

#[pyfunction]
fn smooth_trace(values: Vec<i64>, passes: usize) -> Vec<i64> {
    pp::smooth_trace(&values, passes)
}

#[pyfunction]
fn cubic_weights(radius: usize) -> Vec<f64> {
    sg::cubic_weights(radius)
}

#[pyfunction]
fn sg_derivative(values: Vec<i64>, weights: Vec<f64>) -> Vec<f64> {
    sg::derivative(&values, &weights)
}

/// Returns (peaks as dicts, diagnostics counts dict) with the reference key names.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
fn detect_peaks<'py>(
    py: Python<'py>,
    values: Vec<i64>,
    derivative: Vec<f64>,
    threshold: f64,
    min_width: f64,
    tau_begin: f64,
    tau_end: f64,
    offscale: Vec<(f64, f64)>,
) -> PyResult<(Vec<Bound<'py, PyDict>>, Bound<'py, PyDict>)> {
    let (peaks, d) = sg::detect(&values, &derivative, threshold, min_width, tau_begin, tau_end, &offscale);
    let mut out = Vec::with_capacity(peaks.len());
    for p in peaks {
        let x = PyDict::new_bound(py);
        x.set_item("scan", p.scan)?;
        x.set_item("sg_scan", p.sg_scan)?;
        x.set_item("begin", p.begin)?;
        x.set_item("end", p.end)?;
        x.set_item("height", p.height)?;
        x.set_item("begin_height", p.begin_height)?;
        x.set_item("end_height", p.end_height)?;
        x.set_item("fwhm_scan", p.fwhm_scan)?;
        x.set_item("area", p.area)?;
        x.set_item("offscaled", p.offscaled)?;
        out.push(x);
    }
    let g = PyDict::new_bound(py);
    g.set_item("sg_candidates", d.sg_candidates)?;
    g.set_item("invalid_bounds", d.invalid_bounds)?;
    g.set_item("height_rejected", d.height_rejected)?;
    g.set_item("width_rejected", d.width_rejected)?;
    g.set_item("edge_rejected", d.edge_rejected)?;
    g.set_item("duplicate_snapped_scans", d.duplicate_snapped_scans)?;
    g.set_item("accepted_peaks", d.accepted_peaks)?;
    Ok((out, g))
}

#[pymodule]
fn opentrace_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_function(wrap_pyfunction!(is_int32_trace, m)?)?;
    m.add_function(wrap_pyfunction!(all_finite, m)?)?;
    m.add_function(wrap_pyfunction!(best_paths, m)?)?;
    m.add_function(wrap_pyfunction!(extrema, m)?)?;
    m.add_function(wrap_pyfunction!(average, m)?)?;
    m.add_function(wrap_pyfunction!(baseline_correct, m)?)?;
    m.add_function(wrap_pyfunction!(estimate_baseline, m)?)?;
    m.add_function(wrap_pyfunction!(smooth_trace, m)?)?;
    m.add_function(wrap_pyfunction!(cubic_weights, m)?)?;
    m.add_function(wrap_pyfunction!(sg_derivative, m)?)?;
    m.add_function(wrap_pyfunction!(detect_peaks, m)?)?;
    Ok(())
}

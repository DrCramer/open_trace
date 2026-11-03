//! Port of `genemapper_open/demo.py` and `str_demo.py`: invented ABIF fixtures and demo definitions.
//! Float math uses the C library (`sin`, `exp`, `pow`) exactly as CPython does.
use crate::abif::AbifFile;
use crate::numeric::{py_pow, py_sum};
use crate::small_svd::SOLVER_ID;
use serde_json::{json, Map, Value};

extern "C" {
    #[link_name = "sin"]
    fn libm_sin(x: f64) -> f64;
    #[link_name = "exp"]
    fn libm_exp(x: f64) -> f64;
}
fn sin(x: f64) -> f64 {
    unsafe { libm_sin(x) }
}
fn exp(x: f64) -> f64 {
    unsafe { libm_exp(x) }
}
/// Python `round(x)` (half to even) as an int.
fn pyround(x: f64) -> i64 {
    x.round_ties_even() as i64
}

pub const SYNTHETIC_STANDARD_BP: [i64; 9] = [75, 105, 155, 205, 275, 335, 425, 505, 605];
pub const SYNTHETIC_PIPELINE_STANDARD_BP: [i64; 6] = [40, 70, 110, 145, 195, 260];

type Item = (Vec<u8>, i32, u16, u16, u32, Vec<u8>);

fn pstr(s: &[u8]) -> Vec<u8> {
    let mut v = vec![s.len() as u8];
    v.extend_from_slice(s);
    v
}
fn pack_i16(values: &[i64]) -> Vec<u8> {
    values.iter().flat_map(|v| (*v as i16).to_be_bytes()).collect()
}

pub fn build_abif(items: &[Item]) -> Vec<u8> {
    let mut directory = Vec::new();
    let mut payloads: Vec<u8> = Vec::new();
    let data_offset = 128 + items.len() * 28;
    let entry = |d: &mut Vec<u8>, name: &[u8], number: i32, kind: u16, width: u16, count: u32, size: u32, offset: u32| {
        d.extend_from_slice(name);
        d.extend_from_slice(&number.to_be_bytes());
        d.extend_from_slice(&kind.to_be_bytes());
        d.extend_from_slice(&width.to_be_bytes());
        d.extend_from_slice(&count.to_be_bytes());
        d.extend_from_slice(&size.to_be_bytes());
        d.extend_from_slice(&offset.to_be_bytes());
        d.extend_from_slice(&0u32.to_be_bytes());
    };
    for (name, number, kind, width, count, payload) in items {
        let offset: u32 = if payload.len() <= 4 {
            let mut b = payload.clone();
            b.resize(4, 0);
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        } else {
            let o = (data_offset + payloads.len()) as u32;
            payloads.extend_from_slice(payload);
            o
        };
        entry(&mut directory, name, *number, *kind, *width, *count, payload.len() as u32, offset);
    }
    let mut root = Vec::new();
    entry(&mut root, b"tdir", 1, 1023, 28, items.len() as u32, directory.len() as u32, 128);
    let mut header = b"ABIF".to_vec();
    header.extend_from_slice(&101u16.to_be_bytes());
    header.extend_from_slice(&root);
    header.resize(128, 0);
    header.extend_from_slice(&directory);
    header.extend_from_slice(&payloads);
    header
}

fn item(name: &str, number: i32, kind: u16, width: u16, count: usize, payload: Vec<u8>) -> Item {
    (name.as_bytes().to_vec(), number, kind, width, count as u32, payload)
}

pub fn demo_bytes() -> Vec<u8> {
    let mut items: Vec<Item> = Vec::new();
    items.push(item("SMPL", 1, 18, 1, 15, b"\x0eSynthetic demo".to_vec()));
    items.push(item("Dye#", 1, 4, 2, 1, 6i16.to_be_bytes().to_vec()));
    for (i, number) in [1, 2, 3, 4, 105, 106].iter().enumerate() {
        let channel = (i + 1) as i64;
        let name = format!("Dye {}", channel);
        items.push(item("DyeN", channel as i32, 18, 1, name.len() + 1, pstr(name.as_bytes())));
        let mut values = Vec::with_capacity(3000);
        for scan in 0..3000i64 {
            let mut signal = 15.0 + 3.0 * sin(scan as f64 * 0.17 + channel as f64);
            let centers = [350 + 70 * channel, 950 + 40 * channel, 1700 + 50 * channel, 2350 + 20 * channel];
            for (pi, center) in centers.iter().enumerate() {
                let z = (scan - center) as f64 / (5 + channel) as f64;
                signal += (300 + 70 * channel + pi as i64 * 90) as f64 * exp(-0.5 * py_pow(z, 2.0));
            }
            values.push(pyround(signal));
        }
        items.push(item("DATA", *number, 4, 2, values.len(), pack_i16(&values)));
    }
    build_abif(&items)
}

pub fn sizing_demo_bytes() -> Vec<u8> {
    let original = AbifFile::parse(&demo_bytes(), "").unwrap();
    let label = b"Synthetic sizing";
    let mut items: Vec<Item> = vec![item("SMPL", 1, 18, 1, label.len() + 1, pstr(label))];
    for key in &original.order {
        if key.0 == "SMPL" || (key.0 == "DATA" && key.1 == 106) {
            continue;
        }
        let t = &original.tags[key];
        items.push((t.name.as_bytes().to_vec(), t.number, t.element_type, t.element_size, t.count, t.data.clone()));
    }
    let mut centers: Vec<(f64, f64, f64)> = SYNTHETIC_STANDARD_BP
        .iter()
        .enumerate()
        .map(|(i, bp)| {
            let bp = *bp as f64;
            (200.0 + 3.2 * bp + 0.0015 * bp * bp, (700 + i as i64 * 21) as f64, 5.0)
        })
        .collect();
    centers.push((120.0, 1200.0, 5.0));
    centers.push((2125.0, 1500.0, 5.0));
    let mut values = Vec::with_capacity(3000);
    for scan in 0..3000i64 {
        let mut signal = 15.0 + 2.0 * sin(scan as f64 * 0.13);
        signal += py_sum(centers.iter().map(|(c, h, w)| {
            let z = (scan as f64 - c) / w;
            h * exp(-0.5 * py_pow(z, 2.0))
        }));
        values.push(pyround(signal));
    }
    items.push(item("DATA", 106, 4, 2, values.len(), pack_i16(&values)));
    build_abif(&items)
}

pub fn pipeline_demo_bytes() -> Vec<u8> {
    let label = b"Synthetic morphology sizing";
    let mut items: Vec<Item> = vec![
        item("SMPL", 1, 18, 1, label.len() + 1, pstr(label)),
        item("Dye#", 1, 4, 2, 1, 6i16.to_be_bytes().to_vec()),
    ];
    for (i, number) in [1, 2, 3, 4, 105, 106].iter().enumerate() {
        let channel = (i + 1) as i64;
        let name = format!("Dye {}", channel);
        items.push(item("DyeN", channel as i32, 18, 1, name.len() + 1, pstr(name.as_bytes())));
        let centers: Vec<i64> = if channel == 6 {
            SYNTHETIC_PIPELINE_STANDARD_BP.iter().map(|bp| 200 + 20 * bp).collect()
        } else {
            vec![1400 + channel * 15, 2400, 3800, 4800 - channel * 10]
        };
        let mut values = Vec::with_capacity(6501);
        for s in 0..6501i64 {
            let sum = py_sum(centers.iter().enumerate().map(|(j, c)| {
                let h = if channel == 6 { 1000 } else { 300 + channel * 60 + j as i64 * 70 };
                let z = (s - c) as f64 / 5.0;
                h as f64 * exp(-0.5 * py_pow(z, 2.0))
            }));
            values.push(pyround(20.0 + 0.001 * s as f64 + sum));
        }
        items.push(item("DATA", *number, 4, 2, values.len(), pack_i16(&values)));
    }
    build_abif(&items)
}

// ------------------------------------------------------------------ str_demo

pub struct Fixture {
    pub sample_bytes: Vec<u8>,
    pub ladder_bytes: Vec<u8>,
    pub kit: Value,
    pub settings: Value,
}

#[derive(Clone)]
pub struct StrOpts {
    pub include_spike: bool,
    pub spike_height: i64,
    pub ladder_heights: Option<Vec<i64>>,
    pub ladder_shift_bp: f64,
    pub jag_height: i64,
    pub pullup_reference_height: i64,
}
impl Default for StrOpts {
    fn default() -> Self {
        StrOpts { include_spike: false, spike_height: 1100, ladder_heights: None, ladder_shift_bp: 0.0, jag_height: 0, pullup_reference_height: 0 }
    }
}

pub fn make_str_demo(o: &StrOpts) -> Fixture {
    let kit = json!({"schema_version": 1, "name": "Invented STR workflow example", "markers": [{
        "name": "ExampleM", "channel": "DATA1", "min_bp": 160, "max_bp": 200, "repeat": 4,
        "bins": [
            {"label": "A", "center_bp": 168, "left_bp": 0.5, "right_bp": 0.5},
            {"label": "B", "center_bp": 176, "left_bp": 0.5, "right_bp": 0.5},
            {"label": "C", "center_bp": 184, "left_bp": 0.5, "right_bp": 0.5},
            {"label": "D", "center_bp": 192, "left_bp": 0.5, "right_bp": 0.5}],
        "ladder_labels": ["A", "B", "C", "D"], "parameters": {
            "ladder_cutoff_ratio": 0.05, "cutoff_ratio": 0.1, "minus_a_ratio": 0.1,
            "minus_a_min_bp": 0.8, "minus_a_max_bp": 1.2,
            "backward_stutter_ranges": [{"min_bp": 3.8, "max_bp": 4.2, "ratio": 0.15}],
            "forward_stutter_ranges": [], "clear_ranges": []}}]});
    let data = |ladder: bool| -> Vec<u8> {
        let name: &[u8] = if ladder { b"Invented ladder" } else { b"Invented sample" };
        let mut items: Vec<Item> = vec![
            item("SMPL", 1, 18, 1, name.len() + 1, pstr(name)),
            item("Dye#", 1, 4, 2, 1, 6i16.to_be_bytes().to_vec()),
        ];
        for (idx, number) in [1, 2, 3, 4, 105, 106].iter().enumerate() {
            let index = (idx + 1) as i32;
            let dye = format!("Example dye {}", index);
            items.push(item("DyeN", index, 18, 1, dye.len() + 1, pstr(dye.as_bytes())));
            let mut bp: Vec<f64> = if *number == 106 {
                SYNTHETIC_PIPELINE_STANDARD_BP.iter().map(|x| *x as f64).collect()
            } else if *number == 1 {
                if ladder {
                    [168.3, 176.3, 184.3, 192.3].iter().map(|x| x + o.ladder_shift_bp).collect()
                } else {
                    vec![176.3, 184.3]
                }
            } else {
                vec![]
            };
            if *number == 2 && !ladder && o.pullup_reference_height > 0 {
                bp = vec![176.3];
            }
            let mut values = Vec::with_capacity(6501);
            for scan in 0..6501i64 {
                let sum = py_sum(bp.iter().enumerate().map(|(i, size)| {
                    let h: i64 = if *number == 2 {
                        o.pullup_reference_height
                    } else if ladder && *number == 1 && o.ladder_heights.is_some() {
                        o.ladder_heights.as_ref().unwrap()[i]
                    } else {
                        1000
                    };
                    let z = (scan as f64 - (200.0 + 20.0 * size)) / 5.0;
                    h as f64 * exp(-0.5 * py_pow(z, 2.0))
                }));
                let spike = if o.include_spike && !ladder && *number == 1 {
                    let z = (scan as f64 - (200.0 + 20.0 * 168.3)) / 0.65;
                    o.spike_height as f64 * exp(-0.5 * py_pow(z, 2.0))
                } else {
                    0.0
                };
                let jag = if !ladder && *number == 1 && scan == 3726 { o.jag_height as f64 } else { 0.0 };
                values.push(pyround(20.0 + 0.001 * scan as f64 + sum + spike + jag));
            }
            items.push(item("DATA", *number, 4, 2, values.len(), pack_i16(&values)));
        }
        build_abif(&items)
    };
    Fixture {
        sample_bytes: data(false),
        ladder_bytes: data(true),
        kit,
        settings: json!({"sizing_mode": "auto", "analysis_engine": "morphology_ratio",
            "standard_channel": "DATA106", "standard_bp": SYNTHETIC_PIPELINE_STANDARD_BP,
            "threshold": 100, "calibration_model": "local_rational", "quality_threshold": 0.75,
            "height_mode": "explicit", "num_extra_peaks": 25}),
    }
}

const QUALITY_WEIGHTS: [&str; 9] =
    ["allele_number", "absolute_height", "high_height", "spike", "ratio", "broad", "offscale", "overlap", "binning"];

pub fn make_quality_demo_definition() -> Value {
    let mut weights = Map::new();
    for k in QUALITY_WEIGHTS {
        weights.insert(k.into(), json!(0.5));
    }
    json!({"schema_version": 1, "sample_sq": {"source": "base_quality"}, "gq_thresholds": {"good": 0.75, "bad": 0.25},
        "amel_marker": null, "control_mode": "none", "markers": {"ExampleM": {
            "kind": "autosomal", "confidence_source": "bin_confidence_weight",
            "thresholds": {"max_alleles": 2, "homozygous_height": 100, "heterozygous_height": 50,
                           "high_height": 5000, "ratio": 0.5, "width_bp": 2},
            "weights": weights,
            "flags": {"spike": false, "offscale": false, "overlap": false, "binning": false}}}})
}

pub fn make_ladder_quality_demo_definition(reject: bool) -> Value {
    json!({"schema_version": 1, "calling_parameters_source": "ladder_matching_calls",
        "ladder_sq": {"source": "base_quality"}, "sq_thresholds": {"good": 0.75, "bad": 0.25},
        "gq_thresholds": {"good": 0.75, "bad": 0.25}, "markers": {"ExampleM": {
            "min_height_ratio": if reject { json!(1) } else { json!(0.2) }, "spike_metric": 0, "spike_weight": 0.5,
            "offscale_metric": 0, "offscale_weight": 0.5, "spacing": {"enabled": true, "max_distance_bp": 20,
                "curve": [{"distance_bp": 0, "fraction": 0.2, "interpolate": false}],
                "exceptions": [], "excluded_alleles": []}}}})
}

pub struct RunDemo {
    pub fixture: Fixture,
    pub ladders: Vec<(String, Vec<u8>)>,
    pub ladder_quality_definition: Value,
    pub marker_quality_definition: Value,
}

pub fn make_run_demo() -> RunDemo {
    let fixture = make_str_demo(&StrOpts::default());
    let specs: [(&str, f64, Option<Vec<i64>>); 3] =
        [("Own-A.fsa", -0.2, None), ("Own-B.fsa", 0.2, None), ("Own-Failed.fsa", 3.0, Some(vec![400, 1000, 1000, 1000]))];
    let ladders = specs
        .into_iter()
        .map(|(name, shift, heights)| {
            let o = StrOpts { ladder_shift_bp: shift, ladder_heights: heights, ..StrOpts::default() };
            (name.to_string(), make_str_demo(&o).ladder_bytes)
        })
        .collect();
    RunDemo { fixture, ladders, ladder_quality_definition: make_ladder_quality_demo_definition(false), marker_quality_definition: make_quality_demo_definition() }
}

pub fn make_cross_check_demo() -> (Fixture, Value) {
    let mut fixture = make_str_demo(&StrOpts::default());
    let prototype = fixture.kit["markers"][0].clone();
    let mut amel = prototype.clone();
    let mut y = prototype.clone();
    {
        let a = amel.as_object_mut().unwrap();
        a.insert("name".into(), json!("AMEL"));
        a.insert("min_bp".into(), json!(160));
        a.insert("max_bp".into(), json!(180));
        let bins: Vec<Value> = a["bins"].as_array().unwrap()[..2].to_vec();
        a.insert("bins".into(), Value::Array(bins));
        a.insert("ladder_labels".into(), json!(["Y", "X"]));
        for (entry, label) in a["bins"].as_array_mut().unwrap().iter_mut().zip(["Y", "X"]) {
            entry["label"] = json!(label);
        }
        let b = y.as_object_mut().unwrap();
        b.insert("name".into(), json!("ExampleY"));
        b.insert("min_bp".into(), json!(180));
        b.insert("max_bp".into(), json!(200));
        let bins: Vec<Value> = b["bins"].as_array().unwrap()[2..].to_vec();
        b.insert("bins".into(), Value::Array(bins));
        b.insert("ladder_labels".into(), json!(["1", "2"]));
        for (entry, label) in b["bins"].as_array_mut().unwrap().iter_mut().zip(["1", "2"]) {
            entry["label"] = json!(label);
        }
    }
    let kit = fixture.kit.as_object_mut().unwrap();
    kit.insert("name".into(), json!("Invented AMEL cross-check example"));
    kit.insert("markers".into(), json!([amel, y]));
    let mut definition = make_quality_demo_definition();
    let d = definition.as_object_mut().unwrap();
    let recipe = d["markers"].as_object_mut().unwrap().shift_remove("ExampleM").unwrap();
    d.insert("schema_version".into(), json!(2));
    d.insert("sample_type".into(), json!("Sample"));
    d.insert("amel_marker".into(), json!("AMEL"));
    d.insert("amel_cross_check".into(), json!({"weight": 0.5}));
    let mut a = recipe.clone();
    let mut yy = recipe;
    a["kind"] = json!("amel");
    yy["kind"] = json!("y");
    d.insert("markers".into(), json!({"AMEL": a, "ExampleY": yy}));
    (fixture, definition)
}

pub fn make_negative_demo_definitions() -> (Value, Value) {
    let mut quality = make_quality_demo_definition();
    let q = quality.as_object_mut().unwrap();
    q.insert("schema_version".into(), json!(2));
    q.insert("sample_type".into(), json!("Negative Control"));
    q.insert("control_mode".into(), json!("negative_attached"));
    q.insert("amel_cross_check".into(), Value::Null);
    q.insert(
        "markers".into(),
        json!({"ExampleM": {"kind": "autosomal", "confidence_source": "bin_confidence_weight", "spike_metric": 0, "spike_weight": 0.5}}),
    );
    let control = json!({"schema_version": 1, "sample_type": "Negative Control", "markers": {
        "ExampleM": {"expected_alleles": [], "quality_sensor": false, "is_y_marker": false}}});
    (quality, control)
}

pub fn make_spike_demo_definition() -> Value {
    json!({"schema_version": 1, "measurement_source": "prepared_signal", "scope": "non_reference_selected_channels",
        "morphology_enabled": false, "units": "scan", "min_height": 100, "exhalf_width_threshold": 6,
        "near_base_width_threshold": 10, "area_ratio_threshold": 0.8, "treat_overlap_as_spike": true,
        "near_base_fraction": 0.1, "max_base_height": -1})
}

pub fn make_spike_adjustment_demo(adjusted: bool) -> Fixture {
    let mut fixture = make_str_demo(&StrOpts { include_spike: true, spike_height: 300, ..StrOpts::default() });
    fixture.kit["markers"][0]["parameters"]["cutoff_ratio"] = json!(0.2);
    let mut definition = make_spike_demo_definition();
    if adjusted {
        let d = definition.as_object_mut().unwrap();
        d.insert("schema_version".into(), json!(2));
        d.insert("adjustments".into(), json!({"sample_cutoff": true, "sample_marker_envelope": true, "ladder_height_ratio": 0.2}));
    }
    fixture.settings["spike_definition"] = definition;
    fixture
}

pub fn make_morphology_demo(enabled: bool, pullup_reference_height: i64) -> Fixture {
    let mut fixture = make_str_demo(&StrOpts { jag_height: 600, pullup_reference_height, ..StrOpts::default() });
    if enabled {
        let mut definition = make_spike_demo_definition();
        let d = definition.as_object_mut().unwrap();
        d.insert("schema_version".into(), json!(3));
        d.insert("measurement_source".into(), json!("baseline_corrected_companion"));
        d.insert("morphology_enabled".into(), json!(true));
        d.insert("morphology".into(), json!({"jag_threshold": 0.2, "fit_score_threshold": 0.995}));
        d.insert("adjustments".into(), json!({"sample_cutoff": false, "sample_marker_envelope": false, "ladder_height_ratio": 0}));
        fixture.settings["spike_definition"] = definition;
    }
    fixture
}

pub fn make_true_peak_demo(exclude_overlap: bool, pullup_reference_height: i64) -> Fixture {
    let mut fixture = make_morphology_demo(true, pullup_reference_height);
    let d = fixture.settings["spike_definition"].as_object_mut().unwrap();
    d.insert("schema_version".into(), json!(4));
    d.insert("treat_overlap_as_spike".into(), json!(exclude_overlap));
    d.insert("true_peak".into(), json!({"polynomial_solver": SOLVER_ID, "max_denominator_scan2": 10000}));
    fixture
}

pub fn make_pullup_demo_definition(detector: bool) -> Value {
    let det = if detector {
        json!({"algorithm": "static_ar_pairwise_body", "calling_policy": "explicit_ordered_channel_pairs",
            "phase": "after_spike_restoration_before_calling", "reference_order": "stable_scan_midpoint_binary_search",
            "max_scan_distance": 1, "height_ratio_threshold": 0.08,
            "channel_pairs": [{"candidate_channel": "DATA1", "reference_channel": "DATA2"}]})
    } else {
        Value::Null
    };
    json!({"schema_version": 1, "profile_threshold": 0.4, "detector": det})
}

pub fn make_pullup_demo(detector: bool, requested: bool) -> Fixture {
    let mut fixture = make_str_demo(&StrOpts { pullup_reference_height: 18000, ..StrOpts::default() });
    if requested {
        fixture.settings["pullup_definition"] = make_pullup_demo_definition(detector);
    }
    fixture
}

pub fn make_post_sizing_demo(kind: &str) -> Result<Fixture, String> {
    if !["sq-pass", "sq-check", "sq-fail", "sq-ladder-fail"].contains(&kind) {
        return Err("Unknown own SQ fixture".into());
    }
    let mut fixture = make_str_demo(&StrOpts::default());
    fixture.settings["sizing_quality_definition"] = json!({"schema_version": 1, "context": "fresh_hid_no_override",
        "width_conversion": "unrounded_standard_map_span",
        "warning_width_bp": if kind == "sq-pass" { json!(2) } else { json!(0.1) },
        "broad_weight": if kind == "sq-fail" || kind == "sq-ladder-fail" { json!(0.8) } else { json!(0.5) },
        "thresholds": {"good": 0.75, "bad": 0.25}});
    Ok(fixture)
}

pub fn make_ladder_calling_demo(legacy: bool) -> (Fixture, Value) {
    let mut fixture = make_str_demo(&StrOpts { ladder_heights: Some(vec![700, 900, 1100, 1300]), ..StrOpts::default() });
    let p = fixture.kit["markers"][0]["parameters"].as_object_mut().unwrap();
    p.insert("minus_a_ratio".into(), json!(0.95));
    p.insert("minus_a_min_bp".into(), json!(7.5));
    p.insert("minus_a_max_bp".into(), json!(8.5));
    let mut quality = make_ladder_quality_demo_definition(false);
    if legacy {
        quality["calling_parameters_source"] = json!("kit_sample_parameters");
    }
    (fixture, quality)
}

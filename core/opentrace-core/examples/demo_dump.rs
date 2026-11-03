//! Counterpart of scratchpad/demo_check.py: digests of demo bytes and canonical JSON of fixtures.
use opentrace_core::demo::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn canon(v: &Value) -> Value {
    match v {
        Value::Number(n) if n.is_f64() => json!({"__f": format!("{:016x}", n.as_f64().unwrap().to_bits())}),
        Value::Array(a) => Value::Array(a.iter().map(canon).collect()),
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), canon(x))).collect()),
        o => o.clone(),
    }
}
fn h(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{:02x}", x)).collect()
}
fn fx(name: &str, f: &Fixture) {
    println!("{} sample {}", name, h(&f.sample_bytes));
    println!("{} ladder {}", name, h(&f.ladder_bytes));
    println!("{} kit {}", name, canon(&f.kit));
    println!("{} settings {}", name, canon(&f.settings));
}
fn main() {
    println!("demo {}", h(&demo_bytes()));
    println!("sizing {}", h(&sizing_demo_bytes()));
    println!("pipeline {}", h(&pipeline_demo_bytes()));
    fx("str", &make_str_demo(&StrOpts::default()));
    fx("spike", &make_str_demo(&StrOpts { include_spike: true, ..StrOpts::default() }));
    let r = make_run_demo();
    fx("run", &r.fixture);
    for (n, b) in &r.ladders {
        println!("run ladder {} {}", n, h(b));
    }
    println!("run lq {}", canon(&r.ladder_quality_definition));
    println!("run mq {}", canon(&r.marker_quality_definition));
    let (f, d) = make_cross_check_demo();
    fx("amel", &f);
    println!("amel def {}", canon(&d));
    let (q, c) = make_negative_demo_definitions();
    println!("neg {} {}", canon(&q), canon(&c));
    println!("lq {} {}", canon(&make_ladder_quality_demo_definition(false)), canon(&make_ladder_quality_demo_definition(true)));
    println!("spikedef {}", canon(&make_spike_demo_definition()));
    fx("spadj0", &make_spike_adjustment_demo(false));
    fx("spadj1", &make_spike_adjustment_demo(true));
    fx("morph0", &make_morphology_demo(false, 0));
    fx("morph1", &make_morphology_demo(true, 0));
    fx("tp0", &make_true_peak_demo(false, 0));
    fx("tp1", &make_true_peak_demo(true, 0));
    println!("pudef {} {}", canon(&make_pullup_demo_definition(true)), canon(&make_pullup_demo_definition(false)));
    fx("pu00", &make_pullup_demo(false, false));
    fx("pu01", &make_pullup_demo(false, true));
    fx("pu11", &make_pullup_demo(true, true));
    for k in ["sq-pass", "sq-check", "sq-fail", "sq-ladder-fail"] {
        fx(k, &make_post_sizing_demo(k).unwrap());
    }
    for legacy in [false, true] {
        let (f, q) = make_ladder_calling_demo(legacy);
        fx(&format!("lc{}", legacy), &f);
        println!("lc{} q {}", legacy, canon(&q));
    }
}

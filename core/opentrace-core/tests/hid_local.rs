//! Reads real .hid files when they are available on this machine (never in the repository):
//! OPENTRACE_TEST_HID_DIR=<folder with .hid> OPENTRACE_IDX_GMP_JAR=<GeneMapper ID-X 1.6 GMp.jar> cargo test --test hid_local -- --nocapture
use opentrace_core::abif::AbifFile;

#[test]
fn local_hid_files_decode() {
    let (Some(dir), Some(jar)) = (std::env::var_os("OPENTRACE_TEST_HID_DIR"), std::env::var_os("OPENTRACE_IDX_GMP_JAR")) else {
        eprintln!("skipped: OPENTRACE_TEST_HID_DIR / OPENTRACE_IDX_GMP_JAR not set");
        return;
    };
    let codec = opentrace_core::hid::from_jar_file(std::path::Path::new(&jar)).expect("codec from GMp.jar");
    opentrace_core::hid::install(codec, "test");
    let mut n = 0;
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()).map(|x| x.eq_ignore_ascii_case("hid")) != Some(true) {
            continue;
        }
        let data = std::fs::read(&p).unwrap();
        let f = match AbifFile::parse(&data, &p.file_name().unwrap().to_string_lossy()) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("{}: {}", p.display(), e);
                continue;
            }
        };
        let ch = f.channels().expect("channels");
        let raw: Vec<String> = ch.iter().filter(|c| c.group == "raw").map(|c| format!("{}:{}:{}", c.tag, c.dye, c.values.len())).collect();
        eprintln!("{} v{} raw={:?}", f.filename, f.version, raw);
        assert!(raw.len() >= 4);
        n += 1;
    }
    assert!(n > 0);
}

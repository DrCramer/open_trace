//! HID files (ABIF v3 from 3500-series HID instruments) as GeneMapper ID-X 1.6 reads them.
//!
//! Format (research/IDX16_HID_CODEC.md, static analysis of ID-X 1.6 `GMp.jar`, never executed): a normal ABIF
//! directory whose 28-byte entries carry a codec mode in the high 16 bits of the last four bytes. Typed payloads
//! (element types 27..=35) with mode 1..3 are zlib, mode 4 gzip, mode 5 DES-CBC with PKCS#5 padding; scalar entries
//! with mode 5 are plain. After decoding, type 28 is a big-endian `short[]` (the DATA signals), 33 a C string, 34 a
//! string without a length byte, 35 float, 32 double, 30 char/bytes; the sample count is the decoded length / 2, not
//! the stored element count.
//!
//! The DES key and IV of mode 5 belong to the vendor software and are **not** part of OpenTrace. They are read, as
//! data, from the user's own licensed GeneMapper ID-X installation (`GMp.jar`, class `com/apldbio/shared/ABIF/a/a`):
//! the constructor builds a serialized `java.security.KeyRep` (DES, RAW) and an 8-byte IV from byte-array literals.
//! Nothing from the JAR is executed and Java serialization is not deserialized: the byte arrays are rebuilt from the
//! bytecode, the key bytes are taken from the `encoded` byte array of the KeyRep stream.
use des::cipher::{BlockDecrypt, KeyInit};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Codec {
    key: [u8; 8],
    iv: [u8; 8],
}

impl std::fmt::Debug for Codec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("hid::Codec(..)") // key material is never printed
    }
}

static CODEC: RwLock<Option<(Codec, String)>> = RwLock::new(None);
static CONFIGURED: RwLock<Vec<PathBuf>> = RwLock::new(Vec::new());

/// Paths of `GMp.jar` from the application settings (desktop) or the server configuration, tried first.
pub fn set_configured_jars(paths: Vec<PathBuf>) {
    *CONFIGURED.write().unwrap_or_else(|e| e.into_inner()) = paths;
}

pub fn configured_jars() -> Vec<PathBuf> {
    CONFIGURED.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The codec, loading it on first use from the configured path, `OPENTRACE_IDX_GMP_JAR` or the usual install folder.
pub fn available() -> Option<Codec> {
    current().or_else(|| {
        ensure_from_known_places(&configured_jars());
        current()
    })
}

/// Status for the settings pages (no key bytes): where the codec comes from, or which places were tried.
pub fn status_json() -> serde_json::Value {
    let c = available();
    serde_json::json!({
        "available": c.is_some(),
        "source": source(),
        "configured": configured_jars().iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "searched": candidate_jars().iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
    })
}
const PARAM_CLASS: &str = "com/apldbio/shared/ABIF/a/a.class";

/// Installs the codec for this process; `source` says where it came from (shown in diagnostics, no key bytes).
pub fn install(codec: Codec, source: &str) {
    *CODEC.write().unwrap_or_else(|e| e.into_inner()) = Some((codec, source.to_string()));
}

pub fn current() -> Option<Codec> {
    CODEC.read().unwrap_or_else(|e| e.into_inner()).as_ref().map(|c| c.0)
}

pub fn source() -> Option<String> {
    CODEC.read().unwrap_or_else(|e| e.into_inner()).as_ref().map(|c| c.1.clone())
}

/// Usual install locations of GeneMapper ID-X, then `OPENTRACE_IDX_GMP_JAR`.
pub fn candidate_jars() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = std::env::var_os("OPENTRACE_IDX_GMP_JAR").filter(|p| !p.is_empty()) {
        v.push(PathBuf::from(p));
    }
    if cfg!(windows) {
        for drive in ["C", "D", "E"] {
            v.push(PathBuf::from(format!("{}:\\AppliedBiosystems\\GeneMapperID-X\\app\\GMp.jar", drive)));
        }
    }
    v
}

/// Installs the codec from the first readable candidate unless one is installed already. Returns the source.
pub fn ensure_from_known_places(extra: &[PathBuf]) -> Option<String> {
    if current().is_some() {
        return source();
    }
    for p in extra.iter().cloned().chain(candidate_jars()) {
        if !p.is_file() {
            continue;
        }
        if let Ok(c) = from_jar_file(&p) {
            let s = p.display().to_string();
            install(c, &s);
            return Some(s);
        }
    }
    None
}

pub fn from_jar_file(path: &Path) -> Result<Codec, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| format!("{}: not a JAR ({})", path.display(), e))?;
    let mut class = Vec::new();
    zip.by_name(PARAM_CLASS)
        .map_err(|_| format!("{}: no {} (not GeneMapper ID-X 1.6 GMp.jar?)", path.display(), PARAM_CLASS))?
        .read_to_end(&mut class)
        .map_err(|e| e.to_string())?;
    from_class(&class)
}

/// Key and IV from the parameter class bytes (see module docs).
pub fn from_class(class: &[u8]) -> Result<Codec, String> {
    let arrays = constructor_byte_arrays(class)?;
    let keyrep = arrays.iter().find(|a| a.len() > 64 && a.starts_with(&[0xAC, 0xED])).ok_or("no serialized key in the class")?;
    let iv = arrays.iter().find(|a| a.len() == 8).ok_or("no 8-byte IV in the class")?;
    let key = keyrep_des_key(keyrep)?;
    let mut ivb = [0u8; 8];
    ivb.copy_from_slice(iv);
    Ok(Codec { key, iv: ivb })
}

/// The key bytes of a serialized `java.security.KeyRep` for DES/RAW: the first byte array of the stream (fields of
/// KeyRep are written algorithm, encoded, format, type; `encoded` is the only array).
fn keyrep_des_key(s: &[u8]) -> Result<[u8; 8], String> {
    if s.len() < 8 || s[0..2] != [0xAC, 0xED] {
        return Err("key: not a Java serialization stream".into());
    }
    let has = |needle: &[u8]| s.windows(needle.len()).any(|w| w == needle);
    if !has(b"java.security.KeyRep") || !has(b"DES") || !has(b"RAW") {
        return Err("key: not a DES/RAW KeyRep".into());
    }
    // TC_ARRAY TC_CLASSDESC "[B" serialVersionUID(8) flags(1) fields(2)=0 TC_ENDBLOCKDATA TC_NULL length(4) bytes
    let pat = [0x75u8, 0x72, 0x00, 0x02, b'[', b'B'];
    let i = s.windows(pat.len()).position(|w| w == pat).ok_or("key: no byte array in KeyRep")?;
    let mut p = i + pat.len() + 8 + 1;
    if s.get(p..p + 2) != Some(&[0, 0]) {
        return Err("key: unexpected byte[] class description".into());
    }
    p += 2;
    if s.get(p..p + 2) != Some(&[0x78, 0x70]) {
        return Err("key: unexpected byte[] class description".into());
    }
    p += 2;
    let len = s.get(p..p + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]])).ok_or("key: truncated")?;
    if len != 8 {
        return Err(format!("key: {} bytes, DES needs 8", len));
    }
    let mut k = [0u8; 8];
    k.copy_from_slice(s.get(p + 4..p + 12).ok_or("key: truncated")?);
    Ok(k)
}

// ---------- class file: byte-array literals built in <init> ----------

fn u16be(b: &[u8], p: usize) -> Result<u16, String> {
    b.get(p..p + 2).map(|x| u16::from_be_bytes([x[0], x[1]])).ok_or_else(|| "class: truncated".to_string())
}
fn u32be(b: &[u8], p: usize) -> Result<u32, String> {
    b.get(p..p + 4).map(|x| u32::from_be_bytes([x[0], x[1], x[2], x[3]])).ok_or_else(|| "class: truncated".to_string())
}

/// Bytecode of `<init>` methods; for each `newarray byte` the array its `dup / index / value / bastore` sequence fills.
fn constructor_byte_arrays(c: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    if u32be(c, 0)? != 0xCAFEBABE {
        return Err("class: bad magic".into());
    }
    let cp_count = u16be(c, 8)? as usize;
    let mut utf8: Vec<Option<String>> = vec![None; cp_count];
    let mut p = 10;
    let mut i = 1;
    while i < cp_count {
        let tag = *c.get(p).ok_or("class: truncated constant pool")?;
        p += 1;
        match tag {
            1 => {
                let n = u16be(c, p)? as usize;
                utf8[i] = Some(String::from_utf8_lossy(c.get(p + 2..p + 2 + n).ok_or("class: truncated")?).into_owned());
                p += 2 + n;
            }
            3 | 4 => p += 4,
            5 | 6 => {
                p += 8;
                i += 1;
            }
            7 | 8 | 16 | 19 | 20 => p += 2,
            9 | 10 | 11 | 12 | 17 | 18 => p += 4,
            15 => p += 3,
            t => return Err(format!("class: constant pool tag {}", t)),
        }
        i += 1;
    }
    p += 6; // access, this, super
    let ifaces = u16be(c, p)? as usize;
    p += 2 + 2 * ifaces;
    let skip_attrs = |mut p: usize| -> Result<usize, String> {
        let n = u16be(c, p)? as usize;
        p += 2;
        for _ in 0..n {
            p += 6 + u32be(c, p + 2)? as usize;
        }
        Ok(p)
    };
    let fields = u16be(c, p)? as usize;
    p += 2;
    for _ in 0..fields {
        p = skip_attrs(p + 6)?;
    }
    let methods = u16be(c, p)? as usize;
    p += 2;
    let mut out = Vec::new();
    for _ in 0..methods {
        let name = utf8.get(u16be(c, p + 2)? as usize).cloned().flatten().unwrap_or_default();
        let n = u16be(c, p + 6)? as usize;
        let mut q = p + 8;
        for _ in 0..n {
            let an = utf8.get(u16be(c, q)? as usize).cloned().flatten().unwrap_or_default();
            let len = u32be(c, q + 2)? as usize;
            if name == "<init>" && an == "Code" {
                let code_len = u32be(c, q + 10)? as usize;
                let code = c.get(q + 14..q + 14 + code_len).ok_or("class: truncated code")?;
                out.extend(byte_arrays_in(code));
            }
            q += 6 + len;
        }
        p = q;
    }
    Ok(out)
}

/// Integer pushed by the instruction at `p`: (value, length) for iconst_*, bipush, sipush.
fn push_int(code: &[u8], p: usize) -> Option<(i32, usize)> {
    match *code.get(p)? {
        op @ 0x02..=0x08 => Some((op as i32 - 3, 1)),
        0x10 => Some((*code.get(p + 1)? as i8 as i32, 2)),
        0x11 => Some((i16::from_be_bytes([*code.get(p + 1)?, *code.get(p + 2)?]) as i32, 3)),
        _ => None,
    }
}

fn byte_arrays_in(code: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < code.len() {
        // <count> newarray byte(8)
        if let Some((count, l)) = push_int(code, p) {
            if code.get(p + l) == Some(&0xBC) && code.get(p + l + 1) == Some(&8) && count > 0 && count <= 65536 {
                let mut arr = vec![0u8; count as usize];
                let mut q = p + l + 2;
                // dup <index> <value> bastore, repeated
                while code.get(q) == Some(&0x59) {
                    let Some((idx, l1)) = push_int(code, q + 1) else { break };
                    let Some((val, l2)) = push_int(code, q + 1 + l1) else { break };
                    if code.get(q + 1 + l1 + l2) != Some(&0x54) || idx < 0 || idx >= count {
                        break;
                    }
                    arr[idx as usize] = val as u8;
                    q += 1 + l1 + l2 + 1;
                }
                out.push(arr);
                p = q;
                continue;
            }
        }
        p += 1;
    }
    out
}

// ---------- payload decoding ----------

pub fn des_cbc_decrypt(c: &Codec, data: &[u8]) -> Result<Vec<u8>, String> {
    if data.is_empty() || data.len() % 8 != 0 {
        return Err(format!("encrypted payload of {} bytes is not a multiple of 8", data.len()));
    }
    let cipher = des::Des::new_from_slice(&c.key).map_err(|e| e.to_string())?;
    let mut prev = c.iv;
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(8) {
        let mut block = des::cipher::generic_array::GenericArray::clone_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        for k in 0..8 {
            out.push(block[k] ^ prev[k]);
        }
        prev.copy_from_slice(chunk);
    }
    let pad = *out.last().unwrap() as usize;
    if pad == 0 || pad > 8 || out.len() < pad || out[out.len() - pad..].iter().any(|&b| b as usize != pad) {
        return Err("wrong HID key or corrupt payload (padding check failed)".into());
    }
    out.truncate(out.len() - pad);
    Ok(out)
}

/// Decodes the payload of a typed entry (types 27..=35) by codec mode; other entries are returned unchanged.
pub fn decode_payload(element_type: u16, mode: u16, data: &[u8]) -> Result<Vec<u8>, String> {
    if mode == 0 || !(27..=35).contains(&element_type) {
        return Ok(data.to_vec());
    }
    match mode {
        1..=3 => {
            let mut out = Vec::new();
            flate2::read::ZlibDecoder::new(data).take(64 << 20).read_to_end(&mut out).map_err(|e| format!("zlib: {}", e))?;
            Ok(out)
        }
        4 => {
            let mut out = Vec::new();
            flate2::read::GzDecoder::new(data).take(64 << 20).read_to_end(&mut out).map_err(|e| format!("gzip: {}", e))?;
            Ok(out)
        }
        5 => {
            let c = available().ok_or_else(|| {
                "HID data is encrypted; reading it needs the GeneMapper ID-X 1.6 installation (GMp.jar). Set its path in the \
                 settings or OPENTRACE_IDX_GMP_JAR."
                    .to_string()
            })?;
            des_cbc_decrypt(&c, data)
        }
        m => Err(format!("unknown HID codec mode {}", m)),
    }
}

/// Standard ABIF element type and element size for a decoded HID type.
pub fn plain_type(element_type: u16) -> (u16, u16) {
    match element_type {
        27 => (1, 1),  // byte
        28 => (4, 2),  // short
        29 => (2, 1),  // IString (kept as bytes)
        30 => (2, 1),  // char
        31 => (5, 4),  // int: OvrV/OvrI override values and scan indices (checked on 3500 HID files)
        32 => (8, 8),  // double
        33 => (2, 1),  // C string (read as text, trailing NUL dropped)
        34 => (2, 1),  // string without length byte: the whole payload is the text (ID-X `new String(decoded)`)
        35 => (7, 4),  // float
        t => (t, 1),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use des::cipher::BlockEncrypt;

    /// Test codec (not the vendor's): only for files built by `encrypt` below.
    pub fn test_codec() -> Codec {
        Codec { key: *b"OTtestK1", iv: *b"OTtestIV" }
    }

    pub fn encrypt(c: &Codec, data: &[u8]) -> Vec<u8> {
        let cipher = des::Des::new_from_slice(&c.key).unwrap();
        let pad = 8 - data.len() % 8;
        let mut buf = data.to_vec();
        buf.extend(std::iter::repeat(pad as u8).take(pad));
        let mut prev = c.iv;
        let mut out = Vec::new();
        for chunk in buf.chunks(8) {
            let mut b = [0u8; 8];
            for k in 0..8 {
                b[k] = chunk[k] ^ prev[k];
            }
            let mut block = des::cipher::generic_array::GenericArray::clone_from_slice(&b);
            cipher.encrypt_block(&mut block);
            prev.copy_from_slice(&block);
            out.extend_from_slice(&block);
        }
        out
    }

    #[test]
    fn des_cbc_roundtrip_and_padding_check() {
        let c = test_codec();
        for n in [0usize, 1, 7, 8, 9, 100] {
            let data: Vec<u8> = (0..n as u8).collect();
            assert_eq!(des_cbc_decrypt(&c, &encrypt(&c, &data)).unwrap(), data);
        }
        let wrong = Codec { key: *b"wrongkey", iv: c.iv };
        assert!(des_cbc_decrypt(&wrong, &encrypt(&c, b"hello world")).is_err());
    }

    /// An ABIF v1 file turned into a v3 "HID": every DATA entry DES-encrypted (mode 5, type 28) at the end of the file,
    /// the dye names as type-34 strings without a length byte.
    pub fn to_hid(fsa: &[u8], c: &Codec) -> Vec<u8> {
        let mut d = fsa.to_vec();
        d[4..6].copy_from_slice(&300u16.to_be_bytes());
        let rd = |d: &[u8], p: usize| u32::from_be_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]]) as usize;
        let (n, dir) = (rd(&d, 18), rd(&d, 26));
        let mut extra: Vec<(usize, Vec<u8>, u16)> = Vec::new();
        for i in 0..n {
            let o = dir + i * 28;
            let name = &fsa[o..o + 4];
            let (ty, size, off) = (u16::from_be_bytes([fsa[o + 8], fsa[o + 9]]), rd(fsa, o + 16), rd(fsa, o + 20));
            let payload = if size <= 4 { fsa[o + 20..o + 20 + size].to_vec() } else { fsa[off..off + size].to_vec() };
            if name == b"DATA" && ty == 4 {
                extra.push((o, encrypt(c, &payload), 28));
            } else if name == b"DyeN" && ty == 18 && !payload.is_empty() {
                extra.push((o, encrypt(c, &payload[1..]), 34));
            }
        }
        for (o, enc, ty) in extra {
            let at = d.len();
            d.extend_from_slice(&enc);
            d[o + 8..o + 10].copy_from_slice(&ty.to_be_bytes());
            d[o + 10..o + 12].copy_from_slice(&1u16.to_be_bytes());
            d[o + 12..o + 16].copy_from_slice(&(enc.len() as u32).to_be_bytes());
            d[o + 16..o + 20].copy_from_slice(&(enc.len() as u32).to_be_bytes());
            d[o + 20..o + 24].copy_from_slice(&(at as u32).to_be_bytes());
            d[o + 24..o + 26].copy_from_slice(&5u16.to_be_bytes());
        }
        d
    }

    #[test]
    fn hid_file_reads_like_its_fsa() {
        let fsa = crate::demo::demo_bytes();
        let hid = to_hid(&fsa, &test_codec());
        install(test_codec(), "test");
        let a = crate::abif::AbifFile::parse(&fsa, "x.fsa").unwrap();
        let b = crate::abif::AbifFile::parse(&hid, "x.hid").unwrap();
        assert_eq!(b.version, 300);
        let (ca, cb) = (a.channels().unwrap(), b.channels().unwrap());
        assert_eq!(ca.len(), cb.len());
        for (x, y) in ca.iter().zip(&cb) {
            assert_eq!((x.tag.as_str(), &x.dye, &x.values), (y.tag.as_str(), &y.dye, &y.values));
        }
        // wrong key: signals and dye names are reported, not silently dropped
        install(Codec { key: *b"wrongkey", iv: *b"OTtestIV" }, "test");
        assert!(crate::abif::AbifFile::parse(&hid, "x.hid").unwrap_err().0.contains("padding check failed"));
        install(test_codec(), "test");
    }

    #[test]
    fn keyrep_and_bytecode_literals() {
        // A minimal KeyRep-like stream and a constructor that builds it plus an IV, as javac emits byte[] literals.
        let mut keyrep = vec![0xAC, 0xED, 0, 5];
        keyrep.extend(b"java.security.KeyRep DES RAW ");
        keyrep.extend([0x75, 0x72, 0, 2, b'[', b'B']);
        keyrep.extend([0xAC, 0xF3, 0x17, 0xF8, 0x06, 0x08, 0x54, 0xE0]); // serialVersionUID of byte[]
        keyrep.extend([0x02, 0, 0, 0x78, 0x70, 0, 0, 0, 8]);
        keyrep.extend(b"OTtestK1");
        keyrep.extend(vec![0x70; 80]);
        let mut code = Vec::new();
        for arr in [keyrep.clone(), b"OTtestIV".to_vec()] {
            code.extend([0x11, (arr.len() >> 8) as u8, arr.len() as u8, 0xBC, 8]);
            for (i, b) in arr.iter().enumerate() {
                code.push(0x59);
                if i <= 5 { code.push(0x03 + i as u8) } else if i < 128 { code.extend([0x10, i as u8]) } else { code.extend([0x11, (i >> 8) as u8, i as u8]) }
                code.extend([0x10, *b]);
                code.push(0x54);
            }
            code.extend([0xB5, 0, 1]);
        }
        let arrays = byte_arrays_in(&code);
        assert_eq!(arrays.len(), 2);
        assert_eq!(arrays[0], keyrep);
        assert_eq!(keyrep_des_key(&arrays[0]).unwrap(), *b"OTtestK1");
    }
}

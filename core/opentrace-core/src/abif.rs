//! Port of `genemapper_open/abif.py`: bounds-checked reader of the public ABIF v1 directory and data types.
//!
//! No vendor software or algorithms are used. Unknown data stays opaque (`Value::Bytes`).
use std::collections::BTreeMap;
use std::fmt;

pub const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_ENTRIES: u32 = 20000;
const ENTRY_SIZE: usize = 28;

#[derive(Debug, Clone, PartialEq)]
pub struct AbifError(pub String);

impl fmt::Display for AbifError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AbifError {}

type Result<T> = std::result::Result<T, AbifError>;

fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(AbifError(msg.into()))
}

/// A numeric sample as Python sees it: `int` for integer element types, `float` for real ones.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Num {
    Int(i64),
    Float(f64),
}

/// Decoded standard data of a directory entry (`Tag.value()` in Python).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bytes(Vec<u8>),
    Str(String),
    Int(i64),
    Float(f64),
    Ints(Vec<i64>),
    Floats(Vec<f64>),
    Date(Vec<i64>),
    Dates(Vec<Vec<i64>>),
}

impl Value {
    fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Tag {
    pub name: String,
    pub number: i32,
    pub element_type: u16,
    pub element_size: u16,
    pub count: u32,
    pub size: u32,
    pub offset: u32,
    pub data: Vec<u8>,
}

impl Tag {
    pub fn key(&self) -> String {
        format!("{}{}", self.name, self.number)
    }

    fn numeric_spec(kind: u16) -> Option<(char, usize)> {
        match kind {
            1 => Some(('B', 1)),
            3 => Some(('H', 2)),
            4 => Some(('h', 2)),
            5 => Some(('i', 4)),
            7 => Some(('f', 4)),
            8 => Some(('d', 8)),
            13 => Some(('B', 1)),
            _ => None,
        }
    }

    /// Decode standard data; unknown types remain bytes. Mirrors `Tag.value()`.
    pub fn value(&self) -> Result<Value> {
        let kind = self.element_type;
        if let Some((code, width)) = Self::numeric_spec(kind) {
            if self.element_size as usize != width || self.size as usize != self.count as usize * width {
                return err(format!("{}: numeric count/size mismatch", self.key()));
            }
            let items: Vec<Num> = self
                .data
                .chunks_exact(width)
                .map(|c| match code {
                    'B' => Num::Int(c[0] as i64),
                    'H' => Num::Int(u16::from_be_bytes([c[0], c[1]]) as i64),
                    'h' => Num::Int(i16::from_be_bytes([c[0], c[1]]) as i64),
                    'i' => Num::Int(i32::from_be_bytes([c[0], c[1], c[2], c[3]]) as i64),
                    'f' => Num::Float(f32::from_be_bytes([c[0], c[1], c[2], c[3]]) as f64),
                    _ => Num::Float(f64::from_be_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]])),
                })
                .collect();
            return Ok(match (self.count, items.as_slice()) {
                (1, [Num::Int(v)]) => Value::Int(*v),
                (1, [Num::Float(v)]) => Value::Float(*v),
                _ => {
                    if items.iter().all(|n| matches!(n, Num::Int(_))) {
                        Value::Ints(items.iter().map(|n| if let Num::Int(v) = n { *v } else { 0 }).collect())
                    } else {
                        Value::Floats(items.iter().map(|n| if let Num::Float(v) = n { *v } else { f64::NAN }).collect())
                    }
                }
            });
        }
        if matches!(kind, 2 | 18 | 19) && (self.element_size != 1 || self.count != self.size) {
            return err(format!("{}: string count/size mismatch", self.key()));
        }
        match kind {
            2 => {
                let text = ascii_replace(&self.data);
                Ok(Value::Str(text.trim_end_matches('\0').to_string()))
            }
            18 => {
                if self.data.is_empty() || self.data[0] as u32 + 1 != self.size {
                    return err(format!("{}: truncated Pascal string", self.key()));
                }
                let n = self.data[0] as usize;
                Ok(Value::Str(ascii_replace(&self.data[1..1 + n])))
            }
            19 => {
                if self.data.is_empty() || *self.data.last().unwrap() != 0 {
                    return err(format!("{}: unterminated C string", self.key()));
                }
                Ok(Value::Str(ascii_replace(&self.data[..self.data.len() - 1])))
            }
            10 | 11 => {
                if self.element_size != 4 || self.size as usize != self.count as usize * 4 {
                    return err(format!("{}: date/time size mismatch", self.key()));
                }
                let values: Vec<Vec<i64>> = self
                    .data
                    .chunks_exact(4)
                    .map(|c| {
                        if kind == 10 {
                            vec![u16::from_be_bytes([c[0], c[1]]) as i64, c[2] as i64, c[3] as i64]
                        } else {
                            vec![c[0] as i64, c[1] as i64, c[2] as i64, c[3] as i64]
                        }
                    })
                    .collect();
                Ok(if self.count == 1 {
                    Value::Date(values.into_iter().next().unwrap_or_default())
                } else {
                    Value::Dates(values)
                })
            }
            _ => Ok(Value::Bytes(self.data.clone())),
        }
    }
}

/// Python `bytes.decode("ascii", "replace")`: every byte above 0x7F becomes U+FFFD.
fn ascii_replace(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| if b < 0x80 { b as char } else { '\u{FFFD}' }).collect()
}

fn read_u16(d: &[u8], p: usize) -> u16 {
    u16::from_be_bytes([d[p], d[p + 1]])
}

fn read_u32(d: &[u8], p: usize) -> u32 {
    u32::from_be_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]])
}

fn check_range(data_len: usize, offset: usize, size: usize, label: &str) -> Result<()> {
    if offset < 128 || offset > data_len || size > data_len - offset {
        return err(format!("{}: range outside file", label));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct AbifFile {
    pub filename: String,
    pub version: u16,
    pub tags: BTreeMap<(String, i32), Tag>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    pub tag: String,
    pub number: i32,
    pub group: &'static str,
    pub dye_index: Option<i32>,
    pub dye: String,
    pub wavelength_nm: Option<i64>,
    pub values: Vec<Num>,
    pub truncated_to: Option<usize>,
}

impl AbifFile {
    pub fn parse(data: &[u8], filename: &str) -> Result<AbifFile> {
        if data.len() > MAX_FILE_BYTES {
            return err("File exceeds 64 MiB limit");
        }
        if data.len() < 128 || &data[..4] != b"ABIF" {
            return err("Expected ABIF signature and 128-byte header");
        }
        let version = read_u16(data, 4);
        if version / 100 != 1 {
            return err(format!(
                "Unsupported ABIF version {}; this reader supports v1. HID/ABIF v3 requires a separate documented decoder.",
                version
            ));
        }
        // Root entry at offset 6: count is the 'count' field, directory is the 'offset' field.
        let count = read_u32(data, 6 + 12);
        let directory = read_u32(data, 6 + 20) as usize;
        if count > MAX_ENTRIES {
            return err("Too many directory entries");
        }
        check_range(data.len(), directory, count as usize * ENTRY_SIZE, "directory")?;
        let mut tags = BTreeMap::new();
        for i in 0..count as usize {
            let pos = directory + i * ENTRY_SIZE;
            let raw_name = &data[pos..pos + 4];
            if raw_name.iter().any(|&c| !(0x21..=0x7e).contains(&c)) {
                return err(format!("Invalid ASCII tag name at entry {}", i));
            }
            let name = String::from_utf8_lossy(raw_name).into_owned();
            let number = read_u32(data, pos + 4) as i32;
            let kind = read_u16(data, pos + 8);
            let width = read_u16(data, pos + 10);
            let n = read_u32(data, pos + 12);
            let size = read_u32(data, pos + 16);
            let offset = read_u32(data, pos + 20);
            let key = (name.clone(), number);
            if tags.contains_key(&key) {
                return err(format!("Duplicate tag {}{}", name, number));
            }
            let payload = if size <= 4 {
                data[pos + 20..pos + 20 + size as usize].to_vec()
            } else {
                check_range(data.len(), offset as usize, size as usize, &format!("{}{}", name, number))?;
                data[offset as usize..offset as usize + size as usize].to_vec()
            };
            let tag = Tag { name, number, element_type: kind, element_size: width, count: n, size, offset, data: payload };
            if Tag::numeric_spec(kind).is_some() || matches!(kind, 2 | 10 | 11 | 18 | 19) {
                tag.value()?;
            }
            tags.insert(key, tag);
        }
        Ok(AbifFile { filename: filename.rsplit('/').next().unwrap_or(filename).to_string(), version, tags })
    }

    pub fn value(&self, name: &str, number: i32) -> Option<Value> {
        self.tags.get(&(name.to_string(), number)).map(|t| t.value().expect("validated on parse"))
    }

    /// Python `text()`: the string value, or empty text when absent or not a string.
    pub fn text(&self, name: &str, number: i32) -> String {
        match self.value(name, number) {
            Some(Value::Str(s)) => s,
            _ => String::new(),
        }
    }

    /// Numeric DATA arrays with the explicit dye mapping of `AbifFile.channels()`.
    pub fn channels(&self) -> Result<Vec<Channel>> {
        let fragment2 = self.text("SVER", 4).trim().to_lowercase() == "fragment format 2.0";
        let edr = fragment2 && matches!(self.value("EDRF", 1), Some(Value::Int(1)));
        let mut channels = Vec::new();
        for ((name, number), tag) in &self.tags {
            if name != "DATA" || Tag::numeric_spec(tag.element_type).is_none() {
                continue;
            }
            let mut values = match tag.value()? {
                Value::Int(v) => vec![Num::Int(v)],
                Value::Float(v) => vec![Num::Float(v)],
                Value::Ints(v) => v.into_iter().map(Num::Int).collect(),
                Value::Floats(v) => v.into_iter().map(Num::Float).collect(),
                _ => continue,
            };
            if values.is_empty() {
                continue;
            }
            if fragment2 {
                // ID-X IfaAbifReader.getAllRawElectropherograms: sparse overrides by index (OvrV/OvrI, EDRV/EDRI).
                let pairs: &[(&str, &str)] = if edr { &[("OvrV", "OvrI"), ("EDRV", "EDRI")] } else { &[("OvrV", "OvrI")] };
                for (vt, it) in pairs {
                    let ov = self.value(vt, *number);
                    let oi = self.value(it, *number);
                    if ov.is_none() && oi.is_none() {
                        continue;
                    }
                    let (ov, oi) = match (ov, oi) {
                        (Some(a), Some(b)) => (to_nums(a)?, to_idx(b)?),
                        _ => return err(format!("DATA{}: {}/{} must both be present", number, vt, it)),
                    };
                    if ov.len() != oi.len() {
                        return err(format!("DATA{}: {}/{} have different sizes of values and indices", number, vt, it));
                    }
                    for (v, i) in ov.into_iter().zip(oi) {
                        if i < 0 || i as usize >= values.len() {
                            return err(format!("DATA{}: {} contains invalid indices", number, it));
                        }
                        values[i as usize] = v;
                    }
                }
            }
            let (dye_index, group): (Option<i32>, &'static str) = match *number {
                1..=4 | 105 | 106 => (Some(if *number <= 4 { *number } else { *number - 100 }), "raw"),
                9..=12 | 205 | 206 => (Some(if *number <= 12 { *number - 8 } else { *number - 200 }), "analyzed"),
                _ => (None, "auxiliary"),
            };
            let dye = match dye_index {
                Some(d) if d != 0 => self.text("DyeN", d),
                _ => String::new(),
            };
            let wavelength_nm = match dye_index {
                Some(d) if d != 0 => self.value("DyeW", d).and_then(|v| v.as_int()),
                _ => None,
            };
            channels.push(Channel {
                tag: tag.key(),
                number: *number,
                group,
                dye_index,
                dye,
                wavelength_nm,
                values,
                truncated_to: None,
            });
        }
        // ID-X: dye traces of different lengths are all cut to the shortest one (no padding).
        let shortest = channels.iter().filter(|c| c.group == "raw").map(|c| c.values.len()).min();
        if let Some(n) = shortest {
            for ch in channels.iter_mut().filter(|c| c.group == "raw") {
                if ch.values.len() > n {
                    ch.values.truncate(n);
                    ch.truncated_to = Some(n);
                }
            }
        }
        Ok(channels)
    }
}

fn to_nums(v: Value) -> Result<Vec<Num>> {
    Ok(match v {
        Value::Int(x) => vec![Num::Int(x)],
        Value::Float(x) => vec![Num::Float(x)],
        Value::Ints(xs) => xs.into_iter().map(Num::Int).collect(),
        Value::Floats(xs) => xs.into_iter().map(Num::Float).collect(),
        _ => return err("override values must be numeric"),
    })
}

fn to_idx(v: Value) -> Result<Vec<i64>> {
    Ok(match v {
        Value::Int(x) => vec![x],
        Value::Ints(xs) => xs,
        _ => return err("override indices must be integers"),
    })
}

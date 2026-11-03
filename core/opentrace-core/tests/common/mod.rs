//! Helpers shared by the golden tests.
#![allow(dead_code)]

pub fn b64(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        } as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

pub fn replace(data: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    if from.is_empty() {
        return data.to_vec();
    }
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i..].starts_with(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.push(data[i]);
            i += 1;
        }
    }
    out
}

fn hex(b: u8) -> bool {
    b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
}

/// UUIDs and `YYYY-MM-DDTHH:MM:SS[.ffffff]+00:00` timestamps become fixed markers.
pub fn mask(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        let rest = &data[i..];
        if rest.len() >= 36 && rest[..36].iter().enumerate().all(|(k, &c)| if [8, 13, 18, 23].contains(&k) { c == b'-' } else { hex(c) }) {
            out.extend_from_slice(b"@UUID@");
            i += 36;
            continue;
        }
        if rest.len() >= 25 && rest.starts_with(b"20") {
            let d = |k: usize| rest[k].is_ascii_digit();
            if (0..4).all(d) && rest[4] == b'-' && d(5) && d(6) && rest[7] == b'-' && d(8) && d(9) && rest[10] == b'T' && d(11) && d(12) && rest[13] == b':' && d(14) && d(15) && rest[16] == b':' && d(17) && d(18) {
                let mut j = 19;
                if rest.get(j) == Some(&b'.') {
                    j += 1;
                    while rest.get(j).map_or(false, |c| c.is_ascii_digit()) {
                        j += 1;
                    }
                }
                if rest[j..].starts_with(b"+00:00") {
                    out.extend_from_slice(b"@TS@");
                    i += j + 6;
                    continue;
                }
            }
        }
        out.push(data[i]);
        i += 1;
    }
    out
}


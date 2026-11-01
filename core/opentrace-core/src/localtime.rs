//! Portable local wall-clock time (`time.localtime()` / `strftime("%Y-%m-%dT%H:%M:%S")`).
//!
//! Unix keeps the original `libc::localtime_r` call (identical output to the previous code). Windows has no
//! `localtime_r`; the UCRT equivalent is `_localtime64_s`, whose `struct tm` is the nine leading `int`
//! fields of the ISO C layout (no `tm_gmtoff`/`tm_zone`).

/// Broken-down local time; fields follow ISO C `struct tm` semantics (year since 1900, month 0-11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocalTm {
    pub year: i32,
    pub mon: i32,
    pub mday: i32,
    pub hour: i32,
    pub min: i32,
    pub sec: i32,
}

impl LocalTm {
    /// `%Y-%m-%dT%H:%M:%S`.
    pub fn iso(&self) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            self.year + 1900,
            self.mon + 1,
            self.mday,
            self.hour,
            self.min,
            self.sec
        )
    }
}

fn unix_seconds() -> i64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

/// Current local time.
pub fn now() -> LocalTm {
    from_unix(unix_seconds())
}

#[cfg(unix)]
pub fn from_unix(t: i64) -> LocalTm {
    let t = t as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { !libc::localtime_r(&t, &mut tm).is_null() };
    if !ok {
        return utc(t as i64);
    }
    LocalTm { year: tm.tm_year, mon: tm.tm_mon, mday: tm.tm_mday, hour: tm.tm_hour, min: tm.tm_min, sec: tm.tm_sec }
}

#[cfg(windows)]
pub fn from_unix(t: i64) -> LocalTm {
    #[repr(C)]
    struct WinTm {
        tm_sec: i32,
        tm_min: i32,
        tm_hour: i32,
        tm_mday: i32,
        tm_mon: i32,
        tm_year: i32,
        tm_wday: i32,
        tm_yday: i32,
        tm_isdst: i32,
    }
    extern "C" {
        fn _localtime64_s(tm: *mut WinTm, time: *const i64) -> i32;
    }
    let mut tm: WinTm = unsafe { std::mem::zeroed() };
    let rc = unsafe { _localtime64_s(&mut tm, &t) };
    if rc != 0 {
        return utc(t);
    }
    LocalTm { year: tm.tm_year, mon: tm.tm_mon, mday: tm.tm_mday, hour: tm.tm_hour, min: tm.tm_min, sec: tm.tm_sec }
}

#[cfg(not(any(unix, windows)))]
pub fn from_unix(t: i64) -> LocalTm {
    utc(t)
}

/// UTC breakdown (fallback when the C library refuses the timestamp).
pub fn utc(t: i64) -> LocalTm {
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    LocalTm {
        year: (y - 1900) as i32,
        mon: (m - 1) as i32,
        mday: d as i32,
        hour: (secs / 3600) as i32,
        min: ((secs % 3600) / 60) as i32,
        sec: (secs % 60) as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_breakdown() {
        assert_eq!(utc(0).iso(), "1970-01-01T00:00:00");
        assert_eq!(utc(951_782_400).iso(), "2000-02-29T00:00:00");
        assert_eq!(utc(1_704_067_199).iso(), "2023-12-31T23:59:59");
    }

    #[test]
    fn now_is_plausible() {
        let n = now();
        assert!(n.year + 1900 >= 2024);
        assert!((0..12).contains(&n.mon) && (1..=31).contains(&n.mday));
    }
}

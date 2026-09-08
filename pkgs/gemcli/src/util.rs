//! Small shared helpers: UTC civil time (no chrono — the rootfs closure
//! stays lean; the device runs UTC), sleeps, hex parsing, io wrapping.

use std::path::Path;
use std::time::Duration;

use crate::error::{cmsg, Res};

pub fn sleep(secs: f64) {
    std::thread::sleep(Duration::from_millis((secs * 1000.0) as u64));
}

/// Read an entire file to a trimmed String.
pub fn read_str(p: &str) -> Res<String> {
    let s = std::fs::read_to_string(p)
        .map_err(|e| cmsg(format!("{}: {}", p, e)))?;
    Ok(s.trim().to_string())
}

pub fn read_str_opt(p: &str) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

pub fn write_str(p: &str, data: &str) -> Res<()> {
    std::fs::write(p, data).map_err(|e| cmsg(format!("{}: {}", p, e)))
}

// --- civil time (UTC) ---------------------------------------------------
// Howard Hinnant's civil_from_days (public-domain algorithm); the
// device's NixOS runs UTC so no TZ table is needed.

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Current UTC wall time as (y, mo, d, h, mi, s).
pub fn now_parts() -> (i64, u32, u32, u32, u32, u32) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (y, mo, d) = civil_from_days(days);
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    (y, mo, d, h as u32, mi as u32, s as u32)
}

/// "%F %T" — full stamp for logs/state ("YYYY-MM-DD HH:MM:SS").
pub fn stamp() -> String {
    let (y, mo, d, h, mi, s) = now_parts();
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

/// "%H:%M:%S" — the bq25896-raw.sh single-sample timestamp.
pub fn hms() -> String {
    let (_, _, _, h, mi, s) = now_parts();
    format!("{h:02}:{mi:02}:{s:02}")
}

pub fn exists(p: &str) -> bool {
    Path::new(p).exists()
}

/// Best-effort `sync()` (the WDT-reboot script syncs before arming).
pub fn sync_all() {
    unsafe { libc::sync() };
}

#[cfg(test)]
mod tests {
    // inverse of civil_from_days, local to the test (public-domain algo)
    fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
        let y = if m <= 2 { y - 1 } else { y };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let mp = if m > 2 { m - 3 } else { m + 9 };
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe - 719468
    }

    #[test]
    fn epoch_is_1970_01_01() {
        assert_eq!(super::civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn civil_roundtrip_known_dates() {
        let dates: [(i64, u32, u32); 4] = [
            (2026, 9, 9),
            (2024, 2, 29), // leap day
            (2000, 1, 1),
            (1972, 12, 31),
        ];
        for (y, m, d) in dates {
            let z = days_from_civil(y, m as i64, d as i64);
            assert_eq!(super::civil_from_days(z), (y, m, d));
        }
    }
}

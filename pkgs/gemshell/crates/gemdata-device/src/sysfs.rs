//! sysfs helpers: power-supply / cpufreq / backlight / cpu reading and
//! the cpu online-map parser.

use std::path::PathBuf;

use crate::error::Res;
use crate::util;

/// Parse a Linux cpumask-style range list ("0-7,9") into cpu numbers.
pub fn parse_range_list(s: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((a, b)) = part.split_once('-') {
            if let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) {
                for c in a..=b {
                    out.push(c);
                }
            }
        } else if let Ok(n) = part.parse::<u32>() {
            out.push(n);
        }
    }
    out
}

pub fn cpu_online() -> Res<Vec<u32>> {
    let s = util::read_str("/sys/devices/system/cpu/online")?;
    Ok(parse_range_list(&s))
}

pub fn cpu_present() -> Res<Vec<u32>> {
    let s = util::read_str("/sys/devices/system/cpu/present")?;
    Ok(parse_range_list(&s))
}

pub fn cpu_is_online(n: u32) -> bool {
    cpu_online().map(|v| v.contains(&n)).unwrap_or(false)
}

/// Write a cpu online/offline request — the PSCI hotplug the cl2
/// scripts drive via `echo 1 > /sys/devices/system/cpu/cpuN/online`.
pub fn set_cpu_online(n: u32, on: bool) -> Res<()> {
    let p = format!("/sys/devices/system/cpu/cpu{n}/online");
    util::write_str(&p, if on { "1" } else { "0" })
}

#[cfg(test)]
mod tests {
    #[test]
    fn ranges() {
        assert_eq!(super::parse_range_list("0-7,9"), vec![0, 1, 2, 3, 4, 5, 6, 7, 9]);
        assert_eq!(super::parse_range_list("0-1"), vec![0, 1]);
        assert_eq!(super::parse_range_list("8"), vec![8]);
        assert_eq!(super::parse_range_list(""), Vec::<u32>::new());
    }
}

/// The first /sys/class/backlight/* entry that exposes `brightness`, if
/// any (the backlight script uses sysfs when present — on this kernel
/// CONFIG_PWM_MTK_DISP=y so the DISP_PWM0 driver provides one — and
/// falls back to direct DISP_PWM0 devmem writes otherwise).
pub fn first_backlight() -> Option<PathBuf> {
    let mut names: Vec<String> = std::fs::read_dir("/sys/class/backlight")
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    for n in names {
        let d = PathBuf::from("/sys/class/backlight").join(&n);
        if d.join("brightness").exists() {
            return Some(d);
        }
    }
    None
}

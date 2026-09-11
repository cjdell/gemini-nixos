//! One-shot battery status — port of services/scripts/battstat.
//!
//! Prints key=value lines from the BQ25896 power supply
//! (/sys/class/power_supply/bq25890-charger-*; mainline bq25890 driver
//! driving the TI BQ25896 on i2c0 @0x6b) and reports an exit code that
//! harnesses depend on:
//!   0  OK (charging, full, or healthy on battery)
//!   2  low battery (on battery power, < 3650 mV)
//!   3  USB present but NOT charging
//!   4  critical (on battery power, < 3500 mV)
//!   5  no bq25890 power supply present (driver missing)
//!
//! Property semantics (from the script header): status Charging/
//! Discharging/Full/Not charging; online 1 when VBUS present;
//! voltage_now in uV (sags below OCV under load — the thresholds
//! account for it); current_now unreliable while charging (the driver
//! only re-triggers the ADC when !online — use charge_type instead).

use std::path::{Path, PathBuf};

use crate::error::Res;
use crate::util;

pub const PSY_PREFIX: &str = "bq25890-charger-";

/// First bq25890-charger-* power supply exposing `status`, if any.
pub fn psy_dir() -> Option<PathBuf> {
    let mut names: Vec<String> = std::fs::read_dir("/sys/class/power_supply")
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    for n in names {
        if n.starts_with(PSY_PREFIX) {
            let d = PathBuf::from("/sys/class/power_supply").join(&n);
            if d.join("status").exists() {
                return Some(d);
            }
        }
    }
    None
}

pub struct Psy {
    pub supply: String,
    pub online: i64,
    pub status: String,
    pub charge_type: String,
    pub vbat_mv: Option<i64>, // None = unreadable/non-numeric ("?")
    pub ibat_ma: Option<i64>,
    pub temp_10c: Option<i64>,
}

fn num_opt(path: &Path) -> Option<i64> {
    let s = util::read_str_opt(&path.to_string_lossy())?;
    s.parse::<i64>().ok()
}

/// Read one snapshot of the first bq25890 power supply.
pub fn read_psy(d: &Path) -> Psy {
    let sup = d.file_name().unwrap_or_default().to_string_lossy().to_string();
    Psy {
        supply: sup,
        online: num_opt(&d.join("online")).unwrap_or(0),
        status: util::read_str_opt(&d.join("status").to_string_lossy())
            .unwrap_or_else(|| "Unknown".into()),
        charge_type: util::read_str_opt(&d.join("charge_type").to_string_lossy())
            .unwrap_or_else(|| "None".into()),
        vbat_mv: num_opt(&d.join("voltage_now")).map(|v| v / 1000),
        ibat_ma: num_opt(&d.join("current_now")).map(|v| v / 1000),
        temp_10c: num_opt(&d.join("temp")),
    }
}

fn guard_state() -> Option<String> {
    let s = util::read_str_opt("/run/battery-guard/state")?;
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("guard=") {
            return Some(v.to_string());
        }
    }
    None
}

/// Full battstat run: returns the key=value lines and the exit code.
pub fn status_lines() -> Res<(Vec<String>, i32)> {
    let Some(psy) = psy_dir() else {
        return Ok((vec!["error=no_bq25890_power_supply".into()], 5));
    };
    let p = read_psy(&psy);
    let mut lines = Vec::new();
    lines.push(format!("supply={}", p.supply));
    lines.push(format!("online={}", p.online));
    lines.push(format!("status={}", p.status));
    lines.push(format!("charge_type={}", p.charge_type));
    match p.vbat_mv {
        Some(v) => lines.push(format!("vbat_mV={v}")),
        None => lines.push("vbat_mV=?".into()),
    }
    match p.ibat_ma {
        Some(v) => lines.push(format!("ibat_mA={v}")),
        None => lines.push("ibat_mA=?".into()),
    }
    match p.temp_10c {
        Some(v) => lines.push(format!("temp_10c={v}")),
        None => lines.push("temp_10c=?".into()),
    }
    if let Some(g) = guard_state() {
        lines.push(format!("guard={g}"));
    }

    let rc = match p.status.as_str() {
        "Full" | "Charging" => 0,
        "Discharging" | "Not charging" => {
            if p.online == 1 {
                3
            } else if let Some(mv) = p.vbat_mv {
                if mv < 3500 {
                    4
                } else if mv < 3650 {
                    2
                } else {
                    0
                }
            } else {
                0
            }
        }
        _ => 0,
    };
    Ok((lines, rc))
}

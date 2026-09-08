//! Battery safety daemon — port of services/scripts/battery-guard.sh.
//!
//! Motivation (experiments on this device):
//!   1. Keep the battery at a safe level: WARN when VBAT is low, and do
//!      an orderly poweroff before the Li-ion reaches a dangerous depth
//!      of discharge (hard brown-out / preloader-only brick state).
//!   2. Verify the device is ACTUALLY charging when USB is connected:
//!      alert when VBUS is present but the BQ25896 is not charging (the
//!      B-19/B-22 failure mode, where the OTG boost puts the charger IC
//!      into source mode so an external charger cannot sink).
//!
//! Safety notes ported verbatim from the script header (do not "simplify"):
//!   - current_now stays 0 while online in the mainline driver (only
//!     re-triggers the ADC when !online/hiz) — charge_type is the
//!     primary charge signal.
//!   - A VBAT read below 2.5 V is a READ ERROR (the ADC floor is
//!     2.304 V), never "critical" — must not trigger a poweroff.
//!   - Poweroff requires TWO CONSECUTIVE CRIT samples.
//!
//! Outputs (same files as the script, so harnesses keep working):
//!   - stdout (systemd captures to the journal when unit-run)
//!   - /run/battery-guard/state  (key=value)
//!   - /var/log/battery-history.csv (one row per poll; rotated at
//!     5 MiB to battery-history.csv.1)
//!
//! [corrected 2026-09-09] the bash rotate path wrote a header without
//! the charge_type column while rows kept 8 fields (latent column
//! misalignment); gemcli always writes the 8-column header.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::battery;
use crate::util;

const RUNDIR: &str = "/run/battery-guard";
const STATE: &str = "/run/battery-guard/state";
const HIST: &str = "/var/log/battery-history.csv";
const HIST_MAX: u64 = 5 * 1024 * 1024;

fn env_u64(name: &str, def: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(def)
}

fn alert_ts() -> String {
    format!("[{}]", util::stamp())
}

/// Run the guard loop until poweroff or fatal error. Returns the exit
/// code (a daemon normally never returns).
pub fn run() -> i32 {
    let poll_s = env_u64("BATTERY_GUARD_POLL_S", 10);
    let warn_mv = env_u64("BATTERY_GUARD_WARN_LOW_MV", 3650) as i64;
    let crit_mv = env_u64("BATTERY_GUARD_CRIT_MV", 3500) as i64;
    let alert_cd = env_u64("BATTERY_GUARD_ALERT_COOLDOWN_S", 300) as i64;
    let stuck_min = env_u64("BATTERY_GUARD_STUCK_CHARGE_MIN", 15) as i64;

    let _ = std::fs::create_dir_all(RUNDIR);
    let hist = Path::new(HIST);
    if !hist.exists() {
        let _ = std::fs::write(
            hist,
            "ts,online,status,charge_type,vbat_mv,ibat_uA,temp_10c,guard\n",
        );
    }

    let mut last_alert: i64 = 0;
    let mut stuck_since: i64 = 0;
    let mut crit_strikes: i64 = 0;
    let mut prev_guard = String::new();

    println!("{} battery-guard: started (poll={poll_s}s warn<{warn_mv}mV crit<{crit_mv}mV)", alert_ts());

    loop {
        let t = now_secs();
        let mut guard = "OK".to_string();

        let Some(psy) = battery::psy_dir() else {
            guard = "NOSUPPLY".into();
            alert(
                &mut last_alert,
                t,
                alert_cd,
                "ERROR",
                "no bq25890-charger power supply under /sys/class/power_supply — charger driver did not probe (check dmesg)",
            );
            // state gets ?-marked values
            write_state(None, &guard, t);
            append_hist(&hist, &StateRow::unavailable(&guard, t));
            util::sleep(poll_s as f64);
            continue;
        };

        let p = battery::read_psy(&psy);
        let psy_name = p.supply.clone();
        // Fields as the script types them (raw uA for ibat in history).
        let vbat_raw = read_u64_or(&psy, "voltage_now");
        let ibat_raw = read_u64_or(&psy, "current_now");
        let temp_raw = read_u64_or(&psy, "temp");

        // validate numerics the script way (unreadable -> guard READERR
        // only when on battery AND voltage is the broken read)
        let vbat_mv = match vbat_raw {
            Some(v) => Some(v as i64 / 1000),
            None => None,
        };

        if p.online == 1 {
            match p.status.as_str() {
                "Full" => { /* holding at full charge: fine */ }
                "Charging" => {
                    // stuck-charge detection: status says Charging but the
                    // chip's charge-status register says NOT charging
                    // (charge_type=NONE) for a long time -> sense fault.
                    if p.charge_type == "None" {
                        if stuck_since == 0 {
                            stuck_since = t;
                        }
                        if t - stuck_since > stuck_min * 60 {
                            alert(
                                &mut last_alert,
                                t,
                                alert_cd,
                                "WARN",
                                format!("status=Charging but charge_type=None for {stuck_min} min — sense/charger fault?"),
                            );
                        }
                    } else {
                        stuck_since = 0;
                    }
                }
                other => {
                    guard = "NOTCHARGING".into();
                    alert(
                        &mut last_alert,
                        t,
                        alert_cd,
                        "WARN",
                        format!("USB present (online=1) but status='{other}' — NOT charging (OTG/boost mode? wrong port/cable? see B-19/B-22)"),
                    );
                }
            }
        } else {
            stuck_since = 0;
            match vbat_mv {
                None => {
                    guard = "READERR".into();
                    alert(
                        &mut last_alert,
                        t,
                        alert_cd,
                        "ERROR",
                        "VBAT read failed — cannot assess battery level",
                    );
                }
                Some(mv) if mv < 2500 => {
                    // below ADC floor: read error, NOT a valid reading
                    guard = "READERR".into();
                    alert(
                        &mut last_alert,
                        t,
                        alert_cd,
                        "ERROR",
                        format!("VBAT {mv} mV below ADC floor — read error, ignoring"),
                    );
                }
                Some(mv) if mv < crit_mv => {
                    crit_strikes += 1;
                    if crit_strikes >= 2 {
                        guard = "CRITICAL".into();
                        println!(
                            "{} battery-guard: CRITICAL: VBAT {mv} mV < {crit_mv} mV with no USB — powering off in 10 s (vendor hard-off is 3400 mV)",
                            alert_ts()
                        );
                        write_state(
                            Some(&StateVals {
                                psy: &psy_name,
                                online: p.online,
                                status: &p.status,
                                charge_type: &p.charge_type,
                                vbat_mv,
                                ibat_uA: ibat_raw,
                                temp_10c: temp_raw,
                            }),
                            &guard,
                            t,
                        );
                        util::sleep(10.0);
                        let _ = std::process::Command::new("systemctl").arg("poweroff").status();
                        return 0;
                    } else {
                        guard = "CRITICAL".into();
                        alert(
                            &mut last_alert,
                            t,
                            alert_cd,
                            "CRIT",
                            format!("VBAT {mv} mV < {crit_mv} mV with no USB — will power off on next sample if still critical (connect USB!)"),
                        );
                    }
                }
                Some(mv) => {
                    crit_strikes = 0;
                    if mv < warn_mv {
                        guard = "LOW".into();
                        alert(
                            &mut last_alert,
                            t,
                            alert_cd,
                            "WARN",
                            format!("low battery on battery power: VBAT {mv} mV (< {warn_mv} mV) — connect USB"),
                        );
                    }
                }
            }
        }

        // temperature sanity (rough TS-pin reading)
        if let Some(t10) = temp_raw {
            if t10 > 450 {
                alert(
                    &mut last_alert,
                    t,
                    alert_cd,
                    "WARN",
                    format!("battery TS pin ~{} C — hot; charging may be throttled (JEITA)", t10 / 10),
                );
            }
        }

        if guard != prev_guard {
            println!(
                "{} battery-guard: state: {} -> {guard} (online={} status={} vbat={}mV)",
                alert_ts(),
                if prev_guard.is_empty() { "<init>".to_string() } else { prev_guard.clone() },
                p.online,
                p.status,
                vbat_mv.map(|v| v.to_string()).unwrap_or_else(|| "?".into())
            );
            prev_guard = guard.clone();
        }

        let vals = StateVals {
            psy: &psy_name,
            online: p.online,
            status: &p.status,
            charge_type: &p.charge_type,
            vbat_mv,
            ibat_uA: ibat_raw,
            temp_10c: temp_raw,
        };
        write_state(Some(&vals), &guard, t);
        append_hist(&hist, &StateRow::from_vals(&vals, &guard, t));
        util::sleep(poll_s as f64);
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn read_u64_or(psy: &PathBuf, prop: &str) -> Option<u64> {
    let p = psy.join(prop);
    util::read_str_opt(&p.to_string_lossy()).and_then(|s| s.parse::<u64>().ok())
}

fn alert(last: &mut i64, t: i64, cd: i64, level: &str, msg: impl Into<String>) {
    if t >= *last + cd {
        println!("{} battery-guard: ALERT {level}: {}", alert_ts(), msg.into());
        *last = t;
    }
}

// The `ibat_uA` (and `temp_10c`) key names are script parity — the
// state/history files keep the exact key the bash daemon wrote.
#[allow(non_snake_case)]
struct StateVals<'a> {
    psy: &'a str,
    online: i64,
    status: &'a str,
    charge_type: &'a str,
    vbat_mv: Option<i64>,
    ibat_uA: Option<u64>,
    temp_10c: Option<u64>,
}

fn write_state(v: Option<&StateVals>, guard: &str, t: i64) {
    let body = match v {
        Some(v) => format!(
            "ts={t}\nsupply={}\nonline={}\nstatus={}\ncharge_type={}\nvbat_mv={}\nibat_uA={}\ntemp_10c={}\nguard={}\n",
            v.psy,
            v.online,
            v.status,
            v.charge_type,
            v.vbat_mv.map(|x| x.to_string()).unwrap_or_else(|| "?".into()),
            v.ibat_uA.map(|x| x.to_string()).unwrap_or_else(|| "?".into()),
            v.temp_10c.map(|x| x.to_string()).unwrap_or_else(|| "?".into()),
            guard
        ),
        None => format!(
            "ts={t}\nsupply=?\nonline=?\nstatus=?\ncharge_type=?\nvbat_mv=?\nibat_uA=?\ntemp_10c=?\nguard={guard}\n"
        ),
    };
    if let Ok(mut f) = OpenOptions::new().write(true).truncate(true).create(true).open(STATE) {
        let _ = f.write_all(body.as_bytes());
    }
}

// The `ibat_uA` field name is script parity (history CSV header + rows).
#[allow(non_snake_case)]
struct StateRow {
    ts: String,
    online: String,
    status: String,
    charge_type: String,
    vbat_mv: String,
    ibat_uA: String,
    temp_10c: String,
    guard: String,
}

impl StateRow {
    fn unavailable(guard: &str, _t: i64) -> Self {
        StateRow {
            ts: util::stamp(),
            online: "?".into(),
            status: "?".into(),
            charge_type: "?".into(),
            vbat_mv: "?".into(),
            ibat_uA: "?".into(),
            temp_10c: "?".into(),
            guard: guard.into(),
        }
    }

    fn from_vals(v: &StateVals, guard: &str, _t: i64) -> Self {
        StateRow {
            ts: util::stamp(),
            online: v.online.to_string(),
            status: v.status.to_string(),
            charge_type: if v.charge_type.is_empty() { "?".into() } else { v.charge_type.to_string() },
            vbat_mv: v.vbat_mv.map(|x| x.to_string()).unwrap_or_else(|| "?".into()),
            ibat_uA: v.ibat_uA.map(|x| x.to_string()).unwrap_or_else(|| "?".into()),
            temp_10c: v.temp_10c.map(|x| x.to_string()).unwrap_or_else(|| "?".into()),
            guard: guard.into(),
        }
    }

    fn csv(&self) -> String {
        format!(
            "{},{},{},{},{},{},{},{}\n",
            self.ts, self.online, self.status, self.charge_type, self.vbat_mv, self.ibat_uA, self.temp_10c, self.guard
        )
    }
}

fn append_hist(hist: &Path, row: &StateRow) {
    // rotate at 5 MiB (the script's stat -c %s check)
    if let Ok(md) = std::fs::metadata(hist) {
        if md.len() > HIST_MAX {
            let _ = std::fs::rename(hist, Path::new(HIST).with_extension("csv.1"));
            let _ = std::fs::write(hist, "ts,online,status,charge_type,vbat_mv,ibat_uA,temp_10c,guard\n");
        }
    }
    if let Ok(mut f) = OpenOptions::new().append(true).create(true).open(hist) {
        let _ = f.write_all(row.csv().as_bytes());
    }
}

/// `guard status` — the daemon's current /run/battery-guard/state, if any.
pub fn status_cmd() -> i32 {
    match util::read_str(STATE) {
        Ok(s) => {
            print!("{s}");
            0
        }
        Err(_) => {
            eprintln!("gemcli guard: no state (daemon not running?)");
            1
        }
    }
}

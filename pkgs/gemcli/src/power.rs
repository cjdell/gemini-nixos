//! Power-management CLI — port of services/scripts/power (battery +
//! backlight + charge direction).
//!
//! The two knobs that matter on this device: the battery (is it
//! charging, at what real current via the BQ25896 raw ADC, at what
//! voltage, what does the safety guard say) and the backlight — the
//! display LED boost is the single biggest load, and at full brightness
//! from a 500 mA USB port the battery cannot charge (ICHGR=0, the
//! BQ25896 power path covers the load). `dim-to-charge` solves that.

use crate::backlight;
use crate::battery;
use crate::charger;
use crate::error::{cmsg, Res};
use crate::util;

fn backlight_pct() -> String {
    backlight::get().map(|v| v.to_string()).unwrap_or_else(|_| "?".into())
}

fn raw_line() -> String {
    charger::sample().map(|s| s.line()).unwrap_or_else(|_| "raw-unavailable".into())
}

fn guard_value() -> String {
    util::read_str_opt("/run/battery-guard/state")
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("guard=").map(|v| v.to_string())))
        .unwrap_or_default()
}

/// `power status` — one-shot battery + backlight + charge direction.
pub fn status() -> Res<()> {
    let line = raw_line();
    let ichgr = charger::ichgr_from_line(&line).map(|v| v.to_string()).unwrap_or_else(|| "?".into());
    let (bst, _rc) = battery::status_lines()?;
    let bl = backlight_pct();

    println!("== battery ==");
    for l in &bst {
        println!("{l}");
    }
    println!("raw: {line}");
    println!();
    println!("== backlight ==");
    println!("brightness={bl}%");
    println!();
    println!("== verdict ==");
    match charger::ichgr_from_line(&line) {
        Some(n) if n > 0 => println!("CHARGING: battery receiving {n}mA (dimming helped)"),
        Some(n) if n < 0 => {
            println!("DISCHARGING: battery supplies {}mA — backlight is stealing the budget", -n)
        }
        Some(0) => println!("NEUTRAL: charge current 0 — load == input power"),
        _ => println!("cannot determine charge current (bq25896 raw read unavailable?)"),
    }
    let _ = ichgr;
    Ok(())
}

/// `power watch [N]` — poll every N s (default 10) until interrupted.
pub fn watch(n: u64) -> Res<()> {
    loop {
        println!("--- {} ---", util::stamp());
        match charger::sample() {
            Ok(s) => println!("{}", s.line()),
            Err(e) => println!("{e}"),
        }
        println!("backlight={}%  guard={}", backlight_pct(), guard_value());
        util::sleep(n as f64);
    }
}

/// `power charge` — print the net charge current (mA) from the raw ADC.
pub fn charge() -> Res<()> {
    let line = raw_line();
    match charger::ichgr_from_line(&line) {
        Some(n) => {
            println!("{n}mA");
            Ok(())
        }
        None => {
            println!("?");
            Err(cmsg("raw read unavailable"))
        }
    }
}

/// `power dim-to-charge [target_ma] [min_pct]` — step the backlight down
/// until raw ICHGR >= target (default 100 mA) or min_pct (default 5).
/// rc: 0 charging OK, 1 error/timeout, 2 floor reached (input power
/// insufficient — use a 2 A charger). Script parity.
pub fn dim_to_charge(target: i64, min_pct: i64) -> i32 {
    let mut pct = match backlight::get() {
        Ok(v) => v as i64,
        Err(e) => {
            eprintln!("gemcli power: cannot read backlight %: {}", e.msg);
            return 1;
        }
    };
    println!("aiming for ICHGR >= {target} mA (backlight floor {min_pct}%)");
    for _ in 0..40 {
        let line = raw_line();
        let Some(n) = charger::ichgr_from_line(&line) else {
            eprintln!("gemcli power: raw read failed: {line}");
            return 1;
        };
        println!("  backlight={pct}%  ICHGR={n}mA");
        if n >= target {
            println!("OK: charging at {n}mA with backlight at {pct}%");
            return 0;
        }
        if pct <= min_pct {
            println!(
                "FLOOR: backlight at {pct}% (min) but ICHGR={n}mA — input power insufficient, use a 2 A charger"
            );
            return 2;
        }
        // step down 10 % at a time, allow the ADC + power path to settle
        pct -= 10;
        if pct < min_pct {
            pct = min_pct;
        }
        if let Err(e) = backlight::set(pct as u32) {
            eprintln!("gemcli power: backlight set failed: {}", e.msg);
            return 1;
        }
        util::sleep(3.0);
    }
    eprintln!("gemcli power: TIMEOUT: no stable charge after 40 steps");
    1
}

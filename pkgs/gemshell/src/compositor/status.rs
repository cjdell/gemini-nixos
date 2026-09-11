//! Status polling — a background thread reads the standard interfaces
//! every 2 s and pushes a [`Status`] to the main loop:
//!
//! - battery: sysfs `bq25890-battery-*` (capacity is voltage-derived,
//!   same numbers UPower shows) + `bq25890-charger-*` (status/online)
//! - wifi: NetworkManager over the system bus (busctl)
//! - bluetooth: BlueZ `org.bluez.Adapter1.Powered`
//! - volume: wpctl @DEFAULT_AUDIO_SINK@ (the system PipeWire session)
//! - brightness: /sys/class/backlight/* (world-writable via the
//!   plumbing udev rule)
//!
//! No D-Bus C library — the standard CLIs (busctl/wpctl) are in the
//! session closure; everything degrades gracefully to "unknown" if a
//! tool is missing.

use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::common::util;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// percent 0..=100, None = unknown
    pub battery: Option<i32>,
    pub charging: bool,
    pub wifi: bool,
    pub bluetooth: bool,
    pub volume: i32,
    pub muted: bool,
    pub brightness: i32,
    pub brightness_max: i32,
    pub hour: u32,
    pub minute: u32,
}

fn cmd_env_stdout(prog: &str, args: &[&str], env: &[(&str, &str)]) -> Option<String> {
    let mut c = std::process::Command::new(prog);
    c.args(args)
        .envs(env.iter().copied())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = c.spawn().ok()?;
    let mut out = String::new();
    child.stdout.as_mut().and_then(|s| std::io::Read::read_to_string(s, &mut out).ok())?;
    child.wait().ok()?;
    Some(out)
}

fn cmd_stdout(prog: &str, args: &[&str]) -> Option<String> {
    cmd_env_stdout(prog, args, &[])
}

fn sysfs_files(prefix: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir("/sys/class/power_supply") {
        for e in rd.flatten() {
            let p = e.path();
            if p.file_name().and_then(|s| s.to_str()).map(|s| s.starts_with(prefix)) != Some(true) {
                continue;
            }
            out.push(p);
        }
    }
    out
}

impl Status {
    pub fn read() -> Self {
        let mut s = Status::default();

        for p in sysfs_files("bq25890-battery") {
            s.battery = util::read_to_string(&p.join("capacity"))
                .and_then(|v| v.trim().parse().ok());
            if s.battery.is_some() {
                break;
            }
        }
        for p in sysfs_files("bq25890-charger") {
            let status = util::read_to_string(&p.join("status")).map(|v| v.trim().to_string());
            if status.as_deref() == Some("Charging") {
                s.charging = true;
            }
            let online = util::read_to_string(&p.join("online")).map(|v| v.trim() == "1");
            if online == Some(true) {
                s.charging = true;
            }
            if status.is_some() {
                break;
            }
        }

        if let Some(out) = cmd_stdout(
            "busctl",
            &[
                "--no-pager",
                "get-property",
                "org.freedesktop.NetworkManager",
                "/org/freedesktop/NetworkManager",
                "org.freedesktop.NetworkManager",
                "Connectivity",
            ],
        ) {
            s.wifi = out.contains("\"full\"");
        }

        if let Some(out) = cmd_stdout(
            "busctl",
            &[
                "get-property",
                "org.bluez",
                "/org/bluez/hci0",
                "org.bluez.Adapter1",
                "Powered",
            ],
        ) {
            s.bluetooth = out.contains("true");
        }

        // the system PipeWire session lives at /run/gemwl-audio (the
        // compositor runs as a system service with no logind session —
        // PULSE_SERVER/PIPEWIRE_RUNTIME_DIR are set by the unit; fall
        // back to the known path)
        let pw = std::env::var("PULSE_SERVER")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("unix:{}/pipewire-0", "/run/gemwl-audio"));
        if let Some(out) = cmd_env_stdout(
            "wpctl",
            &["get-volume", "@DEFAULT_AUDIO_SINK@"],
            &[("PULSE_SERVER", pw.as_str())],
        ) {
            // wpctl prints a 0..1 fraction ("0.42") or, on older
            // versions, a 0..100 integer
            if let Some(v) = out.split_whitespace().next().and_then(|t| t.parse::<f64>().ok()) {
                s.volume = if v > 1.5 { v as i32 } else { (v * 100.0).round() as i32 };
            }
            s.muted = out.to_uppercase().contains("MUTE");
        }

        if let Ok(rd) = std::fs::read_dir("/sys/class/backlight") {
            for e in rd.flatten() {
                let p = e.path();
                s.brightness = util::read_to_string(&p.join("brightness"))
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0);
                s.brightness_max = util::read_to_string(&p.join("max_brightness"))
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(255);
                break;
            }
        }

        (s.hour, s.minute) = util::clock_hm();
        s
    }
}

/// Spawn the poller thread; the receiver goes to the compositor.
pub fn spawn(tx: mpsc::Sender<Status>) {
    std::thread::Builder::new()
        .name("gemshell-status".into())
        .spawn(move || {
            let mut last = Instant::now();
            loop {
                let s = Status::read();
                if tx.send(s).is_err() {
                    return;
                }
                last = Instant::now();
                std::thread::sleep(Duration::from_millis(2000).saturating_sub(last.elapsed()));
            }
        })
        .ok();
}

/// Set the backlight (0..=max). Uses the sysfs file (world-writable via
/// the plumbing udev rule); returns the new value.
pub fn set_brightness(pct: i32, max: i32) -> Option<i32> {
    let max = max.max(1);
    let v = (pct.clamp(0, 100) * max / 100).max(if max > 1 { 1 } else { 0 });
    if let Ok(rd) = std::fs::read_dir("/sys/class/backlight") {
        for e in rd.flatten() {
            let p = e.path().join("brightness");
            if let Ok(mut f) = std::fs::File::create(&p) {
                if f.write_all(v.to_string().as_bytes()).is_ok() {
                    return Some(v);
                }
            }
        }
    }
    None
}

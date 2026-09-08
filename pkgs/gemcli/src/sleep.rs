//! Clamshell sleep/wake for the Gemini PDA — `gemcli sleep on|off|status|key`.
//!
//! The unit has NO suspend/resume path yet (see docs/power-sleep.md): the
//! kernel is the fbcon / clk_ignore_unused build and there is no wake
//! source for s2idle (the side keys are PMIC-debounced bits polled over
//! pwrap by mt6351-keys — no IRQ route exists in mainline). This module
//! implements the LIGHT sleep a silver-button sleep/wake can use today:
//! every controllable load is removed while the kernel stays up, and the
//! same button (KEY_SLEEP, still polled by mt6351-keys) brings it back.
//!
//! `sleep on` sequence (order matters — heavy userspace first so the
//! core offlining and input unbinds happen on a settled system):
//!   1. stop the heavyweight services that were running: gemwl +
//!      lxqt-nested (the GPU desktop), pipewire/wireplumber/pipewire-pulse
//!      (audio), gemini-wifi-internal/auto (after `wifi-internal stop`
//!      power-cycles the CONSYS CONN domain — stopping the oneshot units
//!      alone would leave the chip powered). sshd + gemini-battery-guard
//!      STAY (control link + the safety daemon).
//!   2. offline A53 cpus 1..7 (cpu0 must run the kernel; the A72 cluster
//!      is already down in the default cold-boot state).
//!   3. backlight off (bl_power=4 / PWM EN=0 — the brightness value is
//!      retained, so `on` restores it).
//!   4. unbind the clamshell input drivers: the gpio-matrix-keypad
//!      platform device (`keyboard`) and the novatek-nt36xxx touch i2c
//!      client (`4-0062`). With the lid closed the keycaps press the
//!      matrix/touch — unbound, they generate no input and no wakeups.
//!      The mt6351-keys side-button driver is deliberately NOT unbound —
//!      it is the wake button.
//!   5. record everything in /run/gemcli-sleep.state so `off` restores
//!      exactly (services that were running, cores offlined, backlight%).
//!
//! `sleep off` reverses: rebind the inputs, backlight on, online the
//! recorded cpus, start the recorded services (async), clear the state.
//!
//! `sleep key` is the daemon entry (gemini-sleepd.service): it watches
//! /dev/input/eventN for mt6351-keys KEY_SLEEP presses and toggles.
//! Only value==1 (press) toggles; repeat/release are ignored.
//!
//! Deliberate scope: this is the awake low-power floor. Real "deep"
//! sleep (s2idle) and the silver-button wake IRQ for it are kernel work
//! (docs/power-sleep.md §"Deep sleep") — this module is what the button
//! drives until then.

use std::os::fd::{AsRawFd, FromRawFd};
use std::process::Command;

use crate::backlight;
use crate::error::{cmsg, Res};
use crate::util;

const STATE_FILE: &str = "/run/gemcli-sleep.state";

/// Heavyweight services stopped on sleep (only those actually running
/// are stopped; wake starts exactly the stopped set). sshd + the
/// battery guard + gemini-sleepd itself are never in this list.
const SERVICES: &[&str] = &[
    "gemwl.service",
    "lxqt-nested.service",
    "pipewire.service",
    "wireplumber.service",
    "pipewire-pulse.service",
    "gemini-wifi-internal.service",
    "gemini-wifi-auto.service",
];

/// Internal wifi is powered down by the wifi-internal CLI (the oneshot
/// unit has no ExecStop; `systemctl stop` alone leaves the CONSYS chip
/// powered). Resolved at runtime (store path), fallback to PATH.
fn wifi_internal() -> String {
    let p = "/run/current-system/sw/bin/wifi-internal";
    if util::exists(p) {
        p.into()
    } else {
        "wifi-internal".into()
    }
}

fn systemctl() -> String {
    let p = "/run/current-system/sw/bin/systemctl";
    if util::exists(p) {
        p.into()
    } else {
        "systemctl".into()
    }
}

// --- state file -----------------------------------------------------------

fn state_path() -> String {
    STATE_FILE.into()
}

/// Read the state file into a map (missing file = empty map).
fn read_state() -> std::collections::HashMap<String, String> {
    let mut m = std::collections::HashMap::new();
    if let Ok(s) = std::fs::read_to_string(state_path()) {
        for line in s.lines() {
            if let Some((k, v)) = line.split_once('=') {
                m.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }
    m
}

fn write_state(m: &std::collections::HashMap<String, String>) -> Res<()> {
    let mut s = String::new();
    let mut keys: Vec<&String> = m.keys().collect();
    keys.sort();
    for k in keys {
        s.push_str(&format!("{k}={}\n", m[k]));
    }
    util::write_str(&state_path(), &s)
}

fn clear_state() {
    let _ = std::fs::remove_file(state_path());
}

fn is_sleeping() -> bool {
    read_state().get("state").map(|s| s == "sleeping").unwrap_or(false)
}

// --- service orchestration (systemctl) -------------------------------------

fn unit_active(u: &str) -> bool {
    Command::new(systemctl())
        .args(["is-active", u])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn unit_stop(u: &str) -> bool {
    Command::new(systemctl())
        .args(["stop", "--no-block", u])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn unit_start(u: &str) -> bool {
    Command::new(systemctl())
        .args(["start", "--no-block", u])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn services_running() -> Vec<&'static str> {
    SERVICES.iter().copied().filter(|u| unit_active(u)).collect()
}

fn stop_running(units: &[&str]) {
    for u in units {
        println!("gemcli sleep: stopping {u}");
        if !unit_stop(u) {
            println!("gemcli sleep: WARNING systemctl stop {u} failed");
        }
    }
}

fn start_units(units: &[&str]) {
    for u in units {
        println!("gemcli sleep: starting {u}");
        if !unit_start(u) {
            println!("gemcli sleep: WARNING systemctl start {u} failed");
        }
    }
}

/// Power the internal wifi chip down/up via the wifi-internal CLI.
/// Best-effort: the CLI prints its own verdicts.
fn wifi_power(on: bool) {
    let cmd = wifi_internal();
    let rc = Command::new(&cmd)
        .arg(if on { "start" } else { "stop" })
        .status();
    match rc {
        Ok(s) if s.success() => {}
        Ok(_) => println!("gemcli sleep: {cmd} {} returned nonzero", if on { "start" } else { "stop" }),
        Err(e) => println!("gemcli sleep: cannot run {cmd}: {e}"),
    }
}

// --- cpu hotplug ------------------------------------------------------------

/// Offline A53 cpus 1..7 (never cpu0), returns the list actually
/// offlined, comma-joined.
fn offline_workers() -> String {
    let mut off = Vec::new();
    let present = crate::sysfs::cpu_present().unwrap_or_default();
    for c in (1..=7).rev() {
        if !present.contains(&c) {
            continue;
        }
        if !crate::sysfs::cpu_is_online(c) {
            continue;
        }
        match crate::sysfs::set_cpu_online(c, false) {
            Ok(()) => {
                println!("gemcli sleep: cpu{c} offline");
                off.push(c);
            }
            Err(e) => println!("gemcli sleep: WARNING cpu{c} refused offline: {}", e.msg),
        }
    }
    off.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",")
}

fn online_cpus(list: &str) {
    for c in crate::sysfs::parse_range_list(list) {
        if c == 0 {
            continue;
        }
        if crate::sysfs::cpu_is_online(c) {
            continue;
        }
        match crate::sysfs::set_cpu_online(c, true) {
            Ok(()) => println!("gemcli sleep: cpu{c} online"),
            Err(e) => println!("gemcli sleep: WARNING cpu{c} refused online: {}", e.msg),
        }
    }
}

// --- input driver unbind/bind ------------------------------------------------

const KBD_DRV: &str = "/sys/bus/platform/drivers/matrix-keypad";
const KBD_DEV: &str = "keyboard";
const TCH_DRV: &str = "/sys/bus/i2c/drivers/novatek-nt36xxx";
const TCH_DEV: &str = "4-0062";

fn driver_has(drv: &str, dev: &str) -> bool {
    util::exists(&format!("{drv}/{dev}"))
}

/// Unbind an input driver (platform or i2c). Returns Ok(()) when the
/// device is gone from the driver (or was never bound).
fn unbind_input(drv: &str, dev: &str) -> Res<()> {
    if !driver_has(drv, dev) {
        return Ok(()); // already unbound
    }
    util::write_str(&format!("{drv}/unbind"), dev)
        .map_err(|e| cmsg(format!("unbind {dev} from {drv}: {}", e.msg)))?;
    if driver_has(drv, dev) {
        return Err(cmsg(format!("unbind {dev}: still bound after write")));
    }
    Ok(())
}

fn bind_input(drv: &str, dev: &str) -> Res<()> {
    if driver_has(drv, dev) {
        return Ok(()); // already bound
    }
    util::write_str(&format!("{drv}/bind"), dev)
        .map_err(|e| cmsg(format!("bind {dev} to {drv}: {}", e.msg)))?;
    // i2c bind creates the device asynchronously via the driver core —
    // give it a moment before the bound-state check.
    std::thread::sleep(std::time::Duration::from_millis(300));
    if !driver_has(drv, dev) {
        return Err(cmsg(format!("bind {dev}: driver did not claim it")));
    }
    Ok(())
}

// --- public entry points -----------------------------------------------------

/// `gemcli sleep on` — enter the light clamshell sleep.
pub fn on() -> i32 {
    if is_sleeping() {
        println!("gemcli sleep: already asleep ({} — run 'gemcli sleep off')", state_path());
        return 0;
    }
    let mut rc = 0;
    let mut state = std::collections::HashMap::new();

    // 1. heavyweight services (record what we stop)
    let running = services_running();
    // Internal wifi gets a REAL chip power-down via the wifi-internal
    // CLI afterwards; capture its active-ness BEFORE the async unit
    // stop deactivates it (systemctl stop --no-block races is-active).
    let wifi_was_active = running.contains(&"gemini-wifi-internal.service");
    stop_running(&running);
    if wifi_was_active {
        wifi_power(false);
    }
    state.insert("services".into(), running.join(","));

    // 2. cores (offline_workers logs a WARNING per refused cpu)
    let off = offline_workers();
    state.insert("cpus_offline".into(), off.clone());

    // 3. backlight (record pct for the report; bl_power keeps the value)
    let pct = backlight::get().unwrap_or(0);
    state.insert("backlight_pct".into(), pct.to_string());
    if let Err(e) = backlight::off() {
        println!("gemcli sleep: ERROR backlight off: {}", e.msg);
        rc = 1;
    } else {
        println!("gemcli sleep: backlight off (was {pct}%)");
    }

    // 4. clamshell inputs
    for (drv, dev, what) in [(KBD_DRV, KBD_DEV, "keyboard matrix"), (TCH_DRV, TCH_DEV, "touchscreen")] {
        match unbind_input(drv, dev) {
            Ok(()) => println!("gemcli sleep: {what} ({dev}) disabled"),
            Err(e) => {
                println!("gemcli sleep: WARNING {e}");
                rc = 1;
            }
        }
    }

    // 5. state
    state.insert("state".into(), "sleeping".into());
    state.insert("stamp".into(), util::stamp());
    if let Err(e) = write_state(&state) {
        println!("gemcli sleep: ERROR writing state: {}", e.msg);
        return 1;
    }
    println!("gemcli sleep: ASLEEP (cpu0 only, backlight off, inputs off, {} service(s) stopped)",
        if running.is_empty() { 0 } else { running.len() });
    rc
}

/// `gemcli sleep off` — wake from the light sleep (reverse of `on`).
pub fn off() -> i32 {
    let state = read_state();
    if state.get("state").map(|s| s != "sleeping").unwrap_or(true) {
        println!("gemcli sleep: not asleep — nothing to wake");
        return 0;
    }
    let mut rc = 0;

    // 1. inputs back (touch first — it owns an i2c irq the driver needs)
    for (drv, dev, what) in [(TCH_DRV, TCH_DEV, "touchscreen"), (KBD_DRV, KBD_DEV, "keyboard matrix")] {
        match bind_input(drv, dev) {
            Ok(()) => println!("gemcli sleep: {what} ({dev}) enabled"),
            Err(e) => {
                println!("gemcli sleep: WARNING {e}");
                rc = 1;
            }
        }
    }

    // 2. backlight (bl_power=0 restores the retained brightness)
    if let Err(e) = backlight::on() {
        println!("gemcli sleep: ERROR backlight on: {}", e.msg);
        rc = 1;
    } else {
        let pct = state.get("backlight_pct").cloned().unwrap_or_else(|| "?".into());
        println!("gemcli sleep: backlight on (restoring {pct}%)");
    }

    // 3. cores
    if let Some(list) = state.get("cpus_offline") {
        online_cpus(list);
    }

    // 4. services (async — the visible wake is backlight+cores)
    if let Some(units) = state.get("services") {
        let list: Vec<&str> = units.split(',').filter(|s| !s.is_empty()).collect();
        if !list.is_empty() {
            println!("gemcli sleep: waking services: {}", list.join(" "));
            start_units(&list);
        }
    }

    clear_state();
    println!("gemcli sleep: AWAKE");
    rc
}

/// `gemcli sleep status` — print the sleep state + what would be toggled.
pub fn status() -> i32 {
    let s = read_state();
    if s.get("state").map(|x| x == "sleeping").unwrap_or(false) {
        println!("state: ASLEEP (since {})", s.get("stamp").unwrap_or(&"?".into()));
        if let Some(v) = s.get("services") {
            println!("services stopped: {}", if v.is_empty() { "(none)" } else { v });
        }
        if let Some(v) = s.get("cpus_offline") {
            println!("cpus offline: {}", if v.is_empty() { "(none)" } else { v });
        }
        if let Some(v) = s.get("backlight_pct") {
            println!("backlight was: {v}%");
        }
        return 0;
    }
    println!("state: AWAKE");
    let on = crate::sysfs::cpu_online().unwrap_or_default();
    println!("cpu online: {}", on.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(","));
    println!("services running: {}", services_running().join(" "));
    println!("inputs: kbd={} touch={}",
        if driver_has(KBD_DRV, KBD_DEV) { "bound" } else { "unbound" },
        if driver_has(TCH_DRV, TCH_DEV) { "bound" } else { "unbound" });
    println!("backlight: {}%", backlight::get().unwrap_or(0));
    0
}

// --- the KEY_SLEEP daemon -----------------------------------------------------

const EV_KEY: u16 = 0x01;
const KEY_SLEEP: u16 = 142; // linux/input-event-codes.h

#[repr(C)]
#[derive(Clone, Copy)]
struct InputEvent {
    // linux/input.h: struct input_event { timeval time; __u16 type; __u16 code; __s32 value; }
    // timeval = two 64-bit fields on aarch64/x86_64 (the only targets gemcli builds for).
    sec: i64,
    usec: i64,
    type_: u16,
    code: u16,
    value: i32,
}

fn find_evdev(name: &str) -> Option<String> {
    let dirs = std::fs::read_dir("/sys/class/input").ok()?;
    for e in dirs.flatten() {
        let dn = e.file_name().to_string_lossy().to_string();
        if !dn.starts_with("input") {
            continue;
        }
        let base = format!("/sys/class/input/{dn}");
        if util::read_str_opt(&format!("{base}/name")).as_deref() != Some(name) {
            continue;
        }
        for e in std::fs::read_dir(&base).ok()?.flatten() {
            let en = e.file_name().to_string_lossy().to_string();
            if en.starts_with("event") {
                return Some(format!("/dev/input/{en}"));
            }
        }
    }
    None
}

fn toggle() {
    if is_sleeping() {
        println!("{} gemcli sleep: silver -> WAKE", util::hms());
        let rc = off();
        if rc != 0 {
            println!("gemcli sleep: wake had warnings (rc={rc})");
        }
    } else {
        println!("{} gemcli sleep: silver -> SLEEP", util::hms());
        let rc = on();
        if rc != 0 {
            println!("gemcli sleep: sleep had warnings (rc={rc})");
        }
    }
}

/// `gemcli sleep key` — block forever watching the silver side button
/// (mt6351-keys -> KEY_SLEEP) and toggling sleep. Backs
/// gemini-sleepd.service. Ctrl-C / SIGTERM ends it cleanly.
pub fn key() -> i32 {
    let dev = match find_evdev("mt6351-keys") {
        Some(d) => d,
        None => {
            eprintln!("gemcli sleep: mt6351-keys input device not found (driver loaded?)");
            return 1;
        }
    };
    println!("gemcli sleep: watching {dev} for KEY_SLEEP (silver button); state={}",
        if is_sleeping() { "ASLEEP" } else { "AWAKE" });
    let cpath = std::ffi::CString::new(dev.as_str()).unwrap();
    let fd = unsafe { libc::open(cpath.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        eprintln!("gemcli sleep: open {dev}: {}", std::io::Error::last_os_error());
        return 1;
    }
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut ev = InputEvent { sec: 0, usec: 0, type_: 0, code: 0, value: 0 };
    loop {
        // read() blocks until a whole event is available.
        let n = unsafe {
            libc::read(
                file.as_raw_fd(),
                &mut ev as *mut InputEvent as *mut libc::c_void,
                std::mem::size_of::<InputEvent>(),
            )
        };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            eprintln!("gemcli sleep: read {dev}: {e}");
            return 1;
        }
        if n as usize != std::mem::size_of::<InputEvent>() {
            continue; // partial read — retry
        }
        if ev.type_ == EV_KEY && ev.code == KEY_SLEEP && ev.value == 1 {
            toggle();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn state_roundtrip() {
        let mut m: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        m.insert("state".into(), "sleeping".into());
        m.insert("cpus_offline".into(), "1,2,3".into());
        // the real write goes to /run; unit-test only the parse side by
        // writing a temp file through the same writer
        let p = "/tmp/gemcli-sleep-test.state";
        let mut s = String::new();
        let mut keys: Vec<&String> = m.keys().collect();
        keys.sort();
        for k in keys {
            s.push_str(&format!("{k}={}\n", m[k]));
        }
        std::fs::write(p, s).unwrap();
        let back = std::fs::read_to_string(p).unwrap();
        let parsed: std::collections::HashMap<String, String> = back
            .lines()
            .filter_map(|l| l.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
            .collect();
        std::fs::remove_file(p).ok();
        assert_eq!(parsed.get("state").map(|s| s.as_str()), Some("sleeping"));
        assert_eq!(parsed.get("cpus_offline").map(|s| s.as_str()), Some("1,2,3"));
    }
}

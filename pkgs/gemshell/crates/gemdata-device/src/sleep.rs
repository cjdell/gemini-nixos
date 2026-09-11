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
//! **Responsiveness contract (v2, 2026-09-08):** the first thing `on()`
//! does is turn the backlight off — the visible acknowledgement that the
//! press registered. Everything else (inputs, cores, services, wifi) is
//! fast (≤ ~2 s total). Nothing in the sleep/wake path blocks for long:
//! the CONSYS chip is deliberately NOT powered down on sleep (the WMT
//! `echo off` teardown stalls ~29 s with the chip fully associated —
//! observed on glass — and stopping the RemainAfterExit wifi units does
//! not even kill wpa_supplicant); sleep instead takes the wifi interface
//! down (and, in the legacy stack, kills its daemons) — fast, leaving the
//! chip powered but idle — and wake re-associates (legacy: `wifi auto`,
//! clean-slate; **NetworkManager mode, services/wifi.nix default since
//! 2026-09-10: raise the link and NM's autoconnect re-activates the saved
//! profile** — sleep must not kill NM's wpa_supplicant or restart the
//! non-existent wifi-auto unit, see wifi_down/wifi_up). The
//! `key` daemon additionally debounces presses (1 s) and drains events
//! queued while a toggle was running, so mashing the button can never
//! cascade into rapid sleep/wake flicker.
//!
//! `sleep on` sequence (visible first):
//!   0. write `state=sleeping` to /run/gemcli-sleep.state FIRST — before
//!      any teardown. This makes the profile watcher (gemini-power-profile)
//!      stand down immediately: it must not race the A72 teardown by
//!      trying to bring the cluster back up (that concurrent secure
//!      up/down hung the device on glass, 2026-09-10q). The A72 lock
//!      closes the remaining poll-window race.
//!   1. backlight off (bl_power=4 / PWM EN=0 — brightness is retained,
//!      so `on` restores it). INSTANT — this is the press feedback.
//!   2. unbind the clamshell input drivers: the gpio-matrix-keypad
//!      platform device (`keyboard`) and the novatek-nt36xxx touch i2c
//!      client (`4-0062`). With the lid closed the keycaps press the
//!      matrix/touch — unbound, they generate no input and no wakeups.
//!      The mt6351-keys side-button driver is deliberately NOT unbound —
//!      it is the wake button.
//!   3a. power the A72 cluster down (cpu8/cpu9) IF it was up — the secure
//!      cl2-down teardown is WDT-guarded and drops the DA9214 rail, so it
//!      is only run when the performance power mode (or a manual
//!      `a72 up`) left the cluster online. It runs WHILE the A53s are
//!      still online (the verified watcher path); doing it after the A53
//!      offlines hung the device (2026-09-10q). The recorded state
//!      restores it on wake. [A72 handling added 2026-09-10]
//!   3b. offline A53 cpus 1..7 (cpu0 must run the kernel).
//!   4. stop the heavyweight services that were running: the current
//!      panel owner (display-manager/GDM for GNOME, gemini-gemshell, or
//!      legacy gemwl) + pipewire/wireplumber/pipewire-pulse (audio).
//!      sshd + gemini-battery-guard + gemini-sleepd STAY.
//!   5. wifi down: `ip link set <iface> down`; legacy additionally kills
//!      wpa_supplicant + dhcpcd; NM mode leaves NM's supplicant alone
//!      (the CONSYS chip itself stays powered — see above).
//!   6. the final state write (state=sleeping was set in step 0) so `off`
//!      restores exactly (services that were running, cores offlined,
//!      backlight%, wifi was on).
//!
//! `sleep off` reverses, again visible first: backlight on, rebind the
//! inputs, online the recorded A53 cpus, bring the A72 cluster back up if
//! it was up, then start the recorded services + re-associate wifi
//! (async).
//!
//! `sleep key` is the daemon entry (gemini-sleepd.service): it watches
//! /dev/input/eventN for mt6351-keys KEY_SLEEP presses and toggles.
//! Only value==1 (press) toggles; presses closer than 1 s to a handled
//! press are debounced (a driver double-report or an impatient second
//! press is ONE toggle); events queued while a toggle ran are drained
//! so a mash cannot cascade.

use std::os::fd::{AsRawFd, FromRawFd};
use std::process::Command;
use std::time::Instant;

use crate::backlight;
use crate::error::{cmsg, Res};
use crate::util;

const STATE_FILE: &str = "/run/gemcli-sleep.state";
/// Presses closer than this to a handled press are ignored (one physical
/// press = one toggle even if the polled driver double-reports).
const DEBOUNCE_MS: u64 = 1000;

/// Heavyweight services stopped on sleep (only those actually running
/// are stopped; wake starts exactly the stopped set). The desktop-aware
/// part is the first three entries — whichever session
/// /var/lib/gemini/desktop selects:
///   - GNOME    -> display-manager.service (stopping GDM ends the cjdell
///                 gnome-session; `start` auto-logins it again, verified
///                 on glass 2026-09-12);
///   - gemshell -> gemini-gemshell.service;
///   - legacy gemwl -> gemwl.service (no session client since the nested
///                 LXQt/Phosh sessions were removed 2026-09-12).
/// Plus the audio session. sshd + the battery guard + gemini-sleepd
/// itself are never in this list. The wifi *units* are deliberately not
/// here (RemainAfterExit oneshots — stopping them does not stop wifi);
/// wifi is handled separately via the interface + daemons (§wifi).
const SERVICES: &[&str] = &[
    "display-manager.service", // GNOME via GDM (the default panel owner)
    "gemini-gemshell.service", // native compositor session
    "gemwl.service",           // legacy nested compositor (if ever active)
    "pipewire.service",
    "wireplumber.service",
    "pipewire-pulse.service",
];

/// Resolve a host binary: the stable /run/current-system/sw path on
/// NixOS (systemd-unit PATH is minimal), bare name as the fallback.
fn swbin(name: &str) -> String {
    util::swbin(name)
}

fn systemctl() -> String {
    swbin("systemctl")
}

// --- state file -----------------------------------------------------------

fn read_state() -> std::collections::HashMap<String, String> {
    let mut m = std::collections::HashMap::new();
    if let Ok(s) = std::fs::read_to_string(STATE_FILE) {
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
    util::write_str(STATE_FILE, &s)
}

fn clear_state() {
    let _ = std::fs::remove_file(STATE_FILE);
}

fn is_sleeping() -> bool {
    read_state().get("state").map(|s| s == "sleeping").unwrap_or(false)
}

/// Public read of the sleep state for sibling daemons: the power-profile
/// watcher (`profile.rs`) must NOT touch the A72 cluster while the
/// silver-button light sleep owns it, and it re-applies the active
/// power profile once the device is awake again.
pub fn sleeping() -> bool {
    is_sleeping()
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
    // reset-failed first: a rapid stop/start cycle can leave a unit in
    // the start-limit-hit failed state (observed with gemwl during the
    // v1 button-mash); systemctl start on it then fails until reset.
    let _ = Command::new(systemctl())
        .args(["reset-failed", u])
        .status();
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

// --- wifi (fast path — chip stays powered, §header) -------------------------

/// Is NetworkManager the connectivity manager? (services/wifi.nix default
/// since 2026-09-10: NM owns wpa_supplicant + the connection profile and
/// the legacy `gemini-wifi-auto` unit does not exist.) With NM, sleep
/// only parks the radio (link down) — NM re-activates on wake.
fn nm_active() -> bool {
    Command::new(systemctl())
        .args(["is-active", "--quiet", "NetworkManager.service"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The wifi interface name, if one exists (any netdev with a wireless
/// dir). No external tools needed.
fn wifi_iface() -> Option<String> {
    for e in std::fs::read_dir("/sys/class/net").ok()?.flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if util::exists(&format!("/sys/class/net/{n}/wireless")) {
            return Some(n);
        }
    }
    None
}

/// Take wifi down FAST (no CONSYS chip power-cycle — that `echo off`
/// teardown stalls ~29 s with the chip associated): interface down +
/// kill wpa_supplicant + the iface's dhcpcd. The radio firmware idles;
/// wake re-associates (`wifi auto` in the legacy stack; NetworkManager
/// autoconnect in NM mode).
fn wifi_down() -> bool {
    let Some(iface) = wifi_iface() else {
        return false;
    };
    let _ = Command::new(swbin("ip")).args(["link", "set", &iface, "down"]).status();
    if nm_active() {
        // NM owns wpa_supplicant (killing it just makes NM respawn it)
        // and has no dhcpcd lease of ours — parking the link is enough;
        // on `ip link set up` NM re-activates the saved profile.
        println!("gemcli sleep: wifi {iface} down (NetworkManager manages; chip powered)");
    } else {
        let _ = Command::new(swbin("pkill")).args(["-f", "wpa_supplicant -B"]).status();
        let _ = Command::new(swbin("pkill"))
            .args(["-f", &format!("dhcpcd.*{iface}")])
            .status();
        println!("gemcli sleep: wifi {iface} down (chip powered, daemons stopped)");
    }
    true
}

/// Re-associate. Legacy: restart the wifi-auto unit (its ExecStart =
/// `wifi auto`; RESTART not start — it is a RemainAfterExit oneshot that
/// stayed active through sleep). NM mode: raise the link and let NM's
/// autoconnect re-activate the saved profile (no unit to restart).
/// Async/cheap either way.
fn wifi_up() {
    let Some(iface) = wifi_iface() else {
        return;
    };
    let _ = Command::new(swbin("ip")).args(["link", "set", &iface, "up"]).status();
    if nm_active() {
        println!("gemcli sleep: wifi {iface} up (NetworkManager will re-associate)");
        return;
    }
    println!("gemcli sleep: wifi auto (re-associate)");
    let _ = Command::new(systemctl()).args(["reset-failed", "gemini-wifi-auto.service"]).status();
    let ok = Command::new(systemctl())
        .args(["restart", "--no-block", "gemini-wifi-auto.service"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        println!("gemcli sleep: WARNING restarting gemini-wifi-auto failed");
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

/// `gemcli sleep on` — enter the light clamshell sleep. The backlight
/// goes off FIRST (instant press feedback); the whole transition is
/// ~2 s worst case (nothing blocks for the wifi chip teardown).
pub fn on() -> i32 {
    if is_sleeping() {
        println!("gemcli sleep: already asleep ({STATE_FILE} — run 'gemcli sleep off')");
        return 0;
    }
    let mut rc = 0;
    let mut state = std::collections::HashMap::new();

    // 0. Mark the sleep IMMEDIATELY, before touching the A72 cluster or
    //    the cpus. The profile watcher (gemini-power-profile) reads this
    //    state file: it must stand down before we power the A72 rail
    //    down, or it can race us straight back up — two concurrent secure
    //    A72 ops hung the device on glass (2026-09-10q). Writing the
    //    state first also means a crash mid-transition still leaves the
    //    device "asleep" (wakeable) rather than half-torn-down and awake.
    state.insert("state".into(), "sleeping".into());
    state.insert("stamp".into(), util::stamp());
    if let Err(e) = write_state(&state) {
        println!("gemcli sleep: ERROR writing early state: {}", e.msg);
        return 1;
    }

    // 1. backlight — INSTANT visible acknowledgement (bl_power=4 keeps
    //    the brightness value; on() restores it)
    let pct = backlight::get().unwrap_or(0);
    state.insert("backlight_pct".into(), pct.to_string());
    match backlight::off() {
        Ok(()) => println!("gemcli sleep: backlight off (was {pct}%)"),
        Err(e) => {
            println!("gemcli sleep: ERROR backlight off: {}", e.msg);
            rc = 1;
        }
    }

    // 2. clamshell inputs (closed-lid phantom keys generate nothing)
    for (drv, dev, what) in [(KBD_DRV, KBD_DEV, "keyboard matrix"), (TCH_DRV, TCH_DEV, "touchscreen")] {
        match unbind_input(drv, dev) {
            Ok(()) => println!("gemcli sleep: {what} ({dev}) disabled"),
            Err(e) => {
                println!("gemcli sleep: WARNING {e}");
                rc = 1;
            }
        }
    }

    // 3a. A72 cluster first, WHILE the A53s are still online. The secure
    //     power_off_cl3 (CCI/snoop/SPM) is what the verified watcher path
    //     runs with all little cores up; doing it AFTER offlining the
    //     A53s is what hung the device (2026-09-10q). Only when the
    //     cluster is actually up (performance power mode / manual
    //     `a72 up`) — the default cold-boot state leaves it down.
    let a72_was_up = crate::sysfs::cpu_is_online(8) || crate::sysfs::cpu_is_online(9);
    if a72_was_up {
        println!("gemcli sleep: A72 cluster up — powering down (cl2-down, WDT-guarded)");
        // Hold the A72 lock across the whole secure teardown so the
        // watcher/CLI cannot drive the rail concurrently. The state file
        // above already made the watcher stand down; the lock closes the
        // poll-window race.
        let guard = crate::a72::lock();
        let a72_rc = crate::a72::down_locked("both");
        drop(guard);
        if a72_rc != 0 {
            println!("gemcli sleep: WARNING A72 power-down returned {a72_rc}");
            rc = 1;
        }
        state.insert("a72".into(), "1".into());
    } else {
        println!("gemcli sleep: A72 cluster already down");
    }

    // 3b. A53 cpus 1..7 offline (cpu0 must run the kernel)
    let off = offline_workers();
    state.insert("cpus_offline".into(), off.clone());
    // Persist what we have so far: if the remainder is interrupted, the
    // wake path still restores the backlight/inputs/cpus/A72.
    let _ = write_state(&state);

    // 4. heavyweight services (record, then stop async)
    let running = services_running();
    stop_running(&running);
    state.insert("services".into(), running.join(","));

    // 5. wifi: fast down (iface + daemons; chip stays powered)
    if wifi_down() {
        state.insert("wifi".into(), "1".into());
    }

    // 6. state — the final write (state=sleeping was set early in step 0)
    if let Err(e) = write_state(&state) {
        println!("gemcli sleep: ERROR writing state: {}", e.msg);
        return 1;
    }
    println!(
        "gemcli sleep: ASLEEP (cpu0 only, backlight off, inputs off, {} service(s) stopped)",
        running.len()
    );
    rc
}

/// `gemcli sleep off` — wake from the light sleep (reverse of `on`,
/// visible bits first).
pub fn off() -> i32 {
    let state = read_state();
    if state.get("state").map(|s| s != "sleeping").unwrap_or(true) {
        println!("gemcli sleep: not asleep — nothing to wake");
        return 0;
    }
    let mut rc = 0;

    // 1. backlight on — INSTANT visible acknowledgement (restores the
    //    retained brightness)
    if let Err(e) = backlight::on() {
        println!("gemcli sleep: ERROR backlight on: {}", e.msg);
        rc = 1;
    } else {
        let pct = state.get("backlight_pct").cloned().unwrap_or_else(|| "?".into());
        println!("gemcli sleep: backlight on (restoring {pct}%)");
    }

    // 2. inputs back (touch first — it owns the i2c irq)
    for (drv, dev, what) in [(TCH_DRV, TCH_DEV, "touchscreen"), (KBD_DRV, KBD_DEV, "keyboard matrix")] {
        match bind_input(drv, dev) {
            Ok(()) => println!("gemcli sleep: {what} ({dev}) enabled"),
            Err(e) => {
                println!("gemcli sleep: WARNING {e}");
                rc = 1;
            }
        }
    }

    // 3. cores (the A72 cluster is restored AFTER the A53s, so the warm
    //    cpu9 hotplug has an online cpu8 to land next to). Default to the
    //    full A53 set if the state was truncated by an interrupted sleep.
    let default_off = "1,2,3,4,5,6,7".to_string();
    online_cpus(state.get("cpus_offline").unwrap_or(&default_off));
    if state.get("a72").map(|v| v == "1").unwrap_or(false) {
        println!("gemcli sleep: restoring A72 cluster (cl2-up, WDT-guarded)");
        let guard = crate::a72::lock();
        let a72_rc = crate::a72::up_locked("both");
        drop(guard);
        if a72_rc != 0 {
            println!("gemcli sleep: WARNING A72 bring-up returned {a72_rc}");
            rc = 1;
        }
    }

    // 4. services + wifi (async — the visible wake is backlight+cores)
    if let Some(units) = state.get("services") {
        let list: Vec<&str> = units.split(',').filter(|s| !s.is_empty()).collect();
        if !list.is_empty() {
            println!("gemcli sleep: waking services: {}", list.join(" "));
            start_units(&list);
        }
    }
    if state.get("wifi").map(|v| v == "1").unwrap_or(false) {
        wifi_up();
    }

    clear_state();
    println!("gemcli sleep: AWAKE");
    rc
}

/// `gemcli sleep status` — print the sleep state + what a toggle would
/// touch.
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
        if let Some(v) = s.get("a72") {
            println!("A72 cluster: {}", if v == "1" { "was up (will be restored)" } else { "was down" });
        }
        if let Some(v) = s.get("backlight_pct") {
            println!("backlight was: {v}%");
        }
        if let Some(v) = s.get("wifi") {
            println!("wifi was: {}", if v == "1" { "on (will re-associate)" } else { "off" });
        }
        return 0;
    }
    println!("state: AWAKE");
    let on = crate::sysfs::cpu_online().unwrap_or_default();
    println!("cpu online: {}", on.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(","));
    println!("A72 cluster: {}",
        if on.contains(&8) || on.contains(&9) { "up" } else { "down" });
    println!("services running: {}", services_running().join(" "));
    println!("inputs: kbd={} touch={}",
        if driver_has(KBD_DRV, KBD_DEV) { "bound" } else { "unbound" },
        if driver_has(TCH_DRV, TCH_DEV) { "bound" } else { "unbound" });
    println!("backlight: {}%", backlight::get().unwrap_or(0));
    println!("wifi iface: {}", wifi_iface().unwrap_or_else(|| "(none)".into()));
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

/// Discard any events queued while a toggle was running (the driver
/// polls every 25 ms; presses landing during the ~2 s transition would
/// otherwise be read right after it and cascade into rapid
/// sleep/wake/sleep... flicker).
fn drain_pending(fd: i32) {
    unsafe {
        let fl = libc::fcntl(fd, libc::F_GETFL);
        if fl < 0 {
            return;
        }
        libc::fcntl(fd, libc::F_SETFL, fl | libc::O_NONBLOCK);
        let mut ev = InputEvent { sec: 0, usec: 0, type_: 0, code: 0, value: 0 };
        loop {
            let n = libc::read(
                fd,
                &mut ev as *mut InputEvent as *mut libc::c_void,
                std::mem::size_of::<InputEvent>(),
            );
            if n != std::mem::size_of::<InputEvent>() as isize {
                break; // EAGAIN (buffer empty) or partial — stop
            }
        }
        libc::fcntl(fd, libc::F_SETFL, fl);
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
    let mut last_handled = Instant::now() - std::time::Duration::from_millis(DEBOUNCE_MS);
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
        if ev.type_ != EV_KEY || ev.code != KEY_SLEEP || ev.value != 1 {
            continue; // only a fresh press toggles; repeats/releases ignored
        }
        let now = Instant::now();
        if now.duration_since(last_handled).as_millis() < DEBOUNCE_MS as u128 {
            println!("{} gemcli sleep: press debounced ({} ms since last)", util::hms(),
                now.duration_since(last_handled).as_millis());
            continue;
        }
        last_handled = now;
        toggle();
        // presses that arrived while the toggle ran belong to it
        drain_pending(file.as_raw_fd());
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn state_roundtrip() {
        let mut m: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        m.insert("state".into(), "sleeping".into());
        m.insert("cpus_offline".into(), "1,2,3".into());
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

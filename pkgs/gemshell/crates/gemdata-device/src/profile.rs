//! Power-mode → A72 cluster bridge — `gemcli profile watch|status|set`.
//!
//! The Gemini PDA has no cpufreq/DVFS driver: the ONLY lever the standard
//! GNOME "Power Mode" selector can meaningfully pull is the big A72
//! cluster (cpu8/cpu9), which is normally powered OFF (cold-boot state;
//! docs/power-sleep.md, a72.rs). A72 bring-up is the vendor cold sequence
//! (`gemcli a72 up`, WDT-guarded) and brings real CPU headroom — it is
//! exactly a "performance" mode.
//!
//! power-profiles-daemon (PPD) is what GNOME's Settings → Power ("Power
//! Mode") and the Quick Settings power menu talk to. On non-x86 ACPI
//! hardware PPD uses its generic **placeholder** driver, which upstream
//! advertises only power-saver + balanced (performance is deliberately
//! unavailable there — `src/ppd-driver-placeholder.c`). This tree patches
//! the placeholder to advertise `performance` too (patches/
//! power-profiles-daemon-placeholder-performance.patch), so GNOME shows
//! the full three-way selector; the placeholder still drives no hardware
//! itself.
//!
//! This daemon is the actuator that closes the loop: it polls PPD's
//! active profile and maps
//!
//!   performance              -> `a72 up both`   (cpu8 cold, cpu9 warm)
//!   balanced / power-saver   -> `a72 down both` (cpu9 then cpu8, rail off)
//!
//! It deliberately does nothing while the silver-button light sleep owns
//! the device (`sleep::sleeping()`): the sleep path powers the cluster
//! down itself and restores it on wake, and the watcher must not fight
//! that. When the device wakes, the next poll re-applies the active
//! profile.
//!
//! Boot safety: the PPD profile is PERSISTED across reboots
//! (/var/lib/power-profiles-daemon/state.ini), so a unit left in
//! performance would otherwise request the A72 bring-up while boot is
//! still busy (the exact wedge risk the a72-up unit is opt-in for —
//! docs/phase-2-on-glass.md P2). The watcher therefore waits a short
//! BOOT_SETTLE_SECS before its first A72 action (flagged in /run, tmpfs —
//! a reboot resets it), matching the "bring the cluster up from a settled
//! system" rule. A profile CHANGE during that window bypasses the rest of
//! it, so a user picking a mode in GNOME does not wait. `a72::up`
//! additionally retries the contended DA9214 i2c6 bus with backoff and
//! arms the WDT, so a transient failure self-recovers.

use std::process::Command;

use crate::error::Res;
use crate::sleep;
use crate::sysfs;
use crate::util;
use crate::a72;

/// Poll period (seconds). Profile changes are rare and user-driven, so a
/// 2 s poll is imperceptible while keeping the daemon nearly free.
const POLL_SECS: f64 = 2.0;
/// One-time boot settle before the first A72 action (see the header).
/// Kept short (2026-09-10q: 90 s made GNOME say "performance" while the
/// cores stayed off for a minute and a half); a user profile change during
/// the settle still applies immediately.
const BOOT_SETTLE_SECS: f64 = 20.0;
const SETTLE_FLAG: &str = "/run/gemini-power-profile.settled";

/// A72 cpus (the cluster the performance profile owns).
const A72: [u32; 2] = [8, 9];

fn powerprofilesctl() -> String {
    util::swbin("powerprofilesctl")
}

/// The active profile as PPD persists it
/// (`/var/lib/power-profiles-daemon/state.ini`, rewritten on every change
/// by save_configuration()). Reading this file is how the watcher stays
/// cheap — spawning powerprofilesctl (Python + D-Bus) every 2 s on a
/// battery device would be wasteful.
fn parse_state_profile(text: &str) -> Option<String> {
    let mut in_state = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_state = line == "[State]";
            continue;
        }
        if in_state {
            if let Some(v) = line.strip_prefix("Profile=") {
                let v = v.trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

fn state_profile() -> Option<String> {
    parse_state_profile(&std::fs::read_to_string("/var/lib/power-profiles-daemon/state.ini").ok()?)
}

/// The active profile for the watcher. PPD persists every user change to
/// state.ini; while the file is absent PPD has never had a change, so the
/// profile is its built-in default ("balanced" — see its
/// `data->active_profile = PPD_PROFILE_BALANCED` init). Never spawns the
/// Python powerprofilesctl on the poll path.
pub fn watched_profile() -> String {
    state_profile().unwrap_or_else(|| "balanced".into())
}

/// The active profile for `status`: ask PPD through its CLI (accurate and
/// only run on demand), falling back to the persisted file / default.
pub fn active_profile() -> Option<String> {
    if let Ok(out) = Command::new(powerprofilesctl()).arg("get").output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    Some(watched_profile())
}

fn a72_online() -> bool {
    A72.iter().any(|c| sysfs::cpu_is_online(*c))
}

fn cpu_map() -> String {
    sysfs::cpu_online()
        .map(|v| v.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(","))
        .unwrap_or_else(|_| "?".into())
}

/// Reconcile the cluster with the active profile. Returns true when an
/// a72 action actually ran (so the caller can pace retries).
fn reconcile(verbose: bool) -> bool {
    let profile = watched_profile();
    let want_perf = profile == "performance";
    // Serialize with the sleep path and the CLI: take the A72 lock BEFORE
    // deciding, then re-check the sleep state *under* it. A sleep that
    // started mid-poll therefore always wins, and two secure A72 ops can
    // never overlap (a concurrent up/down hung the device 2026-09-10q).
    let Some(_guard) = a72::lock() else {
        if verbose {
            println!("gemcli profile: A72 lock unavailable — skipping reconcile");
        }
        return false;
    };
    if sleep::sleeping() {
        return false;
    }
    let have = a72_online();
    if want_perf && !have {
        println!(
            "{} gemcli profile: profile=performance -> A72 bring-up (online={})",
            util::hms(),
            cpu_map()
        );
        let rc = a72::up_locked("both");
        if rc != 0 {
            println!("gemcli profile: WARNING A72 bring-up returned {rc}");
        }
        return true;
    }
    if !want_perf && have {
        println!(
            "{} gemcli profile: profile={profile} -> A72 power-down (online={})",
            util::hms(),
            cpu_map()
        );
        let rc = a72::down_locked("both");
        if rc != 0 {
            println!("gemcli profile: WARNING A72 power-down returned {rc}");
        }
        return true;
    }
    if verbose {
        println!(
            "gemcli profile: profile={profile}, A72 {} — consistent",
            if have { "online" } else { "offline" }
        );
    }
    false
}

/// `gemcli profile watch` — the gemini-power-profile daemon (foreground,
/// runs until killed). Backs services/power-profiles.nix.
pub fn watch() -> i32 {
    println!(
        "gemcli profile: watching power-profiles-daemon (poll {POLL_SECS}s); A72 = performance"
    );
    // One-time per-boot settle (see header). A profile CHANGE during the
    // settle bypasses the rest of it: if the user reaches GNOME and picks
    // a mode, it must apply now, not once the boot window closes.
    if !util::exists(SETTLE_FLAG) {
        println!(
            "gemcli profile: boot settle {BOOT_SETTLE_SECS}s before the first A72 action (a profile change applies immediately)"
        );
        let boot_profile = watched_profile();
        let start = std::time::Instant::now();
        while start.elapsed().as_secs_f64() < BOOT_SETTLE_SECS {
            if watched_profile() != boot_profile {
                println!("gemcli profile: profile changed during boot settle — applying now");
                break;
            }
            util::sleep(POLL_SECS);
        }
        let _ = util::write_str(SETTLE_FLAG, &format!("{}\n", util::stamp()));
    }
    loop {
        if sleep::sleeping() {
            // The sleep path owns the cluster; do not fight it. A short
            // poll keeps wake latency low.
            util::sleep(POLL_SECS);
            continue;
        }
        let acted = reconcile(false);
        // A long A72 action (up_cold retries can take minutes) already
        // paces the loop; on a failed action give the bus a wider berth
        // so a hard-failing bring-up is not retried every 2 s.
        let rc_bad = acted && {
            // re-read: a failed a72::up leaves cpu8 offline
            let perf = watched_profile() == "performance";
            let have = a72_online();
            (perf && !have) || (!perf && have)
        };
        util::sleep(if rc_bad { 30.0 } else { POLL_SECS });
    }
}

/// `gemcli profile status` — active profile + cluster state.
pub fn status() -> i32 {
    match active_profile() {
        Some(p) => println!("power profile: {p}"),
        None => println!("power profile: (unknown)"),
    }
    let have = a72_online();
    println!("A72 cluster (cpu8/9): {}", if have { "online" } else { "offline" });
    println!("cpu online: {}", cpu_map());
    println!(
        "sleep state: {}",
        if sleep::sleeping() { "ASLEEP (watcher paused)" } else { "awake" }
    );
    0
}

/// `gemcli profile set <profile>` — thin wrapper over
/// `powerprofilesctl set` (so scripts/one-liners do not need to know the
/// PPD CLI), then a synchronous reconcile so the cluster follows at once.
pub fn set(profile: &str) -> Res<()> {
    match profile {
        "performance" | "balanced" | "power-saver" => {}
        other => {
            return Err(crate::error::cmsg(format!(
                "unknown profile '{other}' (want performance|balanced|power-saver)"
            )))
        }
    }
    let out = Command::new(powerprofilesctl())
        .args(["set", profile])
        .status()
        .map_err(|e| crate::error::cmsg(format!("powerprofilesctl set: {e}")))?;
    if !out.success() {
        return Err(crate::error::cmsg("powerprofilesctl set failed"));
    }
    println!("gemcli profile: set to {profile}");
    // apply immediately in this process too (the watcher would catch up,
    // but this makes the CLI authoritative/instant)
    reconcile(true);
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a72_list_is_cpu8_cpu9() {
        assert_eq!(super::A72, [8, 9]);
    }

    #[test]
    fn parses_ppd_state_ini() {
        let ini = "[State]\nCpuDriver=placeholder\nProfile=performance\nbattery_aware=TRUE\n\n[Actions]\ntrickle_charge=true\n";
        assert_eq!(super::parse_state_profile(ini).as_deref(), Some("performance"));
        // key order does not matter; the [Actions] section is ignored
        let ini = "[Actions]\ntrickle_charge=true\n[State]\nProfile=power-saver\n";
        assert_eq!(super::parse_state_profile(ini).as_deref(), Some("power-saver"));
        assert_eq!(super::parse_state_profile("[State]\nbattery_aware=TRUE\n"), None);
    }
}

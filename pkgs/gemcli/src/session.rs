//! Desktop/session selection — GNOME, COSMIC, or the framebuffer
//! console.
//!
//! GNOME and COSMIC are both ordinary GDM Wayland sessions on the
//! geminipda-drm KMS device, co-installed; exactly one owns the panel
//! per boot. This module owns the PERSISTENT marker
//! `/var/lib/gemini/desktop` (one line: gnome|cosmic|console) that the
//! boot-time `gemini-desktop-apply.service` reads before
//! display-manager.service starts:
//!
//!   gnome|cosmic  the service sets AccountsService `Session` /
//!                 `SessionType=wayland`, which GDM auto-logs into;
//!   console       the service creates /run/gemini-console, and
//!                 display-manager.service carries
//!                 ConditionPathExists=!/run/gemini-console, so GDM is
//!                 skipped and the fbcon console stays.
//!
//! `gemcli session` is the runtime half that plain NixOS options cannot
//! provide. Story + receipts: docs/desktop-selection.md;
//! services/desktop-select.nix; services/scripts/gemini-desktop-apply.

use std::path::Path;
use std::process::Command;

use crate::error::{cmsg, Res};
use crate::util;

/// Valid modes, in display order.
pub const MODES: [&str; 3] = ["gnome", "cosmic", "console"];
/// Persistent marker (one line).
pub const MARKER: &str = "/var/lib/gemini/desktop";
/// Flag file created in console mode; conditions display-manager off.
pub const SENTINEL: &str = "/run/gemini-console";
/// Auto-login user (services.geminiDesktop.user / config users.users).
pub const USER: &str = "cjdell";

fn valid(mode: &str) -> bool {
    MODES.contains(&mode)
}

/// The persisted mode, if the marker exists and is non-empty.
pub fn read_marker() -> Option<String> {
    util::read_str_opt(MARKER).filter(|s| !s.is_empty())
}

/// Persist the mode atomically (tmp + rename).
pub fn write_marker(mode: &str) -> Res<()> {
    if !valid(mode) {
        return Err(cmsg(format!(
            "unknown session '{}' (want {})",
            mode,
            MODES.join("|")
        )));
    }
    let p = Path::new(MARKER);
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| cmsg(format!("{}: {e}", dir.display())))?;
    }
    let tmp = format!("{MARKER}.tmp");
    std::fs::write(&tmp, format!("{mode}\n")).map_err(|e| cmsg(format!("{tmp}: {e}")))?;
    std::fs::rename(&tmp, MARKER).map_err(|e| cmsg(format!("{MARKER}: {e}")))?;
    Ok(())
}

/// `session status` — marker, console sentinel, and the session
/// AccountsService currently reports for the auto-login user (the same
/// value GDM acts on).
pub fn status() -> Res<()> {
    println!(
        "marker ({MARKER}): {}",
        read_marker().unwrap_or_else(|| "(unset — compile-time default applies)".into())
    );
    println!(
        "console sentinel ({SENTINEL}): {}",
        if Path::new(SENTINEL).exists() { "present (display-manager skipped)" } else { "absent" }
    );
    match accounts_session() {
        Some((s, t)) => println!("AccountsService session ({USER}): {s} ({t})"),
        None => println!("AccountsService session ({USER}): (unreadable)"),
    }
    println!("modes: {}", MODES.join(", "));
    Ok(())
}

/// `session list` — the modes this build can select.
pub fn list() {
    let marker = read_marker();
    let here = |m: &str| if marker.as_deref() == Some(m) { " *" } else { "" };
    for m in MODES {
        let desc = match m {
            "gnome" => "GNOME on GDM (Wayland, geminipda-drm KMS + panfrost)",
            "cosmic" => "COSMIC on GDM (Wayland, geminipda-drm KMS + panfrost)",
            "console" => "no desktop — framebuffer console on tty1",
            _ => "",
        };
        println!("{m:8} {desc}{}", here(m));
    }
}

/// AccountsService `Session` + `SessionType` for the desktop user, via
/// busctl. This is the value GDM reads for auto-login.
fn accounts_session() -> Option<(String, String)> {
    let uid = uid_of(USER)?;
    let path = format!("/org/freedesktop/Accounts/User{uid}");
    let s = busctl_session(&path, "Session")?;
    let t = busctl_session(&path, "SessionType")?;
    Some((s, t))
}

fn busctl_session(path: &str, prop: &str) -> Option<String> {
    let out = Command::new(util::swbin("busctl"))
        .args([
            "get-property",
            "org.freedesktop.Accounts",
            path,
            "org.freedesktop.Accounts.User",
            prop,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // busctl prints e.g. `s "cosmic"`.
    let s = String::from_utf8_lossy(&out.stdout);
    s.split('"').nth(1).map(str::to_string)
}

fn uid_of(user: &str) -> Option<u32> {
    let out = Command::new(util::swbin("id")).args(["-u", user]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Re-run the boot applier now (AccountsService / console sentinel).
/// The script is on the system profile (gemini-pda-utils, in
/// environment.systemPackages).
pub fn apply() -> Res<()> {
    let st = Command::new(util::swbin("gemini-desktop-apply"))
        .status()
        .map_err(|e| cmsg(format!("gemini-desktop-apply: {e}")))?;
    if !st.success() {
        return Err(cmsg(format!(
            "gemini-desktop-apply failed (rc {})",
            st.code().unwrap_or(-1)
        )));
    }
    Ok(())
}

/// `session set <mode>` — persist and (optionally) apply / reboot.
pub fn set(mode: &str, apply_now: bool, reboot: bool) -> Res<()> {
    write_marker(mode)?;
    println!("session: {mode} (marker {MARKER})");
    if apply_now {
        apply()?;
        println!("session: applied now (AccountsService / console sentinel updated)");
    }
    if reboot {
        // A clean switch: the marker is persistent, the boot applier
        // reads it before GDM. Plain systemctl reboot is the healthy
        // path since the mt6797-power driver (docs/power-states.md).
        println!("session: rebooting to switch to {mode}");
        crate::boot::reboot_system()?;
    } else if !apply_now {
        println!("session: takes effect at next boot (or pass --apply / --reboot)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn modes_are_the_three_known() {
        assert_eq!(super::MODES, ["gnome", "cosmic", "console"]);
        assert!(super::valid("gnome"));
        assert!(super::valid("console"));
        assert!(!super::valid("kde"));
    }
}

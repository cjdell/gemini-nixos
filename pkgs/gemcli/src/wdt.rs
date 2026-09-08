//! MTK watchdog (WDT) helpers + device-side self-boot reboot — port of
//! services/scripts/gemini-wdt-reboot.
//!
//! On this unit a plain software reset (reboot(2)/TOPRGU, or a WDT
//! soft-reset) POWERS THE PDA OFF and it stays off — only the MTK WDT
//! external-reset (EXRST) path configured by LK self-boots: when the
//! WDT expires, the PMIC power-cycles and LK boots the `boot` partition
//! (verified 2026-08-31).
//!
//! Arm: WDT_LENGTH 0x10007004 = (SECS<<5)|0x8 — 1 count = 1 s on this
//! SoC, key bit 0x8 (encoding field-verified from cl2-up.sh's 15 s
//! guard and the host device-reboot.sh's 0x48/2 s).
//! Disarm: MODE 0x10007000 = 0x22000000 (key-protected — a plain 0 is
//! IGNORED and the armed WDT fires later; [corrected 2026-09-04]).

use crate::devmem;
use crate::error::{cmsg, Res};

const WDT_LENGTH: u64 = 0x10007004;
const WDT_MODE: u64 = 0x10007000;

/// Arm the WDT for `secs` (2..=31). Free-running: fires at ~SECS
/// regardless of whether this process is still alive.
pub fn arm(secs: u32) -> Res<()> {
    devmem::wr32(WDT_LENGTH, (secs << 5) | 8)
}

pub fn disarm() -> Res<()> {
    devmem::wr32(WDT_MODE, 0x22000000)
}

/// `wdt-reboot [SECS]` (2..31, default 20): arm, sync, and wait for the
/// EXRST power-cycle. If the WDT does not fire the device is still up —
/// say so loudly instead of looking idle (script parity).
pub fn wdt_reboot(secs: u32) -> Res<()> {
    if !(2..=31).contains(&secs) {
        return Err(cmsg("usage: gemcli wdt-reboot [2..31]"));
    }
    println!("gemcli wdt-reboot: arming WDT for {secs}s (EXRST) — device power-cycles in ~{}s", secs + 2);
    arm(secs)?;
    crate::util::sync_all();
    crate::util::sleep(secs as f64);
    Err(cmsg(format!(
        "WARNING — WDT did not fire after {secs}s; no reboot happened"
    )))
}

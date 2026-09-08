//! A72 cluster (cpu8/cpu9) bring-up / power-down — ports of
//! services/scripts/cl2-up.sh and cl2-down.sh.
//!
//! Sequence (BSP receipt: gemini-linux-kernel-3.18
//! arch/arm64/kernel/psci.c cpu_power_on_buck under CONFIG_CL2_BUCK_CTRL,
//! validated 2026-09-04 on hardware): plain PSCI CPU_ON of a COLD A72
//! cluster hangs the secure world (whole box freezes); with this
//! pre-sequence CPU_ON returns and the core boots:
//!   1. DA9214 BUCKB enable (VPROC2/A72 rail): reg 0x5E bit0 @ 0x68 on
//!      the i2c6-hw bus (0x1100e000). READS are unreliable here
//!      (SCP/DVFSP shares the bus) — i2c ACK is the trusted channel.
//!   2. SPM 0x10006218 bit0; 3. SWSYSRST latch (0x10007018 |=
//!      0x88000800); 4. SPM 0x10006290 &= ~3 (clear EXT_BUCK_ISO);
//!      5. SRAM-LDO SMC 0xC20003BF arg 110000 via /dev/idvfs-sramldo;
//!      6. PSCI CPU_ON.
//!
//! Safety: the MTK WDT (15 s arm: 0x10007004 = s<<5|0x8) is armed right
//! before CPU_ON and disarmed after, so ANY freeze self-recovers via
//! EXRST; the whole attempt retries with backoff so a transiently-busy
//! DA9214 bus doesn't fail the bring-up.
//!
//! Down: per-core PSCI offline is SAFE (cpu9 while cpu8 is up); the
//! LAST A72's teardown runs the secure world's power_off_cl3 inside the
//! controlling core's AFFINITY_INFO SMC (CCI/snoop/SPM/ISO), which has
//! EIGHT+ UNBOUNDED wait sites — a stall = cpu0 stuck in the secure
//! world = whole-box freeze, so the WDT (20 s) is the ONLY recovery.
//! After the teardown re-asserts ISO (0x10006290 bit1) Linux drops the
//! external DA9214 BUCKB rail (the secure world has no direct i2c).
//! Proven live 2026-09-07 ("psci: CPU9 killed (polled 0 ms)");
//! nobody had run the last-A72 branch on mainline before then.
//!
//! [corrected 2026-09-09] cl2-up.sh's final process exit code was the
//! trailing `log` echo's rc (always 0 even after GAVE UP); gemcli
//! returns the real outcome (0 = requested cpus online, 1 = failure).

use crate::devmem;
use crate::error::Res;
use crate::i2c;
use crate::sysfs;
use crate::util;
use crate::wdt;

const DA9214_ADDR: u8 = 0x68;
const DA9214_CTRL_BASE: &str = "1100e000";
const BUCKB_REG: u8 = 0x5e;

const SPM_PWR_CON: u64 = 0x10006218; // MP2_CPUSYS_PWR_CON (bit0 = cluster on)
const SWSYSRST: u64 = 0x10007018; // SWSYSRST latch
const SPM_ISO: u64 = 0x10006290; // EXT_BUCK_ISO (bit1) + neighbours

fn log(prefix: &str, msg: &str) {
    println!("{prefix}: {msg}");
}

fn cpu_online_list() -> String {
    sysfs::cpu_online().map(|v| v.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",")).unwrap_or_default()
}

// WDT arm/disarm live in wdt.rs (the MODE register is key-protected — a
// plain 0 write is IGNORED; must OR MTK_WDT_MODE_KEY 0x22000000).
// [corrected 2026-09-04]

/// Full cold-cluster up for one cpu (cpu8), retried with backoff — the
/// cl2-up.sh up_cold loop. Returns true when the cpu came online.
fn up_cold(cpu: u32) -> bool {
    let prefix = "cl2-up";
    for attempt in 1..=6 {
        // Re-resolve the DA9214 bus EVERY attempt: the i2c6 controller
        // can probe LATE (deferred probe), so a bus computed once at
        // start is empty when the unit runs early after boot (observed
        // 2026-09-07 on #329).
        let bus = i2c::adapter_for(DA9214_CTRL_BASE).ok().flatten();
        log(prefix, &format!("[cpu{cpu}] attempt {attempt} (bus i2c-{})", bus.map(|b| b.to_string()).unwrap_or_default()));
        let mut acked = false;
        if let Some(bus) = bus {
            for _ in 0..10 {
                if i2c::wr_reg_ok(bus, DA9214_ADDR, BUCKB_REG, 0x01, false) {
                    acked = true;
                    break;
                }
                util::sleep(0.2);
            }
        }
        if !acked {
            match bus {
                None => log(prefix, &format!("[cpu{cpu}] DA9214 i2c adapter (0x1100e000) not probed yet — backing off 20s")),
                Some(_) => log(prefix, &format!("[cpu{cpu}] BUCKB write never ACKed — bus busy (SCP?), backing off 20s")),
            }
            util::sleep(20.0);
            continue;
        }
        // SPM pre-sequence (vendor cpu_power_on_buck order)
        let _ = devmem::rmw_set(SPM_PWR_CON, 1);
        let sr = match devmem::rd32(SWSYSRST) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let _ = devmem::wr32(SWSYSRST, sr | 0x88000800);
        let _ = devmem::rmw_clr(SPM_ISO, 3);
        let sr = match devmem::rd32(SWSYSRST) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let _ = devmem::wr32(SWSYSRST, (sr | 0x88000000) & !0x800);
        if util::exists("/dev/idvfs-sramldo") {
            let _ = std::fs::write("/dev/idvfs-sramldo", b"110000");
        } else {
            log(prefix, &format!("[cpu{cpu}] /dev/idvfs-sramldo missing — insmod sramldo-smc.ko"));
        }
        util::sleep(0.1);
        log(prefix, &format!("[cpu{cpu}] armed WDT 15s, PSCI CPU_ON"));
        let _ = wdt::arm(15);
        let ok = sysfs::set_cpu_online(cpu, true).is_ok();
        let _ = wdt::disarm();
        if ok {
            log(prefix, &format!("[cpu{cpu}] ONLINE — online={}", cpu_online_list()));
            return true;
        }
        log(prefix, &format!("[cpu{cpu}] hotplug returned nonzero — retrying"));
        util::sleep(10.0);
    }
    log(prefix, &format!("[cpu{cpu}] GAVE UP after 6 attempts"));
    false
}

fn up_warm(cpu: u32) -> bool {
    let r = sysfs::set_cpu_online(cpu, true);
    // log the outcome as ok/failed — the earlier "rc=1" form printed a
    // success-boolean and read backwards (1 = is_ok). [fixed 2026-09-08]
    log("cl2-up", &format!("cpu{cpu} warm {} — online={}", if r.is_ok() { "OK" } else { "FAILED" }, cpu_online_list()));
    r.is_ok()
}

/// `a72 up [cpu8|cpu9|both]` (default both). Exit code = the real
/// outcome, not the bash trailing-echo quirk (see header).
pub fn up(target: &str) -> i32 {
    log("cl2-up", &format!("== target={target} online={}", cpu_online_list()));
    let (ok8, ok9) = match target {
        "cpu8" => (up_cold(8), true),
        "cpu9" => (true, up_warm(9)),
        _ => {
            let ok8 = up_cold(8);
            let ok9 = ok8 && up_warm(9);
            (ok8, ok9)
        }
    };
    let final_online = cpu_online_list();
    log("cl2-up", &format!("final online={final_online}"));
    match target {
        "cpu8" => i32::from(!ok8),
        "cpu9" => i32::from(!ok9),
        _ => i32::from(!(ok8 && ok9)),
    }
}

// --- down ------------------------------------------------------------------

fn iso_set() -> bool {
    devmem::rd32(SPM_ISO).map(|v| v & 0x2 == 0x2).unwrap_or(false)
}

fn pwrcon_bit0_clear() -> bool {
    devmem::rd32(SPM_PWR_CON).map(|v| v & 0x1 == 0).unwrap_or(false)
}

/// WDT-armed PSCI offline of one cpu; returns true iff it left the map.
fn offline_one(prefix: &str, cpu: u32, secs: u32) -> bool {
    let _ = wdt::arm(secs);
    log(prefix, &format!("[cpu{cpu}] echo 0 > cpu{cpu}/online (WDT armed; hang => EXRST in {secs}s)"));
    let _ = sysfs::set_cpu_online(cpu, false);
    if !sysfs::cpu_is_online(cpu) {
        log(prefix, &format!("[cpu{cpu}] OFFLINE — online={}", cpu_online_list()));
        let _ = wdt::disarm();
        true
    } else {
        log(prefix, &format!("[cpu{cpu}] FAILED (still online) — aborting down; WDT disarmed"));
        let _ = wdt::disarm();
        false
    }
}

fn buckb_off() -> bool {
    // DA9214 BUCKB disable: reg 0x5E bit0 = 0. Reads on this bus are
    // unreliable (SCP/DVFSP shares i2c6); i2cset ACK is the trusted
    // channel.
    let Ok(Some(bus)) = i2c::adapter_for(DA9214_CTRL_BASE) else {
        log("cl2-down", "DA9214 bus (0x1100e000) not probed — rail left ON");
        return false;
    };
    for _ in 0..10 {
        if i2c::wr_reg_ok(bus, DA9214_ADDR, BUCKB_REG, 0x00, false) {
            log("cl2-down", &format!("BUCKB (A72 rail) DISABLED (i2c-{bus} 0x68 reg 0x5e=0)"));
            return true;
        }
        util::sleep(0.2);
    }
    log("cl2-down", "BUCKB write never ACKed — bus busy; rail left ON (power saving = cluster only)");
    false
}

fn down_cpu9() -> bool {
    offline_one("cl2-down", 9, 20)
}

fn down_cpu8() -> bool {
    if sysfs::cpu_is_online(9) {
        log("cl2-down", "REFUSING cpu8 offline: cpu9 still online. Run 'gemcli a72 down both' or offline cpu9 first.");
        return false;
    }
    log("cl2-down", "[cpu8] LAST-A72 offline — secure power_off_cl3 (CCI/snoop/SPM/ISO) runs in the affinity SMC;");
    log("cl2-down", "[cpu8] UNBOUNDED secure waits; WDT 20s is the only recovery if it stalls.");
    if !offline_one("cl2-down", 8, 20) {
        return false;
    }
    // teardown completed: the secure path RMW-set B_EXT_BUCK_ISO + cleared 0x218 bit0
    let iso = devmem::rd32(SPM_ISO).map(|v| format!("{v:#x}")).unwrap_or_else(|_| "?".into());
    let pwr = devmem::rd32(SPM_PWR_CON).map(|v| format!("{v:#x}")).unwrap_or_else(|_| "?".into());
    log("cl2-down", &format!("[cpu8] post-offline state: 0x10006290(ISO)={iso} 0x10006218(PWR_CON)={pwr}"));
    if iso_set() && pwrcon_bit0_clear() {
        log("cl2-down", "cluster ISOLATED from rail (ISO bit1 set, PWR_CON bit0 clear) — dropping the DA9214 rail");
        buckb_off();
    } else {
        log("cl2-down", "WARNING: ISO/PWR_CON not in the expected off state — rail LEFT ON (cluster off only)");
    }
    true
}

/// `a72 down [cpu9|cpu8|both]` (default both). Exit code parity with
/// cl2-down.sh (0 ok / 1 failed / 2 usage).
pub fn down(target: &str) -> i32 {
    log("cl2-down", &format!("== cl2-down: target={target} online={}", cpu_online_list()));
    let rc = match target {
        "cpu9" => i32::from(!down_cpu9()),
        "cpu8" => i32::from(!down_cpu8()),
        "both" => i32::from(!(down_cpu9() && down_cpu8())),
        _ => {
            log("cl2-down", "usage: gemcli a72 down [cpu9|cpu8|both]");
            return 2;
        }
    };
    log("cl2-down", &format!("final online={}   (re-enable: gemcli a72 up)", cpu_online_list()));
    rc
}

/// `a72 status` — online map + the cluster power-control registers
/// (read-only diagnostics).
pub fn status() -> Res<String> {
    let online = sysfs::cpu_online().map_err(|e| crate::error::cmsg(e.msg))?;
    let present = sysfs::cpu_present().unwrap_or_default();
    let mut s = String::new();
    s.push_str(&format!(
        "cpu online: {} (present: {})\n",
        online.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(","),
        present.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",")
    ));
    let iso = devmem::rd32(SPM_ISO).map(|v| format!("{v:#010x}")).unwrap_or_else(|_| "?".into());
    let pwr = devmem::rd32(SPM_PWR_CON).map(|v| format!("{v:#010x}")).unwrap_or_else(|_| "?".into());
    let sw = devmem::rd32(SWSYSRST).map(|v| format!("{v:#010x}")).unwrap_or_else(|_| "?".into());
    s.push_str(&format!("0x10006290 (EXT_BUCK_ISO): {iso}  (bit1 set = isolated)\n"));
    s.push_str(&format!("0x10006218 (PWR_CON):     {pwr}  (bit0 set = cluster on)\n"));
    s.push_str(&format!("0x10007018 (SWSYSRST):    {sw}\n"));
    Ok(s)
}

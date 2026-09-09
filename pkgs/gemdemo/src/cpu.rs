//! CPU setup for the show: the Gemini MT6797X ships a tri-cluster SoC —
//! 2× Cortex-A72 (cpu8/cpu9, POWERED DOWN at boot by design) + 8× A53.
//! The A72 cluster is the "we probably want A72 cores for this" knob:
//! the render/submission thread gets pinned there when the cluster is
//! (or can be made) online, leaving the A53s for audio + the compositor.
//!
//! Bring-up is NOT done from here with raw hotplug (writing cpu8/online
//! on a cold cluster hangs the secure world unless the DA9214 BUCKB +
//! SPM + SRAM-LDO pre-sequence runs first — see services/gemini-pda.nix
//! and services/scripts/cl2-up.sh). The demo therefore asks the system
//! unit `gemini-a72-up` to do it (best-effort; the unit retries with
//! WDT-armed backoff, so it is safe to fire even while the box is busy
//! rendering), polls a bounded time for cpu8 to appear, then pins.
//!
//! Everything here is best-effort: a failure to get A72s (not root, unit
//! missing, bus contended past the timeout) logs a warning and the show
//! runs on the A53s — the pipeline is light enough to hold 60 fps there
//! too in most chapters; A72s buy headroom + consistent worst cases.

use std::time::{Duration, Instant};

/// Which CPUs we want the render thread on (the A72 cluster).
const A72: &[usize] = &[8, 9];

/// The result of the CPU setup, for the identity banner + HUD.
pub struct CpuInfo {
    pub a72_online: bool,
    pub pinned_a72: bool,
    pub n_online: usize,
    pub bringup_attempted: bool,
}

impl CpuInfo {
    fn new() -> Self {
        CpuInfo {
            a72_online: false,
            pinned_a72: false,
            n_online: 0,
            bringup_attempted: false,
        }
    }
}

fn read_online() -> Vec<usize> {
    // /sys/devices/system/cpu/online — "0-7" or "0-3,5-9" style
    let mut out = Vec::new();
    let Ok(s) = std::fs::read_to_string("/sys/devices/system/cpu/online") else {
        return out;
    };
    for range in s.trim().split(',') {
        let mut it = range.split('-');
        match (it.next(), it.next()) {
            (Some(a), None) => {
                if let Ok(v) = a.trim().parse() {
                    out.push(v);
                }
            }
            (Some(a), Some(b)) => {
                let (Ok(lo), Ok(hi)) = (a.trim().parse(), b.trim().parse()) else {
                    continue;
                };
                for v in lo..=hi {
                    out.push(v);
                }
            }
            _ => {}
        }
    }
    out
}

fn cpu_is_online(cpu: usize) -> bool {
    std::fs::read_to_string(format!("/sys/devices/system/cpu/cpu{cpu}/online"))
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

fn pin_current_thread(cpus: &[usize]) -> bool {
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_ZERO(&mut set);
        for &c in cpus {
            if c < libc::CPU_SETSIZE as usize {
                libc::CPU_SET(c, &mut set);
            }
        }
        libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set) == 0
    }
}

/// Attempt to start the A72 bring-up unit. Only meaningful as root on the
/// actual device; on a host or non-root session this is a no-op warning.
fn request_a72_up() {
    // Guard: never poke systemd on a non-device host — only attempt when
    // the gemini-a72-up unit actually exists on this system.
    let Ok(out) = std::process::Command::new("systemctl")
        .args(["list-unit-files", "gemini-a72-up.service"])
        .output()
    else {
        eprintln!("cpu: no systemctl (host?) — no A72 bring-up attempted");
        return;
    };
    let listed = String::from_utf8_lossy(&out.stdout)
        .contains("gemini-a72-up.service");
    if !listed {
        eprintln!("cpu: gemini-a72-up unit not present on this system — A53s only");
        return;
    }
    let Ok(status) = std::process::Command::new("systemctl")
        .args(["start", "gemini-a72-up"])
        .status()
    else {
        eprintln!("cpu: cannot run systemctl start gemini-a72-up (running on A53s)");
        return;
    };
    if status.success() {
        eprintln!("cpu: requested gemini-a72-up (A72 cluster bring-up)");
    } else {
        eprintln!("cpu: `systemctl start gemini-a72-up` failed (rc {status}) — A53s only");
    }
}

/// Best-effort A72 bring-up + render-thread pinning. Call ONCE from the
/// thread that will run the winit/render loop (the main thread).
pub fn init() -> CpuInfo {
    let mut info = CpuInfo::new();
    let online = read_online();
    info.n_online = online.len();
    info.a72_online = A72.iter().any(|&c| cpu_is_online(c));

    if !info.a72_online {
        info.bringup_attempted = true;
        request_a72_up();
        // The unit can take ~30 s+ when the DA9214 i2c bus is contended
        // (SCP shares it early after boot) — bound the wait so a slow
        // bring-up never delays the show start; poll every ~1 s.
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if A72.iter().any(|&c| cpu_is_online(c)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        info.a72_online = A72.iter().any(|&c| cpu_is_online(c));
    }

    // Pin the calling (render) thread. On the A72 cluster if we have it,
    // else to all online CPUs (no pin — the scheduler is fine).
    if info.a72_online {
        let got = pin_current_thread(A72);
        info.pinned_a72 = got;
        if got {
            eprintln!("cpu: render thread pinned to A72 cluster (cpu8/cpu9)");
        } else {
            eprintln!("cpu: A72s online but affinity denied — scheduler placement");
        }
    } else {
        eprintln!(
            "cpu: no A72 cores ({} cpu(s) online) — running on A53s; \
             start gemini-a72-up later and rerun for the headroom",
            info.n_online
        );
    }
    info
}

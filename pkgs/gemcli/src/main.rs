//! gemcli — Gemini PDA device control, Rust native.
//!
//! One binary for the device functions the bring-up shell scripts
//! handle (services/scripts/*): backlight, battery/charger, the battery
//! safety guard daemon, A72 cluster bring-up/down, WDT reboot, boot
//! target (para) selection, GPU power, speaker amps. Each subcommand is
//! a semantic port of the corresponding verified script (see the module
//! headers for the receipts); exit codes match the script contracts
//! where a caller depends on them.
//!
//! Migration model (docs/gemcli.md): gemcli ships in the rootfs
//! ALONGSIDE the scripts — no systemd unit has been flipped yet. Run
//! `gemcli selfcheck` on the device (read-only parity probes) before
//! flipping any unit's ExecStart from script to gemcli.
//!
//! Version hygiene (AGENTS rule 0): `gemcli version` + `-V` identify
//! the exact build.

mod a72;
mod backlight;
mod battery;
mod boot;
mod charger;
mod devmem;
mod error;
mod gpio;
mod gpu;
mod guard;
mod i2c;
mod power;
mod sleep;
mod speaker;
mod status;
mod sysfs;
mod util;
mod wdt;

use clap::{Parser, Subcommand};

const BUILD_ID: &str = "gemini-nixos pkgs/gemcli (nix build; rustPlatform)";

#[derive(Parser)]
#[command(
    name = "gemcli",
    version,
    about = "Gemini PDA device control (backlight/battery/A72/WDT/boot/GPU/speaker)",
    long_about = "gemcli — Gemini PDA device control, native Rust.

Manages the device functions the bring-up shell scripts handle
(services/scripts/*): backlight, battery + the safety guard daemon,
A72 cluster up/down, WDT reboot, boot-target selection, GPU power,
speaker amps. Semantic port of the verified scripts; exit codes match
the script contracts. Run 'gemcli selfcheck' before flipping any unit
from its script to gemcli (see docs/gemcli.md)."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Display backlight (DISP_PWM0 / sysfs) — port of `backlight`
    Backlight {
        #[command(subcommand)]
        cmd: BlCmd,
    },
    /// Battery status — port of `battstat` (same exit codes 0/2/3/4/5)
    Battery {
        #[command(subcommand)]
        cmd: BatCmd,
    },
    /// BQ25896 raw ADC reads — port of `bq25896-raw.sh`
    Charger {
        #[command(subcommand)]
        cmd: ChgCmd,
    },
    /// Power management (battery + backlight + charge) — port of `power`
    Power {
        #[command(subcommand)]
        cmd: PwrCmd,
    },
    /// Battery safety guard daemon — port of `battery-guard.sh`
    Guard {
        #[command(subcommand)]
        cmd: GuardCmd,
    },
    /// A72 cluster (cpu8/cpu9) bring-up / power-down — ports of
    /// cl2-up.sh / cl2-down.sh
    A72 {
        #[command(subcommand)]
        cmd: A72Cmd,
    },
    /// Mali-T880 GPU power-on / status — port of gemini-gpu-poweron.sh
    Gpu {
        #[command(subcommand)]
        cmd: GpuCmd,
    },
    /// WDT EXRST self-boot reboot — port of gemini-wdt-reboot
    WdtReboot {
        /// WDT seconds (2..31, default 20)
        secs: Option<u32>,
    },
    /// Boot target via the para partition (recovery/debian/nixos) —
    /// ports of gemini-boot-recovery / gemini-boot-debian
    Boot {
        #[command(subcommand)]
        cmd: BootCmd,
    },
    /// Speaker amps (pads 243/244) via the gpio chardev — `speaker`
    Speaker {
        #[command(subcommand)]
        cmd: SpkCmd,
    },
    /// Clamshell sleep / wake — the silver side-button mechanism
    /// (backlight off, A53 cpus 1-7 offline, keyboard+touch inputs
    /// disabled, heavyweight services stopped; fully reversible). No
    /// kernel suspend is involved — see pkgs/gemcli/src/sleep.rs.
    Sleep {
        #[command(subcommand)]
        cmd: SleepCmd,
    },
    /// One-shot aggregate device status (best-effort)
    Status,
    /// Read-only parity harness (run on the device before any unit flip)
    Selfcheck,
    /// Version banner (version hygiene — rule 0)
    Version,
}

#[derive(Subcommand)]
enum BlCmd {
    /// Print brightness % (0-100)
    Get,
    /// Set brightness 0-100
    Set { pct: u8 },
    /// Alias for set 100
    Max,
    /// Alias for set 0
    Min,
    /// Disable the PWM entirely (saves power)
    Off,
    /// Re-enable at the last duty
    On,
    /// Registers + clock gate state
    Status,
    /// CON1 raw value (hex) — debugging
    Raw,
}

#[derive(Subcommand)]
enum BatCmd {
    /// key=value lines + script exit code
    Status,
}

#[derive(Subcommand)]
enum ChgCmd {
    /// One fresh raw sample line
    Raw {
        /// number of samples (default 1)
        samples: Option<usize>,
        /// interval between samples, seconds (default 0)
        interval: Option<f64>,
    },
}

#[derive(Subcommand)]
enum PwrCmd {
    /// Battery + backlight + charge direction
    Status,
    /// Poll every N s (default 10)
    Watch { secs: Option<u64> },
    /// Net charge current from the raw ADC
    Charge,
    /// Step backlight down until ICHGR >= target (default 100 mA) or
    /// min_pct (default 5) is reached
    DimToCharge {
        target_ma: Option<i64>,
        min_pct: Option<i64>,
    },
}

#[derive(Subcommand)]
enum GuardCmd {
    /// Run the guard loop in the foreground (daemon)
    Run,
    /// Print the daemon's /run/battery-guard/state
    Status,
}

#[derive(Subcommand)]
enum A72Cmd {
    /// Bring the A72 cluster up: cpu8 cold, then cpu9 warm (default
    /// both; `a72 up cpu9` = warm hotplug only)
    Up {
        #[arg(value_parser = ["cpu8", "cpu9", "both"], default_value = "both")]
        target: String,
    },
    /// Power the cluster down: cpu9 then cpu8 (default both). cpu8 =
    /// last-A72 secure teardown — WDT-guarded.
    Down {
        #[arg(value_parser = ["cpu9", "cpu8", "both"], default_value = "both")]
        target: String,
    },
    /// Online map + cluster power-control registers (read-only)
    Status,
}

#[derive(Subcommand)]
enum GpuCmd {
    /// Full vendor MFG MTCMOS power-on (idempotent)
    Poweron,
    /// Read-only power state
    Status,
}

#[derive(Subcommand)]
enum BootCmd {
    /// Show the current para marker
    Status,
    /// Write para=boot-recovery and reboot → TWRP on next power-on
    Recovery {
        /// Only write the marker, do not reboot
        #[arg(long)]
        no_reboot: bool,
    },
    /// Write para=boot-debian and reboot → Debian p29 on next power-on
    Debian {
        /// Only write the marker, do not reboot
        #[arg(long)]
        no_reboot: bool,
    },
    /// Clear para (NixOS p32 default) and reboot
    Nixos {
        /// Only write the marker, do not reboot
        #[arg(long)]
        no_reboot: bool,
    },
}

#[derive(Subcommand)]
enum SleepCmd {
    /// Enter the light sleep (idempotent)
    On,
    /// Wake from the light sleep (idempotent)
    Off,
    /// Current sleep state + what a toggle would touch
    Status,
    /// Watch the silver side button (KEY_SLEEP) and toggle sleep — the
    /// gemini-sleepd daemon entry (foreground, runs until killed)
    Key,
}

#[derive(Subcommand)]
enum SpkCmd {
    /// Enable the built-in speaker amps (pads 243/244 high)
    On,
    /// Disable them (3.5 mm jack only)
    Off,
    /// Live pad levels
    Status,
}

fn main() {
    // Default Rust ignores SIGPIPE, so a write to a closed pipe (e.g.
    // `gemcli … | head`) panics with a Broken-pipe backtrace instead of
    // dying quietly. Reset to the OS default: EPIPE then kills the
    // process silently (rc 141), which is right for both CLI pipes and
    // the sleepd daemon's journal stdout.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    let cli = Cli::parse();
    let rc = match cli.cmd {
        Cmd::Backlight { cmd } => backlight_cmd(cmd),
        Cmd::Battery { cmd } => battery_cmd(cmd),
        Cmd::Charger { cmd } => charger_cmd(cmd),
        Cmd::Power { cmd } => power_cmd(cmd),
        Cmd::Guard { cmd } => guard_cmd(cmd),
        Cmd::A72 { cmd } => a72_cmd(cmd),
        Cmd::Gpu { cmd } => gpu_cmd(cmd),
        Cmd::WdtReboot { secs } => wdt_cmd(secs),
        Cmd::Boot { cmd } => boot_cmd(cmd),
        Cmd::Speaker { cmd } => speaker_cmd(cmd),
        Cmd::Sleep { cmd } => match cmd {
            SleepCmd::On => sleep::on(),
            SleepCmd::Off => sleep::off(),
            SleepCmd::Status => sleep::status(),
            SleepCmd::Key => sleep::key(),
        },
        Cmd::Status => {
            status::status_cmd();
            0
        }
        Cmd::Selfcheck => status::selfcheck(),
        Cmd::Version => {
            println!("gemcli {}", env!("CARGO_PKG_VERSION"));
            println!("build: {BUILD_ID}");
            0
        }
    };
    std::process::exit(rc);
}

fn run(f: impl FnOnce() -> error::Res<()>) -> i32 {
    match f() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("gemcli: {}", e.msg);
            e.code
        }
    }
}

fn backlight_cmd(cmd: BlCmd) -> i32 {
    // backlight exit-code contract: 1 usage / 2 /dev/mem / 3 bad value.
    // Every devmem failure inside backlight.rs is already re-stamped 2.
    match cmd {
        BlCmd::Get => run(|| {
            println!("{}", backlight::get()?);
            Ok(())
        }),
        BlCmd::Set { pct } => run(|| backlight::set(pct as u32)),
        BlCmd::Max => run(|| backlight::set(100)),
        BlCmd::Min => run(|| backlight::set(0)),
        BlCmd::Off => run(backlight::off),
        BlCmd::On => run(backlight::on),
        BlCmd::Status => run(|| {
            print!("{}", backlight::status()?);
            Ok(())
        }),
        BlCmd::Raw => run(|| {
            println!("{}", backlight::raw()?);
            Ok(())
        }),
    }
}

fn battery_cmd(cmd: BatCmd) -> i32 {
    match cmd {
        BatCmd::Status => match battery::status_lines() {
            Ok((lines, rc)) => {
                for l in lines {
                    println!("{l}");
                }
                rc
            }
            Err(e) => {
                eprintln!("gemcli: {}", e.msg);
                e.code
            }
        },
    }
}

fn charger_cmd(cmd: ChgCmd) -> i32 {
    match cmd {
        ChgCmd::Raw { samples, interval } => {
            run(|| charger::raw_cmd(samples.unwrap_or(1), interval.unwrap_or(0.0)))
        }
    }
}

fn power_cmd(cmd: PwrCmd) -> i32 {
    match cmd {
        PwrCmd::Status => run(power::status),
        PwrCmd::Watch { secs } => run(|| {
            // watch never returns on its own (Ctrl-C / kill ends it)
            power::watch(secs.unwrap_or(10))
        }),
        PwrCmd::Charge => power::charge().map(|_| 0).unwrap_or_else(|e| {
            eprintln!("gemcli: {}", e.msg);
            1
        }),
        PwrCmd::DimToCharge { target_ma, min_pct } => {
            power::dim_to_charge(target_ma.unwrap_or(100), min_pct.unwrap_or(5))
        }
    }
}

fn guard_cmd(cmd: GuardCmd) -> i32 {
    match cmd {
        GuardCmd::Run => guard::run(),
        GuardCmd::Status => guard::status_cmd(),
    }
}

fn a72_cmd(cmd: A72Cmd) -> i32 {
    match cmd {
        A72Cmd::Up { target } => a72::up(&target),
        A72Cmd::Down { target } => a72::down(&target),
        A72Cmd::Status => run(|| {
            print!("{}", a72::status()?);
            Ok(())
        }),
    }
}

fn gpu_cmd(cmd: GpuCmd) -> i32 {
    match cmd {
        GpuCmd::Poweron => run(gpu::poweron),
        GpuCmd::Status => run(|| {
            print!("{}", gpu::status()?);
            Ok(())
        }),
    }
}

fn wdt_cmd(secs: Option<u32>) -> i32 {
    run(|| wdt::wdt_reboot(secs.unwrap_or(20)))
}

fn boot_cmd(cmd: BootCmd) -> i32 {
    match cmd {
        BootCmd::Status => run(|| {
            print!("{}", boot::status_text()?);
            Ok(())
        }),
        BootCmd::Recovery { no_reboot } => {
            run(|| boot_marker(b"boot-recovery", "TWRP (para=boot-recovery)", no_reboot))
        }
        BootCmd::Debian { no_reboot } => {
            run(|| boot_marker(b"boot-debian", "Debian p29 (para=boot-debian)", no_reboot))
        }
        BootCmd::Nixos { no_reboot } => {
            run(|| boot_marker(b"", "NixOS p32 (para cleared — the default)", no_reboot))
        }
    }
}

/// Write a para marker (32-byte padded) and, unless --no-reboot, reboot.
fn boot_marker(cmd: &[u8], what: &str, no_reboot: bool) -> error::Res<()> {
    let Some(p) = boot::find_para() else {
        return Err(error::cmsg("para partition not found"));
    };
    boot::write_cmd(&p, cmd)?;
    if cmd == b"boot-debian" {
        // the dual-boot initrd compares byte-exact (gemini-boot-debian
        // verify step)
        let (_p, buf) = boot::read_cmd()?;
        if !buf.starts_with(b"boot-debian") {
            return Err(error::cmsg("para write verify FAILED (boot-debian not read back)"));
        }
    }
    println!("gemcli boot: {what} ({})", p.display());
    if no_reboot {
        println!("gemcli boot: marker written (--no-reboot) — next power-on uses it");
        return Ok(());
    }
    println!("gemcli boot: rebooting; para is sticky — the next power-on boots the target");
    boot::reboot_system()?;
    Ok(())
}

fn speaker_cmd(cmd: SpkCmd) -> i32 {
    match cmd {
        SpkCmd::On => run(speaker::on),
        SpkCmd::Off => run(speaker::off),
        SpkCmd::Status => run(|| {
            println!("{}", speaker::status()?);
            Ok(())
        }),
    }
}

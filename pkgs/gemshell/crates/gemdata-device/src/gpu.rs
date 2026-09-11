//! Mali-T880 GPU power-on / status — port of
//! services/scripts/gemini-gpu-poweron.sh.
//!
//! Runs the EXACT vendor power-on sequences from the BSP kernel
//! (gemini-linux-kernel-3.18/drivers/clk/mediatek/clk-mt6797-pg.c,
//! spm_mtcmos_ctrl_mfg_async / _mfg / _mfg_core0-3) against the live
//! SPM registers, then opens the mfg_bg3d clock gate.
//!
//! Registers (SPM base 0x10006000; clk-mt6797-pg.c is authoritative
//! over mt_spm.h):
//!   MFG_ASYNC  0x334  status BIT(13)
//!   MFG        0x338  status BIT(12)
//!   MFG_SRAM   0x33C  SRAM_PDN bits[1:0], ACK bits[17:16]; CORE0-3
//!                     SRAM ACK bits[23:20]
//!   MFG_CORE0  0x340  BIT(11), SRAM_PDN bit 8        (…CORE3 0x34C BIT(8))
//!   PWR_STATUS 0x180 / PWR_STATUS_2ND 0x184
//!   POWERON_CONFIG_EN 0x000 (write 1 = SPM register control on)
//! PWR bits: PWR_RST_B=1<<0 PWR_ISO=1<<1 PWR_ON=1<<2 PWR_ON_2ND=1<<3
//!           PWR_CLK_DIS=1<<4
//!
//! [corrected 2026-08-31] THE GPU SRAM LDO (DVDD_SRAM_GPU) IS PART OF
//! THE POWER-ON — with it off the GPU faults GPU_SHAREABILITY_FAULT on
//! the first job (root-caused + A/B-verified). The vendor kbase
//! power-on writes the infracfg_ao GPU-LDO bank 0x10001fbc (VGPU_SRAM
//! enable) + 0xfc0/0xfc4/0xfc8/0xfd8/0xfe4 (voltage config), the MFG
//! timing register 0x1300001c and the MFG PMU enable 0x130003e0-0x3f0.
//!
//! Idempotent (vendor sequence is read-modify-write). Exit 0 = all
//! domains ON + clock gate open.

use crate::devmem;
use crate::error::{cmsg, Res};
use crate::i2c;
use crate::util;

const SRAM: u64 = 0x1000633c;
const MFGCFG_CLR: u64 = 0x13000008;
const VGPU_BUS_BASE: &str = "11010000"; // i2c7-hw (RT5735 @0x1c)

fn rd(a: u64) -> Res<u32> {
    devmem::rd32(a)
}
fn wr(a: u64, v: u32) -> Res<()> {
    devmem::wr32(a, v)
}
fn setb(a: u64, bits: u32) -> Res<()> {
    let v = rd(a)?;
    wr(a, v | bits)
}
fn clrb(a: u64, bits: u32) -> Res<()> {
    let v = rd(a)?;
    wr(a, v & !bits)
}

fn ok(msg: &str) {
    println!("GPU-POWERON: {msg}");
}

// wait until `mask` is set in BOTH status regs
fn wait_sta(mask: u32, tries: u32) -> bool {
    for _ in 0..tries {
        if let (Ok(a), Ok(b)) = (rd(0x10006180), rd(0x10006184)) {
            if a & mask == mask && b & mask == mask {
                return true;
            }
        }
        util::sleep(0.02);
    }
    false
}

// wait until `mask` of `reg` is CLEARED
fn wait_clr(mask: u32, reg: u64, tries: u32) -> bool {
    for _ in 0..tries {
        if let Ok(v) = rd(reg) {
            if v & mask == 0 {
                return true;
            }
        }
        util::sleep(0.02);
    }
    false
}

/// Power on one domain (vendor sequence). ctl = ctl reg, sta = status
/// bit mask, ack = SRAM ACK bit mask (0 = none).
fn power_on(ctl: u64, sta: u32, ack: u32, what: &str) -> Res<()> {
    setb(ctl, 0x4 | 0x8).map_err(|e| cmsg(format!("FAILED at set PWR_ON for {what}: {}", e.msg)))?;
    if !wait_sta(sta, 250) {
        return Err(cmsg(format!("FAILED at status BIT({sta:#x}) for {ctl:#x}")));
    }
    clrb(ctl, 0x10).map_err(|e| cmsg(format!("FAILED at PWR_CLK_DIS {what}: {}", e.msg)))?; // PWR_CLK_DIS
    clrb(ctl, 0x2).map_err(|e| cmsg(format!("FAILED at PWR_ISO {what}: {}", e.msg)))?; // PWR_ISO
    setb(ctl, 0x1).map_err(|e| cmsg(format!("FAILED at PWR_RST_B {what}: {}", e.msg)))?; // PWR_RST_B
    if ack != 0 {
        clrb(ctl, 0x100).map_err(|e| cmsg(format!("FAILED at SRAM_PDN {what}: {}", e.msg)))?; // SRAM_PDN (bit 8)
        if !wait_clr(ack, SRAM, 250) {
            return Err(cmsg(format!("FAILED at sram ack BIT({ack:#x}) for {ctl:#x}")));
        }
    }
    Ok(())
}

/// MFG (SRAM_PDN lives in the SRAM reg, bits[1:0]/ACK[17:16]).
fn power_on_mfg() -> Res<()> {
    let ctl = 0x10006338;
    setb(ctl, 0x4 | 0x8).map_err(|e| cmsg(format!("FAILED at set PWR_ON MFG: {}", e.msg)))?;
    if !wait_sta(1 << 12, 250) {
        return Err(cmsg("FAILED at status BIT(12) MFG"));
    }
    clrb(ctl, 0x10).map_err(|e| cmsg(format!("FAILED at PWR_CLK_DIS MFG: {}", e.msg)))?;
    clrb(ctl, 0x2).map_err(|e| cmsg(format!("FAILED at PWR_ISO MFG: {}", e.msg)))?;
    setb(ctl, 0x1).map_err(|e| cmsg(format!("FAILED at PWR_RST_B MFG: {}", e.msg)))?;
    clrb(SRAM, 0x3).map_err(|e| cmsg(format!("FAILED at MFG SRAM_PDN: {}", e.msg)))?;
    if !wait_clr(0x3 << 16, SRAM, 250) {
        return Err(cmsg("FAILED at sram ack MFG"));
    }
    Ok(())
}

/// `gpu poweron` — full vendor sequence (idempotent).
pub fn poweron() -> Res<()> {
    // VGPU rail first: the Richtek RT5735 powers the GPU (vendor
    // VGPU_SET_BY_EXTIC). At stock boot both VSEL EN bits (0x80) are
    // clear — VGPU is OFF — and the MFG MTCMOS domains cannot power on
    // without it (proven 2026-08-31). VSEL0=0x33 (918 mV), VSEL1=0x50
    // (1.1 V) are the preloader boot states; we just enable both.
    let bus = i2c::adapter_for(VGPU_BUS_BASE)?;
    let mut vgpu = false;
    if let Some(bus) = bus {
        if i2c::wr_reg_ok(bus, 0x1c, 0x11, 0xb3, false)
            && i2c::wr_reg_ok(bus, 0x1c, 0x10, 0xd0, false)
        {
            vgpu = true;
        }
    }
    if vgpu {
        let vs = i2c::rd_reg(bus.unwrap(), 0x1c, 0x11, false)
            .map(|v| format!("{v:#x}"))
            .unwrap_or_else(|_| "?".into());
        ok(&format!("VGPU rail enabled (RT5735 VSEL1={vs})"));
    } else {
        println!("GPU-POWERON: WARNING: VGPU enable failed (bus={:?})", bus.map(|b| format!("i2c-{b}")).unwrap_or_else(|| "not probed".into()));
    }
    util::sleep(0.2);

    wr(0x10006000, 1).map_err(|e| cmsg(format!("FAILED at POWERON_CONFIG_EN: {}", e.msg)))?; // POWERON_CONFIG_EN = 1
    ok("POWERON_CONFIG_EN set");

    // VGPU SRAM LDO + voltage config (vendor mtk_platform_init +
    // mt_gpufreq_ext_ic_init; infracfg_ao base 0x10001000). Without
    // these the GPU faults GPU_SHAREABILITY_FAULT on the first job
    // (proven 2026-08-31).
    wr(0x10001fc0, 0x0f0f0f0f).map_err(|e| cmsg(format!("FAILED at SRAM LDO cfg: {}", e.msg)))?;
    wr(0x10001fc4, 0x0f0f0f0f).map_err(|e| cmsg(format!("FAILED at SRAM LDO cfg: {}", e.msg)))?;
    wr(0x10001fc8, 0x0f).map_err(|e| cmsg(format!("FAILED at SRAM LDO cfg: {}", e.msg)))?;
    wr(0x10001fd8, 0x88888888).map_err(|e| cmsg(format!("FAILED at SRAM LDO cfg: {}", e.msg)))?; // RG_GPULDO_RSV_H_0-8
    wr(0x10001fe4, 0x00000008).map_err(|e| cmsg(format!("FAILED at SRAM LDO cfg: {}", e.msg)))?;
    wr(0x10001fbc, 0x1ff).map_err(|e| cmsg(format!("FAILED at VGPU_SRAM LDO enable: {}", e.msg)))?;
    let fbc = rd(0x10001fbc).map(|v| format!("{v:#x}")).unwrap_or_else(|_| "?".into());
    ok(&format!("VGPU SRAM LDO enabled + voltage config (0x10001fbc={fbc})"));

    // MFG timing + PMU enable (vendor mtk_pm_callback_power_on)
    setb(0x1300001c, 0x5).map_err(|e| cmsg(format!("FAILED at MFG timing: {}", e.msg)))?;
    for a in [0x130003e0u64, 0x130003e4, 0x130003e8, 0x130003ec, 0x130003f0] {
        wr(a, 0xffffffff).map_err(|e| cmsg(format!("FAILED at MFG PMU enable: {}", e.msg)))?;
    }
    let t1c = rd(0x1300001c).map(|v| format!("{v:#x}")).unwrap_or_else(|_| "?".into());
    let t3e0 = rd(0x130003e0).map(|v| format!("{v:#x}")).unwrap_or_else(|_| "?".into());
    ok(&format!("MFG timing + PMU enabled (0x1300001c={t1c} 0x130003e0={t3e0})"));

    // MFG bus protection release (vendor spm_topaxi_protect(MFG_PROT_MASK, 0);
    // MFG_PROT_MASK = bit 21 per clk-mt6797-pg.h)
    clrb(0x10001220, 0x200000).map_err(|e| cmsg(format!("FAILED at topaxi release: {}", e.msg)))?;
    ok("MFG topaxi protection released");

    let a = rd(0x10006180).map(|v| format!("{v:#x}")).unwrap_or_else(|_| "?".into());
    let b = rd(0x10006184).map(|v| format!("{v:#x}")).unwrap_or_else(|_| "?".into());
    println!("GPU-POWERON: status before: 0x180={a} 0x184={b}");

    power_on(0x10006334, 1 << 13, 0, "MFG_ASYNC")?;
    ok("MFG_ASYNC ON");
    power_on_mfg()?;
    ok("MFG ON");
    power_on(0x10006340, 1 << 11, 1 << 20, "MFG_CORE0")?;
    ok("MFG_CORE0 ON");
    power_on(0x10006344, 1 << 10, 1 << 21, "MFG_CORE1")?;
    ok("MFG_CORE1 ON");
    power_on(0x10006348, 1 << 9, 1 << 22, "MFG_CORE2")?;
    ok("MFG_CORE2 ON");
    power_on(0x1000634c, 1 << 8, 1 << 23, "MFG_CORE3")?;
    ok("MFG_CORE3 ON");

    wr(MFGCFG_CLR, 1).map_err(|e| cmsg(format!("FAILED at BG3D gate: {}", e.msg)))?; // open BG3D clock gate
    let g = rd(0x13000000).map(|v| format!("{v:#x}")).unwrap_or_else(|_| "?".into());
    ok(&format!("mfg_bg3d gate opened (0x13000000={g})"));

    let a = rd(0x10006180).map(|v| format!("{v:#x}")).unwrap_or_else(|_| "?".into());
    let b = rd(0x10006184).map(|v| format!("{v:#x}")).unwrap_or_else(|_| "?".into());
    println!("GPU-POWERON: status after:  0x180={a} 0x184={b}");
    for r in [0x10006334u64, 0x10006338, 0x1000633c, 0x10006340, 0x10006344, 0x10006348, 0x1000634c] {
        let v = rd(r).map(|x| format!("{x:#x}")).unwrap_or_else(|_| "?".into());
        println!("GPU-POWERON: {r:#x} = {v}");
    }

    // final check: all six status bits in both regs
    let st: u32 = 0x2B00; // bits 8..13 = CORE3..ASYNC
    let a = rd(0x10006180)?;
    let b = rd(0x10006184)?;
    if a & st == st && b & st == st {
        ok("ALL MFG DOMAINS POWERED");
        Ok(())
    } else {
        Err(cmsg(format!("FAILED: status bits missing: 0x180={a:#x} 0x184={b:#x}")))
    }
}

/// `gpu status` — read-only snapshot of the MFG power state.
pub fn status() -> Res<String> {
    let a = rd(0x10006180).map_err(|e| cmsg(e.msg))?;
    let b = rd(0x10006184).map_err(|e| cmsg(e.msg))?;
    let cfg = rd(0x10006000).map(|v| format!("{v:#x}")).unwrap_or_else(|_| "?".into());
    let gate = rd(0x13000000).map(|v| format!("{v:#x}")).unwrap_or_else(|_| "?".into());
    let fbc = rd(0x10001fbc).map(|v| format!("{v:#x}")).unwrap_or_else(|_| "?".into());
    let st: u32 = 0x2B00;
    let powered = a & st == st && b & st == st;
    let mut s = String::new();
    s.push_str(&format!("PWR_STATUS 0x180={a:#x} 0x184={b:#x}\n"));
    s.push_str(&format!("POWERON_CONFIG_EN 0x10006000={cfg}  BG3D gate 0x13000000={gate}  VGPU_SRAM 0x10001fbc={fbc}\n"));
    s.push_str(&format!("verdict: {}", if powered { "ALL MFG DOMAINS POWERED" } else { "GPU OFF/partial — run 'gemcli gpu poweron'" }));
    Ok(s)
}

//! Display backlight control — port of services/scripts/backlight.
//!
//! Controls DISP_PWM0 @ 0x1100f000 (the LCD LED-boost PWM). On kernels
//! with CONFIG_PWM_MTK_DISP the /sys/class/backlight path exists and is
//! preferred (get/set/max/min/off/on); otherwise — and always for
//! status/raw — the tool drives the hardware directly via /dev/mem with
//! the exact LK sequence (repos/gemini-lk/lk/platform/mt6797/ddp_pwm.c
//! disp_pwm_set_backlight).
//!
//! Registers (mt8173-compatible layout, verified live 2026-09-01):
//!   EN     0x00  bit0  PWM enable
//!   COMMIT 0x08  bit0  double-buffer commit
//!   CON0   0x10  CLKDIV [26:16]  (we keep LK's 0)
//!   CON1   0x14  PERIOD [11:0] = 0x3ff (1024 steps), DUTY [30:16]
//!   duty 0..1023, duty=1023 -> 100 %
//!
//! Clocks (MTK gates are ACTIVE-LOW — a set bit means DISABLED; the
//! "phantom block" 2026-09-01 session was a wrongly-set gate):
//!   pwm_sel        topckgen 0x10000050 bit7 (mux bits 0-2 = clk26m)
//!   infra_disp_pwm infracfg ICG1: set 0x10001088 / clr 0x1000108c /
//!                  sta 0x10001094, bit17
//! Both are normally left enabled by LK + clk_ignore_unused; if a gate
//! is found disabled this tool re-enables it (preserving other bits).
//!
//! Exit codes (script contract): 0 ok, 1 usage, 2 cannot access
//! /dev/mem, 3 bad value. Every devmem failure is re-stamped to 2.

use std::fmt::Write as _;
use std::path::PathBuf;

use crate::devmem;
use crate::error::{cmsg, cerr, Cerr, Res};
use crate::util;

const BASE: u64 = 0x1100f000;
const REG_EN: u64 = BASE + 0x00;
const REG_COMMIT: u64 = BASE + 0x08;
const REG_CON1: u64 = BASE + 0x14;

const TOP_PWM_SEL: u64 = 0x10000050; // pwm_sel mux+gate; bit7 = gate (SET=disabled)
const ICG1_CLR: u64 = 0x1000108c; // infra_disp_pwm enable: write bit17 here
const ICG1_STA: u64 = 0x10001094; // gate state; bit17 SET = disabled

fn mem2(e: Cerr) -> Cerr {
    e.recode(2)
}

fn mem_err(what: impl Into<String>) -> Cerr {
    cerr(2, what)
}

/// Make sure both PWM clocks are running (idempotent, script parity).
fn ensure_clocks() -> Res<()> {
    let v = devmem::rd32(TOP_PWM_SEL).map_err(mem2)?;
    if v & 0x80 != 0 {
        devmem::wr32(TOP_PWM_SEL, v & !0x80).map_err(mem2)?;
    }
    let v = devmem::rd32(ICG1_STA).map_err(mem2)?;
    if v & 0x20000 != 0 {
        devmem::wr32(ICG1_CLR, 0x00020000).map_err(mem2)?;
    }
    Ok(())
}

/// CON1 value -> brightness % (0-100); duty masked [16..28] then capped
/// at 1023 (exact script arithmetic).
pub fn con1_to_pct(con1: u32) -> u32 {
    let duty = (con1 >> 16) & 0x1fff;
    let duty = duty.min(1023);
    duty * 100 / 1023
}

/// LK write sequence: COMMIT=0, CON1, EN, COMMIT=1, COMMIT=0.
fn write_duty(duty: u32) -> Res<()> {
    devmem::wr32(REG_COMMIT, 0).map_err(mem2)?;
    devmem::wr32(REG_CON1, (duty << 16) | 0x3ff).map_err(mem2)?;
    devmem::wr32(REG_EN, if duty > 0 { 1 } else { 0 }).map_err(mem2)?;
    devmem::wr32(REG_COMMIT, 1).map_err(mem2)?;
    devmem::wr32(REG_COMMIT, 0).map_err(mem2)?;
    Ok(())
}

fn sysfs_max_brightness(d: &PathBuf) -> Res<u32> {
    let s = util::read_str(&d.join("max_brightness").to_string_lossy())
        .map_err(|e| Cerr::new(1, format!("{}", e.msg)))?;
    s.parse::<u32>().map_err(|_| cmsg("bad max_brightness"))
}

/// `backlight get` — brightness % (0-100).
pub fn get() -> Res<u32> {
    if let Some(d) = crate::sysfs::first_backlight() {
        let max = sysfs_max_brightness(&d)?;
        let cur: u32 = util::read_str(&d.join("brightness").to_string_lossy())
            .map_err(|e| Cerr::new(1, e.msg.clone()))?
            .parse()
            .map_err(|_| cmsg("bad brightness"))?;
        if max == 0 {
            return Ok(0);
        }
        return Ok(cur * 100 / max);
    }
    ensure_clocks()?;
    let en = devmem::rd32(REG_EN).map_err(mem2)?;
    if en & 1 == 0 {
        return Ok(0); // PWM disabled (EN=0): effective brightness 0
    }
    let v = devmem::rd32(REG_CON1).map_err(mem2)?;
    Ok(con1_to_pct(v))
}

/// `backlight set <0-100>`.
pub fn set(pct: u32) -> Res<()> {
    if pct > 100 {
        return Err(cerr(3, "brightness must be 0-100"));
    }
    if let Some(d) = crate::sysfs::first_backlight() {
        let max = sysfs_max_brightness(&d)?;
        let val = pct * max / 100;
        util::write_str(&d.join("brightness").to_string_lossy(), &val.to_string())
            .map_err(|e| Cerr::new(1, e.msg))
    } else {
        ensure_clocks()?;
        let duty = pct * 1023 / 100;
        write_duty(duty).map_err(|e| mem_err(format!("write failed: {}", e.msg)))
    }
}

/// `backlight off` — disable the PWM entirely (EN=0 / bl_power=4).
pub fn off() -> Res<()> {
    if let Some(d) = crate::sysfs::first_backlight() {
        // bl_power=4 (FB_BLANK_POWERDOWN): PWM off, brightness kept
        util::write_str(&d.join("bl_power").to_string_lossy(), "4")
            .map_err(|e| Cerr::new(1, e.msg))
    } else {
        ensure_clocks()?;
        devmem::wr32(REG_EN, 0).map_err(|e| mem_err(format!("write failed: {}", e.msg)))
    }
}

/// `backlight on` — re-enable at the last duty.
pub fn on() -> Res<()> {
    if let Some(d) = crate::sysfs::first_backlight() {
        util::write_str(&d.join("bl_power").to_string_lossy(), "0")
            .map_err(|e| Cerr::new(1, e.msg))
    } else {
        ensure_clocks()?;
        devmem::wr32(REG_EN, 1).map_err(|e| mem_err(format!("write failed: {}", e.msg)))
    }
}

/// `backlight raw` — CON1 value as hex (debugging; always devmem).
pub fn raw() -> Res<String> {
    ensure_clocks()?;
    let v = devmem::rd32(REG_CON1).map_err(mem2)?;
    Ok(format!("{v:#x}"))
}

/// `backlight status` — registers + clock gate state (always devmem,
/// mirroring the script).
pub fn status() -> Res<String> {
    ensure_clocks()?;
    let en = devmem::rd32(REG_EN).map_err(mem2)?;
    let commit = devmem::rd32(REG_COMMIT).map_err(mem2)?;
    let con1 = devmem::rd32(REG_CON1).map_err(mem2)?;
    let top = devmem::rd32(TOP_PWM_SEL).map_err(mem2)?;
    let icg = devmem::rd32(ICG1_STA).map_err(mem2)?;
    let mut s = String::new();
    let _ = writeln!(s, "DISP_PWM0 @ {BASE:#x}");
    let _ = writeln!(s, "  EN     = {en:#x}");
    let _ = writeln!(s, "  COMMIT = {commit:#x}");
    let _ = writeln!(
        s,
        "  CON1   = {con1:#x}  (duty={}/1023 = {}%)",
        (con1 >> 16) & 0x1fff,
        con1_to_pct(con1)
    );
    let _ = writeln!(s, "clocks:");
    let _ = writeln!(s, "  pwm_sel        0x{top:08x}  (bit7=0 enabled)");
    let _ = writeln!(s, "  infra_disp_pwm 0x{icg:08x}  (bit17=0 enabled)");
    Ok(s)
}

#[cfg(test)]
mod tests {
    #[test]
    fn con1_pct() {
        // duty 1023 (= CON1 bits[28:16] all set) -> 100 %
        assert_eq!(super::con1_to_pct(0x3ff << 16), 100);
        // duty 511 -> 49 % (integer math like the script: 511*100/1023)
        assert_eq!(super::con1_to_pct((0x3ff >> 1) << 16), 49);
        // duty 0 -> 0 %
        assert_eq!(super::con1_to_pct(0), 0);
        // duty capped at 1023 even if higher bits are set
        assert_eq!(super::con1_to_pct(0x7fff << 16), 100);
    }
}

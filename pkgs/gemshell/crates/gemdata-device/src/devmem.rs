//! /dev/mem 32-bit register access — the busybox-devmem equivalent the
//! scripts use (backlight, cl2-up/down, gpu-poweron, wdt).
//!
//! Kernel: CONFIG_DEVMEM=y + CONFIG_STRICT_DEVMEM=y but
//! CONFIG_IO_STRICT_DEVMEM not set and GENERIC_LIB_DEVMEM_IS_ALLOWED=y
//! (devices/planet-geminipda/kernel/config) — the SPM/topckgen/
//! infracfg/DISP_PWM0 ranges have been mmap'd from userspace on glass
//! since 2026-09-01. One 4 KiB page is mapped per access, exactly like
//! busybox devmem (open, mmap page-aligned, R/W volatile u32 — all our
//! targets are little-endian), then munmap + close.
//!
//! Errors carry code 1; callers that define a distinct code (backlight's
//! "cannot access /dev/mem" = 2) re-stamp with Cerr::recode.

use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;

use crate::error::{cmsg, Res};

const PAGE: u64 = 4096;
const DEVMEM: &str = "/dev/mem";

fn map(phys: u64) -> Res<*mut u8> {
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .open(DEVMEM)
        .map_err(|e| cmsg(format!("cannot access {DEVMEM}: {e}")))?;
    let fd = f.as_raw_fd();
    // SAFETY: standard mmap; MAP_SHARED so the kernel page tables reflect
    // device-side changes (the scripts rely on shared semantics too).
    let p = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            PAGE as usize,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd,
            phys as libc::off_t,
        )
    };
    // mmap returns MAP_FAILED = -1 on error.
    let _ = f; // closed on drop
    if p as isize == -1 {
        return Err(cmsg(format!("mmap {phys:#x} failed (errno {})", std::io::Error::last_os_error())));
    }
    Ok(p as *mut u8)
}

fn unmap(p: *mut u8) {
    // SAFETY: p came from map() above with PAGE length.
    unsafe { libc::munmap(p as *mut libc::c_void, PAGE as usize) };
}

/// Read a 32-bit register at a physical address.
pub fn rd32(addr: u64) -> Res<u32> {
    let page = addr & !(PAGE - 1);
    let off = (addr & (PAGE - 1)) as usize;
    let p = map(page)?;
    // SAFETY: p..p+PAGE is our mapping; off is 4-aligned for every
    // register we touch.
    let v = unsafe { (p.add(off) as *const u32).read_volatile() };
    unmap(p);
    Ok(v)
}

/// Write a 32-bit register at a physical address.
pub fn wr32(addr: u64, v: u32) -> Res<()> {
    let page = addr & !(PAGE - 1);
    let off = (addr & (PAGE - 1)) as usize;
    let p = map(page)?;
    // SAFETY: see rd32.
    unsafe { (p.add(off) as *mut u32).write_volatile(v) };
    unmap(p);
    Ok(())
}

/// Read-modify-write: set `bits`, clear `clr` (non-atomic; matches the
/// bash `v=$(dm $r); dm $r $((v op bits))` pattern — no contention on
/// any register we touch).
pub fn rmw(addr: u64, set: u32, clr: u32) -> Res<()> {
    let v = rd32(addr)?;
    wr32(addr, (v | set) & !clr)
}

pub fn rmw_set(addr: u64, bits: u32) -> Res<()> {
    rmw(addr, bits, 0)
}

pub fn rmw_clr(addr: u64, bits: u32) -> Res<()> {
    rmw(addr, 0, bits)
}

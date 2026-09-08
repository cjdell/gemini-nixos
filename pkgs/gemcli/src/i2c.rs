//! i2c-dev access — the Rust equivalent of i2cget/i2cset (i2c-tools).
//!
//! Used by: charger.rs (BQ25896 on i2c0 @0x6b, `-f` force — the kernel
//! bq25890 driver claims the address), a72.rs (DA9214 BUCKB on the
//! i2c6-hw bus 0x1100e000 @0x68) and gpu.rs (RT5735 VGPU rail on the
//! i2c7-hw bus 0x11010000 @0x1c).
//!
//! The kernel's i2c-dev interface needs the adapter number, but adapter
//! NUMBERS SHIFT when i2c nodes are enabled ([corrected 2026-09-04] in
//! gemini-gpu-poweron.sh: enabling i2c6 moved the RT5735 from i2c-2 to
//! i2c-3). Like the scripts, resolve the adapter by its controller base
//! from /sys/class/i2c-adapter/i2c-*/of_node/reg (the DT `reg`
//! property's last 4 bytes, big-endian hex).

use std::fs;
use std::os::unix::io::AsRawFd;

use crate::error::{cmsg, cerr, Res};

// linux/i2c-dev.h ioctl numbers (raw constants, not _IOC-encoded).
const I2C_SLAVE: libc::c_ulong = 0x0703;
const I2C_SLAVE_FORCE: libc::c_ulong = 0x0706;

/// Find the i2c adapter whose controller DT base equals `ctrl_hex`
/// (e.g. "1100e000"). Ok(None) = controller not probed yet (the DA9214
/// bus can probe late — a72-up resolves per attempt for this reason).
pub fn adapter_for(ctrl_hex: &str) -> Res<Option<i32>> {
    let want = ctrl_hex.trim_start_matches("0x").to_ascii_lowercase();
    let base = "/sys/class/i2c-adapter";
    let dir = fs::read_dir(base)
        .map_err(|e| cmsg(format!("cannot list {base}: {e}")))?;
    for ent in dir.flatten() {
        let name = ent.file_name().to_string_lossy().to_string();
        let Some(num) = name.strip_prefix("i2c-") else { continue };
        let Ok(num) = num.parse::<i32>() else { continue };
        let reg = ent.path().join("of_node/reg");
        let Ok(bytes) = fs::read(&reg) else { continue };
        // The script: od -An -N8 -tx1 then compare ${h:8:8} — i.e. the
        // last 4 of the first 8 bytes, hex, big-endian (DT cells).
        if bytes.len() >= 8 {
            let hex: String = bytes[4..8].iter().map(|b| format!("{b:02x}")).collect();
            if hex == want {
                return Ok(Some(num));
            }
        }
    }
    Ok(None)
}

/// Open /dev/i2c-{bus} and claim `addr`. Tries plain I2C_SLAVE first;
/// if the address is claimed by a kernel driver (EBUSY) and `force` is
/// set, falls back to I2C_SLAVE_FORCE — the exact semantics of
/// `i2cget/i2cset -f` (charger raw reads) vs plain `-y` (DA9214/RT5735).
fn open_slave(bus: i32, addr: u8, force: bool) -> Res<std::fs::File> {
    let path = format!("/dev/i2c-{bus}");
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| cmsg(format!("cannot open {path}: {e}")))?;
    let fd = f.as_raw_fd();
    // SAFETY: ioctl with a scalar argument.
    let r = unsafe { libc::ioctl(fd, I2C_SLAVE, addr as libc::c_ulong) };
    if r < 0 {
        let e = std::io::Error::last_os_error();
        if force && e.raw_os_error() == Some(libc::EBUSY) {
            // SAFETY: see above.
            let r2 = unsafe { libc::ioctl(fd, I2C_SLAVE_FORCE, addr as libc::c_ulong) };
            if r2 < 0 {
                return Err(cmsg(format!("i2c-{bus} addr {addr:#x}: force claim failed: {}", std::io::Error::last_os_error())));
            }
        } else {
            return Err(cmsg(format!("i2c-{bus} addr {addr:#x}: {e}")));
        }
    }
    Ok(f)
}

/// Write one register (2 bytes: reg, value) — `i2cset -[f]y BUS ADDR REG VAL`.
pub fn wr_reg(bus: i32, addr: u8, reg: u8, val: u8, force: bool) -> Res<()> {
    let f = open_slave(bus, addr, force)?;
    let buf = [reg, val];
    // SAFETY: fixed 2-byte buffer.
    let n = unsafe { libc::write(f.as_raw_fd(), buf.as_ptr() as *const libc::c_void, 2) };
    if n != 2 {
        return Err(cmsg(format!("i2c-{bus} addr {addr:#x} reg {reg:#x}: short write ({n})")));
    }
    Ok(())
}

/// Best-effort register write used where the scripts treat "i2cset
/// returned 0 (ACK)" as the trusted signal and retry on failure — the
/// DA9214 bus's READS are unreliable (SCP/DVFSP shares i2c6), so the
/// scripts never verify by reading back. Returns false instead of
/// erroring so callers can retry with backoff.
pub fn wr_reg_ok(bus: i32, addr: u8, reg: u8, val: u8, force: bool) -> bool {
    wr_reg(bus, addr, reg, val, force).is_ok()
}

/// Read one register — `i2cget -[f]y BUS ADDR REG`.
pub fn rd_reg(bus: i32, addr: u8, reg: u8, force: bool) -> Res<u8> {
    let f = open_slave(bus, addr, force)?;
    let fd = f.as_raw_fd();
    // SAFETY: one-byte write (register pointer), then one-byte read.
    let n = unsafe { libc::write(fd, &reg as *const u8 as *const libc::c_void, 1) };
    if n != 1 {
        return Err(cmsg(format!("i2c-{bus} addr {addr:#x} reg {reg:#x}: short write ({n})")));
    }
    let mut out = [0u8; 1];
    let n = unsafe { libc::read(fd, out.as_mut_ptr() as *mut libc::c_void, 1) };
    if n != 1 {
        return Err(cerr(1, "i2c read failed"));
    }
    Ok(out[0])
}

//! Kernel gpio chardev access (the v1 linehandle API) — the Rust
//! equivalent of gpioout.c (pkgs/speaker-amp): request SoC pads 243/244
//! (the Gemini speaker-amp enables) as outputs and drive them.
//!
//! gpiolib owns the pads (proper request/release — no raw /dev/mem
//! pinctrl writes, unlike the spkamp diagnostic tool). Lines stay at
//! their last value after the handle closes (gpiolib does not reset
//! outputs on release), which the `speaker` CLI relies on.
//!
//! ioctl numbers are the asm-generic _IOC encoding (aarch64):
//!   _IOC(dir,type,nr,size) = dir<<30 | type<<8 | nr | size<<16
//!   dir: _IOC_WRITE=1 _IOC_READ=2 (handle/data ioctls are _IOWR = 3)
//! struct sizes are computed with mem::size_of so the encoding cannot
//! drift from the layout (asserted in tests below).
//!
//! [fixed 2026-09-08, on glass] the first version opened the chip
//! O_RDWR and always used /dev/gpiochip0 — the request failed EINVAL
//! while the C gpioout (O_RDONLY open, same ioctls) succeeded. Match
//! gpioout exactly (O_RDONLY chip open — values go through the handle
//! fd, never chip-fd writes) and resolve the chip by asking each
//! /dev/gpiochipN for its line count (GPIO_GET_CHIPINFO_IOCTL) instead
//! of assuming chip0 (chip indexes follow probe order and are not
//! guaranteed).

use std::os::unix::io::AsRawFd;

use crate::error::{cmsg, Res};

const GPIO_TYPE: u32 = 0xB4;
// v1 ioctl numbers per the v6.6 UAPI (include/uapi/linux/gpio.h) — the
// numbering was REORGANISED after the pre-5.x kernels (linehandle moved
// 0x02 -> 0x03 when lineinfo took 0x02); the regression test below pins
// the exact _IOC literals.
const GPIO_GET_CHIPINFO_IOCTL_NR: u32 = 0x01;
const GPIO_GET_LINEHANDLE_IOCTL_NR: u32 = 0x03;
const GPIOHANDLE_GET_LINE_VALUES_IOCTL_NR: u32 = 0x08;
const GPIOHANDLE_SET_LINE_VALUES_IOCTL_NR: u32 = 0x09;

const GPIOHANDLE_REQUEST_INPUT: u32 = 1;
const GPIOHANDLE_REQUEST_OUTPUT: u32 = 2;

const MAX_LINES: usize = 64;

#[repr(C)]
struct GpioHandleRequest {
    lineoffsets: [u32; MAX_LINES],
    flags: u32,
    default_values: [u8; MAX_LINES],
    consumer_label: [i8; 32],
    lines: u32,
    fd: libc::c_int,
}

#[repr(C)]
struct GpioHandleData {
    values: [u8; MAX_LINES],
}

// linux/gpio.h struct gpiochip_info: char name[32]; char label[32];
// __u32 lines;
#[repr(C)]
struct GpioChipInfo {
    name: [i8; 32],
    label: [i8; 32],
    lines: u32,
}

#[cfg(test)]
mod tests {
    #[test]
    fn ioctl_struct_sizes_match_kernel_uapi() {
        // kernel: 64*4 + 4 + 64 + 32 + 4 + 4 = 364
        assert_eq!(std::mem::size_of::<super::GpioHandleRequest>(), 364);
        assert_eq!(std::mem::size_of::<super::GpioHandleData>(), 64);
        assert_eq!(std::mem::size_of::<super::GpioChipInfo>(), 68);
    }

    #[test]
    fn ioctl_numbers_match_v66_uapi() {
        // _IOC(3,0xB4,nr,size) literals computed from the v6.6 header
        // definitions (the linehandle nr 0x03 is the one that bit us on
        // glass 2026-09-08 — pre-5.x headers had 0x02).
        assert_eq!(
            super::ioc(3, super::GPIO_GET_LINEHANDLE_IOCTL_NR, 364),
            0xC16CB403
        );
        assert_eq!(super::ioc(2, super::GPIO_GET_CHIPINFO_IOCTL_NR, 68), 0x8044B401);
        assert_eq!(
            super::ioc(3, super::GPIOHANDLE_GET_LINE_VALUES_IOCTL_NR, 64),
            0xC040B408
        );
        assert_eq!(
            super::ioc(3, super::GPIOHANDLE_SET_LINE_VALUES_IOCTL_NR, 64),
            0xC040B409
        );
    }
}

fn ioc(direction: u32, nr: u32, size: usize) -> libc::c_ulong {
    ((direction << 30) | (GPIO_TYPE << 8) | nr | ((size as u32) << 16)) as libc::c_ulong
}

/// Open a gpio chip device read-only (gpioout.c parity — the linehandle
/// fd carries the writes, the chip fd is only for ioctls).
fn open_chip(chip: &str) -> Res<std::fs::File> {
    std::fs::OpenOptions::new()
        .read(true)
        .open(chip)
        .map_err(|e| cmsg(format!("{chip}: {e}")))
}

/// (name, label, ngpio) for one chip, via GPIO_GET_CHIPINFO_IOCTL.
pub fn chip_info(chip: &str) -> Res<(String, String, u32)> {
    let f = open_chip(chip)?;
    let mut info: GpioChipInfo = unsafe { std::mem::zeroed() };
    let nr = ioc(2, GPIO_GET_CHIPINFO_IOCTL_NR, std::mem::size_of::<GpioChipInfo>());
    // SAFETY: ioctl with a correctly-laid-out &mut struct.
    let r = unsafe { libc::ioctl(f.as_raw_fd(), nr, &mut info as *mut GpioChipInfo) };
    if r < 0 {
        return Err(cmsg(format!("GPIO_GET_CHIPINFO on {chip}: {}", std::io::Error::last_os_error())));
    }
    let name = cstr(&info.name);
    let label = cstr(&info.label);
    Ok((name, label, info.lines))
}

/// Enumerate /dev/gpiochip0..N present on the system.
pub fn list_chips() -> Vec<(String, String, String, u32)> {
    let mut out = Vec::new();
    for i in 0..8 {
        let p = format!("/dev/gpiochip{i}");
        if !std::path::Path::new(&p).exists() {
            continue;
        }
        if let Ok((name, label, ngpio)) = chip_info(&p) {
            out.push((p, name, label, ngpio));
        }
    }
    out
}

/// Find the chip whose line range covers `line` (base 0 per-chip in the
/// v1 API: line offsets are relative to the chip).
pub fn chip_for_line(line: u32) -> Option<String> {
    for (path, _name, _label, ngpio) in list_chips() {
        if line < ngpio {
            return Some(path);
        }
    }
    None
}

fn cstr(a: &[i8; 32]) -> String {
    let bytes: Vec<u8> = a.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    String::from_utf8_lossy(&bytes).to_string()
}

/// Request `lines` on `chip`. `output=true` requests them as outputs
/// with `vals` as the default; returns the linehandle fd (caller closes
/// it when done).
pub fn request_lines(chip: &str, lines: &[u32], output: bool, vals: &[u8]) -> Res<libc::c_int> {
    if lines.is_empty() || lines.len() > MAX_LINES {
        return Err(cmsg("no lines requested"));
    }
    let f = open_chip(chip)?;
    let mut req: GpioHandleRequest = unsafe { std::mem::zeroed() };
    for (i, &l) in lines.iter().enumerate() {
        req.lineoffsets[i] = l;
        req.default_values[i] = vals.get(i).copied().unwrap_or(0);
    }
    req.flags = if output { GPIOHANDLE_REQUEST_OUTPUT } else { GPIOHANDLE_REQUEST_INPUT };
    req.lines = lines.len() as u32;
    let label = b"speaker-amp\0";
    for (i, b) in label.iter().enumerate() {
        req.consumer_label[i] = *b as i8;
    }
    let nr = ioc(3, GPIO_GET_LINEHANDLE_IOCTL_NR, std::mem::size_of::<GpioHandleRequest>());
    // SAFETY: ioctl with a correctly-laid-out &mut struct.
    let r = unsafe { libc::ioctl(f.as_raw_fd(), nr, &mut req as *mut GpioHandleRequest) };
    drop(f);
    if r < 0 {
        return Err(cmsg(format!(
            "GPIO_GET_LINEHANDLE on {chip}: {} (ioctl {nr:#x}, {} line(s) {lines:?})",
            std::io::Error::last_os_error(),
            lines.len(),
        )));
    }
    Ok(req.fd)
}

fn set_values(fd: libc::c_int, vals: &[u8]) -> Res<()> {
    let mut data: GpioHandleData = unsafe { std::mem::zeroed() };
    for (i, &v) in vals.iter().enumerate() {
        if i < MAX_LINES {
            data.values[i] = v;
        }
    }
    let nr = ioc(3, GPIOHANDLE_SET_LINE_VALUES_IOCTL_NR, std::mem::size_of::<GpioHandleData>());
    // SAFETY: see above.
    let r = unsafe { libc::ioctl(fd, nr, &mut data as *mut GpioHandleData) };
    if r < 0 {
        return Err(cmsg(format!("GPIOHANDLE_SET_LINE_VALUES: {}", std::io::Error::last_os_error())));
    }
    Ok(())
}

fn get_values(fd: libc::c_int, n: usize) -> Res<Vec<u8>> {
    let mut data: GpioHandleData = unsafe { std::mem::zeroed() };
    let nr = ioc(3, GPIOHANDLE_GET_LINE_VALUES_IOCTL_NR, std::mem::size_of::<GpioHandleData>());
    // SAFETY: see above.
    let r = unsafe { libc::ioctl(fd, nr, &mut data as *mut GpioHandleData) };
    if r < 0 {
        return Err(cmsg(format!("GPIOHANDLE_GET_LINE_VALUES: {}", std::io::Error::last_os_error())));
    }
    Ok(data.values[..n.min(MAX_LINES)].to_vec())
}

/// Drive `lines` to `vals` on `chip` (one request + one set; lines stay
/// at their last value after the handle closes).
pub fn drive(chip: &str, lines: &[u32], vals: &[u8]) -> Res<()> {
    let fd = request_lines(chip, lines, true, vals)?;
    let r = set_values(fd, vals);
    // SAFETY: close an fd we own.
    unsafe { libc::close(fd) };
    r
}

/// Read the current level of `lines` on `chip` (requested as inputs —
/// the gpioout -g mode; gpiolib direction config is the cost of the v1
/// API here).
pub fn read_levels(chip: &str, lines: &[u32]) -> Res<Vec<u8>> {
    let fd = request_lines(chip, lines, false, &[])?;
    let r = get_values(fd, lines.len());
    // SAFETY: close an fd we own.
    unsafe { libc::close(fd) };
    r
}

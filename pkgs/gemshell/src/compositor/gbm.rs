//! gbm — a hand-rolled FFI shim.
//!
//! **mesa 26 API change (verified against the mesa-26.2.2 source,
//! 2026-09-11):** `gbm_create_device` now takes an **int fd**, not a
//! node path (src/gbm/main/gbm.c: `gbm_create_device(int fd)`). The
//! pre-26 `const char *node` signature is gone from the same soname —
//! passing a path made the lib fstat() a truncated pointer and return
//! a bare NULL (that was the on-glass "gbm_create_device failed" —
//! open() and drmOpen() of the node themselves are fine). So: open the
//! node ourselves, hand the fd to gbm.
//!
//! Usage: `Gbm::new("/dev/dri/renderD128")` → the device whose fd the
//! EGL side consumes: eglGetPlatformDisplay(EGL_PLATFORM_GBM_MESA,
//! gbm_device) — Mesa allocates its own bo pool; we never touch the bo.

use std::os::raw::{c_int, c_void};

extern "C" {
    /// The mesa 26 signature: an fd, NOT a node name.
    pub fn gbm_create_device(fd: c_int) -> *mut c_void;
    pub fn gbm_device_destroy(dev: *mut c_void);
    pub fn gbm_device_get_fd(dev: *mut c_void) -> c_int;
}

/// The gbm device (owned; passed by pointer to EGL).
pub struct Gbm {
    pub ptr: *mut c_void,
    /// the device fd (the one gbm was created with — poll it for
    /// flip/backbuffer events)
    pub fd: c_int,
}

impl Gbm {
    /// Open the gbm device on the given node (falling back through the
    /// others). The fd is handed to gbm and kept for event polling.
    pub fn new(node: &str) -> Result<Self, String> {
        let nodes = [
            node,
            "/dev/dri/renderD129",
            "/dev/dri/renderD128",
            "/dev/dri/card0",
            "/dev/dri/card1",
        ];
        let mut errs = String::new();
        for n in nodes {
            let nc = std::ffi::CString::new(n).map_err(|_| "nul in node".to_string())?;
            let fd = unsafe { libc::open(nc.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
            if fd < 0 {
                errs.push_str(&format!("{n}: open() failed: {:?}; ", std::io::Error::last_os_error()));
                continue;
            }
            let ptr = unsafe { gbm_create_device(fd) };
            if ptr.is_null() {
                unsafe { libc::close(fd) };
                errs.push_str(&format!(
                    "{n}: open() ok, gbm_create_device(fd) NULL (errno {}); ",
                    std::io::Error::last_os_error().raw_os_error().unwrap_or(-1)
                ));
                continue;
            }
            // gbm_create_device takes ownership of the fd on success.
            return Ok(Gbm {
                ptr,
                fd: unsafe { gbm_device_get_fd(ptr) },
            });
        }
        Err(format!("gbm: no usable node — {errs}"))
    }

    /// Drain the pending flip event(s).
    pub fn consume_flip(&self) -> bool {
        let mut ev = 0u32;
        let n = unsafe {
            libc::read(self.fd, &mut ev as *mut u32 as *mut _, std::mem::size_of::<u32>())
        };
        n > 0
    }
}

impl Drop for Gbm {
    fn drop(&mut self) {
        unsafe { gbm_device_destroy(self.ptr) };
    }
}

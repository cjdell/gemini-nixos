//! gbm (the KMS buffer allocator) — a hand-rolled FFI shim.
//!
//! Usage: `gbm_create_device("/dev/dri/card0")` gives us a device whose
//! fd reports page-flip events (a u32: GBM_BACK_BUFFER=1 / GBM_FLIP=2).
//! The EGL side (render.rs) takes the SAME device pointer:
//! eglGetPlatformDisplay(EGL_PLATFORM_GBM_MESA, gbm_device) +
//! eglCreatePlatformSurface(EGL_SURFACE_TYPE, gbm_device, config,
//! EGL_WIDTH/EGL_HEIGHT) — Mesa then allocates its own double-buffered
//! bo pool on the device, renders into it and page-flips through the
//! geminipda-drm shadow plane on eglSwapBuffers. We never touch the bo
//! ourselves; we only poll the device fd for flip pacing.
//!
//! (An explicit gbm_surface_lock_front_buffer + eglCreatePbufferFromGBMB…
//! style flow would also work, but the platform-surface path is what
//! weston/mesa-egl do and needs no manual bo bookkeeping.)

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};

extern "C" {
    pub fn gbm_create_device(node: *const c_char) -> *mut c_void;
    /// The fd-based API (wlroots/weston use this; the node-name variant
    /// above was returning NULL for EVERY node on this device's mesa
    /// 26.2.2 + libdrm 2.4.134 — 2026-09-11, open() itself succeeds).
    pub fn gbm_device_new_fd(fd: c_int) -> *mut c_void;
    pub fn gbm_device_destroy(dev: *mut c_void);
    pub fn gbm_device_get_fd(dev: *mut c_void) -> c_int;
}

/// The gbm device (owned; passed by pointer to EGL).
pub struct Gbm {
    pub ptr: *mut c_void,
    /// the device fd (poll it for flip events)
    pub fd: c_int,
}

impl Gbm {
    /// Open the gbm device. Tries the nodes in order and reports each
    /// failure (diagnostic, added 2026-09-11: the service hit a bare
    /// NULL from gbm_create_device(renderD128) — the direct-open probe
    /// below tells open() from gbm apart).
    pub fn new(node: &str) -> Result<Self, String> {
        let nodes = [
            node,
            "/dev/dri/renderD129",
            "/dev/dri/card0",
            "/dev/dri/card1",
        ];
        let mut errs = String::new();
        for n in nodes {
            // Direct open probe: separates a device/permission failure
            // from a gbm-internal failure.
            let nc = CString::new(n).map_err(|_| "nul in node".to_string())?;
            let ofd = unsafe { libc::open(nc.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
            if ofd >= 0 {
                unsafe { libc::close(ofd) };
            } else {
                errs.push_str(&format!("{n}: open() failed: {:?}; ", std::io::Error::last_os_error()));
                continue;
            }
            let ptr = unsafe { gbm_create_device(nc.as_ptr()) };
            if ptr.is_null() {
                // Fallback: the fd-based API (wlroots' path).
                let ofd2 = unsafe { libc::open(nc.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
                if ofd2 >= 0 {
                    let ptr2 = unsafe { gbm_device_new_fd(ofd2) };
                    if !ptr2.is_null() {
                        let fd = unsafe { gbm_device_get_fd(ptr2) };
                        if fd >= 0 {
                            return Ok(Gbm { ptr: ptr2, fd });
                        }
                        unsafe { gbm_device_destroy(ptr2) };
                    }
                    unsafe { libc::close(ofd2) };
                }
                errs.push_str(&format!("{n}: gbm_create_device + gbm_device_new_fd both NULL; "));
                continue;
            }
            let fd = unsafe { gbm_device_get_fd(ptr) };
            if fd >= 0 {
                return Ok(Gbm { ptr, fd });
            }
            unsafe { gbm_device_destroy(ptr) };
            errs.push_str(&format!("{n}: open() ok, gbm_create_device ok, get_fd failed; "));
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

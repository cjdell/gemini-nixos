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
    pub fn new(node: &str) -> Result<Self, String> {
        let node_c = CString::new(node).map_err(|_| "nul in node".to_string())?;
        let ptr = unsafe { gbm_create_device(node_c.as_ptr()) };
        if ptr.is_null() {
            return Err(format!("gbm_create_device({node}) failed — is the card present?"));
        }
        let fd = unsafe { gbm_device_get_fd(ptr) };
        if fd < 0 {
            unsafe { gbm_device_destroy(ptr) };
            return Err("gbm_device_get_fd failed".into());
        }
        Ok(Gbm { ptr, fd })
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

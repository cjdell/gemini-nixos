#![allow(dead_code)] // context handle + helpers retained for the surface
//! EGL glue: create an OpenGL ES 3.1 context on the winit (Wayland)
//! window surface via the raw `egl` bindings crate (0.2.7, seankerr),
//! then load the `gl` 0.14 global facade with eglGetProcAddress.
//!
//! The Mali-T880 serves GLES 3.1 (panfrost, geminipda fork). We request
//! an ES3 config + a context of client version 3; the compositor-side
//! surface is a plain wl_surface, so no visual negotiation is needed.

use egl::EGLint;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};

/// ES3 renderable bit — not present in the egl 0.2.7 constant list.
const EGL_OPENGL_ES3_BIT: EGLint = 0x0040;
/// EGL 1.5 platform id for Wayland (not in the egl 0.2.7 constant list).
const EGL_PLATFORM_WAYLAND_E: EGLint = 0x31D8;

// Platform-surface entry points — the egl 0.2.7 crate predates EGL 1.5
// in its constant/function list. Mesa (including the geminipda
// panfrost fork) exports the *EXT names.
extern "C" {
    // libglvnd exports the EGL 1.5 core names (no EXT suffix)
    fn eglGetPlatformDisplay(
        platform: EGLint,
        native_display: *mut std::os::raw::c_void,
        attrib_list: *const EGLint,
    ) -> egl::EGLDisplay;
    fn eglCreatePlatformWindowSurface(
        display: egl::EGLDisplay,
        config: egl::EGLConfig,
        surface: *mut std::os::raw::c_void,
        attrib_list: *const EGLint,
    ) -> egl::EGLSurface;
}

// Mesa's Wayland EGL platform requires the native window to be a
// wl_egl_window (it carries the surface SIZE, which a bare wl_surface
// has none of) — passing the raw wl_surface returns EGL_BAD_NATIVE_WINDOW
// (found on-glass 2026-09-08). wl_egl_window_create lives in
// libwayland-egl (the `wayland` pkg; linked below, on the RUNPATH via
// pkgs/gemdemo.nix's postFixup).
#[link(name = "wayland-egl")]
extern "C" {
    fn wl_egl_window_create(
        surface: *mut std::os::raw::c_void,
        width: std::os::raw::c_int,
        height: std::os::raw::c_int,
    ) -> *mut std::os::raw::c_void;
}

pub struct GlCtx {
    pub display: egl::EGLDisplay,
    pub surface: egl::EGLSurface,
    pub context: egl::EGLContext,
}

impl GlCtx {
    pub fn new(window: &winit::window::Window) -> Result<GlCtx, String> {
        // 1) WAYLAND platform display (the default display is the X11/
        //    DRM path — wrong under labwc/gemwl) + initialize
        let dl = window
            .display_handle()
            .map_err(|e| format!("display_handle: {e}"))?;
        let wl_display: *mut std::os::raw::c_void = match dl.as_ref() {
            winit::raw_window_handle::RawDisplayHandle::Wayland(h) => h.display.as_ptr(),
            other => {
                return Err(format!(
                    "expected a Wayland display, got {other:?} — is WAYLAND_DISPLAY set?"
                ))
            }
        };
        let display = unsafe {
            eglGetPlatformDisplay(EGL_PLATFORM_WAYLAND_E, wl_display, std::ptr::null())
        };
        if display == egl::EGL_NO_DISPLAY {
            return Err(format!(
                "eglGetPlatformDisplay(WAYLAND) failed (error 0x{:x}) — does the Mesa driver expose the Wayland platform?",
                egl::get_error()
            ));
        }
        let (mut major, mut minor) = (0, 0);
        if !egl::initialize(display, &mut major, &mut minor) {
            return Err(format!(
                "eglInitialize failed (error 0x{:x}) — is libEGL present in the session?",
                egl::get_error()
            ));
        }

        // 2) window handle → native wl_surface
        let handle = window
            .window_handle()
            .map_err(|e| format!("window_handle: {e}"))?;
        let surface_ptr: *mut std::os::raw::c_void = match handle.as_ref() {
            winit::raw_window_handle::RawWindowHandle::Wayland(h) => h.surface.as_ptr(),
            other => {
                return Err(format!(
                    "expected a Wayland window surface, got {other:?} — is WAYLAND_DISPLAY set?"
                ))
            }
        };
        if surface_ptr.is_null() {
            return Err("null wl_surface".to_string());
        }

        // 3) config: ES3 renderable + window surface + 8-bit RGB. (The
        //    first on-glass run hit EGL_BAD_ATTRIBUTE here: the list
        //    originally also passed EGL_RENDER_BUFFER/EGL_RGB_BUFFER as
        //    a key/value pair — neither is a valid eglChooseConfig key
        //    (EGL_RGB_BUFFER is a VALUE for EGL_COLOR_BUFFER_TYPE; the
        //    render-buffer attribute belongs on surfaces).
        let attrs: [EGLint; 11] = [
            egl::EGL_RENDERABLE_TYPE,
            EGL_OPENGL_ES3_BIT | egl::EGL_OPENGL_ES2_BIT,
            egl::EGL_SURFACE_TYPE,
            egl::EGL_WINDOW_BIT,
            egl::EGL_RED_SIZE,
            8,
            egl::EGL_GREEN_SIZE,
            8,
            egl::EGL_BLUE_SIZE,
            8,
            egl::EGL_NONE,
        ];
        let config = egl::choose_config(display, &attrs, 1)
            .ok_or_else(|| format!("eglChooseConfig failed (error 0x{:x})", egl::get_error()))?;

        // 4) platform window surface (EGL 1.5 entry point). The native
        //    handle must be a wl_egl_window wrapping the winit wl_surface
        //    (EGL needs the window size): create it at the window's
        //    current inner size. NOTE: a later interactive resize would
        //    need wl_egl_window_resize to follow — the demo's windows are
        //    fixed-size today (fullscreen or the --windowed default).
        let (ww, wh) = {
            let s = window.inner_size();
            (s.width.max(1) as std::os::raw::c_int, s.height.max(1) as std::os::raw::c_int)
        };
        let egl_window = unsafe { wl_egl_window_create(surface_ptr, ww, wh) };
        if egl_window.is_null() {
            return Err("wl_egl_window_create failed".to_string());
        }
        let surface = unsafe {
            eglCreatePlatformWindowSurface(display, config, egl_window, std::ptr::null())
        };
        if surface == egl::EGL_NO_SURFACE {
            return Err(format!(
                "eglCreatePlatformWindowSurface failed (error 0x{:x})",
                egl::get_error()
            ));
        }

        // 5) ES 3.x context
        let context = egl::create_context(display, config, std::ptr::null_mut(), &[
            egl::EGL_CONTEXT_CLIENT_VERSION,
            3,
            egl::EGL_NONE,
        ])
        .ok_or_else(|| format!("eglCreateContext failed (error 0x{:x})", egl::get_error()))?;

        // 6) make current
        if !egl::make_current(display, surface, surface, context) {
            return Err(format!("eglMakeCurrent failed (error 0x{:x})", egl::get_error()));
        }

        Ok(GlCtx {
            display,
            surface,
            context,
        })
    }

    /// Loader for the gl 0.14 facade.
    pub fn load_gl(&self) {
        // gl 0.14: `gl::load_with(|name| ptr)` — global load.
        gl::load_with(|name: &str| -> *const std::os::raw::c_void {
            let p: extern "C" fn() = egl::get_proc_address(name);
            p as *const std::os::raw::c_void
        });
    }

    pub fn swap(&self) {
        let ok = egl::swap_buffers(self.display, self.surface);
        // A failed swap after the first frames points at the wl_egl_window
        // no longer matching the compositor's surface (on-glass 2026-09-08:
        // the demo froze after ~5 presented frames).
        if !ok {
            static mut SWAP_FAILS: u32 = 0;
            unsafe {
                let n = {
                    let v = SWAP_FAILS;
                    SWAP_FAILS += 1;
                    v
                };
                if n < 3 || n % 300 == 0 {
                    eprintln!(
                        "gemdemo: eglSwapBuffers FAILED x{} (egl err 0x{:x})",
                        n + 1,
                        egl::get_error()
                    );
                }
            }
        }
    }

    pub fn version_string(&self) -> String {
        // (kept for API symmetry; main.rs reports via gl_identity)
        let _ = (self.display, self.surface, self.context);
        unsafe {
            // GLubyte* — c_char is i8 on x86_64, u8 on aarch64
            let p = gl::GetString(gl::VERSION) as *const std::os::raw::c_char;
            if p.is_null() {
                return "unknown".to_string();
            }
            std::ffi::CStr::from_ptr(p)
                .to_string_lossy()
                .trim()
                .to_string()
        }
    }
}

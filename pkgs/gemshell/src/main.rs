//! gemshell — the native Gemini PDA Wayland compositor (binary 1 of 2).
//!
//! KMS/GBM + Mesa GL rendering, wl_shm + xdg-shell clients, raw-evdev
//! multitouch with compositor-level gestures, compositor-drawn shell
//! (status bar / taskbar / launcher / app switcher), 3 workspaces,
//! window snapping. Design + receipts: docs/gemshell.md.
//!
//! Display path: /dev/dri/card0 = `geminipda-drm` (the LK framebuffer
//! exposed as DRM/KMS — one fixed 1080x2160 portrait mode, XRGB8888
//! shadow plane). gbm surface on card0 -> EGL platform surface (GBM) ->
//! eglSwapBuffers page-flips the shadow plane. Mesa `kmsro` pairs card0
//! with renderD128 (panfrost) — the exact stack GNOME uses
//! (docs/gnome-feasibility.md, 2026-09-10).
//!
//! Input: raw evdev (keyboard = AW9523 gpio-matrix, touch = NT36772
//! protocol-B multitouch). The compositor consumes gestures itself and
//! forwards one-finger interactions as wl_touch to clients (touch-only —
//! there is no mouse on this device).

mod common;
mod compositor;

fn main() {
    let (display, mut comp) = match compositor::Compositor::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("gemshell: {e}");
            std::process::exit(1);
        }
    };
    std::process::exit(comp.run(display));
}

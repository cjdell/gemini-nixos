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

/// Unbind the fbcon consoles (the gemwl receipt): a bound console would
/// redraw into the same LK framebuffer gemshell composites into. In
/// gemshell desktop mode there is no tty1 getty (the apply script), but
/// fbcon may still hold a bind — gemwl does this at startup (gemwl.c
/// unbind_fbcons). Best-effort; run as the service user (the sysfs files
/// need root — the service runs with the video group only, so this may
/// fail harmlessly; the real protection is no console on the panel).
fn unbind_fbcons() {
    for i in 0..4 {
        let p = format!("/sys/class/vtconsole/vtcon{i}/bind");
        let _ = std::fs::write(&p, "0");
    }
}

fn main() {
    // Nested mode (x86_64 development): run as a client under the host
    // compositor. Env-driven so the same binary is used everywhere.
    let nested = std::env::var_os("GEMSHELL_NESTED").is_some();
    if !nested {
        unbind_fbcons();
    }
    let (display, mut comp) = match compositor::Compositor::new(nested) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("gemshell: {e}");
            std::process::exit(1);
        }
    };
    std::process::exit(comp.run(display));
}

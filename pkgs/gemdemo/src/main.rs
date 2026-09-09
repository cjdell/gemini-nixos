//! gemdemo 0.3.0 — minimal OpenGL ES 3.1 template for the Gemini PDA.
//!
//! ONE FILE on purpose. This is the "how do I talk to the hardware from
//! Rust" skeleton that survived the 0.2.0 EXODUS demoscene purge: it
//! opens a Wayland window (winit), puts a GLES 3.1 context on it (EGL
//! via the raw `egl` crate), draws one spinning shaded triangle with
//! the gl 0.14 bindings, and plays a 440 Hz sine through the MT6351
//! codec (cpal → ALSA `gemini16` plug). Copy this file + pkgs/gemdemo.nix
//! to start a real OpenGL app; the comments are the hardware receipts.
//!
//! ============ THE RECEIPTS (why this file looks the way it does) ============
//! Every one of these was learned the expensive way on this device
//! (2026-09-08/09, gemdemo 0.1.0/0.2.0 on glass). Do not "modernize"
//! them away:
//!
//! * The GPU is a Mali-T880 MP4 on the geminipda Mesa/panfrost FORK
//!   (pkgs/mesa-geminipda.nix) serving **GLES 3.1 only**. Shaders must
//!   be ESSL 300 es. The window is a wl_surface under gemwl/labwc.
//! * **DSA → glGen\***: glCreateTextures/Buffers/Framebuffers/
//!   VertexArrays are "unsupported function" stubs on this context and
//!   silently create NOTHING. Create objects with glGen* + Bind.
//! * **One interleaved buffer per VAO** with per-attr offsets. No
//!   instancing (segfaulted the fork), no multi-buffer VAOs (panfrost
//!   JOB_BUS_FAULT storms), no glDrawElements (index-minmax crash) —
//!   all draws are expanded glDrawArrays triangles.
//! * EGL wants a **wl_egl_window**, not a bare wl_surface, as the
//!   native window (wl_egl_window_create in libwayland-egl; linked
//!   below, -L via RUSTFLAGS, RUNPATH pinned in the derivation).
//! * The wl_egl_window is created at the window's CURRENT size and the
//!   demo never resizes (interactive resize would need
//!   wl_egl_window_resize to follow). Fixed-size windows only.
//! * **Audio wire state: S16_LE @ 44100 Hz** on the 16-bit-only MT6351
//!   (the AFE advertises 48k / 32-bit without programming it — S32 or
//!   48k = white noise). cpal 0.15 finds `gemini16` only because
//!   asound.conf gives it `hint { show on }`; the `default` PCM
//!   fallback converts f32→S32-on-wire = noise, so we always ask the
//!   device list for gemini16 first and force an I16 @ 44.1k config
//!   when offered.
//! * Missing uniforms are not errors at link on GLES (they optimize
//!   away) — GetUniformLocation returns -1; assert so typos are caught.
//! * glReadPixels: GLES3 forbids RGB/UNSIGNED_BYTE from an RGBA8
//!   buffer (the 0.2.0 PPM-dump QA path had to read RGBA). Not used
//!   here; remembered in case the template grows a screenshot.
//! =============================================================================

use std::os::raw::{c_char, c_int, c_void};
use std::time::Instant;

use winit::dpi::PhysicalSize;
use winit::event::{ElementState, Event, KeyEvent, WindowEvent};
use winit::event_loop::EventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::{Fullscreen, WindowBuilder, WindowLevel};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::StreamConfig;

// ---------------------------------------------------------------------------
// EGL bootstrap (kept ~verbatim from 0.2.0's glctx.rs — this is the
// "GLES context on a Wayland surface" recipe).
// ---------------------------------------------------------------------------

/// ES3 renderable bit — not in the egl 0.2.7 constant list.
const EGL_OPENGL_ES3_BIT: egl::EGLint = 0x0040;
/// EGL 1.5 platform id for Wayland (not in the egl 0.2.7 constant list).
const EGL_PLATFORM_WAYLAND_E: egl::EGLint = 0x31D8;

// Platform-surface entry points — the egl 0.2.7 crate predates EGL 1.5
// in its constant/function list; libglvnd exports the core names.
extern "C" {
    fn eglGetPlatformDisplay(
        platform: egl::EGLint,
        native_display: *mut c_void,
        attrib_list: *const egl::EGLint,
    ) -> egl::EGLDisplay;
    fn eglCreatePlatformWindowSurface(
        display: egl::EGLDisplay,
        config: egl::EGLConfig,
        surface: *mut c_void,
        attrib_list: *const egl::EGLint,
    ) -> egl::EGLSurface;
}

#[link(name = "wayland-egl")]
extern "C" {
    // wl_egl_window carries the surface SIZE (a bare wl_surface →
    // EGL_BAD_NATIVE_WINDOW, on glass 2026-09-08). libwayland-egl.
    fn wl_egl_window_create(surface: *mut c_void, width: c_int, height: c_int) -> *mut c_void;
}

struct GlCtx {
    display: egl::EGLDisplay,
    surface: egl::EGLSurface,
    context: egl::EGLContext,
}

impl GlCtx {
    fn new(window: &winit::window::Window) -> Result<GlCtx, String> {
        // 1) Wayland platform display + initialize (the DEFAULT display
        //    is the X11/DRM path — wrong under labwc/gemwl).
        let dl = window
            .display_handle()
            .map_err(|e| format!("display_handle: {e}"))?;
        let wl_display: *mut c_void = match dl.as_ref() {
            winit::raw_window_handle::RawDisplayHandle::Wayland(h) => h.display.as_ptr(),
            other => {
                return Err(format!(
                    "expected a Wayland display, got {other:?} — is WAYLAND_DISPLAY set?"
                ))
            }
        };
        let display =
            unsafe { eglGetPlatformDisplay(EGL_PLATFORM_WAYLAND_E, wl_display, std::ptr::null()) };
        if display == egl::EGL_NO_DISPLAY {
            return Err(format!(
                "eglGetPlatformDisplay(WAYLAND) failed (error 0x{:x})",
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
        let surface_ptr: *mut c_void = match handle.as_ref() {
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

        // 3) config: ES3 renderable + window surface + 8-bit RGB. (Do
        //    NOT pass EGL_RENDER_BUFFER or EGL_RGB_BUFFER here — neither
        //    is a valid eglChooseConfig key; that was an on-glass
        //    EGL_BAD_ATTRIBUTE.)
        let attrs: [egl::EGLint; 11] = [
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

        // 4) platform window surface: the native handle MUST be a
        //    wl_egl_window created at the window's current size
        //    (fixed-size windows only — see the header).
        let (ww, wh) = {
            let s = window.inner_size();
            (s.width.max(1) as c_int, s.height.max(1) as c_int)
        };
        let egl_window = unsafe { wl_egl_window_create(surface_ptr, ww, wh) };
        if egl_window.is_null() {
            return Err("wl_egl_window_create failed".to_string());
        }
        let surface =
            unsafe { eglCreatePlatformWindowSurface(display, config, egl_window, std::ptr::null()) };
        if surface == egl::EGL_NO_SURFACE {
            return Err(format!(
                "eglCreatePlatformWindowSurface failed (error 0x{:x})",
                egl::get_error()
            ));
        }

        // 5) ES 3.x context + make current
        let context = egl::create_context(display, config, std::ptr::null_mut(), &[
            egl::EGL_CONTEXT_CLIENT_VERSION,
            3,
            egl::EGL_NONE,
        ])
        .ok_or_else(|| format!("eglCreateContext failed (error 0x{:x})", egl::get_error()))?;
        if !egl::make_current(display, surface, surface, context) {
            return Err(format!("eglMakeCurrent failed (error 0x{:x})", egl::get_error()));
        }
        Ok(GlCtx { display, surface, context })
    }

    /// Load the gl 0.14 facade through eglGetProcAddress.
    fn load_gl(&self) {
        let _ = (self.display, self.surface, self.context);
        gl::load_with(|name: &str| -> *const c_void {
            let p: extern "C" fn() = egl::get_proc_address(name);
            p as *const c_void
        });
    }

    fn swap(&self) {
        egl::swap_buffers(self.display, self.surface);
    }
}

// ---------------------------------------------------------------------------
// Tiny GL helpers (from 0.2.0's glutil.rs; the diagnostic panics are
// what make shader typos debuggable from a serial console).
// ---------------------------------------------------------------------------

/// Compile + link a program, panicking with the driver log on failure.
unsafe fn build_program(name: &str, vs: &str, fs: &str) -> u32 {
    let compile = |src: &str, kind: u32| -> u32 {
        let csrc = std::ffi::CString::new(src).expect("shader has no NUL");
        let sh = gl::CreateShader(kind);
        gl::ShaderSource(sh, 1, &csrc.as_ptr(), std::ptr::null());
        gl::CompileShader(sh);
        let mut ok = 0;
        gl::GetShaderiv(sh, gl::COMPILE_STATUS, &mut ok);
        if ok == 1 {
            return sh;
        }
        let mut len = 0;
        gl::GetShaderiv(sh, gl::INFO_LOG_LENGTH, &mut len);
        let mut buf = vec![0u8; len.max(1) as usize];
        gl::GetShaderInfoLog(sh, len, &mut len, buf.as_mut_ptr() as *mut _);
        panic!("shader {name} failed: {}", String::from_utf8_lossy(&buf).trim());
    };
    let vs_id = compile(vs, gl::VERTEX_SHADER);
    let fs_id = compile(fs, gl::FRAGMENT_SHADER);
    let prog = gl::CreateProgram();
    gl::AttachShader(prog, vs_id);
    gl::AttachShader(prog, fs_id);
    gl::LinkProgram(prog);
    gl::DeleteShader(vs_id);
    gl::DeleteShader(fs_id);
    let mut ok = 0;
    gl::GetProgramiv(prog, gl::LINK_STATUS, &mut ok);
    if ok != 1 {
        let mut len = 0;
        gl::GetProgramiv(prog, gl::INFO_LOG_LENGTH, &mut len);
        let mut buf = vec![0u8; len.max(1) as usize];
        gl::GetProgramInfoLog(prog, len, &mut len, buf.as_mut_ptr() as *mut _);
        panic!("link {name} failed: {}", String::from_utf8_lossy(&buf).trim());
    }
    prog
}

/// Uniform lookup — a missing uniform (optimized away or typo'd) is a
/// bug in the shader, not a runtime question: assert it exists.
unsafe fn uniform(prog: u32, name: &str) -> i32 {
    let loc = gl::GetUniformLocation(prog, std::ffi::CString::new(name).unwrap().as_ptr());
    assert!(loc >= 0, "uniform {name:?} missing from program");
    loc
}

/// Print the first error of each kind once (quiet otherwise).
unsafe fn check(op: &str) {
    static mut LAST: u32 = 0;
    let e = gl::GetError();
    if e != gl::NO_ERROR && e != LAST {
        LAST = e;
        eprintln!("gemdemo: GL error 0x{e:04x} after {op}");
    }
}

unsafe fn gl_string(pname: u32) -> String {
    let p = gl::GetString(pname) as *const c_char;
    if p.is_null() {
        return "unknown".into();
    }
    std::ffi::CStr::from_ptr(p).to_string_lossy().trim().to_string()
}

// ---------------------------------------------------------------------------
// Audio: 440 Hz sine through cpal → ALSA (the receipt-compliant open).
// ---------------------------------------------------------------------------

const SINE_HZ: f64 = 440.0;
const SINE_AMP: f32 = 0.25; // modest; full-scale squares the codec
const TAU: f64 = std::f64::consts::TAU;

fn start_sine() -> Result<cpal::Stream, String> {
    let host = cpal::default_host(); // cpal 0.15: Host directly (ALSA on Linux)

    // gemini16 is the S16-pinning plug (services/audio.nix); visible to
    // cpal only because asound.conf gives it `hint { show on }`.
    let mut devices = host.devices().map_err(|e| e.to_string())?;
    let device = devices
        .find(|d| d.name().map(|n| n.eq_ignore_ascii_case("gemini16")).unwrap_or(false))
        .or_else(|| host.default_output_device())
        .ok_or_else(|| "no output device (MT6351 card up? gemini16 / default in asound)".to_string())?;
    let devname = device.name().unwrap_or_else(|_| "?".into());

    // Force an I16 @ 44100 config when offered — that is the audible
    // wire state (S32-width or 48k on this codec = white noise).
    let def = device.default_output_config().map_err(|e| e.to_string())?;
    let mut fmt = def.sample_format();
    let mut sr = def.sample_rate();
    let mut channels = def.channels();
    if fmt != cpal::SampleFormat::I16 || sr.0 != 44_100 {
        if let Ok(mut it) = device.supported_output_configs() {
            while let Some(range) = it.next() {
                let want_lo =
                    range.min_sample_rate().0 <= 44_100 && range.max_sample_rate().0 >= 44_100;
                if range.sample_format() == cpal::SampleFormat::I16 && want_lo {
                    fmt = cpal::SampleFormat::I16;
                    sr = cpal::SampleRate(44_100);
                    channels = range.channels();
                    break;
                }
            }
        }
    }
    let ch = channels.max(1) as usize;
    let stream_cfg = StreamConfig {
        sample_rate: sr,
        channels: channels.max(1),
        buffer_size: cpal::BufferSize::Default,
    };
    eprintln!("gemdemo: audio '{devname}' — {} Hz, {} ch, {:?}", sr.0, ch, fmt);

    let err_cb = |e: cpal::StreamError| eprintln!("gemdemo: audio error: {e}");
    let attack_s = (sr.0 as f64 / 100.0).max(1.0); // 10 ms fade-in (open click)

    // Callback body (per sample index s, one value copied to every
    // channel of the frame): the count n advances one per FRAME.
    let stream = if fmt == cpal::SampleFormat::I16 {
        let mut n: u64 = 0;
        device
            .build_output_stream(
                &stream_cfg,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    for (f, out) in data.chunks_exact_mut(ch).enumerate() {
                        let s = (n + f as u64) as f64;
                        let amp = ((s / attack_s).min(1.0)) as f32 * SINE_AMP;
                        let v = (amp * (TAU * SINE_HZ * s / sr.0 as f64).sin() as f32 * 32767.0) as i16;
                        for x in out {
                            *x = v;
                        }
                    }
                    n += (data.len() / ch) as u64;
                },
                err_cb,
                None,
            )
            .map_err(|e| e.to_string())?
    } else {
        let mut n: u64 = 0;
        device
            .build_output_stream(
                &stream_cfg,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    for (f, out) in data.chunks_exact_mut(ch).enumerate() {
                        let s = (n + f as u64) as f64;
                        let amp = ((s / attack_s).min(1.0)) as f32 * SINE_AMP;
                        let v = amp * (TAU * SINE_HZ * s / sr.0 as f64).sin() as f32;
                        for x in out {
                            *x = v;
                        }
                    }
                    n += (data.len() / ch) as u64;
                },
                err_cb,
                None,
            )
            .map_err(|e| e.to_string())?
    };
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

// ---------------------------------------------------------------------------
// The scene: one shaded triangle. A new app replaces this whole section
// with its own content (keep the object-creation receipts).
// ---------------------------------------------------------------------------

const VS: &str = r#"#version 300 es
layout(location=0) in vec2 pos;   // position on the unit circle
layout(location=1) in vec3 col;   // per-vertex colour → smooth shading
out vec3 vcol;
uniform float u_time;             // seconds; the spin angle
uniform float u_aspect;           // w/h — keep the spin circular
void main() {
    float s = sin(u_time), c = cos(u_time);
    vec2 p = mat2(c, -s, s, c) * pos;
    p.x /= u_aspect;
    vcol = col;
    gl_Position = vec4(p, 0.0, 1.0);
}
"#;

const FS: &str = r#"#version 300 es
precision highp float;
in vec3 vcol;
out vec4 frag;
void main() {
    frag = vec4(vcol, 1.0);
}
"#;

/// 3 vertices: pos.xy + col.rgb interleaved (the one-buffer receipt).
fn triangle() -> [f32; 15] {
    let (r, red, grn, blu) = (0.72f32, [1.0f32, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let (a1, a2, a3) = (90f32.to_radians(), 210f32.to_radians(), 330f32.to_radians());
    [
        a1.cos() * r, a1.sin() * r, red[0], red[1], red[2],
        a2.cos() * r, a2.sin() * r, grn[0], grn[1], grn[2],
        a3.cos() * r, a3.sin() * r, blu[0], blu[1], blu[2],
    ]
}

fn main() {
    let windowed = std::env::args().any(|a| a == "--windowed");

    // ---- window: borderless fullscreen by default (real glass under
    //      gemwl), 1024×576 fixed with --windowed (nested labwc QA).
    let event_loop = EventLoop::new().expect("event loop");
    let mut builder = WindowBuilder::new().with_title("gemdemo 0.3.0 — GLES 3.1 template");
    if windowed {
        builder = builder
            .with_inner_size(PhysicalSize::new(1024, 576))
            .with_resizable(false);
    } else {
        builder = builder
            .with_fullscreen(Some(Fullscreen::Borderless(None)))
            .with_window_level(WindowLevel::AlwaysOnTop);
    }
    let window = builder.build(&event_loop).expect("window build");

    // ---- EGL ES3 context on the wl_surface, then the gl bindings ----
    let ctx = GlCtx::new(&window).expect("egl context");
    ctx.load_gl();
    let (vendor, renderer, version) =
        unsafe { (gl_string(gl::VENDOR), gl_string(gl::RENDERER), gl_string(gl::VERSION)) };
    if !version.starts_with("OpenGL ES 3.") {
        eprintln!("gemdemo: need OpenGL ES 3.x, got: {version} — aborting");
        std::process::exit(1);
    }
    println!(
        "gemdemo {0} — {1} / {2} — {3}",
        env!("CARGO_PKG_VERSION"),
        vendor,
        renderer,
        version
    );

    let (w, h) = {
        let s = window.inner_size();
        (s.width.max(1), s.height.max(1))
    };
    let aspect = w as f32 / h as f32;
    println!("window {w}x{h}{}", if windowed { " (windowed)" } else { " (fullscreen)" });

    // ---- audio (best-effort: no card → run silent, like the demo) ----
    let _audio = match start_sine() {
        Ok(s) => {
            println!("gemdemo: audio on — {SINE_HZ} Hz sine, amp {SINE_AMP}");
            Some(s)
        }
        Err(e) => {
            eprintln!("gemdemo: audio unavailable: {e} — running silent");
            None
        }
    };

    unsafe {
        gl::Viewport(0, 0, w as i32, h as i32);
        gl::ClearColor(0.02, 0.03, 0.06, 1.0);

        // ---- program + ONE interleaved VBO per VAO (glGen*, no DSA) ----
        let prog = build_program("tri", VS, FS);
        let u_time = uniform(prog, "u_time");
        let u_aspect = uniform(prog, "u_aspect");
        let mut vao = 0;
        gl::GenVertexArrays(1, &mut vao);
        gl::BindVertexArray(vao);
        let verts = triangle();
        let mut _vbo = 0;
        gl::GenBuffers(1, &mut _vbo);
        gl::BindBuffer(gl::ARRAY_BUFFER, _vbo);
        gl::BufferData(
            gl::ARRAY_BUFFER,
            (verts.len() * 4) as isize,
            verts.as_ptr() as *const c_void,
            gl::STATIC_DRAW,
        );
        let stride = 5 * 4; // pos.xy + col.rgb, all f32
        gl::EnableVertexAttribArray(0);
        gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, stride, std::ptr::null());
        gl::EnableVertexAttribArray(1);
        gl::VertexAttribPointer(1, 3, gl::FLOAT, gl::FALSE, stride, 8 as *const c_void);

        // ---- render loop: pump RedrawRequested (vsync via the
        //      compositor); quit on q/esc/close ----
        let t0 = Instant::now();
        let mut frames: u64 = 0;
        let mut last_frames: u64 = 0;
        let mut last_log = Instant::now();
        let _ = event_loop.run(move |event, elwt| match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => elwt.exit(),
            Event::WindowEvent {
                event:
                    WindowEvent::KeyboardInput {
                        event:
                            KeyEvent {
                                logical_key,
                                state: ElementState::Pressed,
                                ..
                            },
                        ..
                    },
                ..
            } => match logical_key {
                Key::Named(NamedKey::Escape) => elwt.exit(),
                Key::Character(c) if c.eq_ignore_ascii_case("q") => elwt.exit(),
                _ => {}
            },
            Event::WindowEvent {
                event: WindowEvent::RedrawRequested,
                ..
            } => {
                let t = t0.elapsed().as_secs_f32();
                gl::Clear(gl::COLOR_BUFFER_BIT);
                gl::UseProgram(prog);
                gl::Uniform1f(u_time, t);
                gl::Uniform1f(u_aspect, aspect);
                gl::BindVertexArray(vao);
                gl::DrawArrays(gl::TRIANGLES, 0, 3);
                gl::BindVertexArray(0);
                check("frame");
                ctx.swap();
                window.request_redraw(); // continuous: next frame on vsync
                frames += 1;
                if last_log.elapsed().as_secs_f32() >= 2.0 {
                    let dt = last_log.elapsed().as_secs_f64().max(1e-9);
                    let fps = (frames - last_frames) as f64 / dt;
                    last_frames = frames;
                    last_log = Instant::now();
                    eprintln!("gemdemo: t={t:6.1}s frames={frames} ({fps:.0} fps)");
                }
            }
            _ => {}
        });
    }
}

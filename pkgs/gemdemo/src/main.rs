//! GEMINI: EXODUS — the gemdemo 0.2.0 rewrite: a cinematic spacesynth
//! assembly for the Planet Computers Gemini PDA (Mali-T880 MP4, panfrost
//! fork, GLES 3.1, 1280×720). Completely replaces the 0.1.0 "AETHER"
//! architecture (multipass bloom/FB chain + SSAA + raymarch + per-act
//! scenes) whose single-digit-FPS, flawed-visuals, broken-music results
//! sent it back to the shop.
//!
//! Architecture (docs/gemdemo.md "EXODUS rewrite" section):
//!   * audio thread  — cpal (ALSA→PipeWire) callback drives the synth
//!                     engine (synth.rs); the engine is the clock master
//!                     and publishes beat/kick/section atomics.
//!   * render thread — winit event loop on the nested Wayland session.
//!                     Each frame the Director (show.rs) reads the beat
//!                     clock and produces a pure-data FrameState (no GL);
//!                     gfx.rs draws it DIRECTLY into the window surface —
//!                     no intermediate FBOs, no post chain, no SSAA.
//!                     Titles (font.rs) and the optional HUD go on top.
//!   * cpu.rs        — best-effort A72 cluster bring-up (asks the
//!                     gemini-a72-up unit) + render-thread affinity.
//!
//! 60 fps plan (the rewritten renderer): one cheap fullscreen gradient
//! per frame; stars/nebula/particles as tiny pre-baked-texture quads;
//! the only heavy fragment work is the planet sphere + ring and those
//! are confined to a modest screen region and re-tuned to a light
//! noise budget. No multipass, no feedback, no readback.

mod audio;
mod cpu;
mod font;
mod gfx;
mod glctx;
mod glutil;
mod shaders;
mod show;
mod synth;

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use winit::event::{ElementState, Event, KeyEvent, WindowEvent};
use winit::event_loop::EventLoop;
use winit::keyboard::{Key, KeyCode, NamedKey};
use winit::window::{Fullscreen, WindowBuilder, WindowLevel};

/// The beat clock shared between audio (writer) and render (reader)
/// threads. Beat units are fixed-point: 1_000_000 per beat.
#[derive(Default)]
pub struct Clock {
    /// Current beat * 1_000_000 (wraps at TOTAL_BARS * 4 = 320 beats).
    pub beat: AtomicU64,
    /// Kick envelope 0..=65536 (set by the drum synth, decays per sample).
    pub kick: AtomicU32,
    /// Current section 0..6 (audio publishes; render reads).
    pub section: AtomicU32,
    /// Pause flag: audio renders silence + freezes the clock; render
    /// freezes its own time.
    pub paused: AtomicBool,
    /// Section jump request (render thread sets; audio consumes). -1 = none.
    pub jump: AtomicI32,
}

#[derive(Clone)]
pub struct Config {
    pub silent: bool,
    pub windowed: bool,
    pub start_section: i32,
    pub dump_frame: Option<u32>,
    pub dump_path: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            silent: false,
            windowed: false,
            start_section: 0,
            dump_frame: None,
            dump_path: None,
        }
    }
}

fn parse_args(argv: &[String]) -> Result<Config, String> {
    let mut c = Config::default();
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        i += 1;
        let missing = || format!("missing value for {a}");
        match a.as_str() {
            "--silent" => c.silent = true,
            "--windowed" => c.windowed = true,
            "--section" => {
                c.start_section = argv
                    .get(i)
                    .cloned()
                    .ok_or_else(missing)?
                    .parse()
                    .map_err(|_| "--section wants 0..6")?;
                i += 1;
            }
            "--dump" => {
                c.dump_frame = Some(
                    argv.get(i)
                        .cloned()
                        .ok_or_else(missing)?
                        .parse()
                        .map_err(|_| "--dump wants a frame count")?,
                );
                i += 1;
                c.dump_path = Some(argv.get(i).cloned().ok_or_else(missing)?);
                i += 1;
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(format!("unknown flag {other} (see --help)")),
        }
    }
    if c.start_section < 0 || c.start_section > 6 {
        return Err("--section must be 0..6".into());
    }
    Ok(c)
}

fn print_help() {
    println!(
        "gemdemo (GEMINI: EXODUS) — cinematic spacesynth for the Gemini PDA
usage: gemdemo [options]
  --section N   start at section N (0=earth .. 6=origin)
  --silent      no audio (beat clock driven by system time)
  --windowed    run in a window instead of borderless fullscreen
  --dump N p    after N frames, write the window to p.ppm and exit (glass aid)
keys: space=pause  [ / ] = prev/next chapter  h=info line  q/esc=quit"
    );
}

struct AppState {
    window: winit::window::Window,
    ctx: glctx::GlCtx,
    cfg: Config,
    clock: Arc<Clock>,
    // scene
    stage: gfx::Stage,
    dir: show::Director,
    font: font::Font,
    text: font::Writer,
    // frame state
    hud_on: bool,
    fps: f64,
    time: f32,
    last_instant: Instant,
    phys_w: u32,
    phys_h: u32,
    // silent clock
    silent_start: Instant,
    silent_beat_offset: f64,
    // dump counter
    dump_counter: u32,
}

impl AppState {
    fn frame(&mut self, elwt: &winit::event_loop::EventLoopWindowTarget<()>) {
        let now = Instant::now();
        let dt = self.last_instant.elapsed().as_secs_f32();
        self.last_instant = now;
        let paused = self.clock.paused.load(Ordering::Relaxed);
        if !paused {
            self.time += dt;
            if dt > 0.0 {
                self.fps = self.fps * 0.95 + (1.0 / dt as f64) * 0.05;
            }
        }

        // section jumps (keys + --section) → the audio side consumes the
        // same request so the clock follows
        let jump = self.clock.jump.swap(-1, Ordering::Relaxed);
        if jump >= 0 {
            let start = synth::section_start_beat(jump as u32);
            if self.cfg.silent {
                self.silent_beat_offset = start;
                self.silent_start = Instant::now();
            }
        }

        // ---- beat clock (audio master; system time when silent) ----
        let mut beat: f64;
        if self.cfg.silent {
            beat = now
                .duration_since(self.silent_start)
                .as_secs_f64()
                * synth::BPM
                / 60.0
                + self.silent_beat_offset;
            beat %= synth::TOTAL_BARS as f64 * 4.0;
            self.clock
                .beat
                .store((beat * 1_000_000.0) as u64, Ordering::Relaxed);
            self.clock
                .section
                .store(synth::section_for_beat(beat), Ordering::Relaxed);
        } else {
            beat = self.clock.beat.load(Ordering::Relaxed) as f64 / 1_000_000.0;
        }
        let section = synth::section_for_beat(beat) as usize;
        let kick = if self.cfg.silent {
            let kf = beat.fract();
            if kf < 0.10 {
                (1.0 - kf / 0.10) as f32
            } else {
                0.0
            }
        } else {
            self.clock.kick.load(Ordering::Relaxed) as f32 / 65536.0
        };

        // chapter progress within the current section
        let start = synth::section_start_beat(section as u32);
        let cp = (((beat - start) / (synth::SECTION_BARS[section] as f64 * 4.0)) as f32).clamp(0.0, 1.0);
        let t = if paused {
            self.time
        } else {
            (beat * synth::BPM as f64 / 60.0) as f32
        };

        // the director's frame (pure data) — borrows self.dir
        let fs = self.dir.frame(t, section, cp, kick, beat);

        // CPU-side animation (stars, particles, trail) + world draw; both
        // touch self.stage (disjoint from the self.dir borrow above)
        self.stage.update(if paused { 0.0 } else { dt }, fs);
        let (w, h) = (self.phys_w, self.phys_h);
        unsafe {
            self.stage.render(w, h, fs);
        }

        // copy the director's text out (ends the fs borrow), then text +
        // swap together so titles are part of the presented frame
        let tf = self.dir.text;
        if !paused || self.hud_on {
            self.render_text(w, h, tf, section, beat, paused);
        }
        self.ctx.swap();
        self.window.request_redraw();

        // glass-aid dump
        self.dump_counter += 1;
        if let Some(d) = self.cfg.dump_frame {
            if self.dump_counter == d {
                self.dump_window();
                elwt.exit();
                return;
            }
        }

        // heartbeat: proves pacing + stays inside a slow serial log
        unsafe {
            static mut HB_LAST: f64 = 0.0;
            let tt = self.time as f64;
            if tt - HB_LAST >= 2.0 {
                HB_LAST = tt;
                eprintln!(
                    "gemdemo: t={tt:.1}s beat={beat:.1} s{section} fps={:.1} frames={}",
                    self.fps, self.dump_counter
                );
            }
        }
    }

    /// Titles + optional HUD, drawn in window pixel space (top-left).
    fn render_text(&mut self, w: u32, h: u32, tf: show::TextFrame, section: usize, beat: f64, paused: bool) {
        let fw = w as f32;
        let fh = h as f32;
        let t = &mut self.text;
        t.clear();

        // centred-text helper (x computed; y given) — the font renders in
        // window-pixel space with a top-left origin
        let put_center =
            |t: &mut font::Writer, y: f32, s: f32, gap: f32, str_: &str, col: (f32, f32, f32)| {
                let width = measure_text(s, gap, str_);
                spaced_text(t, (fw - width) * 0.5, y, s, gap, str_, col);
            };

        // ---- the director's four text slots ----
        if let Some((k, a)) = tf.kicker {
            if a > 0.01 {
                // small eyebrow line above the middle third
                let s = (fw / 150.0).clamp(2.5, 5.0);
                put_center(t, fh * 0.30, s, 2.0, k, tint((0.6, 0.95, 1.0), a));
            }
        }
        if let Some((b, a)) = tf.big {
            if a > 0.01 {
                let s = (fw / 110.0).clamp(5.0, 11.0);
                let y = fh * 0.42;
                // wide-tracked title with a soft underglow + shadow
                let gap = s * 0.7;
                let wdt = measure_text(s, gap, b);
                let x = (fw - wdt) * 0.5;
                // glow (drawn twice offset for cheap blur) + shadow + face
                spaced_text(t, x + 2.0, y + 2.0 + s, s, gap, b, tint((0.05, 0.15, 0.3), a * 0.8));
                spaced_text(t, x, y - 1.0, s, gap, b, tint((0.2, 0.6, 0.9), a * 0.35));
                spaced_text(t, x, y - 2.0, s, gap, b, tint((0.95, 1.0, 1.0), a));
            }
        }
        if let Some((s_, a)) = tf.sub {
            if a > 0.01 {
                let s = (fw / 170.0).clamp(2.2, 4.5);
                put_center(t, fh * 0.565, s, 2.4, s_, tint((0.72, 0.85, 1.0), a));
            }
        }
        if let Some((b, a)) = tf.bottom {
            if a > 0.01 {
                let s = (fw / 160.0).clamp(2.2, 4.5);
                let y = fh * 0.855;
                put_center(t, y, s, 2.2, b, tint((1.0, 0.85, 0.6), a));
                let wdt = measure_text(s, 2.2, b);
                t.quad((fw - wdt) * 0.5, y + 10.0 * s, wdt, 1.0, tint((1.0, 0.7, 0.4), a * 0.6));
            }
        }

        // ---- HUD (info line) — subtle, top-left ----
        if self.hud_on {
            let bar = (beat / 4.0) as u32 % synth::TOTAL_BARS;
            let bar_in =
                (bar - synth::section_start_bar(section as u32)) % synth::SECTION_BARS[section] + 1;
            let s = 2.2;
            let line = format!(
                "EXODUS v{} | {} | {:.0} fps | bar {}/{} | {} BPM{}",
                env!("CARGO_PKG_VERSION"),
                synth::SECTION_NAMES[section],
                self.fps,
                bar_in,
                synth::SECTION_BARS[section],
                synth::BPM as u32,
                if paused { " - PAUSED" } else { "" },
            );
            t.text(10.0, 8.0, s, &line, tint((0.55, 0.75, 0.9), 0.55));
            t.text(
                10.0,
                8.0 + 9.0 * s,
                s,
                "space pause | [ ] chapter | h info | q quit",
                tint((0.4, 0.5, 0.6), 0.35),
            );
        }

        unsafe {
            // font output is premultiplied-ish (rgb*a, a); straight alpha
            // blend so text sits translucent over the scene
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
            t.draw(&self.font, (w, h));
            gl::Disable(gl::BLEND);
        }
    }

    fn dump_window(&self) {
        let (w, h) = (self.phys_w as i32, self.phys_h as i32);
        eprintln!("gemdemo: dumping frame to {:?}", self.cfg.dump_path);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        let mut px = vec![0u8; (w * h * 3) as usize];
        unsafe {
            gl::Finish();
            gl::PixelStorei(gl::PACK_ALIGNMENT, 1);
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            gl::ReadPixels(
                0, 0, w, h, gl::RGBA, gl::UNSIGNED_BYTE, rgba.as_mut_ptr() as *mut _,
            );
        }
        for i in 0..(w * h) as usize {
            px[i * 3] = rgba[i * 4];
            px[i * 3 + 1] = rgba[i * 4 + 1];
            px[i * 3 + 2] = rgba[i * 4 + 2];
        }
        if let Some(p) = &self.cfg.dump_path {
            use std::io::Write;
            if let Ok(mut fh) = std::fs::File::create(p) {
                let _ = write!(fh, "P6\n{w} {h}\n255\n");
                let _ = fh.write_all(&px);
                eprintln!("gemdemo: dumped window {w}x{h} -> {p}");
            }
        }
    }
}

fn tint(c: (f32, f32, f32), a: f32) -> (f32, f32, f32) {
    (c.0 * a, c.1 * a, c.2 * a)
}

fn measure_text(s: f32, gap: f32, str_: &str) -> f32 {
    let mut cx = 0.0;
    for c in str_.chars() {
        cx += match c {
            ' ' => 4.0 * s + gap,
            _ => 6.0 * s + gap,
        };
    }
    cx - gap
}

/// Text at (x, y) with extra tracking `gap` per glyph (titles).
fn spaced_text(
    t: &mut font::Writer,
    x: f32,
    y: f32,
    s: f32,
    gap: f32,
    str_: &str,
    col: (f32, f32, f32),
) {
    let mut cx = x;
    for c in str_.chars() {
        match c {
            ' ' => cx += 4.0 * s + gap,
            other => {
                t.char_glyph(cx, y, s, other, col);
                cx += 6.0 * s + gap;
            }
        }
    }
}

unsafe fn gl_identity() -> (String, String, String) {
    let v = |p: *const u8| -> String {
        let p = p as *const std::os::raw::c_char;
        std::ffi::CStr::from_ptr(p)
            .to_string_lossy()
            .trim()
            .to_string()
    };
    let vendor = v(gl::GetString(gl::VENDOR));
    let renderer = v(gl::GetString(gl::RENDERER));
    let version = v(gl::GetString(gl::VERSION));
    (vendor, renderer, version)
}

fn main() {
    let cfg = match parse_args(&std::env::args().collect::<Vec<String>>()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("gemdemo: {e}");
            std::process::exit(2);
        }
    };

    let clock = Arc::new(Clock::default());
    if cfg.start_section > 0 {
        clock.jump.store(cfg.start_section, Ordering::Relaxed);
    }

    // ---- audio first so beat 0 == frame 0 ----
    let audio = if cfg.silent {
        None
    } else {
        match audio::start(clock.clone()) {
            Ok(stream) => Some(stream),
            Err(e) => {
                eprintln!("gemdemo: audio start failed: {e} — continuing silent");
                None
            }
        }
    };

    // ---- A72 headroom (best-effort; ask the unit + pin the render
    //      thread to cpu8/cpu9 when they come online) ----
    let cpu = cpu::init();

    // ---- window (Wayland client of the nested labwc/gemwl session) ----
    let event_loop = EventLoop::new().expect("event loop");
    let mut builder = WindowBuilder::new().with_title("GEMINI: EXODUS — gemdemo");
    if cfg.windowed {
        builder = builder
            .with_inner_size(winit::dpi::PhysicalSize::new(1024, 576))
            .with_resizable(false);
    } else {
        builder = builder
            .with_fullscreen(Some(Fullscreen::Borderless(None)))
            .with_window_level(WindowLevel::AlwaysOnTop);
    }
    let window = builder.build(&event_loop).expect("window build");

    // ---- EGL: ES 3.1 context on the wl_surface, then load gl ----
    let ctx = glctx::GlCtx::new(&window).expect("egl context");
    ctx.load_gl();
    let (vendor, renderer, version) = unsafe { gl_identity() };
    if !version.starts_with("OpenGL ES 3.") {
        eprintln!("gemdemo: need OpenGL ES 3.x, got: {version} — aborting");
        std::process::exit(1);
    }

    let (phys_w, phys_h) = {
        let s = window.inner_size();
        (s.width.max(1), s.height.max(1))
    };

    let mut state = AppState {
        window,
        ctx,
        cfg,
        clock: clock.clone(),
        stage: unsafe { gfx::Stage::new() },
        dir: show::Director::new(),
        font: unsafe { font::Font::new() },
        text: unsafe { font::Writer::new(4096) },
        hud_on: false,
        fps: 60.0,
        time: 0.0,
        last_instant: Instant::now(),
        phys_w,
        phys_h,
        silent_start: Instant::now(),
        silent_beat_offset: 0.0,
        dump_counter: 0,
    };

    println!(
        "GEMINI: EXODUS v{} — {} / {} — {}",
        env!("CARGO_PKG_VERSION"),
        vendor,
        renderer,
        version
    );
    println!(
        "window {}x{} · {} · A72 {}",
        phys_w,
        phys_h,
        if audio.is_some() {
            "audio: cpal/alsa → gemini16 (S16 @ 44.1k)"
        } else {
            "audio: SILENT (system-time clock)"
        },
        if cpu.a72_online {
            if cpu.pinned_a72 {
                "online + render thread pinned (cpu8/cpu9)"
            } else {
                "online (affinity denied — scheduler)"
            }
        } else {
            "not online — A53s only (try the gemini-a72-up unit)"
        }
    );

    let _audio = audio; // keep the stream alive

    let _ = event_loop.run(move |event, elwt| match event {
        Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } => elwt.exit(),
        Event::WindowEvent {
            event: WindowEvent::RedrawRequested,
            ..
        } => state.frame(elwt),
        Event::WindowEvent {
            event: WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key,
                        logical_key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            },
            ..
        } => {
            match physical_key {
                winit::keyboard::PhysicalKey::Code(KeyCode::BracketRight) => {
                    let s = state.clock.section.load(Ordering::Relaxed) as i32;
                    state.clock.jump.store((s + 1) % 7, Ordering::Relaxed);
                    return;
                }
                winit::keyboard::PhysicalKey::Code(KeyCode::BracketLeft) => {
                    let s = state.clock.section.load(Ordering::Relaxed) as i32;
                    state.clock.jump.store((s + 6) % 7, Ordering::Relaxed);
                    return;
                }
                _ => {}
            }
            match &logical_key {
                Key::Named(NamedKey::Escape) => elwt.exit(),
                Key::Character(c) if c.eq_ignore_ascii_case("q") => elwt.exit(),
                Key::Named(NamedKey::Space) => {
                    state.clock.paused.fetch_xor(true, Ordering::Relaxed);
                }
                Key::Character(c) if c.eq_ignore_ascii_case("h") => state.hud_on = !state.hud_on,
                _ => {}
            }
        }
        _ => {}
    });
}

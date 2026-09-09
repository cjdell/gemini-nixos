//! AETHER — a demoscene + GPU stress test for the Gemini PDA (MT6797X,
//! Mali-T880 MP4 via the geminipda Mesa/panfrost fork, GLES 3.1).
//!
//! Architecture (docs/gemdemo.md):
//!   * audio thread  — cpal (PulseAudio-compat → PipeWire) callback drives
//!                     the synth engine; the engine is the clock master:
//!                     it publishes a fixed-point beat counter + kick
//!                     envelope into atomics each sample.
//!   * render thread — winit event loop on the nested Wayland session;
//!                     each frame reads the beat clock and renders the
//!                     current act into an FBO (ssaa), optional extra
//!                     "stress" raymarch passes, then a post chain
//!                     (bright/blur/bloom, feedback trail, chroma, grain)
//!                     composited to the window; a HUD dashboard reports
//!                     FPS/fill/battery/thermals — the stress readout.
//!
//! Acts (72-bar loop @ 128 BPM, A minor):
//!   0 GENESIS (stars)  1 DESCENT (plasma tunnel)  2 CORE (raymarched
//!   crystal kaleidoscope)  3 SHATTER (synthwave terrain)  4 SURGE
//!   (particle vortex)  5 IGNITION (vortex + logo)  6 AFTERGLOW (stars).

mod audio;
mod font;
mod glctx;
mod glutil;
mod post;
mod raymarch;
mod scenes;
mod sensors;
mod shaders;
mod synth;

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::event::{Event, ElementState, KeyEvent, WindowEvent};
use winit::event_loop::EventLoop;
use winit::keyboard::{Key, KeyCode, NamedKey};
use winit::window::{Fullscreen, WindowBuilder, WindowLevel};

/// The beat clock shared between the audio (writer) and render (reader)
/// threads. Beat units are fixed-point: 1_000_000 per beat.
#[derive(Default)]
pub struct Clock {
    /// Current beat * 1_000_000 (wraps at 72 bars = 288 beats).
    pub beat: AtomicU64,
    /// Kick envelope 0..=65536 (set by the drum synth, decays per sample).
    pub kick: AtomicU32,
    /// Current section 0..6 (published by the audio side; the render side
    /// reads it so visuals and arrangement always agree).
    pub section: AtomicU32,
    /// Pause flag: the audio callback renders silence + freezes the
    /// clock; the render thread freezes its own time.
    pub paused: AtomicBool,
    /// Section jump request (render thread sets; audio consumes). -1 = none.
    pub jump: AtomicI32,
    /// Manual flash trigger (F key); 0..=65536, consumed by the renderer.
    pub flash: AtomicU32,
}

#[derive(Clone)]
pub struct Config {
    pub ssaa: u32,
    pub stress: u32,
    pub bloom: f32,
    pub feedback: f32,
    pub no_hud: bool,
    pub silent: bool,
    pub windowed: bool,
    pub start_section: i32,
    // debug: after this frame #, glReadPixels the presented back buffer
    pub dump_frame: Option<u32>,
    pub dump_path: Option<String>,
    // debug: at dump time, clear the scene FBO + window to this solid
    // colour first (target-path self-test: if these read back correct,
    // the FBO/EGL targets work and any black must be in the draws)
    pub solid: Option<(f32, f32, f32)>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            ssaa: 2,
            stress: 1,
            bloom: 1.0,
            feedback: 0.30,
            no_hud: false,
            silent: false,
            windowed: false,
            start_section: 0,
            dump_frame: None,
            dump_path: None,
            solid: None,
        }
    }
}

fn parse_args(argv: &[String]) -> Result<Config, String> {
    let mut c = Config::default();
    let mut i = 1;
    // Loop contract: consume the flag, then (for value flags) the value
    // at argv[i], advancing past BOTH. The original version advanced only
    // inside the value arms (leaving `i` ON the value -> "unknown flag 2")
    // and not at all in the flag-only arms (100% CPU spin) — neither path
    // had ever been exercised (on-glass 2026-09-08).
    while i < argv.len() {
        let a = &argv[i];
        i += 1; // the flag is consumed
        let missing = || format!("missing value for {a}");
        match a.as_str() {
            "--ssaa" => {
                c.ssaa = argv.get(i).cloned().ok_or_else(missing)?.parse().map_err(|_| "--ssaa wants 1..3")?;
                i += 1;
            }
            "--stress" => {
                c.stress = argv.get(i).cloned().ok_or_else(missing)?.parse().map_err(|_| "--stress wants 0..8")?;
                i += 1;
            }
            "--bloom" => {
                c.bloom = argv.get(i).cloned().ok_or_else(missing)?.parse().map_err(|_| "--bloom wants a float")?;
                i += 1;
            }
            "--feedback" => {
                c.feedback = argv.get(i).cloned().ok_or_else(missing)?.parse().map_err(|_| "--feedback wants a float")?;
                i += 1;
            }
            "--no-hud" => c.no_hud = true,
            "--silent" => c.silent = true,
            "--windowed" => c.windowed = true,
            "--section" => {
                c.start_section = argv.get(i).cloned().ok_or_else(missing)?.parse().map_err(|_| "--section wants 0..6")?;
                i += 1;
            }
            "--dump" => {
                // --dump <frame#> <path>
                c.dump_frame = Some(argv.get(i).cloned().ok_or_else(missing)?.parse().map_err(|_| "--dump wants a frame count")?);
                i += 1;
                c.dump_path = Some(argv.get(i).cloned().ok_or_else(missing)?);
                i += 1;
            }
            "--solid" => {
                // --solid R,G,B — target self-test at dump time
                let v = argv.get(i).cloned().ok_or_else(missing)?;
                i += 1;
                let mut parts = v.split(',');
                let f = |p: &str| p.trim().parse::<f32>().map_err(|_| "--solid wants R,G,B floats");
                let (r, g, b) = (f(parts.next().ok_or("bad --solid")?)?, f(parts.next().ok_or("bad --solid")?)?, f(parts.next().ok_or("bad --solid")?)?);
                c.solid = Some((r / 255.0, g / 255.0, b / 255.0));
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(format!("unknown flag {other} (see --help)")),
        }
    }
    if !(1..=3).contains(&c.ssaa) {
        return Err("--ssaa must be 1..3".into());
    }
    if c.stress > 8 {
        return Err("--stress must be 0..8".into());
    }
    if c.start_section < 0 || c.start_section > 6 {
        return Err("--section must be 0..6".into());
    }
    Ok(c)
}

fn print_help() {
    println!(
        "gemdemo (AETHER) — demoscene + GPU stress for the Gemini PDA
usage: gemdemo [options]
  --ssaa N      main-scene supersampling 1..3        (default 2 — 2560x1440 on the 1280x720 fb)
  --stress N    extra full-res raymarch passes/frame, output discarded (0..8, default 1)
  --bloom F     bloom intensity multiplier           (default 1.0)
  --feedback F  feedback-trail amount 0..0.9         (default 0.30)
  --section N   start at section N (0=genesis .. 6=afterglow)
  --no-hud      hide the stress dashboard
  --silent      no audio (beat clock driven by the system time)
  --windowed    run in a window instead of fullscreen
keys: space=pause  f=flash  h=HUD  [ / ] = previous/next section  q/esc=quit"
    );
}

struct AppState {
    window: winit::window::Window,
    ctx: glctx::GlCtx,
    cfg: Config,
    clock: Arc<Clock>,
    // GL side
    scene: scenes::SceneSet,
    post: post::Post,
    fbo_scene: glutil::Fbo,
    fbo_stress: glutil::Fbo,
    // frame state
    hud_on: bool,
    time: f32, // demo time (s), frozen while paused
    last_instant: Instant,
    fps: f64,
    ms: f64,
    flash: f32,
    act: i32,
    act_started_at: f32,
    sensors: sensors::Sensors,
    last_sensor_poll: Instant,
    // geometry (physical window pixels)
    phys_w: u32,
    phys_h: u32,
    // silent-mode clock origin
    silent_start: Instant,
    silent_beat_offset: f64,
    // rule-0 identity
    gl_version: String,
    // --dump frame counter
    dump_counter: std::sync::atomic::AtomicU32,
}

impl AppState {
    fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 || (w, h) == (self.phys_w, self.phys_h) {
            return;
        }
        self.phys_w = w;
        self.phys_h = h;
        let sw = (w as u64 * self.cfg.ssaa as u64) as u32;
        let sh = (h as u64 * self.cfg.ssaa as u64) as u32;
        self.fbo_scene.resize(sw, sh);
        self.fbo_stress.resize(w, h);
        self.post.resize(w, h);
    }

    fn passes_per_frame(&self) -> u32 {
        // scene act (1-2) + stress + post chain (5 passes + resolve)
        2 + self.cfg.stress + 6
    }

    fn frame(&mut self, _elwt: &winit::event_loop::EventLoopWindowTarget<()>) {
        let now = Instant::now();
        let dt = self.last_instant.elapsed();
        self.last_instant = now;
        if !self.clock.paused.load(Ordering::Relaxed) {
            self.time += dt.as_secs_f32();
            if dt.as_secs_f64() > 0.0 {
                self.fps = self.fps * 0.95 + (1.0 / dt.as_secs_f64()) * 0.05;
                self.ms = self.ms * 0.95 + (dt.as_secs_f64() * 1000.0) * 0.05;
            }
        }

        // Section jump requests (keyboard / --section)
        let jump = self.clock.jump.swap(-1, Ordering::Relaxed);
        if jump >= 0 {
            let beat = jump as f64 * synth::SECTION_BARS[jump as usize] as f64 * 4.0;
            if self.cfg.silent {
                self.silent_beat_offset = beat;
            }
            self.act = -1; // force flash-cut into the new act
        }

        // Sensors at 2 Hz
        if now.duration_since(self.last_sensor_poll) >= Duration::from_millis(500) {
            self.sensors.poll();
            self.last_sensor_poll = now;
        }

        // Beat: audio is the clock master; silent mode drives it from time.
        let mut beat: f64;
        if self.cfg.silent {
            let b = now
                .duration_since(self.silent_start)
                .as_secs_f64()
                * synth::BPM / 60.0
                + self.silent_beat_offset;
            beat = b % (synth::TOTAL_BARS as f64 * 4.0);
            self.clock
                .beat
                .store((beat * 1_000_000.0) as u64, Ordering::Relaxed);
            let sec = synth::section_for_beat(beat);
            self.clock.section.store(sec, Ordering::Relaxed);
        } else {
            beat = self.clock.beat.load(Ordering::Relaxed) as f64 / 1_000_000.0;
        }
        let section = self.clock.section.load(Ordering::Relaxed) as i32;
        let act = scenes::section_act(section);
        if act != self.act {
            self.act = act;
            self.act_started_at = self.time;
            self.flash = 1.0; // demoscene flash-cut on every act change
        }
        self.flash *= 0.82;
        let manual_flash = self.clock.flash.swap(0, Ordering::Relaxed) as f32 / 65536.0;
        self.flash = self.flash.max(manual_flash);

        let kick = if self.cfg.silent {
            let kf = beat.fract();
            if kf < 0.12 {
                (1.0 - kf / 0.12) as f32
            } else {
                0.0
            }
        } else {
            self.clock.kick.load(Ordering::Relaxed) as f32 / 65536.0
        };

        unsafe {
        // 1) main scene into the ssaa FBO
        gl::BindFramebuffer(gl::FRAMEBUFFER, self.fbo_scene.id());
        gl::Viewport(
            0,
            0,
            (self.phys_w * self.cfg.ssaa) as i32,
            (self.phys_h * self.cfg.ssaa) as i32,
        );
        gl::ClearColor(0.0, 0.0, 0.0, 1.0);
        gl::Clear(gl::COLOR_BUFFER_BIT);
        scenes::draw_act(
            &mut self.scene,
            act,
            self.phys_w * self.cfg.ssaa,
            self.phys_h * self.cfg.ssaa,
            self.time,
            beat,
            kick,
            self.flash,
            self.time - self.act_started_at,
            section,
        );
        glutil::check("scene render");

        // 2) stress passes: full-res raymarch, output discarded
        for s in 0..self.cfg.stress {
            gl::BindFramebuffer(gl::FRAMEBUFFER, self.fbo_stress.id());
            self.scene
                .rm
                .draw(self.phys_w, self.phys_h, self.time + 13.7 * s as f32, beat, kick, s as u32, 1.0);
        }
        if self.cfg.stress > 0 {
            glutil::check("stress passes");
        }

        // 3) post: bloom + feedback + FX -> window
        self.post.render(
            &self.fbo_scene,
            self.cfg.bloom,
            self.cfg.feedback,
            self.flash,
            kick,
            self.time,
            self.phys_w,
            self.phys_h,
        );
        glutil::check("post chain");

        // 4) HUD
        if self.hud_on && !self.cfg.no_hud {
            draw_hud(self, kick, beat, section);
            glutil::check("hud");
        }

        self.ctx.swap();
        self.window.request_redraw();
        // frame counter (also surfaced in the heartbeat for debugging)
        let f = self.dump_counter.fetch_add(1, Ordering::Relaxed);
        // --dump N: after N frames, read the presented (back) buffer back
        // and write a PPM — pixel truth without a compositor in the loop
        // (on-glass verification aid, 2026-09-08).
        if let Some(dump) = self.cfg.dump_frame {
            if f == dump {
                eprintln!(
                    "gemdemo: dumping frame {f} to {:?}",
                    self.cfg.dump_path
                );
                unsafe {
                    gl::Finish();
                    if let Some((r, g, b)) = self.cfg.solid {
                        // target self-test: overwrite both targets with the
                        // solid before reading
                        eprintln!("gemdemo: solid self-test {:?}", self.cfg.solid);
                        gl::ClearColor(r, g, b, 1.0);
                        gl::BindFramebuffer(gl::FRAMEBUFFER, self.fbo_scene.id());
                        gl::Clear(gl::COLOR_BUFFER_BIT);
                        gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
                        gl::Clear(gl::COLOR_BUFFER_BIT);
                        gl::Finish();
                    }
                    // window default framebuffer
                    let (w, h) = (self.phys_w as i32, self.phys_h as i32);
                    let mut rgba = vec![0u8; (w * h * 4) as usize];
                    let mut px = vec![0u8; (w * h * 3) as usize];
                    gl::PixelStorei(gl::PACK_ALIGNMENT, 1);
                    gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
                    gl::ReadPixels(
                        0,
                        0,
                        w,
                        h,
                        gl::RGBA,
                        gl::UNSIGNED_BYTE,
                        rgba.as_mut_ptr() as *mut _,
                    );
                    for i in 0..(w * h) as usize {
                        px[i * 3] = rgba[i * 4];
                        px[i * 3 + 1] = rgba[i * 4 + 1];
                        px[i * 3 + 2] = rgba[i * 4 + 2];
                    }
                    // also snapshot the offline stages (scene + post-final)
                    // so a black window is attributable to a stage
                    let stage = |tag: &str, fbo: u32, w: u32, h: u32| {
                        let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
                        let mut q = vec![0u8; (w as usize) * (h as usize) * 3];
                        gl::BindFramebuffer(gl::FRAMEBUFFER, fbo);
                        gl::ReadPixels(
                            0,
                            0,
                            w as i32,
                            h as i32,
                            gl::RGBA,
                            gl::UNSIGNED_BYTE,
                            rgba.as_mut_ptr() as *mut _,
                        );
                        for i in 0..(w as usize * h as usize) {
                            q[i * 3] = rgba[i * 4];
                            q[i * 3 + 1] = rgba[i * 4 + 1];
                            q[i * 3 + 2] = rgba[i * 4 + 2];
                        }
                        if let Some(p) = &self.cfg.dump_path {
                            use std::io::Write;
                            let f2 = format!("{p}.{tag}.ppm");
                            if let Ok(mut fh) = std::fs::File::create(&f2) {
                                let _ = write!(fh, "P6\n{w} {h}\n255\n");
                                let _ = fh.write_all(&q);
                                eprintln!("gemdemo:   stage {tag} {w}x{h} -> {f2}");
                            }
                        }
                    };
                    let (sw, sh) = self.fbo_scene.size();
                    stage("scene", self.fbo_scene.id(), sw, sh);
                    let (pw, ph) = self.post.fbo_final.size();
                    stage("final", self.post.fbo_final.id(), pw, ph);
                    gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
                    if let Some(p) = &self.cfg.dump_path {
                        use std::io::Write;
                        if let Ok(mut fh) = std::fs::File::create(p) {
                            let _ = write!(fh, "P6\n{w} {h}\n255\n");
                            let _ = fh.write_all(&px);
                            eprintln!("gemdemo: dumped window {w}x{h} -> {p}");
                        } else {
                            eprintln!("gemdemo: cannot write {p:?}");
                        }
                    }
                }
                std::process::exit(0);
            }
        }
    }
}
}

/// The HUD (stress dashboard), rendered with the demo's own 5x7 bitmap
/// font — no external assets, demoscene style.
fn draw_hud(state: &mut AppState, kick: f32, beat: f64, section: i32) {
    // gather every state read BEFORE borrowing the HUD writer (it sits inside state.scene)
    let fps = state.fps;
    let ms = state.ms;
    let passes = state.passes_per_frame();
    let paused = state.clock.paused.load(Ordering::Relaxed);
    let cfg = state.cfg.clone();
    let phys_w = state.phys_w;
    let phys_h = state.phys_h;
    let s = state.sensors.clone();

    let si = section as usize; // section index for the constant tables
    let px = 2.0; // font pixel multiplier
    let ox = 12.0;
    let mut oy = 14.0;
    let line = 8.0 * px + 4.0;

    let bar = (beat as u32 / 4) % synth::TOTAL_BARS as u32;
    let step = (beat as u32) % 4; // 16th within the bar
    let energy = scenes::section_energy(section, beat);

    let mut t = &mut state.scene.hud_w;
    t.clear();
    let cyan = (0.55, 1.0, 1.0);
    let dim = (0.5, 0.55, 0.65);
    let amber = (1.0, 0.75, 0.35);
    let green = (0.55, 1.0, 0.5);

    t.text(
        ox,
        oy,
        px,
        &format!("AETHER v{} - MALI-T880 - PANFROST - GLES 3.1", env!("CARGO_PKG_VERSION")),
        cyan,
    );
    oy += line;
    t.text(
        ox,
        oy,
        px,
        &format!(
            "FPS {:4.1} ({:4.1} ms)  {}x{} ssaa{}  {:.1}MP x {} pass/f",
            fps,
            ms,
            phys_w * cfg.ssaa,
            phys_h * cfg.ssaa,
            cfg.ssaa,
            ((phys_w * cfg.ssaa) as f64 * (phys_h * cfg.ssaa) as f64 / 1e6),
            passes,
        ),
        cyan,
    );
    oy += line;
    t.text(
        ox,
        oy,
        px,
        &format!(
            "{} bar {}/{}  {} BPM  16th {}",
            synth::SECTION_NAMES[si],
            bar % synth::SECTION_BARS[si] as u32 + 1,
            synth::SECTION_BARS[si],
            synth::BPM as u32,
            step,
        ),
        amber,
    );
    oy += line;
    if s.battery_present {
        t.text(
            ox,
            oy,
            px,
            &format!(
                "BAT {}% {}  {} mA {}  T0 {}  CPU {}",
                s.battery_pct.map_or("-".into(), |v| v.to_string()),
                s.battery_mv
                    .map_or("-".into(), |v| format!("{:.3}V", v as f32 / 1e6)),
                s.battery_ma.map_or("-".into(), |v| v.to_string()),
                s.charger_state.clone(),
                s.temp_mk
                    .map_or("-".into(), |v| format!("{:.1}C", v as f32 / 1000.0 - 273.15)),
                s.cpus_online.map_or("-".into(), |v| v.to_string()),
            ),
            if s.battery_ma.map_or(false, |v| v < -2500) {
                (1.0, 0.4, 0.4)
            } else {
                green
            },
        );
        oy += line;
    }
    t.text(
        ox,
        oy,
        px,
        &format!(
            "STRESS +{}  BLOOM {:.2}  FB {:.2}{}{}{}",
            cfg.stress,
            cfg.bloom,
            cfg.feedback,
            if paused { "  PAUSED" } else { "" },
            if cfg.silent { "  SILENT" } else { "" },
            if cfg.stress > 0 || cfg.ssaa > 1 { "  GPU BUSY" } else { "" },
        ),
        dim,
    );
    oy += line;
    t.text(
        ox,
        oy,
        px,
        "[space] pause  [f] flash  [h] hud  [ / ] section  [q] quit",
        dim,
    );

    // meters (filled blocks)
    let mw = 150.0;
    oy += line + 4.0;
    let bg = (0.12, 0.13, 0.17);
    t.quad(ox, oy, mw, 7.0, bg);
    t.quad(ox, oy, mw * energy.clamp(0.0, 1.0), 7.0, amber);
    oy += 11.0;
    let n = 16.0;
    let bw = mw / n - 2.0;
    for i in 0..16u32 {
        let on = i <= step;
        let c = if i % 4 == 0 {
            if on {
                (1.0, 1.0, 1.0)
            } else {
                (0.35, 0.37, 0.45)
            }
        } else if on {
            cyan
        } else {
            bg
        };
        t.quad(ox + i as f32 * (mw / n), oy, bw, 7.0, c);
    }
    if kick > 0.05 {
        let k = kick * 0.8 + 0.2;
        t.quad(ox, oy + 9.0, mw, 2.0, (k, k, k));
    }
    unsafe {
        t.draw(&state.scene.font, (phys_w, phys_h));
    }
}

unsafe fn gl_identity() -> (String, String, String) {
    let v = |p: *const u8| -> String {
        // GLubyte* — c_char is i8 on x86_64, u8 on aarch64
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

    // ---- audio: start before the first frame so beat 0 == frame 0 ----
    let audio = if cfg.silent {
        None
    } else {
        match audio::start(clock.clone()) {
            Ok(stream) => Some(stream),
            Err(e) => {
                eprintln!("gemdemo: audio start failed: {e} — continuing silent");
                eprintln!("  (is pipewire-pulse running with XDG_RUNTIME_DIR=/run/gemwl-audio?)");
                None
            }
        }
    };

    // ---- window (Wayland client of the nested labwc/gemwl session) ----
    let event_loop = EventLoop::new().expect("event loop");
    let mut builder = WindowBuilder::new().with_title("AETHER — gemdemo");
    if cfg.windowed {
        builder = builder
            .with_inner_size(winit::dpi::PhysicalSize::new(960, 540))
            .with_resizable(true);
    } else {
        builder = builder
            .with_fullscreen(Some(Fullscreen::Borderless(None)))
            .with_window_level(WindowLevel::AlwaysOnTop);
    }
    let window = builder.build(&event_loop).expect("window build");

    // ---- EGL: ES 3.1 context on the wl_surface, then load the gl facade ----
    let ctx = glctx::GlCtx::new(&window).expect("egl context");
    ctx.load_gl();
    let (vendor, renderer, version) = unsafe { gl_identity() };
    if !version.starts_with("OpenGL ES 3.") {
        eprintln!("gemdemo: need OpenGL ES 3.x, got: {version} — aborting");
        std::process::exit(1);
    }

    let (phys_w, phys_h) = {
        let s = window.inner_size();
        let (w, h) = (s.width.max(1), s.height.max(1));
        (w, h)
    };
    let sw = (phys_w as u64 * cfg.ssaa as u64) as u32;
    let sh = (phys_h as u64 * cfg.ssaa as u64) as u32;

    let mut state = AppState {
        window,
        ctx,
        cfg,
        clock: clock.clone(),
        scene: unsafe { scenes::SceneSet::new() },
        post: unsafe { post::Post::new() },
        fbo_scene: unsafe { glutil::Fbo::new(sw, sh) },
        fbo_stress: unsafe { glutil::Fbo::new(phys_w, phys_h) },
        hud_on: true,
        time: 0.0,
        last_instant: Instant::now(),
        fps: 60.0,
        ms: 16.6,
        flash: 1.0,
        act: -1,
        act_started_at: 0.0,
        sensors: sensors::Sensors::default(),
        last_sensor_poll: Instant::now(),
        phys_w,
        phys_h,
        silent_start: Instant::now(),
        silent_beat_offset: 0.0,
        dump_counter: std::sync::atomic::AtomicU32::new(0),
        gl_version: version.clone(),
    };
    // Post::new() sizes its FBOs 1x1 and state.resize() above no-ops
    // (the phys guard already matches), so a window that never sends a
    // Resized event (fullscreen on gemwl) leaves the whole post chain
    // 1x1 — the resolve then magnifies a single texel to a flat colour
    // (found on-glass 2026-09-08). Force the real size here.
    unsafe {
        state.post.resize(phys_w, phys_h);
    }
    state.sensors.poll();
    state.resize(phys_w, phys_h);

    println!(
        "AETHER v{v} — {vendor} / {renderer} — {version}",
        v = env!("CARGO_PKG_VERSION")
    );
    println!(
        "window {}x{} · scene {}x{} (ssaa{}) · stress +{} · bloom {:.1} · fb {:.2} · {}",
        phys_w,
        phys_h,
        sw,
        sh,
        state.cfg.ssaa,
        state.cfg.stress,
        state.cfg.bloom,
        state.cfg.feedback,
        if audio.is_some() {
            "audio: cpal/pulse → PipeWire (S16 at the AFE)"
        } else {
            "audio: SILENT (system-time clock)"
        }
    );
    println!("battery guard orderly-powers-off below 3.50 V — watch the HUD or plug in USB");

    let _audio = audio; // keep the stream alive

    let _ = event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => elwt.exit(),
            Event::WindowEvent {
                event: WindowEvent::RedrawRequested,
                ..
            } => state.frame(elwt),
            Event::AboutToWait => {
                // one heartbeat line per second: proves the loop + frame
                // pacing are alive when stdout is a tty/pipe
                unsafe {
                    static mut HB_LAST: f64 = 0.0;
                    let t = state.time as f64;
                    if t - HB_LAST >= 1.0 {
                        HB_LAST = t;
                        eprintln!(
                            "gemdemo: heartbeat t={t:.1}s fps={:.1} frames={}",
                            state.fps,
                            state.dump_counter.load(Ordering::Relaxed)
                        );
                    }
                }
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(size),
                ..
            } => state.resize(size.width, size.height),
            Event::WindowEvent {
                event:
                    WindowEvent::KeyboardInput {
                        event: KeyEvent {
                            physical_key,
                            logical_key,
                            state: ElementState::Pressed,
                            ..
                        },
                        ..
                    },
                ..
            } => {
                // layout-independent keys go through the physical key
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
                    Key::Character(c) if c.eq_ignore_ascii_case("f") => {
                        state.clock.flash.store(65536, Ordering::Relaxed);
                    }
                    Key::Character(c) if c.eq_ignore_ascii_case("h") => {
                        state.hud_on = !state.hud_on
                    }
                    _ => {}
                }
            },
            _ => {}
        }
    });
}

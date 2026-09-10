//! The EXODUS spacesynth engine — a real-time music engine in Rust,
//! rewritten for gemdemo 0.2.0. Old engine problems (on glass 2026-09-09:
//! "music completely broken", master rms ~0.88 = the naive per-sample
//! peak-follower limiter flattened EVERY beat to full scale) are fixed by
//! design: a proper mix bus with per-part gains leaving real headroom, a
//! soft-knee ceiling + a fast-attack/slow-release peak limiter that only
//! catches transients, no startup chime, and a composition written as an
//! actual piece rather than one-bar patterns.
//!
//! The score — "GEMINI: EXODUS" — is an epic spacesynth (Koto / Laserdance
//! school): 126 BPM, A minor, a driving sequencer bass, echo-plated
//! chord arpeggios (dotted-8th ping-pong), anthemic analog lead themes
//! and big string pads. 80 bars, seven chapters, shared bar counts with
//! the visual director (show.rs):
//!
//!   S0 EARTH      16 bars  Am F C G Am F C E — pad/drone/arp, countdown
//!   S1 ASCENT      8 bars  Am F C G             — groove enters
//!   S2 WARP       16 bars  Am F C G Am F G E    — full theme
//!   S3 VOID        8 bars  Am F C G             — breakdown, beacon
//!   S4 RENDEZVOUS  8 bars  F G F G              — build
//!   S5 PLANET     16 bars  Am F C G Am F G Am   — anthem finale
//!   S6 ORIGIN      8 bars  Am F C G             — fade to starlight
//!
//! Voices: kick, snare, clap, closed/open hats, crash, tom, sub drone,
//! analog bass (saw+sub), pluck arp, supersaw lead (mono + glide + vib),
//! detuned-saw pads, plus FX: riser, downlifter, reverse crash, impacts,
//! signal blips. FX chain: per-part sends → dotted-8 ping-pong delay +
//! Schroeder reverb, sidechain pump on the kick, soft clip, limiter.
//! Sample-accurate Clock publishing (beat/kick/section atomics) as
//! before. The audio callback thread owns this struct.

use std::sync::atomic::Ordering;

use crate::Clock;

pub const BPM: f64 = 126.0;
pub const STEPS_PER_BAR: u32 = 16;
pub const TOTAL_BARS: u32 = 80;

pub const SECTION_BARS: [u32; 7] = [16, 8, 16, 8, 8, 16, 8];
pub const SECTION_NAMES: [&str; 7] = [
    "EARTH", "ASCENT", "WARP", "VOID", "RENDEZVOUS", "PLANET", "ORIGIN",
];

fn section_for_bar(bar: u32) -> u32 {
    let mut acc = 0;
    for (i, b) in SECTION_BARS.iter().enumerate() {
        acc += *b;
        if bar < acc {
            return i as u32;
        }
    }
    6
}

pub fn section_for_beat(beat: f64) -> u32 {
    let bar = ((beat / 4.0) as u32) % TOTAL_BARS;
    section_for_bar(bar)
}

pub fn section_start_beat(section: u32) -> f64 {
    section_start_bar(section) as f64 * 4.0
}

pub fn section_start_bar(section: u32) -> u32 {
    SECTION_BARS[..section as usize].iter().sum()
}

fn midi_freq(m: i32) -> f32 {
    440.0 * 2f32.powf((m as f32 - 69.0) / 12.0)
}

fn beat_secs(bars: f32) -> f32 {
    bars * 4.0 * 60.0 / BPM as f32
}

// ------------------------------------------------------------------ dsp

/// RBJ biquad.
#[derive(Clone, Copy)]
struct Biquad {
    b0: f32, b1: f32, b2: f32, a1: f32, a2: f32,
    x1: f32, x2: f32, y1: f32, y2: f32,
}

impl Biquad {
    fn new(fc: f32, q: f32, kind: u8, sr: f32) -> Self {
        let w0 = std::f32::consts::PI * 2.0 * fc.max(20.0) / sr;
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * q);
        let (b0, b1, b2) = match kind {
            0 => { let x = (1.0 - cw) * 0.5; (x, 1.0 - cw, x) }
            1 => { let x = (1.0 + cw) * 0.5; (x, -(1.0 + cw), x) }
            _ => (alpha, 0.0, -alpha),
        };
        let a0 = 1.0 + alpha;
        Biquad {
            b0: b0 / a0, b1: b1 / a0, b2: b2 / a0,
            a1: -2.0 * cw / a0, a2: (1.0 - alpha) / a0,
            x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0,
        }
    }
    #[inline]
    fn proc(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1; self.x1 = x;
        self.y2 = self.y1; self.y1 = y;
        y
    }
    fn reset(&mut self) {
        self.x1 = 0.0; self.x2 = 0.0; self.y1 = 0.0; self.y2 = 0.0;
    }
}

/// Deterministic white noise in [-1, 1).
struct Nsg {
    x: u64,
}

impl Nsg {
    fn new() -> Self {
        Nsg { x: 0x9E3779B97F4A7C15 }
    }
    #[inline]
    fn next(&mut self) -> f32 {
        self.x ^= self.x << 13;
        self.x ^= self.x >> 7;
        self.x ^= self.x << 17;
        ((self.x >> 12) % 4096) as f32 / 2048.0 - 1.0
    }
}

// ------------------------------------------------------------------ voices

#[derive(Clone, Copy)]
struct Kick {
    phase: f32,
    t: f32,
    amp: f32,
    active: bool,
}

impl Kick {
    fn new() -> Self {
        Kick { phase: 0.0, t: 0.0, amp: 1.0, active: false }
    }
    fn trigger(&mut self, amp: f32) {
        self.phase = 0.0;
        self.t = 0.0;
        self.amp = amp;
        self.active = true;
    }
    fn render(&mut self, sr: f32, nz: &mut Nsg) -> f32 {
        if !self.active {
            return 0.0;
        }
        let two_pi = std::f32::consts::PI * 2.0;
        // pitch 160→44 Hz exp drop over ~55 ms
        let f = 44.0 + 116.0 * (-28.0 * self.t).exp();
        self.phase += two_pi * f / sr;
        self.phase -= two_pi * (self.phase / two_pi).floor();
        let body = self.phase.sin() * (-18.0 * self.t).exp();
        let click = nz.next() * (-900.0 * self.t).exp() * 0.4;
        self.t += 1.0 / sr;
        if self.t > 0.32 {
            self.active = false;
        }
        (body + click).tanh() * 1.5 * self.amp
    }
}

/// Filtered-noise burst: snare / clap / hat / crash / fx.
#[derive(Clone, Copy)]
struct NoiseBurst {
    t: f32,
    dur: f32,
    amp: f32,
    active: bool,
    f: Biquad,
    kind: u8,
    env: u8, // 0 exp decay, 1 clap bursts, 2 sweep up, 3 reverse
    fc0: f32,
    fc1: f32,
}

impl NoiseBurst {
    fn new(sr: f32) -> Self {
        NoiseBurst {
            t: 0.0, dur: 0.1, amp: 0.5, active: false,
            f: Biquad::new(2000.0, 1.0, 2, sr), kind: 2, env: 0,
            fc0: 2000.0, fc1: 2000.0,
        }
    }
    fn trigger(&mut self, kind: u8, fc: f32, q: f32, dur: f32, amp: f32) {
        self.t = 0.0;
        self.dur = dur;
        self.amp = amp;
        self.active = true;
        self.kind = kind;
        self.env = 0;
        self.fc0 = fc;
        self.fc1 = fc;
        self.f = Biquad::new(fc, q, kind, 48000.0);
    }
    fn trigger_bursts(&mut self, kind: u8, fc: f32, q: f32, dur: f32, amp: f32) {
        self.trigger(kind, fc, q, dur, amp);
        self.env = 1;
    }
    fn trigger_sweep(&mut self, fc0: f32, fc1: f32, dur: f32, amp: f32) {
        self.t = 0.0;
        self.dur = dur;
        self.amp = amp;
        self.active = true;
        self.kind = 2;
        self.env = 2;
        self.fc0 = fc0;
        self.fc1 = fc1;
        self.f = Biquad::new(fc0, 1.1, 2, 48000.0);
    }
    fn trigger_reverse(&mut self, fc: f32, dur: f32, amp: f32) {
        self.t = 0.0;
        self.dur = dur;
        self.amp = amp;
        self.active = true;
        self.kind = 1;
        self.env = 3;
        self.fc0 = fc;
        self.fc1 = fc;
        self.f = Biquad::new(fc, 0.8, 1, 48000.0);
    }
    fn render(&mut self, nz: &mut Nsg, sr: f32) -> f32 {
        if !self.active {
            return 0.0;
        }
        let p = (self.t / self.dur).clamp(0.0, 1.0);
        if self.env == 2 {
            // riser: filter climbs (or falls) over the whole duration
            let f = self.fc0 + (self.fc1 - self.fc0) * p * p;
            self.f = Biquad::new(f, 1.1, 2, sr);
        }
        let x = nz.next();
        let e = match self.env {
            1 => {
                // clap: three decaying bursts
                let b = (self.t * 190.0).floor().min(2.0);
                let bs = [1.0, 0.55, 0.25];
                bs[b as usize] * (-25.0 * self.t).exp()
            }
            2 => {
                let e = (1.0 - p) * 0.3 + 0.7;
                e * (self.t / (self.dur * 0.5)).min(1.0)
            }
            3 => p * p,
            _ => (-self.t / (self.dur * 0.3)).exp(),
        };
        let y = self.f.proc(x) * e * self.amp;
        self.t += 1.0 / sr;
        if self.t > self.dur {
            self.active = false;
        }
        y
    }
}

#[derive(Clone, Copy)]
struct Tom {
    phase: f32,
    t: f32,
    f0: f32,
    amp: f32,
    active: bool,
}

impl Tom {
    fn new() -> Self {
        Tom { phase: 0.0, t: 0.0, f0: 180.0, amp: 0.5, active: false }
    }
    fn trigger(&mut self, f0: f32, amp: f32) {
        self.phase = 0.0;
        self.t = 0.0;
        self.f0 = f0;
        self.amp = amp;
        self.active = true;
    }
    fn render(&mut self, sr: f32) -> f32 {
        if !self.active {
            return 0.0;
        }
        let f = self.f0 * (-6.0 * self.t).exp() + 55.0;
        self.phase += std::f32::consts::PI * 2.0 * f / sr;
        let y = self.phase.sin() * (-14.0 * self.t).exp() * self.amp;
        self.t += 1.0 / sr;
        if self.t > 0.3 {
            self.active = false;
        }
        y
    }
}

/// Sub drone (sine at the root, slow vibrato).
#[derive(Clone, Copy)]
struct Sub {
    phase: f32,
    t: f32,
    f: f32,
    amp: f32,
    active: bool,
}

impl Sub {
    fn new() -> Self {
        Sub { phase: 0.0, t: 0.0, f: 55.0, amp: 0.4, active: false }
    }
    fn trigger(&mut self, f: f32, amp: f32) {
        self.f = f;
        self.phase = 0.0;
        self.t = 0.0;
        self.amp = amp;
        self.active = true;
    }
    fn render(&mut self, sr: f32) -> f32 {
        if !self.active {
            return 0.0;
        }
        let vib = 1.0 + 0.004 * (self.t * 5.0).sin();
        self.phase += std::f32::consts::PI * 2.0 * self.f * vib / sr;
        let atk = (self.t * 200.0).min(1.0);
        let y = self.phase.sin() * atk * self.amp;
        self.t += 1.0 / sr;
        if self.t > 4.0 {
            self.active = false;
        }
        y
    }
}

/// Analog bass: sub sine + saw, lp env.
#[derive(Clone, Copy)]
struct Bass {
    f: f32,
    phase: f32,
    sub_phase: f32,
    t: f32,
    dur: f32,
    amp: f32,
    active: bool,
    lp: Biquad,
}

impl Bass {
    fn new(sr: f32) -> Self {
        Bass {
            f: 55.0, phase: 0.0, sub_phase: 0.0, t: 0.0, dur: 0.3, amp: 0.5,
            active: false, lp: Biquad::new(200.0, 1.0, 0, sr),
        }
    }
    fn trigger(&mut self, freq: f32, dur: f32, amp: f32) {
        self.f = freq;
        self.phase = 0.0;
        self.sub_phase = 0.0;
        self.t = 0.0;
        self.dur = dur;
        self.amp = amp;
        self.active = true;
        self.lp.reset();
    }
    fn render(&mut self, sr: f32) -> f32 {
        if !self.active {
            return 0.0;
        }
        let two_pi = std::f32::consts::PI * 2.0;
        let dt = 1.0 / sr;
        let saw = 2.0 * (self.phase / two_pi) - 1.0;
        self.phase += two_pi * self.f * dt;
        self.sub_phase += two_pi * self.f * dt;
        self.sub_phase -= two_pi * (self.sub_phase / two_pi).floor();
        let sub = self.sub_phase.sin();
        // filter opens from a pluck then settles
        let fc = 120.0 + 1900.0 * (-14.0 * self.t).exp();
        self.lp = Biquad::new(fc, 0.9, 0, sr);
        let x = self.lp.proc(saw * 0.6 + sub * 1.1);
        let a = (self.t * 900.0).min(1.0) * (if self.t > self.dur { ((self.t - self.dur) / 0.05).min(1.0) } else { 0.0 } * -1.0 + 1.0) * self.amp;
        self.t += dt;
        if self.t > self.dur + 0.06 {
            self.active = false;
        }
        x * a
    }
}

/// Pluck (arp): saw through a fast lp decay.
#[derive(Clone, Copy)]
struct Pluck {
    f: f32,
    phase: f32,
    t: f32,
    amp: f32,
    active: bool,
    lp: Biquad,
}

impl Pluck {
    fn new(sr: f32) -> Self {
        Pluck {
            f: 440.0, phase: 0.0, t: 0.0, amp: 0.3, active: false,
            lp: Biquad::new(2600.0, 0.8, 0, sr),
        }
    }
    fn trigger(&mut self, freq: f32, amp: f32) {
        self.f = freq;
        self.phase = 0.0;
        self.t = 0.0;
        self.amp = amp;
        self.active = true;
        self.lp.reset();
    }
    fn render(&mut self, sr: f32) -> f32 {
        if !self.active {
            return 0.0;
        }
        let dt = 1.0 / sr;
        let two_pi = std::f32::consts::PI * 2.0;
        self.phase += two_pi * self.f * dt;
        self.phase -= two_pi * (self.phase / two_pi).floor();
        let saw = 2.0 * (self.phase / two_pi) - 1.0;
        let fc = 1800.0 + 4200.0 * (-60.0 * self.t).exp();
        self.lp = Biquad::new(fc, 0.7, 0, sr);
        let y = self.lp.proc(saw);
        let a = (-22.0 * self.t).exp() * self.amp;
        self.t += dt;
        if self.t > 0.24 {
            self.active = false;
        }
        y * a
    }
}

/// Supersaw lead (4 detuned saws) with per-note env + optional glide.
#[derive(Clone, Copy)]
struct Lead {
    f: f32,
    tf: f32, // glide target
    ph: [f32; 4],
    det: [f32; 4],
    t: f32,
    dur: f32,
    amp: f32,
    active: bool,
    lp: Biquad,
}

impl Lead {
    fn new(sr: f32) -> Self {
        Lead {
            f: 440.0, tf: 440.0, ph: [0.0; 4], det: [0.996, 0.999, 1.001, 1.004],
            t: 0.0, dur: 0.3, amp: 0.3, active: false,
            lp: Biquad::new(3000.0, 0.8, 0, sr),
        }
    }
    fn trigger(&mut self, freq: f32, dur: f32, amp: f32) {
        if self.active {
            // glide from the previous note if it is still ringing
            self.tf = freq;
        } else {
            self.f = freq;
            self.tf = freq;
            self.ph = [0.0; 4];
        }
        self.t = 0.0;
        self.dur = dur;
        self.amp = amp;
        self.active = true;
        self.lp.reset();
    }
    fn render(&mut self, sr: f32) -> f32 {
        if !self.active {
            return 0.0;
        }
        let dt = 1.0 / sr;
        let two_pi = std::f32::consts::PI * 2.0;
        // portamento toward target while the note is young
        self.f += (self.tf - self.f) * (20.0 * dt).min(1.0);
        let vib = 1.0 + 0.006 * ((self.t - 0.18).max(0.0) * 42.0).sin();
        let mut x = 0.0;
        for i in 0..4 {
            let fr = self.f * self.det[i] * vib;
            self.ph[i] += two_pi * fr * dt;
            self.ph[i] -= two_pi * (self.ph[i] / two_pi).floor();
            let p = self.ph[i] / two_pi;
            x += 2.0 * p - 1.0;
        }
        x *= 0.25;
        let fc = 900.0 + 5200.0 * (-24.0 * self.t).exp() * (0.5 + 0.5 * (1.0 - (self.t / self.dur).clamp(0.0, 1.0)));
        self.lp = Biquad::new(fc.clamp(300.0, 6000.0), 0.7, 0, sr);
        let atk = (self.t * 600.0).min(1.0);
        let rel = if self.t > self.dur {
            ((self.t - self.dur) / 0.06).min(1.0)
        } else {
            0.0
        };
        let a = atk * (1.0 - rel) * self.amp;
        self.t += dt;
        if self.t > self.dur + 0.07 {
            self.active = false;
        }
        self.lp.proc(x) * a
    }
}

/// Detuned-saw pad voice (2 saws + chorus LFO), returns (L, R).
#[derive(Clone, Copy)]
struct PadVoice {
    f: f32,
    ph: [f32; 2],
    t: f32,
    dur: f32,
    amp: f32,
    pan: f32,
    active: bool,
    lfo: f32,
    lp: Biquad,
}

impl PadVoice {
    fn new(sr: f32) -> Self {
        PadVoice {
            f: 220.0, ph: [0.0; 2], t: 0.0, dur: 4.0, amp: 0.1, pan: 0.0,
            active: false, lfo: 0.0, lp: Biquad::new(1800.0, 0.7, 0, sr),
        }
    }
    fn trigger(&mut self, freq: f32, dur: f32, amp: f32, pan: f32) {
        self.f = freq;
        self.ph = [0.0, 0.0];
        self.t = 0.0;
        self.dur = dur;
        self.amp = amp;
        self.pan = pan;
        self.active = true;
        self.lp.reset();
    }
    fn render(&mut self, sr: f32) -> (f32, f32) {
        if !self.active {
            return (0.0, 0.0);
        }
        let dt = 1.0 / sr;
        let two_pi = std::f32::consts::PI * 2.0;
        self.lfo += 0.24 * dt;
        let det = 1.0 + 0.005 * self.lfo.sin();
        let mut x = 0.0;
        for i in 0..2 {
            let fr = self.f * if i == 0 { 0.997 * det } else { 1.003 * det };
            self.ph[i] += two_pi * fr * dt;
            self.ph[i] -= two_pi * (self.ph[i] / two_pi).floor();
            x += 2.0 * (self.ph[i] / two_pi) - 1.0;
        }
        x *= 0.5;
        let fc = 1500.0 + 900.0 * (0.5 + 0.5 * self.lfo.sin());
        self.lp = Biquad::new(fc, 0.7, 0, sr);
        let atk = (self.t / 0.5).min(1.0);
        let atk = atk * atk;
        let rel = if self.t > self.dur {
            ((self.t - self.dur) / 1.1).min(1.0)
        } else {
            0.0
        };
        let a = atk * (1.0 - rel) * self.amp;
        self.t += dt;
        if self.t > self.dur + 1.2 {
            self.active = false;
        }
        let y = self.lp.proc(x) * a;
        let gl = y * ((1.0 - self.pan) * 0.5).sqrt();
        let gr = y * ((1.0 + self.pan) * 0.5).sqrt();
        (gl, gr)
    }
}

/// Sine blip (the VOID beacon / UI-ish accents) with fast decay.
#[derive(Clone, Copy)]
struct Blip {
    f: f32,
    phase: f32,
    t: f32,
    amp: f32,
    active: bool,
}

impl Blip {
    fn new() -> Self {
        Blip { f: 880.0, phase: 0.0, t: 0.0, amp: 0.4, active: false }
    }
    fn trigger(&mut self, f: f32, amp: f32) {
        self.f = f;
        self.phase = 0.0;
        self.t = 0.0;
        self.amp = amp;
        self.active = true;
    }
    fn render(&mut self, sr: f32) -> f32 {
        if !self.active {
            return 0.0;
        }
        self.phase += std::f32::consts::PI * 2.0 * self.f / sr;
        let y = self.phase.sin() * (-55.0 * self.t).exp() * self.amp;
        self.t += 1.0 / sr;
        if self.t > 0.3 {
            self.active = false;
        }
        y
    }
}

// ------------------------------------------------------------------ fx

/// Ping-pong dotted-8th delay with damped feedback.
struct Delay {
    buf: [Vec<f32>; 2],
    wr: usize,
    rd_l: usize,
    rd_r: usize,
    fb: f32,
    lp: Biquad,
}

impl Delay {
    fn new(sr: f32) -> Self {
        let tap = ((0.75 * 60.0 / BPM) * sr as f64) as usize;
        let rd_l = (Self::N - tap) % Self::N;
        Delay {
            buf: [vec![0.0; Self::N], vec![0.0; Self::N]],
            wr: 0,
            rd_l,
            rd_r: (rd_l + Self::N - 53) % Self::N,
            fb: 0.42,
            lp: Biquad::new(4800.0, 0.7, 0, sr),
        }
    }
    const N: usize = 65536;
    fn push(&mut self, xl: f32, xr: f32) -> (f32, f32) {
        let dl = self.buf[0][self.rd_l];
        let dr = self.buf[1][self.rd_r];
        // cross-feed + damping in the feedback loop
        let fb_l = self.lp.proc(dr) * self.fb;
        self.buf[0][self.wr] = xl + fb_l;
        self.buf[1][self.wr] = xr + self.lp.proc(dl) * self.fb;
        self.wr = (self.wr + 1) % Self::N;
        self.rd_l = (self.rd_l + 1) % Self::N;
        self.rd_r = (self.rd_r + 1) % Self::N;
        (dl, dr)
    }
    fn flush(&mut self) {
        for b in self.buf.iter_mut() {
            b.fill(0.0);
        }
        self.lp.reset();
    }
}

/// Schroeder reverb (4 combs + 2 allpasses per channel).
struct Reverb {
    combs: [Vec<f32>; 8],
    cpos: [usize; 8],
    allp: [Vec<f32>; 4],
    apos: [usize; 4],
}

impl Reverb {
    fn new() -> Self {
        let comb_lens = [355usize, 271, 229, 197, 331, 257, 223, 191];
        let ap_lens = [61usize, 37, 61, 37];
        Reverb {
            combs: comb_lens.map(|l| vec![0.0; l]),
            cpos: [0; 8],
            allp: ap_lens.map(|l| vec![0.0; l]),
            apos: [0; 4],
        }
    }
    fn flush(&mut self) {
        for b in self.combs.iter_mut() {
            b.fill(0.0);
        }
        for b in self.allp.iter_mut() {
            b.fill(0.0);
        }
    }
    fn process(&mut self, xl: f32, xr: f32) -> (f32, f32) {
        let fb = [0.84, 0.81, 0.78, 0.75, 0.84, 0.81, 0.78, 0.75];
        let mut x = [xl, xr];
        for ch in 0..2 {
            for i in 0..4 {
                let ci = ch * 4 + i;
                let l = self.combs[ci].len();
                let y = x[ch] - fb[ci] * self.combs[ci][self.cpos[ci]];
                self.combs[ci][self.cpos[ci]] = y;
                self.cpos[ci] = (self.cpos[ci] + 1) % l;
                x[ch] += y * 0.42;
            }
            for ai in 0..2 {
                let ci = ch * 2 + ai;
                let l = self.allp[ci].len();
                let d = self.allp[ci][self.apos[ci]];
                let y = x[ch] - 0.5 * d;
                self.allp[ci][self.apos[ci]] = 0.5 * x[ch] + d;
                self.apos[ci] = (self.apos[ci] + 1) % l;
                x[ch] = y;
            }
        }
        (x[0] * 0.22, x[1] * 0.22)
    }
}

// ------------------------------------------------------------------ score

/// Chord tones for a root + quality: [r, 5, 8ve, 3rd+8ve, 5th+8ve, 3rd+2·8ve].
fn chord_tones(root: i32, minor: bool) -> [f32; 6] {
    let t3 = if minor { 3 } else { 4 };
    [
        root as f32,
        (root + 7) as f32,
        (root + 12) as f32,
        (root + 12 + t3) as f32,
        (root + 19) as f32,
        (root + 24 + t3) as f32,
    ]
}

/// Pad voicing: [root-12? no — register around root+12..: r+12, t3+12, 5+12, 7].
fn pad_notes(root: i32, minor: bool) -> [f32; 4] {
    let t3 = if minor { 3 } else { 4 };
    let t7 = if minor { 10 } else { 11 };
    [
        (root + 12) as f32,
        (root + 12 + t3) as f32,
        (root + 19) as f32,
        (root + 12 + t7) as f32,
    ]
}

// Chord progressions per section (2-bar chords). root midi, minor flag.
struct Progression {
    chords: &'static [(i32, bool)],
}

const PROG: [Progression; 7] = [
    Progression { chords: &[(45, true), (41, false), (48, false), (43, false), (45, true), (41, false), (48, false), (40, false)] }, // S0
    Progression { chords: &[(45, true), (41, false), (48, false), (43, false)] },                                                  // S1
    Progression { chords: &[(45, true), (41, false), (48, false), (43, false), (45, true), (41, false), (43, false), (40, false)] }, // S2
    Progression { chords: &[(45, true), (41, false), (48, false), (43, false)] },                                                  // S3
    Progression { chords: &[(41, false), (43, false), (41, false), (43, false)] },                                                // S4
    Progression { chords: &[(45, true), (41, false), (48, false), (43, false), (45, true), (41, false), (43, false), (45, true)] }, // S5
    Progression { chords: &[(45, true), (41, false), (48, false), (43, false)] },                                                  // S6
];

fn prog_for(section: usize) -> &'static [(i32, bool)] {
    PROG[section].chords
}

/// chord for an absolute bar index (2 bars per chord, within its section)
fn chord_for_bar(bar: u32) -> (i32, bool) {
    let section = section_for_bar(bar) as usize;
    let bar_in = (bar - section_start_bar(section as u32)) % SECTION_BARS[section];
    let chords = prog_for(section);
    let i = (bar_in / 2) as usize % chords.len();
    chords[i]
}

/// 2-bar melodic motif per chord, 32 slots of 16ths (0 = rest).
const MOTIF_AM: [i32; 32] = [
    76, 0, 76, 0, 72, 0, 76, 0, 81, 0, 76, 0, 74, 72, 0, 0,
    69, 0, 72, 0, 76, 0, 74, 0, 72, 0, 69, 0, 67, 0, 69, 0,
];
const MOTIF_F: [i32; 32] = [
    69, 0, 69, 0, 69, 0, 72, 0, 74, 0, 76, 0, 77, 0, 76, 0,
    74, 0, 72, 0, 69, 0, 72, 0, 74, 0, 76, 0, 77, 0, 74, 0,
];
const MOTIF_C: [i32; 32] = [
    72, 0, 76, 0, 79, 0, 76, 0, 81, 0, 79, 0, 76, 0, 74, 0,
    72, 0, 74, 0, 76, 0, 79, 0, 81, 0, 79, 0, 76, 0, 72, 0,
];
const MOTIF_G: [i32; 32] = [
    74, 0, 74, 0, 79, 0, 74, 0, 83, 0, 81, 0, 79, 0, 77, 0,
    74, 0, 72, 0, 74, 0, 76, 0, 79, 0, 76, 0, 74, 0, 72, 0,
];
const MOTIF_E: [i32; 32] = [
    71, 0, 71, 0, 76, 0, 71, 0, 80, 0, 76, 0, 71, 0, 71, 0,
    71, 0, 71, 0, 71, 0, 76, 0, 71, 0, 71, 0, 71, 0, 71, 0,
];

/// motif chosen by the chord's root midi
fn motif_for(root: i32) -> &'static [i32; 32] {
    match root {
        45 => &MOTIF_AM,
        41 => &MOTIF_F,
        48 => &MOTIF_C,
        43 => &MOTIF_G,
        40 => &MOTIF_E,
        _ => &MOTIF_AM,
    }
}

// ------------------------------------------------------------------ engine

pub struct Engine {
    beat: f64,
    sr: f32,
    next_step: u64,
    last_chord_bar: i32,
    kick_env: f32,
    lim_peak: [f32; 2],
    sample: u64,

    kick: [Kick; 4],
    kick_i: usize,
    snare: [NoiseBurst; 4],
    snare_i: usize,
    hat: [NoiseBurst; 8],
    hat_i: usize,
    tom: [Tom; 4],
    tom_i: usize,
    fx: [NoiseBurst; 6],
    fx_i: usize,
    sub: [Sub; 2],
    sub_i: usize,
    bass: [Bass; 8],
    bass_i: usize,
    arp: [Pluck; 16],
    arp_i: usize,
    lead: [Lead; 8],
    lead_i: usize,
    pad: [PadVoice; 16],
    pad_i: usize,
    blip: [Blip; 4],
    blip_i: usize,

    delay: Delay,
    reverb: Reverb,
    nz: Nsg,
}

impl Engine {
    pub fn new(sr: f32) -> Self {
        Engine {
            beat: 0.0,
            sr,
            next_step: 1,
            last_chord_bar: -1,
            kick_env: 0.0,
            lim_peak: [0.0, 0.0],
            sample: 0,
            kick: [Kick::new(); 4],
            kick_i: 0,
            snare: [NoiseBurst::new(sr); 4],
            snare_i: 0,
            hat: [NoiseBurst::new(sr); 8],
            hat_i: 0,
            tom: [Tom::new(); 4],
            tom_i: 0,
            fx: [NoiseBurst::new(sr); 6],
            fx_i: 0,
            sub: [Sub::new(); 2],
            sub_i: 0,
            bass: [Bass::new(sr); 8],
            bass_i: 0,
            arp: [Pluck::new(sr); 16],
            arp_i: 0,
            lead: [Lead::new(sr); 8],
            lead_i: 0,
            pad: [PadVoice::new(sr); 16],
            pad_i: 0,
            blip: [Blip::new(); 4],
            blip_i: 0,
            delay: Delay::new(sr),
            reverb: Reverb::new(),
            nz: Nsg::new(),
        }
    }

    pub fn jump_to(&mut self, section: u32) {
        self.beat = section_start_beat(section);
        self.next_step = (self.beat * 4.0) as u64 + 1;
        self.last_chord_bar = -1;
        self.kick_env = 0.0;
        for v in self.kick.iter_mut() { v.active = false; }
        for v in self.snare.iter_mut() { v.active = false; }
        for v in self.hat.iter_mut() { v.active = false; }
        for v in self.tom.iter_mut() { v.active = false; }
        for v in self.fx.iter_mut() { v.active = false; }
        for v in self.sub.iter_mut() { v.active = false; }
        for v in self.bass.iter_mut() { v.active = false; }
        for v in self.arp.iter_mut() { v.active = false; }
        for v in self.lead.iter_mut() { v.active = false; }
        for v in self.pad.iter_mut() { v.active = false; }
        for v in self.blip.iter_mut() { v.active = false; }
        self.delay.flush();
        self.reverb.flush();
        self.lim_peak = [0.0, 0.0];
    }

    // ------------------------------------------------------------- triggers

    fn trigger_kick(&mut self, amp: f32) {
        let len = self.kick.len();
        let k = &mut self.kick[self.kick_i % len];
        self.kick_i = (self.kick_i + 1) % len;
        k.trigger(amp);
        self.kick_env = 1.0;
    }
    fn trigger_snare(&mut self, amp: f32) {
        let len = self.snare.len();
        let s = &mut self.snare[self.snare_i % len];
        self.snare_i = (self.snare_i + 1) % len;
        s.trigger(2, 1900.0, 0.8, 0.16, amp);
    }
    fn trigger_clap(&mut self, amp: f32) {
        let len = self.snare.len();
        let s = &mut self.snare[self.snare_i % len];
        self.snare_i = (self.snare_i + 1) % len;
        s.trigger_bursts(2, 1400.0, 1.2, 0.3, amp);
    }
    fn trigger_hat(&mut self, amp: f32, open: bool) {
        let len = self.hat.len();
        let h = &mut self.hat[self.hat_i % len];
        self.hat_i = (self.hat_i + 1) % len;
        if open {
            h.trigger(1, 8200.0, 0.8, 0.22, amp);
        } else {
            h.trigger(1, 9000.0, 0.8, 0.045, amp);
        }
    }
    fn trigger_crash(&mut self, amp: f32) {
        let len = self.hat.len();
        let h = &mut self.hat[self.hat_i % len];
        self.hat_i = (self.hat_i + 1) % len;
        h.trigger(1, 6500.0, 0.9, 1.3, amp);
    }
    fn trigger_tom(&mut self, f0: f32, amp: f32) {
        let len = self.tom.len();
        let t = &mut self.tom[self.tom_i % len];
        self.tom_i = (self.tom_i + 1) % len;
        t.trigger(f0, amp);
    }
    fn trigger_sub(&mut self, freq: f32, amp: f32) {
        let len = self.sub.len();
        let s = &mut self.sub[self.sub_i % len];
        self.sub_i = (self.sub_i + 1) % len;
        s.trigger(freq, amp);
    }
    fn trigger_bass(&mut self, root: i32, dur: f32, amp: f32) {
        let len = self.bass.len();
        let b = &mut self.bass[self.bass_i % len];
        self.bass_i = (self.bass_i + 1) % len;
        b.trigger(midi_freq(root), dur, amp);
    }
    fn trigger_arp(&mut self, freq: f32, amp: f32) {
        let len = self.arp.len();
        let a = &mut self.arp[self.arp_i % len];
        self.arp_i = (self.arp_i + 1) % len;
        a.trigger(freq, amp);
    }
    fn trigger_lead(&mut self, freq: f32, dur: f32, amp: f32) {
        let len = self.lead.len();
        let l = &mut self.lead[self.lead_i % len];
        self.lead_i = (self.lead_i + 1) % len;
        l.trigger(freq, dur, amp);
    }
    fn trigger_pad(&mut self, chord: (i32, bool)) {
        let dur = beat_secs(2.0) + 0.6;
        let len = self.pad.len();
        for (i, n) in pad_notes(chord.0, chord.1).iter().enumerate() {
            let p = &mut self.pad[(self.pad_i + i) % len];
            let pan = [-0.7, -0.2, 0.2, 0.7][i];
            p.trigger(midi_freq(*n as i32), dur, 0.10, pan);
        }
        self.pad_i = (self.pad_i + 4) % len;
    }
    fn trigger_fx(&mut self, f: impl FnOnce(&mut NoiseBurst)) {
        let len = self.fx.len();
        let fx = &mut self.fx[self.fx_i % len];
        self.fx_i = (self.fx_i + 1) % len;
        f(fx);
    }
    fn trigger_blip(&mut self, freq: f32, amp: f32) {
        let len = self.blip.len();
        let b = &mut self.blip[self.blip_i % len];
        self.blip_i = (self.blip_i + 1) % len;
        b.trigger(freq, amp);
    }

    // ------------------------------------------------------------- patterns
    // 16-step patterns; -1 = rest. Bass values = semitones above the
    // chord root (played an octave under); arp values index chord_tones.

    const BASS_DRIVE: [i32; 16] = [0, -1, 0, -1, 0, -1, 12, -1, 0, -1, 0, -1, 0, -1, 12, -1];
    const BASS_PULSE: [i32; 16] = [0, -1, 12, -1, 0, -1, 12, -1, 0, -1, 12, -1, 0, -1, 12, -1];
    const BASS_GALLOP: [i32; 16] = [0, 0, 12, -1, 0, 0, 12, -1, 7, -1, 12, -1, 0, 0, 12, 12];

    const ARP_SPARSE: [i32; 16] = [0, -1, 2, -1, 3, -1, 2, -1, 1, -1, 2, -1, 4, -1, 2, -1];
    const ARP_RIPPLE: [i32; 16] = [0, 2, 1, 2, 3, 2, 1, 2, 0, 2, 1, 3, 2, 1, 2, 0];
    const ARP_UP: [i32; 16] = [0, -1, 1, -1, 2, -1, 3, -1, 4, -1, 3, -1, 2, -1, 1, -1];

    fn chord_at(&self, bar: u32) -> (i32, bool) {
        chord_for_bar(bar)
    }

    /// Schedule one 16th step.
    fn step(&mut self, stepn: u64) {
        let stepn = stepn as u32;
        let bar = (stepn / STEPS_PER_BAR) % TOTAL_BARS;
        let s = (stepn % STEPS_PER_BAR) as usize;
        let section = section_for_bar(bar) as usize;
        let bar_in = ((bar - section_start_bar(section as u32)) % SECTION_BARS[section]) as usize;
        let (root, minor) = self.chord_at(bar);
        let bar_in_chord = (bar_in % 2) as usize; // 0/1 within the 2-bar chord
        let tones = chord_tones(root, minor);
        let bass_root = root - 12;
        let step_beat = stepn % (STEPS_PER_BAR / 4) == 0; // quarter boundary

        // -------- chord changes: pads on the first bar of each chord
        if s == 0 && bar_in_chord == 0 {
            let b = bar as i32;
            if b != self.last_chord_bar {
                self.last_chord_bar = b;
                match section {
                    // no pads in the silent/beat-less voids
                    _ => self.trigger_pad((root, minor)),
                }
            }
        }

        match section {
            // ================================================ S0 EARTH (ambient)
            0 => {
                let t = bar_in;
                // drone + soft pad harmonics
                if s == 0 {
                    self.trigger_sub(midi_freq(bass_root), 0.30);
                    if t >= 2 {
                        self.trigger_bass(bass_root, beat_secs(2.0), 0.10);
                    }
                }
                // sparse echo arp
                let v = Self::ARP_SPARSE[s];
                if v >= 0 {
                    self.trigger_arp(midi_freq(tones[v as usize] as i32 + 12), 0.09);
                }
                // breathy ticks at the very end of each 2-bar phrase
                if t >= 10 && s == 0 && bar_in_chord == 1 {
                    self.trigger_hat(0.05, false);
                }
                // countdown: soft ticks accelerate, then the riser
                if t == 13 {
                    if step_beat {
                        self.trigger_tom(140.0, 0.12);
                    }
                } else if t == 14 {
                    if s % 4 == 0 {
                        self.trigger_tom(150.0, 0.14);
                    }
                } else if t == 15 {
                    if s % 2 == 0 {
                        self.trigger_tom(160.0 + s as f32 * 2.0, 0.16);
                    }
                    if s == 0 {
                        // one-bar riser into the launch
                        self.trigger_fx(|f| f.trigger_sweep(250.0, 6000.0, beat_secs(1.0), 0.30));
                    }
                }
            }
            // ================================================ S1 ASCENT
            1 => {
                // groove: kick lands from the first bar (launch impact),
                // four-on-floor after the pickup
                if step_beat {
                    let amp = if bar_in < 2 { 0.65 } else { 0.95 };
                    self.trigger_kick(amp);
                }
                if bar_in >= 2 && (s == 4 || s == 12) {
                    self.trigger_clap(0.45);
                }
                // hats: eighths, open on the off of beat 4
                if s % 2 == 0 {
                    self.trigger_hat(0.10, false);
                }
                if s == 14 && bar_in % 2 == 1 {
                    self.trigger_hat(0.12, true);
                }
                // bass: pulsing octaves
                let v = Self::BASS_PULSE[s];
                if v >= 0 {
                    self.trigger_bass(bass_root + v, beat_secs(0.5), 0.42);
                }
                // arp 8ths
                let a = Self::ARP_SPARSE[s];
                if a >= 0 {
                    self.trigger_arp(midi_freq(tones[a as usize] as i32 + 12), 0.13);
                }
                // lift riser at the very end into the warp
                if bar_in == 7 && s == 8 {
                    self.trigger_fx(|f| f.trigger_sweep(250.0, 7000.0, beat_secs(2.0), 0.26));
                }
            }
            // ================================================ S2 WARP
            2 => {
                // full kit, 4/4
                if step_beat {
                    let amp = if s == 0 || s == 8 { 1.0 } else { 0.85 };
                    self.trigger_kick(amp);
                }
                if s == 4 || s == 12 {
                    self.trigger_clap(0.7);
                }
                if s == 14 && bar_in % 2 == 1 {
                    self.trigger_snare(0.25);
                }
                self.trigger_hat(if s % 4 == 0 { 0.13 } else { 0.09 }, s == 14);
                if bar_in == 0 && s == 0 {
                    self.trigger_crash(0.5);
                }
                // 16th gallop bass
                let v = Self::BASS_GALLOP[s];
                if v >= 0 {
                    self.trigger_bass(bass_root + v, beat_secs(0.22), 0.46);
                }
                // shimmering 16th arp under the lead
                let a = Self::ARP_RIPPLE[s];
                if a >= 0 {
                    self.trigger_arp(midi_freq(tones[a as usize] as i32 + 12), 0.10);
                }
                // the anthem theme (2 bars per chord = the motif)
                let m = motif_for(root);
                let n = m[bar_in_chord * 16 + s];
                if n > 0 {
                    self.trigger_lead(midi_freq(n), beat_secs(0.28), 0.42);
                }
                // third-below harmony on the back half — two lead voices
                // make the theme read as an ensemble
                if bar_in >= 8 && n > 0 && s % 2 == 0 {
                    self.trigger_lead(midi_freq(n - 3), beat_secs(0.24), 0.16);
                }
                // fills + section-end energy
                if bar_in == 7 && s >= 12 {
                    self.trigger_snare(0.2 + 0.05 * s as f32);
                }
                if bar_in == 15 {
                    if s == 4 || s == 10 || s == 14 {
                        self.trigger_snare(0.3);
                    }
                    if s == 15 {
                        // downlifter into the void
                        self.trigger_fx(|f| f.trigger_sweep(6000.0, 200.0, beat_secs(1.5), 0.3));
                    }
                }
                if bar_in == 15 && s == 0 {
                    self.trigger_fx(|f| f.trigger_reverse(4000.0, beat_secs(1.2), 0.35));
                }
            }
            // ================================================ S3 VOID
            3 => {
                // near-silence: pad + sparse echo blips + a sub floor
                if s == 0 {
                    self.trigger_sub(midi_freq(bass_root), 0.20);
                    // beacon blip on every bar start (visual sync)
                    self.trigger_blip(midi_freq(76), 0.16);
                }
                // lonely high plucks with echo
                if s == 0 {
                    self.trigger_arp(midi_freq(tones[3] as i32 + 12), 0.10);
                }
                if s == 8 {
                    self.trigger_arp(midi_freq(tones[0] as i32 + 24), 0.07);
                }
                // swells: soft tick patterns enter at the end
                if bar_in >= 5 {
                    if s % 4 == 2 {
                        self.trigger_hat(0.04, false);
                    }
                }
                // build out: riser + roll into the rendezvous
                if bar_in == 7 {
                    if s == 0 {
                        self.trigger_fx(|f| f.trigger_sweep(300.0, 6000.0, beat_secs(2.0), 0.3));
                    }
                    if s >= 10 {
                        self.trigger_snare(0.12 + 0.05 * s as f32);
                    }
                }
            }
            // ================================================ S4 RENDEZVOUS
            4 => {
                if step_beat {
                    let amp = if bar_in < 4 { 0.7 } else { 0.95 };
                    self.trigger_kick(amp);
                }
                if bar_in >= 4 && (s == 4 || s == 12) {
                    self.trigger_clap(0.5);
                }
                if s % 2 == 0 {
                    self.trigger_hat(0.1, false);
                }
                // driving 8th bass
                let v = Self::BASS_DRIVE[s];
                if v >= 0 {
                    self.trigger_bass(bass_root + v, beat_secs(0.4), 0.44);
                }
                // arp grows from sparse to 16ths
                let pat = if bar_in >= 4 { Self::ARP_RIPPLE } else { Self::ARP_SPARSE };
                let a = pat[s];
                if a >= 0 {
                    self.trigger_arp(midi_freq(tones[a as usize] as i32 + 12), 0.12);
                }
                // climbing run into the finale
                if bar_in == 7 {
                    let run = [69, 71, 72, 74, 76, 77, 79, 81];
                    if s % 2 == 0 {
                        self.trigger_lead(midi_freq(run[s / 2]), beat_secs(0.5), 0.3);
                    }
                }
                // crash at the top of S5 handled there
            }
            // ================================================ S5 PLANET
            5 => {
                if step_beat {
                    let amp = if s == 0 { 1.0 } else { 0.88 };
                    self.trigger_kick(amp);
                }
                if s == 4 || s == 12 {
                    self.trigger_clap(0.75);
                }
                if s == 10 && bar_in % 4 == 3 {
                    self.trigger_snare(0.28);
                }
                self.trigger_hat(if s % 4 == 0 { 0.13 } else { 0.08 }, s == 14);
                if bar_in == 0 && s == 0 {
                    self.trigger_crash(0.6);
                    self.trigger_tom(170.0, 0.3);
                }
                // tom fills at each 4-bar mark
                if s == 0 && (bar_in == 4 || bar_in == 8 || bar_in == 12) {
                    self.trigger_tom(190.0, 0.35);
                    self.trigger_tom(140.0, 0.3);
                }
                // gallop bass, accented
                let v = Self::BASS_GALLOP[s];
                if v >= 0 {
                    self.trigger_bass(bass_root + v, beat_secs(0.2), 0.5);
                }
                let a = Self::ARP_UP[s];
                if a >= 0 && bar_in < 14 {
                    self.trigger_arp(midi_freq(tones[a as usize] as i32 + 12), 0.07);
                }
                // the anthem — full section, motif per chord
                let m = motif_for(root);
                let n = m[bar_in_chord * 16 + s];
                if n > 0 {
                    let amp = if bar_in < 8 { 0.44 } else { 0.5 };
                    self.trigger_lead(midi_freq(n), beat_secs(0.26), amp);
                }
                // low-octave counter-melody on the half-bar: the finale
                // lift (same lead voice, an octave down)
                if bar_in >= 4 && n > 0 && (s == 0 || s == 8) {
                    self.trigger_lead(midi_freq(n - 12), beat_secs(1.7), 0.20);
                }
                // timpani under the anthem — tom voice at the low end
                if s == 0 && bar_in % 4 == 0 {
                    self.trigger_tom(70.0, 0.55);
                    self.trigger_tom(105.0, 0.40);
                }
                // octave shimmer above the motif in the second half
                if bar_in >= 8 {
                    let n2 = m[bar_in_chord * 16 + s];
                    if n2 > 0 && s % 2 == 0 {
                        self.trigger_lead(midi_freq(n2 + 12), beat_secs(0.2), 0.16);
                    }
                }
                if bar_in == 15 {
                    if s == 0 {
                        self.trigger_fx(|f| f.trigger_reverse(4000.0, beat_secs(1.6), 0.5));
                    }
                    if s == 12 {
                        self.trigger_fx(|f| f.trigger_sweep(3000.0, 150.0, beat_secs(1.0), 0.28));
                    }
                }
            }
            // ================================================ S6 ORIGIN
            6 => {
                if s == 0 {
                    self.trigger_sub(midi_freq(bass_root), 0.22);
                }
                // soft half-time beat returns, then fades out via master
                if bar_in < 6 {
                    if step_beat {
                        self.trigger_kick(0.5);
                    }
                    if s % 2 == 0 {
                        self.trigger_hat(0.06, false);
                    }
                    if s == 4 || s == 12 {
                        self.trigger_clap(0.25);
                    }
                }
                let a = Self::ARP_SPARSE[s];
                if a >= 0 {
                    self.trigger_arp(midi_freq(tones[a as usize] as i32 + 12), 0.08);
                }
                if s == 0 && bar_in >= 2 {
                    self.trigger_bass(bass_root, beat_secs(2.0), 0.18);
                }
            }
            _ => {}
        }
    }

    /// Render `frames` stereo frames into `out` (interleaved f32).
    pub fn render(&mut self, out: &mut [f32], clock: &Clock) {
        let frames = out.len() / 2;
        let sr = self.sr;

        let jump = clock.jump.swap(-1, Ordering::Relaxed);
        if jump >= 0 {
            self.jump_to(jump as u32);
        }
        let paused = clock.paused.load(Ordering::Relaxed);
        if paused {
            out.fill(0.0);
            return;
        }

        let dt_beat = BPM / 60.0 / sr as f64;
        for i in 0..frames {
            self.beat += dt_beat;
            let cur = (self.beat * 4.0) as u64;
            while self.next_step <= cur {
                self.step(self.next_step);
                self.next_step += 1;
            }

            // ---- voice rendering into buses
            let pump = self.kick_env;
            let mut l = 0.0f32;
            let mut r = 0.0f32;
            let mut dly_l = 0.0f32;
            let mut dly_r = 0.0f32;
            let mut rev_l = 0.0f32;
            let mut rev_r = 0.0f32;

            // drums (dry)
            for k in self.kick.iter_mut() {
                let y = k.render(sr, &mut self.nz);
                l += y * 1.0;
                r += y * 1.0;
            }
            for s_ in self.snare.iter_mut() {
                let y = s_.render(&mut self.nz, sr);
                l += y * 0.85;
                r += y * 0.8;
                rev_l += y * 0.55;
                rev_r += y * 0.6;
            }
            for h in self.hat.iter_mut() {
                let y = h.render(&mut self.nz, sr);
                l += y * 0.5;
                r += y * 0.5;
            }
            for t in self.tom.iter_mut() {
                let y = t.render(sr);
                l += y * 0.8;
                r += y * 0.8;
                rev_l += y * 0.2;
                rev_r += y * 0.2;
            }
            for f in self.fx.iter_mut() {
                let y = f.render(&mut self.nz, sr);
                l += y;
                r += y;
                rev_l += y * 0.35;
                rev_r += y * 0.35;
            }

            // bass (sidechain pump, centred)
            let bs = 1.0 - 0.42 * pump;
            for b in self.bass.iter_mut() {
                let y = b.render(sr) * bs;
                l += y;
                r += y;
            }
            // sub drone
            for s_ in self.sub.iter_mut() {
                let y = s_.render(sr);
                l += y;
                r += y;
            }
            // arp (pumped, heavy delay send — the spacesynth echo)
            let as_ = 1.0 - 0.28 * pump;
            for a in self.arp.iter_mut() {
                let y = a.render(sr) * as_;
                l += y * 0.9;
                r += y;
                dly_l += y * 0.8;
                dly_r += y * 0.85;
                rev_l += y * 0.12;
                rev_r += y * 0.12;
            }
            // lead (delay + reverb)
            let ls = 1.0 - 0.18 * pump;
            for a in self.lead.iter_mut() {
                let y = a.render(sr) * ls;
                l += y;
                r += y;
                dly_l += y * 0.22;
                dly_r += y * 0.2;
                rev_l += y * 0.3;
                rev_r += y * 0.32;
            }
            // pad (heavy pump + reverb)
            let ps = 1.0 - 0.5 * pump;
            for p in self.pad.iter_mut() {
                let (y1, y2) = p.render(sr);
                l += y1 * ps;
                r += y2 * ps;
                rev_l += y1 * 0.6;
                rev_r += y2 * 0.6;
            }
            // blips
            for b in self.blip.iter_mut() {
                let y = b.render(sr);
                l += y;
                r += y;
                dly_l += y * 0.6;
                dly_r += y * 0.6;
            }

            // ---- fx returns
            let (dl, dr) = self.delay.push(dly_l, dly_r);
            l += dl * 0.34;
            r += dr * 0.34;
            let (rvl, rvr) = self.reverb.process(rev_l, rev_r);
            l += rvl * 0.9;
            r += rvr * 0.9;

            // ---- master: headroom + soft ceiling + transient limiter
            let bar = (self.beat / 4.0) as u32 % TOTAL_BARS;
            let section = section_for_bar(bar) as usize;
            let bar_in = ((bar - section_start_bar(section as u32)) % SECTION_BARS[section]) as u32;
            let fade = if section == 6 && bar_in >= 5 {
                ((8.0 - bar_in as f32) / 3.0).clamp(0.0, 1.0) as f32
            } else {
                1.0
            };
            // soft knee: only shapes overs > 0.8
            let soft = |x: f32| {
                let a = x.abs();
                if a <= 0.8 {
                    x
                } else {
                    x.signum() * (0.8 + (a - 0.8).min(0.6) * 0.5)
                }
            };
            let mut gl = soft(l);
            let mut gr = soft(r);
            // fast-attack / ~120 ms-release peak limiter, ceiling 0.97
            let ceil = 0.97f32;
            self.lim_peak[0] = (self.lim_peak[0] * 0.9992).max(gl.abs());
            self.lim_peak[1] = (self.lim_peak[1] * 0.9992).max(gr.abs());
            if self.lim_peak[0] > ceil {
                gl *= ceil / self.lim_peak[0];
            }
            if self.lim_peak[1] > ceil {
                gr *= ceil / self.lim_peak[1];
            }
            let g = 0.95 * fade;
            out[2 * i] = gl * g;
            out[2 * i + 1] = gr * g;

            // clock publishing
            self.kick_env *= (-1.0 / (0.07 * sr)).exp();
            self.sample += 1;
            if self.sample % 4 == 0 {
                clock.kick.store((self.kick_env * 65536.0) as u32, Ordering::Relaxed);
            }
            if self.sample % 64 == 0 {
                let b = self.beat % (TOTAL_BARS as f64 * 4.0);
                clock.beat.store((b * 1_000_000.0) as u64, Ordering::Relaxed);
                clock.section.store(section_for_beat(b), Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_cover_80_bars() {
        let sum: u32 = SECTION_BARS.iter().sum();
        assert_eq!(sum, TOTAL_BARS);
        assert_eq!(section_for_beat(0.0), 0);
        assert_eq!(section_for_beat(63.9), 0);
        assert_eq!(section_for_beat(64.0), 1);
        assert_eq!(section_for_beat(96.0), 2);
        assert_eq!(section_for_beat(160.0), 3);
        assert_eq!(section_for_beat(192.0), 4);
        assert_eq!(section_for_beat(224.0), 5);
        assert_eq!(section_for_beat(288.0), 6);
    }

    #[test]
    fn beat_math() {
        assert!((section_start_beat(2) - 96.0).abs() < 1e-9);
        assert!((section_start_beat(5) - 224.0).abs() < 1e-9);
        assert_eq!(section_for_beat(150.0), 2);
    }

    #[test]
    fn chords_reach_section_roots() {
        // bar 0 = S0 start (Am), bar 56 = S5 start (Am), bar 70 = last
        // S5 chord (Am), bar 78 = S6 last chord (G)
        assert_eq!(chord_for_bar(0), (45, true));
        assert_eq!(chord_for_bar(56), (45, true)); // start of S5
        assert_eq!(chord_for_bar(70), (45, true)); // last chord of S5
        assert_eq!(chord_for_bar(78), (43, false)); // S6 ends on G
    }

    #[test]
    fn engine_runs_one_loop_cleanly() {
        let clock = Clock::default();
        let mut e = Engine::new(44100.0);
        let bars_secs = TOTAL_BARS as usize * 4 * 44100 / (BPM as usize / 60 * 60);
        let _ = bars_secs;
        // render 3 seconds
        let mut buf = vec![0.0f32; 2 * 44100 * 3];
        e.render(&mut buf, &clock);
        // audible, not clipped, and NOT over-limited (the 0.1.0 bug:
        // master rms ~0.88 meant every beat was slammed to full scale)
        let n = buf.len();
        let mut energy = 0.0f32;
        let mut peak = 0.0f32;
        for &v in buf.iter() {
            energy += v * v;
            peak = peak.max(v.abs());
        }
        let rms = (energy / n as f32).sqrt();
        assert!(rms > 0.01, "engine produced silence: rms {rms}");
        assert!(peak <= 1.001, "engine clipped: peak {peak}");
        assert!(rms < 0.45, "engine over-limited (the 0.1.0 bug): rms {rms}");
        // beat clock advanced ~3 s * 126/60 ≈ 6.3 beats
        let b = clock.beat.load(Ordering::Relaxed) as f64 / 1e6;
        assert!(b > 5.5 && b < 8.0, "beat {b}");
    }
}

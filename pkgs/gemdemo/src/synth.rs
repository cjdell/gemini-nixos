//! The AETHER synth engine — a complete real-time music engine in Rust.
//!
//! 72-bar loop @ 128 BPM in A minor (Am–G–F–E, two bars per chord):
//!   S0 GENESIS    8 bars   pad + arp + ghost hats          (bars  0– 7)
//!   S1 DESCENT    8 bars   + bass, hats, kick enters bar 4 (bars  8–15)
//!   S2 CORE      16 bars   full drums, lead riff, pump     (bars 16–31)
//!   S3 SHATTER    8 bars   breakdown: pad/arp/sub only     (bars 32–39)
//!   S4 SURGE      8 bars   build 2: riser + kick at bar 4  (bars 40–47)
//!   S5 IGNITION  16 bars   everything, double-time riff    (bars 48–63)
//!   S6 AFTERGLOW  8 bars   pad + arp, master fade-out      (bars 64–71)
//!
//! Voices: kick (sine pitch-drop + click), clap/snare (banded noise + tone),
//! hats (highpassed noise), bass (saw+sub through an env lowpass), arp
//! (plucked saw), lead (3 detuned pulses), pad (4 detuned-saw voices),
//! sub drop, riser + reverse-cymbal noise FX. FX chain: ping-pong dotted-8th
//! delay + Schroeder reverb, sidechain pump on the kick, tanh soft clip +
//! a peak limiter. The audio callback thread owns this struct; it
//! publishes the beat clock (fixed-point) and the kick envelope to the
//! render thread via `Clock` atomics.

use std::sync::atomic::Ordering;

use crate::Clock;

pub const BPM: f64 = 128.0;
pub const STEPS_PER_BAR: u32 = 16; // sixteenth notes
pub const TOTAL_BARS: u32 = 72;

pub const SECTION_BARS: [u32; 7] = [8, 8, 16, 8, 8, 16, 8];
pub const SECTION_NAMES: [&str; 7] = [
    "GENESIS", "DESCENT", "CORE", "SHATTER", "SURGE", "IGNITION", "AFTERGLOW",
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
    let bar: u32 = SECTION_BARS[..section as usize].iter().sum();
    bar as f64 * 4.0
}

fn midi_freq(m: i32) -> f32 {
    440.0 * 2f32.powf((m as f32 - 69.0) / 12.0)
}

/// RBJ biquad (lowpass/highpass/bandpass) with state.
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
            0 => { let x = (1.0 - cw) * 0.5; (x, 1.0 - cw, x) }             // lowpass
            1 => { let x = (1.0 + cw) * 0.5; (x, -(1.0 + cw), x) }          // highpass
            _ => (alpha, 0.0, -alpha),                                      // bandpass
        };
        let a0 = 1.0 + alpha;
        Biquad {
            b0: b0 / a0, b1: b1 / a0, b2: b2 / a0,
            a1: -2.0 * cw / a0, a2: (1.0 - alpha) / a0,
            x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0,
        }
    }

    fn set_fc(&mut self, fc: f32, q: f32, kind: u8, sr: f32) {
        let n = Self::new(fc, q, kind, sr);
        self.b0 = n.b0; self.b1 = n.b1; self.b2 = n.b2;
        self.a1 = n.a1; self.a2 = n.a2;
    }

    #[inline]
    fn proc(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1; self.x1 = x;
        self.y2 = self.y1; self.y1 = y;
        y
    }

    #[inline]
    fn reset(&mut self) {
        self.x1 = 0.0; self.x2 = 0.0; self.y1 = 0.0; self.y2 = 0.0;
    }
}

/// Deterministic white noise in [-1, 1) (xorshift64, no std::rand).
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

// ---------------------------------------------------------------- voices

#[derive(Clone, Copy)]
struct Kick {
    phase: f32,
    t: f32,
    active: bool,
}

impl Kick {
    fn new() -> Self {
        Kick { phase: 0.0, t: 0.0, active: false }
    }
    fn trigger(&mut self) {
        self.phase = 0.0;
        self.t = 0.0;
        self.active = true;
    }
    fn render(&mut self, sr: f32, nz: &mut Nsg) -> f32 {
        if !self.active {
            return 0.0;
        }
        let f = 42.0 + 150.0 * (-28.0 * self.t).exp();
        self.phase += std::f32::consts::PI * 2.0 * f / sr;
        let two_pi = std::f32::consts::PI * 2.0;
        self.phase -= two_pi * (self.phase / two_pi).floor();
        let body = self.phase.sin() * (-16.0 * self.t).exp() * 1.35;
        let click = nz.next() * (-700.0 * self.t).exp() * 0.35;
        self.t += 1.0 / sr;
        if self.t > 0.35 {
            self.active = false;
        }
        body + click
    }
}

/// A filtered-noise burst (snare/hat/crash/reverse/riser).
#[derive(Clone, Copy)]
struct NoiseBurst {
    sr: f32,
    t: f32,
    dur: f32,
    amp: f32,
    active: bool,
    f: Biquad,
    kind: u8, // 0 lp, 1 hp, 2 bp
    sweep: bool, // riser: fc sweeps 300 -> 8000 over dur
    reverse: bool, // gain ramps up (reverse cymbal)
}

impl NoiseBurst {
    fn new(sr: f32) -> Self {
        NoiseBurst {
            sr,
            t: 0.0, dur: 0.1, amp: 0.5, active: false,
            f: Biquad::new(2000.0, 1.0, 2, sr),
            kind: 2, sweep: false, reverse: false,
        }
    }
    fn trigger(&mut self, kind: u8, fc: f32, q: f32, dur: f32, amp: f32) {
        self.t = 0.0;
        self.dur = dur;
        self.amp = amp;
        self.active = true;
        self.kind = kind;
        self.sweep = false;
        self.reverse = false;
        self.f = Biquad::new(fc, q, kind, self.sr);
    }
    fn trigger_sweep(&mut self, dur: f32, amp: f32) {
        self.t = 0.0;
        self.dur = dur;
        self.amp = amp;
        self.active = true;
        self.kind = 2;
        self.sweep = true;
        self.reverse = false;
        self.f = Biquad::new(300.0, 1.2, 2, self.sr);
    }
    fn trigger_reverse(&mut self, dur: f32, amp: f32) {
        self.t = 0.0;
        self.dur = dur;
        self.amp = amp;
        self.active = true;
        self.kind = 1;
        self.sweep = false;
        self.reverse = true;
        self.f = Biquad::new(3500.0, 0.8, 1, self.sr);
    }
    fn render(&mut self, nz: &mut Nsg) -> f32 {
        if !self.active {
            return 0.0;
        }
        if self.sweep {
            let p = (self.t / self.dur).clamp(0.0, 1.0);
            self.f.set_fc(300.0 + p * p * 7700.0, 1.2, 2, self.sr);
        }
        let x = nz.next();
        let env = if self.reverse {
            let p = (self.t / self.dur).clamp(0.0, 1.0);
            p * p
        } else {
            (-self.t / (self.dur * 0.35)).exp()
        };
        let y = self.f.proc(x) * env * self.amp;
        self.t += 1.0 / self.sr;
        if self.t > self.dur {
            self.active = false;
        }
        y
    }
}

#[derive(Clone, Copy)]
struct Bass {
    f: f32,
    phase: f32,
    sub_phase: f32,
    t: f32,
    dur: f32,
    amp: f32,
    active: bool,
    sub_only: bool,
    lp: Biquad,
}

impl Bass {
    fn new(sr: f32) -> Self {
        Bass {
            f: 55.0, phase: 0.0, sub_phase: 0.0, t: 0.0, dur: 0.2, amp: 0.5,
            active: false, sub_only: false, lp: Biquad::new(400.0, 1.0, 0, sr),
        }
    }
    fn trigger(&mut self, freq: f32, dur: f32, amp: f32, sub_only: bool) {
        self.f = freq;
        self.phase = 0.0;
        self.sub_phase = 0.0;
        self.t = 0.0;
        self.dur = dur;
        self.amp = amp;
        self.active = true;
        self.sub_only = sub_only;
        self.lp.reset();
        self.lp.set_fc(200.0, 1.4, 0, 48000.0);
    }
    fn render(&mut self, sr: f32) -> f32 {
        if !self.active {
            return 0.0;
        }
        let dt = 1.0 / sr;
        let two_pi = std::f32::consts::PI * 2.0;
        let p = self.phase / two_pi;
        let saw = 2.0 * p - 1.0;
        self.phase += self.f * dt;
        self.sub_phase += self.f * dt;
        self.sub_phase -= two_pi * (self.sub_phase / two_pi).floor();
        let sub = self.sub_phase.sin();
        self.lp.set_fc(160.0 + (1500.0 - 160.0) * (-12.0 * self.t).exp(), 1.4, 0, sr);
        let x = if self.sub_only { sub } else { sub * 0.9 + saw * 0.55 };
        let atk = (self.t * 500.0).min(1.0);
        let rel = if self.t > self.dur {
            ((self.t - self.dur) / 0.08).min(1.0)
        } else {
            0.0
        };
        let a = atk * (1.0 - rel) * self.amp;
        self.t += dt;
        if self.t > self.dur + 0.1 {
            self.active = false;
        }
        self.lp.proc(x) * a
    }
}

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
            lp: Biquad::new(3800.0, 0.8, 0, sr),
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
        let p = self.phase / two_pi;
        let saw = 2.0 * p - 1.0;
        self.phase += self.f * dt;
        let a = (-18.0 * self.t).exp() * self.amp;
        self.t += dt;
        if self.t > 0.28 {
            self.active = false;
        }
        self.lp.proc(saw * 0.3) * (a * 4.0)
    }
}

/// Lead: 3 detuned pulse oscillators through a lowpass.
#[derive(Clone, Copy)]
struct Lead {
    f: f32,
    ph: [f32; 3],
    t: f32,
    amp: f32,
    active: bool,
    lp: Biquad,
}

impl Lead {
    fn new(sr: f32) -> Self {
        Lead {
            f: 440.0, ph: [0.0; 3], t: 0.0, amp: 0.3, active: false,
            lp: Biquad::new(4200.0, 0.7, 0, sr),
        }
    }
    fn trigger(&mut self, freq: f32, amp: f32) {
        self.f = freq;
        self.ph = [0.0, 0.0, 0.0];
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
        let ratios = [0.996, 1.0, 1.004];
        let mut x = 0.0;
        for i in 0..3 {
            let fr = self.f * ratios[i];
            self.ph[i] += fr * dt;
            self.ph[i] -= two_pi * (self.ph[i] / two_pi).floor();
            if self.ph[i] / two_pi < 0.25 {
                x += 1.0;
            } else {
                x -= 1.0;
            }
        }
        x /= 3.0;
        let atk = (self.t * 300.0).min(1.0);
        let a = atk * (-9.0 * self.t).exp() * self.amp;
        self.t += dt;
        if self.t > 0.32 {
            self.active = false;
        }
        self.lp.proc(x) * (a * 1.6)
    }
}

#[derive(Clone, Copy)]
struct PadVoice {
    f: f32,
    ph: [f32; 2],
    t: f32,
    dur: f32,
    amp: f32,
    pan: f32, // -1..1
    active: bool,
    lp: Biquad,
    lfo: f32,
}

impl PadVoice {
    fn new(sr: f32) -> Self {
        PadVoice {
            f: 220.0, ph: [0.0; 2], t: 0.0, dur: 3.0, amp: 0.1, pan: 0.0,
            active: false, lp: Biquad::new(1200.0, 0.7, 0, sr), lfo: 0.0,
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
        self.lfo += 0.31 * dt;
        let det = 1.0 + 0.004 * self.lfo.sin();
        let mut x = 0.0;
        for i in 0..2 {
            let fr = self.f * det * if i == 0 { 0.998 } else { 1.002 };
            self.ph[i] += fr * dt;
            self.ph[i] -= two_pi * (self.ph[i] / two_pi).floor();
            let p = self.ph[i] / two_pi;
            x += 2.0 * p - 1.0;
        }
        x *= 0.5;
        let atk = (self.t / 0.35).min(1.0);
        let rel = if self.t > self.dur {
            ((self.t - self.dur) / 0.8).min(1.0)
        } else {
            0.0
        };
        let a = atk * (1.0 - rel) * self.amp;
        self.t += dt;
        if self.t > self.dur + 0.85 {
            self.active = false;
        }
        let y = self.lp.proc(x) * a;
        // equal-power pan
        let gl = y * ((1.0 - self.pan) * 0.5).sqrt();
        let gr = y * ((1.0 + self.pan) * 0.5).sqrt();
        (gl, gr)
    }
}

// ---------------------------------------------------------------- fx

/// Ping-pong delay. TAP = dotted 8th = 0.75 beat (16875 samples @48k).
struct Delay {
    buf: [Vec<f32>; 2],
    wr: usize,
    rd_l: usize,
    rd_r: usize,
    fb: f32,
    tap: usize,
}

impl Delay {
    const N: usize = 32768;

    fn new(sr: f32) -> Self {
        // 0.75 beat at the device's sample rate
        let tap = ((0.75 * 60.0 / BPM) * sr as f64) as usize;
        let rd_l = (Self::N - tap) % Self::N;
        Delay {
            buf: [vec![0.0; Self::N], vec![0.0; Self::N]],
            wr: 0,
            rd_l,
            rd_r: (rd_l + Self::N - 37) % Self::N,
            fb: 0.38,
            tap,
        }
    }

    fn push(&mut self, xl: f32, xr: f32) -> (f32, f32) {
        let dl = self.buf[0][self.rd_l];
        let dr = self.buf[1][self.rd_r];
        self.buf[0][self.wr] = xl + dr * self.fb;
        self.buf[1][self.wr] = xr + dl * self.fb;
        self.wr = (self.wr + 1) % Self::N;
        self.rd_l = (self.rd_l + 1) % Self::N;
        self.rd_r = (self.rd_r + 1) % Self::N;
        (dl, dr)
    }

    fn flush(&mut self) {
        for b in self.buf.iter_mut() {
            b.fill(0.0);
        }
        self.wr = 0;
        let rd_l = (Self::N - self.tap) % Self::N;
        self.rd_l = rd_l;
        self.rd_r = (rd_l + Self::N - 37) % Self::N;
    }
}

/// Schroeder reverb: 4 combs + 2 allpasses per channel.
struct Reverb {
    combs: [Vec<f32>; 8],
    cpos: [usize; 8],
    allp: [Vec<f32>; 4],
    apos: [usize; 4],
}

impl Reverb {
    fn new() -> Self {
        let comb_lens = [257usize, 251, 241, 233, 257, 251, 241, 233];
        let ap_lens = [55usize, 33, 55, 33];
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

    /// Process one sample per channel; returns the wet signal (scaled).
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
                x[ch] += y * 0.4;
            }
            // two allpasses (g = 0.5): y = x - g*d; d' = g*x + d
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
        (x[0] * 0.20, x[1] * 0.20)
    }
}

// ---------------------------------------------------------------- engine

pub struct Engine {
    beat: f64,
    sr: f32,
    next_step: u64,
    last_chord: i32,
    kick_env: f32,

    kick: [Kick; 4],
    kick_i: usize,
    snare: [NoiseBurst; 4],
    snare_i: usize,
    hat: [NoiseBurst; 8],
    hat_i: usize,
    fx: [NoiseBurst; 4],
    fx_i: usize,
    bass: [Bass; 8],
    bass_i: usize,
    arp: [Pluck; 16],
    arp_i: usize,
    lead: [Lead; 16],
    lead_i: usize,
    pad: [PadVoice; 16],
    pad_i: usize,

    delay: Delay,
    reverb: Reverb,
    nz: Nsg,
    master_gain: f32,
    peak: [f32; 2],
    sample: u64,
    // startup chime (on-glass audio debug, 2026-09-09): the first 0.6 s
    // output a descending 880-660-440 Hz tone instead of music, so a
    // listener can immediately tell whether the demo's stream is audible
    // AT ALL ("no music" reports had the whole stream in doubt).
    chime: u32,       // samples remaining
    chime_total: u32, // total chime length (samples)
    chime_seg: u32,   // samples per note
}

impl Engine {
    pub fn new(sr: f32) -> Self {
        Engine {
            beat: 0.0,
            sr,
            next_step: 0,
            last_chord: -1,
            kick_env: 0.0,
            kick: [Kick::new(); 4],
            kick_i: 0,
            snare: [NoiseBurst::new(sr); 4],
            snare_i: 0,
            hat: [NoiseBurst::new(sr); 8],
            hat_i: 0,
            fx: [NoiseBurst::new(sr); 4],
            fx_i: 0,
            bass: [Bass::new(sr); 8],
            bass_i: 0,
            arp: [Pluck::new(sr); 16],
            arp_i: 0,
            lead: [Lead::new(sr); 16],
            lead_i: 0,
            pad: [PadVoice::new(sr); 16],
            pad_i: 0,
            delay: Delay::new(sr),
            reverb: Reverb::new(),
            nz: Nsg::new(),
            master_gain: 0.9,
            peak: [1.0, 1.0],
            sample: 0,
            chime_total: (0.6 * sr) as u32,
            chime: (0.6 * sr) as u32,
            chime_seg: (0.2 * sr) as u32,
        }
    }

    pub fn beat(&self) -> f64 {
        self.beat
    }

    /// Reset to the start of a section (keyboard jump / --section).
    pub fn jump_to(&mut self, section: u32) {
        self.beat = section_start_beat(section);
        self.next_step = (self.beat * 4.0) as u64 + 1;
        self.last_chord = -1;
        self.kick_env = 0.0;
        for v in self.kick.iter_mut() { v.active = false; }
        for v in self.snare.iter_mut() { v.active = false; }
        for v in self.hat.iter_mut() { v.active = false; }
        for v in self.fx.iter_mut() { v.active = false; }
        for v in self.bass.iter_mut() { v.active = false; }
        for v in self.arp.iter_mut() { v.active = false; }
        for v in self.lead.iter_mut() { v.active = false; }
        for v in self.pad.iter_mut() { v.active = false; }
        self.delay.flush();
        self.reverb.flush();
        self.peak = [1.0, 1.0];
    }

    // ------------------------------------------------- arrangement data

    const CHORDS: [[i32; 4]; 4] = [
        [57, 60, 64, 69], // Am
        [55, 59, 62, 67], // G
        [53, 57, 60, 65], // F
        [52, 56, 59, 64], // E
    ];
    const BASS_ROOT: [i32; 4] = [33, 31, 29, 28]; // A1 G1 F1 E1
    const ARP_SEQ: [i32; 16] =
        [0, 7, 12, 7, 12, 7, 0, 7, 12, 19, 12, 7, 12, 19, 24, 19];
    const LEAD_RIFFS: [[i32; 16]; 4] = [
        [69, 0, 72, 69, 76, 0, 72, 69, 67, 0, 64, 67, 69, 0, 64, 0],
        [67, 0, 71, 67, 74, 0, 71, 67, 62, 0, 59, 62, 67, 0, 59, 0],
        [65, 0, 69, 65, 72, 0, 69, 65, 60, 0, 57, 60, 65, 0, 57, 0],
        [64, 0, 68, 64, 71, 0, 68, 64, 59, 0, 56, 59, 64, 0, 56, 0],
    ];
    const HAT_VEL: [f32; 16] = [
        0.85, 0.40, 0.60, 0.40, 0.85, 0.40, 0.60, 0.50,
        0.85, 0.40, 0.60, 0.40, 0.70, 0.50, 0.60, 0.30,
    ];
    const BASS_PAT: [i32; 8] = [0, -1, 0, -1, 0, -1, 0, 12];
    const BASS_GALLOP: [i32; 16] = [0, -1, 0, 12, -1, 0, 12, -1, 0, -1, 0, 12, -1, 0, 12, -1];

    fn trigger_kick(&mut self, amp: f32) {
        let k = &mut self.kick[self.kick_i];
        self.kick_i = (self.kick_i + 1) % 4;
        k.trigger();
        self.kick_env = 1.0;
        // sub thump under the kick
        let b = &mut self.bass[self.bass_i];
        self.bass_i = (self.bass_i + 1) % 8;
        b.trigger(48.0, 0.09, amp * 0.5, true);
    }

    fn trigger_clap(&mut self, amp: f32) {
        let s = &mut self.snare[self.snare_i];
        self.snare_i = (self.snare_i + 1) % 4;
        s.trigger(2, 1800.0, 0.9, 0.14, amp);
    }

    fn trigger_snare(&mut self, amp: f32) {
        let s = &mut self.snare[self.snare_i];
        self.snare_i = (self.snare_i + 1) % 4;
        s.trigger(2, 1900.0, 0.8, 0.12, amp);
    }

    fn trigger_hat(&mut self, amp: f32, open: bool) {
        let h = &mut self.hat[self.hat_i];
        self.hat_i = (self.hat_i + 1) % 8;
        if open {
            h.trigger(1, 6000.0, 0.7, 0.30, amp);
        } else {
            h.trigger(1, 7200.0, 0.7, 0.06, amp);
        }
    }

    fn trigger_bass(&mut self, midi: i32, amp: f32, eighths: bool, sub_only: bool) {
        let b = &mut self.bass[self.bass_i];
        self.bass_i = (self.bass_i + 1) % 8;
        let dur = if eighths { 0.22 } else { 0.6 };
        b.trigger(midi_freq(midi), dur, amp, sub_only);
    }

    fn trigger_arp(&mut self, midi: i32, amp: f32) {
        let a = &mut self.arp[self.arp_i];
        self.arp_i = (self.arp_i + 1) % 16;
        a.trigger(midi_freq(midi), amp);
    }

    fn trigger_lead(&mut self, midi: i32, amp: f32) {
        let l = &mut self.lead[self.lead_i];
        self.lead_i = (self.lead_i + 1) % 16;
        l.trigger(midi_freq(midi), amp);
    }

    fn trigger_pad(&mut self, chord: usize) {
        let notes = Self::CHORDS[chord as usize];
        let pans = [-0.6, -0.2, 0.2, 0.6];
        let dur = (2.0 * 4.0 * 60.0 / BPM) as f32; // 2 bars
        for i in 0..4 {
            let p = &mut self.pad[self.pad_i];
            self.pad_i = (self.pad_i + 1) % 16;
            p.trigger(midi_freq(notes[i]), dur, 0.16, pans[i]);
        }
    }

    /// Schedule one sixteenth step of the arrangement.
    fn step(&mut self, step: u64) {
        let step = step as u32;
        let bar = (step / STEPS_PER_BAR) % TOTAL_BARS;
        let s = step % STEPS_PER_BAR;
        let section = section_for_bar(bar);
        let bar_in = bar % SECTION_BARS[section as usize];
        let chord = (bar / 2) % 4;

        // chord change (every 2 bars): retrigger the pad
        if s == 0 && chord as i32 != self.last_chord {
            self.last_chord = chord as i32;
            if section != 3 {
                self.trigger_pad(chord as usize);
            }
        }

        // section-edge one-shots
        if bar_in == 0 && s == 0 {
            match section {
                2 => {
                    let f = &mut self.fx[self.fx_i];
                    self.fx_i = (self.fx_i + 1) % 4;
                    f.trigger(1, 4500.0, 0.7, 1.4, 0.5);
                }
                3 => {
                    let f = &mut self.fx[self.fx_i];
                    self.fx_i = (self.fx_i + 1) % 4;
                    f.trigger(1, 4200.0, 0.7, 1.2, 0.4);
                }
                5 => {
                    let f = &mut self.fx[self.fx_i];
                    self.fx_i = (self.fx_i + 1) % 4;
                    f.trigger(1, 4500.0, 0.7, 1.6, 0.55);
                    // sub drop
                    let b = &mut self.bass[self.bass_i];
                    self.bass_i = (self.bass_i + 1) % 8;
                    b.trigger(38.0, 0.3, 1.0, true);
                }
                _ => {}
            }
        }
        // reverse cymbal into SHATTER (21 sixteenths before bar 32)
        if section == 2 && bar_in == SECTION_BARS[2] - 1 && s == 11 {
            let f = &mut self.fx[self.fx_i];
            self.fx_i = (self.fx_i + 1) % 4;
            f.trigger_reverse((21.0 * 60.0 / BPM / 4.0) as f32, 0.5);
        }
        // riser into IGNITION (4 bars, starts at SURGE bar 4)
        if section == 4 && bar_in == 4 && s == 0 {
            let f = &mut self.fx[self.fx_i];
            self.fx_i = (self.fx_i + 1) % 4;
            f.trigger_sweep((4.0 * 4.0 * 60.0 / BPM) as f32, 0.6);
        }

        match section {
            // ------------------------------------------------ S0 GENESIS
            0 => {
                if s % 2 == 0 {
                    let n = Self::CHORDS[chord as usize][0]
                        + Self::ARP_SEQ[(s / 2) as usize];
                    self.trigger_arp(n, 0.20);
                }
                if matches!(s, 2 | 6 | 10 | 14) {
                    self.trigger_hat(0.12, false);
                }
            }
            // ------------------------------------------------ S1 DESCENT
            1 => {
                self.trigger_arp(
                    Self::CHORDS[chord as usize][0] + Self::ARP_SEQ[s as usize],
                    0.24,
                );
                if s % 2 == 0 {
                    let d = Self::BASS_PAT[(s / 2) as usize];
                    if d >= 0 {
                        self.trigger_bass(Self::BASS_ROOT[chord as usize] + d, 0.4, true, false);
                    }
                }
                self.trigger_hat(Self::HAT_VEL[s as usize] * 0.55, false);
                if bar_in >= 4 && matches!(s, 0 | 4 | 8 | 12) {
                    self.trigger_kick(0.9);
                }
                if bar_in == 7 && s >= 8 {
                    self.trigger_snare(0.25 + 0.08 * (s - 8) as f32);
                }
            }
            // ------------------------------------------------ S2 CORE
            2 => {
                if matches!(s, 0 | 4 | 8 | 12) {
                    self.trigger_kick(1.0);
                }
                if matches!(s, 4 | 12) {
                    self.trigger_clap(0.8);
                }
                self.trigger_hat(Self::HAT_VEL[s as usize], s == 14);
                if s % 2 == 0 {
                    let d = Self::BASS_PAT[(s / 2) as usize];
                    if d >= 0 {
                        self.trigger_bass(Self::BASS_ROOT[chord as usize] + d, 0.5, true, false);
                    }
                }
                let n = Self::LEAD_RIFFS[chord as usize][s as usize];
                if n != 0 {
                    self.trigger_lead(n, 0.26);
                }
                self.trigger_arp(
                    Self::CHORDS[chord as usize][1] + Self::ARP_SEQ[s as usize],
                    0.12,
                );
            }
            // ------------------------------------------------ S3 SHATTER
            3 => {
                self.trigger_arp(
                    Self::CHORDS[chord as usize][0] + Self::ARP_SEQ[s as usize],
                    0.16,
                );
                if s == 0 || s == 8 {
                    self.trigger_bass(Self::BASS_ROOT[chord as usize] - 12, 0.5, false, true);
                }
            }
            // ------------------------------------------------ S4 SURGE
            4 => {
                self.trigger_arp(
                    Self::CHORDS[chord as usize][0] + Self::ARP_SEQ[s as usize],
                    0.22,
                );
                if s % 2 == 0 {
                    let d = Self::BASS_PAT[(s / 2) as usize];
                    if d >= 0 {
                        self.trigger_bass(Self::BASS_ROOT[chord as usize] + d, 0.45, true, false);
                    }
                }
                if bar_in >= 4 {
                    self.trigger_hat(Self::HAT_VEL[s as usize] * 0.8, s == 14);
                    if matches!(s, 0 | 4 | 8 | 12) {
                        self.trigger_kick(0.95);
                    }
                }
                if bar_in == 7 && s >= 8 {
                    self.trigger_snare(0.25 + 0.08 * (s - 8) as f32);
                }
            }
            // ------------------------------------------------ S5 IGNITION
            5 => {
                if matches!(s, 0 | 4 | 8 | 12) {
                    self.trigger_kick(1.0);
                }
                if matches!(s, 4 | 12) {
                    self.trigger_clap(0.85);
                }
                if s == 10 {
                    self.trigger_snare(0.35); // ghost
                }
                self.trigger_hat(Self::HAT_VEL[s as usize], s == 6 || s == 14);
                {
                    // galloping 16th bass
                    let d = Self::BASS_GALLOP[s as usize];
                    if d >= 0 {
                        self.trigger_bass(Self::BASS_ROOT[chord as usize] + d, 0.5, true, false);
                    }
                }
                {
                    // double-time riff: up to two notes per sixteenth
                    let riff = Self::LEAD_RIFFS[chord as usize];
                    let i1 = (s * 2) % 16;
                    let i2 = (s * 2 + 1) % 16;
                    if riff[i1 as usize] != 0 {
                        self.trigger_lead(riff[i1 as usize], 0.22);
                    }
                    if riff[i2 as usize] != 0 && s % 2 == 1 {
                        self.trigger_lead(riff[i2 as usize], 0.16);
                    }
                }
            }
            // ------------------------------------------------ S6 AFTERGLOW (and anything unexpected)
            6 | _ => {
                if s % 2 == 0 {
                    let n = Self::CHORDS[chord as usize][0]
                        + Self::ARP_SEQ[(s / 2) as usize];
                    self.trigger_arp(n, 0.14);
                }
                if bar_in >= 4 {
                    if s == 0 || s == 8 {
                        self.trigger_bass(Self::BASS_ROOT[chord as usize] - 12, 0.35, false, true);
                    }
                    if matches!(s, 2 | 6 | 10 | 14) {
                        self.trigger_hat(0.1, false);
                    }
                }
            }
        }
    }

    /// Render `frames` stereo frames into `out` (interleaved f32).
    /// `paused` freezes the clock and mutes. `clock` publishes beat/kick.
    pub fn render(&mut self, out: &mut [f32], clock: &Clock) {
        let frames = out.len() / 2;

        // consume a section jump (cheap: once per callback chunk)
        let jump = clock.jump.swap(-1, Ordering::Relaxed);
        if jump >= 0 {
            self.jump_to(jump as u32);
        }
        let paused = clock.paused.load(Ordering::Relaxed);
        if paused {
            out.fill(0.0);
            return;
        }

        let dt_beat = BPM / 60.0 / self.sr as f64;
        for i in 0..frames {
            self.beat += dt_beat;
            // step scheduling
            let cur = (self.beat * 4.0) as u64;
            while self.next_step <= cur {
                self.step(self.next_step);
                self.next_step += 1;
            }

            // ---- render voices
            let pump = self.kick_env;
            let mut l = 0.0;
            let mut r = 0.0;
            let mut dly_l = 0.0;
            let mut dly_r = 0.0;
            let mut rev_l = 0.0;
            let mut rev_r = 0.0;
            // drums (dry, hard)
            for k in self.kick.iter_mut() {
                l += k.render(self.sr, &mut self.nz);
            }
            for s in self.snare.iter_mut() {
                let y = s.render(&mut self.nz);
                l += y;
                r += y * 0.9;
                rev_l += y * 0.5;
                rev_r += y * 0.55;
            }
            for h in self.hat.iter_mut() {
                let y = h.render(&mut self.nz);
                l += y * 0.9;
                r += y;
                rev_l += y * 0.15;
                rev_r += y * 0.2;
            }
            for f in self.fx.iter_mut() {
                let y = f.render(&mut self.nz);
                l += y;
                r += y;
                rev_l += y * 0.4;
                rev_r += y * 0.4;
            }
            // bass (pumped)
            let bs = 1.0 - 0.35 * pump;
            for b in self.bass.iter_mut() {
                let y = b.render(self.sr) * bs;
                l += y;
                r += y;
            }
            // arp (pumped, delay send)
            let as_ = 1.0 - 0.30 * pump;
            for a in self.arp.iter_mut() {
                let y = a.render(self.sr) * as_;
                l += y * 0.9;
                r += y;
                dly_l += y * 0.5;
                dly_r += y * 0.55;
            }
            // lead (pumped, delay + reverb sends)
            let ls = 1.0 - 0.25 * pump;
            for a in self.lead.iter_mut() {
                let y = a.render(self.sr) * ls;
                l += y * 0.95;
                r += y * 0.95;
                dly_l += y * 0.35;
                dly_r += y * 0.3;
                rev_l += y * 0.3;
                rev_r += y * 0.35;
            }
            // pad (pumped hard, reverb send)
            let ps = 1.0 - 0.45 * pump;
            for p in self.pad.iter_mut() {
                let (y1, y2) = p.render(self.sr);
                l += y1 * ps;
                r += y2 * ps;
                rev_l += y1 * 0.5;
                rev_r += y2 * 0.5;
            }

            // ---- fx
            let (dl2, dr2) = self.delay.push(dly_l, dly_r);
            l += dl2 * 0.22;
            r += dr2 * 0.24;
            let (rv_l, rv_r) = self.reverb.process(rev_l, rev_r);
            l += rv_l;
            r += rv_r;

            // ---- master: soft clip + limiter + outro fade
            l = l.tanh() * 1.15;
            r = r.tanh() * 1.15;
            let bar = (self.beat / 4.0) as u32 % TOTAL_BARS;
            let section = section_for_bar(bar);
            let bar_in = bar % SECTION_BARS[section as usize];
            let fade = if section == 6 && bar_in >= 6 {
                ((8.0 - bar_in as f32) / 2.0).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let g = self.master_gain * fade;
            self.peak[0] = self.peak[0].max(l.abs());
            self.peak[1] = self.peak[1].max(r.abs());
            let gl = if self.peak[0] > 1.0 { 1.0 / self.peak[0] } else { 1.0 };
            let gr = if self.peak[1] > 1.0 { 1.0 / self.peak[1] } else { 1.0 };
            out[2 * i] = l * gl * g;
            out[2 * i + 1] = r * gr * g;
            self.peak[0] *= 0.9997;
            self.peak[1] *= 0.9997;

            // startup chime override (see struct comment)
            if self.chime > 0 {
                self.chime -= 1;
                let el = self.chime_total - self.chime; // elapsed
                let f = [880.0, 660.0, 440.0, 220.0][(el / self.chime_seg) as usize % 4];
                let se = el % self.chime_seg;
                let seglen = self.chime_seg;
                let ramp = (if se < 400 {
                    se as f32 / 400.0
                } else if se > seglen - 400 {
                    (seglen - se) as f32 / 400.0
                } else {
                    1.0
                });
                let v = 0.4 * ramp
                    * (std::f32::consts::TAU * f * el as f32 / self.sr).sin();
                out[2 * i] = v;
                out[2 * i + 1] = v;
            }

            // ---- clock publishing
            self.kick_env *= (-1.0 / (0.05 * self.sr)).exp();
            self.sample += 1;
            if self.sample % 4 == 0 {
                clock
                    .kick
                    .store((self.kick_env * 65536.0) as u32, Ordering::Relaxed);
            }
            if self.sample % 64 == 0 {
                let b = self.beat % (TOTAL_BARS as f64 * 4.0);
                clock
                    .beat
                    .store((b * 1_000_000.0) as u64, Ordering::Relaxed);
                clock
                    .section
                    .store(section_for_beat(b), Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_cover_72_bars() {
        let sum: u32 = SECTION_BARS.iter().sum();
        assert_eq!(sum, TOTAL_BARS);
        assert_eq!(section_for_bar(0), 0);
        assert_eq!(section_for_bar(7), 0);
        assert_eq!(section_for_bar(8), 1);
        assert_eq!(section_for_bar(15), 1);
        assert_eq!(section_for_bar(16), 2);
        assert_eq!(section_for_bar(31), 2);
        assert_eq!(section_for_bar(32), 3);
        assert_eq!(section_for_bar(39), 3);
        assert_eq!(section_for_bar(40), 4);
        assert_eq!(section_for_bar(47), 4);
        assert_eq!(section_for_bar(48), 5);
        assert_eq!(section_for_bar(63), 5);
        assert_eq!(section_for_bar(64), 6);
        assert_eq!(section_for_bar(71), 6);
    }

    #[test]
    fn beat_math() {
        assert!((section_start_beat(2) - 64.0).abs() < 1e-9);
        assert!((section_start_beat(5) - 192.0).abs() < 1e-9);
        assert_eq!(section_for_beat(100.0), 2); // bar 25 = CORE
        assert_eq!(section_for_beat(250.0), 5); // bar 62 = IGNITION
    }

    #[test]
    fn engine_runs_one_bar() {
        let clock = Clock::default();
        let mut e = Engine::new(48000.0);
        let mut buf = vec![0.0f32; 2 * 48000 * 2]; // 2 s stereo
        e.render(&mut buf, &clock);
        // something must have been produced (pad + arp + hats)
        let energy: f32 = buf.iter().map(|s| s.abs()).sum::<f32>() / buf.len() as f32;
        assert!(energy > 0.001, "engine produced silence: {energy}");
        // and it must not clip
        assert!(buf.iter().all(|s| s.abs() <= 1.0001));
        // beat clock advanced ~2 s * 128/60 = ~4.27 beats
        let b = clock.beat.load(Ordering::Relaxed) as f64 / 1e6;
        assert!(b > 3.5 && b < 5.0, "beat {b}");
    }
}

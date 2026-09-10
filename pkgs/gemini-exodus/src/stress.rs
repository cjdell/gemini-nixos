//! stress.rs — the GPU stress-test load model + benchmark capture.
//!
//! Two jobs, both deliberately kept free of GL so they are unit-testable
//! on the host:
//!
//! 1. `Stress` — a named load level. The renderer asks it for multipliers
//!    (star count, warp-streak count, particle emission, debris field,
//!    nebula overdraw) instead of hard-coding demoscene quantities. Level
//!    0 is the classic Director's-Cut look; 1..3 progressively push the
//!    T880 toward its fragment/vertex/raster limits. Nothing here adds a
//!    render pass — every level is more of the SAME single-pass layers,
//!    which is the only way to stay 60 fps on this GPU (0.1.0's multipass
//!    AETHER is the cautionary receipt).
//!
//! 2. `Stats` — a rolling frametime ring for the on-screen HUD plus a
//!    full per-chapter capture for `--bench`. The benchmark's headline is
//!    not the mean (a demo that averages 60 can still stutter) but the
//!    p1 (99th-percentile slowest) frame and the worst frame, reported per
//!    chapter so a heavy reveal (S5 PLANET) can be told apart from a cheap
//!    starfield.
//!
//! Frametime is what we measure (1/fps, milliseconds) because it is the
//! quantity the compositor's vsync actually constrains; fps is derived for
//! display only.

/// Highest stress level we define.
pub const MAX_LEVEL: u8 = 3;

/// Max stars allocated at startup (level 3 draws them all).
pub const STAR_CAP: usize = 5_400; // 1_800 * 3
/// Max warp streaks allocated at startup (level 3 draws them all).
pub const WARP_CAP: usize = 6_000; // 2_400 * 2.5
/// Sprite VBO capacity in quads (stars+glows+particles in one frame).
pub const SPRITE_QUAD_CAP: usize = 6_000;
/// Max drifting debris shards allocated at startup.
pub const DEBRIS_CAP: usize = 340;
/// Hard cap on live particles. Emission and bursts are scaled by the
/// stress level, and all particles share one `Sprites::draw` call, so this
/// must stay below `SPRITE_QUAD_CAP` or `DynVbo::update` panics (overflow)
/// mid-benchmark. 200 quads of margin; see gfx.rs.
pub const PART_CAP: usize = SPRITE_QUAD_CAP - 200;

/// A named graphics load profile.
#[derive(Clone, Copy)]
pub struct Stress {
    pub level: u8,
    /// Fraction of the max star field drawn.
    pub star_mult: f32,
    /// Fraction of the max warp-streak field drawn.
    pub warp_mult: f32,
    /// Particle emission / burst multiplier.
    pub part_mult: f32,
    /// Number of drifting debris rocks (extra sprite layer).
    pub debris: usize,
    /// Nebula-sprite overdraw multiplier.
    pub nebula_mult: f32,
}

impl Stress {
    pub const fn off() -> Self {
        Stress {
            level: 0,
            star_mult: 1.0,
            warp_mult: 1.0,
            part_mult: 1.0,
            debris: 0,
            nebula_mult: 1.0,
        }
    }

    pub fn from_level(level: u8) -> Self {
        let level = level.min(MAX_LEVEL);
        match level {
            0 => Self::off(),
            1 => Stress {
                level,
                star_mult: 1.6,
                warp_mult: 1.4,
                part_mult: 1.8,
                debris: 90,
                nebula_mult: 1.4,
            },
            2 => Stress {
                level,
                star_mult: 2.4,
                warp_mult: 1.9,
                part_mult: 3.0,
                debris: 200,
                nebula_mult: 1.9,
            },
            _ => Stress {
                level,
                star_mult: 3.0,
                warp_mult: 2.5,
                part_mult: 4.5,
                debris: 340,
                nebula_mult: 2.4,
            },
        }
    }

    pub fn label(&self) -> &'static str {
        match self.level {
            0 => "off",
            1 => "load-1",
            2 => "load-2",
            _ => "MAX",
        }
    }

    pub fn active(&self) -> bool {
        self.level > 0
    }

    /// Stars to draw this frame (clamped to the allocated field).
    pub fn star_count(&self) -> usize {
        ((STAR_CAP as f32 / 3.0 * self.star_mult) as usize).min(STAR_CAP)
    }

    /// Warp streaks to draw this frame (clamped to the allocated field).
    pub fn warp_count(&self) -> usize {
        ((WARP_CAP as f32 / 2.5 * self.warp_mult) as usize).min(WARP_CAP)
    }
}

impl Default for Stress {
    fn default() -> Self {
        Self::off()
    }
}

// ---------------------------------------------------------------- stats

/// Rolling + captured frametime statistics.
pub struct Stats {
    ring: [f32; Self::RING],
    ri: usize,
    frames: u64,
    /// EMA of instantaneous fps, for the HUD.
    pub fps_ema: f32,
    capture: Vec<f32>,
    per_chapter: [Vec<f32>; 7],
    chapter_now: usize,
    pub collecting: bool,
}

impl Stats {
    pub const RING: usize = 240; // ~4 s at 60 fps

    pub fn new() -> Self {
        Stats {
            ring: [0.0; Self::RING],
            ri: 0,
            frames: 0,
            fps_ema: 60.0,
            capture: Vec::with_capacity(4096),
            per_chapter: [
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ],
            chapter_now: 0,
            collecting: false,
        }
    }

    /// Record one frame's wall time (seconds). Ignores absurd values
    /// (first frame after a stall, pause/resume).
    pub fn push(&mut self, dt: f32) {
        let ms = dt * 1000.0;
        if !ms.is_finite() || ms <= 0.0 || ms > 5_000.0 {
            return;
        }
        self.ring[self.ri % Self::RING] = ms;
        self.ri += 1;
        let inst = 1000.0 / ms;
        self.fps_ema += (inst - self.fps_ema) * 0.08;
        self.frames += 1;
        if self.collecting {
            self.capture.push(ms);
            if self.chapter_now < 7 {
                self.per_chapter[self.chapter_now].push(ms);
            }
        }
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Most recent frame time (ms).
    pub fn last_ms(&self) -> f32 {
        if self.ri == 0 {
            return 0.0;
        }
        self.ring[(self.ri + Self::RING - 1) % Self::RING]
    }

    /// The rolling history, newest-last (wraps; index via `ri`).
    pub fn ring(&self) -> &[f32; Self::RING] {
        &self.ring
    }

    /// Total pushes so far; the valid ring span is `ri.min(RING)`, stored
    /// oldest-to-newest starting at `(ri + RING - n) % RING`.
    pub fn ring_index(&self) -> usize {
        self.ri
    }

    /// Oldest→newest iterator over the valid ring entries, for the graph.
    /// (Unused by the binary — kept for tests/tools.)
    #[allow(dead_code)]
    pub fn series(&self) -> Vec<f32> {
        let n = self.ri.min(Self::RING);
        let mut v = Vec::with_capacity(n);
        for k in 0..n {
            let idx = (self.ri + Self::RING - n + k) % Self::RING;
            v.push(self.ring[idx]);
        }
        v
    }

    /// Route subsequent samples to chapter `c`. Does NOT clear — the
    /// per-chapter buffers are only reset by `start_capture` (otherwise a
    /// per-frame call would wipe the benchmark data it is collecting).
    pub fn set_chapter(&mut self, c: usize) {
        self.chapter_now = c.min(6);
    }

    pub fn start_capture(&mut self) {
        self.collecting = true;
        self.capture.clear();
        for v in self.per_chapter.iter_mut() {
            v.clear();
        }
    }

    pub fn stop_capture(&mut self) {
        self.collecting = false;
    }

    pub fn summary(&self) -> Summary {
        Summary::of(&self.capture)
    }

    pub fn chapter_summary(&self, c: usize) -> Summary {
        if c < 7 {
            Summary::of(&self.per_chapter[c])
        } else {
            Summary::default()
        }
    }

    #[allow(dead_code)]
    pub fn chapter_frames(&self, c: usize) -> usize {
        if c < 7 {
            self.per_chapter[c].len()
        } else {
            0
        }
    }
}

impl Default for Stats {
    fn default() -> Self {
        Self::new()
    }
}

/// Frametime summary. fps fields are derived; the ms fields are the truth.
#[derive(Clone, Copy, Default)]
pub struct Summary {
    pub frames: usize,
    pub avg_ms: f32,
    pub avg_fps: f32,
    pub p1_ms: f32,
    pub p1_fps: f32,
    pub worst_ms: f32,
}

impl Summary {
    pub fn of(v: &[f32]) -> Summary {
        if v.is_empty() {
            return Summary::default();
        }
        let mut sorted = v.to_vec();
        let mut sum = 0.0f64;
        let mut worst = 0.0f32;
        for &x in &sorted {
            sum += x as f64;
            if x > worst {
                worst = x;
            }
        }
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        // p1 (nearest-rank 99th percentile): the frame time that 99% of
        // frames came in under. NOTE the `ceil(n*0.99) - 1` indexing — the
        // first cut used `floor` and a 100-sample run returned the single
        // worst frame as "p1", i.e. it measured nothing.
        let rank = ((sorted.len() as f64 * 0.99).ceil() as usize).max(1);
        let p1 = sorted[(rank - 1).min(sorted.len() - 1)];
        let avg = (sum / sorted.len() as f64) as f32;
        Summary {
            frames: sorted.len(),
            avg_ms: avg,
            avg_fps: 1000.0 / avg.max(1e-6),
            p1_ms: p1,
            p1_fps: 1000.0 / p1.max(1e-6),
            worst_ms: worst,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_monotonic() {
        let l0 = Stress::from_level(0);
        let l3 = Stress::from_level(3);
        assert!(!l0.active());
        assert!(l3.active());
        assert!(l3.star_mult > l0.star_mult);
        assert!(l3.warp_mult > l0.warp_mult);
        assert!(l3.part_mult > l0.part_mult);
        assert!(l3.debris > l0.debris);
        assert!(Stress::from_level(9).level == MAX_LEVEL);
    }

    #[test]
    fn counts_are_bounded() {
        for l in 0..=MAX_LEVEL {
            let s = Stress::from_level(l);
            assert!(s.star_count() <= STAR_CAP);
            assert!(s.warp_count() <= WARP_CAP);
        }
        assert_eq!(Stress::from_level(0).star_count(), STAR_CAP / 3);
        assert_eq!(Stress::from_level(3).star_count(), STAR_CAP);
    }

    #[test]
    fn particle_cap_fits_one_sprite_batch() {
        // every particle is one quad in a single Sprites::draw call; the
        // DynVbo holds SPRITE_QUAD_CAP quads and asserts (panics) on
        // overflow, so the live-particle cap must leave margin.
        assert!(PART_CAP < SPRITE_QUAD_CAP);
        assert!(STAR_CAP <= SPRITE_QUAD_CAP);
    }

    #[test]
    fn summary_percentiles() {
        // 100 frames: 99 at 16.666 ms, one 33.3 ms hitch.
        let mut v = vec![16.666f32; 99];
        v.push(33.333);
        let s = Summary::of(&v);
        assert_eq!(s.frames, 100);
        assert!((s.avg_fps - 60.0).abs() < 2.0);
        // the 99th-percentile-slowest index is within the 16.6 ms mass
        assert!(s.p1_ms < 20.0, "p1={}", s.p1_ms);
        assert!((s.worst_ms - 33.333).abs() < 0.01);
    }

    #[test]
    fn stats_rejects_garbage() {
        let mut st = Stats::new();
        st.push(0.0);
        st.push(f32::NAN);
        st.push(-1.0);
        st.push(10.0); // > 5 s
        assert_eq!(st.frames(), 0);
        st.push(1.0 / 60.0);
        assert_eq!(st.frames(), 1);
        assert!((st.fps_ema - 60.0).abs() < 1.0);
    }
}

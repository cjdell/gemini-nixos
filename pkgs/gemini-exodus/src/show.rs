//! The show director — "GEMINI: EXODUS", a cinematic spacesynth flight.
//!
//! Seven chapters, one continuous flight (bar counts shared with the
//! music engine in synth.rs — the visual and audio arrangements are the
//! same score, so every visual beat lands on a musical hit):
//!
//!   S0 EARTH         16 bars   depart: Earth below, countdown, liftoff
//!   S1 ASCENT         8 bars   cruise: GEMINI ONE formation, groove in
//!   S2 WARP          16 bars   fold drive: the streak storm + "EXODUS"
//!   S3 VOID           8 bars   deep-space drift, lone beacon, signal
//!   S4 RENDEZVOUS     8 bars   build: the ringed world appears
//!   S5 PLANET        16 bars   arrival: PLANET COMPUTERS revealed
//!   S6 ORIGIN         8 bars   credits, fade to starlight
//!
//! Each frame the director reads the shared beat clock (already turned
//! into beats/bar/step by main.rs) and produces a `FrameState` — pure
//! data (positions/colors/sizes), no GL — which gfx.rs draws. Chapter
//! transitions are instant cuts: main.rs paints the flash (white for
//! energy cuts, black for the VOID) while the director animates its own
//! in-chapter motion from `cp` (0..1 progress through the chapter).

use crate::synth;

// ------------------------------------------------------------ frame state

pub type C = (f32, f32, f32); // a colour

#[derive(Clone, Copy)]
pub struct Pal {
    pub sky_top: C,
    pub sky_mid: C,
    pub sky_bot: C,
}

#[derive(Clone, Copy)]
pub struct NebulaBlob {
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    pub a: f32,
    pub col: C,
}

#[derive(Clone, Copy)]
pub struct SpriteGlow {
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[derive(Clone, Copy)]
pub struct PlanetPal {
    pub ocean: C,
    pub land: C,
    pub desert: C,
    pub ice: C,
    pub cloud: C,
    pub atmos: C,
    pub clouds: f32,
    pub phase: f32,
}

/// The two planet materials, shared by the director (show.rs) and the
/// texture baker (gfx.rs) — phase doubles as the bake selector
/// (0 = Earth, != 0 = Planet Computers).
pub const PAL_EARTH: PlanetPal = PlanetPal {
    ocean: (0.05, 0.22, 0.42),
    land: (0.16, 0.34, 0.14),
    desert: (0.45, 0.30, 0.12),
    ice: (0.82, 0.88, 0.95),
    cloud: (0.92, 0.94, 0.98),
    atmos: (0.25, 0.6, 1.0),
    clouds: 0.55,
    phase: 0.0,
};

pub const PAL_PC: PlanetPal = PlanetPal {
    ocean: (0.05, 0.35, 0.40),
    land: (0.55, 0.30, 0.62),
    desert: (0.78, 0.55, 0.35),
    ice: (0.85, 0.9, 1.0),
    cloud: (0.95, 0.9, 0.98),
    atmos: (0.8, 0.45, 1.0),
    clouds: 0.65,
    phase: 3.1,
};

/// The GEMINI constellation, as a list of segment endpoints (pairs): two
/// stick-figure twins joined at the shoulder — the visual namesake of the
/// device. Normalized stage coords, origin top-left. Drawn by gfx.rs as
/// dotted additive star-quads, so it costs no new geometry or shader.
pub const GEMINI_EDGES: &[(f32, f32)] = &[
    // Pollux (left twin): head - shoulder - arms - torso - legs
    (0.36, 0.30), (0.36, 0.40),
    (0.36, 0.40), (0.26, 0.48),
    (0.36, 0.40), (0.46, 0.47),
    (0.36, 0.40), (0.35, 0.58),
    (0.35, 0.58), (0.28, 0.74),
    (0.35, 0.58), (0.41, 0.75),
    // the clasp that makes them twins
    (0.36, 0.40), (0.60, 0.38),
    // Castor (right twin)
    (0.60, 0.38), (0.60, 0.28),
    (0.60, 0.38), (0.51, 0.46),
    (0.60, 0.38), (0.70, 0.45),
    (0.60, 0.38), (0.61, 0.56),
    (0.61, 0.56), (0.54, 0.72),
    (0.61, 0.56), (0.68, 0.73),
];

#[derive(Clone, Copy)]
pub struct PlanetPose {
    pub cx: f32,
    pub cy: f32,
    pub r: f32,
    pub spin: f32,
    pub ring: bool,
    pub ring_squash: f32,
    pub ring_roll: f32,
    pub ring_col: C,
    pub pal: PlanetPal,
}

#[derive(Clone, Copy)]
pub struct ShipPose {
    pub cx: f32,
    pub cy: f32,
    pub scale: f32,
    pub heading: f32,
}

#[derive(Clone, Copy)]
pub struct Shock {
    pub cx: f32,
    pub cy: f32,
    pub r0: f32,
    pub r1: f32,
    pub age: f32,
    pub max: f32,
    pub col: C,
    pub alpha: f32,
}

/// One frame of visual data, rebuilt each frame by the Director.
pub struct FrameState {    pub t: f32,
    pub chapter: usize,
    pub cp: f32,
    pub kick: f32,
    pub warp: f32,
    pub warp_c: (f32, f32),
    pub warp_col: C,
    pub warp_col2: C,
    pub star_dim: f32,
    pub pal: Pal,
    pub sky_glow: (f32, f32),
    pub sky_glow_col: C,
    pub sky_glow_r: f32,
    pub sky_glow_g: f32,
    pub nebula: Vec<NebulaBlob>,
    pub halos: Vec<SpriteGlow>,
    pub constellation: Vec<(f32, f32)>,
    pub constellation_a: f32,
    pub planet: Option<PlanetPose>,
    pub ship: Option<ShipPose>,
    pub shocks: Vec<Shock>,
    pub flash: f32,
}

// ------------------------------------------------------------ text frame

/// Fixed text slots, filled per frame (all static strings — no per-frame
/// allocation of the text itself).
#[derive(Clone, Copy, Default)]
pub struct TextFrame {
    pub kicker: Option<(&'static str, f32)>, // top-centre, small
    pub big: Option<(&'static str, f32)>,    // centre, huge, letterspaced
    pub sub: Option<(&'static str, f32)>,    // below big / centre, small
    pub bottom: Option<(&'static str, f32)>, // bottom-centre, small
}

// ------------------------------------------------------------ director

fn ease(t: f32) -> f32 {
    // smooth in-out
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn ramp(t: f32) -> f32 {
    t.clamp(0.0, 1.0)
}

pub struct Director {
    fs: FrameState,
    pub text: TextFrame,
    // per-chapter static colour/behaviour data
    palettes: [Pal; 7],
    neb_sets: Vec<Vec<NebulaBlob>>,
    planet_pals: [PlanetPal; 7],
    // chapter start bookkeeping (for one-shot shock spawns)
    was_chapter: usize,
    shock_age: f32,
    shock_hz: f32,
}

impl Director {
    pub fn new() -> Self {
        // --- sky palettes per chapter (top / mid / bottom)
        let palettes = [
            // S0 EARTH — night launch, warm horizon glow
            Pal { sky_top: (0.010, 0.012, 0.030), sky_mid: (0.03, 0.045, 0.10), sky_bot: (0.075, 0.03, 0.09) },
            // S1 ASCENT — deepening blue
            Pal { sky_top: (0.008, 0.012, 0.040), sky_mid: (0.02, 0.05, 0.12), sky_bot: (0.05, 0.09, 0.18) },
            // S2 WARP — almost black, cyan core
            Pal { sky_top: (0.001, 0.002, 0.008), sky_mid: (0.003, 0.010, 0.030), sky_bot: (0.008, 0.02, 0.05) },
            // S3 VOID — cold darkness
            Pal { sky_top: (0.001, 0.002, 0.004), sky_mid: (0.002, 0.004, 0.010), sky_bot: (0.004, 0.008, 0.016) },
            // S4 RENDEZVOUS — indigo theatre, target approaches
            Pal { sky_top: (0.002, 0.004, 0.012), sky_mid: (0.015, 0.025, 0.07), sky_bot: (0.045, 0.02, 0.10) },
            // S5 PLANET — warm arrival around the ringed world
            Pal { sky_top: (0.004, 0.006, 0.014), sky_mid: (0.02, 0.03, 0.08), sky_bot: (0.10, 0.05, 0.06) },
            // S6 ORIGIN — fade back to deep space
            Pal { sky_top: (0.002, 0.004, 0.010), sky_mid: (0.008, 0.014, 0.040), sky_bot: (0.02, 0.03, 0.07) },
        ];

        // --- planet materials
        // --- planet materials (shared consts — gfx.rs bakes textures
        // from the same palette literals so bake and scene never drift)
        let unused = PlanetPal {
            ocean: (0.0, 0.0, 0.0),
            land: (0.0, 0.0, 0.0),
            desert: (0.0, 0.0, 0.0),
            ice: (0.0, 0.0, 0.0),
            cloud: (0.0, 0.0, 0.0),
            atmos: (0.0, 0.0, 0.0),
            clouds: 0.0,
            phase: 0.0,
        };
        let planet_pals = [PAL_EARTH, PAL_EARTH, unused, unused, PAL_PC, PAL_PC, PAL_PC];

        // --- per-chapter nebula sets (normalized positions)
        let mk_neb = |items: &[(f32, f32, f32, f32, C, f32)]| -> Vec<NebulaBlob> {
            items
                .iter()
                .map(|&(cx, cy, w, h, col, a)| NebulaBlob { cx, cy, w, h, a, col })
                .collect()
        };
        let neb_sets = vec![
            // S0: warm launch haze + cold edges
            mk_neb(&[
                (0.55, 0.92, 0.8, 0.4, (0.25, 0.12, 0.25), 0.5),
                (0.3, 0.75, 0.7, 0.35, (0.1, 0.2, 0.45), 0.35),
                (0.75, 0.3, 0.5, 0.25, (0.06, 0.12, 0.3), 0.3),
                (0.2, 0.2, 0.5, 0.3, (0.05, 0.1, 0.25), 0.25),
            ]),
            // S1: cool blues
            mk_neb(&[
                (0.5, 0.6, 0.8, 0.4, (0.1, 0.2, 0.45), 0.4),
                (0.75, 0.8, 0.6, 0.3, (0.05, 0.16, 0.35), 0.3),
                (0.15, 0.35, 0.55, 0.3, (0.1, 0.14, 0.3), 0.3),
            ]),
            // S2: thin, racing past
            mk_neb(&[
                (0.5, 0.45, 0.9, 0.5, (0.05, 0.12, 0.3), 0.18),
                (0.2, 0.6, 0.7, 0.4, (0.1, 0.05, 0.25), 0.15),
            ]),
            // S3: the void — a whisper of cold gas
            mk_neb(&[
                (0.5, 0.5, 1.0, 0.6, (0.03, 0.08, 0.18), 0.12),
                (0.35, 0.3, 0.6, 0.35, (0.02, 0.05, 0.12), 0.1),
            ]),
            // S4: theatre curtain toward the target
            mk_neb(&[
                (0.5, 0.35, 0.9, 0.45, (0.25, 0.1, 0.4), 0.4),
                (0.2, 0.75, 0.7, 0.35, (0.05, 0.15, 0.35), 0.3),
                (0.8, 0.6, 0.6, 0.3, (0.4, 0.12, 0.2), 0.25),
            ]),
            // S5: warm + violet celebration
            mk_neb(&[
                (0.5, 0.25, 1.1, 0.5, (0.5, 0.2, 0.5), 0.4),
                (0.3, 0.85, 0.7, 0.35, (0.2, 0.12, 0.35), 0.3),
                (0.7, 0.7, 0.6, 0.3, (0.25, 0.3, 0.5), 0.3),
            ]),
            // S6: sparse receding
            mk_neb(&[
                (0.5, 0.4, 0.8, 0.4, (0.1, 0.14, 0.3), 0.2),
                (0.25, 0.7, 0.5, 0.25, (0.05, 0.1, 0.22), 0.15),
            ]),
        ];

        Director {
            fs: FrameState {
                t: 0.0,
                chapter: 0,
                cp: 0.0,
                kick: 0.0,
                warp: 0.0,
                warp_c: (0.5, 0.46),
                warp_col: (0.35, 0.85, 1.0),
                warp_col2: (1.0, 0.5, 0.9),
                star_dim: 1.0,
                pal: palettes[0],
                sky_glow: (0.5, 0.95),
                sky_glow_col: (1.0, 0.7, 0.4),
                sky_glow_r: 0.5,
                sky_glow_g: 0.3,
                nebula: Vec::new(),
                halos: Vec::new(),
                constellation: Vec::new(),
                constellation_a: 0.0,
                planet: None,
                ship: None,
                shocks: Vec::new(),
                flash: 0.0,
            },
            text: TextFrame::default(),
            palettes,
            neb_sets,
            planet_pals,
            was_chapter: usize::MAX,
            shock_age: 0.0,
            shock_hz: 0.0,
        }
    }

    /// Build the frame for time `t`, chapter `chapter`, progress `cp`
    /// (0..1 through the chapter), kick envelope, beat (global, beats).
    /// Reuses the internal FrameState buffers.
    pub fn frame(&mut self, t: f32, chapter: usize, cp: f32, kick: f32, beat: f64) -> &FrameState {
        let fs = &mut self.fs;
        fs.t = t;
        fs.chapter = chapter;
        fs.cp = cp;
        fs.kick = kick;
        fs.flash = 0.0;
        fs.nebula.clear();
        fs.halos.clear();
        fs.constellation.clear();
        fs.constellation_a = 0.0;
        fs.shocks.clear();
        fs.planet = None;
        fs.ship = None;

        let pal = self.palettes[chapter];
        fs.pal = pal;
        // default sky glow off
        fs.sky_glow_g = 0.0;

        // nebula base + gentle time wobble
        fs.nebula.extend_from_slice(&self.neb_sets[chapter]);

        let bar = (beat / 4.0).floor() as u32 % synth::TOTAL_BARS;
        let bar_in =
            ((bar - synth::section_start_bar(chapter as u32)) % synth::SECTION_BARS[chapter]) as u32;
        let beat_in_bar = (beat - (bar as f64 * 4.0)) as f32;

        self.text = TextFrame::default();

        match chapter {
            // ================================================= S0 EARTH
            0 => {
                // Earth below — a modest limb arc (a huge disc of fbm
                // sphere pixels is the 0.1.0 single-digit-fps trap; the
                // planet shader stays on a small region and the glow
                // sprites carry the scale)
                fs.planet = Some(PlanetPose {
                    cx: 0.5,
                    cy: 1.07 - 0.01 * ease(cp),
                    r: 0.125 + 0.012 * (t * 0.3).sin(),
                    spin: t * 0.10,
                    ring: false,
                    ring_squash: 0.4,
                    ring_roll: 0.0,
                    ring_col: (0.8, 0.8, 1.0),
                    pal: self.planet_pals[0],
                });
                fs.sky_glow = (0.5, 0.88);
                fs.sky_glow_col = (1.0, 0.65, 0.45);
                fs.sky_glow_r = 0.7;
                fs.sky_glow_g = 0.5 + 0.2 * (t * 1.7).sin();
                fs.ship = Some(ShipPose {
                    cx: 0.5,
                    cy: 0.98 - 1.05 * ease(cp), // rises and leaves
                    scale: 0.055 + 0.05 * ease(cp),
                    heading: 0.0,
                });
                fs.star_dim = 0.7;
                fs.warp = 0.0;

                // captions + countdown
                let t = (cp * 16.0) as u32; // bar index
                match t {
                    1..=2 => self.text.bottom = Some(("EARTH 2147 - PROJECT EXODUS", fade_in(t as f32 * 4.0, 1.0))),
                    6..=8 => self.text.kicker = Some(("GEMINI ONE - PRE-FLIGHT", fade_in(t as f32 * 2.0, 1.0))),
                    12..=13 => {
                        fs.sky_glow_g = 0.6 + 0.3 * (t as f32 * 3.0).sin();
                        self.text.bottom = Some(("T-MINUS - MAIN ENGINE IGNITION", 0.9));
                    }
                    14..=15 => {
                        // countdown digits pulse on each beat
                        let n = 3 - (t as i32 - 14) * 2 - if beat_in_bar < 0.5 { 1 } else { 0 };
                        let n = n.max(1);
                        let label = match n as u32 { 1 => "1", 2 => "2", 3 => "3", _ => "GO" };
                        let a = 0.6 + 0.4 * (1.0 - (beat_in_bar * 2.0).fract());
                        self.text.big = Some((label, a));
                        fs.star_dim = 0.9;
                        // ignition shake on the last beats
                        if bar_in >= 15 && beat_in_bar > 0.5 {
                            fs.warp = 0.1;
                        }
                    }
                    _ => {}
                }
                if bar_in >= 13 {
                    // flame trail builds
                    fs.halos.push(SpriteGlow { cx: 0.5, cy: 0.9, w: 0.05, h: 0.05, r: 1.0, g: 0.6, b: 0.2, a: 0.8 });
                }
            }
            // ================================================= S1 ASCENT
            1 => {
                // Earth recedes to the lower-left
                let d = 1.0 - ease(cp);
                fs.planet = Some(PlanetPose {
                    cx: 0.09 + 0.05 * d,
                    cy: 0.87 + 0.07 * d,
                    r: 0.05 + 0.075 * d,
                    spin: t * 0.10,
                    ring: false,
                    ring_squash: 0.4,
                    ring_roll: 0.0,
                    ring_col: (0.8, 0.8, 1.0),
                    pal: self.planet_pals[0],
                });
                fs.sky_glow = (0.14, 0.9);
                fs.sky_glow_col = (1.0, 0.6, 0.4);
                fs.sky_glow_r = 0.3;
                fs.sky_glow_g = 0.5 * d;
                // formation flight: ship centre, slight sway
                fs.ship = Some(ShipPose {
                    cx: 0.5 + 0.02 * (t * 0.7).sin(),
                    cy: 0.60 + 0.01 * (t * 0.5).cos(),
                    scale: 0.11 + 0.01 * (t * 0.9).sin() + 0.015 * kick,
                    heading: -0.06 * (t * 0.7).sin(), // gentle weave
                });
                fs.star_dim = 0.9;
                fs.warp = 0.0;
                let t = (cp * 8.0) as u32;
                match t {
                    0..=1 => self.text.kicker = Some(("ASCENT - GEMINI ONE ONLINE", fade_in(t as f32 * 6.0, 1.0))),
                    5..=6 => self.text.bottom = Some(("FOLD DRIVE CHARGE - 100%", fade_in((t as f32 - 5.0) * 4.0, 1.0))),
                    _ => {}
                }
                if bar_in >= 6 {
                    fs.star_dim = 1.0;
                }
            }
            // ================================================= S2 WARP
            2 => {
                fs.warp = 0.7 + 0.3 * ease(cp) + 0.15 * kick;
                fs.warp_c = (0.5 + 0.03 * (t * 0.6).sin(), 0.46 + 0.02 * (t * 0.4).cos());
                fs.warp_col = (0.35, 0.85, 1.0);
                fs.warp_col2 = (1.0, 0.45, 0.85);
                fs.star_dim = 0.15;
                // core glow at the vanishing point
                fs.halos.push(SpriteGlow {
                    cx: fs.warp_c.0, cy: fs.warp_c.1,
                    w: 0.28 + 0.06 * kick, h: 0.28 + 0.06 * kick,
                    r: 0.7, g: 0.95, b: 1.0, a: 0.8,
                });
                fs.halos.push(SpriteGlow {
                    cx: fs.warp_c.0, cy: fs.warp_c.1,
                    w: 0.1, h: 0.1, r: 1.0, g: 1.0, b: 1.0, a: 0.9,
                });
                let t = (cp * 16.0) as u32;
                match t {
                    0..=1 => {
                        self.text.big = Some(("EXODUS", fade_in((cp * 16.0 - 0.0) * 2.0, 1.0) * 1.0));
                        self.text.kicker = Some(("FOLD DRIVE ENGAGED", fade_in((cp * 16.0 - 0.4) * 4.0, 1.0)));
                    }
                    8..=9 => self.text.bottom = Some(("+12.7 AU / 40 SECONDS", fade_in((cp * 16.0 - 8.0) * 6.0, 1.0))),
                    _ => {}
                }
            }
            // ================================================= S3 VOID
            3 => {
                fs.warp = 0.0;
                fs.star_dim = 0.55;
                fs.sky_glow = (0.5, 0.35);
                fs.sky_glow_col = (0.35, 0.8, 1.0);
                fs.sky_glow_r = 0.25;
                fs.sky_glow_g = 0.25;
                // the constellation namesake resolves out of the dark
                fs.constellation.extend_from_slice(GEMINI_EDGES);
                fs.constellation_a = (0.20 + 0.18 * (t * 0.6).sin())
                    * ease(((cp - 0.04) / 0.30).clamp(0.0, 1.0));
                // a lone beacon ring, far off
                let a = t * 0.25;
                let r0 = 0.16 + 0.01 * (t * 0.3).sin();
                for i in 0..48 {
                    let an = a + i as f32 / 48.0 * std::f32::consts::TAU;
                    let rr = if i % 12 == 0 { r0 * 1.18 } else { r0 };
                    let x = 0.5 + an.cos() * rr;
                    let y = 0.42 + an.sin() * rr * 0.55;
                    // the beacon blips once per bar (audio has a blip there)
                    let blip = (1.0 - beat_in_bar).max(0.0);
                    let on = if i % 12 == 0 { 0.5 + 0.5 * blip } else { 0.3 + 0.3 * blip };
                    fs.halos.push(SpriteGlow {
                        cx: x, cy: y,
                        w: 0.006 + 0.003 * on, h: 0.006 + 0.003 * on,
                        r: 0.45, g: 0.85, b: 1.0, a: on,
                    });
                }
                let t = (cp * 8.0) as u32;
                match t {
                    0..=1 => self.text.bottom = Some(("TRANSMISSION INTERRUPTED", fade_in(t as f32 * 6.0, 1.0))),
                    3..=5 => self.text.sub = Some(("... AWAITING BEACON ...", fade_in((t as f32 - 2.4) * 3.0, 1.0) * 0.8)),
                    _ => {}
                }
            }
            // ================================================= S4 RENDEZVOUS
            4 => {
                // the ringed world rises ahead — small, growing
                let g = ease(cp);
                fs.planet = Some(PlanetPose {
                    cx: 0.5 + 0.02 * (t * 0.4).cos(),
                    cy: 0.46 - 0.02 * g,
                    r: 0.03 + 0.055 * g,
                    spin: t * 0.3,
                    ring: true,
                    ring_squash: 0.42,
                    ring_roll: 0.0,
                    ring_col: (0.95, 0.8, 0.55),
                    pal: self.planet_pals[4],
                });
                fs.sky_glow = (0.5, 0.40);
                fs.sky_glow_col = (0.9, 0.7, 1.0);
                fs.sky_glow_r = 0.35;
                fs.sky_glow_g = 0.4 + 0.3 * g;
                fs.star_dim = 0.9;
                fs.warp = 0.0;
                let t = (cp * 8.0) as u32;
                match t {
                    0..=1 => self.text.kicker = Some(("BEACON ACQUIRED", fade_in(t as f32 * 6.0, 1.0))),
                    4..=5 => self.text.bottom = Some(("INBOUND - 2.1 MILLION KM", fade_in((t as f32 - 4.0) * 3.0, 1.0))),
                    _ => {}
                }
            }
            // ================================================= S5 PLANET
            5 => {
                // arrival: the ringed world fills the stage. Cap the
                // sphere at ~0.22 of min(w,h) — beyond that the fbm
                // surface cost eats the 60 fps budget on the T880.
                let g = ease(((cp * 16.0 - 2.0) / 12.0).clamp(0.0, 1.0));
                fs.planet = Some(PlanetPose {
                    cx: 0.5,
                    cy: 0.42,
                    r: 0.075 + 0.13 * g + 0.01 * kick,
                    spin: t * 0.35 + 0.5,
                    ring: true,
                    ring_squash: 0.42,
                    ring_roll: 0.0,
                    ring_col: (0.95, 0.78, 0.5),
                    pal: self.planet_pals[4],
                });
                fs.sky_glow = (0.5, 0.40);
                fs.sky_glow_col = (1.0, 0.75, 1.0);
                fs.sky_glow_r = 0.6;
                fs.sky_glow_g = 0.8;
                fs.star_dim = 0.8;
                fs.warp = 0.0;
                // The destination's binary suns — the literal "Gemini"
                // pair. Additive halos behind the planet (layer 4), so
                // they crest the ringed world instead of washing over it.
                // Unconditional (Director's Cut motif), not stress-gated.
                fs.halos.push(SpriteGlow { cx: 0.27, cy: 0.24, w: 0.36, h: 0.36, r: 0.95, g: 0.78, b: 0.50, a: 0.40 });
                fs.halos.push(SpriteGlow { cx: 0.74, cy: 0.29, w: 0.26, h: 0.26, r: 0.60, g: 0.82, b: 1.00, a: 0.36 });
                fs.halos.push(SpriteGlow { cx: 0.27, cy: 0.24, w: 0.10, h: 0.10, r: 1.00, g: 0.95, b: 0.80, a: 0.75 });
                fs.halos.push(SpriteGlow { cx: 0.74, cy: 0.29, w: 0.07, h: 0.07, r: 0.90, g: 0.95, b: 1.00, a: 0.75 });
                // the fly-by: ship crosses the foreground then exits
                let fly = (cp * 16.0).clamp(0.0, 5.0) / 5.0;
                if fly < 1.0 {
                    fs.ship = Some(ShipPose {
                        cx: -0.1 + 1.2 * fly,
                        cy: 0.85 - 0.1 * fly,
                        scale: 0.09,
                        heading: 0.18 * fly - 0.4,
                    });
                }
                let t = (cp * 16.0) as u32;
                match t {
                    0..=1 => self.text.kicker = Some(("ARRIVAL", fade_in(t as f32 * 6.0, 1.0))),
                    5..=8 => {
                        self.text.big = Some(("PLANET COMPUTERS", fade_in((cp * 16.0 - 5.0) * 2.0, 1.0)));
                        self.text.sub = Some(("HOME OF THE GEMINI", fade_in((cp * 16.0 - 5.6) * 2.0, 1.0) * 0.85));
                    }
                    _ => {}
                }
            }
            // ================================================= S6 ORIGIN
            6 => {
                // the ringed world recedes as the credits roll
                let g = 1.0 - ease(cp);
                fs.planet = Some(PlanetPose {
                    cx: 0.5,
                    cy: 0.44 + 0.03 * (1.0 - g),
                    r: 0.075 + 0.13 * g,
                    spin: t * 0.3,
                    ring: true,
                    ring_squash: 0.42,
                    ring_roll: 0.0,
                    ring_col: (0.9, 0.75, 0.5),
                    pal: self.planet_pals[4],
                });
                fs.sky_glow = (0.5, 0.42);
                fs.sky_glow_col = (1.0, 0.7, 1.0);
                fs.sky_glow_r = 0.5;
                fs.sky_glow_g = 0.6 * g;
                fs.star_dim = 0.8;
                fs.warp = 0.0;
                let t = (cp * 8.0) as u32;
                fs.constellation.extend_from_slice(GEMINI_EDGES);
                fs.constellation_a = (1.0 - ease(cp)) * 0.5;
                match t {
                    0..=2 => self.text.sub = Some(("A PLANET COMPUTERS ORIGINAL", fade_in(t as f32 * 4.0, 1.0))),
                    3..=5 => self.text.bottom = Some(("PLANET COMPUTERS \u{00b7} GEMINI PDA", fade_in((t as f32 - 3.0) * 3.0, 1.0))),
                    6..=7 => self.text.bottom = Some(("MALI-T880 \u{00b7} PANFROST \u{00b7} NIXOS", fade_in((t as f32 - 6.0) * 4.0, 1.0) * 0.9)),
                    _ => {}
                }
            }
            _ => {}
        }

        // chapter-top impact shock (skip the quiet void + the cold open)
        if chapter != self.was_chapter && chapter != 3 && chapter != 0 {
            self.shock_hz = match chapter {
                1 => 0.5,  // liftoff burst
                2 => 0.5,  // fold engaged
                4 => 0.35, // beacon acquired
                5 => 0.9,  // arrival
                _ => 0.4,
            };
            self.shock_age = 0.0;
            if chapter == 5 {
                fs.ship = None; // planet reveal owns the frame
            }
        }
        self.was_chapter = chapter;
        // expand the chapter-top shockwave
        if self.shock_age < 1.4 {
            if fs.shocks.len() < 2 {
                let col = match chapter {
                    0 | 1 => (1.0, 0.8, 0.6),
                    2 => (0.6, 0.9, 1.0),
                    5 => (1.0, 0.85, 0.7),
                    _ => (0.7, 0.8, 1.0),
                };
                let (cx, cy) = if chapter == 5 {
                    (0.5, 0.42)
                } else {
                    (fs.warp_c.0, fs.warp_c.1)
                };
                fs.shocks.push(Shock {
                    cx,
                    cy,
                    r0: 0.06,
                    r1: 1.15,
                    age: 0.0,
                    max: 1.3,
                    col,
                    alpha: 0.55,
                });
            }
        }
        self.shock_age += 1.0 / 60.0;

        // age the shock rings (they only live one chapter top)
        for s in fs.shocks.iter_mut() {
            s.age += 1.0 / 60.0;
        }
        fs.shocks.retain(|s| s.age < s.max);

        &self.fs
    }
}

// ---- helpers ------------------------------------------------------------------

fn fade_in(t: f32, speed: f32) -> f32 {
    ramp(t * speed)
}

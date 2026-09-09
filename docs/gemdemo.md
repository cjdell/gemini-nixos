# gemdemo — "GEMINI: EXODUS" (0.2.0): cinematic spacesynth assembly

Last updated: 2026-09-09

A from-scratch Rust show for the Planet Computers Gemini PDA (1280×720
GLES 3.1 Mali-T880 via the geminipda Mesa/panfrost fork). It is a
**complete rewrite of the 0.1.0 "AETHER" demoscene** after that build's
on-glass verdict (2026-09-09: single-digit FPS, flawed visuals, music
"completely broken") — and it keeps the role the project earned: the
first sustained T880 load + the on-glass proof that both 3D drawing and
speaker output work on this bring-up.

The 0.2.0 pitch: an **Assembly-style cinematic flight with an epic
spacesynth score**. Seven chapters, one continuous shot, musically
driven (126 BPM, A minor, ~38 s per chapter / 4 min 45 s full loop):

| # | Chapter | Bars | Visuals | Score |
|---|---|---|---|---|
| S0 | EARTH | 16 | Earth's limb, launch glow, countdown, T-MINUS | pad/drone ambience → countdown → riser |
| S1 | ASCENT | 8 | GEMINI ONE in formation cruise | groove enters (four-on-floor, pulse bass) |
| S2 | WARP | 16 | Fold-drive streak storm + **EXODUS** title | full theme: gallop bass, motif lead |
| S3 | VOID | 8 | Deep-space drift, lone pulsing beacon | breakdown: pads, blips, sub floor |
| S4 | RENDEZVOUS | 8 | The ringed world grows ahead | build (F G F G), riser |
| S5 | PLANET | 16 | **PLANET COMPUTERS** reveal, fly-past | anthem finale (drops back to A minor) |
| S6 | ORIGIN | 8 | Credits, world recedes, fade | soft beat out → fade to starlight |

The name/branding lean is deliberate — the show is for the Planet
Computers Gemini PDA, so it flies a GEMINI ONE spacecraft to the ringed
homeworld "PLANET COMPUTERS".

Status: 🟡 0.2.0 built for aarch64 (host type-check + synth unit tests
green 2026-09-09); ON-GLASS verification pending (same safe launch
path as before: LXQt/labwc windowed or gemwl fullscreen).

## Why 0.1.0 failed and what 0.2.0 changed

0.1.0 rendered at **2× SSAA into a multipass post chain** (brightness/
blur/bloom passes + feedback FBOs at half res) **plus a 128-step
raymarched act** — on a T880 that is ~2–3 fps for heavy per-pixel
fragment work even at 720p. On top of that the music master was a naive
per-sample peak-follower limiter that slammed EVERY beat to full scale
(measured master rms ≈ 0.88) and a saw-DC bug flattened the voices
(details below). 0.2.0:

- **Direct single-framebuffer renderer** (gfx.rs): every frame is a
  back-to-front stack of layers drawn straight into the window surface —
  sky (the only fullscreen fragment pass, a cheap gradient) → nebula
  cloud sprites → stars → halo glows → ring-far → planet → ring-near →
  warp streaks → ship → particles/shocks → titles. No intermediate
  FBOs, no SSAA, no post chain. **Glows ARE the bloom.**
- **Sprite-field rendering**: stars (1800), nebula blobs, particles,
  glows and shock rings are tiny textured quads (four 64–96 px RGBA
  sprites baked once: soft, star, ring, cloud). Blending carries the
  look; per-pixel overdraw averages ~2, and the only region with heavy
  fragment math (the planet's lit, banded, rotating fbm surface + its
  ring arcs) is kept small and the noise budget low — that's the 60 fps
  story (ship + fields ~A53s; the planet pass is where an A72's extra
  fragment headroom shows).
- **Music engine rewritten** (synth.rs): per-part buses with real
  headroom, sends to a dotted-8th ping-pong delay + Schroeder reverb,
  per-part sidechain pump on the kick, a soft-knee ceiling (only shapes
  overs > 0.8) and a fast-attack/slow-release transient limiter. The
  startup chime is gone. Score sanity is unit-tested (no clipping, no
  over-limiting, no silence, section/chord bookkeeping).
- **A72 cores** (cpu.rs): best-effort — asks the `gemini-a72-up` unit
  to bring cpu8/cpu9 online (bounded 20 s wait, never hangs the show),
  then pins the render thread to them. Without the cluster it runs on
  the A53s (the pipeline mostly fits); with it, worst-case planet/warp
  frames hold the budget.

## Architecture / files

| File | Role |
|---|---|
| `pkgs/gemdemo.nix` | derivation (native aarch64; `alsa-lib.dev` for the alsa crate via pkg-config; `libglvnd` for `-lEGL` via `RUSTFLAGS -L`; wayland + libxkbcommon pinned on the RUNPATH via postFixup — winit dlopens them) |
| `src/main.rs` | winit 0.29 event loop, frame pacing, beat-clock bridge, section jumps, titles/HUD drawing, FPS meter, `--dump` |
| `src/show.rs` | the **director**: reads the beat clock → builds a pure-data `FrameState` (positions/colors/sizes — zero GL) for every chapter; chapter text slots |
| `src/gfx.rs` | the **renderer**: sky gradient, sprite layers (baked textures), warp streaks, rotating planet sphere + ring arcs, vector ship, particles/shockwaves — all straight to the surface |
| `src/synth.rs` | the **spacesynth engine**: voices (kick/snare/clap/hats/crash/tom/sub/bass/pluck-arp/supersaw lead/pads/blips/fx), dotted-8 delay + Schroeder reverb, soft-knee + limiter master, the 7-chapter score (bar counts shared with show.rs), sample-accurate `Clock` publishing |
| `src/cpu.rs` | A72 cluster bring-up request (gemini-a72-up unit) + render-thread affinity (best-effort; libc sched_setaffinity) |
| `src/audio.rs` | cpal 0.15 ALSA stream: opens the `gemini16` S16-pinning plug (services/audio.nix), falls back cleanly to silent mode |
| `src/font.rs` | embedded 5×7 glyph atlas → textured quads (used by main for titles/HUD) |
| `src/glctx.rs` | EGL 1.5 bootstrap: wayland platform + `wl_egl_window` → ES 3.1 context, `eglSwapInterval(1)` |
| `src/glutil.rs` | gl 0.14 helpers: program build with full shader-log diagnostics, uniform lookup (missing = build-time panic — typos are the #1 demo killer), VBO/dyn-VBO. ALL object creation via `glGen*` |
| `src/shaders.rs` | shared GLSL prelude (ES 3.00): hash/noise/fbm, IQ cosine palette, fullscreen VS |

**Clock & sync**: the audio thread owns the synth and is the master
clock; per render frame main reads published atomics (beat
fixed-point, kick envelope, section). Chapter changes come from the
engine (score) and the renderer just follows — except `[`/`]` (user
jumps) which write a `jump` atomic the audio engine consumes. Bar
counts live in synth.rs and show.rs imports them, so a visual beat
always lands on a musical hit. `--silent` substitutes a system-time
clock for development runs without a sound card.

## The fork-panfrost receipts (STILL TRUE — gfx.rs is written around them)

From the 0.1.0 glass work (dated 2026-09-08) — these are why gfx.rs
looks the way it does; do not "modernize" them away:

- **DSA → `glGen*`**: `glCreateTextures/Buffers/Framebuffers/
  VertexArrays` are "unsupported function" stubs on this GLES 3.1
  context and silently create NOTHING. All object creation is
  glGen + Bind.
- **One interleaved buffer per VAO**, per-attr offsets, no instancing
  (instancing segfaulted the fork), no multi-buffer VAOs (panfrost
  `JOB_BUS_FAULT` storms), no DrawElements (index-minmax crash) — all
  draws are expanded triangles.
- **`wl_egl_window_create`** is required for a Wayland window surface
  (bare wl_surface → EGL_BAD_NATIVE_WINDOW) — `-lwayland-egl` at link.
- **GLES-strict shaders**: uniforms that optimize away at link are not
  looked up (missing uniform = compile-time panic); don't declare
  samplers twice; ES 3.00 only.
- **glReadPixels**: GLES3 forbids GL_RGB/UNSIGNED_BYTE from an RGBA8
  buffer — the dump reads RGBA (the old silent-failure cost a phantom
  "everything is black" panic).

## Audio receipts (from 0.1.0, still governing audio.rs)

- cpal 0.15 enumerates ALSA via name hints: a custom plug needs
  `hint { show on }` (added to `gemini16` in
  `services/pipewire/asound.conf`) or cpal falls back to `default`,
  which converts F32-in → S32-on-wire → **noise on the 16-bit-only
  MT6351**. The stream is forced I16 @ 44.1 k.
- The 0.1.0 saw bug `2.0*(p-p.fract())-1.0` was a constant -1 → every
  saw voice was DC → limiter → flat inaudible. 0.2.0 saws are
  `2.0*p-1.0`; the engine test asserts a sane master rms (not silence,
  not ~0.88 over-limit).

## Controls & CLI

| Key | Action |
|---|---|
| `space` | pause/resume (audio + clock + animation) |
| `[` / `]` | previous / next chapter (physical keys) |
| `h` | HUD/info line on/off |
| `q` / `esc` | quit |

CLI: `--windowed` (dev, 1024×576), `--section N` (start at S0..S6),
`--silent` (no audio; system-time clock), `--dump N path.ppm`
(after N frames, glReadPixels the surface to a PPM and exit — the
glass/QA screenshot path), `--help`.

On glass: run in the LXQt/labwc session (windowed or maximized) or
fullscreen on gemwl. Logs to stdout: one line per 2 s with time, beat,
section and FPS — the pacing heartbeat for the serial console.

## Next actions

- ON-GLASS run of 0.2.0 (windowed first), then sweep chapters with
  `[`/`]` while watching fps at each chapter's worst frame (planet S5 +
  warp S2 are the expensive ones) → decide whether S5's planet radius
  needs shrinking or the A72 unit needs to be up before the show.
- Verify audio quality ears-on (mix headroom, delay/reverb tails, the
  S3 beacon blip against the visual blip).
- `--dump` frame QA per chapter: sky gradient orientation, planet/ring
  arc depth, title centering.

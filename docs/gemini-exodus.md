# GEMINI: EXODUS (Director's Cut) — cinematic spacesynth + GPU stress test

Last updated: 2026-09-11

**Package:** `pkgs/gemini-exodus.nix` → binary `gemini-exodus` (v1.0.0).
**Ship:** in the rootfs closure via `services/gemini-pda.nix`
(`environment.systemPackages`). **Not** the same thing as `gemdemo`
(`docs/gemdemo.md`), which stays the one-file hardware template.

This is the 0.2.0 demo engine — the one that held **60 fps on glass**
(2026-09-09, gen33) — restored from git history as a first-class package
and upgraded. The 0.3.0 "purge" had deleted it down to the gemdemo
triangle; the source was recovered byte-for-byte from commit `32ba356`
and then extended here. It is the flagship "show off the hardware" piece.

## What is new over 0.2.0

| Upgrade | Where | Why |
|---|---|---|
| GPU **stress load model** `--stress 0..3` | `stress.rs`, `gfx.rs` | Higher star/warp/debris/particle counts and nebula overdraw — always more of the SAME single-pass layers, never a new pass. |
| **Benchmark harness** `--bench SECS [--chapter-secs S]` | `main.rs`, `stress.rs` | Timed run that sweeps all seven chapters and prints per-chapter frametime stats (avg / p1 / worst). `--json` for machines. |
| **Frametime/stats overlay** `--stats` | `main.rs` | 60/30 fps reference lines + 30-bucket frametime bar chart + GPU/A72/stress readout. Drawn with the existing 5×7 font; **off during `--bench`** so it can't perturb the measurement. |
| **GEMINI constellation** motif | `show.rs` (`GEMINI_EDGES`), `gfx.rs` | The twins (Castor & Pollux) drawn as dotted additive star-quads in S3 VOID and S6 ORIGIN — the device's namesake, no new shader. |
| **Twin-sun finale** | `show.rs` (S5) | Two binary suns (warm + blue) cresting behind the ringed world — the literal "Gemini" pair, always on. |
| **Enriched anthem** | `synth.rs` (S2, S5) | A third-below harmony lead in the warp theme; a low-octave counter-melody + timpani in the finale. Reuses existing voices — no new DSP. |
| **Chapter-jump race fix** | `main.rs` | 0.2.0 had the render thread `swap()` the `jump` atomic even in audio mode, racing the audio callback for keyboard `[`/`]`. Now exactly one consumer: audio owns the clock, or the render thread when `--silent`. |
| **Debris field** | `gfx.rs` | Glinting ice shards (a new baked `rock` sprite) at stress ≥ 1. |

## Architecture (unchanged from the 0.2.0 engine)

```
audio thread  ── cpal/ALSA `gemini16` (S16 @ 44.1k) ──> synth::Engine
                 publishing beat/kick/section/`jump` atomics (clock master)
render thread ── winit event loop (Wayland client)
                 Director (show.rs) reads the clock → pure-data FrameState
                 gfx.rs draws it DIRECTLY into the window surface:
                   sky → nebula → stars → constellation → debris → halos
                   → [ring far] → planet → [ring near] → warp → ship
                   → particles → shock rings
                 font.rs paints titles + the optional stats overlay on top
```

**There is no intermediate framebuffer.** No FBO, no bloom/feedback
chain, no SSAA, no readback. The only fullscreen fragment pass is the
cheap sky gradient; everything else is tiny pre-baked-texture quads, and
the heavy math (planet sphere + ring) is confined to a small screen
region. This is *the* reason 0.2.0 went from 0.1.0's single-digit fps to
60. **Do not reintroduce multipass** — if you want a stress test, add
more instances of an existing layer (that is all `--stress` does).

The panfrost/fork receipts (DSA `glCreate*` are stubs, one interleaved
VBO per VAO, no instancing, no `glDrawElements`, `wl_egl_window`, the
`gemini16` S16@44.1k wire state) are the ones documented in
`docs/gemdemo.md` and carried in the module headers. Read those before
touching the GL path.

## The show — seven chapters, one continuous flight

| # | Chapter | Bars | What happens |
|---|---|---|---|
| 0 | EARTH | 16 | Earth's limb below, countdown, main-engine ignition |
| 1 | ASCENT | 8 | GEMINI ONE formation flight, groove in |
| 2 | WARP | 16 | Fold-drive streak storm, "EXODUS" |
| 3 | VOID | 8 | Cold drift, lone beacon, **the Gemini constellation resolves** |
| 4 | RENDEZVOUS | 8 | The ringed world appears |
| 5 | PLANET | 16 | Arrival — PLANET COMPUTERS, the twin suns, anthem finale |
| 6 | ORIGIN | 8 | Credits, constellation fading, back to starlight |

Visual bar counts and the synth arrangement share the same score
(`synth::SECTION_BARS`), so every visual beat lands on a musical hit.

## On-glass usage

Run it **fullscreen from a desktop session** (GNOME default, or a nested
compositor — it is just a Wayland client). It needs the compositor
running; audio opens the `gemini16` plug and falls back to a silent
system-time clock if the card is absent.

```sh
gemini-exodus                       # the show, classic look, fullscreen
gemini-exodus --stress 2 --stats    # heavy load + live frametime graph
gemini-exodus --bench 75 --chapter-secs 10 --json   # stress report
```

Keys: `space` pause · `[` `]` prev/next chapter · `h` info line ·
`q`/`esc` quit.

### Reading the benchmark

`--bench` sweeps all seven chapters (every `--chapter-secs` seconds),
capturing wall-clock frametimes, then prints:

```
GEMINI: EXODUS benchmark — 1280x720, stress 3 (MAX)
gpu: Mali-T880 (Panfrost)   a72: A72 pinned (cpu8/9)
chapter     frames   avg ms    p1 ms  worst ms  avg fps    p1 fps
EARTH          ...
...
TOTAL          ...
p1 fps/p1 ms = 99th-percentile slowest frame (the stutter metric); worst ms = single worst frame.
```

The headline is **p1** (nearest-rank 99th percentile), not the mean: a
demo that averages 60 can still stutter, and p1 is what catches it. The
per-chapter split tells the heavy reveals (S5 PLANET) apart from the
cheap starfields. `--json` emits the same numbers for scripting.

> **Measurement hygiene:** the stats overlay is suppressed during
> `--bench` unless you explicitly pass `--stats`. The overlay is drawn
> with the same single-pass font path, so leaving it on is fine, but a
> clean run is the defensible number.

## A72 cores

The MT6797X is 2× Cortex-A72 (cpu8/9, powered down at boot) + 8× A53.
`cpu.rs` asks the `gemini-a72-up` unit to bring the big cluster up,
polls for it (~20 s bound), then pins the **render/submission thread** to
cpu8/cpu9 with `sched_setaffinity`, leaving the A53s for audio + the
compositor. `--no-a72` skips the request (host QA). The stats overlay
and the identity banner report which happened; the benchmark's `a72`
field records it.

Rendering is GPU-bound, so the A72s buy headroom and consistent worst
cases rather than raw fps — which is exactly what a stress test wants to
measure.

## Build / test / deploy

- **Native aarch64 build** (canonical):
  `nix build .#packages.aarch64-linux.gemini-exodus` (remote builder).
- **Host type-check loop** (seconds):
  `bash bin/gemini-exodus-host-check.sh`.
- **Host tests** (pure stress/bench model; no device link):
  `cargo test --lib` in `pkgs/gemini-exodus`. The full suite
  (`cargo test --release`, includes the synth engine's output/RMS test)
  needs the `-L` paths for EGL/wayland — see the script header.
- **Ship:** it is in the rootfs closure already; `bin/deploy.sh deploy`
  (or `bin/device-rebuild.sh`) delivers it on the next profile switch.

## Status / receipts

- **Build-level (host):** `cargo check --release` clean; 5 lib tests
  (stress model + percentile math + the particle-cap guard) + 4 bin tests
  (synth score, incl. the "audible, not clipped, not over-limited" mix
  test) pass.
- **aarch64 build (remote builder, 2026-09-11):**
  `nix build .#packages.aarch64-linux.gemini-exodus` OK →
  `/nix/store/khfnmqyqbrdwr62nlli8spf1phxyd96p-gemini-exodus-1.0.0`
  (aarch64 ELF, 4.98 MB binary; RUNPATH pins libglvnd `libEGL`,
  wayland/libwayland-egl, libxkbcommon and alsa-lib).
- **On glass (2026-09-11, gen 12, GNOME at native 2160×1080):**
  deployed and running. Steady per-chapter averages (12 s each,
  `--silent`), avg fps / p1 fps — ASCENT 30.3/18.6, WARP 28.5/19.2,
  VOID 26.0/17.6, RENDEZVOUS 35.6/25.5, PLANET 26.5/17.3, ORIGIN
  26.2/14.6. GPU line `Mali-T880 MC4 (Panfrost)`, A72 render-thread pin
  confirmed. (A 6 s sweep with 1 s chapter jumps is *not* a fair number:
  jumps + warmup dominate — that first run read 29 fps.)
- **The renderer is not the bottleneck.** Under identical load the
  trivial `gemdemo` triangle fullscreen reads ~40 fps; the entire EXODUS
  scene costs only **~3 ms more** (27.8 ms vs 25 ms at stress 0). The
  wall is the platform present path under GNOME: `geminipda-drm`'s
  generic `drm_fb_blit` **XRGB8888→ARGB8888 full-panel shadow blit**
  runs in the DRM commit worker, which sits at **95 %** of one CPU core
  while the demo runs (`events_unbound`, `/proc/<pid>/stack`), and the
  driver has no vblank. The `2026-09-10k` receipt (gemdemo 74 fps after
  the alpha-loop fix) shows the same path exceeds 60 when the machine is
  idle; this session had Chrome + GNOME loaded.
- **`--stress` scales as designed:** same chapter, stress 0 → 27.8 ms,
  stress 3 → 37.8 ms (S4); S2 WARP stress 3 → 45.0 ms. So MAX stress
  still runs (~22–26 fps) with the present-path ceiling on top.
- **Paths to a clean 60 fps (not yet pursued):** (a) the platform fix
  already identified in the 2026-09-10 COSMIC handover — a driver-local
  single-pass shadow blit (drop the redundant XRGB→ARGB conversion when
  the source alpha is already `0xff`) plus vblank timing; a kernel-delta
  + boot.img change (flash + reboot); (b) run on **`gemwl`** (GPU-direct
  to the LK framebuffer, no shadow plane) — a session switch, no kernel
  change; (c) run with the machine idle (no Chrome) to reach the
  ~74 fps-class ceiling. On-glass run helper:
  **`bin/exodus-on-glass.sh`**.
- The 0.1.0 "AETHER" multipass/raymarch result (single-digit fps) and the
  S5 planet-map perf fix (42 → 60 fps by baking 256×128 maps once on the
  CPU instead of per-pixel fbm) are recorded in `docs/gemdemo.md`'s
  version history; they remain the two hard lessons this renderer obeys.

## Version history

- **1.0.0 (2026-09-11)** — "Director's Cut": 0.2.0 engine restored as
  `pkgs/gemini-exodus` + stress model, benchmark, stats overlay,
  constellation/twin-sun motif, enriched anthem, chapter-jump race fix,
  debris field. Build-level verified and **deployed on glass (gen 12)**;
  the engine costs ~3 ms over a flat triangle — the current fullscreen
  ceiling is the platform shadow blit (see Status).
- 0.2.0 (2026-09-09, `gemdemo`, git `32ba356`) — "GEMINI: EXODUS":
  first 60 fps on-glass cinematic spacesynth (gen33). Source the base
  here came from.
- 0.1.0 (2026-09-08) — "AETHER": multipass + raymarch, single-digit fps,
  retired.

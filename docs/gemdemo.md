# gemdemo — "AETHER": demoscene + GPU stress test

Last updated: 2026-09-08

A demoscene-style show for the Gemini PDA, written from scratch in Rust,
doubles as the first serious **GPU stress test** for the Mali-T880 MP4
(geminipda Mesa/panfrost fork, GLES 3.1). Audio is **fully synthesized**
in the app (no samples): a 7-section, 120 BPM chiptune-ish track whose
events drive the visuals through a shared beat clock.

Status (2026-09-08): ✅ RUNNING ON GLASS — in-repo aarch64 build green
and verified live in the LXQt/labwc session (windowed) and directly on
the gemwl compositor (fullscreen): real scene content (scene FBO → post
chain → window), continuous frames, zero panfrost kernel faults, no
crashes (60 s+ soak). Audio follow-up 2026-09-09: MUSIC now audible on
glass (S16@44.1k forced + the saw-DC/step-decode engine fixes below)
but the MIX IS AWFUL (over-limited, master rms ~0.88) — tuning the
master gain/limiter + removing the debug chime is the next step.

Audio on glass (2026-09-09 receipt, in `audio.rs`/`synth.rs` headers):
cpal 0.15 enumerates ALSA devices via name hints — a custom plug needs
`hint { show on }` (added to gemini16 in services/pipewire/asound.conf)
or cpal falls back to `default` → F32-in → S32-on-wire = NOISE on the
16-bit-only MT6351. The stream is forced I16 @ 44.1 k when offered.
THE engine silence was the saw bug: `2.0*(p-p.fract())-1.0` is constant
-1 for p∈[0,1) → every saw voice was -1 DC → limiter → flat inaudible
+0.9 DC. Fixed to `2.0*p-1.0` (3 sites) + step decode `s = step %
STEPS_PER_BAR` (was bar % → frozen arrangements) + step 0 fires at
t=0. A 0.6 s 880→660→440 Hz startup chime (bypasses the limiter) is
the audibility aid — remove when the mix is fixed. The mix is currently
over-limited (master rms ~0.88); quality = follow-up.

Fixes that took it from "green build" to "on glass" (all dated
2026-09-08):
- **parse_args rewrite** (main.rs): the original loop never advanced
  past flag-only args → 100% CPU spin, and value flags left `i` ON the
  value → "unknown flag". Both paths had never run without flags.
- **winit dlopen deps** (gemdemo.nix): winit 0.29 dlopens
  `libwayland-client.so.0`/libxkbcommon at runtime; the generic rpath
  fixup only pulled their `-dev` outputs → `NoWaylandLib` abort. The
  derivation pins the REAL lib dirs on the RUNPATH via postFixup
  patchelf.
- **wl_egl_window_create** (glctx.rs): Mesa's wayland platform rejects
  a bare wl_surface as a window surface (EGL_BAD_NATIVE_WINDOW); EGL
  needs the `wl_egl_window` wrapper carrying the surface size
  (`-lwayland-egl` added to the link).
- **eglChooseConfig attrs** (glctx.rs): `EGL_RENDER_BUFFER`/
  `EGL_RGB_BUFFER` passed as a key/value pair are not valid config keys
  (EGL_RGB_BUFFER is a VALUE of EGL_COLOR_BUFFER_TYPE) → BAD_ATTRIBUTE.
- **font atlas 16×4 → 16×6** (font.rs): the glyph table (80 + sentinel)
  outgrew 64 cells → OOB bake. + an assert so it can't silently recur.
- **GLES-strict shaders** (scenes.rs, post.rs): `vec5` split to
  pos(3)+uv(2); `pal()` duplicated into the vortex VS (prelude-only
  before); composite declared `u_bloom` twice (sampler2D + float) →
  renamed the intensity `u_bloomamt`; dead `u_res` dropped (optimized
  out at link → uniform() panic).
- **DSA → glGen\*** (glutil.rs): `glCreateTextures/Buffers/Framebuffers/
  VertexArrays` are "unsupported function" stubs on the GLES 3.1
  context and silently created NOTHING (ghost objects — every draw
  then no-op'd; mesa logs `glBufferData(no buffer bound)`). All four
  are glGen + Bind now.
- **dangling font pointer** (font.rs): `Writer::new(&font)` stored a
  `*const Font` into a SceneSet that is then MOVED → every later
  Writer::draw dereferenced a stale address. Writers borrow the Font at
  draw time instead.
- **no indexed draws** (scenes.rs): the fork's DrawElements index-
  minmax scan crashes reading the buffer → tunnel emits expanded
  triangles.
- **no instancing, single interleaved VAO buffers** (scenes.rs, font.rs):
  instancing segfaulted u_vbuf (copy from NULL+offset), and the
  multi-buffer-per-VAO pattern page-faulted the GPU (`panfrost: js
  fault, JOB_BUS_FAULT` storms + sched timeouts). All draws now use one
  interleaved buffer per VAO with nonzero-offset attrs — the exact
  pattern the desktop GLES2 path provably runs.
- **post FBO 1×1 resize bug** (main.rs): `Post::new()` allocates 1×1
  and the init `state.resize()` no-ops when the size already matches —
  a window that never sends a Resized event (fullscreen on gemwl) left
  the ENTIRE post chain at 1×1 → the resolve magnified one texel to a
  flat colour. Force `post.resize` after init.
- **glReadPixels RGB→RGBA** (main.rs dump): GLES3 forbids
  GL_RGB/UNSIGNED_BYTE for RGBA8 framebuffers — the readback error was
  silent and returned zeros, manufacturing a phantom "everything
  renders black" (the scene was fine all along).

## Files

| File | Role |
|---|---|
| `pkgs/gemdemo.nix` | derivation (native aarch64; `alsa-lib.dev` for the alsa crate via pkg-config; `libglvnd` for `-lEGL` via `RUSTFLAGS -L`; wayland + libxkbcommon pinned on the RUNPATH via postFixup — winit dlopens them) |
| `pkgs/gemdemo/src/main.rs` | winit 0.29 event loop (Wayland), render loop, HUD, key handling, FPS meter |
| `pkgs/gemdemo/src/glctx.rs` | EGL 1.5 bootstrap: `eglGetPlatformDisplay(EGL_PLATFORM_WAYLAND_E)` + `wl_egl_window_create` wrapper → `eglCreatePlatformWindowSurface` (raw-window-handle 0.6 `Wayland { surface: NonNull }`), GLES 3 context, `eglSwapInterval(1)` |
| `pkgs/gemdemo/src/glutil.rs` | tiny GL helpers (build/program/uniforms, VBO/dyn-VBO, FBO, error checks) over the `gl` 0.14 crate (generated 4.5-core list, Fallbacks::All) — ALL object creation via `glGen*` (the DSA `glCreate*` are stubs on the GLES 3.1 context: silently create nothing) |
| `pkgs/gemdemo/src/shaders.rs` | all GLSL (ES 3.00): tunnel, starfield, particles, raymarch, bloom/feedback/post, text |
| `pkgs/gemdemo/src/scenes.rs` | scene implementations: warp tunnel (expanded triangles, no ELEMENT buffer), starfield + particles (single interleaved VBOs — the fork's panfrost faults on multi-buffer VAOs and instancing), afterglow, HUD text via `font.rs` |
| `pkgs/gemdemo/src/raymarch.rs` | the SDF raymarcher (the main GPU load: domain-warped repeated SDFs, 128 steps, per-pass seed) |
| `pkgs/gemdemo/src/post.rs` | post chain: 2×SSAA → brightness/blur → additive bloom → feedback-trail mix → scanlines/vignette |
| `pkgs/gemdemo/src/synth.rs` | the whole engine: voices (kick/snare/hat/bass/arp/pad/lead/pluck/riser/sweep/fx), FX (delay, reverb, master), the 7-section song sequencer, sample-accurate `Clock` |
| `pkgs/gemdemo/src/clock` (in `sensors.rs`) | `Clock` — the shared audio-thread → render-thread beat/kick/flash state (atomics) |
| `pkgs/gemdemo/src/sensors.rs` | `Sensors` — battery/charger/temp/cpu sysfs polled ~2 Hz for the HUD |
| `pkgs/gemdemo/src/font.rs` | embedded 8×8 bitmap font atlas (no TTF dep) → texture + quads |
| `pkgs/gemdemo/src/audio.rs` | cpal 0.15 stream: opens the `gemini16` ALSA plug (S16-pinning, `services/audio.nix` `/etc/asound.conf`) if present, else ALSA `default`; f32 or i16 callback; any failure → silent mode (system-time clock). ON GLASS 2026-09-09: `gemini16` needs a `hint { show on }` block (added) or cpal can't enumerate it and falls back to `default` — F32-in there converts to S32 on the wire → buzz on the 16-bit-only MT6351 |

## Controls

| Key | Action |
|---|---|
| `space` | pause/resume the beat clock (visuals + audio) |
| `f` | white flash (GPU + backlight moment) |
| `h` | HUD on/off |
| `[` / `]` | jump to previous/next section (layout-independent, physical keys) |
| `q` / `esc` | quit |

CLI: `--stress N` (extra raymarch passes/frame, default 1),
`--ssaa N` (supersampling factor, default 2), `--bloom 0..1`,
`--feedback 0..1`, `--silent` (no audio — pure GPU load), `--no-hud`,
`--dump <frame#> <path>` (stage PPM dumps), `--solid R,G,B`
(target self-test). On glass, start with `--ssaa 1 --stress 0` or
`--windowed` (the ssaa2 default fullscreen scene is 4320×2160 —
~1–2 fps on the T880).

## GPU stress-test protocol

The T880 has never been loaded like this. Run it deliberately, with the
battery on AC (the battery guard powers off <3.65 V under load):

1. Baseline: default (`--ssaa 2 --stress 1`). Watch the HUD FPS +
   battery drain (mA) + T0 temperature for ~5 min.
2. Ramp: `--stress 4`, then `--stress 8` (each = full-res raymarch
   passes/frame on top of the scene).
3. Soak: `--stress 8 --ssaa 2` for 15–30 min; log T0 every minute
   (`sensors` in the HUD or `gemcli status`).
4. Abort criteria: thermal throttle visible as FPS cliff, T0 > ~55 °C
   sustained, or any driver error in dmesg (`panfrost`).
5. Record the results here (FPS vs stress, temp curve, drain) — this is
   the receipt for the "T880 headroom" claim.

⚠️ Rule 5 (LCD): the demo runs under the gemwl/labwc compositor — the
panel is LK-initialized and the compositor is the verified path; but if
the flicker EVER appears, stop and go to TWRP per AGENTS.md.

## On-glass test plan (DONE 2026-09-08 — receipt above)

1. aarch64 build green → `nix copy` to the device store (or the
   device-rebuild loop) — the binary needs the system's libglvnd +
   alsa-lib (both in the rootfs already).
2. From the LXQt/labwc session (Wayland running): run
   `gemdemo` — check: EGL platform display works (panfrost
   `eglGetPlatformDisplay(WL)` — the fork's dma-buf path), shader
   compile (GLES 3.1 subset), audio via `gemini16`.
3. Expected failure candidates: (a) panfrost fork + `gl` crate's
   `glCreateTextures`/`glCreateBuffers` (core 4.3/4.4 entry points —
   the GLES 3.1 surface should expose them via KHR aliases; if not,
   fall back to `glGen*`), (b) cpal/ALSA plug negotiation,
   (c) perf (T880 is a 2016 midrange chip — the raymarcher is
   deliberately the load; tune `--stress` down if FPS < ~20 at
   defaults).

Resolved on glass (see receipt): (a) — DSA entry points were NOT
available (silent stubs); everything is `glGen*`. (b) — pulse/PipeWire
works. (c) — yes: defaults are too heavy for fullscreen; windowed /
`--ssaa 1` for normal use.

## Next session (mix + stress tuning)

- Fix the mix: master rms ~0.88 = over-limited (flat tanh/limiter for
  most of every beat). Drop master_gain (0.9 → ~0.5?) and/or retune the
  limiter; remove the 0.6 s startup chime (debug aid).
- Then deploy gen32 (binary is in the flake; `bash bin/deploy.sh build
  && deploy`) so plain `gemdemo` on the device has fixed audio.
- Run the GPU stress-test protocol (T880 headroom vs `--stress`,
  temp/drain curve).

## Pins

- winit 0.29.15 · egl 0.2.7 · gl 0.14.0 (Fallbacks::All) · cpal 0.15.3
  (ALSA) · raw-window-handle 0.6.2 · ab_glyph 0.2.31 · tiny-skia 0.11.4 —
  all pinned by `pkgs/gemdemo/Cargo.lock` (committed).
- Build: nixpkgs `dc5d91f84032` (flake pin), rustPlatform from the
  flake's eval pkgs.

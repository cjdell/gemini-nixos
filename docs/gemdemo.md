# gemdemo — minimal GLES 3.1 + ALSA template for the Gemini PDA

Last updated: 2026-09-09

**0.3.0: the "how do we talk to the hardware from Rust" skeleton.** The
0.2.0 "GEMINI: EXODUS" demoscene (10 modules, ~5200 lines — see git
history) was judged useless except as proof of the GPU/codec
interaction, so it was deleted down to a **single Rust source file**
(`pkgs/gemdemo/src/main.rs`) that:

1. opens a Wayland window (winit 0.29) — fullscreen borderless by
   default (gemwl), `--windowed` 1024×576 for nested-labwc QA;
2. boots an **OpenGL ES 3.1** context on it via the raw `egl` crate
   (`eglGetPlatformDisplay(Wayland)` → `wl_egl_window` →
   `eglCreatePlatformWindowSurface` → ES3 context, gl loaded with
   `eglGetProcAddress`) — this is the geminipda Mesa/panfrost fork
   driving the Mali-T880;
3. draws **one spinning shaded triangle** (per-vertex RGB → smooth
   interpolated shading, rotated by a `u_time` uniform);
4. plays a **440 Hz sine** through the MT6351 codec via cpal 0.15
   (ALSA backend, `gemini16` plug, S16 @ 44100).

That is the entire hello-world. **To start a real OpenGL app: copy
`src/main.rs` (and `pkgs/gemdemo.nix`) and replace the "scene" section**
— the EGL bootstrap, the audio open and the object-creation pattern are
the hard-won parts; everything from the `// ---- The scene` marker down
is example content.

## Why the file looks the way it does — THE RECEIPTS

Each is dated/verified on glass (2026-09-08/09, gemdemo 0.1.0/0.2.0)
and spelled out inline in the main.rs header comment — they are the
reason the template exists, and the reason the code must not be
"modernized" naively:

- **GLES 3.1 only** (panfrost fork); shaders ESSL 300 es. There is no
  desktop GL / GL4 / DSA here.
- **DSA `glCreate*` are silent stubs** on this context (create nothing)
  → all objects via `glGen*` + Bind.
- **One interleaved buffer per VAO**, per-attr offsets; **no
  instancing** (segfaulted the fork), no multi-buffer VAOs (panfrost
  JOB_BUS_FAULT storms), **no `glDrawElements`** (index-minmax crash) —
  all draws are expanded `glDrawArrays(GL_TRIANGLES, …)`.
- EGL's native window must be a **`wl_egl_window`** (bare wl_surface →
  `EGL_BAD_NATIVE_WINDOW`); created at the window's current size, so
  windows are fixed-size (resize would need `wl_egl_window_resize`).
  Link `-lwayland-egl` (RUNPATH-pinned by the derivation).
- **Audio wire state: S16_LE @ 44100 Hz** (the 16-bit-only MT6351 AFE
  advertises 48k/32-bit without programming it — S32 or 48k on the
  wire = white noise). cpal 0.15 sees the `gemini16` plug only because
  asound.conf gives it `hint { show on }`; the `default` PCM fallback
  converts f32→S32 = noise, so open gemini16 by name and force an I16
  @ 44.1 k config when offered. f32 streams are still handled (the plug
  converts) for devices that don't offer I16.
- **Uniform typos are silent on GLES** (optimized-away uniforms return
  -1 from GetUniformLocation) → assert, don't ignore.

## File map (single file — section order)

| Section | What it is |
|---|---|
| header comment | the receipts (above) — read before editing |
| EGL bootstrap | `GlCtx::new`: wayland display → config → wl_egl_window surface → ES3 context; `load_gl`; `swap` |
| GL helpers | `build_program` (panic w/ driver log), `uniform` (assert), `check` (glGetError), `gl_string` |
| `start_sine()` | cpal ALSA open (gemini16 → I16@44.1k) + the per-frame fill (one sample copied to all channels, 10 ms attack) |
| The scene | `VS`/`FS` (ESSL 300), `triangle()` verts, main(): window → ctx → program/VAO → loop |
| render loop | RedrawRequested pump (vsync via compositor), q/esc quit, 2 s heartbeat line |

## Build / deploy / run

- Build = the flake's native aarch64 package (remote builder):
  `nix build .#packages.aarch64-linux.gemdemo`. Host type-check loop:
  `bash bin/gemdemo-host-check.sh` (seconds, x86_64 cargo check with
  the same pkg-config/alsa trick).
- The package is in the rootfs closure via
  `services/gemini-pda.nix` (`environment.systemPackages`) — a normal
  `bin/deploy.sh deploy` (or `bin/device-rebuild.sh`) ships it.
- On the PDA, inside the LXQt/labwc session or on gemwl:
  `gemdemo` (fullscreen) or `gemdemo --windowed`.
  Keys: `q` / `esc` quit. Logs: identity banner + one line per 2 s with
  time, frame count and fps — the pacing heartbeat for the serial
  console.

## Version history

- **0.3.0 (2026-09-09)** — EXODUS purge: single-file template (spinning
  shaded triangle + 440 Hz sine). Deployed as gen34.
- 0.2.0 (2026-09-09) — "GEMINI: EXODUS" cinematic spacesynth assembly
  (7 chapters, real-time synth score; 60 fps on glass; gen33). Source
  preserved in git history — still the reference for textured quads,
  baked sprites, direct single-framebuffer layering and the synth
  engine if a demoscene is ever wanted again.
- 0.1.0 (2026-09-08) — "AETHER" (multipass + raymarch; single-digit
  fps; superseded).

## Next actions

- ON-GLASS check of the 0.3.0 template (this session's deploy): the
  triangle should spin smoothly at ~60 fps fullscreen on gemwl AND
  windowed under labwc; the 440 Hz sine should be a clean steady tone
  with no buzz (S16@44.1k) and no open click; heartbeat lines prove
  pacing. Confirm `gemdemo` (the system copy) is the new binary, note
  the store path + fps/audio state in the session log.

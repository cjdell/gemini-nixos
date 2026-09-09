# gemdemo — "GEMINI: EXODUS" (0.2.0): cinematic spacesynth assembly
# for the Mali-T880 (see docs/gemdemo.md "EXODUS rewrite" for the
# architecture + the 60 fps strategy and controls).
#
# Source (pkgs/gemdemo/): a single-binary Rust app:
#   * winit 0.29 (Wayland surface) + the egl 0.2 crate (EGL 1.5 loader
#     — eglGetPlatformDisplay(WL) + eglCreatePlatformWindowSurface)
#     driving the geminipda Mesa/panfrost fork's GLES 3.1
#     (pkgs/mesa-geminipda.nix);
#   * gl 0.14 (bindgen-style FFI, Fallbacks::All) — no GL loader crate;
#   * cpal 0.15 (ALSA backend only — 0.15 dropped Pulse; opens the
#     `gemini16` S16-pinning ALSA plug defined by services/audio.nix,
#     so the MT6351 AFE path is the verified one);
#   * everything else (show director, synth engine, direct renderer,
#     font) is in-tree — zero non-crate dependencies (libc for the
#     A72 affinity call).
#
# 0.1.0 → 0.2.0 (2026-09-09): complete rewrite after the 0.1.0 verdict
# (single-digit FPS, flawed visuals, music completely broken — receipts
# docs/gemdemo.md). The renderer is now DIRECT (no multipass post chain,
# no SSAA, no raymarch): sky/nebula/stars/glows/particles drawn straight
# into the window as layered textured quads; the synth is a real mix
# bus (per-part gains, dotted-8 ping-pong + reverb sends, soft ceiling +
# transient limiter — the old naive limiter slammed every beat to full
# scale). New cpu.rs asks the gemini-a72-up unit for the A72 cluster
# and pins the render thread there (best-effort).
#
# Build = native aarch64 (this flake's canonical model): rustPlatform
# from eval.pkgs on the aarch64 remote builder. Non-crate inputs:
#   * alsaLib.dev — the alsa crate resolves libasound through
#     pkg-config (ALSA_NO_PKG_CONFIG is NOT set);
#   * libglvnd — the binary links -lEGL; the glvnd loader dispatches
#     to the vendor driver at runtime (on the PDA: the geminipda
#     panfrost fork). -L is passed via RUSTFLAGS because the store
#     lib dir is not on the default linker search path.
#   * wayland + libxkbcommon — NOT link-time deps: winit 0.29
#     dlopens libwayland-client.so.0 / libwayland-cursor / libxkbcommon
#     at runtime (NoWaylandLib panic if absent — first on-glass run,
#     2026-09-08). The generic stdenv rpath fixup pulls only the -dev
#     OUTPUTS (header symlinks), so the derivation's postFixup patchelf
#     pins the real lib dirs on the RUNPATH explicitly (see below).
#
# ON GLASS 2026-09-08 (verified, full receipt in docs/gemdemo.md):
# the source keeps ONE interleaved buffer per VAO with per-attr offsets
# and glGen* objects — no DSA glCreate* (silent stubs on GLES 3.1), no
# instancing, no multi-buffer VAOs (fork panfrost GPU page faults), no
# DrawElements (index-minmax crash). Don't reintroduce those patterns.
#
# Host-side iteration: `bash bin/gemdemo-host-check.sh` type-checks the
# crate on x86_64 (seconds); unit tests (synth score sanity) via
# `cargo test --release` with the same pkg-config/alsa trick.

{ lib, rustPlatform, pkg-config, alsa-lib, libglvnd, wayland, libxkbcommon, patchelf }:

let alsaLib = alsa-lib; in

rustPlatform.buildRustPackage rec {
  pname = "gemdemo";
  version = "0.2.0";

  src = ./gemdemo;

  # winit 0.29 + egl 0.2 + gl 0.14 + cpal 0.15 + their transitive set,
  # pinned by the committed lockfile. Regenerate with
  # `cargo generate-lockfile` inside pkgs/gemdemo and commit.
  cargoLock.lockFile = ./gemdemo/Cargo.lock;

  nativeBuildInputs = [ pkg-config alsaLib.dev patchelf ];
  buildInputs = [ alsaLib libglvnd wayland libxkbcommon ];

  # resolve -lEGL (libglvnd) and -lwayland-egl (wl_egl_window_create)
  # from the store, not the system path
  RUSTFLAGS = "-L ${libglvnd}/lib -L ${wayland}/lib";

  # The generic stdenv rpath fixup only pulled the -dev outputs (header
  # symlinks) of wayland/libxkbcommon, so dlopen("libwayland-client.so.0")
  # failed at runtime (winit: NoWaylandLib) — first on-glass probe
  # 2026-09-08. Pin the REAL lib dirs on the RUNPATH explicitly (the
  # ELF references pull them into the runtime closure automatically).
  postFixup = ''
    ${patchelf}/bin/patchelf --set-rpath \
      "$(${patchelf}/bin/patchelf --print-rpath $out/bin/gemdemo)"':${wayland}/lib:${libxkbcommon}/lib' \
      $out/bin/gemdemo
  '';

  doCheck = false;

  meta = with lib; {
    description = "gemdemo — GEMINI: EXODUS: cinematic spacesynth assembly + Mali-T880 GPU stress test (GLES 3.1, synthesized score)";
    longDescription = ''
      0.2.0 rewrite of the AETHER demoscene. Seven-chapter cinematic
      flight (Earth lift-off → warp → void beacon → rendezvous with a
      ringed world → PLANET COMPUTERS reveal → credits), synced to an
      epic 126 BPM spacesynth score synthesized in real time (drums,
      analog bass, arpeggio, supersaw lead, pads — dotted-8 ping-pong
      + reverb, soft ceiling + transient limiter). Renderer is direct
      single-framebuffer layering for the 60 fps budget: one cheap
      gradient, procedural-sprite fields, small-region planet sphere.
      Best-effort A72 cluster bring-up (gemini-a72-up unit) + render
      affinity. Run fullscreen on gemwl/labwc; keys: space pause,
      [ ] chapter, h HUD, q quit. See docs/gemdemo.md.
    '';
    homepage = "https://github.com/planet-computers"; # gemini-nixos repo (local)
    license = licenses.mit;
    platforms = [ "aarch64-linux" ];
    mainProgram = "gemdemo";
  };
}

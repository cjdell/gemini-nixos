# gemdemo — minimal OpenGL ES 3.1 + ALSA template for the Gemini PDA
# (the "how do we talk to the GPU and codec from Rust" skeleton).
#
# 0.3.0 (2026-09-09): the 0.2.0 "GEMINI: EXODUS" demoscene (10 modules,
# ~5200 lines) was judged useless except as proof of the hardware
# interaction, and stripped to ONE source file (pkgs/gemdemo/src/main.rs):
# a winit (Wayland) window with an EGL ES 3.1 context on the geminipda
# Mesa/panfrost fork (pkgs/mesa-geminipda.nix) drawing a spinning shaded
# triangle + a cpal 440 Hz sine out through the MT6351 codec. Copy the
# file (and this derivation) to start a real OpenGL app; the receipts
# (DSA stubs, one-interleaved-VAO, wl_egl_window, S16@44.1k wire state,
# gemini16 plug) are spelled out in the file header + docs/gemdemo.md.
#
# Source deps (all in-tree from the demo purge):
#   * winit 0.29 (Wayland surface) + the egl 0.2 crate (EGL 1.5 loader
#     — eglGetPlatformDisplay(WL) + eglCreatePlatformWindowSurface)
#     driving the fork's GLES 3.1;
#   * gl 0.14 (bindgen-style FFI, Fallbacks::All) — no GL loader crate;
#   * cpal 0.15 (ALSA backend only — 0.15 dropped Pulse; opens the
#     `gemini16` S16-pinning ALSA plug defined by services/audio.nix,
#     so the MT6351 AFE path is the verified one);
#   * libc is GONE — it only served the 0.2.0 A72-affinity code.
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
# ON GLASS 2026-09-08/09 (verified, full receipts in docs/gemdemo.md):
# the source keeps ONE interleaved buffer per VAO with per-attr offsets
# and glGen* objects — no DSA glCreate* (silent stubs on GLES 3.1), no
# instancing, no multi-buffer VAOs (fork panfrost GPU page faults), no
# DrawElements (index-minmax crash). Don't reintroduce those patterns.
#
# Host-side iteration: `bash bin/gemdemo-host-check.sh` type-checks the
# crate on x86_64 (seconds) with the same pkg-config/alsa trick.

{ lib, rustPlatform, pkg-config, alsa-lib, libglvnd, wayland, libxkbcommon, patchelf }:

let alsaLib = alsa-lib; in

rustPlatform.buildRustPackage rec {
  pname = "gemdemo";
  version = "0.3.0";

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
    description = "gemdemo — minimal GLES 3.1 + ALSA template for the Gemini PDA (spinning shaded triangle + 440 Hz sine)";
    longDescription = ''
      0.3.0: the hardware-interaction skeleton that survived the 0.2.0
      EXODUS demoscene purge — ONE Rust file that opens a Wayland window
      (winit 0.29), boots a GLES 3.1 EGL context on the geminipda
      Mesa/panfrost fork (Mali-T880), draws one spinning per-vertex
      shaded triangle (ESSL 300 es, glGen* objects, one interleaved
      VBO), and plays a 440 Hz sine via cpal 0.15 (ALSA `gemini16`
      plug, S16 @ 44.1k). The file's comments carry the hardware
      receipts; copy file + derivation to start a real OpenGL app.
      Keys: q/esc quit. --windowed runs 1024×576 (nested labwc QA);
      default is borderless fullscreen (gemwl). See docs/gemdemo.md.
    '';
    homepage = "https://github.com/planet-computers"; # gemini-nixos repo (local)
    license = licenses.mit;
    platforms = [ "aarch64-linux" ];
    mainProgram = "gemdemo";
  };
}

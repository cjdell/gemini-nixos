# gemini-exodus — GEMINI: EXODUS (Director's Cut): a cinematic spacesynth
# + GPU stress test for the Planet Computers Gemini PDA.
#
# This is the 0.2.0 demo engine restored as a first-class package and
# upgraded (stress load model, `--bench` per-chapter frametime
# percentiles, `--stats` overlay, the GEMINI constellation / twin-sun
# motif, an enriched anthem, and the chapter-jump race fix). Rendering is
# DIRECT and single-pass — no FBOs, no bloom chain, no SSAA — which is
# exactly why the 0.2.0 engine held 60 fps on the Mali-T880 where the
# 0.1.0 multipass AETHER managed single digits.
#
# It is deliberately NOT the gemdemo template: gemdemo stays the one-file
# "how to talk to the hardware" skeleton; this is the flagship show. The
# two share the same EGL/GLES/ALSA receipts (see docs/gemdemo.md for the
# hardware story, docs/gemini-exodus.md for this app).
#
# Source deps (all in-tree, lockfile-pinned):
#   * winit 0.29 (Wayland surface) + egl 0.2 (EGL 1.5 loader:
#     eglGetPlatformDisplay(WL) + eglCreatePlatformWindowSurface) driving
#     the fork's GLES 3.1;
#   * gl 0.14 (bindgen-style FFI, Fallbacks::All);
#   * cpal 0.15 (ALSA backend: opens the `gemini16` S16-pinning plug);
#   * libc 0.2 (A72 render-thread affinity in cpu.rs).
#
# Build = native aarch64 (this flake's canonical model): rustPlatform from
# eval.pkgs on the aarch64 remote builder. Non-crate inputs:
#   * alsaLib.dev — alsa crate resolves libasound through pkg-config;
#   * libglvnd — the binary links -lEGL (the glvnd loader dispatches to
#     the geminipda panfrost fork at runtime);
#   * wayland + libxkbcommon — winit dlopens libwayland-client.so.0 /
#     libwayland-cursor / libxkbcommon at runtime, so the generic stdenv
#     rpath fixup only pulls the -dev outputs; the postFixup patchelf pins
#     the REAL lib dirs on the RUNPATH (first on-glass run 2026-09-08 hit
#     NoWaylandLib). -L for -lEGL/-lwayland-egl comes via RUSTFLAGS.
#
# Tests: the pure stress/benchmark module is the crate's lib target, so
# `cargo test --lib` runs on a host with no device link. Host check:
# `bash bin/gemini-exodus-host-check.sh`; full suite (needs the device
# -L paths, see the script/docs): `cargo test --release`.
#
# ON GLASS: the base engine was verified at 60 fps on 2026-09-09 (0.2.0,
# gen33). This 1.0.0 build's on-glass pass is owed — it ships in the
# rootfs closure via services/gemini-pda.nix; run
# `gemini-exodus --stats` then `gemini-exodus --bench 75 --chapter-secs 10`
# for the stress report.

{ lib, rustPlatform, pkg-config, alsa-lib, libglvnd, wayland, libxkbcommon, patchelf }:

let alsaLib = alsa-lib; in

rustPlatform.buildRustPackage rec {
  pname = "gemini-exodus";
  version = "1.0.0";

  src = ./gemini-exodus;

  cargoLock.lockFile = ./gemini-exodus/Cargo.lock;

  nativeBuildInputs = [ pkg-config alsaLib.dev patchelf ];
  buildInputs = [ alsaLib libglvnd wayland libxkbcommon ];

  # resolve -lEGL (libglvnd) and -lwayland-egl (wl_egl_window_create)
  RUSTFLAGS = "-L ${libglvnd}/lib -L ${wayland}/lib";

  postFixup = ''
    ${patchelf}/bin/patchelf --set-rpath \
      "$(${patchelf}/bin/patchelf --print-rpath $out/bin/gemini-exodus)"':${wayland}/lib:${libxkbcommon}/lib' \
      $out/bin/gemini-exodus
  '';

  doCheck = false;

  meta = with lib; {
    description = "GEMINI: EXODUS (Director's Cut) — cinematic spacesynth + GPU stress test for the Gemini PDA";
    longDescription = ''
      A real-time spacesynth flight over seven chapters (Earth → ascent →
      fold drive → the void → rendezvous → Planet Computers → origin),
      with a real-time synth score and the GEMINI constellation / twin-sun
      motif. Single-pass GLES 3.1 on the geminipda Mesa/panfrost fork
      (Mali-T880) — no intermediate FBOs. Stress load model (`--stress`),
      time-boxed benchmark with per-chapter frametime percentiles
      (`--bench`), frametime/stats overlay (`--stats`), A72 render-thread
      pinning. Keys: space=pause, [ ]=chapter, h=info, q/esc=quit.
      See docs/gemini-exodus.md.
    '';
    homepage = "https://github.com/planet-computers"; # gemini-nixos repo (local)
    license = licenses.mit;
    platforms = [ "aarch64-linux" ];
    mainProgram = "gemini-exodus";
  };
}

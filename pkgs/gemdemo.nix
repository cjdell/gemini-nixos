# gemdemo — "AETHER": demoscene + GPU stress test for the Mali-T880
# (see docs/gemdemo.md for the architecture, controls and the
# stress-test protocol).
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
#   * everything else (synth, scenes, post, font) is in-tree — zero
#     non-crate dependencies.
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
# No tests (doCheck = false): the real check is on-glass.

{ lib, rustPlatform, pkg-config, alsa-lib, libglvnd, wayland, libxkbcommon, patchelf }:

let alsaLib = alsa-lib; in

rustPlatform.buildRustPackage rec {
  pname = "gemdemo";
  version = "0.1.0";

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
    description = "gemdemo — AETHER demoscene + Mali-T880 GPU stress test (GLES 3.1, synthesized audio)";
    longDescription = ''
      Demoscene-style show (7-section audio-driven structure, 120 BPM:
      intro, warp tunnel, raymarched SDF, starfield, particles,
      afterglow, finale) rendered at 2x SSAA with bloom + feedback
      trails, plus a live device-telemetry HUD (battery/temp/CPU/
      FPS). Doubles as the GPU stress test for the T880: --stress
      multiplies the raymarch passes/frame, --ssaa raises the
      supersampling factor, --silent drops audio for pure GPU load.
      See docs/gemdemo.md.
    '';
    homepage = "https://github.com/planet-computers"; # gemini-nixos repo (local)
    license = licenses.mit;
    platforms = [ "aarch64-linux" ];
    mainProgram = "gemdemo";
  };
}

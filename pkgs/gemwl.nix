# gemwl — the Gemini PDA Wayland compositor (phase-4 desktop).
#
# Sources (pkgs/gemwl/, byte-identical copies of the verified GeminiPDA
# project's build/wayland/ files):
#   gemwl.c         the compositor: a minimal wlroots 0.18 compositor
#                   whose single output IS the LK bootloader framebuffer,
#                   GPU-direct (shadow dma-buf -> GLES3 compute blit into
#                   the gemfb dma-buf; zero CPU pixels). Includes the
#                   custom gemfb backend + allocator + buffer, the
#                   xdg-shell/scene/seat server, libinput input, cursor.
#   tinytest.c      minimal xdg-shell client: red wl_shm buffer on
#                   configure, prints every event (debugging aid).
#   tinytest-anim.c animated xdg-shell client (red/green/blue on every
#                   frame callback) — proves the compositor delivers
#                   frames at the output refresh rate.
#   wlegltst.c      EGL wayland-platform client (red GLES2 window via
#                   wl_egl_window) — the KWin/Qt buffer-path proof.
#
# All four were cross-checked against the same wlroots 0.18.2 that
# pkgs/wlroots-geminipda.nix builds (the on-glass verified combo:
# wlroots 0.18.2 + Mesa 25.0.7 fork + kernel #329 CONFIG_FB_GEMINIPDA).
#
# Build = the prototype's on-device build.sh, as a Nix derivation:
#   gcc -O2 -Wall -DWLR_USE_UNSTABLE gemwl.c
#       $(pkg-config --cflags --libs wlroots-0.18 wayland-server
#                    xkbcommon libdrm) -lEGL -lGLESv2 -lm
# (the Nix build passes the include dirs explicitly instead of pkg-config
# — see the buildPhase comment). xdg-shell protocol headers are generated
# by wayland-scanner exactly as build.sh did: the SERVER header for
# wlroots' installed wlr_xdg_shell.h, the CLIENT header + interface code
# for the tinytest/wlegltst clients. libdrm is a build input for its
# headers only (drm.h/drm_fourcc.h/panfrost_drm.h ioctl structs — /dev/dri
# is driven with raw ioctls, so no DT_NEEDED on libdrm).
#
# EGL/GLES2 headers + client libs come from libglvnd (egl.pc/glesv2.pc,
# libEGL.so.1/libGLESv2.so.2 — the glvnd dispatch layer; the actual
# driver is the mesa-geminipda ICD via the /etc/glvnd manifest wired by
# config/gemini.nix). GLES3/gl31.h (compute shaders) is also in
# libglvnd-dev.
#
# Runtime contract (see services/desktop.nix for the unit):
#   PAN_MESA_DEBUG=noafbc            AFBC readback bug workaround (must
#                                    stay set; harmless otherwise)
#   XDG_RUNTIME_DIR=/run/gemwl       socket dir (RuntimeDirectory=gemwl)
#   -t 90                            output transform (panel is physical
#                                    landscape, fb portrait)
#   /dev/gemfb + /dev/dri/renderD129 must exist (kernel #329:
#   CONFIG_FB_GEMINIPDA=y, panfrost module) — gpu-poweron first.
#
# The GEMINI_BUILD_ID banner macro (gemini-build-id.h) is generated here
# so every binary says which port/version it came from (version hygiene,
# same idea as the prototype's build/gen-build-id.sh — but deterministic,
# no hostname/date in the output).
{ lib
, stdenv
, wayland-scanner
, wayland-protocols
  # the pinned wlroots 0.18.2 — MUST be passed explicitly; nixpkgs'
  # wlroots (0.20.1) would break the compile (different API)
, wlroots
, wayland
, libdrm
, libxkbcommon
, pixman
, libinput
, libglvnd
, linuxHeaders
}:

let
  # Cross stdenv: the gcc wrapper only exposes the prefixed binary name
  # (aarch64-unknown-linux-gnu-cc); there is no bare `cc` on PATH (same
  # note as pkgs/speaker-amp.nix).
  ccCmd = "${stdenv.cc}/bin/${stdenv.cc.targetPrefix}cc";
in
stdenv.mkDerivation rec {
  pname = "gemwl";
  version = "1.0"; # first NixOS port snapshot (sources dated 2026-09-04)

  src = ./gemwl;

  nativeBuildInputs = [
    wayland-scanner # build-machine tool (generates the client header)
    wayland-protocols # xdg-shell.xml, read at build time (build-machine copy)
  ];

  buildInputs = [
    wlroots # wlroots-0.18.pc + headers + libwlroots-0.18.so
    wayland # wayland-server.pc / wayland-client.pc
    libdrm # drm.h, drm_fourcc.h, panfrost_drm.h (headers only)
    libxkbcommon
    pixman # wlr public headers include <pixman-1/pixman.h>
    libinput # wlr/backend/libinput.h includes <libinput.h>
    libglvnd # EGL/GLES2/GLES3 headers + libEGL.so.1/libGLESv2.so.2
    linuxHeaders # <linux/input-event-codes.h>
  ];

  buildPhase = ''
    runHook preBuild

    # The source dir (pkgs/gemwl/) is a read-only store path, so all
    # generated files go to a writable scratch dir on the include path.
    gen=$TMPDIR/gen
    mkdir -p $gen

    # Version-identity banner (see header comment): deterministic, so
    # the derivation stays reproducible. gemwl.c picks it up via
    # __has_include("gemini-build-id.h") + -I$gen.
    cat > $gen/gemini-build-id.h <<EOF
    #ifndef GEMINI_BUILD_ID_H
    #define GEMINI_BUILD_ID_H
    #define GEMINI_BUILD_ID "gemini-nixos gemwl-${version} (nix build; wlroots 0.18.2 + mesa-geminipda)"
    #endif
    EOF

    # wlroots installs its public headers but NOT the generated protocol
    # headers, and wlr/types/wlr_xdg_shell.h includes the SERVER header
    # "xdg-shell-protocol.h" — so generate both here (exactly what
    # build.sh's proto step did for the Debian libwlroots-0.18-dev
    # layout). tinytest/wlegltst use the CLIENT header.
    wayland-scanner server-header \
      "${wayland-protocols}/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml" \
      $gen/xdg-shell-protocol.h
    wayland-scanner client-header \
      "${wayland-protocols}/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml" \
      $gen/xdg-shell-client-protocol.h

    # The xdg-shell interface *definitions* (xdg_wm_base_interface & co.)
    # live in a generated .c (the client header only declares them
    # extern). Compile it once and link it into the clients; NOT into
    # gemwl (wlroots provides the server-side interfaces itself).
    wayland-scanner private-code \
      "${wayland-protocols}/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml" \
      $gen/xdg-shell-private.c
    ${ccCmd} -c $gen/xdg-shell-private.c -o $gen/xdg-shell-private.o -I$gen -O2

    # No pkg-config on PATH in a cross build (the target wrapper is only
    # reachable through the prefixed name), and gemwl is one C file — so
    # pass the include dirs explicitly: wlroots installs headers under
    # include/wlroots-0.18 (its pc: -I''${includedir}/wlroots-0.18), and
    # libdrm under include/libdrm (drm.h / drm_fourcc.h / panfrost_drm.h
    # are headers-only for gemwl — the DRM device is driven with raw
    # ioctls, so no -ldrm). The remaining headers (wayland, xkbcommon,
    # EGL/GLES2/GLES3, linux/input-event-codes.h) come from the
    # buildInputs' dev outputs via the cc wrapper's -isystem, and -L/
    # -rpath for every input lib dir are added by the wrapper too.
    local incs="-I$gen -I${wlroots}/include/wlroots-0.18 -I${libdrm.dev}/include/libdrm -I${pixman}/include/pixman-1"
    local common="-O2 -Wall -Wno-unused-function -DWLR_USE_UNSTABLE $incs"

    # The compositor (links libwlroots + the glvnd EGL/GLES2 stubs).
    ${ccCmd} $common -o gemwl gemwl.c \
      -lwlroots-0.18 -lwayland-server -lxkbcommon -lEGL -lGLESv2 -lm

    # The xdg-shell smoke clients:
    #   tinytest        red wl_shm buffer (compositor event/debug probe)
    #   tinytest-anim   color-cycling frame-callback probe
    ${ccCmd} $common -o tinytest tinytest.c $gen/xdg-shell-private.o -lwayland-client
    ${ccCmd} $common -o tinytest-anim tinytest-anim.c $gen/xdg-shell-private.o -lwayland-client
    #   wlegltst        EGL wayland-platform client (the KWin/Qt buffer
    #                   path): needs wayland-egl + EGL/GLES2
    ${ccCmd} $common -o wlegltst wlegltst.c $gen/xdg-shell-private.o \
      -lwayland-client -lwayland-egl -lEGL -lGLESv2 -lm

    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    mkdir -p $out/bin
    install -m 755 gemwl tinytest tinytest-anim wlegltst $out/bin/
    runHook postInstall
  '';

  meta = with lib; {
    description = "gemwl — GPU-direct Wayland compositor for the Gemini PDA (wlroots 0.18)";
    longDescription = ''
      The verified GeminiPDA compositor: renders the Wayland scene graph
      with the Mali-T880 (panfrost, mesa-geminipda fork) straight into
      the LK bootloader framebuffer via /dev/gemfb, with a shadow-buffer
      compute blit. Sources are byte-identical copies of the GeminiPDA
      project's build/wayland/ files (gemwl.c + tinytest clients).
    '';
    homepage = "https://github.com/planet-computers"; # upstream: GeminiPDA project (local)
    license = licenses.unlicense;
    platforms = [ "aarch64-linux" ];
    maintainers = [ ];
  };
}

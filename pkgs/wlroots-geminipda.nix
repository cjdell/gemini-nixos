# wlroots 0.18.2 — the exact version the verified GeminiPDA desktop used.
#
# Why a standalone derivation (not nixpkgs' wlroots, which is 0.20.1 in
# the system pin): the phase-4 desktop is the custom `gemwl` compositor
# (pkgs/gemwl.nix), a single C file written against the wlroots 0.18
# API and verified on glass against Debian's libwlroots-0.18-dev. The
# 0.19/0.20 API differs (renderer/output/session changes), so gemwl.c
# would not even compile against the pin's wlroots — the feasibility
# doc's R2 mitigation is "ship the exact verified versions first".
#
# Build shape mirrors nixpkgs' own wlroots expression (the version-
# generic recipe in pkgs/development/libraries/wlroots/default.nix —
# strictDeps, depsBuildBuild pkg-config, wayland-scanner native) but
# scoped to exactly what gemwl uses, because this device has no DRM:
#
#   backends = libinput   (gemwl creates the multi backend, adds the
#                          libinput backend for keyboard/touch, and its
#                          OWN gemfb backend via wlr_backend_impl — no
#                          drm/x11 backends are compiled or used)
#   renderers = gles2     (wlr_renderer_autocreate -> GLES2; the pixman
#                          software renderer would defeat the GPU path)
#   session = enabled     (libseat + udev; wlr_session_create falls
#                          back to the direct session when run as root
#                          outside a logind seat, exactly like the
#                          verified Debian root service)
#
# gles2 renderer consequences (verified against wlroots-0.18.2
# meson.build while writing this):
#   - render/gles2 needs the `egl` AND `gbm` pkg-config modules (gbm is
#     hard-required when gles2 is requested; render/egl.c calls
#     gbm_create_device), so libgbm must come from mesa-geminipda
#     (-Dgbm=enabled — see pkgs/mesa-geminipda.nix) and EGL/glesv2 from
#     libglvnd (nixpkgs' libGL == libglvnd 1.7.0 in this pin, which
#     ships egl.pc/glesv2.pc + the EGL/GLES2/GLES3 headers incl.
#     GLES3/gl31.h that gemwl.c includes).
#   - the GLES2 shaders are embedded with a shell script (embed.sh), so
#     no glslang; only 0.19+ needs it.
#   - backend/session needs libudev (for the direct-session fallback
#     path, backend/session/direct.c) + libseat (seatd).
#   - 0.18 shaders/headers install under include/wlroots-0.18 and the
#     pkg-config module is `wlroots-0.18` (versioned_name) — gemwl's
#     build matches build.sh's WLR_PC discovery.
#
# All other deps are the wlroots 0.18.2 core set (wayland-server >=
# 1.23, libdrm >= 2.4.122, xkbcommon, pixman) plus wayland-client for
# the always-built nested-wayland backend.
{ lib
, stdenv
, fetchurl
, meson
, ninja
, pkg-config
, wayland-scanner
, wayland-protocols
, wayland
, libinput
, libxkbcommon
, pixman
, libdrm
, libGL
, systemd
, seatd
  # mesa 25.0.7 geminipda fork (gbm + EGL ICD provider; must be the
  # fork, NOT nixpkgs mesa — see pkgs/mesa-geminipda.nix)
, mesaGeminipda
}:

let
  version = "0.18.2";
in
stdenv.mkDerivation (finalAttrs: {
  pname = "wlroots-geminipda";
  inherit version;

  # Published base, fetched by hash (self-hosting pattern,
  # docs/library-deltas.md): the byte-stable canonical `/-/archive/`
  # tarball of the upstream tag 0.18.2 (NOT fetchFromGitLab's API
  # endpoint — byte-unstable on gitlab.freedesktop.org, same lesson as
  # the mesa derivation).
  src = fetchurl {
    url = "https://gitlab.freedesktop.org/wlroots/wlroots/-/archive/${version}/wlroots-${version}.tar.gz";
    hash = "sha256-cDxRWRfZ6yWORClnlfhiYZDhGbDui/sy/p+DTasaFGI=";
  };

  patches = [
    # libinput >= 1.28 added LIBINPUT_SWITCH_KEYPAD_SLIDE, which wlroots
    # 0.18.2's handle_switch_toggle does not enumerate — its default
    # -Werror turns that into a build failure with the pin's libinput
    # 1.31.3. Add the case (event dropped; no WLR_SWITCH_TYPE for it).
    ../patches/wlroots-0.18.2-libinput-1.28-keypad-slide.patch
  ];

  strictDeps = true;
  depsBuildBuild = [ pkg-config ];

  nativeBuildInputs = [
    meson
    ninja
    pkg-config
    wayland-scanner # native (build-machine); generates server protocol headers
  ];

  propagatedBuildInputs = [
    # wlroots headers #include <libinput.h>; consumers of wlroots need
    # it without listing it (same as nixpkgs' wlroots expression).
    libinput
  ];

  buildInputs = [
    libGL # egl.pc/glesv2.pc + EGL/GLES2/GLES3 headers (libglvnd in this pin)
    mesaGeminipda # gbm.pc + gbm.h + libgbm.so.1 (fork, -Dgbm=enabled)
    wayland # wayland-server.pc + wayland-client.pc (+ libwayland-egl bits)
    wayland-protocols # xdg-shell.xml etc. (pkgdatadir for protocol gen)
    libxkbcommon
    pixman
    libdrm
    systemd # libudev.pc (direct-session fallback; cross systemd = the
    # overlay's withLibBPF=false build, same as the rest of the system)
    seatd # libseat.pc (session; nixpkgs seatd package ships the libseat
    # client library + pkg-config)
  ];

  # nixpkgs' meson hook force-enables auto features by default
  # (-Dauto_features=enabled); that would pull every backend (drm/x11),
  # the vulkan renderer, xwayland, xcb-errors and color-management
  # (lcms2) into the build. This device needs none of them — disable
  # auto features and enable exactly the set above.
  mesonAutoFeatures = "disabled";

  mesonFlags = [
    "-Dbackends=libinput"
    "-Drenderers=gles2"
    "-Dsession=enabled"
    "-Dxwayland=disabled"
    "-Dxcb-errors=disabled"
    "-Dcolor-management=disabled"
    "-Dexamples=false"
  ];

  # wlroots 0.18 links with --version-script (wlroots.syms) which
  # versioned_name handles; nothing extra needed. Default outputs: a
  # single $out carrying lib + headers + the wlroots-0.18.pc module
  # (like nixpkgs' expression, which has no dev split for wlroots).

  meta = {
    description = "wlroots 0.18.2 (pinned for the gemwl compositor, no-DRM build)";
    longDescription = ''
      wlroots 0.18.2 built for the Gemini PDA desktop path: libinput
      backend + gles2 renderer + libseat session only (no drm/x11/xwayland/
      vulkan). Exactly the library version gemwl.c was verified against on
      the device (Debian libwlroots-0.18-dev). pkg-config module:
      wlroots-0.18. Do NOT use with the pin's newer nixpkgs wlroots.
    '';
    homepage = "https://gitlab.freedesktop.org/wlroots/wlroots";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
    maintainers = [ ];
  };
})

# labwc 0.8.3 — the exact version the verified GeminiPDA desktop ran nested
# inside gemwl (Debian apt labwc 0.8.3-1, on libwlroots0.18; LXQt/Wayland
# A/B, session logs 2026-09-02 + live LXQt on #329 2026-09-07).
#
# Why a standalone derivation (not nixpkgs' labwc, which is 0.20.0 in the
# system pin): the verified combo is labwc 0.8.x on wlroots 0.18
# (`dependency('wlroots-0.18', '>=0.18.1, <0.19.0')` in its meson.build),
# and this repo pins wlroots 0.18.2 (pkgs/wlroots-geminipda.nix) for
# gemwl. Newer labwc (0.9+) requires wlroots 0.19/0.20 APIs. So this
# mirrors the wlroots-geminipda pattern: published base fetched by hash
# (byte-stable GitHub `refs/tags/` archive — same self-hosting pattern as
# docs/library-deltas.md), built against the pinned wlroots 0.18.2.
#
# Build shape mirrors nixpkgs' own labwc expression but scoped to what
# this device needs (a nested compositor with no DRM/X11 of its own):
#
#   xwayland = disabled  — our wlroots is built -Dxwayland=disabled
#                          (labwc errors out if xwayland is requested but
#                          wlroots lacks it: "no wlroots Xwayland
#                          support"); LXQt's verified app set is
#                          Wayland-native anyway (docs R3 A/B note)
#   icon = disabled      — window icons need libsfdo-* (not in the pin's
#                          verified stack); skip, titlebars get the
#                          fallback icon
#   svg  = disabled      — svg window buttons need librsvg (also unused)
#   nls/man-pages/test   — disabled: no gettext/scdoc/cmocka
#
# Everything else is the 0.8.3 core set: wayland-server >= 1.19,
# wayland-protocols >= 1.35, xkbcommon, libxml-2.0, glib-2.0, cairo,
# pangocairo, libinput >= 1.14, pixman-1, libpng, libdrm (partial dep:
# compile args + includes only).
#
# Runtime contract (see services/lxqt.nix for the unit):
#   WLR_BACKENDS=wayland WAYLAND_DISPLAY=wayland-0  — nested on gemwl
#   -S lxqt-session                                  — LXQt is the session
#   -C <cfg dir>  rc.xml + autostart (config/lxqt/ in this repo)
#   decorations use the vendored openbox theme "Gemini" (themerc
#   installed under XDG data dirs; labwc reads
#   <data>/themes/Gemini/openbox-3/themerc — src/common/dir.c).
{ lib
, stdenv
, fetchurl
, meson
, ninja
, pkg-config
, wayland-scanner
, wayland-protocols
, wayland
, libxkbcommon
, libxml2
, glib
, cairo
, pango
, libdrm
, libinput
, pixman
, libpng
  # the pinned wlroots 0.18.2 (pkgs/wlroots-geminipda.nix) — MUST be
  # passed explicitly; nixpkgs' wlroots would not satisfy labwc 0.8.3's
  # `wlroots-0.18` dependency
, wlroots
}:

stdenv.mkDerivation (finalAttrs: {
  pname = "labwc-geminipda";
  version = "0.8.3"; # the on-glass-verified nested compositor (Debian apt 0.8.3-1)

  src = fetchurl {
    url = "https://github.com/labwc/labwc/archive/refs/tags/${finalAttrs.version}.tar.gz";
    hash = "sha256-dGvi/y0MDAt5XJf6JMcFj3VYZoXIihGUwkO2qEb5OKU=";
  };

  strictDeps = true;
  depsBuildBuild = [ pkg-config ];

  nativeBuildInputs = [
    meson
    ninja
    pkg-config
    wayland-scanner # generates the protocol headers at build time
  ];

  buildInputs = [
    wlroots # wlroots-0.18.pc (>=0.18.1,<0.19.0 satisfied by 0.18.2)
    wayland # wayland-server.pc >= 1.19
    wayland-protocols # xdg-shell + layer-shell etc. xml (>= 1.35)
    libxkbcommon
    libxml2 # libxml-2.0
    glib # glib-2.0
    cairo
    pango # pangocairo (SSD text/labels)
    libdrm # partial dep — headers/compile args only (no DT_NEEDED link)
    libinput # >= 1.14
    pixman
    libpng
  ];

  # nixpkgs' meson hook force-enables auto features by default
  # (-Dauto_features=enabled); disable and enable exactly the set above
  # (see the header comment — this device needs none of xwayland/icon/
  # svg/nls/man/test).
  mesonAutoFeatures = "disabled";

  mesonFlags = [
    "-Dxwayland=disabled"
    "-Dicon=disabled"
    "-Dsvg=disabled"
    "-Dnls=disabled"
    "-Dman-pages=disabled"
    "-Dtest=disabled"
  ];

  meta = {
    description = "labwc 0.8.3 (pinned for the nested LXQt session on gemwl)";
    longDescription = ''
      labwc 0.8.3 built for the Gemini PDA desktop: a Wayland stacking
      compositor running NESTED inside gemwl (WLR_BACKENDS=wayland),
      hosting the LXQt 2.x session. Built against the repo's pinned
      wlroots 0.18.2 (pkgs/wlroots-geminipda.nix) — the exact version
      pair verified on the device (Debian labwc 0.8.3-1 on
      libwlroots0.18). xwayland/icon/svg/nls disabled: the verified LXQt
      app set is Wayland-native (docs R3 A/B note). Do NOT use with the
      pin's newer nixpkgs labwc (0.20.0, wlroots 0.20 API).
    '';
    homepage = "https://labwc.github.io/";
    license = lib.licenses.gpl2Plus;
    platforms = lib.platforms.linux;
    maintainers = [ ];
  };
})

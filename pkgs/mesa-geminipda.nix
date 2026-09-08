# Mesa 25.0.7 + the geminipda panfrost fork (Mali-T880 / MT6797).
#
# Self-hosting pattern (long-standing goal; docs/library-deltas.md):
# published base + in-repo delta. The base is the published upstream
# mesa 25.0.7 archive (fetched by hash below); the delta is the single
# git-tracked fork patch. Nothing mesa is vendored in this repo.
#
# Why this standalone derivation rather than nixpkgs' `mesa` (26.1.4 in
# the system pin): the fork patch (patches/mesa-panfrost-geminipda-
# 25.0.7.patch, byte-for-byte the GeminiPDA project's submodule commit
# ac19be0 = mesa-25.0.7-1-gac19be0) is written against 25.0.7 panfrost
# and applies cleanly to the vanilla 25.0.7 tree only (different major
# from the pin's 26.x, and the 26.x recipe also drags in the desktop
# rusticl/LLVM stack). The fork is what makes
# EGL_EXT_image_dma_buf_import work on panfrost (rendering straight into
# the LK framebuffer) plus the polygon-list memset fix.
#
# Source recipe (matches the GeminiPDA mesa-cross README):
#   gitlab mesa-25.0.7 tarball  +  patches/mesa-panfrost-geminipda-25.0.7.patch
#   => byte-identical to the verified on-glass fork tree (all 5 touched
#      files cmp-verified 2026-09-05).
#
# Meson flags: exactly the verified cross-build set from
# build/mesa-cross/build-mesa.sh (surfaceless EGL-only, glvnd ICD
# layout, panfrost as the only gallium driver) + --sysconfdir=/etc.
#
# Runtime layout (NixOS) — meson 25.0.7 has no multiarch DRI dir
# option, so both the ICD and the monolithic DRI target install to
# $out/lib (same as nixpkgs' mesa derivation):
#   $out/lib/libEGL_mesa.so.0        glvnd ICD (EGL+GLES)
#   $out/lib/libgallium-25.0.7.so    DRI target with panfrost linked in
#   $out/lib/libgbm.so.1.0.0   libgbm for wlroots 0.18's gles2 (see note below)
#   $out/{etc,share}/glvnd/egl_vendor.d/50_mesa.json  ICD manifest
#       (absolute library_path). The $out/etc copy is wired into the
#       system /etc by config/gemini.nix (environment.etc — NixOS does
#       NOT merge package etc dirs automatically), which libglvnd's
#       compiled-in scan list always includes.
# libEGL_mesa.so.0 has a DT_NEEDED on libgallium-25.0.7.so (verified
# device layout, mesa-cross README: missing it = glvnd dlopen fails).
# NixOS has no ldconfig cache for the store, so postInstall appends the
# ICD's own lib dir to its DT_RUNPATH (--add-rpath: the build-time
# entries for libdrm/libm/libc are kept) — glibc resolves a dlopen'd
# library's dependencies against its own DT_RUNPATH. libgallium's own
# NEEDED entries are covered by its unmodified build-time RUNPATH.
# The libglvnd CLIENT libs (libEGL.so.1, libGLESv2.so.2) come from
# pkgs.libglvnd (added to the system by config/gemini.nix).
#
# gbm (added 2026-09-07 for the phase-4 desktop): wlroots 0.18's gles2
# renderer hard-requires the `gbm` pkg-config module + gbm.h
# (render/meson.build: gbm = dependency('gbm', required: 'gles2' in
# renderers)) and libwlroots ends up with a DT_NEEDED on libgbm.so.1
# (render/egl.c calls gbm_create_device). Building libgbm from THIS
# derivation (mesa 25.0.7 -Dgbm=enabled) keeps the "one mesa in the
# closure" property — the alternative is nixpkgs' mesa 26.1.4 (full
# desktop driver set, a much larger cross-build). This is a library
# addition only: no renderer code changes, and on the gemwl path gbm is
# linked but never exercised (gemwl uses its own dma-buf allocator;
# EGL_KHR_platform_gbm is absent on the surfaceless display).
{ lib
, stdenv
, buildPackages
, fetchurl
, meson
, ninja
, pkg-config
, libdrm
, libglvnd
, wayland
, wayland-protocols
, wayland-scanner
}:

let
  version = "25.0.7";
in
stdenv.mkDerivation (finalAttrs: {
  pname = "mesa-geminipda";
  inherit version;

  # Published base, fetched by hash (self-hosting pattern, docs/library-
  # deltas.md): the byte-stable canonical `/-/archive/` tarball of the
  # upstream tag mesa-25.0.7 — NOT nixpkgs' fetchFromGitLab, which hits
  # the GitLab *API* archive endpoint (/api/v4/...) that is byte-unstable
  # on gitlab.freedesktop.org (three fetches produced three different tar
  # bytes on 2026-09-05, breaking fixed-output hashes). This URL's bytes
  # are what was previously vendored in mesa/mesa-25.0.7.tar.gz (same
  # sha256 a0c8a2db...), so contents + the fork patch below stay
  # byte-identical to the verified on-glass fork tree.
  #
  # The delta on top of the base is ONLY the git-tracked fork patch
  # (patches/mesa-panfrost-geminipda-25.0.7.patch, byte-for-byte the
  # GeminiPDA submodule commit ac19be0, itself exactly one commit on the
  # mesa-25.0.7 tag). No mesa source is copied into this repo.
  src = fetchurl {
    url = "https://gitlab.freedesktop.org/mesa/mesa/-/archive/mesa-${version}/mesa-mesa-${version}.tar.gz";
    hash = "sha256-oMii2/mb9jm9nUKg9KdJkGt234Nx4R/bwoWJGKPoz5M=";
  };

  patches = [
    # geminipda panfrost fork: dma-buf import/export caps, polygon-list
    # memset (the M2 fix), debug/oracle hooks (PAN_TILERDBG etc.).
    ../patches/mesa-panfrost-geminipda-25.0.7.patch
  ];

  # Keep build-ids so drivers can use them for caching; nixpkgs notes
  # that some drivers segfault without this.
  separateDebugInfo = true;
  strictDeps = true;

  # nixpkgs' meson setup hook passes `-Dauto_features=enabled` unless
  # overridden. The verified reference build ran plain meson (auto
  # features stay `auto`). Under `enabled`, every auto feature is
  # force-on: `microsoft-clc` (Windows CLC compiler) flips
  # with_clc=true and meson demands the `libclc` dependency
  # (meson.build:850), and `xlib-lease` is force-enabled and its
  # require(X11 && KMS/DRM) becomes a hard error (meson.build:472).
  # Both failures hit in the 2026-09-05 builds.
  mesonAutoFeatures = "auto";

  nativeBuildInputs = [
    meson
    ninja
    pkg-config
    # patchelf runs on the build host (sets RUNPATH in postInstall). In
    # this cross package set a bare `patchelf` resolves to the aarch64
    # cross version ("cannot execute binary file: Exec format error"),
    # so take the build-machine one explicitly.
    buildPackages.patchelf
    # panfrost's compiler frontend (pan_compiler) has bison/flex
    # grammar files; meson.build:2014 requires bison (or byacc) + flex.
    # Build-machine tools (run during the build; same as nixpkgs' mesa
    # derivation, which lists plain bison/flex).
    buildPackages.bison
    buildPackages.flex
    # Mesa 25.0.7's meson.build runs python3 (mako >= 0.8.0, PyYAML,
    # packaging) at configure time for code generation. find_program
    # falls back to meson's own interpreter when no python3 is on the
    # build PATH, and that one has none of these modules — so put a
    # fully provisioned interpreter on the PATH (same set nixpkgs' mesa
    # derivation uses). It must be a BUILD-machine python (x86_64):
    # configure runs on the build host, and the plain `python3` in a
    # cross derivation is the aarch64 target python.
    (buildPackages.python3.withPackages
      (ps: with ps; [ mako pyyaml packaging ]))
    # EGL wayland platform codegen (mesa 25 vendors wayland-drm.xml, but
    # needs wayland.xml + the wayland-scanner binary at build time;
    # nixpkgs splits the scanner into its own wayland-scanner package).
    buildPackages.wayland
    buildPackages.wayland-protocols
    buildPackages.wayland-scanner
  ];

  # wayland's + the scanner's .pc files live ONLY in their -dev outputs
  # (wayland-protocols' sits in share/pkgconfig); the pkg-config wrapper
  # role vars don't reliably surface all of them to mesa 25's build-time
  # dependency() lookups (meson.build:2054 wayland-scanner, :2061
  # wayland-protocols). Seed both role vars directly — the plain + _FOR_BUILD
  # split is what tripped each of the two lookups in turn. [2026-09-08]
  env = {
    PKG_CONFIG_PATH =
      "${wayland.dev}/lib/pkgconfig:${wayland-scanner.dev}/lib/pkgconfig:${wayland-protocols}/share/pkgconfig";
    PKG_CONFIG_PATH_FOR_BUILD =
      "${wayland.dev}/lib/pkgconfig:${wayland-scanner.dev}/lib/pkgconfig:${wayland-protocols}/share/pkgconfig";
  };

  buildInputs = [
    libdrm # panfrost (dep_libdrm)
    libglvnd # -Dglvnd=true (dep_glvnd)
    wayland # libwayland-client (DT_NEEDED of libEGL_mesa with the wayland platform)
  ];

  mesonFlags = [
    "--sysconfdir=/etc"

    # --- what to build (verified set, see build-mesa.sh) -------------
    "-Dplatforms=wayland" # + wayland EGL platform (third-party GL clients: Firefox/Chrome WebGL + the wlegltst smoke client — Debian-parity, 2026-09-04 Firefox-WebGL session; without it the fork EGL cannot serve browser GL). surfaceless stays via -Degl-native-platform below (gemwl/tinytest/wlroots-gles2 path unchanged)
    "-Degl-native-platform=surfaceless"
    "-Dgallium-drivers=panfrost"
    "-Dvulkan-drivers="
    "-Dgallium-opencl=disabled"
    "-Dgbm=enabled"   # libgbm.so.1 + gbm.h (wlroots 0.18 gles2; see header)
    # Vulkan X11 extension — irrelevant here (no vulkan, no X11).
    # Pinned explicitly as a second line of defense next to
    # mesonAutoFeatures = "auto" (see above); the verified
    # build-mesa.sh left it unset.
    "-Dxlib-lease=disabled"
    "-Dglx=disabled"
    "-Degl=enabled"
    "-Dopengl=false"
    "-Dgles1=enabled"
    "-Dgles2=enabled"
    "-Dglvnd=true"
    "-Dglvnd-vendor-name=mesa"
    "-Dshared-glapi=enabled"

    # --- off-by-default-on-device cruft ------------------------------
    "-Dzlib=disabled"
    "-Dzstd=disabled"
    "-Dshader-cache=disabled"
    "-Dvalgrind=disabled"
    "-Dlibunwind=disabled"
    "-Dlmsensors=disabled"
    "-Dosmesa=false"
    "-Dgallium-nine=false"
    "-Dgallium-va=disabled"
    "-Dgallium-vdpau=disabled"
    "-Dgallium-xa=disabled"
    "-Dgallium-extra-hud=false"
    "-Dteflon=false"
    "-Dgpuvis=false"
    "-Dvmware-mks-stats=false"
    "-Dxmlconfig=disabled"
    "-Dsplit-debug=disabled"
    "-Dbuild-tests=false"

    # --- nix conventions ---------------------------------------------
    "-Db_ndebug=true"
    "-Db_staticpic=true"
  ];

  postInstall = ''
    # The glvnd ICD manifest must carry an absolute library_path: the
    # store is not in the dynamic linker's default search path.
    # (Same rewrite nixpkgs' mesa derivation does in postFixup.)
    for js in $out/share/glvnd/egl_vendor.d/*.json; do
      substituteInPlace "$js" --replace-fail '"libEGL_' '"'"$out/lib/libEGL_"
    done

    # Resolve the ICD's DT_NEEDED on libgallium-25.0.7.so without an
    # ldconfig cache: append its own lib dir to the ICD's RUNPATH (glibc
    # resolves a dlopen'd library's dependencies against its own
    # DT_RUNPATH). Must be --add-rpath, not --set-rpath: the build-time
    # RUNPATH already carries the store paths for the ICD's other
    # NEEDED entries (libdrm.so.2, libm, libc) and replacing it would
    # make those unresolvable.
    ${buildPackages.patchelf}/bin/patchelf --add-rpath $out/lib \
      $out/lib/libEGL_mesa.so.0.0.0

    # libglvnd's compiled-in scan list is
    # /run/opengl-driver/share/glvnd/egl_vendor.d (hardware.graphics —
    # off on this system), /etc/glvnd/egl_vendor.d and
    # /usr/share/glvnd/egl_vendor.d (no /usr on NixOS). Install the
    # manifest under $out/etc; config/gemini.nix points
    # environment.etc."glvnd/egl_vendor.d/50_mesa.json" at it.
    mkdir -p $out/etc/glvnd/egl_vendor.d
    cp -v $out/share/glvnd/egl_vendor.d/50_mesa.json \
      $out/etc/glvnd/egl_vendor.d/
  '';

  meta = {
    description = "Mesa 25.0.7 + geminipda panfrost fork (Mali-T880, dma-buf import)";
    longDescription = ''
      Upstream Mesa 25.0.7 plus the geminipda panfrost fork:
      EGL_EXT_image_dma_buf_import support on panfrost (rendering into
      the LK framebuffer without a CPU pixel round-trip) and the
      polygon-list whole-BO memset fix. Surfaceless EGL-only build with
      the glvnd ICD layout (libEGL_mesa.so + 50_mesa.json).
    '';
    homepage = "https://www.mesa3d.org/";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
    maintainers = [ ];
  };
})

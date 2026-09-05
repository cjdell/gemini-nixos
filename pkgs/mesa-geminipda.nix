# Mesa 25.0.7 + the geminipda panfrost fork (Mali-T880 / MT6797).
#
# Why a standalone derivation instead of nixpkgs' `mesa` (26.1.4 in this
# pin): the fork patch (patches/mesa-panfrost-geminipda-25.0.7.patch,
# byte-for-byte the GeminiPDA project's submodule commit ac19be0) is
# written against 25.0.7 panfrost and applies cleanly to the vanilla
# 25.0.7 tarball only. The fork is what makes
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
{ lib
, stdenv
, buildPackages
, meson
, ninja
, pkg-config
, libdrm
, libglvnd
}:

let
  version = "25.0.7";
in
stdenv.mkDerivation (finalAttrs: {
  pname = "mesa-geminipda";
  inherit version;

  # Vendored tarball (bin/snapshot-mesa.sh), same pattern as the kernel
  # snapshot: the GitLab *API* archive endpoint used by fetchFromGitLab
  # is byte-unstable (three fetches produced three different tar bytes
  # on 2026-09-05), so the source is pinned locally. This copy is the
  # canonical `/-/archive/` tarball of tag mesa-25.0.7
  # (sha256 a0c8a2dbf99bf639bd9d42a0f4a749906b76df8371e11fdbc2858918a3e8cf93),
  # whose contents + the fork patch below are byte-identical to the
  # verified on-glass fork tree.
  src = ../mesa/mesa-${version}.tar.gz;

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
  ];

  buildInputs = [
    libdrm # panfrost (dep_libdrm)
    libglvnd # -Dglvnd=true (dep_glvnd)
  ];

  mesonFlags = [
    "--sysconfdir=/etc"

    # --- what to build (verified set, see build-mesa.sh) -------------
    "-Dplatforms=" # no X11/Wayland EGL platforms
    "-Degl-native-platform=surfaceless"
    "-Dgallium-drivers=panfrost"
    "-Dvulkan-drivers="
    "-Dgallium-opencl=disabled"
    "-Dgbm=disabled"
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

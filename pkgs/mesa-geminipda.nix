# Mesa 26.2.2 + the geminipda Mali-T880 panfrost delta — the ONE Mesa
# the Gemini PDA uses for EGL/GL/GBM.
#
# This used to be a standalone Mesa 25.0.7 build carrying three classes
# of change: (1) the panfrost dma-buf IMPORT|EXPORT caps, (2) a whole-BO
# polygon-list memset for the T880 tiler, (3) an assortment of debug
# hooks (PAN_TILERDBG, tiler-census dumps). On the pinned nixpkgs Mesa it
# is a THIN override instead, because:
#
#   - (1) is obsolete upstream: the generic u_init_pipe_screen_caps()
#     now derives caps->dmabuf from the kernel's DRM_CAP_PRIME
#     (src/gallium/auxiliary/util/u_screen.c); the 25.0.7 fork had to
#     patch pan_screen.c because it predated that helper.
#   - (3) was bring-up scaffolding, not a shipping delta.
#   - (2) is still carried: patches/mesa-panfrost-polygon-list-26.2.2.patch
#     (upstream zeroes the polygon list only when there are no draws and
#     otherwise WRITE_VALUEs the first word; stale empty-bin headers on a
#     reused BO drop far bins on the T880).
#
# Why a thin override and not a bespoke build: the 2026-09-10 dual-vendor
# incident. With hardware.graphics on (nixpkgs default) glvnd got TWO
# mesa ICDs — the nixpkgs 26.2.2 vendor and the fork's 25.0.7 one — and
# cosmic-comp (and other compositors) ended up with libgallium-25.0.7
# AND libgallium-26.2.2
# loaded in one process, unable to import buffers between them
# ("import for wrong devices"), falling back to a CPU composition path.
# One version, one ICD, one driver is the fix; keeping the delta a thin
# override means the device tracks the pinned nixpkgs Mesa instead of
# drifting years behind. See docs/library-deltas.md and
# docs/handover-2026-09-10-gnome-perf-touch.md.
#
# Wiring: config/gemini.nix sets hardware.graphics.package to this
# derivation, so /run/opengl-driver (and its 50_mesa.json ICD) is the
# patched build and no /etc/glvnd fork manifest exists any more. The
# gemwl/gemshell stack also consumes it as `mesaGeminipda` for
# both EGL and GBM (libgbm-external=false bundles libgbm, exactly like
# the old fork, so those packages keep their single-mesa contract).
{ pkgs }:

let
  lib = pkgs.lib;
in
(pkgs.mesa.override {
  # Lean: the device renders on panfrost (Mali-T880). No Vulkan, no
  # patent-encumbered codecs, no valgrind instrumentation — the same
  # shape as the old fork's verified minimal build.
  galliumDrivers = [ "panfrost" ];
  vulkanDrivers = [ ];
  enablePatentEncumberedCodecs = false;
  withValgrind = false;
}).overrideAttrs (old: {
  # Dozen (spirv2dxil) is not built with vulkanDrivers = [ ]; drop its
  # (empty) output so the builder does not demand it.
  outputs = builtins.filter (o: o != "spirv2dxil") (old.outputs or [ "out" "opencl" "cross_tools" ]);

  patches = (old.patches or [ ]) ++ [
    ../patches/mesa-panfrost-polygon-list-26.2.2.patch
  ];

  # nixpkgs forces -Dauto_features=enabled, which switches on state
  # trackers (gallium-va, ...) whose driver requirements a panfrost-only
  # build cannot satisfy (configure fails with "Feature gallium-va
  # cannot be enabled"). "auto" + the explicit disables reproduces the
  # old fork's verified minimal feature set.
  mesonAutoFeatures = "auto";
  mesonFlags = (old.mesonFlags or [ ]) ++ [
    (lib.mesonEnable "gallium-va" false)
    (lib.mesonBool "teflon" false)
    (lib.mesonEnable "intel-rt" false)
    (lib.mesonOption "vulkan-layers" "")
    # Bundle libgbm (and gbm.pc/gbm.h) rather than using the separate
    # mesa-libgbm package: keeps `mesaGeminipda` self-contained for
    # wlroots/gemwl and the browser GBM_BACKENDS_PATH, on the SAME
    # 26.2.2 base as the EGL driver.
    (lib.mesonBool "libgbm-external" false)
  ];
})

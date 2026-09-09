# wine-x86.nix — the x86-64-Windows-on-PDA stack, evaluated from the
# SAME nixpkgs channel rev the flake pins (dc5d91f84032, 26.11pre1068949,
# verified cache-healthy per AGENTS.md rule 9). See docs/wine-d3d.md.
#
# Attributes (all hydra-cached — verified 2026-09-09):
#   wine64   x86_64-linux wine 11.0 (the Windows emulator; runs under
#            box64 on the PDA). 1.8 GB closure. NOTE: the wine64 closure
#            carries glvnd (libEGL dispatch) but NO GL implementation —
#            on a normal box glvnd finds the distro's ICD; on the PDA
#            we ship `mesa` (below) and point the guest at its glvnd
#            manifest via __EGL_VENDOR_LIBRARY_FILE (docs/wine-d3d.md).
#   box64    aarch64-linux box64 0.4.4 (x86-64 binary translator).
#   d3d9test x86_64-windows PE (mingw-w64 cross; ./d3d9test.nix).
#   mesa     x86_64-linux mesa 26.2.2 (gallium: panfrost_dri + swrast;
#            eglPlatforms x11+wayland) — the GL impl for guest wined3d.
#   grim     aarch64-linux wayland screenshot (receipts for on-glass runs).
#
# This is deliberately NOT part of the flake's aarch64 package set:
# the wine64 closure is 1.8 GB and would bloat every system deploy;
# bin/wine-x86-deploy.sh ships it to the device as a standalone GC
# root instead.
let
  np = builtins.fetchTree {
    type = "tarball";
    url = "https://github.com/NixOS/nixpkgs/archive/dc5d91f840324650bac8c379428c7037a416959a.tar.gz";
    narHash = "sha256-VaWGJ6+cIYN2erfSecbRV+4ljI185Ty2wUrXyvQbgOw=";
  };
  x64 = import np {
    system = "x86_64-linux";
    # allowUnsupportedSystem: needed for the mingw-w64 cross stdenv.
    config = { allowUnsupportedSystem = true; };
  };
  a64 = import np { system = "aarch64-linux"; };
in
{
  wine64 = x64.wine64;
  box64 = a64.box64;
  d3d9test = x64.pkgsCross.mingwW64.callPackage ./d3d9test.nix { };
  mesa = x64.mesa;
  grim = a64.grim;
}

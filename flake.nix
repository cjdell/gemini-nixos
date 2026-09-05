# Mobile NixOS for the Planet Computers Gemini PDA (MT6797X).
#
# Build (from an x86_64 host, cross-compiling to aarch64):
#
#   nix build .#packages.x86_64-linux.default  # boot+recovery+system img, flash script
#   nix build .#packages.x86_64-linux.bootimg  # boot.img only
#   nix build .#packages.x86_64-linux.rootfs   # rootfs.img only
#   nix build .#packages.x86_64-linux.initrd   # stage-1 initrd (size measurement, docs R1)
#
# Flashing is manual (no fastboot on this device): dd the images to the
# `boot` and `linux` partitions via the patched TWRP, or via adb. See
# README.md and docs/mobile-nixos-port-feasibility.md §3.5.
#
# Mobile NixOS is not a flake, so we import its evaluation entry point
# from the in-tree clone at repos/mobile-nixos (pinned commit, see
# README.md). Nixpkgs is resolved by Mobile NixOS's own npins pin
# (nixos-unstable 26.11pre1031299.0bb7ec54c848).
{
  description = "Mobile NixOS for the Planet Computers Gemini PDA (MT6797X, LK framebuffer, no DRM)";

  outputs = { self, ... }:
    let
      # Cross-build host. The device is aarch64; the supported workflow is
      # cross-building from x86_64 (docs R7, examples/hello precedent).
      buildSystem = "x86_64-linux";

      mnx = ./repos/mobile-nixos;

      # Out-of-tree device (path-based, per mobile-nixos
      # lib/release-tools.nix) plus the system configuration.
      # `nixpkgs.buildPlatform` is passed explicitly, following the
      # release.nix pattern, so the cross setup does not depend on flake
      # purity or the local system.
      eval = import (mnx + "/lib/eval-with-configuration.nix") {
        system = buildSystem;
        device = ./devices/planet-geminipda;
        configuration = [
          { nixpkgs.buildPlatform = buildSystem; }
          ./config/gemini.nix
        ];
      };

      inherit (eval) outputs;

      # Minimal busybox initrd (docs R1 — the Mobile NixOS stage-1 does
      # not fit the 16 MiB boot partition). Same derivation the device
      # module wires into mobile.outputs.initrd.
      initrd = (import ./devices/planet-geminipda/initrd.nix) {
        inherit (eval) pkgs;
      };

      # Mesa 25.0.7 + geminipda panfrost fork (also in the system closure
      # via config/gemini.nix; standalone here for size/iteration checks).
      mesa = eval.pkgs.callPackage ./pkgs/mesa-geminipda.nix { };
    in
    {
      packages.${buildSystem} = {
        # android-fastboot-images: boot.img + system.img + flash script
        # (no recovery partition on this device; see the device module).
        default = outputs.default;

        bootimg = outputs.android.android-bootimg;
        rootfs  = outputs.generatedFilesystems.rootfs;
        initrd  = initrd; # minimal busybox initrd, for size measurement
        toplevel = outputs.toplevel;
        mesa    = mesa;   # geminipda panfrost fork (Mali-T880, dma-buf import)
      };

      devShells.${buildSystem}.default =
        let
          pkgs = import (mnx + "/pkgs.nix") { system = buildSystem; };
        in
        pkgs.mkShell {
          packages = with pkgs; [
            python3 # boot.img header inspection
            git
          ];
        };
    };
}

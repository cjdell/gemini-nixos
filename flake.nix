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
# Mobile NixOS is NOT a flake (no flake.nix, even at branch tip), so on
# nix 2.34 it cannot be a flake input (inputs must contain a flake.nix).
# We fetch the pinned commit as a tarball instead — the same mechanism
# Mobile NixOS itself uses to pin nixpkgs (npins, builtins.fetchTarball).
# The full commit SHA in the URL is the pin (deterministic tarball).
# Nixpkgs is then resolved by Mobile NixOS's own npins pin
# (nixos-unstable 26.11pre1031299.0bb7ec54c848).
{
  description = "Mobile NixOS for the Planet Computers Gemini PDA (MT6797X, LK framebuffer, no DRM)";

  outputs = { self, ... }:
    let
      # Cross-build host. The device is aarch64; the supported workflow is
      # cross-building from x86_64 (docs R7, examples/hello precedent).
      buildSystem = "x86_64-linux";

      # Mobile NixOS source tree, pinned to an exact commit (was: the
      # repos/mobile-nixos submodule). Bump the SHA to update.
      # NOTE: do not bind `inputs` in the outputs pattern on nix 2.34 —
      # the pattern's bindings are treated as input declarations, and a
      # bare `inputs` binding spawns a phantom "flake:inputs" registry
      # lookup that fails ("cannot find flake 'flake:inputs'").
      # nix 2.34: builtins.fetchTarball no longer pins content (its old
      # `sha256` file-hash arg is gone); use fetchTree, which pins the
      # NAR hash of the UNPACKED tree (GitHub tarball bytes are not a
      # canonical representation). Re-verify the narHash when bumping
      # the rev:  nix flake prefetch github:mobile-nixos/mobile-nixos/<rev>
      mnx = builtins.fetchTree {
        type = "tarball";
        url =
          "https://github.com/mobile-nixos/mobile-nixos/archive/2c132754323fc1915e8d21dcfc0ef68ab084c6fb.tar.gz";
        narHash = "sha256-CzwmiKxuh1u+H8hDnrVeUl/fL59PvsgLbr2l0FhqWK0=";
      };

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

      # Phase 4 desktop stack (wlroots 0.18.2 + gemwl + tinytest clients;
      # in the system closure via services/desktop.nix — standalone here
      # for standalone builds/checks). Same callPackage args as the
      # service module, so the flake packages and the system share paths.
      wlroots = eval.pkgs.callPackage ./pkgs/wlroots-geminipda.nix {
        mesaGeminipda = mesa;
      };
      gemwl = eval.pkgs.callPackage ./pkgs/gemwl.nix {
        inherit wlroots;
      };
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
        wlroots = wlroots; # wlroots 0.18.2 pin (gemwl's library; R2)
        gemwl   = gemwl;   # GPU-direct fb compositor + tinytest clients
      };

      devShells.${buildSystem}.default =
        let
          pkgs = import (mnx + "/pkgs.nix") { system = buildSystem; };
        in
        pkgs.mkShell {
          packages = with pkgs; [
            python3 # boot.img header inspection
            git
            android-tools # adb/fastboot — host-side flash/recovery tooling (bin/boot-switch.sh, bin/flash-nixos.sh)
            usbutils # lsusb — device-state detection (POC 0e8d:2008, preloader 0e8d:2000, BROM 0e8d:0003, TWRP 18d1:4ee2)
          ];
        };
    };
}

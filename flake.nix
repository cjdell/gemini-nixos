# Mobile NixOS for the Planet Computers Gemini PDA (MT6797X).
#
# Build model = NATIVE aarch64 (canonical since the native-aarch64 merge
# 2026-09-08; formerly a branch + worktree, now main): every drv is
# system = aarch64-linux — built on the aarch64 remote builder
# (192.168.49.191, /etc/nix/machines ssh://cjdell@…; run as root with
# `--store local` + `--option builders @/etc/nix/machines --fallback`,
# e.g. bin/deploy.sh build) with cache.nixos.org substitution where the
# pinned nixpkgs rev is cached. NOTE: this nixpkgs snapshot
# (26.11pre1031299.0bb7ec54c848) is NOT on hydra's cache (native x86_64
# + aarch64 narinfo both 404), so most of the Qt6/LXQt closure compiles
# on the builder regardless. The CROSS toplevel model (x86_64 host,
# buildSystem=x86_64-linux) is ABANDONED — it hit the nixpkgs cross
# walls (Qt6CoreTools missing for the lxqt scope, etc.; see
# docs/handover-2026-09-07-lxqt-native.md). Native aarch64 drvs hash-
# match nothing cross-built (no shared store paths with pre-merge gens).
#
#   nix build .#packages.aarch64-linux.default  # boot+recovery+system img (native aarch64 drvs)
#   nix build .#packages.aarch64-linux.bootimg  # boot.img only
#   nix build .#packages.aarch64-linux.rootfs   # rootfs.img only
#   nix build .#packages.aarch64-linux.initrd   # stage-1 initrd (size measurement, docs R1)
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
      # Native build target = aarch64 (this branch). drvs are
      # system=aarch64-linux: nix routes them to the 192.168.49.191
      # remote builder (machines file) or substitutes from cache.nixos.org.
      buildSystem = "aarch64-linux";

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

      # Out-of-tree device + system configuration. `nixpkgs.buildPlatform`
      # = aarch64-linux: native eval (build machine == device arch).
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
      # labwc 0.8.3 pinned against the wlroots 0.18.2 line (nixpkgs'
      # labwc is 0.20/0.20-wlroots — too new for this stack). Hosts the
      # nested LXQt session (services/lxqt.nix). labwc 0.8.3 compiles
      # wlr_drm_lease_v1 unconditionally, which requires a
      # drm-backend-enabled wlroots BUILD (header only — labwc runs
      # nested and never opens a DRM device; see pkgs/wlroots-
      # geminipda.nix withDrmBackend).
      labwc = eval.pkgs.callPackage ./pkgs/labwc-geminipda.nix {
        wlroots = eval.pkgs.callPackage ./pkgs/wlroots-geminipda.nix {
          mesaGeminipda = mesa;
          withDrmBackend = true;
        };
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
        # Self-contained bring-up kernel (Linux v6.6 base + in-repo delta;
        # source model docs/library-deltas.md). Same derivation the device
        # module wires into mobile.boot.stage-1.kernel, exposed for
        # standalone builds/iteration (the toplevel/bootimg consume it).
        kernel  = eval.pkgs.callPackage ./devices/planet-geminipda/kernel { };
        toplevel = outputs.toplevel;
        mesa    = mesa;   # geminipda panfrost fork (Mali-T880, dma-buf import)
        wlroots = wlroots; # wlroots 0.18.2 pin (gemwl's library; R2)
        gemwl   = gemwl;   # GPU-direct fb compositor + tinytest clients
        labwc   = labwc;   # 0.8.3 nested compositor (pinned wlroots 0.18.2)
      };

      # Host tooling stays x86_64 (this flake is evaluated from an x86_64
      # host; the native aarch64 builds go to the remote builder).
      devShells.x86_64-linux.default =
        let
          pkgs = import (mnx + "/pkgs.nix") { system = "x86_64-linux"; };
        in
        pkgs.mkShell {
          packages = with pkgs; [
            python3 # boot.img header inspection
            git
            android-tools # adb/fastboot — host-side flash/recovery tooling (bin/boot-switch.sh, bin/flash-nixos.sh)
            usbutils # lsusb — device-state detection (POC 0e8d:2008, preloader 0e8d:2000, BROM 0e8d:0003, TWRP 18d1:4ee2)
            mtkclient # preloader/BROM recovery tooling (store pkg + Loader DAs for bin/run-mtk.sh; the CDC-ACM patched copy shadows it — docs/disaster-recovery/)
          ];
        };
    };
}

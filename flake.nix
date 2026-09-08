# Mobile NixOS for the Planet Computers Gemini PDA (MT6797X).
#
# Build model = NATIVE aarch64 (canonical since the native-aarch64 merge
# 2026-09-08; formerly a branch + worktree, now main): every drv is
# system = aarch64-linux — built on the aarch64 remote builder
# (192.168.49.191, /etc/nix/machines ssh://cjdell@…; run as root with
# `--store local` + `--option builders @/etc/nix/machines --fallback`,
# e.g. bin/deploy.sh build) with cache.nixos.org substitution. NOTE:
# nixpkgs is pinned BY THIS FLAKE (below), not by MNX's npins, since
# 2026-09-08 — to the nixos-unstable CHANNEL snapshot dc5d91f84032
# (26.11pre1068949, cut 2026-09-07), whose FULL aarch64 closure hydra
# built and published (qtbase/qtwayland/lxqt-*/systemd/pipewire/…
# narinfos all 200, verified 2026-09-08). Under the old npins rev
# (0bb7ec54c848) those compiled outputs were 404 for x86_64 AND aarch64
# (base closure only — glibc 200, qtbase 404), so Qt6/LXQt compiled
# from source every time. The only remaining local compiles are the
# custom drvs: mesa-geminipda fork, wlroots/labwc/gemwl pins, the
# kernel, gemini-firmware. The CROSS toplevel model (x86_64 host,
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
# Nixpkgs is NOT taken from Mobile NixOS's npins anymore (its pin,
# nixos-unstable 26.11pre1031299.0bb7ec54c848, is only base-closure-
# cached on hydra — see the header): this flake pins its own rev below
# and hands it to the MNX eval via the eval shim's `pkgs` argument
# (the shim forbids system + pkgs together; system is carried by pkgs).
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

      # Nixpkgs pin (2026-09-08 repin — see the header): the current
      # nixos-unstable CHANNEL snapshot, i.e. the newest rev whose FULL
      # closure hydra published (raw master commits newer than the
      # channel cut only get per-commit trunk-combined coverage — the
      # exact slow situation this repin fixes). Bump by taking the rev
      # behind https://channels.nixos.org/nixos-unstable/git-revision,
      # then re-verify the narHash:
      #   nix flake prefetch github:NixOS/nixpkgs/<rev>
      nixpkgs = builtins.fetchTree {
        type = "tarball";
        url =
          "https://github.com/NixOS/nixpkgs/archive/dc5d91f840324650bac8c379428c7037a416959a.tar.gz";
        narHash = "sha256-VaWGJ6+cIYN2erfSecbRV+4ljI185Ty2wUrXyvQbgOw=";
      };

      # Out-of-tree device + system configuration. `nixpkgs.buildPlatform`
      # = aarch64-linux: native eval (build machine == device arch). The
      # MNX eval shim builds pkgs from its own npins unless `pkgs` is
      # passed; pass ours (system comes from pkgs.stdenv.hostPlatform,
      # and the nixpkgs module re-imports the same source via pkgs.path,
      # so module pkgs == eval pkgs == this rev).
      eval = import (mnx + "/lib/eval-with-configuration.nix") {
        pkgs = import nixpkgs { system = buildSystem; };
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
      # gemini xkb symbols (layout "gemini", UK default / us variant;
      # config/xkb/symbols/gemini, vendored byte-identical from
      # GeminiPDA) as an xkbcommon include dir — wired into the gemwl +
      # lxqt-nested units via XKB_CONFIG_EXTRA_PATH (services/desktop.nix,
      # services/lxqt.nix). Standalone here for keymap checks
      # (xkbcli compile-keymap --layout gemini -I <dir>).
      geminiXkb = eval.pkgs.callPackage ./pkgs/gemini-xkb.nix { };
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
        gemini-xkb = geminiXkb; # xkb layout include dir (XKB_CONFIG_EXTRA_PATH)
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

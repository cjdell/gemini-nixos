# Kernel derivation for the Gemini PDA bring-up kernel.
#
# Source: the `geminipda-bringup` branch of the local linux-6.6 clone
# (GeminiPDA project), pinned to a commit and snapshotted with `git archive`
# (see ../../../bin/snapshot-kernel.sh). The snapshot is clean — no .git,
# no build artifacts — which is what the Mobile NixOS kernel-builder
# expects (it runs `make O=build` and verifies the resulting
# `kernel.release` against `version` below; without .git, setlocalversion
# produces no suffix).
#
# Config: `config.aarch64` is the exact .config of the working on-glass
# bring-up build (GeminiPDA build/out-6.6/config). It still carries
# `CONFIG_CMDLINE_FORCE=y` with the full verified cmdline; moving the
# cmdline into `boot.kernelParams` (docs R4) is a deliberate A/B step on
# hardware, not part of the initial port.
{ mobile-nixos
, buildPackages
, ...
}:

let
  # Unpacks on the build machine (x86_64), so use buildPackages tools —
  # the target stdenv here is aarch64 and its binaries won't run locally.
  kernelSrc = buildPackages.runCommand "linux-geminipda-6.6-src" {
    nativeBuildInputs = [ buildPackages.gnutar buildPackages.gzip ];
  } ''
    mkdir $out
    ${buildPackages.gzip}/bin/gunzip -c ${../../../kernel/geminipda-bringup-733c0c7ea.tar.gz} | ${buildPackages.gnutar}/bin/tar -x -C $out
  '';

  # Out-of-tree A72 bring-up module (sramldo-smc.ko), built against this
  # exact tree in postInstall and shipped in the module dir (phase 3).
  # `let` so the postInstall string can interpolate them (Nix string
  # interpolation does not see sibling derivation attrs). modDirVersion
  # is the builder's `modDirify version`, which is the identity here.
  sramldoModuleSrc = ./modules/sramldo-smc;
  modDirVersion = "6.6.0";
in

mobile-nixos.kernel-builder {
  src = kernelSrc;
  # Makefile: VERSION=6 PATCHLEVEL=6 SUBLEVEL=0, no localversion.
  version = "6.6.0";
  configfile = ./config.aarch64;

  # The stock MediaTek LK bootloader gunzips the kernel payload and scans
  # its last 2 MiB for the DTB (docs §3.1-3.2). So: plain Image.gz here;
  # the DTB is appended by the android system type via `appendDTB`
  # (do NOT use isImageGzDtb: bootimg.nix consumes `target` = Image.gz).
  isCompressed = "gz";
  buildDTBs = true;

  # The bring-up config has 1018 =m entries (panfrost, pwm-mtk-disp,
  # usb configfs, ...). Build the module tree so stage-2 can use them;
  # stage-1 itself stays non-modular (16 MiB boot partition, docs R1).
  isModular = true;

  # GCC 15 (nixpkgs 26.11pre) vs a 6.6 tree: mask -Werror failures.
  # The current bring-up pipeline cross-compiles the same tree with
  # GCC 15.2.0 successfully (docs §3.6); this is insurance for the
  # mobile-nixos builder's slightly different flag handling.
  enableRemovingWerror = true;

  # FIX (2026-09-05): the builder's prePatch sed rewires the top-level
  # `DEPMOD ?= ...` make variable, but the modules_install rule actually
  # runs `scripts/depmod.sh`, which looks up `depmod` in the *PATH* and
  # silently skips when it is missing ("Warning: 'make modules_install'
  # requires depmod" — observed in the kernel build log). Without it the
  # installed module tree has no modules.dep and stage-2 `modprobe` of
  # panfrost & co. cannot resolve names. Putting kmod in the build PATH
  # fixes it.
  nativeBuildInputs = [ buildPackages.kmod ];

  # Build the A72 bring-up module in the same tree/config (ABI-identical)
  # and ship it in the module dir under extra/ — modprobe/depmod index
  # that subdirectory. postInstall runs after zinstall + modules_install,
  # in the build dir (the builder's configurePhase ends with
  # `cd $buildRoot`), so this is the same invocation shape as the
  # builder's own dtbs step. Re-run depmod so modules.dep covers the
  # added module.
  postInstall = ''
    echo ":: Building out-of-tree module sramldo-smc (A72 bring-up)"
    # The module source is a store path (read-only); the out-of-tree
    # build writes .o/.o.d files next to the sources, so build from a
    # writable copy in the build dir (cwd is the kernel build dir).
    # M= must be absolute: a relative M= is resolved against the kernel
    # *source* tree (read-only store path), not the cwd (observed:
    # "../sramldo-smc-src/Makefile: No such file", 2026-09-05 build).
    cp -r ${sramldoModuleSrc} sramldo-smc-src
    chmod -R u+w sramldo-smc-src
    make $makeFlags "''${makeFlagsArray[@]}" M="$PWD/sramldo-smc-src" modules
    mkdir -p "$out/lib/modules/${modDirVersion}/extra"
    install -m 644 sramldo-smc-src/sramldo-smc.ko \
      "$out/lib/modules/${modDirVersion}/extra/sramldo-smc.ko"
    ${buildPackages.kmod}/bin/depmod -a ${modDirVersion} -b "$out"
  '';
}

# Borrowed kernel package — bridge to the self-contained kernel build.
#
# PHASE CONTEXT: the rootfs port currently borrows the working bring-up
# kernel #329 (6.6.0-00048-g188aade698dd) from the GeminiPDA project
# instead of rebuilding the 225 MB kernel source in this repo. The
# borrowed artifacts are vendored in kernel/borrowed/ (small, tracked in
# git — an exception to the repo's "tarballs are local snapshots" rule,
# because the ROOTFS build needs them and the source commit
# geminipda-bringup@188aade69 is local-only, not fetchable):
#
#   Image.gz                                   bare kernel payload
#                                             (extracted from the verified
#                                             new_kali_boot.img; the DTB is
#                                             appended by the android system
#                                             type via appendDTB, exactly as
#                                             in the in-tree build)
#   dtbs/mediatek/mt6797-gemini-pda.dtb        the appended DTB (same
#                                             extraction; matches the
#                                             device's running boot.img)
#   modules-6.6.0-00048-g188aade698dd.tar.xz   the module tree of that
#                                             exact kernel (bring-up build
#                                             out-6.6): panfrost, mtk_wcn,
#                                             wlan_gen3, rtw88_*, ...
#   sramldo-smc.ko                             out-of-tree A72 bring-up
#                                             module, vermagic
#                                             6.6.0-00048-g188aade698dd
#                                             (built against the #329 tree;
#                                             the in-tree kernel/ derivation
#                                             would produce a 6.6.0-vermagic
#                                             module that this kernel
#                                             refuses to load)
#
# What this derivation provides (the shape NixOS expects of a kernel
# package — see nixpkgs boot/kernel.nix + mobile-nixos initrd-kernel.nix):
#   out:      $out/Image.gz + $out/dtbs/... — the kernel file the toplevel
#             symlinks (boot.loader.kernelFile = target) and the android
#             bootimg output consumes (it cats Image.gz + the DTB, so the
#             built boot.img is the correct boot image for the NixOS
#             rootfs: borrowed kernel + DTB + the minimal initrd).
#   modules:  the module tree under lib/modules/<version>/, with
#             sramldo-smc.ko under extra/ (depmod-indexed) — becomes
#             system.modulesTree -> /run/booted-system/kernel-modules,
#             where NixOS's modprobe/udev look for /lib/modules/$(uname -r).
#
# The in-tree kernel build (./kernel) stays for the eventual
# self-contained phase: it must first be re-synced to the #329+ kernel
# line (it is pinned to the older 733c0c7ea, whose vermagic 6.6.0 would
# not match these modules anyway).
{ buildPackages, ... }:

let
  version = "6.6.0-00048-g188aade698dd";

  # All inputs are plain data; the whole derivation runs on the build
  # host (x86_64) — nothing aarch64 is compiled here.
  b = buildPackages;
in
b.runCommand "gemini-borrowed-kernel-${version}" {
  nativeBuildInputs = [ b.gnutar b.xz b.kmod ];

  # --- NixOS kernel-package interface -------------------------------
  # Everything non-materializable (functions, interface attrs) goes in
  # passthru — the same arrangement as the mobile-nixos kernel-builder
  # (derivationStrict refuses to stringify top-level function attrs).
  passthru = {
    # version/modDirVersion: uname -r of the borrowed kernel; the module
    # tree dir must equal it or /lib/modules/$(uname -r) misses.
    inherit version;
    modDirVersion = version;
    # The kernel's .config (the exact #329 build config, vendored) —
    # nixpkgs generates sysctl.d/55-nixos-aslr-entropy.conf from it
    # (greps CONFIG_ARCH_MMAP_RND_BITS_MAX; fails without it).
    configfile = ../../kernel/borrowed/config-${version};
    # The kernel file in $out (toplevel: ln -s $out/<target> kernel; the
    # -f check in the toplevel builder).
    target = "Image.gz";
    # packagesFor() inherits these; boot.kernelPackages set plumbing
    # (hardware.firmwareCompression calls kernelAtLeast at eval).
    kernelOlder = v: version < v;
    kernelAtLeast = v: v <= version;
    # The android system type accesses these unconditionally on the
    # kernel package (mobile-nixos android/default.nix) — the
    # kernel-builder defaults them to false; the DTB here is appended,
    # not a dt.img.
    isQcdt = false;
    isExynosDT = false;
  };

  # "modules" is consumed by system.modulesTree (lib.getOutput).
  outputs = [ "out" "modules" ];
} ''
  mkdir -p $out
  install -m 644 ${../../kernel/borrowed/Image.gz} $out/Image.gz
  install -Dm 644 ${../../kernel/borrowed/dtbs/mediatek/mt6797-gemini-pda.dtb} \
    "$out/dtbs/mediatek/mt6797-gemini-pda.dtb"

  # The tarball is the contents of /lib/modules/<version>/ (no
  # lib/modules prefix) — depmod -b and the on-disk layout both want
  # it under lib/modules/<version>/.
  mkdir -p "$modules/lib/modules"
  ${b.gnutar}/bin/tar -xJf ${../../kernel/borrowed/modules-${version}.tar.xz} \
    -C "$modules/lib/modules"
  # The tarball carries a dangling `build` symlink into the original
  # (GeminiPDA) build tree — meaningless here, drop it.
  rm -f "$modules/lib/modules/${version}/build"

  # Out-of-tree A72 bring-up module, same layout the in-tree kernel
  # derivation ships (extra/ is depmod-indexed, so `modprobe
  # sramldo-smc` resolves by name).
  mkdir -p "$modules/lib/modules/${version}/extra"
  install -m 644 ${../../kernel/borrowed/sramldo-smc.ko} \
    "$modules/lib/modules/${version}/extra/sramldo-smc.ko"
  ${b.kmod}/bin/depmod -a ${version} -b "$modules"
''

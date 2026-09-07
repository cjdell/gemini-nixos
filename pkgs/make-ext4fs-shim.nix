# mke2fs shim for mobile-nixos's android `make_ext4fs` CLI (R13, 2026-09-07).
#
# WHY: mobile-nixos builds every ext4 rootfs/system image with the
# Android make_ext4fs tool (overlay/make_ext4fs; the image-builder's
# ext4.nix copyPhase). The resulting geometry cannot be grown by the
# kernel beyond exactly 2x the image size: ext4_resize_fs fails -EINVAL
# at the 2x mark (819200 blocks / 25 groups on the 1.5 GiB Gemini
# image — the fs stops there and systemd-growfs-root dies, leaving "/"
# at ~3.1 GiB). Verified on glass (kernel #329, borrowed) AND reproduced
# on the host kernel with the real image: the failure is in
# reserve_backup_gdb()/verify_reserved_gdb() (fs/ext4/resize.c) when the
# fs must add a new sparse_super backup group with reserved-GDT blocks
# the resize inode was never given — geometry-intrinsic, not a kernel
# regression.
#
# Every mke2fs geometry tested (plain defaults, -E resize=, meta_bg)
# grows 1.5G -> 27G online without issue, so this shim re-implements the
# make_ext4fs CLI on top of mke2fs with default ext4 features
# (flex_bg/64bit/metadata_csum). Hooked in via config/gemini.nix
# nixpkgs.overlays: the image-builder resolves make_ext4fs from
# pkgs.buildPackages, which carries the same overlays, so the swap is
# global for this configuration.
#
# CLI contract replicated (the only invocations the image-builder makes):
#   make_ext4fs [-b block_size] -l fs_size_bytes [-U uuid] [-L label] IMG DIR
# mke2fs -d DIR IMG NBLOCKS is the direct equivalent (make_ext4fs packs
# DIR as the fs root). Blocks = floor(bytes / block_size), matching
# make_ext4fs's exact-fit output (image block counts have always been
# byte-aligned multiples of 4096). The builder runs this under faketime
# (1970) for reproducible mtimes and follows with an e2fsprogs fsck
# checkPhase against the result.
{ writeShellScriptBin, e2fsprogs }:
writeShellScriptBin "make_ext4fs" ''
  # make_ext4fs (mke2fs shim) — growable ext4 image builder.
  bs=4096
  size=""
  uuid=""
  label=""
  img=""
  dir=""
  while [ $# -gt 0 ]; do
    case "$1" in
      -b) bs="$2"; shift 2 ;;
      -l) size="$2"; shift 2 ;;
      -U) uuid="$2"; shift 2 ;;
      -L) label="$2"; shift 2 ;;
      -*)
        echo "make_ext4fs-shim: unknown option $1" >&2
        exit 2
        ;;
      *)
        if [ -z "$img" ]; then
          img="$1"
        elif [ -z "$dir" ]; then
          dir="$1"
        else
          echo "make_ext4fs-shim: too many positional arguments" >&2
          exit 2
        fi
        shift
        ;;
    esac
  done
  if [ -z "$img" ] || [ -z "$size" ]; then
    echo "make_ext4fs-shim: usage: make_ext4fs [-b block_size] -l size_bytes [-U uuid] [-L label] IMG [DIR]" >&2
    exit 2
  fi
  [ -n "$dir" ] || dir="."

  blocks=$(( size / bs ))

  set -- -t ext4 -b "$bs" -m 0
  [ -n "$uuid" ] && set -- "$@" -U "$uuid"
  [ -n "$label" ] && set -- "$@" -L "$label"
  set -- "$@" -d "$dir"

  exec ${e2fsprogs}/bin/mke2fs "$@" "$img" "$blocks"
''

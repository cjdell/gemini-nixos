# Minimal root-scanning initramfs for the Gemini PDA.
#
# WHY NOT MOBILE NIXOS STAGE-1 (docs R1, headline risk)
# -----------------------------------------------------
# Measured on this kernel/config: the Mobile NixOS stage-1 initrd is
# 9.66 MiB xz (39 MiB unpacked: loader + mruby + systemd-minimal + udev
# + lvgui stack). With the bring-up kernel payload (13.45 MiB gz + DTB)
# the boot image totals ~23.3 MiB — far over the 16 MiB `boot`
# partition cap.
#
# This initrd follows the verified bring-up design instead
# (GeminiPDA/build/initramfs-6.6/init, ~1.3 MiB gzip): the stock
# MediaTek LK copies the *compressed* ramdisk verbatim to 0x45000000
# and passes it to the kernel via ATAG_INITRD2; the kernel
# (CONFIG_RD_GZIP=y) decompresses it itself. So a plain busybox
# cpio.gz is exactly the contract, and gzip matches the verified
# reference format.
#
# The initrd does one thing: find the NixOS rootfs among the eMMC
# partitions (by probing for /etc/os-release + /nix/store — the
# mmcblk numbering is not stable on this unit, per the bring-up
# notes) and switch_root into it. Everything else (udev, networking,
# GPU, ...) happens in stage-2 (the NixOS systemd system).
#
# BUILD NOTE: this deliberately does NOT use nixpkgs makeInitrd, which
# archives the *nix-store closure* of its inputs (a static busybox
# still drags in the whole glibc package via its derivation
# references, ~12 MiB). Instead it stages a flat tree of plain file
# copies and makes the cpio directly — the same approach as the
# verified bring-up script. The output contains no store paths.
{ pkgs, ... }:
let
  lib = pkgs.lib;

  # Static: no dynamic libc/libgcc to ship at all.
  busybox = pkgs.busybox.override { enableStatic = true; };

  # The applets the init script uses. `sh` is busybox ash; the shebang
  # in /init is `#!/bin/busybox sh`.
  applets = [
    "sh"
    "mount"
    "umount"
    "mkdir"
    "mknod"
    "cat"
    "ls"
    "echo"
    "uname"
    "dmesg"
    "grep"
    "sed"
    "awk"
    "sleep"
    "switch_root"
    "losetup"
    "true"
    "cp"
    "rm"
    "find"
    "ln"
    "blkid"
    "readlink"
  ];

  init = pkgs.writeText "gemini-initrd-init" ''
    #!/bin/busybox sh
    # Gemini PDA NixOS rootfs initramfs.
    #
    # Finds the NixOS rootfs partition regardless of MMC host numbering
    # (on the reference unit the eMMC lands on mmcblk1, not mmcblk0),
    # mounts it, and switch_roots into NixOS.
    #
    # Adapted from the verified bring-up init
    # (GeminiPDA/build/initramfs-6.6/init), extended for Mobile NixOS
    # rootfs images, which contain ONLY the Nix store (no /init, /etc
    # or /bin symlinks): the stage-2 init lives at
    # <generation>/init where <generation> is the nixos-system store
    # path, found via /nix/var/nix/profiles/system (after first boot)
    # or the /nix-path-registration file (first boot) — the same
    # lookup Mobile NixOS's own stage-1 performs
    # (repos/mobile-nixos/boot/init/tasks/switch_root.rb).

    echo "==> gemini-nixos initramfs: starting (kernel $(uname -r))"

    mount -t proc proc /proc 2>/dev/null
    mount -t sysfs sysfs /sys 2>/dev/null
    mount -t devtmpfs devtmpfs /dev 2>/dev/null
    mkdir -p /dev/pts /newroot
    mount -t devpts devpts /dev/pts 2>/dev/null

    echo "==> waiting for eMMC block devices"
    ROOT=""
    for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
      for blk in /dev/mmcblk*; do
        [ -b "$blk" ] || continue
        case "$blk" in
        *boot0|*boot1|*rpmb) continue ;;
        esac
        # A DIRTY ext4 (unclean journal after a crash / mid-write eMMC
        # abort) cannot be mounted -o ro (EUCLEAN — replay needs rw).
        # Fall back to an rw probe, which replays the journal and mounts.
        if mount -t ext4 -o ro "$blk" /newroot 2>/dev/null || \
           mount -t ext4 -o rw "$blk" /newroot 2>/dev/null; then
          # Mobile NixOS rootfs: store-only image with a registration
          # file (first boot) or a system profile (later boots).
          # Classic full NixOS rootfs: /init + /etc/os-release.
          if { [ -d /newroot/nix/store ] && \
               { [ -f /newroot/nix-path-registration ] || [ -e /newroot/nix/var/nix/profiles/system ]; }; } || \
             { [ -x /newroot/init ] && [ -f /newroot/etc/os-release ]; }; then
            ROOT="$blk"
            umount /newroot
            echo "==> rootfs found on $ROOT"
            break 2
          fi
          umount /newroot 2>/dev/null
        fi
      done
      sleep 1
    done

    if [ -z "$ROOT" ]; then
      echo "!! no NixOS rootfs found — dropping to a shell"
      exec /bin/sh
    fi

    echo "==> mounting $ROOT rw"
    mount -t ext4 -o rw "$ROOT" /newroot 2>/dev/null || \
      mount -t ext4 -o rw "$ROOT" /newroot 2>&1 || { echo "!! mount failed — shell"; exec /bin/sh; }

    # NixOS's fstab references / by label (/dev/disk/by-label/NIXOS_SYSTEM).
    # A normal NixOS initrd provides that symlink via udev; this one has
    # no udev, so create it on devtmpfs before handing over — systemd
    # resolves the fstab / entry against it at mount time.
    label=$(blkid -s LABEL -o value "$ROOT" 2>/dev/null)
    if [ -n "$label" ]; then
        mkdir -p /dev/disk/by-label
        ln -sf "$ROOT" "/dev/disk/by-label/$label"
        echo "==> /dev/disk/by-label/$label -> $ROOT"
    else
        echo "!! could not read ext4 label of $ROOT — systemd may not find /"
    fi

    # Classic full NixOS rootfs: hand over directly.
    if [ -x /newroot/init ]; then
        echo "==> switching root to /init"
        exec switch_root /newroot /init
    fi

    # Mobile NixOS rootfs: locate the nixos-system generation.
    GEN=""
    # First boot: the closure registration file (removed after the
    # store is re-hydrated on first boot).
    p=$(grep "^/nix/store/[a-z0-9]*-nixos-system-" /newroot/nix-path-registration 2>/dev/null | sed -n 1p)
    # Later boots: the system profile (system -> system-N-link -> store path).
    if [ -z "$p" ]; then
        p=$(readlink /newroot/nix/var/nix/profiles/system 2>/dev/null)
        case "$p" in
            /nix/store/*) :
            ;;
            *)
                q=$(readlink "/newroot/nix/var/nix/profiles/$p" 2>/dev/null)
                case "$q" in
                    /nix/store/*) p="$q" ;;
                esac
            ;;
        esac
    fi
    if [ -n "$p" ] && [ -x "/newroot$p/init" ]; then
        GEN="$p"
    fi

    if [ -z "$GEN" ]; then
        echo "!! no NixOS generation found in rootfs — dropping to a shell"
        exec /bin/sh
    fi

    echo "==> switching root to $GEN/init"
    exec switch_root /newroot "$GEN/init"
  '';
in
# cpio/gzip run on the build host (cross-build); the inputs are only
# used to stage the tree, never referenced from the output.
pkgs.runCommand "gemini-minimal-initrd" {
  nativeBuildInputs = with pkgs.buildPackages; [ cpio gzip ];
  busyboxBinary = "${busybox}/bin/busybox";
  initScript = init;
} ''
  mkdir -p $out root/bin root/dev root/proc root/sys

  # Plain file copy: the cpio must contain no nix/store paths.
  cp $busyboxBinary root/bin/busybox
  chmod 555 root/bin/busybox
  for app in ${lib.escapeShellArgs applets}; do
    ln -s busybox root/bin/$app
  done

  cp $initScript root/init
  chmod 555 root/init

  (cd root && find . -print0 | cpio --quiet -o -H newc -R +0:+0 --reproducible --null | gzip -9n > $out/initrd)
  ln -s initrd $out/initrd.gz
''

#!/usr/bin/env bash
# flash-nixos.sh — flash THIS repo's Mobile NixOS artifacts onto the Gemini
# PDA. The device has NO fastboot; every write happens from the patched
# no-swipe TWRP (root adbd). This script is the NixOS-port equivalent of
# the GeminiPDA project's flash-nohelp.sh pipeline, adapted to converge to
# TWRP from WHATEVER state the device is in:
#
#   running Linux (g_ether ssh — no adbd):  para=boot-recovery over ssh,
#     then WDT EXRST self-boot (MODE=0x2200005D restore + 0x10007004=0x48) → LK boots
#     TWRP.  (Works from the current GeminiPDA Debian rootfs AND from the
#     future NixOS rootfs — both ship busybox.)
#   Android: adb reboot recovery hop (bin/boot-switch.sh twrp)
#   POC / preloader / offline: physical interaction prompts, same as
#     bin/boot-switch.sh.
#
# Usage (repo root; adb-only steps re-exec inside the devshell):
#   bash bin/flash-nixos.sh status
#       Device state + which local artifacts exist (result/ = the
#       `nix build .#packages.x86_64-linux.default` symlink).
#   bash bin/flash-nixos.sh boot [boot.img]
#       Converge to TWRP → back up current `boot` → flash the image into
#       `boot`. STAYS in TWRP (para untouched): the unverified image is
#       never booted unattended. Next step = `boot-nixos` when ready.
#   bash bin/flash-nixos.sh rootfs [rootfs.img] [--yes]
#       Converge to TWRP → stream the image into the big `linux`
#       partition (p27, by-name; ~58 GiB). DESTROYS the current NixOS
#       rootfs; prompts unless --yes (docs/repartition-android-space.md §12).
#   bash bin/flash-nixos.sh all [--yes]
#       boot + rootfs, skipping the interactive prompts.
#   bash bin/flash-nixos.sh boot-nixos
#       Clear para + reboot from TWRP → NORMAL boots the `boot` partition
#       (the flashed boot.img; para zeros = NixOS `linux` default).
#       Rollback of `boot` from TWRP: bin/boot-switch.sh restore.
#   bash bin/flash-nixos.sh grow-rootfs
#       Converge to TWRP → OFFLINE-grow the `linux` rootfs filesystem to the
#       full partition size (e2fsck -fy + resize2fs with a pushed static
#       e2fsprogs). This is the recovery path for make_ext4fs-geometry
#       images whose fs the kernel can only online-grow to 2x (R13 —
#       images built since 2026-09-07 use mke2fs and grow on first boot
#       via systemd-growfs-root). NOT destructive (grows in place), but
#       it IS a TWRP cycle: reboots the device. Follow with `boot-nixos`
#       to boot the grown rootfs.
#
# Default images: result/boot.img + result/system.img (the `default`
# flake output's android-fastboot-images layout). Built with:
#   nix build .#packages.aarch64-linux.default
#
# LONG OPERATION: the rootfs push+dd can take 5-20 min over USB. Run it
# under the detached job runner so a session never stalls:
#   bash bin/run-job.sh start flash-rootfs -- \
#     bash bin/flash-nixos.sh rootfs --yes
#   bash bin/run-job.sh wait flash-rootfs
#
# SAFETY MODEL (why the default leaves TWRP sticky):
#   A failed boot image on this device can strand the unit (a hung kernel
#   has no software path back; recovery then = mtkclient preloader mode,
#   see the DR playbook docs/disaster-recovery/). So: flash while
#   para=boot-recovery (every power-on = TWRP), verify your images, and
#   only then `boot-nixos` (para-clear + reboot). Keep the boot backups in
#   stock-dump/ — restore is one adb command. Since the 2026-09-10
#   repartition TWRP + NixOS are the only systems (Android and the
#   Debian `linux` rootfs were reclaimed — docs/repartition-android-space.md §12).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# adb lives in the flake devshell (bare host PATH has no adb — AGENTS.md
# rule 7). Re-exec once inside `nix develop` when missing; the flag
# prevents an infinite re-exec if the devshell lacks the tool.
if ! command -v adb >/dev/null 2>&1; then
  if [ -z "${GEMINI_DEVSH_REEXEC:-}" ]; then
    export GEMINI_DEVSH_REEXEC=1
    cd "$ROOT"
    exec nix develop --command bash "bin/flash-nixos.sh" "$@"
  fi
  echo "!! adb not found even inside the devshell — does the flake devShell" >&2
  echo "   carry android-tools? (flake.nix, devShells.x86_64-linux.default)" >&2
  exit 1
fi

DEV=10.15.19.82
KEY="${GEMINI_SSH_KEY:-$HOME/.ssh/id_ed25519_gemini}"
BOOT_IMG_DEFAULT="$ROOT/result/boot.img"
ROOTFS_IMG_DEFAULT="$ROOT/result/system.img"
# TWRP by-name partition directory (verified path on this unit)
P=/dev/block/platform/mtk-msdc.0/11230000.msdc0/by-name
# The single NixOS rootfs partition (2026-09-10 repartition): the old
# Android system/cache/userdata + Debian linux + boot2/boot3 collapsed
# into one ~58 GiB `linux` (p27). See bin/repartition-nixos.sh.
TARGET_PART=linux
BACKUP_DIR="$ROOT/stock-dump"
# Static (musl) aarch64 e2fsprogs for offline rootfs growth from TWRP
# (grow-rootfs verb). Rebuild + pin if GC'd:
#   nix build nixpkgs#legacyPackages.x86_64-linux.pkgsCross.aarch64-multiplatform.pkgsStatic.e2fsprogs
#   bash bin/gc-pin.sh e2fsprogs-static-aarch64 <out>
E2FS_STATIC=/nix/store/k0wplgv6nwhcp710y5z7zh37c6rvk87j-e2fsprogs-static-aarch64-unknown-linux-musl-1.47.4-bin
YES=0

adb_q()  { timeout 30 adb "$@"; }
adb_sh() { timeout 300 adb shell "$@"; }
# long ops: the 1.5 GiB rootfs push + on-device dd need minutes, not 30 s.
adb_push() { timeout 900 adb "$@"; }
devssh() { bash "$ROOT/bin/device-ssh.sh" "$@"; }

# Arm the LK watchdog for an EXRST self-boot (Linux → TWRP/Debian hop).
# The A72 bring-up (services/scripts/cl2-up.sh, run by gemini-a72-up at
# every boot) leaves WDT MODE disarmed (0x10007000 = 0), so the LENGTH
# arm write alone silently no-ops — the documented "reboot trap",
# docs/phase-2-on-glass.md §2b. Restore LK's mode value (key | 0x5D)
# first, then arm. [added 2026-09-10]
wdt_exrst() {
  devssh "busybox devmem 0x10007000 32 0x2200005D; busybox devmem 0x10007004 32 0x48" 2>/dev/null || true
}

say() { printf '>> %s\n' "$*"; }
die() { echo "!! $*" >&2; exit 1; }

usage() { awk 'NR==1{next} /^#/{sub(/^# ?/,""); print; next} {exit}' "$0"; }

# ---- state --------------------------------------------------------------
# adb-ish states: twrp|android|unauthorized|adb-offline|poc|preloader|brom|offline
# PLUS: linux (ssh reachable over g_ether, no adb)
state() {
  local line
  line=$(adb devices 2>/dev/null | tail -n +2 | grep -v '^$' || true)
  if [ -n "$line" ]; then
    if echo "$line" | grep -q 'recovery'; then echo twrp; return; fi
    if echo "$line" | grep -qE '\bdevice\b'; then echo android; return; fi
    if echo "$line" | grep -q 'unauthorized'; then echo unauthorized; return; fi
    echo adb-offline; return
  fi
  # no adb device — is a Linux rootfs up over g_ether instead?
  if timeout 2 bash -c "ping -c 1 -W 1 $DEV >/dev/null 2>&1"; then
    if timeout 8 ssh -i "$KEY" -o BatchMode=yes -o IdentitiesOnly=yes \
        -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
        -o ConnectTimeout=5 root@"$DEV" true >/dev/null 2>&1; then
      echo linux; return
    fi
    echo linux-nossh; return
  fi
  if lsusb 2>/dev/null | grep -q '0e8d:2008'; then echo poc; return; fi
  if lsusb 2>/dev/null | grep -q '0e8d:2000'; then echo preloader; return; fi
  if lsusb 2>/dev/null | grep -q '0e8d:0003'; then echo brom; return; fi
  echo offline
}

wait_for() { # want [iterations x5s]
  local want="$1" n="${2:-36}" i s
  for ((i=1; i<=n; i++)); do
    s=$(state)
    [ "$s" = "$want" ] && { echo "  $(date +%H:%M:%S) state: $want"; return 0; }
    sleep 5
  done
  die "timed out waiting for '$want' (last: $s)"
}

# ---- converge to TWRP from any state ------------------------------------
converge_twrp() {
  local s
  s=$(state)
  case "$s" in
    twrp) say "already in TWRP"; return 0 ;;
    linux|linux-nossh)
      say "Linux up over g_ether (no adb) — para-write + WDT EXRST self-boot to TWRP"
      # para = p2 of the LARGEST mmcblk (eMMC numbering differs across
      # kernel builds — a hardcoded /dev/mmcblk1p2 once silently created a
      # regular file instead of writing the eMMC; detect by size always).
      devssh 'best=""; bs=0; for D in $(lsblk -dn -o NAME | grep -E "^mmcblk[0-9]+$"); do S=$(blockdev --getsize64 /dev/$D 2>/dev/null || echo 0); if [ "$S" -gt "$bs" ]; then bs=$S; best=$D; fi; done; [ -b /dev/${best}p2 ] || { echo "no para partition (largest mmcblk=$best)"; exit 1; }; { printf "boot-recovery\0"; head -c 18 /dev/zero; } > /tmp/bootcmd.bin; dd if=/tmp/bootcmd.bin of=/dev/${best}p2 bs=32 count=1 conv=fsync 2>/dev/null && dd if=/dev/${best}p2 bs=32 count=1 2>/dev/null | grep -qa "boot-recovery" && echo "PARA-WRITTEN+VERIFIED ($best)" || { echo "!! para write/verify FAILED"; exit 1; }' \
        || die "para write over ssh failed"
      say "arming WDT for EXRST self-boot (MODE=0x2200005D restore + 0x10007004=0x48)"
      wdt_exrst
      say "device resetting — waiting for TWRP (USB 18d1:4ee2)..."
      local i
      for ((i=1; i<=36; i++)); do
        sleep 5
        if lsusb 2>/dev/null | grep -q '18d1:4ee2'; then
          say "TWRP up after ~$((i*5))s (adbd settling)"
          sleep 10
          wait_for twrp 12
          return 0
        fi
      done
      die "TWRP did not appear within 180s — may need a physical power-on"
      ;;
    android)
      say "in Android — hopping to TWRP (adb reboot recovery + sticky para)"
      bash "$ROOT/bin/boot-switch.sh" twrp
      return 0
      ;;
    poc)
      say "Power-Off-Charging (0e8d:2008) — press the POWER KEY on the device"
      say "(sticky para lands it in TWRP). Waiting up to 3 min..."
      local i s
      for ((i=1; i<=36; i++)); do
        sleep 5; s=$(state)
        [ "$s" = twrp ] && { wait_for twrp 6; return 0; }
        [ "$s" = android ] && { bash "$ROOT/bin/boot-switch.sh" twrp; return 0; }
      done
      die "still in POC — press the power key or replug USB"
      ;;
    preloader|brom)
      die "device in $s download mode (no adb). Power off / press power to abort, then re-run."
      ;;
    offline)
      die "no device on USB. Connect the cable and power on (para sticky = TWRP), then re-run."
      ;;
    unauthorized|adb-offline)
      die "adb state '$s' — accept the RSA prompt / replug USB, then re-run."
      ;;
  esac
}

# ---- artifact checks ------------------------------------------------------
need_img() { # path what
  [ -f "$1" ] || die "$2 not found: $1 — build it: nix build .#packages.x86_64-linux.default"
}

# ---- TWRP-side helpers ----------------------------------------------------
twrp_dd_part() { # devnode src-dest-label  (image already pushed to /tmp on device)
  adb_sh "dd if=$1 of=$P/$2 bs=1M conv=fsync"
}

# ---- commands --------------------------------------------------------------
cmd_status() {
  echo "device state : $(state)"
  for f in "$BOOT_IMG_DEFAULT" "$ROOTFS_IMG_DEFAULT"; do
    if [ -f "$f" ]; then
      printf 'artifact      : %s  (%s, %s)\n' "$f" "$(du -h "$f" | cut -f1)" \
        "$(stat -c%y "$f" | cut -d. -f1)"
    else
      printf 'artifact      : %s  (MISSING — build with nix build .#packages.x86_64-linux.default)\n' "$f"
    fi
  done
  echo "hint: adb-side boot-target control = bash bin/boot-switch.sh status"
}

cmd_boot() {
  local img="${1:-$BOOT_IMG_DEFAULT}"
  need_img "$img" "boot image"
  converge_twrp
  bash "$ROOT/bin/boot-switch.sh" flash "$img" twrp
  say "boot.img flashed. Device is in TWRP (para sticky). When ready to test:"
  say "  bash bin/flash-nixos.sh boot-nixos     (or: bash bin/boot-switch.sh android)"
}

cmd_rootfs() {
  local img="${1:-$ROOTFS_IMG_DEFAULT}"
  need_img "$img" "rootfs image"
  converge_twrp
  # sanity: the target partition exists and is big (>= 20 GiB). TWRP has
  # no blockdev; resolve the by-name symlink and read the size from
  # /proc/partitions (column 3, KiB units) on the device.
  local tgt base kb
  tgt=$(adb_sh "readlink -f $P/$TARGET_PART" | tr -d '\r' || true)
  base=$(basename "$tgt")
  kb=$(adb_sh 'cat /proc/partitions' | tr -d '\r' | awk -v b="$base" '$4==b{print $3}')
  if [ -z "$tgt" ] || [ -z "$kb" ] || [ "$kb" -lt $((20 * 1024 * 1024)) ]; then
    die "by-name/$TARGET_PART missing or too small (readlink=$tgt, blocks=$kb) — refusing. Partition list: $(adb_sh 'ls '$P | tr '\n' ' ')"
  fi
  echo ">> target: $P/$TARGET_PART -> $tgt = $((kb / 1024 / 1024)) GiB"
  if [ "$YES" != 1 ]; then
    echo "!! This DESTROYS the current NixOS rootfs on $TARGET_PART."
    read -r -p "Type 'wipe rootfs' to continue: " ans
    [ "$ans" = "wipe rootfs" ] || { echo "aborted."; exit 1; }
  fi
  # Stream the image STRAIGHT to the partition: the rootfs image is now
  # ~8 GB (GNOME closure) and does NOT fit TWRP's ~1.9 GiB /tmp tmpfs.
  # Unmount first so TWRP cannot flush stale data over the image.
  adb_sh "umount /data 2>/dev/null; umount /sdcard 2>/dev/null; umount /cache 2>/dev/null; umount $P/$TARGET_PART 2>/dev/null; sync; true" >/dev/null
  say "streaming rootfs -> $P/$TARGET_PART ($(stat -c%s "$img") bytes; several minutes)..."
  timeout 3600 adb shell "dd of=$P/$TARGET_PART bs=1M conv=fsync" < "$img"
  say "rootfs flashed. Device is in TWRP (para sticky)."
}

cmd_all() {
  cmd_boot "${1:-$BOOT_IMG_DEFAULT}"
  cmd_rootfs "${2:-$ROOTFS_IMG_DEFAULT}"
  say "boot + rootfs flashed. Boot into NixOS when ready: bash bin/flash-nixos.sh boot-nixos"
}

cmd_boot_nixos() {
  case "$(state)" in
    twrp) : ;;
    *) converge_twrp ;;
  esac
  say "clearing para + rebooting → NORMAL boots the \`boot\` partition"
  say "  (para zeros = NixOS on the linux partition)"
  adb_sh "dd if=/dev/zero of=$P/para bs=32 count=1 conv=fsync" >/dev/null
  adb_q reboot >/dev/null 2>&1 || true
  say "reboot sent. First NixOS boot: watch the serial console (ttyS0,921600) or fbcon."
  say "If it comes back, expect g_ether at $DEV (ssh). If it hangs: no software path —"
  say "recovery = mtkclient preloader mode OR re-power-on (para cleared now = normal boot)."
}

cmd_grow_rootfs() {
  converge_twrp
  [ -d "$E2FS_STATIC/bin" ] || die "static e2fsprogs not present: $E2FS_STATIC (rebuild + gc-pin, see header)"
  local tgt base kb
  tgt=$(adb_sh "readlink -f $P/$TARGET_PART" | tr -d '\r' || true)
  base=$(basename "$tgt")
  kb=$(adb_sh 'cat /proc/partitions' | tr -d '\r' | awk -v b="$base" '$4==b{print $3}')
  if [ -z "$tgt" ] || [ -z "$kb" ] || [ "$kb" -lt $((20 * 1024 * 1024)) ]; then
    die "by-name/$TARGET_PART missing or too small (readlink=$tgt, blocks=$kb) — refusing"
  fi
  say "target: $P/$TARGET_PART -> $tgt = $((kb / 1024 / 1024)) GiB"
  # TWRP may auto-mount the partition; offline growth needs it unmounted.
  adb_sh "umount /data 2>/dev/null; umount $tgt 2>/dev/null; umount $P/$TARGET_PART 2>/dev/null; true" >/dev/null
  say "pushing static e2fsprogs (e2fsck + resize2fs)..."
  adb_push push "$E2FS_STATIC/bin/e2fsck" /tmp/e2fsck >/dev/null
  adb_push push "$E2FS_STATIC/sbin/resize2fs" /tmp/resize2fs >/dev/null
  adb_sh "chmod +x /tmp/e2fsck /tmp/resize2fs" >/dev/null
  say "e2fsck -fy (journal replay + health check — offline, no online-resize limits)"
  # e2fsck exits 1 when it MODIFIED the fs (journal replay / repairs) —
  # that is success here; don't let set -euo pipefail kill the run.
  adb_sh "/tmp/e2fsck -fy $P/$TARGET_PART" 2>&1 | tail -3 || true
  say "resize2fs -> full partition size"
  adb_sh "/tmp/resize2fs $P/$TARGET_PART" 2>&1 | tail -3
  say "verify: fs state + free space"
  adb_sh "/tmp/e2fsck -fn $P/$TARGET_PART" 2>&1 | tail -3 || true
  say "grow done. Device is in TWRP. Boot the grown rootfs when ready:"
  say "  bash bin/flash-nixos.sh boot-nixos"
}

# (Debian/dual-boot para marker helpers removed 2026-09-10: after the
# repartition TWRP + NixOS are the only systems — see bin/repartition-nixos.sh.)

# ---- main --------------------------------------------------------------------
args=()
for a in "$@"; do
  case "$a" in
    --yes) YES=1 ;;
    *) args+=("$a") ;;
  esac
done
set -- "${args[@]}"

case "${1:-}" in
  status)      cmd_status ;;
  boot)        cmd_boot "${2:-$BOOT_IMG_DEFAULT}" ;;
  rootfs)      cmd_rootfs "${2:-$ROOTFS_IMG_DEFAULT}" ;;
  all)         cmd_all "${2:-$BOOT_IMG_DEFAULT}" "${3:-$ROOTFS_IMG_DEFAULT}" ;;
  boot-nixos)  cmd_boot_nixos ;;
  grow-rootfs) cmd_grow_rootfs ;;
  -h|--help|help|"") usage ;;
  *) echo "!! unknown command: ${1:-}" >&2; usage; exit 1 ;;
esac

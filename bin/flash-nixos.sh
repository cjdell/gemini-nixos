#!/usr/bin/env bash
# flash-nixos.sh — flash THIS repo's Mobile NixOS artifacts onto the Gemini
# PDA. The device has NO fastboot; every write happens from the patched
# no-swipe TWRP (root adbd). This script is the NixOS-port equivalent of
# the GeminiPDA project's flash-nohelp.sh pipeline, adapted to converge to
# TWRP from WHATEVER state the device is in:
#
#   running Linux (g_ether ssh — no adbd):  para=boot-recovery over ssh,
#     then WDT EXRST self-boot (busybox devmem 0x10007004 0x48) → LK boots
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
#       Converge to TWRP → flash the image into Android's `userdata`
#       partition (p32, by-name). DESTROYS the Android FDE userdata —
#       prompts unless --yes. The GeminiPDA Debian rootfs on p29
#       (`linux`) is NOT touched (docs/repartition-android-space.md).
#   bash bin/flash-nixos.sh all [--yes]
#       boot + rootfs, skipping the interactive prompts.
#   bash bin/flash-nixos.sh boot-nixos
#       Clear para + reboot from TWRP → NORMAL boots the `boot` partition
#       (the flashed dual-boot boot.img; para zeros = NixOS p32 default).
#       Rollback of `boot` from TWRP: bin/boot-switch.sh restore. Debian
#       stays bootable any time via para=boot-debian (bin/boot-switch.sh
#       debian / this script's `debian` / on-device gemini-boot-debian).
#   bash bin/flash-nixos.sh debian
#       Switch to the Debian rootfs on p29: para=boot-debian + reboot
#       (from running Linux: WDT EXRST self-boot; from TWRP: adb reboot).
#       Reverse (back to NixOS): boot-nixos (or clear para + reboot).
#   bash bin/flash-nixos.sh grow-rootfs
#       Converge to TWRP → OFFLINE-grow the p32 rootfs filesystem to the
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
#   nix build .#packages.x86_64-linux.default
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
#   stock-dump/ — restore is one adb command. The p29 Debian rootfs is
#   never written by this script; it stays bootable via the `boot-debian`
#   marker even after the NixOS boot.img is installed.
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
      say "arming WDT for EXRST self-boot (busybox devmem 0x10007004 32 0x48)"
      devssh "busybox devmem 0x10007004 32 0x48" 2>/dev/null || true
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
  # sanity: the target partition exists and is big (>= 20 GiB = p32
  # userdata). TWRP has no blockdev — resolve the by-name symlink and
  # read the size from /proc/partitions (column 3, KiB units) on the HOST.
  local tgt base kb
  tgt=$(adb_sh "readlink -f $P/userdata" | tr -d '\r' || true)
  base=$(basename "$tgt")
  kb=$(adb_sh 'cat /proc/partitions' | tr -d '\r' | awk -v b="$base" '$4==b{print $3}')
  if [ -z "$tgt" ] || [ -z "$kb" ] || [ "$kb" -lt $((20 * 1024 * 1024)) ]; then
    die "p32 (by-name/userdata) missing or too small (readlink=$tgt, blocks=$kb) — refusing. Partition list: $(adb_sh 'ls '$P | tr '\n' ' ')"
  fi
  echo ">> target: $P/userdata -> $tgt = $((kb / 1024 / 1024)) GiB (p32)"
  if [ "$YES" != 1 ]; then
    echo "!! This DESTROYS Android's userdata on p32 (factory FDE data) —"
    echo "   the NixOS rootfs replaces it. The Debian rootfs on p29 is untouched."
    read -r -p "Type 'wipe android' to continue: " ans
    [ "$ans" = "wipe android" ] || { echo "aborted."; exit 1; }
  fi
  say "pushing rootfs image to the device (~1.5 GiB — allow several minutes)..."
  adb_push push "$img" /tmp/rootfs.img >/dev/null
  # belt: confirm the pushed copy is complete before the destructive dd
  # (TWRP's busybox stat has no -c — wc -c works everywhere)
  local got want
  got=$(adb_sh "wc -c < /tmp/rootfs.img 2>/dev/null" | tr -d '\r' || true)
  want=$(stat -c %s "$img")
  [ "$got" = "$want" ] || die "push incomplete (device $got vs host $want bytes) — re-run"
  say "push verified ($got bytes on device)"
  say "flashing -> $P/userdata (ext4, label NIXOS_SYSTEM; first boot auto-resizes to fill p32 + rehydrates the store)"
  twrp_dd_part /tmp/rootfs.img userdata
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
  say "  (dual-boot boot.img; para zeros = NixOS p32 default)"
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
  tgt=$(adb_sh "readlink -f $P/userdata" | tr -d '\r' || true)
  base=$(basename "$tgt")
  kb=$(adb_sh 'cat /proc/partitions' | tr -d '\r' | awk -v b="$base" '$4==b{print $3}')
  if [ -z "$tgt" ] || [ -z "$kb" ] || [ "$kb" -lt $((20 * 1024 * 1024)) ]; then
    die "p32 (by-name/userdata) missing or too small (readlink=$tgt, blocks=$kb) — refusing"
  fi
  say "target: $P/userdata -> $tgt = $((kb / 1024 / 1024)) GiB (p32)"
  # TWRP auto-mounts userdata as /data; offline growth needs it unmounted.
  adb_sh "umount /data 2>/dev/null; umount $tgt 2>/dev/null; umount $P/userdata 2>/dev/null; true" >/dev/null
  say "pushing static e2fsprogs (e2fsck + resize2fs)..."
  adb_push push "$E2FS_STATIC/bin/e2fsck" /tmp/e2fsck >/dev/null
  adb_push push "$E2FS_STATIC/sbin/resize2fs" /tmp/resize2fs >/dev/null
  adb_sh "chmod +x /tmp/e2fsck /tmp/resize2fs" >/dev/null
  say "e2fsck -fy (journal replay + health check — offline, no online-resize limits)"
  # e2fsck exits 1 when it MODIFIED the fs (journal replay / repairs) —
  # that is success here; don't let set -euo pipefail kill the run.
  adb_sh "/tmp/e2fsck -fy $P/userdata" 2>&1 | tail -3 || true
  say "resize2fs -> full partition size"
  adb_sh "/tmp/resize2fs $P/userdata" 2>&1 | tail -3
  say "verify: fs state + free space"
  adb_sh "/tmp/e2fsck -fn $P/userdata" 2>&1 | tail -3 || true
  say "grow done. Device is in TWRP. Boot the grown rootfs when ready:"
  say "  bash bin/flash-nixos.sh boot-nixos"
}

# ---- para helpers ------------------------------------------------------------
# Write a 32-byte boot command to the para partition (offset 0) from TWRP:
# "" clears (zeros = NixOS default), otherwise "<marker>\0" + zero padding to
# 32 bytes — the exact layout the dual-boot initrd compares against
# (initrd.nix; boot-debian = 11 chars + NUL + 20 zeros).
twrp_para() { # [marker] — "" clears
  local marker="${1:-}" cmd=/tmp/para-cmd.bin
  if [ -n "$marker" ]; then
    { printf '%s\0' "$marker"; head -c $((31 - ${#marker})) /dev/zero; } > "$cmd"
  else
    dd if=/dev/zero of="$cmd" bs=32 count=1 2>/dev/null
  fi
  adb_q push "$cmd" "$cmd" >/dev/null
  adb_sh "dd if=$cmd of=$P/para bs=32 count=1 conv=fsync" >/dev/null
}

cmd_debian() {
  case "$(state)" in
    linux)
      say "Linux up over g_ether (no adb) — para=boot-debian + WDT EXRST self-boot"
      # largest-mmcblk rule + read-back verify (same as converge_twrp)
      devssh 'best=""; bs=0; for D in $(lsblk -dn -o NAME | grep -E "^mmcblk[0-9]+$"); do S=$(blockdev --getsize64 /dev/$D 2>/dev/null || echo 0); if [ "$S" -gt "$bs" ]; then bs=$S; best=$D; fi; done; [ -b /dev/${best}p2 ] || { echo "no para partition (largest mmcblk=$best)"; exit 1; }; { printf "boot-debian\0"; head -c 20 /dev/zero; } > /tmp/bootcmd.bin; dd if=/tmp/bootcmd.bin of=/dev/${best}p2 bs=32 count=1 conv=fsync 2>/dev/null && dd if=/dev/${best}p2 bs=32 count=1 2>/dev/null | grep -qa "boot-debian" && echo "PARA=debian (verified on $best)" || { echo "!! para write/verify FAILED"; exit 1; }' \
        || die "para write over ssh failed"
      say "arming WDT for EXRST self-boot (busybox devmem 0x10007004 32 0x48)"
      devssh "busybox devmem 0x10007004 32 0x48" 2>/dev/null || true
      say "device resetting — Debian should come up on g_ether ($DEV) in ~40-90 s;"
      say "then: bash bin/device-ssh.sh 'uname -a' to confirm (or bin/net-up.sh first)"
      ;;
    twrp)
      say "in TWRP — para=boot-debian, rebooting into Debian"
      twrp_para "boot-debian"
      adb_q reboot >/dev/null 2>&1 || true
      say "Debian has no adbd — expect g_ether at $DEV (ssh) in ~30-60 s"
      ;;
    *)
      converge_twrp
      twrp_para "boot-debian"
      adb_q reboot >/dev/null 2>&1 || true
      say "Debian has no adbd — expect g_ether at $DEV (ssh) in ~30-60 s"
      ;;
  esac
}

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
  debian)      cmd_debian ;;
  -h|--help|help|"") usage ;;
  *) echo "!! unknown command: ${1:-}" >&2; usage; exit 1 ;;
esac

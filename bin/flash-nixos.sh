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
#   bash bin/flash-nixos.sh rootfs [rootfs.img] [--backup-rootfs FILE]
#       Converge to TWRP → flash the image into p29 (the `linux`
#       partition, by-name). DESTROYS the current p29 rootfs (the
#       GeminiPDA Debian rootfs) — prompts unless --yes. Optional
#       --backup-rootfs dd's the current p29 to FILE first (big/slow:
#       ~27 GiB — run under bin/run-job.sh, see below).
#   bash bin/flash-nixos.sh all [--yes]
#       boot + rootfs, skipping the interactive prompts.
#   bash bin/flash-nixos.sh boot-nixos
#       Clear para + reboot from TWRP → NORMAL boots the `boot` partition
#       (the flashed NixOS boot.img). Rollback from TWRP: bin/boot-switch.sh
#       restore (boot) — p29 rollback = re-flash the pre-NixOS rootfs.
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
#   see GeminiPDA docs/flashing.md). So: flash while para=boot-recovery
#   (every power-on = TWRP), verify your images, and only then
#   `boot-nixos` (para-clear + reboot). Keep the boot backups in
#   stock-dump/ — restore is one adb command.
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
YES=0
BACKUP_ROOTFS=""

adb_q()  { timeout 30 adb "$@"; }
adb_sh() { timeout 300 adb shell "$@"; }
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
  # sanity: the target partition exists and is big (>= 20 GiB = p29).
  # TWRP has no blockdev — resolve the by-name symlink and read the
  # size from /proc/partitions (column 3, KiB units) on the HOST.
  local tgt base kb
  tgt=$(adb_sh "readlink -f $P/linux" | tr -d '\r' || true)
  base=$(basename "$tgt")
  kb=$(adb_sh 'cat /proc/partitions' | tr -d '\r' | awk -v b="$base" '$4==b{print $3}')
  if [ -z "$tgt" ] || [ -z "$kb" ] || [ "$kb" -lt $((20 * 1024 * 1024)) ]; then
    die "p29 (by-name/linux) missing or too small (readlink=$tgt, blocks=$kb) — refusing. Partition list: $(adb_sh 'ls '$P | tr '\n' ' ')"
  fi
  echo ">> target: $P/linux -> $tgt = $((kb / 1024 / 1024)) GiB (p29)"
  if [ "$YES" != 1 ]; then
    echo "!! This DESTROYS the current p29 rootfs (the GeminiPDA Debian rootfs)."
    read -r -p "Type 'wipe p29' to continue: " ans
    [ "$ans" = "wipe p29" ] || { echo "aborted."; exit 1; }
  fi
  if [ -n "$BACKUP_ROOTFS" ]; then
    say "backing up current p29 -> $BACKUP_ROOTFS (27 GiB — can take 30+ min over USB; run under bin/run-job.sh)"
    adb exec-out "dd if=$P/linux bs=1M 2>/dev/null" > "$BACKUP_ROOTFS"
    say "backup done: $(du -h "$BACKUP_ROOTFS" | cut -f1)"
  fi
  say "pushing rootfs image to the device (~1.7 GiB)..."
  adb_q push "$img" /tmp/rootfs.img >/dev/null
  say "flashing -> $P/linux (ext4, label NIXOS_SYSTEM; first boot auto-resizes + rehydrates the store)"
  twrp_dd_part /tmp/rootfs.img linux
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
  say "clearing para + rebooting → NORMAL boots the \`boot\` partition (the NixOS boot.img)"
  adb_sh "dd if=/dev/zero of=$P/para bs=32 count=1 conv=fsync" >/dev/null
  adb_q reboot >/dev/null 2>&1 || true
  say "reboot sent. First NixOS boot: watch the serial console (ttyS0,921600) or fbcon."
  say "If it comes back, expect g_ether at $DEV (ssh). If it hangs: no software path —"
  say "recovery = mtkclient preloader mode OR re-power-on (para cleared now = normal boot)."
}

# ---- main --------------------------------------------------------------------
args=()
for a in "$@"; do
  case "$a" in
    --yes) YES=1 ;;
    --backup-rootfs) : ;; # consumed below with its value
    *) args+=("$a") ;;
  esac
done
# pull --backup-rootfs FILE out of the positional args
for ((i=0; i<${#args[@]}; i++)); do
  if [ "${args[$i]}" = "--backup-rootfs" ]; then
    BACKUP_ROOTFS="${args[$((i+1))]:-}"
    unset 'args[$i]'; unset 'args[$((i+1))]'
    [ -n "$BACKUP_ROOTFS" ] || die "--backup-rootfs needs a FILE path"
  fi
done
set -- "${args[@]}"

case "${1:-}" in
  status)      cmd_status ;;
  boot)        cmd_boot "${2:-$BOOT_IMG_DEFAULT}" ;;
  rootfs)      cmd_rootfs "${2:-$ROOTFS_IMG_DEFAULT}" ;;
  all)         cmd_all "${2:-$BOOT_IMG_DEFAULT}" "${3:-$ROOTFS_IMG_DEFAULT}" ;;
  boot-nixos)  cmd_boot_nixos ;;
  -h|--help|help|"") usage ;;
  *) echo "!! unknown command: ${1:-}" >&2; usage; exit 1 ;;
esac

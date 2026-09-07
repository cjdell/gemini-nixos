#!/usr/bin/env bash
# boot-switch.sh — choose the Gemini PDA's boot target, driven entirely
# over USB from the host (adb states: TWRP / Android).
#
# Adapted from the GeminiPDA project's build/boot-switch.sh (same
# mechanism, verified on this hardware 2026-08-30). Three deltas:
#   * adb/lsusb come from the repo flake devshell — if they are not on
#     the bare host PATH the script re-executes itself inside `nix develop`;
#   * no `linux` (Gemian boot2-copy) command — this project's Linux is the
#     NixOS boot.img flashed into `boot` itself; booting it = `android`
#     below (para-clear + reboot → NORMAL → whatever is in `boot`);
#   * `debian` verb (2026-09-07): para=boot-debian selects the Debian
#     branch of the dual-boot initrd (docs/repartition-android-space.md).
#
# Mechanism (GeminiPDA docs/boot-chain.md §8c): LK boots RECOVERY whenever
# the MISC command in the `para` partition (offset 0) is "boot-recovery".
# TWRP's adbd is root, so every partition write happens from TWRP — the
# script first makes sure the device IS in TWRP, whatever its current
# state:
#   Android -> `adb reboot recovery` (RTC_PDN1 FAC_RESET flag, verified
#              2026-08-30)
#   POC     -> press the power key on the device (charging screen has no
#              USB comms; sticky para lands it in TWRP, else Android and
#              we hop via reboot recovery)
#   offline -> connect the cable and power on
#   preloader/BROM download mode -> power off / press power to abort
#
# NOTE: this script talks ONLY adb. If the device is running Linux (g_ether
# ssh — no adbd), converge to TWRP with bin/flash-nixos.sh (para write over
# ssh + WDT EXRST self-boot), which then uses this script's `flash`.
#
# Usage (from the repo root, any host):
#   bash bin/boot-switch.sh status
#   bash bin/boot-switch.sh twrp|android|debian
#   bash bin/boot-switch.sh flash [image] [twrp|android]
#   bash bin/boot-switch.sh restore
#
# State after each target:
#   twrp    -> TWRP on every power-on (default, sticky; para=boot-recovery)
#   android -> clear para + reboot → NORMAL boots the `boot` partition
#              (stock Android, or the flashed NixOS boot.img — whatever is
#              in `boot` right now). With the dual-boot initrd installed
#              this is the NixOS boot (p32 default; boot-debian marker
#              below overrides).
#   debian  -> para=boot-debian + reboot → NORMAL boots the `boot` image's
#              Debian branch (p29 rootfs). NOTE: Debian has no adbd, so
#              this verb does not wait for an adb state — expect g_ether
#              (10.15.19.82, ssh) instead.
#   flash   -> back up current `boot`, write [image] into `boot`; stays in
#              TWRP (target=twrp) or clears para + reboots (target=android)
#   restore -> put the latest stock-dump/boot-*.img backup back into `boot`
#
# Safe: only touches `para` (backed up to stock-dump/para.bin on first
# write) and, for flash/restore, the `boot` partition. Never
# nvram/proinfo/protect*.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# adb/lsusb live in the flake devshell (bare host PATH has no adb —
# AGENTS.md rule 7). Re-exec once inside `nix develop` when missing;
# the flag prevents an infinite re-exec if the devshell lacks the tool.
if ! command -v adb >/dev/null 2>&1 || ! command -v lsusb >/dev/null 2>&1; then
  if [ -z "${GEMINI_DEVSH_REEXEC:-}" ]; then
    export GEMINI_DEVSH_REEXEC=1
    cd "$ROOT"
    exec nix develop --command bash "bin/boot-switch.sh" "$@"
  fi
  echo "!! adb/lsusb not found even inside the devshell — does the flake devShell" >&2
  echo "   carry android-tools + usbutils? (flake.nix, devShells.x86_64-linux.default)" >&2
  exit 1
fi

P=/dev/block/platform/mtk-msdc.0/11230000.msdc0/by-name
PARA_BACKUP=${PARA_BACKUP:-$ROOT/stock-dump/para.bin}
BOOTIMG_DEFAULT=${BOOTIMG_DEFAULT:-$ROOT/result/boot.img}
# (result/ = `nix build .#packages.x86_64-linux.default`; pass an explicit
# image path if you built only .#bootimg.)
BOOT_BACKUP_DIR=${BOOT_BACKUP_DIR:-$ROOT/stock-dump}

adb_q()  { timeout 30 adb "$@"; }
adb_sh() { timeout 120 adb shell "$@"; }

usage() { awk 'NR==1{next} /^#/{sub(/^# ?/,""); print; next} {exit}' "$0"; }

# ---- device state detection ------------------------------------------------
# one of: twrp | android | unauthorized | adb-offline | poc | preloader |
#         brom | offline
state() {
  local line
  line=$(adb devices 2>/dev/null | tail -n +2 | grep -v '^$' || true)
  if [ -n "$line" ]; then
    if echo "$line" | grep -q 'recovery'; then echo twrp; return; fi
    if echo "$line" | grep -qE '\bdevice\b'; then echo android; return; fi
    if echo "$line" | grep -q 'unauthorized'; then echo unauthorized; return; fi
    echo adb-offline; return
  fi
  if lsusb 2>/dev/null | grep -q '0e8d:2008'; then echo poc; return; fi
  if lsusb 2>/dev/null | grep -q '0e8d:2000'; then echo preloader; return; fi
  if lsusb 2>/dev/null | grep -q '0e8d:0003'; then echo brom; return; fi
  echo offline
}

# wait_for <state> [iterations x5s] — exact match
wait_for() {
  local want="$1" n="${2:-36}" i s
  for ((i=1; i<=n; i++)); do
    s=$(state)
    if [ "$s" = "$want" ]; then
      printf '  %s state is now: %s\n' "$(date +%H:%M:%S)" "$want"; return 0
    fi
    sleep 5
  done
  echo "!! timed out waiting for '$want' (last: $s)" >&2
  return 1
}

# wait_any <stateA> <stateB> [iterations x5s] — first match wins
wait_any() {
  local a="$1" b="$2" n="${3:-36}" i s
  for ((i=1; i<=n; i++)); do
    s=$(state)
    if [ "$s" = "$a" ] || [ "$s" = "$b" ]; then
      printf '  %s state is now: %s\n' "$(date +%H:%M:%S)" "$s"; return 0
    fi
    sleep 5
  done
  echo "!! timed out waiting for '$a'/'$b' (last: $s)" >&2
  return 1
}

# ---- converge the device into TWRP (partition writes need root adbd) -------
ensure_twrp() {
  local rounds s
  for ((rounds=6; rounds>0; rounds--)); do
    s=$(state)
    case "$s" in
      twrp) echo ">> already in TWRP"; return 0 ;;
      android)
        echo ">> in Android — hopping to TWRP via 'adb reboot recovery' (RTC flag)"
        adb_q reboot recovery >/dev/null 2>&1 || true
        wait_for twrp 36 && return 0
        ;;
      poc)
        echo ">> Power-Off-Charging (USB 0e8d:2008, no adb)."
        echo "   Press the POWER KEY on the device (sticky para lands it in TWRP,"
        echo "   else Android and we hop from there). Waiting up to 3 min..."
        wait_any twrp android 36 && [ "$(state)" = twrp ] && return 0
        ;;
      preloader|brom)
        echo ">> device in $s download mode (no adb). Power off / press power to abort it."
        wait_any twrp android 36 && [ "$(state)" = twrp ] && return 0
        ;;
      offline)
        echo ">> no device on USB. Connect the cable and power on the device (TWRP default)."
        wait_any twrp android 36 && [ "$(state)" = twrp ] && return 0
        ;;
      unauthorized)
        echo ">> adb wants authorization — accept the RSA prompt on the device screen."
        sleep 10 ;;
      adb-offline)
        echo ">> adb reports 'offline' — replug USB or reboot the device."
        sleep 10 ;;
      *) echo "!! unknown state: $s" >&2; return 1 ;;
    esac
  done
  echo "!! could not reach TWRP — run 'boot-switch.sh status' and check the cable/state" >&2
  return 1
}

# ---- helpers ----------------------------------------------------------------
backup_para() {
  if [ ! -f "$PARA_BACKUP" ]; then
    echo ">> backing up para -> $PARA_BACKUP (first write this session)"
    adb_sh "dd if=$P/para of=/tmp/para.bin bs=512 count=1 conv=fsync" >/dev/null
    adb_q pull /tmp/para.bin "$PARA_BACKUP" >/dev/null
  fi
}

# ---- commands ----------------------------------------------------------------
cmd_status() {
  local s
  s=$(state)
  echo "device state : $s"
  case "$s" in
    twrp|android)
      adb devices | tail -n +2 | grep -v '^$' | sed 's/^/adb          : /'
      echo -n "para command : "
      adb_sh "dd if=$P/para bs=32 count=1 2>/dev/null" | od -An -tx1 | head -1 || true
      ;;
  esac
  # 0e8d = MediaTek (android 201c / poc 2008 / preloader 2000 / brom 0003),
  # 18d1 = Google AOSP gadget (TWRP/Android recovery adb)
  lsusb 2>/dev/null | grep -iE '0e8d|18d1' | sed 's/^/usb          : /' \
    || echo "usb          : (no MediaTek/AOSP gadget on USB)"
}

cmd_twrp() {
  ensure_twrp
  backup_para
  { printf 'boot-recovery\0'; head -c 18 /dev/zero; } > /tmp/bootcmd.bin
  adb_q push /tmp/bootcmd.bin /tmp/bootcmd.bin >/dev/null
  adb_sh "dd if=/tmp/bootcmd.bin of=$P/para bs=32 count=1 conv=fsync" >/dev/null
  echo ">> para = boot-recovery (sticky TWRP default). Rebooting into TWRP."
  adb_q reboot >/dev/null 2>&1 || true
  wait_for twrp 36
}

cmd_android() {
  ensure_twrp
  backup_para
  adb_sh "dd if=/dev/zero of=$P/para bs=32 count=1 conv=fsync" >/dev/null
  echo ">> para cleared. Rebooting: NORMAL boots whatever is in \`boot\`"
  echo "   (stock Android, or the flashed NixOS boot.img)."
  adb_q reboot >/dev/null 2>&1 || true
  wait_for android 36
}

cmd_debian() {
  ensure_twrp
  backup_para
  # 32-byte command: "boot-debian\0" + 20 zero bytes (para offset 0). LK
  # ignores it (only "boot-recovery" is special to LK); the dual-boot
  # initrd in \`boot\` selects Debian p29 on it (repartition doc §5).
  { printf 'boot-debian\0'; head -c 20 /dev/zero; } > /tmp/bootcmd.bin
  adb_q push /tmp/bootcmd.bin /tmp/bootcmd.bin >/dev/null
  adb_sh "dd if=/tmp/bootcmd.bin of=$P/para bs=32 count=1 conv=fsync" >/dev/null
  echo ">> para=boot-debian. Rebooting — the boot image's Debian branch (p29)."
  echo "   Debian has no adbd: do not wait for adb — expect g_ether"
  echo "   (10.15.19.82 over ssh) in ~30-60s, or use bin/net-up.sh."
  adb_q reboot >/dev/null 2>&1 || true
}

cmd_flash() {
  local img="${1:-$BOOTIMG_DEFAULT}" target="${2:-twrp}" bak
  [ -f "$img" ] || { echo "!! image not found: $img (build it: nix build .#packages.x86_64-linux.default)" >&2; exit 1; }
  case "$target" in twrp|android) ;; *) echo "!! target must be twrp|android (got: $target)" >&2; exit 1 ;; esac
  ensure_twrp
  backup_para
  mkdir -p "$BOOT_BACKUP_DIR"
  bak="$BOOT_BACKUP_DIR/boot-$(date +%Y%m%d-%H%M%S).img"
  echo ">> backing up current boot -> $bak"
  adb_sh "dd if=$P/boot of=/tmp/cur-boot.img bs=1M conv=fsync" >/dev/null
  adb_q pull /tmp/cur-boot.img "$bak" >/dev/null
  echo ">> flashing $img -> boot (16 MiB partition)"
  adb_q push "$img" /tmp/new-boot.img >/dev/null
  adb_sh "dd if=/tmp/new-boot.img of=$P/boot bs=1M conv=fsync" >/dev/null
  echo ">> flashed. Restore with: boot-switch.sh restore (backup kept in $BOOT_BACKUP_DIR)"
  if [ "$target" = android ]; then
    echo ">> rebooting into the flashed image (clears para)..."
    adb_sh "dd if=/dev/zero of=$P/para bs=32 count=1 conv=fsync" >/dev/null
    adb_q reboot >/dev/null 2>&1 || true
    wait_for android 36
  else
    echo ">> staying in TWRP (para untouched — TWRP remains the default)."
  fi
}

cmd_restore() {
  local bak
  ensure_twrp
  bak=$(ls -1t "$BOOT_BACKUP_DIR"/boot-*.img 2>/dev/null | head -1) \
    || { echo "!! no boot backups in $BOOT_BACKUP_DIR" >&2; exit 1; }
  echo ">> restoring $bak -> boot"
  adb_q push "$bak" /tmp/restore-boot.img >/dev/null
  adb_sh "dd if=/tmp/restore-boot.img of=$P/boot bs=1M conv=fsync" >/dev/null
  echo ">> restored. TWRP default kept (para untouched)."
}

# ---- main --------------------------------------------------------------------
case "${1:-}" in
  status)  cmd_status ;;
  twrp)    cmd_twrp ;;
  android) cmd_android ;;
  debian)  cmd_debian ;;
  flash)   cmd_flash "${2:-$BOOTIMG_DEFAULT}" "${3:-twrp}" ;;
  restore) cmd_restore ;;
  -h|--help|help|"") usage ;;
  *) echo "!! unknown command: ${1:-}" >&2; usage; exit 1 ;;
esac

#!/usr/bin/env bash
# fsck-p32.sh — offline ext4 repair of the NixOS rootfs (p32 userdata)
# from TWRP. Scripted form of the 2026-09-09 boot-panic recovery (torn
# orphan chain after a hard power-off → repeated pre-mount panics; fixed
# with e2fsck -fy in TWRP). The initrd now auto-repairs p32 at boot
# (devices/planet-geminipda/initrd.nix), so this is the OPERATOR fallback
# for when the device will not boot at all (TWRP-only).
#
# Usage (from the repo root, device IN TWRP — adb state "recovery"):
#   bash bin/fsck-p32.sh check   # read-only e2fsck -fn (report only)
#   bash bin/fsck-p32.sh repair  # e2fsck -fy + verify (-fn rc=0)
#   bash bin/fsck-p32.sh status  # partition present + e2fsck version
#
# The rootfs must be UNMOUNTED for a trustworthy check: TWRP auto-mounts
# userdata as /data (+/sdcard). This script unmounts both first and
# verifies, then runs the fsck unmounted (the 2026-09-09 lesson: the
# first -fn ran while TWRP had p32 mounted live and reported spurious
# free-count drift).
#
# Safe: read-only for `check`; `repair` writes only the p32 filesystem
# (journal replay + count rebuild). Never touches para/boot/nvram.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# adb lives in the flake devshell (AGENTS.md rule 7) — re-exec once.
if ! command -v adb >/dev/null 2>&1; then
  if [ -z "${GEMINI_DEVSH_REEXEC:-}" ]; then
    export GEMINI_DEVSH_REEXEC=1
    cd "$ROOT"
    exec nix develop --command bash "bin/fsck-p32.sh" "$@"
  fi
  echo "!! adb not found even inside the devshell" >&2
  exit 1
fi

VERB="${1:-check}"
P32=/dev/block/platform/mtk-msdc.0/11230000.msdc0/by-name/userdata

# ---- device must be in TWRP (root adbd) -------------------------------
state=$(adb devices | awk 'NR==2 {print $2}')
if [ "$state" != "recovery" ]; then
  echo "!! device not in TWRP (adb state: '${state:-none}')." >&2
  echo "   Converge with 'bash bin/boot-switch.sh twrp' or RTC FAC_RESET" >&2
  echo "   (hold power + side button ~10 s)." >&2
  exit 1
fi

case "$VERB" in
  status)
    echo "device: $(adb devices | awk 'NR==2 {print $1}') (TWRP)"
    adb shell "ls -l $P32"
    adb shell "e2fsck -V 2>&1 | head -1"
    ;;
  check|repair)
    # ---- unmount p32 (TWRP mounts it as /data and /sdcard) -------------
    adb shell "umount /data 2>/dev/null; umount /sdcard 2>/dev/null; umount /sdcard 2>/dev/null; umount /data 2>/dev/null; sleep 1" >/dev/null
    if adb shell "mount | grep -q $P32"; then
      echo "!! $P32 still mounted — refusing to fsck a live filesystem." >&2
      exit 1
    fi
    echo "==> $P32 unmounted — e2fsck -fn (read-only check)"
    adb shell "e2fsck -fn $P32" | tee /tmp/fsck-p32-check.log || true
    if grep -q "clean" /tmp/fsck-p32-check.log && ! grep -qE "still has errors|was not cleanly|corrupt" /tmp/fsck-p32-check.log; then
      echo "==> filesystem is CLEAN — nothing to repair"
      exit 0
    fi
    if [ "$VERB" = "check" ]; then
      echo "!! filesystem needs repair (see above). Run: bash bin/fsck-p32.sh repair"
      exit 2
    fi
    echo "==> repairing: e2fsck -fy (journal replay + fixes)"
    adb shell "e2fsck -fy $P32" | tee /tmp/fsck-p32-fy.log || true
    echo "==> verifying: e2fsck -fn (expect rc=0, all passes clean)"
    adb shell "e2fsck -fn $P32; echo verify_rc=\$?"
    ;;
  *)
    echo "usage: $0 check|repair|status" >&2
    exit 2
    ;;
esac

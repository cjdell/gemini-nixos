#!/usr/bin/env bash
# usb-watch.sh — watch the Gemini's USB state over time (timestamps).
# Ported from the legacy GeminiPDA build/usb-watch.sh (2026-09-07,
# golden-repo pivot). DR use: boot classification + the BROM-entry
# experiment (docs/disaster-recovery/gather.md step 5).
#
# Device USB states (VID map — see docs/disaster-recovery/README.md):
#   0e8d:2000  preloader download-wait (~9 s window on every power-on)
#   0e8d:2001  patched preloader (post-reset; mtkclient can't handshake it)
#   0e8d:2008  POC charging gadget (charger kernel running, screen dark)
#   0e8d:201c  Android (adb available)
#   18d1:4ee2  TWRP (adb available)
#   0525:a4a2  g_ether (Debian/NixOS: SSH 10.15.19.82)
#   0525:a4a7  g_serial console
#   0e8d:0003  BROM mode
#
# Diagnosis: a repeating 2000 window every ~15-30 s = boot loop (kernel
# hang, LK WDT reset); a single 2000 then silence = battery-gated boot
# (preloader runs on USB power but refuses LK handoff — flat battery).
#
# Usage: bash bin/usb-watch.sh [seconds]   (default: forever, Ctrl-C)
# (needs lsusb — run inside the devshell, or as root on the bare host)
SECS="${1:-0}"
i=0
while true; do
  if [ "$SECS" -gt 0 ] && [ "$i" -ge "$SECS" ]; then break; fi
  line=$(lsusb 2>/dev/null | grep -E "0e8d:|18d1:4ee2|0525:a4a|18d1:" | head -3)
  ts=$(date +%H:%M:%S)
  if [ -n "$line" ]; then
    echo "[$ts] $line" | tr '\n' ' '; echo
  else
    echo "[$ts] (nothing — device offline)"
  fi
  sleep 2
  i=$((i+2))
done

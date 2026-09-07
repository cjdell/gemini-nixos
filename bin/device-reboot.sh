#!/bin/bash
# device-reboot.sh — REMOTE-REBOOT the Gemini PDA.
#
# The device is reachable over the g_ether USB network (10.15.19.82).
# Software resets (systemctl reboot / WDT SWRST) POWER THE DEVICE OFF on
# this unit (verified 2026-08-31) — only the LK-configured WDT EXRST path
# self-boots. So we set the WDT timeout to 2s via /dev/mem and let it
# expire: the WDT fires the external PMIC reset, the device power-cycles
# and boots on its own. Gadget + SSH are back in ~5-10s.
#
# Device-side equivalent (from a root shell, no host): the gemini-wdt-reboot
# unit/CLI in services/scripts does the same devmem write.
#
# Ported from the GeminiPDA project (build/device-reboot.sh).
set -e
cd "$(dirname "$0")/.."
DEV=10.15.19.82
KEY="${GEMINI_SSH_KEY:-$HOME/.ssh/id_ed25519_gemini}"
[ -f "$KEY" ] || { echo "!! SSH key $KEY not found — see bin/device-ssh.sh header" >&2; exit 1; }
SSH() { ssh -i "$KEY" \
  -o BatchMode=yes -o IdentitiesOnly=yes \
  -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
  -o ConnectTimeout=8 root@"$DEV" "$@"; }

if ! ping -c 1 -W 2 "$DEV" >/dev/null 2>&1; then
  echo "!! $DEV not reachable — is the device booted (USB connected)?" >&2
  exit 1
fi

echo ">> arming WDT at 2s (WDT_LENGTH=0x48 @0x10007004) — device will EXRST-reboot in ~5s"
SSH "busybox devmem 0x10007004 32 0x48"
echo ">> reboot triggered; waiting for the gadget to return..."
for i in $(seq 1 30); do
  sleep 5
  if lsusb 2>/dev/null | grep -q "0525:a4a2"; then
    IF=$(ls /sys/class/net/ | grep -E "enp.*u1u2|usb0" | head -1)
    [ -n "$IF" ] && { sudo ip link set "$IF" up 2>/dev/null || true; sudo ip addr add 10.15.19.1/24 dev "$IF" 2>/dev/null || true; }
    for j in $(seq 1 8); do
      sleep 3
      ping -c 1 -W 2 "$DEV" >/dev/null 2>&1 && { echo ">> device back at $DEV ✓"; exit 0; }
    done
  fi
done
echo "!! gadget not seen within 150s — may need a physical power-on" >&2
exit 1

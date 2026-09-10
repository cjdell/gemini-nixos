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
# [added 2026-09-10] The A72 cluster bring-up (services/scripts/cl2-up.sh,
# run by gemini-a72-up on every boot) leaves LK's WDT MODE disarmed
# (0x10007000 = 0) — with MODE clear, the 0x48 LENGTH arm silently
# no-ops and no EXRST ever fires (the documented "reboot trap",
# docs/phase-2-on-glass.md §2b; the register values were re-confirmed on
# glass 2026-09-10: MODE=0, LENGTH=0x40). So restore LK's mode value
# (key | 0x5D) first, then arm. Without this the reboot looks like it
# did nothing and the unit stays up (or `systemctl reboot` powers it off).
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

BEFORE_ID=$(SSH 'cat /proc/sys/kernel/random/boot_id' 2>/dev/null || true)

echo ">> restoring WDT MODE (key|0x5D — A72 bring-up disarms it) + arming WDT at 2s (0x48) — EXRST-reboot in ~5s"
SSH "busybox devmem 0x10007000 32 0x2200005D; busybox devmem 0x10007004 32 0x48"

# Wait for the gadget to DROP first. Without this the "is the gadget
# present?" loop could succeed before the reset fired (the USB stays up
# for the ~2 s WDT window and can re-enumerate fast), reporting a reboot
# that had not happened — observed 2026-09-10.
echo ">> reboot triggered; waiting for the USB gadget to drop (proves the reset fired)..."
dropped=0
for i in $(seq 1 15); do
  sleep 2
  if ! lsusb 2>/dev/null | grep -q "0525:a4a2"; then
    dropped=1
    echo ">> gadget gone after ~$((i * 2))s"
    break
  fi
done
if [ "$dropped" != 1 ]; then
  echo "!! gadget never dropped — the WDT did not fire (A72 MODE disarm trap? see docs/phase-2-on-glass.md section 2b); device is still up" >&2
  exit 1
fi

echo ">> waiting for the gadget to return + ssh..."
for i in $(seq 1 40); do
  sleep 5
  if lsusb 2>/dev/null | grep -q "0525:a4a2"; then
    IF=$(ls /sys/class/net/ | grep -E "enp.*u1u2|usb0" | head -1)
    [ -n "$IF" ] && { sudo ip link set "$IF" up 2>/dev/null || true; sudo ip addr add 10.15.19.1/24 dev "$IF" 2>/dev/null || true; }
    for j in $(seq 1 10); do
      sleep 3
      if ping -c 1 -W 2 "$DEV" >/dev/null 2>&1; then
        # Reachable != rebooted: require the boot id to have changed.
        AFTER_ID=$(SSH 'cat /proc/sys/kernel/random/boot_id' 2>/dev/null || true)
        if [ -n "$BEFORE_ID" ] && [ "$AFTER_ID" = "$BEFORE_ID" ]; then
          echo "!! device reachable but boot_id is unchanged — it did not reboot" >&2
          exit 1
        fi
        echo ">> device back at $DEV (new boot_id) OK"
        exit 0
      fi
    done
  fi
done
echo "!! gadget not seen within 200s — may need a physical power-on" >&2
exit 1

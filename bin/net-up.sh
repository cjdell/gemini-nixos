#!/bin/bash
# net-up.sh — bring up the host side of the g_ether USB network after the
# device has been power-cycled (the gadget iface disappears on power-off,
# so the host address must be re-added every cold boot / WDT-free power
# loss). Idempotent. Usage: sudo bash bin/net-up.sh
#
# Device side: none — the gadget auto-configures 10.15.19.82 at boot.
#
# GOLDEN RULE: the host link is DOWN after any device power-off/power-cycle.
# bin/device-ssh.sh auto-runs this (via passwordless sudo) when the link is
# down, so plain `bash bin/device-ssh.sh '<cmd>'` works after power-on.
#
# Ported from the GeminiPDA project (build/net-up.sh), which verified the
# mechanism on this hardware. See AGENTS.md.
set -u

[ "$(id -u)" = 0 ] || { echo "!! needs root for 'ip' — run: sudo bash bin/net-up.sh" >&2; exit 1; }

IFACE="enp10s0f4u1u2"     # USB gadget/RNDIS interface name on this host
HOST_IP="10.15.19.1/24"
DEV_IP="10.15.19.82"

# discover the iface if the hardcoded name is missing (laptop USB ports
# can renumber it)
if ! ip link show "$IFACE" >/dev/null 2>&1; then
  CAND=$(ip -o link show 2>/dev/null | grep -iE "enp.*u[0-9]|usb" | awk -F': ' '{print $2}' | tr -d ' ' | head -1)
  [ -n "${CAND:-}" ] && IFACE="$CAND"
fi

ip link show "$IFACE" >/dev/null 2>&1 || { echo "!! gadget iface '$IFACE' not found — is the device powered on and USB connected?" >&2; exit 1; }

ip link set "$IFACE" up
ip addr replace "$HOST_IP" dev "$IFACE" 2>/dev/null || ip addr add "$HOST_IP" dev "$IFACE"

ping -c 1 -W 2 "$DEV_IP" >/dev/null 2>&1 && echo "OK: $DEV_IP reachable on $IFACE" || echo "!! $DEV_IP not answering yet (device still booting? retry in a few s)"

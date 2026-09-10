#!/bin/bash
# device-ssh.sh — SSH into the Gemini PDA over the g_ether USB network.
# Usage: bash bin/device-ssh.sh [command...]   (no args = interactive shell)
# Device: 10.15.19.82, root. KEY-BASED auth (2026-09-02): the host key
# ~/.ssh/id_ed25519_gemini is installed on the device
# (/root/.ssh/authorized_keys).
#
# To re-provision a fresh rootfs (password: toor):
#   nix shell nixpkgs#sshpass -- -p toor ssh -o StrictHostKeyChecking=no \
#     -o UserKnownHostsFile=/dev/null root@10.15.19.82 \
#     'umask 077; cat >> /root/.ssh/authorized_keys' < ~/.ssh/id_ed25519_gemini.pub
#
# GOLDEN RULE: the host side of the g_ether link is DOWN after any device
# power-off/power-cycle. If the link looks down, bring it up automatically
# via bin/net-up.sh (passwordless sudo). This is why plain
# `bash bin/device-ssh.sh '<cmd>'` works right after the device boots.
#
# NOTE: while the device runs the *GeminiPDA Debian* rootfs (pre-NixOS),
# this reaches that rootfs over sshd. After the NixOS rootfs is flashed
# to p29, the same address/key reach the NixOS stage-2 sshd instead —
# nothing else changes.
#
# Ported from the GeminiPDA project (build/device-ssh.sh).
KEY="${GEMINI_SSH_KEY:-$HOME/.ssh/id_ed25519_gemini}"
if [ ! -f "$KEY" ]; then
  echo "!! SSH key $KEY not found — re-provision it (see header)" >&2
  exit 1
fi
cd "$(dirname "$0")/.."

# Explicit device address override (GEMINI_DEV_IP <ip>): use it when the
# unit is reachable some other way (Wi-Fi/LAN) and the g_ether USB link
# is not in use. Host-side gadget setup is skipped in that case.
if [ -n "${GEMINI_DEV_IP:-}" ]; then
  exec ssh -i "$KEY" \
    -o BatchMode=yes -o IdentitiesOnly=yes \
    -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
    -o ConnectTimeout=8 root@"$GEMINI_DEV_IP" "$@"
fi

IFACE="enp10s0f4u1u2"
# iface discovery when the hardcoded name is missing (USB ports renumber)
if ! ip link show "$IFACE" >/dev/null 2>&1; then
  CAND=$(ip -o link show 2>/dev/null | grep -iE "enp.*u[0-9]|rndis|usb" | awk -F': ' '{print $2}' | tr -d ' ' | head -1)
  [ -n "${CAND:-}" ] && IFACE="$CAND"
fi

if ! ip link show "$IFACE" >/dev/null 2>&1; then
  echo "!! gadget iface '$IFACE' not found — device powered on and USB connected?" >&2
  exit 1
fi
# link up but no host address, or device unreachable -> (re)assign the IP
if ! ip addr show "$IFACE" | grep -q "10.15.19.1/24" ||
     ! ping -c 1 -W 1 10.15.19.82 >/dev/null 2>&1; then
  if sudo -n ip link set "$IFACE" up 2>/dev/null &&
     sudo -n ip addr replace 10.15.19.1/24 dev "$IFACE" 2>/dev/null; then
    echo "device-ssh: g_ether link brought up (10.15.19.1/24 on $IFACE)" >&2
  else
    echo "!! cannot configure $IFACE — run: sudo bash bin/net-up.sh" >&2
    exit 1
  fi
fi

exec ssh -i "$KEY" \
  -o BatchMode=yes -o IdentitiesOnly=yes \
  -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
  -o ConnectTimeout=8 root@10.15.19.82 "$@"

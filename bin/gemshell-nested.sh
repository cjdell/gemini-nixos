#!/bin/bash
# gemshell-nested.sh — run the gemshell compositor as a NESTED Wayland
# client on this (x86_64) workstation, for fast UI iteration.
#
# What it does:
#   1. builds the x86_64-linux package (`nix build .#packages.x86_64-linux.gemshell`)
#   2. runs it with GEMSHELL_NESTED=1 under the CURRENT graphical session
#      (Wayland; KWin on this box), presenting the scene in an
#      xdg_toplevel window.
#   3. optionally autostarts gemsettings (GEMSHELL_AUTOSTART) so the
#      window has content immediately.
#
# Env knobs:
#   GEMSHELL_NESTED_SCALE   window size / logical scene size (default 0.5)
#   GEMSHELL_AUTOSTART      client to launch (default gemsettings; "" = none)
#   WAYLAND_DISPLAY         host socket (default wayland-0)
#   GEMSHELL_BUILD=0        skip the nix build (use the cached path)
#
# Design + receipts: docs/gemshell.md ("Nested mode").
#
# Usage: bash bin/gemshell-nested.sh [extra gemshell args...]
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo"

# --- host session env -------------------------------------------------------
: "${XDG_RUNTIME_DIR:=/run/user/$(id -u)}"
export XDG_RUNTIME_DIR
export WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-0}"
if [ ! -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]; then
    echo "gemshell-nested: no Wayland socket at $XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" >&2
    echo "  (run from inside a graphical session, or set WAYLAND_DISPLAY)" >&2
    exit 1
fi
export HOME="${HOME:-/home/$(id -un)}"
export GEMSHELL_NESTED=1
export GEMSHELL_NESTED_SCALE="${GEMSHELL_NESTED_SCALE:-0.5}"
# The gemini xkb layout (Fn layer) straight from the repo tree.
export XKB_CONFIG_EXTRA_PATH="$repo/config/xkb"
export XKB_DEFAULT_LAYOUT="${XKB_DEFAULT_LAYOUT:-gemini}"
# A UI font: find_font() checks GEMSHELL_FONT first; the workstation has
# no /usr/share/fonts, so ask fontconfig.
if [ -z "${GEMSHELL_FONT:-}" ]; then
    if command -v fc-match >/dev/null 2>&1; then
        GEMSHELL_FONT=$(fc-match -f '%{file}' sans 2>/dev/null || true)
    fi
fi
export GEMSHELL_FONT
# Autostart a client by default so something is on screen.
if [ -z "${GEMSHELL_AUTOSTART+x}" ]; then
    GEMSHELL_AUTOSTART=gemsettings
fi
export GEMSHELL_AUTOSTART

# --- build ------------------------------------------------------------------
if [ "${GEMSHELL_BUILD:-1}" != 0 ]; then
    echo "== building x86_64 gemshell ==" >&2
    out=$(nix build --no-link --print-out-paths ".#packages.x86_64-linux.gemshell" | tail -1)
else
    out=$(nix build --no-link --print-out-paths ".#packages.x86_64-linux.gemshell" | tail -1)
fi
echo "== gemshell: $out ==" >&2

# Put gemsettings on PATH for GEMSHELL_AUTOSTART.
export PATH="$out/bin:$PATH"

exec "$out/bin/gemshell" "$@"

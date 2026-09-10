#!/bin/bash
# exodus-on-glass.sh — run GEMINI: EXODUS on the PDA's Wayland session
# from the host, with the correct session env + user, so the long
# `sudo -u cjdell -H env XDG_RUNTIME_DIR=… WAYLAND_DISPLAY=… gemini-exodus …`
# incantation does not have to be retyped (rule 6: scripts over ad-hoc
# commands). This is how the 2026-09-11 on-glass pass was run.
#
# Usage:
#   bash bin/exodus-on-glass.sh [--session gnome|gemwl] [-- GEMINI_ARGS...]
#   (no args → the show, fullscreen, with audio)
#
# Examples:
#   bash bin/exodus-on-glass.sh                                  # watch the show
#   bash bin/exodus-on-glass.sh -- --stats                       # + frametime HUD
#   bash bin/exodus-on-glass.sh -- --stress 3 --section 5 --stats
#   bash bin/exodus-on-glass.sh -- --bench 20 --chapter-secs 4 --json
#   bash bin/exodus-on-glass.sh --session gemwl -- --stats
#
# Sessions:
#   gnome (default) — user cjdell, XDG_RUNTIME_DIR=/run/user/1000
#     (GDM/Mutter). NOTE: the display path is CPU-shadow-blit bound, so
#     fullscreen tops out ~26-40 fps depending on machine load even for a
#     flat triangle; see docs/gemini-exodus.md "Status".
#   gemwl — user cjdell, XDG_RUNTIME_DIR=/run/gemwl (the GPU-direct LK
#     framebuffer compositor). Only valid when GNOME is OFF (services/
#     desktop.nix); this is the no-shadow-blit path to 60 fps.
#
# The app blocks the ssh session until it exits (or `--bench` finishes).
# Long runs belong under `bash bin/run-job.sh start <name> -- bash bin/exodus-on-glass.sh …`.

set -euo pipefail
cd "$(dirname "$0")/.."

session=gnome
args=()
while [ $# -gt 0 ]; do
    case "$1" in
    --session)
        session="${2:?--session needs gnome|gemwl}"
        shift 2
        ;;
    --)
        shift
        args=("$@")
        break
        ;;
    *)
        args+=("$1")
        shift
        ;;
    esac
done

case "$session" in
gnome)
    rt=/run/user/1000
    user=cjdell
    dbus="DBUS_SESSION_BUS_ADDRESS=unix:path=$rt/bus"
    ;;
gemwl)
    rt=/run/gemwl
    user=cjdell
    dbus=""
    ;;
*)
    echo "exodus-on-glass: unknown session '$session' (gnome|gemwl)" >&2
    exit 2
    ;;
esac

q() { printf '%q ' "$@"; }
remote="sudo -u $user -H env XDG_RUNTIME_DIR=$rt WAYLAND_DISPLAY=wayland-0 $dbus gemini-exodus"
if [ ${#args[@]} -gt 0 ]; then
    remote+=" $(q ${args[@]+"${args[@]}"})"
fi

exec bash bin/device-ssh.sh "$remote"

#!/bin/sh
# wine-x86-deploy.sh — x86-64 Windows apps on the Gemini PDA:
# wine64 under box64, deployed as a standalone GC root (NOT part of the
# flake system — the 1.8 GB wine64 closure must not bloat system
# deploys). Full story + receipts: docs/wine-d3d.md.
#
# Stack (nixpkgs channel rev dc5d91f84032 — see pkgs/wine-x86.nix):
#   box64 0.4.4      aarch64 x86-64 binary translator
#   wine64 11.0      x86_64-linux Windows emulator (runs under box64)
#   mesa 26.2.2      x86_64-linux GL impl for guest wined3d (panfrost
#                    gallium; wired in via __EGL_VENDOR_LIBRARY_FILENAMES —
#                    the wine64 closure has glvnd but no ICD)
#   d3d9test 1.0     x86_64-windows PE (mingw-w64 cross; pkgs/d3d9test.nix)
#   grim 1.5.0       wayland screenshots on gemwl (run receipts)
#
# Usage (from repo root; needs the device on g_ether — see device-ssh.sh):
#   bash bin/wine-x86-deploy.sh status
#   bash bin/wine-x86-deploy.sh deploy     # host store -> device store +
#                                          # launcher + GC roots (~1.9 GB copy)
#   bash bin/wine-x86-deploy.sh init       # create the wine prefix (wineboot)
#   bash bin/wine-x86-deploy.sh run [exe]  # run an app detached on the device
#                                          # (default d3d9test.exe; log:
#                                          # /root/wine-x86/logs/app.log)
#   bash bin/wine-x86-deploy.sh log        # show the app log
#   bash bin/wine-x86-deploy.sh shot [f]   # grim screenshot -> device,
#                                          # copied to $f (default ./wine-shot.png)
#   bash bin/wine-x86-deploy.sh kill       # stop the wine app
#
# Notes:
#  - `deploy` is a long nix copy: run under `bash bin/run-job.sh start
#    wine-deploy -- bash bin/wine-x86-deploy.sh deploy` (rule 8).
#  - The device launcher is /root/wine-x86/wine-x86; it sets the wine
#    prefix + the gemwl wayland env and execs
#    `box64 <wine64>/bin/.wine "$@"`.
#  - box86 (32-bit guests) is NOT shipped: d3d9test is a 64-bit PE and
#    wine64 is a 64-bit-only prefix.

set -eu

cd "$(dirname "$0")/.."

VERB="${1:-status}"
shift || true

dev() { bash bin/device-ssh.sh "$@"; }

NP_NIX=pkgs/wine-x86.nix

# Resolve a stack attribute to its (host-store) out path, building if
# needed (substitutes from cache.nixos.org — all verified cached).
path_of() {
    nix build --impure --print-out-paths --no-link \
        --expr "(import ./${NP_NIX}) . ${1}" 2>/dev/null | tail -1
}

WINE64=""; BOX64=""; D3D9=""; MESA=""; GRIM=""
resolve_paths() {
    [ -n "$WINE64" ] && return 0
    WINE64=$(path_of wine64);   BOX64=$(path_of box64)
    D3D9=$(path_of d3d9test);   MESA=$(path_of mesa)
    GRIM=$(path_of grim)
}

case "$VERB" in
status)
    echo "== host store =="
    for a in wine64 box64 d3d9test mesa grim; do
        p=$(path_of "$a")
        case "$p" in
            /nix/store/*) printf '  in-store  %s\n' "$p" ;;
            *)            printf '  NOT BUILT %s\n' "$a" ;;
        esac
    done
    echo "== device =="
    # NOTE: pkill/pgrep -f patterns must use the [x] bracket trick — a
    # plain pattern matches this very ssh shell's cmdline and kills/
    # lists it (observed 2026-09-09: self-killed shells = empty output).
    dev 'ls -d /root/wine-x86 /root/.wine-x86 2>/dev/null; ls /root/wine-x86/apps 2>/dev/null; pgrep -a -f "[d]3d9test" | head -3' \
        || echo "  (device unreachable)"
    ;;

deploy)
    resolve_paths
    echo "paths: wine64=$WINE64 box64=$BOX64 d3d9=$D3D9 mesa=$MESA grim=$GRIM"
    echo "copying to device (ssh://10.15.19.82) — ~2.2 GB, use run-job..."
    # nix copy ships each path's closure; already-present paths are
    # skipped by the device store.
    # NIX_SSHOPTS: same known_hosts independence as bin/deploy.sh (fresh
    # rootfs = new host key; 2026-09-10). [added 2026-09-10p]
    NIX_SSHOPTS="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null" \
        nix copy --to ssh://10.15.19.82 "$WINE64" "$BOX64" "$D3D9" "$MESA" "$GRIM"
    echo "installing launcher + app + GC root on device..."
    dev "
set -e
mkdir -p /root/wine-x86/apps /root/wine-x86/logs
cp -f $D3D9/bin/d3d9test.exe /root/wine-x86/apps/d3d9test.exe
cat > /root/wine-x86/wine-x86 <<'EOF'
#!/usr/bin/env bash
# wine-x86 — run an x86_64 Windows app on the Gemini PDA: wine64 under
# box64. Usage: wine-x86 <app.exe> [args...]
# Display: gemwl wayland socket (XDG_RUNTIME_DIR=/run/gemwl).
set -euo pipefail
WINE64=$WINE64
BOX64=$BOX64
MESAPATH=$MESA
export WINEPREFIX=\"\${WINEPREFIX:-/root/.wine-x86}\"
export WINEDEBUG=\"\${WINEDEBUG:-warn-all}\"
export HOME=\"\${HOME:-/root}\"
# Guest GL: the wine64 closure has glvnd (libEGL dispatch) but no ICD —
# point it at the shipped x86_64 mesa (panfrost gallium; the manifest's
# store path is identical on this machine). glvnd 1.7 env var names
# verified from the lib: __EGL_VENDOR_LIBRARY_FILENAMES / _DIRS.
export __EGL_VENDOR_LIBRARY_FILENAMES=\"\${__EGL_VENDOR_LIBRARY_FILENAMES:-\$MESAPATH/share/glvnd/egl_vendor.d/50_mesa.json}\"
if [ -z \"\${WAYLAND_DISPLAY:-}\" ]; then
    if [ -S /run/gemwl/wayland-0 ]; then
        export XDG_RUNTIME_DIR=/run/gemwl
        export WAYLAND_DISPLAY=wayland-0
    fi
fi
exec \"$BOX64/bin/box64\" \"$WINE64/bin/.wine\" \"\$@\"
EOF
chmod +x /root/wine-x86/wine-x86
mkdir -p /nix/var/nix/gcroots/wine-x86
ln -sfn $WINE64 /nix/var/nix/gcroots/wine-x86/wine64
ln -sfn $BOX64  /nix/var/nix/gcroots/wine-x86/box64
ln -sfn $D3D9   /nix/var/nix/gcroots/wine-x86/d3d9test
ln -sfn $MESA   /nix/var/nix/gcroots/wine-x86/mesa
ln -sfn $GRIM   /nix/var/nix/gcroots/wine-x86/grim
echo 'device install OK'
"
    echo "pinning host GC root..."
    sudo mkdir -p /nix/var/nix/gcroots/wine-x86
    sudo ln -sfn "$WINE64" /nix/var/nix/gcroots/wine-x86/wine64
    sudo ln -sfn "$BOX64"  /nix/var/nix/gcroots/wine-x86/box64
    sudo ln -sfn "$D3D9"   /nix/var/nix/gcroots/wine-x86/d3d9test
    sudo ln -sfn "$MESA"   /nix/var/nix/gcroots/wine-x86/mesa
    sudo ln -sfn "$GRIM"   /nix/var/nix/gcroots/wine-x86/grim
    echo "deploy done."
    ;;

init)
    resolve_paths
    echo "wineboot (prefix init) on device — first run can take a few minutes under box64..."
    dev "
if WINEPREFIX=/root/.wine-x86 WINEDEBUG=-all \
    $BOX64/bin/box64 $WINE64/bin/.wine wineboot -u; then
  echo WINEBOOT-OK
else
  echo WINEBOOT-FAILED
fi
ls /root/.wine-x86/drive_c/ 2>/dev/null | head
"
    ;;

run)
    APP="${1:-d3d9test.exe}"
    case "$APP" in
        /*) : ;;
        *)  APP="/root/wine-x86/apps/$APP" ;;
    esac
    # stop any previous instance first (wine renames the process to the
    # app basename — pkill -x box64 alone does NOT kill a running app).
    # pkill -x matches the process COMM (15-char), which can never match
    # this shell's cmdline — a pkill -f pattern here would, because the
    # app PATH itself (…/d3d9test.exe) is on this very command line.
    APPNAME=$(printf '%.15s' "${APP##*/}")
    dev "
(pkill -9 -x wineserver; pkill -9 -x box64; pkill -9 -x $APPNAME) 2>/dev/null || true
sleep 1; rm -f /root/.wine-x86/lock 2>/dev/null || true
setsid /root/wine-x86/wine-x86 $APP \
  > /root/wine-x86/logs/app.log 2>&1 < /dev/null &
echo started pid=\$!
"
    echo "app started (log: device:/root/wine-x86/logs/app.log; 'bash bin/wine-x86-deploy.sh log')"
    ;;

log)
    dev 'tail -40 /root/wine-x86/logs/app.log 2>/dev/null || echo "(no log yet)"'
    ;;

shot)
    OUT="${1:-wine-shot.png}"
    resolve_paths
    dev "
XDG_RUNTIME_DIR=/run/gemwl WAYLAND_DISPLAY=wayland-0 \
  $GRIM/bin/grim /root/wine-x86/logs/shot.png
"
    scp -q root@10.15.19.82:/root/wine-x86/logs/shot.png "$OUT"
    echo "screenshot: $OUT"
    ;;

kill)
    dev '(pkill -9 -x wineserver; pkill -9 -x box64; pkill -9 -f "[d]3d9test") 2>/dev/null || true; echo killed'
    ;;

*)
    echo "unknown verb: $VERB (status|deploy|init|run|log|shot|kill)" >&2
    exit 2
    ;;
esac

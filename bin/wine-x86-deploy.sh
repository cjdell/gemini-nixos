#!/bin/sh
# wine-x86-deploy.sh — run Windows PEs on the Gemini PDA:
# wine-wow64 11.0 under box64, deployed as a standalone GC root (NOT part
# of the flake system — the wow64 closure must not bloat system deploys).
# Full story + receipts: docs/wine-d3d.md.
#
# Stack (nixpkgs channel rev dc5d91f84032 — see pkgs/wine-x86.nix):
#   wineWow64 11.0   x86_64-linux wine-wow64 (runs under box64); carries
#                    the i386-windows syswow64 payload, so BOTH 32- and
#                    64-bit PEs run from it
#   box64 0.4.4      aarch64 x86-64 binary translator
#   mesa 26.2.2      x86_64-linux GL impl for guest wined3d (panfrost
#                    gallium; wired in via __EGL_VENDOR_LIBRARY_FILENAMES —
#                    the wow64 closure has glvnd but no ICD)
#   d3d9test 1.0     x86_64-windows PE (mingw-w64 cross; pkgs/d3d9test.nix)
#   grim 1.5.0       wayland screenshots (gemwl-only receipts)
#
# Runtime lives OUTSIDE /root so the unprivileged desktop user (cjdell)
# can exec it: /var/lib/wine-x86/wine-wow (launcher), GC roots under
# /nix/var/nix/gcroots/wine-x86/. The system `wine`/`wine64` wrappers
# (pkgs/wine-cli.nix) exec that launcher; the per-user WINEPREFIX
# defaults to $HOME/.wine-x86. [moved from /root 2026-09-11: the closure
# wrapper exec'd /root/wine-x86/wine-wow, unreachable for non-root]
#
# Usage (from repo root; needs the device on g_ether — see device-ssh.sh):
#   bash bin/wine-x86-deploy.sh status
#   bash bin/wine-x86-deploy.sh deploy       # host store -> device store +
#                                            # launcher + GC roots
#   bash bin/wine-x86-deploy.sh init [user]  # create the prefix (wineboot)
#   bash bin/wine-x86-deploy.sh run [exe]    # run an app detached on the
#                                            # device (default d3d9test.exe)
#   bash bin/wine-x86-deploy.sh log          # show the app log
#   bash bin/wine-x86-deploy.sh shot [f]     # grim screenshot -> host
#   bash bin/wine-x86-deploy.sh kill         # stop wine apps/servers
#
# Notes:
#  - `deploy` is a long nix copy: run under `bash bin/run-job.sh start
#    wine-deploy -- bash bin/wine-x86-deploy.sh deploy` (rule 8).
#  - pkill/pgrep -f patterns MUST use the [x] bracket trick — a plain
#    pattern matches this very ssh shell's cmdline (observed 2026-09-09).
#  - GEMINI_WINE_USER overrides the desktop user (default cjdell).

set -eu

cd "$(dirname "$0")/.."

VERB="${1:-status}"
shift || true

dev() { bash bin/device-ssh.sh "$@"; }

NP_NIX=pkgs/wine-x86.nix
RUNDIR=/var/lib/wine-x86
RUNUSER="${GEMINI_WINE_USER:-cjdell}"

# Resolve a stack attribute to its (host-store) out path, building if
# needed (substitutes from cache.nixos.org — all verified cached).
path_of() {
    nix build --impure --print-out-paths --no-link \
        --expr "(import ./${NP_NIX}) . ${1}" 2>/dev/null | tail -1
}

WOW64=""; BOX64=""; MESA=""; GRIM=""; D3D9=""
resolve_paths() {
    [ -n "$WOW64" ] && return 0
    WOW64=$(path_of wineWow64); BOX64=$(path_of box64)
    MESA=$(path_of mesa);       GRIM=$(path_of grim)
    D3D9=$(path_of d3d9test)
}

case "$VERB" in
status)
    echo "== host store =="
    for a in wineWow64 box64 d3d9test mesa grim; do
        p=$(path_of "$a")
        case "$p" in
            /nix/store/*) printf '  in-store  %s\n' "$p" ;;
            *)            printf '  NOT BUILT %s\n' "$a" ;;
        esac
    done
    echo "== device =="
    dev "ls -la $RUNDIR 2>/dev/null; pgrep -a -f '[b]ox64|[w]ine' | head -3" \
        || echo "  (device unreachable)"
    ;;

deploy)
    resolve_paths
    echo "paths: wow64=$WOW64 box64=$BOX64 d3d9=$D3D9 mesa=$MESA grim=$GRIM"
    echo "copying to device (ssh://10.15.19.82) — ~1 GB, use run-job..."
    # nix copy ships each path's closure; already-present paths are
    # skipped by the device store.
    # NIX_SSHOPTS: same known_hosts independence as bin/deploy.sh (fresh
    # rootfs = new host key; 2026-09-10).
    NIX_SSHOPTS="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null" \
        nix copy --to ssh://10.15.19.82 "$WOW64" "$BOX64" "$D3D9" "$MESA" "$GRIM"
    echo "installing launcher + app + GC root on device..."
    dev "
set -e
mkdir -p $RUNDIR/apps $RUNDIR/logs
cp -f $D3D9/bin/d3d9test.exe $RUNDIR/apps/d3d9test.exe
cat > $RUNDIR/wine-wow <<'EOF'
#!/bin/sh
# wine-wow — run a Windows PE (32- or 64-bit) on the Gemini PDA:
# wine-wow64 11.0 under box64. Usage: wine-wow <app.exe> [args...]
# Prefix defaults to \$HOME/.wine-x86. GL = guest panfrost via the
# x86_64 mesa beside the stack (docs/wine-d3d.md §7).
set -eu
export WINEPREFIX=\"\${WINEPREFIX:-\$HOME/.wine-x86}\"
export WINEDEBUG=\"\${WINEDEBUG:-warn-all}\"
export __EGL_VENDOR_LIBRARY_FILENAMES=\"\${__EGL_VENDOR_LIBRARY_FILENAMES:-$MESA/share/glvnd/egl_vendor.d/50_mesa.json}\"
exec \"$BOX64/bin/box64\" \"$WOW64/bin/.wine\" \"\$@\"
EOF
chmod 0755 $RUNDIR/wine-wow
ln -sfn wine-wow $RUNDIR/wine64
chown -R $RUNUSER $RUNDIR/apps $RUNDIR/logs
mkdir -p /nix/var/nix/gcroots/wine-x86
ln -sfn $WOW64 /nix/var/nix/gcroots/wine-x86/wine-wow64
ln -sfn $BOX64 /nix/var/nix/gcroots/wine-x86/box64
ln -sfn $MESA  /nix/var/nix/gcroots/wine-x86/mesa
ln -sfn $GRIM  /nix/var/nix/gcroots/wine-x86/grim
ln -sfn $D3D9  /nix/var/nix/gcroots/wine-x86/d3d9test
echo 'device install OK'
"
    echo "pinning host GC root..."
    sudo mkdir -p /nix/var/nix/gcroots/wine-x86
    sudo ln -sfn "$WOW64" /nix/var/nix/gcroots/wine-x86/wine-wow64
    sudo ln -sfn "$BOX64" /nix/var/nix/gcroots/wine-x86/box64
    sudo ln -sfn "$MESA"  /nix/var/nix/gcroots/wine-x86/mesa
    sudo ln -sfn "$GRIM"  /nix/var/nix/gcroots/wine-x86/grim
    sudo ln -sfn "$D3D9"  /nix/var/nix/gcroots/wine-x86/d3d9test
    echo "deploy done."
    ;;

init)
    resolve_paths
    U="${1:-$RUNUSER}"
    echo "wineboot (prefix init) for $U on device — first run can take a few minutes under box64..."
    dev "
UHOME=\$(getent passwd $U | cut -d: -f6)
[ -n \"\$UHOME\" ] || { echo '!! no such user: $U' >&2; exit 1; }
if su -s /bin/sh $U -c 'WINEDEBUG=-all $RUNDIR/wine-wow wineboot -u' >/dev/null 2>&1; then
  echo WINEBOOT-OK
else
  echo WINEBOOT-FAILED
fi
ls -d \"\$UHOME/.wine-x86/drive_c\" 2>/dev/null && echo PREFIX-OK || echo PREFIX-MISSING
"
    ;;

run)
    APP="${1:-d3d9test.exe}"
    case "$APP" in
        /*) : ;;
        *)  APP="$RUNDIR/apps/$APP" ;;
    esac
    # stop any previous instance first (wine renames the process to the
    # app basename — pkill -x box64 alone does NOT kill a running app).
    # pkill -x matches the process COMM (15-char), which can never match
    # this shell's cmdline.
    APPNAME=$(printf '%.15s' "${APP##*/}")
    dev "
(pkill -9 -x wineserver; pkill -9 -x box64; pkill -9 -x $APPNAME) 2>/dev/null || true
sleep 1
su -s /bin/sh $RUNUSER -c 'XDG_RUNTIME_DIR=/run/user/\$(id -u) WAYLAND_DISPLAY=\${WAYLAND_DISPLAY:-wayland-0} setsid $RUNDIR/wine-wow $APP > $RUNDIR/logs/app.log 2>&1 < /dev/null &'
echo started
"
    echo "app started (log: device:$RUNDIR/logs/app.log; 'bash bin/wine-x86-deploy.sh log')"
    ;;

log)
    dev "tail -40 $RUNDIR/logs/app.log 2>/dev/null || echo '(no log yet)'"
    ;;

shot)
    OUT="${1:-wine-shot.png}"
    resolve_paths
    dev "
XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-0 \
  $GRIM/bin/grim $RUNDIR/logs/shot.png" || true
    if scp -q root@10.15.19.82:$RUNDIR/logs/shot.png "$OUT" 2>/dev/null; then
        echo "screenshot: $OUT"
    else
        echo "no screenshot (grim needs wlr-screencopy; mutter/GNOME does not expose it)"
    fi
    ;;

kill)
    dev '(pkill -9 -x wineserver; pkill -9 -x box64; pkill -9 -f "[d]3d9test") 2>/dev/null || true; echo killed'
    ;;

*)
    echo "unknown verb: $VERB (status|deploy|init|run|log|shot|kill)" >&2
    exit 2
    ;;
esac

#!/bin/bash
# gemshell-dev.sh — fast on-glass iteration loop for the gemshell
# compositor, WITHOUT a full system generation deploy.
#
# Why this exists: gemshell runs as a systemd SYSTEM service whose
# ExecStart points into the deployed toplevel's store path. Iterating on
# the compositor via `bin/deploy.sh deploy` (build the whole toplevel →
# ship the closure → set the system profile → switch) is minutes per
# round and rewrites /nix/var/nix/profiles/system. During bring-up we
# only need the single `gemshell` package swapped and run with the SAME
# environment the installed unit uses.
#
# How: build the package for aarch64, `nix copy` it to the device, then
# stop the installed unit and run the new binary as a transient
# systemd-run unit (`gemshell-dev.service`) seeded with the environment
# copied verbatim from gemini-gemshell.service. Logs go to the journal.
# Nothing in /etc or the system profile is touched, so the device stays
# on its current generation and `stop` restores the installed service.
#
# Usage (from the repo root):
#   bash bin/gemshell-dev.sh run     build + copy + (re)start the transient unit
#   bash bin/gemshell-dev.sh shot [OUT.png]
#                                     run + gemsettings + GEMSHELL_SCREENSHOT,
#                                     pull the frame (a client is on screen)
#   bash bin/gemshell-dev.sh logs    journal of the transient unit
#   bash bin/gemshell-dev.sh stop    stop transient unit + restart the installed one
#   bash bin/gemshell-dev.sh status  what is running now
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo"

dev="${GEMINI_DEV_IP:-10.15.19.82}"
key="$HOME/.ssh/id_ed25519_gemini"
ssh_opts=(-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -i "$key")
unit=gemshell-dev.service

dev_ssh() { bash "$repo/bin/device-ssh.sh" "$1"; }

build_and_copy() {
    echo "== building gemshell (aarch64) ==" >&2
    local out
    out=$(sudo nix build --store local .#packages.aarch64-linux.gemshell \
        --print-out-paths --no-link \
        --option builders @/etc/nix/machines --fallback 2>/dev/null | tail -1)
    [ -n "$out" ] || { echo "build produced no path" >&2; exit 1; }
    echo "== copying $out to $dev ==" >&2
    NIX_SSHOPTS="${ssh_opts[*]}" nix copy --to "ssh://root@$dev" "$out" >&2
    printf '%s' "$out"
}

run() {
    local out extra="${1:-}" launch="${2:-}" settle="${3:-5}" extra_line="" settings_line=""
    for e in $extra; do extra_line="$extra_line args+=(--setenv=$e)"; done
    out=$(build_and_copy)
    if [ "$launch" = settings ]; then
        settings_line="systemctl reset-failed gemsettings-dev.service 2>/dev/null || true; systemd-run --unit=gemsettings-dev --uid=cjdell --collect --setenv=XDG_RUNTIME_DIR=/run/gemshell --setenv=WAYLAND_DISPLAY=wayland-0 --setenv=HOME=/home/cjdell --setenv=PATH=$out/bin:/run/current-system/sw/bin --setenv=PULSE_SERVER=unix:/run/gemwl-audio/pulse/native --setenv=PIPEWIRE_RUNTIME_DIR=/run/gemwl-audio $out/bin/gemsettings"
    fi
    echo "== running $out on the device (transient unit) ==" >&2
    dev_ssh "set -e
        systemctl stop gemini-gemshell.service 2>/dev/null || true
        systemctl stop $unit 2>/dev/null || true
        systemctl reset-failed $unit 2>/dev/null || true
        envstr=\$(systemctl show gemini-gemshell.service -p Environment | sed 's/^Environment=//')
        args=()
        # HOME is set by the CURRENT module source; the installed unit on
        # the device may predate it, so drop any and re-add ours.
        for kv in \$envstr; do
            case \$kv in HOME=*) continue;; esac
            args+=(\"--setenv=\$kv\")
        done
        args+=("--setenv=HOME=/home/cjdell")
        # Make the compositor's spawn of gemsettings find the
        # freshly-built client, not the old system-profile one.
        args+=("--setenv=PATH=$out/bin:/run/current-system/sw/bin:/run/wrappers/bin")
        $extra_line
        mkdir -p /run/gemshell
        systemd-run --unit=$unit --uid=cjdell \
            --property=RuntimeDirectory=gemshell \
            --property=RuntimeDirectoryMode=0700 \
            --property=Restart=no \
            \"\${args[@]}\" $out/bin/gemshell
        sleep 1
        $settings_line
        sleep $((settle - 1))
        systemctl is-active $unit || true
        systemctl is-active gemsettings-dev.service 2>/dev/null || true
        echo '--- journal ---'
        journalctl -u $unit --no-pager -o cat | tail -60"
}

shot() {
    # Scene-FBO readback only. Reading the LK fb dma-buf from the CPU
    # (GEMSHELL_SCREENSHOT_FB) HUNG the unit until the WDT reset it
    # (2026-09-11) — the driver mmap guard landed, but the path stays
    # unused until it is re-verified on a throwaway boot.
    local outfile="${1:-$repo/gemshell-shot.png}"
    run "GEMSHELL_SCREENSHOT=/tmp/gemshell-shot.png GEMSHELL_SCREENSHOT_DELAY_MS=7000" settings 9
    scp "${ssh_opts[@]}" "root@$dev:/tmp/gemshell-shot.png" "$outfile" >&2
    echo "saved $outfile (scene FBO, compositor + gemsettings)"
}

logs()      { dev_ssh "journalctl -u $unit --no-pager -o cat | tail -120"; }
status()    { dev_ssh "systemctl is-active $unit 2>/dev/null || true; systemctl is-active gemini-gemshell.service || true"; }
stop() {
    dev_ssh "systemctl stop $unit 2>/dev/null || true; systemctl reset-failed $unit 2>/dev/null || true; systemctl restart gemini-gemshell.service 2>/dev/null || true"
    echo "stopped transient unit; installed gemini-gemshell.service restarted"
}

case "${1:-run}" in
    run) run ;;
    shot) shot "${2:-}" ;;
    logs) logs ;;
    status) status ;;
    stop) stop ;;
    *) echo "usage: $0 run|logs|status|stop" >&2; exit 2 ;;
esac

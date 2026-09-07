#!/bin/sh
# deploy.sh — build/switch generations of the gemini-nixos flake the
# workstation way: HOST cross-builds the toplevel (fast, cached, the
# same pipeline that makes the images), ships the DELTA to the device's
# nix store over ssh, then switches the device's system profile +
# activates — no reflash, no TWRP. The device boots whatever
# /nix/var/nix/profiles/system points at (dual-boot initrd gen lookup),
# so a reboot lands on the new generation and old generations stay
# selectable for rollback.
#
# Rationale (2026-09-07): the flake is x86_64-cross; the fork packages
# (mesa/wlroots/gemwl/firmware) are NOT on any binary cache, so a fully
# NATIVE on-device `nixos-rebuild` would compile them on the PDA
# (30-90+ min, thermal risk). Host-cross + device-switch reuses the
# verified image pipeline and keeps per-iteration cost to a delta copy
# (the device store DB already shares the closure).
#
# GC hygiene (rule 0): every deployed toplevel is pinned with
# bin/gc-pin.sh before shipping — `nix-collect-garbage` on the HOST
# would otherwise sweep the cross-built closure (observed 2026-09-07:
# gen3 re-cross-compiled ~259 packages after a host GC).
#
# Usage (run from the repo root):
#   bash bin/deploy.sh status                 device generations + booted gen
#   bash bin/deploy.sh build                  build the toplevel (cross) + pin it
#   bash bin/deploy.sh deploy                 build (if needed) + ship + switch
#   bash bin/deploy.sh deploy PATH            ship + switch an existing toplevel
#   bash bin/deploy.sh rollback [N]           switch device profile N gens back
# Long ops: wrap in `bash bin/run-job.sh start <name> -- bash bin/deploy.sh ...`.
#
# Device access: bin/device-ssh.sh (auto net-up), key ~/.ssh/id_ed25519_gemini
# (host ~/.ssh/config entry '10.15.19.82' supplies it for plain nix copy).
set -eu

repo=/home/cjdell/Projects/gemini-nixos
dev=10.15.19.82
profile=/nix/var/nix/profiles/system

device_ssh() { bash "$repo/bin/device-ssh.sh" "$1"; }

# Build limits: cross-compiles oversubscribe badly at nix's defaults on
# this 16-core host (50+ cc1 processes; observed 2026-09-07).
build() {
    # stdout = the toplevel store path ONLY (callers capture it); the
    # progress line goes to stderr. Build limits: cross-compiles
    # oversubscribe badly at nix's defaults on this 16-core host (50+ cc1
    # processes; observed 2026-09-07).
    echo "deploy: building toplevel (cross)..." >&2
    nix build "$repo#packages.x86_64-linux.toplevel" \
        --print-out-paths --no-link --max-jobs 8 --cores 8 2>/dev/null
}

pin() {
    local tl=$1
    local stamp
    stamp=$(date +%Y%m%d-%H%M)
    bash "$repo/bin/gc-pin.sh" "toplevel-$stamp" "$tl"
}

ship_and_switch() {
    local tl=$1
    echo "deploy: shipping closure delta to $dev (nix copy)..."
    nix copy --to "ssh://$dev" "$tl"
    echo "deploy: switching device profile -> ${tl##*/}"
    device_ssh "nix-env -p $profile --set '$tl'"
    device_ssh "'$tl/bin/switch-to-configuration' switch" \
        | grep -vE '^$' || true
    echo "deploy: done — current device generation:"
    device_ssh "readlink $profile; readlink -f $profile"
}

status() {
    echo "device system generations:"
    device_ssh "nix-env -p $profile --list-generations 2>/dev/null | tail -8; echo; echo 'profile -> \$(readlink -f $profile 2>/dev/null || echo (none))'"
}

rollback() {
    local n=${1:-1}
    echo "deploy: switching device profile $n generation(s) back + activating..."
    device_ssh "nix-env -p $profile --rollback $n 2>/dev/null || nix-env -p $profile --rollback"
    local tl
    tl=$(device_ssh "readlink -f $profile")
    device_ssh "'$tl/bin/switch-to-configuration' switch" | grep -vE '^$' || true
    echo "deploy: now on ${tl##*/}"
}

cmd=${1:-status}
case "$cmd" in
status) status ;;
build)  tl=$(build); pin "$tl"; echo "built+pin: $tl" ;;
deploy)
    if [ $# -ge 2 ]; then
        tl=$2
        [ -e "$tl" ] || { echo "not a store path: $tl" >&2; exit 2; }
    else
        # reuse ./result if it is a toplevel of this flake, else build
        if [ -L "$repo/result" ] && [ -d "$repo/result/init" ]; then
            tl=$(readlink -f "$repo/result")
            echo "deploy: reusing built toplevel $tl (run 'deploy build' to rebuild)"
        else
            tl=$(build)
        fi
        pin "$tl"
    fi
    ship_and_switch "$tl"
    ;;
rollback) rollback "${2:-1}" ;;
*)
    echo "usage: deploy.sh status|build|deploy [PATH]|rollback [N]" >&2
    exit 2
    ;;
esac

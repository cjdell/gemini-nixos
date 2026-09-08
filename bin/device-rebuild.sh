#!/bin/sh
# device-rebuild.sh — build/switch generations of the gemini-nixos flake
# ON THE DEVICE (the deploy.sh device half with no host: no nix copy, no
# remote builder — the PDA builds/substitutes straight into its own
# store). Runs from the repo clone at /root/gemini-nixos on the PDA.
#
# Model [2026-09-08]: the repo is cloned to /root/gemini-nixos (sync
# with the host via bin/device-repo.sh seed|push|pull, or the github
# origin). Builds are NATIVE aarch64. The flake's pinned nixpkgs rev is
# the hydra-built nixos-unstable channel snapshot (golden rule 9), so
# its closure substitutes from cache.nixos.org over the device's wifi /
# g_ether NAT; only the custom drvs (mesa-geminipda fork, the kernel,
# wlroots/labwc/gemwl pins, gemini-firmware, gemcli, make_ext4fs-shim)
# and the config glue compile locally when changed — long on the A72/A53
# mix, prefer the host loop (bin/deploy.sh) for those. Store writes go
# through the socket-activated nix-daemon: /nix/store is bind-mounted ro
# in the main namespace (read-only-store design) while the daemon's
# private mount ns sees it rw; root clients auto-connect to the daemon
# (verified 2026-09-08). The config (config/gemini.nix) enables
# experimental-features, bounds max-jobs/cores, turns the sandbox off
# and points the daemon's build temp at /var/tmp (disk; /tmp is a small
# tmpfs) — all since gen29.
#
# Rule 0 (version hygiene): `build` refuses a dirty repo — a switched
# generation must equal a committed state, so the device commit hash is
# its identity. Commit on the device first, then build/switch.
#
# Usage (on the device, from anywhere):
#   bash /root/gemini-nixos/bin/device-rebuild.sh status
#   bash /root/gemini-nixos/bin/device-rebuild.sh build
#   bash /root/gemini-nixos/bin/device-rebuild.sh switch [STORE_PATH]
#   bash /root/gemini-nixos/bin/device-rebuild.sh rollback [N]
#   bash /root/gemini-nixos/bin/device-rebuild.sh channels  # (re)pin the nix-shell nixpkgs channel
#   bash /root/gemini-nixos/bin/device-rebuild.sh gc        # drop old gens + collect garbage
set -eu

repo=/root/gemini-nixos
profile=/nix/var/nix/profiles/system
# belt+braces: features come from /etc/nix/nix.conf since gen29, but the
# flag keeps this script working against a pre-gen29 config too. Single
# quoted token: --extra-experimental-features takes ONE argv value
# (space-separated list inside it) — unquoted, 'flakes' parses as a
# command (verified 2026-09-08).
FEATURES='--extra-experimental-features "nix-command flakes"'

build() {
    # stdout = the toplevel store path ONLY (callers capture it).
    echo "device-rebuild: building native toplevel from $repo (aarch64, cache.nixos.org substitutes; local compiles for the custom drvs)..." >&2
    cd "$repo"
    if [ -n "$(git status --porcelain)" ]; then
        echo "!! repo is dirty — commit first (rule 0: a switched gen must have an identity)" >&2
        exit 2
    fi
    echo "device-rebuild: $(git log -1 --format='%h %s')" >&2
    nix $FEATURES build .#packages.aarch64-linux.toplevel \
        --print-out-paths --no-link
}

switch_gen() {
    local tl=$1
    [ -e "$tl" ] || { echo "not a store path: $tl" >&2; exit 2; }
    echo "device-rebuild: switching system profile -> ${tl##*/}"
    nix-env -p "$profile" --set "$tl"
    "$tl/bin/switch-to-configuration" switch | grep -vE '^$' || true
    echo "device-rebuild: done — current device generation:"
    readlink -f "$profile"
}

status() {
    echo "device system generations:"
    nix-env -p "$profile" --list-generations 2>/dev/null | tail -8
    echo
    echo "profile -> $(readlink -f "$profile" 2>/dev/null || echo '(none)')"
}

rollback() {
    local n=${1:-1}
    echo "device-rebuild: switching system profile $n generation(s) back + activating..."
    nix-env -p "$profile" --rollback "$n" 2>/dev/null || nix-env -p "$profile" --rollback
    local tl
    tl=$(readlink -f "$profile")
    "$tl/bin/switch-to-configuration" switch | grep -vE '^$' || true
    echo "device-rebuild: now on ${tl##*/}"
}

channels() {
    # Pin the device's <nixpkgs> (nix-shell -p) to the SAME rev the
    # flake pins — read from the local flake.nix so this re-pins
    # automatically whenever the flake pin moves. Rule 9: the flake
    # nixpkgs pin is a hydra-built channel snapshot, so nix-shell
    # packages == the running system's nixpkgs (cache-healthy by
    # construction). The channel dir is a symlink to nix's own
    # fetchTarball store path (busybox tar exists but this is
    # GC-manageable and the SAME content-addressed path the flake's
    # fetchTree input unpacks to — no double download), GC-rooted so
    # the link never dangles. nix-channel --update itself is broken on
    # this NixOS 26.11 per-user channels layout (EINVAL reading the
    # dangling ~/.nix-defexpr/channels symlink; verified 2026-09-08).
    local rev url src dst
    rev=$(sed -n 's#.*NixOS/nixpkgs/archive/\([0-9a-f]\{40\}\).*#\1#p' "$repo/flake.nix" | head -1)
    [ -n "$rev" ] || { echo "!! could not read the nixpkgs rev from $repo/flake.nix" >&2; exit 1; }
    url="https://github.com/NixOS/nixpkgs/archive/$rev.tar.gz"
    echo "device-rebuild: pinning nixpkgs channel to $rev"
    src=$(nix-instantiate --eval --expr "builtins.fetchTarball \"$url\"" --raw)
    dst=/nix/var/nix/profiles/per-user/root/channels/nixpkgs
    mkdir -p "$(dirname "$dst")"
    ln -sfn "$src" "$dst"
    nix-store --add-root /nix/var/nix/gcroots/nixpkgs-channel -r "$src" >/dev/null 2>&1 || true
    echo "device-rebuild: <nixpkgs> -> $(nix-instantiate --find-file nixpkgs)"
}

gc() {
    echo "device-rebuild: deleting old system generations..."
    nix-env -p "$profile" --delete-generations old
    echo "device-rebuild: collecting garbage (daemon-mediated)..."
    nix-collect-garbage -d
    status
}

cmd=${1:-status}
case "$cmd" in
status) status ;;
build)  build ;;
switch)
    [ $# -ge 2 ] || { echo "usage: device-rebuild.sh switch STORE_PATH" >&2; exit 2; }
    switch_gen "$2"
    ;;
rollback) rollback "${2:-1}" ;;
channels) channels ;;
gc) gc ;;
*)
    echo "usage: device-rebuild.sh status|build|switch PATH|rollback [N]|channels|gc" >&2
    exit 2
    ;;
esac

#!/bin/sh
# gc-pin.sh — protect a built store path (and its whole closure) from
# `nix-collect-garbage`.
#
# WHY (2026-09-07): plain `nix build` outputs are only GC roots while the
# flake's ./result symlink points at them. The moment result moves to the
# next build (or a `nix-collect-garbage` runs after an unrelated build),
# every cross-compiled aarch64 closure path that was NOT re-referenced is
# swept — the next `nixos-rebuild`-style deploy then silently re-cross-
# compiles 200+ packages (~30-60 min). The milestone operator's earlier
# `nix-collect-garbage` did exactly this between gen2 and gen3.
#
# Usage:
#   bash bin/gc-pin.sh NAME STORE_PATH
#     registers /nix/var/nix/gcroots/per-user/$USER/gemini-nixos-NAME
#     as a GC root for STORE_PATH (the whole closure reachable from it is
#     then protected). NAME examples: toplevel-20260907-3, system-img-xxx
#   bash bin/gc-pin.sh list
#     shows the project's current roots (and whether each is alive)
#   bash bin/gc-pin.sh unpin NAME
#     removes the root (path becomes collectable again)
#
# The per-user gcroots dir is created once (sudo, since /nix/var/nix is
# root-owned on this host).
set -eu

user=$(id -u -n)
base="/nix/var/nix/gcroots/per-user/$user"

ensure_dir() {
    if [ ! -d "$base" ]; then
        echo "gc-pin: creating $base (sudo)" >&2
        sudo mkdir -p "$base"
        sudo chown "$user:$(id -g -n)" "$base"
    fi
}

case "${1:-list}" in
list)
    ensure_dir
    echo "project GC roots under $base:"
    for f in "$base"/gemini-nixos-*; do
        [ -e "$f" ] || continue
        target=$(readlink "$f")
        if [ -e "$target" ]; then
            alive=alive
        else
            alive="MISSING (collectable)"
        fi
        printf '  %-45s -> %s  [%s]\n' "$(basename "$f")" "${target##*/}" "$alive"
    done
    ;;
unpin)
    ensure_dir
    name=$2
    rm -f "$base/gemini-nixos-$name"
    echo "gc-pin: removed gemini-nixos-$name"
    ;;
*)
    name=$1
    path=$2
    [ -n "$name" ] && [ -n "$path" ] || { echo "usage: gc-pin.sh NAME STORE_PATH | list | unpin NAME" >&2; exit 2; }
    case "$name" in */*) echo "name must not contain /" >&2; exit 2;; esac
    ensure_dir
    nix-store --add-root "$base/gemini-nixos-$name" --realise "$path" >/dev/null
    echo "gc-pin: $name -> ${path##*/} (root: $base/gemini-nixos-$name)"
    ;;
esac

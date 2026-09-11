#!/bin/bash
# gemshell-host-check.sh — fast x86_64 `cargo check` loop for pkgs/gemshell
# (both the gemshell compositor and the gemsettings client).
#
# Why the contortions: the crates link libwayland-server/libwayland-client/
# libxkbcommon via pkg-config at build time (wayland-sys), and the .pc
# files live in the nixpkgs `-dev` outputs, which `nix shell` does not put
# on PKG_CONFIG_PATH. So: build the dev outputs first, point
# PKG_CONFIG_PATH at their lib/pkgconfig dirs, then cargo check inside a
# nix shell carrying the same libraries.
#
# Usage: bash bin/gemshell-host-check.sh [--release|--debug|--test]
#   default = `cargo check` (debug); --release = cargo check --release
#   --test = cargo test --no-run (compiles the test harness)

set -euo pipefail
cd "$(dirname "$0")/.."
PIN="dc5d91f840324650bac8c379428c7037a416959a" # nixpkgs rev pinned in flake.nix
MODE="${1:---debug}"

case "$MODE" in
  --release) CARGO_MODE=(--release) ;;
  --test) CARGO_MODE=(--test --no-run) ;;
  *) CARGO_MODE=() ;;
esac

# dev outputs (lib/pkgconfig) for the pkg-config build script
mapfile -t paths < <(nix build --no-link --print-out-paths \
  "github:NixOS/nixpkgs/${PIN}#wayland.dev" "github:NixOS/nixpkgs/${PIN}#libxkbcommon.dev" 2>/dev/null || true)
PCC=""
for p in "${paths[@]}"; do
  [ -d "$p/lib/pkgconfig" ] && PCC="$PCC:$p/lib/pkgconfig"
done
PCC="${PCC#:}"
if [ -z "$PCC" ]; then
  echo "no pkgconfig dirs found — cannot cargo check" >&2
  exit 1
fi
echo "PKG_CONFIG_PATH=$PCC"

nix shell "github:NixOS/nixpkgs/${PIN}#cargo" "github:NixOS/nixpkgs/${PIN}#rustc" "github:NixOS/nixpkgs/${PIN}#pkg-config" \
  "github:NixOS/nixpkgs/${PIN}#wayland" "github:NixOS/nixpkgs/${PIN}#libxkbcommon" "github:NixOS/nixpkgs/${PIN}#libglvnd" \
  --command bash -c "
    export PKG_CONFIG_PATH='${PCC}'
    cd pkgs/gemshell
    exec cargo check ${CARGO_MODE[*]}
  "

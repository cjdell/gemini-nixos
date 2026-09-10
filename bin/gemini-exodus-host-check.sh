#!/bin/bash
# gemini-exodus-host-check.sh — fast x86_64 cargo check/test loop for
# pkgs/gemini-exodus (the GEMINI: EXODUS Director's Cut demo + GPU stress
# test). The canonical build is the native aarch64 flake build on the
# remote builder; this is the LOCAL type/syntax gate for iterating on the
# Rust before paying for the aarch64 compile (and it runs the stress.rs
# unit tests).
#
# Same contortions as bin/gemdemo-host-check.sh: the crate links no GL at
# compile time, but cpal's alsa-sys build script needs pkg-config to find
# alsa.pc. The bare host PATH has neither (rule 7), so resolve the
# alsa-lib .dev output at the flake's pinned nixpkgs rev and point
# PKG_CONFIG_PATH at it by hand. Cargo/rustc come from ~/.cargo; the
# dependency cache is shared with pkgs/gemdemo via CARGO_TARGET_DIR, so
# after the first build this is seconds.
#
# Usage: bash bin/gemini-exodus-host-check.sh [--release|--debug|--test]
#   default = `cargo check --release`

set -euo pipefail
cd "$(dirname "$0")/.."

PIN="dc5d91f840324650bac8c379428c7037a416959a" # nixpkgs rev pinned in flake.nix
MODE="${1:---release}"

dev=$(nix build --no-link --print-out-paths \
  "github:NixOS/nixpkgs/$PIN#alsa-lib^dev" 2>/dev/null | tail -1)
export PKG_CONFIG_PATH="$dev/lib/pkgconfig"

# Shared target dir: the dependency crates are already compiled for the
# gemdemo template with the same rustc/lockfile, so reuse them.
export CARGO_TARGET_DIR="$PWD/pkgs/gemdemo/target"

cd pkgs/gemini-exodus
exec nix shell "github:NixOS/nixpkgs/$PIN#pkg-config" --command \
  cargo check "$MODE"

#!/bin/bash
# gemdemo-host-check.sh — fast x86_64 `cargo check` loop for pkgs/gemdemo
# (the canonical gemdemo build is the native aarch64 flake build on the
# remote builder; this is the LOCAL type/syntax gate for iterating on the
# Rust source before paying for the aarch64 compile).
#
# Why the contortions: the crate links no GL at compile time, but the
# alsa-sys build script (cpal's ALSA backend) needs `pkg-config` to find
# alsa.pc. The bare host PATH has neither (rule 7). nix shell does not
# run the pkg-config setup hook, so we resolve the alsa-lib `.dev`
# output's store path (same nixpkgs rev the flake pins) and point
# PKG_CONFIG_PATH at it by hand. Cargo + rustc come from the host
# (~/.cargo), and the crate's dependency cache lives in the normal
# CARGO_HOME — first run compiles winit & co (~minutes), afterwards this
# is seconds.
#
# Usage: bash bin/gemdemo-host-check.sh [--release|--debug|--test]
#   default = `cargo check --release` (matches the flake profile flags)

set -euo pipefail
cd "$(dirname "$0")/.."

PIN="dc5d91f840324650bac8c379428c7037a416959a" # nixpkgs rev pinned in flake.nix
MODE="${1:---release}"

dev=$(nix build --no-link --print-out-paths \
  "github:NixOS/nixpkgs/$PIN#alsa-lib^dev" 2>/dev/null | tail -1)
export PKG_CONFIG_PATH="$dev/lib/pkgconfig"

cd pkgs/gemdemo
exec nix shell "github:NixOS/nixpkgs/$PIN#pkg-config" --command \
  cargo check "$MODE"

#!/usr/bin/env bash
# Regenerate the Mesa source snapshot used by pkgs/mesa-geminipda.nix.
#
# Usage: bash bin/snapshot-mesa.sh [version]
#
#   version  default: 25.0.7
#
# Downloads the canonical `/-/archive/` tarball of the upstream tag
# (NOT the /api/v4/.../archive.tar.gz endpoint — that one is
# byte-unstable: it is generated on the fly and the tar/gzip bytes
# differ between fetches, which breaks fixed-output hashes).
#
# After regenerating, verify the fork patch still applies cleanly and
# (optionally) that the result matches the verified fork tree:
#
#   tmp=$(mktemp -d)
#   tar -xzf mesa/mesa-<ver>.tar.gz -C "$tmp"
#   cd "$tmp"/mesa-mesa-<ver>
#   patch -p1 --dry-run < ../patches/mesa-panfrost-geminipda-<ver>.patch
#
# and update the sha256 noted in pkgs/mesa-geminipda.nix if it changed.
set -euo pipefail

VERSION="${1:-25.0.7}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$REPO_ROOT/mesa/mesa-${VERSION}.tar.gz"

URL="https://gitlab.freedesktop.org/mesa/mesa/-/archive/mesa-${VERSION}/mesa-mesa-${VERSION}.tar.gz"

echo "==> Downloading $URL"
curl -fSL --retry 3 -o "$OUT" "$URL"
echo "==> Wrote $OUT ($(du -h "$OUT" | cut -f1))"
echo "    sha256: $(sha256sum "$OUT" | cut -d' ' -f1)"

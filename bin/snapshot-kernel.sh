#!/usr/bin/env bash
# Regenerate the kernel source snapshot used by the flake.
#
# Usage: bash bin/snapshot-kernel.sh [git-dir] [rev]
#
#   git-dir  default: /home/cjdell/Projects/GeminiPDA/repos/linux-6.6
#   rev      default: the rev currently pinned in
#             devices/planet-geminipda/kernel/default.nix
#
# The snapshot is `git archive` of the pinned commit: clean tree, no
# .git, no build artifacts (the working tree of the bring-up clone
# carries in-tree build outputs and must not be fed to the kernel
# builder directly).
#
# If the rev changes, also rename the tarball reference in
# devices/planet-geminipda/kernel/default.nix.
set -euo pipefail

GIT_DIR="${1:-/home/cjdell/Projects/GeminiPDA/repos/linux-6.6}"
REV="${2:-733c0c7ea74195bd30734f599f37e69febfd38e0}"
SHORT="$(git -C "$GIT_DIR" rev-parse --short=11 "$REV")"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$REPO_ROOT/kernel/geminipda-bringup-$SHORT.tar.gz"

echo "==> Archiving $GIT_DIR @ $REV"
git -C "$GIT_DIR" archive --format=tar "$REV" | gzip -9 > "$OUT"
echo "==> Wrote $OUT ($(du -h "$OUT" | cut -f1))"
echo "    sha256: $(sha256sum "$OUT" | cut -d' ' -f1)"

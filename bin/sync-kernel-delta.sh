#!/usr/bin/env bash
# Sync the tracked bring-up kernel delta (devices/planet-geminipda/kernel/delta/)
# with a fork checkout of the geminipda-bringup line.
#
# Usage: bash bin/sync-kernel-delta.sh [git-dir] [rev]
#
#   git-dir  default: /home/cjdell/Projects/GeminiPDA/repos/linux-6.6
#            (legacy source of the local-only bring-up branch; the delta
#            tree HERE is the source of truth for builds — this script is
#            only for folding new fork commits in)
#   rev      default: 188aade698dd286f36a321ffe2ac6ae08762ee97
#            (geminipda-bringup HEAD = the on-glass kernel #329 tree)
#
# Source model (docs/library-deltas.md, kernel entry): the kernel builds
# from the published Linux v6.6 base + this delta as a plain file tree
# (no patch files — agents edit source directly). The delta must be
# exactly "the files the bring-up line changes over v6.6":
#
#   - materialized from `git diff --name-status v6.6..rev` (only A/M
#     entries; the bring-up line adds 457 files and modifies 55, and has
#     never deleted/renamed a file — the script fails loudly if that
#     ever changes, because a pure copy-replace overlay cannot express
#     deletions);
#   - then VERIFIED: v6.6-base + delta must be byte-identical to
#     `git archive rev` (exit 1 otherwise).
#
# After a successful sync, commit the delta tree and update the rev in
# the header comment of devices/planet-geminipda/kernel/default.nix.
set -euo pipefail

GIT_DIR="${1:-/home/cjdell/Projects/GeminiPDA/repos/linux-6.6}"
REV="${2:-188aade698dd286f36a321ffe2ac6ae08762ee97}"
BASE_REV="$(git -C "$GIT_DIR" rev-parse v6.6^{commit})"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DELTA="$REPO_ROOT/devices/planet-geminipda/kernel/delta"

[ -d "$DELTA" ] || { echo "FAIL: $DELTA missing" >&2; exit 1; }

echo "==> Sync delta from $GIT_DIR"
echo "    base (v6.6): $BASE_REV"
echo "    rev        : $(git -C "$GIT_DIR" rev-parse "$REV")"

# 1. Build the file list; reject deletions/renames (copy-replace overlay
#    cannot express them).
LIST="$(git -C "$GIT_DIR" diff --name-status v6.6 "$REV")"
if echo "$LIST" | grep -qE "^(D|R|C|T|U|X)"; then
    echo "FAIL: delta contains non add/modify entries — copy-replace overlay" >&2
    echo "      cannot express them; handle manually:" >&2
    echo "$LIST" | grep -E "^(D|R|C|T|U|X)" >&2
    exit 1
fi
FILES="$(echo "$LIST" | awk '{print $2}')"
echo "    files: $(echo "$FILES" | wc -l) (add/modify)"

# 2. Materialize into the delta tree (clear first — a file that stopped
#    changing would otherwise linger as a stale copy).
rm -rf "$DELTA"
mkdir -p "$DELTA"
git -C "$GIT_DIR" archive "$REV" $FILES | tar -x -C "$DELTA"
echo "    delta tree: $(find "$DELTA" -type f | wc -l) files, $(du -sh "$DELTA" | cut -f1)"

# 3. Verify base + delta == rev (byte-for-byte).
A="$(mktemp -d)"
B="$(mktemp -d)"
trap 'rm -rf "$A" "$B"' EXIT
git -C "$GIT_DIR" archive v6.6 | tar -x -C "$A"
cp -a "$DELTA"/. "$A"/
git -C "$GIT_DIR" archive "$REV" | tar -x -C "$B"
if ! diff -rq "$A" "$B" >/dev/null; then
    echo "FAIL: base + delta != rev (byte compare). Delta not staged." >&2
    diff -rq "$A" "$B" | head -20 >&2
    exit 1
fi
echo "    VERIFIED: v6.6-base + delta == rev (byte-identical)"

# 4. Stage the delta for commit.
git -C "$REPO_ROOT" add "devices/planet-geminipda/kernel/delta"
echo "==> Delta staged. Commit it and update the rev reference in"
echo "    devices/planet-geminipda/kernel/default.nix header."

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
#   rev      default: 06fd13e112b23c5b2b4a9310af633cd27c14c57f
#            (geminipda-bringup rev the delta was last synced from; note
#            the fork rev default USED to be 188aade69 — stale — see the
#            local-divergence note below)
#
# Source model (docs/library-deltas.md, kernel entry): the kernel builds
# from the published Linux v6.6 base + this delta as a plain file tree
# (no patch files — agents edit source directly).
#
# LOCAL-DIVERGENCE GUARD (2026-09-10k). The delta is the build source of
# truth and now carries work that is NOT in the fork rev: the DRM/KMS
# bring-up added `drivers/gpu/drm/tiny/geminipda-drm.c` (+ its tiny/Kconfig
# and tiny/Makefile) directly to the delta, and modified the
# mt6797-gemini-pda.dts (geminipda-drm node). A blind `rm -rf delta` sync
# would silently delete the DRM driver and remove the DTS node, breaking
# GNOME. So this script now REFUSES to clobber a delta that diverges from
# the rev being synced; set FORCE=1 to overwrite anyway (and first fold the
# local work into the fork / a new rev if you want it kept).
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
REV="${2:-06fd13e112b23c5b2b4a9310af633cd27c14c57f}"
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

# 1b. Local-divergence guard (see header). The delta may carry delta-only
#     files or locally-modified fork files; the materialize step below is
#     destructive, so abort instead of silently losing them.
PRE="$(mktemp -d)"
trap 'rm -rf "$PRE"' EXIT
git -C "$GIT_DIR" archive "$REV" $FILES | tar -x -C "$PRE"
declare -A FORKSET=()
while IFS= read -r f; do [ -n "$f" ] && FORKSET["$f"]=1; done <<< "$FILES"
DIVERGED=()
for f in $FILES; do
    if [ ! -f "$DELTA/$f" ]; then
        DIVERGED+=("missing:    $f")
    elif ! cmp -s "$DELTA/$f" "$PRE/$f"; then
        DIVERGED+=("modified:   $f")
    fi
done
while IFS= read -r f; do
    [ -n "${FORKSET[$f]:-}" ] || DIVERGED+=("delta-only: $f")
done < <(cd "$DELTA" && find . -type f | sed 's#^\./##' | sort)
if [ ${#DIVERGED[@]} -gt 0 ]; then
    if [ "${FORCE:-0}" = 1 ]; then
        echo "!! FORCE=1: overwriting ${#DIVERGED[@]} locally-diverged file(s):" >&2
        printf '     %s\n' "${DIVERGED[@]}" >&2
    else
        echo "FAIL: the delta diverges from $REV — refusing to clobber it." >&2
        printf '      %s\n' "${DIVERGED[@]}" >&2
        echo "      Fold these into the fork / a new rev, or set FORCE=1 to overwrite." >&2
        exit 1
    fi
fi
rm -rf "$PRE"; trap - EXIT

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

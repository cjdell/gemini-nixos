#!/usr/bin/env bash
# run-mtk.sh — launcher for the PATCHED mtkclient (preloader/BROM mode)
#
# The nixpkgs mtkclient never sees this device's preloader (it scans USB
# with devclass=10; our preloader is CDC-ACM devclass=2). The patched
# copy lives at /usr/local/lib/mtkclient-patched (see the legacy
# GeminiPDA docs/flashing.md mtkclient section for the patch list;
# port to docs/disaster-recovery/ is M1). This launcher shadows the
# store package with the patched copy, exactly like the legacy
# build/run-mtk.sh it was ported from (2026-09-07, golden-repo pivot).
#
# Prerequisite: the devshell's `mtkclient` package must exist in the nix
# store (it provides the python deps + Loader DAs). Build it once:
#   nix develop --command true
#
# Usage (from the repo root; needs sudo for USB):
#   sudo bash bin/run-mtk.sh printgpt          # read-only: list GPT
#   sudo bash bin/run-mtk.sh r boot out.img    # read-only: readback
#   sudo bash bin/run-mtk.sh w boot stock-dump/boot.bin
#   sudo bash bin/run-mtk.sh reset             # DA-session reset (-> POC)
#   sudo bash bin/run-mtk.sh --debugmode printgpt
#
# It prints retry dots while waiting for the device; power the device on
# (or reboot it) while it waits — the MT6797 preloader enumerates
# 0e8d:2000 for ~9 s on every power-on with USB attached. DR playbook:
# docs/disaster-recovery/.

set -euo pipefail

PATCHED=/usr/local/lib/mtkclient-patched
STORE=/nix/store
# Version-agnostic match: prefer 2.1.4.1 (the version the patches were
# made against), fall back to any mtkclient in the store.
PKG=$(ls -d $STORE/*-mtkclient-2.1.4.1 2>/dev/null | head -1)
[ -z "${PKG:-}" ] && PKG=$(ls -d $STORE/*-mtkclient-* 2>/dev/null | grep -v '\.drv$' | head -1)
LIBUSB=$(ls -d $STORE/*-libusb-1.0.2*/lib 2>/dev/null | tail -1)

if [ -z "${PKG:-}" ]; then
  echo "run-mtk.sh: no mtkclient package in the nix store — build the devshell first:" >&2
  echo "  nix develop --command true" >&2
  exit 1
fi
if [ ! -d "$PATCHED" ]; then
  echo "run-mtk.sh: patched mtkclient copy missing at $PATCHED" >&2
  echo "(the stock client never sees this CDC-ACM preloader — see legacy flashing.md)" >&2
  exit 1
fi

# PYTHONPATH = patched copy first (shadows the store package) + the store
# package's python deps (pyusb, pycryptodomex, pyserial, ...). The
# propagated-build-inputs file lists package ROOTS — the actual importable
# dirs are under /lib/python3.13/site-packages inside each.
DEPS=""
for d in $(cat "$PKG/nix-support/propagated-build-inputs"); do
  if [ -d "$d/lib/python3.13/site-packages" ]; then
    DEPS="$DEPS:$d/lib/python3.13/site-packages"
  fi
done
PP="$PATCHED$DEPS"

export PYTHONPATH="$PP"
export LD_LIBRARY_PATH="${LIBUSB:-}:${LD_LIBRARY_PATH:-}"

# Prefer a plain python3 (not the *-env wrapper, which fails with
# "Exec format error" outside its environment).
PY=$(ls -d $STORE/*-python3-3.13*/bin/python3 2>/dev/null | grep -v '\-env' | tail -1)
[ -z "${PY:-}" ] && PY=python3

exec "$PY" -c "import sys; from mtkclient.mtk import main; sys.exit(main())" "$@"

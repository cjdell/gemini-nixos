# kernel/ — kernel source: published base + tracked delta

The kernel builds from the published upstream **Linux v6.6** base
(fetch-pinned kernel.org tarball in `devices/planet-geminipda/kernel/
default.nix`) + the tracked bring-up delta
(`devices/planet-geminipda/kernel/delta/`). Nothing large is vendored
here. See `devices/planet-geminipda/kernel/default.nix` and the
session-log entry 2026-09-08.

- `kernel/base` = a git submodule pointer to upstream torvalds/linux @
  tag v6.6 (`ffc253263a1375a65fa6c9f62a893e9767fbebfa`) — a read-only
  pointer for reading the base source locally; fetch on demand:

      git submodule update --init --depth 1 kernel/base

  The nix build does NOT read the submodule (flake exports only carry
  tracked files; the base comes from the hash-pinned kernel.org
  tarball — the same content, verified byte-identical 2026-09-08).

- Refresh the delta after fork commits (legacy
  `GeminiPDA/repos/linux-6.6`):

      bash bin/sync-kernel-delta.sh            # materialize + byte-verify

- The old 225 MB snapshot tarball + `bin/snapshot-kernel.sh` were
  retired 2026-09-08 (git history has them).

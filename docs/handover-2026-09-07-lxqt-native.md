# Handover — LXQt native-aarch64 build (2026-09-07 night → check up 2026-09-08)

> **STATUS: COMPLETED 2026-09-08.** The native toplevel build finished
> rc=0 (b8hyqdvz…, 47790 s); it is pinned (per-user
> `gemini-nixos-toplevel-20260907-native` + root-level
> `/nix/var/nix/gcroots/gemini-lxqt-native-20260907`, both verified).
> `native-aarch64` was merged into `main` (commit 88a2699) and the
> worktree removed — the native build model is canonical (flake
> `buildSystem = aarch64-linux`; `bin/deploy.sh build` runs the proven
> root `--store local` distributed build). The LXQt desktop was then
> deployed and tested on glass (gens 6→8): two repo bugs fixed on the
> way (config-seed basenames, wlroots `-Dallocators=gbm`) and the
> session verified after a cold reboot + eyes-on-glass — see
> `docs/session-log.md` 2026-09-08. The 3a nixpkgs-repin question
> remains open (deferred: would re-hash the verified closure).

> The LXQt desktop (labwc-nested, `lxqt-nested.service`) is IN-TREE on
> `main` (commit 7a5bf23) — that part is done repo-side. The open thread
> is the **native-aarch64 closure build** (running, many hours left) and
> the decisions after it lands. Nothing has been flashed; the device is
> untouched on gen5 (`c10qkjdw`, para cleared). Read this first, then
> `docs/session-log.md` 2026-09-07 (entry) + the README desktop section.

## TL;DR

Two branches, two build models:

| Tree | Branch | Commit | Build model | Status |
|---|---|---|---|---|
| `/home/cjdell/Projects/gemini-nixos` | `main` | `7a5bf23` | cross x86_64→aarch64 | **cross toplevel ABANDONED** (nixpkgs "cross is very broken"; blocker chain below) — device-flash path stays cross for now |
| `/home/cjdell/Projects/gemini-nixos-native` | `native-aarch64` | `a689b96` | **native aarch64** (`buildSystem = "aarch64-linux"`) | **toplevel build RUNNING** via 192.168.49.191 |

## The running native build — check this first

- Job: `bash bin/run-job.sh wait build-native-host` **from the native
  worktree** (`/home/cjdell/Projects/gemini-nixos-native`) — job state
  lives in that tree's `logs/jobs/`. rc=0 done / rc=1 failed / rc=2 running.
- Log: `/home/cjdell/Projects/gemini-nixos-native/logs/jobs/build-native-host/log`
  (was ~4500+ lines, deep in Qt modules — qt3d seen at handover time).
- Mechanism (important): run as ROOT against the LOCAL store, distributed
  to the Pi — `sudo nix build --store local .#packages.aarch64-linux.toplevel
  --option builders @/etc/nix/machines --fallback`. Host substitutes from
  cache.nixos.org (stable link), Pi (192.168.49.191, 8 cores) compiles,
  host pulls each finished path back over ssh. This was chosen because
  the Pi's own link to cache.nixos.org drops large NARs (HTTP 206 mid-
  transfer) — do NOT go back to building directly on the Pi for the
  cache fetches.
- Pi side: repo copy at `cjdell@192.168.49.191:/home/cjdell/Projects/
  gemini-nixos-native` (note the `Projects/` segment — matches the host
  layout; there is no copy at `~/gemini-nixos-native` anymore).
  Pi load was ~8 (all cores on cc1plus); its store is accumulating the
  native closure (49 G free at last check).
- Native toplevel store path (deterministic): `5br2r178…-nixos-system-
  gemini-26.11pre…` — same hash host-side or on the Pi.

## When it finishes (rc=0)

1. **PIN IT (user requirement — "super extra certain, never GC'd")**:
   - `bash bin/gc-pin.sh toplevel-20260907-native <out>` (per-user root,
     in the native worktree, uses its `bin/`).
   - PLUS a root-level root owned by root (honored unconditionally):
     `sudo nix-store --add-root /nix/var/nix/gcroots/gemini-lxqt-native-
     20260907 -r <out>`
   - Verify BOTH: `nix-store -q --roots <out>` lists them; `bash
     bin/gc-pin.sh list`.
   - Note: the host root-level build already wrote everything into the
     host store (shared), so the paths are on THIS host; the Pi store
     holds its own copies.
2. Record the version line (kernel #329 borrowed, mesa fork 25.0.7, etc.)
   in `docs/session-log.md`.
3. **Decide the next move** (ask the user):
   a. **Repin nixpkgs** to a Hydra-built unstable rev so future native
      builds are mostly cache downloads (see "why are we compiling Qt"
      below). This is a "pins moved" event: re-hashes the whole closure,
      needs docs + date + graphics re-verify (R2 note). The native-
      aarch64 model is the one that BENEFITS from this (aarch64-native
      drvs hash-match hydra's aarch64 builds; cross never would).
   b. **Deploy the native closure to the device** (device is aarch64;
      `nix copy --to ssh://10.15.19.82` + profile switch — deploy.sh is
      cross-shaped, so this is a manual first). Or keep the device on
      the cross-built gen5 until the native image path is proven.
   c. Whether to reconcile branches (native becomes main eventually?).

## Why we're compiling Qt instead of downloading (asked + answered)

cache.nixos.org has 1315 paths for this build — but the Qt6/LXQt
*compiled outputs* 404: our nixpkgs pin (`26.11pre1031299.0bb7ec54c848`)
was never built/published by Hydra for ANY platform (verified 404 on the
exact qtbase narinfo for both x86_64-native and aarch64-native). Store
paths are keyed by the full derivation incl. the nixpkgs source hash, so
no cache can serve this rev's outputs. → repin (3a) is the fix.

## Cross build — abandoned for now (why, and where to resume)

`main`'s toplevel cross-build fought nixpkgs's own admission that "cross
is currently very broken" (pkgs/kde/lib/mk-kde-derivation.nix). Walls
hit, in order (each fixed or bypassed, last one NOT):

1. openblas 0.3.33 aarch64 DYNAMIC_ARCH → ARMV9SME missing source file —
   **fixed in main config/gemini.nix overlay** (committed): single ARMV8
   target when `hostPlatform.isAarch64`.
2. libfm/libfm-extra/menu-cache autoreconf (AM_GLIB_GNU_GETTEXT) —
   **fixed in main overlay** (committed): native glib.dev/gettext/
   intltool on the aclocal path.
3. shiboken6/pyside6 (KF6 python bindings; only kguiaddons' python
   output needs them) — fixed with a kguiaddons `hasPythonBindings =
   false` overrideScope overlay, **but that overlay was NOT committed and
   was lost when config/gemini.nix was `git checkout`-ed** during the
   shutdown — re-derive it (it evaluated clean: toplevel drv reached)
   if cross resumes.
4. Qt6CoreTools missing for the whole lxqt scope (cross qtbase doesn't
   install it; every lxqt package's cmake does
   `find_package(Qt6CoreTools REQUIRED)`) — **UNRESOLVED**. Prototype
   tried: put the NATIVE qtbase's `lib/cmake` on CMAKE_PREFIX_PATH for
   lxqt packages (whole-scope override recursed — infinite recursion in
   the makeScope; needs per-package or narrower approach).

Cross resume = fix #4 (scope prefix) + re-add #3 + likely more walls.
Decision was to let the native build carry the LXQt-on-glass question.

## Device state (unchanged all session)

gen5 (`c10qkjdw…`) NixOS on p32, para cleared, Debian p29 intact. Nothing
flashed. Host disk was cleaned during the session (~14 G freed on `/`).
Host `/etc/nix/machines` has the Pi entry; the daemon still has NO
`builders =` line in nix.conf (adding it needs a daemon restart — was
deferred to not kill builds; the root `--store local` invocations don't
need it).

## Tomorrow's check-up commands

```sh
# 1. Is the native build done?
cd /home/cjdell/Projects/gemini-nixos-native
bash bin/run-job.sh wait build-native-host     # rc=0 done / 1 failed / 2 running
tail -5 logs/jobs/build-native-host/log
# 2. If running, sanity on the Pi
ssh cjdell@192.168.49.191 'bash -c "uptime"'
# 3. If done: PIN (steps above), record version line, then decide repin/deploy (3a–3c)
```

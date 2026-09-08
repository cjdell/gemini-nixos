# AGENTS.md — gemini-nixos: the GOLDEN repo for the Gemini PDA (Mobile NixOS + bring-up knowledge)

**Repo status: GOLDEN (declared 2026-09-07).** This repo is the primary,
self-contained home for the Gemini PDA (MT6797X) project going forward —
both the Mobile NixOS port and the hardware/boot knowledge base that
used to live in the sibling project `/home/cjdell/Projects/GeminiPDA`.
The intent is that gemini-nixos **completely replaces GeminiPDA**.
GeminiPDA is NOT reverted, deleted or edited-for-new-work; it is the
legacy source that is being folded into this repo (knowledge, receipts,
tooling, artifacts), after which it can be archived.

**Transitional rule (until the migration below completes):** a topic's
source of truth is wherever its latest content is. New knowledge is
written HERE first, with dates and receipts. Where this repo does not
yet carry a GeminiPDA doc's content, GeminiPDA remains the reference
for THAT topic — cite it with its path and port it here per the
migration plan so the pointer decays. Do not put new work in GeminiPDA;
mirroring a correction there is optional (legacy).

Read this file first, then `README.md`,
`docs/mobile-nixos-port-feasibility.md` and `docs/disaster-recovery/`.

## Migration status (GeminiPDA → gemini-nixos)

| # | Item | Status (2026-09-07) | Destination here |
|---|---|---|---|
| M1 | Hardware/boot receipt docs (boot-chain.md, hardware.md, flashing.md, wifi-consys.md, roadmap.md) | ⬜ planned — `docs/boot-process.md` + repartition doc are the in-repo seeds; carry `file:line` receipts + dates when porting | `docs/` |
| M2 | **Disaster-recovery knowledge + image ledger** | ✅ done 2026-09-07 | `docs/disaster-recovery/` (README · inventory · gather · drills) |
| M3 | stock-dump blobs (partition dumps, firmware zip, boot backups) | 🟡 partial — boot-critical set copied 2026-09-07; full bulk pending (see `docs/disaster-recovery/inventory.md` "pending copy") | `stock-dump/` (gitignored) |
| M4 | Recovery tooling (patched-mtkclient launcher, USB watcher, devshell pkg) | ✅ done 2026-09-07 | `bin/run-mtk.sh`, `bin/usb-watch.sh`; devshell `mtkclient` |
| M5 | Kernel + LK source trees (linux-6.6, gemini-lk, mesa fork work) | ✅ kernel done 2026-09-08 (published v6.6 base + tracked delta in `devices/planet-geminipda/kernel/`; source no longer lives in GeminiPDA); gemini-lk + mesa fork still legacy until vendored | `devices/planet-geminipda/kernel/`, `kernel/base` (submodule ptr) |
| M6 | Device services/rootfs files | ✅ (already ported as derivations/scripts — the port repo's original job) | `services/`, `pkgs/`, `bin/` |
| M7 | Session history | ⬜ new sessions log HERE only; old history stays in GeminiPDA | `docs/session-log.md` |

## Status (device, unchanged by the pivot)

Phases 0/1/3 done at the **build level** — the flake's `boot.img` +
`rootfs.img` build green and match the bring-up boot contract — and
**phase 2 is now on glass (2026-09-07 milestone: NixOS boots, sshd over
g_ether from p32)**; the rootfs `growfs`, several services and the
`nixos-rebuild` round-trip are the remaining on-glass work (TODO:
`docs/phase-2-on-glass.md`). Rootfs target (2026-09-07): NixOS → p32
`userdata` with a dual-boot boot.img, Debian stays on p29 (see
`docs/repartition-android-space.md` §10 decisions).

⚠️ **Boot.img cmdline field: KEEP `bootopt=64S3,32N2,64N2` in it** — LK
consumes it via `platform_parse_bootopt`; without it the boot hangs on
the LK logo (~15 s WDT loop) before any kernel output (discovered
2026-09-07; `config/gemini.nix` kernelParams + `docs/phase-2-on-glass.md`
§2a).

## Golden rules

0. **Version & commit hygiene.** Every artifact that goes to the device
   carries an identity, and every session records what it flashed:
   - the kernel is **built in-repo** (since 2026-09-08: published
     Linux v6.6 base + tracked delta + lean config — see the session
     log; the #329 borrow was retired and `kernel/borrowed/` is only
     an unflashed reference until glass verification);
   - Mesa 25.0.7 fork, wlroots 0.18.2, gemwl: exact pins in
     `flake.nix`/the pkgs derivations.
   Log one version line per flash in `docs/session-log.md` (kernel,
   boot.img hash, mesa/wlroots pins). Never flash a build you cannot
   identify. [added 2026-09-07, adapted from GeminiPDA rule 0]
1. **Record before you forget.** Any fact learned from source, a build,
   an eval error, or (eventually) a boot attempt belongs in a doc — with
   a date. This repo's log is `docs/session-log.md`; port/build facts go
   to `README.md` / `docs/mobile-nixos-port-feasibility.md`;
   *hardware/boot* facts go HERE (this repo is golden) — into
   `docs/boot-process.md` and the receipt docs being ported (M1); when
   you must cite a receipt not yet ported, quote it with its original
   `file:line` + date + source path so it survives the port.
2. **Never silently contradict a doc.** Fix it *and* note the correction
   (`[corrected YYYY-MM-DD]`). If unsure, add rather than replace, and
   mark it `unverified`. Claims about *upstream* state ("is in
   mainline") must carry the date they were verified.
3. **Prefer evidence over opinion.** Keep receipts: `file:line` for LK
   behaviour (carry the source path + commit when citing legacy docs),
   derivation/commit ids for nix behaviour (ours — nixpkgs floats, so
   pin + date every claim about it).
4. **Small, dated deltas beat big rewrites.** Append to the tables and
   logs; don't restructure docs casually.
5. **NEVER abuse the LCD panel/matrix (CORE RULE).** The unit's NT36672
   TDDI is initialized ONLY by LK; a badly-initialized panel shows the
   "uninitialised" flicker and can BURN the LCD matrix (permanent damage
   — observed 2026-09-04). For THIS repo:
   - The kernel must stay the **fbcon/exclude-display build** — the
     borrowed #329 payload is exactly that. When the in-repo kernel
     derivation (`devices/planet-geminipda/kernel/`) is finally used,
     build with the fbcon verb (EXCLUDE_DISPLAY=1 FBCON=1) and never
     merge the display-landmine drivers (mediatek-drm*, mtk-mmsys,
     phy-mtk-*, tps65132-regulator) into the image.
   - If the glass EVER shows the flicker (or a kernel/display change
     touches the stack), stop, get the device to TWRP (LK/TWRP drive the
     panel correctly) and fix the image before continuing.
   - The fb buffer is NOT the panel state: a valid framebuffer can sit
     behind a badly-initialized LCD. Judge the glass by eyes/TWRP, not
     by a capture. (Full story: legacy GeminiPDA session-log 2026-09-04;
     port pending M7.)
6. **Scripts over long ad-hoc commands (CORE RULE).** Any multi-step
   host/device operation needed more than once becomes a committed
   script in `bin/` the FIRST time (headers: what/why/usage; reuse
   `bin/device-ssh.sh` / the devshell helpers; fail loudly on missing
   tools). Long inline `ssh '...'` chains and host for-loops are
   throwaway EXPLORATION only — promote them before the session ends.
7. **Devshell-only CLIs — NEVER bare (CORE RULE).** The bare host PATH
   carries only git/nix/coreutils/ssh/usbutils-ish basics — verified
   NO python3/adb/make there (2026-09-07). Every project CLI (python3,
   adb, fastboot, mtkclient, …) lives ONLY in the flake devshell
   (`flake.nix` → `devShells.x86_64-linux.default`: python3, git,
   android-tools, usbutils, mtkclient). Reach them with
   `nix develop --command <tool> …` **from the repo root** (nix does NOT
   search upward for flake.nix). The device scripts self-re-exec inside
   the devshell when adb/lsusb are missing from the host PATH
   (`GEMINI_DEVSH_REEXEC` guards the loop). Never a bare `python3`/`adb`
   and never hard-code /nix/store paths (drift after GC).
8. **Long operations: `bash bin/run-job.sh start|wait`, NEVER inline
   nohup/pgrep loops (CORE RULE).** Anything >~30 s or that must keep
   working across tool calls: `bash bin/run-job.sh start NAME -- CMD`
   (returns immediately; setsid-detached) then
   `bash bin/run-job.sh wait NAME` (rc 0 ok / 1 failed / 2 still
   running → re-run wait). The old `nohup … & for … pgrep -f …` pattern
   stalls sessions: `pgrep -f` matches the polling shell's own cmdline →
   the loop spins until the tool timeout. Job state: `logs/jobs/<name>/`
   (gitignored); `wait-file LOG END_REGEX` salvages already-running ops.
   Long flash ops (rootfs push+dd) MUST run under run-job.
8b. **Poll, never long-sleep (agent rule).** While a job/build runs,
   poll with `bash bin/run-job.sh wait NAME` (it returns immediately with
   rc=2 while running) — do NOT `sleep 60`/`sleep 90` between checks
   (wasted wall-clock + stalled tool calls). If a tool needs a short
   settle between polls, cap sleeps at ~5–10 s. The `run-job.sh wait`
   rc protocol is the poll primitive: rc=0 done-ok, rc=1 done-failed,
   rc=2 still-running → re-run wait.
9. **Cache-healthy nixpkgs pins (CORE RULE, added 2026-09-08).** Every
   nixpkgs pin must be one hydra built the FULL closure for — always the
   nixos-unstable (or a release) CHANNEL snapshot, never a raw
   master/random commit: channel revs are only cut after hydra's
   complete build, so the big closures (Qt6/LXQt — the multi-hour
   compiles) substitute from cache.nixos.org. Raw commits get per-commit
   trunk-combined (base) coverage only — verified 2026-09-08: the old
   npins rev `0bb7ec54c848` qtbase 404'd for x86_64 AND aarch64 (Qt6/
   LXQt compiled every time); the channel rev `dc5d91f84032`
   (26.11pre1068949) has qtbase/qtwayland/lxqt-*/systemd/pipewire
   narinfos all 200. Pin the rev behind
   `https://channels.nixos.org/nixos-unstable/git-revision` (or the
   release-branch equivalent) + narHash from `nix flake prefetch
   github:NixOS/nixpkgs/<rev>`. VERIFY before committing with
   `nix path-info --store https://cache.nixos.org <outPath>` on the
   heavy packages eval'd at the candidate rev (qtbase first) — curl
   narinfo 404s are NOT evidence (proxy); use nix's own HTTP client.
   Also: don't let workaround overlays diverge from the channel
   defaults — each override forces non-cached drv hashes down its
   subtree (the systemd/ffmpeg/openblas/libfm overrides pruned with the
   2026-09-08 repin cost 137+ builds; receipts in config/gemini.nix +
   docs/session-log.md).

## Where things live

| Concern | File |
|---|---|
| Repo purpose, layout, boot chain, flash flow, phase status | `README.md` |
| Feasibility study + the phased plan (phase table = roadmap) | `docs/mobile-nixos-port-feasibility.md` |
| Plain-language boot explainer (receipt pointers) | `docs/boot-process.md` |
| **Phase-2 on-glass knowledge + TODO** (2026-09-07 milestone: bootopt discovery, recovery receipts, not-quite-working list) | `docs/phase-2-on-glass.md` |
| **NixOS-on-p32 + dual-boot design** (decided + implemented repo-side 2026-09-07; §10 choices, §9 change list) | `docs/repartition-android-space.md` |
| "Published base + in-repo delta" pattern (mesa done; kernel next) | `docs/library-deltas.md` |
| **What was actually tried / happened** (dated entries; golden log) | `docs/session-log.md` |
| **Disaster recovery** — full-flash-erase → TWRP playbook (levels 0–2), image ledger + sha256, gather checklist, drills | `docs/disaster-recovery/` (README · inventory · gather · drills) |
| Flake entry; Mobile NixOS pin (`2c132754`); **nixpkgs pinned in-flake** (`dc5d91f84032`, channel rev — see README Pins); devShell (host x86_64, MNX npins) | `flake.nix` |
| Out-of-tree device definition (boot.img geometry, borrowed kernel, minimal initrd wiring) | `devices/planet-geminipda/` |
| Stage-2 system config (headless + g_ether SSH, services, mesa fork, systemd-BPF/cudaLLVM overlays) | `config/gemini.nix` |
| SoC fragment (out-of-tree MT6797) | `modules/hardware-soc-mediatek-mt6797.nix` |
| Device services: GPU poweron / A72-up / battery-guard / WDT reboot | `services/gemini-pda.nix`, `services/scripts/` |
| Audio (PipeWire S16 path), Wi-Fi (CONSYS+USB), desktop (gemwl), **LXQt nested (labwc → lxqt-session)** | `services/audio.nix`, `services/wifi.nix`, `services/desktop.nix`, `services/lxqt.nix` (+ `services/scripts/start-lxqt-nested`, `config/lxqt/`) |
| Keyboard layouts — VT console map + desktop xkb: console `console.keyMap` (config/gemini.nix); desktop layout "gemini" packaged as an xkbcommon include dir and wired into the gemwl/lxqt-nested units via `XKB_CONFIG_EXTRA_PATH` (+ `XKB_DEFAULT_LAYOUT=gemini`) | `config/keymaps/gemini-uk.map`, `config/xkb/symbols/gemini`, `pkgs/gemini-xkb.nix` |
| Mesa 25.0.7+geminipda fork / wlroots 0.18.2 pin / gemwl pkgs / **labwc 0.8.3 pin** | `pkgs/{mesa-geminipda,wlroots-geminipda,gemwl,labwc-geminipda}.nix` + `patches/` |
| Borrowed kernel #329 reference (kept until the self-built kernel is glass-verified; NOT wired into any build since 2026-09-08) | `kernel/borrowed/` (payload, DTB, module tree, sramldo-smc.ko, config) + `devices/planet-geminipda/kernel-borrowed.nix` |
| Kernel source: published v6.6 base (fetch-pinned) + tracked delta + lean config | `devices/planet-geminipda/kernel/` (`default.nix`, `delta/`, `config`, `config.full-329`) + `kernel/base` submodule pointer |
| Kernel config pruning + delta sync tools | `bin/prune-kernel-config.sh`, `bin/sync-kernel-delta.sh` (replaces the retired `bin/snapshot-kernel.sh`) |
| boot.img header inspection | `bin/dump-bootimg-header.sh` |
| **Recovery tooling** — patched-mtkclient launcher (preloader/BROM), USB-state watcher | `bin/run-mtk.sh`, `bin/usb-watch.sh` (+ devshell `mtkclient` = store pkg + DAs) |
| **g_ether net-up / SSH / WDT-EXRST reboot** (host side) | `bin/net-up.sh`, `bin/device-ssh.sh`, `bin/device-reboot.sh` |
| **Boot-target switching + boot-partition flash** (adb/TWRP; twrp/android/debian/flash/restore) | `bin/boot-switch.sh` |
| **Full NixOS flash orchestration** (converge-to-TWRP from any state, boot + p32 userdata rootfs; Debian p29 preserved) | `bin/flash-nixos.sh` (verbs incl. `grow-rootfs` — offline p32 fs growth from TWRP, R13) |
| **Build/switch/rollback generations like a workstation** (native-aarch64 distributed build → delta `nix copy` → device profile switch + activate; NO reflash) | `bin/deploy.sh` (status/build/deploy/rollback) |
| **GC-pin builds** (host `nix-collect-garbage` protection — every deploy pins itself; list/unpin) | `bin/gc-pin.sh` |
| **Detached job runner** (rule 8) | `bin/run-job.sh`; state `logs/jobs/` |
| Device partition backups pulled over adb/dd (gitignored; nvram/IMEI private — never commit) | `stock-dump/` (ledger + copy status: `docs/disaster-recovery/inventory.md`) |
| Legacy hardware/boot receipts (port pending M1) | `/home/cjdell/Projects/GeminiPDA/docs/` (read-only reference until ported) |

## When to update what

- **Port decision / build-level delta** (eval error, derivation quirk,
  new overlay) → this repo's docs + derivation comments; a dated entry
  in `docs/session-log.md`.
- **Milestone status** (phase table in the feasibility doc, README
  status) → update both; keep "build-level vs on-glass" explicit.
- **New hardware/boot fact or correction to one** → THIS repo
  (golden): `docs/boot-process.md` or the receipt doc being ported
  (M1), with `file:line` + date; if the same claim exists in a legacy
  GeminiPDA doc, port-fix it here and optionally leave a one-line
  `[superseded — see gemini-nixos …]` pointer there.
- **A flash/boot attempt happened** → `docs/session-log.md` (MANDATORY:
  versions flashed, hashes, exact behaviour, console output) +
  feasibility doc §9 phase notes.
- **Pins moved** (Mobile NixOS rev/narHash, **nixpkgs flake pin** (channel rev, flake.nix), kernel
  snapshot) → `flake.nix`/README "Pins" + date it (npins drift breaks
  the verified graphics stack — R2; the flake nixpkgs pin should track
  the nixos-unstable CHANNEL rev so hydra's full closure stays cached).
- **DR ledger changed** (a dump taken/verified/copied) →
  `docs/disaster-recovery/inventory.md` + a version line in the log.

## Device operations cheat sheet (mechanisms verified 2026-08-30/31 on
the legacy project; receipts still in GeminiPDA docs until M1 ports them)

**AFTER ANY DEVICE POWER-ON/POWER-CYCLE (GOLDEN RULE):** the host side
of the g_ether link is DOWN (the gadget iface disappears on power-off).
`bash bin/device-ssh.sh '<cmd>'` auto-runs `bin/net-up.sh` (passwordless
sudo) — root shell @ **10.15.19.82** (host 10.15.19.1/24 on
`enp10s0f4u1u2`, renamed per USB port; the device auto-configures its
side at boot). Hand-rolled ssh needs `sudo bash bin/net-up.sh` first.

**Remote reboot of a running Linux:** software resets POWER THE UNIT OFF
(verified 2026-08-31) — only the LK-configured WDT EXRST path
self-boots: `busybox devmem 0x10007004 32 0x48` (2 s WDT). From the host:
`bash bin/device-reboot.sh`. Device-side (NixOS rootfs): the
`gemini-wdt-reboot` unit does the same.

**Boot targets** (adb state machine in `bin/boot-switch.sh`; para = p2
of the largest mmcblk, 32-byte command at offset 0):
- `boot-recovery\0` + 18 zero bytes → **TWRP on every power-on (sticky —
  TWRP does not clear it)**; `boot-debian\0` + 20 zero bytes → NORMAL
  boots the boot image's **Debian branch (p29)**; 32 zero bytes → NORMAL
  boots the boot image's **NixOS branch (p32 — the default)** [dual-boot
  selector 2026-09-07, docs/repartition-android-space.md]. Only `para`
  and `boot` are ever written; never nvram/proinfo/protect*.
- `bash bin/boot-switch.sh status|twrp|android|debian|flash [img]|restore`.
  NOTE: `android`/`boot-nixos` = para-clear + reboot → NORMAL → boots the
  NixOS p32 default with the dual-boot boot.img installed; `debian` =
  para=boot-debian. "Linux" is not a separate slot: our kernel goes in
  `boot` itself (boot2/boot3 are legacy reference slots, untouched).
- Device running Linux (no adbd — the current real state): converge via
  `bin/flash-nixos.sh` (para write over ssh + WDT EXRST → TWRP); OS
  switching over ssh = `flash-nixos.sh debian|boot-nixos`.
- A hung boot image has NO software path back (para cleared = normal
  boot) → recovery = mtkclient preloader mode (`bin/run-mtk.sh`;
  full playbook `docs/disaster-recovery/drills.md`) — this is why the
  SAFE test cycle keeps para = boot-recovery (TWRP sticky) until the
  image is verified, and why `stock-dump/boot-*.img` backups are kept
  (restore = one adb command from TWRP).

**Flash pipeline (no fastboot on this device):** images go to partitions
from the patched no-swipe TWRP (root adbd) by-name paths:
`/dev/block/platform/mtk-msdc.0/11230000.msdc0/by-name/{boot,linux,userdata,para}`.
NixOS rootfs → p32 `userdata` (27.3 GiB, ext4 label `NIXOS_SYSTEM` —
**destroys Android's FDE userdata only**; the Debian rootfs on p29
`linux` is never written and stays bootable via the `boot-debian`
marker). Boot image (dual-boot boot.img) → p22 `boot` (16 MiB).
Orchestrated by `bin/flash-nixos.sh
status|boot|rootfs|all|boot-nixos|debian` (see its header for the safety
model + run-job usage).

**Battery/charger truth** (OS-dependent; verified live on the legacy
project): TWRP sysfs is STALE — read dmesg `[PE+]Ibat=..`; Linux:
`/sys/class/power_supply/bq25890-charger-0` (+ `bq25896-raw.sh` raw ADC
over i2c). ICHGR = 0 is NORMAL when load ≥ input power (power-path);
scale 50 mA/step. No fuel gauge → voltage thresholds only (guard
<3.65 V, orderly poweroff <3.50 V). `i2cget -f -y 0 0x6b` — the `-f` is
MANDATORY (kernel driver claims 0x6b).

## Conventions

- **Statuses:** ✅ done/verified · 🟡 built but not on hardware/partial ·
  ⬜ planned · 🔴 blocked. Update the phase table
  (`docs/mobile-nixos-port-feasibility.md` §9) + README status.
- **Dates:** `YYYY-MM-DD`; header of each doc carries "Last updated".
- **Nix files:** keep formatted (`nixpkgs-fmt` in the devshell if added);
  comment derivations like the existing ones (they carry the quirks —
  R2/R8/R9-style receipts).
- **Kernel delta hygiene:** the kernel builds from the published v6.6
  base + `devices/planet-geminipda/kernel/delta/` (source of truth;
  nix-build-only fixes live in `kernel/default.nix` so the delta stays
  byte-identical to the fork). Change the fork source (legacy
  `GeminiPDA/repos/linux-6.6`), then `bash bin/sync-kernel-delta.sh`
  (materialize + byte-verify base+delta == rev) and update the rev in
  the derivation header. Do not hand-edit the delta without re-running
  the sync verify. The old `bin/snapshot-kernel.sh` + 225 MB tarball
  are retired (git history has them).
- **Device backups:** `stock-dump/` is gitignored — boot/para backups
  live there (ledger + copy status in
  `docs/disaster-recovery/inventory.md`); nvram (IMEI) is private,
  never commit it.
- **Host has NO python3/adb on PATH** (rule 7) — `bin/*.py`-style tooling
  would need `nix develop --command python3 …`; `bin/run-mtk.sh` finds
  its own store python (sudo context) but needs the devshell's
  `mtkclient` closure to exist (build once: `nix develop --command true`).

## Before you start a session

1. Read `docs/session-log.md` (last entries) + the phase table in
   `docs/mobile-nixos-port-feasibility.md` §9 + README status.
2. If the session may touch device behaviour and the topic's receipts
   are not yet ported (M1/M7), glance at the legacy
   `GeminiPDA/docs/session-log.md` tail — it is the ground truth of what
   was tried until the history port lands.
3. Note the pinned versions (README "Pins") and doc "Last updated" dates.
4. Decide what needs updating as you go — don't wait until the end.

## Before you end a session

1. Append a dated entry to `docs/session-log.md` (what was tried, what
   flashed/changed, versions, hashes, next action).
2. Update every doc whose claims the session touched (statuses, dates) —
   including the phase table, README and the DR ledger.
3. If the session changed the kernel delta or its fork rev, re-run
   `bin/sync-kernel-delta.sh` and update the references.
4. If the session ran a flash/boot cycle, leave the version line +
   outcome in the session log and say whether the glass/TWRP state was
   left safe (prefer para = boot-recovery until images are verified).
5. New knowledge goes HERE (golden) — if you also touched a legacy doc,
   say so in the log so the port can pick it up.

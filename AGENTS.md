# AGENTS.md — gemini-nixos: Mobile NixOS for the Gemini PDA

This repo ports the working mainline-Linux bring-up of the Planet
Computers Gemini PDA (MT6797X) onto Mobile NixOS. Read this file first,
then `README.md` and `docs/mobile-nixos-port-feasibility.md`.

**The sibling project is the knowledge authority.** All hardware/boot
truth (LK mechanics with `file:line` receipts, partitions, battery,
display, flash history) lives in `/home/cjdell/Projects/GeminiPDA` —
read `GeminiPDA/AGENTS.md`, `GeminiPDA/docs/{boot-chain,hardware,
flashing,mainline-support,roadmap}.md` and `GeminiPDA/docs/session-log.md`
before touching device behaviour. This repo records **port decisions and
deltas** (declarative Nix packaging of that knowledge) and cross-checks
claims against the sibling's receipts; it does not re-derive hardware
truth from scratch.

Status (2026-09-07): phases 0/1/3 done at the **build level** — the
flake's `boot.img` + `rootfs.img` build green and match the bring-up boot
contract — but **nothing has been flashed/verified on glass yet**. The
real device currently runs the GeminiPDA Debian rootfs on the borrowed
kernel #329 (p29 + `boot`). Flash tooling is in `bin/` and ready; the
"moment of truth" (phase 2: SSH to a NixOS shell on hardware) is the
next milestone.

## Golden rules

0. **Version & commit hygiene.** Every artifact that goes to the device
   carries an identity, and every session records what it flashed:
   - the kernel is **borrowed** (#329, `6.6.0-00048-g188aade698dd`,
     source commit `733c0c7ea74195bd30734f599f37e69febfd38e0`, snapshot
     `kernel/geminipda-bringup-733c0c7ea.tar.gz`, artifacts vendored in
     `kernel/borrowed/` — never rebuilt in-tree yet);
   - Mesa 25.0.7 fork, wlroots 0.18.2, gemwl: exact pins in
     `flake.nix`/the pkgs derivations.
   Log one version line per flash in `docs/session-log.md` (kernel,
   boot.img hash, mesa/wlroots pins). Never flash a build you cannot
   identify. [added 2026-09-07, adapted from GeminiPDA rule 0]
1. **Record before you forget.** Any fact learned from source, a build,
   an eval error, or (eventually) a boot attempt belongs in a doc — with
   a date. This repo's log is `docs/session-log.md`; port/build facts go
   to `README.md` / `docs/mobile-nixos-port-feasibility.md`;
   *hardware/boot* facts found by re-deriving them belong in the sibling
   project's docs (or copy them here with the receipt + a note that the
   sibling needs the same fix).
2. **Never silently contradict a doc.** Fix it *and* note the correction
   (`[corrected YYYY-MM-DD]`). If unsure, add rather than replace, and
   mark it `unverified`. Claims about *upstream* state ("is in
   mainline") must carry the date they were verified.
3. **Prefer evidence over opinion.** Keep receipts: `file:line` for LK
   behaviour (sibling docs), derivation/commit ids for nix behaviour
   (ours — nixpkgs floats, so pin + date every claim about it).
4. **Small, dated deltas beat big rewrites.** Append to the tables and
   logs; don't restructure docs casually.
5. **NEVER abuse the LCD panel/matrix (CORE RULE).** The unit's NT36672
   TDDI is initialized ONLY by LK; a badly-initialized panel shows the
   "uninitialised" flicker and can BURN the LCD matrix (permanent damage
   — observed 2026-09-04 on the sibling unit). For THIS repo:
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
     by a capture. (Full story: GeminiPDA docs/session-log 2026-09-04.)
6. **Scripts over long ad-hoc commands (CORE RULE).** Any multi-step
   host/device operation needed more than once becomes a committed
   script in `bin/` the FIRST time (headers: what/why/usage; reuse
   `bin/device-ssh.sh` / the devshell helpers; fail loudly on missing
   tools). Long inline `ssh '...'` chains and host for-loops are
   throwaway EXPLORATION only — promote them before the session ends.
7. **Devshell-only CLIs — NEVER bare (CORE RULE).** The bare host PATH
   carries only git/nix/coreutils/ssh/usbutils-ish basics — verified
   NO python3/adb/make there (2026-09-07). Every project CLI (python3,
   adb, fastboot, …) lives ONLY in the flake devshell
   (`flake.nix` → `devShells.x86_64-linux.default`: python3, git,
   android-tools, usbutils). Reach them with
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
   Long flash ops (rootfs push+dd, p29 backup) MUST run under run-job.

## Where things live

| Concern | File |
|---|---|
| Repo purpose, layout, boot chain, flash flow, phase status | `README.md` |
| Feasibility study + the phased plan (phase table = roadmap) | `docs/mobile-nixos-port-feasibility.md` |
| "Published base + in-repo delta" pattern (mesa done; kernel next) | `docs/library-deltas.md` |
| **What was actually tried / happened** (dated entries) | `docs/session-log.md` |
| Flake entry; Mobile NixOS pin (`2c132754`); nixpkgs via its npins; devShell | `flake.nix` |
| Out-of-tree device definition (boot.img geometry, borrowed kernel, minimal initrd wiring) | `devices/planet-geminipda/` |
| Stage-2 system config (headless + g_ether SSH, services, mesa fork, systemd-BPF/cudaLLVM overlays) | `config/gemini.nix` |
| SoC fragment (out-of-tree MT6797) | `modules/hardware-soc-mediatek-mt6797.nix` |
| Device services: GPU poweron / A72-up / battery-guard / WDT reboot | `services/gemini-pda.nix`, `services/scripts/` |
| Audio (PipeWire S16 path), Wi-Fi (CONSYS+USB), desktop (gemwl) | `services/audio.nix`, `services/wifi.nix`, `services/desktop.nix` |
| Mesa 25.0.7+geminipda fork / wlroots 0.18.2 pin / gemwl pkgs | `pkgs/{mesa-geminipda,wlroots-geminipda,gemwl}.nix` + `patches/` |
| Borrowed kernel #329 artifacts (vendored, tracked) | `kernel/borrowed/` (payload, DTB, module tree, sramldo-smc.ko, .config) |
| Kernel source snapshot (gitignored, intent-to-add) | `kernel/geminipda-bringup-733c0c7ea.tar.gz` + `bin/snapshot-kernel.sh` |
| boot.img header inspection | `bin/dump-bootimg-header.sh` |
| **g_ether net-up / SSH / WDT-EXRST reboot** (host side) | `bin/net-up.sh`, `bin/device-ssh.sh`, `bin/device-reboot.sh` |
| **Boot-target switching + boot-partition flash** (adb/TWRP) | `bin/boot-switch.sh` |
| **Full NixOS flash orchestration** (converge-to-TWRP from any state, boot + p29 rootfs) | `bin/flash-nixos.sh` |
| **Detached job runner** (rule 8) | `bin/run-job.sh`; state `logs/jobs/` |
| Device partition backups pulled over adb (gitignored; nvram/IMEI private — never commit) | `stock-dump/` |
| Hardware/boot truth with file:line receipts | `/home/cjdell/Projects/GeminiPDA/docs/` (read-only authority) |

## When to update what

- **Port decision / build-level delta** (eval error, derivation quirk,
  new overlay) → this repo's docs + derivation comments; a dated entry
  in `docs/session-log.md`.
- **Milestone status** (phase table in the feasibility doc, README
  status) → update both; keep "build-level vs on-glass" explicit.
- **New hardware/boot fact or correction to one** → sibling
  `GeminiPDA/docs/*` (with receipts); here only if this repo
  contradicts it, then also flag the sibling.
- **A flash/boot attempt happened** → `docs/session-log.md` (MANDATORY:
  versions flashed, hashes, exact behaviour, console output) +
  feasibility doc §9 phase notes.
- **Pins moved** (Mobile NixOS rev/narHash, nixpkgs npins, kernel
  snapshot) → `flake.nix`/README "Pins" + date it (npins drift breaks
  the verified graphics stack — R2).

## Device operations cheat sheet (mechanisms verified on the sibling
project; receipts in GeminiPDA docs/boot-chain.md §8c, docs/flashing.md)

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
  TWRP does not clear it)**; 32 zero bytes → NORMAL boot of the `boot`
  partition. Only `para` and `boot` are ever written; never
  nvram/proinfo/protect*.
- `bash bin/boot-switch.sh status|twrp|android|flash [img]|restore`.
  NOTE: `android` = para-clear + reboot → NORMAL → boots whatever is in
  `boot` — i.e. it is ALSO how you boot the flashed NixOS boot.img.
  "Linux" is not a separate slot: our kernel goes in `boot` itself
  (boot2/boot3 are sibling-project reference slots, untouched).
- Device running Linux (no adbd — the current real state): converge via
  `bin/flash-nixos.sh` (para write over ssh + WDT EXRST → TWRP).
- A hung boot image has NO software path back (para cleared = normal
  boot) → recovery = mtkclient preloader mode (GeminiPDA docs/flashing.md)
  — this is why the SAFE test cycle keeps para = boot-recovery (TWRP
  sticky) until the image is verified, and why `stock-dump/boot-*.img`
  backups are kept (restore = one adb command from TWRP).

**Flash pipeline (no fastboot on this device):** images go to partitions
from the patched no-swipe TWRP (root adbd) by-name paths:
`/dev/block/platform/mtk-msdc.0/11230000.msdc0/by-name/{boot,linux,para}`.
NixOS rootfs → p29 `linux` (27.7 GiB, ext4 label `NIXOS_SYSTEM` — wipes
the current GeminiPDA Debian rootfs; Android p27/p32 untouched). Boot
image → p22 `boot` (16 MiB). Orchestrated by `bin/flash-nixos.sh
status|boot|rootfs|all|boot-nixos` (see its header for the safety
model + run-job usage).

**Battery/charger truth** (OS-dependent; from the sibling's live
verification): TWRP sysfs is STALE — read dmesg `[PE+]Ibat=..`; Linux:
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
- **Kernel snapshot hygiene:** never hand-edit the borrowed artifacts or
  the snapshot tarball; change the source (sibling `repos/linux-6.6`),
  then `bash bin/snapshot-kernel.sh` + rename references. The tarball
  stays gitignored + intent-to-add (`git add -Nf`) — never `git add .`
  (it would commit the tarball). `kernel/borrowed/` IS tracked (small,
  source commit local-only).
- **Device backups:** `stock-dump/` is gitignored — boot/para backups
  live there; nvram (IMEI) is private, never commit it.
- **Host has NO python3/adb on PATH** (rule 7) — `bin/*.py`-style tooling
  would need `nix develop --command python3 …`; there is none in this
  repo yet, but remember when you add one.

## Before you start a session

1. Read `docs/session-log.md` (last entries) + the phase table in
   `docs/mobile-nixos-port-feasibility.md` §9 + README status.
2. Glance at the sibling's `docs/session-log.md` tail if the session may
   touch device behaviour (it is the ground truth of what was tried).
3. Note the pinned versions (README "Pins") and doc "Last updated" dates.
4. Decide what needs updating as you go — don't wait until the end.

## Before you end a session

1. Append a dated entry to `docs/session-log.md` (what was tried, what
   flashed/changed, versions, hashes, next action).
2. Update every doc whose claims the session touched (statuses, dates) —
   including the phase table and README.
3. If the session changed the kernel pin or borrowed artifacts, re-run
   `bin/snapshot-kernel.sh` and update the references.
4. If the session ran a flash/boot cycle, leave the version line +
   outcome in the session log and say whether the glass/TWRP state was
   left safe (prefer para = boot-recovery until images are verified).

# gemini-nixos session log

Dated entries of what was actually tried / decided in this repo.
Hardware/boot ground truth lives in the sibling project
(`/home/cjdell/Projects/GeminiPDA/docs/session-log.md`) — cross-reference
when a session touches device behaviour. Latest entry first.

## 2026-09-07 — boot-process explainer doc (from the Q&A session; repo-only)

Saved the bootstrapping Q&A (kernel identity / cmdline / rootfs selection /
initramfs builds) as **NEW `docs/boot-process.md`** — a plain-language
explainer layered over the receipt docs. Contents: the one NORMAL boot
slot (only `boot` p22 / `recovery` p1 are loadable by LK), boot.img =
kernel+DTB+ramdisk, the shared borrowed #329 kernel (byte-identity
verified: kernel payload sha256 `3a2a7f3a…822` matches between the
sibling's new_kali_boot.img and `kernel/borrowed/`), the ramdisk `/init`
as the rootfs selector (content markers; NixOS store-only vs Debian
`/etc/os-release`), the four cmdline locations + `CMDLINE_FORCE` (both
OSes boot the identical forced cmdline today; per-OS cmdlines only with
per-OS kernels), the para-marker selector table, the two initramfs builds
vs the one proposed dual-boot initrd, and the size constraints shaping it
all. README layout table got a row for the doc. Nothing flashed; no code
changes.

Next action: unchanged — §10 decisions of `docs/repartition-android-space.md`,
then the §9 implementation.

## 2026-09-07 — Android-space repurpose + dual-boot investigation (repo-only; no device interaction, nothing flashed)

Investigated (per the user, no device changes): can the NixOS rootfs go
where Android currently is, keeping the GeminiPDA Debian rootfs (p29) for
testing, ideally bootable without reflashing `boot`? Also: the 16 MiB
`boot` size-constraint risks + workarounds. Outcome = a proposal doc, no
code changes:

- **NEW `docs/repartition-android-space.md`** — full write-up with
  receipts: partition map + LK boot truth (`boot`/`recovery` are the only
  partitions LK loads — `mt_boot.c:1484/1527`; boot2/boot3 never), the
  p32 `userdata` (27.33 GiB) recommendation over p27/GPT surgery, the
  para-command dual-boot selector (`boot-recovery` reserved for LK→TWRP,
  `boot-debian` → p29, zeros → p32 NixOS default), the Debian handoff to
  replicate from the GeminiPDA initramfs (fstab `/` fix, A72 opt-in
  enforcement), boot-budget measurements (boot.img 14.72 MiB of 16 MiB =
  1.28 MiB headroom; kernel gz 13.45 MiB → 34.3 MiB decompressed; LK is
  zlib/gzip-only; RD_* all =y for future initrd formats), the
  shared-kernel constraint (§8), and the repo-side change list (§9) +
  open decisions (§10).
- **`docs/mobile-nixos-port-feasibility.md`**: two dated [superseded
  2026-09-07] annotations — §7 Non-risks "Android partition layout" row
  and §8 decision 4 (Android p27/p32 fate) — pointing at the new doc
  (both previously assumed p29-only repurpose with Android untouched).
- README not touched (nothing flashed/decided yet; its p29-target text
  stays until §10 decisions land).

Evidence gathered this session (facts for the record, all verified
read-only): current on-device `boot` backup `stock-dump/boot-20260907-
013541.img` carries kernel #328 (00047-g3b3a2b6 — the pre-#329 boot),
#329 is what the device runs now (sibling log 2026-09-07); the repo's
borrowed `kernel/borrowed/Image.gz` = the same #329 payload
(00048-g188aade698dd, version string verified); #329 `.config` has
`CONFIG_CMDLINE_FORCE=y` (boot.img cmdline inert for both OSes); store
build artifacts measured: boot.img 15,433,728 B (kernel 14,108,276 B +
ramdisk 1,321,716 B, 2048 pages) + system.img 1,849,479,168 B; para env
window @ 0x20000 (env.h:36-44) — offset-0 command writes never touch it.

Next action: user decides §10 (default OS, marker location, kernel
phase), then implement §9 (initrd dual-boot branch, flash/boot-switch
re-target to p32, boot-debian verb/unit) — still nothing flashed until
the phase-2 on-glass cycle.

## 2026-09-07 — outstanding.md worked: SSH/logind/keymap/DRM-race/udev/backlight/NAT + R10 shebang fix (build-level; nothing flashed)

Worked the `outstanding.md` rootfs-viability list (§13 order). **No
flash** — device still on the GeminiPDA Debian rootfs / kernel #329; it
was reachable over g_ether for read-only captures + one host-NAT test.

Fixes landed (all build-level, toplevel rebuilt + closure-inspected):

- **§2 SSH root key + §11 hostname** (`config/gemini.nix`):
  `users.users.root.openssh.authorizedKeys.keys` = the
  `id_ed25519_gemini.pub` key; `networking.hostName = "gemini"`.
  Closure: `/etc/ssh/authorized_keys.d/root` carries the key;
  `/etc/hostname` = `gemini`. Toplevel now `nixos-system-gemini-…`.
- **§3 logind side-key policy** (`config/gemini.nix`):
  `services.logind.settings.Login.{HandleSuspendKey,HandleHibernateKey,
  HandlePowerKey} = "ignore"` — CORRECTED the audit's fix sketch:
  `services.logind.extraConfig` is **removed** in this nixpkgs pin
  (module now exposes `settings.Login`). Closure `logind.conf` has the
  `[Login]` section.
- **§4 keymap** (vendor + config): `config/keymaps/gemini-uk.map`
  copied verbatim from GeminiPDA `build/rootfs-files/keyboard/` (+ a
  provenance README); `console.keyMap = ./keymaps/gemini-uk.map`.
  Closure `/etc/vconsole.conf` = `KEYMAP=<store path>`;
  `loadkeys --validate` passes.
- **§5 DRM/panfrost race** (`services/gemini-pda.nix`):
  `boot.kernelModules` += `drm drm_shmem_helper gpu-sched panfrost`
  (+ `mt6351-keys` for §11 determinism). Closure
  `/etc/modules-load.d/nixos.conf` lists them; all four .ko verified
  present in the borrowed #329 module tree.
- **§6 udev USB host-PM rule** (`services/gemini-pda.nix`):
  `services.udev.extraRules` with the B-19 three lines; closure
  `99-local.rules` carries them. Device-only rule captured verbatim
  from the live Debian rootfs first.
- **§7 backlight-default** (`services/gemini-pda.nix`):
  `gemini-backlight-default` oneshot unit (10 %, after udevd); in the
  closure + `multi-user.target.wants`.
- **§8 host NAT** (`bin/usb-tether-nat.sh`, NEW): port of the sibling
  `build/usb-tether-nat.sh` — auto-detected upstream iface + tool
  checks. **Verified live** this session: device `ping -c1 1.1.1.1`
  succeeds (~3 ms) with the NAT rule up.
- **§10 wdt/boot-recovery units** (`services/gemini-pda.nix`):
  `gemini-wdt-reboot.service` + `gemini-boot-recovery.service` as
  hand-started oneshots (no `wantedBy`); both in the closure. AGENTS/
  README unit claims now accurate. boot-recovery comment clarified:
  plain reboot powers off → next power-on (sticky para) lands in TWRP.
- **§11 minors**: hostname + mt6351-keys pin done (above);
  renderD129→renderD128 comments fixed in `services/desktop.nix`;
  serial-getty + wifi-DNS remain on-glass checks.
- **§9 GPU warmup**: DECIDED deferred to the first gemwl boot on glass
  (#329 banding question needs glass); risk + both fix options
  documented in the `services/desktop.nix` header.

**R10 — new port delta found while working the list** (feasibility doc
§7 R10, `services/gemini-utils.nix`): the verbatim Debian scripts shebang
`#!/bin/bash`, but a NixOS rootfs has NO `/bin/bash` (stage-2 only makes
`/bin/sh` via `environment.binsh`) and systemd ExecStart execs scripts
straight (kernel resolves `#!`) → every bash unit (gpu-poweron, a72-up/
cl2-up, battery-guard, audio-defaults, backlight) would have failed on
glass with status=203/EXEC. Fix: package-time shebang rewrite to the
store bash. Verified: packaged scripts now `#!<store>/bin/bash`;
`#!/bin/sh` scripts untouched.

Closure inspected: `/nix/store/pscdi0fn9lan4rcnh2c4g9ksvhd3kh58-
nixos-system-gemini-26.11pre1031299.0bb7ec54c848` (== current
`.#packages.x86_64-linux.toplevel`). Rebuilt under `bin/run-job.sh`
(toplevel-rebuild, rc=0). No image was built/flashed — the boot/rootfs
artifacts are unchanged by this session (config/closure only).

Device-only files captured from the live Debian rootfs (pre-flash
insurance, per outstanding.md §0/§12): the udev rule, logind drop-in,
`/etc/modules-load.d/99-gpu.conf`, `backlight-default.service`, root
`authorized_keys` — all match the inline copies in outstanding.md.

Next action (unchanged): phase 2 — on-glass verification. When the
device is next on the bench: `bin/flash-nixos.sh status` → `boot` →
`rootfs --yes` → verify serial/fbcon → `boot-nixos`; then the on-glass
checks per outstanding.md items (ssh `uname -r`, silver button, keymap
Fn combos, `ls /dev/dri`, backlight get, dongle plug, §9 banding watch).
Log the outcome here with image hashes.

## 2026-09-07 — AGENTS.md + flash/recovery tooling imported (build-level; nothing flashed)

What happened (repo-only; no device interaction — unit untouched, still
running the GeminiPDA Debian rootfs on kernel #329):

- **`AGENTS.md` created** at the repo root, adapted from the sibling's
  AGENTS.md (which was read in full). Brought over, re-contextualised for
  the Mobile NixOS port: version/commit hygiene (rule 0), record-before-
  forget + date + receipts, the LCD-panel safety rule (applies to the
  kernel derivation when the in-repo build is finally used — must stay
  the fbcon/EXCLUDE_DISPLAY build), scripts-over-ad-hoc (rule 6),
  devshell-only CLIs (rule 7 — bare host PATH verified to lack
  python3/adb/make, 2026-09-07), run-job detached runner (rule 8), the
  "where things live"/"when to update what" tables, a device-operations
  cheat sheet, and session start/end discipline. New meta-rule for this
  repo: **the sibling GeminiPDA project is the knowledge authority** —
  receipts live there; this repo records port decisions/deltas.
- **Flash/recovery tooling added under `bin/`**, ported from the
  sibling (GeminiPDA @ abc0afb, 2026-09-07):
  - `bin/net-up.sh`, `bin/device-ssh.sh`, `bin/device-reboot.sh` —
    g_ether host-side link/ssh/WDT-EXRST reboot (near-verbatim ports;
    device 10.15.19.82, key ~/.ssh/id_ed25519_gemini).
  - `bin/boot-switch.sh` — adb/TWRP boot-target state machine
    (status/twrp/android/flash/restore). Deltas vs the sibling: adb/lsusb
    resolved via the flake devshell (self re-exec with a
    `GEMINI_DEVSH_REEXEC` guard); dropped the Gemian `linux` command —
    on this project Linux = the NixOS boot.img in `boot` itself, booted
    by `android` (para-clear + reboot); `xxd` replaced with coreutils
    `od` in `status`.
  - `bin/flash-nixos.sh` — NEW orchestration for THIS repo's artifacts:
    converges to TWRP from ANY device state (running Linux → para write
    over ssh + WDT EXRST self-boot; Android → adb hop; POC/offline →
    prompts), then flashes `boot.img` → `boot` and/or `system.img` → p29
    (`linux`, by-name, sanity-checked ≥20 GiB via /proc/partitions since
    TWRP has no blockdev). Safe default: leaves para = boot-recovery
    (TWRP sticky) — never boots an unverified image unattended. Optional
    `--backup-rootfs` (adb exec-out dd, slow → run-job). Rootfs flash
    prompts unless `--yes`/`wipe p29`.
  - `bin/run-job.sh` — verbatim port (usage strings `bin/`-ified);
    smoke-tested 2026-09-07 (sleep job, rc=0).
- **`flake.nix` devShell** extended: python3+git now also
  `android-tools` (adb 36.0.1) + `usbutils` (lsusb) — the bare host
  PATH has no adb (rule 7). Verified `nix develop` resolves both.
- **`.gitignore`**: `logs/` (run-job state) + `stock-dump/` (device
  partition backups — nvram/IMEI private, never commit).
- **`README.md`**: layout table rows for the new `bin/` scripts;
  "Flashing" section rewritten around `bin/flash-nixos.sh` /
  `bin/boot-switch.sh` with the safety model; "do not flash yet" warning
  retained (still build-level only).

Versions (nothing flashed this session — recorded for the record):
kernel #329 `6.6.0-00048-g188aade698dd` (borrowed, pin
`733c0c7ea74195bd30734f599f37e69febfd38e0`); Mesa 25.0.7 fork; wlroots
0.18.2; gemwl 1.0; Mobile NixOS `2c132754`; nixpkgs
`nixos-26.11pre1031299.0bb7ec54c848`.

Next action: phase 2 — on-glass verification. When the device is next on
the bench: `bin/flash-nixos.sh status` → `boot` → `rootfs --yes` → verify
serial/fbcon → `boot-nixos`; log the outcome here with image hashes.

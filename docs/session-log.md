# gemini-nixos session log

Dated entries of what was actually tried / decided in this repo.
Hardware/boot ground truth lives in the sibling project
(`/home/cjdell/Projects/GeminiPDA/docs/session-log.md`) — cross-reference
when a session touches device behaviour. Latest entry first.

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

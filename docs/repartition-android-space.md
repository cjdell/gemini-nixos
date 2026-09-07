# Repurposing the Android partition space for the NixOS rootfs (dual-boot with Debian)

**Status:** ⬜ investigation + proposal — **no device or repo change
implemented** (nothing flashed, nothing rewritten). Decision on the open
questions in §10 is required before the §9 change list is implemented.
**Last updated:** 2026-09-07.
**Ground truth:** sibling GeminiPDA project (`/home/cjdell/Projects/
GeminiPDA`) — this doc only records the port-level decision/deltas and
cross-checks claims against its receipts (boot chain: `docs/boot-chain.md`,
partition map: `docs/hardware.md`, current device state: `docs/session-log.md`,
LK source: `repos/gemini-lk/lk`).

## 1. Why this exists (the problem)

The current flash plan puts the NixOS rootfs on p29 `linux` (27.7 GiB),
**wiping the GeminiPDA Debian 13 rootfs** that is still the reference/test
system (`bin/flash-nixos.sh rootfs` prompts "This DESTROYS the current p29
rootfs", and `outstanding.md` §12 says to capture device-only files
*before* the p29 wipe). The sibling's live device runs Debian on p29 with
the borrowed kernel #329 in `boot` (p22).

This session's goal: keep Debian on p29 for testing, **erase Android and
write the NixOS rootfs where Android currently is**, and keep both OSes
bootable **without reflashing `boot`** when switching. Also: document the
16 MiB `boot` size-constraint risks (docs R1) and the workarounds, since a
dual-boot image changes the pressure on that budget.

## 2. Partition map and boot truth (verified, with receipts)

GPT geometry from the mtkclient dump (`GeminiPDA/stock-dump/
gpt-2026-08-30.txt`, byte-accurate):

| # | name | size | today |
|---|---|---|---|
| p1 | recovery | 16 MiB | TWRP (booted only via para `boot-recovery`) |
| p2 | para | 0.5 MiB | bootloader_message command @ offset 0; LK env @ 0x20000 |
| p22 | boot | 16 MiB | kernel #329 + GeminiPDA scan-initramfs (Debian's boot) |
| p27 | system | 2.50 GiB | Android system (intact, inert) |
| p28 | cache | 0.42 GiB | Android cache |
| p29 | linux | **27.73 GiB** | **GeminiPDA Debian 13 rootfs** |
| p30/p31 | boot2/boot3 | 16 MiB each | reference slots — never loaded by LK |
| p32 | userdata | **27.33 GiB** | Android data (dm-crypt FDE) |
| p33 | flashinfo | 16 MiB | |

LK boot-mode facts (`GeminiPDA/repos/gemini-lk/lk`, receipts):

- **NORMAL** (and META/ADVMETA/ALARM/POC charging) loads **only p22
  `boot`** — `app/mt_boot/mt_boot.c:1484-1485`
  (`mboot_android_load_bootimg_hdr("boot", …)` under `MTK_GPT_SCHEME_SUPPORT`).
- **RECOVERY** is entered only when the para command is exactly
  `boot-recovery` (strict `strcmp`, `platform/mt6797/recovery.c:107`);
  it then loads **only p1 `recovery`** — `app/mt_boot/mt_boot.c:1527-1528`.
- `boot2`/`boot3` are **never** referenced by LK's loader
  (`boot_linux_from_storage` knows only the `boot` and `recovery`
  partition names). ⇒ **There is exactly one NORMAL boot slot.**
- The misc/bootloader_message command lives at **para offset 0** on this
  eMMC build (`platform/mt6797/recovery.c:91-99`: eMMC branch copies from
  `pdata`, no page offset). LK env is a sig/checksum-guarded 16 KiB window
  at **para offset 0x20000** (`platform/mt6797/env.h:36-44`) — writes to
  bytes 0..31 never touch it (this is the pattern `bin/boot-switch.sh`
  already uses on glass).
- Kernel cmdline: the #329 kernel forces its own cmdline
  (`CONFIG_CMDLINE_FORCE=y`, `kernel/borrowed/config-6.6.0-00048-g188aade698dd:532-534`),
  so the boot.img cmdline field is ignored by **both** OSes today.

Current on-device state (sibling session log 2026-09-07): Debian rootfs on
p29, kernel **#329** (`6.6.0-00048-g188aade698dd`) in p22, #329 module
tree installed on the Debian rootfs. The gemini-nixos borrowed kernel is
the **same #329** payload (version string verified in
`kernel/borrowed/Image.gz`).

## 3. Approach (summary)

1. **Rootfs:** reformat p32 `userdata` (27.33 GiB) ext4 with label
   `NIXOS_SYSTEM`, dd `system.img` there. No GPT surgery; Android's FDE
   ciphertext is destroyed (that is the erase). p27/p28 become spare.
2. **Boot:** one **dual-boot boot.img** stays in p22 — the shared #329
   kernel + an initrd that picks the rootfs from the para command field
   (offset 0): `boot-debian` → Debian p29; anything else / zeros → NixOS
   p32. `boot-recovery` stays reserved for LK → TWRP. Switching OS = one
   para write + reboot from either running OS (no reflash).
3. **Debian handoff:** the initrd's Debian branch replicates the proven
   GeminiPDA initramfs behaviour (`GeminiPDA/build/initramfs-6.6/init`)
   so Debian boots unchanged through our ramdisk.

## 4. Why p32 `userdata`, not p27 or GPT surgery

- Android's bulk is p32 (27.33 GiB); p27 `system` is only 2.5 GiB and
  p28 `cache` 0.42 GiB, and they are **not adjacent** to p32 (p29 linux +
  boot2/boot3 sit between p28 and p32). Merging system+cache yields only
  ≈2.92 GiB; merging boot2/boot3+userdata yields ≈ p32 alone but requires
  rewriting the GPT and killing the sibling's reference slots — not worth it.
- The current rootfs image is **1.72 GiB** (measured
  `/nix/store/…-android-fastboot-images-planet-geminipda/system.img`,
  2026-09-07 build) — p32 is ample, and Mobile NixOS's rootfs handling is
  partition-agnostic: root is mounted **by filesystem label**
  (`repos/mobile-nixos/modules/rootfs.nix`: default label `NIXOS_SYSTEM`,
  `autoResize = true` = growfs, first-boot `nix-store --load-db`
  re-hydration, `extraPadding` 20 MiB). Nothing in Mobile NixOS hardcodes
  "linux"/p29; `system_partition_destination` only feeds generated flash
  scripts/docs (flashing here is manual by-name dd via TWRP).
- TWRP side effect after the reformat: its fstab marks userdata
  `encryptable=metadata`, but with no crypto footer left it auto-detects
  plain ext4; flashing goes through by-name dd regardless, so TWRP's view
  of `/data` is irrelevant to the flash pipeline.
- Android side effect: booting Android afterwards = factory-fresh
  (no /data) — fine, Android is being dropped. p27 system can be wiped
  later if its 2.5 GiB is ever wanted.

## 5. Dual-boot mechanics (para command as the selector)

| para command (32 B @ offset 0) | LK behaviour | our initrd behaviour |
|---|---|---|
| `boot-recovery` | RECOVERY → p1 TWRP | (never reaches us) |
| `boot-debian\0` | ≠ `boot-recovery` → NORMAL → p22 | mount p29, Debian handoff (§6) |
| `""` (32 zeros) | NORMAL → p22 | NixOS p32 (default) |
| unknown other | NORMAL → p22 | fallback scan → shell if nothing |

- `boot-debian` is inert to LK (`recovery.c:107` exact-match strcmp) — the
  same property that makes today's `bootonce-bootloader` leftover harmless.
- **Default = NixOS on para-clear**, preserving the existing semantics of
  `bin/flash-nixos.sh boot-nixos` / `bin/boot-switch.sh android` ("clear
  para → NORMAL boots the flashed boot.img"). Debian is one para write
  away.
- Initrd fallback chain makes out-of-order states safe: marker target
  missing → scan for *any* valid rootfs (NixOS by `/nix/store` +
  `nix-path-registration`/system profile; Debian by `/etc/os-release` +
  `/sbin/init`) → busybox shell if none. (E.g. `boot-nixos` issued before
  p32 is flashed falls back to Debian, not a dead shell.)
- Robustness consideration (open question §10b): the marker could instead
  live at a dedicated para offset (e.g. 0x10000, inside the first 0x20000
  which LK only uses for the 6144-byte misc read and env) — fully decoupled
  from the Android bootloader_message struct. The offset-0 command field
  reuses the proven `boot-switch.sh` write pattern; its only quirk is that
  TWRP's own UI reboot actions rewrite the misc command (reboot-to-system
  clears it → choice resets to default). Our `bin/` flows write para with
  dd and are deterministic either way.
- Switching OS from the running OSes: para write + reboot. From NixOS a
  `gemini-boot-debian` CLI/unit would mirror the existing
  `gemini-boot-recovery` unit (WDT EXRST self-boot; plain reboot powers
  this unit off). From Debian the existing converge path in
  `bin/flash-nixos.sh` already does para writes over ssh.

## 6. The Debian handoff the initrd must replicate

When our ramdisk boots Debian it replaces the GeminiPDA initramfs that
Debian has been booting through (Debian's own `/boot/initrd.img` is *not*
used in this chain — kernel + initrd live in the p22 boot.img). To keep
Debian behaviour identical, the Debian branch must copy
`GeminiPDA/build/initramfs-6.6/init` exactly:

1. scan partitions (ext4 probe, ro then rw — a dirty journal needs rw to
   replay) for `/etc/os-release`;
2. mount rw, **rewrite the rootfs fstab `/` entry to the actual device**
   (`sed` — the flashed image hardcodes `/dev/mmcblk0p29`, and the eMMC
   may be mmcblk1 on this unit);
3. **enforce A72 opt-in** (remove `multi-user.target.wants/a72-up.service`
   and neuter an empty `/root/cl2-up.sh`) — the A72-cluster power-on
   wedges the box while boot is still busy; opt-in only from a settled
   system;
4. `exec switch_root /newroot /sbin/init`.

The current NixOS initrd (`devices/planet-geminipda/initrd.nix`) already
contains the NixOS branch (store-only image resolution, `/dev/disk/by-label`
symlink creation without udev, generation lookup). The Debian branch adds
~1-2 KiB of script — negligible against the 1.26 MiB ramdisk.

## 7. `boot` size constraints: measured numbers + workaround ladder

Measured artifacts (2026-09-07 store build; earlier/lighter config in
parentheses):

- `boot.img` = **15,433,728 B = 14.72 MiB** in the 16 MiB p22
  (earlier: 14,815,232 B = 14.13 MiB) → **headroom 1.28 MiB** (1.87 MiB).
- kernel payload (`Image.gz`+appended DTB) = 14,108,276 B = **13.45 MiB**
  gz; `Image.gz` alone 12.84 MiB → **decompresses to 34.3 MiB** (measured).
- ramdisk = 1,321,716 B = **1.26 MiB** gzip. Page size 2048, v0 header.

Hard ceilings (receipts in §2 + `GeminiPDA/docs/boot-chain.md`):

- **Kernel compression is gzip-only in LK** (zlib; `app/mt_boot/
  decompressor.c`) — no lzma/lz4/xz kernel payload.
- Decompressed kernel must fit LK's MT6797 scratch (50 MiB per boot-chain
  §10, overriding the generic 28 MiB) and stay clear of the ramdisk copy
  target 0x45000000 (78 MiB above the 0x40200000 kernel load). 34.3 MiB
  today ⇒ ~15+ MiB decompressed headroom — not binding yet, but watch it
  (decompressed grows ~2.5-3× the gz size).
- The ramdisk is copied verbatim by LK and decompressed **by the kernel**;
  the #329 config enables `CONFIG_RD_GZIP/BZIP2/LZMA/XZ/LZO/LZ4/ZSTD`, so
  the initrd format can change if ever needed.

The rootfs move to p32 does **not** interact with the boot budget (the
boot.img is identical regardless of which partition holds the rootfs). The
real pressure points and workarounds, in order:

1. **Kernel growth when the in-repo kernel replaces the borrowed one.** The
   in-repo pin (`733c0c7ea`, stale vs #329's `g188aade`) lacks the CONSYS
   Wi-Fi / audio-S16 / sidekey commits and must be re-synced first; its
   payload may exceed 13.45 MiB gz. Mitigation: config discipline — the
   mandatory fbcon/EXCLUDE_DISPLAY build (LCD-safety core rule) already
   excludes the display-landmine drivers; a bloat trim toward ~10 MiB gz
   buys ~4 MiB of headroom.
2. **Initrd growth** — the dual-boot logic is ~KiB; if it ever grew, the
   kernel supports xz/lz4/zstd ramdisks.
3. **Last-resort escape valve** (only if a future kernel genuinely cannot
   fit 16 MiB): GPT surgery to enlarge `boot` by absorbing the adjacent
   `logo` p23 (8 MiB) — LK sizes partition reads by GPT length.
   `boot2`/`boot3` **cannot** serve as overflow or a second NORMAL slot
   (LK never loads them).

## 8. Shared-kernel constraint (the one real dual-boot caveat)

Both OSes boot from one boot.img ⇒ **Debian and NixOS must run the same
kernel build**, and Debian's rootfs must carry that kernel's module tree.
True today: Debian runs the borrowed #329 and has its #329 modules
installed on p29 (sibling log 2026-09-07; the "install modules right after
a kernel flash" lesson is recorded three times there). When NixOS later
moves to a self-contained in-repo kernel, Debian-on-#329 would not boot
through the new boot.img (module vermagic mismatch) — testing Debian then
requires reflashing p22. **Recommendation: keep the borrowed #329 kernel
for the whole dual-boot phase; treat the in-repo kernel switch as a
deliberate moment when the Debian test loop changes.**

## 9. Change list (repo-side; implement only after §10 decisions)

- `devices/planet-geminipda/initrd.nix`: para-command selector read,
  Debian branch (§6), fallback chain. Add a few busybox applet symlinks if
  wanted (`dd`/`head` are already in the static busybox; the applet list
  only controls symlinks).
- `devices/planet-geminipda/default.nix`:
  `system_partition_destination = "userdata"` + comment update (p29 →
  p32; Debian stays on p29).
- `bin/flash-nixos.sh`: `rootfs` targets `by-name/userdata` (27.33 GiB
  passes the ≥20 GiB sanity check), prompt becomes "destroys Android
  userdata" not "wipes p29", drop the pointless `--backup-rootfs` of FDE
  ciphertext (or keep for a pre-format sanity dump — see §10d).
- `bin/boot-switch.sh` (+ docs): new `debian` verb (para =
  `boot-debian\0` + zeros, reboot) and a `flash-nixos.sh debian` path;
  keep `android`/`boot-nixos` = para-clear = NixOS default.
- NixOS side: `gemini-boot-debian` CLI/unit mirroring
  `gemini-boot-recovery` (in `services/gemini-pda.nix` /
  `services/scripts/`).
- Docs: README "Flashing"/layout rows, feasibility doc decision/annotations
  (§10 below), session-log entries per the repo rules once anything is
  flashed or changed.

## 10. Open decisions

a. **Default OS on para-clear.** Recommendation: NixOS (keeps the current
   "clear para → boot the flashed image" semantics; Debian one marker
   away).
b. **Marker location**: bootloader_message command field @ para 0
   (proven write pattern; resets to default if TWRP UI rewrites misc) vs
   a dedicated byte at para offset 0x10000 (decoupled from the Android
   struct — slight robustness preference).
c. **Kernel**: stay on borrowed #329 for the dual-boot phase (recommended)
   vs adopt the in-repo kernel now (breaks Debian-on-#329 boot without a
   p22 reflash, §8).
d. Whether to back up p32's ciphertext before the first format (it is
   useless as a restore — FDE — but preserves the option of a forensic
   dump); recommend no.

## 11. Sources / evidence

- This repo: `kernel/borrowed/` (Image.gz = #329 payload, version verified;
  config with `CONFIG_CMDLINE_FORCE`), `devices/planet-geminipda/initrd.nix`
  (current initrd), `bin/flash-nixos.sh` + `bin/boot-switch.sh` (current
  p29 target and para patterns), store builds (boot.img/system.img sizes).
- Sibling `GeminiPDA`: `docs/boot-chain.md` (§8b/§8c/§10: boot modes, para
  layout, decompress limits), `docs/hardware.md` (partition map),
  `docs/session-log.md` (2026-09-07: current #329 device state, modules
  lesson), `build/initramfs-6.6/init` (the Debian handoff to replicate),
  `stock-dump/gpt-2026-08-30.txt` (exact GPT offsets), `repos/gemini-lk/lk`
  (`app/mt_boot/mt_boot.c:1484/1527`, `platform/mt6797/recovery.c:107`,
  `platform/mt6797/env.h:36-44`, `app/mt_boot/decompressor.c`).
- Mobile NixOS `modules/rootfs.nix` (by-label root, growfs, store
  re-hydration — partition-agnostic).

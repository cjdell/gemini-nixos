# Repartitioning the Gemini PDA: TWRP + NixOS only (was: Android space → NixOS rootfs, dual-boot with Debian)

**Status:** ✅ DECIDED, IMPLEMENTED + ON GLASS (2026-09-10) — the final
repartition (§12) replaced Android + Debian + the dual-boot selector with
a single ~58 GiB `linux` partition: **TWRP (p1 recovery) and NixOS (p27)
are now the only bootable systems**. The 2026-09-07 §9/§10 dual-boot plan
(below) is *historical* — see §12.
**Last updated:** 2026-09-10.
> **GOLDEN-REPO note [2026-09-07]:** gemini-nixos is now the primary
> knowledge repo (AGENTS.md); the sibling GeminiPDA project is legacy
> and being folded in. The "ground truth" receipts cited below keep
> their `file:line` values but now live in the legacy repo until each is
> ported (M1).
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

Measured artifacts (2026-09-07 rebuild, sha-verified kernel payload
`3a2a7f3a…822`): `boot.img` = **14,815,232 B = 14.13 MiB** in the 16 MiB
p22 → **headroom 1.87 MiB** (older builds measured 15,433,728 B — a
different gzip encoding of the same #329 payload; [corrected 2026-09-07]).

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

## 10. Decisions (2026-09-07 — all taken; the change list §9 is implemented)

a. **Default OS on para-clear = NixOS** — keeps the existing
   "clear para → boot the flashed image" semantics of `boot-nixos`/
   `android`; Debian is one `boot-debian` marker away. [decided 2026-09-07]
b. **Marker location = the bootloader_message command field @ para 0**
   (32-byte command, offset 0) — the proven dd write pattern shared with
   `boot-recovery`, and what the dual-boot initrd's byte-exact `cmp`
   checks. TWRP-UI reboots rewrite misc (choice resets to NixOS default)
   — harmless, and no flow here uses TWRP's UI (all reboots are dd para
   writes + adb/WDT). The dedicated-0x10000 byte is not used. [decided
   2026-09-07]
c. **Kernel = stay on the borrowed #329** for the whole dual-boot phase
   (Debian keeps booting the same boot.img; the in-repo kernel switch is
   a deliberate, separate moment — §8). [decided 2026-09-07]
d. **No p32 ciphertext backup** — FDE data is useless as a restore;
   `flash-nixos.sh --backup-rootfs` is DROPPED (its p29 twin was the
   whole reason it existed). [decided 2026-09-07]

Implementation record (2026-09-07, §9 change list):

- `devices/planet-geminipda/initrd.nix` — dual-boot selector: reads the
  para command (p2 of the largest mmcblk, byte-exact cmp against
  `boot-debian\0`+zeros), Debian branch replicates
  `GeminiPDA/build/initramfs-6.6/init` (A72 opt-in + fstab fix +
  switch_root `/sbin/init`), NixOS default branch + other-kind fallback.
- `devices/planet-geminipda/default.nix` —
  `system_partition_destination = "userdata"` (p32) + comment.
- `bin/flash-nixos.sh` — `rootfs` targets `by-name/userdata` (p32);
  prompt "Type 'wipe android'"; `--backup-rootfs` removed; new `debian`
  verb (from Linux: ssh para write + WDT EXRST; from TWRP: adb).
- `bin/boot-switch.sh` — new `debian` verb (para=boot-debian + reboot
  from TWRP; no adb wait — Debian has no adbd).
- NixOS side: `services/scripts/gemini-boot-debian` (mirror of
  gemini-boot-recovery; 32-byte conv=sync write + read-back verify) +
  hand-started `gemini-boot-debian.service` in `services/gemini-pda.nix`.
- Docs: this doc, README (flashing + unit table), AGENTS cheat sheet,
  boot-process.md selector table, session-log entry.

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

## 12. Final repartition — TWRP + NixOS only (2026-09-10)

> **Supersedes §4, §5, §6, §9 and §10.** The user asked (2026-09-10)
> for a *clean install*: no user data needed, TWRP + NixOS the only
> bootable systems, NixOS owning all flash not needed by the boot chain.
> Android (`system`/`cache`/`userdata`) and the Debian rootfs on p29 are
> gone; the dual-boot para selector no longer has anything to select.

### 12.1 Layout (verified on glass 2026-09-10)

One large ext4 partition named `linux` (**p27, 0xE000000 = 224 MiB,
121651167 sectors = 58.006 GiB**) replaces Android's
`system`(p27)/`cache`(p28)/`userdata`(p32), the Debian `linux`(p29) and
`boot2`(p30)/`boot3`(p31). Every partition the boot chain or hardware
needs keeps its **exact** offset, GUID and name; `flashinfo` (was p33)
is preserved and renumbered p28.

| part | name | start (sector) | size (sector) | note |
|---|---|---|---|---|
| p1 | `recovery` | 64 | 32768 | TWRP — bootable (LK on `boot-recovery`) |
| p2 | `para` | 32832 | 1024 | boot-mode selector (still used: TWRP vs NixOS) |
| p3..p26 | `expdb frp nvcfg nvdata metadata protect1 protect2 seccfg oemkeystore proinfo md1img md1dsp md1arm7 md3img scp1 scp2 nvram lk lk2` (p22 `boot`), `logo tee1 tee2 keystore` | (unchanged) | (unchanged) | all boot-critical/hardware — every offset/GUID/name preserved; p22 `boot` at 362496 |
| p27 | **`linux`** | **458752** | **121651167** | **NixOS rootfs — the only writable system partition** (p26 `keystore` ends 458751; p27 ends 122109918) |
| p28 | `flashinfo` | 122109919 | 32768 | preserved (was p33); ends at `last-lba` 122142686 |

The exact table lives in `stock-dump/repartition-20260910/gpt-new.txt`
(sgdisk reference) with `gpt-primary-new.bin` / `gpt-backup-new.bin`.
The new layout is contiguous: `linux` 458752..122109918 then
`flashinfo` 122109919..122142686, immediately before the 33-sector backup
GPT (122142687..122142719).

### 12.2 Tooling (new: `bin/repartition-nixos.sh`)

| verb | what |
|---|---|
| `plan` | print the layout + verify the host GPT blobs (`sgdisk --verify`) — no device I/O |
| `backup` | converge to TWRP, pull the live GPT + `recovery para proinfo nvram lk lk2 boot` into `stock-dump/repartition-20260910/` |
| `apply --yes` | **DESTRUCTIVE**: unmount TWRP's data/cache, write + **byte-verify** the new GPT, stream `system.img` to the raw partition offset `0xE000000` (the ~8 GB image no longer fits TWRP's ~1.9 GiB `/tmp`), flash `boot.img`, verify. Leaves para sticky (TWRP), does **not** boot |
| `verify` | full read-back md5 of the rootfs region + boot.img |
| `boot` | clear para + reboot → NixOS |

`converge_twrp` in this script is state-aware (linux→para+WDT EXRST,
android→`boot-switch.sh twrp`, POC→press power), so `apply` works from
any state. It streams with `dd of=<raw disk> seek=224 conv=fsync`, so it
does not depend on the kernel re-reading the GPT mid-session.

### 12.3 What was run + receipts (2026-09-10)

```
nix build .#packages.aarch64-linux.default        # fresh rootfs + boot.img
bash bin/repartition-nixos.sh plan                # GPT blobs sgdisk-verified
bash bin/repartition-nixos.sh backup              # live GPT + boot-critical parts
bash bin/repartition-nixos.sh apply --yes         # write GPT, stream rootfs, flash boot
bash bin/repartition-nixos.sh verify              # full 7.94 GB read-back md5
bash bin/repartition-nixos.sh boot                # para clear + reboot -> NixOS
```

- `boot.img` sha256 `0b176d934de6e97bda3b9089b9dda30ed105f0a0ca3748497e7ef725e1e7c0c6`
  (md5 `f6881750b7c115e2a0f8eb5e4fe1bf17`, 9986048 bytes; GNOME-default build).
- `system.img` sha256 `ccd15c492dac7b69df317cb7d8af1922c5209cc2bda127d787dfae69d83630da`
  (7940786782 bytes; ext4 `NIXOS_SYSTEM`, mke2fs geometry, grows on first boot).
  ⚠️ this first clean-install image embedded **uid 1000** store files
  (no WiFi — §12.6); rebuilt the same day from the fixed shim with
  sha256 `db1d85e2d9d49f27572f5d59b958143fa66ab08fc3a52595e1d9b56e71fc5477`
  (root-owned; `boot.img` unchanged, sha256 `0b176d93…`).
- new GPT: primary sha256 `cbdd72fc6603a826e307ca9eff9bf91d5e859a4234552c5f21f47ba26bd9b6f2`,
  backup sha256 `ce27b641ff14309368f91fca787bd637e68c45ead9b220793c0425941c7575b3`
  (`gpt-new.txt` sha256 `540496ca1fcff84f261bf9b51473c85f5274d275d370608b4210d5222fcce5a7`).
- the on-device `linux` fs auto-grew to fill the partition (systemd-growfs)
  → **58.0 GiB** usable; see the session-log entry for the on-glass check.

### 12.4 Rollback / disaster recovery

The pre-repartition GPT is in `stock-dump/repartition-20260910/`:
`gpt-primary-live.bin` + `gpt-backup-live.bin` (and the byte-identical
`gpt-{primary,backup}-current.bin` pulled from NixOS before the
TWRP cycle)
plus `recovery.bin para.bin proinfo.bin nvram.bin lk.bin lk2.bin boot.bin`.
Restore the GPT from TWRP with `dd` (34 sectors at 0; 33 sectors at
`last-lba-32`) or via mtkclient. **The old Android/Debian data is
unrecoverable and was intentionally discarded** (user confirmed
no user data was needed) — this is a one-way repartition.

### 12.5 Consequences (historical parts of this doc)

- §5/§6 (dual-boot para selector + Debian handoff): the initrd code is
  still present and harmless (para is only ever zeros or `boot-recovery`,
  which the initrd treats as the NixOS default), but there is no Debian
  target any more.
- `bin/flash-nixos.sh`: `rootfs` now streams to `by-name/linux` (was
  `userdata`); the `debian` verb + `twrp_para` helper are removed.
- `bin/boot-switch.sh`: `debian` verb removed; `android` is now just
  "clear para → boot NixOS".
- `devices/planet-geminipda/default.nix`:
  `system_partition_destination = "linux"` (was `"userdata"`).
- DR inventory/playbook updated: `docs/disaster-recovery/inventory.md`
  (new partition layout) + `docs/disaster-recovery/README.md`.

### 12.6 Follow-up bug found by the clean install (2026-09-10)

The fresh image booted with **no WiFi** (`wlan0` unmanaged, GNOME wifi
menu empty) and `logrotate` failing. Both were the same root cause: the
whole `/nix/store` in the image was owned `1000:100`, and
NetworkManager/logrotate refuse files not owned by root. The image
builder shim (`pkgs/make-ext4fs-shim.nix`, R13) runs `mke2fs -d`, which
copies the source uid/gid verbatim from the uid-1000 build sandbox. Fixed
by re-execing the shim under `fakeroot` + `chown -R 0:0` before mke2fs
(details + receipts: session log 2026-09-10o). The running device was
repaired in place with a one-time store chown; future images are correct.

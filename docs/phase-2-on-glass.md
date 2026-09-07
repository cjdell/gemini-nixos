# Phase-2 on glass — first NixOS boot (2026-09-07) — knowledge + open TODO

> **Status:** ✅ **MILESTONE ACHIEVED 2026-09-07** — SSH to a NixOS shell on
> hardware over g_ether (the phase-2 exit criterion's first half). The
> booted system is NOT yet fully working: the TODO list at the bottom is
> the on-glass debt. This doc is the knowledge capture from the milestone
> session; the dated play-by-play lives in `docs/session-log.md`.
> **Last updated:** 2026-09-07.

## 1. What boots (version lines — rule 0)

- **p22 `boot`**: `boot.img` sha256
  `3965955f91162467d17e8659c103ac67ee4c79a4950bed38ea3cd456ffc364fb`
  (the FIXED image: dual-boot initrd + #329 kernel + `bootopt` in the
  cmdline field). Kernel payload sha `3a2a7f3a…822` (#329,
  `6.6.0-00048-g188aade698dd`).
- **p32 `userdata`**: `system.img` sha256
  `091707d716767b31835b821ac6f723b7fb5c4fb8b8058775903b163134b3c056`
  (flashed 14:49, md5-verified), ext4 label `NIXOS_SYSTEM`. Booted
  generation = **`/nix/store/yl6hkkihvk97p3jrafi72anhjf2dzav5-nixos-system-gemini-26.11pre1031299.0bb7ec54c848`**
  == the generation embedded in `result/system.img`'s registration.
  [corrected 2026-09-07: an earlier session check read "dcfv0nsj" from a
  `nix path-info .#toplevel` eval — the image's embedded gen is
  `yl6hkkih`; trust `result/system.img`'s registration file, not a
  separate `path-info` eval.]
- **p29 `linux`**: GeminiPDA Debian 13 rootfs — untouched, still bootable
  via the `boot-debian` para marker.
- First boot completed: store re-hydrated (`/nix-path-registration`
  removed), stage-2 systemd up, sshd answering as `root@10.15.19.82`
  (`hostname gemini`, kernel #329, 3.6 GiB RAM seen).

## 2. The milestone knowledge (the hard-won bits)

### 2a. THE bootopt requirement (the boot-hang cause — read this first)

**Symptom:** a boot.img with the correct #329 kernel field + a valid
minimal initramfs hung on the LK logo ~15 s, then LK-WDT boot-looped.
The LCD never showed a single kernel line (no fbcon text), the preloader
enumerated briefly each cycle, and pstore was empty (death precedes
ramoops/fb registration).

**Root cause:** the **boot.img cmdline field must carry
`bootopt=64S3,32N2,64N2`** (with `log_buf_len=4M`). LK itself parses the
field via `platform_parse_bootopt(boot_hdr->cmdline)`
(`platform/mt6797/load_image.c:839`) BEFORE handing off to the kernel.
Without `bootopt=`, LK does not configure whatever bootopt configures and
the early boot hangs before the kernel console appears. The docs' claim
that the boot.img cmdline is "inert" was only true for the *kernel*
(`CONFIG_CMDLINE_FORCE=y`); **LK reads it first** ([corrected 2026-09-07]
in `docs/boot-process.md` §4).

**Bisection trail:** (1) our boot.img vs the known-good Debian image
differed only in header name/cmdline + ramdisk (kernel field + DTB
sha-identical, geometry identical); (2) re-packing our kernel + ramdisk
with the *old* image's header fields (`build/pack-boot-img-custom-ramdisk.py`,
`--reference` = the good image) booted Debian through OUR initrd's debian
branch — isolating the header; (3) `platform_parse_bootopt` in the LK
source identified the mechanism.

**Fix (landed):** `config/gemini.nix` `boot.kernelParams` now begins with
`bootopt=64S3,32N2,64N2 log_buf_len=4M` (mobile-nixos android system type
builds the boot.img field from `boot.kernelParams` —
`modules/system-types/android/default.nix:14`). The kernel ignores these
(CMDLINE_FORCE); they exist for LK.

### 2b. WDT / A72 disarm — the reboot trap (already known, re-hits every session)

With the A72 cluster having been brought up via `cl2-up.sh` at any point
in a boot, LK's WDT mode is left **disarmed** (`0x10007000 = 0`,
`wdt_disarm` key write) — the 0x48 expiry write to `0x10007004` never
fires, so WDT-EXRST reboots silently no-op. Fresh boots (A72s down) are
fine. **Recovery when mid-boot disarm blocks a reboot:**
`devmem 0x10007000 32 0x2200005D` (key | LK's 0x5D mode value) restores
the mode, then `devmem 0x10007004 32 0x48` arms — EXRST fires ~2 s later.
Receipts: legacy `GeminiPDA/docs/session-log.md` 2026-09-07 (1st/2nd).

### 2c. The boot loop escape hatch (proven repeatedly this session)

A boot-looping device (para cleared, image hangs, LCD logo-frozen) is
recovered in ~1 min via the preloader window: the loop itself provides
power-cycles; `sudo bash bin/run-mtk.sh w para stock-dump/para-boot-recovery.bin`
catches a window and restores TWRP-sticky. Then `run-mtk.sh reset`.

### 2d. run-mtk.sh python-version fragility (fixed)

Two `mtkclient-2.1.4.1` store paths can coexist (devshell evals on
python3.13 vs 3.14). The launcher's deps scan is hardcoded to
`lib/python3.13/site-packages`; `head -1` picked the python3.14 build →
empty PYTHONPATH deps → `ModuleNotFoundError: Cryptodome` intermittently.
Fixed: `pick_pkg()` selects the first candidate whose deps contain a real
python3.13 site-packages dir. [bin/run-mtk.sh hardened 2026-09-07]

### 2e. flash-nixos.sh rootfs push timeout (fixed)

`adb_q` (timeout 30) was used for the 1.5 GiB push → killed at 30 s.
Fixed with `adb_push` (timeout 900) + a `wc -c` completeness check before
the destructive dd (TWRP's busybox `stat` has no `-c`). [bin/flash-nixos.sh]

### 2f. Initrd review fixes that mattered (devices/planet-geminipda/initrd.nix)

- The initrd staging had **no `/tmp`** — `dd of=/tmp/para` would have
  failed and the `boot-debian` marker silently ignored (fixed with
  `mkdir -p /dev/pts /newroot /tmp`).
- `${…}` inside the nix `''` init string is nix interpolation — replaced
  with plain concatenation + `$(basename …)` (eval error, fixed).
- Marker contract verified byte-exact across all four writers
  (boot-switch, flash-nixos twrp_para + ssh inline, gemini-boot-debian)
  against the initrd's `cmp` reference.
- The Debian branch (A72 opt-in + fstab fix + `switch_root /sbin/init`)
  replicates `GeminiPDA/build/initramfs-6.6/init` verbatim.

### 2g. NixOS-boot observations (working as designed)

- fbcon verbose log on the LCD ✓ (identical to Debian's — same kernel).
- Our dual-boot initrd's DEBIAN branch booted Debian on glass (the P3
  test) — initrd proven on hardware.
- NixOS first boot: store re-hydration ran (`nix-path-registration`
  removed), systemd up, sshd on g_ether, `gemini-gpu-poweron` active,
  `gemini-battery-guard` active.

## 3. Device state left at milestone end

- **NixOS is RUNNING** on p32 (the milestone state; para cleared = it
  boots NixOS on every power-on).
- p29 Debian intact; bootable via `boot-debian` marker.
- Battery: charging on USB. A72s offline (fresh boot). WDT armed (fresh
  boot state).

## 4. TODO — things not quite working yet (2026-09-07)

Priorities P0 (blocking full use) → P3. Each gets a dated line when
worked.

- **[P0] growfs: `/` is 3.1 GiB, not 27.3 GiB.** Two stacked causes:
  1. **udev by-label coldplug race** — at boot, `/dev/disk/by-label/`
     was empty (udevd started before/racing the eMMC coldplug), so
     `systemd-growfs-root.service` could not resolve the fstab `/` device
     (`/dev/disk/by-label/NIXOS_SYSTEM`) and failed. A manual
     `udevadm trigger --subsystem-match=block && udevadm settle` created
     the links. Fix idea: a oneshot unit ordered
     `after=systemd-udevd before=systemd-growfs-root` that triggers udev;
     or make the initrd symlink survive switch_root; or point fstab at a
     non-udev path.
  2. **resize2fs EINVAL at group #25** — even with the link present,
     `resize2fs /dev/mmcblk0p32` (and systemd-growfs) fail: kernel
     `ext4_resize_fs:2189 error (-22)`; fs has `resize_inode`, journal
     active (`needs_recovery` — the fs is mounted). Investigate: image
     mkfs geometry (flex_bg? mke2fs defaults in the mobile-nixos image
     builder), offline-resize-before-flash option, or build the image
     with a much larger `extraPadding`. Until fixed: `/` fills at 3.1 GiB
     (1.5 GiB used) — NOT boot-blocking but space-limited.
- **[P1] nixos-rebuild round-trip on the device** (the phase-2 exit
  criterion's second half). Needs a story for sources + binary cache on
  glass (no stage-1 USB networking; the g_ether link + host NAT exist —
  `bin/usb-tether-nat.sh`).
- **[P1] Verify the Debian branch on the FINAL fixed boot.img** —
  boot-debian through `3965955f…` (P3 proved the branch with the old
  header + our ramdisk; the fixed image should behave identically —
  confirm once, then the dual-boot loop is closed).
- **[P2] systemd-vconsole-setup fails** — `setfont: Unable to find file:
  TER16x32` (that is an fbcon/kernel font name, not a kbd console font).
  Fix the console font/keymap config (`console.keyMap` is fine; the font
  setting is wrong).
- **[P2] gemini-audio-defaults fails (status=127, command not found)** —
  something in its ExecStart chain is missing on the NixOS rootfs.
- **[P2] gemini-wifi-internal fails** — `modprobe mtk_wcn failed`; check
  dmesg (module present in the borrowed tree? firmware ordering?).
- **[P2] gemini-a72-up failed at boot** — decide: the Debian handoff
  deliberately DISABLES the boot-time A72 bring-up (wedge risk, opt-in);
  NixOS's boot service should probably match (opt-in from a settled
  system), not run at boot.
- **[P2] gemini-wifi-auto / other auto units** — expected no-op without
  profiles; confirm quiet.
- **[P3] Serial ttyS0 capture of one full boot** for the record (UART0 @
  0x11002000, 921600 8N1) — the only full-console view (fbcon starts
  late).
- **[P3] e2fsck health check of p32** — TWRP auto-mounted userdata
  (`/data`) rw across many recovery sessions after the flash; confirm no
  fs damage (growfs EINVAL may relate).
- **[P3] Gen-hash bookkeeping** — booted gen `yl6hkkih` ==
  `result/system.img` registration (verified twice). Add a one-liner to
  the flash tooling so the flashed gen is always recorded from the image,
  not from a parallel `path-info` eval.
- **[P3] Commit the session's changes** (14 modified + 2 intent-to-add
  files — see `git status`; includes config/gemini.nix bootopt, the
  dual-boot initrd, flash-nixos/boot-switch/run-mtk fixes, this doc).

## 5. Corrections made to existing docs this session

- `docs/boot-process.md` §4 row 2 + §7 — the boot.img cmdline field is
  NOT inert: LK consumes `bootopt` (see 2a); size figures corrected to
  the current build ([corrected 2026-09-07]).
- `docs/session-log.md` — the 2026-09-07 prep entry's "next action"
  superseded by this milestone.
- `docs/disaster-recovery/inventory.md` — 4 MiB boot areas measured live
  (earlier session).

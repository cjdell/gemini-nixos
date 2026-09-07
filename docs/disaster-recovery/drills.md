# DR drills — restore procedures (Level 1: preloader intact; Level 2: total erase)

> **Status:** Level 1 = ✅ procedure with PROVEN commands (preloader-mode
> mtkclient write path verified idempotent 2026-08-30 on `para`; reads on
> many partitions). Level 2 = 🔴 **skeleton — the BROM-mode leg is
> UNVERIFIED on this unit** (gather.md step 5) and every command there
> must be rehearsed read-only before it is ever needed for real.
> **Last updated:** 2026-09-07
> Read `README.md` (rules, failure levels) and `inventory.md` (images +
> shas) first. Tooling: `bin/run-mtk.sh` (patched-mtkclient launcher,
> needs sudo), `bin/boot-switch.sh` / `bin/flash-nixos.sh` (TWRP paths).
> Legacy receipt references (boot-chain/hardware details not yet ported,
> M1): `GeminiPDA/docs/…`.

## Pre-flight (both levels)

1. Battery ≥3.8 V and preferably charging (brown-outs killed recoveries
   before). In TWRP read `dmesg | grep -i "PE+\]Ibat"`; in preloader
   mode there is no battery read — just charge first.
2. Check tooling: devshell built (`nix develop --command true`),
   `sudo bash bin/run-mtk.sh printgpt` handshake works. VID watch via
   `bin/usb-watch.sh`.
3. Know which images you will write and their shas (inventory.md) —
   never write an unidentified image (rule 0).
4. Verify para state and boot-partition contents BEFORE anything:
   `bin/boot-switch.sh status`.
5. Use the **patched mtkclient** via `bin/run-mtk.sh` for everything
   below (the stock nixpkgs client never sees this preloader — CDC-ACM
   devclass=2). SP Flash Tool (Windows) is the reference alternative for
   the Level 2 write; same scatter + images.

---

## Level 1 — GPT/user partitions wiped, preloader (BOOT1) alive

Device symptom: every power-on with USB attached enumerates `0e8d:2000`
preloader mode for ~9 s, then silence (no LK/TWRP/POC) — or a GPT-less
boot loop.

1. **Talk to it (read-only).** Start `sudo bash bin/run-mtk.sh
   printgpt`, power the device on. Confirm the DA session and that the
   partition table it reports (or its absence) matches the damage. If
   `printgpt` shows a healthy GPT, you are actually at Level 0 — skip to
   "post-restore".
2. **Restore the boot chain in order** (rule 4 — every step leaves a
   bootable-or-brick-safe state; the last two make TWRP the default):
   ```sh
   # LK (must exist before anything can boot from user area)
   sudo bash bin/run-mtk.sh w lk  stock-dump/lk.bin
   sudo bash bin/run-mtk.sh w lk2 stock-dump/lk2-20260907.bin   # after gather.md step 4
   # TWRP into p1 recovery (16 MiB part; LK reads sizes from the bootimg
   # header, so the unpadded twrp-noswipe.img is fine)
   sudo bash bin/run-mtk.sh w recovery stock-dump/twrp-noswipe.img
   # para = full readback of the sticky boot-recovery state (preserves
   # the LK env window at 0x20000 — better than a hand-written 32-byte
   # command)
   sudo bash bin/run-mtk.sh w para stock-dump/para-boot-recovery.bin
   ```
   If `w <name>` refuses because the device GPT is gone (no partition
   list), the DA can still address by the scatter (Level 2 section) — or
   rebuild the GPT first from the Level 2 step 2.
3. **Verify on glass.** Power-cycle: LK should show the boot logo, then
   TWRP (sticky, because para now says `boot-recovery`). Confirm with
   `adb devices` → `18d1:4ee2` (TWRP adbd, root) and eyeball the LCD
   (AGENTS rule 5b — TWRP drives the panel correctly; the flicker rule
   applies to kernels, not TWRP).
4. **Post-restore.** From TWRP: flash p22 `boot` with the NixOS image or
   the stock `boot.bin` (TWRP dd path below) — do NOT clear para until
   you have verified the boot image boots (that is the normal
   `bin/flash-nixos.sh boot` job now). Optionally restore
   logo/seccfg/tee/protect/nvram readbacks for full identity.

## Level 2 — everything erased, incl. eMMC BOOT1 (preloader)

Device symptom: only BROM `0e8d:0003` on power-on with USB, or nothing
at all. The SoC BootROM is in-chip and cannot be erased — it is the
entry. **Prerequisites: preloader dump (or the stock zip preloader),
raw GPT image, scatter file, and a validated BROM entry — see gather.md
steps 2/3/7/5. Do not start Level 2 without all four.**

1. **Enter BROM mode** (gather.md step 5 combos) and confirm `0e8d:0003`
   on the host.
2. **Upload the DA.** mtkclient in BROM mode handshakes and auto-selects
   a DA (`MTK_DA_V5.bin` for this unit); when DRAM init needs a
   preloader it uses the bundled `Loader/Preloader/` or an explicit
   `--preloader` — supply our dumped preloader:
   ```sh
   sudo bash bin/run-mtk.sh --preloader stock-dump/preloader-20260907.bin printgpt   # read-only rehearsal
   ```
   🔴 **Unverified:** the exact BROM-mode flags for this unit must be
   validated in a read-only session (gather step 5) while the device is
   still healthy. Until then treat this command as a placeholder and use
   SP Flash Tool (below) as the reference.
3. **Write in order** (each step must verify before the next):
   1. **preloader → eMMC BOOT1** (scatter `EMMC_BOOT_1` entry /
      mtkclient `w boot0`-equivalent; file: `stock-dump/preloader-20260907.bin`
      or zip `preloader_k97v1_64_bsp.bin`, sha aec11712…).
   2. **GPT** — byte-exact restore from `gpt-primary-20260907.img` +
      `gpt-backup-20260907.img` (or let the tool rebuild from the
      scatter, then cross-check offsets against `gpt-2026-08-30.txt`).
   3. Continue with the Level 1 step 2 sequence (lk, lk2, recovery, para)
      — from this point the proven preloader-mode path takes over.
   SP Flash Tool reference: load the scatter, select the images from
   `stock-dump/`, **uncheck nothing you do not mean to write**, and use
   "Download" (NOT "Format all + Download" — that is what erased the
   device; a full-erase restore with our images is equivalent but
   deliberate). nvram: if it was wiped, restore `stock-dump/nvram.bin`
   (IMEI) explicitly and verify IMEI afterwards.
4. **Verify on glass** — same as Level 1 step 3: LK logo → TWRP.
5. **Post-restore** — same as Level 1 step 4.

## Level 0 — only p22 `boot` is bad (common case)

You are never more than one bad boot image from this. If para is already
cleared and `boot` hangs: hold power + side button ~10 s (RTC FAC_RESET)
→ TWRP regardless of para → flash a good `boot` from TWRP. If that combo
fails, the preloader window on next power-on is your channel (`w boot`
with a known-good image).

## TWRP dd path (from TWRP's root adbd)

```sh
nix develop --command adb shell \
  "dd if=/tmp/boot.img of=/dev/block/platform/mtk-msdc.0/11230000.msdc0/by-name/boot bs=1M conv=fsync"
```
(`by-name/{boot,recovery,para,linux,…}`; push the image to /tmp first.)

## DANGER list

- **Never `mtk f` / SPFT "Format all"** without the full inventory set
  verified — that is the exact scenario this folder exists to recover
  from, and Level 2's BROM leg is still unverified.
- **Never write `nvram`/`proinfo`/`protect1`/`protect2`** with anything
  but the exact readbacks; `nvram.bin` = IMEI (private).
- **Never write eMMC BOOT1 while the device boots fine.** A bad preloader
  write is the one damage with no preloader-mode rescue (only BROM).
- **Never leave para cleared with an unverified `boot`** — that
  combination is how a unit ends up with "no software way back".
- Verify every readback (rule 6) and log a version line per write
  (rule 0) in `docs/session-log.md`.

## After TWRP is back

- NixOS: `bin/flash-nixos.sh status|boot|rootfs|boot-nixos` (its safety
  model: keep para = `boot-recovery` until the image is verified on
  glass).
- Stock/Android or Gemian boot image: `bin/boot-switch.sh flash|restore`.
- Log the full session: images written (sha), order, battery state,
  outcome, glass state left behind.

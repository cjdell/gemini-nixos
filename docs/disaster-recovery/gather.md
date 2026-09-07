# DR gather — collect the missing artifacts from a working device

> **Status:** ⬜ Not yet executed — this is the action list for the next
> session that has the device on the bench. Device today: Debian on p29,
> kernel #329 in p22, TWRP (no-swipe) on p1+p31, para = cleared (normal
> boot) unless a later session changed it.
> **Last updated:** 2026-09-07
> Target: close every "missing" row of `inventory.md`, then flip this
> doc's status and record version lines in `docs/session-log.md`.

## Preconditions

- Device healthy and reachable (running Linux via g_ether SSH, or boot it
  to TWRP via para `boot-recovery` + WDT EXRST — `bin/boot-switch.sh
  twrp`).
- Battery ≥3.8 V (TWRP dmesg `[PE+]Ibat=..`; flat units brown out).
- Host tooling ready: devshell built once (`nix develop --command true`
  — provides the `mtkclient` store closure), patched copy at
  `/usr/local/lib/mtkclient-patched`, `bin/run-mtk.sh` present.
- USB cable in the port that gives preloader enumeration on power-on
  (left port, the g_ether one).
- Record the device state + para command first (`bin/boot-switch.sh
  status`).

## File conventions

- Destination: `stock-dump/` in THIS repo (gitignored — never commit).
- Naming: `<partition>-<YYYYMMDD>.bin` (e.g. `preloader-20260907.bin`,
  `gpt-primary-20260907.img`, `lk2-20260907.bin`).
- After every dump: `sha256sum`, compare against any known twin, log the
  version line. Update `inventory.md` + this doc's status.

---

## Step 1 — measure the eMMC boot areas (resolves the 2 vs 4 MiB question)

From the running Linux (kernel #329, root = p29):

```sh
ROOTDEV=$(findmnt -no SOURCE /)              # e.g. /dev/mmcblk0p29 (or mmcblk1p29!)
EMMC=${ROOTDEV%p*}; echo "whole eMMC: $EMMC"
for b in "${EMMC}boot0" "${EMMC}boot1"; do
  echo "$b: $(( $(cat /sys/class/block/${b##*/}/size) * 512 )) bytes"
done
# confirm identity by CID, never by name alone (host numbering shifts):
cat /sys/class/mmc_host/*/mmc*/cid          # expect 450100444634303634014e9c213ca4ad
```

⚠️ The eMMC is `mmcblk0` in some builds and `mmcblk1` in others
(legacy boot-chain.md 2026-08-31 note) — **detect by CID or by walking
up from the mounted root device**, never hardcode.

Expected: DA log says 0x400000 (4 MiB) per area; the legacy hardware.md
says 2 MiB. Whichever it is, record the number here and correct the
claim in this repo's hardware notes (M1).

## Step 2 — preloader dump (BOOT1 area) ← THE critical one

Two independent channels; **both must agree** (rule 7).

**Channel A — DA readback** (the same channel a Level-2 restore uses):

```sh
# device must be OFF; start the launcher, then power on (preloader sits
# in download mode ~9 s). If already booted, reboot it while it waits.
sudo bash bin/run-mtk.sh r boot0 stock-dump/preloader-20260907.bin
```

mtkclient's `boot0` partition is the preloader region on eMMC BOOT1
(named as such in the legacy disassembly notes). If the name is rejected
(partition list empty), fall back to Channel B and/or re-check with
`printgpt` + `--debugmode`.

**Channel B — raw dd from a root shell** (running Linux or TWRP):

```sh
# running Linux:
EMMC=$(findmnt -no SOURCE /); EMMC=${EMMC%p*}
SIZE=$(( $(cat /sys/class/block/${EMMC##*/}boot0/size) * 512 ))
dd if=${EMMC}boot0 of=stock-dump/boot1-area-20260907.bin bs=1M
# TWRP (adb root shell): /dev/block/mmcblk0boot0 — verify the node exists first
```

**Verify:** sizes equal; sha256 of A == sha256 of B; then compare against
the stock zip's preloader:

```sh
sha256sum stock-dump/preloader-20260907.bin
# stock reference (zip still in legacy stock-dump until M3):
#   unzip -p /home/cjdell/Projects/GeminiPDA/stock-dump/gemini_WIFI_base.zip \
#     Gemini_WIFI_17052019/preloader_k97v1_64_bsp.bin | sha256sum
#   = aec1171212818a14172ac8bed6c0985c685e0d66e3fa573a5b65fa672236adae
```

If they match → the stock zip already covers Level 2's preloader
requirement; keep both files anyway (device dump is authoritative).
Also dd the **BOOT2 area** the same way (`…boot1`,
`boot2-area-…bin`) — cheap insurance, rarely needed.

## Step 3 — raw GPT binaries

The text map (`stock-dump/gpt-2026-08-30.txt`) is not byte-restorable
on its own.

```sh
EMMC=$(findmnt -no SOURCE /); EMMC=${EMMC%p*}
SECT=$(( $(cat /sys/class/block/${EMMC##*/}/size) ))
# primary GPT: protective MBR + header + 33 entries (use 64 sectors of slack)
dd if=$EMMC of=stock-dump/gpt-primary-20260907.img bs=512 count=64
# backup GPT: last 34 sectors
dd if=$EMMC of=stock-dump/gpt-backup-20260907.img bs=512 skip=$((SECT-34))
```

Verify: `fdisk -l stock-dump/gpt-primary-20260907.img` (devshell `fdisk`)
shows partitions matching the GPT text dump offsets; sha-record.

## Step 4 — missing partition dumps (lk2, scp1, scp2, cache)

Easiest from TWRP adb (root, by-name) — device currently boots, so:
`bin/boot-switch.sh twrp` → then:

```sh
for p in lk2 scp1 scp2 cache; do
  nix develop --command adb exec-out dd if=/dev/block/platform/mtk-msdc.0/11230000.msdc0/by-name/$p \
    of=stock-dump/$p-20260907.bin 2>/dev/null   # or adb pull after dd to /sdcard
done
```

…or read-only DA readbacks: `sudo bash bin/run-mtk.sh r lk2 …` (same for
each; sizes: lk2 0.5 MiB, scp1/scp2 1 MiB, cache 432 MiB — cache is
optional, Android's; skip if slow). Not boot-gating for TWRP, but a
full-erase restore wants them.

## Step 5 — BROM-mode entry experiment (read-only; validates Level 2)

Level 2 (BOOT1 erased) recovery needs the SoC **BootROM** USB download
mode (`0e8d:0003`) — never proven on this unit. Test while healthy:

1. Device powered OFF, USB host attached. `sudo bash bin/usb-watch.sh`
   running (prints VID transitions with timestamps).
2. Try each entry, one per attempt, ~10 s apart, without writing
   anything: (a) hold **vol-up** (silver side button per DCT notes — the
   KROW0/KCOL0 cell is the preloader download key, so vol-up may select
   preloader mode instead — try all); (b) **vol-down**; (c) **both side
   buttons**; (d) **power + vol-up**.
3. Watch for `0e8d:0003` (BROM) vs `0e8d:2000` (preloader). If BROM
   appears: run `sudo bash bin/run-mtk.sh printgpt` once read-only to
   prove the DA handshake, then reset out. If only preloader ever
   appears, BROM may need a **testpoint** (mainboard short) — note it;
   Level 2 then hinges on the preloader dump (step 2) staying safe.
4. Log which combo (if any) produced BROM in docs/session-log.md.

## Step 6 — fresh identity snapshot of the live boot-critical set

While the device is up, read back and sha what p1/p2/p20/p22 hold TODAY
(TWRP dd or DA readbacks) and compare against the ledger:

- p1 recovery vs `recovery.bin` (c04b70e0…) vs `twrp-noswipe.img`
  (da7d957e…) → answers which TWRP is actually installed.
- p20 lk vs `lk.bin` (75ec9f0b…) — expect equal (stock LK).
- p22 boot vs the newest `boot-*.img` → confirms which kernel backup is
  the current one (update the README/session-log version line).
- p2 para vs `para.bin`/`para-boot-recovery.bin` — expect the 32-byte
  command + env to match one of them.

## Step 7 — scatter file (SPFT format)

The restore reference implementation (SP Flash Tool, and mtkclient's
scatter mode) needs an MT6797 scatter for THIS layout. Sources, in order:

1. Planet's SPFT package (support.planetcom.co.uk — the `Gemini_WIFI`
   download page also carries the SP Flash Tool bundle). The zip in
   legacy stock-dump lacks the scatter — the SPFT bundle usually ships
   `MT6797_Android_scatter.txt`.
2. Generate from the byte-exact GPT text dump (offsets are known; format:
   `partition_index`, `linear_start_addr`, `partition_size`, `type =
   EMMC_BOOT_1` for preloader, …). Validate the generated file by
   loading it in mtkclient read-only against the live `printgpt` before
   trusting it.
3. Record which route worked + the sha of the scatter in inventory.md.

## Step 8 — close the loop

- Update `inventory.md` (files + shas + copy status), flip `gather.md`
  status to ✅ when every step is done, and correct the boot-area figure
  if step 1 contradicts the legacy claim.
- Append the dated entry to `docs/session-log.md` (template in that
  file) with all version lines.
- Optionally promote this checklist's device-side commands into
  `bin/gather-dr.sh` the first time they are run (AGENTS rule 6).

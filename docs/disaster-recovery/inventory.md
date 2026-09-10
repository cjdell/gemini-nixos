# DR inventory — what is archived, where, and what is missing

> **Status:** Audit done 2026-09-07 (all sha256 computed that day from
> the on-disk files in the legacy `GeminiPDA/stock-dump/`, before the
> boot-critical set was copied into THIS repo's `stock-dump/` — hashes
> are identical across both copies, re-verify after any copy (rule 7)).
> Both `stock-dump/` dirs are gitignored — nvram/protect/tee hold
> private/identity data; **never `git add` them**. This repo is the
> golden ledger; the legacy repo is the bulk archive until migration M3.
> **Last updated:** 2026-09-10

## Device / eMMC identity (DA session 2026-08-30; `stock-dump/gpt-2026-08-30.txt`)

- SoC: MT6797/MT6767 Helio X23/X25/X27; HW code 0x279, subcode 0x8a00.
- eMMC: CID `450100444634303634014e9c213ca4ad` (ID DF4064).
  DA-reported sizes: **Boot1/Boot2 0x400000 (4 MiB)**, RPMB 0x400000,
  USER `0xe8f800000` (≈59.5 GiB).
- Security: **SBC/SLA/DAA all False, root cert not required → the unit is
  SECURITY-UNLOCKED** — no signature barriers on any image we write
  (verified in the DA log; this is why a stock-zip preloader/LK restore
  is legal).
- ⚠️ **Open question 2026-09-07 → resolved 2026-09-07 (live measure):**
  the legacy hardware.md says the mmcblk boot areas are "2 MiB each;
  rpmb 2 MiB" — the DA log says 4 MiB. Measured on the live unit
  (kernel #329, g_ether ssh): `mmcblk0boot0` and `mmcblk0boot1` =
  **4 MiB each** (`/sys/class/block/…/size` = 8192 sectors) — agrees
  with the DA log (0x400000); legacy 2 MiB claim superseded
  (port-fix when M1 lands).

## Partition map and dump status (GPT: `stock-dump/gpt-2026-08-30.txt`,
sha `7b4e3615…`)

Byte offsets are partition starts from the GPT dump. **Copy status**
column: `here` = file present in THIS repo's `stock-dump/` (copied
2026-09-07) · `legacy` = still only in `GeminiPDA/stock-dump/` (M3
pending — rsync command in the footer).

| p# | name | size | start (hex) | dump file | copy | boot-crit |
|---|---|---|---|---|---|---|
| p1 | recovery | 16 MiB | 0x0008000 | `recovery.bin` (orig TWRP readback 08-30, = `boot3.bin`) · `twrp-noswipe.img` (patched, flashed p1+p31 08-31) | here | ✅ |
| p2 | para | 0.5 MiB | 0x1008000 | `para.bin` (readback 08-30) · `para-boot-recovery.bin` (sticky boot-recovery state, 08-30) | here | ✅ |
| p3 | expdb | 10 MiB | 0x1088000 | `android/expdb.img` | legacy | |
| p4 | frp | 1 MiB | 0x1a88000 | `android/frp.img` | legacy | |
| p5 | nvcfg | 8 MiB | 0x1b88000 | `android/nvcfg.img` | legacy | |
| p6 | nvdata | 32 MiB | 0x2388000 | `android/nvdata.img` | legacy | |
| p7 | metadata | 32 MiB | 0x4388000 | `android/metadata.img` | legacy | |
| p8 | protect1 | 8 MiB | 0x6388000 | `android/protect1.img` | legacy | (calibration) |
| p9 | protect2 | 12.5 MiB | 0x6b88000 | `android/protect2.img` | legacy | (calibration) |
| p10 | seccfg | 8 MiB | 0x7800000 | `android/seccfg.img` | legacy | ⚠️ security state |
| p11 | oemkeystore | 2 MiB | 0x8000000 | `android/oemkeystore.img` | legacy | |
| p12 | proinfo | 3 MiB | 0x8200000 | `proinfo.bin` (readback) | here | ⚠️ boot-mode flags |
| p13 | md1img | 24 MiB | 0x8500000 | `android/md1img.img` | legacy | modem |
| p14 | md1dsp | 4 MiB | 0x9d00000 | `android/md1dsp.img` | legacy | modem |
| p15 | md1arm7 | 3 MiB | 0xa100000 | `android/md1arm7.img` | legacy | modem |
| p16 | md3img | 5 MiB | 0xa400000 | `android/md3img.img` | legacy | modem |
| p17 | scp1 | 1 MiB | 0xa900000 | ❌ **missing** | — | SCP fw |
| p18 | scp2 | 1 MiB | 0xaa00000 | ❌ **missing** | — | SCP fw |
| p19 | nvram | 5 MiB | 0xab00000 | `nvram.bin` (readback 08-30; **IMEI — private**) | here | ⚠️ identity |
| p20 | lk | 0.5 MiB | 0xb000000 | `lk.bin` (readback) — content = stock `lk_s.img` (verified) | here | ✅ |
| p21 | lk2 | 0.5 MiB | 0xb080000 | ❌ **missing** | — | (backup LK slot) |
| p22 | boot | 16 MiB | 0xb100000 | `boot.bin` (stock Android) + `boot-327-20260906.img`, `boot-20260907-013541.img` (current pre-#329) copied; `boot-20260910-134318.img` = the last nested-desktop boot (pre-GNOME-KMS, rollback point); **~85 further dated `boot-*.img`** | here (sel.) / legacy (bulk) | ✅ |
| p23 | logo | 8 MiB | 0xc100000 | `logo.bin` (readback) | here | (boot logo) |
| p24 | tee1 | 5 MiB | 0xc900000 | `android/tee1.img` | legacy | |
| p25 | tee2 | 5 MiB | 0xce00000 | `android/tee2.img` | legacy | |
| p26 | keystore | 13 MiB | 0xd300000 | `android/keystore.img` | legacy | |
| p27 | system | 2.5 GiB | 0xe000000 | `android/system.img` | legacy | Android |
| p28 | cache | 432 MiB | 0xae000000 | ❌ (stock zip has `cache.img`) | legacy zip | |
| p29 | linux | 27.7 GiB | 0xc9000000 | **live rootfs** (Debian 13, kernel #329) — not archived as image | — | |
| p30 | boot2 | 16 MiB | 0x7b770000 | `boot2.bin` (readback; Gemian ref slot) | here | |
| p31 | boot3 | 16 MiB | 0x7b870000 | `boot3.bin` (readback 08-30, = orig TWRP) | here | |
| p32 | userdata | 27.3 GiB | 0x7b970000 | ❌ not dumped — FDE dm-crypt ciphertext, useless as restore | — | |
| p33 | flashinfo | 16 MiB | 0xe8e7fbe00 | `android/flashinfo.img` | legacy | |

Outside the GPT (eMMC hardware areas):
- **preloader** in **BOOT1** (`mmcblk?boot0`): ❌ **MISSING — the critical
  gap.** Only proxy: the stock firmware zip's `preloader_k97v1_64_bsp.bin`
  (zip still in legacy stock-dump; sha below). Gather: `gather.md` step 2.
- BOOT2 area: ❌ not dumped (rarely needed; dump anyway — cheap).
- RPMB: not dumpable (authenticated) — but on an SBC-off device it does
  not gate booting; the one un-backupable region.

## sha256 ledger (computed 2026-09-07; boot-critical set verified after
the copy into this repo — re-run `sha256sum -c` against this list)

Boot-critical set (all copied `here` unless noted):

```
75ec9f0ba97af9e68d964b304e0de809f9b4546982570bd16b2e7fe88823282c  lk.bin
2417a1adbb952c40e62bac5871f838eccef2e0d95d5e0b99dbb7db6763fff3c3  para.bin
6a342a0ca6dda1e03538b9b1d5d9b4044bb1280114e0d5fe354763a0071acfa6  para-boot-recovery.bin
c04b70e0dbb44fd6ad49e63b2d8a26671b9e0a83b8624507e2954f0e41801724  recovery.bin          (== boot3.bin)
da7d957ede2cf9d56115bf06211abba81181f73f8db99f4422811216fa90bbdb  twrp-noswipe.img      (patched TWRP, on p1+p31)
d8c80417585be1062d90ff7387fc5fbe0a9de282eb8a2c5eeb5b89561f6b9c85  boot.bin              (stock Android boot)
1fa78de9f8744a6818bcef2f6773737939f84364de982413910d4958d6d21513  boot2.bin
185723a032c05b4863667fe4a8f601cd9e0940c3a2d6421522270bd8b21b99c9  logo.bin
2cbaf95fa5ba989f283e9ac6137f73d4a9245c9a04bf902519db1544ad6c6b43  proinfo.bin
c1d5d87386eaf2115f7b475dd742ad6a88f077cc4c1a676826cabbbc64fda179  nvram.bin             (IMEI — private)
66c9e523c9a1a53e7bb4ea7e83c794203f2f8c6522ee61e927258873000ac27a  boot-327-20260906.img
28ae68008155cf26e99ab07ea1190217b70d9cb66d5a021ae9fe1a6c09fe6e97  boot-20260907-013541.img  (pre-#329 current)
959eded5d34cfeac8c0b8e95728f91948105558297c15946bb5641ef452e67e9  boot-20260910-134318.img  (last nested-desktop boot; KMS/GNOME rollback)
7b4e3615d7be37e4bb311dcbf72e751449d752b31906e1d09e904b5aae576c4b  gpt-2026-08-30.txt
```

Legacy-only (M3 pending — hash the copy after the rsync):

```
0b904b17712ec9743af75eceddb0b613a3150071f42cfcc3b1879fe60e293ad1  expdb.img        ed0e289572ab72697dec047a3db6b795497fd59251ac9e591c64da4308629292  frp.img
7fc6b4cf5897be044214f9522399fafb44a7e340d39d71198cdcefe1092493d8  nvcfg.img        057acb2c12e7a9167b4101677bdaa26fb19973d3cc8111ee2e0e55436081be92  nvdata.img
89b1370b5386a0522b0787659e160b589199049fe5916da48c7f5c5e4efa4939  metadata.img     9f8c9fa6fc07e7ffb53068837ab1556ff2e76fe3ccaa43671916e72fb2ab596b  protect1.img
190cf158a3241dc5ea3fc9ae617df6c89a6ddad2139fb51b949ffa07b8a52560  protect2.img     7b0c1652b10c04e036519908e53c519a9e43e6f72695b3905c7e7be0e0253409  seccfg.img
5647f05ec18958947d32874eeb788fa396a05d0bab7c1b71f112ceb7e9b31eee  oemkeystore.img  2cd154f332ee72edb6dee431a68eb5f8b98b4dc05ee14e56591cfbffcf81a9b3  tee1.img  (== tee2.img)
38ad4ad4d56a9f8e0a9aeb8670fa7acfe84939b39d7af9917a777ab71c96df30  keystore.img     f3d514f0cdcdfb2addbf70b9dbf9cbd0e41ea646eb342eb759bbc4f215f2d0cf  md1img.img
2b0d7322d0df0fabf957f38f215822eba3c134d75a30faae2965996dc4c42829  md1dsp.img       756070f69e02249f5478e978b03214eb17aa30adfec591cffe9af8748507ce95  md1arm7.img
dc0c3cbe5b2401919239af49ea6e0a610e5e7bea9bc155cf82a9655434e77750  md3img.img       de9309954b81ad91d1611f0b428d44bd1ecb81a4fb35bc645c5cd0b6013b0371  flashinfo.img
5f5777f8085abe9bda7c0e3bb5d967622e5b75cb723673a6fc5ed75703ccfbeb  system.img
```

## Stock firmware package (independent fallback — legacy stock-dump)

`GeminiPDA/stock-dump/gemini_WIFI_base.zip` (1,338,725,348 B, sha
`edd3ab26c24fd693d05372e0d72cdec78cfa78457b6526cc5ef8abc6e75588eb`),
Planet "Gemini_WIFI_17052019" release. Contents that matter for DR:

```
190856  preloader_k97v1_64_bsp.bin   sha aec1171212818a14172ac8bed6c0985c685e0d66e3fa573a5b65fa672236adae
432128  lk_s.img                     sha 399c07a0e1a246db4e0c9eeccbb8e74abe16201903755b13c19561e1ea999cb0
        boot.img  root_boot.img  recovery.img  twrp_recovery.img  logo_s.bin
        scp.img  secro.img  tee.img  cache.img  userdata.img  system.img
        md1img.img  md1dsp.img  md1arm7.img  md3img.img  ramdisk.img
```

- The zip's `lk_s.img` content is byte-identical to the device's dumped
  `lk.bin` (verified 2026-09-07 — only the 512 KiB partition padding
  differs) → the unit runs the stock 2019 LK generation; the stock
  `preloader_k97v1_64_bsp.bin` is the expected match for the unit's own
  preloader — **confirm by dumping (gather.md step 2) and sha-comparing**.
- The zip has **no scatter file** (SPFT package on support.planetcom.co.uk
  is the scatter source — `gather.md` step 7).

## Tooling (host)

| Tool | Where | Notes |
|---|---|---|
| patched mtkclient launcher | `bin/run-mtk.sh` (this repo, ported 2026-09-07) | CDC-ACM patches required for this preloader (legacy flashing.md); patched copy `/usr/local/lib/mtkclient-patched` |
| mtkclient pkg + DAs | devshell `flake.nix` → `mtkclient` (nixpkgs 2.1.4.1 — same version the patches were made against) | DAs in store `Loader/` (`MTK_DA_V5.bin` auto-selected, AllInOne, `Preloader/` dir for BROM-mode DRAM init) |
| USB state watcher | `bin/usb-watch.sh` (ported 2026-09-07) | boot classification, BROM-entry experiment |
| TWRP dd path | `/dev/block/platform/mtk-msdc.0/11230000.msdc0/by-name/{boot,recovery,para,…}` | root adbd, any-state flash |

## Missing — the gather list (details + commands in `gather.md`)

1. **Preloader (BOOT1 area) dump** — the only boot stage with no on-disk
   dump. ← most important
2. **Raw GPT binaries** (primary + backup) — only the text map exists.
3. **lk2** (p21), **scp1/scp2** (p17/p18), **cache** (p28) dumps.
4. **BOOT2 area** dump (raw, for completeness).
5. **BROM-mode entry verification** (read-only experiment) — Level 2
   depends on it.
6. **Scatter file** (SPFT format) for this exact GPT layout.
7. Fresh identity readback of p1/p20/p21/p22 (what is on the device
   *today*) + resolution of the boot-area-size open question.

## Open questions

- Boot-area size: 2 MiB (legacy hardware.md) vs 4 MiB (DA log) — measure.
- Does p1 currently hold `recovery.bin` or `twrp-noswipe.img`? (Flashing
  history says noswipe was flashed to p1+p31 on 2026-08-31; `recovery.bin`
  is the 08-30 pre-patch readback. Verify live: `dd` p1 in TWRP and sha
  against both.)
- BROM entry combo on this unit (vol-up/vol-down/both? testpoint?) —
  gather.md step 5.

## M3 bulk-migration command (when ready; ~7 GiB, run under run-job)

```sh
# from this repo root, after `nix develop --command true` if needed:
rsync -a --info=progress2 /home/cjdell/Projects/GeminiPDA/stock-dump/ stock-dump/
# then re-verify every sha256 against the ledger above (rule 7) and flip
# the copy column to "here".
```

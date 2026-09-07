# How the Gemini PDA boots — gemini-nixos bootstrapping, explained

**Purpose:** a plain-language explainer of the boot/bootstrap process as
it applies to this port — written from the Q&A of the 2026-09-07 session
(kernel identity, cmdline storage, rootfs selection, initramfs builds).
It does **not** replace the receipts — hardware/boot ground truth with
`file:line` references lives in the sibling project
(`GeminiPDA/docs/boot-chain.md`) and in `docs/repartition-android-space.md`
for the dual-boot proposal. This doc is the "how it fits together" layer.
**Last updated:** 2026-09-07.

## 1. The one boot slot: everything lives in one boot.img

The stock MediaTek LK bootloader on this device boots **one image from one
partition**:

| LK boot mode | partition it loads |
|---|---|
| NORMAL (and POC charging etc.) | p22 `boot` only (`GeminiPDA/repos/gemini-lk/lk/app/mt_boot/mt_boot.c:1484`) |
| RECOVERY (para command = `boot-recovery`) | p1 `recovery` = TWRP (`mt_boot.c:1527`) |
| — | p30 `boot2` / p31 `boot3`: **never loaded by LK** |

Consequence: there is exactly **one** NORMAL boot slot, and that slot
holds a complete image, not a bare kernel:

```
boot.img = ANDROID! header
         + [ Image.gz + appended DTB ]   ← the kernel payload (13.45 MiB gz)
         + [ initrd.gz (gzip cpio) ]     ← the ramdisk / initramfs (1.26 MiB)
```

LK loads kernel, DTB and ramdisk from this one image into RAM, patches the
DTB, and hands off. The kernel and the ramdisk cannot be mixed from
different images — whichever boot.img is in p22 defines both.

## 2. The kernel is shared and OS-agnostic

The port deliberately **borrows** the working bring-up kernel #329
(`6.6.0-00048-g188aade698dd`) instead of building its own. Verified
byte-for-byte (2026-09-07):

```
sha256 of the kernel payload in the sibling's new_kali_boot.img
   (the #329 image flashed into `boot` on the device):
3a2a7f3a1098c0d6f97b774cea9a13993ecde504d39a2ade67247189fbaaa822
sha256 of the repo's borrowed payload (kernel/borrowed/Image.gz
   + appended DTB, as the repo's mkbootimg packs it):
3a2a7f3a1098c0d6f97b774cea9a13993ecde504d39a2ade67247189fbaaa822
```

The kernel binary that boots Debian today **is** the kernel the NixOS
boot.img carries. So "testing NixOS" needs no kernel build or new kernel —
it needs a boot.img whose *ramdisk* is the NixOS one (see §3–§5). The
in-repo kernel derivation (`devices/planet-geminipda/kernel/`) is a
*different, future* kernel build (stale pin, to be re-synced to the #329+
line); adopting it ends kernel-sharing with Debian (see §8 of
`docs/repartition-android-space.md`).

Both rootfses carry their own module trees for #329 (Debian's on p29,
installed after the #329 flash; NixOS's vendored `modules-6.6.0-00048-…`).
Same kernel ⇒ both module trees match (vermagic
`6.6.0-00048-g188aade698dd`) — this is load-bearing for dual-boot.

## 3. The ramdisk decides the rootfs — not the kernel, not the cmdline

Once the kernel unpacks the ramdisk into an initramfs it runs `/init`,
and the kernel's own `root=` machinery is bypassed: the initramfs becomes
the temporary root and its `/init` owns all root mounting. The `/init` in
this port's boot.img is a **minimal busybox script**
(`devices/planet-geminipda/initrd.nix`) that:

1. mounts proc/sysfs/devtmpfs and waits for the eMMC block devices;
2. **probes every `/dev/mmcblk*` partition** (mount ext4 ro, falling back
   to rw for dirty-journal replay) looking for **content markers**:
   - NixOS rootfs (store-only image): `/nix/store` **and**
     (`/nix-path-registration` on first boot, or
     `/nix/var/nix/profiles/system` later);
   - Debian rootfs (proposed branch): `/etc/os-release` **and**
     `/sbin/init`;
3. mounts the chosen partition rw at `/newroot`; for NixOS also creates
   `/dev/disk/by-label/NIXOS_SYSTEM` by hand (no udev in the initrd; the
   stage-2 fstab mounts `/` by label);
4. `exec switch_root /newroot <init>` — NixOS: the resolved
   `<generation>/init`; Debian: `/sbin/init`.

The chosen partition *becomes* `/` because the initrd mounted it and
switched into it. Scanning by content (rather than a fixed `root=`)
exists because the eMMC partition numbering is unstable on this unit
(mmcblk0 vs mmcblk1) and because the Mobile NixOS rootfs is store-only —
no `/etc/os-release` until stage-2 has run.

## 4. Where the cmdline lives (and who wins)

The kernel command line is not one stored blob; it exists in **four
places**, three of them inside the boot.img in p22:

| # | Location | Current content | Wins? |
|---|---|---|---|
| 1 | Compiled into the kernel Image (`CONFIG_CMDLINE`, in `kernel/borrowed/config-…`) | `console=tty0 console=ttyS0,921600n1 earlycon maxcpus=8 nokaslr fbcon=rotate:3 fbcon=font:TER16x32 g_ether.dev_addr=42:00:15:19:82:01 g_ether.host_addr=42:00:15:19:82:00 clk_ignore_unused pd_ignore_unused regulator_ignore_unused consoleblank=0` | **YES — `CONFIG_CMDLINE_FORCE=y`** |
| 2 | boot.img header field `cmdline[512]` @ offset 0x40 | repo build: NixOS set + `console=tty1 loglevel=4 lsm=…`; on-device #329 image: `bootopt=64S3,32N2,64N2 log_buf_len=4M` | ignored (inert) |
| 3 | DTB `/chosen/bootargs` | placeholder — LK overwrites it in RAM each boot with its assembled string (LK base + boot.img field + `lcm=…` + `earlycon=uartmtk,…` + `androidboot.…`) | ignored (see #1) |
| 4 | OS build source: `boot.kernelParams` in `config/gemini.nix` → mkbootimg stamps field #2 | the bring-up param set, declared as the bridge for dropping `CMDLINE_FORCE` | — |

Because #329 is built with `CONFIG_CMDLINE_FORCE=y`, the kernel reads
**only** the compiled-in string (#1) and discards the DTB bootargs that
LK assembled from the boot.img field (#2). Consequences:

- **Today both Debian and NixOS boot with the identical forced cmdline** —
  NixOS does *not* actually get "a different cmdline" until the kernel
  build changes. The extra entries in the repo's boot.img field
  (`console=tty1`, `loglevel=4`, `lsm=…`) are silently ignored.
- **If/when NixOS needs its own cmdline** (feasibility docs R4 / §3.3):
  rebuild the kernel with `CONFIG_CMDLINE_FORCE` dropped
  (`CONFIG_CMDLINE=""`); the kernel then honors `/chosen/bootargs` — i.e.
  LK's assembly of the boot.img field, which NixOS fills from
  `boot.kernelParams`. Cmdline changes then need only a boot.img repack,
  not a kernel rebuild. Ordering caveat: LK appends its own additions
  after the field, so `console=`/`earlycon=` first-valid-wins ordering
  matters, and the field must be complete because LK overwrites
  `bootargs` wholesale.
- **Per-OS cmdlines are impossible while both OSes share one boot.img** —
  kernel (#1), DTB (#3) and field (#2) are all inside that one image, and
  a running kernel cannot change its own cmdline. True divergence comes
  only with per-OS kernels/boot images (reflash), i.e. exactly when the
  shared-kernel window closes. Practical rule for the dual-boot phase:
  keep `CMDLINE_FORCE` on the shared kernel and let both OSes boot the
  proven single cmdline; the future NixOS kernel drops FORCE and takes
  the declarative `boot.kernelParams` route.

## 5. How Debian vs NixOS is selected (the para marker)

The rootfs choice is a **user-space boot decision** made by the shared
initrd every boot. With NixOS on p32 and Debian on p29 both rootfses
match their content markers, so the proposal adds an explicit selector in
the **para partition** (p2; 32-byte command at offset 0 — the same field
LK itself reads for recovery):

| para command | LK does | our `/init` then does |
|---|---|---|
| `boot-recovery` | boots TWRP (p1) — never reaches the initrd | — |
| `boot-debian\0` | ≠ `boot-recovery` → NORMAL → p22 | mount p29 → Debian `/sbin/init` |
| `""` (32 zeros) | NORMAL → p22 | mount p32 → NixOS (default) |

LK's check is an exact `strcmp(command, "boot-recovery")`
(`platform/mt6797/recovery.c:107`), so any other value is inert and the
boot proceeds normally. LK env lives at para offset 0x20000
(`platform/mt6797/env.h:36-44`), so writes to bytes 0..31 never touch it.
The initrd reads the marker from the eMMC's p2 (same "largest mmcblk"
detection the flash scripts use) and falls back to content probing →
busybox shell if the marked OS's partition is missing.

Switching OS = one para write + reboot from either running OS (a
`gemini-boot-debian` unit on the NixOS side would mirror the existing
`gemini-boot-recovery`). Note: TWRP's own UI reboot actions rewrite the
misc command field, which resets the choice to default — the `bin/` flash
scripts write para with dd and are deterministic.

## 6. The two initramfs builds today — and the one proposed

Today there are two *different* initramfs builds, each riding in its own
boot.img:

| | GeminiPDA initramfs | gemini-nixos initrd |
|---|---|---|
| Source | `GeminiPDA/build/initramfs-6.6/init` | `devices/planet-geminipda/initrd.nix` |
| In | the on-device boot.img (p22, boots Debian) | the repo's NixOS boot.img (built, never flashed) |
| Finds | `/etc/os-release` → Debian p29 | `/nix/store` + registration/profile → NixOS |
| Handoff | fix fstab, A72 opt-in, `switch_root /sbin/init` | by-label symlink, resolve generation, `switch_root <gen>/init` |

Notes:

- **Debian's own initramfs (`/boot/initrd.img` on p29) is never used in
  this chain** — LK boots only the boot.img in p22 and never reads the
  rootfs. The thing that boots Debian is the GeminiPDA initramfs *in p22*.
- **NixOS's Mobile NixOS stage-1 initrd is not in the boot image** — it
  cannot fit the 16 MiB budget (feasibility R1); the minimal initrd
  replaces it.
- Switching OS today = swapping the whole boot.img (each carries its own
  initramfs) — the "reflash `boot`" cost.
- **Proposal** (repartition doc §5/§6): one *dual-boot initramfs* — the
  NixOS branch (already written) plus a Debian branch that replicates the
  GeminiPDA init verbatim (fstab `/` rewrite to the real device, A72
  opt-in enforcement, `switch_root /sbin/init`). One boot.img, one
  initramfs build, both OSes boot through it; no reflash.

## 7. Size constraints that shape all of this

Measured (2026-09-07 build): boot.img 14.72 MiB in a 16 MiB partition
(1.28 MiB headroom); kernel payload 13.45 MiB gz (decompresses to
34.3 MiB); ramdisk 1.26 MiB gz. Hard ceilings: LK decompresses the
kernel with **zlib/gzip only** (no lzma/lz4/xz kernel payload); the
decompressed kernel must fit LK's MT6797 scratch (~50 MiB) and stay clear
of the ramdisk copy target (0x45000000); the ramdisk is decompressed by
the **kernel**, which on #329 supports gzip/xz/lz4/zstd/etc.
(`CONFIG_RD_*` all =y) — the initrd format can change if it ever needs to
shrink. The rootfs location does not interact with the boot budget at
all. Full analysis + the workaround ladder: `docs/repartition-android-space.md` §7.

## 8. Where each piece lives (map)

| Concern | Where documented |
|---|---|
| LK mechanics, offsets, para layout, decompress limits (receipts) | `GeminiPDA/docs/boot-chain.md` |
| Partition map, hardware | `GeminiPDA/docs/hardware.md` |
| Boot.img geometry for this port, stage-1 sizing (R1), cmdline contract (R4) | `docs/mobile-nixos-port-feasibility.md` §3/§6/§7 |
| Repurpose p32 + dual-boot proposal + shared-kernel constraint | `docs/repartition-android-space.md` |
| Kernel identity (borrowed #329, vendored artifacts) | `README.md` "Kernel phase" + `kernel/borrowed/` |
| The initrd implementation | `devices/planet-geminipda/initrd.nix` |
| Flash/para tooling | `bin/flash-nixos.sh`, `bin/boot-switch.sh` |

# gemini-nixos — Mobile NixOS for the Gemini PDA

Phase 0/1/3 of porting the mainline-Linux bring-up of the Planet
Computers Gemini PDA (MT6797X "aeon") onto Mobile NixOS. The feasibility
study and phase plan live in
[docs/mobile-nixos-port-feasibility.md](docs/mobile-nixos-port-feasibility.md).

The device boots a stock MediaTek LK from a 16 MiB `boot` partition
(Android `boot.img` v0, gzip `Image.gz` + appended DTB, 2048-byte pages),
with the rootfs on the 27.7 GiB `linux` partition (p29). There is no
fastboot; flashing goes through the patched no-swipe TWRP or `dd` over
adb.

## Layout

| Path | What |
|---|---|
| `flake.nix` | Flake entry point; imports Mobile NixOS eval directly (Mobile NixOS is not a flake) |
| `devices/planet-geminipda/default.nix` | Out-of-tree device definition: SoC, boot.img geometry, DTB append, kernel, minimal-initrd wiring |
| `devices/planet-geminipda/initrd.nix` | Minimal partition-scanning busybox initrd (R1; replaces Mobile NixOS stage-1 in the boot image) |
| `devices/planet-geminipda/kernel/` | Kernel derivation (mobile-nixos kernel-builder) + the working bring-up `.config`; `postInstall` builds the out-of-tree `sramldo-smc.ko` (A72 bring-up SMC) against the same tree |
| `modules/hardware-soc-mediatek-mt6797.nix` | Out-of-tree MT6797 SoC fragment (upstreaming = phase 6) |
| `services/` | Phase 3 device services: `gemini-pda.nix` (systemd units) + `gemini-utils.nix`/`scripts/` (verified bring-up CLIs, ported verbatim) |
| `pkgs/mesa-geminipda.nix` | Mesa 25.0.7 + geminipda panfrost fork (Mali-T880 dma-buf import; phase 4 preview) |
| `patches/` | The geminipda Mesa fork patch (byte-for-byte the GeminiPDA submodule commit `ac19be0`) |
| `config/gemini.nix` | Stage-2 system configuration (headless + g_ether SSH, device services, Mesa fork + libglvnd) |
| `kernel/geminipda-bringup-733c0c7ea.tar.gz` | `git archive` snapshot of the pinned kernel commit |
| `bin/snapshot-kernel.sh` | Regenerates the kernel snapshot |
| `mesa/mesa-25.0.7.tar.gz` | Vendored Mesa 25.0.7 tarball (the GitLab API archive endpoint is byte-unstable, so `fetchFromGitLab` is unusable here) |
| `bin/snapshot-mesa.sh` | Regenerates the Mesa tarball from the canonical `/-/archive/` URL |
| `repos/mobile-nixos/` | Mobile NixOS clone (see pins below) |

## Pins

- **Mobile NixOS**: `repos/mobile-nixos` at commit `2c132754`
  (branch `development`). It is not a flake, so it is referenced by
  path; pin the commit (submodule or recorded SHA) before relying on it.
- **Nixpkgs**: resolved by Mobile NixOS's own `npins` pin
  (`nixos-unstable` @ `nixos-26.11pre1031299.0bb7ec54c848`).
- **Kernel**: `geminipda-bringup` @ `733c0c7ea74195bd30734f599f37e69febfd38e0`
  (GeminiPDA `repos/linux-6.6`), snapshotted into `kernel/`. The
  `.config` is the exact working bring-up config (still
  `CONFIG_CMDLINE_FORCE=y`; switching to `boot.kernelParams` is the
  docs-R4 A/B step on hardware — the matching params are already
  declared in `config/gemini.nix`).

## Build

From an x86_64 host (cross-compiles to aarch64-linux):

```sh
nix build .#packages.x86_64-linux.default   # boot.img + rootfs.img (+ flash script)
nix build .#packages.x86_64-linux.bootimg   # boot.img only
nix build .#packages.x86_64-linux.rootfs    # rootfs.img (→ `linux` partition)
nix build .#packages.x86_64-linux.initrd    # minimal initrd (size measurement, docs R1)
nix build .#packages.x86_64-linux.mesa      # Mesa 25.0.7 + geminipda panfrost fork
```

## Phase 3 device services (in the rootfs)

The verified bring-up utilities are ported as systemd services + CLIs
(`services/`; scripts verbatim from the GeminiPDA project except the
`power` PATH shim and the new `gemini-wdt-reboot`):

| Unit / CLI | What it does |
|---|---|
| `gemini-gpu-poweron.service` | Powers the Mali-T880 (MFG MTCMOS domains via SPM devmem + VGPU rails via i2c RT5735) before anything uses the GPU |
| `gemini-a72-up.service` | Brings cpu8/cpu9 (A72) online: DA9214 BUCKB enable, SPM pre-sequence, SRAM-LDO SMC (`sramldo-smc.ko`, built into the kernel tree), PSCI CPU_ON, WDT-guarded with retry/backoff |
| `gemini-battery-guard.service` | Battery safety daemon: low/critical VBAT alerts + orderly poweroff, USB-present-but-not-charging detection, history CSV |
| `backlight`, `power` | DISP_PWM0 backlight control (no sysfs backlight on this kernel; drives the LED-boost PWM via devmem) + charge CLI (`power dim-to-charge` solves full-brightness-cannot-charge) |
| `battstat`, `bq25896-raw.sh` | BQ25896 status reader; raw ADC reads (bypasses the driver's stale latches) |
| `gemini-boot-recovery` | Writes `boot-recovery` to the `para` partition and reboots into TWRP |
| `gemini-wdt-reboot [s]` | Device-side reboot that self-boots (WDT EXRST — plain `systemctl reboot` powers this unit off) |

`sramldo-smc.ko` is loaded at boot via `boot.kernelModules` (the kernel
derivation builds it in `postInstall` and ships it in the module tree
under `extra/`); `busybox` + `i2c-tools` are system packages for the
scripts and hand use on the serial console.

**Mesa ICD runtime wiring** (phase 4 preview, verified in the rootfs
2026-09-05): the glvnd client libs (`pkgs.libglvnd`) scan the
compiled-in list `/run/opengl-driver/...` (needs `hardware.graphics` —
off), `/etc/glvnd/egl_vendor.d`, `/usr/share/...` (no `/usr`). The
manifest therefore lands in `/etc` via an explicit
`environment.etc` entry (NixOS does not merge package `$out/etc`),
and the ICD gets a RUNPATH to its own lib dir (`--add-rpath`, keeping
the build-time entries for `libdrm`/glibc) so its `DT_NEEDED` on
`libgallium-25.0.7.so` resolves without an ldconfig cache. On-glass
check owed: `eglQueryString(EGL_EXTENSIONS)` must list
`EGL_EXT_image_dma_buf_import` (the fork's headline feature).

The kernel build is the long pole (full 6.6 tree, ~16 cores).

Measured boot image (build 2026-09-05): 15,431,680 B = **14.7 MiB** in
the 16 MiB partition (kernel payload 13.45 MiB + DTB, initrd 1.26 MiB
gzip, ~1.3 MiB headroom).

## Boot chain

```
LK ──► boot.img ("ANDROID!" v0, 2 KiB pages)
       ├─ kernel: Image.gz + appended DTB → 0x40200000 (LK gunzips, finds FDT)
       └─ ramdisk: gzip cpio (busybox + /init) → 0x45000000 (copied verbatim;
          the kernel decompresses it, CONFIG_RD_GZIP=y; passed via ATAG_INITRD2)
            └─ /init: scans /dev/mmcblk* for the Mobile NixOS rootfs
               (ext4 with /nix/store + nix-path-registration or a system
               profile — the image is store-only, no /init at its root),
               mounts it rw, creates /dev/disk/by-label/NIXOS_SYSTEM (no udev
               in the initrd; fstab references / by label), then switch_roots
               to <generation>/init found via nix-path-registration (first
               boot) or /nix/var/nix/profiles/system (later boots)
                 └─ NixOS stage-2 (store rehydrated on first boot via
                    postBootCommands; / grows to fill the partition via
                    x-systemd.growfs)
```

The Mobile NixOS stage-1 is **not** in the boot image (it cannot fit,
see R1); it is disabled for this device. No stage-1 USB gadget, boot
GUI, or boot SSH — serial `ttyS0,921600` + fbcon are the bring-up
interfaces.

The rootfs image (Mobile NixOS `generatedFilesystems.rootfs`) contains
**only** the Nix store + `nix-path-registration` (1.68 GiB for this
config): there is no `/init`, `/etc` or `/bin` in the image — the
stage-2 init is the generation's store-path init, which is why the
initrd resolves the generation explicitly (see above). The
`android-fastboot-images` output renames it `system.img` and ships the
Mobile NixOS `flash-critical.sh` stub, which exits 1 by design
(`flashingMethod = "lk2nd"`; this device has no fastboot — flashing is
manual, see below).

## Flashing (manual — no fastboot)

1. Flash the rootfs image to the `linux` partition (p29) and
   `boot.img` to the `boot` partition (p22), via TWRP (adb) or the
   project's `flash-nohelp.sh`-style pipeline (para-write → WDT EXRST
   self-boot → TWRP adb). The image is complete ext4 with label
   `NIXOS_SYSTEM` (set by image-builder; `rootfs.img` from the `rootfs`
   output, `system.img` in the `default` output — same file) — writing
   it replaces p29 entirely (wipes Gemian only; Android's
   `system`/`userdata` are untouched).
2. The device auto-boots: LK → minimal initrd (partition scan) →
   NixOS stage-2. First boot rehydrates the Nix store
   (`nix-store --load-db`) and auto-resizes the filesystem to fill p29.

Do **not** flash anything from this repo yet — the artifacts build and
match the bring-up boot contract (phases 0/1/3 done at the build level),
but on-glass verification (phase 1/2) has not happened.

## Known constraints (docs §7)

- **R1 (headline) — resolved with the minimal initrd.** The 16 MiB
  `boot` budget cannot hold the Mobile NixOS stage-1 initrd (measured
  9.66 MiB xz / 39 MiB unpacked for this kernel/config; total would be
  ~23.3 MiB). The boot image instead carries the partition-scanning
  busybox initrd (1.26 MiB gzip — the same design and size as the
  verified bring-up image): total 14.7 MiB, ~1.3 MiB headroom. Trade
  -off: no stage-1 USB/GUI/SSH during early boot (serial console only);
  if ever needed, the stage-1 GUI/SSH can go into a separate recovery
  image without affecting the `boot` partition budget.
- **R2 (partially resolved)**: the Mesa 25.0.7 fork now builds in-tree
  (`pkgs/mesa-geminipda.nix` — vanilla 25.0.7 + the fork patch,
  surfaceless EGL/glavnd ICD layout, in the system closure with
  libglvnd). wlroots 0.18.2 / kwin 6.3.6 packaging is still phase-4
  work; nixpkgs 26.11pre floats newer, unverified versions of those.
- **R3**: no DRM — the desktop is `gemwl` (custom wlroots compositor)
  nested KWin/LXQt on the LK framebuffer; packaged as services, not a
  display manager.

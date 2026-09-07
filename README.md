# gemini-nixos — Mobile NixOS for the Gemini PDA

Phase 0/1/3 of porting the mainline-Linux bring-up of the Planet
Computers Gemini PDA (MT6797X "aeon") onto Mobile NixOS. The feasibility
study and phase plan live in
[docs/mobile-nixos-port-feasibility.md](docs/mobile-nixos-port-feasibility.md).

The device boots a stock MediaTek LK from a 16 MiB `boot` partition
(Android `boot.img` v0, gzip `Image.gz` + appended DTB, 2048-byte pages).
One **dual-boot boot.img** (shared #329 kernel + para-selected initrd)
boots both OSes: the NixOS rootfs on Android's former p32 `userdata`
(27.3 GiB, ext4 label `NIXOS_SYSTEM`) by default, and the GeminiPDA
Debian rootfs on p29 `linux` (kept for testing) via a `boot-debian`
para marker — see `docs/repartition-android-space.md`. There is no
fastboot; flashing goes through the patched no-swipe TWRP or `dd` over
adb.

> **GOLDEN REPO (declared 2026-09-07):** gemini-nixos is now the primary
> home for the whole Gemini PDA project — the port AND the
> hardware/boot knowledge base (see `AGENTS.md` for the migration
> plan M1–M7 and the rules). The former sibling repo
> `GeminiPDA` is the legacy source being folded in; it is not deleted
> and not edited for new work. Disaster-recovery knowledge now lives
> here: `docs/disaster-recovery/`. [added 2026-09-07]

## Layout

| Path | What |
|---|---|
| `flake.nix` | Flake entry point; imports Mobile NixOS eval directly (Mobile NixOS is not a flake) |
| `devices/planet-geminipda/default.nix` | Out-of-tree device definition: SoC, boot.img geometry, DTB append, kernel, minimal-initrd wiring |
| `devices/planet-geminipda/initrd.nix` | Minimal partition-scanning busybox initrd (R1; replaces Mobile NixOS stage-1 in the boot image) |
| `devices/planet-geminipda/kernel/` | Kernel derivation (mobile-nixos kernel-builder) + the working bring-up `.config`; `postInstall` builds the out-of-tree `sramldo-smc.ko` (A72 bring-up SMC) against the same tree |
| `modules/hardware-soc-mediatek-mt6797.nix` | Out-of-tree MT6797 SoC fragment (upstreaming = phase 6) |
| `services/` | Device services: `gemini-pda.nix` (GPU/A72/battery units), `audio.nix` (PipeWire system session + S16 pin + speaker-amp), `wifi.nix` (internal CONSYS stack + USB auto-connect + NVRAM), `desktop.nix` (gemwl compositor unit), `lxqt.nix` (**nested LXQt desktop 2026-09-07**: labwc 0.8.3 hosting the nixpkgs lxqt 2.4 session inside gemwl — the verified GeminiPDA stack as NixOS services), `gemini-utils.nix`/`scripts/` (verified bring-up CLIs, ported verbatim) |
| `pkgs/mesa-geminipda.nix` | Mesa 25.0.7 + geminipda panfrost fork (Mali-T880 dma-buf import; phase 4 preview). Base = published upstream 25.0.7 archive fetched by hash + the tracked fork patch — nothing vendored (see `docs/library-deltas.md`). Also builds `libgbm` (needed by wlroots 0.18's gles2 renderer) |
| `pkgs/wlroots-geminipda.nix` | wlroots 0.18.2 pinned from source (the version the verified desktop used; nixpkgs in the pin floats 0.20.1) — libinput backend + gles2 renderer only (no DRM on this device) for gemwl, built against the mesa fork; `withDrmBackend=true` variant for labwc 0.8.3 (its wlr_drm_lease compile needs the header; never instantiated on hardware) |
| `pkgs/labwc-geminipda.nix` | labwc 0.8.3 pinned from source against the pinned wlroots 0.18.2 (the on-glass verified pair; nixpkgs floats labwc 0.20/wlroots 0.20) — the nested compositor hosting the LXQt session |
| `pkgs/gemwl.nix` + `pkgs/gemwl/` | gemwl, the GPU-direct LK-framebuffer Wayland compositor (wlroots 0.18) + the tinytest xdg-shell smoke clients; sources byte-identical to the verified GeminiPDA `build/wayland/` files (tinytest-anim/tinytest listener-lifetime crash fixed 2026-09-07) |
| `config/lxqt/` | The LXQt-session user configs (lxqt.conf, session.conf, labwc rc.xml + autostart, the vendored “Gemini” openbox themerc, desktop launchers) seeded to `/root` by `services/scripts/start-lxqt-nested` |
| `pkgs/speaker-amp.nix` + `pkgs/speaker-amp/` | Speaker-amp GPIO helpers (gpioout/spkamp), cross-compiled |
| `pkgs/gemini-firmware.nix` + `pkgs/gemini-firmware/` | Wi-Fi firmware: MT6630 CONSYS WMT blobs + RTL8821CU + factory NVRAM record |
| `patches/` | The geminipda Mesa fork patch (byte-for-byte the GeminiPDA submodule commit `ac19be0`) |
| `config/gemini.nix` | Stage-2 system configuration (headless + g_ether SSH, device services, Mesa fork + libglvnd) |
| `devices/planet-geminipda/kernel-borrowed.nix` | Borrowed-kernel package (see “Kernel phase” below) |
| `kernel/borrowed/` | The working bring-up kernel #329 artifacts, vendored + tracked (payload, DTB, module tree, sramldo-smc.ko, .config) |
| `kernel/geminipda-bringup-733c0c7ea.tar.gz` | `git archive` snapshot of the in-repo kernel pin (future self-contained build; see “Kernel phase”) |
| `bin/snapshot-kernel.sh` | Regenerates the kernel snapshot (and registers it for nix visibility) |
| `bin/dump-bootimg-header.sh` | Parse an AOSP v0 boot.img header (phase-0 geometry checks) |
| `bin/device-ssh.sh`, `bin/net-up.sh`, `bin/device-reboot.sh` | Host side of the g_ether link (root @ 10.15.19.82), auto link-up after power-on, WDT-EXRST remote reboot |
| `bin/boot-switch.sh` | Boot-target switching + boot-partition flash over adb/TWRP (status/twrp/android/debian/flash/restore; `debian` = para=boot-debian → p29, 2026-09-07) |
| `bin/flash-nixos.sh` | Full NixOS flash orchestration: converges to TWRP from any device state, flashes boot.img → `boot` and system.img → p32 (`userdata`, Android erased — Debian p29 untouched); verbs status/boot/rootfs/all/boot-nixos/debian; safe TWRP-sticky default |
| `bin/run-job.sh` | Detached job runner for long ops (flash waits, big builds) — never inline nohup/pgrep loops |
| `bin/deploy.sh` | **Workstation-style generation loop (2026-09-07)**: host cross-builds the toplevel (pinned via gc-pin), `nix copy` delta → device store over ssh, profile switch + activate. Verbs status/build/deploy [PATH]/rollback [N]. Reboot lands on the new gen; old gens stay selectable — no reflash, no TWRP |
| `bin/gc-pin.sh` | GC-root a build (NAME STORE_PATH | list | unpin) so host `nix-collect-garbage` can't sweep the cross closure (happened once — gen3 silently rebuilt ~259 packages) |
| `bin/flash-nixos.sh` `grow-rootfs` | Offline-grow the p32 rootfs to the full partition from TWRP (e2fsck + resize2fs, static musl e2fsprogs) — the recovery path for make_ext4fs-geometry fs the kernel can't online-grow (R13); images since 2026-09-07 grow on first boot via growfs-root |
| `docs/library-deltas.md` | Long-standing goal + the “published base + in-repo delta” pattern (mesa done; kernel & co next) |
| `docs/repartition-android-space.md` | NixOS rootfs on Android's p32 `userdata` (Debian stays on p29) + dual-boot boot.img via a para marker; boot-budget analysis. **Decided + implemented repo-side 2026-09-07** (§10 decisions; flash is the next milestone) |
| `docs/boot-process.md` | Plain-language explainer: how the Gemini boots for this port — one boot slot, shared kernel, initrd-as-rootfs-selector, cmdline storage/`CMDLINE_FORCE`, para marker, initramfs builds |
| `docs/phase-2-on-glass.md` | **Phase-2 milestone (2026-09-07): NixOS boots on glass** — version lines, the bootopt discovery (§2a), recovery/fix receipts, device state, and the open TODO list of not-quite-working items |
| `docs/session-log.md` | Dated entries; the 2026-09-07 evening sweep: R12/R13/R14 + initrd multi-boot fix + panfrost ordering + gens 2-5 via deploy.sh, rootfs grown to 27.3 GiB |
| DR (device disaster recovery) | `docs/disaster-recovery/` — full-flash-erase restore to TWRP (levels 0–2); image ledger + sha256 in `inventory.md` (blobs in `stock-dump/`, gitignored), gather checklist + drills. NixOS flashing after a restore = `bin/flash-nixos.sh` |
| `repos/mobile-nixos/` | Mobile NixOS clone (see pins below) |

## Pins

- **Mobile NixOS**: commit `2c132754` (branch `development`), fetched
  as a pinned tarball by `flake.nix` (it is not a flake, so nix 2.34
  cannot use it as a flake input). Bump the SHA in `flake.nix` to
  update.
- **Nixpkgs**: resolved by Mobile NixOS's own `npins` pin
  (`nixos-unstable` @ `nixos-26.11pre1031299.0bb7ec54c848`).
- **Kernel (in-repo)**: `geminipda-bringup` @
  `733c0c7ea74195bd30734f599f37e69febfd38e0` (GeminiPDA
  `repos/linux-6.6`), snapshotted into `kernel/` — the pin for the
  eventual self-contained kernel build. Its `.config` is the exact
  working bring-up config (still `CONFIG_CMDLINE_FORCE=y`; switching
  to `boot.kernelParams` is the docs-R4 A/B step on hardware — the
  matching params are already declared in `config/gemini.nix`).

## Kernel phase (rootfs port borrows kernel #329)

The rootfs currently does **not** build the in-repo kernel. It uses the
working bring-up kernel #329 (`6.6.0-00048-g188aade698dd`) from the
GeminiPDA project, wrapped as a thin kernel package
(`devices/planet-geminipda/kernel-borrowed.nix`) whose artifacts are
vendored in `kernel/borrowed/` (tracked — small, and the source commit
is local-only, not fetchable):

- `Image.gz` + `dtbs/...` — the exact kernel payload + DTB extracted
  from the verified `new_kali_boot.img`, so the flake's `bootimg`
  output builds the correct boot image for the NixOS rootfs (borrowed
  kernel + DTB + the minimal initrd) without any GeminiPDA dependency;
- `modules-6.6.0-00048-g188aade698dd.tar.xz` — the module tree of that
  exact kernel (panfrost, mtk_wcn, wlan_gen3, rtw88_*, …), served as
  NixOS `system.modulesTree` → `/run/booted-system/kernel-modules`;
- `sramldo-smc.ko` — the A72 bring-up module with matching vermagic
  (the in-repo kernel would build a `6.6.0`-vermagic module this kernel
  refuses); under `extra/` in the module tree, depmod-indexed;
- `config-…` — the exact #329 `.config` (nixpkgs generates the ASLR
  sysctl file from it).

The in-repo `devices/planet-geminipda/kernel/` build stays for the
self-contained phase; it must first be re-synced to the #329+ kernel
line (it lacks the CONSYS Wi-Fi, audio S16 and sidekey commits).

**Tarball visibility**: nix 2.34 flake exports contain only git-tracked
files, and the host runs `pure-eval = true`. The large kernel source
tarball (`kernel/*.tar.gz`) stays gitignored but is registered with
`git add -Nf` (intent-to-add — path only, content never committed) by
`bin/snapshot-kernel.sh`, which makes it visible to nix. A fresh clone
must run `bash bin/snapshot-kernel.sh` before building. (Do not
`git add .` — it would commit the tarball.)

Mesa no longer needs this: since 2026-09-07 its source is fetched by
hash from the published upstream archive by `pkgs/mesa-geminipda.nix`
(byte-stable canonical `/-/archive/` URL; see `docs/library-deltas.md`).

## Build

From an x86_64 host (cross-compiles to aarch64-linux):

```sh
nix build .#packages.x86_64-linux.default   # boot.img + rootfs.img (+ flash script)
nix build .#packages.x86_64-linux.bootimg   # boot.img only (borrowed kernel #329 payload)
nix build .#packages.x86_64-linux.rootfs    # rootfs.img (→ `linux` partition)
nix build .#packages.x86_64-linux.initrd    # minimal initrd (size measurement, docs R1)
nix build .#packages.x86_64-linux.mesa      # Mesa 25.0.7 + geminipda panfrost fork
```

With the borrowed kernel, the rootfs build no longer compiles the
6.6 kernel tree (the former long pole) — the module tree is vendored.
Mesa is the remaining heavy build (cached in the local store once
built).

Cross-aarch64 note: `ffmpeg`/`ffmpeg-headless` in this nixpkgs pin
default to `withCudaLLVM = true` (`withHeadlessDeps && !isDarwin`) and
fail to configure for a non-Darwin aarch64 target ("cuda_llvm
requested but not found") — they are hard build inputs of
`alsa-plugins` (→ `alsa-utils`, in `environment.systemPackages`).
`config/gemini.nix` carries a `nixpkgs.overlays` entry overriding both
to `withCudaLLVM = false`; the override is applied through
nixpkgs's extensible-derivation machinery (the package attrs are a
derivation, and `override` re-runs the overlay chain with the merged
attrs, so the drv hash does change).

## Phase 3 device services (in the rootfs)

The verified bring-up utilities are ported as systemd services + CLIs
(`services/`; scripts verbatim from the GeminiPDA project except the
`power` PATH shim, the `__GEMINI_UTILS__` bin-dir placeholder in the
audio/wifi CLIs, `gemini-wdt-reboot` (new), and the R10 bash-shebang
rewrite — see feasibility §7 R10). Beyond the bring-up CLIs, the
services carry the rootfs-side deltas from the outstanding.md audit:
the DRM/panfrost early-load (major-226 race), the USB host-PM udev
rules, the 10 % backlight-default unit, and hand-started
`gemini-wdt-reboot` / `gemini-boot-recovery` units (see the header of
`services/gemini-pda.nix`).

| Unit / CLI | What it does |
|---|---|
| `gemini-gpu-poweron.service` | Powers the Mali-T880 (MFG MTCMOS domains via SPM devmem + VGPU rails via i2c RT5735) before anything uses the GPU |
| `gemini-a72-up.service` | Brings cpu8/cpu9 (A72) online: DA9214 BUCKB enable, SPM pre-sequence, SRAM-LDO SMC (`sramldo-smc.ko`, built into the kernel tree), PSCI CPU_ON, WDT-guarded with retry/backoff |
| `cl2-down.sh [cpu9\|cpu8\|both]` | A72 power-DOWN (hand-run CLI, in `gemini-pda-utils`): per-core PSCI offline (safe) + full cluster teardown → cold-boot power state (ISO re-asserted, DA9214 BUCKB rail dropped); WDT-armed 20 s. Re-enable = `cl2-up.sh`. Proven on #329 2026-09-07 (sibling) |
| `gemini-battery-guard.service` | Battery safety daemon: low/critical VBAT alerts + orderly poweroff, USB-present-but-not-charging detection, history CSV |
| `gemini-backlight-default.service` | Boot-time backlight = 10 % (power-saving default; DISP_PWM0 via sysfs on #329, devmem fallback) — outstanding.md item 6 |
| `backlight`, `power` | DISP_PWM0 backlight control (no sysfs backlight on this kernel; drives the LED-boost PWM via devmem) + charge CLI (`power dim-to-charge` solves full-brightness-cannot-charge) |
| `battstat`, `bq25896-raw.sh` | BQ25896 status reader; raw ADC reads (bypasses the driver's stale latches) |
| `gemini-boot-recovery` / `gemini-wdt-reboot [s]` | CLIs (in `gemini-pda-utils`) + hand-started systemd units (outstanding.md item 9): boot into TWRP via sticky para, and device-side WDT EXRST self-boot (plain `systemctl reboot` powers this unit off) |
| `gemini-boot-debian` | CLI + hand-started unit (2026-09-07, dual-boot): writes para=`boot-debian` and reboots → next power-on boots Debian p29 through the shared initrd (docs/repartition-android-space.md §5); reverse = para-clear (`boot-nixos`) |
| `pipewire` / `wireplumber` / `pipewire-pulse` | PipeWire media stack as ONE root system session (`/run/gemwl-audio`); the WirePlumber rule opens the MT6351 card through `pcm.gemini16` (`/etc/asound.conf`), which pins the S16-only analog path to S16_LE at the alsa-lib boundary (S32 plays as white noise, S24 is refused — verified) |
| `gemini-audio-defaults` | Applies the DL1→ADDA→HPL/HPR playback route + the persisted speaker/headphone output mode at boot (after `alsa-restore`) |
| `speaker` / `audio-output` | Built-in-speaker vs headphone output (speaker-amp pads 243/244 via the gpio chardev; no jack detection yet, so manual) |
| `gemini-wifi-nvram` | Installs the factory NVRAM record (real MAC `00:09:34:5a:af:c1` + TX cal) to `/data/nvram/APCFG/APRDEB/WIFI` (tmpfs) before the Wi-Fi stack probes the chip — without it the MAC changes every power cycle |
| `gemini-wifi-internal` | Internal MT6630 CONSYS stack: mtk_wcn + wlan_gen3 (load order matters, B-33) + the WMT pwr-on → wlan0. Runs after `gemini-gpu-poweron` (the CONSYS chip's chrdev, major 226, collides with the GPU's if the wlan modules probe first) |
| `gemini-wifi-auto` | `wifi auto`: associate with the strongest saved profile (`/etc/wifi/profiles.conf`) + dhcpcd lease; silent no-op without profiles |
| `wifi` / `wifi-internal` | CLIs: USB RTL8821CU dongle (scan/connect/networks/forget) and the internal CONSYS stack (start/status/stop) |

`sramldo-smc.ko` is loaded at boot via `boot.kernelModules` (the
borrowed module tree ships it under `extra/`, depmod-indexed; the
in-repo kernel derivation would build it in `postInstall` instead);
`busybox` + `i2c-tools` are system packages for the scripts and hand
use on the serial console.

Beyond the bring-up units above, the rootfs carries the
`outstanding.md` audit deltas: `boot.kernelModules` also loads the DRM
chain (`drm`/`drm_shmem_helper`/`gpu-sched`/`panfrost`) + `mt6351-keys`
early (major-226 race with `wlan_gen3`), the B-19 USB host-PM udev
rules keep USB autosuspend off, and `gemini-backlight-default` dims the
display to 10 % at boot. `gemini-wdt-reboot` / `gemini-boot-recovery`
exist as CLIs *and* hand-started systemd units. The bash-shebang
rewrite (feasibility §7 R10) is applied at package time so the verbatim
Debian scripts exec on the NixOS rootfs.

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

**Desktop — gemwl + nested LXQt (phase 4 preview, in-tree 2026-09-07).**
The desktop is NOT a display manager: `gemwl.service` (services/desktop.nix) owns the LK framebuffer (/dev/gemfb, GPU-direct via panfrost + the mesa fork); the LXQt desktop runs NESTED inside it — `lxqt-nested.service` (services/lxqt.nix): labwc 0.8.3 (pkgs/labwc-geminipda.nix, wlroots 0.18.2 “wayland” backend) on gemwl's wayland-0 exporting wayland-1, hosting the nixpkgs lxqt 2.4 session (panel via wlr-layer-shell, pcmanfm-qt desktop, qterminal, pavucontrol-qt + qpwgraph audio GUIs). User configs/theme seeded from `config/lxqt/`; icon theme Papirus; session bus in /run/gemwl; PipeWire sockets from audio.nix (/run/gemwl-audio). Both units auto-start (wantedBy multi-user.target); console-only boot = `systemctl disable gemwl lxqt-nested`. Built repo-side 2026-09-07; on-glass verification pending (first LXQt boot = the gpu-warmup banding question, see services/desktop.nix header).

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

**Tooling is in-repo and ready** (see `AGENTS.md` for the full cheat
sheet): `bin/flash-nixos.sh` orchestrates the whole pipeline — it
converges the device to TWRP from **any** state (running Linux via
para-write + WDT EXRST self-boot over ssh, Android via adb, or POC/
offline with prompts), then flashes. `bin/boot-switch.sh` is the
adb/TWRP boot-target state machine underneath; device state over ssh
= `bin/device-ssh.sh` / `bin/net-up.sh` (g_ether, 10.15.19.82).

1. Build: `nix build .#packages.x86_64-linux.default` → `result/`
   with `boot.img` (dual-boot initrd + #329 kernel) + `system.img`
   (the NixOS rootfs; same file as the `rootfs` output's `rootfs.img`).
2. `bash bin/flash-nixos.sh status` — device state + local artifacts.
3. `bash bin/flash-nixos.sh boot` — backs up the current `boot`, flashes
   the dual-boot `boot.img` → p22 `boot` (16 MiB). Stays in TWRP.
4. `bash bin/flash-nixos.sh rootfs --yes` — flashes `system.img` → p32
   `userdata` (27.3 GiB ext4, label `NIXOS_SYSTEM`; destroys Android's
   FDE userdata — **Debian on p29 is untouched**). First boot
   auto-resizes the fs to fill p32 and rehydrates the Nix store.
5. `bash bin/flash-nixos.sh boot-nixos` — clear para + reboot: LK →
   dual-boot initrd (para zeros = NixOS default) → p32 → NixOS stage-2.

**Switching OS afterwards = one para write + reboot** (no reflash):
Debian p29 = `boot-debian` marker (`bin/flash-nixos.sh debian` from a
running OS over ssh, `bin/boot-switch.sh debian` from TWRP, or the
on-device `gemini-boot-debian` unit); NixOS = clear para
(`boot-nixos`). If the marked OS's rootfs is missing, the initrd boots
the other OS instead of failing.

**Safety model:** an unverified boot image that hangs has no software
path back (recovery = mtkclient preloader mode), so the scripts default
to para = boot-recovery (TWRP sticky) until you explicitly boot the new
image, and every `boot` flash is backed up to `stock-dump/` first
(`bin/boot-switch.sh restore` rolls back). Rollback of p32 = reflash
`system.img`; Debian p29 is never written by these scripts.

Do **not** flash anything else from this repo yet — the artifacts build
and match the bring-up boot contract, but **NixOS now RUNS on glass**
(2026-09-07 phase-2 milestone: ssh to a NixOS shell over g_ether from
p32; see `docs/phase-2-on-glass.md` for the bootopt discovery + the
open TODO). The device runs the NixOS rootfs on p32 (Debian stays on
p29); the rootfs `growfs` (TODO P0), a few services and the
`nixos-rebuild` round-trip are the remaining on-glass work.

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
  libglvnd). wlroots 0.18.2 is now packaged too
  (`pkgs/wlroots-geminipda.nix` — pinned 0.18.2 from source, libinput
  backend + gles2 only) and gemwl (the compositor, `pkgs/gemwl.nix` +
  `services/desktop.nix`) is in the system closure. The nested
  session is now in-tree too (2026-09-07): labwc 0.8.3 +
  nixpkgs lxqt 2.4 as `lxqt-nested.service` (services/lxqt.nix) — the
  verified GeminiPDA LXQt/labwc stack. Remaining: on-glass
  verification; the Plasma-6-nested alternative stays an open A/B
  (kwin 6.3.6 / Plasma 6 — nixpkgs 26.11pre floats newer, unverified
  versions).
- **R3**: no DRM — the desktop is `gemwl` (custom wlroots compositor)
  with LXQt nested inside it on the LK framebuffer; packaged as
  services, not a display manager (`services/desktop.nix` + `services/lxqt.nix`,
  auto-start at boot; console-only boot via `systemctl disable gemwl lxqt-nested`).

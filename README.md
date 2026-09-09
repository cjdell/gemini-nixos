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
| `services/` | Device services: `gemini-pda.nix` (GPU/A72/battery units), `audio.nix` (PipeWire system session + S16 pin + speaker-amp), `wifi.nix` (internal CONSYS stack + USB auto-connect + NVRAM), `desktop.nix` (gemwl compositor unit), `lxqt.nix` (**nested LXQt desktop 2026-09-07**: labwc 0.8.3 hosting the nixpkgs lxqt 2.4 session inside gemwl — the verified GeminiPDA stack as NixOS services), `phosh.nix` (**Phosh mobile-shell desktop 2026-09-09**: phoc 0.54.0 nested the same way — the LXQt alternative, off by default, NOT yet on glass), `gemini-utils.nix`/`scripts/` (verified bring-up CLIs, ported verbatim) |
| `pkgs/mesa-geminipda.nix` | Mesa 25.0.7 + geminipda panfrost fork (Mali-T880 dma-buf import; phase 4 preview). Base = published upstream 25.0.7 archive fetched by hash + the tracked fork patch — nothing vendored (see `docs/library-deltas.md`). Also builds `libgbm` (needed by wlroots 0.18's gles2 renderer) |
| `pkgs/wlroots-geminipda.nix` | wlroots 0.18.2 pinned from source (the version the verified desktop used; nixpkgs in the pin floats 0.20.1) — libinput backend + gles2 renderer only (no DRM on this device) for gemwl, built against the mesa fork; `withDrmBackend=true` variant for labwc 0.8.3 (its wlr_drm_lease compile needs the header; never instantiated on hardware) |
| `pkgs/labwc-geminipda.nix` | labwc 0.8.3 pinned from source against the pinned wlroots 0.18.2 (the on-glass verified pair; nixpkgs floats labwc 0.20/wlroots 0.20) — the nested compositor hosting the LXQt session |
| `pkgs/phoc-geminipda.nix` | **phoc 0.54.0 (nixpkgs) with its wlroots 0.19.3 rebuilt against the fork libgbm (2026-09-09)** — the nested compositor hosting the Phosh shell (docs/phosh.md). Single-mesa GPU closure (the fork's libgbm resolves via wlroots' RUNPATH, readelf-verified); only non-cached drv the Phosh desktop adds |
| `pkgs/gemwl.nix` + `pkgs/gemwl/` | gemwl, the GPU-direct LK-framebuffer Wayland compositor (wlroots 0.18) + the tinytest xdg-shell smoke clients; sources byte-identical to the verified GeminiPDA `build/wayland/` files (tinytest-anim/tinytest listener-lifetime crash fixed 2026-09-07) |
| `pkgs/gemcli.nix` + `pkgs/gemcli/` | **gemcli — Rust device-control CLI (2026-09-08, ON GLASS gen20+)**: one native binary for the device functions the shell CLIs handle (backlight/battery/guard/a72/wdt/boot/gpu/speaker). clap+libc only; script-parity exit codes; selfcheck ALL PASS on the device; gpio v1 ioctl numbers fixed for v6.6 (docs/gemcli.md). Units NOT flipped yet |
| `pkgs/gemdemo.nix` + `pkgs/gemdemo/` | **gemdemo — minimal GLES 3.1 + ALSA template (0.3.0, deployed as gen34 2026-09-09)**: the hw-interaction skeleton that survived the EXODUS demoscene purge — ONE Rust file (src/main.rs): EGL ES 3.1 context on a winit Wayland surface (panfrost fork, Mali-T880), one spinning shaded triangle, a 440 Hz sine via cpal→gemini16 (S16@44.1k). The receipts (DSA→glGen, single-interleaved VAO, wl_egl_window, S16 wire state) are the file's header doc — copy file + derivation to start a real GL app. 0.2.0 “GEMINI: EXODUS” (gen33) + 0.1.0 “AETHER” sources live in git history. See docs/gemdemo.md. Run: `gemdemo` fullscreen / `gemdemo --windowed` (keys: q/esc quit) |
| `config/lxqt/` | The LXQt-session user configs (lxqt.conf, session.conf, labwc rc.xml + autostart, the vendored “Gemini” openbox themerc, desktop launchers) seeded to `/home/cjdell` (the desktop user, 2026-09-09) by `services/scripts/start-lxqt-nested` |
| `pkgs/speaker-amp.nix` + `pkgs/speaker-amp/` | Speaker-amp GPIO helpers (gpioout/spkamp); cc resolved via stdenv.cc.targetPrefix (cross + native) |
| `pkgs/gemini-firmware.nix` + `pkgs/gemini-firmware/` | Wi-Fi firmware: MT6630 CONSYS WMT blobs + RTL8821CU + factory NVRAM record |
| `patches/` | The geminipda Mesa fork patch (byte-for-byte the GeminiPDA submodule commit `ac19be0`) |
| `config/gemini.nix` | Stage-2 system configuration (headless + g_ether SSH, device services, Mesa fork + libglvnd) |
| `devices/planet-geminipda/kernel/` | Kernel derivation (mobile-nixos kernel-builder): published Linux v6.6 base (fetched by hash) + the tracked file-tree delta + the lean config; postInstall builds sramldo-smc.ko; rule-5 gate + O=-build fixes (see “Kernel phase” and `default.nix` comments) |
| `devices/planet-geminipda/kernel/delta/` | The bring-up source delta over upstream v6.6 (512 plain files; no patches — see “Kernel phase”) |
| `devices/planet-geminipda/kernel/config` | LEAN device config (default build; generated by `bin/prune-kernel-config.sh`) |
| `devices/planet-geminipda/kernel/config.full-329` | The exact on-glass #329 config (A/B + rollback reference) |
| `kernel/base` | git submodule pointer to upstream torvalds/linux @ v6.6 (`ffc253263a…`) — fetch on demand for reading base sources |
| `kernel/borrowed/` + `kernel-borrowed.nix` | Pre-2026-09-08 borrow scaffolding (kept as reference/rollback until the self-built kernel is glass-verified; no longer wired into any build) |
| `bin/sync-kernel-delta.sh` | Refresh `kernel/delta` from a bring-up fork checkout + byte-verify base+delta == rev (replaces bin/snapshot-kernel.sh) |
| `bin/prune-kernel-config.sh` | Generate the lean config from `config.full-329` (foreign SoCs/buses/media/debug cut lists) |
| `bin/dump-bootimg-header.sh` | Parse an AOSP v0 boot.img header (phase-0 geometry checks) |
| `bin/device-ssh.sh`, `bin/net-up.sh`, `bin/device-reboot.sh` | Host side of the g_ether link (root @ 10.15.19.82), auto link-up after power-on, WDT-EXRST remote reboot |
| `bin/boot-switch.sh` | Boot-target switching + boot-partition flash over adb/TWRP (status/twrp/android/debian/flash/restore; `debian` = para=boot-debian → p29, 2026-09-07) |
| `bin/flash-nixos.sh` | Full NixOS flash orchestration: converges to TWRP from any device state, flashes boot.img → `boot` and system.img → p32 (`userdata`, Android erased — Debian p29 untouched); verbs status/boot/rootfs/all/boot-nixos/debian; safe TWRP-sticky default |
| `bin/run-job.sh` | Detached job runner for long ops (flash waits, big builds) — never inline nohup/pgrep loops |
| `bin/deploy.sh` | **Workstation-style generation loop (2026-09-07 → native)**: builds the NATIVE aarch64 toplevel (root `--store local` distributed build to the 192.168.49.191 remote builder, pinned via gc-pin), `nix copy` delta → device store over ssh, profile switch + activate. Verbs status/build/deploy [PATH]/rollback [N]. Reboot lands on the new gen; old gens stay selectable — no reflash, no TWRP |
| `bin/device-rebuild.sh` | **The SAME loop, run ON the PDA** (2026-09-08, from a repo clone at `/root/gemini-nixos`): native aarch64 build straight into the device store (cache.nixos.org substitutes; the custom drvs compile locally only when changed) + profile switch/activate. Verbs status/build/switch [PATH]/rollback [N]/channels/gc. `build` refuses a dirty repo (rule 0). **Since 2026-09-09 the flake also exposes `nixosConfigurations.gemini`, so the plain `nixos-rebuild switch --flake .` loop works directly from the clone** (this script stays for its status/rollback/channels/gc conveniences) |
| `bin/device-repo.sh` | Seed + sync the repo between host and the device clone over g_ether as a git bundle (no github round-trip; works offline). Verbs seed/push/pull — directional, nothing silently lost |
| `bin/gc-pin.sh` | GC-root a build (NAME STORE_PATH | list | unpin) so host `nix-collect-garbage` can't sweep the aarch64 closure (happened once — gen3 silently rebuilt ~259 packages); milestone closures get a root-level root too (`sudo nix-store --add-root /nix/var/nix/gcroots/<name> -r <out>`) |
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
- **Nixpkgs**: pinned by **this flake** (`flake.nix`, since 2026-09-08)
  — no longer Mobile NixOS's npins pin. Chosen rev = the nixos-unstable
  **channel snapshot** `dc5d91f84032` (`26.11pre1068949`, cut 2026-09-07):
  hydra built+p published its FULL aarch64 closure, so the Qt6/LXQt set
  that compiled from source under the old npins rev (`0bb7ec54c848` —
  base-only coverage, qtbase 404 verified) now substitutes from
  cache.nixos.org. The MNX eval shim's `pkgs` argument carries it (the
  shim forbids `system` + `pkgs` together). Bump = take the rev behind
  `https://channels.nixos.org/nixos-unstable/git-revision`, re-verify the
  narHash (`nix flake prefetch github:NixOS/nixpkgs/<rev>`). Host
  tooling devShell stays on MNX's npins (x86_64, independent). **The
  device's `nix-shell -p` channel is pinned to this SAME rev**
  (`bin/device-rebuild.sh channels`, 2026-09-08): legacy `nix-shell -p`
  packages therefore match the running system and substitute from the
  cache (rule 9). When the flake pin moves, re-run `channels` on the
  device.
- **Kernel**: built in-repo from upstream Linux **v6.6** (kernel.org
tarball, fetch-pinned; base commit `ffc253263a…`, also the `kernel/base`
submodule) + the tracked delta (`kernel/delta` == geminipda-bringup @
`188aade69`, the #329 tree). The config is the LEAN device config
(`devices/…/kernel/config`, from `config.full-329` — the exact #329
config, kept for A/B). Still `CONFIG_CMDLINE_FORCE=y`; switching to
`boot.kernelParams` is the docs-R4 A/B step on hardware — the matching
params are already declared in `config/gemini.nix`).

## Kernel phase (SELF-CONTAINED since 2026-09-08: published base + delta tree; lean config)

The kernel is **built in-repo** — no borrowed #329 artifacts, no
GeminiPDA dependency, no vendored source (see
`docs/session-log.md` 2026-09-08 for the full receipt):

- **base** = the published upstream Linux **v6.6** release tarball
  (kernel.org, fetched by hash) — byte-identical to `git archive v6.6`
  of the geminipda-bringup base commit `ffc253263a…`, which is also
  pinned as a git submodule pointer at `kernel/base` (fetch on demand:
  `git submodule update --init --depth 1 kernel/base`);
- **delta** = `devices/planet-geminipda/kernel/delta/` — the 512 plain
  files the bring-up branch changes over v6.6 (457 added + 55 modified,
  no deletions; NO patch files — agents edit source directly).
  `v6.6-base + delta == geminipda-bringup @ 188aade69` (the on-glass
  #329 tree), verified byte-for-byte; refresh with
  `bin/sync-kernel-delta.sh`;
- **config** = the LEAN device config (`./config`, generated by
  `bin/prune-kernel-config.sh` from `config.full-329`, the exact #329
  config kept for A/B). Prunes hardware that can never exist on this
  unit: 4,213 → 1,655 enabled after olddefconfig (61 % fewer
  symbols); result: Image.gz 8.0 MiB (was 13.5), 388 modules (was
  1,165), DTB byte-identical to #329.

`kernel/borrowed/` (the #329 payload/DTB/module-tree/config) and
`devices/planet-geminipda/kernel-borrowed.nix` remain on disk as the
pre-verification reference/rollback — the boot image is now built from
the self-built kernel (NOT flashed yet as of 2026-09-08; on-glass A/B
pending). `bin/snapshot-kernel.sh` and the 225 MB source tarball are
retired (git history has them).

Mesa needs none of this: since 2026-09-07 its source is fetched by
hash from the published upstream archive by `pkgs/mesa-geminipda.nix`
(byte-stable canonical `/-/archive/` URL; see `docs/library-deltas.md`).

## Build

Build model = **native aarch64** (canonical since the 2026-09-08
native-aarch64 merge; the x86_64 cross toplevel is ABANDONED — it hit
nixpkgs cross walls, last one Qt6CoreTools missing for the lxqt scope;
docs/handover-2026-09-07-lxqt-native.md). Every drv is
system=aarch64-linux; builds run as root against the LOCAL store with
`--option builders @/etc/nix/machines --fallback` so the 192.168.49.191
remote builder (8-core Pi) compiles and the host pulls finished paths
back over ssh (`bash bin/deploy.sh build` = exactly this; long builds
under `bash bin/run-job.sh start <name> -- bash bin/deploy.sh build`):

```sh
sudo nix build --store local .#packages.aarch64-linux.default   # boot.img + rootfs.img (+ flash script)
sudo nix build --store local .#packages.aarch64-linux.bootimg   # boot.img only (self-built lean kernel)
sudo nix build --store local .#packages.aarch64-linux.kernel    # the kernel package alone (Image.gz+dtbs+modules)
sudo nix build --store local .#packages.aarch64-linux.rootfs    # rootfs.img (→ `linux` partition)
sudo nix build --store local .#packages.aarch64-linux.initrd    # minimal initrd (size measurement, docs R1)
sudo nix build --store local .#packages.aarch64-linux.mesa      # Mesa 25.0.7 + geminipda panfrost fork
```

Since 2026-09-08 the kernel is compiled in-nix (lean config: ~6 min
on the aarch64 builder; the full #329 config takes much longer — the
lean config drops 61 % of enabled symbols). Mesa is the other heavy
build (cached in the local store once built).

aarch64 note (historical): under the OLD npins rev,
`ffmpeg`/`ffmpeg-headless` defaulted to `withCudaLLVM = true` and failed
for aarch64 ("cuda_llvm requested but not found"), so
`config/gemini.nix` carried an overlay forcing `withCudaLLVM = false`.
**Removed 2026-09-08 with the repin**: the new pinned channel rev's
aarch64 ffmpeg builds are on the cache (hydra built the defaults), and
the override only forced non-cached drv hashes down the
pipewire/alsa-plugins subtree. Same story for the systemd `withLibBPF`
and openblas `dynamicArch` workaround overlays (old-rev/cross-era; see
`config/gemini.nix`). If a real build of this rev re-hits one of the
old bugs, re-add the specific override with a date + receipt.

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
| **`gemini-sleepd.service`** | **Silver side-button sleep/wake daemon (2026-09-08, docs/power-sleep.md)**: `gemcli sleep key` owns the button (KEY_SLEEP on mt6351-keys) and toggles the LIGHT sleep — **backlight off first (instant press feedback)**, A53 cpus 1-7 offline, keyboard-matrix + touch drivers unbound (the closed lid presses the keys), heavyweight services stopped (gemwl/LXQt, pipewire), wifi iface down + daemons killed (CONSYS chip stays powered — the ~29 s `echo off` teardown is too slow for a button; wake re-associates via `wifi auto`). ~1-2 s transitions, press debounce + queue drain (no flicker cascade). sshd + battery-guard + sleepd stay. No kernel suspend yet (no s2idle wake source — deep sleep = PMIC/kernel follow-up) |
| **`gemcli`** | **Rust consolidation of the device CLIs (2026-09-08, docs/gemcli.md)**: backlight/battery/charger/power/guard/a72/gpu/wdt-reboot/boot/speaker + `status` + read-only `selfcheck` parity harness. In the closure next to the scripts; units NOT flipped yet |

`sramldo-smc.ko` is loaded at boot via `boot.kernelModules` (the
kernel derivation builds it in `postInstall` and ships it under
`extra/`, depmod-indexed);
`busybox` + `i2c-tools` are system packages for the scripts and hand
use on the serial console. **gemcli** (2026-09-08) — the Rust
consolidation of the device CLIs (`pkgs/gemcli.nix`, docs/gemcli.md) —
ships in the same closure NEXT TO the scripts; no unit ExecStart has
been flipped yet (the on-glass `selfcheck` parity pass comes first).

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

**Desktop — gemwl + nested LXQt (phase 4 preview, in-tree 2026-09-07, ON GLASS 2026-09-08).**
The desktop is NOT a display manager: `gemwl.service` (services/desktop.nix) owns the LK framebuffer (/dev/gemfb, GPU-direct via panfrost + the mesa fork); the LXQt desktop runs NESTED inside it — `lxqt-nested.service` (services/lxqt.nix): labwc 0.8.3 (pkgs/labwc-geminipda.nix, wlroots 0.18.2 “wayland” backend) on gemwl's wayland-0 exporting wayland-1, hosting the nixpkgs lxqt 2.4 session (panel via wlr-layer-shell, pcmanfm-qt desktop, qterminal, pavucontrol-qt + qpwgraph audio GUIs). **The desktop sessions run as the `cjdell` user (config/gemini.nix users.users.cjdell) since 2026-09-09** — HOME=/home/cjdell, user configs/theme seeded from `config/lxqt/`, session bus in the cjdell-owned /run/lxqt-session; gemwl itself stays a root service (fbcon unbind + /dev/gemfb 0600 are root-only) with its socket made session-reachable. PipeWire sockets from audio.nix (/run/gemwl-audio, also cjdell-owned since 2026-09-09). Both units auto-start (wantedBy multi-user.target); console-only boot = `systemctl disable gemwl lxqt-nested`. **First on-glass run 2026-09-08 (native closure gen8)**: two repo bugs fixed — config-seed layout (store-hashed basenames, services/lxqt.nix sessionConfig) and the labwc wlroots missing the gbm allocator (-Dallocators=gbm, pkgs/wlroots-geminipda.nix); verified over ssh after a cold WDT reboot: gemwl+lxqt-nested active, NRestarts=0, panel/desktop/polkit/notificationd/qterminal all up, EGL on Mali-T880 (Panfrost), continuous compositing. Eyes-on-glass confirmed 2026-09-08 (user): the desktop renders — no flicker/uninitialised-LCD. **GPU stress test on glass 2026-09-08**: `gemdemo` (docs/gemdemo.md) runs in this session — windowed in labwc or fullscreen on gemwl — first sustained real load for the T880 fork.

**Phosh (mobile/phone shell) — the LXQt alternative, in-tree 2026-09-09, NOT yet on glass.** Phosh cannot sit on gemwl directly: gemwl is a minimal xdg-shell KIOSK compositor (no layer-shell / foreign-toplevel / session-lock / text-input, and no phoc-private protocol), so the SAME nested pattern hosts it — `phosh-nested.service` (services/phosh.nix) runs phoc 0.54.0 (pkgs/phoc-geminipda.nix: its wlroots 0.19.3 rebuilt against the fork libgbm, single-mesa GPU closure) with the wlroots “wayland” backend inside gemwl, listening on socket `phosh`, and `-E` execs the phosh shell binary ($out/libexec/phosh — NOT bin/phosh-session, which would start its own DRM phoc). Off by default — LXQt stays the default desktop; switching = `services.lxqtNested.enable = false; services.phoshDesktop.enable = true;` (assert-guarded: two nested desktops would stack on gemwl). GPU stays end-to-end fork-mesa: GTK4 (EGL ICD) → phoc (wlroots 0.19 gles2/gbm on renderD128) → gemwl (GPU blit to LK fb). Design, pins, receipts + the on-glass checklist: docs/phosh.md.

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
`nixos-rebuild` round-trip were the remaining on-glass work — the
round-trip is CLOSED (2026-09-08 on-device gens via
`bin/device-rebuild.sh`; since 2026-09-09 the real `nixos-rebuild
switch --flake .` works from the device clone — see the next section).

## On-device build/switch — `nixos-rebuild switch --flake .` (2026-09-09, flake `nixosConfigurations` added)

The PDA is a first-class flake target: a repo clone at
`/root/gemini-nixos` (sync with this host via `bin/device-repo.sh`
seed/push/pull or the github origin) can iterate the config and add
programs with NO host involved, using the real NixOS tool now:

```sh
# on the device (root, in the repo clone at /root/gemini-nixos):
nixos-rebuild list-generations --flake .     # what is installed / selectable
# edit config/gemini.nix, git add + commit, then:
nixos-rebuild build --flake .                # eval + build on the PDA (no activation)
nixos-rebuild switch --flake .               # profile switch + activate (no reflash)
# ...and the classic ad-hoc shell:
nix-shell -p pkgname        # pinned to the same nixpkgs rev as the flake (rule 9)
```

`--flake .` resolves to `.#nixosConfigurations.gemini` (the device
hostname). The flake exposes that configuration (single eval shared
with `packages.aarch64-linux.toplevel` — the SAME toplevel
derivation, verified 2026-09-09), so `nixos-rebuild` builds, sets the
system profile and runs switch-to-configuration exactly as
`bin/device-rebuild.sh` / `bin/deploy.sh` do by hand. Nothing extra
needed on the device: `nixos-rebuild` (the Python nixos-rebuild-ng —
the bash one is gone from nixpkgs at this pin) lands in the system
closure by default (`system.tools.nixos-rebuild.enable` =
`config.nix.enable`, itself default-true), and MNX's rootfs
postBootCommands created `/etc/NIXOS` + the system profile at first
boot. Every toplevel also records the git rev of the tree it was built
from (`system.configurationRevision` → `nixos-rebuild
list-generations` / `nixos-version --configuration-revision`; a dirty
tree shows as `<sha>-dirty`) — commit before switching so the
generation is a clean commit (rule 0). `bin/device-rebuild.sh` stays
as the convenience wrapper (status/rollback/gc + the `channels`
re-pin for `nix-shell -p`); rollback without it =
`nixos-rebuild --rollback switch --flake .`.

Builds are native aarch64 into the device store (store writes flow
through the socket-activated nix-daemon — `/nix/store` is bind-mounted
ro in the main namespace by design). The pinned nixpkgs rev substitutes
from cache.nixos.org over the device's wifi/USB NAT; only the custom
drvs (mesa fork, kernel, wlroots/labwc/gemwl, firmware) and config glue
compile locally — config tweaks switch in minutes, kernel/mesa changes
are long on the A72/A53 mix (prefer the host `deploy.sh` loop for
theirs). gen30 was built + switched entirely on the device (session
log 2026-09-08); a cold-reboot check is owed.

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
  verified GeminiPDA LXQt/labwc stack. ON GLASS 2026-09-08 (gen8,
  native closure): full session up after two repo fixes (config-seed
  layout; wlroots -Dallocators=gbm for labwc) + a cold-boot check.
  Remaining: nixpkgs repin DONE 2026-09-08 (flake pins the
  hydra-built channel rev `dc5d91f84032`, see Pins — Qt6/LXQt + the
  systemd/ffmpeg/openblas/libfm overrides' subtrees now substitute;
  dry-run: 1447→792 fetched paths and the toplevel's local compiles
  drop to ~242 config-glue + custom-pkg drvs). Repin build + deploy
  DONE 2026-09-08 (gen15 live; 11-min build vs hours; post-switch
  desktop healthy, NRestarts=0; eyes-on-glass DONE by user — session-
  log (4th)). Owed: cold-reboot gen check. The old-rev workaround
  overlays were pruned with the repin (see config/gemini.nix).
  The Plasma-6-nested alternative stays an open A/B (kwin 6.3.6 /
  Plasma 6 — nixpkgs 26.11pre floats newer, unverified versions).
- **R3**: no DRM — the desktop is `gemwl` (custom wlroots compositor)
  with LXQt nested inside it on the LK framebuffer; packaged as
  services, not a display manager (`services/desktop.nix` + `services/lxqt.nix`,
  auto-start at boot; console-only boot via `systemctl disable gemwl lxqt-nested`).

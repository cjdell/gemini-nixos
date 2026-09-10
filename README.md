# gemini-nixos — Mobile NixOS for the Gemini PDA

Phase 0/1/3 of porting the mainline-Linux bring-up of the Planet
Computers Gemini PDA (MT6797X "aeon") onto Mobile NixOS. The feasibility
study and phase plan live in
[docs/mobile-nixos-port-feasibility.md](docs/mobile-nixos-port-feasibility.md).

The device boots a stock MediaTek LK from a 16 MiB `boot` partition
(Android `boot.img` v0, gzip `Image.gz` + appended DTB, 2048-byte pages).
Since the **2026-09-10 repartition**, **TWRP (p1 `recovery`) + NixOS are
the only systems**: the NixOS rootfs lives on a single **58.0 GiB
`linux` partition (p27, ext4 label `NIXOS_SYSTEM`)** and `boot` (p22)
carries the one NixOS boot.img — Android and the Debian rootfs were
reclaimed into that partition (see `docs/repartition-android-space.md`
§12). There is no fastboot; flashing goes through the patched no-swipe
TWRP or `dd` over adb.

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
| `devices/planet-geminipda/kernel/` | Kernel derivation (mobile-nixos kernel-builder) + the working bring-up `.config`; `postInstall` builds the out-of-tree `sramldo-smc.ko` (A72 bring-up SMC) against the same tree. Delta adds **`geminipda-drm`** (2026-09-10, `delta/drivers/gpu/drm/tiny/`): a `simpledrm`-style **DRM/KMS** driver for the LK framebuffer (`/dev/dri/card0`, DSI connector + panel-orientation 90, shadow-plane blit + alpha fix) — the standards path GNOME needs |
| `modules/hardware-soc-mediatek-mt6797.nix` | Out-of-tree MT6797 SoC fragment (upstreaming = phase 6) |
| `services/` | Device services: `gemini-pda.nix` (GPU/A72/battery units), `audio.nix` (PipeWire system session + S16 pin + speaker-amp + the L/R-correcting virtual sink + `gemini-speakerd`), **`power-profiles.nix` (2026-09-10q: patched PPD + `gemini-power-profile` GNOME Power Mode → A72 — docs/power-modes.md)**, `wifi.nix` (**internal CONSYS bring-up + NVRAM + NetworkManager** — NM owns wlan0/wlan1 since 2026-09-10, `services.geminiWifi.useNetworkManager`; legacy wpa_supplicant/dhcpcd CLI preserved), `plumbing.nix` (**DE-agnostic plumbing 2026-09-10**: UPower battery/AC + udev backlight access + brightnessctl — docs/desktop-plumbing.md), `desktop.nix` (gemwl compositor unit), `lxqt.nix` (**nested LXQt desktop 2026-09-07**: labwc 0.8.3 hosting the nixpkgs lxqt 2.4 session inside gemwl — the verified GeminiPDA stack as NixOS services), `phosh.nix` (**Phosh mobile-shell desktop 2026-09-09, ON GLASS as DEFAULT 2026-09-10 [corrected 2026-09-10]**: phoc 0.54.0 nested the same way over gnome-session+gsd), `bluetooth.nix` (MT6630 CONSYS hci_stp, persistent + auto-powered), `gemini-utils.nix`/`scripts/` (verified bring-up CLIs, ported verbatim) |
| `services/gnome-apps.nix` | **GNOME app suite (2026-09-10)**: calculator/calendar/maps/clocks/weather/contacts/text-editor/Papers/Loupe/… as Wayland clients on the system profile (app-grid visible under Phosh/LXQt and under GNOME), plus Adwaita icons/schemas, GOA and geoclue2. |
| `services/gnome.nix` | **Vanilla GNOME desktop (2026-09-10, `services.gnomeDesktop.enable`, set **ON** in `config/gemini.nix`)**: the ordinary NixOS `services.desktopManager.gnome` + `displayManager.gdm` (Wayland, autologin) running on the KMS device. Requires the `geminipda-drm` boot.img (`/dev/dri/card0`). Force-disables gemwl/phosh/LXQt, keeps the custom PipeWire, loads panfrost after GPU power-on. **On glass 2026-09-10 and the three follow-up defects are CLOSED (2026-09-10k/l/m): sluggish UI (a ~99 % DRM-commit kworker from a redundant per-pixel alpha loop — gemdemo 6→74 fps), wrong keymap (gemini registered in the xkb registry + `XKB_CONFIG_ROOT` + locked dconf source) and 90°→180°-rotated touch (DT inverted-x+y). Receipts: `docs/session-log.md` 2026-09-10k/l/m; original analysis: `docs/handover-2026-09-10-gnome-perf-touch.md`.** Evidence: `docs/gnome-feasibility.md`. |
| `services/desktop-select.nix` | **Desktop/session selector (2026-09-10, build-level; niri added 2026-09-11)**: GNOME + COSMIC + niri are **co-installed GDM Wayland sessions** (only one owns the KMS panel per boot) plus a **console** mode. A persistent marker `/var/lib/gemini/desktop` is resolved *before* GDM by `gemini-desktop-apply.service` — `gnome`/`cosmic`/`niri` set the AccountsService session (GDM auto-login), `console` creates `/run/gemini-console` which GDM conditions on and stays on fbcon tty1. `gemcli session set gnome\|cosmic\|niri\|console [--apply\|--reboot]`. COSMIC 1.6.0 and niri 26.04 are aarch64-cache-verified at the flake pin; on-glass pending. **`docs/desktop-selection.md`**. |
| `pkgs/mesa-geminipda.nix` | **ONE Mesa (2026-09-10): a thin `.override` of the pinned nixpkgs Mesa 26.2.2** (panfrost) + the tracked T880 polygon-list delta `patches/mesa-panfrost-polygon-list-26.2.2.patch`; wired as `hardware.graphics.package` so the single glvnd ICD is `/run/opengl-driver/share/glvnd/egl_vendor.d/50_mesa.json`. The old 25.0.7 fork and its `/etc/glvnd` manifest are retired — together they made glvnd load two mesas in one compositor (cosmic-comp had `libgallium-25.0.7` + `libgallium-26.2.2`). Bundles `libgbm` (wlroots 0.18 gles2 / browsers). See `docs/library-deltas.md` |
| `pkgs/wlroots-geminipda.nix` | wlroots 0.18.2 pinned from source (the version the verified desktop used; nixpkgs in the pin floats 0.20.1) — libinput backend + gles2 renderer only (no DRM on this device) for gemwl, built against the mesa fork; `withDrmBackend=true` variant for labwc 0.8.3 (its wlr_drm_lease compile needs the header; never instantiated on hardware) |
| `pkgs/labwc-geminipda.nix` | labwc 0.8.3 pinned from source against the pinned wlroots 0.18.2 (the on-glass verified pair; nixpkgs floats labwc 0.20/wlroots 0.20) — the nested compositor hosting the LXQt session |
| `pkgs/phoc-geminipda.nix` | **phoc 0.54.0 (nixpkgs) with its wlroots 0.19.3 rebuilt against the fork libgbm (2026-09-09)** — the nested compositor hosting the Phosh shell (docs/phosh.md). Single-mesa GPU closure (the fork's libgbm resolves via wlroots' RUNPATH, readelf-verified); only non-cached drv the Phosh desktop adds |
| `pkgs/gemwl.nix` + `pkgs/gemwl/` | gemwl, the GPU-direct LK-framebuffer Wayland compositor (wlroots 0.18) + the tinytest xdg-shell smoke clients; sources byte-identical to the verified GeminiPDA `build/wayland/` files (tinytest-anim/tinytest listener-lifetime crash fixed 2026-09-07) |
| `pkgs/gemcli.nix` + `pkgs/gemcli/` | **gemcli — Rust device-control CLI (2026-09-08, ON GLASS gen20+)**: one native binary for the device functions the shell CLIs handle (backlight/battery/guard/a72/wdt/boot/gpu/speaker). clap+libc only; script-parity exit codes; selfcheck ALL PASS on the device; gpio v1 ioctl numbers fixed for v6.6 (docs/gemcli.md). Units NOT flipped yet |
| `pkgs/gemdemo.nix` + `pkgs/gemdemo/` | **gemdemo — minimal GLES 3.1 + ALSA template (0.3.0, deployed as gen34 2026-09-09)**: the hw-interaction skeleton that survived the EXODUS demoscene purge — ONE Rust file (src/main.rs): EGL ES 3.1 context on a winit Wayland surface (panfrost fork, Mali-T880), one spinning shaded triangle, a 440 Hz sine via cpal→gemini16 (S16@44.1k). The receipts (DSA→glGen, single-interleaved VAO, wl_egl_window, S16 wire state) are the file's header doc — copy file + derivation to start a real GL app. 0.2.0 “GEMINI: EXODUS” (gen33) + 0.1.0 “AETHER” sources live in git history. See docs/gemdemo.md. Run: `gemdemo` fullscreen / `gemdemo --windowed` (keys: q/esc quit) |
| `pkgs/gemini-exodus.nix` + `pkgs/gemini-exodus/` | **GEMINI: EXODUS (Director's Cut) — cinematic spacesynth + GPU stress test (1.0.0, 2026-09-11)**. The 0.2.0 demoscene (git `32ba356`, 60 fps on glass gen33) restored as a first-class package and upgraded: `--stress 0..3` load model (more of the same single-pass layers — never a new render pass), `--bench SECS [--chapter-secs S] [--json]` per-chapter frametime percentiles (avg/p1/worst), `--stats` frametime + GPU/A72 overlay, the GEMINI constellation and twin-sun finale motif, an enriched anthem (third-below harmony + timpani), best-effort **A72 render-thread pinning**, and the chapter-jump atomic race fix. Direct single-pass GLES 3.1 (no FBOs/bloom/SSAA); in the rootfs via `services/gemini-pda.nix`; host loop `bin/gemini-exodus-host-check.sh`. Build-level verified 2026-09-11; on-glass pass owed. See **`docs/gemini-exodus.md`** |
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
| `bin/boot-switch.sh` | Boot-target switching + boot-partition flash over adb/TWRP (status/twrp/android/flash/restore; `android` = clear para → boot `boot`, i.e. NixOS). The old `debian` verb was removed 2026-09-10 |
| `bin/flash-nixos.sh` | Full NixOS flash orchestration: converges to TWRP from any device state, flashes boot.img → `boot` and **streams** system.img → p27 (`linux`, ~58 GiB, does not fit TWRP's /tmp); verbs status/boot/rootfs/all/boot-nixos/grow-rootfs; safe TWRP-sticky default. The old `debian` verb was removed 2026-09-10 |
| `bin/repartition-nixos.sh` | **One-way repartition to TWRP + NixOS only (2026-09-10)**: verbs plan/backup/apply/verify/boot. Writes + byte-verifies an sgdisk-verified GPT (verified blobs in `stock-dump/repartition-20260910/`), streams the rootfs to the raw offset, flashes `boot`; documented in `docs/repartition-android-space.md` §12 |
| `bin/run-job.sh` | Detached job runner for long ops (flash waits, big builds) — never inline nohup/pgrep loops |
| `bin/deploy.sh` | **Workstation-style generation loop (2026-09-07 → native)**: builds the NATIVE aarch64 toplevel (root `--store local` distributed build to the 192.168.49.191 remote builder, pinned via gc-pin), `nix copy` delta → device store over ssh, profile switch + activate. Verbs status/build/deploy [PATH]/rollback [N]. Reboot lands on the new gen; old gens stay selectable — no reflash, no TWRP |
| `bin/device-rebuild.sh` | **The SAME loop, run ON the PDA** (2026-09-08, from a repo clone at `/root/gemini-nixos`): native aarch64 build straight into the device store (cache.nixos.org substitutes; the custom drvs compile locally only when changed) + profile switch/activate. Verbs status/build/switch [PATH]/rollback [N]/channels/gc. `build` refuses a dirty repo (rule 0). **Since 2026-09-09 the flake also exposes `nixosConfigurations.gemini`, so the plain `nixos-rebuild switch --flake .` loop works directly from the clone** (this script stays for its status/rollback/channels/gc conveniences) |
| `bin/device-repo.sh` | Seed + sync the repo between host and the device clone over g_ether as a git bundle (no github round-trip; works offline). Verbs seed/push/pull — directional, nothing silently lost |
| `bin/gc-pin.sh` | GC-root a build (NAME STORE_PATH | list | unpin) so host `nix-collect-garbage` can't sweep the aarch64 closure (happened once — gen3 silently rebuilt ~259 packages); milestone closures get a root-level root too (`sudo nix-store --add-root /nix/var/nix/gcroots/<name> -r <out>`) |
| `bin/flash-nixos.sh` `grow-rootfs` | Offline-grow the p27 `linux` rootfs to the full partition from TWRP (e2fsck + resize2fs, static musl e2fsprogs) — the recovery path for make_ext4fs-geometry fs the kernel can't online-grow (R13); images since 2026-09-07 grow on first boot via growfs-root |
| `docs/library-deltas.md` | Long-standing goal + the “published base + in-repo delta” pattern (mesa done; kernel & co next) |
| `docs/repartition-android-space.md` | Repartition history: the 2026-09-07 dual-boot plan (§9/§10) and **§12 the FINAL 2026-09-10 repartition — TWRP + NixOS only, single 58 GiB p27 `linux` rootfs** (GPT blobs, sha256, tooling, rollback) |
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
sudo nix build --store local .#packages.aarch64-linux.mesa      # patched nixpkgs Mesa 26.2.2 (panfrost + T880 delta)
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
| `gemini-boot-recovery` / `gemini-wdt-reboot [s]` | CLIs (in `gemini-pda-utils`) + hand-started systemd units (outstanding.md item 9): boot into TWRP via sticky para, and device-side WDT EXRST self-boot. **[updated 2026-09-10]** plain `systemctl reboot`/`poweroff` now work (kernel delta `drivers/power/reset/mt6797-power.c`; see `docs/power-states.md`), so `gemini-wdt-reboot` is the fallback, not the primary |
| `gemini-boot-debian` | **Historical (2026-09-07)**: writes para=`boot-debian` and reboots → Debian p29 via the shared initrd. Obsolete after the 2026-09-10 repartition (p29 no longer exists; the initrd falls back to NixOS). Kept as a receipt; removal from `services/gemini-pda.nix` is a pending cleanup |
| `pipewire` / `wireplumber` / `pipewire-pulse` | PipeWire media stack as ONE root system session (`/run/gemwl-audio`); the WirePlumber rule opens the MT6351 card through `pcm.gemini16` (`/etc/asound.conf`), which pins the S16-only analog path to S16_LE at the alsa-lib boundary (S32 plays as white noise, S24 is refused — verified) |
| `gemini-audio-defaults` | Applies the DL1→ADDA→HPL/HPR playback route + the persisted speaker/headphone output mode at boot (after `alsa-restore`) |
| `speaker` / `audio-output` | Built-in-speaker vs headphone output (speaker-amp pads 243/244 via the gpio chardev; no jack detection yet, so manual). **[2026-09-10q]** `audio-output` also sets the PipeWire default sink (`sync-default` retried at boot), and `gemini-speakerd` follows the default sink to drive the amp — so GNOME's Sound/Quick-Settings output picker is the amp toggle; the internal L↔R swap is corrected by the `gemini_speakers` virtual sink (docs/desktop-plumbing.md §Speakers). **[fixed 2026-09-11]** `audio-output`'s stored mode now actually persists (`/etc/gemini` created by tmpfiles + `mkdir -p`), and `gemini-speakerd` rewrites it from the observed sink so GNOME-only choices survive reboot |
| **`gemini-power-profile.service`** | **GNOME Power Mode → A72 cluster (2026-09-10q, ON GLASS, docs/power-modes.md)**: `gemcli profile watch` polls power-profiles-daemon and maps **performance ⇒ A72 up**, balanced/power-saver ⇒ A72 down. PPD's placeholder driver is patched to advertise `performance` (GNOME hides it otherwise). 20 s per-boot settle (a profile change applies immediately, bypassing it); sleep-aware; A72 ops flock-serialized. Verified on glass 2026-09-10q |
| **`gemini-speakerd.service`** | **Default-sink → speaker-amp follower (2026-09-10q, docs/desktop-plumbing.md §Speakers)**: `gemcli speaker watch` polls the PipeWire default sink; `gemini_speakers` ⇒ amp pads 243/244 ON, anything else ⇒ OFF (headphone-only). **[2026-09-11, ON GLASS gen17]** the watcher was dead on arrival — `wpctl inspect @DEFAULT_SINK@` marks the default node's props with `* ` and the parser never matched (so Headphones still played the speakers); fixed + verified, and it now persists the observed sink as the boot mode |
| `gemini-wifi-nvram` | Installs the factory NVRAM record (real MAC `00:09:34:5a:af:c1` + TX cal) to `/data/nvram/APCFG/APRDEB/WIFI` (tmpfs) before the Wi-Fi stack probes the chip — without it the MAC changes every power cycle |
| `gemini-wifi-internal` | Internal MT6630 CONSYS stack: mtk_wcn + wlan_gen3 (load order matters, B-33) + the WMT pwr-on → wlan0. Runs after `gemini-gpu-poweron` (the CONSYS chip's chrdev, major 226, collides with the GPU's if the wlan modules probe first) |
| `gemini-wifi-auto` | `wifi auto`: associate with the strongest saved profile (`/etc/wifi/profiles.conf`) + dhcpcd lease; silent no-op without profiles |
| `wifi` / `wifi-internal` | CLIs: USB RTL8821CU dongle (scan/connect/networks/forget) and the internal CONSYS stack (start/status/stop) |
| **`gemini-sleepd.service`** | **Silver side-button sleep/wake daemon (2026-09-08, docs/power-sleep.md)**: `gemcli sleep key` owns the button (KEY_SLEEP on mt6351-keys) and toggles the LIGHT sleep — **backlight off first (instant press feedback)**, A53 cpus 1-7 offline, **the A72 cluster powered down when it was up (and restored on wake) [2026-09-10q]**, keyboard-matrix + touch drivers unbound (the closed lid presses the keys), heavyweight services stopped (gemwl/LXQt, pipewire), wifi iface down + daemons killed (CONSYS chip stays powered — the ~29 s `echo off` teardown is too slow for a button; wake re-associates via `wifi auto`). ~1-2 s transitions, press debounce + queue drain (no flicker cascade). sshd + battery-guard + sleepd stay. No kernel suspend yet (no s2idle wake source — deep sleep = PMIC/kernel follow-up) |
| **`gemcli`** | **Rust consolidation of the device CLIs (2026-09-08, docs/gemcli.md)**: backlight/battery/charger/power/guard/a72/gpu/wdt-reboot/boot/speaker + **`profile` (2026-09-10q)** + `status` + read-only `selfcheck` parity harness. In the closure next to the scripts; units NOT flipped yet (the power/speaker daemons DO exec gemcli — sleep/speaker-watch/profile-watch) |

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
The desktop is NOT a display manager: `gemwl.service` (services/desktop.nix) owns the LK framebuffer (/dev/gemfb, GPU-direct via panfrost + the mesa fork); the LXQt desktop runs NESTED inside it — `lxqt-nested.service` (services/lxqt.nix): labwc 0.8.3 (pkgs/labwc-geminipda.nix, wlroots 0.18.2 “wayland” backend) on gemwl's wayland-0 exporting wayland-1, hosting the nixpkgs lxqt 2.4 session (panel via wlr-layer-shell, pcmanfm-qt desktop, qterminal, pavucontrol-qt + qpwgraph audio GUIs). **The desktop sessions run as the `cjdell` user (config/gemini.nix users.users.cjdell) since 2026-09-09** — HOME=/home/cjdell, user configs/theme seeded from `config/lxqt/`, session bus in the cjdell-owned /run/lxqt-session; gemwl itself stays a root service (fbcon unbind + /dev/gemfb 0600 are root-only) with its socket made session-reachable. PipeWire sockets from audio.nix (/run/gemwl-audio, also cjdell-owned since 2026-09-09). Both units auto-start while LXQt is the enabled desktop (wantedBy multi-user.target). [changed 2026-09-09] LXQt is now the ALTERNATIVE desktop — its module defaults OFF (Phosh is the default, next paragraph); console-only boot = `systemctl disable gemwl lxqt-nested`. **First on-glass run 2026-09-08 (native closure gen8)**: two repo bugs fixed — config-seed layout (store-hashed basenames, services/lxqt.nix sessionConfig) and the labwc wlroots missing the gbm allocator (-Dallocators=gbm, pkgs/wlroots-geminipda.nix); verified over ssh after a cold WDT reboot: gemwl+lxqt-nested active, NRestarts=0, panel/desktop/polkit/notificationd/qterminal all up, EGL on Mali-T880 (Panfrost), continuous compositing. Eyes-on-glass confirmed 2026-09-08 (user): the desktop renders — no flicker/uninitialised-LCD. **GPU stress test on glass 2026-09-08**: `gemdemo` (docs/gemdemo.md) runs in this session — windowed in labwc or fullscreen on gemwl — first sustained real load for the T880 fork.

**Phosh (mobile/phone shell) — the alternative nested desktop (was the default 2026-09-09 → 2026-09-10; displaced by GNOME, next paragraph).** Phosh cannot sit on gemwl directly: gemwl is a minimal xdg-shell KIOSK compositor (no layer-shell / foreign-toplevel / session-lock / text-input, and no phoc-private protocol), so the SAME nested pattern hosts it — `phosh-nested.service` (services/phosh.nix) runs phoc 0.54.0 (pkgs/phoc-geminipda.nix: its wlroots 0.19.3 rebuilt against the fork libgbm, single-mesa GPU closure) with the wlroots “wayland” backend inside gemwl, listening on socket `phosh`, and `-E` execs the phosh shell binary ($out/libexec/phosh — NOT bin/phosh-session, which would start its own DRM phoc). **[changed 2026-09-09] Phosh was the DEFAULT desktop until 2026-09-10** (`services.phoshDesktop.enable` defaulted true with `services.lxqtNested.enable` false); **since 2026-09-10 GNOME is the default and both nested desktops are off** (see next paragraph). The LXQt alternative = `services.phoshDesktop.enable = false; services.lxqtNested.enable = true;` (assert-guarded: two nested desktops would stack on gemwl). GPU stays end-to-end fork-mesa: GTK4 (EGL ICD) → phoc (wlroots 0.19 gles2/gbm on renderD128) → gemwl (GPU blit to LK fb). Design, pins, receipts + the on-glass checklist: docs/phosh.md. **On glass 2026-09-10 (gen61)**: two bring-up bugs fixed en route — the unit env was missing the NixOS `share/gsettings-schemas/<pkg>/` XDG dirs (gio aborted the shell: "No GSettings schemas are installed") and NixOS had no `phosh` PAM service for the lockscreen (security.pam.services.phosh = { }; passcode 0000 on users.users.cjdell).

**Vanilla GNOME — the DEFAULT desktop since 2026-09-10, ON GLASS and GPU-accelerated (2026-09-10).** **[updated 2026-09-10p: the Mesa fork is retired — the device now runs a single patched nixpkgs Mesa 26.2.2 (`patches/mesa-panfrost-polygon-list-26.2.2.patch`, wired via `hardware.graphics.package`); GNOME and COSMIC each load only that one glvnd ICD (verified gen10), which removed the dual-mesa `libgallium-25.0.7` + `libgallium-26.2.2` load. See `docs/library-deltas.md` + `docs/session-log.md` 2026-09-10v.]** GNOME 50 `gnome-shell`/`mutter` cannot run on the old stack: `mutter` 50.4 (the pin) keeps only the native **KMS** backend plus `--headless` — the X11 and nested backends were removed in GNOME 50 — and there was no **display-capable** `/dev/dri/cardN` (the LCD was the LK framebuffer `/dev/fb0`; panfrost is a 3D-only Mali with no display controller). Phosh is *not* GNOME Shell: it is a GTK4/libadwaita shell **client** over its own wlroots compositor (`phoc`), which is why it nests inside gemwl. So the standards path was taken: **`geminipda-drm`** (kernel delta, `drivers/gpu/drm/tiny/`, modelled on `simpledrm`) exposes the LK framebuffer as a normal KMS device (`/dev/dri/card0`, DSI connector + **panel orientation** property = Left Side Up, shadow-plane blit + alpha fix), and **`services/gnome.nix`** runs the ordinary NixOS GNOME + GDM modules on it. Rendering is **GPU-accelerated**: Mesa's `kmsro` — already built into the mesa fork, since panfrost is a renderonly driver — pairs the display-only `geminipda-drm` card with panfrost; **verified on glass** (`kmscube` on card0 → `OpenGL ES 3.1 Mesa 25.0.7`, `renderer: "Mali-T880 (Panfrost)"`; mutter logs `Added device '/dev/dri/card0' (geminipda-drm) using atomic mode setting`, `Created gbm renderer for '/dev/dri/card0'`, `GPU /dev/dri/card0 selected primary from builtin panel presence`, and holds both card0 + renderD128 open). `services.gnomeDesktop.enable = true` in `config/gemini.nix`; GDM auto-logs in `cjdell` and the session comes up unattended after a clean reboot (0 failed units). The nested gemwl/phosh/LXQt stack is force-disabled by the module and remains the buildable rollback (the kernel keeps `FB_GEMINIPDA`, so reverting needs only a re-deploy). GNOME **applications** are installed (`services/gnome-apps.nix`). Full receipts + the `panel_orientation` override and the geoclue/camera limitations: `docs/gnome-feasibility.md`. **[corrected 2026-09-10, user report] The first on-glass session was *sluggish* and the touchscreen rotated — all three causes are now fixed and confirmed on glass (2026-09-10k/l/m).** The sluggishness was NOT the Mesa-version mix (that hypothesis was demoted): it was `geminipda-drm`'s `force_alpha()` per-pixel `writeb` loop — 2.33 M barriered byte stores per full-screen update in the DRM commit worker, pinning it at ~100 % CPU (`gemdemo` 6 → 74 fps once removed; the loop was also redundant, as the plane advertises XRGB8888 and `drm_fb_blit` already writes `0xff` alpha). The keyboard was plain US and the Fn layer dead because GNOME Shell's `XkbInfo` (libxkbregistry) could not find `gemini` — an include dir is not enough; the layout must be registered in `rules/evdev.xml` (hence `pkgs/gemini-xkeyboard-config.nix` + `XKB_CONFIG_ROOT`, plus a locked dconf source). Touch first went from 90° to 180° off once the kernel reported the raw portrait frame; the sensor is mounted 180° to the panel, so the DTS now inverts both axes. Details, receipts and hashes: `docs/session-log.md` 2026-09-10k/l/m; the original analysis is `docs/handover-2026-09-10-gnome-perf-touch.md`.**

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

1. Build: `nix build .#packages.aarch64-linux.default` → `result/`
   with `boot.img` + `system.img` (the NixOS rootfs, ~8 GB with GNOME).
2. `bash bin/flash-nixos.sh status` — device state + local artifacts.
3. `bash bin/flash-nixos.sh boot` — backs up the current `boot`, flashes
   `boot.img` → p22 `boot` (16 MiB). Stays in TWRP.
4. `bash bin/flash-nixos.sh rootfs --yes` — **streams** `system.img` →
   p27 `linux` (58 GiB ext4, label `NIXOS_SYSTEM`; destroys the current
   NixOS rootfs). First boot auto-resizes the fs to fill p27 and
   rehydrates the Nix store. (Run under `bin/run-job.sh` — ~8 GB.)
5. `bash bin/flash-nixos.sh boot-nixos` — clear para + reboot: LK →
   initrd → p27 `linux` → NixOS stage-2.

**A repartition (TWRP + NixOS only) is a one-way operation** handled by
`bin/repartition-nixos.sh` — see `docs/repartition-android-space.md`
§12. After it there is only one OS, so no para-based OS switching.

**Safety model:** an unverified boot image that hangs has no software
path back (recovery = mtkclient preloader mode), so the scripts default
to para = boot-recovery (TWRP sticky) until you explicitly boot the new
image, and every `boot` flash is backed up to `stock-dump/` first
(`bin/boot-switch.sh restore` rolls back). Rollback of the rootfs =
reflash `system.img`; p27 `linux` is the only partition these scripts
write.

**NixOS RUNS on glass** (2026-09-07 phase-2 milestone; see
`docs/phase-2-on-glass.md` for the bootopt discovery + the open TODO)
and, since the 2026-09-10 repartition, on the single p27 `linux` rootfs.
The rootfs `growfs`, the services and the `nixos-rebuild` round-trip are
CLOSED (2026-09-08 on-device gens via `bin/device-rebuild.sh`; since
2026-09-09 the real `nixos-rebuild switch --flake .` works from the
device clone — see the next section).

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
- **R2 (resolved 2026-09-10)**: the Mesa delta is now a thin override of
  the pinned nixpkgs Mesa 26.2.2 (`pkgs/mesa-geminipda.nix` = panfrost +
  the tracked `patches/mesa-panfrost-polygon-list-26.2.2.patch`), wired as
  `hardware.graphics.package` so the whole device shares one mesa and one
  glvnd ICD (the 25.0.7 fork's dual-ICD setup was the bug). wlroots 0.18.2 is now packaged too
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

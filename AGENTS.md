# AGENTS.md — gemini-nixos: the GOLDEN repo for the Gemini PDA (Mobile NixOS + bring-up knowledge)

**Repo status: GOLDEN (declared 2026-09-07).** This repo is the primary,
self-contained home for the Gemini PDA (MT6797X) project going forward —
both the Mobile NixOS port and the hardware/boot knowledge base that
used to live in the sibling project `/home/cjdell/Projects/GeminiPDA`.
The intent is that gemini-nixos **completely replaces GeminiPDA**.
GeminiPDA is NOT reverted, deleted or edited-for-new-work; it is the
legacy source that is being folded into this repo (knowledge, receipts,
tooling, artifacts), after which it can be archived.

**Transitional rule (until the migration below completes):** a topic's
source of truth is wherever its latest content is. New knowledge is
written HERE first, with dates and receipts. Where this repo does not
yet carry a GeminiPDA doc's content, GeminiPDA remains the reference
for THAT topic — cite it with its path and port it here per the
migration plan so the pointer decays. Do not put new work in GeminiPDA;
mirroring a correction there is optional (legacy).

Read this file first, then `README.md`,
`docs/mobile-nixos-port-feasibility.md` and `docs/disaster-recovery/`.

## Migration status (GeminiPDA → gemini-nixos)

| # | Item | Status (2026-09-07) | Destination here |
|---|---|---|---|
| M1 | Hardware/boot receipt docs (boot-chain.md, hardware.md, flashing.md, wifi-consys.md, roadmap.md) | ⬜ planned — `docs/boot-process.md` + repartition doc are the in-repo seeds; carry `file:line` receipts + dates when porting | `docs/` |
| M2 | **Disaster-recovery knowledge + image ledger** | ✅ done 2026-09-07 | `docs/disaster-recovery/` (README · inventory · gather · drills) |
| M3 | stock-dump blobs (partition dumps, firmware zip, boot backups) | 🟡 partial — boot-critical set copied 2026-09-07; full bulk pending (see `docs/disaster-recovery/inventory.md` "pending copy") | `stock-dump/` (gitignored) |
| M4 | Recovery tooling (patched-mtkclient launcher, USB watcher, devshell pkg) | ✅ done 2026-09-07 | `bin/run-mtk.sh`, `bin/usb-watch.sh`; devshell `mtkclient` |
| M5 | Kernel + LK source trees (linux-6.6, gemini-lk, mesa fork work) | ✅ kernel done 2026-09-08 (published v6.6 base + tracked delta in `devices/planet-geminipda/kernel/`; source no longer lives in GeminiPDA); gemini-lk + mesa fork still legacy until vendored | `devices/planet-geminipda/kernel/`, `kernel/base` (submodule ptr) |
| M6 | Device services/rootfs files | ✅ (already ported as derivations/scripts — the port repo's original job) | `services/`, `pkgs/`, `bin/` |
| M7 | Session history | ⬜ new sessions log HERE only; old history stays in GeminiPDA | `docs/session-log.md` |

## Status (device, unchanged by the pivot)

Phases 0/1/3 done at the **build level** — the flake's `boot.img` +
`rootfs.img` build green and match the bring-up boot contract — and
**phase 2 is now on glass (2026-09-07 milestone: NixOS boots, sshd over
 g_ether from p32)**; the rootfs `growfs`, several services and the
`nixos-rebuild` round-trip were the remaining on-glass work (TODO:
`docs/phase-2-on-glass.md`). **2026-09-08: the round-trip is closed
BOTH ways** — host `bin/deploy.sh` (gens ≤ 28) and device-native
build/switch (`bin/device-rebuild.sh`; gen30 was built + switched
entirely on the PDA from a repo clone at `/root/gemini-nixos`;
`nix-shell -p` works, pinned to the flake's nixpkgs rev — see README
"On-device build/switch"). **2026-09-09: the flake exposes
`nixosConfigurations.gemini` (single eval shared with the toplevel
package), so the STOCK loop works from the device clone too —
`cd /root/gemini-nixos && nixos-rebuild switch --flake .` (the Python
nixos-rebuild-ng; in the device closure by default; nixpkgs dropped
the bash nixos-rebuild at this pin).** **2026-09-10: the unit was
repartitioned one-way to TWRP + NixOS only** — the old Android + Debian
+ p32 `userdata` layout is gone; the NixOS rootfs now lives on ONE
**58.0 GiB p27 `linux`** partition (ext4 label `NIXOS_SYSTEM`), with p1
`recovery` (TWRP) the only other bootable partition
(`bin/repartition-nixos.sh`; `docs/repartition-android-space.md` §12).

⚠️ **Boot.img cmdline field: KEEP `bootopt=64S3,32N2,64N2` in it** — LK
consumes it via `platform_parse_bootopt`; without it the boot hangs on
the LK logo (~15 s WDT loop) before any kernel output (discovered
2026-09-07; `config/gemini.nix` kernelParams + `docs/phase-2-on-glass.md`
§2a).

## Golden rules

0. **Version & commit hygiene.** Every artifact that goes to the device
   carries an identity, and every session records what it flashed:
   - the kernel is **built in-repo** (since 2026-09-08: published
     Linux v6.6 base + tracked delta + lean config — see the session
     log; the #329 borrow was retired and `kernel/borrowed/` is only
     an unflashed reference until glass verification);
   - Mesa 25.0.7 fork, wlroots 0.18.2, gemwl: exact pins in
     `flake.nix`/the pkgs derivations.
   Log one version line per flash in `docs/session-log.md` (kernel,
   boot.img hash, mesa/wlroots pins). Never flash a build you cannot
   identify. [added 2026-09-07, adapted from GeminiPDA rule 0]
1. **Record before you forget.** Any fact learned from source, a build,
   an eval error, or (eventually) a boot attempt belongs in a doc — with
   a date. This repo's log is `docs/session-log.md`; port/build facts go
   to `README.md` / `docs/mobile-nixos-port-feasibility.md`;
   *hardware/boot* facts go HERE (this repo is golden) — into
   `docs/boot-process.md` and the receipt docs being ported (M1); when
   you must cite a receipt not yet ported, quote it with its original
   `file:line` + date + source path so it survives the port.
2. **Never silently contradict a doc.** Fix it *and* note the correction
   (`[corrected YYYY-MM-DD]`). If unsure, add rather than replace, and
   mark it `unverified`. Claims about *upstream* state ("is in
   mainline") must carry the date they were verified.
3. **Prefer evidence over opinion.** Keep receipts: `file:line` for LK
   behaviour (carry the source path + commit when citing legacy docs),
   derivation/commit ids for nix behaviour (ours — nixpkgs floats, so
   pin + date every claim about it).
4. **Small, dated deltas beat big rewrites.** Append to the tables and
   logs; don't restructure docs casually.
5. **NEVER abuse the LCD panel/matrix (CORE RULE).** The unit's NT36672
   TDDI is initialized ONLY by LK; a badly-initialized panel shows the
   "uninitialised" flicker and can BURN the LCD matrix (permanent damage
   — observed 2026-09-04). For THIS repo:
   - The kernel must stay the **fbcon/exclude-display build** — the
     borrowed #329 payload is exactly that. When the in-repo kernel
     derivation (`devices/planet-geminipda/kernel/`) is finally used,
     build with the fbcon verb (EXCLUDE_DISPLAY=1 FBCON=1) and never
     merge the display-landmine drivers (mediatek-drm*, mtk-mmsys,
     phy-mtk-*, tps65132-regulator) into the image.
   - If the glass EVER shows the flicker (or a kernel/display change
     touches the stack), stop, get the device to TWRP (LK/TWRP drive the
     panel correctly) and fix the image before continuing.
   - The fb buffer is NOT the panel state: a valid framebuffer can sit
     behind a badly-initialized LCD. Judge the glass by eyes/TWRP, not
     by a capture. (Full story: legacy GeminiPDA session-log 2026-09-04;
     port pending M7.)
6. **Scripts over long ad-hoc commands (CORE RULE).** Any multi-step
   host/device operation needed more than once becomes a committed
   script in `bin/` the FIRST time (headers: what/why/usage; reuse
   `bin/device-ssh.sh` / the devshell helpers; fail loudly on missing
   tools). Long inline `ssh '...'` chains and host for-loops are
   throwaway EXPLORATION only — promote them before the session ends.
7. **Devshell-only CLIs — NEVER bare (CORE RULE).** The bare host PATH
   carries only git/nix/coreutils/ssh/usbutils-ish basics — verified
   NO python3/adb/make there (2026-09-07). Every project CLI (python3,
   adb, fastboot, mtkclient, …) lives ONLY in the flake devshell
   (`flake.nix` → `devShells.x86_64-linux.default`: python3, git,
   android-tools, usbutils, mtkclient). Reach them with
   `nix develop --command <tool> …` **from the repo root** (nix does NOT
   search upward for flake.nix). The device scripts self-re-exec inside
   the devshell when adb/lsusb are missing from the host PATH
   (`GEMINI_DEVSH_REEXEC` guards the loop). Never a bare `python3`/`adb`
   and never hard-code /nix/store paths (drift after GC).
8. **Long operations: `bash bin/run-job.sh start|wait`, NEVER inline
   nohup/pgrep loops (CORE RULE).** Anything >~30 s or that must keep
   working across tool calls: `bash bin/run-job.sh start NAME -- CMD`
   (returns immediately; setsid-detached) then
   `bash bin/run-job.sh wait NAME` (rc 0 ok / 1 failed / 2 still
   running → re-run wait). The old `nohup … & for … pgrep -f …` pattern
   stalls sessions: `pgrep -f` matches the polling shell's own cmdline →
   the loop spins until the tool timeout. Job state: `logs/jobs/<name>/`
   (gitignored); `wait-file LOG END_REGEX` salvages already-running ops.
   Long flash ops (rootfs push+dd) MUST run under run-job.
8b. **Poll, never long-sleep (agent rule).** While a job/build runs,
   poll with `bash bin/run-job.sh wait NAME` (it returns immediately with
   rc=2 while running) — do NOT `sleep 60`/`sleep 90` between checks
   (wasted wall-clock + stalled tool calls). If a tool needs a short
   settle between polls, cap sleeps at ~5–10 s. The `run-job.sh wait`
   rc protocol is the poll primitive: rc=0 done-ok, rc=1 done-failed,
   rc=2 still-running → re-run wait.
9. **Cache-healthy nixpkgs pins (CORE RULE, added 2026-09-08).** Every
   nixpkgs pin must be one hydra built the FULL closure for — always the
   nixos-unstable (or a release) CHANNEL snapshot, never a raw
   master/random commit: channel revs are only cut after hydra's
   complete build, so the big closures (Qt6/LXQt — the multi-hour
   compiles) substitute from cache.nixos.org. Raw commits get per-commit
   trunk-combined (base) coverage only — verified 2026-09-08: the old
   npins rev `0bb7ec54c848` qtbase 404'd for x86_64 AND aarch64 (Qt6/
   LXQt compiled every time); the channel rev `dc5d91f84032`
   (26.11pre1068949) has qtbase/qtwayland/lxqt-*/systemd/pipewire
   narinfos all 200. Pin the rev behind
   `https://channels.nixos.org/nixos-unstable/git-revision` (or the
   release-branch equivalent) + narHash from `nix flake prefetch
   github:NixOS/nixpkgs/<rev>`. VERIFY before committing with
   `nix path-info --store https://cache.nixos.org <outPath>` on the
   heavy packages eval'd at the candidate rev (qtbase first) — curl
   narinfo 404s are NOT evidence (proxy); use nix's own HTTP client.
   Also: don't let workaround overlays diverge from the channel
   defaults — each override forces non-cached drv hashes down its
   subtree (the systemd/ffmpeg/openblas/libfm overrides pruned with the
   2026-09-08 repin cost 137+ builds; receipts in config/gemini.nix +
   docs/session-log.md).

## Where things live

| Concern | File |
|---|---|
| Repo purpose, layout, boot chain, flash flow, phase status | `README.md` |
| Feasibility study + the phased plan (phase table = roadmap) | `docs/mobile-nixos-port-feasibility.md` |
| Plain-language boot explainer (receipt pointers) | `docs/boot-process.md` |
| **Phase-2 on-glass knowledge + TODO** (2026-09-07 milestone: bootopt discovery, recovery receipts, not-quite-working list) | `docs/phase-2-on-glass.md` |
| **NixOS rootfs layout** — TWRP + NixOS only since 2026-09-10 (single 58 GiB p27 `linux`; supersedes the 2026-09-07 dual-boot plan §9/§10) | `docs/repartition-android-space.md` (§12) |
| "Published base + in-repo delta" pattern (mesa done; kernel next) | `docs/library-deltas.md` |
| **What was actually tried / happened** (dated entries; golden log) | `docs/session-log.md` |
| **Disaster recovery** — full-flash-erase → TWRP playbook (levels 0–2), image ledger + sha256, gather checklist, drills | `docs/disaster-recovery/` (README · inventory · gather · drills) |
| Flake entry; Mobile NixOS pin (`2c132754`); **nixpkgs pinned in-flake** (`dc5d91f84032`, channel rev — see README Pins); devShell (host x86_64, MNX npins) | `flake.nix` |
| Out-of-tree device definition (boot.img geometry, borrowed kernel, minimal initrd wiring) | `devices/planet-geminipda/` |
| Stage-2 system config (headless + g_ether SSH, services, mesa fork, systemd-BPF/cudaLLVM overlays) | `config/gemini.nix` |
| SoC fragment (out-of-tree MT6797) | `modules/hardware-soc-mediatek-mt6797.nix` |
| Device services: GPU poweron / A72-up / battery-guard / WDT reboot | `services/gemini-pda.nix`, `services/scripts/` |
| **Silver-button clamshell sleep/wake** — light sleep (backlight/cpus/inputs/services off) + KEY_SLEEP daemon; deep-sleep (s2idle) follow-up | `services/gemini-pda.nix` (`gemini-sleepd`) + `pkgs/gemcli/src/sleep.rs` + **`docs/power-sleep.md`** |
| **Power states — reboot/poweroff FIXED (research 2026-09-10, on glass 2026-09-10)**: `systemctl reboot` limboed (arm64 `machine_restart` no-handler halt) and poweroff was refused; now the delta driver `drivers/power/reset/mt6797-power.c` registers LK's full WDT SWRST restart (priority 200) + the MT6351 `RTC_BBPU`=0x4309 poweroff over the pwrap regmap. Fork rev `06fd13e11`, boot.img `2fbca314…`; both drivers bind and the transitions verified. **Gotcha:** the shared TOPRGU block at `0x10007000` must be mapped with `devm_ioremap()`, not `devm_platform_ioremap_resource()` (the latter claims it and makes `mtk_wdt` fail `-EBUSY` → watchdog boot loop) | **`docs/power-states.md`** |
| Audio (PipeWire S16 path), Wi-Fi (CONSYS+USB), **GNOME (vanilla, the DEFAULT desktop since 2026-09-10, GPU-accelerated via KMS+kmsro → panfrost)**, nested `gemwl` + **Phosh (phoc → phosh shell; alternative, was default 2026-09-09→10)**, **LXQt nested (labwc → lxqt-session; alternative)** | `services/gnome.nix` (default), `services/desktop.nix`, `services/lxqt.nix`, `services/phosh.nix`, `services/audio.nix`, `services/wifi.nix` (+ `services/scripts/start-lxqt-nested`, `config/lxqt/`; + `services/scripts/{prepare-phosh-session,start-phosh-shell}`) |
| **Desktop plumbing — DE-agnostic system services (2026-09-10)**: battery/AC via **UPower** (kernel Battery supply `bq25890-battery-N` — no fuel gauge, capacity voltage-derived; upower policy Ignore because gemini-battery-guard owns poweroff), Wi-Fi via **NetworkManager** (`services.geminiWifi.useNetworkManager`, default true; wlan0 CONSYS + wlan1 dongle, usb0 unmanaged, home networks as NM profiles), backlight **udev chmod 0666** + `brightnessctl` (no logind session for the system-service desktops). Bluetooth already standard (bluez). **2026-09-10p: GNOME audio + Fn media keys wired** — the GNOME session redirects `PULSE_SERVER`/`PIPEWIRE_RUNTIME_DIR` to the one system PipeWire session (`/run/gemwl-audio`; Settings→Sound + gsd volume keys), and the Fn layer is bound by **xkb keycode** in mutter (`<Mod5>0x36/0x37/0x38/0x39/0x1c` — a *keysym* accelerator resolves at level 0 and can never match Fn). **gen62 deployed + NM/upower/backlight verified on glass 2026-09-10** (battery icon still needs the new boot.img). **Touch (2026-09-10): a real 10-point multitouch `wl_touch` device, no cursor** — gemwl forwards every finger (`wlr_seat_touch_notify_*`, `GEMWL_TOUCH_POINTER_EMU=1` restores the old pointer emulation); the nested compositor's wlroots wayland backend (phoc 0.19.3 / labwc 0.18.2) re-emits it, so phosh/LXQt apps get native touch + pinch (protocol chain verified in journals + synthetic-finger test; on-glass finger test pending) | **`docs/desktop-plumbing.md`** (§Touch), `services/plumbing.nix`, `services/wifi.nix`, `pkgs/gemwl/gemwl.c` |
| **Touch verification tools** (2026-09-10): `bin/touch-inject.c` (writes raw Protocol-B events into the NT36772 evdev node — tap/swipe/pinch; **aarch64 `input_event` is 24 bytes, not 8**) + `bin/touch-probe.c` (WIP wl_touch client; blocked on the wayland ≥1.23 unified-`wl_interface` header change) | `bin/touch-inject.c`, `bin/touch-probe.c` (gotchas: `docs/session-log.md` 2026-09-10c) |
| **Keyboard/keybinding verification tool** (2026-09-10p): `bin/kb-inject.c` — writes raw EV_KEY chords (`fn+c`, `fn+v`, `fn+b`, `fn+n`, …) to the keyboard's evdev node, so the Fn media-key bindings can be exercised over ssh (v6.6 `evdev_write` DOES reach libinput/mutter) | `bin/kb-inject.c` (+ `docs/desktop-plumbing.md` §Volume) |
| **Keyboard verification tool** (2026-09-10m): `bin/wl-keymap-dump.c` — binds wl_seat→wl_keyboard and prints the xkb keymap the compositor actually ships clients (proved GNOME was sending plain US despite correct gsettings/env; `--grep` for one key). Build: `gcc $(pkg-config --cflags --libs wayland-client) bin/wl-keymap-dump.c` | `bin/wl-keymap-dump.c` |
| **Bluetooth bring-up + tools** (MT6630 CONSYS BT half: hci_stp port of the vendor 3.18 drv_bt → 6.6, WAK-line wake fix, rx-stall root cause; **persistent service + GUI/CLI on glass 2026-09-09m** — bluetoothd on the system bus auto-powered at boot, bluetoothctl/btmgmt CLIs, blueman-manager + SNI tray applet) | **`docs/bluetooth-bringup.md`** (+ `docs/session-log.md` 2026-09-09k/l/m; driver in the kernel delta `drv_bt/`; service module `services/bluetooth.nix`) |
| Keyboard layouts — VT console map + desktop xkb: console `console.keyMap` (config/gemini.nix); desktop layout "gemini" packaged as an xkbcommon include dir for the gemwl/lxqt-nested units via `XKB_CONFIG_EXTRA_PATH` (+ `XKB_DEFAULT_LAYOUT=gemini`). **GNOME needs more** (2026-09-10m): gnome-shell validates the input source against the **xkb registry**, so it also needs `pkgs/gemini-xkeyboard-config.nix` (xkeyboard-config copy + `symbols/gemini` + a `<layout>` entry in `rules/evdev.xml`) selected via `XKB_CONFIG_ROOT` — see the GNOME row | `config/keymaps/gemini-uk.map`, `config/xkb/symbols/gemini`, `pkgs/gemini-xkb.nix`, `pkgs/gemini-xkeyboard-config.nix` |
| **Rust device-control CLI** (backlight/battery/guard/a72/wdt/boot/gpu/speaker; script-parity; units NOT flipped until the on-glass `selfcheck` pass) | `pkgs/gemcli.nix` + `pkgs/gemcli/`; migration plan + parity recipe: `docs/gemcli.md` |
| Mesa 25.0.7+geminipda fork / wlroots 0.18.2 pin / gemwl pkgs / **labwc 0.8.3 pin** / **phoc 0.54.0 (fork-gbm wlroots 0.19 override)** | `pkgs/{mesa-geminipda,wlroots-geminipda,gemwl,labwc-geminipda,phoc-geminipda}.nix` + `patches/` |
| **Phosh desktop bring-up design + receipts** (phoc-nested architecture, the “can gemwl host phosh?” answer, GPU chain, on-glass checklist) | **`docs/phosh.md`** (+ `docs/session-log.md` 2026-09-09o) |
| **Vanilla GNOME — now the DEFAULT desktop, ON GLASS 2026-09-10, GPU-accelerated** (was only a feasibility study earlier that day): GNOME 50 `mutter` is KMS-only (X11 + nested backends removed), so `gnome-shell` cannot run on the LK framebuffer — not a Phosh-style protocol gap. Solved by exposing the LK fb as a KMS device + Mesa kmsro. App suite + session + receipts | **`docs/gnome-feasibility.md`** + `services/gnome.nix` + `services/gnome-apps.nix` |
| **`geminipda-drm` — LK framebuffer as DRM/KMS** (2026-09-10): delta `drivers/gpu/drm/tiny/geminipda-drm.c` (simpledrm-modelled) → `/dev/dri/card0`, DSI connector + panel-orientation 90, shadow-plane blit + alpha `0xff` fix; config emitted by `bin/prune-kernel-config.sh` (`CONFIG_DRM_GEMINIPDA=m`); rule-5 gate still passes. **Flashed + verified on glass 2026-09-10** (boot.img sha256 `1d2f350a…`) | `devices/planet-geminipda/kernel/delta/drivers/gpu/drm/tiny/` + `kernel/config` |
| **Vanilla GNOME desktop** (2026-09-10, **default ON**): standard NixOS `services.desktopManager.gnome` + `displayManager.gdm` on the KMS device; force-disables gemwl/phosh/LXQt, keeps custom PipeWire, loads panfrost after GPU power-on. Rendering is **GPU-accelerated** via Mesa `kmsro` (already in the mesa fork: pairs the display-only `geminipda-drm` card with panfrost). On glass 2026-09-10: mutter primary = card0, unattended boot, 0 failed units. **Sluggishness FIXED 2026-09-10k** (the ~99 % kworker was `geminipda-drm`'s redundant per-pixel alpha loop in the DRM commit worker — deleted; gemdemo 6→74 fps). **Keyboard FIXED 2026-09-10m**: gnome-shell's `XkbInfo` (libxkbregistry) needs gemini registered in `rules/evdev.xml`, not just an include dir — hence `pkgs/gemini-xkeyboard-config.nix` + `XKB_CONFIG_ROOT` + a locked dconf source; the keymap mutter ships now has `AE01=[1,!,|,F1]`, `AE03=[3,£,\,F3]`, `RALT=ISO_Level3_Shift`. **Touch**: kernel now reports raw portrait, sensor is 180° to the panel (DT inverted-x+y) — **confirmed on glass 2026-09-10m** | `services/gnome.nix` (`services.gnomeDesktop.enable`, set true in `config/gemini.nix`) |
| **GNOME perf/touch handover** (2026-09-10; **Issue 1 = perf RESOLVED 2026-09-10k**: the old "two Mesa versions + idle panfrost" guess was demoted — the real cause was the `geminipda-drm` alpha loop, now deleted from the kernel delta; touch 90° off = gemwl-era DT touch transform vs KMS panel-orientation, still open) | **`docs/handover-2026-09-10-gnome-perf-touch.md`** |
| **gemdemo minimal GLES 3.1 + ALSA template** (0.3.0, 2026-09-09: the 0.2.0 “GEMINI: EXODUS” spacesynth demoscene — ~5200 lines/10 modules, 60 fps on glass — was judged useless except as hw-interaction proof and stripped to ONE Rust file: spinning shaded triangle + 440 Hz sine via the verified EGL→panfrost + `gemini16` S16@44.1k paths; the receipts that survive it (DSA→glGen, single-interleaved-VAO rule, wl_egl_window, S16 wire state) are the template's doc; **deployed as gen34**) | `pkgs/gemdemo.nix` + `pkgs/gemdemo/src/main.rs` (the single file) + **`docs/gemdemo.md`** (template guide + receipt list; 0.2.0/0.1.0 sources live in git history) |
| Borrowed kernel #329 reference (kept until the self-built kernel is glass-verified; NOT wired into any build since 2026-09-08) | `kernel/borrowed/` (payload, DTB, module tree, sramldo-smc.ko, config) + `devices/planet-geminipda/kernel-borrowed.nix` |
| Kernel source: published v6.6 base (fetch-pinned) + tracked delta + lean config | `devices/planet-geminipda/kernel/` (`default.nix`, `delta/`, `config`, `config.full-329`) + `kernel/base` submodule pointer |
| **DRM/KMS driver for the LK framebuffer** (the standard-device enabler; GNOME prerequisite) | `devices/planet-geminipda/kernel/delta/drivers/gpu/drm/tiny/geminipda-drm.c` |
| Kernel config pruning + delta sync tools | `bin/prune-kernel-config.sh`, `bin/sync-kernel-delta.sh` (replaces the retired `bin/snapshot-kernel.sh`) |
| **Rootfs ext4 image builder shim** (R13: growable mke2fs geometry; **2026-09-10: re-execs under fakeroot + `chown -R 0:0` so the image gets root-owned inodes — without it a fresh install has NO WiFi (NM rejects non-root plugin files) and logrotate fails**) | `pkgs/make-ext4fs-shim.nix` (wired via `config/gemini.nix` nixpkgs overlay) |
| boot.img header inspection | `bin/dump-bootimg-header.sh` |
| **Recovery tooling** — patched-mtkclient launcher (preloader/BROM), USB-state watcher | `bin/run-mtk.sh`, `bin/usb-watch.sh` (+ devshell `mtkclient` = store pkg + DAs) |
| **g_ether net-up / SSH / WDT-EXRST reboot** (host side) | `bin/net-up.sh`, `bin/device-ssh.sh`, `bin/device-reboot.sh` |
| **Boot-target switching + boot-partition flash** (adb/TWRP; twrp/android/flash/restore; the `debian` verb was removed 2026-09-10) | `bin/boot-switch.sh` |
| **Full NixOS flash orchestration** (converge-to-TWRP from any state, boot + stream rootfs → p27 `linux`) | `bin/flash-nixos.sh` (verbs incl. `grow-rootfs` — offline p27 fs growth from TWRP, R13) |
| **One-way repartition to TWRP + NixOS only** (2026-09-10; plan/backup/apply/verify/boot; byte-verified GPT + streamed rootfs) | `bin/repartition-nixos.sh` |
| **Build/switch/rollback generations like a workstation** (native-aarch64 distributed build → delta `nix copy` → device profile switch + activate; NO reflash) | `bin/deploy.sh` (status/build/deploy/rollback) |
| **Same loop, run ON the PDA** (2026-09-08, gen30): build straight into the device store (cache substitutes; custom drvs compile locally when changed) + profile switch/activate, from a repo clone at `/root/gemini-nixos`; `channels` pins the `nix-shell -p` nixpkgs to the flake rev. **2026-09-09: the flake's `nixosConfigurations.gemini` makes the stock `nixos-rebuild switch --flake .` work from the clone — the script is now the convenience wrapper (dirty gate, status/rollback, channels)** | `bin/device-rebuild.sh` (status/build/switch/rollback/channels/gc); clone sync: `bin/device-repo.sh` (seed/push/pull over g_ether as a git bundle) |
| **GC-pin builds** (host `nix-collect-garbage` protection — every deploy pins itself; list/unpin) | `bin/gc-pin.sh` |
| **Detached job runner** (rule 8) | `bin/run-job.sh`; state `logs/jobs/` |
| Device partition backups pulled over adb/dd (gitignored; nvram/IMEI private — never commit) | `stock-dump/` (ledger + copy status: `docs/disaster-recovery/inventory.md`) |
| Legacy hardware/boot receipts (port pending M1) | `/home/cjdell/Projects/GeminiPDA/docs/` (read-only reference until ported) |

## When to update what

- **Port decision / build-level delta** (eval error, derivation quirk,
  new overlay) → this repo's docs + derivation comments; a dated entry
  in `docs/session-log.md`.
- **Milestone status** (phase table in the feasibility doc, README
  status) → update both; keep "build-level vs on-glass" explicit.
- **New hardware/boot fact or correction to one** → THIS repo
  (golden): `docs/boot-process.md` or the receipt doc being ported
  (M1), with `file:line` + date; if the same claim exists in a legacy
  GeminiPDA doc, port-fix it here and optionally leave a one-line
  `[superseded — see gemini-nixos …]` pointer there.
- **A flash/boot attempt happened** → `docs/session-log.md` (MANDATORY:
  versions flashed, hashes, exact behaviour, console output) +
  feasibility doc §9 phase notes.
- **Pins moved** (Mobile NixOS rev/narHash, **nixpkgs flake pin** (channel rev, flake.nix), kernel
  snapshot) → `flake.nix`/README "Pins" + date it (npins drift breaks
  the verified graphics stack — R2; the flake nixpkgs pin should track
  the nixos-unstable CHANNEL rev so hydra's full closure stays cached).
- **DR ledger changed** (a dump taken/verified/copied) →
  `docs/disaster-recovery/inventory.md` + a version line in the log.

## Device operations cheat sheet (mechanisms verified 2026-08-30/31 on
the legacy project; receipts still in GeminiPDA docs until M1 ports them)

**AFTER ANY DEVICE POWER-ON/POWER-CYCLE (GOLDEN RULE):** the host side
of the g_ether link is DOWN (the gadget iface disappears on power-off).
`bash bin/device-ssh.sh '<cmd>'` auto-runs `bin/net-up.sh` (passwordless
sudo) — root shell @ **10.15.19.82** (host 10.15.19.1/24 on
`enp10s0f4u1u2`, renamed per USB port; the device auto-configures its
side at boot). Hand-rolled ssh needs `sudo bash bin/net-up.sh` first.

**Remote reboot of a running Linux:** software resets POWER THE UNIT OFF
(verified 2026-08-31) — only the LK-configured WDT EXRST path
self-boots: `busybox devmem 0x10007004 32 0x48` (2 s WDT). From the host:
`bash bin/device-reboot.sh`. Device-side (NixOS rootfs): the
`gemini-wdt-reboot` unit does the same.

**Boot targets** (adb state machine in `bin/boot-switch.sh`; para = p2
of the largest mmcblk, 32-byte command at offset 0):
- `boot-recovery\0` + 18 zero bytes → **TWRP on every power-on (sticky —
  TWRP does not clear it)**; 32 zero bytes → NORMAL boots the `boot`
  image's **NixOS branch (p27 `linux` — the default)**. Since the
  2026-09-10 repartition TWRP + NixOS are the only systems, so
  `boot-debian` is gone (the initrd still treats it as non-NixOS and
  falls back). Only `para` and `boot` are ever written; never
  nvram/proinfo/protect*.
- `bash bin/boot-switch.sh status|twrp|android|flash [img]|restore`.
  NOTE: `android` = para-clear + reboot → NORMAL → boots the NixOS p27
  default (the historical name is kept). "Linux" is not a separate slot:
  our kernel goes in
  `boot` itself (boot2/boot3 are gone).
- Device running Linux (no adbd — the current real state): converge via
  `bin/flash-nixos.sh` (para write over ssh + WDT EXRST → TWRP); a fresh
  System is flashed with `flash-nixos.sh rootfs` (p27), or the whole
  layout redone with `bin/repartition-nixos.sh`.
- A hung boot image has NO software path back (para cleared = normal
  boot) → recovery = mtkclient preloader mode (`bin/run-mtk.sh`;
  full playbook `docs/disaster-recovery/drills.md`) — this is why the
  SAFE test cycle keeps para = boot-recovery (TWRP sticky) until the
  image is verified, and why `stock-dump/boot-*.img` backups are kept
  (restore = one adb command from TWRP).

**Flash pipeline (no fastboot on this device):** images go to partitions
from the patched no-swipe TWRP (root adbd) by-name paths:
`/dev/block/platform/mtk-msdc.0/11230000.msdc0/by-name/{boot,linux,para}`
(and `recovery`).
NixOS rootfs → p27 `linux` (58.0 GiB, ext4 label `NIXOS_SYSTEM` — the
single system partition since 2026-09-10). Boot image → p22 `boot`
(16 MiB).
Orchestrated by `bin/flash-nixos.sh
status|boot|rootfs|all|boot-nixos|grow-rootfs` (see its header for the safety
model + run-job usage).

**Battery/charger truth** (OS-dependent; verified live on the legacy
project): TWRP sysfs is STALE — read dmesg `[PE+]Ibat=..`; Linux:
`/sys/class/power_supply/bq25890-charger-0` (+ `bq25896-raw.sh` raw ADC
over i2c). ICHGR = 0 is NORMAL when load ≥ input power (power-path);
scale 50 mA/step. No fuel gauge → voltage thresholds only (guard
<3.65 V, orderly poweroff <3.50 V). `i2cget -f -y 0 0x6b` — the `-f` is
MANDATORY (kernel driver claims 0x6b).

## Conventions

- **Statuses:** ✅ done/verified · 🟡 built but not on hardware/partial ·
  ⬜ planned · 🔴 blocked. Update the phase table
  (`docs/mobile-nixos-port-feasibility.md` §9) + README status.
- **Dates:** `YYYY-MM-DD`; header of each doc carries "Last updated".
- **Nix files:** keep formatted (`nixpkgs-fmt` in the devshell if added);
  comment derivations like the existing ones (they carry the quirks —
  R2/R8/R9-style receipts).
- **Kernel delta hygiene:** the kernel builds from the published v6.6
  base + `devices/planet-geminipda/kernel/delta/` (source of truth;
  nix-build-only fixes live in `kernel/default.nix` so the delta stays
  byte-identical to the fork). To fold new fork commits in, point
  `bin/sync-kernel-delta.sh` at the new fork rev (it now defaults to
  `06fd13e11`) and update the rev in the derivation header. **Since
  2026-09-10k the delta deliberately DIVERGES from the fork** in four
  files (the delta-only `drivers/gpu/drm/tiny/{geminipda-drm.c,Kconfig,
  Makefile}` and the modified `mt6797-gemini-pda.dts` DRM node), so the
  script now ABORTS and lists them instead of clobbering — pass `FORCE=1`
  only if you have folded the local work into the fork. Do not hand-edit
  the delta and then blindly sync. The old `bin/snapshot-kernel.sh` +
  225 MB tarball are retired (git history has them).
- **Device backups:** `stock-dump/` is gitignored — boot/para backups
  live there (ledger + copy status in
  `docs/disaster-recovery/inventory.md`); nvram (IMEI) is private,
  never commit it.
- **Host has NO python3/adb on PATH** (rule 7) — `bin/*.py`-style tooling
  would need `nix develop --command python3 …`; `bin/run-mtk.sh` finds
  its own store python (sudo context) but needs the devshell's
  `mtkclient` closure to exist (build once: `nix develop --command true`).

## Before you start a session

1. Read `docs/session-log.md` (last entries) + the phase table in
   `docs/mobile-nixos-port-feasibility.md` §9 + README status.
2. If the session may touch device behaviour and the topic's receipts
   are not yet ported (M1/M7), glance at the legacy
   `GeminiPDA/docs/session-log.md` tail — it is the ground truth of what
   was tried until the history port lands.
3. Note the pinned versions (README "Pins") and doc "Last updated" dates.
4. Decide what needs updating as you go — don't wait until the end.

## Before you end a session

1. Append a dated entry to `docs/session-log.md` (what was tried, what
   flashed/changed, versions, hashes, next action).
2. Update every doc whose claims the session touched (statuses, dates) —
   including the phase table, README and the DR ledger.
3. If the session changed the kernel delta or its fork rev, re-run
   `bin/sync-kernel-delta.sh` and update the references.
4. If the session ran a flash/boot cycle, leave the version line +
   outcome in the session log and say whether the glass/TWRP state was
   left safe (prefer para = boot-recovery until images are verified).
5. New knowledge goes HERE (golden) — if you also touched a legacy doc,
   say so in the log so the port can pick it up.

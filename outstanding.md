# outstanding.md — open items before the first NixOS boot on glass

Produced: 2026-09-07 (rootfs-viability audit session). Statuses: 🔴 blocking
for the next milestone · 🟡 should fix before first glass boot · 🟢 nice /
follow-up. **Nothing here has been flashed** — the device still runs the
GeminiPDA Debian rootfs on borrowed kernel #329 (`6.6.0-00048-g188aade698dd`).

**WORKED 2026-09-07 (same-day session; see docs/session-log.md).** Items 1–7
and 9–10 (buildable parts) are fixed at the BUILD level in the working
tree — verified by rebuilding the toplevel and inspecting the closure
(`/nix/store/pscdi0fn9lan4rcnh2c4g9ksvhd3kh58-nixos-system-gemini-…`, see the
per-item ✅ notes below). Item 8 (GPU warmup) is documented in-tree
(`services/desktop.nix`) but its wiring decision needs the first gemwl boot
on glass — the #329 banding question is unresolved. Item 10's
serial-getty + wifi-DNS checks remain on-glass. **Still nothing flashed.**
See §7 for how each item was verified and how to re-check.

This is a **gap list**, not a plan rewrite: every item below exists on the
working Debian system (live device) or in the sibling GeminiPDA repo but is
missing from the built NixOS rootfs. The built closure that was audited is
`/nix/store/nwjfk7f16jjgbrm3pryf5bfppkhqp85p-nixos-system-nixos-26.11pre1031299.0bb7ec54c848`
(== the current working tree's `.#packages.x86_64-linux.toplevel`).

> **GOLDEN-REPO note [2026-09-07]:** gemini-nixos is now the GOLDEN
> repo (AGENTS.md M1–M7); GeminiPDA is legacy, being folded in. "Sibling
> repo" references below remain valid as the source until each item
> migrates; the authority rules in this file's §0 are superseded by
> AGENTS.md.

## 0. Agent working rules (read before touching anything)

- Hardware/boot truth authority = `/home/cjdell/Projects/GeminiPDA`
  (`docs/`, `AGENTS.md`). This repo records port decisions/deltas. Never
  contradict a sibling doc silently.
- **LCD panel rule**: any in-repo kernel build must stay the
  fbcon/EXCLUDE_DISPLAY build; never merge mediatek-drm*/mtk-mmsys/
  phy-mtk-*/tps65132 into an image (NT36672 TDDI is LK-initialised; a bad
  init can burn the matrix). This repo currently borrows kernel #329, so
  it only matters if you touch `devices/planet-geminipda/kernel/`.
- No bare `adb`/`python3` on the host PATH (CORE RULE 7) — `nix develop
  --command <tool>` from the repo root, or the `bin/` scripts
  (self re-exec inside the devshell). Long builds: `bash bin/run-job.sh
  start|wait` (CORE RULE 8), never inline nohup/pgrep loops.
- Every session logs a dated entry in `docs/session-log.md`; flash/boot
  attempts carry a version line (kernel, boot.img hash, mesa/wlroots
  pins).
- **Capture device-only files before flashing NixOS to p29**: several
  items below exist ONLY on the live Debian rootfs (not in the sibling
  repo). The NixOS rootfs flash wipes them. Verbatim copies are inline
  in this doc where that is the case — use those, but re-verify against
  the device first (`bash bin/device-ssh.sh 'cat …'`).

## 1. TL;DR

| # | Item | Sev (audit) | Where to change |
|---|---|---|---|
| 1 | No SSH login possible on the built rootfs (phase-2 blocker) | 🔴 ✅ built | `config/gemini.nix` |
| 2 | No logind side-key policy (silver key → suspend attempt) | 🔴 ✅ built | `config/gemini.nix` |
| 3 | No Gemini console keymap (Fn/UK layer) — `KEYMAP=us` today | 🔴 ✅ built | `config/gemini.nix` + vendored map |
| 4 | DRM/panfrost vs wlan_gen3 chrdev-major-226 race not made deterministic | 🔴 ✅ built | `services/gemini-pda.nix` |
| 5 | udev USB-host-PM rule not ported (dongle autosuspend kills connect detect) | 🟡 ✅ built | `services/gemini-pda.nix` |
| 6 | No boot-time backlight default (10 %) | 🟡 ✅ built | `services/gemini-pda.nix` |
| 7 | Host-NAT tooling for g_ether internet absent | 🟡 ✅ built + host-verified | new `bin/` script |
| 8 | No GPU-warmup before gemwl (desktop preview banding risk) | 🟢 ⏳ on-glass | `services/desktop.nix` + pkgs |
| 9 | `AGENTS.md` claims a `gemini-wdt-reboot` unit that doesn't exist | 🟢 ✅ units added | doc or unit |
| 10 | Minor: hostname, mt6351-keys pin, serial-getty verify, wifi DNS | 🟢 🔸 partial | `config/gemini.nix` |

## 2. 🔴 1 — No SSH login path on the built rootfs

**Why it matters.** SSH over g_ether (10.15.19.82) IS the bring-up
interface — the next milestone is literally "phase 2: SSH to a NixOS
shell on hardware" (README). The built closure cannot be logged into over
ssh at all.

**Evidence.** `bin/device-ssh.sh` connects as
`root@10.15.19.82` with key `~/.ssh/id_ed25519_gemini` and expects it in
`/root/.ssh/authorized_keys` (its own header comment). But the built
closure's `sshd_config` has `PermitRootLogin prohibit-password`, there is
no `users.users.root` authorized-key config, and the only non-root user
(`gemini`, `config/gemini.nix`) has no password and no keys. Root's
account is locked (NixOS default), so even the script header's "re-provision
with password `toor`" fallback (`PasswordAuthentication yes` is on, but a
locked root + `prohibit-password` blocks it). Serial works (autologin
`gemini`, passwordless wheel sudo), but ssh does not.

**Fix sketch** (in `config/gemini.nix`):
```nix
users.users.root.openssh.authorizedKeys.keys = [
  "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJ1AU4h4b3z6GFRHVRgXCJ5UMfJU5F8B7A38u7migkuh gemini-pda-root (device-ssh.sh)"
];
```
(pubkey == the host's `~/.ssh/id_ed25519_gemini.pub`, the one the Debian
rootfs currently has in `/root/.ssh/authorized_keys`). Consider also
`networking.hostName = "gemini";` while here (see §11).

**Verify.** Rebuild toplevel; confirm the key lands in
`/etc/ssh/authorized_keys.d/root` or `/root/.ssh/authorized_keys` in the
closure; then on glass: `bash bin/device-ssh.sh 'uname -r'` → the NixOS
kernel banner.

**RESOLVED (build level, 2026-09-07).** Key added + hostname set; closure
`/etc/ssh/authorized_keys.d/root` carries the pubkey and
`/etc/hostname` = `gemini` (toplevel now `nixos-system-gemini-…`). On-glass
check still owed: `bash bin/device-ssh.sh 'uname -r'` against the NixOS
rootfs after the phase-2 flash.

## 3. 🔴 2 — No logind side-key policy

**Why it matters.** The kernel driver `mt6351-keys` (`CONFIG_KEYBOARD_MTK_PMIC=m`,
module in the borrowed tree) maps: ESC/On button → `KEY_ESC`, silver
button → `KEY_SLEEP` (verified on the device 2026-09-06; sibling commit
`67a5939`). There is **no suspend/resume path on this unit** — a
`KEY_SLEEP` handled by logind (default `HandleSuspendKey=suspend`) would
attempt suspend on a device that cannot resume (only the LK-configured
WDT EXRST path self-boots; a software reset powers the unit off).

**Evidence.** Live Debian rootfs has
`/etc/systemd/logind.conf.d/99-gemini-sidekeys.conf` (device-only, not in
the sibling repo):
```ini
[Login]
HandleSuspendKey=ignore
HandleHibernateKey=ignore
HandlePowerKey=ignore
```
The built NixOS toplevel has `systemd-logind.service.d/overrides.conf` with
env only — no key handling.

**Fix sketch** (`config/gemini.nix`):
```nix
# CORRECTED 2026-09-07: `services.logind.extraConfig` is REMOVED in this
# nixpkgs pin (26.11) — the module now exposes `services.logind.settings.Login`.
services.logind.settings.Login = {
  HandleSuspendKey = "ignore";
  HandleHibernateKey = "ignore";
  HandlePowerKey = "ignore";
};
```
`mt6351-keys` autoloads via udev modalias (`platform:mt6351-keys`) — if
you want it deterministic, add `"mt6351-keys"` to `boot.kernelModules`
(see §5).

**Verify.** On glass press the silver button → nothing (no journal
suspend attempt). Confirm `/dev/input/eventN` named `mt6351-keys` exists.

**RESOLVED (build level, 2026-09-07).** `logind.conf` `[Login]` section
renders all three `Handle*Key=ignore`; `mt6351-keys` is in
`/etc/modules-load.d/nixos.conf` (see §5). On-glass silver-button press
still owed.

## 4. 🔴 3 — No Gemini console keymap (Fn/UK layer)

**Why it matters.** The built-in keyboard (input event "keyboard") needs
the Gemini layout: UK base + the Fn layer as AltGr/Shift combinations.
Without it the fbcon/serial console types the wrong layout and the Fn
layer is dead. The toplevel's `/etc/vconsole.conf` is `KEYMAP=us`
(NixOS default) — the device runs `gemini-keymap.service` (`busybox
loadkmap` of `/etc/gemini.bkmap`, built from `gemini-uk.map`).

**Evidence / source.** Sibling repo
`/home/cjdell/Projects/GeminiPDA/build/rootfs-files/keyboard/gemini-uk.map`
(438 KB, kbd text format, header `keymaps 0-127`; tracked). The binary
`/etc/gemini.bkmap` is device-only (busybox format) and NOT needed on
NixOS — systemd-vconsole-setup runs `loadkeys` (kbd), which reads the
text `.map` directly.

**Fix sketch.**
1. Vendor the map in this repo: copy
   `GeminiPDA/build/rootfs-files/keyboard/gemini-uk.map` →
   `config/keymaps/gemini-uk.map` (or a `pkgs/gemini-keymap` package) with
   a provenance note.
2. `config/gemini.nix`: `console.keyMap = "/absolute-or-store-path/gemini-uk.map";`
   (NixOS accepts a file path; `console.font = "TER16x32"` already set).
3. Sanity-check the map parses: `loadkeys --verbose <map>` (devshell).

Desktop companion (phase-4, not blocking): xkb layout for gemwl — sibling
`build/rootfs-files/xkb/symbols/gemini` + `build/deploy-xkb-gemini.sh`
(gemwl's xkbcommon wants layout name `gemini`).

**Verify.** On glass, fbcon terminal: type UK chars + a few Fn combos
(e.g. Fn+number symbols) and compare with the map's intent.

**RESOLVED (build level, 2026-09-07).** Map vendored verbatim at
`config/keymaps/gemini-uk.map` (+ provenance README);
`console.keyMap = ./keymaps/gemini-uk.map` (NixOS accepts the path —
`KEYMAP=<store-path>` in the closure's `/etc/vconsole.conf`);
`loadkeys --validate` passes on the map. On-glass typing check owed.

## 5. 🔴 4 — DRM/panfrost vs wlan_gen3 chrdev race not made deterministic

**Why it matters.** The vendor `wlan_gen3` driver registers chrdev
**major 226** — the same major as DRM. Whichever loads first wins; if
wlan wins, `drm.ko` init fails `EBUSY` and the GPU is dead for the boot
(full story: sibling handover-2026-09-07, kernel fix #330 pending upstream).
The live Debian system makes DRM load deterministic and EARLY; NixOS
currently leaves panfrost to udev autoload timing (racy against
`gemini-wifi-internal`'s `modprobe mtk_wcn`/`wlan_gen3`).

**Evidence.** Live device `/etc/modules-load.d/99-gpu.conf` (device-only):
```
# The vendor wlan_gen3 driver registers chrdev major 226 ("ampc0", BT-over-WiFi)
# which COLLIDES with DRM_MAJOR — whoever loads first wins; if wlan wins,
# drm.ko init fails with EBUSY and the GPU dies. udev modalias usually
# autoloads panfrost early anyway; this makes it deterministic.
drm
drm_shmem_helper
gpu-sched
panfrost
```
Our `services/gemini-pda.nix` sets `boot.kernelModules = [ "sramldo-smc" ]`
only. All four `.ko`s are in the borrowed module tree
(`kernel/borrowed/modules-6.6.0-…tar.xz`:
`kernel/drivers/gpu/drm/{drm,drm_shmem_helper}.ko`,
`kernel/drivers/gpu/drm/scheduler/gpu-sched.ko`,
`kernel/drivers/gpu/drm/panfrost/panfrost.ko`).

**Fix sketch** (`services/gemini-pda.nix`):
```nix
boot.kernelModules = [
  "sramldo-smc"
  "drm" "drm_shmem_helper" "gpu-sched" "panfrost"   # major-226 race (see header)
];
```
These load via `systemd-modules-load.service` before any unit;
`gemini-wifi-internal` already has `After=systemd-modules-load.service`
(+ `After=gemini-gpu-poweron.service`), so ordering matches Debian.

**Verify.** Boot; `ls /dev/dri` → `renderD128` present AND `wifi-internal
status` still yields wlan0; `dmesg | grep -i "register_chrdev\|226\|EBUSY"`.
(Note: gemwl opens `renderD129` first with a `renderD128` fallback —
`pkgs/gemwl/gemwl.c:1377`.)

**RESOLVED (build level, 2026-09-07).** `boot.kernelModules` now lists
`drm drm_shmem_helper gpu-sched panfrost` (plus `sramldo-smc` and
`mt6351-keys`); closure `/etc/modules-load.d/nixos.conf` carries all of
them in load order; systemd-modules-load runs before
`gemini-wifi-internal` (which already declares `After=`). All four `.ko`s
verified present in the borrowed module tree. On-glass `ls /dev/dri`
check owed.

## 6. 🟡 5 — udev USB-host-PM rule not ported

**Why it matters.** B-19 (sibling docs): runtime-PM autosuspend on the
MT6797 USB host controllers clears IPPC HOST_SEL and power-cycles the U2
PHY; there is no USB wakeup source wired, so autosuspend **permanently
kills connect detection** until reboot. The USB RTL8821CU dongle path
(and any USB host use) depends on the controllers staying "on".

**Evidence.** Live device `/etc/udev/rules.d/99-gemini-usb-host-pm.rules`
(**device-only — not in the sibling repo build/ tree**; capture it before
flashing):
```
# B-19 USB host mode: runtime-PM autosuspend clears IPPC HOST_SEL and
# power-cycles the U2 PHY; no USB wakeup source is wired on MT6797, so
# autosuspend permanently kills connect detection. Pin controllers + all
# USB devices to "on".
ACTION=="add", SUBSYSTEM=="platform", KERNEL=="11270000.usb", TEST=="power/control", ATTR{power/control}="on"
ACTION=="add", SUBSYSTEM=="platform", KERNEL=="11271000.usb", TEST=="power/control", ATTR{power/control}="on"
ACTION=="add", SUBSYSTEM=="usb", TEST=="power/control", ATTR{power/control}="on"
```

**Fix sketch** (`services/gemini-pda.nix`):
```nix
services.udev.extraRules = ''
  ACTION=="add", SUBSYSTEM=="platform", KERNEL=="11270000.usb", TEST=="power/control", ATTR{power/control}="on"
  ACTION=="add", SUBSYSTEM=="platform", KERNEL=="11271000.usb", TEST=="power/control", ATTR{power/control}="on"
  ACTION=="add", SUBSYSTEM=="usb", TEST=="power/control", ATTR{power/control}="on"
'';
```

**Verify.** On glass, plug the RTL8821CU dongle after boot; it must be
detected (`dmesg`, `wifi scan`). Re-plug after idle ≥ 2 s.

**RESOLVED (build level, 2026-09-07).** Rules ported to
`services.udev.extraRules`; closure
`/etc/udev/rules.d/99-local.rules` contains all three lines (also
captured verbatim from the live device before the rootfs wipe — see the
file in this doc). On-glass dongle-plug check owed.

## 7. 🟡 6 — No boot-time backlight default (10 %)

**Why it matters.** DISP_PWM0 backlight is the single biggest power draw;
at 100 % the battery cannot charge from a 500 mA USB port (BQ25896 power
path — ICHGR=0). The working system dims to 10 % at boot.

**Evidence.** Live device `backlight-default.service` (device-only):
```
# ExecStart=/usr/local/bin/backlight set 10    (After=systemd-modules-load.service)
```
On kernel #329 the sysfs path exists without a modules-load entry
(`CONFIG_PWM_MTK_DISP=y` in the borrowed `.config`), so no module step is
needed here (unlike Debian's `pwm-mtk-disp.conf`, which is vestigial for
#329). Our `backlight` CLI (sysfs-first, devmem fallback) is already in
the closure.

**Fix sketch** (`services/gemini-pda.nix`): oneshot unit
`gemini-backlight-default` — `After=systemd-udevd.service` (sysfs
present), `ExecStart=${utils}/bin/backlight set 10`, `RemainAfterExit=yes`,
`wantedBy = [ "multi-user.target" ]`; give it a `Path` of busybox/coreutils
like the sibling units.

**Verify.** Boot, `backlight get` → 10. Full-brightness-while-charging
check: `power dim-to-charge` still works.

**RESOLVED (build level, 2026-09-07).** Unit `gemini-backlight-default`
added to `services/gemini-pda.nix` (oneshot, `After=systemd-udevd`,
`ExecStart=${utils}/bin/backlight set 10`, `wantedBy=multi-user.target`);
present in the closure + `multi-user.target.wants`. Shebang rewrite (R10)
applies so the bash script execs. On-glass `backlight get` → 10 check
owed.

## 8. 🟡 7 — Host-NAT tooling for g_ether internet absent

**Why it matters.** The NixOS rootfs already declares the g_ether link's
internet assumptions: static `10.15.19.82/24`, `defaultGateway
10.15.19.1`, nameserver 1.1.1.1 (`config/gemini.nix`). Outbound internet
only exists if the HOST NATs `10.15.19.0/24` to its upstream. Link-local
SSH (the phase-2 milestone) does not need this — internet-bound work
(nix/apt on the device, DNS beyond the link) does.

**Evidence.** Sibling `build/usb-tether-nat.sh` (idempotent:
`sysctl ip_forward` + `nft table ip gemini-nat` masquerade out the host's
upstream iface) and device-side `build/rootfs-files/usb-tether/
dhcpcd-tether.conf` (the Debian static-tether recipe this repo mirrors in
Nix form). `bin/net-up.sh` in THIS repo brings the link up but has no NAT.

**Fix sketch.** Port the sibling script to `bin/usb-tether-nat.sh`
(host-side, usage header, devshell-safe). Note the host needs `nft`
(bare host PATH is minimal — check `which nft`; may need
`nix shell nixpkgs#nftables` or a host-system package).

**Verify.** From the device: `ping 1.1.1.1` with the link up + NAT
enabled.

**RESOLVED (2026-09-07).** Ported to `bin/usb-tether-nat.sh` (usage
header, tool checks, auto-detected upstream with explicit-iface
override). Host + device verified LIVE this session: the device's
`ping -c1 1.1.1.1` succeeds (rtt ~3 ms) with the NAT rule in place.

## 9. 🟢 8 — No GPU-warmup before gemwl (desktop preview banding)

**Why it matters.** Phase-4 preview auto-starts gemwl at boot
(`services/desktop.nix`, `wantedBy = multi-user.target`), making gemwl the
**first GL client** of the boot. The Mali-T880 tiler can come out of cold
boot in a "bad state": the first client whose tiler batches exceed 1024 px
in one axis drops the region beyond 1024 px (black band / half screen).
The working system absorbs it with a throwaway full-frame draw BEFORE the
compositor starts; in the 2026-09-02 matrix gemwl-first boots banded 4/4,
t_scene-first boots clean 2/2 (sibling `build/rootfs-files/gpu-warmup/
gpu-warmup-NOTES.md` + `gpu-warmup.service` — the latter device-only but
the NOTES are tracked). `tinytest-anim` (our startup client) draws a small
window and will likely NOT absorb it.

**Fix sketch (verify first).** This was observed on kernel #284 + Mesa
25.0.7 debug; whether #329 still exhibits it is an on-glass question. Two
options:
- add a warmup pass to the gemwl startup client (fullscreen >1024×1024
  FBO draw, i.e. the `t_scene 2 full` pattern ported into tinytest), or
- a `gemini-gpu-warmup` oneshot running a tiny surfaceless-EGL warmup
  binary `Before=gemwl.service` (needs a small aarch64 binary — gltests
  `t_scene` source is in the sibling repo under `build/gltests/`).

Keep `PAN_MESA_DEBUG=noafbc` on whatever does the warmup (AFBC readback
workaround must stay).

**Verify.** First boot with gemwl auto-start: full-frame desktop image
with no banded region on glass (judge by eyes / TWRP, NOT fbcap — fb
buffer ≠ panel state, LCD rule).

**DECISION 2026-09-07 (deferred to glass).** The banding question on
kernel #329 cannot be answered without a gemwl boot on glass, so no
warmup unit was wired. The risk is now documented in-tree
(`services/desktop.nix` header: "KNOWN RISK (gpu-warmup)") with both fix
options + the PAN_MESA_DEBUG=noafbc requirement, ready to implement at
the first gemwl boot if it bands.

## 10. 🟢 9 — `gemini-wdt-reboot`/`gemini-boot-recovery` units exist only as CLIs

**Why it matters.** `AGENTS.md` (this repo) says "Device-side (NixOS
rootfs): the `gemini-wdt-reboot` unit does the same" — implying a systemd
unit. Only the CLI scripts exist (`services/scripts/gemini-wdt-reboot`,
`gemini-boot-recovery`, packaged into `gemini-pda-utils`); no unit is
wired in `services/gemini-pda.nix` and none appears in the built
toplevel's `multi-user.target.wants`. A plain `systemctl reboot` on this
unit powers it OFF (only the WDT EXRST path self-boots).

**Fix sketch.** Either add two oneshot units (no `wantedBy`; started by
hand — `systemctl start gemini-wdt-reboot`) or amend the AGENTS.md wording
to "CLI". Prefer the units: they make the safe-reboot path explicit on the
device and match the README/AGENTS claims.

**Verify.** `bash bin/device-reboot.sh` (host) still works; device-side
unit, when started, WDT-EXRST self-boots into the current para target.

**RESOLVED (2026-09-07) — units added.** Both `gemini-wdt-reboot.service`
and `gemini-boot-recovery.service` now exist in `services/gemini-pda.nix`
as hand-started oneshots (no `wantedBy` — never at boot) and appear in
the closure. AGENTS.md/README claims are now accurate (wdt unit
self-boots; boot-recovery writes sticky para then powers off — the next
power-on lands in TWRP). On-glass hand-start check owed.

## 11. 🟢 10 — Minor / verify-on-glass

- **hostname** is `nixos` (`/etc/hostname` in the closure); the device is
  `gemini` on Debian. Cosmetic but cheap: `networking.hostName = "gemini";`
  (do with §2). → **DONE (build level)**: `/etc/hostname` = `gemini`.
- **serial getty**: the toplevel does not statically enable
  `serial-getty@ttyS0`, but systemd-getty-generator instantiates it at
  boot from the (kernel-forced) `console=ttyS0,921600n1` cmdline — verify
  on glass that a serial login appears (NixOS's own `kernel-params` list
  is inert while the borrowed kernel enforces `CONFIG_CMDLINE_FORCE`).
- **`mt6351-keys` determinism**: udev autoload is *expected* to work; pin
  it in `boot.kernelModules` while doing §5 if you want zero doubt.
  → **DONE (build level)**: in `/etc/modules-load.d/nixos.conf`.
- **Wi-Fi DNS / resolv.conf**: the `wifi` CLI runs `dhcpcd` per-interface
  (verbatim from Debian, where dhcpcd owns `/etc/resolv.conf`); on NixOS
  `/etc/resolv.conf` is system-managed (static 1.1.1.1 for the g_ether
  link). Verify an associated wlan0 gets working DNS once the wifi path is
  exercised; if not, run dhcpcd with `--nohook resolv.conf` or route the
  static nameserver correctly.
- **`/dev/dri/renderD128 vs D129`**: gemwl tries `renderD129` then falls
  back to `renderD128` (`pkgs/gemwl/gemwl.c:1377`); the device exposes
  only `renderD128`. Harmless, but the fallback order and comments
  (`services/desktop.nix`) should be reconciled with reality.
  → **DONE (docs)**: `services/desktop.nix` comments now say renderD128
  (with the D129-first fallback noted as harmless).

## 12. How this audit was produced (re-run it)

Three sources of truth were cross-checked:

1. **Built NixOS rootfs** — inspect the closure, not the source:
   `nix build .#packages.x86_64-linux.toplevel --no-link --print-out-paths`
   then read `$SYS/etc/systemd/system/{multi-user.target.wants,*.service.d}`,
   `$SYS/etc/ssh/sshd_config`, `$SYS/etc/vconsole.conf`, `$SYS/etc/fstab`,
   `$SYS/etc/hostname`, `$SYS/sw/bin` (system-path CLIs),
   `$SYS/kernel-modules` (module availability).
2. **Live device** (still the reference Debian 13 rootfs on kernel #329):
   `bash bin/device-ssh.sh '<cmd>'` — compare enabled units
   (`systemctl list-unit-files --state=enabled`), `/usr/local/bin`,
   `/etc/udev/rules.d/`, `/etc/systemd/logind.conf.d/`,
   `/etc/modules-load.d/`, `/etc/systemd/system/*.service`.
3. **Sibling repo** `/home/cjdell/Projects/GeminiPDA`: `build/rootfs-files/`
   is the canonical inventory of the working rootfs services (backlight,
   battery-guard, gpu-poweron, gpu-warmup, keyboard, pipewire, sidekeys,
   soak-monitor, speaker-amp, usb-tether, wifi, wifi-consys, xkb);
   `build/pack-gemini-6.6.sh` = the kernel/module packaging contract;
   `docs/session-log.md` + handovers = what was actually tried/verified.

Device-only files (no sibling-repo copy — inline above): the udev rule
(§6), logind drop-in (§3), `99-gpu.conf` module list (§5),
`backlight-default.service` (§7). Capture them from the device
(`bash bin/device-ssh.sh 'cat …'`) before the NixOS rootfs flash wipes p29.

## 13. Commit order the 2026-09-07 session followed (all landed)

1. `config/gemini.nix`: SSH root key (§2) + logind §3 + hostname §11 —
   one config commit, rebuild toplevel, verify closure contents.
2. `services/gemini-pda.nix`: boot.kernelModules (§5) + udev rule (§6) +
   backlight-default unit (§7) + wdt/boot-recovery units (§10).
3. Vendor the keymap + `console.keyMap` (§4).
4. `bin/usb-tether-nat.sh` (§8).
5. §9 decision deferred to glass (risk documented in `services/desktop.nix`);
   §10 resolved with units.
   Also landed: R10 shebang fix (`services/gemini-utils.nix`; new port
delta found while working the list — see feasibility doc §7 R10).
Remaining: the phase-2 flash cycle per README ("Flashing") +
`bin/flash-nixos.sh` safety model (para = boot-recovery sticky until the
image is verified), the on-glass checks noted per item above, and a dated
`docs/session-log.md` entry with image hashes.

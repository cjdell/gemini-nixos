# Handover — mediatek wifi + keyboard mappings on glass (2026-09-08)

**Intent:** get the two device features that have **never worked in
NixOS** working on glass:
1. **Internal Wi-Fi** (on-die MT6630 CONSYS, `wlan0`) — worked on the
   legacy GeminiPDA line (#325/#326: connected to a real AP, DHCP,
   ~8 MB/s) but never produced `wlan0` in NixOS.
2. **Keyboard mappings** (Gemini UK silkscreen + Fn layer) — the matrix
   device is up, but keys produce wrong symbols because the Gemini xkb
   layout is missing from the NixOS closure.

Both were re-diagnosed this session (2026-09-08 afternoon, while the
device ran gen9 on the self-built lean 6.6.0 kernel) and **both are
rootfs/config packaging gaps — NOT the deep CONSYS kernel issue the
2026-09-07 session log assumed**. Full receipts: `docs/session-log.md`
2026-09-08 (afternoon) entry. Precedent for the format:
`docs/handover-2026-09-08-kernel-on-glass.md` (that run PASSED — gen9
lean 6.6.0 is on glass, desktop verified; see its session-log entry).

---

## 1. Current device state (gen9, lean 6.6.0 — this session's baseline)

> **Updated 2026-09-08:** gen10 `y7v5z1sg…` deployed (keyboard xkb +
> shell fixes below); the baseline facts here (input inventory, kernel
> pairing, wifi root causes) are unchanged by that deploy.

- p32 `userdata`: NixOS **gen9** `r2mr8hf2l3k7l3029yhb8dgc27i7m84g-…`,
  booted on the self-built kernel **`6.6.0 #1-mobile-nixos`**; para
  cleared; Debian p29 untouched; desktop (gemwl + labwc → LXQt) up,
  NRestarts=0; battery-guard active; only failed units =
  `gemini-wifi-internal` + `gemini-wifi-nvram` (both root-caused below).
- Input inventory on glass (all kernel-side GOOD):
  `event0` = "Novatek NT36772 Touchscreen"; `event1` = USB mouse;
  `event2` = **"keyboard"** (gpio-matrix-keypad over AW9523B, full
  114-key bitmap, handlers `sysrq kbd`); `event3` = "mt6351-keys".
- `/run/booted-system/kernel-modules/lib/modules` = `6.6.0` (pairing
  with the booted kernel correct — §3 of the kernel-A/B handover).

## 2. Wifi — root causes (both ROOTFS-side, dated receipts on glass)

### 2a. `gemini-wifi-nvram.service` is malformed — never ran on ANY boot

The unit's ExecStart is a multi-line Nix string:

```nix
ExecStart =
  ''/bin/sh -c '
    set -e
    mkdir -p /data/nvram/APCFG/APRDEB
    cp -f ${firmware}/nvram/WIFI /data/nvram/APCFG/APRDEB/WIFI
    …
  ' '';
```

NixOS writes that into the unit file with **literal newlines and
indent**, and systemd parses each physical line as a new directive →
`Unbalanced quoting, ignoring: "/bin/sh -c '"` (:12) then
`Invalid section header '[ -e /etc/wifi/profiles.conf ] …'` (:17) →
**bad-setting, unit never starts**. Verified:
- `systemd-analyze verify` fails **identically on ALL stored gens**
  (gen2 `6jj0a4d…`, gen7 `cmia7hj8…`, gen9 `v0985k13…`) → broken since
  the original port (commit `c6afc5c`), not a new regression.
- Journal receipts from Sep 07 15:41 and every boot since.
- Consequence on glass: `/data/nvram/APCFG/APRDEB/WIFI` (factory MAC +
  TX cal) and `/etc/wifi/profiles.conf` were **never installed**
  (`/etc/wifi/` does not exist).

Fix direction: single-line the ExecStart (NixOS-idiomatic: put the
shell in a `pkgs.writeShellScript` / the utils package, or use
`ExecStart = lib.concatStringsSep " " […]`); then `deploy.sh deploy`
and verify the unit starts + the file lands in `/data/…` on next boot.

### 2b. wlan_gen3 probe fails on two missing files (dmesg, this boot)

```
[wlan]nvram_read: failed to open!!            ← 2a (nvram unit dead)
[wlan]glLoadNvram fail
[wlan]kalFirmwareOpen: Open FW image failed! Cur/Max ECO Ver[E1/E1]
[wlan]kalFirmwareOpen: Open FW image: /storage/sdcard0/WIFI_RAM_CODE_6797 failed, errno[-2]
[wlan]kalFirmwareOpen: Open FW image: /vendor/firmware/WIFI_RAM_CODE_6797 failed, errno[-2]
[wlan]kalFirmwareOpen: Open FW image: /lib/firmware/WIFI_RAM_CODE_6797 failed, errno[-2]
[wlan]wlanProbe: probe failed
[WMT-FUNC][E]wmt_func_wifi_on: wmt call wlan probe fail(-1)
```

wlan_gen3's `kalFirmwareOpen` uses a **hardcoded path list**, NOT
`request_firmware` — and none of the three paths exist on NixOS
(`/storage/sdcard0`, `/vendor/firmware`, `/lib/firmware` all absent).
The kernel firmware_class path (`/nix/store/h45q…/lib/firmware`, via
`/run/current-system/firmware`) served the WMT/ROMv3 leg fine —
**the CONSYS MCU link is healthy on this kernel**: "live client
re-synced to the patched full-mode MCU", func-on leg → 0. Only
wlan_gen3's own RAM-code open fails. Legacy Debian satisfied it by
installing the blobs into `/lib/firmware/`
(GeminiPDA `build/rootfs-files/wifi-consys/install-wifi-consys.sh`).

Fix direction: make `/lib/firmware` resolve to the firmware dir on
NixOS — e.g. `systemd.tmpfiles.rules = [ "L+ /lib/firmware - - - - /run/current-system/firmware" ]`
(or an activation script). The blobs (`WIFI_RAM_CODE_6797`,
`ROMv3_patch_1_{1,0}_hdr.bin`, `WMT_SOC.cfg`) are already in the
closure via `hardware.firmware` (`/run/current-system/firmware` shows
all of them). Then re-test `wifi-internal start`; expect wlan0.

### 2c. After 2a+2b: re-test and re-diagnose the "deep CONSYS" claim

The 2026-09-07 log ("wifi-internal REMAINS genuinely broken — deep
CONSYS issue: live client resync FAIL / STP-not-ready") was written
before the nvram-unit defect was known. The resync/STP path now shows
healthy. **Re-run `wifi-internal start` after the two fixes**; if wlan0
still fails, THEN escalate to the CONSYS deep-dive with this boot's
dmesg as the new baseline (docs/wifi-consys.md receipts in GeminiPDA
cover the full vendor init sequence that was made reliable there —
Build B2/B-33/B-34/B-35 gates G2a-G2c).

## 3. Keyboard mappings — root cause (xkb layout missing from closure)

The Gemini physical keyboard is a **47-key matrix** (UK silkscreen on
this unit) whose shifted/Fn layers differ from any stock PC layout.
Two mapping layers exist:

1. **VT console**: `console.keyMap = ./keymaps/gemini-uk.map`
   (vendored from GeminiPDA, `config/keymaps/`) — covers the tty
   console only. Not the desktop problem.
2. **gemwl/LXQt desktop**: gemwl compiles the keymap itself
   (`pkgs/gemwl/gemwl.c`):
   ```c
   struct xkb_rule_names rules = {
       .model = "pc105",
       .layout = getenv("GEMWL_XKB_LAYOUT") ? : "gemini",
       .variant = getenv("GEMWL_XKB_VARIANT") ? : NULL,
   };
   ```
   and xkbcommon fails **every boot**:
   ```
   xkbcommon: ERROR: [XKB-338] Couldn't find file "symbols/gemini" in include paths
   xkbcommon: ERROR: [XKB-338]   /nix/store/gaxrhs…-xkeyboard-config-2.47/etc/X11/xkb
   xkbcommon: ERROR: [XKB-338]   (could not be added: /root/.config/xkb, /root/.xkb, /etc/xkb)
   xkbcommon: ERROR: [XKB-769] Abandoning symbols file "(unnamed map)"
   ```
   → gemwl falls back to the default keymap → wrong symbols, no Fn
   (level3) layer, no `£`/`@`/`;` etc.

The Gemini xkb symbols live in the legacy project
(`GeminiPDA/build/rootfs-files/xkb/symbols/gemini`, adapted from
Gemian's xkeyboard-config fork; keysym facts UK-verified 2026-09-04 in
its header comment) and were deployed to the Debian rootfs by
`build/deploy-xkb-gemini.sh` → `/usr/share/X11/xkb/symbols/gemini`.
No NixOS equivalent existed.

**DONE (2026-09-08, gen10 `y7v5z1sg…` — deployed + verified on glass):**
`config/xkb/symbols/gemini` (vendored byte-identical + provenance README)
→ `pkgs/gemini-xkb.nix` (xkbcommon include dir, flake pkg `gemini-xkb`)
→ `XKB_CONFIG_EXTRA_PATH` on the gemwl + lxqt-nested units and
`XKB_DEFAULT_LAYOUT=gemini` on lxqt-nested (labwc 0.8.3 builds its
keymap from that env, src/input/keyboard.c `set_layout`). Session log:
no XKB-338 after the gemwl restart, labwc logs "Found layout **Gemini
English (UK)**". Shell ask also landed: `users.defaultUserShell` =
bashInteractive store bash + `SHELL=` exported to the lxqt session
(qterminal spawns bash, not /bin/sh). **User on-glass typing check
owed:** Fn+K `@`, Fn+L `;`, shift+3 `£`, shift+' `~`, shift+. `?` and
`echo $0` → bash.

Fix direction (chosen one; the compositor picks the layout up at next
start — no kernel change):
- Package `symbols/gemini` (with provenance) and put it on xkbcommon's
  include path for gemwl/labwc. NixOS-idiomatic: an
  `environment.etc`/`systemd.tmpfiles` symlink into the xkeyboard-config
  tree is not possible (read-only store) — instead set
  `XKB_CONFIG_EXTRA_PATH`/`XKB_CONFIG_ROOT` on the gemwl + lxqt-nested
  units to a store dir containing `symbols/gemini` (xkbcommon honours
  these), or overlay xkeyboard-config with the extra file via a package
  override in the closure.
- Verify with `xkbtest` (GeminiPDA `build/wayland/xkbtest.c` — compile
  on-device against xkbcommon) or a keysym dump, then a real typing
  test on glass (UK silkscreen chars + Fn combos).
- Kernel-side KEY_APOSTROPHE (matrix key (5,0)) is already correct in
  the DTS (`config/dts … mt6797-gemini-pda.dts` MATRIX_KEY(5,0,
  KEY_APOSTROPHE)) — no kernel work expected.

## 4. What is NOT needed / already ruled out

- **Kernel**: the lean 6.6.0 build carries the full bring-up delta
  (CONSYS spike, wlan_gen3/mtk_wcn, aw9523b + GPIOLIB_IRQCHIP select,
  matrix keypad polling patch). Input + CONSYS link verified on glass
  this session. Do not touch the kernel for either workstream first.
- **Rule 5 (LCD)**: untouched by both fixes — no display changes.
- **Flash**: both fixes are rootfs-only → `bin/deploy.sh deploy`
  round-trip, no TWRP/boot.img cycle needed (unless a kernel debug
  iteration becomes necessary later).

## 5. Run plan (next session)

1. ~~Fix the nvram unit (§2a)~~ ⬜ still open (not touched this session) — single-line ExecStart (script in the
   utils package or writeShellScript). `deploy.sh deploy`; on the
   device: `systemd-analyze verify` clean, unit active, and
   `ls /data/nvram/APCFG/APRDEB/WIFI /etc/wifi/profiles.conf` exist.
2. ~~Fix `/lib/firmware` (§2b)~~ ⬜ still open (not touched this session) — tmpfiles symlink to the firmware dir.
   `deploy.sh deploy`; then `bash bin/device-ssh.sh
   'wifi-internal start'` — expect wlan0 (30 s wait). If it appears:
   `wifi auto` against a saved profile or wpa_supplicant manual
   connect; log the result (see §2c if it does not).
3. ~~Ship the xkb layout (§3)~~ ✅ **DONE 2026-09-08 (gen10)** — see
   §3 DONE note; remaining = user on-glass typing check.
4. ~~Default shell bash~~ ✅ **DONE 2026-09-08 (gen10)** —
   `users.defaultUserShell = "${pkgs.bashInteractive}/bin/bash"`
   (`config/gemini.nix`) + `SHELL=` on the lxqt-nested unit
   (`services/lxqt.nix`); `/etc/passwd` + unit env verified on glass.
5. Record version lines + outcomes in `docs/session-log.md`
   (per rule 0) and update this doc's state. → done (entry 2026-09-08).

## 6. References

- Session receipts: `docs/session-log.md` 2026-09-08 (afternoon) —
  kernel A/B PASS + this root-cause spot.
- Kernel A/B plan (the run that produced the gen9 baseline):
  `docs/handover-2026-09-08-kernel-on-glass.md`.
- Legacy wifi knowledge (the working implementation):
  GeminiPDA `docs/wifi-consys.md` (gates G2a-G2c, B-33..B-35),
  `build/rootfs-files/wifi-consys/install-wifi-consys.sh`.
- Legacy keyboard knowledge: GeminiPDA
  `build/rootfs-files/xkb/symbols/gemini` (keysym facts in header),
  `build/wayland/xkbtest.c`, `build/deploy-xkb-gemini.sh`.
- NixOS sides: `services/wifi.nix` (nvram unit to fix, §2a),
  `pkgs/gemwl/gemwl.c` (keymap code, §3), `config/gemini.nix`
  (`console.keyMap`), `config/keymaps/gemini-uk.map` (VT map).

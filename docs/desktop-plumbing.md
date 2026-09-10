# Desktop plumbing for the Gemini PDA — UPower, NetworkManager, backlight, touch

Last updated: 2026-09-10s. Status: 🟡 implemented; **gen62 deployed
2026-09-10 — NM/upower/backlight verified on glass** (see §Verification
checklist); the battery icon is the one item left: it needs the new
boot.img (kernel battery supply), not just the rootfs generation.
**2026-09-10q:** internal-speaker L/R swap fixed by a virtual sink +
`gemini-speakerd` couples the speaker amp to the selected output device
(§Speakers — **amp coupling verified on glass 2026-09-10q**: headphones
⇒ `dout=0`, speakers ⇒ `dout=1`; the L/R correction itself still needs an
ear test). Touch is now a **real multi-touch
wl_touch device** (2026-09-10, §Touch — protocol chain verified
source-level + on the journals; on-glass finger test pending).
**2026-09-10p:** GNOME audio wired (session → the one system PipeWire
session: Settings→Sound device list + gsd volume keys) and the Fn
volume/brightness keys bound in mutter — see §Volume.
**2026-09-10s:** the on-screen keyboard is permanently suppressed
(mutter reports `touch_mode=true` on this touchscreen+no-pointer
hardware, so gnome-shell auto-created the OSK regardless of the a11y
toggle) — see §"On-screen keyboard".
"make every desktop environment just work" layer: the system services
that Phosh (default desktop), LXQt (alternative), GNOME or KDE would
all consume through the standard D-Bus APIs, with zero gemini-specific
glue in the shells.

Why this exists: Phosh was on glass (gen61, 2026-09-09/10) but had no
plumbing under it — the DE's top bar / quick settings query
**org.freedesktop.UPower** (battery), **org.freedesktop.NetworkManager**
(wifi) and **org.bluez** (bluetooth), and none of those had the
gemini-specific data to show. Battery was the worst gap: the kernel
only exposed a `USB`-type charger supply, which upower/DEs ignore for
battery display.

## What was wired (all DE-agnostic, all on the system bus)

| Piece | Standard API / service | Gemini-specific bits |
|---|---|---|
| **Battery / AC status** | UPower (`services.upower`, system bus) | kernel delta registers a `Battery`-type supply `bq25890-battery-N` in the BQ25896 driver (voltage-derived capacity — see below) |
| **Wi-Fi** | NetworkManager (`networking.networkmanager`) | CONSYS bring-up units unchanged (wifi.nix); NM manages wlan0/wlan1; home networks as NM profiles; usb0 unmanaged |
| **Bluetooth** | bluez `org.bluez` (persistent service + auto-power) | already done 2026-09-09 — `services/bluetooth.nix`, `docs/bluetooth-bringup.md`; blueman UI present |
| **Backlight** | `/sys/class/backlight/*/brightness` (sysfs) + `brightnessctl` | udev chmod 0666 rule (`services/plumbing.nix`) — see §brightness |
| **Volume** | PipeWire/WirePlumber session (`services/audio.nix`) | one system session at `XDG_RUNTIME_DIR=/run/gemwl-audio`; GNOME clients redirected via `PULSE_SERVER`/`PIPEWIRE_RUNTIME_DIR`, Fn keys bound by xkb keycode — see §Volume |
| **Speakers vs headphones** | PipeWire sink list (GNOME Settings → Sound / Quick Settings output picker) | a virtual L/R-correcting "Built-in Speakers" sink + `gemini-speakerd` drives the speaker-amp pads to match the default sink — see §Speakers |
| **Touch** | `wl_touch` (Wayland core protocol) — a real 10-point multi-touch device | gemwl forwards the NT36772's fingers to its seat as wl_touch (no cursor); the nested compositor's wlroots wayland backend re-emits it to the shell — see §Touch |

### Battery: no fuel gauge ⇒ voltage-derived capacity in the kernel

The Gemini has **no fuel-gauge IC** (bring-up verified: the BQ25896 is
charger-only, no coulomb counter on any i2c bus; Android's MTK
"pseudo-FG" did exactly what we do — estimate from VBAT). Consequences:

- Upower only reports a battery when a `Battery`-type power_supply
  exists → the kernel delta (`devices/planet-geminipda/kernel/delta/
  drivers/power/supply/bq25890_charger.c`, fork commit c8f0787d,
  2026-09-10) registers `bq25890-battery-N` beside the charger supply
  on the **same chip/regmap/lock**.
- Reported properties: `capacity` (VBAT ADC µV → piecewise-linear 1S
  Li-ion OCV table, 4.20 V = 100 % … 3.45 V = 0 %; clamped to ≤90 %
  while pre/fast-charging — a cell under charge sits near regulation
  voltage — and 100 % only at the chip's charge-termination status);
  `status` (from PG/CHG_STAT via the shared live
  `bq25890_update_state()` path — the driver keeps continuous ADC
  conversion on while online, so reads are fresh); `voltage_now`
  (VBAT ADC, raw µV), `temp` (TS %, same table the charger reports),
  `health` (fault bits → GOOD/OVERVOLTAGE/OVERHEAT/…).
- `power_supply_changed()` notifications now fan out to BOTH supplies
  (`bq25890_supplies_changed()` — otherwise a bare charger-only notify
  never woke the battery client).
- **Safety stance (important):** the % is a *UI estimate* — load-
  dependent by nature. It is deliberately not wired to any power
  action: `services/plumbing.nix` sets
  `services.upower.criticalPowerAction = "Ignore"` (with the module's
  allowRisky… flag) because the only trusted poweroff on this device is
  `gemini-battery-guard` (services/gemini-pda.nix; orderly poweroff at
  3.50 V on the same VBAT ADC). Upower thresholds are tuned to the
  curve: low 15 % (~3.64 V — guard warns at 3.65 V), critical 5 %
  (~3.57 V), action 2 % (~3.50 V) — the guard may beat upower anyway;
  that's the design.
- upower thresholds are percent-based (`usePercentageForPolicy`) —
  time-based policy needs energy data the supply does not have.

On-glass check: `cat /sys/class/power_supply/bq25890-battery-0/type`
→ `Battery`; `upower -d` shows a battery device; phosh top bar shows %
+ charging bolt; unplug AC → status flips within a poll tick.

### Wi-Fi: NetworkManager takes over from the standalone stack

wifi.nix now defaults `services.geminiWifi.useNetworkManager = true`:

- NM manages wlan0 (CONSYS — created by the unchanged
  `gemini-wifi-internal` bring-up unit, which NM is ordered after and
  Wants=) and wlan1 (RTL8821CU dongle) via its wpa_supplicant backend.
- Home networks declared with `ensureProfiles` ("The Lab" + "The Lab
  2.4GHz", same psk as `etc/wifi/profiles.conf` — keep the two in
  sync) → NM autoconnects at boot; nmcli/DE UI edits persist in
  `/etc/NetworkManager/system-connections` (the ensure-profiles unit
  re-seeds the two home profiles every boot — rename via the UI if you
  want that to stick).
- `usb0` (g_ether) is `unmanaged` — the static host link config in
  config/gemini.nix owns it; NM's auto-default would otherwise claim it.
- DNS stays on the NixOS default resolvconf rc-manager: NM's DHCP
  nameservers merge in front of the static 1.1.1.1 base. (Deliberately
  no systemd-resolved — one less daemon on this lean stack.)
- `wifi.scanRandMacAddress = false`: the gen3 CONSYS driver has no
  mac-randomization handling; keep probes deterministic like the old
  stack.
- ModemManager is explicitly off (no modem).
- **Polkit**: the NM module's rule lets group `networkmanager` do
  anything on NM; cjdell is in it (config/gemini.nix). Critical here —
  the desktop has no logind "active local user", which is what the
  default NM polkit prompt expects.
- Legacy: `services.geminiWifi.useNetworkManager = false` restores the
  pre-2026-09-10 behaviour (standalone wpa_supplicant + dhcpcd via the
  `wifi` CLI + `gemini-wifi-auto` unit; the units/CLI remain installed
  either way for diagnostics).

On-glass check: `nmcli dev status` shows wlan0; `nmcli con up "The
Lab"`; phosh quick settings lists networks; unplug USB-C host link →
wifi stays up.

### Backlight access (session-less desktop problem)

Why not the standard path: phosh's brightness control and
gnome-settings-daemon's use logind `Session.SetBrightness`; both fail
when the session is a systemd **system service** (our desktop model —
no logind session, no uaccess tag). The kernel cannot express wider
modes on sysfs attrs at build time (`VERIFY_OCTAL_PERMISSIONS` refuses
write bits for group/other — an earlier kernel-delta approach failed
the build on exactly that, 2026-09-10), so the standard runtime fix is
a udev rule (`services/plumbing.nix`) that chmods
`/sys/class/backlight/%k/{brightness,bl_power}` to 0666 on add. Sysfs
honours the inode mode at open(2); no kernel change needed. Phosh's
own brightness manager has a sysfs backend and writes the file
directly; `brightnessctl` (installed) is the CLI. Root's `backlight`
CLI (gemini-pda-utils) remains the devmem fallback / console path.

Caveat: a backlight class device only appears once the disp-pwm
backlight node actually registers (DTS has `backlight_lcd`, led_mode=5,
PWM at 0x1100f000). **On glass it does register**: gen62 shows
`/sys/class/backlight/backlight/` (type `raw`, max_brightness 255) and
the LCD boost is on that PWM; the desktop sliders/`brightnessctl` now
drive it. The Fn+B/N keys are also wired to gnome-shell's
`screen-brightness-*` bindings (see §Volume); GNOME 50 moved screen
backlight handling into mutter/gnome-shell, so under GNOME it is a
CSD/`Meta.Backlight` path, not phosh's sysfs backend. The udev RUN rule
fires on device *add* — after a live deploy
of the rule the device must be re-added, either a reboot or
`udevadm trigger --action=add /sys/devices/platform/backlight/backlight/backlight`
(the class-glob form `--subsystem-match=backlight` does NOT match;
receipt 2026-09-10). Verified: an unprivileged `su cjdell -c
'brightnessctl -c backlight set 9%'` writes 23/255. Root's `backlight`
CLI (gemini-pda-utils, devmem PWM) remains the console fallback.

### Volume + Fn media keys (GNOME; 2026-09-10p)

Volume is PipeWire + WirePlumber (`services/audio.nix`, S16 sink
config) — **one system-wide session** whose `XDG_RUNTIME_DIR` is
`/run/gemwl-audio` (the gemwl/phosh/LXQt-era design: the desktop was a
system service with no logind session). GNOME is now a real logind
session under GDM, so its clients looked in
`/run/user/1000/pulse/native` and found nothing — **GNOME Settings →
Sound listed no devices** even though the system session was healthy.
Fix (`services/gnome.nix`, `environment.sessionVariables`): point the
GNOME session at the existing system session instead of starting a
second PipeWire (which would re-solve the MT6351 S16 path twice):

- `PULSE_SERVER=unix:/run/gemwl-audio/pulse/native` — libpulse clients:
  gnome-control-center's Sound panel, gsd-media-keys' Gvc (the volume
  keys / OSD), gnome-shell.
- `PIPEWIRE_RUNTIME_DIR=/run/gemwl-audio` — native PipeWire clients
  (`wpctl`, `pavucontrol`, …).

Verified 2026-09-10p: the pulse socket is `srwxrwxrwx`;
gnome-shell/gsd-media-keys/blueman appear in `pw-cli ls Client`; the
gsd volume keys drive the sink.

**Fn volume/brightness keys.** The Fn layer is XKB level 3, selected by
`ISO_Level3_Shift` on RALT (Mod5): Fn+C=XF86AudioLowerVolume,
Fn+V=XF86AudioRaiseVolume, Fn+T=XF86AudioMute,
Fn+B=XF86MonBrightnessDown, Fn+N=XF86MonBrightnessUp
(`config/xkb/symbols/gemini`). mutter matches global keybindings on
(keycode, modifier-mask); it does **not** mask Mod5, but it resolves a
*keysym* accelerator to the **lowest** xkb level that produces that
keysym (`add_keysym_keycodes_from_layout()` stops at the first level
with a match) — and the compiled keymap carries those XF86 keysyms at
level 0 on the standard evdev consumer keycodes (`<VOL->`=0x7a,
`<MUTE>`=0x79, `<I232/233>`). So `<Mod5>XF86AudioLowerVolume` resolves
to (0x7a, Mod5) and can never match the Fn event (keycode 0x36 = C,
Mod5). Verified on glass: the keysym form did nothing.

Fix: bind the Fn layer's **xkb keycodes** directly as schema DEFAULTS
(still user-remappable) via
`services.desktopManager.gnome.extraGSettingsOverrides`, with
`…extraGSettingsOverridePackages = [ pkgs.gnome-settings-daemon ]` so
the media-keys schema is in the override set:

    [org.gnome.shell.keybindings]
    screen-brightness-up=['XF86MonBrightnessUp', '<Mod5>0x39']      # N
    screen-brightness-down=['XF86MonBrightnessDown', '<Mod5>0x38']  # B
    [org.gnome.settings-daemon.plugins.media-keys]
    volume-up-static=[…, '<Mod5>0x37']    # V
    volume-down-static=[…, '<Mod5>0x36']  # C
    volume-mute-static=['XF86AudioMute', '<Mod5>0x1c']  # T

xkb keycode = evdev code + 8; the built-in matrix is fixed, so the
keycodes are the layout contract. Verified on glass 2026-09-10p: sink
0.39→0.33 (Fn+C), →0.44 (Fn+V), `[MUTED]` toggle (Fn+T), backlight
25↔37 (Fn+B/N). Test tool: `bin/kb-inject.c` (writes raw key events to
the keyboard evdev node; `kb-inject fn+c fn+v fn+b fn+n`).

Under the old phoc+phosh default this was unsolved (phosh has no
media-key code; gsd cannot global-grab keys on Wayland without
GNOME Shell). With GNOME as the default desktop the standard
shell+gsd path exists; the keycode bindings are the only
gemini-specific part.

### Speakers: L/R swap + amp/headphone toggle (2026-09-10)

Two reported problems, one mechanism each:

1. **The built-in left/right speakers are swapped.** The flanking
   speakers are wired LEFT↔RIGHT (the right-hand speaker plays the left
   channel); the 3.5 mm jack is wired correctly. Both ride the same
   codec HPL/HPR drivers — the speakers through external amps enabled by
   SoC pads 243/244 — so the swap is in the speaker PCB, not a codec
   register (`devices/planet-geminipda/kernel/delta/sound/soc/codecs/
   mt6351.c`: `HPL Select`/`HPR Select` are 1:1 with DACL/DACR).
2. **There was no way to switch the internal speaker amp off** (for
   headphone-only listening the jack plays the speakers too — they are
   electrically in parallel; there is no jack detection on this mainline
   stack yet).

Fix: a **virtual sink** that crosses the channel pair, and a **watcher
that couples the amp to the default sink**:

- `services/pipewire/60-gemini-speakers.conf` — a
  `libpipewire-module-filter-chain` node named **`gemini_speakers`**
  ("Built-in Speakers"): two `copy` nodes with the inputs/outputs arrays
  swapped (`inputs=[toL:In toR:In]`, `outputs=[toR:Out toL:Out]`), so
  FL→right and FR→left. Its playback stream is pinned to the hardware
  sink with `target.object` and flagged `node.passive` +
  `node.dont-fallback` (a loopback whose output reached the default sink
  would feed back into itself; `node.link-group` prevents a self-link).
  The hardware sink otherwise keeps its ALSA/ACP name
  (`alsa_output.platform-sound.stereo-fallback`);
  `services/pipewire/50-gemini-alsa-s16.conf` only renames its
  *description* to "Headphones / Jack".
- `gemini-speakerd.service` (`gemcli speaker watch`) polls the PipeWire
  default sink (`wpctl inspect @DEFAULT_SINK@`, via
  `PIPEWIRE_RUNTIME_DIR=/run/gemwl-audio`) and drives the amp pads:
  default sink == `gemini_speakers` ⇒ amps **ON**; anything else ⇒
  **OFF** (jack only). Reading the pad state is side-effect-free
  (pinctrl DOUT via /dev/mem — the gpio chardev v1 API can only read by
  requesting the pad as *input*, which would release the amp drive).

So the GNOME way to choose output is just the normal one: pick
**Built-in Speakers** or **Headphones / Jack** in Settings → Sound (or
the Quick Settings output picker); the amp follows within ~1 s. The
choice persists (WirePlumber's configured default sink), and
`gemini-audio-defaults` re-syncs it at boot.

CLI / console equivalent: `audio-output speaker|headphone|toggle|status`
now also sets the PipeWire default sink (and `sync-default`, the
retryable boot-time half), so the CLI and GNOME agree. `speaker
on|off|status` drives the pads directly.

**Files:** `services/pipewire/60-gemini-speakers.conf`,
`services/pipewire/50-gemini-alsa-s16.conf`,
`services/scripts/audio-output`, `services/audio.nix`
(`gemini-speakerd`), `pkgs/gemcli/src/speaker.rs`.

**On-glass check (pending):** play a left/right test tone to the
Built-in Speakers sink and confirm it is no longer reversed; select
Headphones and confirm the speakers go silent while the jack still
plays; `gemcli speaker status` shows `dout=1` for speakers and `dout=0`
for headphones; unplug/replug and reboot to confirm persistence.

### Touch: a real multi-touch device (no cursor) [2026-09-10]

**Problem.** The NT36772 TDDI kernel driver
(`devices/planet-geminipda/kernel/delta/drivers/input/touchscreen/
novatek-nt36xxx.c`) was already a correct 10-point Protocol-B
multi-touch device — the problem was the COMPOSITOR: gemwl consumed
`wl_touch` and **emulated the first finger as an absolute pointer**
(warp the cursor + synthetic `BTN_LEFT` press/release). Every app
therefore saw a mouse: a visible on-screen cursor that followed the
finger, no second finger, no gestures — so GNOME apps (WebKit pinch
zoom, GTK4 multi-touch) could never benefit from the hardware.

**The chain (verified at source level, 2026-09-10).** The nested
architecture is gemwl (wlroots 0.18.2, owns the LK fb) → phoc 0.54
(wlroots 0.19.3, `WLR_BACKENDS=wayland`) → phosh apps. wlroots' wayland
backend (both 0.19.3 `backend/wayland/seat.c` for phoc and 0.18.2 for
labwc) synthesizes a `wlr_touch` input device ("wayland-touch-seat0")
from the OUTER compositor's `wl_seat` when the seat advertises
`WL_SEAT_CAPABILITY_TOUCH`, and forwards `wl_touch.down/motion/up/frame`
into it (normalized 0..1 per-output coords). phoc's own touch stack
(`src/seat.c seat_add_touch` → `src/cursor.c phoc_cursor_handle_touch_*`
→ `wlr_seat_touch_notify_*`) then delivers a real `wl_touch` to the
phosh apps — plus phoc's own compositor-side gesture recognizers
(`gesture-zoom.c` pinch, `gesture-swipe.c`, `gesture-drag.c`) and
compositor-drawn touch-point feedback (`touch-point.c`).

**The fix (`pkgs/gemwl/gemwl.c`):**

- `server_new_input` now adds `WL_SEAT_CAPABILITY_TOUCH` to the seat
  when a touch device is attached (so phoc/labwc create their
  synthesized `wlr_touch`).
- The touch handlers forward **every** finger:
  `touch_handle_down` hit-tests the scene (normalized → output box →
  `wlr_scene_node_at` → surface-local; the nested toplevel is
  full-screen at scale 1, so the wl_touch "relative to the down
  surface" contract holds) and calls
  `wlr_seat_touch_notify_down + _frame`; motion/up are the same
  (`notify_motion`/`notify_up` + frame). No pointer events are
  synthesized anymore — the cursor is untouched by touch.
- A `client_has_touch()` guard skips a down until the nested client
  has actually called `wl_seat.get_touch()` (wlroots would otherwise
  log an error per down; phoc requests it as soon as it sees the
  capability, so this only bites pre-session).
- **Fallback:** `GEMWL_TOUCH_POINTER_EMU=1` on the gemwl unit restores
  the legacy first-finger-as-pointer behaviour (A/B on glass; in emu
  mode the TOUCH capability is NOT advertised, so clients see exactly
  the pre-change seat).

**Consequences.** Phosh: taps/one-finger drags are native touch now
(same feel, no cursor); multi-finger works — pinch/zoom in GTK4/WebKit
apps, and phoc's own desktop zoom/swipe gestures activate. LXQt
(labwc, wlroots 0.18.2 wayland backend has the identical touch path):
Qt Widgets apps receive `wl_touch` and Qt's own touch→mouse
compatibility synthesizes clicks inside the app (no compositor
anymore); Qt Quick content gets real multi-touch. A USB mouse, when
plugged in, still gets the pointer as before.

### On-screen keyboard: never show it [2026-09-10s]

**Problem.** GNOME popped the on-screen keyboard up on every text entry
even though the device has a real keyboard. The accessibility toggle was
already off (`org.gnome.desktop.a11y.applications screen-keyboard-enabled
= false` in cjdell's dconf), so that was not the cause.

**Root cause (source-level, verified).** gnome-shell creates the OSK when
*either* path is true (`js/ui/keyboard.js`, `KeyboardManager._syncEnabled()`):

```
enabled = a11y(screen-keyboard-enabled)
          || (seat.get_touch_mode() && lastDeviceIsTouchscreen())
```

and mutter computes touch-mode as `!has_pointer` when the seat has a
touchscreen but no tablet-mode switch (`src/backends/native/
meta-seat-impl.c`, `update_touch_mode()`):

```
if (!has_touchscreen)            touch_mode = FALSE;
else if (has_tablet_switch ...)  touch_mode = <switch state>;
else                             touch_mode = !has_pointer;
```

The Gemini has a touchscreen (`Novatek NT36772 Touchscreen`), a real
keyboard, and **no pointer**, so mutter reports `touch_mode=true` and the
second path fires. A keypress does not clear it either: the shell's
`last-device-changed` handler ignores `KEYBOARD_DEVICE`, so the touchscreen
stays the last non-keyboard device — the classic "touchscreen laptop with
no trackpad looks like a tablet" case.

**Fix — two halves, both in `services/gnome.nix`:**

1. `pkgs/gnome-extension-no-osk/` — a small GNOME 45+ ESM Shell extension
   that forces `KeyboardManager._lastDeviceIsTouchscreen()` to false,
   killing exactly the auto path (the accessibility OSK still works if it
   is ever turned on). It uses a private `KeyboardManager` method, so
   re-check `js/ui/keyboard.js` on a gnome-shell upgrade.
2. The system dconf DB enables the extension
   (`org.gnome.shell enabled-extensions`) and **locks** it, and locks
   `screen-keyboard-enabled=false` — the a11y path off for good. NixOS has
   no first-class option for enabling an extension, and a GSettings
   override only moves the *default*, so the system-db + lock is what
   makes this actually stick.

**On-device proof (2026-09-10, GNOME Shell 50.4).** A temporary diagnostic
extension set the shell's last-device state to a fake touchscreen and
called the real `_syncEnabled()`, reporting for the same live shell:

| no-osk extension | `seat.touch_mode` | OSK object after `_syncEnabled()` |
|---|---|---|
| **off** | `true` | **CREATED** |
| **on** | `true` | **not-created** |

`touch_mode` really is true on this hardware, and the extension really
does suppress the OSK object the shell would otherwise create (the OSK is
only opened on text focus, so no object ⇒ no keyboard, ever).

**Deploy note.** The extension is a *system* extension under
`$out/share/gnome-shell/extensions/no-osk@gemini-nixos`, found via the
system `XDG_DATA_DIRS`. dconf enabling takes effect on the next session
start (gnome-shell only scans extension dirs at startup); no reboot/rootfs
flash is involved. During bring-up it was also installed under
`~/.local/share/gnome-shell/extensions/` for the live A/B — that copy
shadows the system one and should be removed after the first
`nixos-rebuild switch`/deploy that carries this change.

## Where it lives

- `services/plumbing.nix` — UPower + thresholds + udev backlight rule +
  brightnessctl (module option `services.geminiPlumbing.enable`,
  default true). Imported from config/gemini.nix.
- `services/wifi.nix` — NM integration (option
  `services.geminiWifi.useNetworkManager`).
- kernel delta `drivers/power/supply/bq25890_charger.c` (fork commit
  c8f0787d, synced + byte-verified 2026-09-10) — the Battery supply.
- `pkgs/gemwl/gemwl.c` — the touch → wl_touch forwarding (see §Touch);
  the NT36772 kernel driver itself was already correct (Protocol B,
  10 points, output-space ABS).
- `pkgs/gnome-extension-no-osk/` — the never-show-the-OSK GNOME Shell
  extension (§"On-screen keyboard"); `services/gnome.nix` installs it and
  enables + locks it (and `screen-keyboard-enabled=false`) in the system
  dconf DB.
- `services/audio.nix` — the one PipeWire system session, the S16
  WirePlumber rule, the L/R-correcting virtual sink (`60-gemini-
  speakers.conf`) and `gemini-speakerd` (default-sink → amp pads).
- `services/scripts/audio-output` — the CLI half of the output mode
  (amp pads + the matching PipeWire default sink).
- `config/gemini.nix` — imports + the existing NM polkit/group wiring.

Phosh itself needed no changes: its wifi page speaks NM, its BT page
BlueZ, its battery icon UPower, brightness its sysfs backend. LXQt
(re-enabled later) needs its panel's pulseaudio + battery plugins
(standard LXQt) and a wifi applet (nm-tray/nm-applet) — nothing
gemini-specific.

### Sleep integration (silver button)

`gemcli sleep`/`gemini-sleepd` is NM-aware as of the same session
(`pkgs/gemcli/src/sleep.rs`):

- `phosh-nested.service` added to the stop/start list — it was missing,
  so a sleep left phoc + the phosh session running against a stopped
  gemwl (the observed "phosh not usable" after the silver button).
- Wifi: NM mode parks the link only and lets NM autoconnect on wake;
  the legacy kill-wpa_supplicant + restart-`gemini-wifi-auto` path is
  used only when NM is not active (detected via
  `systemctl is-active NetworkManager`). docs/power-sleep.md §4/§5.

## Verification checklist

On-glass results (2026-09-10; gen62/gen63 + the new boot.img):

- ✅ `nmcli dev` → wlan0 managed and **connected to "The Lab"**
  without any manual step (also after every reboot since); usb0
  `unmanaged`; both home profiles seeded.
- ✅ `upower -d` → battery device present after the kernel flash:
  `battery_bq25890_battery_0`, model "gemini-battery (voltage-derived)",
  `state=charging percentage=90%`,
  `icon-name=battery-full-charging-symbolic`; DisplayDevice mirrors it
  (→ the phosh top-bar icon). `line_power` for the AC side.
- ✅ Backlight: `/sys/class/backlight/backlight/{brightness,bl_power}`
  are `rw-rw-rw-` after a normal boot (the udev rule fires on device
  add); unprivileged `brightnessctl -c backlight set 9%` → 23/255.
- ✅ gemwl + phosh-nested + bluetooth active; NM reconnects wifi on
  every boot.
- ✅ WDT EXRST reboot: `bin/device-reboot.sh` → gadget drops ~12 s →
  device back with a new boot_id (~47 s round trip). `systemctl
  reboot` is broken on this unit (open P1, docs/phase-2-on-glass.md
  §4) — use the WDT path.
- ⬜ Physical eyeball items: the phosh brightness slider moves the LCD
  (rule 5 — judge on glass), and the silver-button sleep/wake round
  trip returns both desktop and wifi.
- ✅ GNOME audio (2026-09-10p): `PULSE_SERVER=unix:/run/gemwl-audio/
  pulse/native` + `PIPEWIRE_RUNTIME_DIR=/run/gemwl-audio` in the
  session → Settings→Sound sees the device, gsd volume keys drive the
  sink (`pw-cli ls Client` shows GNOME Shell Volume Control / GNOME
  Volume Control Media Keys).
- ✅ Fn media keys (2026-09-10p): Fn+C/V/T volume down/up/mute
  (0.39→0.33→0.44, `[MUTED]`) and Fn+B/N brightness (25↔37) via the
  `<Mod5>` keycode bindings; `bin/kb-inject.c fn+c …` is the
  reproducible test.
- Touch (2026-09-10): ✅ gemwl logs `touchscreen attached (wl_touch
  forwarding)`; ✅ phoc logs `Adding touch device: wayland-touch-seat0`
  (the synthesized device exists end-to-end) — see the session log
  2026-09-10c for the journal receipts. ⬜ on glass with fingers: no
  visible cursor on touch, tap/one-finger-drag work in phosh, and a
  two-finger pinch zooms in a GTK4/WebKit app (e.g. a web page).

### Follow-up fixed in the same session

- The WDT EXRST arm no-ops unless `MODE` is restored first — the A72
  bring-up disarms it (docs/phase-2-on-glass.md §2b). Fixed in
  `bin/device-reboot.sh`, `bin/flash-nixos.sh` and `gemcli`'s
  `wdt.rs arm()`.
- `bin/device-reboot.sh` used to report success before the reset had
  fired (it pinged the USB gadget that is still up during the 2 s WDT
  window). It now waits for the gadget to drop and requires a changed
  boot_id.

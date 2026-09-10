# Desktop plumbing for the Gemini PDA — UPower, NetworkManager, backlight access

Last updated: 2026-09-10. Status: 🟡 implemented + eval-green (kernel
build in progress); ON-GLASS VERIFICATION OWED (see §end). This is the
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
| **Volume** | PipeWire/WirePlumber session (`services/audio.nix`) | already done; control via `wpctl` / any DE's volume widget (audio.nix runs its own PW session with the S16 ALSA sink config) |

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
PWM at 0x1100f000). If `/sys/class/backlight` is empty on glass, the
LCD boost is still driven by LK only and the desktop sliders no-op —
verify and, if needed, finish the pwm/backlight driver wiring (the
`backlight` CLI still works either way — it drives the same PWM by
devmem).

### Volume

Volume control is already PipeWire + WirePlumber (`services/audio.nix`,
S16 sink config); any DE's mixer talks to it through pulse-compat /
wpctl. Physical volume keys (Fn combos on the Gemini keyboard) are NOT
wired by any standard path under phoc+phosh: phosh has no media-key
code and gsd's media-keys plugin cannot global-grab keys on Wayland
without gnome-shell (same story as PinePhone — postmarketOS users ended
up with actkbd-style daemons). Follow-up if needed: a tiny
`wevdaemon`/actkbd-style key daemon bound to the Fn-volume combos →
`wpctl set-volume`. Not part of this plumbing layer.

## Where it lives

- `services/plumbing.nix` — UPower + thresholds + udev backlight rule +
  brightnessctl (module option `services.geminiPlumbing.enable`,
  default true). Imported from config/gemini.nix.
- `services/wifi.nix` — NM integration (option
  `services.geminiWifi.useNetworkManager`).
- kernel delta `drivers/power/supply/bq25890_charger.c` (fork commit
  c8f0787d, synced + byte-verified 2026-09-10) — the Battery supply.
- `config/gemini.nix` — imports + the existing NM polkit/group wiring.

Phosh itself needed no changes: its wifi page speaks NM, its BT page
BlueZ, its battery icon UPower, brightness its sysfs backend. LXQt
(re-enabled later) needs its panel's pulseaudio + battery plugins
(standard LXQt) and a wifi applet (nm-tray/nm-applet) — nothing
gemini-specific.

## Verification checklist (owed, 🟡 → ✅)

1. Rebuild + deploy; `systemctl status upower NetworkManager` on glass.
2. `ls /sys/class/power_supply/` shows `bq25890-battery-0`; `upower -d`
   shows it with sane % tracking AC plug/unplug; phosh top bar shows
   battery + charging bolt; unplug → %/bolt update live.
3. `nmcli dev` shows wlan0 managed; phosh quick-settings wifi page lists
   + connects to "The Lab"; g_ether link (usb0) unaffected; resolv.conf
   gains DHCP nameservers when on wifi.
4. `ls -l /sys/class/backlight/*/brightness` = `-rw-rw-rw-`; phosh
   brightness slider moves the LCD (eyes!), `brightnessctl s 50%`.
   (If no backlight dir: see §backlight caveat.)
5. Bluetooth quick toggle unaffected (bluetoothd already up).
6. Record the version line in docs/session-log.md (rule 0): kernel rev
   c8f0787d, gens flashed.

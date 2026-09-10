# Desktop plumbing for the Gemini PDA — UPower, NetworkManager, backlight, touch

Last updated: 2026-09-11. Status: 🟡 implemented; **gen62 deployed
2026-09-10 — NM/upower/backlight verified on glass** (see §Verification
checklist); the battery icon is the one item left: it needs the new
boot.img (kernel battery supply), not just the rootfs generation.
**2026-09-11:** Fn transport keys (Q/W/E = play-pause/prev/next) and
the UK regional settings (Europe/London + en_GB.UTF-8, §Regional) added
and **deployed on glass (gen 15)** — timezone/locale verified, and the
Fn transport keys verified against a live Chromium MPRIS player. On
first deploy they appeared dead: the cause was the **session-variable
re-login gotcha** (see §Volume).
**2026-09-11 (DOSBox-X):** installed DOSBox-X with a Gemini keyboard fix
(UK table + Fn+1..0 ⇒ F1..F10 via a seeded mapper, since its SDL2 input
is scancode-based and cannot see XKB level 3) — §DOSBox-X; deployed to
the device as **gen 18** and headless-verified (UK table + mapper seed);
the interactive Fn-key test is owed.
**2026-09-10q:** internal-speaker L/R swap fixed by a virtual sink +
`gemini-speakerd` couples the speaker amp to the selected output device
(§Speakers — **amp coupling verified on glass 2026-09-10q**: headphones
⇒ `dout=0`, speakers ⇒ `dout=1`; the L/R correction itself still needs an
ear test). **2026-09-11: the coupling was actually DEAD** — the watcher's
`wpctl` parse never matched (the default node carries a `* ` marker), so
selecting Headphones left the amp ON; fixed + verified on glass (gen 17),
plus mode persistence (§Speakers → Correction). Touch is now a **real multi-touch
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
| **Regional settings** | systemd `localed`/`timedated` (`time.timeZone`, `i18n.*`) | Europe/London + British English (`en_GB.UTF-8`, every `LC_*`); GNOME `org.gnome.system.locale` pinned to match — see §Regional |
| **Speakers vs headphones** | PipeWire sink list (GNOME Settings → Sound / Quick Settings output picker) | a virtual L/R-correcting "Built-in Speakers" sink + `gemini-speakerd` drives the speaker-amp pads to match the default sink — see §Speakers |
| **Touch** | `wl_touch` (Wayland core protocol) — a real 10-point multi-touch device | gemwl forwards the NT36772's fingers to its seat as wl_touch (no cursor); the nested compositor's wlroots wayland backend re-emits it to the shell — see §Touch |

### Regional settings: Europe/London + British English [2026-09-11]

The unit is the **UK silkscreen** Gemini (shift+3 = £; see
`config/xkb/symbols/gemini` and the console `config/keymaps/
gemini-uk.map`). The OS side
now matches it, in `config/gemini.nix`:

- `time.timeZone = "Europe/London"` — NixOS writes `/etc/localtime`
  (GMT/BST, automatic DST). systemd-timedated serves the same data, so
  GNOME **Settings → Date & Time** shows London and cannot drift; no
  dconf key is involved (timezone is not a GSettings value).
- `i18n.defaultLocale = "en_GB.UTF-8"` — sets `LANG` in
  `/etc/locale.conf` for every session and service (British English,
  `£`, `dd/mm/yyyy`).
- `i18n.extraLocaleSettings` pins **every** `LC_*` category
  (ADDRESS, IDENTIFICATION, MEASUREMENT, MONETARY, NAME, NUMERIC,
  PAPER, TELEPHONE, TIME) to `en_GB.UTF-8`, so a stray inherited
  `LC_ALL`/category variable cannot fall back to `en_US`/`C`.
- `services/gnome.nix` also sets (and locks) the GNOME-side locale
  formats key. GNOME's `org.gnome.system.locale` declares
  `path="/system/locale/"` in its gschema, so the dconf path is
  **`system/locale`** — *not* the schema id (`org/gnome/system/locale`),
  which is where a naive write lands and nothing reads. Verified on
  glass 2026-09-11: with the wrong path `gsettings get
  org.gnome.system.locale region` stayed `''` while `dconf dump /`
  showed the orphan node; after the fix it returns `'en_GB.UTF-8'`
  (and `gsettings writable … region` is `false`, i.e. locked). Locked
  for the same reason as `input-sources`: a stale per-user dconf value
  outranks a system default.

The keymap side was already correct; this closes the timezone/locale
gap. **Deployed + verified on glass 2026-09-11 (gen 15):**
`timedatectl` → `Time zone: Europe/London (BST, +0100)`; `/etc/locale.conf`
and `locale` → every category `en_GB.UTF-8`; `gsettings get
org.gnome.system.locale region` → `'en_GB.UTF-8'`.

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
Fn+B=XF86MonBrightnessDown, Fn+N=XF86MonBrightnessUp, and the media
**transport** keys Fn+Q=XF86AudioPlay (play/pause toggle),
Fn+W=XF86AudioPrev (previous track), Fn+E=XF86AudioNext (next track)
(`config/xkb/symbols/gemini` — the Fn layer symbols were already there;
2026-09-11 added the GNOME bindings). mutter matches global keybindings on
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
    play-static=[…, '<Mod5>0x18']         # Q  (play/pause toggle)
    previous-static=[…, '<Mod5>0x19']     # W
    next-static=[…, '<Mod5>0x1a']         # E

xkb keycode = evdev code + 8; the built-in matrix is fixed, so the
keycodes are the layout contract. Q/W/E are evdev 16/17/18 → xkb
0x18/0x19/0x1a. `play-static` (not `pause-static`) is the play/pause
toggle, so Fn+Q toggles. Verified on glass 2026-09-10p: sink
0.39→0.33 (Fn+C), →0.44 (Fn+V), `[MUTED]` toggle (Fn+T), backlight
25↔37 (Fn+B/N). Test tool: `bin/kb-inject.c` (writes raw key events to
the keyboard evdev node; `kb-inject fn+c fn+v fn+b fn+n`).

**Transport keys verified on glass 2026-09-11.** Injected `fn+q`
(Chromium \[MPRIS] PlaybackStatus `Playing`→`Paused`→`Playing`) and
`fn+e`/`fn+w` (track changed). The first deploy *appeared* to do
nothing — the bindings were right, the **session environment was
stale**:

> `extraGSettingsOverrides` reach gsd via `NIX_GSETTINGS_OVERRIDES_DIR`,
an `environment.sessionVariables` value baked into the GNOME session at
**login**. `nixos-rebuild switch` / `bin/deploy.sh` rewrite
`/etc/set-environment` but do **not** restart the running user session,
so the live `systemd --user` (and every `gsd-*`, `gnome-shell`) keeps
the OLD overrides store path. Symptom: `gsettings get …play-static`
from a fresh `su -` login shell shows the new array, while the running
daemon still reads the old one — and restarting `gsd-media-keys` does
not help because it inherits the stale env from `systemd --user`.

Fix for the live session (no reboot):

    NEW=$(grep -o '/nix/store/[^"]*-gnome-gsettings-overrides' /etc/set-environment)/share/gsettings-schemas/nixos-gsettings-overrides/glib-2.0/schemas
    su - cjdell -c "XDG_RUNTIME_DIR=/run/user/1000 \
      systemctl --user set-environment NIX_GSETTINGS_OVERRIDES_DIR=$NEW; \
      systemctl --user restart org.gnome.SettingsDaemon.MediaKeys.target"

(`org.gnome.SettingsDaemon.MediaKeys.service` is `RefuseManualStart/
Stop=yes`, so restart the **.target**.) This is packaged as
**`bin/gnome-session-env-refresh.sh`** (`--all` restarts every
`org.gnome.SettingsDaemon.*` target; default user `cjdell`). A
logout/reboot applies `/etc/set-environment` permanently. The same
applies to ANY `environment.sessionVariables` change and to the other
gsd `-static` bindings (volume etc.). Overrides read by gnome-shell
itself (the `org.gnome.shell.keybindings` screen-brightness bindings)
are cached in the running shell and still need a re-login.

Under the old phoc+phosh default this was unsolved (phosh has no
media-key code; gsd cannot global-grab keys on Wayland without
GNOME Shell). With GNOME as the default desktop the standard
shell+gsd path exists; the keycode bindings are the only
gemini-specific part.

### DOSBox-X: scancode apps never see the Fn layer [2026-09-11]

Symptom: in DOSBox-X the gemini-specific keys come out wrong and every
Fn combination (Fn+1..0 = F1..F10, Fn+C/V/T volume, …) does nothing.

Cause — two halves of the same design fact, already documented above
for mutter: the host's whole keyboard contract lives in the `gemini`
XKB layout's **symbol levels**, and DOSBox-X does not consume an XKB
layout at all:

1. **DOSBox-X is scancode/position based.** Its SDL2 mapper keys off
   `SDL_KeyboardEvent.keysym.scancode` (physical position), not
   `keysym.sym` (the level-resolved symbol):
   `src/gui/sdl_mapper.cpp` `CKeyBindGroup::CreateEventBind/CheckEvent`
   use `event->key.keysym.scancode`, and `MakeDefaultBind()` maps the
   physical `SDL_SCANCODE_*` straight to the emulated PC key. So the
   `gemini` layout is irrelevant; DOSBox-X translates positions through
   **its own** `[dos] keyboardlayout` table (default **US**) and picks
   the character itself. That is why the UK silkscreen (`£` on Shift+3,
   `@` under Fn+K, `;` under Fn+L, the `'`/`.` keys) is wrong.
2. **Fn is XKB level 3 only.** Fn is kernel `KEY_RIGHTALT`, used as
   `ISO_Level3_Shift` (`config/xkb/symbols/gemini:11‑12,28`), so
   Fn+1..0 is *the same scancode as plain `1`* with RALT held — there is
   no Fn scancode and no F1/XF86 scancode to translate. This is the
   exact failure mutter had with `<Mod5>XF86…` keysyms
   (`services/gnome.nix:278‑311`): a keysym that only exists at level 3
   cannot be matched from a (keycode, modifier) event. The SDL2 binary
   ignores `usescancodes` (`useScanCode()` returns false), so that knob
   is not a way out either.

Fix (app-local, no host-XKB change) — `pkgs/dosbox-x-gemini.nix` wraps
the upstream package and replaces `bin/dosbox-x` with
`pkgs/dosbox-x-gemini.sh`; the upstream `share/` (`.desktop`, metainfo)
is symlink-joined through. The wrapper does two things:

- forces DOSBox-X's own **UK** table (`-set "dos keyboardlayout=uk"`,
  applied after the user config so it wins; overridable with a later
  user `-set`, or `DOSBOX_X_GEMINI_NO_UK=1`) so the base/Shift layer
  matches the UK silkscreen;
- seeds a **complete** mapper file (`config/dosbox-x/
  mapper-dosbox-x.map`) into the user config dir on first run. DOSBox-X
  loads any mapper file it finds **instead of** its built-in defaults
  (`MAPPER_Init()` → `if (!MAPPER_LoadBinds()) CreateDefaultBinds();`),
  so the file must reproduce the whole default binding set, not just the
  additions. It adds a second binding to each F-key event:
  `key_f1 "key 58" "key 30 mod2"` — i.e. **mod2 (RALT = Fn) + the
  number scancode → F1**, so Fn+1..0 produce real DOS F1..F10 while an
  external keyboard's F-keys still work.

Regenerate the mapper when the flake's dosbox-x pin moves:
`bin/gen-dosbox-x-mapper.sh <src>/src/gui/sdl_mapper.cpp \
  <SDL2-dev>/include/SDL2/SDL_scancode.h config/dosbox-x/mapper-dosbox-x.map`
(it derives the `DefaultKeys[]` SDL2 table + scancode values).

Receipts / gotchas (2026-09-11):
- The mapper **section name is `[SDL2]`**, not `[sdl]` — `SDL_STRING` is
  `"SDL2"` for SDL2 builds (`include/shell.h:27`). A wrong section makes
  `MAPPER_LoadBinds()` silently discard every line and fall back to
  defaults (no warning at default log level). The first generated file
  had this bug; caught by running the mapper through the x86_64 binary
  under `SDL_VIDEODRIVER=dummy`.
- Verified build-level: the x86_64 dosbox-x 2026.08.02 loads the file
  (strace shows `~/.config/dosbox-x/mapper-dosbox-x.map` opened twice)
  and logs `DOS keyboard layout loaded with main language code UK for
  layout uk`. The wrapper seeds the file 0644 (the store copy is 0444,
  so `chmod` matters — the mapper GUI must be able to save).
  `DOSBOX_X_GEMINI_MAPPER_RESET=1` re-seeds (backing up the old file);
  an existing mapper without the Fn binds prints a one-line warning.
- Fn also *is* Alt (RALT is bound to mapper `mod2` and to the guest's
  right-Alt in the default map), so Fn+1 reaches the guest as Alt+F1.
  The Gemini has no separate Alt, so this is the deliberate trade — DOS
  games keep an Alt key; drop the `key_ralt`/`mod_2` binds if a game
  objects.
- The XF86 media layer (Fn+C/V/T/B/N/Q/W/E) is **not** covered: DOS has
  no media-key scancodes. F-keys are the DOS-relevant part; media keys
  would have to be bound to DOSBox-X's own mixer/handler events.
- Still unwrappable by DOSBox-X: symbols the Gemini puts only on level 3
  (Fn+K=`@`, Fn+L=`;`) cannot be expressed because DOSBox-X never sees
  the level.

Status: 🟡 deployed to glass as **gen 18** (2026-09-11) — the wrapper
+ `.desktop` are live and a headless run as the desktop user logs the UK
layout and seeds the mapper. The mapper *load* path was verified with the
x86_64 binary (strace + UK log).
⬜ interactive on-glass test owed: launch from the app grid, confirm Fn+1
gives F1 in a DOS program and Shift+3 gives `£` (needs a human at the
keyboard or an evdev-injection harness).

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
choice persists in WirePlumber state **and** in
`/etc/gemini/audio-output-mode`, which `gemini-audio-defaults` re-syncs
at boot.

CLI / console equivalent: `audio-output speaker|headphone|toggle|status`
now also sets the PipeWire default sink (and `sync-default`, the
retryable boot-time half), so the CLI and GNOME agree. `speaker
on|off|status` drives the pads directly.

**Files:** `services/pipewire/60-gemini-speakers.conf`,
`services/pipewire/50-gemini-alsa-s16.conf`,
`services/scripts/audio-output`, `services/audio.nix`
(`gemini-speakerd`), `pkgs/gemcli/src/speaker.rs`.

#### Correction + fix (2026-09-11, on glass): headphones-only was dead

**Symptom (user report).** Selecting **Headphones / Jack** in GNOME left
the internal speakers playing.

**Root cause.** `speaker::default_sink()` parsed only lines starting
with `node.name = `, but `wpctl inspect @DEFAULT_SINK@` prefixes the
default node's properties with a `* ` marker:

    * node.name = "alsa_output.platform-sound.stereo-fallback"

so the parse always returned `None`. `sync_amp()` then bailed before
touching the pads — the watcher never drove the amp at all, and because
`gemini-audio-defaults` had already forced the default sink to
`gemini_speakers` at boot, the amps stayed ON. (The 2026-09-10q "amp
coupling verified on glass" receipt exercised `audio-output
headphone`, i.e. the CLI's *direct* `speaker off`, never the watcher.)
Fixed in `parse_sink_name()` (strip `* `/space from the trimmed line
before matching) + 4 regression unit tests in `speaker.rs`.

**Secondary defects found while fixing it:**

1. `/etc/gemini` did not exist on the fresh (post-repartition) rootfs,
   so `audio-output`'s `echo > /etc/gemini/audio-output-mode` silently
   failed and **no** stored mode ever persisted. New tmpfiles rule
   `d /etc/gemini 0755 root root -` in `services/audio.nix`; the script
   also `mkdir -p`s it now.
2. A choice made **only in GNOME** was never written back to the mode
   file, so the next boot reverted to the stored (or default "speaker")
   mode. `gemini-speakerd` now persists the observed default sink to
   that file (`mode_for_sink`), and the unit is ordered
   `after gemini-audio-defaults.service` so the boot-time sink write is
   settled before it reads (no transient hardware-sink default can be
   persisted by accident).

**Files changed:** `pkgs/gemcli/src/speaker.rs`,
`services/scripts/audio-output`, `services/audio.nix`.

**On glass 2026-09-11 (device system-16 → 17; final toplevel
`hra1d2ajgkblw9ql7hf8m9nkiyp3akhg-nixos-system-gemini-26.11pre-git`;
rootfs only, no boot.img/kernel):** `journalctl -u gemini-speakerd` shows
`default sink alsa_output.platform-sound.stereo-fallback -> amps OFF`
and `default sink gemini_speakers -> amps ON`; `speaker status` reads
`dout=0` on both pads for Headphones and `dout=1` for Built-in
Speakers, following `wpctl set-default`. Selecting Headphones in GNOME
now silences the internal speakers. `audio-output status` reports
`mode: headphone (stored intent: headphone)` and `/etc/gemini` exists;
toggling the default sink rewrites `/etc/gemini/audio-output-mode`
(`headphone` ↔ `speaker`) within ~1 s, and the unit's `After=` includes
`gemini-audio-defaults.service`. gemcli test suite: 16 passed.

**Still owed:** L/R correction by ear (test tone to Built-in Speakers —
the 2026-09-10q virtual sink is unchanged); a real reboot to confirm the
new mode persistence; jack-plugged playback under "Headphones / Jack".

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
`~/.local/share/gnome-shell/extensions/` for the live A/B. That copy was
removed after the first deploy that carried this change (gen8,
2026-09-10s); the extension is now served from the system profile
(`/run/current-system/sw/share/gnome-shell/extensions/no-osk@gemini-nixos`)
and dconf reports both keys locked (`gsettings writable` = false).

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
- `pkgs/dosbox-x-gemini.{nix,sh}` — the DOSBox-X wrapper (§DOSBox-X): UK
  table + mapper seeding; the mapper is
  `config/dosbox-x/mapper-dosbox-x.map`, regenerated by
  `bin/gen-dosbox-x-mapper.sh`.
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
- ✅ Fn transport keys (2026-09-11, gen 15): Fn+Q play/pause, Fn+W
  previous, Fn+E next. **Verified on glass** with `bin/kb-inject.c`
  against a live Chromium MPRIS player (fn+q toggled Playing↔Paused;
  fn+e/fn+w changed track) after the session-env fix in §Volume.
- ✅ Regional settings (2026-09-11, gen 15): `timedatectl` →
  Europe/London (BST), `/etc/locale.conf` + `locale` all `en_GB.UTF-8`,
  GNOME `org.gnome.system.locale region` → `'en_GB.UTF-8'` (locked).
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

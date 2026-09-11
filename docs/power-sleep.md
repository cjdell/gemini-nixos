# Power sleep on the Gemini PDA — investigation + the silver-button sleep/wake

**Last updated:** 2026-09-12 (desktop-aware light sleep: GDM/GNOME, gemshell; removed-session references dropped)

Scope: how to make the unit draw as little battery current as possible,
and the silver side button (KEY_SLEEP, mt6351-keys) as device sleep/wake.
The clamshell keyboard must not generate input while closed (the lid
presses the keys), and the same press that sleeps must not be seen by
the desktop.

## TL;DR (status)

- **`systemctl suspend` is disabled outright (2026-09-11).** It is a
  different mechanism from `gemcli sleep`: it asks logind/systemd to
  enter kernel **s2idle**, which this bring-up unit cannot resume — on
  glass it **locks the system up**. `config/gemini.nix` now suppresses
  the upstream `{sleep,suspend,hibernate,hybrid-sleep,
  suspend-then-hibernate}.target` + `systemd-*sleep*.service` units, so
  every initiate path (a shell command, logind's `Suspend()` D-Bus
  method) fails fast with "Unit … not found" instead of hanging, and
  GNOME's power plugin is dconf-locked to `sleep-inactive-*-type =
  'nothing'` / `power-button-action = 'nothing'` so it never calls
  logind in the first place. Sleep/wake is **only** the silver button +
  `gemcli sleep` + `gemini-sleepd`.
- **There is no suspend/resume path on this unit yet** (no wake source
  for s2idle — the PMIC side keys are *polled* over pwrap, not
  IRQ-driven, and the kernel boots `clk_ignore_unused
  pd_ignore_unused regulator_ignore_unused`). This was the roadmap's
  "suspend/resume … until PMIC work" out-of-scope line; it still holds.
  Deep sleep is a kernel-change follow-up (§Deep sleep).
- **Implemented on glass (2026-09-08): a reversible LIGHT sleep** driven
  by the silver button — `gemcli sleep on|off|status|key`
  (`pkgs/gemshell/crates/gemdata-device/src/sleep.rs`) + the `gemini-sleepd.service` daemon
  (services/gemini-pda.nix) that owns the button. It powers down every
  controllable load while the kernel stays up; the same button wakes it.
  No kernel change, no flash — a normal package/system deploy. Since
  2026-09-10 it also powers the **A72 cluster** down when it was up (the
  GNOME performance power mode can leave it online) and restores it on
  wake — `docs/power-modes.md`.
- **Responsiveness (v2, same day — the button must feel instant):**
  `sleep on` turns the backlight off FIRST (the visible acknowledgement
  that the press registered) and the whole transition is ~1-2 s.
  Nothing blocks on the CONSYS chip teardown: the WMT `echo off` stalls
  ~29 s with the chip associated (observed on glass), so sleep takes
  the wifi interface down + kills wpa_supplicant/dhcpcd instead (chip
  powered but idle) and wake re-associates via `wifi auto`. The
  `sleep key` daemon debounces presses (1 s — one physical press = one
  toggle even if the polled driver double-reports) and drains events
  queued while a toggle ran, so mashing the button can no longer
  cascade into rapid sleep/wake/sleep backlight flicker (the v1 bug:
  a ~30 s sleep stall made the user press repeatedly; each queued
  press toggled after the stall at ~1 s cadence, and the rapid
  stop/start cycling tripped gemwl's start rate limit — wake now
  `reset-failed`s units before starting them).
- Measured effect (USB 500 mA input, ICHGR charge-current proxy —
  50 mA ADC steps): backlight is the dominant controllable load (100 %
  → off ≈ ≥150 mA@5 V, and at 100 % the battery discharges even at the
  full 500 mA input). Desktop / wifi / audio / extra A53 cores each
  measure ≈ ≤50 mA — at or below the ADC resolution. The awake floor
  with everything off ≈ 400 mA@4 V ≈ 1.6 W (LCD panel logic + TDDI stay
  on — the fbcon kernel cannot blank the panel; rule 5).

## What the light sleep does (`gemcli sleep on`)

Order matters — the VISIBLE step first, everything else fast (~1-2 s
worst case total, nothing blocks on the wifi chip):

1. **Backlight off** (`bl_power=4`/PWM EN=0). FIRST — this is the
   instant acknowledgement that the press registered. The brightness
   value is retained, so wake restores exactly.
2. **Clamshell input drivers unbound** — the closed-lid fix: the
   gpio-matrix-keypad platform device (`keyboard`) and the
   novatek-nt36xxx touch i2c client (`4-0062`) are unbound, so the keys
   the lid presses generate no events and no wakeups. `mt6351-keys`
   (silver) is deliberately NOT unbound — it is the wake button.
3. **A53 cpus 1..7 offlined** (cpu0 must run the kernel), then the
   **A72 cluster (cpu8/9) powered down** IF it was up — the secure
   `cl2-down` teardown (WDT-guarded, DA9214 rail drop). The default
   cold-boot state leaves the cluster down, so this is usually a no-op;
   it matters when the GNOME **performance** power mode (or a manual
   `gemcli a72 up`) brought it online. The recorded state restores it on
   wake. **[A72 handling added 2026-09-10 — docs/power-modes.md]**
   Per-core A53 offline is the safe PSCI path (the cl2-down receipts);
   the A53 *cluster* power-down (below) is not wired.
4. **Heavyweight services stopped** (only those actually running, the
   list is recorded): the current panel owner — `display-manager.service`
   (GDM, i.e. the GNOME session; stopping GDM ends cjdell's
   `gnome-session` and `start` auto-logins it again, verified on glass
   2026-09-12), `gemini-gemshell.service` (the native compositor), or
   legacy `gemwl.service` — plus the audio session
   (`pipewire/wireplumber/pipewire-pulse`). `sshd` +
   `gemini-battery-guard` + `gemini-sleepd` STAY (control link, the
   safety daemon, the wake button). **[2026-09-12]** the stop-list is now
   desktop-agnostic and no longer names the removed
   `phosh-nested`/`lxqt-nested` units; it stops and re-starts whichever
   panel owner is actually active. (History: `phosh-nested.service` had
   been missing from `SERVICES` — a sleep left phoc + the session running
   against a stopped gemwl, the observed "phosh not usable after the
   silver button".)
5. **Wifi down — fast**: `ip link set <iface> down` + kill
   wpa_supplicant + the iface's dhcpcd (legacy stack). The CONSYS chip
   itself stays powered (radio firmware idles): the WMT `echo off`
   teardown stalls ~29 s with the chip associated (observed
   2026-09-08), far too slow for a button — and stopping the
   RemainAfterExit wifi units alone does NOT stop wifi (no ExecStop;
   wpa_supplicant survives). Wake re-associates by RESTARTING
   gemini-wifi-auto (`wifi auto` — its clean-slate wpa_ensure restarts
   the daemon from scratch). **[changed 2026-09-10]** With the default
   NetworkManager stack (`services.geminiWifi.useNetworkManager`,
   docs/desktop-plumbing.md) sleep no longer kills wpa_supplicant (NM
   owns it; killing it just makes NM respawn it) or restarts the
   non-existent `gemini-wifi-auto` unit: it only parks the link and
   raises it on wake, letting NM autoconnect the saved profile
   (`gemcli sleep` detects NM via `systemctl is-active
   NetworkManager`).
6. **State recorded** in `/run/gemcli-sleep.state` (tmpfs — a reboot
   clears it and boots awake): services stopped, cpus offlined,
   backlight %, wifi was on.

`gemcli sleep off` reverses, again visible first: backlight on →
rebind inputs → online the recorded A53 cpus → bring the A72 cluster
back up if it was up at sleep time → start the recorded services +
`wifi auto` (async — the visible wake is backlight + cores; the
desktop comes back in the background).

`gemcli sleep key` is the daemon entry (`gemini-sleepd.service`, enabled
at boot, Restart=always): it reads `/dev/input/eventN` for mt6351-keys
(device found by scanning /sys/class/input names, not a hardcoded
eventN) and toggles on KEY_SLEEP press (value==1; repeats/releases
ignored).

Deliberately not in sleep's stop list (must survive to hear the wake
press): `gemini-sleepd.service` itself.

## Investigation receipts (2026-09-08, on glass, kernel 6.6.0 lean)

Setup: device USB-charging at a fixed 500 mA input (BQ25896
`iinlim=500mA`), load changes read as ICHGR changes (charge current —
when system load drops, charge current rises; 50 mA ADC steps) + VBAT
trend. See the per-state soak in the session log; raw ladder:
`/tmp/ladder.sh` on the device (7 states × 12 s).

| State | ICHGR (mA) | VBAT | Reading |
|---|---|---|---|
| backlight 100 %, desktop up | 0 (vbat sagging) | 3984-4004 | load > 500 mA input — battery discharging |
| backlight off | ~100 | 4084 rising | ≥100 mA@5 V recovered vs full backlight |
| + desktop stopped | ~100 | 4084 | desktop idle ≈ ≤50 mA |
| + audio stopped | ~100 | 4084 | ≤50 mA |
| + wifi off (CONSYS pwr-off) | ~100-150 | 4084-4104 | ≤50 mA |
| + cpus 1-7 offline | ~100-150 | 4104 | ≤50 mA bias |

Baseline on battery ≈ 450-480 mA@4.1 V ≈ 1.9 W (100 % backlight, LXQt
desktop up). Light-sleep floor ≈ 400 mA ≈ 1.6 W. **The light sleep is
the correct first step, but the big prize is deep sleep** (s2idle +
SPM/PMIC low-power), which would target <50 mA.

### What limits the awake floor (and why)

- **LCD panel logic stays on**: the NT36672 TDDI was initialised by LK
  into a self-refreshing state; the fbcon kernel (rule 5 — never merge
  the display stack) has no panel-blank path. Backlight off ≠ panel off.
- **CPU/DDR/SoC rails stay up at boot clocks**: `clk_ignore_unused
  pd_ignore_unused regulator_ignore_unused` (bring-up cmdline) stop the
  kernel gating anything; there is no cpufreq driver for MT6797 (no
  `/sys/devices/system/cpu/cpufreq/policy*` — DVFS is SCP/DVFSP-side
  and not exposed), so the A53 clusters run at fixed clocks.
- **Polled drivers keep the CPU busy-ish**: mt6351-keys polls pwrap
  TOPSTATUS every 25 ms (the side keys have no IRQ route in mainline);
  novatek touch is polled too (unbound in sleep).
- **A53 cluster power-down is unwired**: per-core PSCI offline is safe
  (what sleep does), but powering the A53 *clusters* off needs the
  vendor SPM sequences like the A72's cl2-down teardown — not done.

## `systemctl suspend` vs `gemcli sleep` — why they are not the same

**2026-09-11.** Two different layers; do not conflate them.

| | `systemctl suspend` | `gemcli sleep on` |
|---|---|---|
| Layer | kernel/systemd suspend-to-RAM (`s2idle`/`mem` via `/sys/power/state`) | userspace light sleep; kernel stays fully up |
| Mechanism | logind → `systemd-suspend.service` → every driver's `.suspend` callback → SoC low-power | backlight off, inputs unbound, A53s offline, A72 down, services stopped, wifi parked |
| Wake | needs a **wake-source IRQ** | the same silver button (`gemini-sleepd` toggles back) |
| Status here | **disabled** (locks up) | implemented + on glass (2026-09-08) |

`gemcli sleep` deliberately never enters the kernel suspend path. Before
deep sleep lands (below), `systemctl suspend` must stay disabled: there
is no `s2idle` wake source, so entering it means the unit never comes
back.

**What was changed (2026-09-11):**
- `config/gemini.nix`: `systemd.suppressedSystemUnits` removes the sleep
  targets + services (this nixpkgs pin has no `systemd.mask` option;
  NixOS generates `/etc/systemd/system` itself with no
  `/usr/lib/systemd/system` fallback, so a suppressed unit genuinely
  no longer exists).
- `services/gnome.nix`: locked dconf keys under
  `org.gnome.settings-daemon.plugins.power` (`sleep-inactive-ac-type`,
  `sleep-inactive-battery-type`, `power-button-action` = `nothing`) so
  GNOME never asks logind to suspend. `HandleSuspendKey=ignore` alone
  was **not** enough: it only stops the KEY_SLEEP evdev event, not
  logind's `Suspend()` D-Bus method.

The planned unification still stands once deep sleep works: drop the
suppression + `HandleSuspendKey=ignore`, set `systemd.sleep.settings`
`SuspendState=mem`, and move the light-sleep pieces into
`systemd-suspend.service` `ExecStartPre/Post` so the silver button and
logind share one path.

## Deep sleep (follow-up kernel work — the real <50 mA target)

The light sleep is what the button drives until these land:

1. **A wake source for s2idle.** `/sys/power/state` already offers
   `freeze`/`mem` (s2idle; `mem_sleep=[s2idle]`) and CONFIG_SUSPEND=y,
   but nothing can wake it: the silver/ESC keys are PMIC-debounced bits
   (TOPSTATUS 0x220) polled by mt6351-keys over pwrap with no IRQ path
   in mainline. Stock Android wakes via the PMIC INT → pwrap EINT
   status (vendor 3.18 `pmic_irq.c` reads `pmic_wrap_eint_status()`);
   wiring that (PMIC HOMEKEY/PWRKEY INT enable + pwrap INT_EN + a
   wake-capable IRQ the keys driver arms in suspend) is the missing
   piece. The pwrap IRQ (SPI 178, mt-pmic-pwrap) is already registered
   on glass.
2. **Suspend entry probe**: whether s2idle entry hangs on this bring-up
   kernel (any driver's .suspend callback) needs a WDT-escaped test
   (arm ~30 s WDT → EXRST is the recovery; holding Esc/On ~8-10 s is
   the PMIC-level hardware reset — always available).
3. Optional once (1)+(2) work: drop `clk_ignore_unused` etc. for the
   suspend path so clocks/domains actually gate in s2idle (a separate
   A/B — the flags exist because incomplete drivers wedge when gated).

When a wake source exists, the natural evolution: logind
`HandleSuspendKey=suspend` (drop the `=ignore`), `/etc/systemd/sleep.conf`
SuspendState=mem, and the sleep/wake prep in `systemd-suspend.service`
ExecStartPre/Post (the gemcli light-sleep pieces move there). Until
then, `HandleSuspendKey=ignore` stays and gemini-sleepd owns the button.

## Controls

| What | Command |
|---|---|
| Sleep now | `gemcli sleep on` (device) |
| Wake | `gemcli sleep off` |
| State | `gemcli sleep status` |
| Daemon (silver button) | `gemini-sleepd.service` (enabled; `gemcli sleep key` foreground) |
| Hand test without the button | ssh in, `gemcli sleep on` … `gemcli sleep off` |

State file: `/run/gemcli-sleep.state`. The daemon logs to the journal
(`journalctl -u gemini-sleepd -f`).

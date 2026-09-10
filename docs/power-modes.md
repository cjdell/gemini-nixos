# Power modes on the Gemini PDA — GNOME "Power Mode" → the A72 cluster

**Last updated:** 2026-09-10. **Status: ✅ ON GLASS** (deployed gens
4→6 via `bin/deploy.sh`; PPD advertises performance, the watcher maps
performance/balanced to the A72 cluster in ~2–4 s, and sleep round-trips
the cluster). Three device bugs were found and fixed during the flash —
see "On-glass history" below and `docs/session-log.md` 2026-09-10q.
Remaining: confirm the L/R correction by ear and one real reboot through
the 20 s settle.

Scope: make the standard GNOME power controls (Settings → Power "Power
Mode", and the Quick Settings power menu) do something real on this
device, and make the silver-button light sleep power the big A72 cluster
down too.

## The three asks this closes

1. **Sleep must turn off the A72 cores.** `gemcli sleep on` now powers
   the A72 cluster (cpu8/cpu9) down through the secure `cl2-down` path
   when it was up, and `sleep off` brings it back (it was already down
   in the default cold-boot state, so this is usually a no-op).
2. **A "performance" power mode visible in GNOME.** power-profiles-daemon
   (PPD) is patched so its generic *placeholder* driver advertises the
   `performance` profile, and a small watcher (`gemini-power-profile`)
   maps the active profile onto the A72 cluster:
   **performance ⇒ A72 up, balanced/power-saver ⇒ A72 down.**
3. (Audio asks live in `docs/desktop-plumbing.md` §Speakers:
   the swapped built-in speakers and the amp/headphone toggle.)

## Why the A72 cluster is the power-mode lever

There is **no cpufreq/DVFS driver** for MT6797 (no
`/sys/devices/system/cpu/cpufreq/policy*`; DVFS is SCP/DVFSP-side and not
exposed — `docs/power-sleep.md` §"What limits the awake floor"). The
A53s run at fixed clocks. The one CPU power lever this port controls is
the big **A72 cluster (cpu8/cpu9)**, normally **off** (cold-boot state):
bringing it up is the vendor cold sequence in `pkgs/gemcli/src/a72.rs`
(DA9214 BUCKB rail over i2c6 + WDT-armed PSCI) and adds genuine CPU
headroom. That is exactly a "performance" mode, and its absence is the
safe default.

## The standard interface, and the placeholder problem

GNOME's Power Mode selector talks to **power-profiles-daemon** on the
system bus (`org.freedesktop.UPower.PowerProfiles`). On hardware with no
ACPI/platform_profile CPU driver, PPD falls back to its generic
**placeholder** driver. Upstream, that driver advertises only
`power-saver` and `balanced` — `src/ppd-driver-placeholder.c`:

    g_object_set (object,
                  "driver-name", "placeholder",
                  "profiles", PPD_PROFILE_POWER_SAVER | PPD_PROFILE_BALANCED,
                  NULL);

so GNOME shows only two modes and there is no "Performance".

**Patch** (`patches/power-profiles-daemon-placeholder-performance.patch`,
one line + comment, applied in `services/power-profiles.nix`) adds
`PPD_PROFILE_PERFORMANCE` to that mask. This only changes what the
daemon *advertises*; the placeholder still drives no hardware itself.
The PPD build for the patched package runs with `doCheck = false` and
`-Dtests=false`: PPD's own integration suite asserts the unpatched
placeholder behaviour (e.g. `test_amd_pstate_error` expects the
performance transition to *fail* on a placeholder-only system), and the
test build needs umockdev. The daemon is exercised on glass instead.

PPD still persists the active profile across reboots
(`/var/lib/power-profiles-daemon/state.ini`, `[State] Profile=`).

## The actuator: `gemini-power-profile`

`gemini-power-profile.service` runs
`gemcli profile watch` (`pkgs/gemcli/src/profile.rs`). It polls PPD's
active profile and reconciles the cluster:

| PPD profile | A72 cluster |
|---|---|
| `performance` | `a72 up both` (cpu8 cold + cpu9 warm; WDT-guarded) |
| `balanced` / `power-saver` | `a72 down both` (cpu9 then cpu8; rail off) |

Design notes:

- **Cheap poll.** The watcher reads the active profile from PPD's
  `state.ini` on the poll path (rewritten on every change); when the
  file is absent PPD has never had a change, so the profile is its
  default `balanced`. It never spawns the Python `powerprofilesctl`
  from the watch loop (only the `status`/`set` one-shots do).
- **Boot settle.** PPD's profile is *persisted*, so a unit left in
  performance would otherwise request the A72 bring-up while boot is
  still busy. The watcher waits a short **20 s** (once per boot, flagged
  in `/run` — tmpfs, so a reboot resets it) before its first A72 action,
  matching the "bring the cluster up from a settled system" rule
  (`docs/phase-2-on-glass.md` P2). **A profile change during that window
  bypasses the rest of the settle**, so a user picking a mode in GNOME
  never waits for it (on glass 2026-09-10q the 90 s version made GNOME
  say "performance" while the cores stayed off for a minute and a half).
  `a72::up` also retries the contended DA9214 i2c6 bus with backoff and
  arms the WDT.
- **A72 operations are serialized by an flock**
  (`/run/gemini-a72.lock`, `pkgs/gemcli/src/a72.rs`). Two concurrent
  secure A72 ops (a `sleep on` teardown and a watcher bring-up) hung the
  device on glass 2026-09-10q: the second op drove the DA9214 rail and
  the secure SMC while the first was still stepping. The sleep path
  writes `state=sleeping` before any teardown and the watcher re-checks
  that state while holding the lock, so a sleep that starts mid-poll
  always wins.
- **Sleep-aware.** The watcher does nothing while the light sleep owns
  the device (`sleep::sleeping()`); the sleep path powers the cluster
  down itself and restores it on wake, and the watcher re-applies the
  active profile after wake.
- **Manual `a72` remains authoritative.** With no PPD the watcher has
  nothing to do; `gemcli a72 up|down` still works directly.

`gemini-sleepd` (`services/gemini-pda.nix`) records whether the A72 was
up, powers it down in `sleep on`, and brings it back in `sleep off`.

## Manual use

| What | Command |
|---|---|
| Set the mode (GNOME or CLI) | `gemcli profile set performance` / `balanced` / `power-saver` |
| Active mode + cluster state | `gemcli profile status` |
| Watch daemon | `gemini-power-profile.service` (`gemcli profile watch` foreground) |
| A72 directly (bypass PPD) | `gemcli a72 up\|down\|status` |

GNOME's Settings → Power "Power Mode" and the Quick Settings power menu
set the PPD profile; the watcher applies it to the cluster within ~2 s
(or immediately via `gemcli profile set`).

## Files

| Path | What |
|---|---|
| `services/power-profiles.nix` | PPD (patched) + `gemini-power-profile.service` |
| `patches/power-profiles-daemon-placeholder-performance.patch` | advertise `performance` in the placeholder |
| `pkgs/gemcli/src/profile.rs` | the watcher + `profile status/set` |
| `pkgs/gemcli/src/a72.rs` | the WDT-guarded cluster up/down |
| `pkgs/gemcli/src/sleep.rs` | sleep now powers the A72 down / restores it |
| `config/gemini.nix` | imports `services/power-profiles.nix` |

## On-glass verification checklist (do this before calling it done)

1. `systemctl status gemini-power-profile power-profiles-daemon` — both
   active; `gemcli profile status` shows the profile + cluster state.
2. `powerprofilesctl list` on the device lists **performance**.
3. GNOME Settings → Power shows the three-way **Power Mode** selector;
   the Quick Settings power menu lists it too.
4. `gemcli profile set performance` → within a few seconds `cpu8/9`
   online (`gemcli profile status`, `/sys/devices/system/cpu/online`).
   `gemcli profile set balanced` → back to `0-7`.
5. Silver button: with the A72 up, `sleep on` → cluster down;
   `sleep off` → cluster back up. (`gemcli sleep status` shows
   `A72 cluster: was up (will be restored)`.)
6. Reboot with performance selected: confirm the watcher's short
   (20 s) settle and that the unit still boots clean.
7. Log one version line per flash in `docs/session-log.md`.

**Recovery:** a wedged A72 bring-up is recovered by the WDT EXRST
(`bin/device-reboot.sh`) or the PMIC reset (hold Esc/On); the full
playbook is `docs/disaster-recovery/drills.md`.

## On-glass history (2026-09-10q, after the first flash)

- ✅ PPD advertises performance (`powerprofilesctl list`); the GNOME
  selector and the watcher both work.
- ✅ `performance` → A72 up (cpu 0-9) and `balanced` → A72 down (0-7),
  driven through PPD exactly as GNOME does.
- ✅ Speaker amp follows the PipeWire default sink: headphones →
  `dout=0`, speakers → `dout=1`.
- 🐛 The 90 s settle made GNOME show performance while the cores were
  off — shortened to 20 s + change-bypass (above).
- 🐛 A `sleep on` teardown raced the watcher and hung the device; fixed
  by the early sleep state + the flock (above).
- 🐛 The first deploy had an ordering cycle (`gemini-power-profile`
  after `power-profiles-daemon`, which is `After=multi-user.target`) and
  the unit was skipped at boot; fixed by dropping the ordering (a
  `wants` only).

## Risks / limitations

- The A72 bring-up is the vendor secure path; it has been round-tripped
  on glass (2026-09-08, `docs/gemcli.md` §On-glass receipts) but a power
  mode makes it user-reachable. The WDT is armed during the sequence, so
  a hang self-recovers with a reboot rather than a dead unit.
- performance runs hotter and draws more; there is no thermal governor
  on this port yet. The mode is opt-in (default balanced) for that
  reason.
- No A53 frequency scaling, so power-saver == balanced for the cluster;
  the profiles differ only in the A72 state (and whatever desktop power
  saving PPD/other daemons do).

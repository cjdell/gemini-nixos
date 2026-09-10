# gemcli — the Rust device-control CLI

**Status (2026-09-08): ON GLASS since gen20.** Built + packaged (native
aarch64, `rc=0`, 8 unit tests green in-sandbox), deployed to the
device via `bin/deploy.sh` (gens 16–21), **`gemcli selfcheck` = ALL
PASS** (all 8 read-only probes). Battery/charger/backlight parity is
byte-identical with the scripts; `a72 up`/`down` round-trip verified
on glass (cpu8 cold + cpu9 warm up; per-core + last-A72 teardown down
with the DA9214 rail drop — device left in cold-boot state). The
systemd units STILL ExecStart the scripts — the unit flips are the
remaining step, lowest-risk-first (§Parity & migration).

gemcli consolidates the device functions that the verified bring-up
shell scripts in `services/scripts/` handle into ONE native Rust
binary installed on the device. Each subcommand is a semantic port of
its script (module headers carry the receipts), with the script's exit
codes where a caller depends on them.

## Layout & build

| Path | What |
|---|---|
| `pkgs/gemcli.nix` | `rustPlatform.buildRustPackage` derivation (native aarch64; `cargoLock` on the committed `Cargo.lock`) |
| `pkgs/gemcli/Cargo.toml` | crate manifest — deps: **clap 4.5 (derive)** + **libc 0.2** only |
| `pkgs/gemcli/src/*.rs` | one module per function (see map below) + `main.rs` (clap tree) |

Standalone build (host, distributes to the 192.168.49.191 builder):
`bash bin/run-job.sh start gemcli-build -- sudo nix build --store
local --option builders @/etc/nix/machines --fallback --print-out-paths
.#packages.aarch64-linux.gemcli` (86 s first build). In the rootfs
closure via `services/gemini-pda.nix` → `environment.systemPackages`;
also exposed as the flake package `.#packages.aarch64-linux.gemcli`.
(The compiled test suite is now **11 tests** — the original 8 plus an
A72-list and a PPD state.ini parser test — 2026-09-10.)

Local typecheck/test loop (host, x86_64 cargo): `cd pkgs/gemcli &&
cargo check && cargo test` (hardware-free unit tests only; keep
`target/` out of git — `.gitignore` has it).

## Subcommand ↔ script map

| gemcli | Script (services/scripts/) | Unit it would back |
|---|---|---|
| `backlight get\|set\|max\|min\|off\|on\|status\|raw` | `backlight` | `gemini-backlight-default` |
| `battery status` | `battstat` | (harness interface) |
| `charger raw [samples] [int]` | `bq25896-raw.sh` | — |
| `power status\|watch\|charge\|dim-to-charge` | `power` | — |
| `guard run\|status` | `battery-guard.sh` | `gemini-battery-guard` |
| `a72 up [cpu8\|cpu9\|both]`, `a72 down […], a72 status` | `cl2-up.sh`, `cl2-down.sh` | `gemini-a72-up` |
| `sleep on\|off\|status\|key` | — (new; the light clamshell sleep — see docs/power-sleep.md) | **`gemini-sleepd`** (2026-09-08) |
| `profile watch\|status\|set <performance\|balanced\|power-saver>` | — (new; power-profiles-daemon → A72 — see docs/power-modes.md) | **`gemini-power-profile`** (2026-09-10) |
| `gpu poweron\|status` | `gemini-gpu-poweron.sh` | `gemini-gpu-poweron` |
| `wdt-reboot [SECS]` | `gemini-wdt-reboot` | `gemini-wdt-reboot` |
| `boot status\|recovery\|debian\|nixos [--no-reboot]` | `gemini-boot-recovery`, `gemini-boot-debian` | those units |
| `speaker on\|off\|status`, `speaker watch\|sync` | `speaker` (gpioout/spkamp C helpers) | gemini-speakerd (2026-09-10) |
| `status` | aggregate (battstat + charger raw + backlight + cpu/a72 + gpu + boot + guard) | — |
| `selfcheck` | read-only on-glass parity harness | — |
| `version` / `-V` | — | rule-0 identity banner |

NOT ported (phase 2 of the migration, still shell): the *orchestrator*
CLIs — `wifi`/`wifi-internal` (wpa_supplicant/dhcpcd/iw process + config
management) and `audio-output`/`audio-defaults` (ALSA route setup +
PipeWire session). They manage daemons and card state rather than
device registers; the migration doc will treat them separately.

## Semantics & access model

### `sleep` (added 2026-09-08 (7th), ON GLASS — the silver-button sleep/wake)

`gemcli sleep on|off|status|key` — the reversible LIGHT clamshell sleep
(no kernel suspend — see docs/power-sleep.md for the full investigation
and the deep-sleep follow-up). Responsiveness contract (v2,
2026-09-08): the backlight goes off FIRST (instant press feedback) and
the whole transition is ~1-2 s — nothing blocks on the CONSYS chip
teardown. `on`: backlight off (`bl_power=4`; brightness retained),
unbind the clamshell input drivers — the gpio-matrix-keypad platform
device `keyboard` and the novatek-nt36xxx i2c client `4-0062` — so the
closed lid's key presses produce no input, offline A53 cpus 1..7
(cpu0 stays), power the A72 cluster down if it was up (the performance
power mode can leave it online — docs/power-modes.md), stop the
heavyweight services that were running
(gemwl/LXQt, pipewire/wireplumber/pipewire-pulse), and take wifi down
fast (`ip link set <iface> down` + kill wpa_supplicant/dhcpcd — the
CONSYS chip STAYS powered; the WMT `echo off` teardown stalls ~29 s
and the RemainAfterExit wifi units have no ExecStop, so stopping them
never stopped wifi anyway). `mt6351-keys` stays bound: KEY_SLEEP is
the wake button. State (services stopped / cpus offlined / backlight %
/ wifi was on) is recorded in `/run/gemcli-sleep.state` (tmpfs) and
`off` reverses it visible-first (backlight on → rebind inputs → cpus
→ services async + `wifi auto` via a unit RESTART — plain start is a
no-op on the still-active oneshot). `key` watches the mt6351-keys
evdev node (found by scanning /sys/class/input names, not a hardcoded
eventN), toggles on KEY_SLEEP press (value==1), debounces presses
(1 s — one physical press = one toggle) and drains events queued
while a toggle ran (no mash cascade); it backs
`gemini-sleepd.service` (enabled at boot, Restart=always). Exit codes
0 ok / 1 failure; idempotent both ways.

Same runtime access as the scripts — no new kernel features:
- **Registers**: `/dev/mem` mmap (busybox-devmem equivalent; kernel has
  `CONFIG_DEVMEM=y`, no IO_STRICT_DEVMEM — the SPM/DISP_PWM0/WDT ranges
  are glass-proven since 2026-09-01). Module `devmem.rs`.
- **i2c**: i2c-dev ioctls (i2c-tools equivalent) — including the
  adapter-by-DT-base resolution (`adapter_for("1100e000")`, the
  2026-09-04 fix for shifting adapter numbers), plain vs `-f`
  (I2C_SLAVE_FORCE) claiming, and the "ACK is the trusted channel"
  rule for the DA9214 bus. Module `i2c.rs`.
- **Charger**: BQ25896 raw conversion-trigger + register decode
  (`charger.rs`, i2c-0 @0x6b).
- **GPIO**: kernel gpio chardev v1 linehandle API for driving pads
  243/244 (gpioout.c equivalent — request as output, set, close; no
  /dev/mem pinctrl writes). Reading a pad level is DIFFERENT: the v1
  API can only read by requesting the pad as *input*, which would
  release the amp's output drive, so `speaker status` / `selfcheck`
  read the pinctrl DOUT register via /dev/mem (spkamp's side-effect-
  free method) **[changed 2026-09-10]**. Module `gpio.rs` (drive) +
  `speaker.rs` (pinctrl read).
- **sysfs**: power supplies, backlight, cpu hotplug + online maps.
- **Time**: UTC civil time computed in-house (the device runs UTC) —
  no chrono in the closure.

### Intentional corrections over the scripts (`[corrected 2026-09-08]`,
each noted in the module header too)

1. **cl2-up.sh exit code**: the script's final status was the trailing
   `log` echo's rc — always 0 even after "GAVE UP after 6 attempts";
   `gemcli a72 up` returns the real outcome (0 = requested cpus online).
   The down path's exit codes were already correct and are preserved.
2. **gemini-boot-recovery para write**: the script wrote a SHORT
   15-byte record (no `conv=sync`, so bytes 15..31 of the 32-byte MISC
   region were left stale); gemcli always writes the full padded 32-byte
   command — matching the newer `gemini-boot-debian` behaviour and the
   layout documented in AGENTS.md.
3. **battery-guard history header**: the bash rotation wrote a 7-column
   header while rows kept 8 fields (latent CSV misalignment); gemcli
   always writes the 8-column header. State file and CSV formats are
   otherwise byte-identical (including the `ibat_uA` key name).

### Exit codes (script contracts preserved)

| Command | Codes |
|---|---|
| `backlight` | 0 ok / 1 usage / 2 /dev/mem / 3 bad value |
| `battery status` (battstat) | 0 ok / 2 low (<3650 mV, on battery) / 3 USB but not charging / 4 critical (<3500 mV) / 5 no bq25890 psy |
| `power dim-to-charge` | 0 charging / 1 error / 2 floor (input power insufficient) |
| `a72 down`, `speaker` | 0 ok / 1 failed / 2 usage |
| everything else | 0 ok / 1 failure |

`guard run` honours the same env knobs as the script
(`BATTERY_GUARD_POLL_S`, `BATTERY_GUARD_WARN_LOW_MV=3650`,
`BATTERY_GUARD_CRIT_MV=3500`, `BATTERY_GUARD_ALERT_COOLDOWN_S=300`,
`BATTERY_GUARD_STUCK_CHARGE_MIN=15`) and writes the same
`/run/battery-guard/state` + `/var/log/battery-history.csv` outputs.

## On-glass receipts (2026-09-08, gens 16–21)

- **gpio ioctl fix**: `speaker`/`selfcheck` gpio probes failed EINVAL
  while the C `gpioout` succeeded. strace + the v6.6 UAPI header
  (`include/uapi/linux/gpio.h`) showed the v1 ioctl numbers were
  REORGANISED after the pre-5.x kernels: `GPIO_GET_LINEHANDLE_IOCTL`
  is nr **0x03** (nr 0x02 is `GPIO_GET_LINEINFO_IOCTL`). Fixed +
  regression test pinning the exact `_IOC` literals. Chip resolution
  also scans `GPIO_GET_CHIPINFO_IOCTL` ngpio per chip (this kernel
  boots gpiochip0 `pinctrl_paris` ngpio=262 + gpiochip1 `aw9523b` 16).
- **selfcheck ALL PASS** (8/8): devmem reads, bq25890 psy, raw i2c
  charger read, backlight, cpu map, para, gpio pads 243/244, gpu regs.
- **Byte-identical parity**: `battstat` vs `battery status` (output +
  rc 0), `bq25896-raw.sh` vs `charger raw`, `backlight get` (9 % on
  the 10 % default — duty rounding) and a write round-trip
  (`set 20` → both tools + sysfs read 20, restored to 10).
- **a72 round-trip**: `gemcli a72 up both` — cpu8 cold on attempt 1
  (DA9214 bus i2c-2, WDT-armed PSCI) + cpu9 warm → online 0-9;
  `gemcli a72 down both` — cpu9 per-core, cpu8 last-A72 secure
  teardown (ISO bit1 re-asserted, PWR_CON bit0 clear), DA9214 BUCKB
  rail dropped → online 0-7, cold-boot state, rc 0 both ways.

## Parity & migration (the on-glass pass — step 1–2 DONE, flips remain)

Order of work once gemcli is on a device (deploy via `bin/deploy.sh`
or a rootfs flash):

1. **Install + identity**: `gemcli version` on the device; log the
   version line (rule 0) in `docs/session-log.md`.
2. **`gemcli selfcheck`** — read-only probes (devmem reads, psy
   presence, raw charger read, backlight read, cpu map, para present,
   gpio pads readable, gpu status regs). Every probe PASS before
   anything else. Note in the log which probes failed and why.
3. **Per-command parity diff** (script vs gemcli output, same boot):
   - `battstat` vs `gemcli battery status` (stdout + exit code)
   - `bq25896-raw.sh 1 0` vs `gemcli charger raw`
   - `backlight get/status` vs `gemcli backlight get/status`
   - `power status` vs `gemcli power status`
   - `boot` read side: `gemcli boot status` vs a `para` hexdump
   - `gemcli a72 status` vs the cl2 scripts' register reads
   - guard: run BOTH the daemon and `gemcli guard status` (state file
     is the shared contract); compare a history CSV row
   - `speaker status` vs `spkamp status 243 244` pad levels
4. **Read-only → write flips, lowest risk first**, one per boot, each
   verified + logged:
   1. `gemini-backlight-default` ExecStart → `gemcli backlight set 10`
      (pure PWM write; verify `backlight get` reads back 10).
   2. hand-run CLIs only: `gemcli wdt-reboot 20` from a shell,
      `gemcli boot recovery`/`debian`/`nixos` with `--no-reboot` first
      then for real, `gemcli a72 up`/`down` on a settled system
      (same WDT guard rails as the scripts).
   3. `gemini-gpu-poweron` ExecStart → `gemcli gpu poweron`
      (watch: same "status bits both regs" final check; GPU must not
      fault — rule 5: if the glass flickers STOP and go to TWRP).
   4. `gemini-battery-guard` ExecStart → `gemcli guard run` LAST —
      it is the safety daemon; only flip after the parity pass shows
      identical state/CSV behaviour, and keep the bash daemon around
      for one WDT-reboot A/B.
   5. `gemini-a72-up` (opt-in unit) → `gemcli a72 up`.
5. Only after the flips land may `services/scripts/` CLIs be pruned
   from the closure — and even then keep `busybox` (devmem) + i2c-tools
   for serial-console hand use per services/gemini-pda.nix.

## Versioning

Every on-device run/flip logs one version line: `gemcli 0.1.0`
(crate version) — the exact store path + build id from
`gemcli version`. Regenerating the lockfile changes the vendor closure;
commit `Cargo.lock` with the code.

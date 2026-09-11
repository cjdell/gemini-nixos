# Desktop/session selection — GNOME, COSMIC, niri, gemshell, or the framebuffer console

Last updated: 2026-09-11

**Status (2026-09-10): DEPLOYED to glass as device generation 9
(`zqkz1vh…`); the selector + GNOME path are verified live (AccountsService
session set, sentinel logic in place, GNOME session preserved). The
COSMIC session itself is still UNVERIFIED on glass.**

**niri (2026-09-11): added as a fourth co-installed session and
DEPLOYED to glass as device generation 11 (`343g0r3v…`, commit
`5e43187`).** It uses the same GDM Wayland-session + selector path as
GNOME/COSMIC (`programs.niri.enable` registers `niri`;
`gemcli session set niri`); niri 26.04 is aarch64-cache-verified at the
flake pin. The deploy did **not** restart display-manager (live COSMIC
session preserved, 0 failed units). On-glass niri boot pending — see the
checklist below.

## The ask

> Try the COSMIC desktop without breaking GNOME. Can they co-exist? Can
> `gemcli` switch between them and/or set the startup default? A third
> no-desktop mode that just stays in the framebuffer console would be
> useful.

## Short answer

- **Yes, GNOME, COSMIC and niri co-exist** — they are all ordinary GDM
  Wayland *sessions* (`services.displayManager.sessionPackages`), so
  all are installed and GDM offers all of them. Only **one owns the
  panel at a time**, and nothing about installing one changes the other
  sessions or their builds.
- **Yes, `gemcli session` switches them** — a persistent marker
  (`/var/lib/gemini/desktop`) is resolved at boot before GDM starts, so
  the choice is runtime-mutable without a rebuild. `gemcli session set
  niri --reboot` is the "switch now" path.
- **Console mode exists** — `gemcli session set console`: GDM is
  skipped and the fbcon console (with an autologin getty on tty1) stays.

## Why they can co-exist (and why only one runs at a time)

Since 2026-09-10 the LCD is a real KMS device
(`geminipda-drm` → `/dev/dri/card0`) rendered through panfrost via Mesa
`kmsro`. GNOME's `mutter` holds that device as its primary GPU; COSMIC's
`cosmic-comp` (smithay) and niri (smithay) would too. Two compositors
cannot both modeset the one CRTC, but *nothing about that is a build
conflict*:

- `services.desktopManager.gnome.enable`,
  `services.desktopManager.cosmic.enable` and `programs.niri.enable`
  are independent options that each append their session package to
  `services.displayManager.sessionPackages` (eval receipt below:
  `["gnome","cosmic","niri"]`).
- They also all add `xdg.portal` backends; the list-typed options
  concatenate, so the merged portal packages are unchanged by niri
  (its `xdg.portal.config.niri` reuses `xdg-desktop-portal-gnome`/`-gtk`,
  already pulled in by GNOME, and lists `gnome`/`gtk` as its backends),
  and the portal picks the backend from `XDG_CURRENT_DESKTOP`.
- The legacy nested stack (`gemwl` + Phosh/LXQt) is a *different*
  architecture (it owns `/dev/gemfb` directly). `services/gnome.nix`
  still force-disables it; the selector does not disturb that.

The selector only decides **which session GDM auto-logs into per boot**.

## Architecture

```
/var/lib/gemini/desktop              "gnome" | "cosmic" | "niri" | "console"
        |                            (persistent, root-owned; written by
        |                             `gemcli session set`)
        v
gemini-desktop-apply.service         oneshot, Before=display-manager.service,
  (services/scripts/                 WantedBy=multi-user.target
   gemini-desktop-apply)
        |
        | gnome|cosmic|niri : AccountsService SetSession/SetSessionType
        |                for cjdell (Wayland)  + rm /run/gemini-console
        | gemshell     : touch /run/gemini-console (NO getty, NO chvt —
        |                gemini-gemshell.service starts on that sentinel)
        |
        | console      : touch /run/gemini-console
        |                + start getty@tty1 (autologin cjdell)
        v
display-manager.service (GDM)
  ConditionPathExists=!/run/gemini-console   -> skipped in console mode
  otherwise GDM auto-logs cjdell into the AccountsService session
```

Module: `services/desktop-select.nix` (`services.geminiDesktop.*`),
wired in `config/gemini.nix`.

## Why a marker and not `services.displayManager.defaultSession`

`defaultSession` is emitted as a GDM **preStart** call to
`set-session` (nixpkgs `gdm.nix`: `preStart = "... set-session
$autologinSession"` when `defaultSession != null`). That runs on
**every** display-manager start, so it would overwrite a runtime choice
on the next boot. `services/gnome.nix` therefore leaves it unset
(`null`), and the applier reproduces set-session's mechanism directly
(`AccountsService` `SetSession` + `SetSessionType("wayland")` via
`busctl` — no python in the closure). GDM reads that saved session for
auto-login. **`programs.niri` is the exception that makes this worth
spelling out**: its module pins `displayManager.defaultSession = "niri"`
with `mkDefault`, so `services/desktop-select.nix` now `mkForce`-es the
option back to `null` — otherwise GDM's preStart (which runs *after* the
applier) would overwrite the marker's session with `niri` on every
display-manager start.

## Modes

| Mode | Panel owner | Start path | Notes |
|---|---|---|---|
| `gnome` | `mutter` on card0 (panfrost/kmsro) | GDM autologin → `gnome` session | **Verified on glass** (docs/gnome-feasibility.md). Default. |
| `cosmic` | `cosmic-comp` on card0 | GDM autologin → `cosmic` session | COSMIC 1.6.0. **Unverified on glass** — see checklist. |
| `niri` | `niri` on card0 (smithay) | GDM autologin → `niri` session | niri 26.04. **Unverified on glass** — see checklist. |
| `gemshell` | the `gemshell` compositor on card0 (a SYSTEM service, User=cjdell — not a GDM session) | display-manager skipped; `gemini-gemshell.service` (ConditionPathExists=/run/gemini-console) | Native Rust Wayland compositor + gemsettings (docs/gemshell.md). **Build-level only** (2026-09-11) — on-glass checklist owed. No tty1 getty (the compositor owns the panel). |
| `console` | none (fbcon tty1) | display-manager skipped; `getty@tty1` | No desktop; serial console (ttyS0) unaffected. |

## gemcli commands

```
gemcli session status          # marker, console sentinel, AccountsService session
gemcli session list            # the five modes
gemcli session set niri        # persist; takes effect next boot
gemcli session set niri --apply    # also update the applier now
gemcli session set niri --reboot   # persist + reboot (clean switch)
gemcli session set cosmic      # (same, for COSMIC)
gemcli session set gemshell    # native compositor from next boot
gemcli session set console     # no desktop from next boot
gemcli session apply           # re-run the applier for the current marker
```

`set` writes the marker atomically. `--apply` runs the same applier the
boot unit runs; `--reboot` uses the healthy `systemctl reboot`
(docs/power-states.md).

The marker lives on the 58 GiB p27 `linux` rootfs, so it survives
reboots and `nixos-rebuild switch`; the compile-time
`services.geminiDesktop.mode` is only the fallback for a fresh install
with no marker.

## COSMIC build facts (rule 9 verification, 2026-09-10)

Verified at the flake pin `dc5d91f84032`
(26.11pre1068949):

| Check | Result |
|---|---|
| `services.desktopManager.cosmic` option | present (`nixos/modules/services/desktop-managers/cosmic.nix`) |
| `cosmic-session` provided session name | `cosmic` |
| aarch64 cache.nixos.org for `cosmic-session/comp/panel/settings` + `xdg-desktop-portal-cosmic` | **CACHED** (`nix path-info --store https://cache.nixos.org`, 2026-09-10) |
| merged session list (eval) | `["gnome","cosmic","niri"]` |
| `services.displayManager.defaultSession` (eval) | `null` (the selector `mkForce`-es it back to null; see above) |
| `display-manager` unit Condition (eval) | `!/run/gemini-console` |
| applier unit (eval) | `ExecStart` = gemini-pda-utils `gemini-desktop-apply`; env `GEMINI_DESKTOP_DEFAULT=gnome`, `GEMINI_DESKTOP_USER=cjdell`; `before=["display-manager.service"]` |

The only local compiles this adds are the rebuilt `gemini-pda-utils`
(new script) and `gemcli` (new `session` subcommand).

## niri build facts (rule 9 verification, 2026-09-11)

Verified at the flake pin `dc5d91f84032` (26.11pre1068949):

| Check | Result |
|---|---|
| `programs.niri` module | present (`nixos/modules/programs/wayland/niri.nix`) |
| `niri` package version | `niri-26.04` |
| `niri` provided session name | `niri` (`passthru.providedSessions = ["niri"]`; `share/wayland-sessions/niri.desktop`) |
| aarch64 cache.nixos.org for `niri` | **CACHED** (`nix path-info --store https://cache.nixos.org`, 2026-09-11) |
| `programs.niri` side effects | adds `niri` to `environment.systemPackages` + `services.displayManager.sessionPackages`; `systemd.packages = [niri]`; `xdg.portal.config.niri`; `gnome-keyring` (`mkDefault`) |
| ⚠ `programs.niri` opens | `services.displayManager.defaultSession = lib.mkDefault "niri"` — **overridden back to `null`** by `services/desktop-select.nix` (`mkForce`) so the marker stays authoritative |

niri is a native KMS/smithay compositor (like `cosmic-comp`), not a
nested one; it needs the same `/dev/dri/card0` (geminipda-drm) +
panfrost/kmsro pairing the verified GNOME path uses.

## On-glass checklist (COSMIC / niri, in order)

1. Deploy the toplevel (`bin/deploy.sh`) — no reflash; the KMS boot.img
   is unchanged.
2. `gemcli session status` — marker/effective session sane.
3. `gemcli session set cosmic --reboot` (then repeat the block below for
   `niri`).
4. After boot: `systemctl status display-manager`, `journalctl -b -u
   display-manager`, `systemctl --user` / `loginctl` show a COSMIC
   session; `XDG_CURRENT_DESKTOP=COSMIC` in the session env.
5. Confirm the panel renders (eyes-on-glass — **rule 5**: if the glass
   ever flickers, stop and go to TWRP). COSMIC must not need more from
   `geminipda-drm` than mutter already uses (atomic modeset + a shadow
   primary plane; there is no cursor/overlay plane — smithay is expected
   to fall back to a software cursor). niri has the same expectation.
6. `gemcli session set gnome --reboot` — verify the return trip.
7. `gemcli session set console --reboot` — verify no GDM, fbcon tty1
   autologin; then back to `gnome`.
8. `gemcli session set gemshell --reboot` — verify no GDM, no tty1
   getty, `gemini-gemshell.service` active and the panel shows the
   gemshell scene (its own checklist: docs/gemshell.md);
   `gemcli session set gnome --reboot` for the return trip.
9. Log the version lines + outcomes in `docs/session-log.md` (rule 0).

### niri-specific checks

- `gemcli session set niri --reboot`; after boot verify `niri` is the
  session (`loginctl`, `$XDG_CURRENT_DESKTOP=niri`), `journalctl -b -u
  display-manager` clean, and the panel renders.
- niri config lives at `~/.config/niri/config.kdl` (a default config is
  shipped if absent — `niri --validate`). This repo deliberately ships
  NO niri config; add one later if the on-glass pass needs device keys
  (the gemini xkb layout reaches it via `XKB_CONFIG_ROOT`, same as
  GNOME).
- niri has no built-in greeter of its own; GDM stays the greeter.

### Known risks / open questions

- **COSMIC/niri + `geminipda-drm` capability matrix.** mutter is
  verified on this driver; `cosmic-comp`/smithay and niri (also
  smithay) may request a cursor plane, more formats or modifiers. If it
  fails to bring up the output, capture the journal and consider an
  smithay software-cursor/rgba8888 config before touching the driver
  (the driver is shared with the verified GNOME path — do not regress
  it).
- **GDM auto-login session source.** The mechanism (AccountsService
  `Session`/`SessionType`) is the one nixpkgs' `set-session.py` uses for
  `displayManager.defaultSession`, so it is the sanctioned path; the
  on-glass check in step 4 is the confirmation.
- A live switch (no reboot) is deliberately NOT offered: restarting GDM
  over a running KMS session is a panel-risk we do not need.

## Files

| Path | What |
|---|---|
| `services/desktop-select.nix` | the selector module (`services.geminiDesktop.*`) |
| `services/scripts/gemini-desktop-apply` | boot applier (marker → AccountsService / console sentinel) |
| `config/gemini.nix` | `services.desktopManager.cosmic.enable = true` + `programs.niri.enable = true` + selector wiring |
| `services/gnome.nix` | leaves `displayManager.defaultSession` unset (selector owns it) |
| `pkgs/gemcli/src/session.rs` | `gemcli session` (modes gnome/cosmic/niri/gemshell/console) |
| `pkgs/gemcli/src/main.rs` | `session set <mode>` clap value_parser (gnome/cosmic/niri/console) |

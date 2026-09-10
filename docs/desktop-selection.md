# Desktop/session selection — GNOME, COSMIC, or the framebuffer console

Last updated: 2026-09-10

**Status: build-level (eval + toplevel build green). On-glass COSMIC
verification pending; GNOME + console paths follow the already-verified
GNOME/KMS stack.** Nothing here is on glass yet as a *selector*
(2026-09-10).

## The ask

> Try the COSMIC desktop without breaking GNOME. Can they co-exist? Can
> `gemcli` switch between them and/or set the startup default? A third
> no-desktop mode that just stays in the framebuffer console would be
> useful.

## Short answer

- **Yes, GNOME and COSMIC co-exist** — they are both ordinary GDM
  Wayland *sessions* (`services.displayManager.sessionPackages`), so
  both are installed and GDM offers both. Only **one owns the panel at
  a time**, and nothing about installing COSMIC changes the GNOME
  session or its build.
- **Yes, `gemcli session` switches them** — a persistent marker
  (`/var/lib/gemini/desktop`) is resolved at boot before GDM starts, so
  the choice is runtime-mutable without a rebuild. `gemcli session set
  cosmic --reboot` is the "switch now" path.
- **Console mode exists** — `gemcli session set console`: GDM is
  skipped and the fbcon console (with an autologin getty on tty1) stays.

## Why they can co-exist (and why only one runs at a time)

Since 2026-09-10 the LCD is a real KMS device
(`geminipda-drm` → `/dev/dri/card0`) rendered through panfrost via Mesa
`kmsro`. GNOME's `mutter` holds that device as its primary GPU; COSMIC's
`cosmic-comp` (smithay) would too. Two compositors cannot both modeset
the one CRTC, but *nothing about that is a build conflict*:

- `services.desktopManager.gnome.enable` and
  `services.desktopManager.cosmic.enable` are independent options that
  each append their session package to
  `services.displayManager.sessionPackages` (eval receipt below:
  `["gnome","cosmic"]`).
- They also both add `xdg.portal` backends; the list-typed options
  concatenate, so the merged set is
  `gnome-session + xdg-desktop-portal-cosmic` (verified), and the portal
  picks the backend from `XDG_CURRENT_DESKTOP`.
- The legacy nested stack (`gemwl` + Phosh/LXQt) is a *different*
  architecture (it owns `/dev/gemfb` directly). `services/gnome.nix`
  still force-disables it; the selector does not disturb that.

The selector only decides **which session GDM auto-logs into per boot**.

## Architecture

```
/var/lib/gemini/desktop              "gnome" | "cosmic" | "console"
        |                            (persistent, root-owned; written by
        |                             `gemcli session set`)
        v
gemini-desktop-apply.service         oneshot, Before=display-manager.service,
  (services/scripts/                 WantedBy=multi-user.target
   gemini-desktop-apply)
        |
        | gnome|cosmic : AccountsService SetSession/SetSessionType
        |                for cjdell (Wayland)  + rm /run/gemini-console
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
auto-login.

## Modes

| Mode | Panel owner | Start path | Notes |
|---|---|---|---|
| `gnome` | `mutter` on card0 (panfrost/kmsro) | GDM autologin → `gnome` session | **Verified on glass** (docs/gnome-feasibility.md). Default. |
| `cosmic` | `cosmic-comp` on card0 | GDM autologin → `cosmic` session | COSMIC 1.6.0. **Unverified on glass** — see checklist. |
| `console` | none (fbcon tty1) | display-manager skipped; `getty@tty1` | No desktop; serial console (ttyS0) unaffected. |

## gemcli commands

```
gemcli session status          # marker, console sentinel, AccountsService session
gemcli session list            # the three modes
gemcli session set cosmic      # persist; takes effect next boot
gemcli session set cosmic --apply    # also update AccountsService now
gemcli session set cosmic --reboot   # persist + reboot (clean switch)
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
| merged session list (eval) | `["gnome","cosmic"]` |
| `services.displayManager.defaultSession` (eval) | `null` |
| `display-manager` unit Condition (eval) | `!/run/gemini-console` |
| applier unit (eval) | `ExecStart` = gemini-pda-utils `gemini-desktop-apply`; env `GEMINI_DESKTOP_DEFAULT=gnome`, `GEMINI_DESKTOP_USER=cjdell`; `before=["display-manager.service"]` |

The only local compiles this adds are the rebuilt `gemini-pda-utils`
(new script) and `gemcli` (new `session` subcommand).

## On-glass checklist (COSMIC, in order)

1. Deploy the toplevel (`bin/deploy.sh`) — no reflash; the KMS boot.img
   is unchanged.
2. `gemcli session status` — marker/effective session sane.
3. `gemcli session set cosmic --reboot`.
4. After boot: `systemctl status display-manager`, `journalctl -b -u
   display-manager`, `systemctl --user` / `loginctl` show a COSMIC
   session; `XDG_CURRENT_DESKTOP=COSMIC` in the session env.
5. Confirm the panel renders (eyes-on-glass — **rule 5**: if the glass
   ever flickers, stop and go to TWRP). COSMIC must not need more from
   `geminipda-drm` than mutter already uses (atomic modeset + a shadow
   primary plane; there is no cursor/overlay plane — smithay is expected
   to fall back to a software cursor).
6. `gemcli session set gnome --reboot` — verify the return trip.
7. `gemcli session set console --reboot` — verify no GDM, fbcon tty1
   autologin; then back to `gnome`.
8. Log the version lines + outcomes in `docs/session-log.md` (rule 0).

### Known risks / open questions

- **COSMIC + `geminipda-drm` capability matrix.** mutter is verified on
  this driver; `cosmic-comp`/smithay may request a cursor plane, more
  formats or modifiers. If it fails to bring up the output, capture the
  journal and consider an smithay software-cursor/rgba8888 config before
  touching the driver (the driver is shared with the verified GNOME
  path — do not regress it).
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
| `config/gemini.nix` | `services.desktopManager.cosmic.enable = true` + selector wiring |
| `services/gnome.nix` | leaves `displayManager.defaultSession` unset (selector owns it) |
| `pkgs/gemcli/src/session.rs` | `gemcli session` |

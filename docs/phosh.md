# Phosh on the Gemini PDA (nested inside gemwl) — bring-up design + receipts

Last updated: 2026-09-10. Status: ✅ **on glass as the DEFAULT desktop**
since gen61 (2026-09-10) — two bring-up bugs found + fixed on the way
(gen59 crashed; receipts below); user unlocked the phosh lockscreen on
glass. Cold-boot re-verify pending (user was on the device — not
rebooted under them).

## TL;DR — the answer to "will gemwl support phosh with GPU accel?"

**No — gemwl cannot host the phosh shell, and it doesn't need to.** gemwl
(`pkgs/gemwl.c`) is a minimal **xdg-shell KIOSK** compositor: it creates
compositor/subcompositor/viewporter/data-device/output-layout/xdg-shell/
seat globals and nothing else — no `wlr-layer-shell`, no foreign-toplevel,
no `ext-session-lock`, no text-input, no input-method. Phosh is a GTK4
phone shell whose entire UI (top bar, background, lockscreen, app grid)
is built on those protocols, and it additionally expects **phoc's private
protocol** (phoc gates input until the shell attaches — `phoc -S`, src/
server.c `allow_input`/shell-state — and upstream's whole session shape is
`phoc -E gnome-session`). Running phosh on gemwl would require growing
gemwl into a full wlroots compositor — a big rewrite of a verified,
on-glass compositor.

**The pattern that works is the one LXQt already proves**: a full wlroots
compositor *nested inside gemwl* (gemwl's output is the LK framebuffer,
GPU-direct; labwc has filled the nested slot since 2026-09-07). Phosh's
slot is filled by **phoc** — its native compositor — nested the same way:

```
phosh GTK4 apps (gnome-console, firefox/chrome, …)
  -> phoc 0.54.0   wlroots 0.19 "wayland" backend, nested on gemwl's
                   wayland-0, own socket "phosh"; gles2 renderer + gbm
                   allocator on the fork mesa (renderD128)
    -> gemwl        wlroots 0.18.2, owns /dev/gemfb, GPU blit to LK fb
      -> LK framebuffer -> panel
```

**GPU acceleration is preserved end-to-end**: GTK4 renders with EGL
(via the /etc/glvnd ICD → mesa-geminipda fork → panfrost), phoc
composites with wlroots 0.19's GLES2 renderer on the same fork (gbm
dma-bufs on `/dev/dri/renderD128`), and gemwl imports those dma-bufs and
blits GPU-direct to the framebuffer — the identical chain labwc + the
LXQt session (and gemdemo's 60 fps) run today. There is no software
rendering hop anywhere.

## Pins (rule 0 — every artifact identified)

| Piece | Version | Source | Why |
|---|---|---|---|
| phosh (shell) | 0.54.0 | pinned nixpkgs (`pkgs.phosh`) | matches phoc 0.54 in the same pin; pure Wayland/GTK client (no wlroots dep) |
| phoc (compositor) | 0.54.0 | pinned nixpkgs (`pkgs.phoc`), **overridden** in `pkgs/phoc-geminipda.nix` | nixpkgs' phoc is the tested phosh pairing |
| wlroots (phoc's) | 0.19.3 | pinned nixpkgs (`pkgs.wlroots_0_19`), **overrideAttrs** in `pkgs/phoc-geminipda.nix` | phoc 0.54 does NOT build against wlroots 0.18 (the repo's `wlroots-geminipda` pin stays gemwl's, untouched) |
| libgbm/EGL for phoc | mesa **fork** 25.0.7 | `pkgs/mesa-geminipda.nix` | single-mesa GPU closure — see the gbm swap note below |
| OSK | squeekboard 1.43.1 | pinned nixpkgs | phosh's osk-manager toggles the `sm.puri.OSK0` D-Bus name (VIRTBOARD, src/osk-manager.c); squeekboard owns it |

### The libgbm swap (why `pkgs/phoc-geminipda.nix` exists)

nixpkgs' wlroots expression gets its gbm from `mesa-libgbm` (a separate
lean mesa build — **26.1.3 at this pin**, a different mesa than the fork
the rest of the desktop runs, and with a baked gbm-backends-path pointing
at `/run/opengl-driver/lib/gbm`, which only exists with
`hardware.graphics`). Letting phoc composite with a *different-mesa*
libgbm while its EGL comes from the fork's ICD is exactly the
cross-mesa AFBC/modifier mismatch the fork + `PAN_MESA_DEBUG=noafbc`
exist to avoid. So the override rebuilds phoc's wlroots 0.19.3 with the
fork's libgbm in place of nixpkgs' mesa-libgbm. Receipts:

- The swap is inert-free: `pkgs.phoc.drvPath` differs from stock; the
  forked wlroots' RUNPATH carries the fork's lib dir and its
  `libgbm.so.1` resolves to the fork's `libgbm.so.1.0.0` (25.0.7) —
  readelf-verified 2026-09-09 (`/nix/store/10q9ia58…-wlroots-0.19.3`,
  `/nix/store/g9qs7qfwr…-phoc-0.54.0`).
- First build failure caught the hidden dependency: nixpkgs'
  mesa-libgbm **propagates** libdrm (`gbm.nix`
  `propagatedBuildInputs = [ libdrm ]`), and nixpkgs wlroots relies on
  that for its meson `libdrm` lookup — swapping in the fork (which
  propagates nothing) dropped it ("Run-time dependency libdrm found:
  NO"). Fix: add `pkgs.libdrm` back to the override's buildInputs
  (comment in the derivation).
- Cost: one local aarch64 rebuild (wlroots 0.19.3 + phoc relink, ~40 s
  on the remote builder 2026-09-09); everything else (incl. phosh, a
  GNOME-sized closure) substitutes from cache.nixos.org — the pinned
  rev is the hydra-built channel snapshot (golden rule 9).

## What was implemented (2026-09-09, built + eval'd; 2026-09-09 → default desktop; 2026-09-10 → on glass gen61)

- `services/phosh.nix` — option `services.phoshDesktop.enable`
  (default **true** since 2026-09-09 — Phosh is the DEFAULT desktop;
  was **false** — LXQt the default — from the module's 2026-09-09o
  landing until the flip) + `.scale`
  (default 1.5 — the verified LXQt readability on this panel; phoc
  parses scale with `strtof`, src/settings.c, fractional fine). Enabling
  is assert-guarded to require `services.lxqtNested.enable = false`
  (two nested desktops would stack fullscreen on gemwl — each session
  now owns its own runtime dir/bus, /run/lxqt-session vs
  /run/phosh-session, so they no longer fight over a shared bus, but
  both fullscreen on gemwl is never what you want). Unit `phosh-nested.service`
  (After=gemwl.service, wantedBy multi-user.target):
  - ExecStartPre `prepare-phosh-session`: session bus at
    $XDG_RUNTIME_DIR/bus = /run/phosh-session/bus (idempotent — the
    unit's cjdell-owned RuntimeDirectory; see lxqt's bus model,
    2026-09-09) + wait for gemwl's wayland-0 (cold-boot ordering;
    gemwl may still be in panfrost-load.sh when this unit starts).
  - ExecStart `phoc -v -S -C <generated phoc.ini> --socket phosh -E
    start-phosh-shell` with `WLR_BACKENDS=wayland WLR_RENDERER=gles2`
    (nested backend — never the drm backend, there is no DRM here; and
    deterministic gles2 since no vulkan driver exists). `--socket phosh`
    = fixed socket name (gemwl holds wayland-0) so no socket hunting.
  - `-E start-phosh-shell` execs **`$out/libexec/phosh`** (the shell
    binary; NOT `bin/phosh-session`, which starts its OWN phoc on DRM),
    after starting squeekboard in the background with a respawn guard
    (phosh doesn't spawn the OSK — it only toggles it via D-Bus).
    phoc's exit == phosh's exit == unit lifecycle (Restart=on-failure),
    the labwc/lxqt-nested semantics.
  - phoc.ini: `[core] xwayland = false` + `[output:WL-1] scale = …`
    (WL-1 = the wlroots wayland-backend nested output name — upstream
    phosh data/phoc.ini carries the same `[output:WL-1]` section for
    the nested dev flow). No mode: gemwl sizes phoc's toplevel to the
    full gemfb (gemwl.c `gemwl_toplevel_size_to_output` — the same
    sizing labwc adopts), so only the UI scale is set here.
  - Env = the cjdell desktop-session env lxqt.nix documents (the
    session runs as cjdell — config/gemini.nix users.users.cjdell,
    2026-09-09; HOME=/home/cjdell + XDG_*, the unit's cjdell-owned
    /run/phosh-session runtime dir, PipeWire/audio sockets from
    audio.nix), `XDG_CURRENT_DESKTOP=Phosh:GNOME`,
    `PAN_MESA_DEBUG=noafbc`, `GBM_BACKENDS_PATH` (fork lib/gbm — the
    fork libgbm has no baked path), `NIXOS_OZONE_WL`, SHELL, and
    `PHOSH_SHELL_BIN` (unit env, not hardcoded in the script).
- `services/scripts/{prepare-phosh-session,start-phosh-shell}` (into
  the `gemini-utils` package with the other service scripts).
- `pkgs/phoc-geminipda.nix` (+ flake packages `phoc`, `phosh`,
  `squeekboard` for standalone builds; module and flake callPackage with
  the same args → shared store paths).
- System packages on enable: phosh, phoc, squeekboard, gnome-console
  (a terminal + GL test client for the app grid), adwaita-icon-theme,
  cantarell-fonts; dbus + polkit defaults; dejavu/cantarell fonts.
  The grid also shows the existing system-profile apps (firefox,
  chrome — both strong GL clients for the GPU check).

### v1 wiring receipts (verified against the built store paths 2026-09-09)

- `$out/libexec/phosh` is an UNWRAPPED ELF — wrapGAppsHook4 wraps
  bin/ only, and in the real flow the shell is started by gnome-session
  (which supplies the env). The module therefore adds gnome-shell's
  GSettings schema dir to XDG_DATA_DIRS itself (nixpkgs' phosh wrapper
  would have done exactly `--prefix XDG_DATA_DIRS :
  getSchemaDataDirPath gnome-shell`, phosh package.nix preFixup) plus
  phosh's own share — schema lookup works without the wrapper.
  `PHOSH_SHELL_BIN=${phosh}/libexec/phosh` is unit env, not hardcoded.
- squeekboard 1.43.1 installs `bin/squeekboard` (a wrapGApps wrapper
  over `bin/.squeekboard-wrapped`), so bare-name resolution on the
  unit PATH works.
- phoc's generated phoc.ini carries `[output:WL-1] scale = …` — WL-1 is
  the wlroots wayland-backend nested output name (upstream phosh
  data/phoc.ini carries the identical section for the nested dev flow).

## Desktop default (changed 2026-09-09) / how to switch

Phosh is the DEFAULT desktop since 2026-09-09: `services.phoshDesktop.enable`
defaults **true** and `services.lxqtNested.enable` defaults **false** (the
module defaults flipped; before that LXQt defaulted on — since its
2026-09-07 landing — and phosh was off). To boot the LXQt alternative:

```nix
# config/gemini.nix (or the /root/gemini-nixos clone on the device)
services.phoshDesktop.enable = false;
services.lxqtNested.enable = true;    # phosh's assert needs it off
# services.phoshDesktop.scale = 1.5;  # phosh UI scale; 2 = phone 1080x540
```

Host loop: commit → `bash bin/deploy.sh build|deploy` (profile switch,
no reflash). Device loop: `cd /root/gemini-nixos && nixos-rebuild
switch --flake .`. Console-only boot: `systemctl disable gemwl
phosh-nested`.

## On-glass receipts (2026-09-10 — gen59 crash → gen61 works)

Gen59 (the default-desktop flip) crashed ~6 s after phoc's "Enabling
shell mode" on EVERY run: silent SIGABRT in phosh, then SIGBUS in phoc.
Backtraced via a throwaway btpreload LD_PRELOAD (signal handler +
backtrace_symbols_fd) shipped to the device — no tooling added to the
image:

- **Bug 1 (fatal, gen59): no GSettings schemas on the unit env.**
  g_settings_set_property (glib gio gsettings.c:676) g_error'd "No
  GSettings schemas are installed on the system" → abort. Cause: NixOS
  gsettings packages install schemas under
  `share/gsettings-schemas/<pkgname>/glib-2.0/schemas` (the nixpkgs
  gsettings-schemas hook layout), NOT upstream `share/glib-2.0/`
  schemas — and libexec/phosh is unwrapped (started by phoc -E), so
  wrapGAppsHook4 never rewrote XDG_DATA_DIRS for it. The phosh-nested
  env only listed plain /share dirs → gio's default schema source was
  empty → first g_settings_new aborted. Fix (408071b): carry the
  gsettings-schemas dirs of phosh/gnome-shell/squeekboard/
  gnome-console/gsettings-desktop-schemas/gnome-settings-daemon on
  XDG_DATA_DIRS. Runtime-verified first (drop-in env override): "Phosh
  ready after 3.39s".
- **Bug 2 (lockscreen, gen60): no 'phosh' PAM service.** cjdell had NO
  password (locked account) so the passcode pad could never succeed —
  set passcode 0000 (user's choice; chpasswd live + hashedPassword in
  users.users.cjdell, 408071b). Then 0000 STILL failed on glass:
  phosh PAM-authenticates in-process under service name "phosh" but
  NixOS generates no such /etc/pam.d/phosh → pam falls back to the
  deny-all "other" file (pam_warn spam in the journal) → every code
  rejected. Fix (7f33bee): security.pam.services.phosh = { } (default
  unix rules; pam_unix as euid 1000 verifies via the setuid
  /run/wrappers/bin/unix_chkpwd NixOS already provides). User unlocked
  with 0000 on gen61.

GPU truth on glass (phoc journal): WLR_RENDERER=gles2 → "OpenGL ES 3.1
Mesa 25.0.7, Mali-T880 (Panfrost)", EGL display extensions incl.
EGL_EXT_image_dma_buf_import — same fork-mesa chain as LXQt/gemdemo.
Remaining journal noise = the documented v1 gaps (no gnome-session /
dconf; upower + NetworkManager are now provided — see the corrected
Known-gaps bullet below and docs/desktop-plumbing.md [corrected
2026-09-10]).

## On-glass verification checklist (owed — the 🟡 → ✅ step)

1. **Build + switch** (above). Expect the only local compiles to be
   wlroots 0.19.3 + phoc (already built host-side; cache/`nix copy`
   delivers them).
2. `systemctl status phosh-nested gemwl` — both active, low NRestarts
   (the known startup orderings: prepare-phosh-session waits for gemwl's
   socket; gemwl's panfrost-load retry can delay it).
3. **GPU truth**: `journalctl -u phosh-nested` should show phoc's
   wlroots choosing the gles2 renderer and gbm allocator on
   renderD128 (no pixman fallback), and phosh attaching (phoc -S input
   unlock message). A `tinytest`/`wlegltst` on gemwl's socket still
   proves the bottom layer.
4. **Eyes on glass**: top bar renders, clock/status appear; touch /
   keyboard (gemini layout comes from gemwl's keymap through the nested
   chain) drive the shell; swipe up opens the app grid; launch
   gnome-console and type; launch firefox/chrome → WebRender on the
   fork (the LXQt browser receipts apply — they were the GPU-probe
   clients there).
5. If the top bar/background don't map, suspect the layer-shell output
   size path (phoc's output vs gemwl's sizing) — check
   `journalctl -u phosh-nested` for phoc's output geometry and compare
   with the WL-1 size labwc reports. Scale feel = `phoshDesktop.scale`.
6. Record the version line (rule 0) + outcome in `docs/session-log.md`.
   Phosh is the default desktop since 2026-09-09; if the glass is wrong:
   `bash bin/deploy.sh rollback` (gen58 = the LXQt desktop) or flip the
   options back (above). [done 2026-09-10: gen61 on glass, unlocked; a
   cold-boot re-verify is the remaining step — do NOT reboot while the
   user is on the device.]

## Known gaps / deliberate v1 scope (next steps, in rough order)

- **No gnome-session scaffolding.** Upstream phosh runs inside
  `gnome-session --session=phosh` (phosh's mobi.phosh.* units are
  gnome-session-managed), which supplies autostart, portals, a11y,
  settings-daemon and session lifecycle. v1 execs the shell directly
  (the classic nested-dev flow) — the shell itself renders and takes
  input, but expect warnings/criticals about missing session pieces and
  reduced features (no idle/lock integration, no gsd-powered
  brightness/volume OSD).
- **No dconf / GSettings persistence** (no dconf-service on the session
  bus) — phosh falls back to defaults; `GSETTINGS_BACKEND=memory` is
  deliberately NOT set so real stores can be added later without a
  config flip.
- **No xdg-desktop-portal-phosh** (file/share dialogs fall back to the
  GTK portal if ever added), **no stevia/ibus input method** (hardware
  keyboard first; OSK text-entry into Qt apps would need the ibus route
  the nixpkgs module uses). **[corrected 2026-09-10]** The former "no
  upower / no NetworkManager" gap is CLOSED by the desktop-plumbing
  layer (`services/plumbing.nix` + `services/wifi.nix`,
  docs/desktop-plumbing.md): a kernel Battery supply makes upower show
  real battery state (the unit has no fuel gauge — capacity is
  voltage-derived) and NetworkManager now owns wlan0/wlan1 on the
  standard D-Bus API, so the phosh status-bar/quick-settings battery,
  wifi and bluetooth (bluez, already up) widgets have their services.
  ModemManager stays off (no modem); the cellular icon remains
  absent/unknown by design.
- **wlroots 0.19 vs the verified 0.18.2**: phoc's compositing runs on
  nixpkgs' wlroots 0.19.3 (userspace-only here — no DRM touched), whose
  GLES2/gbm path is a sibling of the verified 0.18.2 path but has not
  itself been on glass. The gbm/EGL userspace is the fork either way;
  this is the main "unverified" the checklist above closes.

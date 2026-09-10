# Vanilla GNOME on the Gemini PDA — the standards path.
#
# This is the desktop that becomes possible once the LK framebuffer is a
# real DRM/KMS device (devices/planet-geminipda/kernel/delta/drivers/gpu/
# drm/tiny/geminipda-drm.c -> /dev/dri/card0).  With a normal KMS device
# there is no reason to hand-roll a session: this module uses the
# ORDINARY NixOS GNOME modules (services.xserver.desktopManager.gnome +
# GDM), exactly as on a laptop.  That is the whole point of the KMS work —
# no bespoke compositor, no nesting, and future GNOME releases keep
# working.
#
# Relationship to the nested desktops (services/{desktop,phosh,lxqt}.nix):
#   gemwl + phosh/lxqt own the raw LK framebuffer through /dev/gemfb and
#   present a nested Wayland session.  GNOME needs the whole screen and a
#   logind seat, so it REPLACES that stack: this module force-disables
#   gemwl and the nested sessions.  The two modes are mutually exclusive
#   by construction; enable exactly one.
#
# Why GNOME is off by default: it needs /dev/dri/card0, i.e. the
# geminipda-drm module in a NEW boot.img.  Booting this config on a kernel
# without that driver gives no session at all.  So the switch is a
# deliberate two-step (flash the KMS boot.img, confirm card0, then set
# services.gnomeDesktop.enable = true) — see docs/gnome-feasibility.md.
#
# X11/Xwayland: the session is Wayland-native.  GNOME 50's mutter has no
# X11 backend at all (the X11 and nested backends were removed), so no
# Xorg is used; Xwayland is built into nixpkgs' mutter but is only
# started on demand for X clients, of which this image has none.  (A
# mutter built with -Dxwayland=false would remove it outright; not done
# here because the pinned nixpkgs' mutter is shared with the pinned GNOME
# packages.)
#
# GPU: panfrost is blacklisted at boot (services/gemini-pda.nix) because
# an early probe on the un-powered Mali soft-resets.  In the nested stack
# gemwl.service loads it in ExecStartPre; with gemwl disabled this module
# owns that step, ordered after gemini-gpu-poweron and before GDM.
{ config, lib, pkgs, ... }:

let
  cfg = config.services.gnomeDesktop;
  utils = pkgs.callPackage ./gemini-utils.nix { };
  # Full xkeyboard-config tree with the gemini layout registered in
  # rules/evdev.xml — required for GNOME to *find* the layout (see the
  # package header); the plain gemini-xkb include dir alone only makes it
  # compilable, which is not enough for GNOME Shell.
  geminiXkeyboardConfig = pkgs.callPackage ../pkgs/gemini-xkeyboard-config.nix { };
  # Never auto-show the on-screen keyboard (mutter's touch_mode heuristic;
  # see the package header + docs/desktop-plumbing.md §"On-screen
  # keyboard").  Enabled + locked below via the system dconf DB.
  noOskExtension = pkgs.callPackage ../pkgs/gnome-extension-no-osk { };
in
{
  options.services.gnomeDesktop = {
    enable = lib.mkOption {
      type = lib.types.bool;
      # OFF until the geminipda-drm KMS boot.img is on the device — see
      # the header. Flipping it on is the actual "GNOME as the desktop"
      # switch.
      default = false;
      description = ''
        Run the standard NixOS GNOME desktop (services.xserver.
        desktopManager.gnome + GDM, Wayland, autologin) on the
        geminipda-drm KMS device. Requires a boot.img whose kernel
        provides /dev/dri/card0 (the geminipda-drm delta driver); a
        kernel without it yields no session. Mutually exclusive with the
        nested gemwl/phosh/LXQt stack, which it force-disables.
      '';
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "cjdell";
      description = ''
        User to auto-login into the GNOME session (the device's desktop
        user, config/gemini.nix users.users.cjdell).
      '';
    };

    softwareRendering = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Force Mesa's software rasteriser (llvmpipe) for the whole session.
        DEBUG FALLBACK ONLY — normally leave this off.

        The Gemini PDA displays through two DRM devices: panfrost
        (/dev/dri/card0 + renderD128; the Mali GPU, no display) and
        geminipda-drm (/dev/dri/cardN; the LK framebuffer as KMS, no GPU).
        Mesa's kmsro layer is compiled into pkgs/mesa-geminipda.nix (it is
        auto-enabled because panfrost is a renderonly driver) and pairs the
        display-only KMS card with panfrost for rendering: Mesa's
        pipe-loader falls back to the kmsro driver for the unknown
        "geminipda-drm" name, and kmsro_drm_screen_create() calls
        pipe_loader_get_compatible_render_capable_device_fds(), which pairs
        any PLATFORM display-only device with a platform render driver
        (panfrost).  The result is a hardware-accelerated EGL screen whose
        GL_RENDERER is "Mali-T880 (Panfrost)", so mutter
        (meta-render-device.c: anything not llvmpipe/softpipe/swrast is
        "hardware accelerated") selects geminipda-drm as its primary GPU
        (it is the GPU with the built-in DSI panel) and renders through
        panfrost.  Setting this option forces llvmpipe instead and throws
        that away; it exists only to get a session up if kmsro ever fails
        to pair on glass.  Receipts + the on-glass check: docs/
        gnome-feasibility.md.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    # ---- The standard session ------------------------------------
    # Exactly the modules a normal NixOS GNOME machine enables.
    services.xserver.enable = true;
    services.desktopManager.gnome.enable = true;
    services.displayManager.gdm.enable = true;
    # defaultSession is deliberately left UNSET (null): GDM persists the
    # pre-selected session in AccountsService (set-session), and the
    # desktop/session selector (services/desktop-select.nix) sets that
    # at boot from /var/lib/gemini/desktop so `gemcli session set` is
    # authoritative. Setting it here would emit a GDM preStart call
    # that OVERWRITES the marker on every display-manager start.
    services.displayManager.autoLogin.enable = true;
    services.displayManager.autoLogin.user = cfg.user;

    # ---- Replace the nested stack --------------------------------
    # GNOME owns the panel; gemwl owns /dev/gemfb and the nested sessions
    # own gemwl's socket. Force them off so the two models can never
    # fight over the scanout.
    systemd.services.gemwl.enable = lib.mkForce false;
    services.phoshDesktop.enable = lib.mkForce false;
    services.lxqtNested.enable = lib.mkForce false;

    # ---- Keep this repo's custom PipeWire ------------------------
    # services/audio.nix runs PipeWire/WirePlumber/pipewire-pulse as a
    # root system session tuned for the MT6351 S16 path (docs/audio).
    # nixpkgs' GNOME module (via gnome-remote-desktop) sets
    # services.pipewire.enable = true, which would start a SECOND
    # pipewire.service and collide with the custom unit of the same
    # name. Keep the custom one: its `pipewire.service` still satisfies
    # GNOME's Requires=, and GNOME audio works through the existing
    # session. [2026-09-10]
    services.pipewire.enable = lib.mkForce false;

    # GPU bring-up (gemwl used to do this) --------------------
    # panfrost must be loaded before the session so that Mesa's kmsro can
    # pair it with the geminipda-drm KMS card (see softwareRendering).
    systemd.services.gemini-panfrost-load = {
      description = "Load panfrost for the GNOME/KMS session";
      after = [ "gemini-gpu-poweron.service" "systemd-udevd.service" ];
      wants = [ "gemini-gpu-poweron.service" ];
      before = [ "display-manager.service" ];
      wantedBy = [ "display-manager.service" "multi-user.target" ];
      path = [ pkgs.kmod pkgs.coreutils ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        # Same retry logic gemwl used: keeps re-probing until
        # /dev/dri/renderD128 appears (docs/desktop-plumbing.md).
        ExecStart = "${utils}/bin/panfrost-load.sh";
      };
    };

    # ---- Keyboard: the gemini xkb layout -------------------------
    # XKB_CONFIG_ROOT points at a full xkeyboard-config tree (a copy) with
    # the gemini layout added to symbols/ AND registered in
    # rules/evdev.xml, so BOTH xkbcommon (mutter compiling the keymap) and
    # libxkbregistry (libgnome-desktop's XkbInfo, which GNOME Shell uses to
    # decide whether a source id is a real layout) see it.  The plain
    # XKB_CONFIG_EXTRA_PATH dir was not enough: XkbInfo could not find
    # "gemini", so gnome-shell quietly fell back to 'us' and the keymap
    # stayed US.  XKB_DEFAULT_LAYOUT only helps clients that do not set a
    # layout themselves. [fixed 2026-09-10m; the 2026-09-10l attempt only
    # set XKB_CONFIG_EXTRA_PATH + the model and was not sufficient]
    # ---- On-screen keyboard: never show it ------------------------------
    # The Gemini has a real keyboard, so the OSK must never appear.  The
    # accessibility toggle is not enough: mutter reports touch_mode=true
    # (touchscreen + no pointer, no tablet switch), and gnome-shell then
    # auto-creates the OSK on every text focus regardless.  This installs
    # the small extension that kills that auto path, and the dconf DB below
    # enables it + locks the a11y toggle off.  [added 2026-09-10]
    environment.systemPackages = [ noOskExtension ];

    environment.sessionVariables = {
      XKB_CONFIG_ROOT = "${geminiXkeyboardConfig}/etc/X11/xkb";
      XKB_DEFAULT_LAYOUT = "gemini";
      XKB_DEFAULT_MODEL = "pc105";

      # ---- Audio socket redirection (2026-09-10p) ------------------
      # services/audio.nix runs the ONE PipeWire/WirePlumber/pipewire-pulse
      # session for the whole device as a system service with
      # XDG_RUNTIME_DIR=/run/gemwl-audio (the design from the gemwl/phosh/
      # LXQt era, where the desktop was a system service with no logind
      # session).  GNOME, however, is a REAL logind session via GDM:
      # cjdell's XDG_RUNTIME_DIR is /run/user/1000, so every GNOME audio
      # client (gnome-control-center's Sound panel, gsd-media-keys' Gvc
      # mixer for the volume keys, gnome-shell's OSD) looked in
      # /run/user/1000/pulse/native and found nothing — the panel showed
      # no devices and the volume keys never touched the sink.
      #
      # Rather than run a second PipeWire for the session (and re-solve the
      # MT6351 S16 path twice), point the session's clients at the existing
      # system session.  PULSE_SERVER covers libpulse (gnome-control-center
      # / gsd / gnome-shell); PIPEWIRE_RUNTIME_DIR covers native PipeWire
      # clients (wpctl, pavucontrol, …).  The session runs as cjdell, which
      # owns /run/gemwl-audio — no privilege involved.
      PULSE_SERVER = "unix:/run/gemwl-audio/pulse/native";
      PIPEWIRE_RUNTIME_DIR = "/run/gemwl-audio";
    } // lib.optionalAttrs cfg.softwareRendering {
      LIBGL_ALWAYS_SOFTWARE = "1";
    };

    # ---- Keyboard: force the gemini layout in the GNOME session ----
    # The env vars above do NOT select the layout for mutter: mutter
    # builds its keymap from the gsettings `input-sources` list
    # [('xkb', <layout>)] and ignores XKB_DEFAULT_LAYOUT.  That gsettings
    # key defaults to [('xkb','us')], and a stale per-user value (the
    # device had [('xkb','us')]) also outranks a plain system default.
    # Install it in a system dconf database AND lock it: the dconf module
    # documents that a locked key takes its value from the database that
    # holds the lock, so the system value wins over user-db.  This is
    # what makes the UK silkscreen (shift+3 = £, Fn+K = @, …) and the Fn
    # layer (Fn+1..0 = F1..F10, media keys) work. [added 2026-09-10l]
    # NOTE: this only takes effect together with geminiXkeyboardConfig —
    # gnome-shell rejects a source id that is not in the xkb registry.
    programs.dconf.profiles.user.databases = [
      {
        settings = {
          "org/gnome/desktop/input-sources" = {
            sources = lib.gvariant.mkArray [
              (lib.gvariant.mkTuple [ "xkb" "gemini" ])
            ];
          };
          # On-screen keyboard: the a11y toggle is off and locked.  The
          # other half (mutter's touch_mode auto path) is killed by
          # noOskExtension; see the package header for the receipts.
          "org/gnome/desktop/a11y/applications" = {
            screen-keyboard-enabled = false;
          };
          # Enable the no-osk extension declaratively.  NixOS has no
          # first-class option for this (the nixpkgs GNOME doc: a GSettings
          # override "will only influence the default value"), so put it in
          # the system dconf DB and LOCK it — the lock makes the system
          # value win over any stale per-user enabled-extensions list.  The
          # package installs to share/gnome-shell/extensions, which
          # gnome-shell scans on the system XDG_DATA_DIRS path.
          "org/gnome/shell" = {
            enabled-extensions = [ "no-osk@gemini-nixos" ];
          };
        };
        locks = [
          "/org/gnome/desktop/input-sources/sources"
          "/org/gnome/desktop/a11y/applications/screen-keyboard-enabled"
          "/org/gnome/shell/enabled-extensions"
        ];
      }
    ];

    # ---- Keyboard: make the Fn media keys reach GNOME (2026-09-10p) --
    # The Fn layer is XKB level 3, selected by ISO_Level3_Shift on RALT
    # ("gemini" symbols; xkbcli confirms RALT -> ISO_Level3_Shift ->
    # Mod5, and Fn+C/V/B/N -> XF86AudioLower/RaiseVolume and
    # XF86MonBrightnessDown/Up at level 3).  mutter matches global
    # keybindings on (keycode, modifier-mask) and only masks out
    # scroll-lock/Mod2/Lock (src/core/keybindings.c
    # mask_from_event_params()); Mod5 is NOT masked.
    #
    # Why the bindings are raw KEYCODES, not the XF86 keysym names:
    # mutter's accelerator parser supports <Mod5> (meta-accel-parse.c
    # -> CLUTTER_MOD5_MASK), but when an accelerator is a *keysym* mutter
    # resolves it to the LOWEST xkb level that produces that keysym
    # (add_keysym_keycodes_from_layout() stops at the first level with a
    # match).  The compiled gemini keymap has XF86AudioLowerVolume/etc at
    # level 0 on the standard evdev consumer keycodes (<VOL->=0x7a,
    # <MUTE>=0x79, <I232/233>), so `<Mod5>XF86AudioLowerVolume` binds to
    # (0x7a, Mod5) and never matches the Fn event (keycode 0x36 = C,
    # Mod5).  Verified on glass 2026-09-10p: `<Mod5>0x39` (N) raised the
    # backlight, the keysym form did nothing.  So bind the Fn layer's
    # xkb keycodes directly — Fn+C=0x36, Fn+V=0x37, Fn+B=0x38, Fn+N=0x39,
    # Fn+T=0x1c (xkb keycode = evdev code + 8; see config/xkb/symbols/
    # gemini for the level-3 keysym assignments).  A device with a fixed
    # built-in keyboard only: the keycodes *are* the layout contract.
    #
    # Brightness lives in gnome-shell's keybinding schema (GNOME 50 moved
    # the screen backlight into mutter/gnome-shell:
    # js/misc/brightnessManager.js); volume lives in gsd-media-keys'
    # "-static" arrays (gsd-media-keys-manager.c get_bindings() merges the
    # empty dynamic array with the static one).  Setting these as schema
    # DEFAULTS (not a locked dconf value) leaves them user-remappable.
    services.desktopManager.gnome.extraGSettingsOverrides = ''
      [org.gnome.shell.keybindings]
      screen-brightness-up=['XF86MonBrightnessUp', '<Mod5>0x39']
      screen-brightness-down=['XF86MonBrightnessDown', '<Mod5>0x38']

      [org.gnome.settings-daemon.plugins.media-keys]
      volume-up-static=['XF86AudioRaiseVolume', '<Ctrl>XF86AudioRaiseVolume', '<Mod5>0x37']
      volume-down-static=['XF86AudioLowerVolume', '<Ctrl>XF86AudioLowerVolume', '<Mod5>0x36']
      volume-mute-static=['XF86AudioMute', '<Mod5>0x1c']
    '';
    # gsd's media-keys schema is not in the default gnome override set
    # (which only carries gsettings-desktop-schemas + gnome-shell), so it
    # must be listed explicitly for the [org.gnome.settings-daemon...]
    # block above to be compiled.
    services.desktopManager.gnome.extraGSettingsOverridePackages = [
      pkgs.gnome-settings-daemon
    ];
  };
}

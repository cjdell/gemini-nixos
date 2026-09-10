# Phosh (mobile/phone shell) nested inside gemwl — the DEFAULT desktop
# since 2026-09-09 (LXQt, services/lxqt.nix, is the alternative).
#
# Architecture (full design + receipts: docs/phosh.md):
#
#   Phosh GTK4 apps (gnome-console, firefox/chrome, ...)
#     -> phoc 0.54.0 (wlroots 0.19 "wayland" backend, nested on gemwl's
#                     wayland-0, listens on socket "phosh"; the gles2
#                     renderer + gbm allocator on the fork mesa — the
#                     layer-shell/foreign-toplevel/session-lock/text-
#                     input/phoc-private protocols phosh needs)
#       -> gemwl (services/desktop.nix; the wlroots-0.18 compositor that
#                 owns the LK framebuffer /dev/gemfb, GPU-direct)
#         -> LK framebuffer -> panel
#
# WHY NESTED PHOC, NOT PHOSH ON GEMWL (the "will gemwl support it?"
# question): gemwl is a minimal xdg-shell KIOSK compositor — it
# implements compositor/xdg-shell/seat/... only (pkgs/gemwl.c creates
# no wlr-layer-shell, no foreign-toplevel, no ext-session-lock, no
# text-input, no wlr-output-management), and Phosh additionally expects
# phoc's private protocol (phoc -S gates input until the shell
# attaches; phosh's osk-manager drives the OSK over the sm.puri.OSK0
# D-Bus name). Phosh simply does not run on gemwl's protocol surface.
# The pattern that DOES work (proven with LXQt since 2026-09-07) is a
# full wlroots compositor NESTED inside gemwl — phoc replaces labwc in
# that slot. GPU acceleration is preserved end-to-end: GTK4 renders via
# EGL (mesa-geminipda ICD via hardware.graphics /run/opengl-driver)
# into phoc; phoc composites with wlroots
# 0.19 gles2 on the same fork (gbm dma-bufs on renderD128); gemwl
# imports those dma-bufs and blits GPU-direct to the LK framebuffer.
#
# Pinning notes:
#   - phoc 0.54.0 comes from the pinned nixpkgs (its own wlroots dep is
#     wlroots_0_19; phoc 0.54 does NOT build against wlroots 0.18 — the
#     repo's wlroots-geminipda pin stays gemwl's, untouched).
#   - pkgs/phoc-geminipda.nix rebuilds that wlroots 0.19 so its libgbm
#     is the FORK mesa (single-mesa closure, like the rest of the
#     desktop — cross-mesa EGL/gbm mixing is exactly the AFBC/modifier
#     landmine the fork avoids). The rebuild is the only non-cached drv
#     this module adds.
#   - phosh 0.54.0 + squeekboard 1.43.1 come from nixpkgs as-is (pure
#     Wayland/GTK clients; no wlroots dependency).
#
# [2026-09-09] The session runs as cjdell (the device's default desktop
# user, config/gemini.nix users.users.cjdell) like the LXQt session —
# HOME=/home/cjdell, its own cjdell-owned runtime dir /run/phosh-session
# (bus + phoc's "phosh" socket); gemwl stays root (services/desktop.nix
# explains the 0755 dir + 0666 socket that lets cjdell connect).
#
# HOW the session assembles (receipts from the phosh 0.54 sources,
# 2026-09-09): upstream phosh-session runs
#   phoc -v -S -C phoc.ini -E "gnome-session --session=phosh"
# on DRM with a gnome-session-managed shell (mobi.phosh.Shell.service).
# This unit runs the same phoc, nested (WLR_BACKENDS=wayland), with a
# phoc.ini that only sets the nested output scale, and -E the repo's
# start-phosh-shell which execs the phosh SHELL binary directly
# ($out/libexec/phosh) — the gnome-session scaffolding (autostart,
# portals, gnome-settings-daemon, dconf, input method) is deliberately
# NOT wired yet: this is the bring-up step (does the shell render +
# accept input on this stack?). Env below carries what a root system
# service must (no HOME/XDG_*/DBUS by default — same list lxqt.nix
# documents), plus XDG_CURRENT_DESKTOP=Phosh:GNOME so OnlyShowIn=Phosh
# entries and phosh's settings schemas resolve.
#
# [changed 2026-09-09] Phosh IS the default desktop: this module now
# defaults ON (services.phoshDesktop.enable = true) and lxqt.nix
# defaults OFF. To boot the LXQt alternative instead:
#   services.phoshDesktop.enable = false;
#   services.lxqtNested.enable = true;
# (the assert below enforces exactly one nested desktop — two would
# stack on gemwl; each session owns its own runtime dir/bus now —
# /run/lxqt-session vs /run/phosh-session, 2026-09-09 — so the
# shared-bus argument is gone, fullscreen stacking is not).
# `systemctl disable gemwl phosh-nested` returns to a console-only boot.
{ config, lib, pkgs, ... }:

let
  cfg = config.services.phoshDesktop;

  # Same callPackage args as the flake package outputs (flake.nix), so
  # the system closure and standalone builds share one store path each.
  mesaGeminipda = pkgs.callPackage ../pkgs/mesa-geminipda.nix { };
  # phoc 0.54.0 + the fork-gbm wlroots 0.19 (pkgs/phoc-geminipda.nix).
  phoc = pkgs.callPackage ../pkgs/phoc-geminipda.nix {
    inherit mesaGeminipda;
  };
  # phosh 0.54.0 (nixpkgs). Its wrapGAppsHook4 wrapper carries the
  # gnome-shell GSettings schema dir etc. on XDG_DATA_DIRS for the
  # phosh process; apps it launches get the session env below.
  phosh = pkgs.phosh;
  utils = pkgs.callPackage ./gemini-utils.nix { };

  # The phoc.ini this unit passes with -C. [output:WL-1] = the nested
  # output (wlroots wayland backend names its outputs WL-N; upstream
  # phosh data/phoc.ini carries the same [output:WL-1] section for the
  # nested dev flow). No mode: gemwl sizes phoc's toplevel to the full
  # gemfb (gemwl.c gemwl_toplevel_size_to_output, the same sizing labwc
  # adopts); only the UI scale is set here. Fractional scale is fine
  # (phoc parses scale with strtof, src/settings.c).
  phocIni = pkgs.writeText "phosh-phoc.ini" ''
    [core]
    xwayland = false

    [output:WL-1]
    scale = ${toString cfg.scale}
  '';

  # GSettings schema dirs. NixOS gsettings packages install schemas at
  # share/gsettings-schemas/<pkgname>/glib-2.0/schemas (the nixpkgs
  # gsettings-schemas hook layout), NOT the upstream
  # share/glib-2.0/schemas. wrapGAppsHook4 rewrites XDG_DATA_DIRS to
  # point at them for WRAPPED binaries — but libexec/phosh is unwrapped
  # (started by phoc -E), so the unit must carry them itself.
  # [2026-09-09 receipt — gen59 crash]: without them gio's default
  # GSettings schema source is empty and the first g_settings_new
  # aborts the whole shell: g_settings_set_property
  # (gsettings.c:676) g_error 'No GSettings schemas are installed on
  # the system' → SIGABRT (backtraced via a btpreload LD_PRELOAD).
  # With them phosh logs 'Phosh ready after 3.39s'. gnome-shell +
  # gsettings-desktop-schemas = the org.gnome.Shell/desktop schemas
  # phosh reads; gnome-settings-daemon = the power plugin schema.
  schemaPkgs = [
    phosh
    pkgs.gnome-shell
    pkgs.squeekboard
    pkgs.gnome-console
    pkgs.gsettings-desktop-schemas
    pkgs.gnome-settings-daemon
  ];
  gsettingsSchemaDirs = map (p: "${p}/share/gsettings-schemas/${p.name}") schemaPkgs;
in
{
  options.services.phoshDesktop = {
    enable = lib.mkOption {
      type = lib.types.bool;
      # [changed 2026-09-09] true = Phosh is the DEFAULT desktop (was
      # false — off, LXQt the default — since the module landed
      # 2026-09-09o until this flip; docs/phosh.md).
      default = true;
      description = ''
        Enable the Phosh (mobile shell) desktop nested inside gemwl
        (phoc 0.54.0 on wlroots 0.19 nested on gemwl's wayland-0, GPU-
        accelerated through the fork mesa — docs/phosh.md). The default
        desktop since 2026-09-09; set false + services.lxqtNested.enable
        = true for the LXQt alternative (they cannot share the gemwl
        session). Disable with gemwl for a console-only boot.
      '';
    };
    scale = lib.mkOption {
      type = lib.types.number;
      default = 1.5;
      description = ''
        UI scale of the nested phoc output (phoc.ini [output:WL-1]
        scale). 1.5 matches the verified LXQt readability on this panel
        (2160x1080 landscape logical; effective 1440x720); 2 gives a
        phone-like 1080x540. Fractional supported (phoc strtof).
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    # Two nested desktops on one gemwl is never what you want: they
    # stack fullscreen on each other (each session now owns its own
    # runtime dir/bus — /run/lxqt-session vs /run/phosh-session — so
    # the old shared-bus argument is gone, 2026-09-09). Phosh is the
    # default desktop since 2026-09-09; booting LXQt is the deliberate
    # flip (phosh off, lxqt on) — this assert catches a config that
    # leaves both on.
    assertions = [
      {
        assertion = !config.services.lxqtNested.enable;
        message = ''
          Phosh + lxqt-nested would both nest fullscreen on gemwl.
          Phosh is the default desktop since 2026-09-09: to boot the
          LXQt alternative set services.phoshDesktop.enable = false and
          services.lxqtNested.enable = true.
        '';
      }
    ];

    services.dbus.enable = lib.mkDefault true;
    security.polkit.enable = lib.mkDefault true;

    # phosh's lockscreen PAM-authenticates the session user in-process
    # under the service name "phosh". NixOS generates no such service by
    # default, so pam falls back to the deny-all "other" file and NO
    # passcode ever unlocks — observed on glass gen60 (0000 rejected
    # with pam_warn spam in the phosh-nested journal). The default unix
    # rules verify against the cjdell shadow entry (users.users.cjdell
    # hashedPassword) via the setuid /run/wrappers/bin/unix_chkpwd
    # (pam_unix execs it when euid != 0 — phosh runs as cjdell).
    security.pam.services.phosh = { };

    # UI fonts + icons (phosh is Adwaita/Cantarell; fontconfig defaults
    # are DejaVu).
    fonts.packages = [ pkgs.dejavu_fonts pkgs.cantarell-fonts ];

    environment.systemPackages = [
      phoc # compositor (also handy for ad-hoc nested tests)
      phosh # shell; bin/phosh-session starts ITS OWN phoc — run
      # libexec/phosh for the nested shell (see start-phosh-shell)
      pkgs.squeekboard # OSK (sm.puri.OSK0; only shows on text focus)
      pkgs.gnome-console # terminal in the app grid (GPU GL test client)
      pkgs.adwaita-icon-theme # the phosh icon theme
      pkgs.dbus # dbus-daemon + dbus-update-activation-environment
      utils # prepare/start-phosh-session + the gemini CLIs
    ];

    systemd.services.phosh-nested = {
      description = "Phosh (mobile shell) session nested on the gemwl compositor (phoc)";
      after = [ "gemwl.service" ];
      wants = [ "gemwl.service" ];
      wantedBy = [ "multi-user.target" ];

      # PATH for phoc + the session it spawns: phoc itself, squeekboard
      # (start-phosh-shell's OSK), dbus tools. nixpkgs appends the
      # default path (coreutils, findutils, gnugrep, gnused, systemd)
      # after this list.
      path = [ phoc pkgs.squeekboard pkgs.dbus ];

      serviceConfig = {
        Type = "simple";
        # The session's user (config/gemini.nix users.users.cjdell — the
        # device's default desktop user since 2026-09-09; see the header
        # comment). systemd chowns RuntimeDirectory below to cjdell.
        User = "cjdell";
        # The session's own runtime dir (cjdell-owned): the session bus
        # and phoc's "phosh" socket live here — NOT /run/gemwl (gemwl's
        # dir, deleted on compositor restart).
        RuntimeDirectory = "phosh-session";
        RuntimeDirectoryMode = "0700";
        ExecStartPre = "${utils}/bin/prepare-phosh-session";
        ExecStart = "${phoc}/bin/phoc -v -S -C ${phocIni} --socket phosh -E ${utils}/bin/start-phosh-shell";
        # phoc exits when its -E session (the phosh shell) exits; a
        # shell crash restarts the whole nested session (lxqt-nested
        # semantics).
        Restart = "on-failure";
        RestartSec = "5";
        # The session env (see the header — systemd system services get
        # no HOME/XDG_*/DBUS by default; User=cjdell sets HOME, the rest
        # is explicit; same set services/lxqt.nix documents for the LXQt
        # session).
        Environment = [
          # Runtime dirs + sockets (see lxqt.nix: /run/phosh-session is
          # THIS unit's cjdell-owned RuntimeDirectory; gemwl's socket is
          # at /run/gemwl/wayland-0; audio.nix owns /run/gemwl-audio —
          # never put the sound server under a compositor/session
          # runtime dir: it is DELETED when that unit restarts
          # [2026-09-07 receipt]).
          "XDG_RUNTIME_DIR=/run/phosh-session"
          "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/phosh-session/bus"
          # phoc nests on gemwl (WLR_BACKENDS=wayland); the bare name
          # resolves against XDG_RUNTIME_DIR, where prepare-phosh-
          # session symlinked wayland-0 -> /run/gemwl/wayland-0
          # (2026-09-09).
          "WAYLAND_DISPLAY=wayland-0"
          "PULSE_SERVER=unix:/run/gemwl-audio/pulse/native"
          "PIPEWIRE_RUNTIME_DIR=/run/gemwl-audio"
          # User env (HOME=/home/cjdell + XDG_* — the cjdell desktop
          # session, not root).
          "HOME=/home/cjdell"
          "XDG_CONFIG_HOME=/home/cjdell/.config"
          "XDG_CACHE_HOME=/home/cjdell/.cache"
          "XDG_DATA_HOME=/home/cjdell/.local/share"
          # XDG data dirs: phosh (schemas + app entries), gnome-shell
          # (its GSettings schemas — nixpkgs' phosh wrapper would add
          # getSchemaDataDirPath gnome-shell, but libexec/phosh is NOT
          # wrapped by wrapGAppsHook4 (bin/ only): the shell is started
          # by gnome-session in the real flow, so the module must carry
          # the dir here), the icon/font themes, and the system profile
          # (systemPackages' applications — chrome/firefox/.desktop
          # files in the grid). The gsettingsSchemaDirs prefix is the
          # 2026-09-09 fix: without those XDG entries gio sees NO
          # schemas at all and g_settings_new aborts phosh (receipt
          # above).
          "XDG_DATA_DIRS=${lib.concatStringsSep ":" gsettingsSchemaDirs}:${lib.makeSearchPath "share" [ phosh pkgs.gnome-shell pkgs.adwaita-icon-theme pkgs.squeekboard pkgs.gnome-console pkgs.dejavu_fonts pkgs.cantarell-fonts ]}:/run/current-system/sw/share"
          # Session identity (OnlyShowIn=Phosh desktop entries; phosh's
          # GSettings schemas; xdg-desktop-portal config).
          "XDG_CURRENT_DESKTOP=Phosh:GNOME"
          "XDG_SESSION_DESKTOP=phosh"
          "XDG_SESSION_TYPE=wayland"
          # AFBC readback workaround (must stay; same as gemwl/lxqt).
          "PAN_MESA_DEBUG=noafbc"
          # Browsers (same as lxqt.nix): the fork libgbm has no baked
          # backend path — GBM_BACKENDS_PATH is how glxtest / the gbm
          # allocator find dri_gbm.so; NIXOS_OZONE_WL flips chrome to
          # Wayland.
          "GBM_BACKENDS_PATH=${mesaGeminipda}/lib/gbm"
          "NIXOS_OZONE_WL=1"
          # Deterministic phoc rendering: gles2 on the fork EGL (no
          # vulkan driver exists on this device; don't let autocreate
          # try), and the nested wayland backend (never drm — there is
          # no DRM/KMS here; the LCD is gemwl's LK framebuffer).
          "WLR_BACKENDS=wayland"
          "WLR_RENDERER=gles2"
          # The phoc -E launcher execs the phosh shell from this path
          # (libexec/phosh — the wrapped shell binary, NOT bin/phosh-
          # session which starts its own DRM phoc).
          "PHOSH_SHELL_BIN=${phosh}/libexec/phosh"
          # Terminal apps in the session spawn $SHELL (qterminal-less
          # but gnome-console uses it for its shell tab); a systemd
          # system service has no SHELL env (see lxqt.nix).
          "SHELL=${pkgs.bashInteractive}/bin/bash"
          # a11y bus is not part of this minimal session — silence
          # GTK's at-spi attempts (phosh brings its own later).
          "NO_AT_BRIDGE=1"
        ];
      };
    };
  };
}

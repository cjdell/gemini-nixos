# Gemini desktop/session selector — GNOME, COSMIC, niri, or the
# framebuffer console.
#
# GNOME, COSMIC and niri are all ordinary GDM Wayland sessions on the
# geminipda-drm KMS device (drivers/gpu/drm/tiny/geminipda-drm.c ->
# /dev/dri/card0, rendered through panfrost via Mesa kmsro). They can be
# INSTALLED side by side — `services.desktopManager.gnome`,
# `services.desktopManager.cosmic` and `programs.niri` each register
# their session with the display manager
# (`services.displayManager.sessionPackages`), so GDM offers them all in
# its chooser. Only ONE can own the panel at a time, but that is what the
# marker below decides per boot, not the build.
#
# This module makes the choice runtime-mutable, which the plain NixOS
# options are not:
#
#   /var/lib/gemini/desktop   one of gnome|cosmic|niri|console (persistent)
#         |
#         v
#   gemini-desktop-apply.service   (Before=display-manager.service)
#         | gnome|cosmic|niri: SetSession/SetSessionType in AccountsService
#         | console:      create /run/gemini-console
#         v
#   display-manager.service (GDM)  has
#         ConditionPathExists=!/run/gemini-console  -> skipped on console
#         and auto-logs the user into the AccountsService session otherwise
#
# `gemcli session set gnome|cosmic|niri|console` writes the marker (and can
# apply it now / reboot). The compile-time `services.geminiDesktop.mode`
# is only the fallback used when the marker is absent (fresh install).
#
# Why AccountsService and not `services.displayManager.defaultSession`:
# that option is emitted as a GDM preStart call to set-session, i.e. it
# OVERWRITES the session on every display-manager start. services/
# gnome.nix therefore leaves it unset (defaultSession = null) so the
# marker is authoritative; this module reproduces set-session's two
# setters directly (busctl, no python).
#
# Full story, receipts and the on-glass checklist:
# docs/desktop-selection.md.
{ config, lib, pkgs, ... }:

let
  cfg = config.services.geminiDesktop;
  # Same callPackage as services/gemini-pda.nix / services/gnome.nix, so
  # the closure shares the one gemini-pda-utils store path (the apply
  # script lives there).
  utils = pkgs.callPackage ./gemini-utils.nix { };
in
{
  options.services.geminiDesktop = {
    enable = lib.mkEnableOption "the Gemini desktop/session selector (boot marker -> GDM/console)";

    mode = lib.mkOption {
      type = lib.types.enum [ "gnome" "cosmic" "niri" "console" ];
      default = "gnome";
      description = ''
        Desktop to start when the persistent marker
        ({file}`/var/lib/gemini/desktop`) is absent — a fresh install.
        `gnome`, `cosmic` and `niri` are GDM Wayland sessions on the
        geminipda-drm KMS device (all must be enabled at build time, see
        services.desktopManager.* / programs.niri.enable); `console`
        starts no display manager and stays on the framebuffer console.
        Change at runtime with `gemcli session set <mode>`.
      '';
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "cjdell";
      description = ''
        Auto-login user whose session is selected (config/gemini.nix
        users.users.cjdell; services.getty.autologinUser for console).
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    # Resolve the marker into the display stack, before GDM starts.
    # Wants/After accounts-daemon because the SetSession* calls go
    # through it (D-Bus activation works too, but ordering is clearer).
    systemd.services.gemini-desktop-apply = {
      description = "Gemini desktop/session boot selection (marker -> GDM/console)";
      wants = [ "accounts-daemon.service" ];
      after = [ "accounts-daemon.service" "systemd-user-sessions.service" ];
      before = [ "display-manager.service" ];
      wantedBy = [ "multi-user.target" ];
      # busctl (systemd), id/tr (coreutils), chvt (kbd). systemctl is the
      # systemd package's /run/current-system/sw/bin symlink.
      path = [ pkgs.systemd pkgs.coreutils pkgs.kbd ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        # Creates /var/lib/gemini (persistent, root-owned) for the marker.
        StateDirectory = "gemini";
        Environment = [
          "GEMINI_DESKTOP_DEFAULT=${cfg.mode}"
          "GEMINI_DESKTOP_USER=${cfg.user}"
        ];
        ExecStart = "${utils}/bin/gemini-desktop-apply";
      };
    };

    # Console mode: the apply service drops this flag; GDM's Condition
    # then evaluates false and the unit is skipped, leaving the fbcon
    # console (and the tty1 getty the script starts). No other unit
    # writes ConditionPathExists, so this does not clobber one.
    systemd.services.display-manager.unitConfig.ConditionPathExists =
      "!/run/gemini-console";

    # The selector owns the auto-login session: it writes the
    # AccountsService `Session` before GDM starts. Session packages that
    # pin a default would undo that — `programs.niri` (enabled in
    # config/gemini.nix for the co-installed niri session) sets
    # services.displayManager.defaultSession to "niri" with mkDefault,
    # and GDM's preStart would then overwrite the marker's choice on
    # every display-manager start. Force it to null so the marker stays
    # authoritative (services/gnome.nix intentionally leaves it unset;
    # this makes that explicit and robust).
    services.displayManager.defaultSession = lib.mkForce null;
  };
}

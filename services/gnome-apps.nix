# GNOME application suite for the Gemini PDA — the "supporting GNOME apps"
# (calculator, calendars, maps, clocks, contacts, viewer, …).
#
# SCOPE: these are GNOME *applications* (GTK4/libadwaita Wayland clients).
# They run fine as clients of the GNOME session (services/gnome.nix) or the
# gemshell compositor — both put /run/current-system/sw/share on
# XDG_DATA_DIRS, so their .desktop entries land in the app grid. This module
# provides only the applications; the GNOME session/shell itself lives in
# services/gnome.nix (docs/gnome-feasibility.md).
#
# Why a module at all (vs. dumping them in config/gemini.nix): the set is
# a single opt-in toggle, and the app-specific services (geoclue for
# Maps/Weather location) belong with it. Turn it off for a lean image.
#
# What is deliberately NOT here:
#   - xdg-desktop-portal-gnome: the real GNOME session (services/gnome.nix)
#     brings its own portal; this app-only module leaves that to the session
#     so it is not picked on the gemshell session.
#   - gnome-software: its NixOS story is a non-starter (no Nix backend);
#     `nixos-rebuild`/`nix` are the package tools on this device.
#   - gnome-settings-daemon / gnome-session: shell-side scaffolding; the
#     shells own their own settings daemons.
{ config, lib, pkgs, ... }:

let
  cfg = config.services.geminiGnomeApps;

  # The GNOME app set. Names verified against the pinned nixpkgs
  # (dc5d91f84032, GNOME 50.x — `nix eval .#nixosConfigurations.gemini.pkgs`,
  # 2026-09-10): all aarch64 outputs substitute from cache.nixos.org
  # (channel pin, golden rule 9).
  gnomeApps = with pkgs; [
    gnome-calculator       # the "calc" ask
    gnome-calendar         # the "calendar" ask (local EDS; GOA for sync)
    gnome-maps             # the "maps" ask (needs geoclue for a fix)
    gnome-clocks
    gnome-weather          # needs geoclue for the location
    gnome-contacts         # local EDS; GOA for online accounts
    gnome-characters
    gnome-text-editor
    gnome-system-monitor
    gnome-disk-utility
    gnome-connections      # RDP/VNC client
    gnome-usage
    baobab                 # Disk Usage Analyzer
    seahorse               # Passwords and Keys
    file-roller            # archive manager
    papers                 # document viewer (Evince successor)
    loupe                  # image viewer
    snapshot               # camera
    gnome-screenshot
    gnome-console          # terminal
  ];
in
{
  options.services.geminiGnomeApps = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Install the GNOME application suite (gnome-calculator,
        gnome-calendar, gnome-maps, gnome-clocks, gnome-weather,
        gnome-contacts, Papers, Loupe, …) on the system profile, plus the
        app-facing support (Adwaita icons, GSettings schemas, GNOME
        Online Accounts) and the geoclue2 location service. These are
        Wayland clients that appear in the app grid of the default GNOME
        session (services/gnome.nix). This module installs only the apps,
        not the GNOME session/shell.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = gnomeApps ++ [
      # Icon theme + schemas the apps assume; safe to duplicate with the
      # shells' own copies (same store paths).
      pkgs.adwaita-icon-theme
      pkgs.gsettings-desktop-schemas
      # GOA daemon/package: D-Bus-activatable for Calendar/Contacts
      # online-account sync when a session bus exposes it. Installing the
      # package is enough for activation; there is no gnome-session that
      # would start it eagerly.
      pkgs.gnome-online-accounts
    ];

    # Maps/Weather/Clocks ask geoclue2 over the system bus. The standard
    # setup needs a shell-side location *agent* (gnome-shell provides one
    # on vanilla GNOME; the gemshell compositor does not), so without an
    # agent geoclue
    # denies requests — the service is installed for when a front-end
    # exists, and it is harmless on its own. Config + receipts:
    # docs/gnome-feasibility.md §"App support".
    services.geoclue2.enable = lib.mkDefault true;

    # GNOME apps are Cantarell/Adwaita; fontconfig defaults to DejaVu.
    # List option: this concatenates with the shells' font definitions.
    fonts.packages = [ pkgs.cantarell-fonts pkgs.dejavu_fonts ];
  };
}

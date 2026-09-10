# Desktop/system plumbing — battery + power status on the standard
# buses, so ANY desktop environment (Phosh today, LXQt/gemwl tomorrow,
# a future GNOME/KDE session) gets power state without gemini-specific
# glue. Design + receipts: docs/desktop-plumbing.md.
#
# WHAT THIS MODULE OWNS (all DE-agnostic, all on the system bus):
#
#   - UPower (org.freedesktop.UPower): the daemon every DE shell reads
#     for battery/AC state. Phosh's top-bar battery icon, LXQt's panel
#     battery widget, GNOME's power panel … all speak UPower; nothing
#     gemini-specific speaks to it.
#   - The battery data itself comes from a Battery-type power_supply
#     ("bq25890-battery-N") the kernel driver now registers next to the
#     BQ25896 charger (no fuel-gauge IC on this board — capacity is
#     voltage-derived; kernel delta bq25890_charger.c, 2026-09-10).
#   - Brightness: /sys/class/backlight attrs are world-writable (0666)
#     kernel-side (delta backlight.c) because the desktop sessions are
#     systemd SYSTEM services — they have no logind session, so the
#     standard logind SetBrightness path rejects them and uaccess never
#     tags their devices. cjdell is already in the video group for DRI.
#   - brightnessctl on PATH (the standard sysfs backlight CLI; volume
#     side is wpctl — shipped with wireplumber, services/audio.nix).
#
# Power policy: UPower must NEVER power the unit off/hibernate — the
# device's only real battery guard is gemini-battery-guard
# (services/gemini-pda.nix; 3.65 V warn / 3.50 V orderly poweroff over
# the BQ25896's own VBAT ADC). The voltage-derived capacity % is a UI
# estimate for status icons; it is deliberately not wired to any action.
# IgnoreLid is a no-op safety (no lid ACPI switch exists; logind side
# keys are already ignored in config/gemini.nix).
{ config, lib, pkgs, ... }:

let
  cfg = config.services.geminiPlumbing;
in
{
  options.services.geminiPlumbing = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Enable the DE-agnostic desktop plumbing: UPower (battery/AC
        status for phosh/LXQt/…), brightness sysfs access and the
        standard control CLIs. Harmless on a console-only boot; disable
        only for a minimal headless profile.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    services.upower = {
      enable = true;
      # Percent-based thresholds (time-based needs energy data the
      # voltage-derived battery does not have).
      usePercentageForPolicy = true;
      percentageLow = 15; # ~3.64 V on the kernel's OCV curve (battery-guard WARN 3.65 V)
      percentageCritical = 5; # ~3.57 V
      percentageAction = 2; # ~3.50 V — battery-guard's CRIT poweroff point
      # Upower's critical action would hibernate/poweroff on its own
      # schedule; this device's only trusted poweroff is
      # gemini-battery-guard's (it reads the same VBAT ADC the %
      # estimates come from — no double policy). [2026-09-10]
      allowRiskyCriticalPowerAction = true;
      criticalPowerAction = "Ignore";
      ignoreLid = true;
    };

    environment.systemPackages = [
      # Standard sysfs backlight control (works because the kernel delta
      # makes /sys/class/backlight/*/brightness world-writable; see
      # docs/desktop-plumbing.md §brightness). Root's `backlight`/gemcli
      # CLIs remain for the console path.
      pkgs.brightnessctl
    ];
  };
}

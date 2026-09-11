# Desktop/system plumbing — battery + power status on the standard
# buses, so ANY desktop environment (GNOME today, gemshell, or a future
# KDE session) gets power state without gemini-specific
# glue. Design + receipts: docs/desktop-plumbing.md.
#
# WHAT THIS MODULE OWNS (all DE-agnostic, all on the system bus):
#
#   - UPower (org.freedesktop.UPower): the daemon every DE shell reads
#     for battery/AC state. GNOME's power panel, a shell's top-bar
#     battery widget, GNOME's power panel … all speak UPower; nothing
#     gemini-specific speaks to it.
#   - The battery data itself comes from a Battery-type power_supply
#     ("bq25890-battery-N") the kernel driver now registers next to the
#     BQ25896 charger (no fuel-gauge IC on this board — capacity is
#     voltage-derived; kernel delta bq25890_charger.c, 2026-09-10).
#   - Brightness: /sys/class/backlight/*/brightness gets world-writable
#     (0666) via a udev RUN rule below. Kernel sysfs attrs are compiled
#     0644 (VERIFY_OCTAL_PERMISSIONS rejects wider modes at build time)
#     and the desktop sessions are systemd SYSTEM services — they have
#     no logind session, so the standard logind SetBrightness route
#     rejects them and uaccess never tags their devices. chmod from udev
#     is the standard runtime answer (same trick Android/others use on
#     sysfs); GNOME's brightness handling and brightnessctl both write
#     the file directly.
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
        status for any desktop shell), brightness sysfs access and the
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

    services.udev.extraRules = ''
      # Gemini backlight access for the session-less desktop (2026-09-10):
      # the desktop runs as a systemd system service as cjdell — no logind
      # session exists, so uaccess tagging never applies and the logind
      # SetBrightness D-Bus route is unavailable. chmod the sysfs attrs
      # on add (sysfs honours the inode mode on open; the kernel cannot
      # express 0666 at build time — VERIFY_OCTAL_PERMISSIONS). 0666
      # matches the trust model of cjdell's passwordless sudo on this
      # single-user PDA.
      SUBSYSTEM=="backlight", ACTION=="add", RUN+="${pkgs.coreutils}/bin/chmod 0666 /sys/class/backlight/%k/brightness /sys/class/backlight/%k/bl_power"
    '';

    environment.systemPackages = [
      # Standard sysfs backlight control (works because the kernel delta
      # makes /sys/class/backlight/*/brightness world-writable; see
      # docs/desktop-plumbing.md §brightness). Root's `backlight`/gemcli
      # CLIs remain for the console path.
      pkgs.brightnessctl
    ];
  };
}

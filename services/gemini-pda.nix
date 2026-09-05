# Phase 3: Gemini PDA device services (rootfs side).
#
# Ports the verified bring-up utilities from the GeminiPDA project into
# the NixOS system (services/scripts/ holds the scripts, verbatim except
# the power PATH shim and the new gemini-wdt-reboot):
#
#   gemini-gpu-poweron   Mali-T880 MFG MTCMOS power-on + VGPU rails
#   gemini-a72-up        A72 cluster (cpu8/cpu9) bring-up via sramldo-smc.ko
#   gemini-battery-guard battery safety daemon (level + USB-charge checks)
#   backlight / power    display LED-boost + charge CLI
#   battstat / bq25896-raw  BQ25896 status / raw ADC readers
#   gemini-boot-recovery reboot into TWRP (para=boot-recovery)
#   gemini-wdt-reboot    device-side reboot that self-boots (WDT EXRST)
#
# Kernel prerequisites (all in devices/planet-geminipda/kernel/config.aarch64
# and the DTS): CONFIG_DEVMEM, I2C_CHARDEV + I2C_DESIGNWARE (i2c0 charger,
# i2c6 DA9214 @0x1100e000, i2c7 RT5735 @0x11010000), CHARGER_BQ25890.
# sramldo-smc.ko is built by the kernel derivation's postInstall hook and
# shipped in the module tree (extra/); boot.kernelModules loads it at boot.
{ config, lib, pkgs, ... }:

let
  utils = pkgs.callPackage ./gemini-utils.nix { };
  kernelModulePath =
    # Runtime path of the A72 bring-up module in the booted system's
    # module tree (NixOS's kmod searches /run/booted-system/kernel-modules
    # first; the explicit path is the insmod fallback).
    "/run/booted-system/kernel-modules/lib/modules/$(uname -r)/extra/sramldo-smc.ko";
in
{
  # busybox (devmem) + i2c-tools are used by the scripts and by hand on
  # the serial console.
  environment.systemPackages = [ utils pkgs.busybox pkgs.i2c-tools ];

  # Load the out-of-tree A72 module at boot (systemd-modules-load, via
  # the NixOS-wrapped modprobe that searches the store module tree).
  boot.kernelModules = [ "sramldo-smc" ];

  systemd.services.gemini-gpu-poweron = {
    description = "Gemini PDA GPU power-on (Mali-T880 MFG MTCMOS domains, VGPU rails)";
    after = [ "systemd-udevd.service" ];
    wantedBy = [ "multi-user.target" ];
    unitConfig = {
      Type = "oneshot";
      RemainAfterExit = "yes";
      Restart = "on-failure";
      RestartSec = "30";
    };
    serviceConfig = {
      ExecStart = "${utils}/bin/gemini-gpu-poweron.sh";
      Path = lib.makeBinPath [
        pkgs.busybox # devmem
        pkgs.i2c-tools # i2cset/i2cget (RT5735 VGPU rail)
        pkgs.coreutils # od, sleep
        pkgs.gnused # i2c bus resolution
      ];
    };
  };

  systemd.services.gemini-a72-up = {
    description = "Gemini PDA A72 cluster bring-up (cpu8/cpu9 online)";
    # The reference service runs After=multi-user.target: the DA9214 i2c6
    # bus is contended by SCP for the first ~2 minutes after boot, so
    # running late (and retrying with backoff) is deliberate.
    after = [ "systemd-udevd.service" "multi-user.target" ];
    wantedBy = [ "multi-user.target" ];
    unitConfig = {
      Type = "oneshot";
      RemainAfterExit = "yes";
      Restart = "on-failure";
      RestartSec = "30";
    };
    serviceConfig = {
      # belt and braces: boot.kernelModules already loads it; re-assert
      # here so a failed modules-load does not wedge the bring-up.
      ExecStartPre =
        "/bin/sh -c 'modprobe sramldo-smc 2>/dev/null || insmod ${kernelModulePath} 2>/dev/null || true'";
      ExecStart = "${utils}/bin/cl2-up.sh";
      Path = lib.makeBinPath [
        pkgs.bash
        pkgs.busybox # devmem
        pkgs.i2c-tools # i2cset (DA9214 BUCKB)
        pkgs.coreutils # od, sleep, cat
        pkgs.gnused
        pkgs.util-linux # logger
        pkgs.kmod # modprobe, insmod
      ];
    };
  };

  systemd.services.gemini-battery-guard = {
    description = "Gemini PDA battery safety guard (safe level + USB-charge check)";
    after = [ "multi-user.target" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      ExecStart = "${utils}/bin/battery-guard.sh";
      Restart = "always";
      RestartSec = "10";
      Environment = [
        "BATTERY_GUARD_POLL_S=10"
        "BATTERY_GUARD_WARN_LOW_MV=3650"
        "BATTERY_GUARD_CRIT_MV=3500"
      ];
      Path = lib.makeBinPath [
        pkgs.bash
        pkgs.coreutils # date, stat, sleep, cat
        pkgs.gawk # state file parsing
        pkgs.util-linux # logger
        pkgs.systemd # systemctl poweroff
      ];
    };
  };
}

# Phase 3: Gemini PDA device services (rootfs side).
#
# Ports the verified bring-up utilities from the GeminiPDA project into
# the NixOS system (services/scripts/ holds the scripts, verbatim except
# the power PATH shim and the new gemini-wdt-reboot):
#
#   gemini-gpu-poweron   Mali-T880 MFG MTCMOS power-on + VGPU rails
#   gemini-a72-up        A72 cluster (cpu8/cpu9) bring-up via sramldo-smc.ko
#   cl2-down.sh          A72 cluster power-DOWN (hand-run CLI, NOT a unit —
#                        the reverse of a72-up, see the service below)
#   gemini-battery-guard battery safety daemon (level + USB-charge checks)
#   backlight / power    display LED-boost + charge CLI
#   battstat / bq25896-raw  BQ25896 status / raw ADC readers
#   gemini-boot-recovery reboot into TWRP (para=boot-recovery)
#   gemini-boot-debian   reboot into Debian p29 (para=boot-debian; the
#                        dual-boot initrd selector, 2026-09-07 — see
#                        docs/repartition-android-space.md §5)
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
  #
  # The DRM core chain + panfrost load EARLY and deterministically here
  # (systemd-modules-load runs before gemini-wifi-internal.service, which
  # already declares After=systemd-modules-load). The vendor wlan_gen3
  # driver registers chrdev major 226 ("ampc0", BT-over-WiFi), which
  # COLLIDES with DRM_MAJOR — whoever loads first wins; if wlan wins,
  # drm.ko init fails EBUSY and the GPU dies for the boot. Same list as
  # the verified Debian /etc/modules-load.d/99-gpu.conf (outstanding.md
  # item 4; kernel fix #330 — alloc_chrdev_region — pending upstream).
  # mt6351-keys is the PMIC key driver (ESC/On + silver KEY_SLEEP); pinned for zero doubt
  # over udev modalias autoload. All modules verified present in the
  # borrowed #329 module tree (drm{, _shmem_helper}.ko, gpu-sched.ko,
  # panfrost.ko, mt6351-keys.ko).
  boot.kernelModules = [
    "sramldo-smc"
    "drm" # major-226 race (see header + outstanding.md item 4)
    "drm_shmem_helper"
    "gpu-sched"
    "panfrost"
    "mt6351-keys" # deterministic side keys (silver button)
  ];

  # B-19 USB host mode: runtime-PM autosuspend on the MT6797 USB host
  # controllers clears IPPC HOST_SEL and power-cycles the U2 PHY; there
  # is no USB wakeup source wired, so autosuspend permanently kills
  # connect detection until reboot (boot.md build #231 defect 2). Pin the
  # controllers + all USB devices to "on". Same rule as the verified
  # Debian /etc/udev/rules.d/99-gemini-usb-host-pm.rules
  # (outstanding.md item 5).
  services.udev.extraRules = ''
    ACTION=="add", SUBSYSTEM=="platform", KERNEL=="11270000.usb", TEST=="power/control", ATTR{power/control}="on"
    ACTION=="add", SUBSYSTEM=="platform", KERNEL=="11271000.usb", TEST=="power/control", ATTR{power/control}="on"
    ACTION=="add", SUBSYSTEM=="usb", TEST=="power/control", ATTR{power/control}="on"
  '';

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

  # The REVERSE path ships as the hand-run `cl2-down.sh` CLI in the same
  # utils package (2026-09-07, sibling commit 738d19f): per-core PSCI
  # offline is safe, and the last-A72 teardown (secure power_off_cl3 —
  # CCI/snoop/SPM/ISO + external DA9214 BUCKB rail drop) is proven on
  # #329, returning the box to the cold-boot state cl2-up was built for.
  # Deliberately NOT a unit: the power-saving down is on-demand only — a
  # boot-time auto-down would fight this service. Usage on the device:
  # `cl2-down.sh [cpu9|cpu8|both]` (both = full cluster off); re-enable
  # with `cl2-up.sh`. WDT-armed (20 s) inside the script.
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

  # DISP_PWM0 backlight = power-saving 10 % default at boot. The display
  # LED boost is the single biggest load and at full backlight the
  # battery cannot charge from a 500 mA USB port (BQ25896 power path,
  # ICHGR=0; verified). 10 % keeps the screen usable while charging.
  # Runs after udev so /sys/class/backlight exists on kernel #329
  # (CONFIG_PWM_MTK_DISP=y — built-in, no module step needed, unlike the
  # Debian unit's pwm-mtk-disp modules-load entry, which is vestigial
  # for #329). `backlight` prefers the sysfs path and falls back to
  # direct DISP_PWM0 devmem writes; same behaviour as the verified
  # Debian backlight-default.service (outstanding.md item 6). Change the
  # value here and `systemctl restart gemini-backlight-default`.
  systemd.services.gemini-backlight-default = {
    description = "Gemini display backlight = power-saving default (10 %)";
    after = [ "systemd-udevd.service" ];
    wantedBy = [ "multi-user.target" ];
    unitConfig = {
      Type = "oneshot";
      RemainAfterExit = "yes";
    };
    serviceConfig = {
      ExecStart = "${utils}/bin/backlight set 10";
      Path = lib.makeBinPath [
        pkgs.bash
        pkgs.busybox # devmem (fallback path)
        pkgs.coreutils # ls/cat/head (sysfs path)
      ];
    };
  };

  # Device-side reboot helpers as hand-started units (NOT enabled — a
  # boot-time wdt arm would be catastrophic). These make the safe-reboot
  # paths explicit on the device and back the AGENTS.md cheat-sheet
  # claim that a `gemini-wdt-reboot` unit exists (outstanding.md item 9
  # — previously only the CLIs were packaged). See the script headers:
  #   gemini-wdt-reboot [SECS]  WDT EXRST SELF-BOOT (the only software
  #                             path that comes back on; plain reboot /
  #                             systemctl reboot POWERS the PDA off,
  #                             verified 2026-08-31)
  #   gemini-boot-recovery      writes para=boot-recovery then plain
  #                             reboot → the unit powers OFF; the next
  #                             power-on (para is sticky) lands in TWRP
  #   gemini-boot-debian        writes para=boot-debian then plain reboot
  #                             → next power-on boots Debian p29 through
  #                             the dual-boot initrd (repartition doc §5)
  systemd.services.gemini-wdt-reboot = {
    description = "Gemini PDA WDT EXRST self-boot (device-side safe reboot)";
    after = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      ExecStart = "${utils}/bin/gemini-wdt-reboot 20";
      TimeoutStartSec = "35"; # 20 s arm + margin; the WDT fires first
      # busybox (devmem arm) + coreutils (sync/sleep while waiting).
      Path = lib.makeBinPath [ pkgs.busybox pkgs.coreutils ];
    };
  };
  systemd.services.gemini-boot-recovery = {
    description = "Gemini PDA reboot into TWRP (para=boot-recovery)";
    after = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      ExecStart = "${utils}/bin/gemini-boot-recovery";
      # coreutils (dd/sync) + systemd (reboot, via its sw/bin symlink).
      Path = lib.makeBinPath [ pkgs.coreutils pkgs.util-linux pkgs.systemd ];
    };
  };
  systemd.services.gemini-boot-debian = {
    description = "Gemini PDA reboot into Debian p29 (para=boot-debian)";
    after = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      ExecStart = "${utils}/bin/gemini-boot-debian";
      # coreutils (dd/sync) + gnugrep (para verify) + systemd (reboot).
      Path = lib.makeBinPath [ pkgs.coreutils pkgs.util-linux pkgs.gnugrep pkgs.systemd ];
    };
  };
}

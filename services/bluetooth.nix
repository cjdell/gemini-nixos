# Bluetooth: MT6630 CONSYS hci_stp stack as a persistent NixOS service
# (bring-up story: docs/bluetooth-bringup.md; glass state 2026-09-09l:
# hci0 inits clean, BR/EDR + LE both enabled, discovery finds real
# devices over the air).
#
# WHAT THIS ADDS over the bring-up's ad-hoc `nix-shell -p bluez` +
# systemd-run test daemons:
#   - a REAL bluetoothd on the system bus (org.bluez dbus policy now
#     generated: bluez joins services.dbus.packages -> its
#     share/dbus-1/system.d/bluetooth.conf lands in /etc/dbus-1/
#     system.d; the bring-up had to fake this with a permissive
#     private bus because org.bluez was absent from the static tree).
#   - the hci0 controller module (hci_stp) loaded at boot, ordered
#     after the internal-wifi CONSYS bring-up (mtk_wcn + chip pwr-on).
#   - bluetoothctl/btmgmt/hciconfig/hcitool on PATH (CLI).
#   - blueman (GUI manager + tray applet) for the LXQt session.
#
# OPTION NAME: the nixpkgs option at this pin (dc5d91f84032) is
# `hardware.bluetooth`, NOT `services.bluetooth` — the services.*
# alias was removed upstream before this rev (bring-up doc follow-up #1
# claimed services.bluetooth does not exist in the MNX eval; the real
# story is the rename — the MNX eval imports the FULL nixpkgs module
# list, verified in mobile-nixos lib/release-tools.nix evalWith:
# modules/module-list.nix ++ nixos/modules/module-list.nix).
#
# CONTROLLER QUIRKS handled kernel-side (module-only, no boot.img):
# HCI_QUIRK_EXT_INIT_BEST_EFFORT (MT6630 over-advertises + refuses
# cmd) + local LMP_HOST_LE/HCI_LE_ENABLED record when 0x200d is
# refused (mgmt/LE unlock). main.conf here stays at stock AutoEnable
# (=powerOnBoot) so bluetoothd powers hci0 on when it appears.
{ config, lib, pkgs, ... }:

{
  # ---- bluez stack (nixpkgs module: bluetoothd unit + dbus policy +
  # udev rules + /etc/bluetooth/main.conf + bluez in systemPackages).
  hardware.bluetooth.enable = true;
  # Policy.AutoEnable in main.conf: bluetoothd powers up hci0 when the
  # controller registers (the CONSYS func-on + full HCI init happen on
  # the first open — the exact path gen50 verified by hand).
  hardware.bluetooth.powerOnBoot = true;
  hardware.bluetooth.settings = {
    General = {
      ControllerMode = "dual"; # BR/EDR + LE (the bring-up's btmgmt state)
      # Privacy=off (default "device"): bluez 5.87 runs mgmt set-privacy
      # during the AUTO-power path at daemon start; the MT6630 rejects it
      # ("Failed to set privacy: Rejected (0x0b)") which aborts AutoEnable
      # and leaves the controller Powered: no after every boot. With
      # Privacy=off the daemon-start power on completes (verified on glass
      # 2026-09-09m with a manual bluetoothd -f test conf). Interactive
      # `bluetoothctl power on` always worked; this makes the BOOT path
      # work too.
      Privacy = "off";
    };
  };

  # ---- hci0 controller bring-up -------------------------------
  # hci_stp (the 6.6 port of the vendor drv_bt driver) registers hci0
  # on modprobe; the kernel module tree ships it in current-system
  # (drivers/misc/mediatek-connectivity/drv_bt). modprobe pulls in
  # bluetooth + mtk_wcn as dependencies. Order after the internal-wifi
  # CONSYS stack so the chip is already powered when hci0 opens (the
  # WAK heartbeat + wake-before-send fixes then apply cleanly); Wants=
  # (not Requires=) so BT still tries if wifi failed — mtk_wcn
  # autoloads via the module dependency either way.
  systemd.services.gemini-bt-hci = {
    description = "Bluetooth HCI (MT6630 CONSYS hci_stp) module bring-up";
    after = [ "gemini-wifi-internal.service" "systemd-modules-load.service" ];
    wants = [ "gemini-wifi-internal.service" ];
    before = [ "bluetooth.service" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = "yes";
      ExecStart = "${pkgs.kmod}/bin/modprobe hci_stp";
    };
    path = [ pkgs.kmod ];
  };

  # ---- bluetoothd at boot (dbus-org.bluez.service activation) --
  # The nixpkgs module's bluetooth.service only hangs off
  # bluetooth.target; pull that target into the normal boot.
  systemd.targets.bluetooth.wantedBy = [ "multi-user.target" ];

  # ---- GUI tools ----------------------------------------------
  # blueman = the GTK manager (blueman-manager) + tray applet
  # (blueman-applet; autostarted in the LXQt session via its shipped
  # etc/xdg/autostart/blueman.desktop — see services/lxqt.nix, which
  # puts blueman on the session XDG_CONFIG_DIRS/PATH when bluetooth is
  # enabled). CLI tools (bluetoothctl/btmgmt/hciconfig/hcitool) come
  # from the bluez package hardware.bluetooth puts in systemPackages.
  # blueman here too so the manager is runnable from an ssh shell
  # without the desktop.
  environment.systemPackages = [ pkgs.blueman ];
}

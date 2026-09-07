# Gemini PDA Wi-Fi (rootfs side).
#
# Ports the verified Wi-Fi stack from the GeminiPDA project
# (build/rootfs-files/wifi/ + build/rootfs-files/wifi-consys/) into the
# NixOS system. Two independent radios, one CLI each (both in
# gemini-pda-utils):
#
#   INTERNAL — the on-die MT6630 CONSYS: mtk_wcn (WMT core) +
#   wlan_gen3 (vendor gen3 802.11 host stack) + the debugfs pwr-on that
#   runs the full WMT vendor init (hw_check -> ROMv3 patches ->
#   STP_RDY -> FUNC_ON -> wlan0). gemini-wifi-internal.service brings the
#   kernel stack up at boot. Load ORDER matters: wlan_gen3 must be
#   loaded before the pwr-on (a func-on with no wlan registered returns
#   -2 and powers the chip back down — B-33), and the whole unit runs
#   AFTER gemini-gpu-poweron (the CONSYS chip's chrdev, major 226,
#   collides with the GPU's chrdev if the wlan modules probe before the
#   GPU power-on sequence — handover-2026-09-07).
#
#   USB — the 0bda:c811 RTL8821CU dongle: rtw88_8821cu (udev
#   auto-loads it when the dongle is plugged in; the firmware is in the
#   gemini firmware package) + `wifi` CLI + gemini-wifi-auto.service
#   (wpa_supplicant + dhcpcd against /etc/wifi/profiles.conf, ordered
#   after the internal stack so wlan0 — the internal STA interface —
#   exists when it runs on CONSYS builds).
#
#   Factory NVRAM — the gen3 driver reads /data/nvram/APCFG/APRDEB/WIFI
#   at probe for the MAC + TX calibration; /data is a tmpfs (kept out of
#   the rootfs image, as on the verified device), so
#   gemini-wifi-nvram.service installs the factory record before the
#   internal stack comes up.
#
# g_ether (usb0, 10.15.19.82) is the host link — Wi-Fi never touches it.
{ config, lib, pkgs, ... }:

let
  utils = pkgs.callPackage ./gemini-utils.nix { };
  firmware = pkgs.callPackage ../pkgs/gemini-firmware.nix { };

  # Shared CLI path (R12: module-level `path`, not the removed
  # serviceConfig.Path unit key): the scripts call ip/iw/wpa_supplicant/
  # dhcpcd by name. Systemd turns this into an Environment PATH prepended
  # to the unit's default PATH.
  cliPath = [
    pkgs.bash
    pkgs.coreutils # sleep, cat, grep
    pkgs.gnused # sed
    pkgs.gawk # awk (scan parsing)
    pkgs.iproute2 # ip
    pkgs.iw
    pkgs.wpa_supplicant # wpa_supplicant, wpa_cli
    pkgs.dhcpcd
    pkgs.kmod # modprobe (wifi-internal)
    pkgs.util-linux # mount (debugfs), logger
    pkgs.procps # pkill
    utils
  ];
in
{
  # The kernel firmware_class path is pointed at this by nixpkgs
  # (modprobe.d/firmware.conf + the udevd activation script). The stock
  # firmware-linux package is replaced: this device needs exactly the
  # blobs above (same set as the verified device install).
  hardware.firmware = [ firmware ];

  # nixpkgs would zstd-compress the firmware (default for kernel >= 5.19),
  # but the #329 bring-up kernel has CONFIG_FW_LOADER_COMPRESS unset — it
  # only loads plain firmware files. Keep the blobs uncompressed.
  hardware.firmwareCompression = "none";

  # Volatile, non-rootfs state (the gen3 driver's nvram file), matching
  # the verified device layout where /data is a tmpfs.
  fileSystems."/data" = {
    device = "tmpfs";
    fsType = "tmpfs";
  };

  # NOTE: /etc/wifi/profiles.conf is deliberately NOT an environment.etc
  # entry: etc-managed files are read-only symlinks, and the `wifi` CLI
  # rewrites it (connect/forget). Instead it is seeded by
  # gemini-wifi-nvram (only when absent), so it stays a plain writable
  # file like on the verified device.

  environment.systemPackages = [
    pkgs.wpa_supplicant
    pkgs.iw
    pkgs.dhcpcd
    utils # wifi, wifi-internal CLIs
  ];

  systemd.services.gemini-wifi-nvram = {
    description = "Wi-Fi state install (factory NVRAM + profile seed)";
    # /data is a tmpfs, so the factory NVRAM record (MAC + TX cal) is
    # re-installed on every boot before the internal stack probes the
    # chip. The profiles file is seeded ONCE (only when absent) from the
    # verified device's copy — kept out of environment.etc so it stays
    # writable for the `wifi` CLI's connect/forget.
    after = [ "local-fs.target" ];
    before = [ "gemini-wifi-internal.service" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = "yes";
      ExecStart =
        ''/bin/sh -c '
          set -e
          mkdir -p /data/nvram/APCFG/APRDEB
          cp -f ${firmware}/nvram/WIFI /data/nvram/APCFG/APRDEB/WIFI
          mkdir -p /etc/wifi
          [ -e /etc/wifi/profiles.conf ] || cp ${../etc/wifi/profiles.conf} /etc/wifi/profiles.conf
        ' '';
    };
    path = [ pkgs.coreutils pkgs.bash ];
  };

  systemd.services.gemini-wifi-internal = {
    description = "Internal Wi-Fi (MT6630 CONSYS) stack bring-up";
    # mtk_wcn + wlan_gen3 + WMT pwr-on -> wlan0. GPU power-on first (the
    # major-226 chrdev collision, see header).
    after = [ "systemd-modules-load.service" "gemini-gpu-poweron.service" ];
    before = [ "gemini-wifi-auto.service" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = "yes";
      ExecStart = "${utils}/bin/wifi-internal start";
    };
    path = cliPath;
  };

  systemd.services.gemini-wifi-auto = {
    description = "Wi-Fi auto-connect (saved profiles)";
    # `wifi auto`: if /etc/wifi/profiles.conf has entries, associate with
    # the strongest known network + dhcpcd lease. Silent no-op without
    # profiles. After the internal stack (wlan0 exists on CONSYS
    # builds); Wants= (not Requires=) so a missing internal stack never
    # blocks the USB-dongle path.
    after = [ "network.target" "gemini-wifi-internal.service" ];
    wants = [ "network.target" "gemini-wifi-internal.service" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = "yes";
      ExecStart = "${utils}/bin/wifi auto";
    };
    path = cliPath;
  };
}

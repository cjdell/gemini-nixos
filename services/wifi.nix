# Gemini PDA Wi-Fi (rootfs side).
#
# Ports the verified Wi-Fi stack from the GeminiPDA project
# (build/rootfs-files/wifi/ + build/rootfs-files/wifi-consys/) into the
# NixOS system. Two independent radios:
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
#   gemini firmware package).
#
# [changed 2026-09-10] CONNECTIVITY MANAGER = NetworkManager by default
# (services.geminiWifi.useNetworkManager). The DE status bars / wifi
# pickers (phosh quick settings, GNOME, LXQt via nm-applet/nm-tray, …)
# all talk org.freedesktop.NetworkManager; the old bring-up stack drove
# wlan0 with a standalone wpa_supplicant + dhcpcd (`wifi` CLI +
# gemini-wifi-auto), which no desktop can show or control. NM owns
# wlan0 (CONSYS) and the dongle's wlan1 through its wpa_supplicant
# backend; the CONSYS bring-up UNITS above stay exactly as they are
# (chip power + stack init are kernel-adjacent, not NM's job). Saved
# networks move from /etc/wifi/profiles.conf (legacy CLI store) to NM
# connection profiles (networking.networkmanager.ensureProfiles below).
# The `wifi` CLI + dhcpcd stay installed for diagnostics / the
# standalone fallback (useNetworkManager = false restores the old
# auto-connect unit). usb0 (g_ether) is unmanaged — the static host
# link config in config/gemini.nix owns it (see below).
#
# Factory NVRAM — the gen3 driver reads /data/nvram/APCFG/APRDEB/WIFI
# at probe for the MAC + TX calibration; /data is a tmpfs (kept out of
# the rootfs image, as on the verified device), so
# gemini-wifi-nvram.service installs the factory record before the
# internal stack comes up.
#
# g_ether (usb0, 10.15.19.82) is the host link — Wi-Fi never touches it.
{ config, lib, pkgs, ... }:

let
  cfg = config.services.geminiWifi;
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

  # Wi-Fi state-install script run by gemini-wifi-nvram below. R15
  # [2026-09-08]: the ExecStart MUST be this single script — the first
  # port wrote the whole `/bin/sh -c '…'` body as a multi-line Nix
  # string, and systemd parsed each physical line as a new unit
  # directive ("Invalid section header '[ -e /etc/wifi/profiles.conf ]
  # …'"), so the unit came up bad-setting on EVERY boot since c6afc5c:
  # the factory NVRAM (MAC + TX cal) and /etc/wifi/profiles.conf were
  # never installed, and wlan_gen3's probe then died on the missing
  # /data/nvram/APCFG/APRDEB/WIFI (dmesg "[wlan]nvram_read: failed to
  # open!!"). writeShellScript keeps the unit file single-line; the
  # script embeds the firmware store path + the repo's profiles seed.
  wifiStateInstall = pkgs.writeShellScript "gemini-wifi-state-install" ''
    set -e
    mkdir -p /data/nvram/APCFG/APRDEB
    cp -f ${firmware}/nvram/WIFI /data/nvram/APCFG/APRDEB/WIFI
    mkdir -p /etc/wifi
    [ -e /etc/wifi/profiles.conf ] || cp ${../etc/wifi/profiles.conf} /etc/wifi/profiles.conf
  '';
in
{
  options.services.geminiWifi = {
    useNetworkManager = lib.mkOption {
      type = lib.types.bool;
      # [changed 2026-09-10] NetworkManager owns wlan0/wlan1 (desktop
      # plumbing — docs/desktop-plumbing.md §wifi); was: standalone
      # wpa_supplicant + dhcpcd via the `wifi` CLI + gemini-wifi-auto.
      default = true;
      description = ''
        Manage Wi-Fi through NetworkManager (the org.freedesktop.
        NetworkManager system service every desktop shell's wifi UI
        speaks). The CONSYS/USB bring-up units still run; NM replaces
        the standalone wpa_supplicant + dhcpcd auto-connect. Set false
        for the legacy `wifi` CLI behaviour (profiles.conf + 
        gemini-wifi-auto).
      '';
    };
  };

  config = {
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
    # file like on the verified device. In NM mode the networks in it are
    # ALSO declared as NM profiles (ensureProfiles below) — keep the two
    # in sync when the home network changes.

    environment.systemPackages = [
      pkgs.wpa_supplicant
      pkgs.iw
      pkgs.dhcpcd # legacy `wifi` CLI backend (standalone mode / diagnostics)
      pkgs.networkmanager # nmcli etc. (NM mode; also the system NM binary)
      utils # wifi, wifi-internal CLIs
    ];

    # wlan_gen3's kalFirmwareOpen does NOT use request_firmware — the
    # driver walks a HARDCODED path list (/storage/sdcard0,
    # /vendor/firmware, /lib/firmware) with kernel file-open, and none of
    # the three exist on NixOS. The blobs are already in the closure via
    # hardware.firmware (exposed at /run/current-system/firmware — the
    # WMT/ROMv3 request_firmware leg loads fine from there), so point
    # /lib/firmware at that tree (L+ recreates the symlink even if a
    # previous generation left something in the way). The live switch
    # needs `systemctl restart systemd-tmpfiles-setup` (or reboot) for the
    # symlink to appear. [2026-09-08, handover-2026-09-08-wifi-keyboard
    # §2b]
    systemd.tmpfiles.rules = [
      "L+ /lib/firmware - - - - /run/current-system/firmware"
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
        ExecStart = "${wifiStateInstall}";
      };
      path = [ pkgs.coreutils pkgs.bash ];
    };

    systemd.services.gemini-wifi-internal = {
      description = "Internal Wi-Fi (MT6630 CONSYS) stack bring-up";
      # mtk_wcn + wlan_gen3 + WMT pwr-on -> wlan0. GPU power-on first (the
      # major-226 chrdev collision, see header). After tmpfiles too: the
      # /lib/firmware symlink (wlan_gen3's hardcoded firmware path, see
      # the tmpfiles rule above) must exist before the probe reads the RAM
      # code. (The before= ordering on the legacy auto-connect unit only
      # applies in standalone mode.)
      after = [ "systemd-modules-load.service" "systemd-tmpfiles-setup.service" "gemini-gpu-poweron.service" ];
      before = lib.mkIf (!cfg.useNetworkManager) [ "gemini-wifi-auto.service" ];
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = "yes";
        ExecStart = "${utils}/bin/wifi-internal start";
      };
      path = cliPath;
    };

    # ---- Legacy standalone auto-connect (only when NM is off) ---------
    systemd.services.gemini-wifi-auto = lib.mkIf (!cfg.useNetworkManager) {
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

    # ---- NetworkManager (default since 2026-09-10) --------------------
    # Standard DE-facing wifi: NM manages wlan0 (CONSYS, after the
    # bring-up unit created it) + the dongle's wlan1, over its own
    # wpa_supplicant backend. Everything a DE needs comes from the
    # org.freedesktop.NetworkManager bus service. cjdell is in the
    # `networkmanager` group (config/gemini.nix) — the NM polkit rule
    # (added by the NM module) lets that group change settings without
    # prompting, which matters because the desktop has no logind session
    # for polkit's "active local user" test to pass.
    networking.networkmanager = lib.mkIf cfg.useNetworkManager {
      enable = true;
      # usb0/g_ether stays owned by the static config in config/gemini.nix
      # (fixed 10.15.19.82 link, host gateway) — never let NM auto-claim
      # it ("auto-default" would otherwise grab it on every boot).
      unmanaged = [ "interface-name:usb0" ];
      # DNS: default rc-manager (resolvconf — already on, the NixOS
      # config/resolvconf default) merges NM's DHCP nameservers with the
      # static 1.1.1.1 base from networking.nameservers. Standard NixOS
      # desktop behaviour; no systemd-resolved needed on this lean stack.
      # Deterministic probing: the CONSYS gen3 driver has no
      # mac-randomization handling — keep scan MACs stable (was the
      # behaviour of the standalone stack).
      wifi.scanRandMacAddress = false;
      # The home networks, declared as NM profiles. These mirror
      # etc/wifi/profiles.conf (the legacy CLI store, seeded by
      # gemini-wifi-nvram); keep both in sync. NM autoconnects to the
      # strongest saved network at boot (autoconnect default). Profiles
      # are written to /run/NetworkManager/system-connections (volatile,
      # re-seeded each boot from this config); edits made with nmcli are
      # persisted by NM in /etc/NetworkManager/system-connections — the
      # ensure-profiles unit re-adds these two on every boot, so rename
      # via the UI instead of editing if you want changes to stick
      # (module docs on networking.networkmanager.ensureProfiles).
      ensureProfiles.profiles = {
        "the-lab" = {
          connection = {
            id = "The Lab";
            type = "wifi";
          };
          wifi = {
            ssid = "The Lab";
            mode = "infrastructure";
          };
          wifi-security = {
            key-mgmt = "wpa-psk";
            psk = "Graft0nSt.";
          };
          ipv4 = {
            method = "auto";
          };
          ipv6 = {
            method = "auto";
          };
        };
        "the-lab-2.4ghz" = {
          connection = {
            id = "The Lab 2.4GHz";
            type = "wifi";
          };
          wifi = {
            ssid = "The Lab 2.4GHz";
            mode = "infrastructure";
          };
          wifi-security = {
            key-mgmt = "wpa-psk";
            psk = "Graft0nSt.";
          };
          ipv4 = {
            method = "auto";
          };
          ipv6 = {
            method = "auto";
          };
        };
      };
    };

    # Start NM only after the CONSYS bring-up created wlan0 (it watches
    # udev anyway; this keeps the ordering deterministic and the first
    # autoconnect prompt-free). Wants= so a CONSYS failure still leaves
    # the USB-dongle path (wlan1) managed.
    systemd.services.NetworkManager = lib.mkIf cfg.useNetworkManager {
      after = [ "gemini-wifi-internal.service" ];
      wants = [ "gemini-wifi-internal.service" ];
    };

    # No cellular modem on this unit — don't run ModemManager (the NM
    # module mkDefaults it on).
    networking.modemmanager.enable = lib.mkIf cfg.useNetworkManager false;
  };
}

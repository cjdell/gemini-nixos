# Firmware for the Gemini PDA's Wi-Fi stacks.
#
# Layout follows the verified device install (GeminiPDA
# build/rootfs-files/wifi{, -consys}/install-*.sh), which NixOS picks up
# via hardware.firmware (the kernel firmware_class path is pointed at
# ${hardware.firmware}/lib/firmware by nixpkgs udev.nix):
#
#   lib/firmware/WMT_SOC.cfg           80 B; the WMT core reads this at
#                                      mtk_wcn module load
#   lib/firmware/ROMv3_patch_1_{1,0}_hdr.bin
#                                      MCU-reported ROMv3 patches (the
#                                      vendor 3.18 WMT driver loads the
#                                      same blobs; the #329 kernel also
#                                      bakes 1_1/1_0 in via
#                                      CONFIG_EXTRA_FIRMWARE — the files
#                                      here cover the standard
#                                      request_firmware path)
#   lib/firmware/WIFI_RAM_CODE_6797    451904 B; pushed into the CONSYS
#                                      EMI window by wlan_gen3 at
#                                      wlanProbe (not used at boot)
#   lib/firmware/rtw88/rtw8821c_fw.bin USB Wi-Fi dongle (0bda:c811
#                                      RTL8821CU, rtw88_8821cu)
#
#   nvram/WIFI                         512 B; the gen3 driver's nvram
#                                      file (WIFI_NVRAM_FILE_NAME in
#                                      platform.c), installed to
#                                      /data/nvram/APCFG/APRDEB/WIFI at
#                                      boot by gemini-wifi-nvram.service.
#                                      This is the unit's FACTORY record
#                                      (real MAC 00:09:34:5a:af:c1 +
#                                      factory TX-cal table, extracted
#                                      from stock-dump/nvram.bin
#                                      2026-09-06) — without it the MAC
#                                      is "dynamically generated" from
#                                      the time tick and changes every
#                                      power cycle.
{ stdenvNoCC, lib, ... }:

stdenvNoCC.mkDerivation {
  pname = "gemini-wifi-firmware";
  version = "1.0";

  # Pure install layout — no source to unpack/build.
  dontUnpack = true;
  dontConfigure = true;
  dontBuild = true;

  installPhase = ''
    mkdir -p $out/lib/firmware/rtw88 $out/nvram
    # NOTE: every install names its destination explicitly. The blob
    # refs are store files named <hash>-<basename>; `install SRC dir/`
    # would keep the store basename and the kernel's firmware loader
    # (which requests plain names) would never find them.
    # Internal (on-die) MT6630 CONSYS stack:
    install -m 644 ${./gemini-firmware/WMT_SOC.cfg}              $out/lib/firmware/WMT_SOC.cfg
    install -m 644 ${./gemini-firmware/ROMv3_patch_1_1_hdr.bin}  $out/lib/firmware/ROMv3_patch_1_1_hdr.bin
    install -m 644 ${./gemini-firmware/ROMv3_patch_1_0_hdr.bin}  $out/lib/firmware/ROMv3_patch_1_0_hdr.bin
    install -m 644 ${./gemini-firmware/WIFI_RAM_CODE_6797}       $out/lib/firmware/WIFI_RAM_CODE_6797
    # USB dongle (RTL8821CU):
    install -m 644 ${./gemini-firmware/rtw8821c_fw.bin}          $out/lib/firmware/rtw88/rtw8821c_fw.bin
    # Factory NVRAM record (MAC + TX cal):
    install -m 644 ${./gemini-firmware/WIFI_factory.bin}         $out/nvram/WIFI
  '';

  meta = with lib; {
    description = "Gemini PDA Wi-Fi firmware (MT6630 CONSYS WMT + RTL8821CU)";
    license = licenses.unfreeRedistributable;
    platforms = platforms.all;
  };
}

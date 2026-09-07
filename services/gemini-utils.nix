# Gemini PDA userspace helpers, as a single package.
#
# The scripts are the verified bring-up utilities from the GeminiPDA
# project (build/gpu-poweron.sh, build/a72-bringup/cl2-up.sh,
# build/rootfs-files/{backlight,battery-guard,gemini-boot-recovery,
# pipewire,speaker-amp,wifi,wifi-consys}), copied verbatim except for
# the `__GEMINI_UTILS__` placeholder (each script's own bin dir, so the
# CLIs find their siblings regardless of the store path they end up in)
# and gemini-wdt-reboot, a new device-side reboot helper (see the
# script's header). They are wired into the system as systemd services
# + CLIs by services/gemini-pda.nix, services/audio.nix and
# services/wifi.nix.
#
# The speaker-amp C helpers (gpioout/spkamp) are cross-compiled by
# pkgs/speaker-amp.nix and copied in here so the `speaker` CLI finds
# them next to itself.
{ runCommand, callPackage, ... }:

let
  speakerAmp = callPackage ../pkgs/speaker-amp.nix { };
in
runCommand "gemini-pda-utils" { } ''
  mkdir -p $out
  # Copy the CONTENTS into a fresh, writable bin dir: `cp -r DIR DEST`
  # would copy the store dir's read-only (0555) mode onto $out/bin.
  install -m 755 -d $out/bin
  cp -r ${./scripts}/. $out/bin/
  # Scripts reference their siblings by the bin-dir placeholder:
  for f in speaker audio-output audio-defaults.sh; do
    substituteInPlace $out/bin/$f --replace-fail __GEMINI_UTILS__ $out/bin
  done
  # Speaker-amp helpers (aarch64, see pkgs/speaker-amp.nix):
  cp ${speakerAmp}/bin/gpioout $out/bin/
  cp ${speakerAmp}/bin/spkamp $out/bin/
  chmod +x $out/bin/*
''

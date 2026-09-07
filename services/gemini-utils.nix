# Gemini PDA userspace helpers, as a single package.
#
# The scripts are the verified bring-up utilities from the GeminiPDA
# project (build/gpu-poweron.sh, build/a72-bringup/cl2-up.sh +
# cl2-down.sh,
# build/rootfs-files/{backlight,battery-guard,gemini-boot-recovery,
# pipewire,speaker-amp,wifi,wifi-consys}), copied verbatim except for
# the `__GEMINI_UTILS__` placeholder (each script's own bin dir, so the
# CLIs find their siblings regardless of the store path they end up in),
# gemini-wdt-reboot (a new device-side reboot helper, see the script's
# header), gemini-boot-debian (2026-09-07: the para=boot-debian half of
# the dual-boot selector — see docs/repartition-android-space.md §5) and the bash shebang rewrite below (R10). They are wired into
# the system as systemd services + CLIs by services/gemini-pda.nix,
# services/audio.nix and services/wifi.nix.
#
# The speaker-amp C helpers (gpioout/spkamp) are cross-compiled by
# pkgs/speaker-amp.nix and copied in here so the `speaker` CLI finds
# them next to itself.
#
# R10 (port delta, 2026-09-07 — see docs/mobile-nixos-port-feasibility.md
# §7 R10): the Debian-rootfs scripts shebang `#!/bin/bash`, but a NixOS
# stage-2 only creates `/bin/sh` (via `environment.binsh`), never
# `/bin/bash` — and systemd ExecStart execs the script directly (kernel
# resolves the `#!` line), so every bash-shebanged unit (gpu-poweron,
# a72-up/cl2-up, battery-guard, audio-defaults, backlight-default) would
# fail on glass with ENOENT (status=203). Rewrite bash shebangs to the
# store bash at package time (the NixOS-idiomatic patchShebangs
# equivalent). `#!/bin/sh` scripts keep their shebang: NixOS provides
# /bin/sh.
{ runCommand, callPackage, bash, ... }:

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
  # R10: rewrite `#!/bin/bash` and `#!/usr/bin/env bash` shebangs to the
  # store bash (the aarch64 one in this package set — the interpreter
  # that will exist in the closure). Line 1 only; leave #!/bin/sh alone.
  for f in $out/bin/*; do
    [ -f "$f" ] || continue
    first=$(head -n1 "$f")
    case "$first" in
      "#!/bin/bash"|"#!/usr/bin/env bash")
        sed -i "1c#!${bash}/bin/bash" "$f"
        ;;
    esac
  done
  # Speaker-amp helpers (aarch64, see pkgs/speaker-amp.nix):
  cp ${speakerAmp}/bin/gpioout $out/bin/
  cp ${speakerAmp}/bin/spkamp $out/bin/
  chmod +x $out/bin/*
''

# Gemini PDA userspace helpers, as a single package.
#
# The scripts are the verified bring-up utilities from the GeminiPDA
# project (build/gpu-poweron.sh, build/a72-bringup/cl2-up.sh,
# build/rootfs-files/{backlight,battery-guard,gemini-boot-recovery}),
# copied verbatim (plus gemini-wdt-reboot, a new device-side reboot
# helper — see the script's header). They are wired into the system as
# systemd services + CLIs by services/gemini-pda.nix.
{ runCommand, ... }:

runCommand "gemini-pda-utils" { } ''
  mkdir -p $out
  cp -r ${./scripts} $out/bin
  chmod +x $out/bin/*
''

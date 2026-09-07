# Speaker-amp helper binaries for the Gemini PDA (pkgs/speaker-amp/).
#
# gpioout: drives SoC pads 243/244 (the external speaker-amp enables)
#           through the kernel gpio chardev (gpiolib — proper request/
#           release, no raw register access).
# spkamp:  pinctrl-register diagnostic/exploration tool (/dev/mem, the
#          vendor AW8736-style enable pattern).
#
# The sources are the verified bring-up tools from the GeminiPDA project
# (build/rootfs-files/speaker-amp/), cross-compiled for aarch64. They are
# bundled into the gemini-pda-utils package by services/gemini-utils.nix
# and driven by the `speaker` CLI there (see also services/audio.nix:
# the amp is the only thing separating built-in-speaker from headphone
# output on this board).
{ stdenv, lib, ... }:

stdenv.mkDerivation rec {
  pname = "gemini-speaker-amp";
  version = "1.0";

  src = ./speaker-amp;

  # Cross stdenv: the gcc wrapper only exposes the prefixed binary name
  # (aarch64-unknown-linux-gnu-cc); there is no bare `cc` on PATH. This
  # repo is a single-device (aarch64) tree, so the prefix is explicit.
  ccCmd = "${stdenv.cc}/bin/aarch64-unknown-linux-gnu-cc";

  buildPhase = ''
    runHook preBuild
    ${ccCmd} -O2 -o gpioout gpioout.c
    ${ccCmd} -O2 -o spkamp spkamp.c
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    mkdir -p $out/bin
    install -m 755 gpioout spkamp $out/bin/
    runHook postInstall
  '';

  meta = with lib; {
    description = "Gemini PDA speaker-amp GPIO helpers (pads 243/244)";
    license = licenses.unlicense;
    platforms = [ "aarch64-linux" ];
  };
}

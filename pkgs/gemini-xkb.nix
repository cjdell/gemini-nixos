# gemini-xkb — the Gemini PDA xkb layout as an xkbcommon include dir.
#
# Source tree: config/xkb/ (symbols/gemini, vendored byte-identical from
# the sibling project — provenance + keysym facts in config/xkb/README.md;
# do not hand-edit, re-copy from GeminiPDA
# build/rootfs-files/xkb/symbols/gemini instead).
#
# Why a separate package: the desktop keymaps are compiled by xkbcommon
# (libxkbcommon) inside gemwl (pkgs/gemwl.nix) and the nested labwc
# (pkgs/labwc-geminipda.nix), and xkbcommon resolves symbols from its
# include path. The nixpkgs xkeyboard-config tree (the compiled-in
# XKB_CONFIG_ROOT) does not carry "gemini" — without it both compositors
# log XKB-338 and fall back to the default US keymap (Fn = Alt, no £).
# NixOS-idiomatic injection: xkbcommon honours the XKB_CONFIG_EXTRA_PATH
# env var (>= 1.0) — a colon list of extra include dirs searched before
# the system xkb tree. Each extra dir must look like the top of an xkb
# tree, i.e. contain symbols/…; rules/keycodes/types/compat keep
# resolving from the system tree. Point it at this package's $out.
#
# Consumers (the units that export XKB_CONFIG_EXTRA_PATH=$out):
#   services/desktop.nix  — gemwl.service (compiles layout "gemini" by
#                           default, gemwl.c xkb_rule_names)
#   services/lxqt.nix     — lxqt-nested.service (labwc 0.8.3 reads
#                           XKB_DEFAULT_LAYOUT; set to "gemini")
{ lib, stdenvNoCC }:

stdenvNoCC.mkDerivation {
  pname = "gemini-xkb";
  version = "2026-09-08"; # vendored from GeminiPDA on this date
  src = ../config/xkb;

  installPhase = ''
    mkdir -p $out
    cp -r $src/symbols $out/symbols
    # The layout README (config/xkb/README.md) rides along for
    # provenance in the store; only symbols/ is on xkbcommon's include
    # path.
    cp $src/README.md $out/README.md
  '';

  meta = {
    description = "Gemini PDA xkb symbols (layout gemini, UK default / us variant)";
    license = lib.licenses.mit; # xkeyboard-config heritage (Gemian fork)
    maintainers = [ ];
    platforms = lib.platforms.all;
  };
}

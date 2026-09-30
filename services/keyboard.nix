# Gemini PDA keyboard silkscreen: one option for every keymap consumer.
#
# The Gemini ships with regional keycap prints on the same 47-key matrix
# (config/xkb/symbols/gemini header). This repo was brought up on a UK
# unit, so everything defaulted to UK. A unit with another print sets
# `services.geminiKeyboard.variant`; the consumers read it:
#
#   - text console: config/gemini.nix console.keyMap
#     (config/keymaps/gemini-<variant>.map)
#   - GNOME: services/gnome.nix input source ("gemini" / "gemini+us")
#     and session XKB_DEFAULT_VARIANT; the variant is registered in
#     pkgs/gemini-xkeyboard-config.nix so gnome-shell accepts it
#
# Not covered: gemshell (services/gemshell.nix) keeps the default UK
# layout; its compositor passes the layout to xkbcommon explicitly and
# has no variant input yet.
#
# "uk" = the xkb `gemini` default ("basic") block, today's behaviour;
# "us" = xkb `gemini(us)` (Gemian's US silkscreen deltas).
# [added 2026-09-26; "us" verified on a US-print unit: text console,
# GNOME]
{ lib, ... }:

{
  options.services.geminiKeyboard = {
    variant = lib.mkOption {
      type = lib.types.enum [ "uk" "us" ];
      default = "uk";
      description = ''
        Keycap print of the built-in keyboard: "uk" (default) or "us".
        Selects the text-console keymap and the xkb variant of the
        `gemini` layout for GNOME (not gemshell, which stays UK).
      '';
    };
  };
}

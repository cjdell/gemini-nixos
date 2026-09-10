# GNOME Shell extension: never auto-show the on-screen keyboard.
#
# WHY: gnome-shell's KeyboardManager decides whether an OSK exists at all
# with (js/ui/keyboard.js, _syncEnabled()):
#
#     enabled = a11y('screen-keyboard-enabled')
#               || (seat.touch_mode && last-device-is-touchscreen)
#
# and mutter computes touch-mode as `!has_pointer` for a seat that has a
# touchscreen but NO tablet-mode switch (src/backends/native/
# meta-seat-impl.c, update_touch_mode()).  The Gemini has a touchscreen, a
# real keyboard and no pointer, so mutter reports touch_mode=true and the
# OSK pops up on every text focus even with the accessibility toggle off
# (a physical keypress does not help: the shell ignores keyboard devices
# in its last-device tracking).
#
# This extension forces lastDeviceIsTouchscreen() to false, killing exactly
# the auto path; the explicit accessibility OSK still works if turned on,
# so we also lock screen-keyboard-enabled=false in the system dconf DB
# (services/gnome.nix).  It uses a private KeyboardManager method, so
# re-check keyboard.js on a gnome-shell upgrade.
#
# On-device proof (2026-09-10, GNOME Shell 50.4): with the extension off,
# forcing the last-device state produced `touch_mode=true osk_object=CREATED`;
# with it on, `touch_mode=true osk_object=not-created`.
#
# The JS is plain files next to this expression (no build step).
{ lib, stdenvNoCC }:

stdenvNoCC.mkDerivation {
  pname = "gnome-shell-extension-no-osk";
  version = "1";

  src = ./.;

  strictDeps = true;
  dontConfigure = true;
  dontBuild = true;

  installPhase = ''
    runHook preInstall
    uuid=no-osk@gemini-nixos
    mkdir -p "$out/share/gnome-shell/extensions/$uuid"
    install -m644 metadata.json extension.js \
      "$out/share/gnome-shell/extensions/$uuid/"
    runHook postInstall
  '';

  passthru.extensionUuid = "no-osk@gemini-nixos";

  meta = with lib; {
    description = "Never auto-show the GNOME Shell on-screen keyboard (Gemini PDA)";
    platforms = platforms.linux;
  };
}

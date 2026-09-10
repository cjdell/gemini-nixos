# gemini-xkeyboard-config — the xkeyboard-config tree with the Gemini
# layout registered as a FIRST-CLASS layout.
#
# Why this exists on top of pkgs/gemini-xkb.nix:
#
#   XKB_CONFIG_EXTRA_PATH makes xkbcommon (the keymap *compiler*) find
#   symbols/gemini, but it does NOT make the layout visible to GNOME.
#   GNOME Shell chooses the layout through libgnome-desktop's XkbInfo,
#   which enumerates layouts with libxkbregistry (rxkb): it reads
#   rules/evdev.xml and looks the source id ("gemini") up in that
#   registry (gnome-desktop 44.5 libgnome-desktop/gnome-xkb-info.c:
#   rxkb_context_new(NO_FLAGS) + rxkb_context_parse(ctx, "evdev")).
#   rxkb does not read XKB_CONFIG_EXTRA_PATH, so with only gemini-xkb
#   the lookup fails and gnome-shell's KeyboardManager silently falls
#   back to DEFAULT_LAYOUT ('us') — mutter then compiles a US keymap no
#   matter what the gsettings input source says.  Verified on glass
#   2026-09-10: `xkbcli list` did not contain gemini, and the compositor
#   handed clients a plain two-level US keymap.
#
# Fix: copy the whole xkeyboard-config tree, (a) drop in symbols/gemini,
# (b) insert a <layout> entry into rules/evdev.xml, and point
# XKB_CONFIG_ROOT at $out/etc/X11/xkb.  Both xkbcommon and rxkb honour
# XKB_CONFIG_ROOT, so mutter and gnome-desktop agree on the same tree and
# the layout is both *selectable* (registry) and *compilable* (symbols).
#
# Verified with: XKB_CONFIG_ROOT=$(nix-build)/etc/X11/xkb xkbcli list
# -> "- layout: 'gemini'"; xkbcli compile-keymap --layout gemini
#    --model pc105 --rules evdev.
#
# Consumers: services/gnome.nix (XKB_CONFIG_ROOT in the session env).
{ lib, runCommand, xkeyboard-config, writeText }:

let
  registryEntry = writeText "gemini-evdev-layout.xml" ''
        <layout>
          <configItem>
            <name>gemini</name>
            <shortDescription>gem</shortDescription>
            <description>English (UK, Gemini PDA)</description>
            <countryList>
              <iso3166Id>GB</iso3166Id>
            </countryList>
            <languageList>
              <iso639Id>eng</iso639Id>
            </languageList>
          </configItem>
        </layout>
  '';
in
runCommand "gemini-xkeyboard-config-2026-09-10"
  {
    inherit registryEntry;
    meta = {
      description = "xkeyboard-config with the Gemini PDA layout registered (registry + symbols)";
      license = lib.licenses.mit; # xkeyboard-config heritage (Gemian fork)
      platforms = lib.platforms.all;
    };
  }
  ''
    # The real tree lives in share/X11/xkb; etc/X11/xkb is a symlink to it
    # (so cp -r would copy a dangling link — dereference instead).
    if [ -d "${xkeyboard-config}/share/X11/xkb" ]; then
      xkbsrc="${xkeyboard-config}/share/X11/xkb"
    else
      xkbsrc="${xkeyboard-config}/etc/X11/xkb"
    fi
    mkdir -p $out/etc/X11
    cp -rL "$xkbsrc" $out/etc/X11/xkb
    chmod -R u+w $out/etc/X11/xkb

    install -m 0644 ${../config/xkb/symbols/gemini} \
      $out/etc/X11/xkb/symbols/gemini

    ev=$out/etc/X11/xkb/rules/evdev.xml
    n=$(grep -n '</layoutList>' "$ev" | head -1 | cut -d: -f1)
    test -n "$n"
    head -n $((n - 1)) "$ev" > "$ev.new"
    cat "$registryEntry" >> "$ev.new"
    tail -n +$n "$ev" >> "$ev.new"
    mv "$ev.new" "$ev"

    # sanity: entry present exactly once, and the tree is complete
    test "$(grep -c '<name>gemini</name>' "$ev")" = 1
    test -f $out/etc/X11/xkb/symbols/gemini
  ''

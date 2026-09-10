# dosbox-x-gemini — DOSBox-X for the Gemini PDA with a usable keyboard.
#
# WHY a wrapper package (full story: docs/desktop-plumbing.md §DOSBox-X):
# DOSBox-X's SDL2 input path is scancode/position based, so it ignores the
# host `gemini` XKB layout and cannot see the Fn layer at all.  The wrapper
#   (a) selects DOSBox-X's own UK keyboard table (the Gemini is a
#       UK-silkscreen unit; DOSBox-X defaults to US), and
#   (b) seeds a complete mapper file that binds RALT (= Fn, mapper mod2) +
#       the number row to the DOS F1..F10 events.
#
# The mapper is all-or-nothing (DOSBox-X loads whatever mapper file it finds
# INSTEAD of its built-in defaults), so config/dosbox-x/mapper-dosbox-x.map
# reproduces the full default binding set plus the Fn additions.  Regenerate
# it with bin/gen-dosbox-x-mapper.sh when the flake's dosbox-x pin moves.
#
# The upstream output is symlink-joined so the .desktop/metainfo entries
# still ship; only bin/dosbox-x is replaced by pkgs/dosbox-x-gemini.sh.
{ lib, symlinkJoin, dosbox-x }:

symlinkJoin {
  name = "dosbox-x-gemini";
  paths = [ dosbox-x ];
  postBuild = ''
    rm -f $out/bin/dosbox-x
    cp ${./dosbox-x-gemini.sh} $out/bin/dosbox-x
    substituteInPlace $out/bin/dosbox-x \
      --replace-fail '@DOSBOX_X@' '${lib.getExe dosbox-x}' \
      --replace-fail '@MAPPER@' '${../config/dosbox-x/mapper-dosbox-x.map}'
    chmod 755 $out/bin/dosbox-x
  '';
}

#!/bin/sh
# gen-dosbox-x-mapper.sh — generate the Gemini PDA DOSBox-X mapper file.
#
# WHY: DOSBox-X is an SDL2 app whose input path is SCANCODE/position based
# (src/gui/sdl_mapper.cpp: CKeyBindGroup::CheckEvent/CreateEventBind use
# event->key.keysym.scancode; useScanCode() is hardwired false for SDL2).
# It therefore ignores the host's "gemini" XKB layout entirely and cannot see
# the Fn layer.  The fix is DOSBox-X's own key mapper.  See
# docs/desktop-plumbing.md §DOSBox-X for the full story and receipts.
#
# The mapper file is ALL-OR-NOTHING: if any mapper file is found DOSBox-X
# loads it INSTEAD of the built-in defaults (MAPPER_Init ->
# if (!MAPPER_LoadBinds()) CreateDefaultBinds();).  So this script reproduces
# the complete default binding set from the pinned dosbox-x source
# (DefaultKeys[] in src/gui/sdl_mapper.cpp, the SDL2 branch) and adds the
# Gemini overlay.
#
# GEMINI OVERLAY (all numbers are SDL scancodes of the HOST key):
#   * key_ralt is DROPPED.  Fn is kernel KEY_RIGHTALT; the default map turns
#     that into guest right-Alt, and a held guest Alt makes the guest keyboard
#     layout skip its normal/shift planes (layout_key: AltGr planes only, and
#     the FreeDOS UK file has almost none) — so Fn+key produced wrong/no
#     characters.  The Gemini has a SEPARATE physical Alt (KEY_LEFTALT, matrix
#     (4,1)), still bound via key_lalt, so dropping the RALT bind costs nothing.
#   * Each Fn combination is bound with mod2 (host RALT) to the guest key whose
#     UK-layout character is the symbol the gemini XKB layout puts on level 3
#     (config/xkb/symbols/gemini).  Characters that sit on the guest layout's
#     shift plane also get a key_lshift bind emitted BEFORE the character bind
#     (activation is in file order, so Shift is down first).
#   * F1..F10 are the gemini XKB *level 4* (Shift+Fn) of the number row, so
#     they are bound with mod2+mod3 to the F-key events.
#   * Fn+C/V drive DOSBox-X's own mixer volume (volup/voldown); the remaining
#     Fn+letter combos are media/brightness keys with no DOS equivalent and are
#     left unmapped (they fall through to the plain letter — documented).
#
# USAGE:
#   bin/gen-dosbox-x-mapper.sh <sdl_mapper.cpp> <SDL_scancode.h> [outfile]
#
# Both inputs must come from the same dosbox-x / SDL2 the device uses:
#   $ nix build --no-link --print-out-paths nixpkgs#dosbox-x.src
#   $ nix build --no-link --print-out-paths nixpkgs#SDL2.dev
# The checked-in config/dosbox-x/mapper-dosbox-x.map was generated for
# dosbox-x 2026.08.02 (the flake pin), 2026-09-11.
#
# Output goes to stdout if outfile is omitted or "-".
set -eu

SRC=${1:?usage: gen-dosbox-x-mapper.sh <sdl_mapper.cpp> <SDL_scancode.h> [outfile]}
HDR=${2:?usage: gen-dosbox-x-mapper.sh <sdl_mapper.cpp> <SDL_scancode.h> [outfile]}

[ -f "$SRC" ] || { echo "gen-dosbox-x-mapper: no such file: $SRC" >&2; exit 1; }
[ -f "$HDR" ] || { echo "gen-dosbox-x-mapper: no such file: $HDR" >&2; exit 1; }

case "${3:-}" in
  ""|-|/dev/stdout) exec > /dev/stdout ;;
  *)                exec > "$3" ;;
esac

awk '
  # ---------------------------------------------------------------- pass 1
  # SDL_scancode.h: SDL_SCANCODE_X = N  ->  name[N]
  NR == FNR {
    line = $0
    while (match(line, /SDL_SCANCODE_[A-Z0-9_]+[ \t]*=[ \t]*[0-9]+/)) {
      s = substr(line, RSTART, RLENGTH)
      line = substr(line, RSTART + RLENGTH)
      split(s, a, /[ \t]*=[ \t]*/)
      val[a[1]] = a[2]
    }
    next
  }

  # ---------------------------------------------------------------- pass 2
  # src/gui/sdl_mapper.cpp: the SDL2 DefaultKeys[] table.
  /^static struct \{/ { maybe = 1 }
  maybe && /DefaultKeys\[\]=\{/ { in_keys = 1; maybe = 0; next }
  in_keys && /^#else/         { exit }
  in_keys && /\{nullptr, 0\}/ { exit }
  in_keys {
    line = $0
    while (match(line, /\{"[a-z0-9_]+",[ \t]*SDL_SCANCODE_[A-Z0-9_]+\}/)) {
      e    = substr(line, RSTART, RLENGTH)
      line = substr(line, RSTART + RLENGTH)
      gsub(/[{]/, "", e)
      gsub(/[}]/, "", e)
      gsub(/"/, "", e)
      split(e, p, /,[ \t]*/)
      name = p[1]; sym = p[2]
      if (!(name in seen) && (sym in val)) {
        seen[name] = 1
        order[++n] = name
        code[name]  = val[sym]
      }
    }
  }

  # ---------------------------------------------------------------- output
  END {
    # NB: the section must be SDL_STRING = "SDL2" for SDL2 builds
    # (include/shell.h); "sdl" makes MAPPER_LoadBinds() silently discard
    # every line and fall back to the built-in defaults.
    print "[SDL2]"

    # Default binding set.  key_ralt is deliberately omitted (see header).
    for (i = 1; i <= n; i++) {
      name = order[i]
      if (name == "ralt") continue
      printf "key_%s \"key %s\"\n", name, code[name]
    }

    # Default modifier + host binds (CreateDefaultBinds, SDL2 branch).
    print "mod_1 \"key 224\""   # RCTRL
    print "mod_1 \"key 228\""   # LCTRL
    print "mod_2 \"key 230\""   # RALT (== the Gemini Fn key)
    print "mod_2 \"key 226\""   # LALT (== the Gemini physical Alt)
    print "mod_3 \"key 229\""   # RSHIFT
    print "mod_3 \"key 225\""   # LSHIFT
    print "host \"key 69\""     # F12

    # ---- Gemini Fn overlay: shift synthesis first (file order ==
    # ---- activation order, so guest Shift is already down), then symbols.
    # host-key  guest-event        needs-shift  comment
    ov = "\
30 key_lessthan 1 |\n\
31 key_backslash 0 #\n\
32 key_lessthan 0 backslash\n\
34 key_comma 1 <\n\
35 key_period 1 >\n\
36 key_leftbracket 0 [\n\
37 key_rightbracket 0 ]\n\
38 key_leftbracket 1 {\n\
39 key_rightbracket 1 }\n\
12 key_equals 1 +\n\
18 key_minus 0 -\n\
19 key_equals 0 =\n\
13 key_minus 1 _\n\
14 key_quote 1 @\n\
15 key_semicolon 0 ;\n\
16 key_grave 0 grave\n\
52 key_semicolon 1 :\n\
28 key_left 0 leftarrow\n\
24 key_down 0 downarrow\n\
21 key_printscreen 0 print"

    n_ov = split(ov, rows, "\n")
    for (i = 1; i <= n_ov; i++) {
      split(rows[i], f, " ")
      if (f[3] == 1) printf "key_lshift \"key %s mod2\"\n", f[1]
    }
    for (i = 1; i <= n_ov; i++) {
      split(rows[i], f, " ")
      printf "%s \"key %s mod2\"\n", f[2], f[1]
    }

    # Shift+Fn + number row -> F1..F10 (gemini XKB level 4).  SDL: 1=30..0=39.
    for (i = 1; i <= 10; i++) {
      base = (i == 10) ? 39 : (30 + i - 1)
      printf "key_f%d \"key %d mod2 mod3\"\n", i, base
    }

    # Fn+C/V -> DOSBox-X mixer volume (the gemini XKB puts XF86Audio
    # Lower/RaiseVolume on level 3 of c/v).
    print "voldown \"key 6 mod2\""
    print "volup \"key 25 mod2\""

    # Shift+Fn + y/u -> right/up arrows (gemini XKB level 4).
    print "key_right \"key 28 mod2 mod3\""
    print "key_up \"key 24 mod2 mod3\""
  }
' "$HDR" "$SRC"

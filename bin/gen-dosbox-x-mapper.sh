#!/bin/sh
# gen-dosbox-x-mapper.sh — generate the Gemini PDA DOSBox-X mapper file.
#
# WHY: DOSBox-X is an SDL2 app whose input path is SCANCODE/position based
# (src/gui/sdl_mapper.cpp: CKeyBindGroup::CheckEvent/CreateEventBind use
# event->key.keysym.scancode; useScanCode() is hardwired false for SDL2).
# It therefore ignores the host's "gemini" XKB layout entirely and never sees
# the Fn layer — Fn is kernel KEY_RIGHTALT -> ISO_Level3_Shift, so Fn+1..0 is
# just the "1" key with the RALT modifier held, not F1..F10.  The only
# app-local fix is DOSBox-X's own key mapper: bind each Fn combination
# (mod2 = Alt/RALT + base scancode) to the DOS F-key event.  See
# docs/desktop-plumbing.md §DOSBox-X.
#
# The mapper file is ALL-OR-NOTHING: if any mapper file is found DOSBox-X
# loads it INSTEAD of the built-in defaults (MAPPER_Init ->
# if (!MAPPER_LoadBinds()) CreateDefaultBinds();).  So the generated file must
# reproduce the complete default binding set, not just the Fn additions.
# This script derives that set from the pinned dosbox-x source
# (DefaultKeys[] in src/gui/sdl_mapper.cpp, the SDL2 branch) and the SDL2
# scancode enum.
#
# USAGE:
#   bin/gen-dosbox-x-mapper.sh <sdl_mapper.cpp> <SDL_scancode.h> [outfile]
#
# Get the two inputs from the same dosbox-x / SDL2 the device uses (the flake
# pin dosbox-x 2026.08.02 + its SDL2):
#   $ nix build --no-link --print-out-paths nixpkgs#dosbox-x.src
#   $ nix build --no-link --print-out-paths nixpkgs#SDL2.dev
#   # <src>/src/gui/sdl_mapper.cpp and <dev>/include/SDL2/SDL_scancode.h
#
# Output goes to stdout if outfile is omitted or "-".
set -eu

SRC=${1:?usage: gen-dosbox-x-mapper.sh <sdl_mapper.cpp> <SDL_scancode.h> [outfile]}
HDR=${2:?usage: gen-dosbox-x-mapper.sh <sdl_mapper.cpp> <SDL_scancode.h> [outfile]}

[ -f "$SRC" ] || { echo "gen-dosbox-x-mapper: no such file: $SRC" >&2; exit 1; }
[ -f "$HDR" ] || { echo "gen-dosbox-x-mapper: no such file: $HDR" >&2; exit 1; }

# OUT "-" or empty => stdout.
case "${3:-}" in
  ""|-|/dev/stdout) exec > /dev/stdout ;;
  *)                exec > "$3" ;;
esac

awk '
  # ---- pass 1: SDL_scancode.h  name -> numeric value ----------------
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

  # ---- pass 2: sdl_mapper.cpp  SDL2 DefaultKeys[] table -------------
  /^static struct \{/ { maybe = 1 }
  maybe && /DefaultKeys\[\]=\{/ { in_keys = 1; maybe = 0; next }

  in_keys && /^#else/        { exit }
  in_keys && /\{nullptr, 0\}/ { exit }

  in_keys {
    line = $0
    while (match(line, /\{"[a-z0-9_]+",[ \t]*SDL_SCANCODE_[A-Z0-9_]+\}/)) {
      e    = substr(line, RSTART, RLENGTH)
      line = substr(line, RSTART + RLENGTH)
      gsub(/[{}]/, "", e)
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

  END {
    # NB: the section is SDL_STRING, which is "SDL2" for SDL2 builds
    # (src/include/shell.h) — NOT "sdl".  A wrong section name makes
    # MAPPER_LoadBinds() discard every line and silently fall back to the
    # built-in defaults.
    print "[SDL2]"
    for (i = 1; i <= n; i++) {
      name = order[i]; c = code[name]
      extra = ""
      # Fn (RALT == mod2) + number row -> F1..F10.  SDL_SCANCODE_1 == 30.
      if (name ~ /^f([1-9]|10)$/) {
        num  = substr(name, 2) + 0
        base = (num == 10) ? 39 : (30 + num - 1)
        extra = " \"key " base " mod2\""
      }
      printf "key_%s \"key %s\"%s\n", name, c, extra
    }
    # Default modifier + host binds (CreateDefaultBinds, SDL2 branch).
    print "mod_1 \"key 224\""   # RCTRL
    print "mod_1 \"key 228\""   # LCTRL
    print "mod_2 \"key 230\""   # RALT (== the Gemini Fn key)
    print "mod_2 \"key 226\""   # LALT
    print "mod_3 \"key 229\""   # RSHIFT
    print "mod_3 \"key 225\""   # LSHIFT
    print "host \"key 69\""     # F12
  }
' "$HDR" "$SRC"

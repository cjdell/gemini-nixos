# config/xkb — Gemini PDA xkb symbols (layout "gemini")

Vendored **byte-identical** from the sibling project:

- Source: `/home/cjdell/Projects/GeminiPDA/build/rootfs-files/xkb/symbols/gemini`
  (5,937 bytes; sha256 `f56fbab83176cc105db14c145ebca31a806e8d186291fbc782a7dace0562d4f8`;
  adapted from Gemian's xkeyboard-config fork — keysym facts UK-verified
  2026-09-04 in its header comment).
- Copied: 2026-09-08 (keyboard-mapping session; see `docs/session-log.md`).
- No byte changes — do not hand-edit this file; change the source in the
  sibling repo and re-copy (same rule as `config/keymaps/`).

## What it is / why it exists

The Gemini's 47-key matrix (AW9523, gpio-matrix-keypad) does not match any
stock PC layout: `? ~ @ ; £ …` live on shifted/Fn layers that differ per
regional silkscreen. UK unit facts (verified 2026-09-04):

- Fn key = matrix (4,3) → kernel `KEY_RIGHTALT` → `<RALT>` → level3 (the
  Fn layer) via `level3(ralt_switch)`.
- `Fn+K` = `@` (K = `<AC08>`), `Fn+L` = `;` (`<AC09>`), `shift+3` = `£`
  (UK default block; `<AE03>` level2 = sterling).
- Kernel (5,0) = `KEY_APOSTROPHE` → `<AC11>` = apostrophe/asciitilde/colon.

Layout name **`gemini`** (no variant) = UK default; variant `us` = US
silkscreen deltas.

## NixOS side — how the desktop consumes it

`console.keyMap` (VT) is a separate map (`config/keymaps/gemini-uk.map`).
The **desktop** (gemwl + nested labwc/LXQt) compiles its keymap with
xkbcommon from rules — layout `gemini` — so `symbols/gemini` must be on
xkbcommon's include path:

- `pkgs/gemini-xkb.nix` packages this tree (`$out/symbols/gemini`).
- `services/desktop.nix` (gemwl) + `services/lxqt.nix` (labwc) set
  `XKB_CONFIG_EXTRA_PATH=<that store dir>`; labwc additionally gets
  `XKB_DEFAULT_LAYOUT=gemini`.

Without it xkbcommon logs `XKB-338 Couldn't find file "symbols/gemini"`
and both compositors fall back to the default (US) keymap — Fn behaves as
Alt (`<RALT>` = Alt_R, no level3) and `shift+3` = `#` instead of `£`
(observed on glass gen9, 2026-09-08). Verify on glass:
`xkbcli compile-keymap --layout gemini` (if `xkbcli` is on the device) or
a typing test; journal `xkbcommon: ERROR` absence in `-u gemwl` /
`-u lxqt-nested` and labwc's "Found layout Gemini English (UK)" line.

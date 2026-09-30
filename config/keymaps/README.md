# gemini-uk.map — Gemini PDA built-in keyboard layout (UK base + Fn layer)

Vendored **verbatim** from the sibling project:

- Source: `/home/cjdell/Projects/GeminiPDA/build/rootfs-files/keyboard/gemini-uk.map`
  (438,457 bytes; kbd text format, header `keymaps 0-127`; the file the
  verified Debian rootfs compiles with busybox `loadkmap` into
  `/etc/gemini.bkmap` for `gemini-keymap.service`).
- Copied: 2026-09-08 (session working the `outstanding.md` list, item 3).
- Provenance of the layout itself: GeminiPDA project (author: the bring-up
  work; see that repo's docs). No byte changes — do not hand-edit this
  file; change the source in the sibling repo and re-copy.

NixOS side: `config/gemini.nix` sets `console.keyMap = ./keymaps/gemini-uk.map`.
NixOS accepts the store path; the console module writes `KEYMAP=<path>` into
`/etc/vconsole.conf` and systemd-vconsole-setup runs kbd's `loadkeys` on it
(no busybox .bkmap binary needed). Sanity check:
`loadkeys --validate gemini-uk.map` (kbd) passes.

# gemini-us.map — US keycap print (added 2026-09-26)

Not vendored: derived from `gemini-uk.map` above by changing only the
keys where it differs from the xkb `gemini(us)` variant
(`config/xkb/symbols/gemini`, Gemian's US deltas), plus Fn+4:

| Key (kernel keycode) | gemini-uk.map (plain / Shift / Fn) | gemini-us.map |
| --- | --- | --- |
| 3 (4) | `3` / `£` / `\` | `3` / `#` / `£` |
| 4 (5) | `4` / `$` / `$` | `4` / `$` / `€` (`U+20AC`) |
| K (37) | Fn `@` | Fn `;` |
| L (38) | Fn `;` | Fn `"` |
| key left of Enter (40) | `'` / `~` / `:` | `\` / `\|` / `:` (Alt: `Meta_backslash`) |

Keys 1, 2 and M already have the US assignments in `gemini-uk.map`
(Shift+2 = `@`, Fn+1 = `~`, Fn+2 = `` ` ``, Fn+M = `'`) and are
unchanged. Everything else is byte-identical to `gemini-uk.map`
(`diff` shows only those lines).

Selected by `services.geminiKeyboard.variant = "us"` (services/keyboard.nix).
Checked on a US-print unit 2026-09-26: `loadkeys --parse` passes; loaded
at runtime, every Shift/Fn symbol matches the keycaps. The kernel's
built-in console font (TER16x32) has no `€` glyph and draws a box; the
key does produce U+20AC.

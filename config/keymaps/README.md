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

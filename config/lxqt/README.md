# config/lxqt — LXQt session configs (seeded to /home/cjdell on first boot (the cjdell desktop session, 2026-09-09))

The LXQt 2.x (Wayland) desktop nested inside gemwl (services/lxqt.nix +
services/scripts/start-lxqt-nested) seeds these to `$HOME` = `/home/cjdell` **if absent**
(never overwrites user changes):

| File here | Seeded to | Purpose |
|---|---|---|
| `lxqt.conf` | `/home/cjdell/.config/lxqt/lxqt.conf` | LXQt look (theme=Clearlooks from lxqt-themes), **icon_theme=Papirus** (the nixpkgs pin has no breeze-icons; missing icon theme = blank panel buttons — 2026-09-04 Debian receipt), Qt font/style |
| `session.conf` | `/home/cjdell/.config/lxqt/session.conf` | lxqt-session defaults (ported from lxqt-session 2.4.0 config/session.conf; [Environment] is exported to the session's children) |
| `labwc-rc.xml` | `/home/cjdell/.config/labwc/rc.xml` | labwc config (ported from the verified GeminiPDA lxqt-wayland/labwc-rc.xml): server decorations, fonts, window rules forcing SSD for non-Qt GL clients (wlegltst/tinytest/weston-*) |
| `labwc-autostart` | `/home/cjdell/.config/labwc/autostart` | runs at labwc start — only `qterminal` (first visible Qt/EGL client). The LXQt panel/desktop/polkit/notificationd are NOT here: lxqt-session assembles them from the XDG autostart entries in the lxqt packages' `etc/xdg` (the lxqt-nested unit puts those dirs on XDG_CONFIG_DIRS — see services/lxqt.nix) |
| `themerc` | `/home/cjdell/.local/share/themes/Gemini/openbox-3/themerc` | the vendored labwc openbox-3 theme ("Gemini", Clearlooks-style). labwc reads `<xdg-data>/themes/<name>/openbox-3/themerc` (src/common/dir.c); a missing theme = no titlebars (2026-09-04 receipt — the Debian config named uninstalled "Vent") |
| `Desktop/*.desktop` | `/home/cjdell/Desktop/` | pcmanfm-qt desktop launchers (Terminal / File Manager / LXQt Settings / Audio Volume); ported from the sibling deploy-lxqt-launchers.sh |

Sources: labwc-rc.xml/labwc-autostart are ports of the verified GeminiPDA
`build/rootfs-files/lxqt-wayland/` files (byte-behaviour preserved);
lxqt.conf/session.conf follow the lxqt-session 2.4.0 upstream defaults
with the Papirus icon-theme delta. [2026-09-07]

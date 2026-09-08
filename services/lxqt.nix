# LXQt 2.x (Wayland) desktop nested inside gemwl — the NixOS port of the
# verified GeminiPDA lxqt-nested stack (build/rootfs-files/lxqt-wayland/,
# on glass since 2026-09-02; live LXQt audio GUI on #329 2026-09-07):
#
#   LXQt apps (lxqt-panel, pcmanfm-qt --desktop, qterminal, pavucontrol-qt)
#     -> labwc 0.8.3 (wlroots 0.18 "wayland" backend, nested on gemwl's
#                     wayland-0, exports wayland-1)
#       -> gemwl (services/desktop.nix; the wlroots-0.18 compositor that
#                 owns the LK framebuffer /dev/gemfb, GPU-direct)
#         -> LK framebuffer -> panel
#
# Labwc is pkgs/labwc-geminipda.nix (0.8.3 pinned against the repo's
# wlroots 0.18.2 — the verified version pair; nixpkgs floats labwc
# 0.20/wlroots 0.20, unverified on this hardware). LXQt itself comes from
# the pinned nixpkgs lxqt scope (2.4.x on Qt 6.11 — newer than Debian's
# 2.1 but the same Qt6 line; deltas noted in the session log).
#
# HOW LXQt assembles itself (lxqt-session 2.4 source, verified
# 2026-09-07): lxqt-session does NOT hardcode a module list — it runs
# the XDG autostart desktop files it can see (OnlyShowIn=LXQt) plus the
# session's own config. The panel / desktop / polkit-agent /
# notificationd autostart entries are shipped by their packages under
# $out/etc/xdg/autostart. Debian merges them into /etc/xdg; NixOS has no
# merged /etc/xdg, so this unit puts every LXQt package's etc/xdg on
# XDG_CONFIG_DIRS (and every share on XDG_DATA_DIRS for menus/icons).
# Nothing LXQt-ish is started from labwc's autostart (config/lxqt/
# labwc-autostart only starts qterminal, matching the Debian file).
#
# Session env notes (receipts from the Debian units + 2026-09-04
# session-config bug): this is a SYSTEM service — no HOME/XDG_* exist by
# default, and Qt's QSettings would fall back to /etc/xdg (the panel then
# logs "Icon Theme not set" and renders blank). The full user-ish env is
# set here (HOME=/root + XDG_CONFIG/CACHE/DATA_HOME), like the Debian
# start script. Qt plugin discovery: every Qt binary is wrapped by
# nixpkgs with --prefix QT_PLUGIN_PATH (its closure's plugin dirs), and
# Qt6 plugins live under $out/lib/qt-6/plugins (qtbase-setup-hook
# qtPluginPrefix). The unit's QT_PLUGIN_PATH adds the shared plugin dirs
# (qtbase, qtwayland — the wayland platform plugin qterminal/pcmanfm-qt/
# pavucontrol-qt need — qtsvg, lxqt-qtplugin's platform theme) for apps
# whose own closure does not carry them.
#
# The desktop is enabled by default (services.lxqtNested.enable). To boot
# to the console only: `systemctl disable gemwl lxqt-nested` (gemwl
# without a session client shows a black-but-live framebuffer; lxqt-nested
# without gemwl fails on the missing socket and restart-loops).
{ config, lib, pkgs, ... }:

let
  cfg = config.services.lxqtNested;

  # Same callPackage args as the flake's package outputs + desktop.nix,
  # so the system closure shares one store path per package. labwc needs
  # the DRM-BACKEND-enabled wlroots build (0.8.3 compiles wlr_drm_lease_v1
  # unconditionally; wlroots only installs its header with the drm backend
  # on — pkgs/wlroots-geminipda.nix header). Runtime is unaffected: labwc
  # runs nested (WLR_BACKENDS=wayland) and never opens a DRM device.
  mesaGeminipda = pkgs.callPackage ../pkgs/mesa-geminipda.nix { };
  wlroots = pkgs.callPackage ../pkgs/wlroots-geminipda.nix {
    inherit mesaGeminipda;
  };
  wlrootsDrm = pkgs.callPackage ../pkgs/wlroots-geminipda.nix {
    inherit mesaGeminipda;
    withDrmBackend = true;
  };
  labwc = pkgs.callPackage ../pkgs/labwc-geminipda.nix { wlroots = wlrootsDrm; };
  lxqt = pkgs.lxqt;
  qt = pkgs.kdePackages;
  utils = pkgs.callPackage ./gemini-utils.nix { };
  # Gemini xkb symbols (layout "gemini") as an xkbcommon include dir
  # (config/xkb/symbols/gemini; see config/xkb/README.md). labwc 0.8.3
  # compiles its keymap from XKB_DEFAULT_LAYOUT (src/input/keyboard.c
  # set_layout); without symbols/gemini on the include path it logs XKB-
  # 338 and falls back to US (Fn = Alt, shift+3 = #, no £/@ Fn layer).
  geminiXkb = pkgs.callPackage ../pkgs/gemini-xkb.nix { };

  # The LXQt apps that make up the session (nixpkgs lxqt scope). Every
  # binary here is also on the unit PATH so autostart Exec= names and the
  # desktop launchers resolve.
  lxqtApps = [
    lxqt.lxqt-session # labwc -S lxqt-session (the session client)
    lxqt.lxqt-panel # XDG autostart -> layer-shell panel
    lxqt.pcmanfm-qt # XDG autostart -> --desktop (wallpaper + icons)
    lxqt.lxqt-policykit # XDG autostart -> polkit agent
    lxqt.lxqt-notificationd # XDG autostart -> notifications
    lxqt.lxqt-config # LXQt Settings (desktop launcher)
    lxqt.lxqt-globalkeys
    lxqt.lxqt-runner
    lxqt.qterminal # first visible client (labwc autostart)
    lxqt.pavucontrol-qt # audio volume GUI (PipeWire/Pulse)
    lxqt.lxqt-qtplugin # the LXQt Qt platform theme (QT_QPA_PLATFORMTHEME)
    lxqt.lxqt-themes # the Clearlooks look (lxqt.conf theme=)
  ];

  qtMods = [ qt.qtbase qt.qtwayland qt.qtsvg ];

  # The repo's config/lxqt/ tree as one read-only store dir, laid out
  # for services/scripts/start-lxqt-nested to seed to /root (idempotent).
  # NOTE (2026-09-08, first on-glass run): the seed layout is FLAT —
  # $out/lxqt/{lxqt.conf,session.conf}, $out/labwc/{rc.xml,autostart},
  # $out/themerc, $out/Desktop/. The earlier `cp ${file} ${file} $out/d`
  # multi-source copies kept the store basenames (<hash>-lxqt.conf …),
  # so start-lxqt-nested's seed failed ("cannot stat …/lxqt/lxqt.conf")
  # and lxqt-nested.service restart-looped. cp each file to its exact
  # target name.
  sessionConfig = pkgs.runCommand "gemini-lxqt-config" { } ''
    mkdir -p $out/lxqt $out/labwc $out/Desktop
    cp ${../config/lxqt/lxqt.conf} $out/lxqt/lxqt.conf
    cp ${../config/lxqt/session.conf} $out/lxqt/session.conf
    cp ${../config/lxqt/labwc-rc.xml} $out/labwc/rc.xml
    cp ${../config/lxqt/labwc-autostart} $out/labwc/autostart
    cp ${../config/lxqt/themerc} $out/themerc
    cp ${../config/lxqt/Desktop}/* $out/Desktop/
  '';
in
{
  options.services.lxqtNested.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = ''
      Enable the LXQt (Wayland) desktop nested inside gemwl (labwc 0.8.3
      on wlroots 0.18.2). Disable with gemwl for a console-only boot.
    '';
  };

  config = lib.mkIf cfg.enable {
    # The LXQt apps + supporting bits on the device PATH (the binaries
    # themselves; the session env is set per-unit below — this is for
    # interactive/ssh use and the /run/current-system/sw profile).
    environment.systemPackages = [
      labwc
      lxqt.lxqt-session
      lxqt.lxqt-panel
      lxqt.pcmanfm-qt
      lxqt.qterminal
      lxqt.pavucontrol-qt
      lxqt.lxqt-config
      qt.qtwayland # wayland platform plugin (for ad-hoc Qt clients)
      pkgs.papirus-icon-theme # icon_theme=Papirus in lxqt.conf
      pkgs.wlr-randr
      pkgs.qpwgraph # PipeWire graph GUI (audio.nix's stack)
      pkgs.dejavu_fonts # UI fonts (fontconfig defaults are DejaVu)
      pkgs.dbus # dbus-daemon + dbus-update-activation-environment
      utils # start-lxqt-nested + the gemini CLIs
    ];

    # polkitd on the SYSTEM bus (the lxqt-policykit-agent registers
    # against it); the session bus is started by start-lxqt-nested.
    services.dbus.enable = lib.mkDefault true;
    security.polkit.enable = lib.mkDefault true;

    # UI fonts reachable through fontconfig (fonts.fontconfig.enable
    # defaults true; this list is what it points at).
    fonts.packages = [ pkgs.dejavu_fonts ];

    systemd.services.lxqt-nested = {
      description = "LXQt (Wayland) session nested on the gemwl compositor (labwc)";
      after = [ "gemwl.service" ];
      wants = [ "gemwl.service" ];
      wantedBy = [ "multi-user.target" ];

      # PATH for the whole session (children inherit it): the LXQt
      # binaries (autostart Exec= + desktop launcher Exec= resolve by
      # bare name, like Debian's merged /etc/xdg + /usr/bin), labwc,
      # dbus + wlr-randr. nixpkgs appends a default path (coreutils,
      # findutils, gnugrep, gnused, systemd) after this list.
      path = lxqtApps ++ [ labwc pkgs.dbus pkgs.wlr-randr ];

      serviceConfig = {
        Type = "simple";
        ExecStart = "${utils}/bin/start-lxqt-nested";
        # The whole stack is inside this unit; a labwc/lxqt-session
        # failure brings it down -> restart (mirrors the Debian unit).
        Restart = "on-failure";
        RestartSec = "5";
        # The session env (see the header comment — systemd system
        # services get no HOME/XDG_*/DBUS by default).
        Environment = [
          # Runtime dirs + sockets (gemwl.service's RuntimeDirectory
          # hosts the wayland + dbus sockets; audio.nix owns
          # /run/gemwl-audio — never put the sound server under /run/
          # gemwl: it is DELETED when gemwl restarts [2026-09-07
          # receipt]).
          "XDG_RUNTIME_DIR=/run/gemwl"
          "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/gemwl/bus"
          "PULSE_SERVER=unix:/run/gemwl-audio/pulse/native"
          "PIPEWIRE_RUNTIME_DIR=/run/gemwl-audio"
          # User-ish env so Qt config/icon/cache resolution works
          # (HOME=/root + XDG_*; see the header).
          "HOME=/root"
          "XDG_CONFIG_HOME=/root/.config"
          "XDG_CACHE_HOME=/root/.cache"
          "XDG_DATA_HOME=/root/.local/share"
          # XDG data dirs: icon theme (Papirus), LXQt themes (Clearlooks
          # look), every LXQt package's share (menus/desktop files) and
          # the system profile (systemPackages). The vendored labwc
          # theme is seeded to /root/.local/share/themes (XDG_DATA_HOME
          # above), which labwc scans first.
          "XDG_DATA_DIRS=${lib.makeSearchPath "share" (lxqtApps ++ [ pkgs.papirus-icon-theme ])}:/run/current-system/sw/share"
          # XDG config dirs: the LXQt packages' autostart .desktop files
          # ($out/etc/xdg/autostart — the panel/desktop/polkit/
          # notificationd autostart lxqt-session scans) + the system
          # profile. (User configs live in XDG_CONFIG_HOME=/root/.config.)
          "XDG_CONFIG_DIRS=${lib.makeSearchPath "etc/xdg" lxqtApps}:/run/current-system/sw/etc/xdg"
          # Qt plugin discovery: qtbase + the wayland platform plugin
          # (qtwayland), qtsvg (svg icons), the LXQt platform theme
          # (lxqt-qtplugin). Wrapped Qt binaries prepend their own
          # closure plugin dirs to this value (--prefix).
          "QT_PLUGIN_PATH=${lib.makeSearchPath "lib/qt-6/plugins" (qtMods ++ [ lxqt.lxqt-qtplugin ])}"
          # Session identity (LXQt-only autostart entries match on
          # OnlyShowIn=LXQt; lxqt-session skips X-LXQt-X11-Only on
          # wayland).
          "XDG_SESSION_TYPE=wayland"
          "XDG_CURRENT_DESKTOP=LXQt:labwc:wlroots"
          "XDG_MENU_PREFIX=lxqt-"
          "QT_QPA_PLATFORMTHEME=lxqt"
          "QT_AUTO_SCREEN_SCALE_FACTOR=0"
          "QT_ACCESSIBILITY=1"
          # AFBC readback workaround (must stay; same as gemwl).
          "PAN_MESA_DEBUG=noafbc"
          # Gemini keyboard layout for the labwc keymap: xkbcommon
          # include path for symbols/gemini + the layout name itself.
          # Both are read by xkbcommon when labwc builds its keymap
          # (XKB_DEFAULT_LAYOUT, src/input/keyboard.c set_layout).
          # Without them: XKB-338 + US fallback (observed on glass
          # gen9: labwc "Found layout English (US)"). [2026-09-08]
          "XKB_CONFIG_EXTRA_PATH=${geminiXkb}"
          "XKB_DEFAULT_LAYOUT=gemini"
          # Terminal apps in the session (qterminal) spawn $SHELL; a
          # systemd system service has no SHELL env, and without it
          # qterminal falls back to /bin/sh (bash sh-mode). Match the
          # accounts' login shell (users.defaultUserShell, config/
          # gemini.nix). [2026-09-08]
          "SHELL=${pkgs.bashInteractive}/bin/bash"
          # Seed configs dir for start-lxqt-nested.
          "GEMINI_LXQT_CONFIGS=${sessionConfig}"
        ];
      };
    };
  };
}

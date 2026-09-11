# gemshell desktop — the native Rust compositor as a boot session.
#
# The gemshell compositor (pkgs/gemshell.nix) is the fourth panel owner,
# alongside the GDM sessions (GNOME/COSMIC/niri) and the fbcon console —
# but it is NOT a GDM session: it is a SYSTEM service (User=cjdell) that
# owns /dev/dri/card0 directly, exactly like the nested gemwl stack did
# (no logind session, no display manager). Selection is by the SAME
# persistent marker as everything else (docs/desktop-selection.md):
#
#   /var/lib/gemini/desktop = gemshell
#     -> gemini-desktop-apply creates /run/gemini-console
#     -> display-manager.service (ConditionPathExists=!sentinel) skips GDM
#     -> gemini-gemshell.service (ConditionPathExists=sentinel) starts
#
# so `gemcli session set gemshell --reboot` is a clean switch and the
# unit is inert in every other mode (its Condition fails). Like console
# mode it must NOT start a tty1 getty: the compositor owns the panel and
# a console would fight the shadow-plane blit for the scanout memory
# (the apply script's gemshell branch creates only the sentinel).
#
# GPU ordering mirrors services/gnome.nix: panfrost is blacklisted at
# boot (gemini-pda.nix — early probe soft-resets the un-powered Mali);
# gemini-gemshell-panfrost-load loads it after gemini-gpu-poweron, and
# the compositor starts only after that + the card0 node exists.
#
# Story + receipts + on-glass checklist: docs/gemshell.md.
{ config, lib, pkgs, ... }:

let
  cfg = config.services.gemshellDesktop;
  utils = pkgs.callPackage ./gemini-utils.nix { };
  # The compositor package (single binary; the settings panel is the
  # in-process egui overlay since 2026-09-12). Same callPackage args as
  # the flake package, so the system and
  # `.#packages.aarch64-linux.gemshell` share the store path.
  # The mesa fork (T880 delta) — the compositor's ICD + DRI driver.
  mesa = pkgs.callPackage ../pkgs/mesa-geminipda.nix { };
  gemshell = pkgs.callPackage ../pkgs/gemshell.nix { mesa = mesa; };
  # DejaVu (the fontconfig default on this device; fonts.packages in the
  # GNOME/LXQt/Phosh modules) — the compositor's UI font.
  fonts = pkgs.dejavu_fonts;
  # xkbcommon include dir with symbols/gemini (Fn = level-3 shift); the
  # compositor reads XKB_CONFIG_EXTRA_PATH + XKB_DEFAULT_LAYOUT when it
  # compiles the keymap (src/compositor/input.rs).
  geminiXkb = pkgs.callPackage ../pkgs/gemini-xkb.nix { };
  # gemdemo — the GL-client smoke test (docs/gemshell.md checklist 6).
  gemdemo = pkgs.callPackage ../pkgs/gemdemo.nix { };
  # The mesa fork's libgbm has NO baked backend path and honors only
  # GBM_BACKENDS_PATH (the LXQt/Phosh receipt, 2026-09-08) — point it at
  # the fork's lib/gbm so gbm_create_device("/dev/dri/card0") loads the
  # SAME backend the /run/opengl-driver ICD uses.
  mesaGeminipda = pkgs.callPackage ../pkgs/mesa-geminipda.nix { };
in
{
  options.services.gemshellDesktop = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Install the gemshell compositor as a boot session (co-install
        model: it is inert unless the desktop marker
        /var/lib/gemini/desktop says `gemshell`, resolved at boot by
        gemini-desktop-apply; `gemcli session set gemshell`).
        Mutually exclusive at RUNTIME with the GDM sessions (same
        sentinel mechanism); at BUILD time it force-disables the nested
        gemwl/phosh/LXQt stack like services/gnome.nix does.
      '';
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "cjdell";
      description = "User the compositor runs as (the desktop user).";
    };
  };

  config = lib.mkIf cfg.enable {
    # ---- The compositor (system service, panel owner) ----------------
    systemd.services.gemini-gemshell = {
      description = "gemshell — native Wayland compositor (desktop session)";
      wantedBy = [ "multi-user.target" ];
      after = [
        "gemini-desktop-apply.service" # the sentinel (Condition) exists first
        "gemini-gemshell-panfrost-load.service"
        "gemini-gpu-poweron.service"
        "systemd-udevd.service"
        "local-fs.target"
      ];
      wants = [ "gemini-gpu-poweron.service" ];
      # The mode gate: starts ONLY in gemshell marker mode (the apply
      # service created the sentinel). In gnome/cosmic/niri/console mode
      # the condition fails and GDM (or nothing) owns the panel.
      unitConfig.ConditionPathExists = "/run/gemini-console";
      serviceConfig = {
        Type = "simple";
        User = cfg.user;
        # cjdell needs the panel + input + audio + nm + bt (config/
        # gemini.nix extraGroups: video/audio/networkmanager/input/
        # bluetooth).
        RuntimeDirectory = "gemshell";
        RuntimeDirectoryMode = "0700";
        # A crash must not wedge the panel into a dead state — restart;
        # the compositor re-grabs card0 cleanly (the gemwl-era units did
        # not restart; this one is a long-running service).
        Restart = "on-failure";
        RestartSec = "2";
        Environment = [
          # Wayland socket dir (the display auto-creates wayland-0 here).
          "XDG_RUNTIME_DIR=/run/gemshell"
          # The UI font (deterministic store path — /usr/share/fonts does
          # not exist on NixOS; the compositor's fallback walk covers
          # /run/current-system/sw/share/fonts too).
          "GEMSHELL_FONT=${fonts}/share/fonts/truetype/DejaVuSans.ttf"
          # CRITICAL (discovered on glass 2026-09-11): the EGL vendor
          # manifest dir. libglvnd only looks in $EGL_VENDOR_PATH (or its
          # compiled-in default /usr/share/glvnd/egl_vendor.d — which does
          # not exist on NixOS). Without this it silently loads NO ICD and
          # falls back to its built-in STUB: eglGetPlatformDisplayEXT
          # returns a display, eglInitialize "succeeds" (1.5), but
          # eglChooseConfig has ZERO configs (EGL_BAD_MATCH / 0x3004) and
          # no loader output appears at all. The NixOS graphics-drivers
          # bundle at /run/opengl-driver carries the manifest for the
          # FORK (50_mesa.json → mesa-geminipda's libEGL_mesa, T880 delta).
          "EGL_VENDOR_PATH=/run/opengl-driver/share/glvnd/egl_vendor.d"
          # The DRI driver dir (mesa 26's pipe-loader still consults
          # search-path env vars for <driver>_dri.so; keep it pointed at
          # the fork's dri dir so the DRI driver matches the ICD).
          "DRI_DRIVER_DIR=${mesa}/lib/dri"
          # Keymap: the gemini layout (Fn = level-3) via the symbols dir.
          "XKB_CONFIG_EXTRA_PATH=${geminiXkb}"
          "XKB_DEFAULT_LAYOUT=gemini"
          "XKB_DEFAULT_MODEL=pc105"
          # GBM: the fork backend path (see the let-binding receipt).
          "GBM_BACKENDS_PATH=${mesaGeminipda}/lib/gbm"
          # AFBC readback workaround (must stay; same as gemwl/GNOME).
          "PAN_MESA_DEBUG=noafbc"
          # Audio for session-launched clients: the ONE system PipeWire
          # session (services/audio.nix), same redirection GNOME uses.
          "PULSE_SERVER=unix:/run/gemwl-audio/pulse/native"
          "PIPEWIRE_RUNTIME_DIR=/run/gemwl-audio"
          # The launcher scans $HOME/.local/share/applications (and the
          # XDG_DATA_DIRS entries below) — HOME was UNSET, so the
          # scanner fell back to /root and found 0 apps (and the icons
          # too). The service user's real home.
          "HOME=/home/${cfg.user}"
          # Touch is confirmed working (2026-09-12, ABS_MT_SLOT fix), so
          # the GEMSHELL_TOUCH_TRAIL test mode is off — normal gestures
          # are active. To re-enable the finger-drawing overlay for
          # debugging, add "GEMSHELL_TOUCH_TRAIL=1" here (docs/gemshell.md).
          # Apps launched from the launcher need $SHELL + a sane PATH
          # (the system-profile bin dir carries the app binaries). The
          # explicit store bins are for the gemdata-device DataProvider,
          # which shells out to nmcli/bluetoothctl/wpctl — do not rely on
          # the ambient system profile for them (2026-09-12).
          "SHELL=${pkgs.bashInteractive}/bin/bash"
          "PATH=${lib.makeBinPath [ pkgs.bashInteractive pkgs.coreutils pkgs.systemd pkgs.networkmanager pkgs.bluez pkgs.pipewire ]}:/run/current-system/sw/bin:/run/wrappers/bin"
          # Apps launched by the compositor get .desktop files from the
          # system profile (environment.systemPackages) + the user's
          # dir (src/common/apps.rs scans those).
          "XDG_DATA_DIRS=/run/current-system/sw/share"
        ];
        ExecStart = "${gemshell}/bin/gemshell";
      };
    };

    # ---- /dev/gemfb access (the LK-fb dma-buf exporter) --------------
    # The built-in geminipda-fb driver creates /dev/gemfb 0600 root;
    # the compositor (User=cjdell) needs it for the GPU-direct present
    # (GEMFB_IOC_EXPORT). Same region the card0 KMS shadow plane blits
    # to — only one desktop stack drives the screen at a time (the
    # mode gate above).
    services.udev.extraRules = ''
      SUBSYSTEM=="misc", KERNEL=="gemfb", MODE="0660", GROUP="video"
    '';

    # ---- GPU bring-up (same shape as gnome.nix's panfrost-load) ------
    systemd.services.gemini-gemshell-panfrost-load = {
      description = "Load panfrost for the gemshell session";
      after = [ "gemini-gpu-poweron.service" "systemd-udevd.service" ];
      wants = [ "gemini-gpu-poweron.service" ];
      before = [ "gemini-gemshell.service" ];
      wantedBy = [ "gemini-gemshell.service" ];
      path = [ pkgs.kmod pkgs.coreutils ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${utils}/bin/panfrost-load.sh";
      };
    };

    # ---- Replace the nested stack (same as gnome.nix) ----------------
    # gemshell owns the panel; gemwl owns /dev/gemfb and the nested
    # sessions own gemwl's socket. Force them off so the two models can
    # never fight over the scanout.
    systemd.services.gemwl.enable = lib.mkForce false;
    services.phoshDesktop.enable = lib.mkForce false;
    services.lxqtNested.enable = lib.mkForce false;

    # Keep the custom PipeWire (services/audio.nix), same as gnome.nix.
    services.pipewire.enable = lib.mkForce false;

    # ---- Apps ----------------------------------------------------------
    # The launcher scans XDG_DATA_DIRS for .desktop entries; the system
    # profile (environment.systemPackages) is the app suite. gemdemo is
    # the GL-client smoke test (docs/gemshell.md checklist item 6). The
    # UI font is for the compositor (GEMSHELL_FONT above) and the egui
    # settings panel.
    fonts.packages = [ fonts ];
    environment.systemPackages = [ gemshell gemdemo fonts ];
  };
}

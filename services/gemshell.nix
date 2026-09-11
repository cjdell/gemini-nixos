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
  # The compositor package (compositor + gemsettings client in one store
  # path). Same callPackage args as the flake package, so the system and
  # `.#packages.aarch64-linux.gemshell` share the store path.
  gemshell = pkgs.callPackage ../pkgs/gemshell.nix {
    mesa = pkgs.callPackage ../pkgs/mesa-geminipda.nix { };
  };
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
          # Apps launched from the launcher need $SHELL + a sane PATH
          # (the system-profile bin dir carries the app binaries).
          "SHELL=${pkgs.bashInteractive}/bin/bash"
          "PATH=${pkgs.bashInteractive}/bin:${pkgs.coreutils}/bin:${pkgs.systemd}/bin:/run/current-system/sw/bin:/run/wrappers/bin"
          # Apps launched by the compositor get .desktop files from the
          # system profile (environment.systemPackages) + the user's
          # dir (src/common/apps.rs scans those).
          "XDG_DATA_DIRS=/run/current-system/sw/share"
        ];
        ExecStart = "${gemshell}/bin/gemshell";
      };
    };

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
    # profile (environment.systemPackages) is the app suite. gemsettings
    # must be on PATH (the compositor spawns it by name). gemdemo is the
    # GL-client smoke test (docs/gemshell.md checklist item 6).
    environment.systemPackages = [ gemshell gemdemo ];
  };
}

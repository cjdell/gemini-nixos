# Phase 4 (preview): gemwl desktop — services, not a display manager.
#
# Ports the verified GeminiPDA desktop units (build/wayland/gemwl.service
# + run-gemwl.sh) into the NixOS system:
#
#   gemwl.service   the wlroots-0.18 compositor that OWNS the LK
#                   framebuffer: opens /dev/gemfb (kernel #329,
#                   CONFIG_FB_GEMINIPDA=y) + the panfrost render node
#                   (/dev/dri/renderD128 — gemwl.c tries renderD129 first
#                   and falls back to renderD128; this device exposes only
#                   renderD128, so the D129 attempt fails harmlessly),
#                   renders the Wayland scene graph GPU-direct (shadow
#                   dma-buf + compute blit), unbinds fbcon while it runs.
#                   Starts a smoke-test client (tinytest-anim) so the
#                   first boot visibly proves the chain on glass; replace
#                   with a real session (weston-terminal, then the nested
#                   KWin/Plasma or labwc/LXQt session — docs R3/phase 4)
#                   once verified.
#
# Differences from the Debian unit (semantics preserved):
#   - After=gemini-gpu-poweron.service (the Debian unit spelled it
#     gpu-poweron.service), so the MFG MTCMOS domains + VGPU rails are
#     up before EGL touches the GPU.
#   - ExecStartPre pkill Xorg / Conflicts=spin-demo.service are gone
#     (no Xorg or spin-demo in this system).
#   - PAN_MESA_DEBUG=noafbc + XDG_RUNTIME_DIR=/run/gemwl +
#     RuntimeDirectory=gemwl (0700) are kept exactly.
#
# Kernel prerequisites (all present in the borrowed kernel #329):
#   CONFIG_FB_GEMINIPDA=y   (/dev/gemfb, GEMFB_IOC_EXPORT)
#   CONFIG_DRM_PANFROST=m    (/dev/dri/renderD128, loaded early via
#                            boot.kernelModules, services/gemini-pda.nix)
#
# KNOWN RISK (gpu-warmup; outstanding.md item 8): gemwl is the
# FIRST GL client of the boot, and the Mali-T880 tiler can come out of a
# cold boot in a "bad state" — the first client whose tiler batches exceed
# 1024 px in one axis drops the region beyond 1024 px (black band). The
# tinytest-anim startup client draws a small window and likely does NOT
# absorb it. Whether kernel #329 still exhibits this (observed on #284 +
# Mesa 25.0.7 debug, 2026-09-02) is an on-glass question to answer at the
# first gemwl boot; if it bands, add a throwaway full-frame warmup as the
# first GL client Before=gemwl (pattern: GeminiPDA
# build/rootfs-files/gpu-warmup/, PAN_MESA_DEBUG=noafbc kept).
# Serial console (ttyS0) is unaffected by the fbcon unbind — the NixOS
# bring-up/debug path stays available while the compositor owns the fb.
# To fall back to the console-only boot: systemctl disable gemwl.
#
# Packages: gemwl (the compositor + tinytest clients, pkgs/gemwl.nix)
# and wlroots-geminipda (pkgs/wlroots-geminipda.nix, the pinned 0.18.2
# it links) are added to the system so the closure carries libwlroots/
# libgbm/libinput/... at their exact verified versions. Mesa is the
# existing mesa-geminipda (pkgs/mesa-geminipda.nix, EGL ICD wired by
# config/gemini.nix); this module only needs its store path for the
# derivation wiring, the ICD manifest /etc entry stays in config.
{ config, lib, pkgs, ... }:

let
  # Same callPackage args as the flake's package outputs, so the system
  # closure shares one store path per package.
  mesaGeminipda = pkgs.callPackage ../pkgs/mesa-geminipda.nix { };
  wlroots = pkgs.callPackage ../pkgs/wlroots-geminipda.nix {
    inherit mesaGeminipda;
  };
  gemwl = pkgs.callPackage ../pkgs/gemwl.nix { inherit wlroots; };

  # What to run inside the compositor at startup. The default is the
  # animated xdg-shell client (cycles red/green/blue on frame
  # callbacks) — a self-evident on-glass proof of the whole chain and a
  # frame-rate probe; point this at a real client (weston-terminal,
  # later the nested Plasma session) once the chain is verified.
  startupClient = "${gemwl}/bin/tinytest-anim";
in
{
  environment.systemPackages = [
    gemwl # compositor + tinytest clients
    wlroots # libwlroots-0.18.so etc. (closure requirement; also handy
    # for pkg-config on the device)
    mesaGeminipda # libgbm (wlroots DT_NEEDED) — same pkg config/gemini.nix adds
  ];

  systemd.services.gemwl = {
    description = "gemwl Wayland compositor (GPU-direct to LK framebuffer)";
    # The GPU must be powered on first (MFG MTCMOS + VGPU rails via
    # gemini-gpu-poweron.service); udev for the input devices / /dev
    # nodes.
    after = [ "gemini-gpu-poweron.service" "systemd-udevd.service" ];
    wants = [ "gemini-gpu-poweron.service" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "simple";
      # Private runtime dir holding the wayland socket (clients connect
      # through XDG_RUNTIME_DIR/wayland-N). Same layout as the verified
      # Debian unit; do NOT share the audio session's dir (audio.nix
      # documents why /run/gemwl-audio exists separately).
      RuntimeDirectory = "gemwl";
      RuntimeDirectoryMode = "0700";
      Environment = [
        "PAN_MESA_DEBUG=noafbc" # AFBC readback workaround (must stay)
        "XDG_RUNTIME_DIR=/run/gemwl"
        "HOME=/root"
      ];
      # -t 90: output transform (panel is physically landscape, fb is
      # portrait). The startup client runs in a forked shell from gemwl
      # itself, inheriting XDG_RUNTIME_DIR + WAYLAND_DISPLAY.
      ExecStart = "${gemwl}/bin/gemwl -t 90 -s '${startupClient}'";
      Restart = "on-failure";
      RestartSec = "2";
    };
  };
}

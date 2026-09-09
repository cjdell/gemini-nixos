# phoc 0.54.0 — nixpkgs' phoc, but its wlroots 0.19 is rebuilt so the
# GL stack is the SAME fork mesa the rest of the desktop uses
# (services/phosh.nix; flake package `phoc`).
#
# Why this override exists (2026-09-09, design + receipts in
# docs/phosh.md): phosh cannot run on gemwl directly — gemwl is a
# minimal xdg-shell KIOSK compositor (no wlr-layer-shell / foreign-
# toplevel / session-lock / text-input, and no phoc's private protocol,
# which phosh's shell-state gating expects). The proven nested pattern
# (labwc in gemwl) is reused with phoc: WLR_BACKENDS=wayland phoc
# nested on gemwl's wayland-0, hosting phosh on its own socket. Phoc
# then composites with wlroots 0.19's gles2 renderer + gbm allocator,
# so the whole chain must stay on the ONE mesa userspace verified on
# this GPU (the 25.0.7 geminipda fork — PAN_MESA_DEBUG=noafbc,
# EGL_EXT_image_dma_buf_import): mixing the fork's EGL ICD with a
# different-mesa libgbm risks exactly the AFBC/modifier mismatch the
# fork exists to avoid.
#
# nixpkgs pins phoc (pkgs/by-name/ph/phoc) against `wlroots_0_19`
# (wlroots 0.19.3; phoc 0.54 does NOT build against wlroots 0.18 — the
# repo's wlroots-geminipda pin is gemwl's library and stays untouched).
# nixpkgs' wlroots expression pulls libgbm from its own `mesa-libgbm`
# (a separate lean mesa build, 26.1.3 at this pin — NOT the same mesa
# as pkgs.mesa 26.2.2), whose baked gbm-backends-path is
# /run/opengl-driver/lib/gbm (libglvnd.driverLink — only present with
# hardware.graphics). This override swaps that libgbm for the fork's
# (which, like all wlroots-geminipda consumers, honours
# GBM_BACKENDS_PATH — set on the phosh-nested unit).
#
# phoc's own package expression then adds its layer-shell
# 0-dimension revert patch on top of the override (the nixpkgs phoc
# recipe: a wlroots patch that otherwise crashes Phosh; it composes
# fine — patch list is appended by phoc's overrideAttrs).
#
# Runtime shape (all in services/phosh.nix's phosh-nested unit):
#   WLR_BACKENDS=wayland WLR_RENDERER=gles2 \
#     phoc -v -S -C <phoc.ini> --socket phosh -E start-phosh-shell
# -S = shell mode: input stays gated until the phosh shell attaches
#   (phoc src/server.c allow_input / shell-state, same as upstream
#   phosh-session which runs `phoc -v -S -C ... -E gnome-session`).
# -E child (start-phosh-shell) inherits WAYLAND_DISPLAY=phosh (phoc
#   g_setenvs it before running the session).
# --socket phosh: fixed name (gemwl holds wayland-0) so no socket
#   hunting; phosh + squeekboard connect to WAYLAND_DISPLAY=phosh.
#   (phoc supports [core] socket= in phoc.ini too; CLI wins.)
{ pkgs
, mesaGeminipda
}:

let
  # wlroots 0.19.3 (nixpkgs' phoc dep) rebuilt with the fork's libgbm
  # replacing nixpkgs' mesa-libgbm. Only phoc consumes this override at
  # the pin — a single local aarch64 rebuild (wlroots ~ a few min on the
  # remote builder; phoc relinks after). Everything else stays cached.
  #
  # libdrm is added back explicitly: nixpkgs' mesa-libgbm propagates it
  # (gbm.nix propagatedBuildInputs = [ libdrm ]) and nixpkgs wlroots
  # relies on that propagation for its meson `libdrm` dependency; the
  # fork derivation propagates nothing, so swapping it out dropped
  # libdrm from the build (first build failure: "Run-time dependency
  # libdrm found: NO"). [2026-09-09]
  wlroots019Fork = pkgs.wlroots_0_19.overrideAttrs (old: {
    buildInputs = (map (b:
      if b.pname or "" == "mesa-libgbm" then mesaGeminipda else b
    ) old.buildInputs) ++ [ pkgs.libdrm ];
  });
in
# Re-runs pkgs/by-name/ph/phoc/package.nix with this wlroots_0_19:
#   wlroots = wlroots_0_19.overrideAttrs (+ layer-shell revert patch)
#   buildInputs = [ ... finalAttrs.wlroots ... ]
# — phoc's own wlroots patch chain applies on top of the gbm swap.
pkgs.phoc.override {
  wlroots_0_19 = wlroots019Fork;
}

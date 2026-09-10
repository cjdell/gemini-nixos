# wine-cli — `wine` / `wine64` on the PDA shell (thin wrappers over the
# standalone wine-wow64 stack, NOT the stack itself).
#
# WHY a wrapper, not the real wine: the box64 + wineWow64Packages.full
# 11.0 stack (~700 MB closure) is deliberately OUTSIDE the flake system
# — bin/wine-x86-deploy.sh ships it to the device as a standalone GC
# root (/nix/var/nix/gcroots/wine-x86/wine-wow64) with launcher
# /var/lib/wine-x86/wine-wow. Baking the full stack into systemPackages
# would bloat every system deploy (docs/wine-d3d.md §5). This pkg only
# provides the PATH convenience: `wine foo.exe` / `wine64 foo.exe`.
#
# The launcher lives under /var/lib, NOT /root (moved 2026-09-11): the
# first version exec'd /root/wine-x86/wine-wow, which the unprivileged
# desktop user cjdell cannot reach (/root is 0700) — running `wine` in
# the GNOME session failed with "Permission denied". /var/lib/wine-x86
# is world-readable/executable and holds the per-user prefix at
# $HOME/.wine-x86.
#
# The wrapper execs the device-state launcher by absolute path — if the
# wine stack was never deployed (fresh rootfs before wine-x86-deploy.sh
# deploy) the exec fails loudly with that path's ENOENT. Acceptable:
# it degrades to a clear error instead of a phantom command.
#
# 32-bit guests (like the D3D9/OpenGL demos of the 2003 scene) run:
# the wow64 build carries the i386-windows syswow64 payload. GL renders
# on the T880 via guest panfrost — env-forced llvmpipe does NOT stick
# (docs/wine-d3d.md §7, verified 2026-09-09).
#
# gen36 (2026-09-09): added to config/gemini.nix systemPackages.
{ writeShellScriptBin }:

let
  launcher = "/var/lib/wine-x86/wine-wow";
in
{
  wine = writeShellScriptBin "wine" ''
    # wine — run a Windows PE (32- or 64-bit) on the Gemini PDA via the
    # standalone wine-wow64 stack (box64 + wine 11.0 wow64; launcher
    # ${launcher}, GC root wine-x86/wine-wow64). 32-bit apps work
    # (syswow64 payload). Full story: docs/wine-d3d.md.
    exec ${launcher} "$@"
  '';
  wine64 = writeShellScriptBin "wine64" ''
    # wine64 — alias of wine: the wine-wow64 11.0 build runs BOTH 32-bit
    # and 64-bit Windows apps (64-bit apps are not special-cased).
    exec ${launcher} "$@"
  '';
}

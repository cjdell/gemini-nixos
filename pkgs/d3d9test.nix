# d3d9test — a minimal Windows D3D9 smoke app (rotating coloured cube +
# FPS overlay; source: pkgs/d3d9test/d3d9test.cpp).
#
# BUILT FOR: x86_64-windows PE. Compiled with the mingw-w64 cross
# toolchain (pkgsCross.mingwW64 stdenv) using mingw-w64's own D3D9
# headers — NOT winegcc: the winegcc shipped in the nixpkgs wine64
# package is configured for a native x86_64-linux target
# (winegcc -dumpmachine → x86_64-unknown-linux-gnu, verified
# 2026-09-09) and emits an ELF, which wine cannot load.
#
# The PE imports d3d9.dll / user32.dll / gdi32.dll, which the wine64
# prefix provides at runtime on the PDA (see docs/wine-d3d.md).
#
# SYSTEM: evaluate with an x86_64-linux pkgs set and build natively on
# the host (the cross stdenv compiles here; the OUTPUT is a
# x86_64-windows PE). NOT part of the aarch64 flake package set.
#
# RUN ON THE PDA: bin/wine-x86-deploy.sh ships it; `wine-x86 d3d9test.exe`.
{ stdenv }:

stdenv.mkDerivation {
  pname = "d3d9test";
  version = "1.0";

  src = ./d3d9test;

  # stdenv here IS the x86_64-w64-mingw32 cross stdenv
  # (callPackage from pkgsCross.mingwW64) — stdenv.cc is the mingw-w64
  # x86_64-w64-mingw32-g++, headers/libs are mingw-w64's (d3d9.h,
  # d3d9types.h with the real FVF macros, libd3d9.a, ...).
  dontConfigure = true;
  buildPhase = ''
    # -O2, C++ (mingw's d3d9.h exposes the COM interfaces as classes in
    # C++ mode). -static-libstdc++ keeps the PE self-contained (wine's
    # libstdc++ shim would also work, but static is fewer moving parts).
    # d3d9 + user32 + gdi32 only (no D3DX).
    $CXX $CXXFLAGS -O2 -o d3d9test.exe d3d9test.cpp \
      -ld3d9 -luser32 -lgdi32 -lm -static-libstdc++
  '';
  installPhase = ''
    mkdir -p $out/bin
    mv d3d9test.exe $out/bin/d3d9test.exe
    # strip: mingw's strip handles PE relocations correctly; wine does
    # not require symbols.
    ${stdenv.cc}/bin/x86_64-w64-mingw32-strip --strip-unneeded $out/bin/d3d9test.exe || true
  '';

  meta = {
    description = "Minimal Direct3D 9 test app (x86_64-windows) for the wine64+box64 stack on the Gemini PDA";
    platforms = [ "x86_64-linux" ];
    mainProgram = "d3d9test.exe";
  };
}
